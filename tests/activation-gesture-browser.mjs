// Offline browser contract for the exact shared shell script. Actions' real
// Dioxus dispatch is covered separately by valve-gesture-browser.mjs.
import assert from 'node:assert/strict';
import fs from 'node:fs';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const script=fs.readFileSync(new URL('../static/activation_gesture_guard.js',import.meta.url),'utf8');
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
const results=[];
try {
 const control=await browser.newPage();await control.setContent('<button style="width:250px;height:60px">Unguarded control</button>');
 await control.evaluate(()=>{window.count=0;document.querySelector('button').onclick=()=>count++;});
 await control.mouse.move(80,30);await control.mouse.down();await control.mouse.move(95,30);await control.mouse.move(80,30);await control.mouse.up();
 assert.equal(await control.evaluate(()=>window.count),1,'unguarded drag-return must reproduce activation');
 console.log(JSON.stringify({control:'unguarded drag-return dispatches once',passed:true}));await control.close();
 for(const {fallback,missingCapabilities} of [{fallback:false,missingCapabilities:false},{fallback:true,missingCapabilities:false},{fallback:true,missingCapabilities:true}]) {
  const context=await browser.newContext({viewport:{width:390,height:600},hasTouch:true,isMobile:true});
  if(fallback)await context.addInitScript(({missingCapabilities})=>{window.PointerEvent=undefined;if(missingCapabilities)Object.defineProperty(MouseEvent.prototype,'sourceCapabilities',{get:()=>undefined});},{missingCapabilities});
  const page=await context.newPage();assert.equal(await page.evaluate(()=>Boolean(window.PointerEvent)),!fallback,'actual selected PointerEvent branch');const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.setContent(`<style>#scroller{height:250px;overflow:auto}button{height:60px;width:250px}#spacer{height:800px}</style><div id="scroller"><button id="action"><span>Valve action</span></button><div role="dialog"><button id="sequence">Sequence action</button></div><input id="text"><div id="spacer"></div></div>`);
  await page.evaluate(()=>{window.gestureTrace=[];for(const type of ['pointerdown','pointermove','pointerup','pointercancel','touchstart','touchmove','touchend','touchcancel','mousedown','mousemove','mouseup','click','scroll'])window.addEventListener(type,event=>gestureTrace.push({type,detail:event.detail,pointerType:event.pointerType,primary:event.isPrimary,button:event.button,touchSource:event.sourceCapabilities?.firesTouchEvents,target:event.target.id}),{capture:true,passive:true});});
  await page.addScriptTag({content:script});await page.addScriptTag({content:script});
  await page.evaluate(()=>{window.endEvents=[];document.addEventListener('touchend',e=>endEvents.push({cancelable:e.cancelable,prevented:e.defaultPrevented,trusted:e.isTrusted}));window.activations=[];document.addEventListener('click',e=>{const b=e.target.closest('button');if(b)activations.push(b.id);});});
  const action=page.locator('#action');const cdp=await context.newCDPSession(page);
  const touch=async(type,x,y)=>cdp.send('Input.dispatchTouchEvent',{type,touchPoints:type==='touchEnd'||type==='touchCancel'?[]:[{x,y,id:1}]});
  const coords=async()=>{const b=await action.boundingBox();return{x:b.x+80,y:b.y+30};};
  const take=async(expected,label)=>{const evidence=await page.evaluate(()=>({activations,trace:gestureTrace}));assert.deepEqual(evidence.activations,expected,JSON.stringify({fallback,missingCapabilities,label,trace:evidence.trace}));await page.evaluate(()=>{activations=[];gestureTrace=[];});results.push({fallback,missingCapabilities,label});console.log(JSON.stringify({fallback,missingCapabilities,label,passed:true,trace:evidence.trace}));};
  await action.tap();await take(['action'],'normal touch exactly once after double installation');
  await action.click();await take(['action'],'normal mouse click exactly once');
  await page.locator('#text').focus();await action.click();await take(['action'],'leaving a focused form input does not cancel a normal click');
  await action.focus();await page.keyboard.press('Enter');await page.keyboard.press('Space');await take(['action','action'],'keyboard Enter and Space preserved');
  await action.evaluate(e=>e.click());await take(['action'],'assistive/programmatic detail-zero click preserved');
  let {x,y}=await coords();await page.mouse.move(x,y);await page.mouse.down();await page.mouse.move(x+15,y);await page.mouse.move(x,y);await page.mouse.up();await take([],'mouse drag returning to original target');
  await action.click();await take(['action'],'next legitimate click after drag');
  ({x,y}=await coords());await page.mouse.move(x,y);await page.mouse.down();await page.evaluate(()=>document.querySelector('#scroller').scrollTop=1);await page.mouse.up();await take([],'actual scroll offset changed during press');
  await page.evaluate(()=>document.querySelector('#scroller').scrollTop=0);await action.tap();await take(['action'],'next touch after cancelled scroll without ghost click');
  ({x,y}=await coords());await touch('touchStart',x,y);await touch('touchMove',x,y-80);await touch('touchEnd');await page.waitForTimeout(80);assert(await page.locator('#scroller').evaluate(el=>el.scrollTop)>0,'trusted touch swipe must actually scroll');await take([],'real touch swipe remains scrolling and never activates');
  await page.evaluate(()=>document.querySelector('#scroller').scrollTop=0);await action.tap();await take(['action'],'fresh tap after real swipe');
  ({x,y}=await coords());await touch('touchStart',x,y);await touch('touchCancel');await take([],'trusted touchcancel has no activation default');
  await page.mouse.move(x,y);await page.mouse.down();await page.mouse.up();await take(['action'],'fresh same-coordinate mouse immediately after touchcancel');
  await page.waitForTimeout(850);await page.mouse.down();await page.mouse.up();await take(['action'],'fresh same-coordinate mouse delayed after touchcancel');
  ({x,y}=await coords());await touch('touchStart',x,y);await page.evaluate(()=>document.querySelector('#scroller').scrollTop=1);await touch('touchEnd');await take([],'trusted touchend after actual scroll cannot activate');
  if(fallback){const ends=await page.evaluate(()=>endEvents);assert(ends.at(-1)?.trusted&&ends.at(-1)?.cancelable&&ends.at(-1)?.prevented,'legacy cancelled touchend must suppress browser compatibility generation');}
  await page.evaluate(()=>document.querySelector('#scroller').scrollTop=0);await page.mouse.move(x,y);await page.mouse.down();await page.mouse.up();await take(['action'],'fresh same-coordinate mouse after prevented touchend');
  await action.tap();await take(['action'],'fresh tap after cancellation');
  const sequence=page.locator('#sequence');const b=await sequence.boundingBox();await page.mouse.move(b.x+40,b.y+20);await page.mouse.down();await page.mouse.move(b.x+55,b.y+20);await page.mouse.move(b.x+40,b.y+20);await page.mouse.up();await take([],'modal sequence button shares drag cancellation');
  await sequence.click();await take(['sequence'],'modal sequence normal click preserved');
  await page.locator('#text').fill('typed normally');assert.equal(await page.locator('#text').inputValue(),'typed normally');
  assert.deepEqual(errors,[]);await context.close();
 }
 console.log(JSON.stringify({passed:results.length,results}));
}finally{await browser.close();}
