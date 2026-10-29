// Synthetic oscillator capture and fake programme/VC sockets. Never calls a
// physical microphone/camera or connects to a live GroundStation.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
const server=http.createServer((req,res)=>{const path=new URL(req.url,'http://localhost').pathname;if(path==='/assets/voice-worklet.js'){res.setHeader('Content-Type','text/javascript');res.end(fs.readFileSync('station_ui/scripts/voice-worklet.js'));}else{res.statusCode=404;res.end();}});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));const origin=`http://127.0.0.1:${server.address().port}`;
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY,args:['--disable-audio-output','--autoplay-policy=no-user-gesture-required']});
try {
 const context=await browser.newContext({viewport:{width:390,height:844}});const sockets={crew:[],program:[]},frames={crew:[],program:[]},controls=[];const errors=[];
 await context.addInitScript(()=>{
  window.syntheticMicRequests=0;window.syntheticMicStops=0;
  navigator.mediaDevices.getUserMedia=async constraints=>{
   if(constraints.video)throw Error('No camera capture allowed');window.syntheticMicRequests++;
   const context=new AudioContext(),oscillator=context.createOscillator(),gain=context.createGain(),destination=context.createMediaStreamDestination();
   oscillator.frequency.value=440;gain.gain.value=.02;oscillator.connect(gain);gain.connect(destination);oscillator.start();await context.resume();
   const track=destination.stream.getAudioTracks()[0],stop=track.stop.bind(track);track.stop=()=>{if(track.readyState!=='ended'){window.syntheticMicStops++;stop();oscillator.stop();context.close();}};
   return destination.stream;
  };
 });
 await context.route('**/*',route=>{
  const url=new URL(route.request().url());
  if(url.pathname==='/fixture')return route.fulfill({contentType:'text/html',body:`<iframe id="desk" src="/studio-audio?embedded=1" style="border:0;width:100%;height:780px" allow="microphone;autoplay"></iframe><script>window.fixtureToken='studio-fixture';setInterval(()=>document.querySelector('iframe').contentWindow.postMessage({type:'gs26-session',token:fixtureToken,visible:true},location.origin),100);</script>`});
  if(url.pathname==='/studio-audio')return route.fulfill({contentType:'text/html',body:fs.readFileSync('station_ui/pages/studio-audio.html')});
  if(url.pathname==='/assets/studio-audio.js')return route.fulfill({contentType:'text/javascript',body:fs.readFileSync('station_ui/scripts/studio-audio.js')});
  if(url.pathname==='/assets/voice-worklet.js')return route.fulfill({contentType:'text/javascript',body:fs.readFileSync('station_ui/scripts/voice-worklet.js')});
  if(url.pathname==='/assets/media-login.js')return route.fulfill({contentType:'text/javascript',body:'export async function signIn(){return {token:"studio-fixture"}}'});
  if(url.pathname==='/api/voice/status')return route.fulfill({json:{can_listen:true,can_transmit:true,can_commentate:true,commentary_enabled:true,broadcast_enabled:false}});
  return route.fulfill({status:404,body:''});
 });
 await context.routeWebSocket(url=>['/api/voice/ws','/api/program/commentary/ws'].includes(url.pathname),socket=>{
  const kind=new URL(socket.url()).pathname.includes('commentary')?'program':'crew';sockets[kind].push(socket);
  socket.onMessage(message=>{
   if(typeof message==='string'){
    const m=JSON.parse(message);controls.push({kind,...m});
    if(m.type==='auth'){assert.equal(m.token,'studio-fixture');socket.send(JSON.stringify({type:'joined',id:kind==='crew'?1:2,destination:kind==='program'?'audience_program':'crew'}));}
   }else frames[kind].push(Buffer.from(message));
  });
 });
 const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));await page.goto(origin+'/fixture');const desk=page.frameLocator('#desk');
 await desk.getByRole('button',{name:'Start commentary microphone'}).waitFor();await page.waitForFunction(()=>!document.querySelector('iframe').contentDocument.querySelector('#commentate').disabled);
 const frame=page.frames().find(f=>f.url().includes('/studio-audio'));assert(frame);
 assert.equal(await frame.evaluate(()=>syntheticMicRequests),0,'mount/auth does not request microphone');
 await desk.getByRole('button',{name:'Start commentary microphone'}).click();await desk.getByText('Commentary to audience only',{exact:true}).waitFor({timeout:12000}).catch(async e=>{console.error(await frame.locator('body').innerText(),errors,controls);throw e;});
 for(let i=0;i<100&&frames.program.length<5;i++)await page.waitForTimeout(20);
 assert(frames.program.length>=5,'real audio worklet emits programme PCM');assert.equal(sockets.crew.length,0,'commentary never joins crew socket');
 assert.equal(frames.crew.length,0);assert.equal(await frame.evaluate(()=>syntheticMicRequests),1);
 await desk.getByRole('checkbox',{name:'Monitor crew voice (VC)'}).check();
 for(let i=0;i<100&&!sockets.crew.length;i++)await page.waitForTimeout(20);
 assert.equal(sockets.crew.length,1);assert.equal(await frame.evaluate(()=>syntheticMicRequests),1,'monitoring cannot acquire another microphone');
 const incoming=Buffer.alloc(644);incoming.writeUInt32LE(42);for(let i=4;i<644;i+=2)incoming.writeInt16LE(20000,i);sockets.crew[0].send(incoming);
 await page.waitForTimeout(150);assert.equal(frames.crew.length,0,'monitoring while commentating does not transmit to VC');
 const button=await desk.getByRole('button',{name:'Hold to speak to VC'}).boundingBox();await page.mouse.move(button.x+button.width/2,button.y+button.height/2);await page.mouse.down();
 await desk.getByText('Speaking to VC · commentary paused',{exact:true}).waitFor();await page.waitForTimeout(100);const programPaused=frames.program.length;
 for(let i=0;i<100&&frames.crew.length<5;i++)await page.waitForTimeout(20);
 assert(frames.crew.length>=5);await page.waitForTimeout(100);assert.equal(frames.program.length,programPaused,'PTT routes to only one destination');
 await page.mouse.up();await desk.getByText('Commentary to audience only',{exact:true}).waitFor();await page.waitForTimeout(100);const crewStopped=frames.crew.length;
 await page.waitForTimeout(150);assert.equal(frames.crew.length,crewStopped,'release stops crew transmission');assert(frames.program.length>programPaused,'release resumes commentary');
 for(const bytes of [...frames.crew,...frames.program]){assert.equal(bytes.length,640);for(let i=0;i<640;i+=2)assert(Math.abs(bytes.readInt16LE(i))<5000,'monitor playback must not be recaptured into microphone output');}
 assert(!controls.some(m=>m.type==='radio'),'desk can never enable recovery-radio transmission');
 await desk.getByRole('button',{name:'Stop commentary microphone'}).click();await desk.getByText('Microphone off',{exact:true}).waitFor();assert.equal(await frame.evaluate(()=>syntheticMicStops),1);
 for(const width of [320,390,844,1280]){await page.setViewportSize({width,height:900});assert(await frame.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'audio desk fits'+width);}
 await page.evaluate(()=>fixtureToken='');await page.waitForFunction(()=>document.querySelector('iframe').contentDocument.querySelector('#speak').disabled);
 assert(await desk.getByRole('checkbox',{name:'Monitor crew voice (VC)'}).isDisabled());assert.deepEqual(errors,[]);
 await context.close();console.log('PASS: programme-only microphone, independent receive-only VC monitor, explicit exclusive PTT, release/resume, no monitor recapture/radio routing, no automatic capture, auth cleanup and four widths');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
