// Actual programme document, local routed observations only; no cameras/commands.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true,...(process.env.CHROME_BINARY?{executablePath:process.env.CHROME_BINARY}:{})});
let reportedMode=true,phase='Idle',hold=false,pending=null,requests=0;
const signal=(value,evidence='board_reported')=>({value,evidence,age_ms:0,valid_for_ms:2000});
const observation=()=>({server_now_ms:Date.now(),broadcast:{label:'Mode artwork fixture',layout:'model',delay_seconds:1,replay:null},streams:[],telemetry:{scene:'model',phase,test_fire_mode:reportedMode,profile:'ground',stats:[],cycle_seconds:1,stats_per_page:4,t_clock:'T− 00:05.00',sequence_visuals:{version:1,clock:{kind:'t_minus',remaining_ms:5000,advancing:true},igniter:{planned_t_minus_ms:5000,output:signal(false),command:signal(true,'command_accepted')},valves:{pilot:{output:signal(true)}}}}});
try{
 const page=await browser.newPage({viewport:{width:390,height:844}}),errors=[],writes=[];page.setDefaultTimeout(12000);page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',async route=>{
  const req=route.request(),url=new URL(req.url());if(req.method()!=='GET')writes.push({url:req.url(),method:req.method()});
  if(url.origin!=='http://station.invalid')return route.abort();
  const files={'/api/media-assets/program':'station_ui/pages/program.html','/assets/program-art.js':'station_ui/scripts/program-art.js','/assets/stream-sequence-visuals.js':'station_ui/scripts/stream-sequence-visuals.js'};
  if(files[url.pathname])return route.fulfill({contentType:url.pathname.endsWith('/program')?'text/html':'text/javascript',body:fs.readFileSync(files[url.pathname])});
  if(url.pathname==='/api/media-assets/program/state'){requests++;if(hold){pending=route;return;}return route.fulfill({json:observation()});}
  return route.fulfill({contentType:'text/javascript',body:''});
 });
 await page.goto('http://station.invalid/api/media-assets/program?ticket=EXAMPLE');
 const artwork=page.locator('#rocket-inset > svg');
 await page.waitForFunction(()=>document.querySelector('#rocket-inset > svg')?.dataset.artwork==='test_fire');
 assert(await page.locator('#test-fire-stand').isVisible());assert.equal(await page.locator('#flight-vehicle').isVisible(),false);
 assert.match(await artwork.getAttribute('aria-label'),/Illustrative motor on a test stand; not ignition or flow evidence/);
 assert.match(await page.locator('#rocket-phase').textContent(),/Test-fire motor stand · Idle/);
 await page.waitForFunction(()=>document.querySelector('.gs26-igniter')?.dataset.planned==='true',null,{timeout:1000});
 assert.match(await page.locator('.gs26-igniter-label').innerText(),/Board reports off/);
 assert.equal(await page.locator('.gs26-igniter').getAttribute('data-planned'),'true','T−5 remains a plan independently of board off');
 assert.match(await page.locator('.gs26-valve-status').first().innerText(),/Flow unmeasured/);
 assert.match(await page.locator('#ground-equipment').getAttribute('aria-label'),/tank levels unknown/);
 phase='Ascent';await page.waitForFunction(()=>document.querySelector('#rocket-phase').textContent.includes('Ascent'));
 assert(await page.locator('#test-fire-stand').isVisible());assert.equal(await page.locator('#flame').isVisible(),false,'test-fire phase must not fabricate motor combustion');
 reportedMode=false;await page.waitForFunction(()=>document.querySelector('#rocket-inset > svg').dataset.artwork==='flight');
 assert(await page.locator('#flight-vehicle').isVisible());assert.equal(await page.locator('#test-fire-stand').isVisible(),false);
 assert.match(await artwork.getAttribute('aria-label'),/Illustrative flight phase; not ignition or flow evidence/);
 phase='Descent';await page.waitForFunction(()=>document.querySelector('#chute').style.display==='');assert(await page.locator('#chute').isVisible());
 for(const [value,nextPhase] of [[undefined,'Idle'],['true','Armed'],[1,'Ascent'],[null,'Descent']]){reportedMode=value;phase=nextPhase;await page.waitForFunction(expected=>document.querySelector('#rocket-inset > svg').dataset.artwork==='unknown'&&document.querySelector('#rocket-phase').textContent.includes(expected),nextPhase);assert(await page.locator('#vehicle-mode-unknown').isVisible());assert.equal(await page.locator('#flight-vehicle').isVisible(),false);assert.equal(await page.locator('#test-fire-stand').isVisible(),false);}
 reportedMode=true;phase='Armed';await page.waitForFunction(()=>document.querySelector('#rocket-inset > svg').dataset.artwork==='test_fire');
 hold=true;const deadline=Date.now()+12000;while(!pending&&Date.now()<deadline)await page.waitForTimeout(25);assert(pending);
 await page.waitForTimeout(2800);await page.waitForFunction(()=>document.querySelector('#rocket-inset > svg').dataset.artwork==='unknown');
 assert.equal(await page.locator('#test-fire-stand').isVisible(),false,'stale programme cannot preserve a fresh mode indication');
 assert.equal(await page.locator('.gs26-igniter').getAttribute('data-planned'),'false');
 hold=false;await pending.fulfill({json:observation()});pending=null;await page.waitForFunction(()=>document.querySelector('#rocket-inset > svg').dataset.artwork==='test_fire');
 for(const viewport of [{width:320,height:844},{width:390,height:844},{width:1100,height:900}]){await page.setViewportSize(viewport);await page.waitForTimeout(100);assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'mode caption does not create horizontal overflow');}
 assert.deepEqual(errors,[]);assert.deepEqual(writes,[]);console.log(JSON.stringify({status:'PASS',actual_program_document:true,testfire_motor_stand:true,flight_phase_illustration:true,missing_or_malformed_mode_unknown:true,stale_demoted_unknown:true,recovery_requires_fresh_frame:true,planned_T5_not_ignition:true,board_output_not_motion_or_flow:true,no_commands:true,requests}));
}finally{if(pending)await pending.abort().catch(()=>{});await browser.close();}
