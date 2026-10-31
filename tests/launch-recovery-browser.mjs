// Private real-Wasm UI fixture. Every API/WS is synthetic; no hardware or output.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8'),boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture=fixture.replace('let lists={',`let recovery={recovery_marker_supported:true,clock_reset_request_supported:true,clock:{session:'a'.repeat(32),t0_utc_ms:Date.now()-90000},synchronized:true,persistence_error:null,station:'primary',events:[],reset:{pending:false,error:null}};
let markPosts=0,markRequests=[],exports=[];
let lists={`);
fixture=fixture.replace("  if(pathname==='/api/checklists'||",`  if(pathname==='/api/recovery/reached'){
 assert.equal(req.headers.authorization,'Bearer ui-token');let body='';for await(const data of req)body+=data;const mark=JSON.parse(body);markRequests.push(mark);assert.deepEqual(Object.keys(mark),['launch_session']);assert.equal(mark.launch_session,recovery.clock.session);markPosts++;
 const event={id:'event1',launch_session:recovery.clock.session,station:recovery.station,t_relative_ms:90000,reached_utc_ms:Date.now(),actor:'ui-test'};if(!recovery.events.length)recovery.events.push(event);res.setHeader('Content-Type','application/json');res.end(JSON.stringify(recovery.events[0]));return;
 }
 if(pathname==='/api/recovery'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(recovery));return;}
 if(pathname==='/api/recovery/export.json'){assert.equal(req.headers.authorization,'Bearer ui-token');exports.push(pathname);res.setHeader('Content-Type','application/json');res.end(JSON.stringify({events:recovery.events}));return;}
 if(pathname==='/api/launch_clock'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(recovery.clock.t0_utc_ms?{kind:'t_plus',anchor_timestamp_ms:recovery.clock.t0_utc_ms,duration_ms:null}:{kind:'idle',anchor_timestamp_ms:null,duration_ms:null}));return;}
 if(pathname==='/api/checklists'||`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1280,height:900},acceptDownloads:true});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const sockets=new Set(),commands=[];await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>{sockets.add(s);s.onClose(()=>sockets.delete(s));s.onMessage(msg=>commands.push(JSON.parse(msg)));});
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));page.on('dialog',d=>d.accept());
 await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();assert.equal(await page.getByRole('button',{name:'Mark recovered',exact:true}).count(),0,'recovery commands are not global');await page.getByRole('link',{name:'Actions',exact:true}).click();const mark=page.getByRole('button',{name:'Mark recovered',exact:true});await mark.waitFor();assert(await mark.isEnabled());await mark.click();
 await page.getByRole('button',{name:'Rocket reached',exact:true}).waitFor();assert.equal(markPosts,1);assert(await page.getByRole('button',{name:'Rocket reached',exact:true}).isDisabled());await page.getByText(/Rocket reached T\\+00:01:30/).waitFor();
 await page.reload();await page.getByRole('button',{name:'Rocket reached',exact:true}).waitFor();assert.equal(markPosts,1,'UI reload does not repeat recovery write');
 assert.equal(await page.getByRole('button',{name:/^Export /}).count(),0);await page.getByRole('link',{name:'Data Export',exact:true}).click();const download=page.waitForEvent('download');await page.getByRole('button',{name:'Export recovery json',exact:true}).click();await download;assert.deepEqual(exports,['/api/recovery/export.json']);
 await page.getByRole('link',{name:'Actions',exact:true}).click();
 for(const width of[320,390,844,1280]){await page.setViewportSize({width,height:900});assert(await page.locator('.gs26-recovery-marker').evaluate(e=>e.getBoundingClientRect().width<=innerWidth),'recovery row fits '+width);}
 const previousSession=recovery.clock.session;recovery.clock={session:'b'.repeat(32),t0_utc_ms:null};await page.getByRole('button',{name:'Mark recovered',exact:true}).waitFor();assert(await page.getByRole('button',{name:'Mark recovered',exact:true}).isDisabled());assert.equal(recovery.events[0].launch_session,previousSession,'reset retains old audit');
 recovery={};await page.locator('.gs26-recovery-marker').waitFor({state:'detached'});assert.equal(markPosts,1,'older backend never enables a recovery mutation');
 assert.deepEqual(commands,[],'recovery marking/export sends no telemetry command');assert.deepEqual(errors,[]);await context.close();console.log('PASS real Wasm recovery capability, authenticated metadata POST, same-session idempotent display/reload, JSON export, explicit reset history, legacy backend and mobile widths');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
