// Isolated fake API and synthetic browser location; never contacts a real station.
import assert from 'node:assert/strict';
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
const bundle=path.resolve(process.env.GS_STATION_POSITIONS_BUNDLE||'target/station-positions-web');
let saved={version:1,revision:0,ground_station:null,rrls:null},posts=0;
const server=http.createServer((req,res)=>{
 if(req.url==='/api/station_positions'){
  res.setHeader('content-type','application/json');
  if(req.method==='GET'){res.end(JSON.stringify(saved));return;}
  let body='';req.on('data',b=>{body+=b});req.on('end',()=>{
   const draft=JSON.parse(body);posts++;
   if(draft.expected_revision!==saved.revision){res.statusCode=409;res.end('Positions changed elsewhere; reload before saving');return;}
   saved={version:1,revision:saved.revision+1,ground_station:draft.ground_station,rrls:draft.rrls,updated_at_ms:Date.now(),updated_by:'admin'};res.end(JSON.stringify(saved));
  });return;
 }
 const name=req.url==='/'?'index.html':req.url.slice(1),file=path.resolve(bundle,name);
 if(!file.startsWith(bundle+path.sep)||!fs.existsSync(file)){res.statusCode=404;res.end();return;}
 res.setHeader('content-type',file.endsWith('.wasm')?'application/wasm':file.endsWith('.js')?'application/javascript':'text/html');fs.createReadStream(file).pipe(res);
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin=`http://127.0.0.1:${server.address().port}`;
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY,args:['--js-flags=--expose-gc']});
const button=name=>({name,exact:true});
// Replaces navigator.geolocation with a recorder; `mode` decides how the one request ends.
const mockGeolocation=mode=>{
 window.__geoCalls=[];window.__watchCalls=0;
 Object.defineProperty(navigator,'geolocation',{configurable:true,value:{
  getCurrentPosition:(ok,fail,options)=>{window.__geoCalls.push(options);
   if(mode==='denied')setTimeout(()=>fail({code:1,message:'User denied Geolocation'}),10);
   else if(mode==='timeout')setTimeout(()=>fail({code:3,message:'Timeout expired'}),10);
   else if(mode==='unavailable')setTimeout(()=>fail({code:2,message:'Position unavailable'}),10);
   // A slow answer (prompt left open) arrives after garbage collection has run.
   else if(mode==='slow')setTimeout(()=>{if(typeof gc==='function')gc();setTimeout(()=>ok({coords:{latitude:42.25,longitude:-79.5,altitude:null,accuracy:20}}),50);},800);
   else if(mode==='success')setTimeout(()=>ok({coords:{latitude:42.5,longitude:-79.25,altitude:null,accuracy:7.6}}),10);
   /* 'silent': the permission prompt is never answered */},
  watchPosition:()=>{window.__watchCalls++;return 1;},clearWatch:()=>{}}});
};
async function oneShot(mode,expected,{insecure=false,clock=false}={}){
 const context=await browser.newContext();
 await context.addInitScript(mockGeolocation,mode);
 if(insecure)await context.addInitScript(()=>Object.defineProperty(window,'isSecureContext',{configurable:true,value:false}));
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 if(clock)await page.clock.install();
 await page.goto(origin);
 const rr=page.getByRole('group',{name:'RRLS',exact:true});
 await rr.getByLabel('Latitude (°)').fill('41.5');await rr.getByLabel('Longitude (°)').fill('-77.5');
 const postsBefore=posts;
 await rr.getByRole('button',button('Use this device’s location')).click();
 if(clock){
  await rr.getByRole('button',button('Locating…')).waitFor();
  assert(await rr.locator('input').first().isDisabled(),'inputs locked while the request is pending');
  await page.clock.fastForward(31_000);await page.clock.runFor(1_000);
 }
 await page.getByRole('status').filter({hasText:expected}).waitFor();
 // The busy state always clears; a failed request leaves the typed draft untouched.
 await rr.getByRole('button',button('Use this device’s location')).waitFor();
 assert(await rr.getByLabel('Latitude (°)').isEnabled());
 if(!['success','slow'].includes(mode)||insecure)assert.equal(await rr.getByLabel('Latitude (°)').inputValue(),'41.5');
 assert(await page.getByRole('button',button('Save positions')).isEnabled());
 const calls=await page.evaluate(()=>({geo:window.__geoCalls,watch:window.__watchCalls}));
 assert.equal(calls.watch,0,'never starts a watch');
 if(insecure)assert.equal(calls.geo.length,0,'no request outside a secure context');
 else assert.deepEqual(calls.geo,[{enableHighAccuracy:true,timeout:20000,maximumAge:0}],'exactly one fresh high-accuracy request');
 assert.equal(posts,postsBefore,'a location request never saves');
 assert.deepEqual(errors,[]);
 return {context,page,rr};
}
try{
 const context=await browser.newContext({geolocation:{latitude:43.02,longitude:-78.82,accuracy:12},permissions:['geolocation']});
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(origin);const gs=page.getByRole('group',{name:'GroundStation',exact:true}),rr=page.getByRole('group',{name:'RRLS',exact:true});
 await page.getByRole('button',button('Save positions')).waitFor();
 await gs.getByLabel('Latitude (°)').fill('91');await gs.getByLabel('Longitude (°)').fill('-78.8');
 await page.getByRole('button',button('Save positions')).click();await page.getByRole('status').filter({hasText:'latitude must be within'}).waitFor();assert.equal(posts,0);
 await gs.getByLabel('Latitude (°)').fill('43.0');await gs.getByLabel('Longitude (°)').fill('');
 await page.getByRole('button',button('Save positions')).click();await page.getByRole('status').filter({hasText:'GroundStation: enter both latitude and longitude'}).waitFor();assert.equal(posts,0);
 await gs.getByRole('button',button('Use this device’s location')).click();await page.getByText('GroundStation location copied from this device. Save positions to share it.',{exact:true}).waitFor();
 assert.equal(await gs.getByLabel('Latitude (°)').inputValue(),'43.02');assert.equal(await gs.getByLabel('Longitude (°)').inputValue(),'-78.82');
 assert.equal(await gs.locator('.station-location-source').textContent(),'From device location, ±12 m · not saved yet');
 assert.equal(posts,0,'the device location stays a draft until Save');
 // Decimal commas are accepted for typed coordinates.
 await rr.getByLabel('Latitude (°)').fill('43,0');await rr.getByLabel('Longitude (°)').fill('-78,7');
 assert.equal(await rr.locator('.station-location-source').textContent(),'Entered manually · not saved yet');
 await page.getByRole('button',button('Save positions')).click();await page.getByText('Station positions saved.',{exact:true}).waitFor();
 assert.equal(saved.ground_station.source,'device_location');assert.equal(saved.ground_station.accuracy_m,12);
 assert.equal(saved.rrls.source,'manual');assert.equal(saved.rrls.accuracy_m,null);assert.equal(saved.rrls.latitude,43);assert.equal(saved.rrls.longitude,-78.7);assert.equal(saved.revision,1);
 await page.getByText('Last saved just now by admin.',{exact:true}).waitFor();
 await page.reload();await page.getByRole('button',button('Save positions')).waitFor();await gs.getByLabel('Latitude (°)').waitFor();
 assert.equal(await gs.getByLabel('Latitude (°)').inputValue(),'43.02');assert.equal(await rr.getByLabel('Longitude (°)').inputValue(),'-78.7');
 assert.equal(await gs.locator('.station-location-source').textContent(),'From device location, ±12 m');
 // Another station (or the paired RRLS) saved a new GroundStation position meanwhile.
 saved={...saved,revision:saved.revision+1,ground_station:{...saved.ground_station,latitude:43.5,source:'manual',accuracy_m:null}};
 await rr.getByLabel('Longitude (°)').fill('-78.6');await page.getByRole('button',button('Save positions')).click();
 await page.getByRole('status').filter({hasText:'Positions were changed elsewhere. Rows you did not edit now show the newly saved values'}).waitFor();
 assert.equal(saved.rrls.longitude,-78.7,'a stale draft never overwrites');assert.equal(await rr.getByLabel('Longitude (°)').inputValue(),'-78.6');
 assert.equal(await gs.getByLabel('Latitude (°)').inputValue(),'43.5','untouched row shows the newly saved value');
 await page.getByRole('button',button('Save positions')).click();await page.getByText('Station positions saved.',{exact:true}).waitFor();
 assert.equal(saved.ground_station.latitude,43.5,'saving RRLS kept the other GroundStation edit');assert.equal(saved.rrls.longitude,-78.6);
 // Both sides edited RRLS: the draft is kept and the contested row is named.
 saved={...saved,revision:saved.revision+1,rrls:{...saved.rrls,longitude:-78.4}};
 await rr.getByLabel('Longitude (°)').fill('-78.55');await page.getByRole('button',button('Save positions')).click();
 await page.getByRole('status').filter({hasText:'RRLS was also changed elsewhere. Your draft is kept'}).waitFor();
 assert.equal(saved.rrls.longitude,-78.4);
 await page.getByRole('button',button('Reload')).click();await page.getByText('Saved positions loaded.',{exact:true}).waitFor();assert.equal(await rr.getByLabel('Longitude (°)').inputValue(),'-78.4');
 for(const width of [320,390,844,1280]){
  await page.setViewportSize({width,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no page overflow '+width);
  assert(await page.locator('.station-positions-admin').evaluate(e=>e.scrollWidth<=e.clientWidth),'no form overflow '+width);
  for(const b of await page.locator('button').all()){const box=await b.boundingBox();assert(box.height>=44,'touch height '+width);}
 }
 await rr.getByRole('button',button('Clear')).click();assert.equal(saved.rrls.longitude,-78.4);
 assert.equal(await rr.locator('.station-location-source').textContent(),'Not set · not saved yet');
 await page.getByRole('button',button('Save positions')).click();await page.getByText('Station positions saved.',{exact:true}).waitFor();assert.equal(saved.rrls,null);
 assert.deepEqual(errors,[]);await context.close();
 const s=await oneShot('success','RRLS location copied from this device.');
 assert.equal(await s.rr.getByLabel('Latitude (°)').inputValue(),'42.5');assert.equal(await s.rr.getByLabel('Longitude (°)').inputValue(),'-79.25');
 assert.equal(await s.rr.locator('.station-location-source').textContent(),'From device location, ±8 m · not saved yet');
 assert.equal(saved.rrls,null);await s.context.close();
 const slow=await oneShot('slow','RRLS location copied from this device.');
 assert.equal(await slow.rr.getByLabel('Latitude (°)').inputValue(),'42.25');await slow.context.close();
 // The remaining cases must leave the typed draft alone, so oneShot checks it.
 const fix=async (...args)=>{const r=await oneShot(...args);await r.context.close();};
 await fix('denied','Location permission was denied. Allow location for this page or enter coordinates.');
 await fix('timeout','The device did not find its location in time. Try again outdoors or enter coordinates.');
 await fix('unavailable','The device location is unavailable. Check location services or enter coordinates.');
 await fix('silent','No location arrived in time. Answer the location prompt or enter coordinates.',{clock:true});
 await fix('success','Device location needs a secure page. Open this station over https:// or enter coordinates.',{insecure:true});
 assert.equal(saved.rrls,null);
 console.log('PASS: bounds and partial pairs, decimal commas, one-shot device location (success, slow answer after GC, denied, timeout, unavailable, unanswered prompt, insecure page), draft-only until Save, source/accuracy/age labels, persistence, per-row conflict merge, explicit clear and four responsive widths');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
