// Mount examples/network_health_preview.rs before running this isolated browser test.
import assert from 'node:assert/strict';
import fs from 'node:fs';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true, executablePath:process.env.CHROME_BINARY});
try {
 const page=await browser.newPage();const errors=[];page.on('pageerror', e=>errors.push(e.message));
 await page.goto(process.env.GS_HEALTH_PREVIEW_URL||'http://127.0.0.1:19094');
 await page.getByRole('heading',{name:'Network health'}).waitFor();
 assert.equal(await page.getByText('Healthy',{exact:true}).count(),1);
 assert.equal(await page.getByText('Unhealthy',{exact:true}).count(),1);
 const sensor=page.locator('article').filter({hasText:'New-runtime-sensor-with-long-name'});
 // Cards keep the headline count; full counters and diagnostics are in More info.
 const details=async card=>{await card.getByRole('button',{name:'More info',exact:true}).click();const dialog=page.getByRole('dialog',{name:'Board details'});await dialog.waitFor();const text=await dialog.innerText();await dialog.getByRole('button',{name:'Close details',exact:true}).click();await dialog.waitFor({state:'detached'});return text;};
 assert.match(await sensor.innerText(),/Disconnects: 3/);
 assert.match(await details(sensor),/Disconnects\s+3\s+Reconnects\s+2/);
 const valve=page.locator('article').filter({hasText:'VB'});
 const valveInfo=await details(valve);
 assert.match(valveInfo,/Reported uptime/);
 assert.match(valveInfo,/Diagnostic report stale/);
 assert.match(valveInfo,/SD accepted \/ writer-processed \/ flushed: 12345 \/ 12000 \/ 11000/);
 assert.match(valveInfo,/CAN receive drops: 17/);
 for (const width of [320,390,844,1280]) {
  await page.setViewportSize({width,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no horizontal overflow at '+width);
  await page.screenshot({path:`${process.env.GS_UI_ARTIFACTS||'/tmp'}/network-health-${width}.png`, fullPage:true});
 }
 await page.getByRole('button',{name:'Toggle stream'}).click();
 assert.equal(await page.getByText('Healthy',{exact:true}).count(),0);
 assert.equal(await page.getByText('Unhealthy',{exact:true}).count(),2);
 assert.match(await details(sensor),/Disconnects\s+3\s+Reconnects\s+2/,'client reconnect does not alter backend counters');
 await page.getByRole('button',{name:'Toggle empty'}).click();
 await page.getByText('No boards observed in this mode.').waitFor();
 assert.equal(await page.locator('article').count(),0);
 assert.deepEqual(errors,[]);
 console.log('PASS: dynamic unknown board, unhealthy retained, counts unchanged on client disconnect, empty mode, 320/390/844/1280 layouts, no browser errors');
} finally {await browser.close();}
