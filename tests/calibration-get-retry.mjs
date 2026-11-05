// Actual production Wasm + original request wrapper. Synthetic server, no commands.
import fs from 'node:fs';
const original=fs.readFileSync('tests/calibration-page-lifetime.mjs','utf8');
const boundary=original.indexOf('fixture+=`try {');
if(boundary<0)throw Error('Lifetime fixture prefix changed');
let script=original.slice(0,boundary);
script+=String.raw`fixture=fixture.replace('let heldGetStarted=null;', 'let heldGetStarted=null;let oldGetDone=null;let calGets=0;let oldGetClosed=false;');
fixture=fixture.replace("  if(pathname==='/api/calibration'&&req.method==='GET'&&holdNextCalibrationGet)", "  if(pathname==='/api/calibration'&&req.method==='GET'){calGets++;res.on('close',()=>{if(!res.writableFinished)oldGetClosed=true;});}  if(pathname==='/api/calibration'&&req.method==='GET'&&holdNextCalibrationGet)");
fixture=fixture.replace('res.end(old);return;', 'res.end(old);if(oldGetDone){oldGetDone();oldGetDone=null;}return;');
fixture+=\`try {
 const errors=[],commands=[];
 session.station_role='primary';
 const context=await browser.newContext({viewport:{width:1280,height:900}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',socket=>socket.onMessage(value=>commands.push(value)));
 const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));page.on('dialog',d=>{errors.push('Unexpected native dialog');d.dismiss();});
 await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 const coefficientsBefore=JSON.stringify(calibration.extra_channels);holdNextCalibrationGet=true;const held=new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error('Original actual calibration GET absent')),30000);heldGetStarted=()=>{clearTimeout(timer);resolve();};});
 await page.getByRole('link',{name:'Calibration',exact:true}).click();await held;assert.equal(calGets,1);const cal=page.locator('.gs26-calibration-page');assert.equal(await cal.getByRole('button',{name:'New sequence…',exact:true}).count(),0);
 session.station_role='rrls';await page.waitForFunction(()=>JSON.parse(localStorage.getItem('auth_session_v1')||'null')?.entries?.[0]?.session?.session?.station_role==='rrls');
 const reload=cal.getByRole('button',{name:'Reload calibration',exact:true});await reload.waitFor();assert.equal(calGets,1,'Retirement cannot silently retry/rebind');assert.equal(await cal.getByRole('button',{name:'New sequence…',exact:true}).count(),0);assert.deepEqual(inputPosts,[]);
 // A different current configuration models another operator/source; no local
 // input POST is used to recover this retired read.
 calibration.kg50_input='p6_differential';const fresh=page.waitForResponse(r=>r.url().endsWith('/api/calibration')&&r.request().method()==='GET');await reload.click();const response=await fresh;assert.equal(response.status(),200);assert.equal((await response.json()).kg50_input,'p6_differential');await cal.getByRole('button',{name:'New sequence…',exact:true}).waitFor();assert.equal(await cal.getByRole('button',{name:'New sequence…',exact:true}).isEnabled(),true);assert.equal(calGets,2);
 // The original GET is already canceled by scope retirement; the server can
 // still attempt its old amp2 response after the new view is ready.
 assert.equal(oldGetClosed,true,'Actual original response transport closed before finish');const oldAttempted=new Promise(resolve=>oldGetDone=resolve);heldGetRelease();heldGetRelease=null;await oldAttempted;await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));assert.equal(await reload.count(),0,'Old read completion cannot retire new editor');assert.equal(await cal.getByRole('button',{name:'New sequence…',exact:true}).isEnabled(),true);assert.equal(calGets,2);assert.deepEqual(inputPosts,[]);assert.deepEqual(writes,[]);assert.deepEqual(commands,[]);assert.deepEqual(errors,[]);assert.equal(JSON.stringify(calibration.extra_channels),coefficientsBefore);
 await context.close();console.log('PASS actual held GET retirement -> explicit Calibration Reload -> fresh current GET, canceled old response cannot retire new editor, no input POST/coefficient mutation');
}catch(error){console.error('GET_RETRY_FAILURE '+String(error));throw error;}finally{if(heldGetRelease)heldGetRelease();for(const release of postReleases.splice(0))release();await browser.close();for(const socket of serverSockets)socket.destroy();await new Promise(resolve=>server.close(resolve));}\`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
`;
script=script.replaceAll('\\`','`');
try{await import('data:text/javascript;base64,'+Buffer.from(script).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
