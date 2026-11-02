// Actual Wasm against a synthetic station. No hardware or command execution.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture=fixture.replace('let lists={',`const writes=[],queries=[];let calibration=example('calibration-file');calibration.kg50_input='amp2';let fillTargets={version:1,fill_source:'kg50',nitrogen:{target_mass_kg:10,target_pressure_psi:100},nitrous:{target_mass_kg:20,target_pressure_psi:200}};let lists={`);
fixture=fixture.replace("let actionPolicy=example('action-policy');", "let actionPolicy=example('action-policy');actionPolicy.controls=actionPolicy.controls.filter(c=>c.cmd!=='StartFill');actionPolicy.controls.push({cmd:'StartFill',enabled:true,actuated:false,blink:'slow'});");
fixture=fixture.replace('let gseStatus=null,',`let gseStatus={phase:'idle',message:'Ready',nitrogen_passed:false,self_test_locked:false,current_step_psi:0,pressure_psi:0,baseline:null,valves:[false,false,false,false,false]},`);
fixture=fixture.replace("  if(pathname==='/api/checklists'||",`  if(req.method==='POST'){if(['/api/actuator/plumbing/query','/api/board-recovery/query'].includes(pathname))queries.push(pathname);else writes.push(pathname);}
  if(pathname==='/api/auto_zero'){res.setHeader('Content-Type','application/json');res.end('{"enabled":true}');return;}
  if(pathname==='/api/calibration'||pathname==='/api/calibration_config'||pathname==='/api/calibration/kg50_input'){
    if(req.method==='POST'){assert.equal(req.headers.authorization,'Bearer ui-token');let raw='';for await(const c of req)raw+=c;calibration.kg50_input=JSON.parse(raw).input;}
    res.setHeader('Content-Type','application/json');res.end(JSON.stringify(pathname==='/api/calibration_config'?example('calibration-config'):calibration));return;
  }
  if(pathname==='/api/flight_setup'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(example('flight-setup')));return;}
  if(pathname==='/api/fill_targets'){
    if(req.method==='POST'){assert.equal(req.headers.authorization,'Bearer ui-token');let raw='';for await(const c of req)raw+=c;fillTargets=JSON.parse(raw);}
    res.setHeader('Content-Type','application/json');res.end(JSON.stringify(fillTargets));return;
  }
  if(pathname==='/api/gse/config'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({nitrogen_target_psi:100,pressure_step_psi:50,pressure_ceiling_psi:650,maximum_zero_offset_psi:20,dry_self_test_confirmed:false,grouped_panel:false}));return;}
  if(pathname==='/api/checklists'||`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const commands=[],errors=[];await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(data=>commands.push(data)));
 const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));await page.goto(origin+'/dashboard');
 await page.getByRole('link',{name:'Settings',exact:true}).click();
 const scope=page.getByRole('navigation',{name:'Settings scope'});await scope.getByRole('button',{name:'Client',exact:true}).waitFor();
 assert.equal(await scope.getByRole('button',{name:'Client',exact:true}).getAttribute('aria-pressed'),'true');
 assert.equal(await page.getByLabel('50 kg input',{exact:true}).count(),0,'no station hardware selection in Client');
 assert.equal(await page.getByRole('button',{name:'Save Fill Targets',exact:true}).count(),0);
 await scope.getByRole('button',{name:'Ground Station',exact:true}).click();
 const station=page.locator('.gs26-ground-station-settings');await station.getByLabel('50 kg input',{exact:true}).waitFor();
 await station.getByLabel('Nitrous target mass (kg)',{exact:true}).waitFor();
 assert.equal(await station.getByLabel('50 kg input',{exact:true}).count(),1,'one shared input editor');
 assert.deepEqual(writes,[],'opening settings never changes station configuration; dedicated read-only capability queries are tracked separately');assert(queries.every(p=>['/api/actuator/plumbing/query','/api/board-recovery/query'].includes(p)));
 await station.getByLabel('50 kg input',{exact:true}).selectOption('p6_differential');await station.getByText('Selection saved. Waiting for fresh readings from the selected input.',{exact:true}).waitFor();
 assert.equal(calibration.kg50_input,'p6_differential');assert.deepEqual(writes,['/api/calibration/kg50_input']);
 const mass=station.getByLabel('Nitrous target mass (kg)',{exact:true});await mass.fill('23.5');await mass.blur();
 assert.equal(fillTargets.nitrous.target_mass_kg,20,'draft edit does not publish');
 await station.getByRole('button',{name:'Save Fill Targets',exact:true}).click();await station.getByText('Fill targets saved.',{exact:true}).waitFor();assert.equal(fillTargets.nitrous.target_mass_kg,23.5);
 assert.deepEqual(writes,['/api/calibration/kg50_input','/api/fill_targets']);
 assert.equal(await station.getByRole('button',{name:'Start fill',exact:true}).count(),0,'settings does not embed actuation controls');
 for(const width of[320,390,844,1440]){await page.setViewportSize({width,height:1000});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'settings fits '+width);}
 await page.setViewportSize({width:1440,height:1000});await station.getByRole('button',{name:'Open calibration',exact:true}).click();
 await page.getByRole('heading',{name:'Calibration',exact:true}).waitFor();await page.getByRole('button',{name:'New sequence…',exact:true}).waitFor();
 assert.equal(await page.getByLabel('50 kg input',{exact:true}).count(),0,'calibration has no second input editor');
 assert(await page.getByText('Advanced fit and manual point tools',{exact:true}).isVisible());
 assert.equal(await page.locator('.gs26-calibration-temperature').getAttribute('open'),null,'thermal diagnostics stay secondary');
 const before=commands.length;await page.getByRole('link',{name:'Actions',exact:true}).click();await page.getByRole('heading',{name:'Actions',exact:true}).waitFor();
 assert.equal(await page.getByRole('button',{name:'Save Fill Targets',exact:true}).count(),0,'Actions contains no duplicate station editor');
 gseStatus.phase='filling';await page.locator('button[data-sequence-state="In progress"]').filter({hasText:'Start fill'}).waitFor();
 const running=page.locator('button[data-sequence-state="In progress"]').filter({hasText:'Start fill'});assert.match(await running.getAttribute('style'),/box-shadow:0 0 14px/);assert.equal(await running.getAttribute('aria-busy'),'true');assert.equal(commands.length,before,'illumination follows observations without sending a command');
 gseUnavailable=true;await page.locator('button[data-sequence-state="Status unavailable"]').filter({hasText:'Start fill'}).waitFor();assert.equal(await running.count(),0,'lost sequence status clears running illumination');const lost=page.locator('button[data-sequence-state=\"Status unavailable\"]').filter({hasText:'Start fill'});assert.doesNotMatch(await lost.getAttribute('style'),/box-shadow:0 0 14px/,'a stale recommended policy cannot keep sequence illumination');assert.match(await lost.getAttribute('style'),/animation:none/,'stale recommended policy cannot keep a sequence progress pulse');
 assert.deepEqual(errors,[]);await context.close();console.log('PASS actual Wasm: Client/station split, one loadcell input editor, explicit saved fill targets, no mount effects, guided calibration, sequence progress/loss and responsive settings');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
