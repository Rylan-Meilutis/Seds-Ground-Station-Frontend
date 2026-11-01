// Exercise actual Rust-produced inverted-label HTML, never a reconstructed exporter.
import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';import {pathToFileURL} from 'node:url';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'/tmp/rme-browser-profile/node_modules/playwright/index.mjs');
const html=process.env.GS_REPLAY_INVERTED_HTML_PROOF;if(!html)throw Error('GS_REPLAY_INVERTED_HTML_PROOF must name actual Rust-exported inverted-label HTML');
const fixture=JSON.parse(fs.readFileSync(new URL('./fixtures/offline-valve-semantics.json',import.meta.url)));
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY||'/home/rylan/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome',args:['--use-gl=angle','--use-angle=swiftshader','--ignore-gpu-blocklist','--enable-webgl','--enable-unsafe-swiftshader']});const results=[];
try{
 for(const fallback of [false,true])for(const width of [1280,390,320]){
  const context=await browser.newContext({viewport:{width,height:900}});await context.setOffline(true);
  if(fallback)await context.addInitScript(()=>{const original=HTMLCanvasElement.prototype.getContext;HTMLCanvasElement.prototype.getContext=function(kind,...args){return String(kind).startsWith('webgl')?null:original.call(this,kind,...args);};});
  const page=await context.newPage(),errors=[],network=[];page.on('pageerror',e=>errors.push(e.message));page.on('request',r=>{if(/^https?:/.test(r.url()))network.push(r.url());});await page.goto(pathToFileURL(path.resolve(html)).href);
  for(const mode of ['flight','test_fire']){
   if(mode==='test_fire'){const data=structuredClone(fixture);data.mode=mode;await page.locator('#import').setInputFiles({name:'inverted-stand.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify(data))});}
   await page.waitForFunction(expected=>typeof player!=='undefined'&&player.visual?.canvas.dataset.rendered==='true'&&player.visual.assembly.group.name===expected,mode==='flight'?'ReplayFlightVehicle':'ReplayMotorStand');
   assert.equal(await page.evaluate(()=>player.visual.software),fallback);assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth),false);
   for(const [at,expected]of [[1000,['closed','open','unknown','unknown','closed','unknown','unknown','closed']],[2000,['open','closed','unknown','unknown','open','unknown','unknown','open']],[9000,Array(8).fill('unknown')]]){
    await page.getByRole('slider',{name:'Offline replay position'}).fill(String(at));
    const actual=await page.evaluate(()=>player.visual.assembly.valves.map(v=>({state:v.part.userData.state,color:v.handle.material.color.getHexString(),angle:v.handle.rotation.z,label:v.labelText})));
    assert.deepEqual(actual.map(v=>v.state),expected);
    for(const [i,status]of expected.entries()){assert.equal(actual[i].color,status==='open'?'22c55e':status==='closed'?'f59e0b':'64748b');assert.equal(actual[i].angle,status==='open'?Math.PI/2:status==='closed'?0:Math.PI/4);}
    if(at===1000){await page.getByText('Inverted pilot: Closed',{exact:true}).waitFor();await page.getByText('Custom switch: On',{exact:true}).waitFor();await page.getByText('Localized status: Fermé',{exact:true}).waitFor();assert.equal(actual[2].label,'Custom switch: On');assert.equal(actual[3].label,'Localized status: Fermé');assert.equal(actual[4].label,'Case and spacing:  cLoSeD ');
     assert.deepEqual(await page.evaluate(()=>player.package.samples[0].values),[1,0,1,0,1,null,2,0]);
     if(process.env.GS_REPLAY_SCREENSHOT_DIR){await page.waitForTimeout(80);await page.screenshot({path:path.join(process.env.GS_REPLAY_SCREENSHOT_DIR,`valve-${fallback?'canvas':'webgl'}-${mode}-${width}.png`),fullPage:true});}
    }
    if(at===2000){await page.getByText('Inverted pilot: Open',{exact:true}).waitFor();await page.getByText('Custom switch: Off',{exact:true}).waitFor();}
    if(at===9000){await page.getByText('Inverted pilot: Unknown',{exact:true}).waitFor();assert.equal(await page.locator('progress').count(),0);}
   }
   results.push({fallback,width,mode,invertedCanonical:true,customLocalizedGray:true,staleInvalidUnknown:true,exactLabelsAndRawValuesPreserved:true});
  }
  assert.deepEqual(errors,[]);assert.deepEqual(network,[]);await context.close();
 }
 console.log(JSON.stringify({state:'PASS',actualRustExport:html,httpRequests:0,cases:results},null,2));
}finally{await browser.close();}
