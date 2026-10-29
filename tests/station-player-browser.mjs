// Deterministic WHEP lifecycle/track regression; no camera, microphone or station.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY,args:['--autoplay-policy=no-user-gesture-required','--disable-audio-output']});
try{
  const page=await browser.newPage();
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  let posts=0,deletes=0;
  await page.addInitScript(()=>{
    window.peers=[];
    navigator.mediaDevices.getUserMedia=()=>{throw Error('A viewer must not request capture permission');};
    window.RTCPeerConnection=class{
      constructor(){this.kinds=[];this.iceGatheringState='complete';this.connectionState='new';window.peers.push(this);}
      addTransceiver(kind){this.kinds.push(kind);}
      async createOffer(){return {type:'offer',sdp:'synthetic offer'};}
      async setLocalDescription(offer){this.localDescription=offer;}
      async setRemoteDescription(){
        const canvas=document.createElement('canvas');canvas.width=16;canvas.height=16;
        this.context=new AudioContext();
        const source=this.context.createOscillator(),destination=this.context.createMediaStreamDestination();source.connect(destination);source.start();
        const video=canvas.captureStream(10).getVideoTracks()[0],audio=destination.stream.getAudioTracks()[0];
        canvas.getContext('2d').fillRect(0,0,16,16);video.requestFrame?.();
        this.ontrack?.({track:video,streams:[]});this.ontrack?.({track:audio,streams:[]});
        const callback=this.ontrack;window.emitLateTrack=()=>callback?.({track:video,streams:[]});
        this.connectionState='connected';this.onconnectionstatechange?.();
      }
      close(){this.closed=true;this.connectionState='closed';this.context?.close().catch(()=>{});}
    };
  });
  await page.route('**/*',async route=>{
    const url=new URL(route.request().url());
    assert.equal(url.searchParams.get('ticket'),'player-ticket','WHEP operations retain their scoped capability');
    if(url.pathname==='/player/camera')return route.fulfill({contentType:'text/html',body:readFileSync(process.env.GS_PLAYER_PAGE_SOURCE||'station_ui/pages/player.html')});
    if(url.pathname==='/api/video/streams/camera/whep'){posts++;return route.fulfill({status:201,headers:{Location:'/api/video/sessions/session-'+posts},body:'synthetic answer'});}
    if(url.pathname.startsWith('/api/video/sessions/')){assert.equal(route.request().method(),'DELETE');deletes++;return route.fulfill({status:204});}
    throw Error('Unexpected viewer request '+url.pathname);
  });
  await page.goto('http://127.0.0.1:19999/player/camera?ticket=player-ticket',{waitUntil:'domcontentloaded'});
  await page.waitForFunction(()=>window.peers[0]?.connectionState==='connected');
  await page.waitForLoadState('load',{timeout:5000});
  assert.deepEqual(await page.evaluate(()=>peers[0].kinds),['video','audio'],'viewers negotiate camera microphone audio');
  assert.deepEqual(await page.locator('video').evaluate(v=>v.srcObject.getTracks().map(t=>t.kind).sort()),['audio','video'],'track events share one media stream');
  await page.evaluate(()=>window.dispatchEvent(new PageTransitionEvent('pagehide',{persisted:true})));
  await page.waitForFunction(()=>peers[0].closed);
  assert.equal(await page.locator('video').evaluate(v=>v.srcObject),null,'leaving detaches stale media');
  await page.evaluate(()=>window.emitLateTrack());
  assert.equal(await page.locator('video').evaluate(v=>v.srcObject),null,'late old-peer tracks stay detached');
  await page.evaluate(()=>window.dispatchEvent(new PageTransitionEvent('pageshow',{persisted:true})));
  await page.waitForFunction(()=>window.peers[1]?.connectionState==='connected');
  assert.deepEqual(await page.locator('video').evaluate(v=>v.srcObject.getTracks().map(t=>t.kind).sort()),['audio','video']);
  assert.equal(posts,2,'returning starts exactly one new viewer session');
  await page.waitForFunction(()=>document.querySelector('p').hidden);
  assert.equal(deletes,1,'the abandoned viewer session is deleted');
  assert.deepEqual(errors,[]);
  console.log('PASS: WHEP audio/video negotiation, combined tracks, stale peer cleanup and restored-page reconnect');
}finally{await browser.close();}
