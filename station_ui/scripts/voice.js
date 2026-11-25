import {signIn} from '/assets/media-login.js';
const $=id=>document.getElementById(id);
let token='', session=null, muted=false, held=false, joining=false, radioAvailable=false;
// <noise-suppression> The platform's own suppression where it reports it
// running (Chromium: browsers, the Android app's WebView, WebView2), otherwise
// the built-in suppressor in voice-worklet.js (WebKit, which has no
// noiseSuppression constraint: Safari, the iOS and macOS apps, the Linux app).
// The status says which one runs, never more than that.
const systemNoiseSupported=navigator.mediaDevices?.getSupportedConstraints?.().noiseSuppression===true;
const builtInNoiseSupported=typeof AudioWorkletNode==='function';
const noiseSupported=systemNoiseSupported||builtInNoiseSupported;
// {platform:'ios'|'android'|'macos'|'linux'|'windows'|'web', record_permission?}
// from the dashboard that embeds this page; null in a plain browser tab.
let appInfo=null;
const inNativeApp=()=>!!appInfo?.platform&&appInfo.platform!=='web';
function noiseConstraint(enabled){return systemNoiseSupported?{noiseSuppression:enabled}:{};}
function noiseMode(enabled,track){
  const systemOn=systemNoiseSupported&&track?.getSettings?.().noiseSuppression===true;
  if(!enabled)return systemOn?'system-forced':'off';
  if(systemOn)return 'system';
  return builtInNoiseSupported?'built-in':'unavailable';
}
function noiseLabel(mode){
  switch(mode){
    case 'off':return 'Noise suppression: off';
    case 'system':return 'Noise suppression: on (system)';
    case 'built-in':return 'Noise suppression: on (built-in)';
    case 'system-forced':return 'Noise suppression: on (system; this device keeps it on)';
    default:return `Noise suppression is unavailable ${inNativeApp()?'in this app':'in this browser'}.`;
  }
}
// Before joining: what will run once the microphone opens.
function showNoisePlan(){
  const enabled=$('noise-suppression').checked;
  $('noise-status').textContent=!noiseSupported?noiseLabel('unavailable'):enabled?`${noiseLabel(systemNoiseSupported?'system':'built-in')} when you join`:noiseLabel('off');
}
// In a call: read what the microphone reports and switch the built-in
// suppressor on only when the system one is not running.
function applyNoise(s){
  const mode=noiseMode($('noise-suppression').checked,s?.stream?.getAudioTracks()[0]);
  if(s)s.noiseMode=mode;
  s?.node?.port.postMessage({type:'noise',enabled:mode==='built-in'});
  $('noise-status').textContent=noiseLabel(mode);
  return mode;
}
$('noise-suppression').disabled=!noiseSupported;
if(!noiseSupported)$('noise-suppression').checked=false;
showNoisePlan();
// </noise-suppression>
// <audio-output> Where incoming crew audio plays. Phones (the iOS and Android
// apps) choose natively: Phone, like a call (the earpiece, or the connected
// headset), or Speaker; the dashboard sends the state in app.audio_output and
// this page posts the operator's choice back (gs26-audio-output). Elsewhere
// the page picks an output device with AudioContext.setSinkId where the
// browser or web view has it (Chromium: browsers, WebView2), remembered in
// this browser; without it the control stays hidden (the system default plays).
const OUTPUT_KEY='gs26-voice-output-device';
const sinkSupported=typeof (window.AudioContext||window.webkitAudioContext)?.prototype?.setSinkId==='function';
let dashboardOrigin=null,pendingOutput=null;
function storedSink(){try{return localStorage.getItem(OUTPUT_KEY)||'';}catch{return '';}}
function storeSink(id){try{if(id)localStorage.setItem(OUTPUT_KEY,id);else localStorage.removeItem(OUTPUT_KEY);}catch{/* private mode: this session only */}}
function outputMode(){
  if(appInfo?.audio_output?.control==='native')return 'native';
  return sinkSupported?'device':'none';
}
// The choice shown: the operator's click wins until the dashboard confirms it.
function shownChoice(){
  const reported=appInfo?.audio_output?.choice==='speaker'?'speaker':'phone';
  if(pendingOutput&&(pendingOutput.choice===reported||Date.now()>pendingOutput.until))pendingOutput=null;
  return pendingOutput?.choice??reported;
}
function showOutput(){
  const mode=outputMode();
  $('output-controls').hidden=mode==='none';
  $('output-native').hidden=mode!=='native';
  $('output-device-label').hidden=mode!=='device';
  if(mode==='native'){
    const choice=shownChoice(),state=appInfo.audio_output;
    for(const radio of document.querySelectorAll('input[name=output]'))radio.checked=radio.value===choice;
    $('output-status').textContent=pendingOutput?'':state.note||(state.route?`Playing through ${state.route}.`:'');
  }else if(mode==='none')$('output-status').textContent='';
}
function postOutput(choice){
  if(parent===window||!dashboardOrigin)return;
  parent.postMessage({type:'gs26-audio-output',choice,active:!!session?.stream},dashboardOrigin==='null'?'*':dashboardOrigin);
}
function chooseOutput(choice){
  if(outputMode()!=='native')return;
  pendingOutput={choice,until:Date.now()+3000};
  postOutput(choice);showOutput();
}
// Phones: the dashboard applies the output while voice is joined.
function outputActive(){if(outputMode()==='native')postOutput(shownChoice());}
async function listOutputs(){
  if(outputMode()!=='device')return '';
  const saved=storedSink();let outputs=[];
  try{outputs=(await navigator.mediaDevices.enumerateDevices()).filter(d=>d.kind==='audiooutput'&&d.deviceId&&d.deviceId!=='default');}catch{/* listed after microphone permission */}
  const select=$('output-device');select.replaceChildren(new Option('System default',''));
  outputs.forEach((d,i)=>select.add(new Option(d.label||`Output ${i+1}`,d.deviceId)));
  const present=!saved||outputs.some(d=>d.deviceId===saved);
  select.value=present?saved:'';
  $('output-status').textContent=present?'':'The saved output is not connected; playing through the system default.';
  return present?saved:'';
}
async function applySink(s){
  if(outputMode()!=='device'||typeof s?.context?.setSinkId!=='function')return;
  const id=await listOutputs();
  try{await s.context.setSinkId(id);}
  catch(e){$('output-status').textContent=`Cannot play through that output (${e.message}); playing through the system default.`;}
}
for(const radio of document.querySelectorAll('input[name=output]'))radio.addEventListener('change',()=>{if(radio.checked)chooseOutput(radio.value);});
$('output-device').addEventListener('change',()=>{storeSink($('output-device').value);if(session?.context)applySink(session);else listOutputs();});
navigator.mediaDevices?.addEventListener?.('devicechange',()=>{if(session?.context)applySink(session);else listOutputs();});
showOutput();listOutputs();
// </audio-output>
function radioLink(radio){
  $('radio-controls').hidden=!radio?.configured;
  radioAvailable=radio?.available===true;
  $('radio-enable').disabled=!session?.ready||!radioAvailable;
  $('radio-status').textContent=radioAvailable?'Recovery radio ready · 1.2 kb/s push-to-talk, about half a second delay':'Recovery radio unavailable (no RRLS link, or the primary is busy while the RRLS relays) · local voice still works';
  if(!radioAvailable){$('radio-enable').checked=false;if(session)session.radio=false;}
}
const notice=text=>{$('status').textContent=text;};
// Native apps show this page inside a web view. Say why voice cannot start
// there rather than failing after Join voice.
function voiceUnavailableReason(){
  if(!window.isSecureContext)return 'Voice needs the Ground Station over HTTPS (or localhost): microphones and the voice audio processor are disabled on plain-HTTP pages. Connect with the station\'s https:// address; in the app, a self-signed certificate works once TLS verification is skipped for that station.';
  if(!navigator.mediaDevices?.getUserMedia)return 'This web view cannot open a microphone (no getUserMedia), so voice is unavailable here. Use a browser or an app version with microphone support.';
  const Context=window.AudioContext||window.webkitAudioContext;
  if(!Context||!('audioWorklet' in Context.prototype))return 'This web view lacks AudioWorklet, which voice audio needs. Update the system web view, or use a browser.';
  return '';
}
// <audio-resume> WebKit on iOS interrupts an AudioContext whenever the app's
// audio session changes: when the microphone opens (the session switches to
// play-and-record), for a call or Siri, on some route changes, and when the
// app goes to the background. An interrupted context stops the audio
// processor, so nothing is sent or heard although the call looks connected.
// Resume it when that happens and on the next touch or return to the page
// (WebKit may only allow it from a user gesture), and say so when it stays off.
async function resumeAudio(s,reason){
  const context=s?.context;
  if(!context||context.state==='running'||context.state==='closed')return true;
  try{await bounded(context.resume(),3000,'resume timed out');}catch(e){console.warn(`[voice] audio resume failed (${reason}): ${e?.message}`);}
  const running=context.state==='running';
  console.info(`[voice] audio ${running?'resumed':`still ${context.state}`} (${reason})`);
  if(session===s){
    if(!running){s.audioPaused=true;notice('Voice audio was paused by the system. Tap this page to resume.');}
    else if(s.audioPaused){s.audioPaused=false;notice('Voice audio resumed.');}
  }
  return running;
}
function watchAudio(s){
  s.context.onstatechange=()=>{
    console.info(`[voice] audio context ${s.context.state} at ${s.context.sampleRate} Hz`);
    if(session===s&&s.context.state!=='running'&&s.context.state!=='closed'&&!document.hidden)resumeAudio(s,'statechange');
  };
}
for(const [target,kind] of [[document,'pointerdown'],[document,'keydown'],[document,'visibilitychange'],[window,'focus'],[window,'pageshow']])
  target.addEventListener(kind,()=>{if(session?.context&&!document.hidden)resumeAudio(session,kind);},{capture:true,passive:true});
// </audio-resume>
// <join-errors> Microphone refusals name the setting to change on this platform.
function permissionHint(){
  switch(inNativeApp()?appInfo.platform:'web'){
    case 'ios':return 'Allow the microphone for this app in Settings → Privacy & Security → Microphone, then join again.';
    case 'macos':return 'Allow the microphone for this app in System Settings → Privacy & Security → Microphone, then join again.';
    case 'android':return 'Allow the microphone in Settings → Apps → this app → Permissions, then join again.';
    case 'windows':return 'Allow desktop apps to use the microphone in Settings → Privacy & security → Microphone, then join again.';
    case 'linux':return 'Check that a microphone is connected and that the desktop allows this app to record, then join again.';
    default:return 'Allow the microphone for this site in the browser, then join again.';
  }
}
function joinErrorText(error){
  if(error?.name==='NotAllowedError'||error?.name==='SecurityError')
    return `${appInfo?.record_permission==='denied'?'Microphone access is off for this app.':'Microphone access was refused.'} ${permissionHint()}`;
  if(error?.name==='NotFoundError'||error?.name==='OverconstrainedError')return 'No usable microphone was found. Connect one or choose another, then join again.';
  if(error?.name==='NotReadableError')return 'The microphone is in use by another app or the system. Close it, then join again.';
  return error?.message||String(error);
}
// </join-errors>
async function bounded(promise,ms,message){
 let timer;try{return await Promise.race([promise,new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error(message)),ms);})]);}finally{clearTimeout(timer);}
}
function updateControls(){
  $('join').disabled=!token||!!session||joining;$('leave').disabled=!session;
  $('device').disabled=!!session||joining;$('mode').disabled=joining;
  $('mute').disabled=!session?.ready||$('mode').value==='off'||!!session?.held;$('ptt').disabled=!session?.ready||muted||$('mode').value!=='ptt'||!!session?.held;
  $('logout').disabled=!token;$('signin').disabled=!!session||joining;
  $('radio-enable').disabled=!session?.ready||!radioAvailable;
  $('noise-suppression').disabled=!noiseSupported||joining||!!session?.reconfiguring;
}
// The station forwards at most max_talkers at once (talk floor): a request
// waits until a slot frees. Stations without a floor put every request on air.
const onAir=s=>!!s&&(!s.floorControl||s.floor==='on_air');
function countText(n){return session?.maxParticipants?`${n} / ${session.maxParticipants}`:`${n}`;}
function micState(enabled,mode){
  if(!session)return 'Microphone off';
  if(session.held)return 'Paused · your watch has voice';
  if(session.reconfiguring)return 'Updating microphone…';
  if(enabled&&!onAir(session))return `Waiting for a free channel · ${session.talkers??session.maxTalkers} of ${session.maxTalkers} talking`;
  if(enabled)return 'Transmitting';
  if(mode==='off')return 'Microphone off · listening';
  if(muted)return 'Microphone muted';
  return 'Listening · hold to talk';
}
function transmit(){
  const mode=$('mode').value;
  const enabled=!!session?.ready&&!session.reconfiguring&&!session.held&&!muted&&(mode==='open'||(mode==='ptt'&&held));
  if(session){
    if(session.transmitting!==enabled){
      session.transmitting=enabled;
      if(session.ws?.readyState===WebSocket.OPEN)session.ws.send(JSON.stringify({type:'transmitting',enabled}));
    }
    const radio=$('radio-enable').checked&&$('mode').value==='ptt';
    if(session.radio!==radio&&session.ws?.readyState===WebSocket.OPEN){session.radio=radio;session.ws.send(JSON.stringify({type:'radio',enabled:radio}));}
    session.stream?.getAudioTracks().forEach(t=>{t.enabled=enabled;});
    session.node?.port.postMessage({type:'settings',transmit:enabled,gain:Number($('gain').value)/100,volume:Number($('volume').value)/100});
  }
  $('ptt').setAttribute('aria-pressed',String(enabled&&$('mode').value==='ptt'));
  $('mute').setAttribute('aria-pressed',String(muted));$('mute').textContent=muted?'Unmute microphone':'Mute microphone';
  $('mic-state').textContent=micState(enabled,mode);
  $('mic-state').dataset.floor=session?.transmitting?(onAir(session)?'on_air':'waiting'):'idle';
  updateControls();
}
function leave(message='Left voice. Microphone released.'){
  window.GS26StayAwake?.setActive('voice',false);
  const old=session;session=null;held=false;joining=false;$('radio-enable').checked=false;
  if(old){clearInterval(old.ping);clearTimeout(old.joinTimeout);old.ws?.close();old.stream?.getTracks().forEach(t=>t.stop());old.node?.disconnect();old.context?.close().catch(()=>{});}
  $('peers').replaceChildren();$('count').textContent='0';$('level').value=0;transmit();notice(message);outputActive();
}
async function devices(){
  try{
    const selected=$('device').value;
    const list=await navigator.mediaDevices.enumerateDevices();$('device').replaceChildren(new Option('System default',''));
    list.filter(d=>d.kind==='audioinput').forEach((d,i)=>$('device').add(new Option(d.label||`Microphone ${i+1}`,d.deviceId)));
    if([...$('device').options].some(o=>o.value===selected))$('device').value=selected;
  }catch(e){notice(`Cannot list microphones: ${e.message}`);}
}
$('login').onsubmit=async event=>{
  event.preventDefault();$('signin').disabled=true;
  const password=$('password').value;$('password').value='';
  try{const result=await signIn($('username').value,password);token=result.token;notice('Signed in. Choose Join voice to enable your microphone.');}
  catch(e){notice(e.message);}finally{updateControls();}
};
$('logout').onclick=async()=>{
  leave();const old=token;token='';updateControls();
  try{await fetch('/api/auth/logout',{method:'POST',headers:{Authorization:`Bearer ${old}`}});}finally{notice('Signed out.');}
};
$('join').onclick=async()=>{
  if(session||joining)return;
  const unavailable=voiceUnavailableReason();
  if(unavailable)return notice(unavailable);
  joining=true;muted=false;held=false;updateControls();notice('Starting voice audio…');
  const s={ready:false,transmitting:false};session=s;
  try{
    const Context=window.AudioContext||window.webkitAudioContext;
    if(!Context)throw new Error('This browser does not support voice audio');
    s.context=new Context();watchAudio(s);
    await bounded(s.context.resume(),8000,'Audio could not start. Allow audio playback, then join again.');
    if(!s.context.audioWorklet)throw new Error('Voice audio requires a trusted HTTPS connection and AudioWorklet support');
    await bounded(s.context.audioWorklet.addModule('/assets/voice-worklet.js'),8000,'The voice audio processor did not load. Reload and try again.');
    if(session!==s)return;
    notice('Checking voice access…');
    const access=await fetch('/api/voice/status',{headers:{Authorization:`Bearer ${token}`},cache:'no-store',signal:AbortSignal.timeout(8000)});
    if(!access.ok){
      if(access.status===401)throw new Error('Your session expired or is no longer valid. Sign in again from the dashboard.');
      throw new Error(`Voice access check failed (HTTP ${access.status}). Try again or check the server log.`);
    }
    const capabilities=await access.json();
    if(!capabilities?.can_transmit)throw new Error('Your account needs the voice_transmit permission. Stream viewers can listen through the audience program.');
    radioLink(capabilities.radio);
    if(session!==s)return;
    notice('Waiting for microphone permission…');
    const microphone=navigator.mediaDevices.getUserMedia({audio:{deviceId:$('device').value?{exact:$('device').value}:undefined,channelCount:1,echoCancellation:true,...noiseConstraint($('noise-suppression').checked),autoGainControl:false}});
    microphone.then(stream=>{if(session!==s)stream.getTracks().forEach(track=>track.stop());},()=>{});
    const stream=await bounded(microphone,30000,`Microphone access is still pending. ${permissionHint()}`);
    if(session!==s){stream.getTracks().forEach(t=>t.stop());return;}
    s.stream=stream;s.stream.getAudioTracks().forEach(t=>{t.enabled=false;t.onended=()=>{if(session===s)leave('Microphone disconnected. Rejoin to choose another device.');};});
    s.node=new AudioWorkletNode(s.context,'crew-audio',{numberOfInputs:1,numberOfOutputs:1,outputChannelCount:[1]});
    s.source=s.context.createMediaStreamSource(stream);s.source.connect(s.node);s.node.connect(s.context.destination);
    applyNoise(s);await resumeAudio(s,'microphone started');
    applySink(s);outputActive();
    s.node.port.onmessage=({data:m})=>{
      if(session!==s)return;
      if(m.type==='level')$('level').value=m.value;
      if(m.type==='pcm'&&s.transmitting&&onAir(s)&&s.ws.readyState===WebSocket.OPEN&&s.ws.bufferedAmount<2560)s.ws.send(m.buffer);
    };
    const url=new URL('/api/voice/ws',location.href);url.protocol=location.protocol==='https:'?'wss:':'ws:';
    s.ws=new WebSocket(url);s.ws.binaryType='arraybuffer';
    s.ws.onopen=()=>{if(session===s)s.ws.send(JSON.stringify({type:'auth',token}));};
    s.ws.onmessage=event=>{
      if(session!==s)return;
      if(event.data instanceof ArrayBuffer){
        if(event.data.byteLength!==644)return;
        const id=new DataView(event.data).getUint32(0,true),buffer=event.data.slice(4);
        s.node.port.postMessage({type:'audio',id,buffer},[buffer]);return;
      }
      let m;try{m=JSON.parse(event.data);}catch{return;}
      if(m.type==='error'){leave(m.message);return;}
      if(m.type==='radio_link'){radioLink(m.radio);}
      if(m.type==='radio_state'){$('radio-enable').checked=m.enabled;session.radio=m.enabled;}
      // The watch linked to this session joined voice: the station mutes this
      // page (no audio in, microphone ignored) so phone and watch never echo.
      if(m.type==='handoff'){
        const wasHeld=!!s.held;s.held=m.held===true;held=false;
        if(s.held){
          if($('mode').value==='open')$('mode').value='off';
          $('radio-enable').checked=false;
          for(const id of s.peers||[])s.node.port.postMessage({type:'remove',id});
          if(!wasHeld)notice('Your watch has voice. This page\'s microphone and speaker are paused until the watch leaves voice.');
        }else if(wasHeld)notice('Your watch left voice. Listening here again; press to talk.');
        transmit();
      }

      if(m.type==='floor'){
        s.floor=m.state;s.talkers=m.talkers;if(Number.isInteger(m.max_talkers))s.maxTalkers=m.max_talkers;
        if(m.reason==='priority')notice('An administrator took a talk slot. You are next in line and go back on air when a slot frees.');
        else if(m.reason==='quiet')notice('You were quiet while others waited to talk. You go back on air when a slot frees.');
        transmit();
      }
      if(m.type==='joined'){clearTimeout(s.joinTimeout);s.ready=true;s.id=m.id;s.maxParticipants=Number.isInteger(m.max_participants)?m.max_participants:0;s.floorControl=Number.isInteger(m.max_talkers);s.maxTalkers=m.max_talkers;s.floor='idle';joining=false;window.GS26StayAwake?.setActive('voice',true);transmit();notice('Connected to voice chat.');}
      if(m.type==='roster'){
        const next=new Set(m.peers.map(p=>p.id));
        for(const id of s.peers||[])if(!next.has(id))s.node.port.postMessage({type:'remove',id});s.peers=next;
        $('peers').replaceChildren();$('count').textContent=countText(m.peers.length);
        for(const p of m.peers){const li=document.createElement('li');li.textContent=`${p.name}${p.device==='watch'?' (watch)':''}${p.id===s.id?' (you)':''} · ${p.transmitting?'On air':p.waiting?'Waiting to talk':'Listening'}`;li.className=p.transmitting?'talking':p.waiting?'waiting':'';$('peers').append(li);}
        // The roster is resent every second: it also repairs a lost floor message.
        const me=m.peers.find(p=>p.id===s.id);
        if(me&&s.floorControl){const floor=me.transmitting?'on_air':me.waiting?'waiting':'idle';if(floor!==s.floor){s.floor=floor;s.talkers=m.peers.filter(p=>p.transmitting).length;transmit();}}
      }
    };
    s.ws.onclose=()=>{if(session===s)leave('Voice disconnected. Your microphone is off; select Join voice to reconnect.');};
    s.ws.onerror=()=>{if(session===s)leave('Cannot connect to voice. Check your Ground Station connection.');};
    s.ping=setInterval(()=>{if(s.ws.readyState===WebSocket.OPEN)s.ws.send(JSON.stringify({type:'ping'}));},10000);
    s.joinTimeout=setTimeout(()=>{if(session===s&&!s.ready)leave('Voice connection timed out. Check the station connection and join again.');},10000);
    notice('Connecting to voice…');await devices();
  }catch(e){if(session===s)leave(`Cannot join voice: ${joinErrorText(e)}`);}
};
$('noise-suppression').onchange=async()=>{
  const enabled=$('noise-suppression').checked;
  const current=session;
  if(!current?.stream)return showNoisePlan();
  // The built-in suppressor switches at once; the system one is a microphone
  // setting that some devices apply only when the microphone opens again.
  if(!systemNoiseSupported){applyNoise(current);return;}
  current.reconfiguring=true;held=false;transmit();
  let replacement;
  try {
    let actual;
    try {
      for(const track of current.stream.getAudioTracks())await track.applyConstraints({noiseSuppression:{exact:enabled}});
      actual=current.stream.getAudioTracks()[0]?.getSettings?.().noiseSuppression;
    } catch { /* Some devices apply processing options only when opened. */ }
    if(session!==current)return;
    if(actual!==enabled){
      replacement=await navigator.mediaDevices.getUserMedia({audio:{deviceId:$('device').value?{exact:$('device').value}:undefined,channelCount:1,echoCancellation:true,noiseSuppression:{exact:enabled},autoGainControl:false}});
      replacement.getAudioTracks().forEach(track=>{track.enabled=false;});
      if(session!==current)return;
      const source=current.context.createMediaStreamSource(replacement);
      source.connect(current.node);
      const old=current.stream;
      current.source.disconnect();current.source=source;current.stream=replacement;replacement=null;
      current.stream.getAudioTracks().forEach(track=>{track.onended=()=>{if(session===current)leave('Microphone disconnected. Rejoin to choose another device.');};});
      old.getTracks().forEach(track=>{track.onended=null;track.stop();});
    }
  } catch(error) {
    if(session===current)console.warn('[voice] system noise suppression change failed',error?.message);
  } finally {
    replacement?.getTracks().forEach(track=>track.stop());
    current.reconfiguring=false;
    // Whatever the device did, the status shows what now runs.
    if(session===current){applyNoise(current);transmit();}
    updateControls();
  }
};
$('leave').onclick=()=>leave();$('devices').onclick=devices;
$('mute').onclick=()=>{muted=!muted;held=false;transmit();};
$('mode').onchange=()=>{held=false;if($('mode').value!=='ptt')$('radio-enable').checked=false;transmit();};
$('radio-enable').onchange=()=>{if($('radio-enable').checked)$('mode').value='ptt';held=false;transmit();};
for(const name of ['gain','volume'])$(name).oninput=()=>{$(`${name}-label`).textContent=`${$(name).value}%`;transmit();};
$('ptt').onpointerdown=event=>{if($('ptt').disabled)return;event.preventDefault();$('ptt').setPointerCapture(event.pointerId);held=true;transmit();};
const release=()=>{held=false;transmit();};
for(const event of ['pointerup','pointercancel','lostpointercapture'])$('ptt').addEventListener(event,release);
window.addEventListener('keydown',event=>{
  if(event.code!=='Space'||event.repeat||event.target.closest('input,select,textarea,[contenteditable=true]')||event.target.closest('button')&&event.target!==$('ptt'))return;
  if($('ptt').disabled)return;event.preventDefault();held=true;transmit();
});
window.addEventListener('keyup',event=>{if(event.code==='Space'){if(held)event.preventDefault();release();}});
window.addEventListener('blur',release);
document.addEventListener('visibilitychange',()=>{if(document.hidden)release();});
window.addEventListener('pagehide',()=>leave());
updateControls();

// An embedded tool shares the dashboard session; no credentials are persisted.
if (new URLSearchParams(location.search).get('embedded') === '1' && parent !== window) {
  document.body.dataset.embedded='true';
  $('login').style.display = 'none';
  document.querySelector('nav').style.display = 'none';
  notice('Waiting for the dashboard session…');
  let receivedSession = false;
  window.addEventListener('message', event => {
    if (event.source !== parent || event.data?.type !== 'gs26-session' || typeof event.data.token !== 'string') return;
    if (event.data.theme && typeof event.data.theme==='object') {
      for(const name of ['--gs26-app-background','--gs26-app-text','--gs26-panel-background','--gs26-panel-alt-background','--gs26-border','--gs26-text-muted','--gs26-button-background','--gs26-button-border','--gs26-button-text']) {
        const value=event.data.theme[name];
        if(typeof value==='string' && CSS.supports('color',value))document.documentElement.style.setProperty(name,value);
      }
    }
    dashboardOrigin = event.origin;
    if (event.data.app && typeof event.data.app === 'object') { appInfo = event.data.app; if (!session?.stream) showNoisePlan(); }
    showOutput();
    if (!event.data.visible) release();
    if (!receivedSession || token !== event.data.token) {
      receivedSession = true;
      leave(); token = event.data.token;
      notice(!token ? 'Sign in from the dashboard to join voice chat.' : voiceUnavailableReason() || 'Dashboard session connected. Choose Join voice to enable your microphone.');
      updateControls();
    }
  });
}

for(const kind of ['contextmenu','selectstart','dragstart'])$('ptt').addEventListener(kind,event=>event.preventDefault());
$('ptt').addEventListener('touchstart',event=>event.preventDefault(),{passive:false});
