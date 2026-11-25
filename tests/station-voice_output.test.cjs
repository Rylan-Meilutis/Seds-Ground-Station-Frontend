// Run with node tests/station-voice_output.test.cjs. No browser or station.
// Checks the voice page's audio-output block (station_ui/scripts/voice.js):
// the control shown per platform, the Phone / Speaker choice sent to the
// app, output-device selection where setSinkId exists, and the remembered
// device (with storage that throws, as in private browsing).
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const source = fs.readFileSync('station_ui/scripts/voice.js', 'utf8');
const start = source.indexOf('// <audio-output>'), end = source.indexOf('// </audio-output>');
assert(start >= 0 && end > start, 'voice.js audio-output block');
const block = source.slice(start, end);

function select() {
  const el = { options: [], value: '', listeners: {} };
  el.replaceChildren = (...opts) => { el.options = opts; };
  el.add = opt => el.options.push(opt);
  el.addEventListener = (kind, fn) => { el.listeners[kind] = fn; };
  // A select only takes a value it has.
  Object.defineProperty(el, 'value', { get: () => el._value ?? '', set: v => { el._value = el.options.some(o => o.value === v) ? v : ''; } });
  return el;
}

function page({ sink = false, storage = 'ok', saved = null, outputs = [], app = null } = {}) {
  const radios = ['phone', 'speaker'].map(value => ({ value, checked: false, listeners: {}, addEventListener(kind, fn) { this.listeners[kind] = fn; } }));
  const elements = {
    'output-controls': { hidden: true },
    'output-native': { hidden: true },
    'output-device-label': { hidden: true },
    'output-device': select(),
    'output-status': { textContent: '' },
  };
  const store = new Map(saved ? [['gs26-voice-output-device', saved]] : []);
  const localStorage = storage === 'ok'
    ? { getItem: k => store.get(k) ?? null, setItem: (k, v) => store.set(k, v), removeItem: k => store.delete(k) }
    : { getItem() { throw new Error('denied'); }, setItem() { throw new Error('denied'); }, removeItem() { throw new Error('denied'); } };
  const posted = [];
  const sinks = [];
  const Context = function () {};
  if (sink) Context.prototype.setSinkId = function (id) { sinks.push(id); return Promise.resolve(); };
  const win = { AudioContext: Context };
  const sandbox = {
    $: id => elements[id],
    document: { querySelectorAll: () => radios },
    navigator: { mediaDevices: { enumerateDevices: async () => outputs, addEventListener() {} } },
    Option: function (label, value) { this.label = label; this.value = value; },
    localStorage, window: win,
    parent: { postMessage: (m, origin) => posted.push({ ...JSON.parse(JSON.stringify(m)), origin }) },
    appInfo: app, session: null, Date,
  };
  vm.createContext(sandbox);
  vm.runInContext(`var appInfo=globalThis.appInfo, session=globalThis.session;${block}
    globalThis.api={showOutput,listOutputs,applySink,chooseOutput,outputActive,outputMode,
      setApp:a=>{appInfo=a;}, setSession:s=>{session=s;}, setOrigin:o=>{dashboardOrigin=o;}};`, sandbox);
  return { elements, radios, posted, sinks, store, api: sandbox.api };
}
const nativeState = (choice, extra = {}) => ({ platform: 'ios', audio_output: { control: 'native', options: ['phone', 'speaker'], choice, route: 'the earpiece', available: true, note: null, ...extra } });
const tick = () => new Promise(resolve => setImmediate(resolve));

(async () => {
  // Desktop WebKit (macOS app, Linux app, Safari): no setSinkId, no fake options.
  {
    const { elements, api } = page({ sink: false });
    await tick();
    assert.equal(api.outputMode(), 'none');
    assert.equal(elements['output-controls'].hidden, true);
  }

  // Phones: Phone / Speaker, the default (Phone) checked, the route shown.
  {
    const { elements, radios, posted, api } = page({ app: nativeState('phone') });
    api.showOutput();
    assert.equal(api.outputMode(), 'native');
    assert.equal(elements['output-controls'].hidden, false);
    assert.equal(elements['output-native'].hidden, false);
    assert.equal(elements['output-device-label'].hidden, true);
    assert.deepEqual(radios.map(r => r.checked), [true, false]);
    assert.equal(elements['output-status'].textContent, 'Playing through the earpiece.');

    // Choosing Speaker posts the choice to the dashboard's origin, and the
    // stale state the dashboard keeps resending does not flip it back.
    api.setOrigin('dioxus://index.html');
    radios[1].checked = true; radios[1].listeners.change();
    assert.deepEqual(posted.at(-1), { type: 'gs26-audio-output', choice: 'speaker', active: false, origin: 'dioxus://index.html' });
    api.setApp(nativeState('phone'));
    api.showOutput();
    assert.deepEqual(radios.map(r => r.checked), [false, true], 'the click wins until the app confirms');
    api.setApp(nativeState('speaker', { route: 'the speaker' }));
    api.showOutput();
    assert.deepEqual(radios.map(r => r.checked), [false, true]);
    assert.equal(elements['output-status'].textContent, 'Playing through the speaker.');

    // Joining voice tells the app to apply the output; an unavailable
    // choice is explained, not hidden.
    api.setSession({ stream: {} });
    api.outputActive();
    assert.deepEqual(posted.at(-1), { type: 'gs26-audio-output', choice: 'speaker', active: true, origin: 'dioxus://index.html' });
    api.setApp(nativeState('speaker', { route: 'AirPlay', available: false, note: 'The speaker is not available; playing through AirPlay.' }));
    api.showOutput();
    assert.equal(elements['output-status'].textContent, 'The speaker is not available; playing through AirPlay.');
    api.setSession(null);
    api.outputActive();
    assert.equal(posted.at(-1).active, false, 'leaving releases the communication mode');
  }

  // A plain browser tab never posts to a parent it does not know.
  {
    const { posted, api } = page({ app: nativeState('phone') });
    api.chooseOutput('speaker');
    assert.equal(posted.length, 0);
  }

  // Chromium (browsers, WebView2): output devices with "System default",
  // the choice remembered and applied with setSinkId.
  {
    const outputs = [
      { kind: 'audioinput', deviceId: 'mic', label: 'Mic' },
      { kind: 'audiooutput', deviceId: 'default', label: 'Default' },
      { kind: 'audiooutput', deviceId: 'usb', label: 'USB headset' },
      { kind: 'audiooutput', deviceId: 'hdmi', label: '' },
    ];
    const { elements, sinks, store, api } = page({ sink: true, outputs });
    await tick();
    assert.equal(api.outputMode(), 'device');
    assert.equal(elements['output-controls'].hidden, false);
    assert.equal(elements['output-native'].hidden, true);
    assert.equal(elements['output-device-label'].hidden, false);
    assert.deepEqual(elements['output-device'].options.map(o => [o.label, o.value]), [['System default', ''], ['USB headset', 'usb'], ['Output 2', 'hdmi']]);
    elements['output-device'].value = 'usb';
    elements['output-device'].listeners.change();
    assert.equal(store.get('gs26-voice-output-device'), 'usb');
    const context = { setSinkId(id) { sinks.push(id); return Promise.resolve(); } };
    await api.applySink({ context });
    assert.deepEqual(sinks, ['usb']);
    elements['output-device'].value = '';
    elements['output-device'].listeners.change();
    assert.equal(store.has('gs26-voice-output-device'), false, 'System default forgets the device');
  }

  // A remembered device that is gone: the default plays, and the page says so.
  {
    const { elements, sinks, api } = page({ sink: true, saved: 'usb', outputs: [{ kind: 'audiooutput', deviceId: 'hdmi', label: 'HDMI' }] });
    await api.applySink({ context: { setSinkId(id) { sinks.push(id); return Promise.resolve(); } } });
    assert.deepEqual(sinks, ['']);
    assert.equal(elements['output-device'].value, '');
    assert.equal(elements['output-status'].textContent, 'The saved output is not connected; playing through the system default.');
  }

  // Storage that throws (private browsing, blocked site data) still works.
  {
    const outputs = [{ kind: 'audiooutput', deviceId: 'usb', label: 'USB' }];
    const { elements, sinks, api } = page({ sink: true, storage: 'throws', outputs });
    await tick();
    elements['output-device'].value = 'usb';
    assert.doesNotThrow(() => elements['output-device'].listeners.change());
    await api.applySink({ context: { setSinkId(id) { sinks.push(id); return Promise.resolve(); } } });
    assert.deepEqual(sinks, [''], 'nothing remembered: the system default');
  }

  // A refused device falls back to the default with the reason.
  {
    const { elements, api } = page({ sink: true });
    await api.applySink({ context: { setSinkId: () => Promise.reject(new Error('NotAllowedError')) } });
    assert.match(elements['output-status'].textContent, /^Cannot play through that output \(NotAllowedError\); playing through the system default\.$/);
  }

  console.log('PASS: audio output per platform (phone/speaker in the apps, setSinkId devices in Chromium, hidden elsewhere), the choice posted and kept, remembered device with guarded storage');
})().catch(error => { console.error(error); process.exit(1); });
