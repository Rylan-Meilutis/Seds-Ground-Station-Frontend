// Real wasm and Chromium; isolated API fixtures never contact a station.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture=fixture.replace('let lists={',`let plumbing={primary:true,can_configure:true,backend_instance:'000000000000004d',reset_epoch:'0000000000000001',desired:{mode:'stepper',revision:0,durable:false},board:{mode:'stepper',generation:'000000000000002a',epoch:1,fresh:true,ready:true,result:'ok',busy:false,remaining_ms:0,capabilities:{stepper:true,h_bridge:true,auxiliary:true,extend:true}},pending:false,settings_inflight:false,message:'Readback only'};const saves=[];let holdSave=false,releaseSave=null,oldSave401=false;const seenTokens=new Set();let alternateServer=null;let lists={`);
fixture=fixture.replace('let gseStatus=null,',`let gseStatus={phase:'idle',message:'Ready',request_gate:{hitl_mode:false,flight_state:'Idle',prelaunch:true,button_interlock_satisfied:true}},`);
fixture=fixture.replace("  if(pathname.startsWith('/api/')){",`  if(pathname.startsWith('/api/'))seenTokens.add(req.headers.authorization);
  if(pathname==='/api/auth/challenge'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({challenge_id:'browser-only',algorithm:'pbkdf2_sha256',iterations:1,salt_b64:Buffer.alloc(16,1).toString('base64'),server_nonce_b64:Buffer.alloc(32,2).toString('base64')}));return;}
  if(pathname==='/api/auth/login'){session.username='new-user';res.setHeader('Content-Type','application/json');res.end(JSON.stringify({token:'ui-token-2',session}));return;}
  if(pathname==='/api/auth/logout'){res.setHeader('Content-Type','application/json');res.end('{}');return;}
  if(pathname.startsWith('/api/actuator/plumbing/')){res.setHeader('Content-Type','application/json');if(pathname.endsWith('/settings')){assert(['Bearer ui-token','Bearer ui-token-2'].includes(req.headers.authorization));let body='';for await(const data of req)body+=data;saves.push(JSON.parse(body));if(holdSave)await new Promise(resolve=>releaseSave=resolve);if(oldSave401){res.statusCode=401;res.end('old session expired');return;}plumbing.desired={mode:saves.at(-1).mode,revision:plumbing.desired.revision+1,durable:true};plumbing.pending=true;plumbing.message='Desired drive saved; actual actuator confirmation is pending.';}res.end(JSON.stringify(plumbing));return;}
  if(pathname.startsWith('/api/')){`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:390,height:844}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};const suffix=origin.replace(/[^a-zA-Z0-9]/g,'_');for(const key of ['',suffix])localStorage.setItem('gs26_dashboard_customization_v1_'+key+'_ground_station_view','on');localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(()=>{}));
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(origin+'/dashboard');
 const panel=page.getByRole('region',{name:'Fill-line drive'});await panel.waitFor({timeout:15000}).catch(async e=>{console.error((await page.locator('body').innerText()).slice(-4000));throw e;});
 const select=panel.getByLabel('Use for fill-line retraction');await select.waitFor();await page.waitForTimeout(800);
 assert.equal(await select.locator('option[value=h_bridge]').isDisabled(),true,'bridge prohibited even if peer advertises it');
 assert.equal(await select.locator('option').count(),3,'legacy retained as third selectable profile');assert.equal(saves.length,0,'readback polling never writes');
 await select.selectOption('auxiliary');const release=panel.getByRole('checkbox',{name:'Release the idle stepper’s holding torque when switching drives.'});const save=panel.getByRole('button',{name:'Switch drive and release stepper holding torque',exact:true});
 assert(await save.isDisabled());await release.check();assert(await save.isEnabled());assert.equal(saves.length,0);await save.click();
 await panel.getByText('Desired setting saved; waiting for matching actuator readback.',{exact:true}).waitFor();assert.equal(saves.length,1);assert.deepEqual(saves[0],{mode:'auxiliary',expected_generation:'000000000000002a',expected_epoch:1,backend_instance:'000000000000004d',expected_revision:0,expected_reset_epoch:'0000000000000001',release_stepper:true});
 await panel.getByText('Actuator selection: stepper. Timed motion does not verify physical fill-line position.',{exact:true}).waitFor();
 plumbing.board.mode='auxiliary';plumbing.board.busy=true;plumbing.board.remaining_ms=45000;plumbing.can_configure=false;plumbing.pending=false;
 await panel.getByText('Drive running · 45.0 seconds remaining',{exact:true}).waitFor();assert(await select.isDisabled());
 plumbing.board.busy=false;plumbing.board.mode=null;plumbing.board.result='io';plumbing.board.remaining_ms=0;plumbing.can_configure=true;plumbing.pending=true;
 await panel.getByText('Drive metadata needs an explicit retry. Query actual Idle state, choose the drive, and save again. Motion remains inhibited.',{exact:true}).waitFor();
 await select.selectOption('auxiliary');assert(await release.isVisible());assert(await save.isDisabled());assert.equal(saves.length,1,'unknown metadata never repairs automatically');
 for(const width of[320,390,844,1280]){await page.setViewportSize({width,height:900});assert(await panel.evaluate(e=>e.scrollWidth<=e.clientWidth+1),'selector fits '+width);const box=await select.boundingBox();assert(box.height>=44);await panel.screenshot({path:(process.env.GS_PLUMBING_BROWSER_OUTPUT||'/tmp')+'/plumbing-'+width+'.png'});}
 plumbing.backend_instance='000000000000004e';plumbing.board.epoch=2;await page.waitForTimeout(1200);assert.equal(await release.isChecked(),false,'new board context clears release consent');assert.equal(saves.length,1);
 plumbing.board.mode='stepper';plumbing.board.result='ok';plumbing.desired.mode='stepper';plumbing.pending=false;await page.waitForTimeout(1200);
 await select.selectOption('auxiliary');await release.check();holdSave=true;oldSave401=true;await save.click();while(saves.length<2)await new Promise(r=>setTimeout(r,10));
 const signout=page.getByRole('button',{name:'SIGN OUT ui-test',exact:true});if(!await signout.isVisible())await page.locator('.gs26-utility-menu summary').click();await signout.click();
 await page.getByLabel('Username',{exact:true}).fill('new-user');await page.getByLabel('Password',{exact:true}).fill('browser-fixture');await page.getByRole('button',{name:'Sign In',exact:true}).click();
 await page.waitForTimeout(1500);if(!await page.getByRole('region',{name:'Fill-line drive'}).count()){console.error('POST LOGIN BODY',await page.locator('body').innerText());console.error('POST LOGIN ERRORS',errors);const choose=page.getByRole('button',{name:/Choose tab/});if(await choose.count()){await choose.click();await page.getByRole('button',{name:'Actions',exact:true}).click();}}await page.getByRole('region',{name:'Fill-line drive'}).waitFor({timeout:15000});releaseSave();await page.waitForTimeout(1200);
 assert(await page.evaluate(()=>localStorage.getItem('auth_session_v1').includes('ui-token-2')),'new authenticated token survives old401');assert(seenTokens.has('Bearer ui-token-2'),'new context makes actual authenticated readback requests');
 assert.equal(await page.getByText('Drive change failed: HTTP 401: old session expired',{exact:true}).count(),0,'old response does not overwrite new session state');
 // A held save remains bound to the original URL even when the persisted
 // connection changes. The new loopback source receives read-only queries.
 const nextSource={...plumbing,backend_instance:'000000000000004f',desired:{mode:'stepper',revision:8,durable:true},board:{...plumbing.board,mode:'stepper',epoch:3,result:'ok',busy:false},pending:false,message:'New source readback'};let alternateWrites=0;
 alternateServer=http.createServer(async(req,res)=>{res.setHeader('Access-Control-Allow-Origin','*');res.setHeader('Access-Control-Allow-Headers','authorization,content-type');res.setHeader('Access-Control-Allow-Methods','GET,POST,OPTIONS');if(req.method==='OPTIONS'){res.statusCode=204;res.end();return;}res.setHeader('Content-Type','application/json');if(req.url.endsWith('/settings'))alternateWrites++;res.end(JSON.stringify(req.url.includes('/actuator/plumbing/')?nextSource:req.url==='/api/auth/session'?session:[]));});
 await new Promise(r=>alternateServer.listen(0,'127.0.0.1',r));const alternateOrigin='http://127.0.0.1:'+alternateServer.address().port;
 oldSave401=false;holdSave=true;await page.getByRole('region',{name:'Fill-line drive'}).getByLabel('Use for fill-line retraction').selectOption('auxiliary');await page.getByRole('checkbox',{name:'Release the idle stepper’s holding torque when switching drives.'}).check();await page.getByRole('button',{name:'Switch drive and release stepper holding torque',exact:true}).click();while(saves.length<3)await new Promise(r=>setTimeout(r,10));
 await page.evaluate(base=>localStorage.setItem('gs_base_url',base),alternateOrigin);await page.waitForTimeout(700);releaseSave();await page.waitForTimeout(1400);
 assert.equal(await page.getByLabel('Use for fill-line retraction').inputValue(),'stepper','late old URL result cannot replace new source selection');assert.equal(alternateWrites,0,'source handover never repeats a configuration write');assert.equal(await page.getByText('Desired drive saved; actual actuator confirmation is pending.',{exact:true}).count(),0,'old URL message ignored');
 assert.deepEqual(errors,[]);await context.close();console.log('PASS Chromium actual wasm: explicit release, exact bound body, desired versus actual, bridge blocked, running45s, unknown explicit retry, context reset, old401/new-login token boundary, held-save URL handover, 320/390/844/1280 touch layout');
}finally{await browser.close();await new Promise(r=>server.close(r));if(alternateServer)await new Promise(r=>alternateServer.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
