// Authenticated RRLS proxy UX: local admission and primary acceptance never confirm outputs.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root=path.resolve(process.env.GS_UI_BUNDLE || 'target/dx/groundstation_frontend/debug/web/public');
const example=name=>JSON.parse(fs.readFileSync(`docs/api-examples/${name}.json`,'utf8'));
const session={...example('auth-session.anonymous'),authenticated:true,anonymous:false,username:'ui-test',roles:['admin'],permissions:{view_data:true,send_commands:true,set_system_time:true}};
let lists={revision:0,enforce_before_launch:false,checklists:[{id:'setup',title:'Setup',required_before_launch:true,items:[{id:'pressure',text:'Pressure checked',checked:false,completed_by:null,completed_at_ms:null}]},{id:'recovery',title:'Recovery',required_before_launch:false,items:[{id:'recover',text:'Recovery area clear',checked:false,completed_by:null,completed_at_ms:null}]}]};
let cameraOnline=true,abortClears=0,networkUtcUpdates=[];
let broadcastState=example('live-streams').broadcast;
let fillSource='kg50';
let ptConfig={nitrogen_target_psi:100,pressure_step_psi:50,pressure_ceiling_psi:500,maximum_zero_offset_psi:20,dry_self_test_confirmed:false,grouped_panel:false};let overrides=0;
let gseStatus={phase:'idle',message:'Ready',nitrogen_passed:false,nitrogen_overridden:false,self_test_locked:false,current_step_psi:0,pressure_psi:0,baseline:{average_psi:0,min_psi:0,max_psi:0,noise_psi:0.1,samples:50},valves:[false,false,false,false,false],dry_self_test_confirmed:false,qualification:{version:2,generation:0,self_test:true,nitrogen_test:false,fill_target:false,fault:null,nitrogen_override:null,filling_started:false,fire_started:false},mission_stage:'idle',topology_stable:true,storage_error:null,launch_queued:false,request_gate:{hitl_mode:false,flight_state:'Idle',prelaunch:true,button_interlock_satisfied:true}}, qualificationResets=0,gseUnavailable=false;
let actionPolicy=example('action-policy');
let adminAccounts=[{username:'test-admin',roles:['admin'],permissions:{view_data:true,send_commands:false,set_system_time:false,voice_transmit:false},command_access:{allowed_commands:[]},calibration_access:{view:false,edit:false},disabled:false}];
const dashboardLayout=example('layout.full');
let servedMode=null;
session.station_role='rrls';
gseStatus.primary_observation={fresh:true,age_ms:0,configuration_available:false};
let proxyStatus={supported:true,paired:true,connected:true,available:true,primary_mode:'normal',supported_commands:['Pilot','NormallyOpen','Dump','Nitrogen','Nitrous'],pending:null,last:null,reason:''};
session.command_proxy=structuredClone(proxyStatus);
let proxyAvailable=true;
const proxyRequests=[];
const wsCommands=[];
const physicalValveCommands=['Pilot','NormallyOpen','Dump','Nitrogen','Nitrous'];
dashboardLayout.actions_tab.actions.push({...dashboardLayout.actions_tab.actions[0],cmd:'StartWritingNow',label:'Local recorder fixture',group:'Recording'});
actionPolicy.controls.push({cmd:'StartWritingNow',enabled:true,actuated:null,blink:'none'});

for(const cmd of physicalValveCommands) {
 dashboardLayout.actions_tab.actions=dashboardLayout.actions_tab.actions.filter(a=>a.cmd!==cmd);
 dashboardLayout.actions_tab.actions.push({...dashboardLayout.actions_tab.actions[0],cmd,label:`${cmd} safety fixture`,group:'Fill controls'});
 actionPolicy.controls=actionPolicy.controls.filter(c=>c.cmd!==cmd);
 actionPolicy.controls.push({cmd,enabled:true,actuated:null,blink:'none'});
}
actionPolicy.controls=actionPolicy.controls.filter(c=>c.cmd!=='NitrogenClose');
actionPolicy.controls.push({cmd:'NitrogenClose',enabled:true,actuated:null,blink:'none'});
for(const id of ['LOADCELL','DAQ']) dashboardLayout.data_tab.tabs.push({id,label:id,channels:[],subtabs:[
  {id:'large',label:'1000 kg',data_type:'KG1000',channels:['Raw'],chart:{enabled:true},chart_groups:[{title:'Fill %',data_type:'LOADCELL_FILL_PERCENT',channels:[0]}],summary_items:[{label:'Fill %',data_type:'LOADCELL_FILL_PERCENT',index:0}]},
  {id:'small',label:'50 kg',data_type:'KG50',channels:['Raw'],chart:{enabled:true},chart_groups:[],summary_items:[]}
]});

const server=http.createServer(async(req,res)=>{
  const pathname=new URL(req.url,'http://localhost').pathname;
  if(pathname==='/api/rrls/control') {
    res.setHeader('Content-Type','application/json');
    if(!proxyAvailable){res.statusCode=404;res.end('{}');return;}
    res.end(JSON.stringify(proxyStatus));return;
  }
  if(pathname==='/api/network_time' && req.method==='POST') {
    assert.equal(req.headers.authorization,'Bearer ui-token');
    let body='';for await(const part of req)body+=part;networkUtcUpdates.push(JSON.parse(body));
    proxyStatus.pending={request_id:'utc',operation:'NetworkUtc',phase:'pending_primary',primary_accepted:false,device_confirmed:false,error:null};
    res.statusCode=202;res.setHeader('Content-Type','application/json');res.end(JSON.stringify(proxyStatus));return;
  }
  if(pathname==='/api/command' && req.method==='POST') {
    assert.equal(req.headers.authorization,'Bearer ui-token');
    let body='';for await(const part of req)body+=part;
    const command=JSON.parse(body);proxyRequests.push(command);
    proxyStatus.pending={request_id:String(proxyRequests.length),operation:command,phase:'pending_primary',primary_accepted:false,device_confirmed:false,error:null};
    res.statusCode=202;res.setHeader('Content-Type','application/json');res.end(JSON.stringify(proxyStatus));return;
  }

  if(pathname==='/api/station_mode') {res.setHeader('Content-Type','application/json');res.end(JSON.stringify(session.operating_mode||{effective_mode:'normal',synchronized:true}));return;}
  if(pathname==='/api/layout') {
    const layout=structuredClone(dashboardLayout);
    if(servedMode)layout.actions_tab.actions.find(a=>a.cmd==='Pilot').label=`Primary ${servedMode} valve`;
    res.setHeader('Content-Type','application/json');res.end(JSON.stringify(layout));return;
  }
  if(pathname==='/api/network_time' && req.method==='POST') {
    assert.equal(req.headers.authorization,'Bearer ui-token');
    let body=''; for await(const chunk of req)body+=chunk;
    networkUtcUpdates.push(JSON.parse(body));
    res.setHeader('Content-Type','application/json');res.end(JSON.stringify({network_utc_ms:networkUtcUpdates.at(-1).utc_ms}));return;
  }
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
  for(const suffix of ['',origin.replace(/[^a-zA-Z0-9]/g,'_')])localStorage.setItem(`gs26_dashboard_customization_v1_${suffix}_ground_station_view`,'on');

 },{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 let socket;
 await context.routeWebSocket(url=>url.pathname==='/ws',s=>{socket=s;s.onMessage(data=>{try{wsCommands.push(JSON.parse(String(data)))}catch{}})});
 const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(origin+'/dashboard');
 await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 await page.getByRole('link',{name:'Actions',exact:true}).click();
 const pilot=page.getByRole('button',{name:/Pilot safety fixture/});await pilot.waitFor();
 await page.getByText('Commands are sent through the primary ground station.',{exact:false}).waitFor();
 assert(await pilot.getByText('Unconfirmed',{exact:true}).isVisible());
 await pilot.click();
 await page.getByText('Pilot: queued at RRLS; waiting for the primary.',{exact:false}).waitFor();
 assert.deepEqual(proxyRequests,['Pilot']);assert.equal(wsCommands.filter(p=>p.cmd==='Pilot').length,0);
 assert(await pilot.getByText('Unconfirmed',{exact:true}).isVisible(),'HTTP202 must not confirm a device');
 proxyStatus.last={...proxyStatus.pending,phase:'accepted_by_primary',primary_accepted:true,device_confirmed:true};proxyStatus.pending=null;
 await page.getByText('Pilot: accepted by the primary.',{exact:false}).waitFor();
 assert(await pilot.getByText('Unconfirmed',{exact:true}).isVisible(),'even acceptance with an unexpected true flag cannot confirm outputs');
 actionPolicy.controls.find(c=>c.cmd==='Pilot').actuated=true;
 socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy}));
 await page.waitForFunction(()=>{const b=[...document.querySelectorAll('button')].find(e=>e.textContent.includes('Pilot safety fixture'));return b?.style.opacity==='1'&&!b.textContent.includes('Unconfirmed')});
 actionPolicy.controls.find(c=>c.cmd==='Pilot').actuated=false;
 socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy}));
 await page.waitForFunction(()=>{const b=[...document.querySelectorAll('button')].find(e=>e.textContent.includes('Pilot safety fixture'));return b?.style.opacity==='0.62'&&!b.textContent.includes('Unconfirmed')});
 proxyStatus.last={...proxyStatus.last,phase:'rejected_by_primary',primary_accepted:false,error:'Primary interlock is open'};
 await page.getByText('Pilot: rejected. Primary interlock is open',{exact:false}).waitFor();
 proxyStatus.last={...proxyStatus.last,phase:'unknown',error:'Primary disconnected'};
 await page.getByText('Pilot: outcome unknown.',{exact:false}).waitFor();
 actionPolicy.software_buttons_enabled=false;actionPolicy.controls.find(c=>c.cmd==='Pilot').actuated=null;
 // No WS event: the local authoritative getter poll must expire the old display.
 await page.waitForFunction(()=>[...document.querySelectorAll('button')].find(e=>e.textContent.includes('Pilot safety fixture'))?.disabled===true);
 assert(await pilot.getByText('Unconfirmed',{exact:true}).isVisible());
 await page.getByRole('combobox',{name:'Primary execution command'}).selectOption('Pilot');
 await page.getByRole('button',{name:'Request execution if primary permits',exact:true}).click();
 await page.getByText('Pilot: queued at RRLS; waiting for the primary.',{exact:false}).waitFor();
 assert.deepEqual(proxyRequests,['Pilot','Pilot'],'explicit request works without treating stale policy as current');
 assert(await pilot.isDisabled());assert(await pilot.getByText('Unconfirmed',{exact:true}).isVisible());
 proxyStatus.last={...proxyStatus.pending,phase:'unknown',error:'No primary result'};proxyStatus.pending=null;
 await page.getByText('Pilot: outcome unknown.',{exact:false}).waitFor();
 const recorder=page.getByRole('button',{name:/Local recorder fixture/});
 assert(await recorder.isEnabled(),'local recording survives expired remote physical policy');
 await recorder.click();await page.waitForTimeout(150);
 assert.equal(wsCommands.filter(packet=>packet.cmd==='StartWritingNow').length,1);
 assert.deepEqual(proxyRequests,['Pilot','Pilot']);
 await page.getByRole('button',{name:'Use client time',exact:true}).click();
 await page.getByText('Network UTC request queued at RRLS; waiting for primary acceptance.',{exact:true}).waitFor();
 assert.equal(networkUtcUpdates.length,1);assert.equal(networkUtcUpdates[0].source,'client');
 assert(Math.abs(networkUtcUpdates[0].utc_ms-Date.now())<5000);
 assert.equal(await page.getByText('Network UTC source updated.',{exact:true}).count(),0,'HTTP202 UTC admission is not application');
 proxyStatus.last={...proxyStatus.pending,phase:'unknown',error:'No UTC primary result'};proxyStatus.pending=null;
 proxyStatus.available=false;proxyStatus.connected=false;proxyStatus.reason='No primary hardware authority';
 await page.waitForFunction(()=>[...document.querySelectorAll('button')].find(e=>e.textContent.includes('Pilot safety fixture'))?.disabled===true);
 const before=proxyRequests.length;await pilot.click({force:true});await page.waitForTimeout(100);assert.equal(proxyRequests.length,before);
 proxyAvailable=false;
 await page.getByText('Primary command control is unavailable.',{exact:false}).waitFor();
 assert(await pilot.isDisabled());
 await page.getByRole('link',{name:'Dashboard',exact:true}).click();
 await page.getByRole('heading',{name:'Reconfigure GSE',exact:true}).waitFor();
 gseStatus.valves=[true,true,true,true,true];gseStatus.nitrogen_passed=true;
 await page.getByText('Pilot: OPEN',{exact:true}).waitFor();
 await page.getByText('Fill unlocked',{exact:true}).waitFor();
 gseStatus.primary_observation=null;
 await page.getByText('Pilot: UNKNOWN',{exact:true}).waitFor();
 await page.getByText('Fill status unknown',{exact:true}).waitFor();
 await page.getByText('Test step: unknown',{exact:true}).waitFor();
 assert.equal(await page.getByText('Fill unlocked',{exact:true}).count(),0,'expired GSE snapshot must not retain its prior qualification badge');
 gseStatus.primary_observation={fresh:true,age_ms:0};
 await page.getByText('Pilot: OPEN',{exact:true}).waitFor();
 gseUnavailable=true;
 await page.getByText('Pilot: UNKNOWN',{exact:true}).waitFor();
 assert.deepEqual(errors,[]);
 console.log('PASS: explicit primary execution request with stale display, independent local recording, networkUTC202 honesty, GSE503 clears prior confirmed valves, realWasm RRLS HTTP202/pending/accepted/rejected/unknown; only board policy changes illumination; disconnect and legacy404 stay disabled');
} finally {await browser.close();await new Promise(resolve=>server.close(resolve));}
