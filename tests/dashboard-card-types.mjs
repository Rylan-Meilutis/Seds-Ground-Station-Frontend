// Real Wasm regression: dynamic card type selection and both calibrated load-cell trends.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/operation-workflow.mjs','utf8');
const boundary=source.indexOf('\ntry {\n const context=');
if(boundary<0)throw Error('Operation fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
fixture=fixture.replace("  if(pathname.startsWith('/api/')){",`  if(pathname==='/api/system/time'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({utc_ms:Date.now(),network_utc_ms:Date.now(),can_set_system_time:false}));return;}
 if(pathname.startsWith('/api/')){`);
fixture=fixture.replace("const dashboardLayout=example('layout.full');",`const dashboardLayout=example('layout.full');dashboardLayout.data_tab.tabs.push({id:'FUEL_TANK_PRESSURE',label:'Tank pressure',channels:['Pressure'],chart:{enabled:true},chart_groups:null,subtabs:null,boolean_labels:null,channel_boolean_labels:null,channel_formatters:null,show_min_max:null});dashboardLayout.data_tab.tabs.push({id:'SCHEMA_PICKER_TEST',label:'Schema picker sensor',channels:['Value'],chart:null,chart_groups:null,subtabs:null,boolean_labels:null,channel_boolean_labels:null,channel_formatters:null,show_min_max:null});`);
fixture+=`
try {
 const context=await browser.newContext({viewport:{width:1440,height:1100}});
 await context.addInitScript(({session,origin})=>{
  if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};
  localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));
  const key='gs26_dashboard_customization_v1_'+origin.replace(/[^a-zA-Z0-9]/g,'_')+'_cards_v1_ui-test';
  if(!localStorage.getItem(key))localStorage.setItem(key,JSON.stringify([
   {label:'50 kg loadcell',section:'Fill System',data_type:'LOADCELL_50_WEIGHT_KG',sender:'DAQ',index:0,unit:'kg',display:'number',min:0,max:50,visible:true},
   {label:'1000 kg loadcell',section:'Fill System',data_type:'LOADCELL_WEIGHT_KG',sender:'DAQ',index:0,unit:'kg',display:'trend',min:0,max:1000,visible:true},
   {label:'Tank pressure',section:'Fill System',data_type:'FUEL_TANK_PRESSURE',sender:'VB',index:0,unit:'psi',display:'gauge',min:0,max:1000,visible:true},
   {label:'My offline custom sensor',section:'Custom',data_type:'OFFLINE_CUSTOM_TYPE',sender:'CUSTOM',index:0,unit:'V',display:'number',min:0,max:10,visible:true}
  ]));
 },{session,origin});
 const sockets=new Set();await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{sockets.add(socket);socket.onClose(()=>sockets.delete(socket));});
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));
 await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 let sequence=0;
 const send=(dynamic=false)=>{const value=++sequence;const data=['LOADCELL_WEIGHT_KG','LOADCELL_50_WEIGHT_KG','FUEL_TANK_PRESSURE','COMBUSTION_CHAMBER_PRESSURE'].map(data_type=>({timestamp_ms:Date.now(),data_type,sender_id:data_type==='FUEL_TANK_PRESSURE'?'VB':'DAQ',values:[value]}));if(dynamic)data.push({timestamp_ms:Date.now(),data_type:'AAA_RUNTIME_CUSTOM_TYPE',sender_id:'CUSTOM',values:[123]});for(const socket of sockets)socket.send(JSON.stringify({ty:'TelemetryBatch',data}));};
 const timer=setInterval(()=>send(),40);
 try {
  const card=name=>page.locator('.gs26-dashboard-card').filter({has:page.getByText(name,{exact:true})});
  await card('50 kg loadcell').locator('polyline').waitFor();await card('1000 kg loadcell').locator('polyline').waitFor();
  const first50=await card('50 kg loadcell').locator('polyline').getAttribute('points'),first1000=await card('1000 kg loadcell').locator('polyline').getAttribute('points');
  await page.waitForTimeout(500);clearInterval(timer);send();
  const expected=sequence.toFixed(2)+' kg';await card('50 kg loadcell').getByText(expected,{exact:true}).waitFor();await card('1000 kg loadcell').getByText(expected,{exact:true}).waitFor();
  assert.notEqual(await card('50 kg loadcell').locator('polyline').getAttribute('points'),first50);
  assert.notEqual(await card('1000 kg loadcell').locator('polyline').getAttribute('points'),first1000);
  for(const name of ['50 kg loadcell','1000 kg loadcell']){const points=await card(name).locator('polyline').getAttribute('points');assert(new Set(points.trim().split(/\\s+/).map(point=>point.split(',')[1])).size>1,name+' trend must not be flat');}
  await card('Tank pressure').getByRole('img').waitFor();await card('Chamber pressure').getByRole('img').waitFor();
  await page.getByRole('link',{name:'Telemetry',exact:true}).click();
  for(const label of ['Tank pressure','Combustion chamber pressure']){await page.getByRole('button',{name:label,exact:true}).click();await page.locator('canvas[id^=gs26-chart-canvas]').first().waitFor();}
  await page.getByRole('link',{name:'Dashboard',exact:true}).click();
  await page.getByRole('button',{name:'Customize dashboard',exact:true}).click();
  let types=page.getByRole('combobox',{name:'Data type',exact:true});
  assert.equal(await types.count(),5);
  assert.equal(await types.first().locator('option[value="SCHEMA_PICKER_TEST"]').textContent(),'Schema picker sensor (SCHEMA_PICKER_TEST)');
  assert.equal(await types.first().locator('option[value="OFFLINE_CUSTOM_TYPE"]').count(),1);
  send(true);await types.first().locator('option[value="AAA_RUNTIME_CUSTOM_TYPE"]').waitFor({state:'attached',timeout:3000});assert.equal(await types.first().inputValue(),'LOADCELL_50_WEIGHT_KG','inserting a sorted earlier option preserves selection');
  await page.getByRole('button',{name:'Add card',exact:true}).click();
  types=page.getByRole('combobox',{name:'Data type',exact:true});const added=card('New telemetry card').getByRole('combobox',{name:'Data type',exact:true});await added.selectOption('AAA_RUNTIME_CUSTOM_TYPE');
  await page.waitForTimeout(100);
  const saved=await page.evaluate(()=>{const key=Object.keys(localStorage).find(key=>key.endsWith('_cards_v1_ui-test'));return JSON.parse(localStorage.getItem(key));});
  assert.equal(saved.at(-1).data_type,'AAA_RUNTIME_CUSTOM_TYPE');assert.equal(saved[0].display,'trend');assert.equal(saved.find(card=>card.label==='My offline custom sensor').display,'number');
  const longest=await added.locator('option').evaluateAll(options=>options.reduce((longest,option)=>option.textContent.length>longest.text.length?{value:option.value,text:option.textContent}:longest,{value:'',text:''}));await added.selectOption(longest.value);
  for(const width of [390,320]){await page.setViewportSize({width,height:844});const box=await added.boundingBox();assert(box.width>=44&&box.height>=44&&box.x>=0&&box.x+box.width<=width,'longest type option fits '+width+'px');}
  await added.selectOption('AAA_RUNTIME_CUSTOM_TYPE');
  await card('Chamber pressure').getByRole('button',{name:'Remove',exact:true}).click();
  await page.reload();await page.getByRole('button',{name:'Customize dashboard',exact:true}).click();assert.equal(await page.getByText('Chamber pressure',{exact:true}).count(),0);
  const offline=card('New telemetry card').getByRole('combobox',{name:'Data type',exact:true});
  assert.equal(await offline.inputValue(),'AAA_RUNTIME_CUSTOM_TYPE');assert.equal(await offline.locator('option[value="AAA_RUNTIME_CUSTOM_TYPE"]').count(),1);
  assert.deepEqual(errors,[]);
  console.log('PASS actual Wasm: both calibrated loadcells show moving trends; both pressure gauges/Data charts available; removed chamber stays removed; native data-type picker merges schema/live/offline types and saves added card; mobile44px control fits');
 }finally{clearInterval(timer)}
 await context.close();
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
