// Real Wasm UI with the synthetic board-recovery backend of
// board-recovery-browser.mjs. No hardware. Each board card shows separate
// Network, State and (where the board reports storage) SD lines, each with
// its own indicator; one "Healthy" badge must never stand for an aborted board.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const base=fs.readFileSync('tests/board-recovery-browser.mjs','utf8'),cut=base.indexOf('\nfixture+=`try {');
if(cut<0)throw Error('Board recovery fixture boundary changed');
let prefix=base.slice(0,cut);
if(process.env.PLAYWRIGHT_MODULE)prefix=prefix.replace("import.meta.resolve('playwright')",JSON.stringify(pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href));
// DAQ reports SD health in its diagnostics; the other boards report none.
prefix=prefix.replace("return {sender_id:id,healthy:","return {...(id==='DAQ'&&daqSd?{diagnostics:daqDiagnostics()}:{}),sender_id:id,healthy:");
prefix=prefix.replace("const observed=['AB','VB','GB','DAQ'];",`const observed=['AB','VB','GB','DAQ'];let daqSd=true,daqSdFlags=3,daqInitFailures=0;
const daqDiagnostics=()=>({version:1,received_age_ms:0,uptime_ticks:1000,uptime_frequency_hz:1000,reset_flags:0,watchdog_missing_mask:0,admission_refusals:0,
 heap_live_bytes:null,heap_peak_bytes:null,heap_region_bytes:null,allocation_failures:null,can_rx_drops:null,can_tx_rejected:null,uart_rx_drops:null,tx_queue_high_water:null,
 sd_accepted_records:10,sd_processed_records:10,sd_rejected_records:0,sd_flushed_records:10,sd_write_errors:0,sd_max_write_ms:3,
 sd_status_received_age_ms:0,sd_flags:daqSdFlags,sd_last_write_age_ms:100,sd_last_flush_age_ms:100,sd_init_failures:daqInitFailures,adc_overruns:0,diagnostic_send_failures:0});`);
if(!prefix.includes('daqDiagnostics()}'))throw Error('Topology fixture changed');
const body=String.raw`
fixture+=${'`'}try {
 const context=await browser.newContext({viewport:{width:1280,height:1000}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const sockets=new Set();await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>{sockets.add(s);const beat=setInterval(()=>{s.send(JSON.stringify({ty:'NetworkTime',data:{timestamp_ms:Date.now()}}));s.send(JSON.stringify({ty:'NetworkTopology',data:topology()}));},500);s.onClose(()=>{clearInterval(beat);sockets.delete(s);});});
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 await page.evaluate(()=>[...document.querySelectorAll('a')].find(a=>['Connection Status','Connection'].includes(a.getAttribute('aria-label')||a.textContent.trim()))?.click());
 const panel=page.locator('.gs26-network-health');await panel.waitFor();
 const card=id=>panel.locator('[data-board="'+id+'"]');
 const line=(id,name)=>card(id).locator('[data-health-line="'+name+'"]');
 const health=async(id,name)=>{const l=line(id,name);await l.waitFor();return l.getAttribute('data-health');};
 const until=async(check,message)=>{for(let end=Date.now()+8000;Date.now()<end;){if(await check())return;await page.waitForTimeout(100);}throw Error(message);};

 // An aborted board is reachable (Network healthy) but its State is unhealthy.
 const ab=boardRecovery.boards.find(b=>b.id==='AB');Object.assign(ab,{state:15,state_label:'Aborted',abort_latched:true});
 if(process.env.GS_UI_ARTIFACT_DIR){// before/after evidence: an aborted AB, a fresh VB and a writing DAQ
  fs.mkdirSync(process.env.GS_UI_ARTIFACT_DIR,{recursive:true});await page.waitForTimeout(2500);
  for(const [width,tag] of [[1280,'desktop'],[390,'narrow']]){await page.setViewportSize({width,height:1000});await page.waitForTimeout(400);await panel.screenshot({path:path.join(process.env.GS_UI_ARTIFACT_DIR,'board-health-'+tag+'.png')});}
  await page.setViewportSize({width:1280,height:1000});}
 await until(async()=>await health('AB','state')==='unhealthy','aborted board State is unhealthy');
 assert.equal(await health('AB','network'),'healthy','an aborted board is still reachable');
 assert.match(await line('AB','state').innerText(),/Aborted/);
 assert.equal(await card('AB').getByText('Healthy',{exact:true}).count(),1,'only the Network line may say Healthy for an aborted board');

 // A board that has not aborted, with a fresh readback, is healthy on both lines.
 const vb=boardRecovery.boards.find(b=>b.id==='VB');vb.abort_latched=false;
 await until(async()=>await health('VB','state')==='healthy','fresh Startup readback is healthy');

 // A stale readback is shown as stale, never healthy.
 Object.assign(vb,{age_ms:60000,readback_valid_for_ms:0});
 await until(async()=>await health('VB','state')==='stale','stale readback State is stale');
 assert.notEqual(await health('VB','state'),'healthy');

 // AV-bay status queries paused: an AV-bay board's state is stale, not healthy.
 // (This fixture observes no AV-bay board, so only the status field is exercised.)
 boardRecovery.av_bay_status_queries_paused='an AV-bay status query went unanswered';

 // SD: only boards that report storage get the line; a fault makes it unhealthy.
 await until(async()=>await health('DAQ','sd')==='healthy','writing SD is healthy');
 assert.equal(await line('AB','sd').count(),0,'a board without SD shows no SD line');
 assert.equal(await line('VB','sd').count(),0);
 daqSdFlags=1;daqInitFailures=2;
 await until(async()=>await health('DAQ','sd')==='unhealthy','SD fault is unhealthy');
 assert.match(await line('DAQ','sd').innerText(),/Card unavailable/);

 // Three short lines, no overlap, at desktop and phone widths.
 for(const width of[1280,390,320]){await page.setViewportSize({width,height:900});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no horizontal overflow at '+width);
  for(const id of['AB','DAQ'])assert(await card(id).evaluate(e=>e.scrollWidth<=e.clientWidth+1),'card fits at '+width);}
 assert.deepEqual(errors,[]);await context.close();
 console.log('PASS board health lines: aborted board Network healthy / State unhealthy, stale readback stale, SD fault unhealthy, no SD line without storage');
}finally{await browser.close();await new Promise(r=>server.close(r));}${'`'};
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
`;
await import('data:text/javascript;base64,'+Buffer.from(prefix+body).toString('base64'));
