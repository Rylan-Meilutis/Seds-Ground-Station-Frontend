// Real Wasm, synthetic APIs and sockets only. Never connects to hardware.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/operation-workflow.mjs','utf8');
const boundary=source.indexOf('\ntry {\n const context=');
if(boundary<0)throw Error('Operation fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
fixture=fixture.replace("const dashboardLayout=example('layout.full');",`const dashboardLayout=JSON.parse(fs.readFileSync(process.env.GS_CHAMBER_LAYOUT,'utf8'));const stamp=Date.now();const pressureRows=[];
for(let i=0;i<30;i++)for(const [data_type,sender_id,value] of [['KG1000','DAQ',4.25],['LOADCELL_WEIGHT_KG','DAQ',120],['KG50','DAQ',2],['LOADCELL_50_WEIGHT_KG','DAQ',10],['FUEL_TANK_PRESSURE','VB',200],['PRESSURE_TRANSDUCER_CALIBRATED','VB',200],['COMBUSTION_CHAMBER_PRESSURE','DAQ',500],['COMBUSTION_CHAMBER_VOLTAGE','DAQ',2.5]])pressureRows.push({timestamp_ms:stamp-3000+i*100,data_type,sender_id,values:[value+i/100]});`);
const extras=fixture.indexOf("for(const id of ['LOADCELL','DAQ'])"),serverStart=fixture.indexOf('\n\nconst server=',extras);
if(extras>=0&&serverStart>extras)fixture=fixture.slice(0,extras)+fixture.slice(serverStart);
fixture=fixture.replace("  if(pathname.startsWith('/api/')){",`  if(pathname==='/api/recent'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(pressureRows));return;}
  if(pathname==='/api/recovery'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({recovery_marker_supported:true,clock:{session:'fixture',t0_utc_ms:null},synchronized:true,station:'primary',events:[]}));return;}
  if(pathname==='/api/station_mode'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({effective_mode:'test_fire',synchronized:true}));return;}
  if(pathname.startsWith('/api/')){`);
fixture+=`
try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const sockets=new Set();await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{sockets.add(socket);socket.onClose(()=>sockets.delete(socket));});
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor().catch(async error=>{console.error((await page.locator('body').innerText()).slice(0,1800));console.error(errors);throw error});
 assert.equal(await page.getByRole('button',{name:/^Export /}).count(),0,'no global export controls');
 assert.equal(await page.locator('.gs26-operating-mode').count(),0,'mode status is not global');
 await page.getByRole('link',{name:'Data',exact:true}).click();
 for(const label of ['Fuel Tank Pressure','Combustion chamber pressure']) {
   await page.getByRole('button',{name:label,exact:true}).click();
   await page.locator('canvas[id^=gs26-chart-canvas]').first().waitFor();
   assert(await page.locator('canvas[id^=gs26-chart-canvas]').count()>0,label+' has graph');
 }
 await page.getByRole('button',{name:'Loadcell',exact:true}).click();
 for(const label of ['1000kg','50kg']) {
   await page.getByRole('button',{name:label,exact:true}).click();
   await page.locator('canvas[id^=gs26-chart-canvas]').first().waitFor();
 }
 await page.getByRole('link',{name:'Data Export',exact:true}).click();
 await page.getByRole('button',{name:'Export recovery json',exact:true}).waitFor();
 await page.getByRole('button',{name:'Export checklists',exact:true}).waitFor();
 await page.getByRole('link',{name:'Data',exact:true}).click();
 assert.equal(await page.getByRole('button',{name:/^Export /}).count(),0,'exports disappear after leaving export');
 const longText='Pressure link status '+('long_sensor_identifier_without_spaces_'.repeat(12))+' · retry pending';
 for(const socket of sockets)socket.send(JSON.stringify({ty:'Notification',data:{id:987,timestamp_ms:Date.now(),message:longText,persistent:true}}));
 for(const socket of sockets)socket.send(JSON.stringify({ty:'Warning',data:{timestamp_ms:Date.now(),message:longText}}));
 await page.locator('.gs26-message-card').first().waitFor();
 for(const width of [320,390,768]) {
   await page.setViewportSize({width,height:900});
   for(const card of await page.locator('.gs26-message-card').all()) {
     const bounds=await card.boundingBox();assert(bounds.x>=0&&bounds.x+bounds.width<=width+1,'message fits viewport '+width);
     if(!(await card.evaluate(e=>e.scrollWidth<=e.clientWidth+1))){console.error(await card.evaluate(e=>({html:e.outerHTML.slice(0,600),client:e.clientWidth,scroll:e.scrollWidth,children:[...e.children].map(c=>({tag:c.tagName,cls:c.className,w:c.getBoundingClientRect().width,client:c.clientWidth,scroll:c.scrollWidth,flex:getComputedStyle(c).flex,wrap:getComputedStyle(c).overflowWrap}))})));await page.screenshot({path:'/tmp/gs-mobile-overflow.png'});throw Error('message horizontal overflow '+width);}
     for(const button of await card.getByRole('button').all())assert((await button.boundingBox()).height>=44,'touch-sized action');
   }
   await page.screenshot({path:'/tmp/gs-messages-'+width+'.png'});
 }
 assert.deepEqual(errors,[]);await context.close();console.log('PASS real Wasm: pressure/loadcell graphs, scoped export/mode controls, mobile long-message layout at320/390/768');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
