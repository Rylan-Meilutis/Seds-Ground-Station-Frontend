// Reuse the isolated fake API fixture; this test never contacts a station.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw new Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary);
fixture=fixture.replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture=fixture.replace("let lists={",`const history={heads:[{clock:{rrls:3},value:{list_id:'setup',item_id:'pressure',checked:true,completion_source:'operator',completed_by:'field operator',completed_at_ms:123}},{clock:{gs:4},value:{list_id:'setup',item_id:'pressure',checked:false}}]};
let syncStatus={enabled:true,ready:true,durability_pending:false,definition_pending:false,error:null,pending_reason:null,conflict_count:1,qualification_binding_valid:true,conflicts:{'checklists/progress/example':history}};let resolutions=[];
let lists={`);
fixture=fixture.replace("  if(pathname==='/api/checklists'||",`  if(pathname==='/api/checklists/sync'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(syncStatus));return;}
  if(pathname==='/api/checklists/resolve'){
    let body='';for await(const data of req)body+=data;const request=JSON.parse(body);assert.equal(req.headers.authorization,'Bearer ui-token');assert.deepEqual(request.expected,history);assert(history.heads.some(h=>{try{assert.deepEqual(h.value,request.value);return true;}catch{return false;}}));resolutions.push(request);lists.checklists[0].items[0]={...lists.checklists[0].items[0],...request.value};lists.revision++;syncStatus={...syncStatus,conflict_count:0,conflicts:{}};res.setHeader('Content-Type','application/json');res.end(JSON.stringify(lists));return;
  }
  if(pathname==='/api/checklists'||`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:390,height:844}});
 await context.addInitScript(({session,origin})=>{localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(()=>{}));
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));await page.goto(origin+'/dashboard');await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Checklists',exact:true}).click();
 await page.getByRole('heading',{name:'Station sync',exact:true}).waitFor();await page.getByText('Review needed',{exact:true}).waitFor();assert(await page.getByRole('checkbox',{name:/^Pressure checked/}).isDisabled());assert.equal(await page.getByRole('button',{name:/Use version/}).count(),2);
 await page.getByRole('button',{name:'Use version 1',exact:false}).click();await page.getByText('Reviewed version saved',{exact:true}).waitFor();assert.equal(resolutions.length,1);assert.equal(resolutions[0].value.completed_by,'field operator');assert(await page.getByRole('checkbox',{name:/^Pressure checked/}).isChecked());assert.equal(await page.getByRole('button',{name:/Use version/}).count(),0);
 syncStatus={...syncStatus,ready:false};await page.reload();await page.getByText('Preparing durable checklists. Checks will be available shortly.',{exact:true}).waitFor();assert(await page.getByRole('checkbox',{name:/^Pressure checked/}).isDisabled());
 syncStatus={...syncStatus,ready:true,durability_pending:true};await page.reload();await page.getByText('Saving checklist state. Sequence starts remain locked until storage is ready.',{exact:true}).waitFor();assert(await page.getByRole('checkbox',{name:/^Pressure checked/}).isDisabled());
 syncStatus={...syncStatus,ready:true,durability_pending:false,definition_pending:true};await page.reload();await page.getByText('Template updates are waiting for a complete reviewed version, Idle and cleared qualification. Progress stays editable.',{exact:true}).waitFor();assert(await page.getByRole('checkbox',{name:/^Pressure checked/}).isEnabled());
 const longHistory={heads:history.heads.map(h=>({...h,value:{...h.value,list_id:'a'.repeat(80),item_id:'b'.repeat(80)}}))};syncStatus={...syncStatus,definition_pending:false,conflict_count:1,conflicts:{'checklists/progress/long':longHistory}};await page.reload();await page.getByText('Review needed',{exact:true}).waitFor();
 for(const width of[320,390,844,1440]){await page.setViewportSize({width,height:900});await page.waitForTimeout(100);assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'conflict panel fits '+width);assert(await page.locator('.gs26-checklist-sync').evaluate(e=>e.scrollWidth<=e.clientWidth+1),'conflict content fits '+width);assert(await page.locator('.gs26-checklist-versions button').evaluateAll(nodes=>nodes.every(e=>e.scrollHeight<=e.clientHeight+1)),'receipt content fits touch targets '+width);if(width===320)await page.locator('.gs26-checklist-sync').screenshot({path:'/tmp/gs-paired-conflicts-320.png'});}
 session.roles=[];await page.evaluate(()=>{const saved=JSON.parse(localStorage.getItem('auth_session_v1'));for(const entry of saved.entries)entry.session.session.roles=[];localStorage.setItem('auth_session_v1',JSON.stringify(saved));});await page.reload();await page.getByText('An administrator must review conflicting updates before those items can count as complete.',{exact:true}).waitFor();assert.equal(await page.getByRole('button',{name:/Use version/}).count(),0);assert.equal(resolutions.length,1);
 assert.deepEqual(errors,[]);await context.close();console.log('PASS paired checklist real Wasm: exact reviewed causal CAS, receipt preservation, read-only conflicts, preparing/storage gate, progress during staged definitions and four responsive widths');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try { await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64')); } catch(error) { console.error(error.name+': '+error.message); process.exitCode=1; }
