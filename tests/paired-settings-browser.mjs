// Real Wasm component with a synthetic, local HTTP peer. No radio or outputs.
import assert from 'node:assert/strict';
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'/tmp/checklist-browser-tools/node_modules/playwright/index.mjs');
const document={heads:[{clock:{gs:2,rrls:1},value:20},{clock:{gs:1,rrls:2},value:21}]};
let requests=[], posts=[], outage=false, stale=false, resolved=false;
let documents={"calibration/full_mass_kg":document};
const server=http.createServer(async(req,res)=>{
 const url=new URL(req.url,'http://localhost');
 if(req.method==='POST') posts.push(url.pathname);
 res.setHeader('Cache-Control','no-store');
 if(url.pathname.startsWith('/api/')) {
  res.setHeader('Content-Type','application/json');
  if(outage){res.statusCode=503;res.end('{}');return;}
  if(url.pathname==='/api/mission-settings'){res.end(JSON.stringify({enabled:true,link_available:true,pending:resolved?1:0,conflicts:resolved?0:Object.keys(documents).length,waiting_for_idle:resolved,error:null}));return;}
  if(url.pathname==='/api/mission-settings/conflicts'){res.end(JSON.stringify(documents));return;}
  if(url.pathname==='/api/mission-settings/resolve') {
   let raw='';for await(const chunk of req)raw+=chunk;
   const body=JSON.parse(raw);requests.push(body);
   if(stale){res.statusCode=409;res.end('{}');return;}
   assert(documents[body.key], 'only displayed settings can be selected');
   assert.deepEqual(body,{key:body.key,expected:documents[body.key],value:documents[body.key].heads[1].value});
   resolved=true;res.end('{"resolved":true}');return;
  }
  res.statusCode=404;res.end('{}');return;
 }
 const name=url.pathname==='/'?'index.html':url.pathname.slice(1);
 const file=path.join(process.env.GS_PAIRED_WEB_ROOT||'/tmp/paired-settings-browser',name);
 if(!fs.existsSync(file)){res.statusCode=404;res.end();return;}
 res.setHeader('Content-Type',name.endsWith('.wasm')?'application/wasm':name.endsWith('.js')?'text/javascript':'text/html');
 res.end(fs.readFileSync(file));
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const url=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
try {
 const page=await browser.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(url);await page.getByRole('button',{name:'Use RRLS',exact:true}).waitFor();
 assert.match(await page.locator('section').first().innerText(),/Connected/);
 for(const width of [320,390,844,1440]) {
  await page.setViewportSize({width,height:900});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),`no overflow at ${width}`);
  assert(await page.getByRole('button',{name:'Use RRLS',exact:true}).evaluate(e=>e.getBoundingClientRect().height>=44));
  await page.screenshot({path:`/tmp/paired-settings-${width}.png`,fullPage:true});
 }
 await page.getByRole('button',{name:'Inspect saved values'}).last().click();
 await page.getByRole('dialog',{name:'Saved values'}).waitFor();
 assert.match(await page.locator('pre').innerText(),/21/);
 await page.getByRole('button',{name:'Close',exact:true}).focus();await page.keyboard.press('Escape');
 await page.getByRole('dialog').waitFor({state:'detached'});
 stale=true;await page.getByRole('button',{name:'Use RRLS',exact:true}).click();
 await page.getByText('Settings changed elsewhere. Review the latest versions.').waitFor();
 assert.equal(requests.length,1);await page.waitForTimeout(2300);assert.equal(requests.length,1,'stale choice is not retried');
 stale=false;await page.getByRole('button',{name:'Use RRLS',exact:true}).click();
 await page.getByText('Choice saved. Settings apply when safe.').waitFor();
 await page.getByText('Return to Idle and clear qualification to apply pending settings.').waitFor();
 assert.equal(requests.length,2);
 outage=true;await page.getByText('Shared settings are unavailable on this station').waitFor();
 outage=false;await page.getByText('Shared settings are unavailable on this station').waitFor({state:'detached'});
 resolved=false;await page.goto(url+'/?readonly=1');await page.getByRole('heading',{name:'Full-mass reference'}).waitFor();
 assert.equal(await page.getByRole('button',{name:/^Use /}).count(),0,'read-only never exposes write');
 assert.equal(requests.length,2);assert.deepEqual(errors,[]);
 const versions=value=>{
  const earlier=structuredClone(value);
  if('enabled'in earlier) earlier.enabled=!earlier.enabled;
  else if('nitrous'in earlier) earlier.nitrous.target_mass_kg-=1;
  else if('pressure_step_psi'in earlier) earlier.pressure_step_psi-=5;
  else if('profiles'in earlier) earlier.profiles[0].label='Earlier profile';
  return {heads:[{clock:{gs:2,rrls:1},value:earlier},{clock:{gs:1,rrls:2},value}]};
 };
 const cases=[
  ['mission/fill_targets','Fill targets',{version:1,fill_source:'kg1000_absolute',nitrous:{target_mass_kg:21,target_pressure_psi:745},nitrogen:{target_mass_kg:10,target_pressure_psi:120}},'21.00 kg · 745 psi'],
  ['mission/gse_limits','Sequence limits',{nitrogen_target_psi:120,pressure_step_psi:20,pressure_ceiling_psi:null,maximum_zero_offset_psi:20,grouped_panel:false},'Pressure step'],
  ['mission/saved_flight_setup','Saved flight setup',{version:1,selected_profile_id:'w2',profiles:[{id:'w2',label:'Wind 2',wind_level:2,kalman:{accel_variance:.2}}]},'Wind 2'],
  ['mission/auto_zero_preference','Automatic zeroing',{enabled:true},'Enabled'],
 ];
 for(const [key,title,value,expected] of cases){
  resolved=false;documents={[key]:versions(value)};
  await page.goto(url+'/?mission_only=1');
  await page.getByRole('heading',{name:title,exact:true}).waitFor();
  assert.match(await page.locator('article').innerText(),new RegExp(expected.replace(/[.*+?^${}()|[\]\\]/g,'\\$&')));
  assert(!/Not calibrated|Fit/.test(await page.locator('article').innerText()));
  for(const width of [320,390,844,1440]) {
   await page.setViewportSize({width,height:900});
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),`${key}: no overflow at ${width}`);
   assert(await page.getByRole('button',{name:'Use RRLS',exact:true}).evaluate(e=>e.getBoundingClientRect().height>=44));
   if(width===390) await page.screenshot({path:`/tmp/frontend-paired-settings/${key.split('/').at(-1)}-390.png`,fullPage:true});
  }
  const before=requests.length;
  await page.getByRole('button',{name:'Use RRLS',exact:true}).click();
  await page.getByText('Choice saved. Settings apply when safe.').waitFor();
  assert.equal(requests.length,before+1);assert.equal(requests.at(-1).key,key);
 }
 resolved=false;documents={'calibration/full_mass_kg':{heads:[{clock:{gs:2,rrls:1},value:.23999999463558197},{clock:{gs:1,rrls:2},value:.07999999821186066}]}};
 await page.goto(url);
 await page.getByRole('heading',{name:'Full-mass reference',exact:true}).waitFor();
 await page.getByRole('button',{name:'Use RRLS',exact:true}).click();
 await page.getByText('Choice saved. Settings apply when safe.').waitFor();
 assert.equal(requests.at(-1).value,.07999999821186066,'preserve exact float in selected and expected causal heads');
 resolved=false;documents={'calibration/full_mass_kg':document,'mission/fill_targets':versions(cases[0][2])};
 await page.goto(url+'/?mission_only=1');
 await page.getByRole('heading',{name:'Fill targets',exact:true}).waitFor();
 assert.equal(await page.getByRole('heading',{name:'Full-mass reference',exact:true}).count(),0,'mission permission cannot reveal calibration');
 assert.equal(await page.getByRole('button',{name:'Use RRLS',exact:true}).count(),1);
 await page.goto(url+'/?calibration_only=1');
 await page.getByRole('heading',{name:'Full-mass reference',exact:true}).waitFor();
 assert.equal(await page.getByRole('button',{name:'Use RRLS',exact:true}).count(),1,'calibration editor cannot resolve mission settings');
 const fillArticle=page.getByRole('heading',{name:'Fill targets',exact:true}).locator('..');
 assert.equal(await fillArticle.getByRole('button',{name:/^Use /}).count(),0);
 assert.equal(requests.length,7);assert(posts.every(path=>path==='/api/mission-settings/resolve'),'review only posts resolution requests');assert.deepEqual(errors,[]);
 const stationPositions={ground_station:{latitude:43,longitude:-78.8,altitude_m:null,accuracy_m:null,source:'manual'},rrls:null};
 const previous=structuredClone(stationPositions);previous.ground_station.latitude=42;
 resolved=false;documents={'mission/saved_station_positions':{heads:[{clock:{gs:2,rrls:1},value:previous},{clock:{gs:1,rrls:2},value:stationPositions}]}};
 await page.goto(url+'/?mission_only=1');
 await page.getByRole('heading',{name:'Saved station positions',exact:true}).waitFor();
 assert.equal(await page.getByRole('button',{name:/^Use /}).count(),0,'command permission alone cannot edit administrator positions');
 assert.match(await page.locator('article').innerText(),/43\.000000, -78\.800000/);
 await page.goto(url+'/?admin=1');
 await page.getByRole('button',{name:'Use RRLS',exact:true}).waitFor();
 for(const width of [320,390,844,1440]) {
  await page.setViewportSize({width,height:900});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),`positions fit at ${width}`);
 }
 await page.getByRole('button',{name:'Use RRLS',exact:true}).click();
 await page.getByText('Choice saved. Settings apply when safe.').waitFor();
 assert.equal(requests.length,8);assert.equal(requests.at(-1).key,'mission/saved_station_positions');
 assert.deepEqual(requests.at(-1).value,stationPositions);
 assert.deepEqual(errors,[]);
 console.log('PASS real Wasm: four widths, 44px controls, details/Escape, exact causal CAS, stale rejection without retry, deferred apply, transient error recovery, read-only access, mission summaries and exact resolution requests, administrator-only station positions, split mission/calibration permissions');
}finally{await browser.close();await new Promise(r=>server.close(r));}
