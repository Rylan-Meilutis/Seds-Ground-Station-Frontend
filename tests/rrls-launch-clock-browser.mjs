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
fixture=fixture.replace("authenticated:true,anonymous:false","authenticated:true,anonymous:false,station_role:'rrls'");
fixture=fixture.replace("let actionPolicy=example('action-policy');","let actionPolicy=example('action-policy');actionPolicy.controls.push({cmd:'ResetTClock',enabled:true,blink:'none',actuated:null});");
fixture=fixture.replace("  if(pathname==='/api/recovery/reached'){",`  if(pathname.startsWith('/tiles/')){res.setHeader('Content-Type','image/png');res.end(fs.readFileSync('assets/icon.png'));return;}
 if(pathname==='/flightstate'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({state:'Recovery'}));return;}
 if(pathname==='/api/recovery/reached'){`);
// Deliberately no valid /api/network_time response: retain TPlus without any
// synchronized time sample. The previous badge effect self-wrote None forever.
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1280,height:900}});
 await context.addInitScript(({session,origin})=>localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]})),{session,origin});
 const sockets=new Set(),commands=[];await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>{sockets.add(s);s.onClose(()=>sockets.delete(s));s.onMessage(msg=>commands.push(JSON.parse(msg)));});
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));page.on('dialog',d=>d.accept());await page.goto(origin+'/dashboard');await page.getByRole('link',{name:'Actions',exact:true}).click();
 const reset=page.getByRole('button',{name:'Request T clock reset',exact:true});await reset.waitFor();await page.waitForTimeout(4000);assert((await page.locator('body').innerText({timeout:3000})).includes('Mark recovered'),'retained TPlus without network time cannot freeze the UI');
 const retained={...recovery.clock};await reset.click();await page.waitForTimeout(500);assert.deepEqual(commands,[{cmd:'ResetTClock'}]);
 recovery.reset.pending=true;recovery.synchronized=false;await page.getByText('Primary T clock reset pending · clock retained',{exact:true}).waitFor({timeout:5000});assert.deepEqual(recovery.clock,retained);assert(await reset.isDisabled());
 recovery.clock={session:'b'.repeat(32),t0_utc_ms:null};recovery.reset.pending=false;recovery.synchronized=true;for(const socket of sockets)socket.send(JSON.stringify({ty:'LaunchClock',data:{kind:'idle',anchor_timestamp_ms:null,duration_ms:null}}));
 await page.getByText('Primary T clock reset pending · clock retained',{exact:true}).waitFor({state:'detached',timeout:5000});assert(await page.getByRole('button',{name:'Mark recovered',exact:true}).isDisabled());
 recovery.clock_reset_request_supported=false;await reset.waitFor({state:'detached',timeout:5000});assert.deepEqual(commands,[{cmd:'ResetTClock'}]);assert.deepEqual(errors,[]);await context.close();console.log('PASS real Wasm RRLS retained TPlus without network time, responsive UI, ResetTClock metadata request, retained pending clock, authoritative Idle and legacy capability gate');
}finally{await browser.close();server.closeAllConnections();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.message);process.exitCode=1;}
