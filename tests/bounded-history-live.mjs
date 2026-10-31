// Actual Wasm bootstrap from the backend sampler's 175k-row regression output.
// Produce with GS_DISPLAY_HISTORY_FIXTURE=/tmp/gs-display-history-sampled-20261030.json
// cargo test -p groundstation_backend display_history_tests; no hardware endpoints.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/operation-workflow.mjs','utf8');
const boundary=source.indexOf('\ntry {\n const context=');
if(boundary<0)throw Error('Operation fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
fixture=fixture.replace("const dashboardLayout=example('layout.full');",`const dashboardLayout=JSON.parse(fs.readFileSync(process.env.GS_CHAMBER_LAYOUT,'utf8'));
const sampled=JSON.parse(fs.readFileSync(process.env.GS_HISTORY_FIXTURE||'/tmp/gs-display-history-sampled-20261030.json','utf8'));assert(sampled.length<=12000);const end=Math.max(...sampled.map(row=>row.timestamp_ms));const offset=Date.now()-end;for(const row of sampled)row.timestamp_ms+=offset;let historyRequests=0;`);
const extras=fixture.indexOf("for(const id of ['LOADCELL','DAQ'])"),serverStart=fixture.indexOf('\n\nconst server=',extras);
if(extras>=0&&serverStart>extras)fixture=fixture.slice(0,extras)+fixture.slice(serverStart);
fixture=fixture.replace("  if(pathname.startsWith('/api/')){",`  if(pathname==='/api/recent'){const query=new URL(req.url,'http://localhost').searchParams;assert.equal(query.get('display'),'true');assert.equal(query.get('max_rows'),'12000');historyRequests++;res.setHeader('Content-Type','application/json');res.end(JSON.stringify(sampled));return;}
 if(pathname==='/api/system/time'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({utc_ms:Date.now(),network_utc_ms:Date.now(),can_set_system_time:false}));return;}
 if(pathname.startsWith('/api/')){`);
fixture+=`
try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));window.__lag=0;let previous=performance.now();setInterval(()=>{const now=performance.now();window.__lag=Math.max(window.__lag,now-previous);previous=now},50);},{session,origin});
 const sockets=new Set();await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{sockets.add(socket);socket.onClose(()=>sockets.delete(socket));});
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));
 const start=Date.now();await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor().catch(async error=>{console.error('UI errors',errors);console.error((await page.locator('body').innerText()).slice(0,2500));throw error});
 await page.getByRole('link',{name:'Data',exact:true}).click();await page.getByRole('button',{name:'Combustion chamber pressure',exact:true}).click();
 await page.locator('canvas[id^=gs26-chart-canvas]').first().waitFor();
 assert(historyRequests>0,'display history requested');
 // A20Hz mobile-friendly display stream must stay live while history is seeded.
 let sequence=0;const timer=setInterval(()=>{const data=[{timestamp_ms:Date.now(),data_type:'COMBUSTION_CHAMBER_PRESSURE',sender_id:'DAQ',values:[700+sequence++]},{timestamp_ms:Date.now(),data_type:'COMBUSTION_CHAMBER_VOLTAGE',sender_id:'DAQ',values:[3.5]}];for(const socket of sockets)socket.send(JSON.stringify({ty:'TelemetryBatch',data}));},50);
 await page.waitForTimeout(1500);clearInterval(timer);
 const expected=(699+sequence).toFixed(2)+' psig';await page.getByText(expected,{exact:true}).waitFor({timeout:3000});
 assert(Date.now()-start<15000,'bounded bootstrap and live updates complete promptly');
 const lag=await page.evaluate(()=>window.__lag);assert(lag<1500,'history must not freeze render event loop: '+lag);
 await page.getByRole('button',{name:'Fuel Tank Pressure',exact:true}).click();await page.getByText('42.00',{exact:false}).first().waitFor();
 assert.deepEqual(errors,[]);await context.close();console.log('PASS actual Wasm:175k-source bounded-history payload, sparsePT/latest retained, live updates catch up after reseed, event-loop maxlag='+Math.round(lag)+'ms');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
