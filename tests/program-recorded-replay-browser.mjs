// Actual programme player, renderer and bundled rocket; fake read-only recordings.
import fs from 'node:fs';import path from 'node:path';import http from 'node:http';import os from 'node:os';import assert from 'node:assert/strict';import {spawn} from 'node:child_process';
const rendering=path.resolve('station_ui/rendering'),html=fs.readFileSync('station_ui/pages/recorded-vehicle.html');
const requests=[],started=Date.now();let fail=false;
const now=()=>50000+Date.now()-started;
let plan={recording_id:'groundstation_recording_1.db',upper_id:10,start_ms:1000,end_ms:11000,position_ms:2000,anchor_server_ms:now(),speed:1,paused:true,loop_playback:false};
const config={model_url:'/assets/models/vehicle.glb',attitude:{roll:{data_type:'ROLL',sender_id:'FC',index:0}},motions:[],phase_animations:{}};
const points=()=>{const base=plan.recording_id.includes('_2.')?40:43;return [0,1,2,3,4].map((v)=>({timestamp_ms:1000+v*1000,latitude:base+v*.001,longitude:-78,altitude_m:[300,4000,10000,8000,1000][v],break_before:v===0}));};
const server=http.createServer((req,res)=>{const u=new URL(req.url,'http://localhost'),p=u.pathname;
 if(p.startsWith('/api/')){requests.push({method:req.method,path:p});if(fail||u.searchParams.get('ticket')!=='audience-fixture'){res.statusCode=401;res.end('unavailable');return;}
  res.setHeader('Content-Type','application/json');
  if(p.endsWith('/state')){res.end(JSON.stringify({plan,config,server_now_ms:now()}));return;}
  if(u.searchParams.get('recording_id')!==plan.recording_id){res.statusCode=409;res.end('{}');return;}
  if(p.endsWith('/trajectory')){res.end(JSON.stringify({recording_id:plan.recording_id,upper_id:10,source_sender_id:'RF',points:points(),truncated:false,sampled:false}));return;}
  const rows=points().map((v,i)=>({id:i+1,timestamp_ms:v.timestamp_ms,sender_id:'RF',data_type:'GPS_DATA',values:[v.latitude,v.longitude,v.altitude_m]}));rows.push({id:8,timestamp_ms:1000,sender_id:'FC',data_type:'ROLL',values:[15]},{id:9,timestamp_ms:1000,sender_id:'FC',data_type:'FLIGHT_STATE',values:[1],phase:'Ascent'});
  res.end(JSON.stringify({recording_id:plan.recording_id,upper_id:10,next_id:10,has_more:false,rows,seeds:[]}));return;
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
  await call('Page.navigate',{url:`http://127.0.0.1:${server.address().port}/?ticket=audience-fixture`});
  await until('document.querySelector("gs-vehicle-viewer")?.dataset.loaded==="true"&&document.querySelector("gs-vehicle-viewer")?.dataset.trailSegments==="2"');
  assert.equal(await value('innerWidth'),width);assert.equal(await value('document.documentElement.scrollWidth<=innerWidth'),true);
  assert.equal(await value('document.body.innerText.includes("RECORDED FLIGHT · REPLAY")'),true);
  assert.equal(await value('document.querySelector("gs-vehicle-viewer").root.position.y>9000'),true);
  assert.equal(await value('document.querySelector("gs-vehicle-viewer").target.distanceTo(document.querySelector("gs-vehicle-viewer").root.position)<.001'),true);
  const before=await value('document.querySelector("gs-vehicle-viewer").renderer.info.render.frame');await sleep(250);assert.equal(await value('document.querySelector("gs-vehicle-viewer").renderer.info.render.frame'),before,'paused programme does not redraw unchanged savedscene');
  const angle=await value('document.querySelector("gs-vehicle-viewer").theta');await call('Input.dispatchMouseEvent',{type:'mousePressed',x:100,y:200,button:'left',clickCount:1});await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:145,y:220,button:'left',buttons:1});await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:145,y:220,button:'left',clickCount:1});assert.notEqual(await value('document.querySelector("gs-vehicle-viewer").theta'),angle);
  console.log('PASS programme recordedapogee/follow/trail/drag/pause/fit',width);
 }
 plan={...plan,position_ms:0,speed:4,anchor_server_ms:now(),paused:false,loop_playback:true};
 await until('JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-state")).replay_timestamp_ms!==3000');
 await sleep(700);const at=await value('JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-state")).replay_timestamp_ms');assert(at>=1000&&at<=11000,'acceleratedloop stayswithinrecordedbounds');
 plan={...plan,recording_id:'groundstation_recording_2.db',position_ms:2000,paused:true};
 await until('JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-state")).gps?.[0]<41');
 assert.equal(await value('JSON.parse(document.querySelector("gs-vehicle-viewer").getAttribute("data-track")).recording_id'),plan.recording_id);
 fail=true;await until('document.querySelector(".time").textContent.includes("unavailable")');const frozen=await value('document.querySelector("gs-vehicle-viewer").getAttribute("data-state")');await sleep(250);assert.equal(await value('document.querySelector("gs-vehicle-viewer").getAttribute("data-state")'),frozen);
 assert(requests.every(r=>r.method==='GET'&&r.path.startsWith('/api/media-assets/recorded-vehicle/')),'programme never sendscommands orrequests livecache');assert.equal(events.filter(e=>e.method==='Network.webSocketCreated').length,0);assert.equal(events.filter(e=>e.method==='Runtime.exceptionThrown').length,0);
 console.log('PASS programme recordedclock/loop/recordingswitch/failurefreeze/no livecache oroutputs');
}finally{ws?.close();chrome.kill('SIGTERM');await new Promise(r=>server.close(r));await Promise.race([new Promise(r=>chrome.once('exit',r)),sleep(3000)]);fs.rmSync(profile,{recursive:true,force:true});}
