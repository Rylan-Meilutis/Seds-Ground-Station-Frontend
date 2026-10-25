// Synthetic API/WS only: held simulator samples must not advance with browser time.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw new Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary);
fixture=fixture.replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture=fixture.replace('let lists={',"let heldClock={kind:'t_minus',anchor_timestamp_ms:null,duration_ms:9167};\nlet lists={");
fixture=fixture.replace("  if(pathname==='/api/checklists'||",`  if(pathname==='/api/launch_clock'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(heldClock));return;}
  if(pathname==='/api/network_time'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({timestamp_ms:Date.now()}));return;}
  if(pathname==='/api/checklists'||`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1280,height:900}});
 await context.addInitScript(({session,origin})=>{localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const sockets=new Set();await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>{sockets.add(s);s.onClose(()=>sockets.delete(s));s.onMessage(()=>{});});
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));await page.goto(origin+'/dashboard');
 const clock=page.locator('.gs26-launch-clock-value');await clock.getByText('00:09.17',{exact:true}).waitFor();
 await page.waitForTimeout(1400);assert.equal(await clock.textContent(),'00:09.17','held T-minus ignores browser wall time');
 const send=async value=>{heldClock=value;for(const s of sockets)s.send(JSON.stringify({ty:'LaunchClock',data:value}));};
 await send({kind:'t_plus',anchor_timestamp_ms:null,duration_ms:2500});await clock.getByText('00:02.50',{exact:true}).waitFor();await page.waitForTimeout(1400);assert.equal(await clock.textContent(),'00:02.50','held T-plus ignores browser wall time');
 await send({kind:'t_plus',anchor_timestamp_ms:null,duration_ms:3750});await clock.getByText('00:03.75',{exact:true}).waitFor();
 await page.reload();await clock.getByText('00:03.75',{exact:true}).waitFor();await page.waitForTimeout(600);assert.equal(await clock.textContent(),'00:03.75','held clock survives API reconnect');
 assert.deepEqual(errors,[]);await context.close();console.log('PASS real Wasm model-only clock: held T-minus/T-plus, explicit sample advancement and API reconnect; fake API/WS only');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.stack);process.exitCode=1;}
