// Reuse the actual live-dashboard fixture; synthetic APIs and telemetry only.
// Run against the composed Wasm bundle with GS_UI_BUNDLE and PLAYWRIGHT_MODULE.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/dashboard-live-graphs.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Dashboard fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE||'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture=fixture.replace("const server=http.createServer",`dashboardLayout.test_fire_mode=true;
const launch=Date.now()-35000;
const recovery={recovery_marker_supported:true,clock_reset_request_supported:true,clock:{authority:'fixture',session:'a'.repeat(32),t0_utc_ms:launch,launch_action_utc_ms:launch},clock_known:true,synchronized:true,persistence_error:null,station:'primary',events:[],reset:{pending:false,error:null}};
const server=http.createServer`);
fixture=fixture.replace("  if(pathname==='/test-camera')",`  if(pathname==='/api/recovery'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(recovery));return;}
  if(pathname==='/test-camera')`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(origin);await page.getByRole('button',{name:'Customize dashboard',exact:true}).waitFor();
 while(!sockets.size)await page.waitForTimeout(20);
 const aggregate=page.getByRole('button',{name:'Show launch aggregate',exact:true});
 await aggregate.waitFor();await page.waitForFunction(()=>Array.from(document.querySelectorAll('button')).some(e=>e.textContent==='Show launch aggregate'&&!e.disabled));
 const card=page.locator('.gs26-dashboard-card').filter({has:page.getByText('50 kg loadcell',{exact:true})});
 const rows=[[-1,-500],[1000,2],[15000,10],[30000,3],[31000,9999]].map(([offset,value])=>({data_type:'LOADCELL_50_WEIGHT_KG',sender_id:'DAQ',values:[value],timestamp_ms:launch+offset,source_timestamp_ms:launch+offset,received_timestamp_ms:Date.now()}));
 broadcast({ty:'TelemetryBatch',data:rows});
 await page.waitForFunction(()=>Array.from(document.querySelectorAll('.gs26-card-value')).some(e=>e.textContent==='9999.00 kg'));
 await aggregate.click();
 await page.waitForFunction(()=>Array.from(document.querySelectorAll('.gs26-card-value')).some(e=>e.textContent==='3.00 kg'));
 assert.equal(await card.locator('.gs26-card-value').innerText(),'3.00 kg');
 assert.match(await card.innerText(),/Launch capture · Min 2.00 · Max 10.00 kg/);
 assert.match(await card.locator('.gs26-card-capture-time').innerText(),/last sample \\+30.0s/);
 assert.equal(await card.locator('.gs26-card-freshness').count(),0,'capture does not claim to be a current measurement');
 assert.equal(await card.locator('svg circle').count(),3,'separated capture observations remain visible');
 broadcast({ty:'TelemetryBatch',data:[{data_type:'LOADCELL_50_WEIGHT_KG',sender_id:'DAQ',values:[8888],timestamp_ms:Date.now(),source_timestamp_ms:Date.now(),received_timestamp_ms:Date.now()}]});
 await page.waitForTimeout(150);assert.equal(await card.locator('.gs26-card-value').innerText(),'3.00 kg','later live samples cannot rewrite closed capture');
 const empty=page.locator('.gs26-dashboard-card').filter({has:page.getByText('Chamber pressure',{exact:true})});
 assert.equal(await empty.locator('.gs26-card-value').innerText(),'—','missing capture has no live fallback');
 await page.getByRole('button',{name:'Show live 60s',exact:true}).click();
 await page.waitForFunction(()=>Array.from(document.querySelectorAll('.gs26-card-value')).some(e=>e.textContent==='8888.00 kg'));
 recovery.clock={...recovery.clock,session:'b'.repeat(32),launch_action_utc_ms:null,t0_utc_ms:Date.now()};
 await page.waitForFunction(()=>Array.from(document.querySelectorAll('button')).some(e=>e.textContent==='Show launch aggregate'&&e.disabled));
 assert.deepEqual(errors,[]);await context.close();console.log('PASS launch capture value, extrema, timestamp, isolated points, missing data, and live/capture switching');
} finally {await browser.close();for(const s of sockets)s.destroy();await new Promise(resolve=>server.close(resolve));}`;
await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));
