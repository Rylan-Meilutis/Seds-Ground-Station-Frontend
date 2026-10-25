// Runs actual bundled renderer/model without a Rust build or Playwright dependency.
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';
import os from 'node:os';
const source=path.resolve('station_ui/rendering');
const html=String.raw`<!doctype html><meta name="viewport" content="width=device-width,initial-scale=1"><style>body{margin:0;background:#101722;color:#eee;font:14px system-ui}gs-vehicle-viewer{height:80vh;width:100%;position:relative;display:block}#result{padding:12px;box-sizing:border-box;white-space:pre-wrap;overflow-wrap:anywhere}</style><gs-vehicle-viewer src="/vehicle.glb" aria-label="Saved flight scene"></gs-vehicle-viewer><pre id="result">Loading saved scene fixture…</pre><script type="module">
const checks={},finish=()=>{document.querySelector('#result').textContent=JSON.stringify(checks,null,2);document.body.dataset.pass=String(Object.values(checks).every(Boolean));document.body.dataset.viewport=String(innerWidth);};
try {
 await import('/vehicle-renderer.js');
 const m=document.querySelector('gs-vehicle-viewer');
 m.setAttribute('data-track',JSON.stringify({upper_id:42,truncated:false,points:[{timestamp_ms:0,latitude:45,longitude:-73,altitude_m:300},{timestamp_ms:1000,latitude:45.002,longitude:-72.999,altitude_m:4000},{timestamp_ms:2000,latitude:45.004,longitude:-72.998,altitude_m:10000},{timestamp_ms:3000,latitude:45.006,longitude:-72.997,altitude_m:8000}]}));
 m.setAttribute('data-state',JSON.stringify({replay_time_seconds:2,attitude:[15,20,5],gps:[45.004,-72.998,10000],motions:[]}));
 await new Promise((resolve,reject)=>{const timer=setInterval(()=>{if(m.dataset.loaded==='true'&&m.ground&&m.lastFrame){clearInterval(timer);resolve();}},40);setTimeout(()=>{clearInterval(timer);reject(Error(m.status?.textContent||'Scene timed out'));},12000);});
 checks.sceneLoaded=!!m.model&&!!m.ground&&!!m.trajectory;
 checks.highAltitudePose=m.root.position.y>9000;
 checks.recordedPath=m.trajectory.geometry.attributes.position.count===6;checks.progressiveTrail=m.trajectory.geometry.drawRange.count===4;
 checks.followTarget=m.target.distanceTo(m.root.position)<1e-6;
 m.setAttribute('data-state',JSON.stringify({replay_time_seconds:1,replay_timestamp_ms:1000,attitude:[15,20,5],gps:[45.002,-72.999,4000],motions:[]}));await new Promise(r=>setTimeout(r,100));checks.ascentFollow=m.target.distanceTo(m.root.position)<1e-6&&m.root.position.y>3000&&m.trajectory.geometry.drawRange.count===2;
 m.setAttribute('data-state',JSON.stringify({replay_time_seconds:2,replay_timestamp_ms:2000,attitude:[15,20,5],gps:[45.004,-72.998,10000],motions:[]}));await new Promise(r=>setTimeout(r,100));
 m.cameraAction('pan-left');checks.panDetachesFollow=!m.follow&&m.target.distanceTo(m.root.position)>0;
 m.cameraAction('follow');checks.followRestoresTarget=m.follow&&m.target.distanceTo(m.root.position)<1e-6;
 const distance=m.distance;m.cameraAction('zoom-out');checks.zoom=m.distance>distance;
 m.cameraAction('rise');checks.verticalMotion=!m.follow&&m.target.y>m.root.position.y;
 m.cameraAction('overview');checks.overview=m.distance>9000&&!m.follow;
 m.cameraAction('reset');checks.reset=m.follow&&m.distance===m.modelDistance;
 m.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true,cancelable:true}));checks.keyboard=Math.abs(m.theta-.45)<1e-6;
 m.setAttribute('data-state',JSON.stringify({replay_time_seconds:2,attitude:[15,20,5],gps:null,motions:[]}));await new Promise(r=>setTimeout(r,150));checks.referenceAttitudeWithoutGps=m.root.visible&&!m.ground.visible&&!m.trajectory.visible&&m.root.position.length()===0&&m.dataset.positionAvailable==='false'&&m.status.textContent.includes('Reference attitude mode');
 m.setAttribute('data-state',JSON.stringify({replay_time_seconds:2,attitude:[15,20,5],gps:[45.004,-72.998,10000],motions:[]}));await new Promise(r=>setTimeout(r,150));checks.restoredPosition=m.root.visible;
 m.renderer.render(m.scene,m.camera);const canvas=m.renderer.domElement,gl=m.renderer.getContext?.();let pixels;
 if(gl){pixels=new Uint8Array(canvas.width*canvas.height*4);gl.readPixels(0,0,canvas.width,canvas.height,gl.RGBA,gl.UNSIGNED_BYTE,pixels);}else pixels=m.renderer.context.getImageData(0,0,canvas.width,canvas.height).data;
 let groundPixels=0,skyPixels=0;for(let i=0;i<pixels.length;i+=4){const r=pixels[i],g=pixels[i+1],b=pixels[i+2];if(g>r+5&&g>b+3)groundPixels++;if(b>g+10&&g>r+10)skyPixels++;}
 checks.visibleGroundAndSky=groundPixels>20&&skyPixels>20;
 const savedTrack=m.getAttribute('data-track');
 m.setAttribute('data-track','null');m.setAttribute('data-state',JSON.stringify({replay_time_seconds:2,attitude:[15,20,5],gps:null,motions:[]}));await new Promise(r=>setTimeout(r,150));
 checks.attitudeWithoutAnyGpsTrack=m.root.visible&&!m.origin&&!m.ground&&m.root.position.length()===0&&Math.abs(m.root.rotation.x-15*Math.PI/180)<1e-6&&m.status.textContent.includes('Reference attitude mode');
 m.setAttribute('data-track',savedTrack);m.setAttribute('data-state',JSON.stringify({replay_time_seconds:2,attitude:[15,20,5],gps:[45.004,-72.998,10000],motions:[]}));await new Promise(r=>setTimeout(r,150));
 checks.noOverflow=document.documentElement.scrollWidth<=innerWidth;document.body.dataset.renderer=m.software?'canvas':'webgl';
 finish();
}catch(error){checks.error=error.message;document.body.dataset.pass='false';document.querySelector('#result').textContent=JSON.stringify(checks);}
</script>
`;
const server=http.createServer((req,res)=>{
 const pathname=new URL(req.url,'http://localhost').pathname;
 if(pathname==='/'){res.setHeader('Content-Type','text/html; charset=utf-8');res.end(html);return;}
 const file=pathname==='/vehicle.glb'?path.join(source,'models/vehicle.glb'):path.resolve(source,'three',pathname.slice(1));
 if(!file.startsWith(source+path.sep)||!fs.existsSync(file)){res.statusCode=404;res.end();return;}
 res.setHeader('Content-Type',file.endsWith('.js')?'text/javascript':'model/gltf-binary');res.end(fs.readFileSync(file));
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const profile=fs.mkdtempSync(path.join(os.tmpdir(),'gs-replay-scene-')); 
const chrome=spawn(process.env.CHROME_BINARY||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',['--headless','--no-sandbox','--no-first-run','--disable-background-networking','--disable-component-update','--disable-sync','--disable-extensions',...(process.env.GS_REPLAY_WEBGL==='1'?['--use-gl=angle','--use-angle=swiftshader','--ignore-gpu-blocklist','--enable-webgl','--enable-unsafe-swiftshader']:[]),'--remote-debugging-port=0','--remote-allow-origins=*','--user-data-dir='+profile,'about:blank'],{stdio:['ignore','ignore','pipe']});
let stderr='';chrome.stderr.on('data',d=>stderr=(stderr+d).slice(-65536));let ws;let next=0;const pending=new Map(),events=[];
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
try{
 for(let i=0;i<100&&!fs.existsSync(profile+'/DevToolsActivePort');i++)await sleep(100);
 if(!fs.existsSync(profile+'/DevToolsActivePort'))throw Error('Chrome did not start: '+stderr.slice(-1000));
 const port=fs.readFileSync(profile+'/DevToolsActivePort','utf8').split('\n')[0];
 const targets=await(await fetch(`http://127.0.0.1:${port}/json/list`)).json();
 ws=new WebSocket(targets.find(t=>t.type==='page').webSocketDebuggerUrl);await new Promise((r,j)=>{ws.onopen=r;ws.onerror=j;});
 ws.onmessage=e=>{const m=JSON.parse(e.data);if(m.id){const {resolve,reject}=pending.get(m.id)||{};pending.delete(m.id);m.error?reject?.(Error(JSON.stringify(m.error))):resolve?.(m.result);}else events.push(m);};
 const call=(method,params={})=>new Promise((resolve,reject)=>{const id=++next;pending.set(id,{resolve,reject});ws.send(JSON.stringify({id,method,params}));});
 await call('Runtime.enable');await call('Page.enable');
 for(const width of [1280,390,320]){
  await call('Emulation.setDeviceMetricsOverride',{width,height:900,deviceScaleFactor:1,mobile:width<500});
  await call('Page.navigate',{url:`http://127.0.0.1:${server.address().port}/`});
  let result;
  for(let i=0;i<160;i++){await sleep(100);result=await call('Runtime.evaluate',{expression:'JSON.stringify({pass:document.body?.dataset.pass,width:innerWidth,renderer:document.body?.dataset.renderer,result:document.querySelector("#result")?.textContent})',returnByValue:true});if(JSON.parse(result.result.value).pass!==undefined)break;}
  const status=JSON.parse(result.result.value);console.log(JSON.stringify(status,null,2));
  if(process.env.GS_REPLAY_SCREENSHOT_DIR){const screenshot=await call('Page.captureScreenshot',{format:'png'});fs.writeFileSync(path.join(process.env.GS_REPLAY_SCREENSHOT_DIR,`replay-scene-cdp-${width}.png`),Buffer.from(screenshot.data,'base64'));}
  if(status.pass!=='true'){console.log('Runtime errors:',JSON.stringify(events.filter(e=>e.method==='Runtime.exceptionThrown')));throw Error('Scene failed');}
  assert.equal(status.width,width,'actual viewport width');if(process.env.GS_REPLAY_WEBGL==='1')assert.equal(status.renderer,'webgl','Explicit SwiftShader run must exercise WebGL');
  const value=expression=>call('Runtime.evaluate',{expression,returnByValue:true}).then(r=>r.result.value);
  const before=await value('document.querySelector("gs-vehicle-viewer").theta');
  await call('Input.dispatchMouseEvent',{type:'mousePressed',x:100,y:200,button:'left',clickCount:1});
  await call('Input.dispatchMouseEvent',{type:'mouseMoved',x:145,y:220,button:'left',buttons:1});
  await call('Input.dispatchMouseEvent',{type:'mouseReleased',x:145,y:220,button:'left',clickCount:1});
  assert.notEqual(await value('document.querySelector("gs-vehicle-viewer").theta'),before,'real pointer drag orbits recordedrocket');
  if(width<500){
   const distance=await value('document.querySelector("gs-vehicle-viewer").distance');
   await call('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:100,y:220},{x:220,y:220}]});
   await call('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{x:80,y:230},{x:240,y:230}]});
   await call('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
   assert.notEqual(await value('document.querySelector("gs-vehicle-viewer").distance'),distance,'twofingerpinchchangesview');
  }

 }
 console.log('PASS isolated actual renderer at desktop/mobile widths');
}catch(error){console.log('Chrome log:',stderr.slice(-2000));throw error;}
finally{ws?.close();chrome.kill('SIGTERM');await new Promise(r=>server.close(r));await Promise.race([new Promise(r=>chrome.once('exit',r)),sleep(3000)]);fs.rmSync(profile,{recursive:true,force:true});}
