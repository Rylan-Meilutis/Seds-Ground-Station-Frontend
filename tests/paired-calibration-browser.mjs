// Real Wasm component with a synthetic, local HTTP peer. No radio or outputs.
import assert from 'node:assert/strict';
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'/tmp/checklist-browser-tools/node_modules/playwright/index.mjs');
const document={heads:[{clock:{gs:2,rrls:1},value:20},{clock:{gs:1,rrls:2},value:21}]};
let requests=[], outage=false, stale=false, resolved=false;
const server=http.createServer(async(req,res)=>{
 const url=new URL(req.url,'http://localhost');
 res.setHeader('Cache-Control','no-store');
 if(url.pathname.startsWith('/api/')) {
  res.setHeader('Content-Type','application/json');
  if(outage){res.statusCode=503;res.end('{}');return;}
  if(url.pathname==='/api/mission-settings'){res.end(JSON.stringify({enabled:true,link_available:true,pending:resolved?1:0,conflicts:resolved?0:1,waiting_for_idle:resolved,error:null}));return;}
  if(url.pathname==='/api/mission-settings/conflicts'){res.end(JSON.stringify({'calibration/full_mass_kg':document}));return;}
  if(url.pathname==='/api/mission-settings/resolve') {
   let raw='';for await(const chunk of req)raw+=chunk;
   const body=JSON.parse(raw);requests.push(body);
   if(stale){res.statusCode=409;res.end('{}');return;}
   assert.deepEqual(body,{key:'calibration/full_mass_kg',expected:document,value:21});
   resolved=true;res.end('{"resolved":true}');return;
  }
  res.statusCode=404;res.end('{}');return;
 }
 const name=url.pathname==='/'?'index.html':url.pathname.slice(1);
 const file=path.join(process.env.GS_PAIRED_WEB_ROOT||'/tmp/paired-calibration-browser',name);
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
  await page.screenshot({path:`/tmp/paired-calibration-${width}.png`,fullPage:true});
 }
 await page.getByRole('button',{name:'Inspect calibration values'}).last().click();
 await page.getByRole('dialog',{name:'Calibration values'}).waitFor();
 assert.match(await page.locator('pre').innerText(),/21/);
 await page.getByRole('button',{name:'Close',exact:true}).focus();await page.keyboard.press('Escape');
 await page.getByRole('dialog').waitFor({state:'detached'});
 stale=true;await page.getByRole('button',{name:'Use RRLS',exact:true}).click();
 await page.getByText('Calibration changed elsewhere. Review the latest versions.').waitFor();
 assert.equal(requests.length,1);await page.waitForTimeout(2300);assert.equal(requests.length,1,'stale choice is not retried');
 stale=false;await page.getByRole('button',{name:'Use RRLS',exact:true}).click();
 await page.getByText('Choice saved. Calibration applies when safe.').waitFor();
 await page.getByText('Return to Idle and clear qualification to apply pending calibration.').waitFor();
 assert.equal(requests.length,2);
 outage=true;await page.getByText('Shared calibration is unavailable on this station').waitFor();
 outage=false;await page.getByText('Shared calibration is unavailable on this station').waitFor({state:'detached'});
 resolved=false;await page.goto(url+'/?readonly=1');await page.getByRole('heading',{name:'Full-mass reference'}).waitFor();
 assert.equal(await page.getByRole('button',{name:/^Use /}).count(),0,'read-only never exposes write');
 assert.equal(requests.length,2);assert.deepEqual(errors,[]);
 console.log('PASS real Wasm: four widths, 44px controls, details/Escape, exact causal CAS, stale rejection without retry, deferred apply, transient error recovery, read-only access');
}finally{await browser.close();await new Promise(r=>server.close(r));}
