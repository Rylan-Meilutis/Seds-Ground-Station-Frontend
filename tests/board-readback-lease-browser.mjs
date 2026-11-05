// Real release Wasm; backend-issued freshness lease, inert localhost APIs only.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
const status=JSON.parse(fs.readFileSync('tests/fixtures/board-recovery/status-example.json'));
status.principal.username='ui-test';
fixture=fixture.replace('let lists={',`const boardRecovery=${JSON.stringify(status)};let lease,delayMs=0,stage='Missing lease',writes=0;let lists={`);
fixture=fixture.replace("  if(pathname==='/api/checklists'||",`  if(pathname==='/api/board-recovery/status'){
 const response=JSON.parse(JSON.stringify(boardRecovery)),ab=response.boards.find(b=>b.id==='AB');
 ab.label=stage;ab.age_ms=90000;if(lease===undefined)delete ab.readback_valid_for_ms;else ab.readback_valid_for_ms=lease;
 const delay=delayMs;if(delay)await new Promise(resolve=>setTimeout(resolve,delay));res.setHeader('Content-Type','application/json');res.end(JSON.stringify(response));return;
 }
 if(pathname.startsWith('/api/board-recovery/')){writes++;throw Error('Readback fixture must never query/prepare/confirm');}
 if(pathname==='/api/checklists'||`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1280,height:1000}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',()=>{});
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));
 await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();await page.getByRole('link',{name:'Connection',exact:true}).click();
 const ab=page.locator('.gs26-board-recovery [data-board="AB"]');
 const stale=()=>ab.getByText('Actual board state: Startup (unconfirmed/stale)',{exact:true});
 await ab.getByRole('heading',{name:'Missing lease (AB)',exact:true}).waitFor();await stale().waitFor();assert(await ab.getByRole('button',{name:'Resync…'}).isDisabled(),'missing backend lease denies');
 lease=123;delayMs=250;stage='Short lease delayed';
 await ab.getByRole('heading',{name:'Short lease delayed (AB)',exact:true}).waitFor();await stale().waitFor();assert(await ab.getByRole('button',{name:'Resync…'}).isDisabled(),'123ms lease expires during250ms transport');
 lease=12345;delayMs=0;stage='Backend long lease';
 await ab.getByRole('heading',{name:'Backend long lease (AB)',exact:true}).waitFor();await ab.getByText('Actual board state: Startup (fresh)',{exact:true}).waitFor();assert(await ab.getByRole('button',{name:'Resync…'}).isEnabled(),'server lease wins over diagnostic90s age and any local2/5s constant');
 lease=0;stage='Backend expired';await ab.getByRole('heading',{name:'Backend expired (AB)',exact:true}).waitFor();await stale().waitFor();assert(await ab.getByRole('button',{name:'Resync…'}).isDisabled(),'zero server lease denies immediately');
 assert.equal(writes,0,'status polling cannot issue board commands');assert.deepEqual(errors,[]);await context.close();
 console.log('PASS server-owned readback lease: absent/zero deny,123ms includes transport,arbitrary12345ms server window independent of diagnostic age,no command writes');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}`;
if(process.env.GS_LEASE_FIXTURE_DEBUG)fs.writeFileSync(process.env.GS_LEASE_FIXTURE_DEBUG,fixture);
try{if(!process.env.GS_LEASE_PREPARE_ONLY)await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
