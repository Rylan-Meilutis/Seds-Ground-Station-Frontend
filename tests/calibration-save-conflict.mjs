// Actual production Wasm against a synthetic station. No hardware or commands.
// A save made from a calibration the station has since changed is refused
// (409); the editor keeps the draft, says why and offers a reload, and saves
// again only from the reloaded revision. Nothing is silently overwritten.
import fs from 'node:fs';
const original=fs.readFileSync('tests/calibration-page-lifetime.mjs','utf8');
const boundary=original.indexOf('fixture+=`try {');
if(boundary<0)throw Error('Lifetime fixture boundary changed');
let script=original.slice(0,boundary);
script+=String.raw`fixture=fixture.replace('let failNextInput=false;', 'let failNextInput=false;const savePosts=[];');
const calibrationGet="  if(pathname==='/api/calibration'&&req.method==='GET'&&holdNextCalibrationGet)";
if(!fixture.includes(calibrationGet))throw Error('Calibration GET fixture changed');
fixture=fixture.replace(calibrationGet, "  if(pathname==='/api/calibration'&&req.method==='POST'){assert.equal(req.headers.authorization,'Bearer ui-token');let raw='';for await(const c of req)raw+=c;const sent=JSON.parse(raw);savePosts.push(sent);if(sent.revision!==calibration.revision){res.statusCode=409;res.setHeader('Content-Type','text/plain');res.end('Calibration changed on the station since it was loaded (loaded revision '+sent.revision+', station revision '+calibration.revision+'). Nothing was saved: reload the calibration and re-apply your changes.');return;}calibration.extra_channels[sent.channel]=sent.calibration.extra_channels[sent.channel]||{};calibration.revision+=1;res.setHeader('Content-Type','application/json');res.end(JSON.stringify(calibration));return;}\n"+calibrationGet);
fixture+=\`try {
 const errors=[],commands=[];
 calibration.version=2;calibration.revision=5;calibration.mcp_vref_v=2.4;
 const context=await browser.newContext({viewport:{width:1280,height:900}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',socket=>socket.onMessage(data=>commands.push(data)));
 const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));page.on('dialog',d=>{errors.push('Unexpected native dialog');d.dismiss();});
 await page.goto(origin+'/dashboard');await page.getByRole('link',{name:'Calibration',exact:true}).click();
 const cal=page.locator('.gs26-calibration-page');await cal.getByRole('button',{name:'New sequence…',exact:true}).waitFor();
 const save=cal.getByRole('button',{name:'Save',exact:true});
 function saved(){return page.waitForResponse(r=>r.url().endsWith('/api/calibration')&&r.request().method()==='POST');}
 // Edit locally, then another client saves first.
 await cal.getByRole('button',{name:'Reset Channel',exact:true}).click();await cal.getByText('Unsaved',{exact:true}).waitFor();
 calibration.revision=6;const stationChannels=JSON.stringify(calibration.extra_channels);
 let response=saved();await save.click();assert.equal((await response).status(),409);
 const alert=cal.locator(".gs26-calibration-stale");await alert.getByText('Not saved. Calibration changed on the station since it was loaded (loaded revision 5, station revision 6).').waitFor();
 assert.equal(savePosts.length,1);assert.equal(savePosts[0].revision,5,'the save names the revision it was edited from');assert.equal(savePosts[0].fitted_mcp_vref_v,2.4,'and the reference its fit was made at');assert.equal(savePosts[0].calibration.revision,5);
 assert.equal(JSON.stringify(calibration.extra_channels),stationChannels,'refused save changed nothing');
 assert.equal(await cal.getByText('Unsaved',{exact:true}).count(),1,'the draft is kept until the operator reloads');
 // Reload is explicit and confirmed; it fetches the station's current revision.
 await alert.getByRole('button',{name:'Reload station calibration…',exact:true}).click();
 const dialog=page.getByRole('alertdialog',{name:'Discard calibration changes'});await dialog.waitFor();
 const reloaded=page.waitForResponse(r=>r.url().endsWith('/api/calibration')&&r.request().method()==='GET');await dialog.getByRole('button',{name:'Discard and reload',exact:true}).click();assert.equal((await reloaded).status(),200);
 await cal.getByText('Draft discarded; current station calibration loaded',{exact:true}).waitFor();assert.equal(await cal.locator(".gs26-calibration-stale").count(),0,'the stale notice clears after the reload');
 // Re-applied on the reloaded revision, the save is accepted.
 await cal.getByRole('button',{name:'Reset Channel',exact:true}).click();await cal.getByText('Unsaved',{exact:true}).waitFor();
 response=saved();await save.click();assert.equal((await response).status(),200);
 await cal.getByText('Selected sensor saved; other calibrations preserved',{exact:true}).waitFor();
 assert.equal(savePosts.length,2);assert.equal(savePosts[1].revision,6);assert.equal(calibration.revision,7);
 assert.deepEqual(commands,[]);assert.deepEqual(errors,[]);
 await context.close();console.log('PASS actual Wasm: stale calibration save refused with its reason, draft kept, explicit reload, re-applied save from the new revision');
}catch(error){console.error('SAVE_CONFLICT_FAILURE '+String(error));throw error;}finally{if(heldGetRelease)heldGetRelease();for(const release of postReleases.splice(0))release();await browser.close();for(const socket of serverSockets)socket.destroy();await new Promise(resolve=>server.close(resolve));}\`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
`;
script=script.replaceAll('\\`','`');
try{await import('data:text/javascript;base64,'+Buffer.from(script).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
