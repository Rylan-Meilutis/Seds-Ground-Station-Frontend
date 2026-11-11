// Real Wasm UI with a synthetic authenticated HTTP/WS backend. No hardware.
// Network flight states in the header ("In flight" with the FC phase beside
// it, "Fill" with the fill step), inline disabled reasons on the board
// recovery card, and the FC reset to Startup from any state with its one
// station confirmation.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8'),boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
const status=JSON.parse(fs.readFileSync('tests/fixtures/board-recovery/status-example.json'));
status.principal.username='ui-test';status.flight_state=16;status.flight_state_label='In flight';status.fill_phase=null;
const ab=status.boards.find(board=>board.id==='AB'),fc=status.boards.find(board=>board.id==='FC');
Object.assign(fc,{...JSON.parse(JSON.stringify(ab)),id:'FC',label:'Flight Computer',state:11,state_label:'Descent',capabilities:{...ab.capabilities,resync:false,reset_startup_any_state:true},
 fc_phase:{value:11,label:'Descent',source:'fc_state_report',age_ms:1200},reset_startup_any_state_blocked_reason:null,
 reset_fc_startup_warning:'The FC reports Descent. A reset stops its flight evaluation; deployment history is kept.',
 blocked_reasons:{query:null,recover:'Recovery requires GS Startup, Idle or Aborted.',prepare_recover:null,resync:'Not available on this board.',reset_startup_any_state:null}});
const gb=status.boards.find(board=>board.id==='GB');gb.blocked_reasons={query:'Gateway Board firmware has no recovery endpoint.',recover:null,prepare_recover:null,resync:null,reset_startup_any_state:null};
const prepare=JSON.parse(fs.readFileSync('tests/fixtures/board-recovery/prepare-example.json'));
fixture=fixture.replace('let lists={',`const captureDir=${JSON.stringify(process.env.GS_UI_ARTIFACT_DIR||'')};let boardRecovery=${JSON.stringify(status)},challengeTemplate=${JSON.stringify(prepare)},prepared=[],confirmed=[];
const observed=['FC','AB','GB'];
const topology=()=>({generated_ms:Date.now(),simulated:false,nodes:[],links:[],health:observed.map(id=>({sender_id:id,healthy:true,age_ms:0,last_seen_ms:Date.now(),disconnects:0,reconnects:0,rx_observations:1,observed_ms:30000,offline_ms:0,longest_outage_ms:0,current_outage_ms:0}))});
let lists={`);
fixture=fixture.replace("  if(pathname==='/api/checklists'||",`  if(pathname==='/api/network_topology'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(topology()));return;}
  if(pathname==='/api/board-recovery/status'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(boardRecovery));return;}
  if(pathname.startsWith('/api/board-recovery/')){
   assert.equal(req.headers.authorization,'Bearer ui-token');let text='';for await(const data of req)text+=data;const body=JSON.parse(text);
   res.setHeader('Content-Type','application/json');
   if(pathname.endsWith('/prepare')){
    assert.equal(body.action,'reset_fc_startup','only the FC reset is prepared here');assert.equal(body.board,'FC');prepared.push(body);
    const result={...challengeTemplate,action:'reset_fc_startup',targets:['FC'],backend_instance:boardRecovery.backend_instance,principal_context:boardRecovery.principal.context,reset_epoch:boardRecovery.reset_epoch,expires_in_ms:30000,
     summary:'Reset the FC to Startup from its current state. '+boardRecovery.boards.find(b=>b.id==='FC').reset_fc_startup_warning,confirmation:'RESET FC TO STARTUP'};
    delete result.global_startup_best_effort;delete result.required_proof_boards;delete result.unselected_board_verification;
    boardRecovery.confirmation={ticket:result.ticket,backend_instance:result.backend_instance,principal_context:result.principal_context,reset_epoch:result.reset_epoch,action:result.action,targets:result.targets,expires_in_ms:result.expires_in_ms};
    res.end(JSON.stringify(result));return;
   }
   if(pathname.endsWith('/confirm')){assert.equal(body.confirmation,'RESET FC TO STARTUP');assert.equal(body.ticket,boardRecovery.confirmation?.ticket);confirmed.push(body);boardRecovery.confirmation=null;
    res.end(JSON.stringify({accepted:true,operation:null,message:'Synthetic accepted; not device confirmed.'}));return;}
   throw Error('Unexpected recovery path '+pathname);
  }
  if(pathname==='/api/checklists'||`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1280,height:900}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const sockets=new Set();await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 let flight={state:'InFlight',fill_phase:null,fc_phase:{value:11,label:'Descent',source:'fc_state_report',age_ms:1200}};
 await context.routeWebSocket(url=>url.pathname==='/ws',s=>{sockets.add(s);const heartbeat=setInterval(()=>{s.send(JSON.stringify({ty:'NetworkTime',data:{timestamp_ms:Date.now()}}));s.send(JSON.stringify({ty:'FlightState',data:flight}));},400);s.onClose(()=>{clearInterval(heartbeat);sockets.delete(s);});s.send(JSON.stringify({ty:'NetworkTopology',data:topology()}));});
 const page=await context.newPage(),errors=[],dialogs=[];page.on('pageerror',e=>errors.push(e.message));page.on('dialog',async d=>{dialogs.push(d.type());await d.dismiss();});
 const shot=async name=>{if(captureDir){fs.mkdirSync(captureDir,{recursive:true});await page.screenshot({path:path.join(captureDir,name)});}};
 await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 const header=page.locator('.gs26-status-phase');
 const state=async(label,detail)=>{
  await page.waitForFunction(([label,detail])=>{const l=document.querySelector('.gs26-status-phase-label')?.textContent,d=document.querySelector('.gs26-status-phase-detail')?.textContent||null;return l===label&&d===detail;},[label,detail],{timeout:8000})
   .catch(async()=>{throw Error('header shows '+JSON.stringify(await header.innerText())+', wanted '+label+' '+detail);});
 };
 await state('In flight','· Descent');assert.equal(await header.getAttribute('data-phase'),'in_flight');await shot('header-in-flight.png');
 flight={state:'Fill',fill_phase:'NitrousFill',fill_phase_label:'Nitrous fill',fc_phase:null};await state('Fill','· Nitrous fill');assert.equal(await header.getAttribute('data-phase'),'fill');await shot('header-fill.png');
 flight={state:'Landed',fill_phase:null,fc_phase:{value:14,label:'Recovery',source:'fc_state_report',age_ms:10}};await state('Landed','· Recovery');
 flight={state:'Aborted',fill_phase:null,fc_phase:null};await state('Aborted',null);
 // An older station's retired values still read as the network state.
 flight={state:'Coast'};await state('In flight','· Coast');
 flight={state:'NitrogenFill'};await state('Fill','· Nitrogen fill');
 flight={state:'Recovery'};await state('Landed','· Recovery');
 flight={state:'InFlight',fill_phase:null,fc_phase:{value:11,label:'Descent',source:'fc_state_report',age_ms:1200}};await state('In flight','· Descent');

 await page.getByRole('link',{name:'Connection',exact:true}).click();await page.getByRole('heading',{name:'Network health',exact:true}).waitFor();
 const panel=page.locator('.gs26-network-health'),fcCard=panel.locator('[data-board="FC"]'),gbCard=panel.locator('[data-board="GB"]');
 const reset=fcCard.getByRole('button',{name:'Reset FC to Startup…',exact:true});await reset.waitFor();
 assert(await reset.isEnabled(),'the FC reset is offered in flight without an Abort');
 assert.equal(await fcCard.getByRole('button',{name:'Recover to Startup…'}).count(),0,'one FC reset button, not two');
 // Disabled controls say why in visible text.
 const gbQuery=gbCard.getByRole('button',{name:'Query board',exact:true});assert(await gbQuery.isDisabled());
 await gbCard.locator('.gs-disabled-reason[data-control="query"]').getByText('Query unavailable: Gateway Board firmware has no recovery endpoint.').waitFor();
 assert(await gbCard.locator('.gs-disabled-reason[data-control="query"]').isVisible());
 const fcRow=boardRecovery.boards.find(b=>b.id==='FC');fcRow.capabilities.reset_startup_any_state=false;fcRow.reset_startup_any_state_blocked_reason='The FC reports a recovery charge firing.';
 await fcCard.getByText('Reset unavailable: The FC reports a recovery charge firing.',{exact:true}).waitFor();assert(await reset.isDisabled());await shot('fc-reset-blocked.png');
 fcRow.capabilities.reset_startup_any_state=true;fcRow.reset_startup_any_state_blocked_reason=null;
 await page.waitForFunction(()=>!document.querySelector('[data-board="FC"] [data-recovery-action="reset_fc_startup"]')?.disabled);
 assert.equal(await fcCard.locator('.gs-disabled-reason[data-control="reset_fc_startup"]').count(),0,'no reason while available');
 await shot('fc-reset-available.png');
 // One click opens the one station confirmation; no intermediate dialog.
 await reset.click();
 const confirm=page.getByRole('alertdialog');await confirm.waitFor();
 assert.equal(await page.getByRole('dialog').count(),0,'no modal before the confirmation');assert.equal(await page.getByRole('alertdialog').count(),1);
 await confirm.getByRole('heading',{name:'Reset FC to Startup',exact:true}).waitFor();
 const warning=confirm.locator('.gs26-fc-reset-warning');assert.equal(await warning.innerText(),fcRow.reset_fc_startup_warning);assert.equal(await warning.getAttribute('role'),'alert');
 assert.equal((await confirm.innerText()).split(fcRow.reset_fc_startup_warning).length,2,'the warning is shown once');
 assert.match(await confirm.innerText(),/Deployment history is kept/);
 await shot('fc-reset-confirmation.png');
 assert.equal(confirmed.length,0,'nothing confirmed by opening the review');
 await confirm.getByRole('button',{name:'Reset FC to Startup',exact:true}).click();
 await page.waitForFunction(()=>!document.querySelector('[role="alertdialog"]'));
 await page.getByText(/Synthetic accepted; not device confirmed\\./).waitFor();
 assert.equal(confirmed.length,1,'exactly one confirmation sent');assert(prepared.length>=1&&prepared.every(p=>p.action==='reset_fc_startup'&&p.board==='FC'));
 assert.deepEqual(dialogs,[]);assert.deepEqual(errors,[]);await context.close();
 console.log('PASS flight state split: header network state with FC/fill detail, retired values mapped, inline disabled reasons, one-step FC reset confirmation');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
