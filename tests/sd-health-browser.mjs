// Run examples/sd_health_preview.rs: deterministic UI only, no hardware commands.
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
try {
 const page=await browser.newPage(); const errors=[];
 page.on('pageerror',e=>errors.push(e.message));
 await page.goto(process.env.GS_SD_HEALTH_PREVIEW_URL||'http://127.0.0.1:19097');
 // SD health lives in the DAQ card's More info; the preview's controls sit behind
 // that dialog, so each check opens it, reads it and closes it again.
 const daq=page.locator('article').filter({hasText:'DAQ'});
 const sd=async(check)=>{await daq.getByRole('button',{name:'More info',exact:true}).click();const dialog=page.getByRole('dialog',{name:'Board details'});await dialog.waitFor();try{return await check(dialog);}finally{await dialog.getByRole('button',{name:'Close details',exact:true}).click();await dialog.waitFor({state:'detached'});}};
 const shows=text=>sd(async dialog=>{await dialog.getByText(text,{exact:true}).waitFor();});
 await sd(async dialog=>{await dialog.locator('[data-sd-state="good"]').waitFor();assert.match(await dialog.locator('[data-sd-state]').innerText(),/Last write\s+\d+s ago\s+Last flush/);
  for(const width of [320,390,844,1280]) {
   await page.setViewportSize({width,height:844});
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no overflow at '+width);
   assert((await dialog.getByRole('button',{name:'Retry SD'}).boundingBox()).height>=44);
   if(width===390) await page.screenshot({path:'/tmp/sd-health-390.png',fullPage:true});
  }
 });
 await page.getByRole('button',{name:'Fail SD',exact:true}).click();
 await shows('Card unavailable');
 await page.getByRole('button',{name:'Stall SD',exact:true}).click();
 await shows('Write stalled');
 await sd(async dialog=>{await dialog.getByRole('button',{name:'Retry SD',exact:true}).click();await dialog.getByText('Retry pending',{exact:true}).waitFor();
  assert(await dialog.getByRole('button',{name:'Retry pending…',exact:true}).isDisabled());
  assert.match(await dialog.locator('[data-sd-state]').innerText(),/queued; waiting for DAQ diagnostics/);
  await dialog.getByText('Launch files open',{exact:true}).waitFor();});
 await page.getByRole('button',{name:'Different launch',exact:true}).click();
 await shows('Awaiting DAQ acceptance');
 await page.getByRole('button',{name:'Stale launch',exact:true}).click();
 await shows('Confirmation stale');
 await page.getByRole('button',{name:'Close launch window',exact:true}).click();
 await shows('Launch window closed');
 await page.getByRole('button',{name:'Accept launch',exact:true}).click();
 await shows('Launch files open');
 assert.deepEqual(errors,[]);
 console.log('PASS: exact launch acceptance/mismatch/stale/cutoff states, SD writing/failure/stall/pending states, retry disabled while pending, 320/390/844/1280 mobile/desktop widths, no browser errors');
} finally {await browser.close();}
