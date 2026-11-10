// Build with `dx build --platform web`, then run with Node and Playwright.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root=path.resolve(process.env.GS_UI_BUNDLE || 'target/dx/groundstation_frontend/debug/web/public');
const example=name=>JSON.parse(fs.readFileSync(`docs/api-examples/${name}.json`,'utf8'));
const session={...example('auth-session.anonymous'),authenticated:true,anonymous:false,username:'ui-test',roles:['stream_master'],permissions:{view_data:true,send_commands:true,voice_transmit:true,camera_publish:true}};
let cameraOnline=true;let holdConfirmation=false,confirmationStatus=200;const confirmationWaiters=[];
let broadcastState=example('live-streams').broadcast;
const alertSockets=new Set(); const alertAckRequests=[]; let alertAckFailure=false;
let fillSource='kg50';
let gseConfig={cold_flow_target_psi:null,nitrogen_target_psi:100,pressure_step_psi:50,nitrogen_steps:[],fill_overfill_kg:0.05,fill_tolerance_kg:0.07,fill_settle_ms:3000,turbocharge_max_added_mass_kg:null,pressure_ceiling_psi:650,maximum_zero_offset_psi:20,dry_self_test_confirmed:false,grouped_panel:false,future_limit:{preserved:true}};
const configSaves=[],calibrationRequests=[],selfTestConfirmations=[];
let holdStatus=false,statusRequests=0,statusResponses=0;const heldStatus=[];let gseStatus=null, qualificationResets=0,gseUnavailable=false;
let actionPolicy=example('action-policy');
let adminAccounts=[{username:'test-admin',roles:['admin'],permissions:{view_data:true,send_commands:false,set_system_time:false,voice_transmit:false},command_access:{allowed_commands:[]},calibration_access:{view:false,edit:false},disabled:false}];
const dashboardLayout=example('layout.full');
for(const id of ['LOADCELL','DAQ']) dashboardLayout.data_tab.tabs.push({id,label:id,channels:[],subtabs:[
  {id:'large',label:'1000 kg',data_type:'KG1000',channels:['Raw'],chart:{enabled:true},chart_groups:[{title:'Fill %',data_type:'LOADCELL_FILL_PERCENT',channels:[0]}],summary_items:[{label:'Fill %',data_type:'LOADCELL_FILL_PERCENT',index:0}]},
  {id:'small',label:'50 kg',data_type:'KG50',channels:['Raw'],chart:{enabled:true},chart_groups:[],summary_items:[]}
]});

// Current stations wrap every GSE status in a sequence-progress envelope; the UI
// treats a status without one as unavailable.
const withSequenceProgress=status=>({sequence_progress:{version:1,backend_instance:'1'.repeat(32),reset_epoch:'2'.repeat(16),cancel_api_version:1,active:[],terminal:[],reporting_error:null},...status});
const server=http.createServer(async(req,res)=>{
  const pathname=new URL(req.url,'http://localhost').pathname;
  if(pathname.startsWith('/api/calibration'))calibrationRequests.push({path:pathname,method:req.method});
  if(pathname==='/api/gse/config'){
    res.setHeader('Content-Type','application/json');
    if(req.method==='POST'){let raw='';for await(const part of req)raw+=part;assert.equal(req.headers.authorization,'Bearer ui-token');gseConfig=JSON.parse(raw);configSaves.push(structuredClone(gseConfig));}
    res.end(JSON.stringify(gseConfig));return;
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
  if(pathname==='/api/gse/self-test-confirmation'){
    let body='';for await(const data of req)body+=data;gseStatus.dry_self_test_confirmed=JSON.parse(body).confirmed;selfTestConfirmations.push(JSON.parse(body));gseStatus.message='Fixture isolation '+selfTestConfirmations.length+':'+gseStatus.dry_self_test_confirmed;
    if(holdConfirmation)await new Promise(resolve=>confirmationWaiters.push(resolve));res.setHeader('Content-Type','application/json');res.statusCode=confirmationStatus;res.end('{}');return;
  }
  if(pathname==='/api/alerts/ack') {
    let raw=''; for await(const part of req)raw+=part;
    const body=JSON.parse(raw);alertAckRequests.push(body);
    if(alertAckFailure){res.statusCode=503;res.end('fixture unavailable');return;}
    const state={warning_ack_timestamp_ms:body.warning_timestamp_ms,error_ack_timestamp_ms:body.error_timestamp_ms};
    for(const socket of alertSockets)socket.send(JSON.stringify({ty:'AlertAckState',data:state}));
    res.setHeader('Content-Type','application/json');res.end(JSON.stringify(state));return;
  }
  if(pathname==='/api/action_policy'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(actionPolicy));return;}
  if(pathname==='/api/gse/status' && gseStatus){statusRequests++;const snapshot=JSON.stringify(withSequenceProgress(gseStatus));if(holdStatus)await new Promise(resolve=>heldStatus.push(resolve));statusResponses++;res.setHeader('Content-Type','application/json');res.end(snapshot);return;}
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
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY || (process.platform==='darwin' && fs.existsSync('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome') ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' : undefined)});
try{
 const errors=[];session.roles=[];session.username='confirmation-operator';
 dashboardLayout.actions_tab.actions=[{label:'ValveSelfTest',cmd:'ValveSelfTest',group:'GSE sequence actions',border:'#53657c',bg:'#142033',fg:'#ffffff'}];
 actionPolicy={key_enabled:true,software_buttons_enabled:true,controls:[{cmd:'ValveSelfTest',enabled:true,blink:'none'}]};
 const idle=()=>({phase:'idle',message:'Current confirmation owner fixture',nitrogen_passed:false,self_test_locked:false,current_step_psi:0,pressure_psi:null,baseline:null,dry_self_test_confirmed:false,request_gate:{hitl_mode:true,flight_state:'Startup',prelaunch:true,button_interlock_satisfied:true}});
 async function open(send){session.permissions.send_commands=send;gseStatus=idle();const context=await browser.newContext({viewport:{width:1280,height:900}});await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{socket.onMessage(()=>{});const beat=setInterval(()=>socket.send(JSON.stringify({ty:'NetworkTime',data:{timestamp_ms:Date.now()}})),500);socket.onClose(()=>clearInterval(beat));});const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));await page.goto(origin+'/dashboard');await page.getByRole('button',{name:/Choose tab/}).click();const actions=page.getByRole('button',{name:'Actions',exact:true});if(await actions.count()){await actions.click();if(send)await page.getByText(/Current confirmation owner fixture/).waitFor({state:'attached'});}return{context,page};}
 const readonly=await open(false);const locked=readonly.page.getByRole('checkbox',{name:/gas supplies are isolated/});assert((await locked.count())===0||await locked.isDisabled(),'read-only user cannot confirm isolation');assert.deepEqual(selfTestConfirmations,[]);await readonly.context.close();
 holdConfirmation=true;confirmationStatus=200;const rapid=await open(true);const checkbox=rapid.page.getByRole('checkbox',{name:/gas supplies are isolated/});await checkbox.click();for(let n=0;n<100&&!selfTestConfirmations.length;n++)await new Promise(r=>setTimeout(r,10));assert.deepEqual(selfTestConfirmations,[{confirmed:true}]);await rapid.page.waitForFunction(()=>[...document.querySelectorAll('input[type=checkbox]')].some(e=>e.closest('label')?.textContent.includes('gas supplies are isolated')&&e.disabled),null,{timeout:3000}).catch(()=>{});assert(await checkbox.isDisabled(),'one original request is pending');await checkbox.dispatchEvent('change');assert.deepEqual(selfTestConfirmations,[{confirmed:true}],'a second event cannot enqueue a second pending confirmation');holdConfirmation=false;for(const resolve of confirmationWaiters.splice(0))resolve();await rapid.page.getByText('Self-test confirmation accepted. Backend interlocks still apply.',{exact:true}).waitFor();assert(await checkbox.isChecked());await rapid.context.close();
 selfTestConfirmations.length=0;holdConfirmation=true;confirmationStatus=401;const gone=await open(true);await gone.page.getByRole('checkbox',{name:/gas supplies are isolated/}).click();for(let n=0;n<100&&!selfTestConfirmations.length;n++)await new Promise(r=>setTimeout(r,10));assert.deepEqual(selfTestConfirmations,[{confirmed:true}]);await gone.page.getByRole('button',{name:/Choose tab/}).click();await gone.page.getByRole('button',{name:'Dashboard',exact:true}).click();holdConfirmation=false;for(const resolve of confirmationWaiters.splice(0))resolve();gseStatus=idle();await gone.page.getByRole('button',{name:/Choose tab/}).click();await gone.page.getByRole('button',{name:'Actions',exact:true}).click();await gone.page.getByText(/Current confirmation owner fixture/).waitFor({state:'attached'});assert.equal(await gone.page.getByRole('checkbox',{name:/gas supplies are isolated/}).isChecked(),false,'unmounted original response cannot confirm new Actions view');assert.equal(await gone.page.getByText('Self-test confirmation accepted. Backend interlocks still apply.',{exact:true}).count(),0);assert((await gone.page.evaluate(()=>localStorage.getItem('auth_session_v1'))).includes('ui-token'),'late401 does not clear the signed-in account');await gone.context.close();assert.deepEqual(errors,[]);console.log('PASS actual readonly, rapid pending-event, unmount/reopen and late401 login-preservation controls');
}finally{holdConfirmation=false;for(const resolve of confirmationWaiters.splice(0))resolve();await browser.close();await new Promise(resolve=>server.close(resolve));}
