// One explicitly chosen microphone destination at a time. Programme commentary
// uses a separate authenticated endpoint that is not a VC/radio participant.
import {signIn} from '/assets/media-login.js';
const $=id=>document.getElementById(id);
let token='',caps={},context,node,source,microphone,micPending=null,micGeneration=0;
let commentating=false,held=false,monitor=false,stopped=false,refreshing=false;
const links={crew:null,program:null};let captureEpoch=0,captureRoute='off',nextLinkId=0;
const notice=text=>{$('status').textContent=text;};
async function bounded(promise,ms,message){let timer;try{return await Promise.race([promise,new Promise((_,reject)=>{timer=setTimeout(()=>reject(Error(message)),ms);})]);}finally{clearTimeout(timer);}}
function route(){return held?'crew':commentating?'program':'off';}
function settings(){
 const target=route(),ready=target!=='off'&&links[target]?.ready&&links[target].ws.readyState===WebSocket.OPEN;
 microphone?.getAudioTracks().forEach(t=>t.enabled=!!ready);
 const capture=ready?target+':'+links[target].instance:'off';
 if(capture!==captureRoute){captureRoute=capture;captureEpoch++;node?.port.postMessage({type:'settings',transmit:false,capture_id:captureEpoch,gain:1,volume:0});}
 node?.port.postMessage({type:'settings',transmit:!!ready,capture_id:captureEpoch,gain:1,volume:monitor?Number($('volume').value)/100:0});
 for(const kind of ['crew','program']){
  const link=links[kind],enabled=!!ready&&target===kind;
  if(link?.ready&&link.ws.readyState===WebSocket.OPEN&&link.transmitting!==enabled){link.transmitting=enabled;link.ws.send(JSON.stringify({type:'transmitting',enabled}));}
 }
 $('commentate').setAttribute('aria-pressed',String(commentating));$('commentate').textContent=commentating?'Stop commentary microphone':'Start commentary microphone';
 $('speak').setAttribute('aria-pressed',String(held&&!!ready));
 $('mic-state').textContent=target==='crew'?(ready?'Speaking to VC · commentary paused':'Preparing crew microphone…'):target==='program'?(ready?'Commentary to audience only':'Preparing commentary microphone…'):'Microphone off';
 $('commentate').disabled=!token||!caps.can_commentate||!caps.commentary_enabled;
 $('monitor').disabled=!token||!caps.can_listen;$('speak').disabled=!token||!caps.can_transmit;
 $('noise').disabled=!!microphone||!!micPending;
}
function closeLink(kind){const old=links[kind];links[kind]=null;if(old){clearInterval(old.ping);clearTimeout(old.timer);old.ws?.close();old.reject?.(Error('Audio connection closed'));}}
function stopMicrophone(){micGeneration++;microphone?.getTracks().forEach(t=>{t.onended=null;t.stop();});microphone=null;source?.disconnect();source=null;micPending=null;}
function stopAll(message='Audio stopped. Microphone released.'){
 commentating=false;held=false;monitor=false;$('monitor').checked=false;stopMicrophone();closeLink('program');closeLink('crew');node?.disconnect();node=null;context?.close().catch(()=>{});context=null;settings();notice(message);
}
async function audio(){
 if(node)return;
 const next=new AudioContext();context=next;
 try{
  await bounded(next.resume(),8000,'Allow audio playback, then try again.');
  if(!next.audioWorklet)throw Error('Use trusted HTTPS for audio.');
  await bounded(next.audioWorklet.addModule('/assets/voice-worklet.js?program-routing=1'),8000,'Audio processor did not load.');
  if(context!==next){await next.close();throw Error('Audio was stopped.');}
  node=new AudioWorkletNode(next,'crew-audio',{numberOfInputs:1,numberOfOutputs:1,outputChannelCount:[1]});node.connect(next.destination);
  node.port.onmessage=({data:m})=>{
   if(m.type!=='pcm'||!microphone||m.capture_id!==captureEpoch)return;
   const target=route(),link=links[target];
   if(link?.ready&&link.transmitting&&link.ws.readyState===WebSocket.OPEN&&link.ws.bufferedAmount<2560)link.ws.send(m.buffer);
  };
  settings();
 }catch(e){if(context===next){context=null;next.close().catch(()=>{});}throw e;}
}
async function connect(kind){
 if(links[kind]?.ready)return;
 if(links[kind])return links[kind].promise;
 const link={ready:false,transmitting:false,instance:++nextLinkId};links[kind]=link;
 link.promise=new Promise((resolve,reject)=>{
  link.reject=reject;
  const url=new URL(kind==='crew'?'/api/voice/ws':'/api/program/commentary/ws',location.href);url.protocol=location.protocol==='https:'?'wss:':'ws:';
  const ws=link.ws=new WebSocket(url);ws.binaryType='arraybuffer';
  ws.onopen=()=>{if(links[kind]===link)ws.send(JSON.stringify({type:'auth',token}));};
  ws.onmessage=event=>{
   if(links[kind]!==link)return;
   if(event.data instanceof ArrayBuffer){if(kind==='crew'&&monitor&&event.data.byteLength===644&&node){const id=new DataView(event.data).getUint32(0,true),buffer=event.data.slice(4);node.port.postMessage({type:'audio',id,buffer},[buffer]);}return;}
   let m;try{m=JSON.parse(event.data);}catch{return;}
   if(m.type==='error'){reject(Error(m.message||'Audio access denied'));if(kind==='program')commentating=false;else{held=false;monitor=false;$('monitor').checked=false;}closeLink(kind);settings();notice(m.message);return;}
   if(m.type==='joined'){clearTimeout(link.timer);link.ready=true;link.reject=null;resolve();settings();}
   if(kind==='program'&&m.type==='program_state'&&!m.enabled&&commentating){commentating=false;closeLink('program');if(!held)stopMicrophone();settings();notice('Programme commentary was disabled. Microphone stopped.');}
  };
  ws.onclose=()=>{if(links[kind]===link){links[kind]=null;clearInterval(link.ping);clearTimeout(link.timer);if(kind==='program')commentating=false;else{held=false;monitor=false;$('monitor').checked=false;}if(route()==='off')stopMicrophone();settings();notice('Audio disconnected. Microphone routing stopped; reconnect explicitly.');}reject(Error('Audio disconnected'));};
  ws.onerror=()=>reject(Error('Audio connection failed'));
  link.timer=setTimeout(()=>{reject(Error('Audio connection timed out'));closeLink(kind);},10000);
  link.ping=setInterval(()=>{if(ws.readyState===WebSocket.OPEN)ws.send(JSON.stringify({type:'ping'}));},10000);
 });
 return link.promise;
}
async function mic(){
 if(microphone)return;if(micPending)return micPending;
 if(!isSecureContext||!navigator.mediaDevices?.getUserMedia)throw Error('Microphone access requires HTTPS.');
 const generation=++micGeneration;
 const request=navigator.mediaDevices.getUserMedia({audio:{channelCount:1,echoCancellation:true,noiseSuppression:$('noise').checked,autoGainControl:false},video:false});
 request.then(stream=>{if(generation!==micGeneration||route()==='off')stream.getTracks().forEach(t=>t.stop());},()=>{});
 const pending=(async()=>{
  const stream=await bounded(request,30000,'Microphone permission is still pending. Check browser permissions and try again.');
  if(generation!==micGeneration||route()==='off'){stream.getTracks().forEach(t=>t.stop());throw Error('Microphone request cancelled');}
  microphone=stream;stream.getAudioTracks().forEach(t=>{t.enabled=false;t.onended=()=>stopAll('Microphone disconnected.');});
  source=context.createMediaStreamSource(stream);source.connect(node);settings();
 })();micPending=pending;
 try{await pending;}finally{if(micPending===pending)micPending=null;settings();}
}
async function refresh(){
 if(!token||refreshing||stopped)return;refreshing=true;
 try{const response=await fetch('/api/voice/status',{headers:{Authorization:`Bearer ${token}`},cache:'no-store',signal:AbortSignal.timeout(8000)});if(!response.ok)throw Error('Audio session unavailable. Sign in again.');caps=await response.json();if(!caps.can_commentate||!caps.commentary_enabled){if(commentating){commentating=false;closeLink('program');if(!held)stopMicrophone();}}if(!caps.can_transmit&&held)release();if(!caps.can_listen&&monitor){monitor=false;$('monitor').checked=false;closeLink('crew');}settings();}
 catch(e){caps={};stopAll(e.message);}finally{refreshing=false;}
}
$('commentate').onclick=async()=>{
 if(commentating){commentating=false;closeLink('program');if(!held)stopMicrophone();settings();return;}
 commentating=true;settings();notice('Preparing programme-only microphone…');
 try{await audio();await connect('program');if(commentating){await mic();notice('Commentary reaches only the audience programme, with its delay.');}}
 catch(e){commentating=false;closeLink('program');if(!held)stopMicrophone();settings();notice(e.message);}
};
$('monitor').onchange=async()=>{
 monitor=$('monitor').checked;settings();
 if(!monitor){node?.port.postMessage({type:'reset'});if(!held)closeLink('crew');return;}
 try{await audio();await connect('crew');notice('Monitoring crew voice. Microphone routing is unchanged.');}
 catch(e){monitor=false;$('monitor').checked=false;if(!held)closeLink('crew');settings();notice(e.message);}
};
async function speak(){
 if($('speak').disabled||held)return;held=true;settings();
 try{await audio();await connect('crew');if(held)await mic();settings();}
 catch(e){held=false;if(!monitor)closeLink('crew');if(!commentating)stopMicrophone();settings();notice(e.message);}
}
function release(){held=false;if(!monitor)closeLink('crew');if(!commentating)stopMicrophone();settings();}
$('speak').onpointerdown=e=>{if($('speak').disabled)return;e.preventDefault();$('speak').setPointerCapture(e.pointerId);speak();};
for(const event of ['pointerup','pointercancel','lostpointercapture'])$('speak').addEventListener(event,release);
$('speak').onkeydown=e=>{if(['Space','Enter'].includes(e.code)&&!e.repeat){e.preventDefault();speak();}};
$('speak').onkeyup=e=>{if(['Space','Enter'].includes(e.code)){e.preventDefault();release();}};
window.addEventListener('blur',release);document.addEventListener('visibilitychange',()=>{if(document.hidden)release();});
$('volume').oninput=settings;$('stop').onclick=()=>stopAll();
$('login').onsubmit=async e=>{e.preventDefault();const password=$('password').value;$('password').value='';try{const result=await signIn($('username').value,password);stopAll();token=result.token;await refresh();notice('Choose commentary, VC monitoring or hold-to-speak.');}catch(e){notice(e.message);}};
if(new URLSearchParams(location.search).get('embedded')==='1'&&parent!==window){
 $('login').hidden=true;
 window.addEventListener('message',e=>{
  if(e.source!==parent||e.data?.type!=='gs26-session'||typeof e.data.token!=='string')return;
  if(e.data.theme)for(const [name,value]of Object.entries(e.data.theme)){if(name.startsWith('--gs26-')&&typeof value==='string'&&value.length<160&&!/[;{}]/.test(value))document.documentElement.style.setProperty(name,value);}
  if(token!==e.data.token){stopAll();token=e.data.token;caps={};settings();refresh();}
  if(!e.data.visible)release();
 });
}
const timer=setInterval(refresh,2000);window.addEventListener('pagehide',()=>{stopped=true;clearInterval(timer);stopAll();});settings();

for(const kind of ['contextmenu','selectstart','dragstart'])$('speak').addEventListener(kind,event=>event.preventDefault());
$('speak').addEventListener('touchstart',event=>event.preventDefault(),{passive:false});
