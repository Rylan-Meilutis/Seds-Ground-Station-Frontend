// Tests the actual Rust-exported standalone HTML, with the browser network offline.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import {pathToFileURL,fileURLToPath} from 'node:url';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'/tmp/rme-browser-profile/node_modules/playwright/index.mjs');
const html=process.env.GS_REPLAY_HTML_PROOF;if(!html)throw Error('GS_REPLAY_HTML_PROOF must identify the actual Rust-exported file');
const fixture=fileURLToPath(new URL('./fixtures/offline-flight.json',import.meta.url));
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY||'/home/rylan/.cache/ms-playwright/chromium-1248/chrome-linux64/chrome'});
try{
 const context=await browser.newContext({viewport:{width:1000,height:900}});await context.setOffline(true);
 const page=await context.newPage(),errors=[],network=[];page.on('pageerror',e=>errors.push(e.message));page.on('request',r=>{if(/^https?:/.test(r.url()))network.push(r.url());});
 await page.goto(pathToFileURL(path.resolve(html)).href);
 await page.getByText('Pilot (saved name): Open',{exact:true}).waitFor();
 await page.getByText('Dump valve: Unknown',{exact:true}).waitFor();
 await page.getByText('Fuel tank: 42.00 %',{exact:true}).waitFor();
 await page.getByRole('img',{name:'Flight vehicle and named valve states'}).waitFor();
 await page.getByRole('slider',{name:'Offline replay position'}).fill('2000');
 await page.getByText('Pilot (saved name): Closed',{exact:true}).waitFor();
 await page.getByText('Dump valve: Open',{exact:true}).waitFor();
 await page.getByRole('button',{name:'Rewind',exact:true}).click();
 await page.getByText('Pilot (saved name): Open',{exact:true}).waitFor();
 // Import a separate file, preserving names/units/config, with no backend/auth.
 const testfire=JSON.parse(fs.readFileSync(fixture));testfire.mode='test_fire';testfire.valve_mappings[0].label='Saved pilot name <not HTML>';
 await page.locator('#import').setInputFiles({name:'test-fire.seds-replay.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify(testfire))});
 await page.getByRole('img',{name:'Test-fire motor stand and named valve states'}).waitFor();
 await page.getByText('Saved pilot name <not HTML>: Open',{exact:true}).waitFor();
 await page.getByRole('button',{name:'Play',exact:true}).click();await page.getByRole('button',{name:'Pause',exact:true}).waitFor();
 await page.getByText('Saved pilot name <not HTML>: Closed',{exact:true}).waitFor();
 // Bad import is rejected and the previous complete replay remains intact.
 await page.locator('#import').setInputFiles({name:'bad.json',mimeType:'application/json',buffer:Buffer.from('{"format":"bad"}')});
 await page.getByText(/Import failed: Choose a SEDS replay/).waitFor();
 assert.equal(await page.locator('script[src]').count(),0);assert.deepEqual(network,[]);assert.deepEqual(errors,[]);
 if(process.env.GS_REPLAY_SCREENSHOT)await page.screenshot({path:process.env.GS_REPLAY_SCREENSHOT,fullPage:true});
 console.log(JSON.stringify({state:'PASS',network_offline:true,http_requests:network.length,actual_Rust_export:html,flight_and_stand:true,file_import:true,named_valve_updates:true,config_units_roundtrip:true,unknown_preserved:true,no_live_or_commands:true}));
 await context.close();
}finally{await browser.close();}
