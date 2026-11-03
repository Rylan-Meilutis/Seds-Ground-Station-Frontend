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
 const results=[];
 const commands=['ResetFlightState','ResetTClock'];
 dashboardLayout.actions_tab.actions=commands.map(cmd=>({label:cmd,cmd,group:'Command acceptance',border:'#53657c',bg:'#142033',fg:'#ffffff'}));
 actionPolicy={key_enabled:true,software_buttons_enabled:true,controls:commands.map(cmd=>({cmd,enabled:true,blink:'none'}))};
 gseStatus={phase:'idle',message:'Isolated test',dry_self_test_confirmed:false,configuration_error:null,request_gate:{hitl_mode:true,flight_state:'Startup',prelaunch:true,button_interlock_satisfied:true}};
 for(const theme of ['default','light']) for(const width of [1280,390]) {
  const context=await browser.newContext({viewport:{width,height:900}});
  await context.addInitScript(({session,origin,theme})=>{localStorage.setItem('navigation-style','tabs');localStorage.setItem('gs_theme_preset',theme);localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base);}catch{return null;}};},{session,origin,theme});
  const requests=[];
  await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
  await context.routeWebSocket(url=>url.pathname==='/ws'&&url.searchParams.get('token')==='ui-token',socket=>{socket.onMessage(message=>requests.push(JSON.parse(String(message)).cmd));const timer=setInterval(()=>socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy})),200);socket.onClose(()=>clearInterval(timer));});
  const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));page.on('dialog',d=>{errors.push('Platform dialog');d.dismiss();});
  await page.goto(origin+'/dashboard');
  await page.getByRole('button',{name:/Choose tab/}).click();
  await page.getByRole('button',{name:'Actions',exact:true}).click();
  const actionButton=cmd=>page.locator('button').filter({has:page.locator(`span[title="${cmd}"]`)}).last();
  await actionButton('ResetTClock').click();
  const dialog=page.getByRole('alertdialog',{name:'Confirm reset'});await dialog.waitFor();
  assert.equal(requests.length,0,'Opening the modal sends no command');
  const values=await dialog.evaluate(el=>{const css=e=>{const s=getComputedStyle(e);return {font:s.fontFamily,background:s.backgroundColor,color:s.color,appearance:s.appearance,minHeight:s.minHeight,borderRadius:s.borderRadius}};return {dialog:css(el),heading:css(el.querySelector('h2,h3')),cancel:css([...el.querySelectorAll('button')].find(b=>b.textContent==='Cancel')),confirm:css([...el.querySelectorAll('button')].find(b=>b.textContent==='Confirm reset')),shellFont:getComputedStyle(document.querySelector('.gs26-dashboard-shell')).fontFamily,width:el.getBoundingClientRect().width,viewport:innerWidth};});
  results.push({theme,viewportWidth:width,...values});
  if(!process.env.GS_MODAL_EXPECT_BASELINE){
   assert.equal(values.dialog.font,values.shellFont,'Modal uses the actual application font');assert.equal(values.cancel.font,values.shellFont);assert.equal(values.heading.font,values.shellFont);
   assert.equal(values.cancel.appearance,'none');
   assert.equal(await page.evaluate(()=>{const button=document.createElement('button');button.textContent='Unrelated scope probe';document.body.appendChild(button);const value=getComputedStyle(button).appearance;button.remove();return value;}),'auto','Modal CSS does not restyle unrelated controls');assert.equal(values.confirm.background,'rgb(158, 38, 54)');assert.equal(values.confirm.color,'rgb(255, 255, 255)');assert.equal(values.cancel.minHeight,'44px');assert.equal(values.cancel.borderRadius,'8px');assert(values.width<=values.viewport,'Dialog fits narrow viewport');
   await dialog.getByRole('button',{name:'Cancel',exact:true}).focus();await page.keyboard.press('Tab');await page.keyboard.press('Shift+Tab');
   const outline=await dialog.getByRole('button',{name:'Cancel',exact:true}).evaluate(e=>getComputedStyle(e).outlineWidth);assert.equal(outline,'3px');
  }
  await page.screenshot({path:`${process.env.GS_MODAL_OUTPUT||'/tmp'}/modal-${theme}-${width}.png`,fullPage:true});
  await dialog.getByRole('button',{name:'Cancel',exact:true}).click();assert.equal(requests.length,0,'Cancel sends no command');assert.deepEqual(errors,[]);await context.close();
 }
 if(!process.env.GS_MODAL_EXPECT_BASELINE)assert.notEqual(results[0].cancel.background,results[2].cancel.background,'Modal controls follow the selected dark/light theme');
 fs.writeFileSync(process.env.GS_MODAL_RESULT||'/tmp/modal-computed.json',JSON.stringify(results,null,2));
 console.log('Modal computed-style browser cases: 4 PASS');
} finally {await browser.close();await new Promise(resolve=>server.close(resolve));}
