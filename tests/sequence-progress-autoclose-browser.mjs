// Actual Wasm, synthetic loopback observations, and genuine notification handlers.
// No command clicks or physical/backend integration claim.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href));
fixture=fixture.replace('let gseStatus=null,',`const commandRequests=[];const fullResults=new Map();let gseStatus=null,`);
fixture=fixture.replace("  const pathname=new URL(req.url,'http://localhost').pathname;",`  const pathname=new URL(req.url,'http://localhost').pathname;
  if(req.method!=='GET'&&req.method!=='HEAD'&&!pathname.startsWith('/api/auth/')&&pathname!=='/api/i18n/translate')commandRequests.push({method:req.method,path:pathname});
  if(pathname.startsWith('/api/gse/sequences/results/')){res.setHeader('Content-Type','application/json');const value=fullResults.get(pathname.split('/').at(-1));res.statusCode=value?200:404;res.end(JSON.stringify(value||{error:'missing fixture result'}));return;}`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1920,height:1080}}),blocked=[],errors=[],sockets=new Set();
 await context.route('**/*',async route=>{const u=new URL(route.request().url());if(['http:','https:'].includes(u.protocol)&&u.origin!==origin){blocked.push(u.origin);return route.abort();}return route.continue();});
 await context.routeWebSocket(u=>u.pathname==='/_dioxus',()=>{});
 await context.routeWebSocket(u=>u.pathname==='/ws',socket=>{sockets.add(socket);socket.onClose(()=>sockets.delete(socket));socket.onMessage(()=>commandRequests.push({method:'WS',path:'/ws'}));});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(x,b)=>{try{return new URL(x,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));await page.goto(origin+'/dashboard');
 const rail=page.getByRole('navigation',{name:'Mission navigation'});await rail.waitFor();await rail.getByRole('link',{name:'Actions',exact:true}).click();
 const rows=[];
 for(const [index,outcome]of ['passed','cancelled'].entries()){
  const key={backend_instance:'1'.repeat(32),reset_epoch:'2'.repeat(16),run_id:String(index+3).repeat(32)};
  const snapshot={version:1,key,revision:1,kind:'nitrogen_test',outcome:'running',origin_client:'remote-station',title:'Auto-close '+outcome,message:'Watching actual fixture progress',current_step:'observe',next_step:null,steps:[{id:'observe',label:'Recorded observation',state:'watching',watch:[]}],observed_age_ms:0,valid_for_ms:2000,cancel_allowed:true,details_complete:true};
  const envelope={version:1,backend_instance:key.backend_instance,reset_epoch:key.reset_epoch,cancel_api_version:1,active:[snapshot],terminal:[],result_links:[],reporting_error:null,board_display:{}};
  gseStatus={message:'Synthetic sequence',sequence_progress:envelope};
  const modal=page.locator('.gs26-sequence-view[data-run-id="'+key.run_id+'"]'),chip=page.locator('.gs26-sequence-chip[data-run-id="'+key.run_id+'"]');
  await modal.waitFor();assert.equal(await modal.getByRole('button',{name:'Close sequence result',exact:true}).count(),0);
  await modal.getByRole('button',{name:'Minimize sequence progress',exact:true}).click();await chip.waitFor();await chip.click();await modal.waitFor();
  // The actual backend terminal snapshot, not a user click or HTTP acceptance,
  // closes progress. Exercise both success and cancellation.
  const timestamp=Date.now(),id=700+index,noticeMessage=(outcome==='passed'?'Info: ':'Warning: ')+'Auto-close '+outcome;
  const finished={...snapshot,revision:2,outcome,cancel_allowed:false,current_step:null,steps:[{...snapshot.steps[0],state:outcome==='passed'?'passed':'skipped'}],notification_id:id,notification_timestamp_ms:timestamp};
  fullResults.set(key.run_id,finished);envelope.active=[];envelope.terminal=[{...finished,steps:[],details_complete:false}];envelope.result_links=[{key,notification_id:id,timestamp_ms:timestamp,warning:outcome!=='passed'}];
  const terminalResponse=page.waitForResponse(async response=>{if(new URL(response.url()).pathname!=='/api/gse/status'||response.status()!==200)return false;try{return(await response.json()).sequence_progress.terminal.some(x=>x.key.run_id===key.run_id&&x.outcome===outcome);}catch{return false;}});
  for(const socket of sockets)socket.send(JSON.stringify({ty:'Notifications',data:[{id,timestamp_ms:timestamp,message:noticeMessage,persistent:true}]}));
  await terminalResponse;await modal.waitFor({state:'hidden'});assert.equal(await chip.count(),0,'completion also removes minimized progress');
  await rail.getByRole('link',{name:'Notifications',exact:true}).click();
  const notice=page.locator('.gs26-message-history').getByText(noticeMessage,{exact:true}).locator('..');
  assert.equal(await notice.getByRole('button',{name:'View sequence results',exact:true}).count(),1,'one result action for the original current notification');
  const resultPath='/api/gse/sequences/results/'+key.backend_instance+'/'+key.reset_epoch+'/'+key.run_id;
  const originalResult=page.waitForResponse(r=>new URL(r.url()).pathname===resultPath&&r.request().method()==='GET'&&r.status()===200);
  const noticeText=await notice.innerText();await notice.getByRole('button',{name:'View sequence results',exact:true}).click();
  const resultReply=await originalResult;assert.deepEqual((await resultReply.json()).key,key,'notification requests original immutable result key');
  if(process.env.GS_SEQUENCE_EVIDENCE)fs.writeFileSync(path.join(process.env.GS_SEQUENCE_EVIDENCE,'notification-'+outcome+'.json'),JSON.stringify({expected_key:key,message:noticeMessage,actual_row:noticeText,result_path:resultPath,http_status:resultReply.status()})+'\\n');
  await modal.waitFor();
  await modal.getByText('Step readings below were recorded while watching each step. They are not current live readings.',{exact:true}).waitFor();
  assert.equal(await modal.getByRole('button',{name:'Cancel sequence',exact:true}).count(),0,'reopened immutable history cannot command');
  for(let poll=0;poll<3;poll++)await page.waitForResponse(r=>new URL(r.url()).pathname==='/api/gse/status'&&r.status()===200);
  assert.equal(await modal.isVisible(),true,'explicit notification reopen survives repeated terminal polls');
  await modal.getByRole('button',{name:'Close sequence result',exact:true}).click();await modal.waitFor({state:'hidden'});
  await page.waitForResponse(r=>new URL(r.url()).pathname==='/api/gse/status'&&r.status()===200);assert.equal(await modal.count(),0,'poll cannot undo manual result close');
  rows.push({outcome,terminal_closed:true,notification_reopened:true,repeated_polls_preserved:true});await rail.getByRole('link',{name:'Actions',exact:true}).click();
 }
 assert.deepEqual(commandRequests,[]);assert.deepEqual(blocked,[]);assert.deepEqual(errors,[]);
 if(process.env.GS_SEQUENCE_EVIDENCE)fs.writeFileSync(path.join(process.env.GS_SEQUENCE_EVIDENCE,'RESULT.json'),JSON.stringify({status:'PASS',rows,commandRequests,blocked,errors})+'\\n');
 await context.close();console.log('PASS actual Wasm terminal success/cancel auto-close and explicit notification reopen; zero commands');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}`;
if(process.env.GS_SEQUENCE_PREPARE_ONLY==='1')fs.writeFileSync(process.env.GS_SEQUENCE_FIXTURE_DEBUG,fixture);else await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));
