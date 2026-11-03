// Real Wasm UI with a synthetic authenticated HTTP/WS backend. No hardware.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8'),boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
const status=JSON.parse(fs.readFileSync('tests/fixtures/board-recovery/status-example.json'));status.principal.username='ui-test';
const prepare=JSON.parse(fs.readFileSync('tests/fixtures/board-recovery/prepare-example.json'));
fixture=fixture.replace('let lists={',`let boardRecovery=${JSON.stringify(status)},challengeTemplate=${JSON.stringify(prepare)},prepared=[],confirmed=[],queried=[],ttl=30000,delayStatusMs=0;
const boardRecoveryInitial=JSON.parse(JSON.stringify(boardRecovery));
let lists={`);
fixture=fixture.replace("  if(pathname==='/api/checklists'||",`  if(pathname==='/api/board-recovery/status'){if(delayStatusMs){await new Promise(r=>setTimeout(r,delayStatusMs));boardRecovery.boards.find(b=>b.id==='AB').label='AB delayed fixture';}res.setHeader('Content-Type','application/json');res.end(JSON.stringify(boardRecovery));return;}
 if(pathname.startsWith('/api/board-recovery/')){
 assert.equal(req.headers.authorization,'Bearer ui-token');let text='';for await(const data of req)text+=data;const body=JSON.parse(text);
 res.setHeader('Content-Type','application/json');
 if(pathname.endsWith('/query')){assert.equal(boardRecovery.boards.find(b=>b.id===body.board)?.capabilities.query,true);queried.push(body);res.end(JSON.stringify({accepted:true,operation:null,message:'Synthetic query accepted; no outputs.'}));return;}
 if(pathname.endsWith('/prepare')){prepared.push(body);const targets=body.board?[body.board]:boardRecovery.boards.map(b=>b.id);const confirmation=body.action==='recover_abort'?'RECOVER '+body.board:body.action==='resync'?'RESYNC '+body.board:body.action==='change_mode'?'CHANGE MODE '+body.mode.toUpperCase():'RESET ALL STARTUP';const summary=body.action==='reset_startup'?challengeTemplate.summary:'Operator '+body.action+' for '+targets.join(', ')+'. Preserve clock, calibration and network power. Readback is not physical valve position.';const result={...challengeTemplate,summary,action:body.action,targets,backend_instance:boardRecovery.backend_instance,principal_context:boardRecovery.principal.context,reset_epoch:boardRecovery.reset_epoch,expires_in_ms:ttl,confirmation};boardRecovery.confirmation={ticket:result.ticket,backend_instance:result.backend_instance,principal_context:result.principal_context,reset_epoch:result.reset_epoch,action:result.action,targets:result.targets,expires_in_ms:result.expires_in_ms};res.end(JSON.stringify(result));return;}
 if(pathname.endsWith('/confirm')){assert.deepEqual(Object.keys(body).sort(),['backend_instance','confirmation','principal_context','reset_epoch','ticket']);assert.equal(body.backend_instance,boardRecovery.backend_instance);assert.equal(body.principal_context,boardRecovery.principal.context);assert.equal(body.reset_epoch,boardRecovery.reset_epoch);assert.equal(body.ticket,boardRecovery.confirmation?.ticket);boardRecovery.confirmation=null;confirmed.push(body);boardRecovery.operation={id:'c'.repeat(32),action:prepared.at(-1).action,phase:'verifying',targets:prepared.at(-1).board?[prepared.at(-1).board]:boardRecovery.boards.map(b=>b.id),completed:false,success:null,age_ms:0,message:'Awaiting actual board readbacks; ACK alone is insufficient.'};res.end(JSON.stringify({accepted:true,operation:boardRecovery.operation,message:'Synthetic accepted; not device confirmed.'}));return;}
 throw Error('Unexpected recovery path '+pathname);
 }
 if(pathname==='/api/checklists'||`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1280,height:1000}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const sockets=new Set(),commands=[];await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>{sockets.add(s);s.onClose(()=>sockets.delete(s));s.onMessage(msg=>commands.push(JSON.parse(msg)));});
 const page=await context.newPage(),errors=[],dialogs=[];page.on('pageerror',e=>errors.push(e.message));page.on('dialog',async d=>{dialogs.push(d.type());await d.dismiss();});
 await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 await page.getByRole('link',{name:'Connection',exact:true}).click();await page.getByRole('heading',{name:'Board state and recovery'}).waitFor();
 const panel=page.locator('.gs26-board-recovery'),ab=panel.locator('[data-board="AB"]'),rf=panel.locator('[data-board="RF"]');
 await ab.getByText('Actual board state: Startup (fresh)',{exact:true}).waitFor();await rf.getByText('Actual board state: Unknown (unconfirmed/stale)',{exact:true}).waitFor();
 assert(await rf.getByRole('button',{name:'Query board'}).isDisabled());assert(await rf.getByRole('button',{name:'Reset metadata to Startup…'}).isDisabled());assert.match(await rf.innerText(),/Cached application metadata only; no physical recovery/);
 assert(await panel.getByRole('button',{name:'Reset ALL to Startup…'}).isDisabled());assert(await panel.getByRole('combobox',{name:'Supported runtime mode'}).isDisabled());
 assert.equal(prepared.length+confirmed.length+queried.length,0,'mount/poll must not send queries or actions');
 await ab.getByRole('button',{name:'Query board'}).click();await page.getByText(/Synthetic query accepted/).waitFor();assert.deepEqual(queried,[{board:'AB'}]);
 await ab.getByRole('button',{name:'Recover to Idle…'}).click();let modal=page.getByRole('alertdialog');await modal.waitFor();
 assert.match(await modal.innerText(),/Frozen targets: AB/);assert.equal(confirmed.length,0);assert(await modal.getByRole('button',{name:'Confirm recovery action'}).isDisabled());
 // Expiring actual state/capability fields cannot destroy a live server ticket.
 const liveAB=boardRecovery.boards.find(b=>b.id==='AB'),savedAB=JSON.parse(JSON.stringify(liveAB));
 Object.assign(liveAB,{age_ms:15000,state:null,abort_latched:null,safe_outputs:null,commanded_outputs:null});liveAB.capabilities={...liveAB.capabilities,recover:false,reset_startup:false};
 await page.waitForTimeout(2250);assert(await modal.isVisible(),'server30s identity survives real2s null/stale readback fields');assert.equal(confirmed.length,0);assert.equal(queried.length,1,'stale confirmation never autoqueries boards');
 assert.equal(await modal.evaluate(e=>getComputedStyle(e).fontFamily),await panel.evaluate(e=>getComputedStyle(e).fontFamily),'native DOM confirmation inherits system font');
 Object.assign(liveAB,savedAB);
 await modal.getByRole('button',{name:'Cancel',exact:true}).click();await modal.waitFor({state:'detached'});assert.equal(confirmed.length,0);
 // Server revocation cancels the frozen confirmation; no silent reprepare/retry.
 await ab.getByRole('button',{name:'Resync…'}).click();await modal.waitFor();boardRecovery.confirmation=null;await modal.waitFor({state:'detached'});assert.equal(confirmed.length,0);const before=prepared.length;
 await page.reload();await page.getByRole('link',{name:'Connection',exact:true}).click();await panel.waitFor();await ab.getByRole('button',{name:'Recover to Idle…'}).waitFor();assert.equal(prepared.length,before,'reconnect must not repeat preparation/action');
 // Explicit confirmation returns a pending result, never a fictitious safe position.
 await ab.getByRole('button',{name:'Recover to Idle…'}).click();await modal.waitFor();await modal.getByLabel('Type RECOVER AB to confirm').fill('RECOVER AB');await modal.getByRole('button',{name:'Confirm recovery action'}).click();await page.getByText(/Request accepted; awaiting fresh device confirmation/).waitFor();assert.equal(confirmed.length,1);assert((await panel.innerText()).includes('Pending / not confirmed'));assert.doesNotMatch(await panel.innerText(),/Confirmed by board readbacks/);
 boardRecovery.operation=null;
 // All seven supported/fresh identities are required before Reset ALL is offered.
 const template=JSON.parse(JSON.stringify(boardRecovery.boards.find(b=>b.id==='AB')));for(const b of boardRecovery.boards){Object.assign(b,JSON.parse(JSON.stringify(template)),{id:b.id,label:b.label});}
 await panel.getByRole('button',{name:'Reset ALL to Startup…'}).waitFor();await page.waitForFunction(()=>!Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='Reset ALL to Startup…')?.disabled);
 await panel.getByRole('button',{name:'Reset ALL to Startup…'}).click();await modal.waitFor();assert.match(await modal.innerText(),/OPEN/);for(const width of[320,390,844,1280]){await page.setViewportSize({width,height:1000});assert(await modal.evaluate(e=>e.getBoundingClientRect().left>=0&&e.getBoundingClientRect().right<=innerWidth),'DOM modal fits '+width);}await modal.getByLabel('Type RESET ALL STARTUP to confirm').fill('RESET ALL STARTUP');await modal.getByRole('button',{name:'Confirm recovery action'}).click();await page.getByText(/Request accepted; awaiting fresh device confirmation/).waitFor();assert.equal(confirmed.length,2);assert.equal(boardRecovery.operation.targets.length,7);boardRecovery.operation=null;
 // Only advertised runtime switching enables a mode request, and acceptance
 // must not optimistically relabel the currently active server mode.
 boardRecovery.mode_capability.runtime_switch_supported=true;boardRecovery.mode_capability.supported_modes=['normal','testing','hitl','test_fire'];
 const modeSelect=panel.getByRole('combobox',{name:'Supported runtime mode'});await page.waitForFunction(()=>!document.querySelector('select[aria-label="Supported runtime mode"]')?.disabled);await modeSelect.selectOption('testing');await panel.getByRole('button',{name:'Review mode change'}).click();await modal.waitFor();await modal.getByLabel('Type CHANGE MODE TESTING to confirm').fill('CHANGE MODE TESTING');await modal.getByRole('button',{name:'Confirm recovery action'}).click();await page.getByText(/Request accepted; awaiting fresh device confirmation/).waitFor();assert.equal(confirmed.length,3);assert((await panel.innerText()).includes('Active: normal;'));
 boardRecovery.mode_capability.current='testing';boardRecovery.operation={...boardRecovery.operation,completed:true,success:true,phase:'completed'};await panel.getByText('Active: testing; server build: normal.',{exact:true}).waitFor();boardRecovery.operation=null;
 // Backend/principal/reset transitions each cancel rather than retarget a ticket.
 for(const field of ['backend_instance','principal','reset_epoch']){
  await ab.getByRole('button',{name:'Resync…'}).click();await modal.waitFor();if(field==='principal')boardRecovery.principal.context='d'.repeat(32);else boardRecovery[field]=field==='reset_epoch'?'0000000000000002':'e'.repeat(32);await modal.waitFor({state:'detached'});assert.equal(confirmed.length,3);
 }
 // A response held in transport beyond the2sreadback bound must not
 // reappear as fresh merely because it has just reached the client.
 delayStatusMs=2500;await ab.getByRole('heading',{name:'AB delayed fixture (AB)',exact:true}).waitFor();assert(await ab.getByRole('button',{name:'Recover to Idle…'}).isDisabled(),'transport delay must be included in readback age');delayStatusMs=0;
 for(const width of[320,390,844,1280]){await page.setViewportSize({width,height:1000});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'populated board cards fit '+width);}
 // An older backend removes capability; the UI never guesses a recovery write.
 boardRecovery={};await panel.getByText('Recovery status unavailable. No control action is enabled.',{exact:true}).waitFor();assert.equal(confirmed.length,3);
 for(const width of[320,390,844,1280]){await page.setViewportSize({width,height:1000});assert(await panel.evaluate(e=>e.getBoundingClientRect().width<=innerWidth),'panel width '+width);assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'page overflow '+width);}
 assert.deepEqual(dialogs,[],'all confirmation stays in DOM');assert.deepEqual(commands,[],'no direct telemetry command escape');assert.deepEqual(errors,[]);await context.close();
 console.log('PASS realWasm Connectivity: actual/unknown states, dedicated endpoint capability, GET-only poll, query explicit, DOM challenge/cancel/consume, immutable serveridentity/stale-readback separation, server/backend/principal/epoch invalidation, pending/readback honesty, no autoretry, all7 reset scope, mode capabilities, mobile widths');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
if(process.env.GS_RECOVERY_FIXTURE_DEBUG)fs.writeFileSync(process.env.GS_RECOVERY_FIXTURE_DEBUG,fixture);
try{if(!process.env.GS_RECOVERY_PREPARE_ONLY)await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
