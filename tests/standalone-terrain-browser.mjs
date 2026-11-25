// The actual Rust-exported standalone replay HTML with the station's terrain
// embedded (recording_download `standalone_html_export_carries_terrain_and_imagery`),
// opened from file:// with the browser offline: the flight scene draws the
// heightmap as terrain and paints the embedded imagery, with no request.
//
//   GS_REPLAY_TERRAIN_HTML_PROOF=/tmp/terrain.html cargo test -- standalone_html_export_carries_terrain
//   GS_REPLAY_TERRAIN_HTML_PROOF=/tmp/terrain.html PLAYWRIGHT_MODULE=… CHROME_BINARY=… node tests/standalone-terrain-browser.mjs
import path from 'node:path';
import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const html=process.env.GS_REPLAY_TERRAIN_HTML_PROOF;
if(!html){console.error('SKIP standalone-terrain-browser.mjs: GS_REPLAY_TERRAIN_HTML_PROOF must name the Rust-exported standalone replay HTML; scripts/run-browser-suite.sh produces it');process.exit(77);}
const shots=process.env.GS_UI_ARTIFACTS;
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY,args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']});
try{
 const context=await browser.newContext({viewport:{width:1100,height:900}});await context.setOffline(true);
 const page=await context.newPage(),errors=[],network=[];
 page.on('pageerror',e=>errors.push(e.message));page.on('request',r=>{if(/^https?:/.test(r.url()))network.push(r.url());});
 await page.goto(pathToFileURL(path.resolve(html)).href);
 await page.waitForFunction(()=>typeof player!=='undefined'&&player.visual?.canvas?.dataset.terrain==='dem',null,{timeout:60000});
 // Imagery from the file is painted on both terrain textures.
 await page.waitForFunction(()=>['terrain-near','terrain-far'].every(n=>player.visual.scene.getObjectByName(n)?.material?.map),null,{timeout:30000});
 const note=await page.evaluate(()=>document.getElementById('replay').innerText);
 assert.match(note,/Terrain: Mapzen\/Tilezen/,'the heightmap attribution is shown');
 assert.match(note,/Imagery: Esri/,'the imagery attribution is shown');
 if(shots)await page.screenshot({path:`${shots}/standalone-replay-terrain.png`,fullPage:true});
 assert.deepEqual(network,[],'no network request');assert.deepEqual(errors,[]);
 console.log('PASS: the standalone replay HTML draws its embedded terrain and imagery offline from file://');
}finally{await browser.close();}
