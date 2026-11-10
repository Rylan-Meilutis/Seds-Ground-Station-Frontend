import assert from 'node:assert/strict';
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

function replaceFixture(oldText,newText){assert(fixture.includes(oldText),'Fixture binding changed: '+oldText);fixture=fixture.replace(oldText,newText);}
replaceFixture('let boardRecovery=', 'let scopeOptional=[],lastTargets=[];let boardRecovery=');
replaceFixture('const targets=body.board?[body.board]:boardRecovery.boards.map(b=>b.id);',
 "const policy=boardRecovery.reset_startup_policy;const targets=body.board?[body.board]:policy?['RF','FC','PB','AB','VB','GB','DAQ'].filter(id=>policy.base_required_boards.includes(id)||scopeOptional.includes(id)):boardRecovery.boards.map(b=>b.id);");
replaceFixture(":'RESET ALL STARTUP';const summary=", ":policy?('RESET ALL STARTUP; VERIFY '+(targets.length?targets.join(' '):'NONE')):'RESET ALL STARTUP';const summary=");
replaceFixture('boardRecovery.confirmation={ticket:result.ticket', "lastTargets=targets;if(policy)Object.assign(result,{global_startup_best_effort:true,required_proof_boards:policy.base_required_boards,unselected_board_verification:'unverified_best_effort'});boardRecovery.confirmation={ticket:result.ticket");
replaceFixture("targets:prepared.at(-1).board?[prepared.at(-1).board]:boardRecovery.boards.map(b=>b.id)", "targets:lastTargets");
replaceFixture("res.end(JSON.stringify({accepted:true,operation:boardRecovery.operation,message:'Synthetic accepted; not device confirmed.'}))", "if(boardRecovery.reset_startup_policy)Object.assign(boardRecovery.operation,{global_startup_best_effort:true,selected_proof_count:lastTargets.length,unselected_board_verification:'unverified_best_effort'});res.end(JSON.stringify({accepted:true,operation:boardRecovery.operation,message:'Synthetic accepted; not device confirmed.'}))");
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1280,height:1000}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const commands=[],errors=[],dialogs=[];await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(m=>commands.push(JSON.parse(m))));
 const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));page.on('dialog',async d=>{dialogs.push(d.type());await d.dismiss();});
 await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();await page.getByRole('link',{name:'Connection',exact:true}).click();
 // Reset all boards lives in Recovery tools > Reset; its result is shown there too.
 await page.locator('.gs26-network-health').getByRole('button',{name:'Recovery tools',exact:true}).click();
 const panel=page.getByRole('dialog',{name:'Recovery tools',exact:true});await panel.getByRole('tab',{name:'Reset',exact:true}).click();
 const button=panel.getByRole('button',{name:'Reset all boards to Idle…',exact:true}),modal=page.getByRole('alertdialog');await button.waitFor();
 const resetEnabled=()=>page.waitForFunction(()=>Array.from(document.querySelectorAll('#gs26-recovery-tools button')).find(b=>b.textContent==='Reset all boards to Idle…')?.disabled===false);
 assert(await button.isDisabled(),'legacy still needs all seven fresh readbacks');
 for(const mode of ['normal','test_fire','hitl']){
  boardRecovery=JSON.parse(JSON.stringify(boardRecoveryInitial));
  boardRecovery.mode_capability={...boardRecovery.mode_capability,current:mode,build_mode:mode,supported_modes:[mode],runtime_switch_supported:false};
  const required=mode==='normal'?['RF','FC','PB','AB','VB','GB','DAQ']:mode==='test_fire'?['AB','VB','GB','DAQ']:[];
  // The station requires no boards up front: a reset covers whichever boards are
  // available, and PREPARE freezes that set as the proof targets.
  scopeOptional=required;boardRecovery.reset_startup_policy={version:1,global_startup_best_effort:true,mode,base_required_boards:[],optional_proofs:true};
  for(const b of boardRecovery.boards){b.age_ms=null;b.state=null;b.boot=null;b.nonce=null;b.epoch=null;b.capabilities={query:false,reset_startup:false,endpoint_verified:false};}
  await resetEnabled();
  const before=confirmed.length;await button.click();await modal.waitFor();assert.match(await modal.innerText(),/GLOBAL Startup request targets ALL boards/);assert.doesNotMatch(await modal.innerText(),/Only the frozen targets above are requested/);
  for(const width of[320,390,844,1280]){await page.setViewportSize({width,height:1000});assert(await modal.evaluate(e=>e.getBoundingClientRect().left>=0&&e.getBoundingClientRect().right<=innerWidth),'C12modal fits '+mode+' '+width);assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'C12page fits '+width);}
  const phrase='RESET ALL STARTUP; VERIFY '+(required.length?required.join(' '):'NONE');assert.equal(await modal.locator('input').count(),0,'nothing to type');await modal.getByRole('button',{name:'Confirm recovery action'}).click();await page.waitForFunction(n=>document.body.innerText.includes('Request accepted'),before);assert.equal(confirmed.length,before+1);assert.equal(confirmed.at(-1).confirmation,phrase);assert.deepEqual(Object.keys(prepared.at(-1)).sort(),['action','backend_instance','board','mode','reset_epoch']);
  boardRecovery.operation={...boardRecovery.operation,completed:true,success:true,phase:'completed',reset_applied_state:0};
  if(mode==='hitl'){await panel.getByText('Host Startup reset; no board proof — completed',{exact:true}).waitFor();assert.doesNotMatch(await panel.innerText(),/Confirmed by board readbacks/);}
  else await panel.getByText('Selected boards verified; global broadcast best effort — completed',{exact:true}).waitFor();
  assert.match(await panel.innerText(),/unselected boards remain unverified/i);boardRecovery.operation=null;
 }
 // Ordinary client/server TestFire scope may include actually observed optional Abort targets.
 boardRecovery.mode_capability={...boardRecovery.mode_capability,current:'test_fire',build_mode:'test_fire',supported_modes:['test_fire']};boardRecovery.reset_startup_policy={version:1,global_startup_best_effort:true,mode:'test_fire',base_required_boards:[],optional_proofs:true};scopeOptional=['FC','AB','VB','GB','DAQ'];
 await resetEnabled();await button.click();await modal.waitFor();assert.match(await modal.innerText(),/Proof targets: FC, AB, VB, GB, DAQ/);await modal.getByRole('button',{name:'Cancel',exact:true}).click();await modal.waitFor({state:'detached'});
 // Actual observed status changes retire confirmations, never silently retry.
 for(const field of ['mode','reset_epoch','principal','permission']){
  await button.click();await modal.waitFor();const count=confirmed.length,preparedBefore=prepared.length;
  if(field==='mode')boardRecovery.mode_capability.current='normal';else if(field==='reset_epoch')boardRecovery.reset_epoch='0000000000000002';else if(field==='principal')boardRecovery.principal.context='d'.repeat(32);else boardRecovery.permissions.reset_startup=false;
  await modal.waitFor({state:'detached'});assert.equal(confirmed.length,count);assert.equal(prepared.length,preparedBefore,'no automatic reprepare');
  boardRecovery.mode_capability.current='test_fire';boardRecovery.permissions.reset_startup=true;await resetEnabled();
 }
 boardRecovery.reset_startup_policy={...boardRecovery.reset_startup_policy,version:2};await page.waitForFunction(()=>Array.from(document.querySelectorAll('#gs26-recovery-tools button')).find(b=>b.textContent==='Reset all boards to Idle…')?.disabled);
 boardRecovery.reset_startup_policy=null;await page.waitForTimeout(1100);assert(await button.isDisabled(),'null policy never narrows legacy unknown-board requirements');
 const beforeMalformed={prepared:prepared.length,confirmed:confirmed.length};boardRecovery.mode_capability={...boardRecovery.mode_capability,current:'hitl',build_mode:'hitl',supported_modes:['hitl']};boardRecovery.reset_startup_policy={version:1,global_startup_best_effort:true,mode:'hitl',optional_proofs:true};await panel.getByText('Recovery status unavailable. No control action is enabled.',{exact:true}).waitFor();assert.equal(prepared.length,beforeMalformed.prepared);assert.equal(confirmed.length,beforeMalformed.confirmed);
 assert.deepEqual(commands,[],'no telemetry command escape');assert.deepEqual(dialogs,[],'no browser popup');assert.deepEqual(errors,[]);await context.close();console.log('PASS actualWasm ordinaryclient C12 normal/testfire/hitl exactscope NONE hostonly/selected/global labels, optionalAbort, stale mode/epoch/principal/permission retirement, legacy/null/unknown failclosed, no automatic retry or directcommand');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
