// Actual compiled Wasm with loopback synthetic HTTP only; no board outputs.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/valve-toggle-browser.mjs','utf8');
const boundary=source.indexOf('try {\n const commands=');
if(boundary<0)throw Error('Shared synthetic browser fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href));
const marker='let cameraOnline=true;';
if(!fixture.includes(marker))throw Error('Shared fixture declaration changed');
fixture=fixture.replace(marker,marker+`let mutationHold=false,mutationCode=200;const qualificationWrites=[],qualificationWaiters=[];`);
const routeMarker="  if(pathname==='/test-camera')";
if(!fixture.includes(routeMarker))throw Error('Shared fixture router changed');
fixture=fixture.replace(routeMarker,`  if(req.method==='POST'&&['/api/gse/cold-flow/override','/api/gse/qualification/reset'].includes(pathname)){
    let raw='';for await(const part of req)raw+=part;
    const code=mutationCode;qualificationWrites.push({path:pathname,body:JSON.parse(raw),authorization:req.headers.authorization});
    if(mutationHold)await new Promise(resolve=>qualificationWaiters.push(resolve));
    res.statusCode=code;res.setHeader('Content-Type','application/json');res.end('{}');return;
  }
`+routeMarker);
fixture+=String.raw`try {
 session.username='qualification-operator';session.roles=[];session.allowed_commands=['ValveSelfTest','NitrogenTest'];
 dashboardLayout.actions_tab.actions=[{label:'Valve test',cmd:'ValveSelfTest',group:'GSE sequence actions',border:'#53657c',bg:'#142033',fg:'#ffffff'}];
 actionPolicy={key_enabled:true,software_buttons_enabled:true,controls:[{cmd:'ValveSelfTest',enabled:true,blink:'none'},{cmd:'NitrogenTest',enabled:true,blink:'none'}]};
 const errors=[],commands=[];
 const waitFor=async(check,message)=>{const end=Date.now()+12000;while(Date.now()<end){if(await check())return;await new Promise(resolve=>setTimeout(resolve,20));}assert.fail(message);};
 const spec=action=>action==='override'?{open:'Override cold-flow requirement',dialog:'Override nitrogen cold flow',confirm:'Confirm override',success:'Cold-flow override saved',path:'/api/gse/cold-flow/override',body:{reason:'Original isolated cold-flow reason'}}:{open:'Reset operation checks',dialog:'Reset operation checks',confirm:'Confirm reset',success:'Operation checks reset',path:'/api/gse/qualification/reset',body:{}};
 async function open(send=true){
   session.permissions.send_commands=send;session.expires_at_ms=Date.now()+3600000;
   gseStatus={phase:'idle',message:'Qualification owner fixture',qualification:{self_test:true,nitrogen_test:false,fill_target:false,fault:null},topology_stable:true,mission_stage:'idle',storage_error:null,nitrogen_passed:false,self_test_locked:false,current_step_psi:0,pressure_psi:0,baseline:null,valves:[false,false,false,false,false],request_gate:{hitl_mode:true,flight_state:'Idle',prelaunch:true,button_interlock_satisfied:true}};
   const context=await browser.newContext({viewport:{width:1280,height:1000}});
   await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
   await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
   await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{socket.onMessage(message=>commands.push(JSON.parse(message)));const timer=setInterval(()=>socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy})),100);socket.onClose(()=>clearInterval(timer));});
   const page=await context.newPage();page.setDefaultTimeout(12000);page.on('pageerror',error=>errors.push(error.message));await page.goto(origin+'/dashboard');await page.locator('.gs26-dashboard-shell').waitFor();
   await page.getByRole('button',{name:/Choose tab/}).click();const actions=page.getByRole('button',{name:'Actions',exact:true});
   if(send){await actions.click();await page.locator('section[aria-label="Operational qualification"]').waitFor();await page.locator('section[aria-label="Operational qualification"]').evaluate(element=>{window.originalQualificationSection=element;});}
   return{context,page,section:page.locator('section[aria-label="Operational qualification"]')};
 }
 async function dialog(view,action){const s=spec(action);await view.section.getByRole('button',{name:s.open,exact:true}).click();const modal=view.page.getByRole('dialog',{name:s.dialog,exact:true});await modal.waitFor();if(action==='override')await modal.getByRole('textbox',{name:'Cold-flow override reason',exact:true}).fill('  Original isolated cold-flow reason  ');return modal;}
 async function replaceSameUserAuthority(page){const marker=session.expires_at_ms+12345;session.expires_at_ms=marker;await waitFor(()=>page.evaluate(marker=>JSON.parse(localStorage.getItem('auth_session_v1')||'{}').entries?.some(row=>row.session.session.expires_at_ms===marker),marker),'actual session-status poll publishes replacement same-user authority');assert(await page.evaluate(()=>window.originalQualificationSection?.isConnected),'same qualification component stays mounted across replacement');}
 const readonly=await open(false);for(const action of ['override','reset']){const target=readonly.page.getByRole('button',{name:spec(action).open,exact:true});assert(await target.count()===0||await target.isDisabled(),'viewer cannot open qualification mutation');}assert.deepEqual(qualificationWrites,[]);await readonly.context.close();
 for(const action of ['override','reset']){
   const view=await open(),s=spec(action),count=qualificationWrites.length;const old=await dialog(view,action);
   await replaceSameUserAuthority(view.page);await old.waitFor({state:'detached'});assert.equal(qualificationWrites.length,count,'old dialog never adopts replacement owner');
   const fresh=await dialog(view,action);await fresh.getByRole('button',{name:s.confirm,exact:true}).click();await view.page.getByText(s.success,{exact:true}).waitFor();assert.equal(qualificationWrites.length,count+1);assert.deepEqual(qualificationWrites.at(-1),{path:s.path,body:s.body,authorization:'Bearer ui-token'});await view.context.close();
 }
 for(const action of ['override','reset']){
   const view=await open(),s=spec(action),count=qualificationWrites.length;mutationHold=true;mutationCode=401;
   const modal=await dialog(view,action),confirm=modal.getByRole('button',{name:s.confirm,exact:true});await confirm.click();await waitFor(()=>qualificationWrites.length===count+1,'original mutation reaches only original station');
   if(action==='override')await modal.getByRole('textbox',{name:'Cold-flow override reason',exact:true}).fill('Edited after the original request was captured');
   await confirm.dispatchEvent('click');assert.equal(qualificationWrites.length,count+1,'pending duplicate handler cannot enqueue another mutation');assert.deepEqual(qualificationWrites.at(-1),{path:s.path,body:s.body,authorization:'Bearer ui-token'});
   await replaceSameUserAuthority(view.page);mutationHold=false;mutationCode=200;for(const resolve of qualificationWaiters.splice(0))resolve();await modal.waitFor({state:'detached'});await view.page.waitForTimeout(200);
   assert((await view.page.evaluate(()=>localStorage.getItem('auth_session_v1')))?.includes('ui-token'),'old qualification401 never clears replacement login');assert.equal(await view.page.getByText(s.success,{exact:true}).count(),0,'old response never reports a new-owner success');assert.equal(qualificationWrites.length,count+1,'no automatic retry after owner replacement');
   const fresh=await dialog(view,action);await fresh.getByRole('button',{name:s.confirm,exact:true}).click();await view.page.getByText(s.success,{exact:true}).waitFor();assert.equal(qualificationWrites.length,count+2,'fresh deliberate confirmation remains usable');assert.deepEqual(qualificationWrites.at(-1),{path:s.path,body:s.body,authorization:'Bearer ui-token'});await view.context.close();
 }
 for(const action of ['override','reset']){
   const view=await open(),s=spec(action),count=qualificationWrites.length;const modal=await dialog(view,action);
   gseStatus.request_gate.flight_state='Ascent';await waitFor(()=>view.section.getByRole('button',{name:'Reset operation checks',exact:true}).isDisabled(),'current operation state removes qualification permission');
   await modal.getByRole('button',{name:s.confirm,exact:true}).dispatchEvent('click');assert.equal(qualificationWrites.length,count,'stale enabled confirmation cannot bypass current operation policy');await view.context.close();
 }
 assert.deepEqual(commands,[],'qualification changes never emit websocket/output commands');assert.deepEqual(errors,[]);
 console.log('PASS actual both qualification dialogs: original-owner opening/confirm, same-mounted late401, original serialized reason, pending dedup, fresh explicit renewal, current policy and read-only gates');
}finally{mutationHold=false;for(const resolve of qualificationWaiters.splice(0))resolve();await browser.close();await new Promise(resolve=>server.close(resolve));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
