// Actual Wasm, shared synthetic station and authenticated clients. No hardware requests.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};
const source=fs.readFileSync('tests/dashboard-live-graphs.mjs','utf8'), boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Live browser fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE||'playwright'",JSON.stringify(pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href));
fixture=fixture.replace('http.createServer((req,res)=>','http.createServer(async(req,res)=>');
fixture=fixture.replace('const server=http.createServer',`const emptyKey={backend_instance:'1'.repeat(32),reset_epoch:'2'.repeat(16),run_id:'3'.repeat(32)};
let boardQueryAt=Date.now()-10000;let boardDisplay={VB:{connected:true,known:true,boot:'0000000000000002',epoch:3,outputs:0}},active=[],terminal=[],notices=[],cancelBodies=[],genericCommands=0,resultGets=0,scope={...emptyKey},fullResult=null;
let holdStatus=false,heldStatus=[],statusFailures=0;
const reader={...session,username:'read-only',roles:[],permissions:{...session.permissions,view_data:true,send_commands:false}};
const envelope=()=>({version:1,backend_instance:scope.backend_instance,reset_epoch:scope.reset_epoch,cancel_api_version:1,active,terminal,result_links:notices,reporting_error:null,board_display:Object.fromEntries(Object.entries(boardDisplay).map(([id,row])=>[id,{...row,outputs_age_ms:row.known?Date.now()-boardQueryAt:null}]))});
const reply=(res,value,status=200)=>{res.statusCode=status;res.setHeader('Content-Type','application/json');res.end(JSON.stringify(value));};
const server=http.createServer`);
fixture=fixture.replace("  if(pathname==='/test-camera')",`  if(pathname==='/api/auth/session'){reply(res,req.headers.authorization==='Bearer reader-token'?reader:session);return;}
  if(pathname==='/api/gse/status'){if(statusFailures>0&&req.headers.authorization==='Bearer ui-token'){statusFailures--;reply(res,{error:'Synthetic temporary failure'},503);return;}if(holdStatus){heldStatus.push({res,value:envelope()});return;}reply(res,{message:'Synthetic station observation',sequence_progress:envelope()});return;}
  if(pathname==='/api/gse/sequences/cancel'){let body='';for await(const part of req)body+=part;cancelBodies.push(JSON.parse(body));reply(res,{pending:true},202);return;}
  if(pathname.startsWith('/api/gse/sequences/results/')){resultGets++;reply(res,fullResult);return;}
  if(pathname==='/api/command'){genericCommands++;reply(res,{});return;}
  if(pathname==='/test-camera')`);
fixture=fixture.replace('const broadcast=msg=>{for(const socket of sockets)send(socket,msg);};',"const broadcast=msg=>{for(const socket of sockets)send(socket,msg);};const heartbeat=setInterval(()=>broadcast({ty:'NetworkTime',data:{timestamp_ms:Date.now()}}),500);");
fixture+=`try {
 const errors=[];
 const open=async(token,status)=>{
  const context=await browser.newContext({viewport:{width:1440,height:1000}});
  await context.addInitScript(({token,status,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token,session:status,remember_me:true}}]}));},{token,status,origin});
  const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));page.on('dialog',async d=>{errors.push('Unexpected browser popup: '+d.type());await d.dismiss();});await page.goto(origin);await page.getByRole('button',{name:'Customize dashboard',exact:true}).waitFor();return {context,page};
 };
 const op=await open('ui-token',session),viewer=await open('reader-token',reader);
 const tab=async(page,name)=>{await page.getByRole('button',{name:/Choose tab/}).click();await page.locator('#dashboard-tab-picker').getByRole('button',{name:new RegExp(name)}).click();};
 const tools=op.page.locator('details.gs26-utility-menu');
 if(!await tools.locator('summary').isVisible()){await op.page.locator('.gs26-header-menu-toggle').click();}
 {
  await tools.locator('summary').click();await tools.getByRole('button',{name:'Clear all graphs',exact:true}).waitFor();
  assert.equal(await tools.getByRole('button',{name:/^(Actions|Notifications|Telemetry|Dashboard)$/}).count(),0,'Tools must not duplicate sidebar/tab navigation');await tools.locator('summary').click();
 }
 await tab(op.page,'Actions');
 const sample=(key,revision=1)=>({version:1,key,revision,kind:'nitrogen_test',outcome:'running',origin_client:'remote-station',title:'Nitrogen test',message:'Watching tank pressure',current_step:'raise-0',next_step:'hold-0',steps:[{id:'raise-0',label:'Raise pressure',state:'watching',watch:[{id:'tank-pt',label:'Tank pressure',target:50,observed:42,unit:'psi',age_ms:0,valid_for_ms:2000,evidence:'measured'}]},{id:'hold-0',label:'Verify hold',state:'planned',watch:[]}],observed_age_ms:0,valid_for_ms:2000,cancel_allowed:true,details_complete:true});
 active=[sample(emptyKey)];active[0].steps[0].watch.push({id:'board-output-0',label:'Pilot board output',target:false,observed:true,unit:null,age_ms:0,valid_for_ms:2000,evidence:'board_reported'});active[0].steps.push(...Array.from({length:38},(_,i)=>({id:'extra-'+i,label:'Recorded operation '+i,state:'planned',watch:[]})));
 const modal=op.page.locator('.gs26-sequence-view[data-run-id="'+emptyKey.run_id+'"]');await modal.waitFor();
 await modal.getByText('42.00',{exact:false}).first().waitFor();await modal.getByText('Next: Verify hold',{exact:false}).waitFor();
 assert.equal(await modal.getByRole('button',{name:'Close sequence result'}).count(),0,'active run cannot be dismissed');
 await modal.getByRole('button',{name:'Minimize sequence progress'}).click();await op.page.locator('.gs26-sequence-chip').waitFor();assert.equal(await op.page.locator('.gs26-sequence-view').count(),0);await op.page.locator('.gs26-sequence-chip').click();await modal.waitFor();
 await viewer.page.locator('.gs26-sequence-chip[data-run-id="'+emptyKey.run_id+'"]').waitFor();await viewer.page.locator('.gs26-sequence-chip').click();const readModal=viewer.page.locator('.gs26-sequence-view');await readModal.waitFor();assert.equal(await readModal.getByRole('button',{name:'Cancel sequence',exact:true}).isDisabled(),true,'read-only client must not acquire cancellation authority');assert.equal(cancelBodies.length,0);
 // Background observations keep the same scroll container/focus at desktop and mobile.
 for(const width of [1280,390]){
  await op.page.setViewportSize({width,height:900});await op.page.waitForTimeout(500);
  await modal.locator('.gs26-sequence-body').evaluate(element=>{element.scrollTop=160;window.__sequenceBody=element;window.__sequenceScroll=element.scrollTop;});
  await modal.getByRole('button',{name:'Minimize sequence progress',exact:true}).focus();
  const start=Date.now();statusFailures=1;
  while(Date.now()-start<10000){
   await op.page.waitForTimeout(350);
   const sameBody=await modal.locator('.gs26-sequence-body').evaluate(element=>element===window.__sequenceBody);if(!sameBody)console.log('Sequence DOM replacement',JSON.stringify({width,elapsed:Date.now()-start,errors,oldConnected:await op.page.evaluate(()=>window.__sequenceBody?.isConnected),visible:await modal.count(),bodyText:(await modal.innerText()).slice(0,120)}));assert.equal(sameBody,true,'poll must not replace scroll body');
   assert.equal(await modal.locator('.gs26-sequence-body').evaluate(element=>Math.abs(element.scrollTop-window.__sequenceScroll)<2),true,'poll must not jump scroll');
   assert.equal(await modal.getByRole('button',{name:'Minimize sequence progress',exact:true}).evaluate(element=>document.activeElement===element),true,'poll must not steal focus');
   const bounds=await modal.boundingBox();assert.ok(bounds.x>=-1&&bounds.x+bounds.width<=width+1&&bounds.y>=-1&&bounds.y+bounds.height<=901,'modal fits viewport');
  }
 }
 active[0].observed_age_ms=30000;
 const pilot=modal.locator('.gs26-sequence-watch').filter({hasText:'Pilot board output'}),tank=modal.locator('.gs26-sequence-watch').filter({hasText:'Tank pressure'});
 await tank.locator('strong').getByText('Unknown psi',{exact:true}).waitFor();assert.equal(await pilot.locator('strong').innerText(),'On','connected board electrical state persists while sensor receipts expire');
 active[0].observed_age_ms=0;await tank.locator('strong').getByText('42.00 psi',{exact:true}).waitFor();
 boardDisplay.VB={...boardDisplay.VB,connected:false,known:false,outputs:null};await pilot.locator('strong').getByText('Unknown',{exact:true}).waitFor();
 boardDisplay.VB={connected:true,known:true,boot:'0000000000000002',epoch:3,outputs:1};active[0].observed_age_ms=0;await tank.locator('strong').getByText('42.00 psi',{exact:true}).waitFor();
 await pilot.locator('strong').getByText('On',{exact:true}).waitFor();
 boardQueryAt=Date.now();boardDisplay.VB.outputs=0;active[0].observed_age_ms=1500;await pilot.locator('strong').getByText('Off',{exact:true}).waitFor();
 active[0].observed_age_ms=30000;boardQueryAt=Date.now()-60000;boardDisplay.VB.outputs=1;await op.page.waitForTimeout(2300);assert.equal(await pilot.locator('strong').innerText(),'Off','older QUERY cannot resurrect a preexisting output after newer QUERY and producer expiry');
 active[0].observed_age_ms=0;await pilot.locator('strong').getByText('On',{exact:true}).waitFor();

 // Pending UI state is synchronous; observe the actual HTTP acceptance before
 // asserting the captured request body. It is still not terminal cancel proof.
 const cancelResponse=op.page.waitForResponse(response=>new URL(response.url()).pathname==='/api/gse/sequences/cancel'&&response.status()===202);
 await modal.getByRole('button',{name:'Cancel sequence',exact:true}).click();await cancelResponse;await modal.getByText("Cancellation requested. Waiting for the station's safe-cancel result.",{exact:true}).waitFor();
 assert.deepEqual(cancelBodies,[emptyKey],'cancel sends only the original run key');assert.equal(genericCommands,0,'no generic command fallback');assert.equal(await modal.getByRole('button',{name:'Close sequence result'}).count(),0,'HTTP202 is not terminal proof');
 const at=Date.now();fullResult={...sample(emptyKey,2),outcome:'cancelled',cancel_allowed:false,current_step:null,next_step:null,notification_id:700,notification_timestamp_ms:at,steps:active[0].steps.map((step,index)=>({...step,state:index===0?'passed':'skipped'}))};
 terminal=[{...fullResult,details_complete:false,steps:[]}];active=[];notices=[{key:emptyKey,notification_id:700,timestamp_ms:at,warning:true}];broadcast({ty:'Notifications',data:[{id:700,timestamp_ms:at,message:'Warning: Nitrogen test safely cancelled',persistent:true}]});
 assert.equal(await modal.locator('.gs26-sequence-body').evaluate(element=>element===window.__sequenceBody),true,'completion must retain dialog scroll body');
 await modal.getByText('Warning: safely cancelled',{exact:true}).waitFor();await modal.getByRole('button',{name:'Close sequence result'}).waitFor();await modal.getByText('Step readings below were recorded while watching each step. They are not current live readings.',{exact:true}).waitFor();assert.ok(resultGets>0,'compact result loads original detailed recording');
 await op.page.waitForTimeout(10000);assert.equal(await modal.isVisible(),true,'completed results stay open until explicitly closed');await modal.getByRole('button',{name:'Close sequence result'}).click();await modal.waitFor({state:'hidden'});await tab(op.page,'Notifications');await op.page.getByRole('button',{name:'View sequence results',exact:true}).click();await modal.waitFor();assert.equal(await modal.getByRole('button',{name:'Cancel sequence'}).count(),0,'historical result is never a live command');await modal.getByRole('button',{name:'Close sequence result'}).click();
 scope={backend_instance:'4'.repeat(32),reset_epoch:'5'.repeat(16),run_id:'6'.repeat(32)};active=[sample(scope)];terminal=[];notices=[];
 await op.page.locator('.gs26-sequence-chip[data-run-id="'+scope.run_id+'"]').waitFor();assert.equal(await op.page.locator('[data-run-id="'+emptyKey.run_id+'"]').count(),0,'new backend/reset scope removes old run controls');await op.page.locator('.gs26-sequence-chip[data-run-id="'+scope.run_id+'"]').click();const current=op.page.locator('.gs26-sequence-view[data-run-id="'+scope.run_id+'"]');await current.waitFor();
 active=[{...sample(scope),key:emptyKey}];await current.getByText('Progress unknown',{exact:true}).waitFor();assert.equal(await current.getByRole('button',{name:'Cancel sequence'}).isDisabled(),true,'mismatched current source cannot enable controls');assert.equal(await current.locator('.gs26-sequence-watch strong').innerText(),'Unknown psi');assert.equal(cancelBodies.length,1);await current.locator('[data-step-state=Unknown] .gs26-sequence-step-glyph').getByText('?',{exact:true}).first().waitFor();assert.equal(await current.locator('[data-step-state=Unknown] .gs26-sequence-step-glyph').first().getAttribute('aria-hidden'),'true','stale icon is redundant to explicit Unknown text');
 assert.deepEqual(errors,[]);await op.context.close();await viewer.context.close();console.log('PASS real Wasm Tools/menu, shared run modal/minimize, read-only auth, pending cancel, terminal/details/manual-close/reopen and current backend/reset source fencing');
} finally {clearInterval(heartbeat);await browser.close();for(const item of heldStatus)if(!item.res.destroyed)item.res.destroy();for(const socket of sockets)socket.destroy();await new Promise(resolve=>server.close(resolve));}`;
if(process.env.GS_SEQUENCE_PREPARE_ONLY==='1'){fs.writeFileSync(process.env.GS_SEQUENCE_FIXTURE_DEBUG||'/tmp/sequence-progress-fixture.mjs',fixture);}else try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exit(1);}
