// The flight replay engine in a real browser: the nominal 10k flight bundle
// (tests/fixtures/backend-contract/replay-bundle/flight.gsreplay) played by the
// offline player with the bundled Three.js scene, no station and no network.
//
//   PLAYWRIGHT_MODULE=… CHROME_BINARY=… node tests/flight-replay-browser.mjs
//
// GS_FLIGHT_REPLAY_SCREENSHOTS=DIR writes boost / apogee-drogue / main / predicted
// screenshots there. GS_REPLAY_WEBGL=1 requires SwiftShader WebGL; otherwise the
// renderer the browser selects is reported.
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import zlib from 'node:zlib';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'/tmp/rme-browser-profile/node_modules/playwright/index.mjs');
const root=path.resolve(new URL('..',import.meta.url).pathname),three=path.join(root,'station_ui/rendering/three');
const shots=process.env.GS_FLIGHT_REPLAY_SCREENSHOTS||'';if(shots)fs.mkdirSync(shots,{recursive:true});
// Minimal ZIP reader (stored/deflated entries) for the bundle.
function unzip(buffer){
 const out={};let eocd=buffer.length-22;while(eocd>=0&&buffer.readUInt32LE(eocd)!==0x06054b50)eocd--;
 let at=buffer.readUInt32LE(eocd+16);const count=buffer.readUInt16LE(eocd+10);
 for(let i=0;i<count;i++){
  const method=buffer.readUInt16LE(at+10),size=buffer.readUInt32LE(at+20),nameLength=buffer.readUInt16LE(at+28),extra=buffer.readUInt16LE(at+30),comment=buffer.readUInt16LE(at+32),local=buffer.readUInt32LE(at+42),name=buffer.toString('utf8',at+46,at+46+nameLength);
  const start=local+30+buffer.readUInt16LE(local+26)+buffer.readUInt16LE(local+28),data=buffer.subarray(start,start+size);
  out[name]=method===8?zlib.inflateRawSync(data):Buffer.from(data);at+=46+nameLength+extra+comment;
 }
 return out;
}
const files=unzip(fs.readFileSync(path.join(root,'tests/fixtures/backend-contract/replay-bundle/flight.gsreplay')));
const json=name=>JSON.parse(files[name].toString('utf8'));
const replay=json('replay.json');replay.view_config??={stages:[],ground_systems:[]};replay.valve_mappings??=[];replay.vehicle_geometry??=json('models/vehicle_geometry.json');
const extras={manifest:json('manifest.json'),trajectory:json('derived/trajectory.json'),flight_review:json('derived/flight_review.json'),
 stations:{ground_station:{latitude:31.7552,longitude:-102.0501,altitude_m:893},rrls:{latitude:31.7651,longitude:-102.0322}}};
const page=String.raw`<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Flight replay</title>
<style>body{margin:0;padding:12px;background:#0b1220;color:#e2e8f0;font:14px system-ui}button,select{font:inherit}</style><main id="replay"></main>
<script src="/three/offline-replay.js"></script><script type="module">
globalThis.GSLoadReplay3D=async()=>{const [T,visuals,fallback]=await Promise.all([import('/three/three.module.js'),import('/three/replay-scene.js'),import('/three/software-renderer.js')]);return {T,...visuals,SoftwareRenderer:fallback.SoftwareRenderer};};
globalThis.ready=true;
</script>`;
const server=http.createServer((req,res)=>{
 const url=new URL(req.url,'http://x');
 if(url.pathname==='/')return res.writeHead(200,{'content-type':'text/html'}),res.end(page);
 if(url.pathname==='/three/terrain.js')return res.writeHead(404),res.end();
 const file=path.join(three,path.basename(url.pathname));
 if(url.pathname.startsWith('/three/')&&fs.existsSync(file))return res.writeHead(200,{'content-type':'text/javascript'}),res.end(fs.readFileSync(file));
 res.writeHead(404);res.end();
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));const origin='http://127.0.0.1:'+server.address().port;
const args=process.env.GS_REPLAY_WEBGL==='1'?['--use-gl=angle','--use-angle=swiftshader','--ignore-gpu-blocklist','--enable-webgl','--enable-unsafe-swiftshader']:['--use-angle=swiftshader','--enable-unsafe-swiftshader'];
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY,args});
const results={};
try{
 const context=await browser.newContext({viewport:{width:1280,height:1000},deviceScaleFactor:1});
 const tab=await context.newPage(),errors=[],external=[];tab.on('pageerror',e=>errors.push(e.message));tab.on('console',m=>{if(m.type()==='error'&&!/terrain\.js|404/.test(m.text()))errors.push(m.text());});
 tab.on('request',r=>{if(!r.url().startsWith(origin))external.push(r.url());});
 await tab.goto(origin+'/');await tab.waitForFunction(()=>globalThis.ready&&globalThis.GSOfflineReplay);
 const mount=async(doc,ext)=>{await tab.evaluate(([d,e])=>{globalThis.player?.dispose();globalThis.player=GSOfflineReplay.mount(document.getElementById('replay'),d,{extras:e});},[doc,ext]);
  await tab.waitForFunction(()=>player.visual?.canvas?.dataset.rendered==='true'&&document.querySelector('#replay').dataset.loaded==='true',null,{timeout:30000});};
 await mount(replay,extras);
 results.renderer=await tab.evaluate(()=>player.visual.software?'canvas2d':'webgl');
 results.flightScene=await tab.evaluate(()=>document.querySelector('[aria-label="Flight vehicle and named valve states"]').dataset.scene);assert.equal(results.flightScene,'flight');
 // Timeline from the recorded FLIGHT_STATE rows, the palette from the contract.
 const segments=await tab.evaluate(()=>[...document.querySelectorAll('.gs-flight-timeline [data-phase]')].map(n=>[n.dataset.phase,getComputedStyle(n).backgroundColor]));
 assert.deepEqual(segments.map(s=>s[0]),['pad','boost','coast','drogue','landed']);
 assert.equal(segments[1][1],'rgb(249, 115, 22)');assert.equal(segments[3][1],'rgb(56, 189, 248)');
 const events=await tab.evaluate(()=>[...document.querySelectorAll('.gs-flight-timeline [data-event]')].map(n=>n.dataset.event));
 assert.deepEqual(events,['ignition','liftoff','burnout','apogee','drogue','landing']);
 results.timeline={segments:segments.map(s=>s[0]),events};
 // Jump to boost: plume lit, attitude from the velocity, HUD values.
 const snap=async name=>{if(shots)await tab.screenshot({path:path.join(shots,name+'.png'),fullPage:true});};
 const at=async seconds=>{await tab.evaluate(s=>player.seek(player.model.timeline.t0+s*1000),seconds);await tab.waitForFunction(()=>player.visual.canvas.dataset.rendered==='true');await tab.waitForTimeout(150);};
 await at(3);
 const boost=await tab.evaluate(()=>({phase:document.querySelector('#replay').dataset.phase,plume:Number(document.querySelector('#replay').dataset.plume),attitude:player.visual.canvas.dataset.attitudeSource,flame:player.visual.vehicle.plume.visible,tclock:document.querySelector('.gs-flight-hud').children[0].textContent,hud:document.querySelector('.gs-flight-hud').innerText}));
 assert.equal(boost.phase,'boost');assert.ok(boost.plume>0&&boost.flame,'plume during boost');assert.equal(boost.attitude,'velocity');assert.equal(boost.tclock,'T+ 00:03');assert.match(boost.hud,/Altitude\s+\d/);assert.match(boost.hud,/Chamber pressure\s+—/,'missing value shows a dash');
 results.boost=boost;await snap('flight-replay-boost');
 // Apogee then the drogue: chute out at its real size, the airframe parted at the joint.
 await tab.evaluate(()=>player.jump('apogee'));await tab.waitForTimeout(150);
 assert.equal(await tab.evaluate(()=>document.querySelector('#replay').dataset.tclock),'T+ 00:26');
 await tab.evaluate(()=>player.jump('drogue'));await tab.evaluate(()=>player.seek(player.clock.t+1500));await tab.waitForTimeout(200);
 const drogue=await tab.evaluate(()=>{const v=player.visual.vehicle,c=v.chutes.drogue;return {phase:document.querySelector('#replay').dataset.phase,visible:c.visible,main:v.chutes.main.visible,separated:v.fore.position.y>0,plume:v.plume.visible,diameter:c.userData.size.diameter_m,canopy:c.getObjectByName('drogue-canopy').geometry.parameters.radius};});
 assert.equal(drogue.phase,'drogue');assert.ok(drogue.visible&&!drogue.main&&drogue.separated&&!drogue.plume);assert.equal(drogue.diameter,replay.vehicle_geometry.recovery.find(r=>r.role==='drogue').diameter_m);
 assert.ok(Math.abs(drogue.canopy*2-drogue.diameter)<1e-6,'canopy drawn at the geometry diameter');
 results.drogue=drogue;await tab.evaluate(()=>player.setCameraMode('chase'));await tab.waitForTimeout(150);await snap('flight-replay-apogee-drogue');
 // Camera modes.
 const modes={};for(const mode of ['follow','chase','pad','station','orbit','top']){await tab.evaluate(m=>player.setCameraMode(m),mode);await tab.waitForTimeout(80);modes[mode]=await tab.evaluate(()=>player.visual.canvas.dataset.cameraMode);}
 assert.deepEqual(modes,{follow:'follow',chase:'chase',pad:'pad',station:'station',orbit:'orbit',top:'top'});results.cameraModes=modes;
 assert.equal(await tab.evaluate(()=>document.querySelectorAll('[data-camera]').length),6);
 // Time controls: step frame and event, A/B loop, slow motion.
 const t0=await tab.evaluate(()=>player.clock.t);await tab.evaluate(()=>player.stepFrame(1));assert.ok(await tab.evaluate(t=>player.clock.t>t,t0));
 assert.equal(await tab.evaluate(()=>player.stepEvent(1)?.kind),'landing');
 await tab.evaluate(()=>{player.seek(player.model.timeline.t0);player.setLoop('a');player.seek(player.model.timeline.t0+2000);player.setLoop('b');player.seek(player.model.timeline.t0+1900);player.setSpeed(4);player.play();});
 await tab.waitForTimeout(600);const looped=await tab.evaluate(()=>{const t=player.clock.t-player.model.timeline.t0;player.pause();return t;});assert.ok(looped>=0&&looped<=2000,'A/B loop keeps the cursor inside the segment: '+looped);
 assert.equal(await tab.evaluate(()=>document.querySelectorAll('[data-loop]').length),1);
 await tab.evaluate(()=>player.clearLoop());
 // The main: the fixture records no main deployment (no Reefing row; its flight
 // review has main null), so a copy adds one where the descent rate changes.
 const withMain=structuredClone(replay),reefAt=replay.samples.find(s=>s.data_type==='FLIGHT_STATE'&&s.phase==='Launch').timestamp_ms+134000;
 withMain.samples.push({id:900001,timestamp_ms:reefAt,source_timestamp_ms:null,sender_id:'FC',data_type:'FLIGHT_STATE',values:[12],phase:'Reefing'});withMain.recording='Nominal 10k flight + test Reefing row';
 await mount(withMain,extras);await at(150);
 const main=await tab.evaluate(()=>{const v=player.visual.vehicle;return {phase:document.querySelector('#replay').dataset.phase,main:v.chutes.main.visible,drogue:v.chutes.drogue.visible,diameter:v.chutes.main.userData.size.diameter_m};});
 assert.equal(main.phase,'main');assert.ok(main.main&&main.drogue);assert.equal(main.diameter,replay.vehicle_geometry.recovery.find(r=>r.role==='main').diameter_m);
 results.main=main;await tab.evaluate(()=>player.setCameraMode('follow'));await tab.waitForTimeout(150);await snap('flight-replay-main');
 // A flight plan played like a recording, and a plan overlaid on the actual flight.
 const launch=reefAt-134000,ring=Array.from({length:36},(_,i)=>{const a=i/36*Math.PI*2;return [31.7584+0.004*Math.cos(a),-102.0355+0.006*Math.sin(a)];});
 const overlay={id:'flight_plan:fp-test',kind:'flight_plan',label:'Predicted: SWEET V6 8.5 kg',predicted:true,tracks:[{id:'boost',phase:'boost',style:'predicted',points:[[31.758015,-102.045972,891],[31.7590,-102.0455,2500],[31.7612,-102.0446,3950]]},{id:'drogue',phase:'drogue',style:'predicted',points:[[31.7612,-102.0446,3950],[31.7600,-102.0400,1400],[31.7586,-102.0360,892]]}],markers:[{id:'landing',kind:'landing',label:'Predicted landing',lat_deg:31.7586,lon_deg:-102.036,alt_msl_m:892},{id:'apogee',kind:'apogee',label:'Apogee 3060 m',lat_deg:31.7612,lon_deg:-102.0446,alt_msl_m:3950}],areas:[{id:'landing_ellipse',kind:'landing_ellipse',label:'95% landing area',polygon:ring}]};
 await mount(replay,{...extras,overlays:[overlay]});await at(60);
 const overlays=await tab.evaluate(()=>{const g=player.visual.overlayGroup;const tracks=g.children.filter(n=>n.name==='OverlayTrack');return {tracks:tracks.length,dashed:tracks.every(n=>n.material.isLineDashedMaterial),opacity:tracks[0].material.opacity,ellipse:g.children.some(n=>n.name==='OverlayAreaFill'&&n.userData.kind==='landing_ellipse')};});
 assert.deepEqual(overlays,{tracks:2,dashed:true,opacity:.55,ellipse:true});results.overlay=overlays;
 await tab.evaluate(()=>player.setCameraMode('top'));await tab.evaluate(()=>player.visual.zoom(18));await tab.waitForTimeout(200);await snap('flight-replay-plan-overlay');
 const plan=structuredClone(replay);plan.predicted=true;plan.label='Predicted: SWEET V6 8.5 kg';plan.recording='fp-test';
 await mount(plan,{flight_plan:{format:'gs26.flight_plan',predicted:true,label:'SWEET V6 8.5 kg',created_ms:launch,events:[{kind:'ignition',t_s:0},{kind:'apogee',t_s:26.4}],overlay}});
 const predicted=await tab.evaluate(()=>({badge:document.querySelector('h3 [data-predicted]').textContent,label:document.querySelector('[data-predicted-label]').textContent,visible:getComputedStyle(document.querySelector('[data-predicted-label]')).display!=='none',stamp:document.querySelector('output').textContent}));
 assert.equal(predicted.badge,'PREDICTED');assert.equal(predicted.label,'Predicted: SWEET V6 8.5 kg');assert.ok(predicted.visible);assert.match(predicted.stamp,/PREDICTED/);results.predicted=predicted;
 await at(12);await snap('flight-replay-predicted');
 // Phone width: no horizontal page scroll.
 await tab.setViewportSize({width:390,height:844});await tab.waitForTimeout(250);
 const phone=await tab.evaluate(()=>({doc:document.documentElement.scrollWidth,view:innerWidth}));assert.ok(phone.doc<=phone.view,'phone width fits: '+JSON.stringify(phone));results.phone=phone;await snap('flight-replay-phone');
 assert.deepEqual(errors,[]);assert.deepEqual(external,[]);
 await context.close();
 // Without WebGL the software renderer draws the same scene with fewer effects.
 const soft=await browser.newContext({viewport:{width:900,height:900}});
 await soft.addInitScript(()=>{const original=HTMLCanvasElement.prototype.getContext;HTMLCanvasElement.prototype.getContext=function(kind,...a){return String(kind).startsWith('webgl')?null:original.call(this,kind,...a);};});
 const softTab=await soft.newPage(),softErrors=[];softTab.on('pageerror',e=>softErrors.push(e.message));await softTab.goto(origin+'/');await softTab.waitForFunction(()=>globalThis.ready);
 await softTab.evaluate(([d,e])=>{globalThis.player=GSOfflineReplay.mount(document.getElementById('replay'),d,{extras:e});},[replay,extras]);
 await softTab.waitForFunction(()=>player.visual?.software&&player.visual.canvas.dataset.rendered==='true',null,{timeout:30000});
 results.software={quality:await softTab.evaluate(()=>player.visual.quality),glow:await softTab.evaluate(()=>!!player.visual.vehicle.plume.getObjectByName('plume-glow'))};
 assert.deepEqual(results.software,{quality:'low',glow:false});assert.deepEqual(softErrors,[]);await soft.close();
 console.log(JSON.stringify({state:'PASS',...results},null,1));
}finally{await browser.close();server.close();}
