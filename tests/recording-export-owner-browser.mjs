// Actual production Wasm bundle + loopback backend. No exported test hooks.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/operation-workflow.mjs','utf8');
const boundary=source.indexOf('\ntry {\n const context=');
if(boundary<0)throw Error('Operation fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
fixture=fixture.replace('const server=http.createServer',`let heldPreview=null,previewRequests=0,sessionReads=0;\nconst server=http.createServer`);
fixture=fixture.replace("  if(pathname.startsWith('/api/')){",`
  if(pathname==='/api/recordings'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify([1,2].map(n=>({id:'groundstation_recording_'+n+'.db',bytes:64,active:false}))));return;}
  if(pathname==='/api/recordings/report'){assert.equal(req.headers.authorization,'Bearer ui-token');previewRequests++;heldPreview=res;return;}
  if(pathname==='/api/auth/session')sessionReads++;
  if(pathname.startsWith('/api/')){`);
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
 async function waitFor(predicate,message){for(let i=0;i<300;i++){if(await predicate())return;await new Promise(r=>setTimeout(r,50));}throw Error(message);}
 function release(){const response=heldPreview;heldPreview=null;if(response&&!response.destroyed){response.setHeader('Content-Type','application/json');response.end(JSON.stringify({start_ms:1000,end_ms:2000,recorded_rows:2,channels:[{key:'sourceA-valve',label:'Original valve state',sender:'AB',data_type:'VALVE_STATE',unit:'state',value_kind:'valve_state',count:2,points:[[1000,0],[1999,1]],regression:{}}]}));}}
 async function open(){await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor({timeout:20000});await page.getByRole('link',{name:'Data Export',exact:true}).click();await page.getByRole('combobox',{name:'Recording to download'}).selectOption('groundstation_recording_1.db');await page.getByRole('button',{name:'Load / refresh graphs',exact:true}).waitFor();}
 await open().catch(async error=>{console.error((await page.locator('body').innerText()).slice(-7000));console.error(errors);throw error;});
 for(const mode of ['current','recordingABA','authABA']){
  console.log('CASE '+mode);
  assert.equal(await page.getByRole('combobox',{name:'Recording to download'}).inputValue(),'groundstation_recording_1.db');
  const before=previewRequests;await page.getByRole('button',{name:'Load / refresh graphs',exact:true}).click();await waitFor(()=>previewRequests>before,'actual preview request not received');
  if(mode==='recordingABA'){
   const choose=page.getByRole('combobox',{name:'Recording to download'});
   await choose.selectOption('groundstation_recording_2.db');await choose.selectOption('groundstation_recording_1.db');
  }
  if(mode==='authABA'){
   let count=sessionReads;session.permissions.send_commands=false;
   await waitFor(()=>sessionReads>count,'authority revoke refresh missing');await page.waitForTimeout(150);
   count=sessionReads;session.permissions.send_commands=true;
   await waitFor(()=>sessionReads>count,'authority restore refresh missing');await page.waitForTimeout(150);
  }
  release();await page.waitForTimeout(500);
  const graph=page.getByRole('img',{name:'Recorded channel sourceA-valve',exact:true});
  assert.equal(await graph.count(),mode==='current'?1:0,mode+' late preview publication');
  if(mode==='current'){
   await page.getByRole('button',{name:'Clear selection',exact:true}).click();assert.equal(await graph.count(),0);
   await page.getByRole('button',{name:'Select all channels',exact:true}).click();assert.equal(await graph.count(),1);
   assert.match(await page.locator('body').innerText(),/Original valve state/);
  }
 }

 // Invalidation intentionally requires a fresh selection; prove it is retryable,
 // including a real page reload rather than adopting the retired preview.
 for(const resume of ['refreshAfterRetirement','reloadAndRefresh']){
  console.log('CASE '+resume);
  if(resume==='reloadAndRefresh'){
   await page.reload();await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
   await page.getByRole('link',{name:'Data Export',exact:true}).click();
   await page.waitForTimeout(500);
  }
  await page.getByRole('button',{name:'Refresh recordings',exact:true}).click();
  await page.getByRole('combobox',{name:'Recording to download'}).selectOption('groundstation_recording_1.db');
  const before=previewRequests;await page.getByRole('button',{name:'Load / refresh graphs',exact:true}).click();
  await waitFor(()=>previewRequests>before,'fresh selected preview missing');release();
  await page.getByRole('img',{name:'Recorded channel sourceA-valve',exact:true}).waitFor();
 }
 assert.deepEqual(errors,[]);await context.close();console.log('PASS actual production Wasm recording selector/channel controls; delayed recordingABA/authABA preview retired without test hooks');
}finally{if(heldPreview&&!heldPreview.destroyed)heldPreview.end();await browser.close();await new Promise(r=>server.close(r));}`;
try {await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.message);process.exitCode=1;}
