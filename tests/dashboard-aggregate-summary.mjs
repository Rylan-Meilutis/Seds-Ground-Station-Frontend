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
dashboardLayout.data_tab.tabs.push(
 {id:'LOADCELL_50_WEIGHT_KG',label:'Configured loadcell',channels:['Mass'],show_min_max:true},
 {id:'BAROMETER_DATA',label:'Configured altitude',channels:['Altitude','Pressure','Temperature'],show_min_max:true},
 {id:'configured-metrics',label:'Generic configuration',channels:[],show_min_max:true,subtabs:[
  {id:'summary',label:'Generic summary',show_min_max:false,summary_items:[
   {label:'Visible metric',data_type:'OPERATOR_METRIC',sender_id:'CUSTOM',index:1,show_min_max:true},
   {label:'Hidden metric',data_type:'OPERATOR_METRIC',sender_id:'CUSTOM',index:0,show_min_max:false},
   {label:'Ambiguous visible',data_type:'OPERATOR_METRIC',sender_id:'CUSTOM',index:2,show_min_max:true},
   {label:'Ambiguous hidden',data_type:'OPERATOR_METRIC',sender_id:'CUSTOM',index:2,show_min_max:false}]},
  {id:'hidden',label:'Hidden subtab',data_type:'HIDDEN_METRIC',sender_id:'CUSTOM',channels:['Value'],show_min_max:false}]},
 {id:'BOOLEAN_METRIC',label:'Binary configuration',channels:['State'],boolean_labels:{true_label:'On',false_label:'Off',unknown_label:'Unknown'},show_min_max:true}
);
const launch=Date.now()-35000;
const recovery={recovery_marker_supported:true,clock_reset_request_supported:true,clock:{authority:'fixture',session:'a'.repeat(32),t0_utc_ms:launch,launch_action_utc_ms:launch},clock_known:true,synchronized:true,persistence_error:null,station:'primary',events:[],reset:{pending:false,error:null}};
const server=http.createServer`);
fixture=fixture.replace("  if(pathname==='/test-camera')",`  if(pathname==='/api/recovery'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(recovery));return;}
  if(pathname==='/test-camera')`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{localStorage.setItem('navigation-style','tabs');
 const key='gs26_dashboard_customization_v1_'+origin.replace(/[^a-zA-Z0-9]/g,'_')+'_cards_v1_ui-test';
 const card=(label,data_type,sender,index,display,unit='u')=>({label,data_type,sender,index,display,unit,section:'Measurements',min:0,max:10000,visible:true});
 localStorage.setItem(key,JSON.stringify([
  card('50 kg loadcell','LOADCELL_50_WEIGHT_KG','DAQ',0,'trend','kg'),
  card('Chamber pressure','COMBUSTION_CHAMBER_PRESSURE','DAQ',0,'gauge','psi'),
  {...card('Loadcell number','LOADCELL_50_WEIGHT_KG','DAQ',0,'number','kg'),pinned:true},
  card('Altitude gauge','BAROMETER_DATA','FC',0,'gauge','m'),
  card('Generic bar','OPERATOR_METRIC','CUSTOM',1,'bar'),
  card('Hidden summary','OPERATOR_METRIC','CUSTOM',0,'number'),
  card('Hidden subtab','HIDDEN_METRIC','CUSTOM',0,'number'),
  card('Hidden trend','HIDDEN_METRIC','CUSTOM',0,'trend'),
  card('Binary trend','BOOLEAN_METRIC','CUSTOM',0,'trend'),
  card('Other sender','OPERATOR_METRIC','OTHER',1,'number'),
  card('Wildcard sender','OPERATOR_METRIC','',1,'number'),
  card('Ambiguous metric','OPERATOR_METRIC','CUSTOM',2,'number'),
  card('Numeric binary','BOOLEAN_METRIC','CUSTOM',0,'number'),
  card('Binary state','BOOLEAN_METRIC','CUSTOM',0,'state'),
  card('Unconfigured number','UNKNOWN_METRIC','CUSTOM',0,'number'),
  card('Unconfigured trend','UNKNOWN_METRIC','CUSTOM',0,'trend')
 ]));localStorage.setItem(key+'_defaults_version','3');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(origin);await page.getByRole('button',{name:'Customize dashboard',exact:true}).waitFor();
 while(!sockets.size)await page.waitForTimeout(20);
 const aggregate=page.getByRole('button',{name:'Show launch aggregate',exact:true});
 await aggregate.waitFor();await page.waitForFunction(()=>Array.from(document.querySelectorAll('button')).some(e=>e.textContent==='Show launch aggregate'&&!e.disabled));
 const card=page.locator('.gs26-dashboard-card').filter({has:page.getByText('50 kg loadcell',{exact:true})});
 const rows=[[-1,-500],[1000,2],[15000,10],[30000,3],[31000,9999]].map(([offset,value])=>({data_type:'LOADCELL_50_WEIGHT_KG',sender_id:'DAQ',values:[value],timestamp_ms:launch+offset,source_timestamp_ms:launch+offset,received_timestamp_ms:Date.now()}));
 // Separate real fixture deliveries across Data's existing20ms receipt buckets.
 // Source and admitted launch timestamps stay unchanged; each new packet gets its actual creation time.
 const deliver=async rows=>{for(const row of rows){broadcast({ty:'TelemetryBatch',data:[{...row,received_timestamp_ms:Date.now()}]});await page.waitForTimeout(40);}};
 await deliver(rows);
 const more=[['BAROMETER_DATA','FC',[100,1,2],[80,3,4]],['OPERATOR_METRIC','CUSTOM',[6,-7,55],[2,9,44]],['HIDDEN_METRIC','CUSTOM',[20],[30]],['OPERATOR_METRIC','OTHER',[0,700],[0,900]],['BOOLEAN_METRIC','CUSTOM',[0],[1]],['UNKNOWN_METRIC','CUSTOM',[4],[8]]].flatMap(([data_type,sender_id,first,last])=>[first,last].map((values,i)=>({data_type,sender_id,values,timestamp_ms:launch+(i?15000:1000),source_timestamp_ms:launch+(i?15000:1000),received_timestamp_ms:Date.now()})));
 await deliver(more);
 const measurement=name=>page.locator('.gs26-dashboard-card').filter({has:page.getByText(name,{exact:true})});
 await page.waitForFunction(()=>Array.from(document.querySelectorAll('.gs26-card-value')).some(e=>e.textContent==='9999.00 kg'));
 assert.match(await measurement('Loadcell number').innerText(),/Data history · Min -500.00 · Max 9999.00 kg/,'Configured numeric measurement retains Data extrema');
 assert.match(await measurement('Altitude gauge').innerText(),/Data history · Min 80.00 · Max 100.00 m/);
 assert.match(await measurement('Generic bar').innerText(),/Data history · Min -7.00 · Max 9.00 u/,'Per-item true overrides inherited false for arbitrary source');
 assert.equal(await measurement('Loadcell number').locator('svg').count(),0,'Number stays Number');
 await measurement('Altitude gauge').getByRole('img').waitFor();assert.equal(await measurement('Generic bar').locator('progress').count(),1,'Bar stays Bar');
 for(const name of ['Hidden summary','Hidden subtab','Other sender','Wildcard sender','Ambiguous metric','Numeric binary','Binary state','Unconfigured number'])assert.doesNotMatch(await measurement(name).innerText(),/ · Min /,name+' must not gain extrema');
 assert.match(await measurement('Unconfigured trend').innerText(),/Data history · Min 4.00 · Max 8.00 u/,'Unknown layout retains existing trend behavior');
 for(const name of ['Hidden trend','Binary trend']){assert.doesNotMatch(await measurement(name).innerText(),/ · Min /,name+' configured exclusion also applies to Trend extrema');assert.equal(await measurement(name).locator('svg').count(),1,name+' retains its Trend plot');}
 assert.equal(await measurement('Hidden trend').locator('.gs26-card-value').innerText(),'30.00 u');
 assert.equal(await measurement('Binary trend').locator('.gs26-card-value').innerText(),'1.00 u');
 assert.doesNotMatch(await page.locator('[aria-label="Pinned telemetry"]').innerText(),/ · Min /,'Compact pin stays compact');

 await page.waitForFunction(()=>Array.from(document.querySelectorAll('.gs26-card-value')).some(e=>e.textContent==='9999.00 kg'));
 await aggregate.click();
 await page.waitForFunction(()=>Array.from(document.querySelectorAll('.gs26-card-value')).some(e=>e.textContent==='3.00 kg'));
 assert.equal(await card.locator('.gs26-card-value').innerText(),'3.00 kg');
 assert.match(await measurement('Loadcell number').innerText(),/Launch capture · Min 2.00 · Max 10.00 kg/);
 assert.match(await measurement('Generic bar').innerText(),/Launch capture · Min -7.00 · Max 9.00 u/);
 for(const name of ['Hidden summary','Hidden subtab','Ambiguous metric'])assert.doesNotMatch(await measurement(name).innerText(),/ · Min /);
 for(const name of ['Hidden trend','Binary trend']){assert.doesNotMatch(await measurement(name).innerText(),/ · Min /,name+' exclusion persists in launch capture');assert.equal(await measurement(name).locator('svg').count(),1,name+' capture plot remains visible');}
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
