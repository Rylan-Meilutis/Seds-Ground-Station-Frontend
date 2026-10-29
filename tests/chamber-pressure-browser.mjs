// Build actual Wasm with dx, then run with Playwright. No hardware endpoints.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/operation-workflow.mjs','utf8');
const boundary=source.indexOf('\ntry {\n const context=');
if(boundary<0)throw Error('Operation fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
fixture=fixture.replace("const dashboardLayout=example('layout.full');",`const dashboardLayout=JSON.parse(fs.readFileSync(process.env.GS_CHAMBER_LAYOUT,'utf8'));let chamberRows=[];`);
const extras=fixture.indexOf("for(const id of ['LOADCELL','DAQ'])");
const serverStart=fixture.indexOf('\n\nconst server=',extras);
if(extras>=0&&serverStart>extras)fixture=fixture.slice(0,extras)+fixture.slice(serverStart);
fixture=fixture.replace("  if(pathname.startsWith('/api/')){",`  if(pathname==='/api/recent'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(chamberRows));return;}\n  if(pathname.startsWith('/api/')){`);
fixture+=`
try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{
  if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};
  localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));
 },{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 await context.routeWebSocket(url=>url.pathname==='/ws',()=>{});
 const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
 async function chamberTab(){await page.getByRole('navigation',{name:'Mission navigation'}).waitFor({timeout:15000}).catch(async e=>{console.error((await page.locator('body').innerText()).slice(0,3000));console.error(errors);throw e});await page.getByRole('link',{name:'Data',exact:true}).click();await page.getByRole('button',{name:'Combustion chamber pressure',exact:true}).click();}
 await page.goto(origin+'/dashboard');await chamberTab();
 assert(!/0\\.00 psi/.test(await page.locator('body').innerText()),'unassigned input must not display invented zero PSI');
 const stamp=Date.now();chamberRows=[{timestamp_ms:stamp,data_type:'COMBUSTION_CHAMBER_PRESSURE',sender_id:'DAQ',values:[1000]},{timestamp_ms:stamp,data_type:'COMBUSTION_CHAMBER_VOLTAGE',sender_id:'DAQ',values:[5.0]}];
 await page.reload();await chamberTab();
 await page.getByText('1000.00 psig',{exact:true}).waitFor();
 await page.screenshot({path:process.env.GS_CHAMBER_SCREENSHOT||'/tmp/chamber-pressure-browser.png',fullPage:true});
 assert.deepEqual(errors,[]);await context.close();console.log('PASS actual Wasm: configured chamber tab, no fabricated unassigned zero, persisted/reloaded pressure with psig units');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try { await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64')); } catch(e) { console.error(e.name+': '+e.message);process.exitCode=1; }
