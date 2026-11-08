// Actual Wasm, loopback synthetic services and original UI event handlers only.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture=fixture.replace('let lists={',`const requests=[];let fixtureLayoutRevision=1,fixtureSessionRevision=1;let lists={`);
fixture=fixture.replace("  const pathname=new URL(req.url,'http://localhost').pathname;",`  const pathname=new URL(req.url,'http://localhost').pathname;requests.push({method:req.method,path:pathname});if(pathname==='/api/layout'){res.setHeader('X-Workspace-Fixture-Layout',String(fixtureLayoutRevision));res.setHeader('Cache-Control','no-store');}if(pathname==='/api/auth/session'){res.setHeader('X-Workspace-Fixture-Session',String(fixtureSessionRevision));res.setHeader('Cache-Control','no-store');}`);
fixture+=`try {
 dashboardLayout.actions_tab.actions=dashboardLayout.actions_tab.actions.filter(action=>['Arm','Abort'].includes(action.cmd));
 dashboardLayout.actions_tab.show_flight_setup=false;dashboardLayout.actions_tab.show_fill_targets=false;
 dashboardLayout.data_tab.tabs=[{id:'BATTERY_VOLTAGE',label:'Workspace voltage',channels:['Supply voltage'],chart:{enabled:true}}];
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 const errors=[],commands=[],blocked=[],sockets=new Set();let socketOpens=0,popups=0;
 await context.route('**/*',async route=>{const url=new URL(route.request().url());if(['http:','https:'].includes(url.protocol)&&url.origin!==origin){blocked.push(url.origin);await route.abort();}else await route.continue();});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{socketOpens++;sockets.add(socket);socket.onClose(()=>sockets.delete(socket));socket.onMessage(data=>commands.push(data));});
 const page=await context.newPage();let documentSerial=0;const requestDocuments=new WeakMap();const documentSession=await context.newCDPSession(page);documentSession.on('Page.frameNavigated',({frame})=>{if(!frame.parentId)documentSerial++;});await documentSession.send('Page.enable');page.on('request',request=>requestDocuments.set(request,documentSerial));page.on('pageerror',e=>errors.push(e.message));page.on('popup',popup=>{popups++;popup.close();});page.on('dialog',async dialog=>{errors.push('Unexpected platform dialog');await dialog.dismiss();});
 const pane=id=>page.locator('[data-workspace-tab="'+id+'"]');
 const waitTabs=async tabs=>{await page.waitForFunction(tabs=>JSON.stringify([...document.querySelectorAll('[data-workspace-tab]')].map(e=>e.dataset.workspaceTab))===JSON.stringify(tabs),tabs);};
 const send=value=>{const now=Date.now();for(const socket of sockets)socket.send(JSON.stringify({ty:'TelemetryBatch',data:[{data_type:'BATTERY_VOLTAGE',sender_id:'PB',values:[value],timestamp_ms:now,received_timestamp_ms:now}]}));};
 // Observe the real new HTTP response AND the existing application's branding
 // and account UI before testing a configuration-dependent pane. Cached LKG or
 // an empty pre-auth nav is not proof that the new layout has been applied.
 const remaining=deadline=>Math.max(1,deadline-Date.now());
 const responseFailures=[];
 const observedResponse=(predicate,deadline,validate,budget={candidates:0})=>new Promise((resolve,reject)=>{
  let done=false;
  const finish=(error,value)=>{if(done)return;done=true;clearTimeout(timer);page.off('response',observe);error?reject(error):resolve(value);};
  const timer=setTimeout(()=>finish(new Error('No completed owned response within original deadline '+JSON.stringify(responseFailures))),remaining(deadline));
  const observe=async response=>{
   if(done||!predicate(response))return;
   if(++budget.candidates>16){finish(new Error('Owned response candidate bound16 exceeded'));return;}
   let body;
   try{const error=await response.finished();if(error)throw error;body=await response.json();}
   catch(error){
    if(responseFailures.length<16)responseFailures.push({path:new URL(response.url()).pathname,document:requestDocuments.get(response.request()),currentDocument:documentSerial,layout:response.headers()['x-workspace-fixture-layout'],session:response.headers()['x-workspace-fixture-session'],failure:response.request().failure()?.errorText||String(error).slice(0,240)});
    return;
   }
   try{validate(body);finish(null,{body,document:requestDocuments.get(response.request())});}catch(error){finish(error);}
  };
  page.on('response',observe);
 });
 const sessionResponse=(revision,deadline,document=documentSerial,expected=structuredClone(session),budget={candidates:0})=>observedResponse(response=>requestDocuments.get(response.request())===document&&new URL(response.url()).pathname==='/api/auth/session'&&response.request().method()==='GET'&&response.status()===200&&response.headers()['x-workspace-fixture-session']===String(revision),deadline,value=>{assert.equal(value.username,expected.username);assert.deepEqual(value.roles,expected.roles);assert.equal(value.permissions.send_commands,expected.permissions.send_commands);},budget);
 const assertSession=async(response,expected)=>{const value=response.body;assert.equal(value.username,expected.username);assert.deepEqual(value.roles,expected.roles);assert.equal(value.permissions.send_commands,expected.permissions.send_commands);};
 const reloadObservedContext=async()=>{
  const deadline=Date.now()+30000,expectedDocument=documentSerial+1,expectedSession=structuredClone(session),expectedTabs=[...dashboardLayout.main_tabs],expectedName=dashboardLayout.branding.app_name,budget={candidates:0};
  const layoutReply=observedResponse(response=>requestDocuments.get(response.request())===expectedDocument&&new URL(response.url()).pathname==='/api/layout'&&response.request().method()==='GET'&&response.status()===200&&response.headers()['x-workspace-fixture-layout']===String(fixtureLayoutRevision),deadline,value=>{assert.deepEqual(value.main_tabs,expectedTabs);assert.equal(value.branding.app_name,expectedName);},budget);
  const authReply=sessionResponse(fixtureSessionRevision,deadline,expectedDocument,expectedSession,budget);
  const [, [layoutResponse,authResponse]]=await Promise.all([page.reload({timeout:remaining(deadline)}),Promise.all([layoutReply,authReply])]);
  assert.equal(layoutResponse.document,expectedDocument);assert.equal(authResponse.document,expectedDocument);
  const layout=layoutResponse.body;assert.deepEqual(layout.main_tabs,expectedTabs);assert.equal(layout.branding.app_name,expectedName);await assertSession(authResponse,expectedSession);
  await page.waitForFunction(name=>document.querySelector('#gs26-sidebar-navigation')?.getAttribute('aria-label')===name+' pages',expectedName,{timeout:remaining(deadline)});
  await page.getByRole('navigation',{name:'Mission navigation'}).waitFor({timeout:remaining(deadline)});
  await page.getByText('Sign out '+expectedSession.username,{exact:true}).waitFor({state:'attached',timeout:remaining(deadline)});
  return deadline;
 };
 const changeLayoutMarker=()=>{fixtureLayoutRevision++;dashboardLayout.branding.app_name='Workspace fixture layout '+fixtureLayoutRevision;};

 await page.goto(origin+'/dashboard');await waitTabs(['state']);
 // Split view is an opt-in mode; turning it on restores the saved panes.
 await page.getByRole('button',{name:'Split view',exact:true}).click();await waitTabs(['data','actions']);
 assert.equal(await page.getByRole('button',{name:'Arm',exact:true}).count(),1,'one Actions instance');
 for(const width of [1440,1920]){
  await page.setViewportSize({width,height:1000});await waitTabs(['data','actions']);
  const boxes=await page.locator('.gs26-workspace-pane').evaluateAll(nodes=>nodes.map(node=>{const r=node.getBoundingClientRect();return {x:r.x,right:r.right,width:r.width,height:r.height};}));
  assert.equal(boxes.length,2);assert.ok(boxes.every(b=>b.width>300&&b.height>200));assert.ok(boxes[0].right<=boxes[1].x+1);assert.ok(boxes[1].right<=width+1);
  assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no horizontal overflow');
  if(process.env.GS_WORKSPACE_EVIDENCE)await page.screenshot({path:path.join(process.env.GS_WORKSPACE_EVIDENCE,'workspace-'+width+'.png')});
 }
 const rail=page.getByRole('navigation',{name:'Mission navigation'});
 await rail.getByRole('link',{name:'Actions',exact:true}).click();await waitTabs(['actions','data']);
 await pane('data').getByRole('button',{name:'Workspace voltage',exact:true}).click();
 send(7.25);await pane('data').getByText('7.2500',{exact:true}).first().waitFor();
 send(8.5);await pane('data').getByText('8.5000',{exact:true}).first().waitFor();
 const originalSockets=socketOpens;assert.equal(sockets.size,1);
 // Real held primary button; replacing only the second view must retain the
 // mounted original button but consume its gesture, never reinterpret release.
 const arm=page.getByRole('button',{name:'Arm',exact:true});await arm.evaluate(node=>window.originalArm=node);
 const bounds=await arm.boundingBox();await page.mouse.move(bounds.x+bounds.width/2,bounds.y+bounds.height/2);await page.mouse.down();
 const started=Date.now();await page.getByLabel('Second workspace pane',{exact:true}).selectOption('connection-status');await waitTabs(['actions','connection-status']);
 assert.equal(await arm.evaluate(node=>node===window.originalArm&&node.isConnected),true);
 assert.ok(Date.now()-started<650,'same-node gesture cancellation is not a press timeout');
 await page.mouse.up();assert.equal(commands.length,0,'layout change cannot dispatch a held original gesture');
 await arm.click();for(let i=0;i<50&&commands.length<1;i++)await page.waitForTimeout(20);assert.equal(commands.length,1,'fresh deliberate original handler still dispatches once');
 // An already genuine emergency intent retains its existing owner across a
 // layout change; the ordinary gesture invalidator must not consume Stop.
 const stop=pane('actions').getByRole('button',{name:'Abort',exact:true});const stopBox=await stop.boundingBox();
 await page.mouse.move(stopBox.x+stopBox.width/2,stopBox.y+stopBox.height/2);await page.mouse.down();const stopStarted=Date.now();
 await page.getByLabel('Second workspace pane',{exact:true}).selectOption('data');await waitTabs(['actions','data']);
 assert.ok(Date.now()-stopStarted<650);await page.mouse.up();for(let i=0;i<50&&commands.length<2;i++)await page.waitForTimeout(20);
 assert.equal(commands.length,2,'original genuine Stop survives pane transition');
 assert.equal(socketOpens,originalSockets,'pane switches share the original socket');const paneSwitchReopens=socketOpens-originalSockets;
 assert.equal(await page.locator('.gs26-actions-page').count(),1);
 // Both panes choose their own view, the selectors show what is really mounted,
 // and choosing the second pane's view as the first swaps rather than dropping.
 const first=page.getByLabel('First workspace pane',{exact:true}),second=page.getByLabel('Second workspace pane',{exact:true});
 const shown=async(a,b)=>{await waitTabs([a,b]);await page.waitForFunction(([a,b])=>document.querySelector('[aria-label="First workspace pane"]').value===a&&document.querySelector('[aria-label="Second workspace pane"]').value===b,[a,b]);};
 await shown('actions','data');
 await second.selectOption('connection-status');await shown('actions','connection-status');
 await first.selectOption('connection-status');await shown('connection-status','actions');
 // Choosing the first pane's view on the second side swaps the two.
 await second.selectOption('connection-status');await shown('actions','connection-status');
 await rail.getByRole('link',{name:'Actions',exact:true}).click();await shown('actions','connection-status');
 await page.evaluate(origin=>{for(const base of ['',origin])localStorage.setItem('gs26_dashboard_customization_v1_'+base.replace(/[^a-zA-Z0-9]/g,'_')+'_ground_station_view','on');},origin);
 await page.reload();await shown('actions','connection-status');
 await second.selectOption('checklists');await shown('actions','checklists');
 // Commands stay beside the dashboard, with Actions as its only ground control surface.
 await first.selectOption('state');await shown('state','checklists');
 await second.selectOption('actions');await shown('state','actions');
 assert.equal(await page.locator('.gs26-actions-page').count(),1);
 assert.equal(await page.getByText('Reconfigure GSE',{exact:true}).count(),0,'State omits its ground controls next to Actions');
 await second.selectOption('checklists');await shown('state','checklists');
 await page.getByText('Reconfigure GSE',{exact:true}).waitFor();
 await second.selectOption('detailed');await shown('state','detailed');
 assert.equal(await page.locator('[data-workspace-tab="detailed"] .gs26-info-masonry').evaluate(node=>getComputedStyle(node).columnCount),'2','information panels fit the pane rather than the window');
 await second.selectOption('data');await first.selectOption('actions');await shown('actions','data');
 // The sequence host observes Actions in the SECOND pane as well.
 await rail.getByRole('link',{name:'Telemetry',exact:true}).click();await waitTabs(['data','actions']);
 const key={backend_instance:'1'.repeat(32),reset_epoch:'2'.repeat(16),run_id:'3'.repeat(32)};
 gseStatus={message:'Synthetic sequence',sequence_progress:{version:1,backend_instance:key.backend_instance,reset_epoch:key.reset_epoch,cancel_api_version:1,terminal:[],result_links:[],reporting_error:null,board_display:{},active:[{version:1,key,revision:1,kind:'nitrogen_test',outcome:'running',origin_client:'remote-station',title:'Workspace sequence',message:'Monitoring unchanged',current_step:'observe',next_step:null,steps:[{id:'observe',label:'Observe',state:'watching',watch:[]}],observed_age_ms:0,valid_for_ms:2000,cancel_allowed:true,details_complete:true}]}};
 await page.locator('.gs26-sequence-view').waitFor();assert.equal(await page.locator('.gs26-sequence-view').count(),1);
 await page.getByRole('button',{name:'Minimize sequence progress',exact:true}).click();gseStatus=null;
 await page.getByRole('link',{name:'Settings',exact:true}).click();
 await page.getByRole('navigation',{name:'Settings scope',exact:true}).waitFor();assert.equal(await page.getByRole('navigation',{name:'Settings scope',exact:true}).count(),1);
 await page.getByRole('button',{name:'Close',exact:true}).first().click();
 // Narrow mobile has one mounted view, not a CSS-hidden second editor/control.
 await page.setViewportSize({width:390,height:844});await waitTabs(['data']);
 assert.equal(await page.locator('.gs26-workspace-tools').count(),0);assert.equal(await page.locator('.gs26-action-control').count(),0);
 assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
 if(process.env.GS_WORKSPACE_EVIDENCE)await page.screenshot({path:path.join(process.env.GS_WORKSPACE_EVIDENCE,'workspace-390.png')});
 await page.setViewportSize({width:1440,height:1000});await waitTabs(['data','actions']);
 await page.getByRole('button',{name:'Close second pane',exact:true}).click();await waitTabs(['data']);
 await page.reload();await waitTabs(['data']);await page.getByRole('button',{name:'Split view',exact:true}).click();await waitTabs(['data','actions']);
 assert.equal(commands.length,2,'resize/reload/preferences never issue commands');
 // Existing auth refresh really removes the grant and remounts its old context.
 const revokeDeadline=Date.now()+9000;fixtureSessionRevision++;session.roles=[];session.permissions.send_commands=false;const revokedReply=sessionResponse(fixtureSessionRevision,revokeDeadline);
 await assertSession(await revokedReply,structuredClone(session));
 await page.getByRole('navigation',{name:'Mission navigation'}).waitFor({timeout:remaining(revokeDeadline)});
 await page.getByRole('navigation',{name:'Mission navigation'}).getByRole('link',{name:'Actions',exact:true}).waitFor({state:'detached',timeout:remaining(revokeDeadline)});
 await page.waitForFunction(()=>!document.querySelector('[data-workspace-tab="actions"]'),null,{timeout:remaining(revokeDeadline)});
 assert.equal(await page.locator('.gs26-action-control').count(),0);
 assert.equal(await page.getByLabel('Second workspace pane').locator('option[value="actions"]').count(),0);
 assert.equal(commands.length,2);
 // A configured removal and a real saved hidden-tab preference both override
 // the saved secondary choice; neither silently remounts an unsupported view.
 session.roles=['admin'];session.permissions.send_commands=true;fixtureSessionRevision++;
 const configuredTabs=[...dashboardLayout.main_tabs];dashboardLayout.main_tabs=configuredTabs.filter(id=>id!=='actions');changeLayoutMarker();
 const removalDeadline=await reloadObservedContext();
 await page.getByRole('button',{name:'Actions Enabled',exact:true}).waitFor({timeout:remaining(removalDeadline)});
 await page.getByRole('navigation',{name:'Mission navigation'}).getByRole('link',{name:'Actions',exact:true}).waitFor({state:'detached',timeout:remaining(removalDeadline)});
 assert.equal(await pane('actions').count(),0);
 dashboardLayout.main_tabs=configuredTabs;changeLayoutMarker();
 await page.evaluate(()=>{const key=Object.keys(localStorage).find(key=>key.startsWith('gs26_wide_workspace_v1:'));const [station,account]=JSON.parse(key.slice('gs26_wide_workspace_v1:'.length));const legacy='gs26_dashboard_customization_v1_'+station.replace(/[^a-zA-Z0-9]/g,'_')+'_user_'+account;localStorage.setItem(legacy,JSON.stringify({order:[],hidden:['actions']}));localStorage.setItem(key,JSON.stringify({enabled:true,primary:'data',secondary:'actions'}));});
 const hiddenDeadline=await reloadObservedContext();
 await page.getByRole('navigation',{name:'Mission navigation'}).getByRole('link',{name:'Actions',exact:true}).waitFor({state:'detached',timeout:remaining(hiddenDeadline)});
 assert.equal(await pane('actions').count(),0,'hidden Actions never mounts through persisted split');
 assert.equal(await page.getByLabel('Second workspace pane').locator('option[value="actions"]').count(),0);
 session.username='other-workspace-account';fixtureSessionRevision++;changeLayoutMarker();await reloadObservedContext();await waitTabs(['data']);
 assert.equal(await page.getByRole('button',{name:'Arm',exact:true}).count(),0,'another account gets its own layout choice');
 await page.getByRole('button',{name:'Split view',exact:true}).click();await waitTabs(['data','actions']);
 assert.equal(await page.getByRole('button',{name:'Arm',exact:true}).count(),1);
 assert.equal(popups,0);assert.deepEqual(blocked,[]);assert.deepEqual(errors,[]);
 assert.equal(requests.filter(r=>r.path==='/api/command').length,0);
 console.log(JSON.stringify({result:'PASS actual wide workspace',widths:[1440,1920,390],one_actions_instance:true,secondary_telemetry_updates:true,held_gesture_canceled:true,original_emergency_survives:true,fresh_commands:commands.length,socket_reopens_from_pane_changes:paneSwitchReopens,auth_revocation_observed:true,external_requests:blocked.length,response_failures:responseFailures,errors},null,2));
 await context.close();
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.stack||error);process.exitCode=1;}
