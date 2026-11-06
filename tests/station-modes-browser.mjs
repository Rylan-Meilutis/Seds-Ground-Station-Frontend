// Actual app, optional custom V2 backend. Source-stage controls deliberately keep
// client switching unqualified; no test-only runtime flag can enable it.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8'),boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Shared browser fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
fixture=fixture.replace('let lists={',String.raw`
function initialMode(){return {version:2,backend_instance:'custom-process-A',principal_context:'operator-context',authority_id:'custom-authority',revision:'choices-1',policy_epoch:'owner-A:epoch-1',authority_current:true,valid_for_ms:5000,epoch_fencing_supported:true,admissions_open:true,change_supported:false,cancel_supported:false,current:{id:'payload-sim',mode_id:'payload/checkout',label:'Payload checkout · Simulation',simulation:true,output_domain:'Isolated simulated telemetry and actions'},modes:[{id:'payload/checkout',label:'Payload checkout'},{id:'custom α',label:'Custom <img id=mode-injected src=x>'}],permissions:{can_change:false,can_cancel_transition:false},options:[],transition:null,reason:'Descriptive only; backend switching is not installed.'};}
let mode=initialMode(),modeStatus=200;const modeReads=[],modeWrites=[];
let lists={`);
fixture=fixture.replace("  if(pathname==='/api/checklists'||",String.raw`
  if(pathname==='/api/station-modes'){
    assert.equal(req.headers.authorization,'Bearer ui-token');modeReads.push(structuredClone(mode));res.setHeader('Content-Type','application/json');res.statusCode=modeStatus;res.end(JSON.stringify(mode));return;
  }
  if(pathname.startsWith('/api/station-modes/')){modeWrites.push(pathname);res.statusCode=501;res.end('{}');return;}
  if(pathname==='/api/checklists'||`);
fixture+=String.raw`try {
 const context=await browser.newContext({viewport:{width:390,height:844}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const commands=[];await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',socket=>socket.onMessage(message=>commands.push(JSON.parse(message))));
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));page.setDefaultTimeout(12000);
 async function until(predicate,message){const by=Date.now()+12000;while(Date.now()<by){if(await predicate())return;await new Promise(resolve=>setTimeout(resolve,25));}throw Error(message);}
 async function open(){await page.goto(origin+'/dashboard');const by=Date.now()+12000;await page.getByRole('navigation',{name:'Mission navigation',includeHidden:true}).waitFor({state:'attached',timeout:Math.max(1,by-Date.now())});await page.locator('.gs26-dashboard-shell').waitFor({timeout:Math.max(1,by-Date.now())});const settings=page.getByRole('link',{name:'Settings',exact:true}).or(page.getByRole('button',{name:'Settings',exact:true}));if(!await settings.isVisible())await page.getByRole('button',{name:'Show navigation sidebar',exact:true}).click();await settings.click();await page.getByRole('button',{name:'Ground Station',exact:true}).click();await page.getByRole('navigation',{name:'Ground Station settings sections'}).getByRole('button',{name:'Mission',exact:true}).click();await page.getByRole('combobox',{name:'Station operating mode',exact:true}).waitFor();}
 const select=page.getByRole('combobox',{name:'Station operating mode',exact:true}),review=page.getByRole('button',{name:'Review mode change',exact:true}),sim=page.getByRole('checkbox',{name:'Simulate telemetry and actions',exact:true});
 await open();await page.getByText('Payload checkout · Simulation',{exact:true}).waitFor();assert(await select.isDisabled());assert(await sim.isDisabled());assert(await review.isDisabled());assert(await sim.isChecked());assert.match(await page.locator('.gs26-simulation-domain').innerText(),/SIMULATION.*simulated telemetry and actions/);assert.deepEqual(modeWrites,[]);
 mode.change_supported=true;mode.permissions.can_change=true;mode.options=[{id:'third-team-sim',mode_id:'custom α',simulation:true,label:'Third team · Simulation',available:true,reason:null,transition_kind:'in_place',output_domain:'Isolated custom simulator'}];
 await until(async()=>!await select.isDisabled(),'draft descriptor never available');await select.selectOption('custom α');assert.equal(await page.locator('#mode-injected').count(),0,'labels stay plain text');await page.getByText(/Draft only: Third team · Simulation/).waitFor();assert(await review.isDisabled(),'server capability cannot bypass incomplete client integration');
 await sim.uncheck();await page.getByText('This mode/Simulation combination is not advertised. No target is inferred.',{exact:true}).waitFor();assert(await review.isDisabled());assert.equal(await sim.isChecked(),false,'draft is independent');assert.match(await page.locator('.gs26-simulation-domain').innerText(),/SIMULATION/,'actual domain remains simulated while live draft is unsubmitted');await sim.check();assert.deepEqual(modeWrites,[],'draft changes never prepare/confirm');
 for(const width of [320,390,844,1280]){await page.setViewportSize({width,height:900});assert(await select.isVisible());assert(await sim.isVisible());}
 mode.authority_current=false;await until(async()=>await select.isDisabled(),'nonauthoritative descriptor never disabled');assert(await review.isDisabled());assert.match(await page.locator('.gs26-simulation-domain').innerText(),/unverified/);
 const before404=modeReads.length;modeStatus=404;await until(()=>modeReads.length>before404,'optional endpoint not observed');await page.getByText(/Mode observation:.*HTTP 404/).waitFor();assert(await review.isDisabled());assert.deepEqual(modeWrites,[]);
 for(const variant of ['missing-current','missing-simulation','duplicate-pair','unknown-mode','v1']){
   mode=initialMode();modeStatus=200;if(variant==='missing-current')delete mode.current;if(variant==='missing-simulation')delete mode.current.simulation;if(variant==='duplicate-pair'){mode.options=[{id:'pair1',mode_id:'custom α',simulation:true,label:'One',available:true,reason:null,transition_kind:'in_place',output_domain:'sim'},{id:'pair2',mode_id:'custom α',simulation:true,label:'Two',available:true,reason:null,transition_kind:'in_place',output_domain:'sim'}];}if(variant==='unknown-mode')mode.current.mode_id='not-supplied';if(variant==='v1')mode.version=1;
   await open();await page.getByText('This backend has not provided a usable V2 mode capability. Existing fixed-mode operation is unchanged.',{exact:true}).waitFor();assert(await select.isDisabled());assert(await review.isDisabled());assert(await sim.isDisabled());
 }
 assert.deepEqual(commands,[],'mode controls never emit commands');assert.deepEqual(modeWrites,[],'unqualified switching never posts');assert.deepEqual(errors,[]);await context.close();console.log('PASS actual V2 custom modes, explicit orthogonal Simulation pairs, actual-vs-draft distinction, readonly client/backend gates, malformed/unsupported capability, plain labels and mobile widths; no writes or commands');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}`;
if(process.env.GS_MODE_FIXTURE_DEBUG)fs.writeFileSync(process.env.GS_MODE_FIXTURE_DEBUG,fixture);
if(!process.env.GS_MODE_PREPARE_ONLY)try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
