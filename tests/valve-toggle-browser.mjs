// Build with `dx build --platform web`, then run with Node and Playwright.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root=path.resolve(process.env.GS_UI_BUNDLE || 'target/dx/groundstation_frontend/debug/web/public');
const example=name=>JSON.parse(fs.readFileSync(`docs/api-examples/${name}.json`,'utf8'));
const session={...example('auth-session.anonymous'),authenticated:true,anonymous:false,username:'ui-test',roles:['stream_master'],permissions:{view_data:true,send_commands:true,voice_transmit:true,camera_publish:true}};
let cameraOnline=true;
let broadcastState=example('live-streams').broadcast;
const alertSockets=new Set(); const alertAckRequests=[]; let alertAckFailure=false;
let fillSource='kg50';
let gseStatus=null, qualificationResets=0,gseUnavailable=false;
let actionPolicy=example('action-policy');
let adminAccounts=[{username:'test-admin',roles:['admin'],permissions:{view_data:true,send_commands:false,set_system_time:false,voice_transmit:false},command_access:{allowed_commands:[]},calibration_access:{view:false,edit:false},disabled:false}];
const dashboardLayout=example('layout.full');
for(const id of ['LOADCELL','DAQ']) dashboardLayout.data_tab.tabs.push({id,label:id,channels:[],subtabs:[
  {id:'large',label:'1000 kg',data_type:'KG1000',channels:['Raw'],chart:{enabled:true},chart_groups:[{title:'Fill %',data_type:'LOADCELL_FILL_PERCENT',channels:[0]}],summary_items:[{label:'Fill %',data_type:'LOADCELL_FILL_PERCENT',index:0}]},
  {id:'small',label:'50 kg',data_type:'KG50',channels:['Raw'],chart:{enabled:true},chart_groups:[],summary_items:[]}
]});

const server=http.createServer(async(req,res)=>{
  const pathname=new URL(req.url,'http://localhost').pathname;
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
    let body='';for await(const data of req)body+=data;gseStatus.dry_self_test_confirmed=JSON.parse(body).confirmed;
    res.setHeader('Content-Type','application/json');res.end('{}');return;
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
  if(pathname==='/api/gse/status' && gseStatus){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(gseStatus));return;}
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
try {
 const commands=['Nitrogen','Nitrous','NitrogenClose','NitrousClose','Abort','CancelFill'];
 dashboardLayout.actions_tab.actions=commands.map(cmd=>({label:cmd,cmd,group:'Manual GSE valves',border:'#53657c',bg:'#142033',fg:'#ffffff'}));
 const controls=commands.map(cmd=>({cmd,enabled:true,blink:'none',actuated:cmd.endsWith('Close')?true:false}));
 actionPolicy={key_enabled:true,software_buttons_enabled:true,controls};
 const context=await browser.newContext({viewport:{width:1280,height:900}});
 await context.addInitScript(({session,origin})=>{localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base);}catch{return null;}};},{session,origin});
 const requests=[],sockets=new Set();await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 await context.routeWebSocket(url=>url.pathname==='/ws'&&url.searchParams.get('token')==='ui-token',socket=>{sockets.add(socket);socket.onMessage(message=>requests.push(JSON.parse(String(message)).cmd));const timer=setInterval(()=>socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy})),100);socket.onClose(()=>{clearInterval(timer);sockets.delete(socket);});});
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));await page.goto(origin+'/dashboard');await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Actions',exact:true}).click();
 const button=cmd=>page.locator('button').filter({has:page.locator(`span[title="${cmd}"]`)}).last();
 for(const close of ['NitrogenClose','NitrousClose'])assert.equal(await button(close).count(),0,'Only main supply button is rendered');
 assert.equal(await button('Abort').count(),1);assert.equal(await button('CancelFill').count(),1);
 const waitFor=async(test,message)=>{for(let i=0;i<100;i++){if(test())return;await page.waitForTimeout(10);}assert.fail(message);};
 const set=(cmd,values)=>{Object.assign(controls.find(c=>c.cmd===cmd),values);for(const socket of sockets)socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy}));};
 for(const cmd of ['Nitrogen','Nitrous']){
  const primary=button(cmd);await primary.waitFor();assert.equal(await primary.getAttribute('data-dispatch-command'),cmd);assert.equal(await primary.isEnabled(),true);
  const count=requests.length;await primary.click();await waitFor(()=>requests.length===count+1,'Open request actually received');assert.equal(requests.at(-1),cmd);
  // Requested ON has no physical confirmation. The same button must close.
  set(cmd,{actuated:null});set(cmd+'Close',{actuated:null});await page.waitForFunction(cmd=>document.querySelector(`button[data-dispatch-command="${cmd}Close"]`),cmd);
  assert.match(await primary.innerText(),/unconfirmed/i);const shadow=await primary.evaluate(e=>getComputedStyle(e).boxShadow);assert(!shadow.includes('0px 0px 14px'),'Unconfirmed close dispatch must not illuminate as physical ON');
  const closeCount=requests.length;await primary.click();await waitFor(()=>requests.length===closeCount+1,'Close request actually received');assert.equal(requests.at(-1),cmd+'Close');
 }
 // Pending intent may expire while feedback remains stale. Never infer OPEN.
 for(const cmd of ['Nitrogen','Nitrous']){
  set(cmd,{enabled:false,actuated:null});await page.waitForFunction(cmd=>document.querySelector(`button[data-dispatch-command="${cmd}Close"]`),cmd);
  assert.equal(await button(cmd).isEnabled(),true);const count=requests.length;await button(cmd).click();await waitFor(()=>requests.length===count+1,'Stale unknown still permits explicit close');assert.equal(requests.at(-1),cmd+'Close');
  set(cmd+'Close',{enabled:false});await page.waitForFunction(cmd=>document.querySelector(`button[data-dispatch-command="${cmd}Close"]`)?.disabled,cmd);const blocked=requests.length;await button(cmd).dispatchEvent('click');assert.equal(requests.length,blocked,'Current policy prevents stale handler dispatch');
 }
 // An older server without an explicit close capability cannot cause unknown OPEN.
 actionPolicy.controls=controls.filter(c=>!c.cmd.endsWith('Close'));for(const socket of sockets)socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy}));
 for(const cmd of ['Nitrogen','Nitrous']){await page.waitForFunction(cmd=>document.querySelector(`button[data-dispatch-command="${cmd}Close"]`)?.disabled,cmd);}
 assert.deepEqual(requests,['Nitrogen','NitrogenClose','Nitrous','NitrousClose','NitrogenClose','NitrousClose']);assert.deepEqual(errors,[]);
 session.permissions.send_commands=false;await page.reload();await page.getByRole('button',{name:/Choose tab/}).click();assert.equal(await page.getByRole('button',{name:'Actions',exact:true}).count(),0,'Read-only actor has no writable Actions tab');await context.close();
 console.log('PASS actualWASM main supply OPEN/CLOSE/pending/stale policy and permission controls; no redundant Close rows or invented physical ON');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
