// Exercise the actual Rust-owned renderer bootstrap in a real browser.
import assert from 'node:assert/strict';
import fs from 'node:fs';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
let js=fs.readFileSync('src/telemetry_dashboard/data_chart.rs','utf8').split('fn chart_canvas_renderer_bootstrap()')[1].split('r##"')[1].split('"##,')[0];
for(const [key,value]of Object.entries({chart_grid_left:96,chart_grid_right_pad:20,chart_grid_top:20,chart_grid_bottom_pad:52}))js=js.replaceAll('{'+key+'}',String(value));
js=js.replaceAll('{{','{').replaceAll('}}','}');
const browser=await chromium.launch({headless:true});
try {
for(const viewport of [{width:1200,height:800},{width:390,height:844}]){
 const page=await browser.newPage({viewport});const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.setContent('<canvas id="chart" style="width:300px;height:200px"></canvas>');await page.evaluate(js);
 await page.evaluate(()=>{window.payload={signature:1,view_w:400,view_h:200,colors:['#38bdf8'],chunks:[{id:1,signature:1,x:0,width:400,right:400,paths:['M 100 100 L 300 40'],gap_paths:[]}],combined_paths:['M 100 100 L 300 40']};__gs26DrawChartCanvas('chart',payload);});await page.waitForTimeout(80);
 assert.equal(await page.locator('canvas').evaluate(e=>e.width),300);
 await page.locator('canvas').evaluate(e=>e.style.width='350px');await page.waitForTimeout(80);
 assert.equal(await page.locator('canvas').evaluate(e=>e.width),350,'actual resize redraws without telemetry/timer');
 const gridBefore=await page.evaluate(()=>{window.gridBefore=__gs26ChartCanvasCache.get('chart').gridBuffer;return true;});
 const scheduled=await page.evaluate(()=>{window.realRAF=requestAnimationFrame;window.frames=[];window.requestAnimationFrame=f=>{frames.push(f);return frames.length};for(let i=2;i<=500;i++)__gs26DrawChartCanvas('chart',{...payload,signature:i});return frames.length;});
 assert.equal(scheduled,1,'one pending draw per canvas, latest payload wins');
 await page.evaluate(()=>{requestAnimationFrame=realRAF;for(const f of frames.splice(0))f();});
 assert.match(await page.locator('canvas').evaluate(e=>e.__gs26RenderSignature),/^500\|/);
 assert.equal(await page.evaluate(()=>__gs26ChartCanvasCache.get('chart').gridBuffer===gridBefore),true,'data updates reuse grid backing canvas');
 await page.locator('canvas').evaluate(e=>e.style.display='none');await page.waitForTimeout(80);
 await page.evaluate(()=>__gs26DrawChartCanvas('chart',{...payload,signature:501}));await page.waitForTimeout(40);
 await page.locator('canvas').evaluate(e=>e.style.display='block');await page.waitForTimeout(100);
 assert.match(await page.locator('canvas').evaluate(e=>e.__gs26RenderSignature),/^501\|/,'hidden chart resumes latest real payload');
 await page.evaluate(()=>{__gs26DisposeChartCanvas('chart');document.getElementById('chart').remove();});await page.waitForTimeout(50);
 assert.equal(await page.evaluate(()=>__gs26ChartCanvasCache.size),0,'unmount releases paths, payload and observer target');
 assert.deepEqual(errors,[]);await page.close();
}
console.log('PASS: resize, latest-frame coalescing, hidden recovery and disposal on desktop/mobile');
}finally{await browser.close()}
