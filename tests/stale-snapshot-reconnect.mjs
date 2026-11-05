// Actual compiled client, original timestamped HTTP snapshot, real loopback WS reconnect.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/chart-history.mjs','utf8'),marker='\ntry {\n  const context=';
if(source.split(marker).length!==2)throw Error('Shared fixture marker changed');
let fixture=source.slice(0,source.indexOf(marker)).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href));
fixture=fixture.replace("const server=http.createServer", "let authority=null; const statusHits=[]; const server=http.createServer");
fixture=fixture.replace("if(pathname==='/api/recent')", "if(pathname==='/api/gse/status'){statusHits.push(authority);res.setHeader('Content-Type','application/json');res.end(JSON.stringify(authority?{sequence_progress:{version:1,backend_instance:authority,reset_epoch:'0'.repeat(16),cancel_api_version:1,active:[],terminal:[],reporting_error:null,result_links:[]}}:{}));return;} if(pathname==='/api/recent')");
fixture=fixture.replace("let heldResponse;", "let heldResponse; const heldResponses=[];").replace("heldResponse=res;return;", "heldResponse=res;heldResponses.push(res);return;");
fixture+=String.raw`
try {
 const outcomes=[];
 for(const scenario of ['unknown','known','refinement','newer-rest','equal-time-rest','calibration','replacement','held-replacement']) {
  authority=['known','replacement','held-replacement'].includes(scenario)?'a'.repeat(32):null;
  const context=await browser.newContext({viewport:{width:390,height:844}});
  await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
  const t0=Date.now(), row=(time,value)=>({timestamp_ms:time,source_timestamp_ms:time,received_timestamp_ms:time,data_type:'LOADCELL_50_WEIGHT_KG',sender_id:'DAQ',values:[value]});
  recentRows=[row(t0,1)];const page=await context.newPage(),errors=[],responses=[],commands=[];
  page.setDefaultTimeout(12000);
  page.on('pageerror',e=>errors.push(e.message));
  page.on('response',async response=>{if(new URL(response.url()).pathname==='/api/recent')responses.push(await response.json());});
  page.on('websocket',socket=>socket.on('framesent',frame=>{try{const message=JSON.parse(String(frame.payload));if(message.cmd)commands.push(message)}catch{}}));
  await page.goto(origin+'/dashboard');const card=page.locator('.gs26-card-label').filter({hasText:/^50 kg loadcell$/}).first().locator('..');
  await card.getByText('1.00 kg',{exact:true}).waitFor();
  if(authority)await page.waitForTimeout(1200);
  const t1=Date.now(),live=row(t1,441);broadcast({ty:'TelemetryBatch',data:[live]});
  await card.getByText('441.00 kg',{exact:true}).waitFor();
  await page.waitForTimeout(3000);
  if(scenario==='refinement'){authority='a'.repeat(32);await page.waitForTimeout(1500);}
  if(scenario==='newer-rest')recentRows=[row(Date.now(),442)];
  if(scenario==='equal-time-rest')recentRows=[row(t1,442)];
  const before=await card.innerText(),hitsBefore=recentHits;
  if(scenario==='held-replacement')holdRecent=true;
  if(scenario==='calibration')broadcast({ty:'CalibrationChanged'});
  else if(scenario==='replacement'){authority='b'.repeat(32);}
  else {assert(sockets.size>0);for(const socket of sockets)socket.destroy();}
  const deadline=Date.now()+12000;while(recentHits===hitsBefore&&Date.now()<deadline)await page.waitForTimeout(25);
  assert(recentHits>hitsBefore,'existing reseed route must fetch HTTP history');
  if(scenario==='held-replacement') {
   assert(heldResponse,'hold the actual old-authority HTTP response');
   const oldReply=heldResponse;
   authority='b'.repeat(32);await page.waitForTimeout(1500);
   const fresh=row(Date.now(),0.25);recentRows=[fresh];broadcast({ty:'TelemetryBatch',data:[fresh]});
   await card.getByText('0.25 kg',{exact:true}).waitFor();
   await card.getByText(/Min 0\.25 · Max 0\.25/).waitFor();
   oldReply.end(JSON.stringify([row(t0,1)]));
   // Keep the next new-authority snapshot withheld, so it cannot hide an
   // illegal old reply application by quickly replacing its contaminated history.
   const nextDeadline=Date.now()+12000;
   while(!heldResponses.some(response=>response!==oldReply)&&Date.now()<nextDeadline)await page.waitForTimeout(25);
   assert(heldResponses.some(response=>response!==oldReply),'natural next seed must reach its held response');
   await page.waitForTimeout(500);
   assert.equal(await card.locator('.gs26-card-value').innerText(),'0.25 kg');
   assert.match(await card.innerText(),/Min 0\.25 · Max 0\.25/,'withheld new response cannot hide old-domain contamination');
   holdRecent=false;
   for(const response of heldResponses)if(response!==oldReply&&!response.writableEnded)response.end(JSON.stringify([fresh]));

  }

  const expected=scenario==='held-replacement'?'0.25 kg':['newer-rest','equal-time-rest'].includes(scenario)?'442.00 kg':['calibration','replacement'].includes(scenario)?'1.00 kg':'441.00 kg';
  await card.getByText(expected,{exact:true}).waitFor();await page.waitForTimeout(350);
  assert.equal(await card.locator('.gs26-card-value').innerText(),expected);
  const after=await card.innerText();
  if(scenario==='held-replacement'){assert.match(after,/Min 0\.25 · Max 0\.25/,'old-authority HTTP must not re-enter history/extrema');assert.equal(await card.getByText(/Min 0\.25 · Max 0\.25/).count(),1);}
  if(expected==='441.00 kg') {
   assert.match(after,/Max[^\n]*441|Max[\s\S]*441/,'history extrema cannot roll back');
   // A snapshot may preserve an old value but cannot renew its original age.
   await page.waitForTimeout(Math.max(0,t1+10500-Date.now()));
   assert.match(await card.innerText(),/Stale/i,'quiet original observation stays visibly stale');
   assert(responses.at(-1).every(r=>r.timestamp_ms===t0&&r.source_timestamp_ms===t0&&r.received_timestamp_ms===t0));
  }
  assert.deepEqual(errors,[]);assert.deepEqual(commands,[]);
  outcomes.push({scenario,originalHTTP:row(t0,1),live,before,after,expected,recentRequests:recentHits-hitsBefore,errors,commands});
  await context.close();for(const socket of sockets)socket.destroy();
 }
 if(process.env.GS_RESEED_CAPTURE)fs.writeFileSync(process.env.GS_RESEED_CAPTURE,JSON.stringify(outcomes,null,2));
 console.log('PASS: eight actual reconnect/authority/calibration/delayed-response scenarios; original ages, extrema, newer REST and zero commands.');
}finally{await browser.close();for(const socket of sockets)socket.destroy();await new Promise(resolve=>server.close(resolve));}
`;
await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));
