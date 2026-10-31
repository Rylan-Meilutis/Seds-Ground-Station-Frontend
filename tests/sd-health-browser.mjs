// Run examples/sd_health_preview.rs: deterministic UI only, no hardware commands.
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
try {
 const page=await browser.newPage(); const errors=[];
 page.on('pageerror',e=>errors.push(e.message));
 await page.goto(process.env.GS_SD_HEALTH_PREVIEW_URL||'http://127.0.0.1:19097');
 await page.locator('[data-sd-state="good"]').waitFor();
 assert.match(await page.locator('[data-sd-state]').innerText(),/Last write\s+\d+s ago\s+Last flush/);
 for(const width of [320,390,844,1280]) {
  await page.setViewportSize({width,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no overflow at '+width);
  assert((await page.getByRole('button',{name:'Retry SD'}).boundingBox()).height>=44);
  assert.equal(await page.getByRole('button',{name:'Retry SD'}).evaluate(e=>getComputedStyle(e).borderRadius),'10px');
  if(width===390) await page.screenshot({path:'/tmp/sd-health-390.png',fullPage:true});
 }
 await page.getByRole('button',{name:'Fail SD',exact:true}).click();
 await page.getByText('Card unavailable',{exact:true}).waitFor();
 await page.getByRole('button',{name:'Stall SD',exact:true}).click();
 await page.getByText('Write stalled',{exact:true}).waitFor();
 await page.getByRole('button',{name:'Retry SD',exact:true}).click();
 await page.getByText('Retry pending',{exact:true}).waitFor();
 assert(await page.getByRole('button',{name:'Retry pending…',exact:true}).isDisabled());
 assert.match(await page.locator('[data-sd-state]').innerText(),/queued; waiting for DAQ diagnostics/);
 assert.deepEqual(errors,[]);
 console.log('PASS: SD writing/failure/stall/pending states, retry disabled while pending, 320/390/844/1280 mobile/desktop widths, no browser errors');
} finally {await browser.close();}
