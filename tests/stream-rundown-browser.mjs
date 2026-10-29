// Reuse the existing isolated fake-backend fixture without changing its source.
// The appended acceptance runs the current real Wasm bundle, never a live station.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {pathToFileURL} from 'node:url';
import path from 'node:path';
const sourcePath='tests/dashboard-customization.mjs';
const original=fs.readFileSync(sourcePath,'utf8');
let fixture=original.slice(0,original.indexOf('try{\n  const context='));
fixture=fixture.replace("let broadcastState=example('live-streams').broadcast;",`let broadcastState=example('live-streams').broadcast;
const metric=label=>({label,binding:{data_type:'BATTERY_VOLTAGE',sender_id:'PB',index:0,scale:1,offset:0},unit:'V',precision:2,source:'telemetry',format:'number'});
broadcastState.rundown={profiles:Object.fromEntries(['ground','flight','recovery','test_fire'].map(id=>[id,{title:id,cycle_seconds:5,stats_per_page:2,scene:'inherit',stats:[metric('Battery'),metric('Current')]}])),phase_profiles:{}};let rejectSave=false;`);
fixture=fixture.replace('const next=JSON.parse(body);assert.equal(next.revision,broadcastState.revision);',"const next=JSON.parse(body);if(rejectSave||next.revision!==broadcastState.revision){res.statusCode=409;res.end('Broadcast changed; reload settings');return;}");
fixture=fixture.replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture+=`try{
 const context=await browser.newContext({viewport:{width:1280,height:900}});
 await context.addInitScript(({session,origin})=>{localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));await page.goto(origin);await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Stream Manager',exact:true}).click();await page.getByRole('heading',{name:'Broadcast studio'}).waitFor();
 const editor=page.locator('.gs26-rundown-editor');await editor.locator('>summary').click();await editor.getByLabel('Title',{exact:true}).fill('Fill coverage');await editor.getByLabel('Seconds per page').fill('7');await editor.getByLabel('Metrics per page').fill('1');await editor.getByLabel(/^Scene/).selectOption('map');
 await editor.getByRole('button',{name:'Add metric',exact:true}).click();const metricEditor=editor.locator('.gs26-program-metric').last();await metricEditor.locator('summary').click();await metricEditor.getByLabel('Label',{exact:true}).fill('Tank PT');await metricEditor.getByLabel('Data type',{exact:true}).fill('PRESSURE_TRANSDUCER_CALIBRATED');await metricEditor.getByRole('button',{name:'Move Tank PT up',exact:true}).click();
 await editor.getByText('Flight phase assignments',{exact:true}).click();await editor.getByLabel(/^Flight phase/).selectOption('Landed');await editor.getByRole('button',{name:'Use selected profile for phase'}).click();await editor.getByRole('button',{name:'Save audience rundown'}).click();await editor.getByText('Audience rundown saved',{exact:true}).waitFor();
 assert.equal(broadcastState.rundown.profiles.ground.title,'Fill coverage');assert.equal(broadcastState.rundown.profiles.ground.cycle_seconds,7);assert.equal(broadcastState.rundown.profiles.ground.stats_per_page,1);assert.equal(broadcastState.rundown.profiles.ground.scene,'map');assert.deepEqual(broadcastState.rundown.profiles.ground.stats.map(s=>s.label),['Battery','Tank PT','Current']);assert.equal(broadcastState.rundown.phase_profiles.Landed,'ground');
 await page.reload();await page.getByRole('heading',{name:'Broadcast studio'}).waitFor();await editor.locator('>summary').click();assert.equal(await editor.getByLabel('Title',{exact:true}).inputValue(),'Fill coverage');
 rejectSave=true;await editor.getByLabel('Title',{exact:true}).fill('Conflicting edit');await editor.getByRole('button',{name:'Save audience rundown'}).click();await editor.getByText(/Broadcast changed; reload settings/).waitFor();assert.equal(broadcastState.rundown.profiles.ground.title,'Fill coverage');rejectSave=false;await editor.getByRole('button',{name:'Reload broadcast settings'}).click();await editor.getByText('Broadcast settings reloaded',{exact:true}).waitFor();assert.equal(await editor.getByLabel('Title',{exact:true}).inputValue(),'Fill coverage');
 for(const width of[320,390,844,1280]){await page.setViewportSize({width,height:900});await page.waitForTimeout(100);const box=await editor.boundingBox();assert(box.width<=width,'rundown fits '+width);assert(await editor.evaluate(e=>e.scrollWidth<=e.clientWidth+1),'editor content fits '+width);}
 assert.deepEqual(errors,[]);console.log('PASS real Wasm rundown editor: stage/timing/scene/custom metric/reorder/phase override/save/reload/conflict, four widths');await context.close();
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
assert.equal(fs.readFileSync(sourcePath,'utf8'),original,'shared fixture remains untouched');
// ESM data URL keeps the generated fixture in memory, without changing any file.
try { await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64')); } catch(error) { console.error(error.name+': '+error.message);process.exitCode=1; }
