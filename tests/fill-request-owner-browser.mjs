// Actual Wasm, synthetic loopback station only. No hardware or external service.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/valve-toggle-browser.mjs','utf8');
const boundary=source.indexOf('try {\n const commands=');
if(boundary<0)throw Error('Shared browser fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href));
fixture=fixture.replace('let cameraOnline=true;',`let cameraOnline=true;let heldPath=null;const pending=[],fillRequests=[];let authReplies=0,statusReplyCode=200;`);
fixture=fixture.replace("  if(pathname==='/test-camera')",`  if(pathname==='/api/auth/session'){authReplies++;res.setHeader('Content-Type','application/json');res.end(JSON.stringify(session));return;}
  if(req.method==='POST'&&(pathname==='/api/command'||pathname==='/api/gse/abort/clear')){
    let raw='';for await(const chunk of req)raw+=chunk;fillRequests.push({path:pathname,body:JSON.parse(raw),authorization:req.headers.authorization});
    if(heldPath===pathname){await new Promise(resolve=>pending.push(resolve));res.statusCode=401;}res.setHeader('Content-Type','application/json');res.end(pathname==='/api/gse/abort/clear'?'{"cleared":true}':'{}');return;
  }
  if(pathname==='/test-camera')`);
fixture=fixture.replace("if(pathname==='/api/gse/status' && gseStatus){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(gseStatus));return;}","if(pathname==='/api/gse/status' && gseStatus){const snapshot=JSON.stringify(gseStatus),code=statusReplyCode;if(heldPath===pathname)await new Promise(resolve=>pending.push(resolve));res.statusCode=code;res.setHeader('Content-Type','application/json');res.end(snapshot);return;}");
fixture+=String.raw`try {
 session.username='fill-owner-operator';session.roles=[];
 session.allowed_commands=['Turbocharge','ValveSelfTest'];
 dashboardLayout.test_fire_mode=true;
 dashboardLayout.actions_tab.actions=[{label:'Valve self-test',cmd:'ValveSelfTest',group:'GSE sequence actions',border:'#53657c',bg:'#142033',fg:'#ffffff'}];
 actionPolicy={key_enabled:true,software_buttons_enabled:true,controls:[{cmd:'Turbocharge',enabled:true,blink:'none'},{cmd:'ValveSelfTest',enabled:true,blink:'none'}]};
 const errors=[],commands=[];
 const waitFor=async(predicate,message)=>{const end=Date.now()+12000;while(Date.now()<end){if(await predicate())return;await new Promise(resolve=>setTimeout(resolve,20));}assert.fail(message);};
 async function open(scene){
   session.expires_at_ms=Date.now()+3600000;
   const state=scene==='turbo'?'Idle':'Aborted';
   gseStatus={phase:scene==='turbo'?'paused':'fault',message:'Owned fill status',nitrogen_passed:true,self_test_locked:false,current_step_psi:0,pressure_psi:750,baseline:null,valves:[false,false,false,false,false],dry_self_test_confirmed:false,request_gate:{hitl_mode:true,flight_state:state,prelaunch:true,button_interlock_satisfied:true}};
   const context=await browser.newContext({viewport:{width:1280,height:1000}});
   await context.addInitScript(({session,origin})=>{
     if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};
     localStorage.setItem('navigation-style','tabs');
     for(const base of ['',origin])localStorage.setItem('gs26_dashboard_customization_v1_'+base.replace(/[^a-zA-Z0-9]/g,'_')+'_ground_station_view','on');
     localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));
   },{session,origin});
   await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
   await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{
     socket.onMessage(message=>commands.push(JSON.parse(message)));
     const timer=setInterval(()=>{socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy}));socket.send(JSON.stringify({ty:'FlightState',data:{state}}));},100);
     socket.onClose(()=>clearInterval(timer));
   });
   const page=await context.newPage();
   // Isolate each real concurrent consumer: Chromium's shared HTTP cache can
   // serialize identical pending GETs behind one network transaction.
   const networkControls=await context.newCDPSession(page);
   await networkControls.send('Network.enable');
   await networkControls.send('Network.setCacheDisabled',{cacheDisabled:true});
   page.setDefaultTimeout(12000);page.on('pageerror',error=>errors.push(error.message));
   await page.goto(origin+'/dashboard');await page.locator('.gs26-dashboard-shell').waitFor();
   if(scene==='actions'){await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Actions',exact:true}).click();}
   const target=scene==='turbo'?page.getByRole('button',{name:'Turbocharge',exact:true}):page.getByRole('button',{name:'Clear GSE abort',exact:true});
   await target.waitFor();assert.equal(await target.count(),1,'one exact original fill control');
   await target.evaluate(element=>{window.originalFillControl=element;});
   return{context,page,target};
 }
 async function changeSameUserAuthority(page){
   const marker=session.expires_at_ms+12345;session.expires_at_ms=marker;
   await waitFor(()=>page.evaluate(marker=>JSON.parse(localStorage.getItem('auth_session_v1')||'{}').entries?.some(entry=>entry.session.session.expires_at_ms===marker),marker),'actual session-status poll must publish replacement authority');
   assert(await page.evaluate(()=>window.originalFillControl?.isConnected),'same-user authority change must keep the actual component mounted');
 }
 // Held old401 must not sign out the current same-user authority. This is
 // intentionally same mounted component, not an unmount-cancels-task oracle.
 for(const scene of ['turbo','actions','state']){
   const opened=await open(scene),{page,target}=opened;
   const pathname=scene==='turbo'?'/api/command':'/api/gse/abort/clear';heldPath=pathname;
   const count=fillRequests.length;
   await target.click();
   if(scene!=='turbo')await page.getByRole('button',{name:'Confirm clear abort',exact:true}).click();
   await waitFor(()=>fillRequests.length===count+1,'original request actually arrives before replacement authority');
   assert.deepEqual(fillRequests.at(-1),{path:pathname,body:scene==='turbo'?'Turbocharge':{},authorization:'Bearer ui-token'});
   await changeSameUserAuthority(page);
   heldPath=null;for(const resolve of pending.splice(0))resolve();
   await page.waitForTimeout(400);
   assert((await page.evaluate(()=>localStorage.getItem('auth_session_v1')))?.includes('ui-token'),'late old401 must not clear current same-user session');
   assert.equal(await page.getByText('GSE abort cleared. Repeat operation checks before filling.',{exact:true}).count(),0,'old response cannot claim recovery');
   assert.equal(fillRequests.length,count+1,'no automatic resend after authority replacement');
   await opened.context.close();
 }
 // Both actual dashboard-lifetime and GsePanel status consumers retain their
 // original owner. Hold their old401 while the same mounted view changes auth.
 {
   const opened=await open('turbo'),{page}=opened;
   heldPath='/api/gse/status';statusReplyCode=401;gseStatus.message='Retired status payload';
   await waitFor(()=>pending.length>=3,'all three serial mounted status consumers have held old responses');
   console.log('HELD_STATUS_REQUESTS',pending.length);
   const originalHeld=pending.slice();
   // Only the original requests are old401s. New-owner GETs must never be
   // admitted into that deliberately held batch while auth polling completes.
   heldPath=null;statusReplyCode=200;gseStatus.message='Awaiting replacement authority';
   await changeSameUserAuthority(page);
   gseStatus.message='Current replacement status';
   await page.getByText('Current replacement status',{exact:true}).waitFor();
   await page.evaluate(()=>{window.retiredStatusPublished=false;window.statusObserver=new MutationObserver(()=>{if(document.body.innerText.includes('Retired status payload'))window.retiredStatusPublished=true;});window.statusObserver.observe(document.body,{subtree:true,childList:true,characterData:true});});
   assert.equal(pending.length,originalHeld.length,'future requests never join the original held batch');
   for(const resolve of originalHeld)resolve();pending.splice(0,originalHeld.length);
   await page.waitForTimeout(400);
   assert.equal(await page.evaluate(()=>window.retiredStatusPublished),false,'old GET cannot overwrite replacement status even briefly');
   assert((await page.evaluate(()=>localStorage.getItem('auth_session_v1')))?.includes('ui-token'),'held status401 cannot clear replacement authority');
   assert(await page.getByText('Current replacement status',{exact:true}).isVisible());
   gseStatus.launch_queued=true;
   await page.getByTitle('Launch queued for QD acknowledgement and settling',{exact:true}).waitFor();
   gseStatus.launch_queued=false;
   await page.locator('.gs26-launch-clock:not(.gs26-launch-queued)').waitFor();
   await page.evaluate(()=>window.statusObserver.disconnect());
   await opened.context.close();
 }
 // An already-open confirmation never adopts the authority observed later.
 for(const scene of ['actions','state']){
   const opened=await open(scene),{page,target}=opened,count=fillRequests.length;
   await target.click();await page.getByRole('button',{name:'Confirm clear abort',exact:true}).waitFor();
   await changeSameUserAuthority(page);
   // Wait for actual owner retirement, no forced DOM mutation or synthetic auth.
   await page.getByRole('button',{name:'Confirm clear abort',exact:true}).waitFor({state:'detached'});
   assert.equal(fillRequests.length,count,'old confirmation sends no request under replacement authority');
   await target.click();await page.getByRole('button',{name:'Confirm clear abort',exact:true}).click();
   await page.getByText('GSE abort cleared. Repeat operation checks before filling.',{exact:true}).waitFor();
   assert.equal(fillRequests.length,count+1,'a fresh explicit confirmation remains usable');
   await opened.context.close();
 }
 assert.deepEqual(commands,[],'HTTP-only fill fixture sends no websocket commands');assert.deepEqual(errors,[]);
 console.log('PASS actual same-mounted Turbo/Actions-clear/State-clear held401 authority replacement, original body/auth, old dialog retirement, fresh explicit confirmation and no auto resend');
}finally{heldPath=null;for(const resolve of pending.splice(0))resolve();await browser.close();await new Promise(resolve=>server.close(resolve));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
