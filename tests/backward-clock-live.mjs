// Real Wasm regression: a client clock correction must not freeze live values/trends.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/operation-workflow.mjs','utf8');
const boundary=source.indexOf('\ntry {\n const context=');
if(boundary<0)throw Error('Operation fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
fixture=fixture.replace("  if(pathname.startsWith('/api/')){",`  if(pathname==='/api/system/time'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({utc_ms:Date.now(),network_utc_ms:Date.now(),can_set_system_time:false}));return;}
 if(pathname.startsWith('/api/')){`);
fixture+=`
try {
 const context=await browser.newContext({viewport:{width:1440,height:1100}});
 await context.addInitScript(({session,origin})=>{
  if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};
  localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));
  const originalNow=Date.now;window.__clockOffset=0;Date.now=()=>originalNow()+window.__clockOffset;
 },{session,origin});
 const sockets=new Set();await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{sockets.add(socket);socket.onClose(()=>sockets.delete(socket));});
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));
 await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 const big=page.locator('.gs26-card-grid').locator('div').filter({has:page.locator('.gs26-card-label').filter({hasText:'1000 kg loadcell'})}).filter({has:page.locator('.gs26-card-value')}).first();
 let sequence=0,clockOffset=0;
 const send=()=>{const value=++sequence;const data=['LOADCELL_WEIGHT_KG','LOADCELL_50_WEIGHT_KG'].map(data_type=>({timestamp_ms:Date.now()+clockOffset,data_type,sender_id:'DAQ',values:[value]}));for(const socket of sockets)socket.send(JSON.stringify({ty:'TelemetryBatch',data}));};
 const timer=setInterval(send,40);
 try {
  await page.getByText('1000 kg loadcell',{exact:true}).waitFor();
  await page.waitForTimeout(7500);
  const trend=page.locator('.gs26-custom-dashboard polyline').first();await trend.waitFor();
  const before=(await trend.getAttribute('points')).trim().split(/\\s+/).length;
  assert(before>50,'trend is populated before clock correction: '+before);
  clockOffset=-600000;await page.evaluate(()=>window.__clockOffset=-600000);
  await page.waitForTimeout(600);clearInterval(timer);send();
  const expected=sequence.toFixed(2)+' kg';await page.getByText(expected,{exact:true}).first().waitFor({timeout:2500});
  assert.equal(await page.getByText(expected,{exact:true}).count(),2,'both loadcells update after backward clock step');
  const after=(await trend.getAttribute('points')).trim().split(/\\s+/).length;
  assert(after>=2&&after<50,'display trend starts a new epoch instead of retaining future points: '+after);
  assert.deepEqual(errors,[]);
  console.log('PASS actual Wasm: both loadcells remain live after client clock -10min; trend epoch '+before+'→'+after+' points');
 }finally{clearInterval(timer)}
 await context.close();
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
