// Actual compiled Wasm against a loopback synthetic station; no hardware.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/valve-toggle-browser.mjs','utf8');
const boundary=source.indexOf('try {\n const commands=');
if(boundary<0)throw Error('Shared fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href));
fixture=fixture.replace("const dashboardLayout=example('layout.full');",`const dashboardLayout=example('layout.full');
dashboardLayout.actions_tab.actions=[{label:'Abort',cmd:'Abort',group:'Manual GSE valves',border:'#53657c',bg:'#142033',fg:'#ffffff'}];
actionPolicy={key_enabled:true,software_buttons_enabled:true,controls:[{cmd:'Turbocharge',enabled:true,blink:'none',actuated:null}]};
gseStatus={phase:'paused',message:'Paused synthetic fill',nitrogen_passed:true,self_test_locked:false,current_step_psi:0,pressure_psi:200,valves:[false,false,false,false,false],qualification:{fill_target:true},turbocharge:{disabled_reason:'Synthetic station pressure target guard'}};`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:390,height:844}});
 await context.addInitScript(({session,origin})=>{localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base);}catch{return null;}};},{session,origin});
 const requests=[],sockets=new Set();await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{sockets.add(socket);socket.onMessage(message=>requests.push(JSON.parse(String(message)).cmd));const timer=setInterval(()=>socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy})),100);socket.onClose(()=>{clearInterval(timer);sockets.delete(socket);});});
 const page=await context.newPage(),errors=[],dialogs=[];page.setDefaultTimeout(8000);page.on('pageerror',e=>errors.push(e.message));page.on('dialog',async d=>{dialogs.push(d.type());await d.dismiss();});await page.goto(origin+'/dashboard');await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Actions',exact:true}).click().catch(async e=>{console.error((await page.locator('body').innerText()).slice(0,8000));throw e});
 const turbo=page.locator('button[data-dispatch-command="Turbocharge"]');
 await turbo.waitFor({timeout:8000});assert.equal(await turbo.count(),1,'legacy custom Actions layout gains exactly one Turbocharge control');
 await page.waitForFunction(()=>!document.querySelector('button[data-dispatch-command="Turbocharge"]')?.disabled);
 assert.equal(requests.length,0,'mount/poll sends no commands');
 await turbo.click();await page.waitForFunction(()=>document.querySelector('button[data-dispatch-command="Turbocharge"]')!==null);await page.waitForTimeout(250);assert.deepEqual(requests,['Turbocharge'],'one activation sends original command exactly once');
 actionPolicy.controls[0].enabled=false;await page.waitForFunction(()=>document.querySelector('button[data-dispatch-command="Turbocharge"]')?.disabled);assert.equal(requests.length,1,'backend policy refusal sends nothing');
 actionPolicy.controls=[];await page.getByText('Command missing from backend policy',{exact:true}).first().waitFor();assert(await turbo.isDisabled());
 for(const width of [320,390,844,1280]){await page.setViewportSize({width,height:900});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Actions fit '+width);}
 // The station may advertise the action while this account lacks its specific permission.
 actionPolicy.controls=[{cmd:'Turbocharge',enabled:true,blink:'none',actuated:null}];session.allowed_commands=['Abort'];
 await context.addInitScript(({session,origin})=>localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]})),{session,origin});
 await page.reload();await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Actions',exact:true}).click();
 await turbo.waitFor();assert(await turbo.isDisabled(),'per-command account permission remains required even with enabled backend policy');
 await page.getByText('Not permitted by your account',{exact:true}).first().waitFor();assert.deepEqual(requests,['Turbocharge'],'permission denial never sends command');
 assert.deepEqual(dialogs,[],'actions do not use native/browser confirmation popups');assert.deepEqual(errors,[]);await context.close();
 console.log('PASS actual Wasm legacy/custom layout Turbocharge: one control, exact one command, backend/missing policy gates, no mount command, DOM-only, mobile widths');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
