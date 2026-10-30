// Actual programme player, renderer and bundled rocket; fake read-only recordings.
import fs from 'node:fs';import path from 'node:path';import http from 'node:http';import os from 'node:os';import assert from 'node:assert/strict';import {spawn} from 'node:child_process';
const rendering=path.resolve('station_ui/rendering'),html=fs.readFileSync('station_ui/pages/recorded-vehicle.html');
const requests=[],started=Date.now();let fail=false;
const now=()=>50000+Date.now()-started;
let plan={recording_id:'groundstation_recording_1.db',upper_id:10,start_ms:1000,end_ms:11000,position_ms:2000,anchor_server_ms:now(),speed:1,paused:true,loop_playback:false};
const config={flight_phase_version:1,model_url:'/assets/models/vehicle.glb',attitude:{roll:{data_type:'ROLL',sender_id:'FC',index:0},pitch:{data_type:'PITCH',sender_id:'FC',index:0},yaw:{data_type:'YAW',sender_id:'FC',index:0}},motions:[{node:'motor-flame',transform:'visible',axis:[0,1,0],from:0,to:1,phase_values:{'*':0,Ascent:1}},{node:'drogue-parachute',transform:'scale',axis:[1,1,1],from:.001,to:1,phase_values:{'*':0,Descent:1}},{node:'main-parachute',transform:'scale',axis:[1,1,1],from:.001,to:1,phase_values:{'*':0,Reefing:1,Landed:1,Recovery:1}}],phase_animations:{}};
const points=()=>{const base=plan.recording_id.includes('_2.')?40:43;return [0,1,2,3,4].map((v)=>({timestamp_ms:1000+v*1000,latitude:base+[0,.001,.014,.018,.025][v],longitude:-78+[0,.006,.035,.05,.06][v],altitude_m:[300,4000,10000,8000,1000][v],break_before:v===0}));};
const server=http.createServer((req,res)=>{const u=new URL(req.url,'http://localhost'),p=u.pathname;
 if(p.startsWith('/api/')){requests.push({method:req.method,path:p});if(fail||u.searchParams.get('ticket')!=='audience-fixture'){res.statusCode=401;res.end('unavailable');return;}
  res.setHeader('Content-Type','application/json');
  if(p.endsWith('/state')){res.end(JSON.stringify({plan,config,server_now_ms:now()}));return;}
  if(u.searchParams.get('recording_id')!==plan.recording_id){res.statusCode=409;res.end('{}');return;}
  if(p.endsWith('/trajectory')){res.end(JSON.stringify({recording_id:plan.recording_id,upper_id:plan.upper_id,source_sender_id:'RF',points:points(),truncated:false,sampled:false}));return;}
  const rows=points().map((v,i)=>({id:i+1,timestamp_ms:v.timestamp_ms,sender_id:'RF',data_type:'GPS_DATA',values:[v.latitude,v.longitude,v.altitude_m]}));rows.push({id:6,timestamp_ms:1000,sender_id:'FC',data_type:'PITCH',values:[0]},{id:7,timestamp_ms:1000,sender_id:'FC',data_type:'YAW',values:[0]},{id:8,timestamp_ms:1000,sender_id:'FC',data_type:'ROLL',values:[15]},{id:9,timestamp_ms:1000,sender_id:'FC',data_type:'FLIGHT_STATE',values:[11],phase:'Descent'},{id:10,timestamp_ms:4000,sender_id:'FC',data_type:'FLIGHT_STATE',values:[12],phase:'Reefing'});
  res.end(JSON.stringify({recording_id:plan.recording_id,upper_id:plan.upper_id,next_id:plan.upper_id,has_more:false,rows,seeds:[]}));return;
 }
 if(p==='/'){res.setHeader('Content-Type','text/html');res.end(html);return;}
 const file=p.startsWith('/assets/')?path.resolve(rendering,p.slice('/assets/'.length)):'';
 if(file&&file.startsWith(rendering+path.sep)&&fs.existsSync(file)){res.setHeader('Content-Type',file.endsWith('.js')?'text/javascript':'model/gltf-binary');res.end(fs.readFileSync(file));return;}res.statusCode=404;res.end();
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const profile=fs.mkdtempSync(path.join(os.tmpdir(),'gs-program-replay-'));
const chrome=spawn(process.env.CHROME_BINARY||'/usr/bin/chromium',['--headless','--no-sandbox','--no-first-run','--disable-background-networking','--disable-component-update','--disable-sync','--disable-extensions','--use-gl=angle','--use-angle=swiftshader','--ignore-gpu-blocklist','--enable-webgl','--enable-unsafe-swiftshader','--remote-debugging-port=0','--remote-allow-origins=*','--user-data-dir='+profile,'about:blank'],{stdio:['ignore','ignore','pipe']});
let stderr='';chrome.stderr.on('data',d=>stderr=(stderr+d).slice(-65536));const sleep=ms=>new Promise(r=>setTimeout(r,ms));let ws,id=0;const pending=new Map(),events=[];
try{
 for(let i=0;i<100&&!fs.existsSync(profile+'/DevToolsActivePort');i++)await sleep(100);
 assert(fs.existsSync(profile+'/DevToolsActivePort'),'Chrome started '+stderr.slice(-1000));
 const port=fs.readFileSync(profile+'/DevToolsActivePort','utf8').split('\n')[0],targets=await(await fetch(`http://127.0.0.1:${port}/json/list`)).json();ws=new WebSocket(targets.find(t=>t.type==='page').webSocketDebuggerUrl);await new Promise((r,j)=>{ws.onopen=r;ws.onerror=j;});
 ws.onmessage=e=>{const m=JSON.parse(e.data);if(m.id){const entry=pending.get(m.id);pending.delete(m.id);m.error?entry?.reject(Error(JSON.stringify(m.error))):entry?.resolve(m.result);}else events.push(m);};
 const call=(method,params={})=>new Promise((resolve,reject)=>{const key=++id;pending.set(key,{resolve,reject});ws.send(JSON.stringify({id:key,method,params}));});
 const value=expression=>call('Runtime.evaluate',{expression,returnByValue:true}).then(r=>r.result.value);
 const until=async(expression)=>{for(let i=0;i<160;i++){if(await value(expression))return;await sleep(100);}throw Error('Timed out: '+expression+' '+await value('document.body.innerText'));};
 await call('Runtime.enable');await call('Network.enable');await call('Page.enable');
 for(const width of [1280,390,320]){
  await call('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:width<500});
  await call('Emulation.setTouchEmulationEnabled',{enabled:width<500,maxTouchPoints:2});
  await call('Page.navigate',{url:`http://127.0.0.1:${server.address().port}/?ticket=audience-fixture`});
  await until('document.querySelector("gs-vehicle-viewer")?.dataset.loaded==="true"&&document.querySelector("gs-vehicle-viewer")?.dataset.trailSegments==="2"');
  assert.equal(await value('JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-state")).motions.find(m=>m.node==="drogue-parachute").value'),1,'FC11Descent shows drogue');
  assert.equal(await value('innerWidth'),width);assert.equal(await value('document.documentElement.scrollWidth<=innerWidth'),true);
  assert.equal(await value('document.body.innerText.includes("RECORDED FLIGHT · REPLAY")'),true);
  const layout=await value('(()=>{const note=document.querySelector(".gs-vehicle-reference-note").getBoundingClientRect(),controls=document.querySelector(".controls").getBoundingClientRect();return{note:note.toJSON(),controls:controls.toJSON(),buttons:[...document.querySelectorAll("[data-camera]")].map(b=>{const r=b.getBoundingClientRect();return{width:r.width,height:r.height,onTop:document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)===b}})}})()');
  assert(layout.note.top>=layout.controls.bottom+6,'reference note stays below touch controls');
  assert(layout.note.left>=8&&layout.note.right<=width-8,'reference note fits narrow viewport');
  assert(layout.buttons.every(b=>b.width>=44&&b.height>=44&&b.onTop),'camera controls remain44px unobstructed touch targets');
  if(width<500){
   for(const [action,following] of [['overview',false],['reset',true]]){
    const point=await value(`(()=>{const r=document.querySelector('[data-camera="${action}"]').getBoundingClientRect();return{x:r.x+r.width/2,y:r.y+r.height/2}})()`);
    await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[point]});await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
    await until(`document.querySelector('gs-vehicle-viewer').follow===${following}`);
   }
  }
  assert.equal(await value('document.querySelector("gs-vehicle-viewer").root.position.y>9000'),true);
  assert.equal(await value('document.querySelector("gs-vehicle-viewer").target.distanceTo(document.querySelector("gs-vehicle-viewer").root.position)<.001'),true);
  await until('document.querySelector("gs-vehicle-viewer").dirty===false');
  const before=await value('document.querySelector("gs-vehicle-viewer").renderer.info.render.frame');await sleep(250);assert.equal(await value('document.querySelector("gs-vehicle-viewer").renderer.info.render.frame'),before,'paused programme does not redraw unchanged savedscene');
  if(process.env.GS_REPLAY_SCREENSHOT_DIR){
   fs.mkdirSync(process.env.GS_REPLAY_SCREENSHOT_DIR,{recursive:true});
   await value(`(()=>{const label=document.createElement('div');label.id='synthetic-proof';label.textContent='SYNTHETIC RECORDED TEST DATA · APOGEE 10,000 m · NO LIVE DATA';Object.assign(label.style,{position:'absolute',top:innerWidth<=440?'152px':'110px',left:'12px',right:'12px',maxWidth:'38rem',zIndex:'12',padding:'8px 10px',border:'1px solid #dea454',borderRadius:'8px',background:'#172535ed',color:'#ffce8a',font:'600 12px system-ui',pointerEvents:'none'});document.body.appendChild(label);document.querySelector('gs-vehicle-viewer').cameraAction('follow');})()`);
   await sleep(150);
   const sceneState='(()=>{const m=document.querySelector("gs-vehicle-viewer");return{renderer:m.software?"canvas":"webgl",savedTimestamp:JSON.parse(m.getAttribute("data-state")).replay_timestamp_ms,altitude:JSON.parse(m.getAttribute("data-state")).gps[2],drawnTrailSegments:m.dataset.trailSegments,totalSavedTrailSegments:m.trackTimes?.length,attitudeAvailable:m.dataset.attitudeAvailable,groundVisible:m.ground.visible,followingRocket:m.follow,followTargetMatchesRocket:m.target.distanceTo(m.root.position)<.001,cameraDistance:m.distance,positionMarkerVisible:m.dataset.positionMarkerVisible};})()';
   const followMeta=await value(sceneState);assert.equal(await value('document.querySelector("gs-vehicle-viewer").dataset.positionMarkerVisible'),'false','follow keeps the actual physical-scale model without a position label');assert.equal(followMeta.followingRocket,true);assert.equal(followMeta.followTargetMatchesRocket,true);assert.equal(followMeta.groundVisible,true);
   const follow=await call('Page.captureScreenshot',{format:'png'});fs.writeFileSync(path.join(process.env.GS_REPLAY_SCREENSHOT_DIR,`replay-apogee-follow-${width}.png`),Buffer.from(follow.data,'base64'));
   await value('document.querySelector("gs-vehicle-viewer").cameraAction("overview")');await sleep(150);
   assert.equal(await value('document.querySelector("gs-vehicle-viewer").dataset.positionMarkerVisible'),'true','path overview identifies the physical-scale rocket with a saved-position marker');assert.equal(await value('document.querySelector("gs-vehicle-viewer").root.scale.toArray().join(",")'),'1,1,1','screen-space marker must never inflate rocket geometry');
   const overview=await call('Page.captureScreenshot',{format:'png'});fs.writeFileSync(path.join(process.env.GS_REPLAY_SCREENSHOT_DIR,`replay-apogee-trajectory-${width}.png`),Buffer.from(overview.data,'base64'));
   const overviewMeta=await value(sceneState);
   fs.writeFileSync(path.join(process.env.GS_REPLAY_SCREENSHOT_DIR,`replay-apogee-proof-${width}.json`),JSON.stringify({syntheticRecordedTestData:true,width,follow:followMeta,trajectory:overviewMeta},null,2)+'\n');
   await value('document.querySelector("#synthetic-proof").remove();document.querySelector("gs-vehicle-viewer").cameraAction("reset")');await sleep(100);
  }
  const angle=await value('document.querySelector("gs-vehicle-viewer").theta');await call('Input.dispatchMouseEvent',{type:'mousePressed',x:100,y:200,button:'left',clickCount:1});await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:145,y:220,button:'left',buttons:1});await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:145,y:220,button:'left',clickCount:1});assert.notEqual(await value('document.querySelector("gs-vehicle-viewer").theta'),angle);
  console.log('PASS programme recordedapogee/follow/trail/drag/pause/fit',width);
 }
 const cameraTransform='JSON.stringify((v=>({follow:v.follow,theta:v.theta,phi:v.phi,distance:v.distance,target:v.target.toArray(),position:v.camera.position.toArray()}))(document.querySelector("gs-vehicle-viewer")))';
 await value('(()=>{const v=document.querySelector("gs-vehicle-viewer");v.cameraAction("pan-left");v.cameraAction("zoom-out");v.cameraAction("left");})()');await sleep(150);
 const freeCamera=await value(cameraTransform);
 plan={...plan,upper_id:11,end_ms:12000,follow_saved:true};
 await until('JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-track"))?.upper_id===11&&JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-state")).replay_timestamp_ms===3000');
 assert.equal(await value('document.querySelector("gs-vehicle-viewer").root.position.y>9000'),true,'growing saved snapshot preserves paused recorded pose');
 await sleep(150);assert.equal(await value(cameraTransform),freeCamera,'snapshot growth preserves free orbit, pan, zoom and actual camera transform');
 await value('document.querySelector("gs-vehicle-viewer").cameraAction("follow")');
 plan={...plan,upper_id:12,end_ms:13000,position_ms:3000};
 await until('JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-track"))?.upper_id===12&&JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-state")).replay_timestamp_ms===4000');await sleep(150);
 assert.equal(await value('(()=>{const v=document.querySelector("gs-vehicle-viewer");return v.follow&&v.target.distanceTo(v.root.position)<1e-6})()'),true,'follow continues tracking the newly selected saved rocket pose');
 assert.equal(await value('JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-state")).motions.find(m=>m.node==="main-parachute").value'),1,'FC12Reefing shows main chute');
 console.log('PASS programme growing saved snapshot retains free/follow camera, pan, zoom and playback');
 plan={...plan,position_ms:0,speed:4,anchor_server_ms:now(),paused:false,loop_playback:true};
 await until('JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-state")).replay_timestamp_ms!==3000');
 await sleep(700);const at=await value('JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-state")).replay_timestamp_ms');assert(at>=1000&&at<=11000,'acceleratedloop stayswithinrecordedbounds');
 plan={...plan,recording_id:'groundstation_recording_2.db',position_ms:2000,paused:true};
 await until('JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-state")).gps?.[0]<41');
 assert.equal(await value('JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-track")).recording_id'),plan.recording_id);await sleep(150);assert.equal(await value('document.querySelector("gs-vehicle-viewer").origin[0]'),40,'new recording resets its coordinate frame');
 fail=true;await until('document.querySelector(".time").textContent.includes("unavailable")');const frozen=await value('document.querySelector("gs-vehicle-viewer").getAttribute("data-state")');await sleep(250);assert.equal(await value('document.querySelector("gs-vehicle-viewer").getAttribute("data-state")'),frozen);
 assert(requests.every(r=>r.method==='GET'&&r.path.startsWith('/api/media-assets/recorded-vehicle/')),'programme never sendscommands orrequests livecache');assert.equal(events.filter(e=>e.method==='Network.webSocketCreated').length,0);assert.equal(events.filter(e=>e.method==='Runtime.exceptionThrown').length,0);
 console.log('PASS programme recordedclock/loop/recordingswitch/failurefreeze/no livecache oroutputs');
}finally{ws?.close();chrome.kill('SIGTERM');await new Promise(r=>server.close(r));await Promise.race([new Promise(r=>chrome.once('exit',r)),sleep(3000)]);fs.rmSync(profile,{recursive:true,force:true});}
