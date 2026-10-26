// Build with `dx build --platform web`, then run with Node and Playwright.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root=path.resolve(process.env.GS_UI_BUNDLE || 'target/dx/groundstation_frontend/debug/web/public');
const example=name=>JSON.parse(fs.readFileSync(`docs/api-examples/${name}.json`,'utf8'));
const session={...example('auth-session.anonymous'),authenticated:true,anonymous:false,username:'ui-test',roles:['admin'],permissions:{view_data:true,send_commands:true}};
let lists={revision:0,enforce_before_launch:false,checklists:[{id:'setup',title:'Setup',required_before_launch:true,items:[{id:'pressure',text:'Pressure checked',checked:false,completed_by:null,completed_at_ms:null}]},{id:'recovery',title:'Recovery',required_before_launch:false,items:[{id:'recover',text:'Recovery area clear',checked:false,completed_by:null,completed_at_ms:null}]}]};
let cameraOnline=true,abortClears=0;
let broadcastState=example('live-streams').broadcast;
let fillSource='kg50';
let ptConfig={nitrogen_target_psi:100,pressure_step_psi:50,pressure_ceiling_psi:500,maximum_zero_offset_psi:20,dry_self_test_confirmed:false,grouped_panel:false};let overrides=0;
let gseStatus={phase:'idle',message:'Ready',nitrogen_passed:false,nitrogen_overridden:false,self_test_locked:false,current_step_psi:0,pressure_psi:0,baseline:{average_psi:0,min_psi:0,max_psi:0,noise_psi:0.1,samples:50},valves:[false,false,false,false,false],dry_self_test_confirmed:false,qualification:{version:2,generation:0,self_test:true,nitrogen_test:false,fill_target:false,fault:null,nitrogen_override:null,filling_started:false,fire_started:false},mission_stage:'idle',topology_stable:true,storage_error:null,launch_queued:false,request_gate:{hitl_mode:false,flight_state:'Idle',prelaunch:true,button_interlock_satisfied:true}}, qualificationResets=0,gseUnavailable=false;
let actionPolicy=example('action-policy');
let adminAccounts=[{username:'test-admin',roles:['admin'],permissions:{view_data:true,send_commands:false,set_system_time:false,voice_transmit:false},command_access:{allowed_commands:[]},calibration_access:{view:false,edit:false},disabled:false}];
const dashboardLayout=example('layout.full');
for(const id of ['LOADCELL','DAQ']) dashboardLayout.data_tab.tabs.push({id,label:id,channels:[],subtabs:[
  {id:'large',label:'1000 kg',data_type:'KG1000',channels:['Raw'],chart:{enabled:true},chart_groups:[{title:'Fill %',data_type:'LOADCELL_FILL_PERCENT',channels:[0]}],summary_items:[{label:'Fill %',data_type:'LOADCELL_FILL_PERCENT',index:0}]},
  {id:'small',label:'50 kg',data_type:'KG50',channels:['Raw'],chart:{enabled:true},chart_groups:[],summary_items:[]}
]});

const server=http.createServer(async(req,res)=>{
  const pathname=new URL(req.url,'http://localhost').pathname;
  if(pathname==='/api/checklists'||pathname==='/api/checklists/check') {
    res.setHeader('Content-Type','application/json');
    if(req.method==='POST') {
      assert.equal(req.headers.authorization,'Bearer ui-token');
      let body='';for await(const data of req)body+=data;const request=JSON.parse(body);
      if(pathname.endsWith('/check')) {const item=lists.checklists.find(c=>c.id===request.checklist_id).items.find(i=>i.id===request.item_id);item.checked=request.checked;item.completed_by=request.checked?'ui-test':null;}
      else {if(request.revision!==lists.revision){res.statusCode=409;res.end('Checklists changed on another device; reload before saving');return;}lists=request;}
      lists.revision++;
    }
    res.end(JSON.stringify(lists));return;
  }
  if(pathname==='/test-camera') {res.setHeader('Content-Type','text/html');res.end('<p>Live camera fixture</p>');return;}
  if(pathname==='/radio'||pathname==='/media') {res.setHeader('Content-Type','text/html');res.end(`<p>Media tool fixture</p><script>window.received=[];setTimeout(()=>addEventListener('message',e=>{if(e.source===parent)received.push(e.data)}),900)</script>`);return;}
  if(pathname==='/api/admin/users'||pathname==='/api/admin/users/create'){
    res.setHeader('Content-Type','application/json');
    if(req.method==='POST'){
      let body='';for await(const data of req)body+=data;
      const account=JSON.parse(body);
      assert.equal(req.headers.authorization,'Bearer ui-token');
      if(account.username==='test-admin'&&account.disabled){res.statusCode=409;res.end('Keep at least one enabled application administrator');return;}
      delete account.password;
      const index=adminAccounts.findIndex(old=>old.username===account.username);
      if(index<0)adminAccounts.push(account);else adminAccounts[index]=account;
      res.end(JSON.stringify(account));return;
    }
    res.end(JSON.stringify(adminAccounts));return;
  }
  if(pathname==='/api/gse/status' && gseUnavailable){res.statusCode=503;res.end('offline fixture');return;}
  if(pathname==='/api/gse/cold-flow/override'){
    assert.equal(req.method,'POST');assert.equal(req.headers.authorization,'Bearer ui-token');
    let body='';for await(const data of req)body+=data;const request=JSON.parse(body);
    assert.equal(request.reason,'External cold-flow procedure reviewed');overrides++;
    gseStatus.qualification.nitrogen_override={operator:'ui-test',reason:request.reason,at_ms:Date.now()};
    gseStatus.nitrogen_overridden=true;gseStatus.phase='passed';
    res.setHeader('Content-Type','application/json');res.end('{"overridden":true}');return;
  }
  if(pathname==='/api/gse/self-test-confirmation'){
    let body='';for await(const data of req)body+=data;gseStatus.dry_self_test_confirmed=JSON.parse(body).confirmed;
    res.setHeader('Content-Type','application/json');res.end('{}');return;
  }
  if(pathname==='/api/action_policy'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(actionPolicy));return;}
  if(pathname==='/api/gse/status' && gseStatus){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(gseStatus));return;}
  if(pathname==='/api/gse/abort/clear'){
    assert.equal(req.method,'POST');assert.equal(req.headers.authorization,'Bearer ui-token');
    let body='';for await(const data of req)body+=data;assert.deepEqual(JSON.parse(body),{});
    abortClears++;gseStatus.automatic_abort_claimed=false;
    res.setHeader('Content-Type','application/json');res.end('{}');return;
  }
  if(pathname==='/api/gse/qualification/reset'){
    assert.equal(req.method,'POST');assert.equal(req.headers.authorization,'Bearer ui-token');
    qualificationResets++;gseStatus.qualification={self_test:false,nitrogen_test:false,fill_target:false,fault:null};
    res.setHeader('Content-Type','application/json');res.end('{"reset":true}');return;
  }
  if(pathname==='/api/live_streams/control') {
    let body='';for await(const data of req)body+=data;
    const next=JSON.parse(body);assert.equal(next.revision,broadcastState.revision);
    next.revision++;
    if(next.replay && !next.replay.id){next.replay.id='fixture-replay';next.replay.started_at_ms=Date.now();next.replay.source_end_ms=Date.now()-10000;}
    broadcastState=next;res.setHeader('Content-Type','application/json');res.end(JSON.stringify(next));return;
  }
  if(pathname==='/api/gse/config') {
    if(req.method==='POST'){let body='';for await(const data of req)body+=data;ptConfig=JSON.parse(body);}
    res.setHeader('Content-Type','application/json');res.end(JSON.stringify(ptConfig));return;
  }
  if(pathname.startsWith('/api/')){
    res.setHeader('Content-Type','application/json');
    const data=pathname==='/api/auth/session'?session:pathname==='/api/layout'?dashboardLayout:pathname==='/api/fill_targets'?{version:1,fill_source:fillSource,nitrogen:{target_mass_kg:10,target_pressure_psi:100},nitrous:{target_mass_kg:20,target_pressure_psi:200}}:pathname==='/api/live_streams'?{...example('live-streams'),broadcast:broadcastState,can_manage_stream:session.roles.some(role=>['stream_master','stream_admin','admin'].includes(role)),program_url:'',can_preview_live:session.roles.some(role=>['stream_master','stream_admin','admin'].includes(role)),streams:[{id:'pad-wide',label:'Pad wide',url:'/test-camera?ticket=fixture',online:cameraOnline,kind:'webrtc'}]}:pathname==='/api/vehicle'?{}:[];
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
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(()=>{}));
 const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(origin+'/dashboard');
 await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 await page.getByRole('link',{name:'Actions',exact:true}).click();
 const flow=page.locator('[aria-label="Mission workflow"]');await flow.waitFor();
 assert.equal(await flow.locator('[aria-current=step]').innerText(),'Idle');
 await page.getByRole('button',{name:'Override cold-flow requirement',exact:true}).click();
 const dialog=page.getByRole('dialog',{name:'Override nitrogen cold flow'});await dialog.waitFor();
 assert(await dialog.getByRole('button',{name:'Confirm override',exact:true}).isDisabled());
 await dialog.getByLabel('Cold-flow override reason').fill('External cold-flow procedure reviewed');
 await dialog.getByRole('button',{name:'Confirm override',exact:true}).click();
 await page.getByText('Cold-flow override saved',{exact:true}).waitFor();
 await page.getByText('Cold-flow override by ui-test',{exact:true}).waitFor();
 assert.equal(overrides,1);assert.equal(gseStatus.nitrogen_passed,false);
 for(const width of [320,390,844,1440]) {
  await page.setViewportSize({width,height:1000});
  assert(await flow.evaluate(e=>e.scrollWidth<=e.clientWidth),'four-state workflow fits viewport');
  assert.equal(Math.round((await page.getByRole('button',{name:'Reset operation checks',exact:true}).boundingBox()).height),44,'operation controls retain standard touch height');
  await page.locator('.gs26-qualification').screenshot({path:`/tmp/gs-operation-flow-${width}.png`});
 }
 for(const [stage,label] of [['fill','Fill'],['armed','Armed'],['fire','Fire']]) {
  gseStatus.mission_stage=stage;
  if(stage==='fire'){gseStatus.qualification.nitrogen_override=null;gseStatus.nitrogen_overridden=false;gseStatus.qualification.fire_started=true;gseStatus.request_gate.flight_state='Launch';}
  await page.waitForFunction(label=>document.querySelector('[aria-label="Mission workflow"] [aria-current="step"]')?.textContent===label,label);
 }
 await page.reload();await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 await page.getByRole('link',{name:'Actions',exact:true}).click();
 await page.waitForFunction(()=>document.querySelector('[aria-label="Mission workflow"] [aria-current="step"]')?.textContent==='Fire');
 assert.equal(await page.getByRole('button',{name:'Override cold-flow requirement',exact:true}).count(),0);
 assert(await page.getByRole('button',{name:'Reset operation checks',exact:true}).isDisabled());
 gseStatus.mission_stage='idle';gseStatus.qualification.fire_started=false;
 gseStatus.request_gate.flight_state='Idle';gseStatus.automatic_abort_claimed=true;
 const clearAbort=page.getByRole('button',{name:'Clear GSE abort',exact:true});await clearAbort.waitFor();
 await clearAbort.click();await page.getByRole('button',{name:'Cancel',exact:true}).click();
 assert.equal(abortClears,0,'cancelling explicit abort recovery sends no request');
 await clearAbort.click();await page.getByRole('button',{name:'Confirm clear abort',exact:true}).click();
 await clearAbort.waitFor({state:'detached'});assert.equal(abortClears,1,'claimed abort remains recoverable after a later Idle report and clears exactly once');
 assert.deepEqual(errors,[]);
 console.log('PASS: four mission states, reasoned cold-flow override, actual-pass distinction, reload, Fire lock, explicit abort recovery after Idle and 320/390/844/1440 layouts');
 await context.close();
} finally {await browser.close();await new Promise(resolve=>server.close(resolve));}
