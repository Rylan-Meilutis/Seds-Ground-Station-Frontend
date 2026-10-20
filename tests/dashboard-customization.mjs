// Build with `dx build --platform web`, then run with Node and Playwright.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root=path.resolve('target/dx/groundstation_frontend/debug/web/public');
const example=name=>JSON.parse(fs.readFileSync(`docs/api-examples/${name}.json`,'utf8'));
const session={...example('auth-session.anonymous'),authenticated:true,anonymous:false,username:'ui-test',roles:['stream_master'],permissions:{view_data:true,send_commands:true}};
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
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY || (process.platform==='darwin' && fs.existsSync('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome') ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' : undefined)});
try{
  const context=await browser.newContext({viewport:{width:1440,height:1000}});
  await context.addInitScript(({session,origin})=>{localStorage.setItem('navigation-style','tabs');if(!localStorage.getItem('auth_session_v1'))localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
  const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.goto(origin);
  await page.waitForTimeout(1000);
  if (!(await page.getByRole('button',{name:/Choose tab/}).count())) console.error('Startup:', await page.locator('body').innerText(), errors);
  const choose=()=>page.getByRole('button',{name:/Choose tab/}).click();
  assert.equal(await page.getByRole('navigation',{name:'Mission navigation'}).count(),0,'tab preference excludes sidebar');
  assert.equal(await page.getByRole('button',{name:'Admin',exact:true}).count(),0,'stream manager is not a user administrator');
  assert.equal(await page.getByRole('button',{name:'Aim antenna',exact:true}).count(),0,'aiming is not a main-screen action');
  await page.getByRole('link',{name:'Settings',exact:true}).click();
  await page.getByRole('button',{name:'Aim antenna',exact:true}).click();
  await page.getByRole('heading',{name:'Find your alignment',exact:true}).waitFor();
  assert.equal(await page.locator('.gs26-dashboard-card').count(),0,'aiming unmounts ordinary data cards');
  assert.equal(await page.locator('#gs26-media, #gs26-crew-voice').count(),0,'aiming unmounts media views');
  await page.getByRole('button',{name:'Exit aiming',exact:true}).click();
  await page.getByRole('button',{name:'Aim antenna',exact:true}).waitFor();
  await page.getByRole('button',{name:'Close',exact:true}).click();
  await page.getByRole('button',{name:'Customize dashboard',exact:true}).waitFor();
  await page.screenshot({path:'/tmp/gs-mission-overview-desktop.png',fullPage:true});
  assert.equal(await page.locator('.antenna-aim').count(),0,'exit restores the dashboard');
  await choose();await page.getByRole('button',{name:/^Detailed/}).click();
  await page.getByRole('navigation',{name:'Information categories'}).waitFor();
  await page.getByRole('button',{name:'Arrange panels',exact:true}).click();
  const firstPanel=await page.locator('.gs26-panel-editor-row').first().innerText();
  await page.getByRole('button',{name:'Move Board timing up',exact:true}).click();
  assert.match(await page.locator('.gs26-panel-editor-row').first().innerText(),/Board timing/);
  await page.getByRole('button',{name:'Done arranging',exact:true}).click();
  assert.equal(await page.locator('.gs26-info-panel').first().getAttribute('data-panel-id'),'timing');
  await page.reload();await page.getByRole('navigation',{name:'Information categories'}).waitFor();
  assert.equal(await page.locator('.gs26-info-panel').first().getAttribute('data-panel-id'),'timing','panel order persists across reload');
  await page.getByRole('button',{name:'Network',exact:true}).click();
  await page.getByRole('heading',{name:'Network health',exact:true}).waitFor();
  for(const width of [320,390,844,1440]) {
    await page.setViewportSize({width,height:900});
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Detailed Info has no horizontal overflow');
    await page.screenshot({path:`/tmp/gs-ui-detailed-${width}.png`,fullPage:true});
  }
  await page.setViewportSize({width:1440,height:1000});
  gseStatus={phase:'idle',message:'Ready',dry_self_test_confirmed:false,configuration_error:null,
    qualification:{self_test:true,nitrogen_test:true,fill_target:false,fault:null},topology_stable:true,
    request_gate:{hitl_mode:false,flight_state:'Idle',prelaunch:true,button_interlock_satisfied:true},launch_queued:false,qd:'NotStarted'};
  await choose();await page.getByRole('button',{name:'Actions',exact:true}).click();
  await page.getByText('Operation checks',{exact:true}).waitFor();
  // This isolated server deliberately has no command WebSocket. A click must
  // report failure rather than claiming dispatch or retaining a success pulse.
  await page.getByRole('button',{name:'Abort',exact:true}).last().click();
  await page.getByRole('alert').filter({hasText:'Command not sent:'}).waitFor();
  gseStatus.physical_panel={key_enabled:true,software_buttons_enabled:true,controls:[
    {physical:'Pilot',action:'Valve test',command:'ValveSelfTest',enabled:true,actuated:false,blink:'slow',brightness:.5},
    {physical:'Nitrous',action:'Start fill',command:'StartFill',enabled:false,actuated:null,blink:'none',brightness:.1}]};
  await page.getByText('Physical panel · Enabled',{exact:true}).click();
  const panel=page.locator('.gs26-physical-panel');
  await panel.getByText('Valve test',{exact:true}).waitFor();
  await panel.getByText('● Blinking',{exact:true}).waitFor();
  await panel.getByText('● Locked',{exact:true}).waitFor();
  gseStatus.physical_panel.controls[0].blink='none';gseStatus.physical_panel.controls[0].actuated=true;
  await panel.getByText('● Active',{exact:true}).waitFor();
  gseUnavailable=true;await page.getByText('Sequence and panel status unavailable',{exact:true}).waitFor();
  await panel.waitFor({state:'detached'});
  gseUnavailable=false;await page.getByText('Operation checks',{exact:true}).waitFor();
  await page.getByRole('button',{name:'About automatic zero',exact:true}).click();
  const zeroHelp=page.getByRole('dialog',{name:'automatic zero',exact:true});
  await zeroHelp.waitFor();
  assert.match(await zeroHelp.innerText(),/200 fresh samples/);
  await zeroHelp.getByRole('button',{name:'Close help',exact:true}).click();
  await zeroHelp.waitFor({state:'detached'});
  await page.getByRole('button',{name:'About empty-tank pressure limit',exact:true}).click();
  const pressureHelp=page.getByRole('dialog',{name:'empty-tank pressure limit',exact:true});
  await pressureHelp.waitFor();
  assert.match(await pressureHelp.innerText(),/not a calibration offset/);
  await pressureHelp.getByRole('button',{name:'Close help',exact:true}).press('Escape');
  await pressureHelp.waitFor({state:'detached'});
  await page.getByRole('button',{name:'Reset operation checks',exact:true}).click();
  await page.getByRole('dialog',{name:'Reset operation checks'}).getByRole('button',{name:'Cancel',exact:true}).click();
  assert.equal(qualificationResets,0,'cancelling must preserve saved checks');
  await page.getByRole('button',{name:'Reset operation checks',exact:true}).click();
  await page.getByRole('button',{name:'Confirm reset',exact:true}).click();
  await page.getByText('Operation checks reset',{exact:true}).waitFor();assert.equal(qualificationResets,1);
  gseStatus.launch_queued=true;gseStatus.qd='Settling { remaining_ms: 3000 }';
  await page.locator('.gs26-launch-queued').waitFor();
  gseStatus.launch_queued=false;await page.locator('.gs26-launch-queued').waitFor({state:'detached'});
  gseStatus.request_gate.flight_state='Launch';
  await page.waitForFunction(()=>[...document.querySelectorAll('button')].some(b=>b.textContent==='Reset operation checks'&&b.disabled));
  gseStatus.request_gate.flight_state='Idle';
  await choose();await page.getByRole('button',{name:'Dashboard',exact:true}).click();

  assert.equal(await page.locator('model-viewer, gs-vehicle-viewer').count(),0,'Dashboard has no 3D model');
  assert.equal((await page.locator('.gs26-title-tab-toggle span').first().innerText()),'Dashboard');
  await choose();await page.getByRole('button',{name:'Mission Live',exact:true}).click();
  assert.equal(await page.locator('.gs26-program-header, .gs26-program-editor').count(),0,'Mission Live is stream only');
  assert.equal(await page.getByText('Ground setup',{exact:true}).count(),0,'no ground controls in stream');
  assert.equal(await page.locator('#ground-checklist-panel').count(),0,'checklist stays collapsed');
  assert.equal(await page.locator('.gs26-status-shell, .gs26-navigation-rail').count(),0,'viewer is dedicated with no dashboard status');
  assert.equal(await page.getByText('GroundStation disconnected. Reconnecting automatically; displayed data may be stale.',{exact:true}).count(),0,'viewer has no mission warnings');
  await page.getByRole('button',{name:'Exit stream mode',exact:true}).click();
  await choose();await page.getByRole('button',{name:'Dashboard',exact:true}).click();

  assert.equal(await page.getByRole('heading',{name:'Dashboard',exact:true}).count(),0,'no duplicate dashboard heading');
  await choose();
  await page.getByRole('button',{name:'Stream Manager',exact:true}).click();
  await page.getByRole('heading',{name:'Broadcast studio'}).waitFor();
  await page.getByRole('heading',{name:'Instant replay'}).waitFor();
  await page.getByLabel('Last seconds',{exact:true}).fill('20');
  await page.getByLabel('Playback',{exact:true}).selectOption('0.25');
  await page.getByRole('button',{name:'Play replay',exact:true}).click();
  await page.getByText('Replay on air',{exact:true}).waitFor();
  await page.screenshot({path:'/tmp/gs-stream-manager-replay.png',fullPage:true});
  assert.equal(broadcastState.replay.duration_seconds,20);assert.equal(broadcastState.replay.speed,0.25);
  await page.getByRole('button',{name:'Return to live',exact:true}).click();
  await page.getByText('Live program restored',{exact:true}).waitFor();assert.equal(broadcastState.replay,null);

  assert.equal(await page.locator('.gs26-status-shell').count(),0,'operator is dedicated with no dashboard status');
  await page.getByRole('button',{name:'Exit stream mode',exact:true}).click();
  await page.getByRole('button',{name:'Customize dashboard',exact:true}).click();
  await page.getByRole('button',{name:'Pin to top bar',exact:true}).first().click();
  await page.locator('[aria-label="Pinned telemetry"]').getByText('50 kg loadcell',{exact:true}).waitFor();
  await page.getByLabel('Title',{exact:true}).first().fill('Pinned load');
  await page.getByRole('button',{name:'Add camera',exact:true}).click();
  await page.getByLabel(/^Camera source/).selectOption('pad-wide');
  await page.getByLabel(/^Card width/).last().selectOption('full');
  await page.getByRole('button',{name:'Done',exact:true}).click();
  await page.frameLocator('iframe[title="Live camera"]').getByText('Live camera fixture').waitFor();
  assert.equal(await page.locator('.gs26-dashboard-card').last().getAttribute('data-width'),'full');
  cameraOnline=false; await page.getByText('Camera offline — waiting for pad-wide').waitFor();
  assert.equal(await page.locator('iframe[title="Live camera"]').count(),0);
  cameraOnline=true; await page.frameLocator('iframe[title="Live camera"]').getByText('Live camera fixture').waitFor();
  await page.locator('[aria-label="Pinned telemetry"]').getByText('Pinned load',{exact:true}).waitFor();
  await choose();await page.getByRole('button',{name:'Voice Chat',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('#gs26-crew-voice')?.contentWindow.received?.some(m=>m.token==='ui-token'&&m.visible));
  await page.frameLocator('#gs26-crew-voice').getByText('Media tool fixture').waitFor({state:'visible'});
  assert((await page.locator('#gs26-crew-voice').boundingBox()).height>300,'voice fills the tab');
  assert.deepEqual((await page.locator('#gs26-crew-voice').getAttribute('sandbox')).split(/\s+/).sort(),
    ['allow-downloads','allow-forms','allow-same-origin','allow-scripts']);
  assert.equal(context.pages().length,1,'opening crew voice stays inside the application');
  assert(await page.evaluate(()=>document.querySelector('#gs26-crew-voice').contentWindow.received.some(m=>m.theme?.['--gs26-app-background'])),'embedded voice receives the selected app theme');
  await page.evaluate(()=>document.querySelector('#gs26-crew-voice').contentWindow.instanceMarker=42);
  await choose();await page.getByRole('button',{name:'Dashboard',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('#gs26-crew-voice')?.contentWindow.received?.some(m=>m.visible===false));
  assert.equal(await page.evaluate(()=>document.querySelector('#gs26-crew-voice').contentWindow.instanceMarker),42,'media document persists when hidden');
  assert.equal(await page.locator('#gs26-crew-voice').isVisible(),false);
  await choose();await page.getByRole('button',{name:'Cameras & Recordings',exact:true}).click();
  await page.frameLocator('#gs26-media').getByText('Media tool fixture').waitFor({state:'visible'});
  assert((await page.locator('#gs26-media').boundingBox()).height>300,'cameras fill the tab');
  await page.evaluate(()=>document.querySelector('#gs26-media').contentWindow.instanceMarker=43);
  await choose();await page.getByRole('button',{name:'Voice Chat',exact:true}).click();
  await page.frameLocator('#gs26-crew-voice').getByText('Media tool fixture').waitFor({state:'visible'});
  assert.equal(await page.locator('#gs26-media').isVisible(),false);
  await choose();await page.getByRole('button',{name:'Cameras & Recordings',exact:true}).click();
  await page.frameLocator('#gs26-media').getByText('Media tool fixture').waitFor({state:'visible'});
  assert.equal(await page.evaluate(()=>document.querySelector('#gs26-media').contentWindow.instanceMarker),43,'camera document persists when reopened');
  await choose();await page.getByRole('button',{name:'Stream Manager',exact:true}).click();
  await page.getByRole('heading',{name:'Broadcast studio',exact:true}).waitFor();
  assert.equal(await page.evaluate(()=>document.querySelector('#gs26-media').contentWindow.instanceMarker),43,'camera source survives entering exclusive operator mode');
  assert.equal(await page.evaluate(()=>document.querySelector('#gs26-crew-voice').contentWindow.instanceMarker),42,'voice session survives exclusive operator mode');
  assert.equal(await page.locator('#gs26-media').isVisible(),false);
  await page.getByRole('button',{name:'Camera recordings',exact:true}).click();
  await page.frameLocator('#gs26-media').getByText('Media tool fixture').waitFor({state:'visible'});
  assert.equal(await page.evaluate(()=>document.querySelector('#gs26-media').contentWindow.instanceMarker),43,'operator camera management reuses the active source document');
  await page.getByRole('button',{name:'Back to stream controls',exact:true}).click();
  await page.getByRole('button',{name:'Exit stream mode',exact:true}).click();

  await choose();await page.getByRole('button',{name:'Dashboard',exact:true}).click();
  await choose();await page.getByRole('button',{name:'Organize tabs',exact:true}).click();
  await page.getByRole('checkbox',{name:'Voice Chat',exact:true}).uncheck();
  await page.getByRole('button',{name:'Done editing',exact:true}).click();
  await choose();assert.equal(await page.getByRole('button',{name:'Voice Chat',exact:true}).count(),0);
  await page.getByRole('button',{name:'Close tab picker',exact:true}).click();
  await page.reload();
  await page.frameLocator('iframe[title="Live camera"]').getByText('Live camera fixture').waitFor();
  await page.locator('[aria-label="Pinned telemetry"]').getByText('Pinned load',{exact:true}).waitFor();
  await choose();assert.equal(await page.getByRole('button',{name:'Voice Chat',exact:true}).count(),0);
  await page.getByRole('button',{name:'Close tab picker',exact:true}).click();
  await page.locator('iframe[title="Live camera"]').scrollIntoViewIfNeeded();
  await page.screenshot({path:'/tmp/gs-dashboard-desktop.png',fullPage:true});
  await choose();
  await page.setViewportSize({width:390,height:844});
  assert.equal(await page.locator('.gs26-header-row button, .gs26-header-secondary button').evaluateAll(buttons=>buttons.filter(button=>{const box=button.getBoundingClientRect();return box.width>0&&box.height>0&&box.height<44}).length),0,'mobile header controls retain 44 px touch targets');
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no horizontal page scrolling on mobile');
  const nav=page.locator('#dashboard-tab-picker');assert(await nav.evaluate(el=>el.scrollWidth<=el.clientWidth),'tab picker does not scroll sideways');
  await page.getByRole('button',{name:'Close tab picker',exact:true}).click();
  await choose();
  const titleToggle=page.locator('.gs26-title-tab-toggle').first();
  assert((await titleToggle.boundingBox()).height>=44,'mobile title has a full touch target');
  await page.screenshot({path:'/tmp/gs-dashboard-mobile-menu.png',fullPage:true});
  await page.getByRole('button',{name:'Close tab picker',exact:true}).click();
  await page.locator('iframe[title="Live camera"]').scrollIntoViewIfNeeded();
  await page.screenshot({path:'/tmp/gs-dashboard-mobile.png',fullPage:true});
  await page.setViewportSize({width:320,height:568});
  await choose();await page.getByRole('button',{name:'Data Export',exact:true}).click();
  const exportPanel=page.locator('.gs26-export-scroll');
  assert(await exportPanel.evaluate(el=>el.scrollHeight>el.clientHeight),'export content exceeds viewport');
  await page.getByText('Legacy CSV / clock settings',{exact:true}).scrollIntoViewIfNeeded();
  assert(await exportPanel.evaluate(el=>el.scrollTop>0),'export scroll reaches lower controls');
  await page.screenshot({path:'/tmp/gs-export-mobile.png',fullPage:true});
  await page.setViewportSize({width:390,height:844});
  session.roles=['admin'];await page.reload();
  await page.getByRole('button',{name:'Admin',exact:true}).click();
  await page.getByRole('heading',{name:'Administration',exact:true}).waitFor();
  await page.getByLabel('Transmit voice',{exact:true}).check();
  await page.getByRole('button',{name:'Save test-admin',exact:true}).click();
  await page.getByText('Account saved',{exact:true}).waitFor();
  assert.equal(adminAccounts[0].permissions.voice_transmit,true);
  assert.equal(adminAccounts[0].permissions.send_commands,false,'voice/admin roles never imply hardware commands');
  await page.getByLabel('Disable account',{exact:true}).check();
  await page.getByRole('button',{name:'Save test-admin',exact:true}).click();
  await page.getByText(/Keep at least one enabled/).waitFor();
  await page.getByLabel('Disable account',{exact:true}).uncheck();
  await page.getByLabel('Username',{exact:true}).fill('new-viewer');
  await page.getByLabel('Temporary password',{exact:true}).fill('FakeTestPassword42');
  await page.getByRole('button',{name:'Create viewer',exact:true}).click();
  await page.getByRole('heading',{name:'new-viewer',exact:true}).waitFor();
  assert.equal(await page.getByLabel('Temporary password',{exact:true}).inputValue(),'');
  await page.setViewportSize({width:320,height:780});
  assert(await page.locator('.gs26-admin').evaluate(n=>n.scrollWidth<=n.clientWidth),'admin fits narrow screens');
  assert.equal((await page.getByRole('button',{name:'Exit admin',exact:true}).boundingBox()).height,44,'admin uses the shared button height');
  await page.getByRole('button',{name:'Broadcast studio',exact:true}).click();
  await page.getByRole('heading',{name:'Broadcast studio',exact:true}).waitFor();
  await page.getByRole('button',{name:'Exit stream mode',exact:true}).click();
  await page.getByRole('button',{name:'Admin',exact:true}).click();
  await page.getByRole('button',{name:'Exit admin',exact:true}).click();
  session.roles=[]; session.username='other-user'; session.permissions.send_commands=false;

  await page.reload();await choose();
  assert.equal(await page.getByRole('button',{name:'Stream Manager',exact:true}).count(),0,'manager is permission gated');
  assert.equal(await page.getByRole('button',{name:'Admin',exact:true}).count(),0,'admin area is permission gated');
  assert.equal(await page.locator('[aria-label="Pinned telemetry"]').count(),0,'another user starts with their own pins');
  assert.equal(await page.getByRole('button',{name:'Voice Chat',exact:true}).count(),1,'another user has independent tab visibility');
  await page.getByRole('button',{name:'Voice Chat',exact:true}).click();
  await page.frameLocator('#gs26-crew-voice').getByText('Media tool fixture').waitFor({state:'visible'});
  assert((await page.locator('#gs26-crew-voice').boundingBox()).height>300,'voice fills mobile tab');
  await choose();await page.getByRole('button',{name:'Cameras & Recordings',exact:true}).click();
  await page.frameLocator('#gs26-media').getByText('Media tool fixture').waitFor({state:'visible'});
  assert((await page.locator('#gs26-media').boundingBox()).height>300,'cameras fill mobile tab');
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'media fits mobile width');
  for(const source of ['kg50','kg1000_absolute']) {
    fillSource=source;
    await page.reload();await choose();await page.getByRole('button',{name:'Telemetry',exact:true}).click();
    for(const tab of ['LOADCELL','DAQ']) {
      await page.getByRole('button',{name:/Show data tabs/}).click();
      await page.getByRole('button',{name:tab,exact:true}).click();
      for(const cell of ['1000 kg','50 kg']) {
        await page.getByRole('button',{name:/Show subtabs/}).click();
        await page.getByRole('button',{name:cell,exact:true}).click();
        const expected=cell===(source==='kg50'?'50 kg':'1000 kg');
        if(expected) await page.getByText('Fill %',{exact:true}).first().waitFor({state:'visible'});
        else assert.equal(await page.getByText('Fill %',{exact:true}).count(),0,`${tab} ${cell} must not show other cell's fill`);
      }
    }
  }
  // Exercise request dispatch for the complete HITL command inventory in an
  // isolated WebSocket mock. Nothing connects to a GroundStation or board.
  const commands=['StartWritingNow','StartWritingLastTwoMinutes','PauseWritingDb','StopWritingDb','Postinit','Launch','GroundStationLaunch','ToggleButtonInterlock','ToggleGroundStationControl','ToggleLaunchInterlock','TogglePhysicalLaunchMode','ResetLaunchLatch','ToggleAvBayUnderglow','ToggleFlightBuzzer','Igniter','IgniterSequence','RetractPlumbing','AdvanceFlightState','RewindFlightState','DeployParachute','ExpandParachute','ReinitSensors','EvaluationRelax','EvaluationFocus','EvaluationAbort','ReinitBarometer','EnableIMU','DisableIMU','MonitorAltitude','RevokeMonitorAltitude','ConsecutiveSamples','RevokeConsecutiveSamples','ResetFailures','RevokeResetFailures','ValidateMeasms','RevokeValidateMeasms','AbortAfter100','AbortAfter40','AbortAfter250','ReinitAfter15','ReinitAfter30','ReinitAfter50','Dump','Pilot','NormallyOpen','Nitrogen','Nitrous','Abort','StartFill','PauseFill','CancelFill','NitrogenTest','ValveSelfTest','ResetFlightState','ResetTClock'];
  dashboardLayout.actions_tab.actions=commands.map(cmd=>({label:cmd,cmd,group:['StartFill','PauseFill','CancelFill','NitrogenTest','ValveSelfTest'].includes(cmd)?'GSE sequence actions':'Command acceptance',border:'#53657c',bg:'#142033',fg:'#ffffff'}));
  actionPolicy={key_enabled:true,software_buttons_enabled:true,controls:commands.map(cmd=>({cmd,enabled:true,blink:'none'}))};
  session.permissions.send_commands=true;
  gseStatus={phase:'idle',message:'Isolated test',dry_self_test_confirmed:false,configuration_error:null,request_gate:{hitl_mode:true,flight_state:'Startup',prelaunch:true,button_interlock_satisfied:true}};
  const commandContext=await browser.newContext({viewport:{width:1280,height:900}});
  await commandContext.addInitScript(({session,origin})=>{localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
  // New Playwright's WebSocket mock uses URL.parse, absent in older system Chrome.
  await commandContext.addInitScript(()=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base);}catch{return null;}};});
  const requests=[];let commandSocket;
  // Keep the debug bundle's hot-reload socket inert in this static fixture.
  await commandContext.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
  await commandContext.routeWebSocket(url=>url.pathname==='/ws'&&url.searchParams.get('token')==='ui-token',socket=>{
    commandSocket=socket;
    socket.onMessage(message=>requests.push(JSON.parse(String(message)).cmd));
    const timer=setInterval(()=>socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy})),500);
    socket.onClose(()=>clearInterval(timer));
  });
  const commandPage=await commandContext.newPage();
  commandPage.on('console',message=>{if(message.type()==='error')fs.appendFileSync('/tmp/gs-command-inventory-console.log',message.text()+'\n');});
  commandPage.on('pageerror',error=>errors.push(error.message));
  let approveRecovery=true;
  commandPage.on('dialog',dialog=>approveRecovery?dialog.accept():dialog.dismiss());
  await commandPage.goto(origin+'/dashboard');
  await commandPage.getByRole('button',{name:/Choose tab/}).click().catch(async error=>{
    fs.writeFileSync('/tmp/gs-command-inventory-errors.json',JSON.stringify(errors));
    fs.writeFileSync('/tmp/gs-command-inventory-body.txt',await commandPage.locator('body').innerText());
    await commandPage.screenshot({path:'/tmp/gs-command-inventory-failure.png',fullPage:true});
    throw error;
  });
  await commandPage.getByRole('button',{name:'Actions',exact:true}).click();
  await commandPage.getByRole('checkbox',{name:/gas supplies are isolated/}).check();
  for(const cmd of commands){
    if(cmd==='ValveSelfTest'){await commandPage.getByRole('checkbox',{name:/gas supplies are isolated/}).check();await commandPage.getByText('Self-test confirmation accepted. Backend interlocks still apply.',{exact:true}).waitFor();}
    const count=requests.length;
    await commandPage.getByRole('button',{name:cmd,exact:true}).last().click().catch(async error=>{fs.writeFileSync('/tmp/gs-command-inventory-body.txt',await commandPage.locator('body').innerText());throw error;});
    for(let attempt=0;attempt<100&&requests.length===count;attempt++)await new Promise(resolve=>setTimeout(resolve,20));
    assert.equal(requests.length,count+1,`${cmd} dispatches exactly one request`);
    assert.equal(requests[count],cmd,`${cmd} retains the backend command name`);
  }
  approveRecovery=false;
  for(const cmd of ['ResetFlightState','ResetTClock']){
    const count=requests.length;
    await commandPage.getByRole('button',{name:cmd,exact:true}).last().click();
    await new Promise(resolve=>setTimeout(resolve,300));
    assert.equal(requests.length,count,`${cmd} cancellation sends no request`);
  }
  assert.equal(await commandPage.getByRole('alert').filter({hasText:'Command not sent:'}).count(),0);
  await commandPage.getByRole('navigation',{name:'Mission navigation'}).getByRole('link',{name:'Dashboard',exact:true}).click();
  const liveRows=[
    ['LOADCELL_50_WEIGHT_KG','DAQ',[12.34]],['LOADCELL_WEIGHT_KG','DAQ',[987.65]],
    ['FUEL_TANK_PRESSURE','VB',[720.5]],['LOADCELL_FILL_PERCENT','DAQ',[82.25]],
    ['BATTERY_VOLTAGE','GB',[14.05]],['BATTERY_VOLTAGE','VB',[16.24]],
    ['VALVE_STATE','VB',[0,1,1,0,0,0,0,0]]
  ];
  for(const width of [1280,844,390,320]) {
    await commandPage.setViewportSize({width,height:900});
    const now=Date.now();commandSocket.send(JSON.stringify({ty:'TelemetryBatch',data:liveRows.map(([data_type,sender_id,values])=>({data_type,sender_id,values,timestamp_ms:now,received_timestamp_ms:now}))}));
    for(const [label,value] of [['50 kg loadcell','12.34 kg'],['1000 kg loadcell','987.65 kg'],['Fill box battery','14.05 V'],['Valve board battery','16.24 V'],['Vent','Open / On'],['Nitrogen','Closed / Off'],['Nitrous','Closed / Off']]) {
      const card=commandPage.locator('.gs26-dashboard-card').filter({has:commandPage.getByText(label,{exact:true})});
      await card.getByText(value,{exact:true}).waitFor();
    }
    assert(await commandPage.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'live overview fits '+width);
    await commandPage.screenshot({path:`/tmp/gs-live-overview-${width}.png`,fullPage:true});
  }
  await commandContext.close();
  session.roles=['stream_viewer'];session.permissions.send_commands=false;
  await page.reload();
  await page.getByText('Waiting for the delayed broadcast program…',{exact:true}).waitFor();
  assert.equal(await page.getByRole('navigation',{name:'Mission navigation'}).count(),0,'viewer role mounts no mission navigation');
  assert.equal(await page.locator('.gs26-status-shell,.gs26-program-editor').count(),0,'viewer has no mission status or operator controls');
  assert.equal(await page.getByRole('button',{name:'Exit stream mode',exact:true}).count(),0,'viewer role cannot return to mission controls');
  assert.deepEqual(errors,[]);
  console.log('PASS: no dashboard model, persistent/resizable camera cards, offline recovery, stream manager permissions, card editing, live pins, per-user persistence, hidden tabs, iframe session handoff/lifetime, desktop and mobile tab picker, shared checklist tab (tested separately), stream-only Mission Live, selected-loadcell fill in Loadcell and DAQ, all 55 command buttons, recovery confirmation acceptance and cancellation');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
