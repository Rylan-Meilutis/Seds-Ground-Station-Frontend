// Live diagrams at /preview of the web bundle: one view per station mode,
// chosen from the mode (demo flight, a test-fire replay, a ground-test replay,
// an older bundle from the per-system set), full screen with the Fullscreen
// API and as a viewport overlay where it is missing (iPhone), Esc and the
// button leave, at desktop and phone widths. No station and zero network.
//
//   GS_PREVIEW_URL=http://127.0.0.1:19096 node tests/live-diagrams-browser.mjs
//   (GS_UI_ARTIFACTS=dir saves a screenshot of each mode, normal and full screen)
import assert from 'node:assert/strict';
import fs from 'node:fs';
import crypto from 'node:crypto';
import zlib from 'node:zlib';
import {fileURLToPath} from 'node:url';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const base=(process.env.GS_PREVIEW_URL||'http://127.0.0.1:19096').replace(/\/$/,'');
const shots=process.env.GS_UI_ARTIFACTS;
const fixture=name=>fileURLToPath(new URL(`./fixtures/${name}`,import.meta.url));

// A .gsreplay with some entries replaced (stored, uncompressed).
function rewriteBundle(file,edit){
 const zip=fs.readFileSync(file);const files=new Map();
 const end=zip.lastIndexOf(Buffer.from([0x50,0x4b,0x05,0x06]));let at=zip.readUInt32LE(end+16);
 for(let i=0;i<zip.readUInt16LE(end+10);i++){
  const method=zip.readUInt16LE(at+10),size=zip.readUInt32LE(at+20),nameLen=zip.readUInt16LE(at+28),extra=zip.readUInt16LE(at+30),comment=zip.readUInt16LE(at+32),local=zip.readUInt32LE(at+42);
  const name=zip.toString('utf8',at+46,at+46+nameLen);at+=46+nameLen+extra+comment;
  const start=local+30+zip.readUInt16LE(local+26)+zip.readUInt16LE(local+28),data=zip.subarray(start,start+size);
  files.set(name,method===8?zlib.inflateRawSync(data):Buffer.from(data));
 }
 const manifest=JSON.parse(files.get('manifest.json'));
 for(const [name,data] of Object.entries(edit(files))){
  files.set(name,data);const entry=manifest.entries.find(e=>e.path===name);
  if(entry){entry.bytes=data.length;entry.sha256=crypto.createHash('sha256').update(data).digest('hex');}
 }
 files.set('manifest.json',Buffer.from(JSON.stringify(manifest)));
 const parts=[],central=[];let offset=0;
 for(const [name,data] of files){
  const n=Buffer.from(name),h=Buffer.alloc(30),c=Buffer.alloc(46),crc=zlib.crc32(data);
  h.writeUInt32LE(0x04034b50,0);h.writeUInt16LE(20,4);h.writeUInt32LE(crc,14);h.writeUInt32LE(data.length,18);h.writeUInt32LE(data.length,22);h.writeUInt16LE(n.length,26);
  c.writeUInt32LE(0x02014b50,0);c.writeUInt16LE(20,4);c.writeUInt16LE(20,6);c.writeUInt32LE(crc,16);c.writeUInt32LE(data.length,20);c.writeUInt32LE(data.length,24);c.writeUInt16LE(n.length,28);c.writeUInt32LE(offset,42);
  parts.push(h,n,data);central.push(c,n);offset+=30+n.length+data.length;
 }
 const dir=Buffer.concat(central),e=Buffer.alloc(22);
 e.writeUInt32LE(0x06054b50,0);e.writeUInt16LE(files.size,8);e.writeUInt16LE(files.size,10);e.writeUInt32LE(dir.length,12);e.writeUInt32LE(offset,16);
 return Buffer.concat([...parts,dir,e]);
}
// A bundle exported by a station of the per-system set: its diagrams (the old
// GET /api/diagrams reply) and layouts without `live_diagram`.
function legacyBundle(){
 const legacy=fs.readFileSync(fixture('live-diagrams/legacy-diagrams.json'));
 return rewriteBundle(fixture('backend-contract/replay-bundle/flight.gsreplay'),files=>{
  const out={'config/diagrams.json':legacy};
  for(const [name,data] of files)if(name.startsWith('config/')&&name.includes('layout')){const l=JSON.parse(data);delete l.live_diagram;delete l.live_diagrams;out[name]=Buffer.from(JSON.stringify(l));}
  return out;
 });
}
// The portable replay file recorded in another mode (an older file: its layout has no live_diagram).
function replayFile(mode,testFire){
 const file=JSON.parse(fs.readFileSync(fixture('backend-contract/replay-export/flight.json'),'utf8'));
 file.mode=mode;if(file.replay_context)file.replay_context.recorded_mode=mode;
 file.layout.test_fire_mode=testFire;delete file.layout.live_diagram;delete file.layout.live_diagrams;
 return Buffer.from(JSON.stringify(file));
}

const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
const errors=[],requests=[];
const watch=page=>{
 page.on('pageerror',e=>errors.push(e.message));
 page.on('request',r=>{const u=new URL(r.url());if(u.protocol==='blob:'||u.protocol==='data:')return;
  if(['document','script','stylesheet','font','image'].includes(r.resourceType())&&u.origin===new URL(base).origin&&!u.pathname.startsWith('/api'))return;
  if(/\.(wasm|js|glb|json)$/.test(u.pathname)&&u.pathname.startsWith('/assets/'))return;
  requests.push(r.method()+' '+r.url());});
};
const panel=page=>page.locator('#gs26-pid-panel');
async function openTab(page){
 await page.locator('nav[aria-label="Mission navigation"] a[aria-label="Live diagrams"]').click();
 await panel(page).waitFor({timeout:30000});
}
async function expectView(page,id,title){
 await page.locator(`#gs26-pid-panel[data-diagram-panel="${id}"]`).waitFor({timeout:30000});
 assert.equal(await panel(page).locator('strong').first().innerText(),title);
 const mode=page.locator(`[role=toolbar][aria-label=Diagram] button[data-view="${id}"]`);
 assert.equal(await mode.getAttribute('aria-pressed'),'true',id+' is selected');
 assert.match(await mode.innerText(),/mode$/,id+' is marked as the mode view');
 assert.equal(await page.locator('[role=toolbar][aria-label=Diagram] button[data-view]').first().getAttribute('data-view'),id,'the mode view comes first');
 const text=await panel(page).innerText();
 assert(!/\bNaN\b|undefined/.test(text),id+': no NaN/undefined');
}
const box=page=>panel(page).evaluate(el=>{const r=el.getBoundingClientRect(),s=el.querySelector('svg.gs26-pid').getBoundingClientRect();
 return {x:r.x,y:r.y,w:r.width,h:r.height,vw:innerWidth,vh:innerHeight,sw:s.width,sh:s.height,native:document.fullscreenElement===el,full:el.dataset.full};});
async function fullScreen(page,id,{native,shot}){
 await panel(page).getByRole('button',{name:'Full screen'}).click();
 await page.locator('#gs26-pid-panel[data-full=true]').waitFor();
 await page.waitForTimeout(300);
 let b=await box(page);
 assert.equal(b.native,native,id+': Fullscreen API '+(native?'used':'absent: overlay'));
 assert(Math.abs(b.x)<1&&Math.abs(b.y)<1&&Math.abs(b.w-b.vw)<1&&Math.abs(b.h-b.vh)<1,id+': covers the viewport '+JSON.stringify(b));
 assert(b.sw>=b.vw*0.85||b.sh>=b.vh*0.7,id+': the diagram scales up to fill the screen '+JSON.stringify(b));
 // Still live: the 10 Hz render keeps going in full screen.
 const before=await panel(page).getAttribute('data-full');assert.equal(before,'true');
 if(shot&&shots)await page.screenshot({path:`${shots}/${shot}-fullscreen.png`});
 // Esc leaves.
 await page.keyboard.press('Escape');
 await page.locator('#gs26-pid-panel[data-full=false]').waitFor();
 b=await box(page);assert(!b.native&&b.w<b.vw+1,id+': left full screen with Esc');
 assert.equal(await page.evaluate(()=>document.fullscreenElement),null);
 // The button leaves too.
 await panel(page).getByRole('button',{name:'Full screen'}).click();
 await page.locator('#gs26-pid-panel[data-full=true]').waitFor();
 await panel(page).getByRole('button',{name:'Exit full screen'}).click();
 await page.locator('#gs26-pid-panel[data-full=false]').waitFor();
 assert.equal(await page.evaluate(()=>document.fullscreenElement),null,id+': left full screen with the button');
}
try {
 // Desktop: demo (flight), the views by hand, then replays recorded in each mode.
 const context=await browser.newContext({viewport:{width:1440,height:900}});
 const page=await context.newPage();watch(page);
 await page.goto(base+'/preview');
 await page.getByText('Demo mode — no station connected',{exact:true}).waitFor();
 await openTab(page);
 await expectView(page,'flight','Flight: fill system and vehicle on the pad');
 for(const needle of ['KG50','Flight computer','Drogue channel','Tank PT (VB)','Fill mass (KG50)'])await panel(page).getByText(needle).first().waitFor({state:'attached'});
 assert.equal(await panel(page).locator('[data-reading="kg1000_kg"],[data-reading="chamber_pt"]').count(),0,'no KG1000 or chamber PT in flight');
 // Live: the demo clock moves values in place.
 await page.getByLabel('Speed').selectOption('50').catch(()=>{});
 const tank=panel(page).locator('[data-reading="tank_pt"]');
 const t0=await tank.textContent();
 await page.waitForFunction(t=>document.querySelector('[data-reading="tank_pt"]')?.textContent!==t,t0,{timeout:30000});
 if(shots)await page.screenshot({path:`${shots}/flight-normal.png`});
 await fullScreen(page,'flight',{native:true,shot:'flight'});
 // Another view by hand, then back to the mode's.
 await page.locator('[role=toolbar][aria-label=Diagram] button[data-view="network"]').click();
 await page.locator('#gs26-pid-panel[data-diagram-panel="network"]').waitFor();
 await fullScreen(page,'network',{native:true,shot:'network'});
 await page.locator('[role=toolbar][aria-label=Diagram] button[data-view="flight"]').click();
 await expectView(page,'flight','Flight: fill system and vehicle on the pad');
 // Broadcast size, full screen too.
 await page.getByRole('button',{name:'Broadcast size'}).click();
 await fullScreen(page,'flight-broadcast',{native:true});
 await page.getByRole('button',{name:'Broadcast size'}).click();

 // A test-fire recording: the stand view, chosen from the mode.
 await page.getByLabel('Open replay file').setInputFiles({name:'test-fire.json',mimeType:'application/json',buffer:replayFile('test_fire',true)});
 await page.getByText('Offline replay — test-fire.json',{exact:true}).waitFor();
 await openTab(page);
 await expectView(page,'test_fire','Test fire: fill system and motor on the stand');
 for(const id of ['kg50_kg','kg1000_kg','thrust','chamber_pt','fill_mass','tank_pt'])assert.equal(await panel(page).locator(`[data-reading="${id}"]`).count(),1,'test fire: '+id);
 assert.equal(await panel(page).locator('[data-node="fc"],[data-node="av_can"]').count(),0,'no AV bay on the stand');
 if(shots)await page.screenshot({path:`${shots}/test_fire-normal.png`});
 await fullScreen(page,'test_fire',{native:true,shot:'test_fire'});

 // An integrated-ground-test recording: the stand and the rocket body.
 await page.getByLabel('Open replay file').setInputFiles({name:'ground-test.json',mimeType:'application/json',buffer:replayFile('integrated_ground_test',false)});
 await page.getByText('Offline replay — ground-test.json',{exact:true}).waitFor();
 await openTab(page);
 await expectView(page,'ground_test','Integrated ground test: stand and rocket');
 await panel(page).getByText('Rocket on the ground (FC replaying: SIMULATED)').waitFor();
 for(const id of ['kg1000_kg','chamber_pt','sim_alt'])assert.equal(await panel(page).locator(`[data-reading="${id}"]`).count(),1,'ground test: '+id);
 if(shots)await page.screenshot({path:`${shots}/ground_test-normal.png`});
 await fullScreen(page,'ground_test',{native:true,shot:'ground_test'});

 // A HITL recording: like flight.
 await page.getByLabel('Open replay file').setInputFiles({name:'hitl.json',mimeType:'application/json',buffer:replayFile('hitl',false)});
 await page.getByText('Offline replay — hitl.json',{exact:true}).waitFor();
 await openTab(page);
 await expectView(page,'flight','Flight: fill system and vehicle on the pad');

 // The station's bundle (new): its layout names the view.
 await page.getByLabel('Open replay file').setInputFiles(fixture('backend-contract/replay-bundle/flight.gsreplay'));
 await page.getByText('Offline replay — flight.gsreplay',{exact:true}).waitFor();
 await openTab(page);
 await expectView(page,'flight','Flight: fill system and vehicle on the pad');

 // A bundle from the per-system set still opens: the mode view from the
 // client copy, and its old views listed after it.
 await page.getByLabel('Open replay file').setInputFiles({name:'legacy.gsreplay',mimeType:'application/octet-stream',buffer:legacyBundle()});
 await page.getByText('Offline replay — legacy.gsreplay',{exact:true}).waitFor();
 await openTab(page);
 await expectView(page,'flight','Flight: fill system and vehicle on the pad');
 for(const id of ['fill','av_bay','stand'])assert.equal(await page.locator(`[role=toolbar][aria-label=Diagram] button[data-view="${id}"]`).count(),1,'legacy view listed: '+id);
 await page.locator('[role=toolbar][aria-label=Diagram] button[data-view="fill"]').click();
 await page.locator('#gs26-pid-panel[data-diagram-panel="fill"]').waitFor();
 await fullScreen(page,'legacy-fill',{native:true});
 await page.getByRole('button',{name:'Back to demo'}).click();
 await page.getByText('Demo mode — no station connected',{exact:true}).waitFor();
 await context.close();

 // Phone width, iPhone-like: no element Fullscreen API, so the overlay.
 const phone=await browser.newContext({viewport:{width:390,height:844},deviceScaleFactor:2,isMobile:true,hasTouch:true});
 await phone.addInitScript(()=>{for(const k of ['requestFullscreen','webkitRequestFullscreen'])delete Element.prototype[k];});
 const mobile=await phone.newPage();watch(mobile);
 await mobile.goto(base+'/preview');
 await mobile.getByText('Demo mode — no station connected',{exact:true}).waitFor();
 const nav=mobile.locator('nav[aria-label="Mission navigation"] a[aria-label="Live diagrams"]');
 if(!(await nav.isVisible())){const menu=mobile.getByRole('button',{name:/menu|navigation|tabs/i}).first();if(await menu.count())await menu.click();}
 await openTab(mobile);
 await expectView(mobile,'flight','Flight: fill system and vehicle on the pad');
 assert(await mobile.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no page overflow at 390');
 if(shots)await mobile.screenshot({path:`${shots}/flight-phone-normal.png`});
 await fullScreen(mobile,'flight-phone',{native:false,shot:'flight-phone'});
 assert(await mobile.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no page overflow after full screen');
 await phone.close();

 assert.deepEqual(requests,[],'no network requests');
 assert.deepEqual(errors,[]);
 console.log('PASS: live diagrams: one view per station mode (flight demo, test-fire, ground-test, HITL and bundle replays, an older per-system bundle), full screen with the Fullscreen API and as an overlay without it (390 px), Esc and the button leave, live in full screen, zero network');
} catch(e) {for(const c of browser.contexts())for(const p of c.pages())if(shots)await p.screenshot({path:`${shots}/live-diagrams-failure.png`}).catch(()=>{});throw e;} finally {await browser.close();}
