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
   saved={version:1,revision:saved.revision+1,ground_station:draft.ground_station,rrls:draft.rrls};res.end(JSON.stringify(saved));
  });return;
 }
 const name=req.url==='/'?'index.html':req.url.slice(1),file=path.resolve(bundle,name);
 if(!file.startsWith(bundle+path.sep)||!fs.existsSync(file)){res.statusCode=404;res.end();return;}
 res.setHeader('content-type',file.endsWith('.wasm')?'application/wasm':file.endsWith('.js')?'application/javascript':'text/html');fs.createReadStream(file).pipe(res);
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin=`http://127.0.0.1:${server.address().port}`;
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
try{
 const context=await browser.newContext({geolocation:{latitude:43.02,longitude:-78.82,accuracy:12},permissions:['geolocation']});
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(origin);const gs=page.getByRole('group',{name:'GroundStation',exact:true}),rr=page.getByRole('group',{name:'RRLS',exact:true});
 await page.getByRole('button',{name:'Save positions',exact:true}).waitFor();
 await gs.getByLabel('Latitude (°)').fill('91');await gs.getByLabel('Longitude (°)').fill('-78.8');
 await page.getByRole('button',{name:'Save positions',exact:true}).click();await page.getByRole('status').filter({hasText:'Latitude must be within'}).waitFor();assert.equal(posts,0);
 await gs.getByRole('button',{name:'Use this device’s location',exact:true}).click();await page.getByText('Location copied. Save positions to share it.',{exact:true}).waitFor();
 assert.equal(await gs.getByLabel('Latitude (°)').inputValue(),'43.02');assert.equal(await gs.getByLabel('Longitude (°)').inputValue(),'-78.82');
 await rr.getByLabel('Latitude (°)').fill('43.0');await rr.getByLabel('Longitude (°)').fill('-78.7');
 await page.getByRole('button',{name:'Save positions',exact:true}).click();await page.getByText('Station positions saved.',{exact:true}).waitFor();
 assert.equal(saved.ground_station.source,'device_location');assert.equal(saved.rrls.source,'manual');assert.equal(saved.revision,1);
 await page.reload();await page.getByRole('button',{name:'Save positions',exact:true}).waitFor();await gs.getByLabel('Latitude (°)').waitFor();
 assert.equal(await gs.getByLabel('Latitude (°)').inputValue(),'43.02');assert.equal(await rr.getByLabel('Longitude (°)').inputValue(),'-78.7');
 saved.revision++;await rr.getByLabel('Longitude (°)').fill('-78.6');await page.getByRole('button',{name:'Save positions',exact:true}).click();
 await page.getByRole('status').filter({hasText:'HTTP 409'}).waitFor();assert.equal(saved.rrls.longitude,-78.7);assert.equal(await rr.getByLabel('Longitude (°)').inputValue(),'-78.6');
 await page.getByRole('button',{name:'Reload',exact:true}).click();await page.getByText('Saved positions loaded.',{exact:true}).waitFor();assert.equal(await rr.getByLabel('Longitude (°)').inputValue(),'-78.7');
 for(const width of [320,390,844,1280]){
  await page.setViewportSize({width,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no page overflow '+width);
  assert(await page.locator('.station-positions-admin').evaluate(e=>e.scrollWidth<=e.clientWidth),'no form overflow '+width);
  for(const b of await page.locator('button').all()){const box=await b.boundingBox();assert(box.height>=44,'touch height '+width);}
 }
 await rr.getByRole('button',{name:'Clear',exact:true}).click();assert.equal(saved.rrls.longitude,-78.7);
 await page.getByRole('button',{name:'Save positions',exact:true}).click();await page.getByText('Station positions saved.',{exact:true}).waitFor();assert.equal(saved.rrls,null);
 const denied=await browser.newContext();await denied.addInitScript(()=>Object.defineProperty(navigator,'geolocation',{value:{getCurrentPosition:(_s,f)=>f({code:1})}}));
 const deniedPage=await denied.newPage();await deniedPage.goto(origin);await deniedPage.getByRole('group',{name:'RRLS',exact:true}).getByRole('button',{name:'Use this device’s location',exact:true}).click();
 await deniedPage.getByText('Location access failed. Check device permissions or enter coordinates.',{exact:true}).waitFor();assert.equal(saved.rrls,null);
 assert.deepEqual(errors,[]);console.log('PASS: position bounds, synthetic GPS, persistence, conflict preservation, explicit clear, denied location and four responsive widths');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
