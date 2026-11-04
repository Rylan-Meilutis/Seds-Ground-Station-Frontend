// Actual programme document; no camera, media server, command API or hardware.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true,...(process.env.CHROME_BINARY?{executablePath:process.env.CHROME_BINARY}:{})});
let mode='hold',held=null,count=0,replay=false,omitTelemetry=false;
const signal=(value,evidence)=>({value,evidence,age_ms:0,valid_for_ms:2000});
const baseState=()=>({server_now_ms:Date.now(),broadcast:{label:'Provenance fixture',layout:'model',delay_seconds:10,featured_stream_id:'',replay:null},streams:[],telemetry:{scene:'model',phase:'Idle',profile:'ground',t_clock:'T− 00:05.00',cycle_seconds:1,stats_per_page:4,stats:[{label:'Fill mass',value:4.2,unit:'kg',precision:1},{label:'Tank pressure',value:123,unit:'psi',precision:0},{label:'Saved fill label',value:42,unit:'%',precision:0,binding:{data_type:'LOADCELL_FILL_PERCENT'}}],sequence_visuals:{version:1,clock:{kind:'t_minus',advancing:true,remaining_ms:5000},igniter:{planned_t_minus_ms:5000,output:signal(false,'board_reported'),command:signal(true,'command_accepted')},valves:{pilot:{output:signal(true,'board_reported')}}}}});
const state=()=>{const next=baseState();if(replay)next.broadcast.replay={id:'saved',duration_seconds:10,speed:1,started_at_ms:Date.now(),source_end_ms:Date.now()};if(omitTelemetry)next.telemetry=null;return next;};
try{
 const page=await browser.newPage({viewport:{width:1100,height:900}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.route('http://station.invalid/**',async route=>{
  const url=new URL(route.request().url());
  if(url.pathname==='/api/media-assets/program')return route.fulfill({contentType:'text/html',body:fs.readFileSync('station_ui/pages/program.html')});
  const scripts={'/assets/program-art.js':'station_ui/scripts/program-art.js','/assets/stream-sequence-visuals.js':'station_ui/scripts/stream-sequence-visuals.js'};
  if(scripts[url.pathname])return route.fulfill({contentType:'text/javascript',body:fs.readFileSync(scripts[url.pathname])});
  if(url.pathname==='/api/media-assets/program/state'){
   count++;
   if(mode==='hold'){held=route;return;}
   return route.fulfill(mode==='error'?{status:503,body:'unavailable'}:{json:state()});
  }
  return route.fulfill({contentType:'text/javascript',body:''});
 });
 await page.goto('http://station.invalid/api/media-assets/program?ticket=EXAMPLE');
 while(!held)await page.waitForTimeout(25);
 await page.waitForTimeout(250);
 assert.match(await page.locator('#banner').innerText(),/Buffering|unavailable/i,'initial held fetch is unknown without a prior state');
 mode='error';await held.fulfill({status:503,body:'unavailable'});held=null;
 await page.waitForFunction(()=>document.querySelector('#status').textContent.includes('unavailable'));
 assert.deepEqual(errors,[],'initial503/held response must not dereference absent state');
 mode='fresh';
 await page.waitForFunction(()=>document.querySelector('#banner').textContent.includes('T− 00:05.00'));
 assert.match(await page.locator('#banner').innerText(),/4\.2 kg/);
 assert((await page.locator('#banner .name').allTextContents()).includes('Saved fill label'),'preserve user label text; CSS uppercase is presentation only');
 assert.match(await page.locator('#banner').innerText(),/Mass-based target estimate · not liquid volume/);
 assert.equal(await page.locator('#banner .stat-note').count(),1,'pressure/mass are not silently relabeled as liquid fill');
 assert.match(await page.locator('#ground-equipment').getAttribute('aria-label'),/tank levels unknown/);
 assert.match(await page.locator('#ground-equipment').textContent(),/Level unknown/);
 await page.waitForFunction(()=>document.querySelector('.gs26-igniter')?.dataset.planned==='true',null,{timeout:1000});
 assert.equal(await page.locator('.gs26-igniter').getAttribute('data-planned'),'true');
 assert.match(await page.locator('.gs26-igniter-label').innerText(),/Board reports off/);
 assert.doesNotMatch(await page.locator('.gs26-igniter-label').innerText(),/Electrical|fired|ignition confirmed/);
 assert.match(await page.locator('.gs26-valve-status').first().innerText(),/Flow unmeasured/);
 for(const viewport of [{width:390,height:800},{width:1100,height:900}]){await page.setViewportSize(viewport);await page.waitForTimeout(100);assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'estimate annotation fits mobile/desktop without horizontal overflow');}
 mode='hold';while(!held)await page.waitForTimeout(25);
 await page.waitForTimeout(2800);
 assert.doesNotMatch(await page.locator('#banner').innerText(),/T− 00:05\.00|4\.2 kg|123 psi/,'held response cannot leave stale Tclock/tank values presented');
 assert.match(await page.locator('#banner').innerText(),/Unavailable|Buffering/);
 assert.equal(await page.locator('.gs26-igniter').getAttribute('data-planned'),'false');
 mode='fresh';await held.fulfill({json:state()});held=null;
 await page.waitForFunction(()=>document.querySelector('#banner').textContent.includes('4.2 kg'));
 mode='error';await page.waitForFunction(()=>document.querySelector('#status').textContent.includes('unavailable'));
 await page.waitForTimeout(150);
 assert.doesNotMatch(await page.locator('#banner').innerText(),/T− 00:05\.00|4\.2 kg|123 psi/,'explicit503 clears saved evidence immediately');
 mode='fresh';await page.waitForFunction(()=>document.querySelector('#banner').textContent.includes('4.2 kg'));
 mode='hold';
 await page.evaluate(()=>{window.dispatchEvent(new PageTransitionEvent('pagehide',{persisted:true}));window.dispatchEvent(new PageTransitionEvent('pageshow',{persisted:true}));});
 while(!held)await page.waitForTimeout(25);
 await page.waitForTimeout(250);
 assert.doesNotMatch(await page.locator('#banner').innerText(),/4\.2 kg|T− 00:05\.00/,'pageshow cannot revive previous clock/tank values before its first response');
 mode='fresh';await held.fulfill({json:state()});held=null;
 await page.waitForFunction(()=>document.querySelector('#banner').textContent.includes('4.2 kg'));
 replay=true;omitTelemetry=true;
 await page.waitForFunction(()=>document.querySelector('#banner').textContent.includes('No saved clock'));
 mode='hold';while(!held)await page.waitForTimeout(25);
 await page.waitForTimeout(2800);
 assert.doesNotMatch(await page.locator('#banner').innerText(),/No saved clock/,'stale replay response does not assert absence of recorded clock');
 assert.match(await page.locator('#banner').innerText(),/Unavailable/);
 mode='fresh';omitTelemetry=false;await held.fulfill({json:state()});held=null;
 await page.waitForFunction(()=>document.querySelector('#banner').textContent.includes('T− 00:05.00'));
 assert.deepEqual(errors,[]);console.log(JSON.stringify({state:'PASS',actual_program_document:true,held_fetch_aged_out:true,http503_clears:true,recovery_requires_fresh_response:true,planned_igniter_not_physical:true,board_output_not_electrical_or_combustion:true,valves_not_measured_flow:true,requests:count,no_commands_or_media:true}));
}finally{if(held)await held.abort().catch(()=>{});await browser.close();}
