// Build with `dx build --platform web`, then run with Node and Playwright.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root=path.resolve('target/dx/groundstation_frontend/debug/web/public');
const example=name=>JSON.parse(fs.readFileSync(`docs/api-examples/${name}.json`,'utf8'));
const session={...example('auth-session.anonymous'),authenticated:true,anonymous:false,username:'ui-test',roles:['admin'],permissions:{view_data:true,send_commands:true}};
let lists={revision:0,enforce_before_launch:false,checklists:[{id:'setup',title:'Setup',required_before_launch:true,items:[{id:'pressure',text:'Pressure checked',checked:false,completed_by:null,completed_at_ms:null}]},{id:'recovery',title:'Recovery',required_before_launch:false,items:[{id:'recover',text:'Recovery area clear',checked:false,completed_by:null,completed_at_ms:null}]}]};
let cameraOnline=true;
let broadcastState=example('live-streams').broadcast;
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
  if(pathname==='/api/gse/self-test-confirmation'){
    let body='';for await(const data of req)body+=data;gseStatus.dry_self_test_confirmed=JSON.parse(body).confirmed;
    res.setHeader('Content-Type','application/json');res.end('{}');return;
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

const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{
  if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};
  localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));
 },{session,origin});
 const sockets=new Set();
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 await context.routeWebSocket(url=>url.pathname==='/ws',s=>{sockets.add(s);s.onClose(()=>sockets.delete(s));s.onMessage(()=>{});});
 const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(origin+'/dashboard');
 const rail=page.getByRole('navigation',{name:'Mission navigation'});await rail.waitFor();
 assert.equal(await page.getByRole('button',{name:/Choose tab/}).isVisible(),false,'desktop sidebar excludes tab picker');
 await page.getByRole('button',{name:'Expand sidebar',exact:true}).click();
 await page.getByRole('button',{name:'Collapse sidebar',exact:true}).click();
 assert.equal(await page.locator('.gs26-navigation-rail').evaluate(e=>e.getBoundingClientRect().width),72);
 await page.reload();await page.getByRole('button',{name:'Expand sidebar',exact:true}).click();
 const icons=await rail.locator('a svg path').evaluateAll(items=>items.map(i=>i.getAttribute('d')));assert.equal(new Set(icons).size,icons.length,'page icons are distinct');
 await page.getByRole('button',{name:/^Settings$/i}).click();
 await page.getByLabel('Navigation',{exact:true}).selectOption('tabs');
 await page.getByRole('button',{name:'Close',exact:true}).click();
 assert.equal(await rail.count(),0);await page.getByRole('button',{name:/Choose tab/}).waitFor();
 await page.reload();await page.getByRole('button',{name:/Choose tab/}).click();
 await page.getByRole('button',{name:'Checklists',exact:true}).click();
 await page.getByRole('checkbox',{name:'Pressure checked'}).check();
 await page.getByText('Completed by ui-test',{exact:true}).waitFor();assert(lists.checklists[0].items[0].checked);
 const second=await context.newPage();await second.goto(origin+'/dashboard');await second.getByRole('checkbox',{name:'Pressure checked'}).waitFor();assert(await second.getByRole('checkbox',{name:'Pressure checked'}).isChecked());
 await second.getByRole('checkbox',{name:'Pressure checked'}).uncheck();
 await page.waitForFunction(()=>!document.querySelector('.gs26-checklist-item input').checked);
 await page.getByRole('button',{name:'Manage checklists'}).click();
 await page.getByRole('button',{name:'Add checklist',exact:true}).click();
 const editor=page.getByRole('region',{name:'Checklist administration'});
 await page.getByRole('textbox',{name:'Checklist name'}).last().fill('Pad clearance');
 await page.getByRole('button',{name:'Add item',exact:true}).last().click();
 await page.getByRole('textbox',{name:'Checklist item',exact:true}).last().fill('Pad clear');
 await page.getByRole('checkbox',{name:'Require selected checklists before launch'}).check();
 await page.getByRole('button',{name:'Save changes',exact:true}).click();await page.getByText('Checklists saved',{exact:true}).waitFor();assert.equal(lists.checklists.length,3);assert(lists.enforce_before_launch);
 // An in-progress admin edit cannot silently overwrite a different operator's check.
 await page.getByRole('button',{name:'Manage checklists'}).click();
 await second.getByRole('checkbox',{name:'Pressure checked'}).check();
 await page.getByRole('button',{name:'Save changes',exact:true}).click();await page.getByText(/Checklists changed on another device; reload before saving/).waitFor();
 await page.getByRole('button',{name:'Cancel',exact:true}).click();
 for(const width of [320,390,844,1440]){
  await page.setViewportSize({width,height:900});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'checklists fit viewport');
  await page.screenshot({path:`/tmp/gs-checklists-${width}.png`,fullPage:true});
 }
 for(const socket of sockets)socket.send(JSON.stringify({ty:'Warning',data:{timestamp_ms:Date.now(),message:'Simulated warning for UI validation'}}));
 await page.getByRole('alert').filter({hasText:'Simulated warning for UI validation'}).waitFor();
 assert.equal(await page.locator('.gs26-dashboard-shell').evaluate(e=>getComputedStyle(e).boxShadow),'none','page frame does not blink');
 assert.equal(await page.locator('.gs26-status-count[data-alert=warning]').evaluate(e=>getComputedStyle(e).animationName),'gs26-button-warning');
 await page.getByRole('button',{name:'Dismiss warning notification'}).click();
 assert.deepEqual(errors,[]);console.log('PASS: exclusive navigation settings, sidebar collapse persistence, distinct icons, shared checklist updates, admin templates, stale-edit protection, mobile layouts, button-only warning pulses and notification');
 await context.close();
}finally{await browser.close();await new Promise(r=>server.close(r));}
