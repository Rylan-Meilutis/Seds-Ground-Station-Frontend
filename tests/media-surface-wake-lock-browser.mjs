// Actual Wasm media surface mount/unmount, isolated backend and synthetic wake lock.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8'),boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1280,height:900}});
 await context.addInitScript(({session,origin})=>{
  localStorage.setItem('navigation-style','tabs');
  localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));
  window.wakeLocks=[];
  Object.defineProperty(navigator,'wakeLock',{configurable:true,value:{request:async()=>{const lock={released:false,listeners:[],addEventListener(name,fn){if(name==='release')this.listeners.push(fn);},async release(){this.released=true;for(const fn of this.listeners)fn();}};wakeLocks.push(lock);return lock;}}});
 },{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 await context.routeWebSocket(url=>url.pathname==='/ws',socket=>socket.onMessage(()=>{}));
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));
 await page.goto(origin+'/dashboard');
 await page.getByRole('button',{name:/Choose tab/}).click();
 await page.getByRole('button',{name:'Cameras & Recordings',exact:true}).click();
 await page.locator('#gs26-media').waitFor({state:'visible'});
 await page.waitForFunction(()=>wakeLocks.length===1&&!wakeLocks[0].released);
 // Capture ownership is normally sent by the camera iframe while sharing.
 await page.locator('#gs26-media').contentFrame().locator('body').evaluate(()=>parent.postMessage({type:'gs26-capture-active',active:true},location.origin));
 await page.waitForTimeout(50);
 await page.getByRole('button',{name:/Choose tab/}).click();
 await page.getByRole('button',{name:'Telemetry',exact:true}).click();
 await page.waitForTimeout(100);
 assert.equal(await page.evaluate(()=>wakeLocks[0].released),false,'hidden capture remains awake while dashboard stays mounted');
 await page.evaluate(()=>{history.pushState({},'', '/version');dispatchEvent(new PopStateEvent('popstate'));});
 await page.locator('#gs26-media').waitFor({state:'detached'});
 await page.waitForFunction(()=>wakeLocks[0].released,undefined,{timeout:5000}).catch(()=>{throw Error('Dashboard unmounted but media view/capture wake lock remains held');});
 assert.equal(await page.evaluate(()=>wakeLocks.length),1,'unmount releases existing lock without reacquisition');
 await page.evaluate(()=>{history.pushState({},'', '/dashboard');dispatchEvent(new PopStateEvent('popstate'));});
 await page.getByRole('button',{name:/Choose tab/}).click();
 await page.getByRole('button',{name:'Cameras & Recordings',exact:true}).click();
 await page.locator('#gs26-media').waitFor({state:'visible'});
 await page.waitForFunction(()=>wakeLocks.length===2&&!wakeLocks[1].released);
 // This surface must not release another active wake-lock owner.
 await page.evaluate(()=>window.GS26StayAwake.setActive('independent-media',true));
 await page.evaluate(()=>{history.pushState({},'', '/version');dispatchEvent(new PopStateEvent('popstate'));});
 await page.locator('#gs26-media').waitFor({state:'detached'});
 assert.equal(await page.evaluate(()=>wakeLocks[1].released),false,'independent mounted owner keeps the lock');
 await page.evaluate(()=>window.GS26StayAwake.setActive('independent-media',false));
 await page.waitForFunction(()=>wakeLocks[1].released);
 assert.deepEqual(errors,[]);console.log('PASS actual Wasm media wake lock: view and iframe ownership released on unmount, remount reacquires, independent owner retained');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
