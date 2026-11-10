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
  // Current stations wrap every GSE status in a sequence-progress envelope; the UI treats a status without one as unavailable.
  if(pathname==='/api/gse/status'&&gseStatus&&!gseStatus.sequence_progress)gseStatus.sequence_progress={version:1,backend_instance:'1'.repeat(32),reset_epoch:'2'.repeat(16),cancel_api_version:1,active:[],terminal:[],reporting_error:null};
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
 const open=async({base='Nitrogen',observed=null,touch=false}={})=>{
  const commands=['Nitrogen','NitrogenClose','Nitrous','NitrousClose'];dashboardLayout.actions_tab.actions=commands.map(cmd=>({label:cmd,cmd,group:'Manual GSE valves',border:'#53657c',bg:'#142033',fg:'#ffffff'}));
  actionPolicy={key_enabled:true,software_buttons_enabled:true,controls:commands.map(cmd=>({cmd,enabled:cmd.endsWith('Close')||observed!==null,blink:'none',actuated:cmd===base?observed:null}))};
  const context=await browser.newContext({viewport:{width:touch?390:1280,height:900},hasTouch:touch,isMobile:touch});
  await context.addInitScript(({session,origin})=>{localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base);}catch{return null;}};window.valveEvents=[];for(const type of ['pointerdown','touchstart','touchcancel','mousedown','click'])document.addEventListener(type,e=>{const button=e.target.closest?.('button');if(button?.querySelector('span[title="Nitrogen"],span[title="Nitrous"]'))window.valveEvents.push({type,command:button.getAttribute('data-dispatch-command'),trusted:e.isTrusted,primary:e.isPrimary});},true);},{session,origin});
  const requests=[],sockets=new Set();await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws'&&url.searchParams.get('token')==='ui-token',socket=>{sockets.add(socket);socket.onMessage(message=>requests.push(JSON.parse(String(message)).cmd));const timer=setInterval(()=>socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy})),100);socket.onClose(()=>{clearInterval(timer);sockets.delete(socket);});});
  const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));await page.goto(origin+'/dashboard');await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Actions',exact:true}).click();
  const primary=page.locator('button').filter({has:page.locator('span[title="'+base+'"]')}).last();await primary.waitFor();await primary.scrollIntoViewIfNeeded();const cdp=touch?await context.newCDPSession(page):null;
  const update=async(observed,closeEnabled=true)=>{Object.assign(actionPolicy.controls.find(c=>c.cmd===base),{actuated:observed,enabled:observed!==null});actionPolicy.controls.find(c=>c.cmd===base+'Close').enabled=closeEnabled;for(const socket of sockets)socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy}));await page.waitForFunction(({base,observed,closeEnabled})=>{const e=[...document.querySelectorAll('button')].find(b=>b.querySelector('span[title="'+base+'"]'));return e?.getAttribute('data-dispatch-command')===(observed===false?base:base+'Close')&&(closeEnabled||e.disabled);},{base,observed,closeEnabled});};
  const down=async()=>{const b=await primary.boundingBox(),x=b.x+b.width/2,y=b.y+b.height/2;if(touch)await cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x,y,id:1}]});else{await page.mouse.move(x,y);await page.mouse.down();}};
  const up=async()=>{if(touch)await cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});else await page.mouse.up();};
  const syntheticTouch=async(count=1)=>primary.evaluate((el,count)=>{const b=el.getBoundingClientRect(),touches=Array.from({length:count},(_,i)=>new Touch({identifier:i+1,target:el,clientX:b.x+10+i,clientY:b.y+10}));el.dispatchEvent(new TouchEvent('touchstart',{bubbles:true,cancelable:true,touches,targetTouches:touches,changedTouches:touches}));},count);
  const cancel=()=>primary.dispatchEvent('touchcancel');
  const ghost=async()=>{await primary.dispatchEvent('mousedown',{button:0,buttons:1});await primary.dispatchEvent('click',{button:0});};
  const finish=async(name,expected)=>{if(expected.length){for(let i=0;i<100&&requests.length<expected.length;i++)await page.waitForTimeout(10);}await page.waitForTimeout(100);assert.deepEqual(requests,expected,name);assert.deepEqual(errors,[]);results.push({name,requests:[...requests],events:await page.evaluate(()=>window.valveEvents)});fs.writeFileSync(process.env.GS_GESTURE_RESULT||'/tmp/valve-gesture-result.json',JSON.stringify(results,null,2));await context.close();};
  return {base,page,context,primary,requests,cdp,update,down,up,syntheticTouch,cancel,ghost,finish};
 };
 for(const base of ['Nitrogen','Nitrous']){
  const c=await open({base});await c.down();await c.update(false);await c.up();await c.finish(base+' mouse Close→OFF never opens',[]);
 }
 {const c=await open();await c.down();await c.up();await c.finish('mouse stable Close positive',['NitrogenClose']);}
 // Real Dioxus Actions controls: dragging away and back still produces a
 // browser click, so target-only pointerleave handlers are insufficient.
 {const c=await open();await c.down();const b=await c.primary.boundingBox();await c.page.mouse.move(b.x+b.width/2+14,b.y+b.height/2);await c.page.mouse.move(b.x+b.width/2,b.y+b.height/2);await c.up();assert.deepEqual(c.requests,[],'drag returning to the same valve must not dispatch');await c.primary.click();await c.finish('drag cancels actual valve; next physical click dispatches once',['NitrogenClose']);}
 {const c=await open();await c.down();const moved=await c.primary.evaluate(el=>{for(let p=el.parentElement;p;p=p.parentElement){if(p.scrollHeight>p.clientHeight+2){const before=p.scrollTop;p.scrollTop=before>0?before-1:before+1;return p.scrollTop!==before;}}return false;});assert(moved,'Actual Actions scroll container must move');await c.up();await c.finish('actual ancestor scroll cancels held valve',[]);}
 {const c=await open({observed:false});await c.down();await c.update(null);await c.up();await c.finish('mouse Open→unknown drops changed intent',[]);}
 {const c=await open();await c.down();await c.update(null,false);await c.up();await c.finish('current Close permission revoked',[]);}
 {const c=await open();await c.down();await c.page.waitForTimeout(700);await c.up();await c.finish('held gesture exceeds actual650ms limit',[]);}
 {const c=await open();await c.down();await c.page.evaluate(()=>{const next=new URL(location.origin);next.hostname='localhost';localStorage.setItem('gs_base_url',next.origin);if(next.origin===location.origin)throw Error('Station fixture must change normalized host');});await c.up();await c.finish('actual persisted station identity changed',[]);}
 {const c=await open();await c.down();await c.page.getByRole('button',{name:/^Sign out/,includeHidden:true}).evaluate(e=>e.click());await c.primary.waitFor({state:'detached'});await c.up();await c.finish('actual signout revokes auth and unmounts Actions',[]);}
 {const c=await open();await c.down();await c.page.getByRole('button',{name:/Choose tab/}).evaluate(e=>e.click());await c.page.getByRole('button',{name:'Dashboard',exact:true}).evaluate(e=>e.click());await c.primary.waitFor({state:'detached'});await c.up();await c.finish('actual tab switch unmounts held control',[]);}
 for(const changed of [false,true]){
  const c=await open({touch:true});await c.down();if(changed)await c.update(false);await c.up();const events=await c.page.evaluate(()=>window.valveEvents);assert(events.some(e=>e.type==='touchstart'&&e.trusted),'Real browser touch path taken');assert(events.some(e=>e.type==='mousedown'&&e.trusted),'Real compatibility mouse path taken');await c.finish('real mobile touch '+(changed?'Close→OFF drops compatibility click':'stable Close exactly once'),changed?[]:['NitrogenClose']);
 }
 {const c=await open({touch:true});await c.syntheticTouch();await c.cancel();await c.update(false);await c.ghost();await c.finish('adversarial touchcancel→OFF→compatmousedown/click',[]);}
 {const c=await open({touch:true});await c.syntheticTouch();await c.cancel();await c.update(false);await c.primary.dispatchEvent('pointerdown',{pointerType:'touch',pointerId:2,isPrimary:false});await c.ghost();await c.finish('secondary pointer cannot rearm cancelled intent',[]);}
 {const c=await open({touch:true});await c.syntheticTouch();await c.cancel();await c.update(false);await c.syntheticTouch(2);await c.ghost();await c.finish('multitouch cannot rearm cancelled intent',[]);}
 {const c=await open({touch:true});await c.syntheticTouch();await c.cancel();await c.update(false);await c.primary.click();await c.finish('fresh real primary mouse gesture after cancellation',['Nitrogen']);}
 {const c=await open({touch:true});await c.syntheticTouch();await c.cancel();await c.update(false);await c.down();await c.up();await c.finish('fresh real touch gesture after cancellation',['Nitrogen']);}
 for(const [observed,key,expected]of [[false,'Enter','Nitrogen'],[null,'Space','NitrogenClose']]){const c=await open({observed});await c.primary.focus();await c.page.keyboard.press(key);await c.finish('keyboard '+key+' uses current command',[expected]);}
 {const c=await open();await c.syntheticTouch();await c.cancel();await c.primary.focus();await c.page.keyboard.press('Enter');await c.finish('new keyboard intent after cancellation',['NitrogenClose']);}

 {const c=await open();await c.primary.focus();await c.page.keyboard.down('Space');await c.update(false);await c.page.keyboard.up('Space');await c.finish('held Space Close→OFF never opens',[]);}
 {const c=await open();await c.primary.focus();await c.page.keyboard.down('Space');await c.update(false);await c.page.keyboard.down('Space');await c.page.keyboard.up('Space');await c.finish('Space autorepeat cannot refresh chosen Close',[]);}
 {const c=await open();await c.primary.focus();await c.page.keyboard.down('Enter');await c.update(false);await c.page.keyboard.down('Enter');await c.page.keyboard.up('Enter');await c.finish('Enter autorepeat cannot become Open',['NitrogenClose']);}
 for(const key of ['Escape','Tab']){const c=await open();await c.primary.focus();await c.page.keyboard.down('Space');await c.page.keyboard.press(key);await c.page.keyboard.up('Space');await c.finish('held Space cancellation '+key,[]);}
 {const c=await open();await c.primary.focus();await c.page.keyboard.down('Space');await c.page.locator('button').filter({has:c.page.locator('span[title="Nitrous"]')}).last().focus();await c.page.keyboard.up('Space');await c.finish('held Space blur to another valve',[]);}
 {const c=await open();await c.primary.focus();await c.page.keyboard.down('Space');await c.update(null,false);await c.page.keyboard.up('Space');await c.finish('held Space current capability revoked',[]);}
 {const c=await open();await c.primary.focus();await c.page.keyboard.down('Space');await c.page.evaluate(()=>{const next=new URL(location.origin);next.hostname='localhost';localStorage.setItem('gs_base_url',next.origin);if(next.origin===location.origin)throw Error('Distinct station required');});await c.page.keyboard.up('Space');await c.finish('held Space actual station identity changed',[]);}
 {const c=await open();await c.primary.focus();await c.page.keyboard.down('Space');await c.page.waitForTimeout(700);await c.page.keyboard.up('Space');await c.finish('held Space exceeds actual650ms limit',[]);}
 {const c=await open();await c.primary.focus();await c.page.keyboard.down('Space');await c.update(false);await c.page.keyboard.up('Space');assert.deepEqual(c.requests,[]);await c.primary.focus();await c.page.keyboard.press('Enter');await c.finish('fresh keyboard Open after retired Close gesture',['Nitrogen']);}
 {const c=await open();await c.primary.dispatchEvent('click',{button:0});await c.finish('unarmed click cannot infer current Close',[]);}
 {const c=await open({touch:true});await c.syntheticTouch();await c.cancel();await c.update(false);await c.ghost();await c.primary.dispatchEvent('click',{button:0});await c.finish('unarmed remount-like compatibility click has no fallback',[]);}

 {const c=await open();await c.primary.focus();await c.page.keyboard.down('Space');await c.page.getByRole('button',{name:/^Sign out/,includeHidden:true}).evaluate(e=>e.click());await c.primary.waitFor({state:'detached'});await c.page.keyboard.up('Space');await c.finish('held Space actual signout revokes auth and unmounts',[]);}
 {const c=await open({touch:true});await c.syntheticTouch();await c.page.getByRole('button',{name:/Choose tab/}).evaluate(e=>e.click());await c.page.getByRole('button',{name:'Dashboard',exact:true}).evaluate(e=>e.click());await c.primary.waitFor({state:'detached'});await c.page.getByRole('button',{name:/Choose tab/}).evaluate(e=>e.click());await c.page.getByRole('button',{name:'Actions',exact:true}).evaluate(e=>e.click());await c.primary.waitFor();await c.update(false);await c.ghost();assert.deepEqual(c.requests,[],'Retired component touch cannot produce new Open');await c.primary.click();await c.finish('actual Actions remount ghost drops then fresh pointer works',['Nitrogen']);}
 // Shell controls formerly had independent touchend actions which bypassed
 // click cancellation. Exercise actual Dioxus overlay open and Close controls.
 for(const [label,title] of [['Settings','Settings'],['Version','UBSEDS GS']]) {
  const c=await open({touch:true});await c.page.setViewportSize({width:390,height:280});
  await c.page.locator('.gs26-header-menu-toggle').filter({hasText:'Tools'}).click();
  const button=c.page.getByRole('button',{name:label,exact:true});await button.scrollIntoViewIfNeeded();
  const touchAndScroll=async(control)=>{
   const b=await control.boundingBox();await c.cdp.send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[{x:b.x+b.width/2,y:b.y+b.height/2,id:1}]});
   const changed=await control.evaluate(el=>{for(let p=el.parentElement;p;p=p.parentElement){if(p.scrollHeight>p.clientHeight+2){const old=p.scrollTop;p.scrollTop=old>0?old-1:old+1;return p.scrollTop!==old;}}return false;});
   assert(changed,'actual '+label+' control scroll container moved');await c.cdp.send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});
  };
  const heading=c.page.getByRole('heading',{name:title,exact:true,level:1});
  await touchAndScroll(button);assert.equal(await heading.count(),0,label+' press and scroll must stay closed');
  await button.tap();await heading.waitFor();
  const close=c.page.getByRole('button',{name:'Close',exact:true}).first();await close.scrollIntoViewIfNeeded();
  await touchAndScroll(close);assert.equal(await heading.count(),1,label+' Close press and scroll must stay open');
  await close.tap();await heading.waitFor({state:'detached'});await c.finish('actual '+label+' and Close scroll cancellation with fresh tap controls',[]);
 }
 fs.writeFileSync(process.env.GS_GESTURE_RESULT||'/tmp/valve-gesture-result.json',JSON.stringify(results,null,2));console.log('PASS '+results.length+' actual gesture cases: pointer/touch/compatibility cancellation, policy/owner/unmount and keyboard/current-command controls');
}catch(error){console.error(error.message);throw error;}finally{await browser.close();server.closeAllConnections();server.close();}
