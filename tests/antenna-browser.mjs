// Real Dioxus component, isolated browser/profile, synthetic positions and sensors.
import assert from 'node:assert/strict';
import fs from 'node:fs';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const output=process.env.GS_UI_ARTIFACTS||'/tmp/seds-antenna-ui-artifacts';fs.mkdirSync(output,{recursive:true});
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
try {
 const page=await browser.newPage({viewport:{width:390,height:844}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
 let offline=false;
 const fix=(latitude,longitude,altitude,source)=>({latitude,longitude,altitude_m:altitude,altitude_reference:'mean_sea_level',age_ms:0,stale:false,source});
 await page.route('**/__antenna_fixture',route=>offline?route.fulfill({status:503,body:'offline'}):route.fulfill({json:{rocket:fix(.01,0,1000,'RF fixture'),ground_station:fix(0,0,0,'GS fixture'),rrls:null,rssi:{value:-75,unit:'dBm',source:'Provided fixture'}}}));
 await page.addInitScript(()=>{
  window.__clearedWatches=[];
  Object.defineProperty(navigator,'geolocation',{value:{watchPosition(cb){window.__gpsCallback=cb;cb({timestamp:Date.now(),coords:{latitude:0,longitude:0,altitude:50}});return 7},clearWatch(id){window.__clearedWatches.push(id)}}});
  DeviceOrientationEvent.requestPermission=async()=> 'granted';
  setInterval(()=>window.dispatchEvent(new DeviceOrientationEvent('deviceorientationabsolute',{alpha:0,beta:90,gamma:0,absolute:true})),100);
 });
 await page.goto(process.env.GS_AIM_PREVIEW_URL||'http://127.0.0.1:19091');
 await page.getByRole('heading',{name:'Find your alignment'}).waitFor();
 await page.getByLabel('GPS origin').selectOption('gs');
 await page.locator('summary').click();
 await page.getByLabel('Manual compass / inclinometer').check();
 await page.getByText('Centered',{exact:true}).first().waitFor();
 assert.match(await page.locator('.aim-map-caption').innerText(),/GS fixture/);
 await page.getByLabel('True bearing (°)').fill('90');
 await page.getByText('Left 90.0°',{exact:true}).waitFor();
 const point=()=>page.locator('.aim-map-target').getAttribute('style');
 assert.match(await point(),/translate\(6[0-9]\./,'east-facing device sees north target on its left');
 assert.equal(await page.locator('.aim-map-user').getAttribute('d'),'M 140 129 L 148 149 L 140 145 L 132 149 Z','user stays centered, forward is up');
 await page.emulateMedia({reducedMotion:'reduce'});
 assert.equal(await page.locator('.aim-turn-arrow').evaluate(n=>getComputedStyle(n).animationName),'none');
 await page.emulateMedia({reducedMotion:'no-preference'});
 await page.getByLabel('True bearing (°)').fill('0');
 await page.getByLabel('Heading trim (°)').fill('');
 await page.getByText('Waiting for position / compass',{exact:true}).waitFor();
 assert.equal(await page.locator('.horizontal .aim-rocket').count(),0,'invalid trim must not silently become zero');
 await page.getByLabel('Heading trim (°)').fill('0');
 await page.locator('summary').click();
 for(const viewport of [{width:320,height:780},{width:390,height:844},{width:844,height:390},{width:1280,height:800}]){
  await page.setViewportSize(viewport);
  await page.locator(".antenna-aim").evaluate(n=>n.scrollTo(0,0));
  await page.waitForTimeout(150);
  const overflow=await page.evaluate(()=>({width:innerWidth,scroll:Math.max(document.documentElement.scrollWidth,document.querySelector(".antenna-aim").scrollWidth),items:[...document.querySelectorAll('body *')].map(n=>({tag:n.tagName,cls:n.className,right:n.getBoundingClientRect().right,width:n.getBoundingClientRect().width})).filter(n=>n.right>innerWidth+1).slice(0,12)}));
  if(overflow.scroll>overflow.width){await page.screenshot({path:`${output}/overflow-${viewport.width}.png`,fullPage:true});console.error(overflow);}
  assert(overflow.scroll<=overflow.width,'no horizontal overflow');
  const boxes=await page.locator('.aim-axis').evaluateAll(nodes=>nodes.map(n=>{const r=n.getBoundingClientRect();return {x:r.x,right:r.right,width:r.width}}));
  const separation=await page.evaluate(()=>({axes:document.querySelector('.aim-axes').getBoundingClientRect().bottom,map:document.querySelector('.aim-map-card').getBoundingClientRect().top}));
  assert(separation.map>=separation.axes+8,'map must never overlap alignment cards');
  assert.equal(boxes.length,2);assert(boxes[0].right<=boxes[1].x+1);assert(boxes.every(b=>b.width>100));
  const height=await page.getByRole('button',{name:'Exit aiming'}).evaluate(n=>n.getBoundingClientRect().height);assert(height>=44);
  await page.screenshot({path:`${output}/antenna-${viewport.width}x${viewport.height}.png`,fullPage:true});
 }
 await page.getByLabel('Aim at').selectOption('rrls');await page.getByText('Waiting for position / compass',{exact:true}).waitFor();
 assert.equal(await page.locator('.horizontal .aim-rocket').count(),0,'no false direction for a missing RRLS');
 assert.equal(await page.locator('.aim-map-target').count(),0,'no stale target on the heading-up map');
 await page.getByLabel('Aim at').selectOption('rocket');
 await page.locator('summary').click();
 await page.getByLabel('Manual compass / inclinometer').uncheck();
 await page.getByRole('button',{name:'Enable compass & device GPS'}).click();
 await page.getByLabel('GPS origin').selectOption('device');
 await page.getByText('Centered',{exact:true}).first().waitFor();
 assert.equal(await page.locator('.vertical .aim-rocket').count(),0,'ellipsoid and sea-level heights must not be mixed');
 await page.getByLabel('Geoid separation (m)').fill('50');
 await page.locator('.vertical .aim-rocket').waitFor();
 await page.evaluate(()=>window.__gpsCallback({timestamp:Date.now()-20000,coords:{latitude:0,longitude:0,altitude:50}}));
 await page.getByText('Waiting for position / compass',{exact:true}).waitFor();
 await page.getByLabel('GPS origin').selectOption('gs');offline=true;
 await page.getByText('Waiting for position / compass',{exact:true}).waitFor();assert.equal(await page.locator('.horizontal .aim-rocket').count(),0);
 await page.getByRole('button',{name:'Exit aiming'}).click();await page.getByRole('heading',{name:'Aiming closed'}).waitFor();
 assert.deepEqual(await page.evaluate(()=>window.__clearedWatches),[7]);assert.equal(await page.evaluate(()=>window.__gs26aim===undefined),true);
 assert.deepEqual(errors,[]);console.log('PASS: antenna source/target selection, split layouts, touch targets, compass/GPS, datum mismatch, stale fixes, API loss and exit cleanup');
}finally{await browser.close();}
