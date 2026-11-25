// Run with node tests/station-voice_native.test.cjs. No browser, microphone or station.
// Checks the voice page's self-contained blocks (station_ui/scripts/voice.js):
// noise-suppression choice and label per platform, microphone-refusal text
// per platform, and resuming an AudioContext the system interrupted (iOS).
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const source = fs.readFileSync('station_ui/scripts/voice.js', 'utf8');
function block(name) {
  const start = source.indexOf(`// <${name}>`), end = source.indexOf(`// </${name}>`);
  assert(start >= 0 && end > start, `voice.js block ${name}`);
  return source.slice(start, end);
}
const bounded = (promise, ms, message) => Promise.race([promise, new Promise((_, reject) => setTimeout(() => reject(new Error(message)), ms))]);

function page({ systemNoise, worklet = true }) {
  const elements = {
    'noise-suppression': { checked: true, disabled: false },
    'noise-status': { textContent: '' },
  };
  const listeners = [];
  const sandbox = {
    console: { info() {}, warn() {} },
    setTimeout, bounded,
    $: id => elements[id],
    navigator: { mediaDevices: { getSupportedConstraints: () => (systemNoise ? { noiseSuppression: true, echoCancellation: true } : { echoCancellation: true }) } },
    AudioWorkletNode: worklet ? function () {} : undefined,
    document: { hidden: false, addEventListener: (kind, fn) => listeners.push([kind, fn]) },
    window: { addEventListener: (kind, fn) => listeners.push([kind, fn]) },
    notices: [],
    session: null,
  };
  sandbox.notice = text => sandbox.notices.push(text);
  vm.createContext(sandbox);
  // `let`/`const` stay script-scoped; expose what the test drives.
  vm.runInContext(`${block('noise-suppression')}${block('audio-resume')}${block('join-errors')}
    globalThis.api={noiseMode,noiseLabel,applyNoise,showNoisePlan,resumeAudio,watchAudio,permissionHint,joinErrorText,setApp:a=>{appInfo=a;}};`, sandbox);
  return { sandbox, elements, listeners, api: sandbox.api };
}
const track = settings => ({ getSettings: () => settings });
// Messages cross a vm realm; compare them as plain JSON.
const callWith = (stream, posted) => ({ stream: { getAudioTracks: () => [stream] }, node: { port: { postMessage: m => posted.push(JSON.parse(JSON.stringify(m))) } } });

// Capability detection: WebKit (no constraint) uses the built-in suppressor;
// Chromium uses the system one when the microphone reports it running.
{
  const webkit = page({ systemNoise: false });
  assert.equal(webkit.elements['noise-status'].textContent, 'Noise suppression: on (built-in) when you join');
  assert.equal(webkit.elements['noise-suppression'].disabled, false, 'native WebKit apps keep the control');
  const posted = [];
  assert.equal(webkit.api.applyNoise(callWith(track({ echoCancellation: true }), posted)), 'built-in');
  assert.deepEqual(posted, [{ type: 'noise', enabled: true }]);
  assert.equal(webkit.elements['noise-status'].textContent, 'Noise suppression: on (built-in)');
  webkit.elements['noise-suppression'].checked = false;
  assert.equal(webkit.api.applyNoise(callWith(track({}), posted)), 'off');
  assert.deepEqual(posted.at(-1), { type: 'noise', enabled: false });
  assert.equal(webkit.elements['noise-status'].textContent, 'Noise suppression: off');

  const chromium = page({ systemNoise: true });
  assert.equal(chromium.elements['noise-status'].textContent, 'Noise suppression: on (system) when you join');
  const sent = [];
  assert.equal(chromium.api.applyNoise(callWith(track({ noiseSuppression: true }), sent)), 'system');
  assert.deepEqual(sent, [{ type: 'noise', enabled: false }], 'no double suppression');
  // The device ignored the request: the built-in one takes over.
  assert.equal(chromium.api.applyNoise(callWith(track({ noiseSuppression: false }), sent)), 'built-in');
  // Turned off but the device keeps it on: say so.
  chromium.elements['noise-suppression'].checked = false;
  assert.equal(chromium.api.applyNoise(callWith(track({ noiseSuppression: true }), sent)), 'system-forced');
  assert.match(chromium.elements['noise-status'].textContent, /^Noise suppression: on \(system; this device keeps it on\)$/);
}
// Nothing available: "browser" only in a browser, never inside an app.
{
  const bare = page({ systemNoise: false, worklet: false });
  assert.equal(bare.elements['noise-suppression'].disabled, true);
  assert.equal(bare.api.noiseLabel('unavailable'), 'Noise suppression is unavailable in this browser.');
  for (const platform of ['ios', 'android', 'macos', 'linux', 'windows']) {
    bare.api.setApp({ platform });
    assert.equal(bare.api.noiseLabel('unavailable'), 'Noise suppression is unavailable in this app.');
  }
}
console.log('PASS: noise suppression picks system or built-in and labels what runs');

// Microphone refusals name this platform's setting.
{
  const { api } = page({ systemNoise: false });
  const refused = { name: 'NotAllowedError', message: 'The request is not allowed by the user agent' };
  assert.match(api.joinErrorText(refused), /^Microphone access was refused\. Allow the microphone for this site in the browser/);
  const expected = { ios: /Settings → Privacy & Security → Microphone/, macos: /System Settings → Privacy & Security → Microphone/, android: /Settings → Apps/, windows: /Privacy & security → Microphone/, linux: /desktop allows this app/ };
  for (const [platform, pattern] of Object.entries(expected)) {
    api.setApp({ platform });
    assert.match(api.joinErrorText(refused), pattern, platform);
    assert.doesNotMatch(api.joinErrorText(refused), /browser/, platform);
  }
  api.setApp({ platform: 'ios', record_permission: 'denied' });
  assert.match(api.joinErrorText(refused), /^Microphone access is off for this app\. Allow the microphone for this app in Settings/);
  assert.match(api.joinErrorText({ name: 'NotFoundError' }), /No usable microphone/);
  assert.match(api.joinErrorText({ name: 'NotReadableError' }), /in use by another app/);
  assert.equal(api.joinErrorText(new Error('Voice connection timed out')), 'Voice connection timed out');
}
console.log('PASS: microphone refusals explain the setting for each platform');

// An interrupted AudioContext (iOS) is resumed; a refusal asks for a tap.
(async () => {
  const { sandbox, api, listeners } = page({ systemNoise: false });
  const context = (canResume) => ({
    state: 'interrupted', sampleRate: 48000, onstatechange: null,
    resume() { if (canResume) this.state = 'running'; return Promise.resolve(); },
  });
  const s = { context: context(true) };
  sandbox.session = s;
  api.watchAudio(s);
  s.context.onstatechange();
  await new Promise(r => setTimeout(r, 0));
  assert.equal(s.context.state, 'running', 'statechange resumes the interrupted context');

  const stuck = { context: context(false) };
  sandbox.session = stuck;
  assert.equal(await api.resumeAudio(stuck, 'microphone started'), false);
  assert.equal(sandbox.notices.at(-1), 'Voice audio was paused by the system. Tap this page to resume.');
  // The next touch is a user gesture: WebKit allows the resume.
  stuck.context.resume = function () { this.state = 'running'; return Promise.resolve(); };
  for (const [kind, fn] of listeners) if (kind === 'pointerdown') fn();
  await new Promise(r => setTimeout(r, 0));
  assert.equal(stuck.context.state, 'running');
  assert.equal(sandbox.notices.at(-1), 'Voice audio resumed.');
  for (const kind of ['pointerdown', 'keydown', 'visibilitychange', 'focus', 'pageshow'])
    assert(listeners.some(([k]) => k === kind), kind);
  // A closed or other session's context is left alone.
  const closed = { context: { state: 'closed', resume() { throw new Error('must not resume'); } } };
  assert.equal(await api.resumeAudio(closed, 'test'), true);
  console.log('PASS: interrupted audio resumes, and a refused resume asks for a tap');
})().catch(error => { console.error(error); process.exit(1); });
