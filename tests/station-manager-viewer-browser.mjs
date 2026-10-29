// Manager preview lifecycle: synthetic tracks and isolated HTTP routes, no station/devices.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY,args:['--autoplay-policy=no-user-gesture-required','--disable-audio-output']});
try{
 const page=await browser.newPage(),errors=[],requests=[],failed=[];let allowed=true,posts=0,deletes=0;
 page.on('pageerror',e=>errors.push(e.message));page.on('requestfailed',request=>failed.push(new URL(request.url()).pathname));
 await page.clock.install();
 await page.addInitScript(()=>{
  window.peers=[];
  navigator.mediaDevices.getUserMedia=()=>{throw Error('Preview must not capture camera or microphone');};
  window.RTCPeerConnection=class{
   constructor(){this.kinds=[];this.iceGatheringState='complete';this.connectionState='new';window.peers.push(this);}
   addTransceiver(kind){this.kinds.push(kind);}
   async createOffer(){return {type:'offer',sdp:'synthetic offer'};}
   async setLocalDescription(offer){this.localDescription=offer;}
   async setRemoteDescription(){
    const canvas=document.createElement('canvas');canvas.width=16;canvas.height=16;
    this.audio=new AudioContext();const oscillator=this.audio.createOscillator(),output=this.audio.createMediaStreamDestination();oscillator.connect(output);oscillator.start();
    this.tracks=[canvas.captureStream(10).getVideoTracks()[0],output.stream.getAudioTracks()[0]];
    this.tracks.forEach(track=>this.ontrack?.({track}));this.lateTrack=this.ontrack;
    this.connectionState='connected';this.onconnectionstatechange?.();
   }
   close(){this.closed=true;this.connectionState='closed';this.audio?.close().catch(()=>{});}
  };
 });
 await page.route('**/*',async route=>{
  const request=route.request(),url=new URL(request.url());requests.push({path:url.pathname,method:request.method(),auth:request.headers().authorization});
  const json=value=>route.fulfill({json:value});
  if(url.pathname==='/media')return route.fulfill({contentType:'text/html',body:readFileSync(process.env.GS_MEDIA_PAGE_SOURCE||'station_ui/pages/media.html')});
  if(url.pathname==='/assets/stay-awake.js')return route.fulfill({contentType:'text/javascript',body:readFileSync('station_ui/scripts/stay-awake.js')});
  if(url.pathname==='/api/live_streams')return json({can_publish_camera:false,can_manage_stream:allowed});
  if(url.pathname==='/api/video/streams')return json([{id:'pad',name:'Pad',live:true}]);
  if(url.pathname==='/api/stage-models')return json([]);
  if(url.pathname==='/api/video/recordings')return json({recordings:[],total:0,offset:0});
  if(url.pathname==='/api/video/streams/pad/whep'){
   assert.equal(request.headers().authorization,'Bearer manager');posts++;
   if(posts===1)return; // A radio/network interruption leaves signaling pending.
   return route.fulfill({status:201,headers:{Location:'/api/video/streams/pad/whep/preview-'+posts},body:'synthetic answer'});
  }
  if(url.pathname.startsWith('/api/video/streams/pad/whep/')){assert.equal(request.method(),'DELETE');deletes++;return route.fulfill({status:204});}
  throw Error('Unexpected request '+url.pathname);
 });
 await page.goto('http://127.0.0.1:19999/media',{waitUntil:'domcontentloaded'});
 await page.evaluate(async()=>{token='manager';await refresh();});
 await page.waitForFunction(()=>peers.length===1);
 await page.clock.runFor(26000);
 await page.waitForFunction(()=>peers.length>=2&&peers.at(-1).connectionState==='connected',undefined,{timeout:3000});
 assert.equal(posts,2,'stalled signaling retries once, without duplicate live sessions');assert(failed.includes('/api/video/streams/pad/whep'),'abandoned signaling fetch is aborted');
 assert.equal(await page.evaluate(()=>peers[0].closed),true,'stalled peer closed');
 assert.deepEqual(await page.locator('#cameras video').evaluate(v=>v.srcObject.getTracks().map(t=>t.kind).sort()),['audio','video']);
 await page.evaluate(()=>window.dispatchEvent(new PageTransitionEvent('pagehide',{persisted:true})));
 assert.equal(await page.evaluate(()=>peers.at(-1).tracks.every(t=>t.readyState==='ended')),true,'leaving releases received tracks');
 await page.clock.runFor(15000);
 assert.equal(posts,2,'hidden/restored documents do not recreate viewers before pageshow');
 await page.evaluate(()=>window.dispatchEvent(new PageTransitionEvent('pageshow',{persisted:true})));
 await page.waitForFunction(()=>peers.length===3&&peers.at(-1).connectionState==='connected');
 allowed=false;await page.evaluate(()=>refreshPublishPermission());
 assert.equal(await page.locator('#cameras article').count(),0,'revoked access removes previews');
 assert.equal(await page.evaluate(()=>peers.at(-1).tracks.every(t=>t.readyState==='ended')),true);
 assert.equal(deletes,2,'each connected abandoned session is deleted');
 assert.deepEqual(errors,[]);
 console.log('PASS manager preview stalled-signaling timeout, combined tracks, hidden-page suspension, restore and permission cleanup');
}finally{await browser.close();}
