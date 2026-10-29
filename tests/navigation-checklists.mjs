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
      if(pathname.endsWith('/check')) {const item=lists.checklists.find(c=>c.id===request.checklist_id).items.find(i=>i.id===request.item_id);item.checked=request.checked;item.completed_by=request.checked?'ui-test':null;item.completion_source=request.checked?(item.complete_on?'manual_override':'operator'):null;}
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
    const next=JSON.parse(body);if(next.revision!==broadcastState.revision){res.statusCode=409;res.end('Broadcast changed; reload before saving');return;}
    next.revision++;
    if(next.replay && !next.replay.id){next.replay.id='fixture-replay';next.replay.started_at_ms=Date.now();next.replay.source_end_ms=Date.now()-10000;}
    broadcastState=next;res.setHeader('Content-Type','application/json');res.end(JSON.stringify(next));return;
  }
  if(pathname.startsWith('/api/')){
    res.setHeader('Content-Type','application/json');
    const data=pathname==='/api/auth/session'?session:pathname==='/api/layout'?dashboardLayout:pathname==='/api/fill_targets'?{version:1,fill_source:fillSource,nitrogen:{target_mass_kg:10,target_pressure_psi:100},nitrous:{target_mass_kg:20,target_pressure_psi:200}}:pathname==='/api/live_streams'?{...example('live-streams'),broadcast:broadcastState,can_manage_stream:session.roles.some(role=>['stream_master','stream_admin','admin'].includes(role)),program_url:'',can_preview_live:session.roles.some(role=>['stream_master','stream_admin','admin'].includes(role)),streams:[{id:'pad-wide',label:'Pad wide',url:'/test-camera?ticket=fixture',online:cameraOnline,kind:'webrtc'},{id:'side',label:'Side camera',url:'/test-camera?ticket=side',online:true,kind:'webrtc'}]}:pathname==='/api/vehicle'?{}:[];
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
 assert.equal(await page.locator('.gs26-navigation-rail').evaluate(e=>e.getBoundingClientRect().width),64);
 await page.reload();await page.getByRole('button',{name:'Expand sidebar',exact:true}).click();
 assert.equal(await page.getByText('Station connected',{exact:true}).count(),0,'sidebar excludes redundant connection label');
 const toggle=await page.getByRole('button',{name:'Collapse sidebar',exact:true}).boundingBox();
 const railBounds=await page.locator('.gs26-navigation-rail').boundingBox();
 assert(toggle.x-railBounds.x<12,'collapse control is left aligned');
 const icons=await rail.locator('a svg path').evaluateAll(items=>items.map(i=>i.getAttribute('d')));assert.equal(new Set(icons).size,icons.length,'page icons are distinct');
 await page.setViewportSize({width:390,height:844});
 const content=page.locator('.gs26-page-content');
 const beforeWidth=await content.evaluate(e=>e.getBoundingClientRect().width);
 await page.getByRole('button',{name:'Hide navigation sidebar'}).click();
 await page.getByRole('button',{name:'Show navigation sidebar'}).click();
 const bounds=await page.locator('.gs26-navigation-rail, .gs26-page-content').evaluateAll(nodes=>nodes.map(e=>{const r=e.getBoundingClientRect();return {left:r.left,right:r.right,width:r.width}}));
 assert(bounds[0].right<=bounds[1].left,'sidebar occupies its own column');
 assert(await page.locator('.gs26-header-row').evaluate(e=>e.getBoundingClientRect().height<=52),'mobile header stays in one compact row');
 assert.equal(await page.locator('.gs26-navigation-mark').count(),0,'no redundant S badge');
 await page.screenshot({path:'/tmp/gs-mobile-sidebar-reflow.png',fullPage:true});
 await page.getByRole('button',{name:'Hide navigation sidebar'}).click();
 assert(await content.evaluate(e=>e.getBoundingClientRect().width)>bounds[1].width,'hiding the sidebar restores page width');
 await page.setViewportSize({width:844,height:1000});
 for(const name of ['Show navigation sidebar','Hide navigation sidebar']) {
  await page.getByRole('button',{name}).click();
  const b=await page.locator('.gs26-navigation-rail').boundingBox();
  assert(b&&b.width>=60,'tablet collapsed and expanded navigation remain available');
  assert(b.x+b.width<=await content.evaluate(e=>e.getBoundingClientRect().left),'tablet navigation never overlays content');
 }
 await page.setViewportSize({width:1440,height:1000});
 assert.equal(await page.getByRole('button',{name:/^Settings$/i}).count(),0,'sidebar has only one Settings entry'); await page.getByRole('link',{name:'Settings',exact:true}).click();
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
 // Wait for the second client's accepted change before opening a fresh template.
 await page.waitForFunction(()=>document.querySelector('.gs26-checklist-item input')?.checked===true);
 // Bind a real checklist template to a sequence and observe asynchronous server completion.
 await page.getByRole('button',{name:'Manage checklists'}).click();
 await page.getByLabel('Complete after',{exact:true}).first().selectOption('valve_self_test');
 for(const width of [320,390,844,1440]){
  await page.setViewportSize({width,height:900});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'automation editor fits viewport');
 }
 await page.getByRole('button',{name:'Save changes',exact:true}).click();
 await page.getByText('Checklists saved',{exact:true}).waitFor();
 assert.equal(lists.checklists[0].items[0].complete_on,'valve_self_test');
 const item=lists.checklists[0].items[0];
 item.checked=true;item.completed_by='automated:valve_self_test';item.completed_at_ms=Date.now();item.completion_source='automated_sequence';lists.revision++;
 await page.getByText('Test passed · saved automatically',{exact:true}).waitFor();
 await second.getByText('Test passed · saved automatically',{exact:true}).waitFor();
 await page.getByRole('checkbox',{name:/^Pressure checked/}).uncheck();
 await page.reload();
 await page.getByRole('checkbox',{name:/^Pressure checked/}).waitFor();
 assert.equal(await page.getByRole('checkbox',{name:/^Pressure checked/}).isChecked(),false,'reload preserves operator clear');
 await page.getByRole('checkbox',{name:/^Pressure checked/}).check();
 await page.getByText('Manual override by ui-test',{exact:true}).waitFor();
 for(const width of [320,390,844,1440]){
  await page.setViewportSize({width,height:900});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'checklists fit viewport');
  await page.screenshot({path:`/tmp/gs-checklists-${width}.png`,fullPage:true});
 }
 for(const socket of sockets)socket.send(JSON.stringify({ty:'Warning',data:{timestamp_ms:Date.now(),message:'Simulated warning for UI validation'}}));
 await page.getByRole('alert').filter({hasText:'Simulated warning for UI validation'}).waitFor();
 const dashboardFont=await page.locator('.gs26-dashboard-shell').evaluate(e=>getComputedStyle(e).fontFamily);
 const warningToast=page.locator('.gs26-warning-toast');
 assert.equal(await warningToast.evaluate(e=>getComputedStyle(e).fontFamily),dashboardFont,'warning popup matches dashboard typography');
 for(const button of await warningToast.locator('button').all()) assert.equal(await button.evaluate(e=>getComputedStyle(e).fontFamily),dashboardFont,'warning popup controls inherit dashboard font');

 assert.equal(await page.locator('.gs26-dashboard-shell').evaluate(e=>getComputedStyle(e).boxShadow),'none','page frame does not blink');
 assert.equal(await page.locator('.gs26-status-count[data-alert=warning]').evaluate(e=>getComputedStyle(e).animationName),'gs26-button-warning');
 const pulseColors=await page.locator('.gs26-status-count[data-alert=warning]').evaluate(element=>{
   const animation=element.getAnimations()[0];animation.pause();
   const colors=[0,160,320,480,640,800].map(time=>{animation.currentTime=time;return getComputedStyle(element).backgroundColor;});
   animation.play();return colors;
 });
 assert(new Set(pulseColors).size>3,'warning pulse must interpolate smoothly rather than jump between two colors');
 await page.getByRole('button',{name:'Dismiss warning notification'}).click();
 assert.deepEqual(errors,[]);console.log('PASS: exclusive navigation settings, sidebar collapse persistence, distinct icons, shared checklist updates, admin templates, stale-edit protection, mobile layouts, button-only warning pulses and notification');
 await context.close();
 // Fresh profiles prevent the administrator's persisted session from masking grants.
 const originalSession=structuredClone(session);
 for(const entry of [
  {name:'anonymous',authenticated:false,anonymous:true,roles:[],permissions:{view_data:true}},
  {name:'publisher',authenticated:true,anonymous:false,roles:['operator'],permissions:{view_data:true,camera_publish:true}},
  {name:'voice',authenticated:true,anonymous:false,roles:['operator'],permissions:{view_data:true,voice_transmit:true}},
  {name:'manager',authenticated:true,anonymous:false,roles:['stream_master'],permissions:{view_data:true}},
 ]){
  Object.assign(session,originalSession,entry,{username:entry.authenticated?entry.name:null});
  const profile=await browser.newContext({viewport:{width:390,height:844}});
  await profile.addInitScript(({session,origin})=>{
   if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};
   if(session.authenticated)localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));
  },{session,origin});
  await profile.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
  await profile.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(()=>{}));
  const mobile=await profile.newPage();await mobile.goto(origin+'/dashboard');
  const menu=mobile.getByRole('button',{name:'Menu',exact:true});await menu.waitFor();
  const authButton=mobile.locator('.gs26-utility-menu button').filter({hasText:/SIGN (IN|OUT)/});
  await authButton.waitFor({state:'attached'});
  assert.equal(await authButton.count(),1,'account action stays in Tools');
  assert.equal(await mobile.getByText(/queue drops/i).count(),0,'queue counters are kept out of the regular UI');
  assert.equal(await authButton.isVisible(),false,'account action follows the closed Tools/menu state');
  await menu.click();const panel=mobile.locator('#gs26-mobile-menu');
  assert(await authButton.isVisible(),'account action is accessible in the expanded menu');
  assert((await panel.boundingBox()).height>700,'menu uses available screen height: '+JSON.stringify(await panel.evaluate(e=>({rect:e.getBoundingClientRect().toJSON(),position:getComputedStyle(e).position,display:getComputedStyle(e).display,transform:getComputedStyle(e.parentElement).transform}))));
  const tabs=mobile.getByRole('navigation',{name:'Mobile navigation'});
  assert(await tabs.getByRole('button').count()>5,'menu contains useful page navigation');
  assert.equal(await tabs.getByRole('button',{name:'Voice Chat',exact:true}).count(),entry.name==='voice'?1:0);
  assert.equal(await tabs.getByRole('button',{name:'Cameras & Recordings',exact:true}).count(),entry.name==='manager'?1:0);
  assert.equal(await tabs.getByRole('button',{name:'Share camera',exact:true}).count(),entry.name==='publisher'?1:0);
  if(entry.name==='manager'){
   await tabs.getByRole('button',{name:'Stream Manager',exact:true}).click();
   const designer=mobile.getByRole('region',{name:'Camera layout designer'});
   await designer.waitFor();
   await designer.getByLabel('Camera layout',{exact:true}).selectOption('main-side');
   await designer.getByLabel('Main camera width',{exact:true}).fill('70');
   await designer.getByRole('button',{name:'Move Pad wide later',exact:true}).click();
   await designer.getByRole('button',{name:'Save camera layout',exact:true}).click();
   await designer.getByText('Layout saved',{exact:true}).waitFor();
   assert.equal(broadcastState.layout,'tiles');assert.equal(broadcastState.tiles[0].stream_id,'side');assert.equal(broadcastState.tiles[0].width,70);
   broadcastState.revision++;
   await designer.getByRole('button',{name:'Save camera layout',exact:true}).click();
   await designer.getByText(/reload before saving/).waitFor();
   session.roles=[];
   await mobile.getByRole('button',{name:'Menu',exact:true}).click();
   await mobile.getByRole('navigation',{name:'Mobile navigation'}).getByRole('button',{name:'Cameras & Recordings',exact:true}).waitFor({state:'detached',timeout:12000});
  }
  if(entry.name==='anonymous'){
   await authButton.click();
   await mobile.waitForURL(/login/);
  }
  await profile.close();
 }
 Object.assign(session,originalSession);
 console.log('PASS: mobile menu dimensions, signed-out login through Tools, independent publisher/voice/manager tabs, saved tile ordering/resizing, stale revisions and live revocation');
}finally{await browser.close();await new Promise(r=>server.close(r));}
