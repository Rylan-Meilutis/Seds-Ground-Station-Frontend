// Actual Wasm and isolated station APIs. No station or hardware commands.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE||'/tmp/rme-browser-profile/node_modules/playwright/index.mjs')).href));
fixture=fixture.replace('let lists={',`let boardAbort={version:10,active_errors:[],required_boards:['FC'],missing_required_state:[],history_error:null,history_unknown_boards:[],retraction:{configured:false,enforced:false,allowed:true,phase:'inactive',message:'No plumbing fixture'}};
let holdRefresh=false,refreshFails=false,releaseRefresh,refreshStarted,refreshStart=new Promise(resolve=>refreshStarted=resolve),connections=0;
let lists={`);
fixture=fixture.replace("let actionPolicy=example('action-policy');",`let actionPolicy=example('action-policy');actionPolicy.controls.push({cmd:'Launch',enabled:true,blink:'slow',actuated:false},{cmd:'ToggleBoardStateProofInterlock',enabled:true,blink:'none',actuated:false});`);
fixture=fixture.replace("const dashboardLayout=example('layout.full');","const dashboardLayout=example('layout.full');dashboardLayout.actions_tab.actions.push({...dashboardLayout.actions_tab.actions[0],cmd:'Launch',label:'Launch'});");
fixture=fixture.replace("  if(pathname==='/api/checklists'||",`  if(pathname==='/api/alerts'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify([{timestamp_ms:Date.now(),severity:'error',message:'Historical fixture error'}]));return;}
  if(pathname==='/api/alerts/ack'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({warning_ack_timestamp_ms:0,error_ack_timestamp_ms:Date.now()}));return;}
  if(pathname==='/api/station_mode'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({local_build_mode:'hitl',effective_mode:'hitl',synchronized:true,primary:true}));return;}
  if(pathname==='/api/board_abort'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(boardAbort));return;}
  if(pathname==='/api/board_abort/refresh'){res.setHeader('Content-Type','application/json');if(refreshFails){res.statusCode=503;res.end('History source unavailable');return;}const snapshot=JSON.parse(JSON.stringify(boardAbort));refreshStarted();if(holdRefresh)await new Promise(resolve=>releaseRefresh=resolve);res.end(JSON.stringify({message:'Fixture readiness response',snapshot}));return;}
  if(pathname==='/api/checklists'||`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1280,height:1000}});
 await context.addInitScript(({session,origin})=>localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]})),{session,origin});
 const sockets=new Set(),commands=[],errors=[];await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{connections++;sockets.add(socket);socket.onClose(()=>sockets.delete(socket));socket.onMessage(data=>commands.push(data));setTimeout(()=>{socket.send(JSON.stringify({ty:'BoardAbort',data:boardAbort}));socket.send(JSON.stringify({ty:'ActionPolicy',data:actionPolicy}));},150);});
 const push=(ty,data)=>{for(const socket of sockets)socket.send(JSON.stringify({ty,data}));};
 const page=await context.newPage();page.on('pageerror',error=>errors.push(error.message));await page.goto(origin+'/dashboard');const rail=page.getByRole('navigation',{name:'Mission navigation'});await rail.getByRole('link',{name:'Actions',exact:true}).click();
 const launch=page.getByRole('button',{name:/^Launch(?: |$)/}).first(),check=page.getByRole('button',{name:'Check launch readiness',exact:true});
 await launch.waitFor();console.log("Launch rendered");await page.waitForFunction(()=>[...document.querySelectorAll('button')].some(e=>e.textContent.trim().startsWith("Launch")&&!e.disabled));
 console.log("Initial Launch enabled");
 // Same-version async history fault arrives after a healthy HTTP request began.
 holdRefresh=true;await check.click();await refreshStart;console.log("Healthy HTTP held; injecting history failure");
 boardAbort={...boardAbort,history_error:'Corrupt saved Abort history fixture'};push('BoardAbort',boardAbort);push('ActionPolicy',actionPolicy);
 await page.waitForFunction(()=>[...document.querySelectorAll('button')].some(e=>e.textContent.trim().startsWith("Launch")&&e.disabled),null,{timeout:3000});
 await page.getByText(/Launch disabled: Abort history unavailable: Corrupt saved Abort history fixture/).first().waitFor();
 releaseRefresh();await page.getByText('A newer station report was received; current readiness was retained.',{exact:true}).waitFor();assert(await launch.isDisabled());
 await rail.getByRole('link',{name:'Errors',exact:true}).click();await page.locator('.gs26-active-history-error').getByText('Corrupt saved Abort history fixture',{exact:true}).waitFor();
 const acknowledge=page.getByRole('button',{name:'Acknowledge',exact:true});await page.waitForFunction(()=>[...document.querySelectorAll('button')].some(e=>e.textContent==='Acknowledge'&&!e.disabled));await acknowledge.click();
 assert.equal(await page.locator('.gs26-active-history-error').count(),1,'acknowledgment cannot clear active history failure');assert.equal(await rail.getByRole('link',{name:'Errors',exact:true}).getAttribute('data-alert'),'error','active history error retains header attention');
 const omitted={...boardAbort,version:11};delete omitted.history_error;push('BoardAbort',omitted);push('ActionPolicy',actionPolicy);await page.waitForTimeout(80);assert.equal(await page.locator('.gs26-active-history-error').count(),1,'absent field is not a healthy report');
 push('BoardAbort',{...boardAbort,version:9,history_error:null});await page.waitForTimeout(80);assert.equal(await page.locator('.gs26-active-history-error').count(),1,'older snapshot cannot clear failure');
 await rail.getByRole('link',{name:'Actions',exact:true}).click();await page.waitForFunction(()=>[...document.querySelectorAll('button')].some(e=>e.textContent==='Check launch readiness'&&!e.disabled));refreshFails=true;await check.click();await page.getByText(/HTTP 503: History source unavailable/).waitFor();assert(await launch.isDisabled());refreshFails=false;
 // Explicit healthy history does not bypass backend-scoped missing state.
 boardAbort={...boardAbort,version:12,history_error:null,missing_required_state:['FC'],history_unknown_boards:['FC']};push('BoardAbort',boardAbort);push('ActionPolicy',actionPolicy);await page.getByText('Launch disabled: fresh required board state is missing for FC.',{exact:true}).first().waitFor();assert(await launch.isDisabled());
 await rail.getByRole('link',{name:'Errors',exact:true}).click();await page.locator('.gs26-active-history-error').waitFor({state:'detached'});
 await rail.getByRole('link',{name:'Actions',exact:true}).click();boardAbort={...boardAbort,version:13,missing_required_state:[]};push('BoardAbort',boardAbort);push('ActionPolicy',actionPolicy);await page.waitForFunction(()=>[...document.querySelectorAll('button')].some(e=>e.textContent.trim().startsWith("Launch")&&!e.disabled));
 assert(await page.getByRole('button',{name:'Board state checks: off',exact:true}).isVisible(),'HITL optional proof remains primary-confirmed');assert.match(await page.locator('section[aria-label="Launch readiness"]').innerText(),/Stored history is unconfirmed for FC/);
 // Old HTTP success across a reconnect must not clear a new source epoch.
 while(await check.isDisabled())await page.waitForTimeout(20);holdRefresh=true;refreshStart=new Promise(resolve=>refreshStarted=resolve);await check.click();await refreshStart;const before=connections;for(const socket of sockets)socket.close();const deadline=Date.now()+12000;while(connections<=before&&Date.now()<deadline)await page.waitForTimeout(20);assert(connections>before,'reconnected fixture');
 boardAbort={...boardAbort,version:1,history_error:'New connection history failure'};push('BoardAbort',boardAbort);push('ActionPolicy',actionPolicy);await page.getByText(/New connection history failure/).first().waitFor();releaseRefresh();
 // The held reply belongs to the retired connection: it is dropped, never applied. The new
 // connection's failure stays, Launch stays disabled, and the check is usable again.
 await page.waitForTimeout(600);assert.equal(await page.getByText('Fixture readiness response',{exact:true}).count(),0,'old-connection reply is not reported as current');
 await page.getByText(/New connection history failure/).first().waitFor();assert(await launch.isDisabled());
 await page.waitForFunction(()=>[...document.querySelectorAll('button')].some(e=>e.textContent==='Check launch readiness'&&!e.disabled),null,{timeout:5000});
 assert.deepEqual(commands,[],'readiness, acknowledgments and reports never send Launch or interlock commands');assert.deepEqual(errors,[]);console.log('PASS history error current-source retention, same-version lateHTTP, absent/stale/failed source, acknowledgment independence, scopedmissing and HITLoptional rawunknown');
}finally{releaseRefresh?.();await browser.close();server.closeAllConnections();await new Promise(resolve=>server.close(resolve));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
