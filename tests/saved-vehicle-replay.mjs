// Build with `dx build --platform web`, then run `node tests/saved-vehicle-replay.mjs`.
// Requires Playwright/Chromium; GS_REPLAY_RENDERING_ROOT can select external 3D assets.
// This fixture never connects to a station or dispatches hardware commands.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root=path.resolve(process.env.GS_FRONTEND_PUBLIC || 'target/dx/groundstation_frontend/debug/web/public');
const rendering=path.resolve(process.env.GS_REPLAY_RENDERING_ROOT || 'station_ui/rendering');
const example=name=>JSON.parse(fs.readFileSync(`docs/api-examples/${name}.json`,'utf8'));
const session={...example('auth-session.anonymous'),authenticated:true,anonymous:false,username:'ui-test',roles:['admin'],permissions:{view_data:true,send_commands:true}};
const dashboardLayout=example('layout.full');dashboardLayout.main_tabs.push('vehicle');
let windowRequests=0,failWindow=false;
const server=http.createServer(async(req,res)=>{
  const pathname=new URL(req.url,'http://localhost').pathname;
  if(pathname==='/api/vehicle_visualization') {const cfg=example('vehicle-visualization');cfg.ground_model_url='';cfg.attitude.roll={data_type:'ROLL',sender_id:'FC',index:0,scale:1,offset:0};res.setHeader('Content-Type','application/json');res.end(JSON.stringify(cfg));return;}
  if(pathname==='/api/recordings') {res.setHeader('Content-Type','application/json');res.end(JSON.stringify([{id:'groundstation_recording_1.db',bytes:1000,active:false},{id:'groundstation_recording_2.db',bytes:1000,active:true}]));return;}
  if(pathname.endsWith('/replay/trajectory')) {res.setHeader('Content-Type','application/json');res.end(JSON.stringify({upper_id:10,points:[],truncated:false}));return;}
  if(pathname.endsWith('/replay')) {res.setHeader('Content-Type','application/json');res.end(JSON.stringify({upper_id:10,start_ms:1000,end_ms:41000}));return;}
  if(pathname.endsWith('/replay/window')) {windowRequests++;if(failWindow){res.statusCode=503;res.end('Saved recording temporarily unavailable');return;}const second=pathname.includes('groundstation_recording_2.db');const url=new URL(req.url,'http://localhost');const start=Number(url.searchParams.get('start_ms')),end=Number(url.searchParams.get('end_ms'));const rows=[{id:1,timestamp_ms:1000,source_timestamp_ms:null,sender_id:'FC',data_type:'ROLL',values:[second?77:10]},{id:2,timestamp_ms:1000,source_timestamp_ms:null,sender_id:'FC',data_type:'FLIGHT_STATE',values:[1],phase:'Idle'},{id:3,timestamp_ms:2000,source_timestamp_ms:null,sender_id:'FC',data_type:'ROLL',values:[20]},{id:4,timestamp_ms:2000,source_timestamp_ms:null,sender_id:'FC',data_type:'FLIGHT_STATE',values:[8],phase:'Ascent'},{id:6,timestamp_ms:3000,source_timestamp_ms:null,sender_id:'FC',data_type:'FLIGHT_STATE',values:[11],phase:'Descent'},{id:7,timestamp_ms:4000,source_timestamp_ms:null,sender_id:'FC',data_type:'FLIGHT_STATE',values:[12],phase:'Reefing'},{id:5,timestamp_ms:31000,source_timestamp_ms:null,sender_id:'FC',data_type:'ROLL',values:[30]}];const seeds=[];for(const type of ['ROLL','FLIGHT_STATE']){const row=rows.filter(r=>r.data_type===type&&r.timestamp_ms<start).at(-1);if(row)seeds.push(row);}res.setHeader('Content-Type','application/json');res.end(JSON.stringify({upper_id:10,next_id:10,has_more:false,rows:rows.filter(r=>r.timestamp_ms>=start&&r.timestamp_ms<=end),seeds}));return;}
  if(pathname.startsWith('/assets/three/')||pathname.startsWith('/assets/models/')) {const file=path.resolve(rendering,pathname.slice('/assets/'.length));if(file.startsWith(rendering+path.sep)&&fs.existsSync(file)){res.setHeader('Content-Type',pathname.endsWith('.js')?'text/javascript':'model/gltf-binary');res.end(fs.readFileSync(file));return;}}
  if(pathname.startsWith('/api/')){
    res.setHeader('Content-Type','application/json');
    const data=pathname==='/api/auth/session'?session:pathname==='/api/layout'?dashboardLayout:[];
    res.end(JSON.stringify(data));return;
  }
  const relative=pathname==='/'||pathname==='/dashboard'?'index.html':pathname.slice(1);
  const file=path.resolve(root,relative);
  if(!file.startsWith(root+path.sep)||!fs.existsSync(file)||!fs.statSync(file).isFile()){res.statusCode=404;res.end();return;}
  const mime={'.html':'text/html','.js':'text/javascript','.wasm':'application/wasm','.css':'text/css','.svg':'image/svg+xml','.png':'image/png'};
  res.setHeader('Content-Type',mime[path.extname(file)]||'application/octet-stream');res.end(fs.readFileSync(file));
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin=`http://127.0.0.1:${server.address().port}`;

const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{
  if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};
  localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));
 },{session,origin});
 const sockets=new Set();
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 await context.routeWebSocket(url=>url.pathname==='/ws',s=>{sockets.add(s);s.onClose(()=>sockets.delete(s));s.onMessage(()=>{});});
 const page=await context.newPage();page.on('response',r=>{if(r.url().includes('/assets/')&&!r.ok())console.error('asset',r.status(),r.url())});const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(origin+'/dashboard');


 await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 await page.getByRole('link',{name:'3D replay',exact:true}).click();
 const select=page.locator('#replay-recording');await select.waitFor();
 assert(await page.getByRole('button',{name:'Play',exact:true}).isDisabled());
 await select.selectOption('groundstation_recording_1.db');
 const model=page.locator('gs-vehicle-viewer');
 await page.waitForFunction(()=>document.querySelector('gs-vehicle-viewer')?.dataset.loaded==='true');
 await page.getByText('Roll 10.0°',{exact:false}).waitFor();
 await page.waitForFunction(()=>JSON.parse(document.querySelector('gs-vehicle-viewer').getAttribute('data-state')).attitude[2]===10);
 // The API fixture carries saved values only. Inject a contradictory live packet: it must never rotate the saved replay.
 for(const socket of sockets)socket.send(JSON.stringify({data_type:'ROLL',sender_id:'FC',values:[999],timestamp:Date.now()}));
 await page.waitForTimeout(100);
 assert.equal(await model.evaluate(e=>JSON.parse(e.getAttribute('data-state')).attitude[2]),10);
 const slider=page.getByRole('slider',{name:'Saved replay position'});
 const before=windowRequests;
 await slider.evaluate(e=>{for(let i=0;i<30;i++){e.value=String(i*1000);e.dispatchEvent(new Event('input',{bubbles:true}));}});
 await page.waitForTimeout(200);assert.equal(windowRequests,before,'drag does not request intermediate windows');
 await slider.evaluate(e=>{e.value='30000';e.dispatchEvent(new Event('change',{bubbles:true}));});
 await page.getByText('Roll 30.0°',{exact:false}).waitFor();
 assert(windowRequests<=before+2,'seek loads one bounded window');
 await page.getByRole('button',{name:'Rewind',exact:true}).click();await page.getByText('Roll 10.0°',{exact:false}).waitFor();
 await page.locator('#replay-speed').selectOption('4');
 await page.getByRole('button',{name:'Play',exact:true}).click();await page.getByRole('button',{name:'Pause',exact:true}).waitFor();
 await page.waitForTimeout(350);await page.getByRole('button',{name:'Pause',exact:true}).click();
 await page.getByText('Roll 20.0°',{exact:false}).waitFor();
 assert((await model.evaluate(m=>m.mixer.time))>=1,'artwork follows accelerated replay time');
 await select.selectOption('groundstation_recording_2.db');await page.getByText('Roll 77.0°',{exact:false}).waitFor();
 await page.waitForFunction(()=>{const m=document.querySelector('gs-vehicle-viewer');return m?.dataset.loaded==='true'&&m.model&&m.renderer?.domElement?.width>0&&m.lastFrame>0;});
 await page.waitForTimeout(500);
 const paused=await model.evaluate(m=>m.mixer.time);await page.waitForTimeout(250);assert.equal(await model.evaluate(m=>m.mixer.time),paused,'paused saved animations stay paused');
 assert.equal(await model.evaluate(m=>m.mixer.time),0,'record switch rewinds artwork');
 const rendered=await model.evaluate(m=>m.renderer.info.render.frame);await page.waitForTimeout(250);assert.equal(await model.evaluate(m=>m.renderer.info.render.frame),rendered,'paused replay does not keep rendering identical frames');
 assert.deepEqual(await model.evaluate(m=>m.model.position.toArray()),[0,0,0],'centering uses a separate pivot and preserves animation root transforms');

 // Canonical FC wire11/12 phases must drive the matching saved chute artwork.
 for(const [position,phase,node] of [[2000,'Descent','drogue-parachute'],[3000,'Reefing','main-parachute']]){
  await slider.evaluate((e,at)=>{e.value=String(at);e.dispatchEvent(new Event('change',{bubbles:true}));},position);
  await page.waitForFunction(({phase,node})=>{const v=JSON.parse(document.querySelector('gs-vehicle-viewer').getAttribute('data-state'));return v.motions.find(m=>m.node===node)?.value===1&&v.motions.find(m=>m.node===(phase==='Descent'?'main-parachute':'drogue-parachute'))?.value===0;},{phase,node});
 }
 console.log('PASS saved canonical FC11Descent/12Reefing chute artwork');
 console.log('renderer',await model.evaluate(m=>({software:m.software,loaded:m.dataset.loaded,canvas:[m.renderer.domElement.width,m.renderer.domElement.height],bounds:[m.clientWidth,m.clientHeight],model:!!m.model,draws:m.renderer.info?.render,status:m.status?.textContent,visible:m.root.visible,children:m.model?.children.length})));

 for(const width of [1440,390]){await page.setViewportSize({width,height:900});assert.equal(await page.locator('.gs26-vehicle-shell').evaluate(e=>e.scrollWidth>e.clientWidth),false);await page.waitForTimeout(150);assert(await model.evaluate(m=>{return m.center.length()<1e-6;}),'rotation pivot is centered');await page.screenshot({path:path.join(process.env.GS_REPLAY_SCREENSHOT_DIR || "/tmp",`saved-replay-${width}.png`)});}
 failWindow=true;await page.getByRole('button',{name:'Reload',exact:true}).click();await page.getByRole('button',{name:'Details',exact:true}).click();await page.getByRole('dialog',{name:'Replay details'}).waitFor();
 assert.deepEqual(errors,[]);console.log('PASS saved replay selection, bounds, live isolation, seek request bound, speed, record switch, desktop/mobile and failure details');
 await context.close();
}finally{await browser.close();await new Promise(r=>server.close(r));}
