// Real MediaMTX/backend integration; synthetic cameras and isolated browser profiles.
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const base=process.env.GS_MEDIA_TEST_ORIGIN||'http://127.0.0.1:19090';
assert(['127.0.0.1','localhost'].includes(new URL(base).hostname));
const headers=name=>({Authorization:`Bearer synthetic-${name}`});
async function api(path,name='producer',body){const response=await fetch(base+path,{method:body?'POST':'GET',headers:{...headers(name),...(body?{'Content-Type':'application/json'}:{})},body:body?JSON.stringify(body):undefined});return response;}
async function until(fn,timeout=45000){const end=Date.now()+timeout;while(Date.now()<end){const value=await fn();if(value)return value;await new Promise(r=>setTimeout(r,250));}throw Error('Timed out waiting for live playback');}
for(const name of ['viewer','publisher','operator']){
 for(const path of ['/api/video/streams','/api/video/recordings','/api/video/transcode/hosts'])assert.equal((await api(path,name)).status,403,`${name} cannot access ${path}`);
}
const publisher=await(await api('/api/live_streams','publisher')).json();assert.equal(publisher.can_publish_camera,true);assert.equal(publisher.can_manage_stream,false);assert.equal(publisher.can_preview_live,false);
let configuration=await(await api('/api/live_streams')).json();
const expectEncoded=configuration.streams.some(stream=>stream.url.includes('/transcoded-'));
let response=await api('/api/live_streams/control','producer',{...configuration.broadcast,delay_seconds:3,layout:'hero',featured_stream_id:'front',hidden_stream_ids:[]});assert.equal(response.status,200);
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY,args:['--autoplay-policy=no-user-gesture-required','--disable-audio-output','--disable-background-timer-throttling','--disable-renderer-backgrounding','--disable-backgrounding-occluded-windows']});
const contexts=[];const errors=[];
try{
 async function viewer(clean=false){
  const context=await browser.newContext({viewport:{width:390,height:844}});contexts.push(context);
  await context.addInitScript(()=>{navigator.mediaDevices.getUserMedia=()=>{throw Error('Audience must not request a microphone');};});
  const page=await context.newPage();page.on('pageerror',error=>errors.push(error.message));
  const config=await(await api('/api/live_streams','viewer')).json();
  await page.goto(base+config.program_url+(clean?'&clean=1':''));
  await until(()=>page.evaluate(encoded=>[...players.values()].some(p=>p.ready&&p.video.getVideoPlaybackQuality().totalVideoFrames>2&&(!encoded||p.url.includes('/transcoded-'))),expectEncoded));
  return page;
 }
 async function sample(page,seconds){
  const rows=[];
  for(let i=0;i<seconds;i++){
   rows.push(await page.evaluate(()=>[...players.entries()].filter(([,p])=>!p.retiring).map(([id,p])=>({id,url:p.url,decoderRecoveries:p.decoderRecoveries,time:p.video.currentTime,frames:p.video.getVideoPlaybackQuality().totalVideoFrames,ready:p.ready,paused:p.video.paused,seeks:p.lastSeek}))));
   await page.waitForTimeout(1000);
  }
  assert(rows.every(r=>r.length),'camera must remain attached');
  if(expectEncoded)assert(rows.every(row=>row.every(p=>p.url.includes('/transcoded-'))),'remote encoding must remain active without silently falling back to raw cameras');
  const first=rows[0][0],last=rows.at(-1)[0];assert(last.frames-first.frames>seconds*5,`continuous decoded frames: ${JSON.stringify(rows)}`);
  assert(last.time-first.time>seconds-3,`playback clock stalled: ${JSON.stringify(rows)}`);
  let stalled=0;for(let i=1;i<rows.length;i++){if(rows[i][0].frames===rows[i-1][0].frames)stalled++;}
  assert(stalled<=1,`repeated frozen frames: ${JSON.stringify(rows)}`);
  return {seconds,frames:last.frames-first.frames,advanced_s:last.time-first.time,stalled_samples:stalled};
 }
 async function cameraSound(page){
  const unmute=page.getByRole('button',{name:'Unmute stream',exact:true});
  if(await unmute.isVisible())await unmute.click();
  await page.waitForFunction(()=>document.querySelector('video')&&!document.querySelector('video').muted);
  await page.evaluate(async()=>{
   const context=new AudioContext(),source=context.createMediaElementSource(document.querySelector('video')),analyser=context.createAnalyser();
   analyser.fftSize=256;source.connect(analyser);analyser.connect(context.destination);await context.resume();
   window.cameraSoundProbe={context,source,analyser};
  });
  const rms=await until(()=>page.evaluate(()=>{
   const values=new Float32Array(256);cameraSoundProbe.analyser.getFloatTimeDomainData(values);
   const rms=Math.sqrt(values.reduce((sum,value)=>sum+value*value,0)/values.length);
   return rms>.001?rms:0;
  }),10000);
  assert(rms>.001,'actual AAC camera audio must decode to non-silent PCM');
  return {rms};
 }
 const first=await viewer();const cameraAudio=await cameraSound(first);const solo=await sample(first,8);
 const second=await viewer();const concurrent=await Promise.all([sample(first,10),sample(second,10)]);
 await contexts[1].close();const afterDisconnect=await sample(first,6);
 // Saved layouts affect the actual camera tiles, not only the decorative graphic.
 configuration=await(await api('/api/live_streams')).json();
 const tiles=[{stream_id:'side',x:0,y:0,width:65,height:100},{stream_id:'front',x:65,y:0,width:35,height:100}];
 response=await api('/api/live_streams/control','producer',{...configuration.broadcast,layout:'tiles',tiles});assert.equal(response.status,200);
 await until(()=>first.evaluate(()=>players.get('side')?.ready&&players.get('front')?.ready));
 await until(()=>first.evaluate(()=>players.get('side').tile.style.width==='65%'&&players.get('front').tile.style.left==='65%'));
 assert.equal(await first.locator('#banner').innerText().then(t=>t.includes('Video delay')),false);
 const clean=await viewer(true);assert.equal(await clean.getByRole('button').count(),0,'OBS feed contains no controls');assert.equal(await clean.locator('#playback-controls').isVisible(),false);
 assert(!await clean.locator('body').innerText().then(t=>/Buffering delayed|Video delay|Exit stream/.test(t)));
 await sample(clean,5);
 await clean.evaluate(()=>{window.dispatchEvent(new PageTransitionEvent('pagehide',{persisted:true}));window.dispatchEvent(new PageTransitionEvent('pageshow',{persisted:true}));});
 await until(()=>clean.evaluate(()=>[...players.values()].some(p=>p.ready&&p.video.getVideoPlaybackQuality().totalVideoFrames>2)));
 const cleanAudio=await cameraSound(clean);assert.equal(await clean.getByRole('button').count(),0);assert.deepEqual(errors,[]);
 console.log('PASS real one/two viewers, decoded camera audio, long-GOP continuous playback, independent disconnect, saved camera geometry and clean OBS',JSON.stringify({cameraAudio,cleanAudio,solo,concurrent,afterDisconnect}));
}finally{await browser.close();}
