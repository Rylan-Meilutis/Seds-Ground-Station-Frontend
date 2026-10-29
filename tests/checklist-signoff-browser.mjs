// Build with `dx build --platform web`, then run with Node and Playwright.
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root=path.resolve(process.env.GS_UI_BUNDLE || 'target/dx/groundstation_frontend/debug/web/public');
const example=name=>JSON.parse(fs.readFileSync(`docs/api-examples/${name}.json`,'utf8'));
const session={...example('auth-session.anonymous'),authenticated:true,anonymous:false,username:'ui-test',roles:['admin'],permissions:{view_data:true,send_commands:true}};
let lists={signoffs:[],revision:0,enforce_before_launch:false,checklists:[{id:'setup',title:'Setup',required_before_launch:true,items:[{id:'pressure',text:'Pressure checked',checked:false,completed_by:null,completed_at_ms:null}]},{id:'recovery',title:'Recovery',required_before_launch:false,items:[{id:'recover',text:'Recovery area clear',checked:false,completed_by:null,completed_at_ms:null}]}]};
let legacyBackend=false, legacyUnsupported=0;
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
  if(legacyBackend && ['/api/checklists/sign','/api/checklists/export','/api/checklists/export.csv'].includes(pathname)) {legacyUnsupported++;res.statusCode=404;res.end('Legacy backend does not support sign-off');return;}
  if(pathname==='/api/checklists/sign') {
    let body='';for await(const chunk of req)body+=chunk;const request=JSON.parse(body);
    assert.equal(req.headers.authorization,'Bearer ui-token');
    assert.deepEqual(Object.keys(request).sort(),['checklist_id','revision']);
    assert.equal(request.revision,lists.revision);
    const list=lists.checklists.find(c=>c.id===request.checklist_id);
    assert(list.items.every(i=>i.checked));
    const canonical={id:list.id,title:list.title,required_before_launch:list.required_before_launch,items:list.items.map(i=>({id:i.id,text:i.text,checked:i.checked,complete_on:i.complete_on??null,completion_source:i.completion_source??null,completed_by:i.completed_by??null,completed_at_ms:i.completed_at_ms??null}))};
    lists.signoffs.push({checklist_id:list.id,checklist_title:list.title,signed_by:'server-reviewer',signed_at_ms:1793260800000,signed_revision:lists.revision,content_digest:createHash('sha256').update(JSON.stringify(canonical)).digest('hex'),invalidated_at_ms:null,invalidation_reason:null});lists.revision++;
    res.setHeader('Content-Type','application/json');res.end(JSON.stringify(legacyBackend?{...lists,signoffs:undefined}:lists));return;
  }
  if(pathname==='/api/checklists/export'||pathname==='/api/checklists/export.csv') {
    assert.equal(req.headers.authorization,'Bearer ui-token');
    res.setHeader('Content-Type',pathname.endsWith('.csv')?'text/csv':'application/json');
    res.end(pathname.endsWith('.csv')?'signed_by,signed_at_ms\r\nserver-reviewer,1793260800000\r\n':JSON.stringify(lists));return;
  }
  if(pathname==='/api/checklists'||pathname==='/api/checklists/check') {
    res.setHeader('Content-Type','application/json');
    if(req.method==='POST') {
      assert.equal(req.headers.authorization,'Bearer ui-token');
      let body='';for await(const data of req)body+=data;const request=JSON.parse(body);
      if(pathname.endsWith('/check')) {for(const sig of lists.signoffs){if(sig.checklist_id===request.checklist_id&&sig.invalidated_at_ms===null){sig.invalidated_at_ms=1793260801000;sig.invalidation_reason='Checklist changed';}}const item=lists.checklists.find(c=>c.id===request.checklist_id).items.find(i=>i.id===request.item_id);item.checked=request.checked;item.completed_by=request.checked?'ui-test':null;item.completion_source=request.checked?(item.complete_on?'manual_override':'operator'):null;}
      else {if(request.revision!==lists.revision){res.statusCode=409;res.end('Checklists changed on another device; reload before saving');return;}lists=request;}
      lists.revision++;
    }
    res.end(JSON.stringify(legacyBackend?{...lists,signoffs:undefined}:lists));return;
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
 async function open(roles,permissions){
  const context=await browser.newContext({viewport:{width:1440,height:1000},acceptDownloads:true});
  const status={...session,roles,permissions:{view_data:true,send_commands:false,...permissions}};
  await context.addInitScript(({status,origin})=>{
   if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};
   localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session:status,remember_me:true}}]}));
  },{status,origin});
  await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
  await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(()=>{}));
  await context.route('**/api/auth/session',async route=>route.fulfill({json:status}));
  const page=await context.newPage();await page.goto(origin+'/dashboard');
  await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
  await page.getByRole('link',{name:'Checklists',exact:true}).click();
  await page.getByRole('main',{name:'Shared checklists'}).waitFor();
  return {context,page};
 }
 let {context,page}=await open([],{});
 assert.equal(await page.getByRole('button',{name:'Sign off checklist',exact:true}).count(),0,'ordinary users cannot approve');
 await context.close();
 ({context,page}=await open(['admin'],{}));
 const sign=page.getByRole('button',{name:'Sign off checklist',exact:true});
 assert(await sign.isDisabled(),'incomplete checklist cannot be signed');
 await page.getByText('Pressure checked',{exact:true}).click();
 await page.waitForFunction(()=>document.querySelector('.gs26-checklist-item input')?.checked);
 await sign.click();
 await page.getByText(/Signed off by server-reviewer/).waitFor();
 assert.equal(await page.getByRole('button',{name:'Sign off checklist',exact:true}).count(),0);
 let download=page.waitForEvent('download');await page.getByRole('button',{name:'Export checklists',exact:true}).click();
 let data=JSON.parse(fs.readFileSync(await (await download).path(),'utf8'));
 assert.equal(data.signoffs[0].signed_by,'server-reviewer');assert.equal(data.signoffs[0].signed_at_ms,1793260800000);
 download=page.waitForEvent('download');await page.getByRole('button',{name:'Export sign-offs CSV',exact:true}).click();
 assert(fs.readFileSync(await (await download).path(),'utf8').includes('server-reviewer,1793260800000'));
 await page.getByText('Pressure checked',{exact:true}).click();
 await page.getByText(/Previous sign-off by server-reviewer/).waitFor();
 assert(await page.getByRole('button',{name:'Sign off checklist',exact:true}).isDisabled());
 await context.close();
 ({context,page}=await open([],{sign_checklists:true}));
 assert.equal(await page.getByRole('button',{name:'Sign off checklist',exact:true}).count(),1,'explicit reviewer permission grants review UI without admin role');
 await context.close();
 // A legacy station omits signoffs entirely. Preserve its item-completion
 // contract; missing approval capability cannot invent an authoritative lock.
 legacyBackend=true;lists.enforce_before_launch=true;
 lists.checklists[0].items[0].checked=true;
 ({context,page}=await open(['admin'],{send_commands:true}));
 const legacyErrors=[];page.on('pageerror',error=>legacyErrors.push(error.message));
 await page.getByText('Required checklists complete',{exact:true}).waitFor();
 await page.getByText('Sign-off is unavailable with this station version.',{exact:true}).waitFor();
 assert.equal(await page.getByRole('button',{name:'Sign off checklist',exact:true}).count(),0);
 assert.equal(await page.getByRole('button',{name:/^Export (checklists|sign-offs CSV)$/}).count(),0);
 const legacyCheckbox=page.locator('.gs26-checklist-item input[type=checkbox]').first();
 await legacyCheckbox.uncheck();
 await page.getByText('1 required checklists incomplete — launch locked',{exact:true}).waitFor();
 await legacyCheckbox.check();
 await page.getByText('Required checklists complete',{exact:true}).waitFor();
 assert.equal(legacyUnsupported,0,'unsupported endpoints are never requested');
 await page.getByRole('link',{name:'Actions',exact:true}).click();
 await page.getByRole('heading',{name:'Actions',exact:true}).waitFor();
 const abortButton=page.locator('button').filter({has:page.locator('span[title="Abort"]')});
 assert(await abortButton.isVisible(),'existing server-configured Abort action remains visible');
 assert(await abortButton.isEnabled(),'legacy checklist capability does not block an allowed action');
 await page.getByRole('link',{name:'Telemetry',exact:true}).click();
 assert(await page.getByRole('navigation',{name:'Mission navigation'}).isVisible());
 assert.deepEqual(legacyErrors,[]);await context.close();
 console.log('PASS actual Wasm checklist approval permissions, completion gate, reviewer audit, exports, invalidation and legacy station completion without unsupported signing');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
