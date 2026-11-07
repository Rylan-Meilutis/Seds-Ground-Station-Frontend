// Tests the actual Rust-exported standalone HTML, with the browser network offline.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {pathToFileURL,fileURLToPath} from 'node:url';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'/tmp/rme-browser-profile/node_modules/playwright/index.mjs');
const html=process.env.GS_REPLAY_HTML_PROOF;if(!html)throw Error('GS_REPLAY_HTML_PROOF must identify the actual Rust-exported file');
const fixture=fileURLToPath(new URL('./fixtures/offline-flight.json',import.meta.url));
const readableMeasurements=[];
const longValve='SavedValve_'+ 'V'.repeat(117), longState='SavedState_'+ 'S'.repeat(117);
async function readable(page,width,kind){
 await page.setViewportSize({width,height:900});
 await page.getByText(longValve+': '+longState,{exact:true}).waitFor();
 const measurement=await page.evaluate(()=>({viewport:innerWidth,document:document.documentElement.scrollWidth,viewer:document.getElementById('replay').getBoundingClientRect().width,raw:player.package.samples[0].values[0],state:player.visual.assembly.valves[0].part.userData.state}));
 readableMeasurements.push({width,kind,...measurement});
 if(process.env.GS_REPLAY_READABLE_RESULT)fs.writeFileSync(process.env.GS_REPLAY_READABLE_RESULT,JSON.stringify(readableMeasurements,null,2));
 assert.equal(measurement.raw,1,'saved raw valve value preserved');assert.equal(measurement.state,'unknown','custom saved label never invents Open/Closed geometry');
 assert(measurement.document<=measurement.viewport,'offline saved labels stay within viewport at '+width+'/'+kind);
}
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY||'/home/rylan/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome'});
try{
 const context=await browser.newContext({viewport:{width:1000,height:900}});await context.setOffline(true);
 const page=await context.newPage(),errors=[],network=[];page.on('pageerror',e=>errors.push(e.message));page.on('request',r=>{if(/^https?:/.test(r.url()))network.push(r.url());});
 await page.goto(pathToFileURL(path.resolve(html)).href);
 await page.getByText('Pilot (saved name): Open',{exact:true}).waitFor();
 await page.getByText('Dump valve: Unknown',{exact:true}).waitFor();
 await page.getByText('Fuel tank: 42.00 %',{exact:true}).waitFor();
 await page.getByRole('img',{name:'Flight vehicle and named valve states'}).waitFor();
 await page.waitForFunction(()=>typeof player!=='undefined'&&player.visual?.canvas.dataset.rendered==='true');
 assert.equal(await page.evaluate(()=>player.visual.assembly.group.name),'ReplayFlightVehicle');
 assert.deepEqual(await page.evaluate(()=>player.visual.assembly.valves.map(v=>v.part.userData.state)),['open','unknown']);
 assert.equal(await page.evaluate(()=>player.visual.assembly.tanks[0].part.userData.fraction),.42);
 await page.getByRole('slider',{name:'Offline replay position'}).fill('2000');
 await page.getByText('Pilot (saved name): Closed',{exact:true}).waitFor();
 await page.getByText('Dump valve: Open',{exact:true}).waitFor();
 assert.deepEqual(await page.evaluate(()=>player.visual.assembly.valves.map(v=>v.part.userData.state)),['closed','open']);
 await page.getByRole('button',{name:'Rewind',exact:true}).click();
 await page.getByText('Pilot (saved name): Open',{exact:true}).waitFor();
 // Import a separate file, preserving names/units/config, with no backend/auth.
 const testfire=JSON.parse(fs.readFileSync(fixture));testfire.mode='test_fire';testfire.valve_mappings[0].label='Saved pilot name <not HTML>';
 await page.locator('#import').setInputFiles({name:'test-fire.seds-replay.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify(testfire))});
 await page.getByRole('img',{name:'Test-fire motor stand and named valve states'}).waitFor();
 await page.getByText('Saved pilot name <not HTML>: Open',{exact:true}).waitFor();
 await page.waitForFunction(()=>player.visual?.canvas.dataset.rendered==='true'&&player.visual.assembly.group.name==='ReplayMotorStand');
 assert.equal(await page.evaluate(()=>!!player.visual.assembly.group.getObjectByName('HorizontalMotor')),true);
 assert.equal(await page.evaluate(()=>!!player.visual.assembly.group.getObjectByName('Nose')),false);
 await page.getByRole('button',{name:'Reset 3D view',exact:true}).click();
 await page.getByRole('button',{name:'Play',exact:true}).click();await page.getByRole('button',{name:'Pause',exact:true}).waitFor();
 await page.getByText('Saved pilot name <not HTML>: Closed',{exact:true}).waitFor();
 // Bad import is rejected and the previous complete replay remains intact.
 await page.locator('#import').setInputFiles({name:'bad.json',mimeType:'application/json',buffer:Buffer.from('{"format":"bad"}')});
 await page.getByText(/Import failed: Choose a SEDS replay/).waitFor();
 assert.equal(await page.locator('script[src]').count(),0);assert.deepEqual(network,[]);assert.deepEqual(errors,[]);
 const pressure=structuredClone(testfire);pressure.view_config.ground_systems[0].unit='psi';
 await page.locator('#import').setInputFiles({name:'pressure.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify(pressure))});
 await page.waitForFunction(()=>player.visual?.canvas.dataset.rendered==='true');
 assert.equal(await page.evaluate(()=>player.visual.assembly.tanks[0].fluid.visible),false);
 assert.equal(await page.evaluate(()=>player.visual.assembly.tanks[0].unknown.visible),true);
 assert.equal(await page.locator('progress').count(),0);
 assert.deepEqual(network,[]);assert.deepEqual(errors,[]);
 const longSaved=structuredClone(testfire);longSaved.recording='Recording_'+ 'R'.repeat(118);longSaved.configuration_provenance='P'.repeat(200);longSaved.valve_mappings[0].label=longValve;longSaved.valve_mappings[0].true_label=longState;
 await page.locator('#import').setInputFiles({name:'long-saved-labels.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify(longSaved))});
 await page.waitForFunction(()=>player.visual?.canvas.dataset.rendered==='true');
 for(const width of [390,320])await readable(page,width,'normal-renderer');
 if(process.env.GS_REPLAY_SCREENSHOT)await page.screenshot({path:process.env.GS_REPLAY_SCREENSHOT,fullPage:true});
 console.log(JSON.stringify({state:'PASS',network_offline:true,http_requests:network.length,actual_Rust_export:html,flight_and_stand:true,actual_3d_models:true,file_import:true,named_valve_updates:true,config_units_roundtrip:true,unknown_preserved:true,pressure_not_liquid:true,no_live_or_commands:true}));
 await context.close();
 // The same actual export must remain usable on WebKit-like systems without
 // WebGL2. Canvas2D renders the shared geometry; saved text remains authoritative.
 const fallback=await browser.newContext();await fallback.setOffline(true);
 await fallback.addInitScript(()=>{const original=HTMLCanvasElement.prototype.getContext;HTMLCanvasElement.prototype.getContext=function(kind,...args){return String(kind).startsWith('webgl')?null:original.call(this,kind,...args);};});
 const compatibility=await fallback.newPage(),fallbackErrors=[],fallbackNetwork=[];compatibility.on('pageerror',e=>fallbackErrors.push(e.message));compatibility.on('request',r=>{if(/^https?:/.test(r.url()))fallbackNetwork.push(r.url());});
 await compatibility.goto(pathToFileURL(path.resolve(html)).href);
 await compatibility.waitForFunction(()=>typeof player!=='undefined'&&player.visual?.software&&player.visual.canvas.dataset.rendered==='true');
 await compatibility.getByText('Pilot (saved name): Open',{exact:true}).waitFor();
 await compatibility.locator('#import').setInputFiles({name:'long-saved-labels.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify(longSaved))});
 await compatibility.waitForFunction(()=>player.visual?.software&&player.visual.canvas.dataset.rendered==='true');
 for(const width of [390,320])await readable(compatibility,width,'canvas-fallback');
 assert.deepEqual(fallbackErrors,[]);assert.deepEqual(fallbackNetwork,[]);await fallback.close();
 console.log('PASS actual standalone reduced-detail 3D fallback with WebGL disabled and zero HTTP');
}finally{await browser.close();}
