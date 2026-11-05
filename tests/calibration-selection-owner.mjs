// Actual production Wasm. Platform/session responses are synthetic; no internal Rust hooks.
import fs from 'node:fs';
const original=fs.readFileSync('tests/calibration-page-lifetime.mjs','utf8');
const boundary=original.indexOf('fixture+=`try {');
if(boundary<0)throw Error('Lifetime fixture boundary changed');
let script=original.slice(0,boundary);
script+=String.raw`fixture=fixture.replace('const writes=[],queries=[],inputPosts=[],postReleases=[],postStartWaiters=[];', 'const writes=[],queries=[],inputPosts=[],postReleases=[],postStartWaiters=[],inputApplied=[],appliedWaiters=[];function noteInputApplied(input,originActor){const row={input,originActor};inputApplied.push(row);for(const waiter of appliedWaiters.splice(0)){if(inputApplied.length>=waiter.count)waiter.resolve(row);else appliedWaiters.push(waiter);}}');
fixture=fixture.replace('const sent=JSON.parse(raw);', 'const sent=JSON.parse(raw);const originActor=session.username;');
fixture=fixture.replaceAll('calibration.kg50_input=sent.input;', 'calibration.kg50_input=sent.input;noteInputApplied(sent.input,originActor);');
`;

script+=String.raw`fixture+=\`try {
 const errors=[],commands=[];
 async function open(){
  session.station_role='primary';session.username='ui-test';
  const context=await browser.newContext({viewport:{width:1280,height:900}});
  await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
  await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',socket=>socket.onMessage(data=>commands.push(data)));
  const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));await page.goto(origin+'/dashboard');await page.getByRole('link',{name:'Calibration',exact:true}).click();
  const cal=page.locator('.gs26-calibration-page');await cal.getByRole('button',{name:'New sequence…',exact:true}).waitFor();
  async function settings(){await page.getByRole('link',{name:'Settings',exact:true}).click();await page.getByRole('navigation',{name:'Settings scope'}).getByRole('button',{name:'Ground Station',exact:true}).click();const station=page.locator('.gs26-ground-station-settings');await station.getByRole('navigation',{name:'Ground Station settings sections'}).getByRole('button',{name:'Loadcells',exact:true}).click();await page.waitForFunction(()=>document.getElementById('kg50-input')?.value);return station;}
  return{context,page,cal,settings};
 }
 function applied(count){if(inputApplied.length>=count)return Promise.resolve(inputApplied[count-1]);return new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Original remote input effect did not finish')),30000);appliedWaiters.push({count,resolve:row=>{clearTimeout(timer);resolve(row);}});});}
 function started(count){if(inputPosts.length>=count)return Promise.resolve();return new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Actual input POST did not arrive')),30000);postStartWaiters.push({count,resolve:()=>{clearTimeout(timer);resolve();}});});}
 async function switchAndDispatch(page,selector,kind,nextRole){
  // Wait for the real 5s /api/auth/session refresh and its remembered-session
  // write, then synchronously exercise the STILL CONNECTED old form before its
  // separate 250ms retirement poll. Failure to reach that seam is a failure.
  const attempt=page.evaluate(({selector,kind,nextRole})=>new Promise((resolve,reject)=>{
   const element=document.querySelector(selector);if(!element)return reject(Error('Original mounted form missing'));
   const deadline=performance.now()+15000;
   const timer=setInterval(()=>{
    const store=JSON.parse(localStorage.getItem('auth_session_v1')||'null');
    const role=store?.entries?.[0]?.session?.session?.station_role;
    if(role===nextRole){clearInterval(timer);if(!element.isConnected)return reject(Error('Old view retired before source-fence seam'));
     if(kind==='change'){element.value='p6_differential';element.dispatchEvent(new Event('change',{bubbles:true}));}else element.click();
     resolve({connected:element.isConnected,role});
    }else if(performance.now()>=deadline){clearInterval(timer);reject(Error('Real session authority refresh missing'));}
   },0);
  }),{selector,kind,nextRole});
  session.station_role=nextRole;
  assert.deepEqual(await attempt,{connected:true,role:nextRole});
  await page.getByRole('button',{name:'Reload settings',exact:true}).waitFor();
 }
 // All three original write triggers reject a retired mounted form even though
 // the replacement session retains admin calibration permission.
 for(const kind of ['change','retry','retired']){
  const f=await open();let station=await f.settings();
  if(kind!=='change'){
   failNextInput=kind==='retry';const before=inputPosts.length;const incoming=started(before+1);
   await station.getByLabel('50 kg input',{exact:true}).selectOption('p6_differential');await incoming;
   if(kind==='retired'){
    session.station_role='rrls';await f.page.waitForFunction(()=>JSON.parse(localStorage.getItem('auth_session_v1')||'null')?.entries?.[0]?.session?.session?.station_role==='rrls');
   }
   postReleases.shift()();
   if(kind==='retry')await station.getByRole('button',{name:'Confirm selected input again',exact:true}).waitFor();
   else{await f.page.getByRole('button',{name:'Reload settings',exact:true}).click();station=f.page.locator('.gs26-ground-station-settings');await station.getByRole('button',{name:'Confirm current station input',exact:true}).waitFor();await f.page.waitForFunction(()=>document.getElementById('kg50-input')?.value);}
  }
  const before=inputPosts.length;
  const selector=kind==='change'?'#kg50-input':kind==='retry'?'.gs26-ground-station-settings button':'.gs26-ground-station-settings button';
  if(kind!=='change')await station.getByRole('button',{name:kind==='retry'?'Confirm selected input again':'Confirm current station input',exact:true}).evaluate(e=>e.setAttribute('data-owner-trigger','actual'));
  await switchAndDispatch(f.page,kind==='change'?selector:'[data-owner-trigger="actual"]',kind==='change'?'change':'click',kind==='retired'?'primary':'rrls');
  assert.equal(inputPosts.length,before,'Retired '+kind+' form cannot capture the new station-role owner');
  if(kind==='retry'){
   await f.page.getByRole('button',{name:'Reload settings',exact:true}).click();station=f.page.locator('.gs26-ground-station-settings');await station.getByRole('button',{name:'Confirm selected input again',exact:true}).click();await station.getByRole('button',{name:'Confirm current station input',exact:true}).waitFor();assert.equal(inputPosts.length,before,'Completed Unknown from the old owner is retired, never resent by the fresh form click');
   await f.page.waitForFunction(()=>document.getElementById('kg50-input')?.value);const freshStarted=started(before+1);await station.getByRole('button',{name:'Confirm current station input',exact:true}).click();await freshStarted;assert.equal(inputPosts.length,before+1,'Recovery requires a distinct fresh current-station action');postReleases.shift()();await station.getByText('Selection saved. Waiting for fresh readings from the selected input.',{exact:true}).waitFor();
  }
  await f.context.close();
 }
 // A retired Confirmed owner removes an actually dirty editor. Its local
 // lock must not wedge fresh current-station confirmation on the SAME main tab.
 const dirty=await open();let dirtyStation=await dirty.settings();let dirtyBefore=inputPosts.length;let dirtyStarted=started(dirtyBefore+1);await dirtyStation.getByLabel('50 kg input',{exact:true}).selectOption('p6_differential');await dirtyStarted;postReleases.shift()();await dirtyStation.getByText('Selection saved. Waiting for fresh readings from the selected input.',{exact:true}).waitFor();await dirty.page.getByRole('button',{name:'Close',exact:true}).click();await dirty.cal.getByRole('button',{name:'New sequence…',exact:true}).waitFor();
 await dirty.cal.getByText('Advanced fit and manual point tools',{exact:true}).click();await dirty.cal.getByLabel('Known weight or reference value',{exact:true}).fill('2');await dirty.cal.getByLabel('Manual raw ADC reading',{exact:true}).fill('0.1');await dirty.cal.getByRole('button',{name:'Add/Update Point',exact:true}).click();dirtyStation=await dirty.settings();assert.equal(await dirtyStation.getByLabel('50 kg input',{exact:true}).isDisabled(),true,'Actual dirty editor owns the lock');await dirty.page.getByRole('button',{name:'Close',exact:true}).click();
 session.station_role='rrls';await dirty.page.waitForFunction(()=>JSON.parse(localStorage.getItem('auth_session_v1')||'null')?.entries?.[0]?.session?.session?.station_role==='rrls');await dirty.cal.getByRole('button',{name:'Reload calibration',exact:true}).waitFor();assert.equal(await dirty.cal.getByRole('button',{name:'Add/Update Point',exact:true}).count(),0,'Retired editor is really unmounted');
 dirtyStation=await dirty.settings();await dirty.page.waitForFunction(()=>!document.getElementById('kg50-input')?.disabled);dirtyBefore=inputPosts.length;await dirtyStation.getByLabel('50 kg input',{exact:true}).selectOption('amp2');await dirtyStation.getByRole('button',{name:'Confirm current station input',exact:true}).waitFor();assert.equal(inputPosts.length,dirtyBefore,'First stale result action only retires the old lineage');dirtyStarted=started(dirtyBefore+1);const currentConfirm=dirtyStation.getByRole('button',{name:'Confirm current station input',exact:true});assert.equal(await currentConfirm.isEnabled(),true,'Unmounted editor lock releases without unrelated tab navigation');await currentConfirm.click();await dirtyStarted;postReleases.shift()();await dirtyStation.getByText('Selection saved. Waiting for fresh readings from the selected input.',{exact:true}).waitFor();await dirty.page.getByRole('button',{name:'Close',exact:true}).click();await dirty.cal.getByRole('button',{name:'New sequence…',exact:true}).waitFor();await dirty.context.close();
 // Real account replacement remounts the dashboard. Late remote application
 // cannot be rolled back; this checks no old-response confirmation or resend.
 const late=await open();let lateStation=await late.settings();const lateBefore=inputPosts.length;const lateStarted=started(lateBefore+1);await lateStation.getByLabel('50 kg input',{exact:true}).selectOption('amp2');await lateStarted;assert.equal(postReleases.length,1);
 session.username='replacement-actor';await late.page.waitForFunction(()=>JSON.parse(localStorage.getItem('auth_session_v1')||'null')?.entries?.[0]?.session?.session?.username==='replacement-actor');await late.page.getByRole('button',{name:'SIGN OUT replacement-actor',exact:true,includeHidden:true}).first().waitFor({state:'attached'});await late.page.getByRole('link',{name:'Calibration',exact:true}).click();await late.cal.getByRole('button',{name:'Reload calibration',exact:true}).waitFor();assert.equal(await late.cal.getByRole('button',{name:'New sequence…',exact:true}).count(),0,'Retired read never grants capture');
 const oldEffect=applied(lateBefore+1);postReleases.shift()();assert.deepEqual(await oldEffect,{input:'amp2',originActor:'ui-test'},'Old remote effect may complete despite canceled transport');assert.equal(inputPosts.length,lateBefore+1,'Late original request is not resent by replacement actor');lateStation=await late.settings();assert.equal(await lateStation.getByText('Selection saved. Waiting for fresh readings from the selected input.',{exact:true}).count(),0,'Late original response cannot confirm replacement actor');await late.page.getByRole('button',{name:'Close',exact:true}).click();
 const freshRead=late.page.waitForResponse(r=>r.url().endsWith('/api/calibration')&&r.request().method()==='GET');await late.cal.getByRole('button',{name:'Reload calibration',exact:true}).click();const freshResponse=await freshRead;assert.equal(freshResponse.status(),200);assert.equal((await freshResponse.json()).kg50_input,'amp2');await late.cal.getByRole('button',{name:'New sequence…',exact:true}).waitFor();assert.equal(await late.cal.getByRole('button',{name:'New sequence…',exact:true}).isEnabled(),true);assert.equal(inputPosts.length,lateBefore+1,'Explicit read retry needs no input POST');await late.context.close();
 assert.deepEqual(commands,[]);assert.deepEqual(errors,[]);console.log('PASS actual auth refresh: all three mounted selector write triggers reject original-view retirement without rebinding to replacement station authority');
}finally{for(const release of postReleases.splice(0))release();await browser.close();for(const socket of serverSockets)socket.destroy();await new Promise(resolve=>server.close(resolve));}\`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
`;
script=script.replaceAll('\\`','`');
try{await import('data:text/javascript;base64,'+Buffer.from(script).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
