// Exercise the shipped graph controller with real pointer/resize events in Chromium.
// The controller is extracted verbatim from the Rust format string; no station needed.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {pathToFileURL} from 'node:url';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE?pathToFileURL(process.env.PLAYWRIGHT_MODULE).href:'playwright');
const rust=fs.readFileSync('src/telemetry_dashboard/network_topology_tab.rs','utf8');
const start=rust.indexOf('fn install_drag_handlers('),body=rust.slice(start).match(/r#"([\s\S]*?)"#,/)?.[1];
assert(body,'production graph controller found');
const script=(width,height,fit)=>{
 const values={viewport_id:'"viewport"',surface_id:'"surface"',canvas_id:'"canvas"',viewport_focus_key:'"fixture"',graph_width:width,graph_height:height,fit_margin_px:36,zoom_min:0.12,zoom_max:2.2,touch_drag_friction:1,force_fit:fit?'true':'false'};
 return body.replace(/\{(\w+)(?::\?)?\}/g,(all,key)=>key in values?values[key]:all).replaceAll('{{','{').replaceAll('}}','}');
};
const browser=await chromium.launch({headless:true,...(process.env.CHROME_BINARY?{executablePath:process.env.CHROME_BINARY}:{})});
try{
 const page=await browser.newPage({viewport:{width:1200,height:900}}),errors=[];
 page.on('pageerror',error=>errors.push(error.message));
 await page.setContent('<style>#viewport{width:800px;height:600px;overflow:auto}#surface{position:relative}#canvas{position:absolute;width:1080px;height:720px;transform-origin:top left}</style><div id="viewport"><div id="surface"><div id="canvas"></div></div></div>');
 await page.evaluate(script(1080,720,true));await page.waitForTimeout(100);
 const state=()=>page.evaluate(()=>{const s=window.__gs26NetworkGraphState;return {scale:s.scale,width:s.graphWidth,height:s.graphHeight,frameWidth:parseInt(s.surface.style.width),pad:s.padLeft,pointers:s.pointers.size,drag:!!s.drag,scroll:s.viewport.scrollLeft};});
 await page.evaluate(()=>window.__gs26NetworkGraphZoomDelta(.4));const zoomed=await state();
 await page.setViewportSize({width:1100,height:850});await page.waitForTimeout(100);
 assert.equal((await state()).scale,zoomed.scale,'window resize preserves manual zoom');
 await page.evaluate(script(2160,1440,false));
 // Existing one-time wheel listeners must use NEW geometry, not initial dimensions.
 await page.locator('#viewport').hover({position:{x:200,y:200}});await page.mouse.wheel(0,-80);await page.waitForTimeout(100);
 const expanded=await state();assert.equal(expanded.width,2160);assert.equal(expanded.frameWidth,Math.round(2160*expanded.scale)+expanded.pad*2,'wheel uses current graph width');
 await page.evaluate(()=>{const surface=document.getElementById('surface');surface.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerId:7,clientX:100,clientY:100,pointerType:'touch'}));});
 assert.equal((await state()).pointers,1);
 await page.evaluate(()=>window.dispatchEvent(new PointerEvent('pointercancel',{pointerId:7})));assert.equal((await state()).pointers,0);assert.equal((await state()).drag,false);
 const before=(await state()).scroll;
 await page.evaluate(()=>window.dispatchEvent(new PointerEvent('pointermove',{pointerId:7,clientX:600,clientY:100,pointerType:'touch'})));await page.waitForTimeout(50);assert.equal((await state()).scroll,before,'cancelled touch cannot keep dragging graph');
 await page.evaluate(()=>document.getElementById('surface').dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerId:8,button:2,pointerType:'mouse'})));assert.equal((await state()).pointers,0,'right click does not begin drag');
 await page.evaluate(()=>window.__gs26NetworkGraphState.dispose());assert.equal(await page.evaluate(()=>window.__gs26NetworkGraphState),undefined,'unmount removes controller');
 await page.evaluate(script(1080,720,true));await page.waitForTimeout(100);
 const remounted=(await state()).scale;
 await page.locator('#viewport').hover({position:{x:200,y:200}});await page.mouse.wheel(0,-80);await page.waitForTimeout(50);
 assert.ok(Math.abs((await state()).scale-remounted*Math.exp(80*.0018))<1e-9,'remount installs exactly one wheel handler');
 assert.deepEqual(errors,[]);console.log('PASS graph controller: current geometry, manual zoom resize, pointer cancellation, right click, cleanup and remount');
}finally{await browser.close();}
