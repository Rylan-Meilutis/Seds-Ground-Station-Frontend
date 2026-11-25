// Studio scenes on the actual programme page: telemetry views as sources
// (live diagram, cards, T-clock, plots, transparent overlays) render in their
// slots and update from the delayed snapshot; a fade take keeps a still of the
// outgoing scene for its duration, a cut does not; `scene=` (Preview) and
// `source=…&transparent=1` (browser source) modes. Local routes only.
// SHOTS=dir saves the programme output; CPU=1 prints the page's CPU use.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true,...(process.env.CHROME_BINARY?{executablePath:process.env.CHROME_BINARY}:{})});
const dir='src/telemetry_dashboard/live_diagrams/topologies';
const topology=JSON.parse(fs.readFileSync(path.join(dir,fs.existsSync(path.join(dir,'flight.json'))?'flight.json':'fill.json'),'utf8'));
const slot=(id,x,y,width,height,z,source)=>({id,x,y,width,height,z,source});
const src=(id,kind,view,label,transparent=false)=>({id,kind,view,label,transparent,stream_id:null});
const scenes={
 diagram:{scene_id:'diagram',name:'Diagram',program_bar:false,slots:[
  slot('main',0,0,66,100,0,src('diagram:flight','telemetry','diagram','Flight')),
  slot('top',66,0,34,50,0,src('telemetry:tclock','telemetry','tclock','T-clock')),
  slot('bottom',66,50,34,50,0,src('telemetry:stats','telemetry','stats','Key telemetry cards')),
  slot('lower',0,75,66,25,2,src('overlay:lower-third','overlay','lower_third','Lower third',true))]},
 plots:{scene_id:'plots',name:'Plots',program_bar:true,slots:[slot('main',0,0,100,100,0,src('telemetry:plot','telemetry','plot','Telemetry plots')),slot('bug',70,0,30,25,3,src('overlay:tclock-bug','overlay','tclock_bug','Bug',true))]},
};
let current='diagram',seq=1,kind='fade',duration=1500,altitude=100,psi=500,requests=[];
const observation=url=>{
 const q=url.searchParams,source=q.get('source'),scene=source?{scene_id:'_source',name:source,program_bar:false,slots:[slot('main',0,0,100,100,0,src(source,'overlay','lower_third','Lower third',true))]}:scenes[q.get('scene')||current];
 const t=Date.now();
 return {server_now_ms:t,broadcast:{label:'SWEET V6 launch',layout:'studio',delay_seconds:3,replay:null},streams:[],
  diagram_topology:scene.slots.some(s=>s.source?.view==='diagram')?topology:null,
  studio:{...scene,mode:source?'source':q.get('scene')?'preview':'program',take_seq:seq,transition:{kind,duration_ms:duration}},
  telemetry:{phase:'InFlight',fc_phase:{label:'Ascent'},scene:'inherit',profile:'flight',cycle_seconds:5,stats_per_page:3,t_clock:'T+ 00:'+String(Math.floor(t/1000)%60).padStart(2,'0')+'.00',
   stats:[{label:'Altitude',value:altitude,unit:'m',precision:1,format:'number'},{label:'Velocity',value:altitude/10,unit:'m/s',precision:1},{label:'Tank pressure',value:psi,unit:'psi',precision:0}],
   diagram:{rows:[{t:'PRESSURE_TRANSDUCER_CALIBRATED',s:'VB',v:[psi],age_ms:100},{t:'VALVE_STATE',s:'VB',v:[0,0,0,0,1,0,0],age_ms:100}],boards:[],controls:[],status:{igt:false}}}};
};
async function open(page,query){
 const errors=[];page.on('pageerror',e=>errors.push(e.message));page.setDefaultTimeout(12000);
 await page.route('**/*',async route=>{
  const url=new URL(route.request().url());if(url.origin!=='http://station.invalid')return route.abort();
  const files={'/api/media-assets/program':'station_ui/pages/program.html','/assets/program-studio.js':'station_ui/scripts/program-studio.js','/assets/program-diagram.js':'station_ui/scripts/program-diagram.js','/assets/program-art.js':'station_ui/scripts/program-art.js','/assets/stream-sequence-visuals.js':'station_ui/scripts/stream-sequence-visuals.js'};
  if(files[url.pathname])return route.fulfill({contentType:url.pathname.endsWith('/program')?'text/html':'text/javascript',body:fs.readFileSync(files[url.pathname])});
  if(url.pathname==='/api/media-assets/program/state'){requests.push(url.search);return route.fulfill({json:observation(url)});}
  if(url.pathname.startsWith('/api/'))return route.fulfill({json:{}});
  return route.fulfill({contentType:'text/javascript',body:''});
 });
 await page.goto('http://station.invalid/api/media-assets/program?ticket=EXAMPLE'+query);
 return errors;
}
try{
 const shots=process.env.SHOTS?path.resolve(process.env.SHOTS):null;if(shots)fs.mkdirSync(shots,{recursive:true});
 const page=await browser.newPage({viewport:{width:1280,height:720}});const errors=await open(page,'&clean=1');
 // The live diagram fills its slot; the telemetry views are drawn beside it.
 await page.waitForSelector('#diagram-scene svg');
 const diagramBox=await page.locator('#diagram-scene').boundingBox();assert(diagramBox.width<1280*0.7&&diagramBox.width>1280*0.6,'diagram sized to its 66 % slot: '+diagramBox.width);
 const stats=page.locator('.gs-st-slot[data-view=stats]');await stats.getByText('Altitude').waitFor();
 assert.match(await stats.innerText(),/100\.0\s*m/);
 const clock=page.locator('.gs-st-slot[data-view=tclock] strong');await page.waitForFunction(()=>/^T\+ 00:\d\d\.00$/.test(document.querySelector('.gs-st-slot[data-view=tclock] strong')?.textContent));
 const lower=page.locator('.gs-st-slot[data-view=lower_third]');assert.match(await lower.innerText(),/SWEET V6 launch/);assert.match(await lower.innerText(),/100\.0 m/);
 assert.equal(await lower.evaluate(e=>getComputedStyle(e).backgroundColor),'rgba(0, 0, 0, 0)','overlay slot is transparent');
 assert.equal(await page.locator('#banner').isVisible(),false,'the scene hides the telemetry bar');
 // It updates: a new snapshot changes the cards, overlay and diagram reading.
 const firstClock=await clock.textContent();altitude=1234.5;psi=640;
 await stats.getByText('1234.5').waitFor();await lower.getByText('1234.5 m').waitFor();
 await page.waitForFunction(()=>[...document.querySelectorAll('#diagram-scene text')].some(t=>/640/.test(t.textContent)));
 await page.waitForFunction(c=>document.querySelector('.gs-st-slot[data-view=tclock] strong').textContent!==c,firstClock,{timeout:3000});
 if(shots)await page.screenshot({path:path.join(shots,'program-telemetry-source.png')});
 if(process.env.CPU){
  const cdp=await page.context().newCDPSession(page);await cdp.send('Performance.enable');const read=async()=>Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map(m=>[m.name,m.value]));
  const a=await read();await page.waitForTimeout(30000);const b=await read();
  console.log(`CPU programme page (diagram+tclock+cards+lower third, 1280x720): script ${((b.ScriptDuration-a.ScriptDuration)/30*100).toFixed(1)} %, layout ${((b.LayoutDuration-a.LayoutDuration)/30*100).toFixed(1)} %, recalc style ${((b.RecalcStyleDuration-a.RecalcStyleDuration)/30*100).toFixed(1)} %, task ${((b.TaskDuration-a.TaskDuration)/30*100).toFixed(1)} % of one core`);
 }
 // Fade take: a still of the outgoing scene stays over the new one for the duration.
 current='plots';seq=2;kind='fade';duration=1500;
 await page.waitForSelector('[data-transition=studio]',{state:'attached'});
 await page.locator('.gs-st-slot[data-view=plot] canvas').first().waitFor();
 const still=await page.evaluate(()=>({opacity:getComputedStyle(document.querySelector('[data-transition=studio]')).transitionDuration,diagram:!!document.querySelector('[data-transition=studio] svg')}));
 assert.equal(still.opacity,'1.5s');assert(still.diagram,'the outgoing diagram is in the still');
 await page.waitForSelector('[data-transition=studio]',{state:'detached',timeout:5000});
 assert.equal(await page.locator('#diagram-scene').isVisible(),false);assert.equal(await page.locator('#banner').isVisible(),true,'this scene keeps the bar');
 // The plot draws a line once history accumulates.
 await page.waitForFunction(()=>{const c=document.querySelector('.gs-st-slot[data-view=plot] canvas');if(!c||!c.width)return false;const d=c.getContext('2d').getImageData(0,0,c.width,c.height).data;for(let i=3;i<d.length;i+=4)if(d[i])return true;return false;},null,{timeout:8000});
 // Cut: no still.
 current='diagram';seq=3;kind='cut';duration=0;
 await page.waitForSelector('#diagram-scene svg');await page.waitForTimeout(300);
 assert.equal(await page.locator('[data-transition]').count(),0,'a cut has no transition');
 assert.deepEqual(errors,[]);await page.close();

 // Preview mode asks the station for that scene, muted.
 requests=[];const pv=await browser.newPage({viewport:{width:640,height:360}});const pvErrors=await open(pv,'&scene=plots');
 await pv.locator('.gs-st-slot[data-view=plot]').waitFor();assert(requests.every(r=>r.includes('scene=plots')));
 assert.equal(await pv.evaluate(()=>window.gsStreamSound.muted),true);assert.deepEqual(pvErrors,[]);await pv.close();

 // A transparent browser source: only the overlay, no page background, no bar.
 const ov=await browser.newPage({viewport:{width:1920,height:1080}});const ovErrors=await open(ov,'&source=overlay:lower-third&transparent=1');
 await ov.locator('.gs-st-slot[data-view=lower_third]').getByText('SWEET V6 launch').waitFor();
 assert.equal(await ov.evaluate(()=>getComputedStyle(document.body).backgroundColor),'rgba(0, 0, 0, 0)');
 assert.equal(await ov.locator('#program-bar').isVisible(),false);
 const fontPx=await ov.locator('.gs-st-lower .metric span').first().evaluate(e=>parseFloat(getComputedStyle(e).fontSize));assert(fontPx>=40,'readable at 1080p: '+fontPx+' px');
 if(shots)await ov.screenshot({path:path.join(shots,'program-lower-third-transparent.png'),omitBackground:true});
 assert.deepEqual(ovErrors,[]);
 console.log('PASS programme studio scenes: diagram/cards/T-clock/plot/overlay sources render and update, fade still for its duration, cut, preview and transparent source modes');
}finally{await browser.close();}
