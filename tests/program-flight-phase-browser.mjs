// Programme rocket inset under the network flight states: In flight with the
// FC phase beside it, and retired detailed names from older stations.
// Actual programme document, local routed observations only; no cameras/commands.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true,...(process.env.CHROME_BINARY?{executablePath:process.env.CHROME_BINARY}:{})});
let phase='Idle',fcPhase=null;
const observation=()=>({server_now_ms:Date.now(),broadcast:{label:'Flight phase fixture',layout:'model',delay_seconds:1,replay:null},streams:[],telemetry:{scene:'model',phase,fc_phase:fcPhase,test_fire_mode:false,profile:'flight',stats:[],cycle_seconds:1,stats_per_page:4,t_clock:'T+ 00:05.00',sequence_visuals:null}});
try{
 const page=await browser.newPage({viewport:{width:1280,height:720}}),errors=[];page.setDefaultTimeout(12000);page.on('pageerror',e=>errors.push(e.message));
 await page.route('**/*',async route=>{
  const url=new URL(route.request().url());
  if(url.origin!=='http://station.invalid')return route.abort();
  const files={'/api/media-assets/program':'station_ui/pages/program.html','/assets/program-art.js':'station_ui/scripts/program-art.js','/assets/stream-sequence-visuals.js':'station_ui/scripts/stream-sequence-visuals.js'};
  if(files[url.pathname])return route.fulfill({contentType:url.pathname.endsWith('/program')?'text/html':'text/javascript',body:fs.readFileSync(files[url.pathname])});
  if(url.pathname==='/api/media-assets/program/state')return route.fulfill({json:observation()});
  return route.fulfill({contentType:'text/javascript',body:''});
 });
 await page.goto('http://station.invalid/api/media-assets/program?ticket=EXAMPLE');
 const label=page.locator('#rocket-phase'),flame=page.locator('#flame'),chute=page.locator('#chute');
 const expect=async(text,{flameShown=false,chuteShown=false,ground=false})=>{
  await page.waitForFunction(t=>document.querySelector('#rocket-phase')?.textContent.endsWith(t),text);
  assert.equal(await page.locator('#gs26-sequence-equipment').evaluate(e=>!e.hidden),ground,text+' ground equipment');
  assert.equal(await flame.isVisible(),flameShown,text+' flame');assert.equal(await chute.isVisible(),chuteShown,text+' chute');
 };
 // Network Fill is a ground phase: the GSE evidence stays shown.
 phase='Fill';await expect('· Fill',{ground:true});
 phase='InFlight';fcPhase={value:8,label:'Ascent',source:'fc_state_report',age_ms:100};
 await expect('· In flight · Ascent',{flameShown:true,chuteShown:false});
 fcPhase={value:11,label:'Descent',source:'fc_state_report',age_ms:100};
 await expect('· In flight · Descent',{flameShown:false,chuteShown:true});
 fcPhase=null;await expect('· In flight',{flameShown:false,chuteShown:false});
 phase='Landed';fcPhase={value:14,label:'Recovery',source:'fc_state_report',age_ms:100};
 await expect('· Landed · Recovery',{flameShown:false,chuteShown:true});
 // Older station: retired detailed names map to their network state.
 phase='Coast';fcPhase=null;await expect('· In flight · Coast',{flameShown:false,chuteShown:false});
 phase='Ascent';await expect('· In flight · Ascent',{flameShown:true,chuteShown:false});
 phase='Recovery';await expect('· Landed · Recovery',{flameShown:false,chuteShown:true});
 phase='NitrousFill';await expect('· Fill · Nitrous fill',{ground:true});
 assert.deepEqual(errors,[]);
 console.log('programme flight phase browser test passed');
}finally{await browser.close();}
