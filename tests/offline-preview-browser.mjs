// Mount examples/offline_preview.rs. No backend, credentials or live hardware.
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
try {
 const context=await browser.newContext();
 // Protect unrelated native/browser settings from training edits.
 const page=await context.newPage();const errors=[],requests=[],ws=[];
 page.on('pageerror',e=>errors.push(e.message));
 page.on('request',r=>{if(['xhr','fetch','media'].includes(r.resourceType())&&!r.url().endsWith('/offline_preview_bg.wasm'))requests.push(r.url());});
 page.on('websocket',s=>ws.push(s.url()));
 await page.goto(process.env.GS_PREVIEW_URL||'http://127.0.0.1:19096');
 await page.getByText('Example data · Controls disabled',{exact:true}).waitFor();
 const stored=await page.evaluate(()=>JSON.stringify({...localStorage}));
 const nav=page.getByRole('navigation',{name:'Mission navigation'});
 const open=async label=>{await nav.getByRole('link',{name:label,exact:true}).click();await page.getByRole('heading',{name:label,exact:true}).waitFor();};
 await open('Dashboard');await page.getByText('12.40 kg',{exact:true}).waitFor();
 await open('Mission');assert.equal(await page.getByRole('button',{name:'Fire',exact:true}).isDisabled(),true);
 await page.getByLabel('Example phase').selectOption('Armed');
 assert.equal(await page.locator('.gs26-preview-steps li[data-active=true]').innerText(),'Armed');
 await open('Data');await page.getByRole('tab',{name:'Tank pressure',exact:true}).click();
 assert.equal(await page.getByRole('tab',{name:'Tank pressure',exact:true}).getAttribute('aria-selected'),'true');
 await page.getByRole('img',{name:'Synthetic tank pressure history'}).waitFor();
 await open('Map');await page.getByRole('img',{name:'Illustrative recovery map without live location'}).waitFor();
 await open('Checklists');const checks=page.locator('.gs26-preview-main input[type=checkbox]');
 await checks.first().check();assert.equal(await checks.first().isChecked(),true);
 await page.getByRole('button',{name:'Reset practice checks'}).click();assert.equal(await checks.first().isChecked(),false);
 await open('Voice Chat');assert.equal(await page.getByRole('button',{name:'Join channel'}).isDisabled(),true);
 await open('Stream Control');assert.equal(await page.getByRole('button',{name:'Open stream controls'}).isDisabled(),true);
 await page.getByRole('link',{name:'Settings',exact:true}).click();await page.getByLabel('Show a test-fire example').check();
 await open('Dashboard');await page.getByText('1000 kg loadcell',{exact:true}).waitFor();
 assert.equal(await page.getByText('Rocket GPS',{exact:true}).count(),0);
 await page.getByRole('link',{name:'Settings',exact:true}).click();await page.getByLabel('Use sidebar navigation').uncheck();
 await page.getByLabel('Page',{exact:true}).selectOption('state');await page.getByRole('heading',{name:'Dashboard',exact:true}).waitFor();
 for(const width of [320,390,844,1280]) {
  await page.setViewportSize({width,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no page overflow '+width);
  assert(await page.locator('.gs26-preview-main').evaluate(e=>e.scrollWidth<=e.clientWidth),'no content overflow '+width);
  if(process.env.GS_UI_ARTIFACTS)await page.screenshot({path:`${process.env.GS_UI_ARTIFACTS}/offline-preview-tabs-${width}.png`});
 }
 await page.getByLabel('Page',{exact:true}).selectOption('settings');await page.getByLabel('Use sidebar navigation').check();
 for(const width of [320,390,844,1280]) {
  await page.setViewportSize({width,height:844});
  const rail=await page.locator('.gs26-navigation-rail').boundingBox(),main=await page.locator('.gs26-preview-main').boundingBox();
  assert(main.x>=rail.x+rail.width,'sidebar occupies its own column '+width);
  assert(await page.locator('.gs26-preview-main').evaluate(e=>e.scrollWidth<=e.clientWidth),'expanded sidebar content fits '+width);
  if(process.env.GS_UI_ARTIFACTS)await page.screenshot({path:`${process.env.GS_UI_ARTIFACTS}/offline-preview-sidebar-${width}.png`});
 }
 await page.getByRole('button',{name:'Collapse sidebar'}).click();
 await page.getByRole('button',{name:'Expand sidebar'}).waitFor();
 await open('Mission');
 for(const button of await page.locator('.gs26-preview-actions button').all())assert(await button.isDisabled(),'operational output always disabled');
 assert.equal(await page.evaluate(()=>JSON.stringify({...localStorage})),stored,'preview cannot change persisted account/station preferences');
 assert.deepEqual(requests,[],'no HTTP API/media requests');assert.deepEqual(ws,[],'no WebSocket connections');assert.deepEqual(errors,[]);
 await open('Checklists');await page.locator('.gs26-preview-main input[type=checkbox]').first().check();
 await page.getByRole('button',{name:'Exit preview'}).click();await page.getByRole('heading',{name:'Connection setup'}).waitFor();
 await page.getByRole('button',{name:'Explore offline'}).click();await page.getByRole('heading',{name:'Dashboard',exact:true}).waitFor();
 await page.getByText('Rocket GPS',{exact:true}).waitFor();
 await open('Checklists');assert.equal(await page.locator('.gs26-preview-main input[type=checkbox]').first().isChecked(),false,'training progress discarded after exit/remount');
 assert.equal(await page.evaluate(()=>JSON.stringify({...localStorage})),stored);
 assert.deepEqual(requests,[]);assert.deepEqual(ws,[]);assert.deepEqual(errors,[]);
 console.log('PASS: offline pages, mode-sensitive examples, local checklist, phase/sensor navigation, sidebar/tabs, four viewport widths, disabled outputs, zero network and unchanged storage');
} finally {await browser.close();}
