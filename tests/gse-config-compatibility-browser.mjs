// Actual Wasm, isolated fake API only; never contacts a station or runs outputs.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture=fixture.replace('let lists={',`let config={cold_flow_target_psi:null,nitrogen_target_psi:100,nitrogen_steps:[],fill_overfill_kg:0.05,fill_tolerance_kg:0.07,fill_settle_ms:3000,turbocharge_max_added_mass_kg:0.25,pressure_step_psi:50,pressure_ceiling_psi:650,maximum_zero_offset_psi:20,dry_self_test_confirmed:false,grouped_panel:false,future_limit:{preserved:true}};const saves=[];let legacy=false;let lists={`);
fixture=fixture.replace('let gseStatus=null,',`let gseStatus={phase:'idle',message:'Ready',nitrogen_passed:false,self_test_locked:false,current_step_psi:0,pressure_psi:0,baseline:null,valves:[false,false,false,false,false]},`);
fixture=fixture.replace("  if(pathname==='/api/checklists'||",`  if(pathname==='/api/gse/config'){res.setHeader('Content-Type','application/json');if(req.method==='POST'){assert.equal(req.headers.authorization,'Bearer ui-token');let body='';for await(const data of req)body+=data;const next=JSON.parse(body);if(legacy&&Object.hasOwn(next,'cold_flow_target_psi')){res.statusCode=422;res.end('unknown field cold_flow_target_psi');return;}config=next;saves.push(config);}res.end(JSON.stringify(config));return;}
  if(pathname==='/api/checklists'||`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:390,height:844}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};const suffix=origin.replace(/[^a-zA-Z0-9]/g,'_');for(const key of ['',suffix])localStorage.setItem('gs26_dashboard_customization_v1_'+key+'_ground_station_view','on');localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(()=>{}));
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>{errors.push(e.message);console.error('PAGE ERROR',e.stack?.slice(0,1200)||e.message)});page.on('console',m=>{if(['error','warning'].includes(m.type())&&!m.text().includes('Failed to load resource'))console.error('BROWSER',m.text().slice(0,1800))});
 async function open(){await page.goto(origin+'/dashboard');await page.getByRole('heading',{name:'Reconfigure GSE',exact:true}).waitFor({timeout:15000}).catch(async error=>{console.error((await page.locator('body').innerText()).slice(-2400));console.error(errors);throw error;});const settings=page.getByRole('link',{name:'Settings',exact:true}).or(page.getByRole('button',{name:/^Settings$/i}));if(!await settings.isVisible())await page.locator('.gs26-header-menu-toggle').filter({hasText:'Tools'}).click();await settings.click();await page.getByRole('button',{name:'Ground Station',exact:true}).click();await page.getByRole('button',{name:'Sequences & limits',exact:true}).click();await page.getByText('Nitrogen test settings',{exact:true}).click();await page.getByLabel('Step 1 pressure in psi',{exact:true}).waitFor();}
 await open();
 const cold=page.getByLabel(/Cold Flow nitrogen target/);
 assert.equal(await cold.inputValue(),'');const original=structuredClone(config);
 await cold.fill('95');assert.equal(saves.length,0);await page.getByRole('button',{name:'Save GSE configuration',exact:true}).click();await page.getByText('GSE configuration saved',{exact:true}).waitFor();
 assert.equal(saves.length,1);assert.equal(config.cold_flow_target_psi,95);
 const withoutCold=structuredClone(config);withoutCold.cold_flow_target_psi=null;assert.deepEqual(withoutCold,original,'Cold target edit preserves every original nitrogen/fill/Turbo/unknown field and legacy empty steps');
 await open();assert.equal(await cold.inputValue(),'95');await cold.fill('');await page.getByRole('button',{name:'Save GSE configuration',exact:true}).click();await page.getByText('GSE configuration saved',{exact:true}).waitFor();assert.equal(config.cold_flow_target_psi,null);assert.equal(saves.length,2);
 await open();assert.equal(await cold.inputValue(),'');
 for(const width of [320,390,844,1280]){await page.setViewportSize({width,height:900});const panel=page.locator('.gs26-station-gse-settings');assert(await panel.evaluate(e=>e.scrollWidth<=e.clientWidth+1),'settings fit '+width);}
 gseStatus.phase='paused';await open();assert(await cold.isDisabled());assert(await page.getByRole('button',{name:'Save GSE configuration',exact:true}).isDisabled());assert.equal(saves.length,2);
 legacy=true;delete config.cold_flow_target_psi;gseStatus.phase='idle';await open();
 assert(await cold.isDisabled(),'old backend cannot edit unsupported Cold Flow field');
 await page.getByRole('button',{name:'Save GSE configuration',exact:true}).click();await page.getByText('GSE configuration saved',{exact:true}).waitFor();
 assert.equal(saves.length,3);assert(!Object.hasOwn(saves[2],'cold_flow_target_psi'),'legacy save must omit even null');
 assert.deepEqual(errors,[]);await context.close();console.log('PASS frozen actual Wasm: independent optional Cold Flow target explicit save/reload/disable, byte-equivalent original settings including legacy N2 plan, active-run refusal UI and narrow/mobile layouts');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
