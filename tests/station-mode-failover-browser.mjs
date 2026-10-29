// Actual Wasm and signed discovery redirect, using two isolated HTTP origins.
// Both synthetic origins have their own pre-existing authorized fixture session.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/operation-workflow.mjs','utf8'),boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Operation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture+=`
const relay=http.createServer(server.listeners('request')[0]);await new Promise(resolve=>relay.listen(0,'127.0.0.1',resolve));
const relayOrigin='http://127.0.0.1:'+relay.address().port;
const {webcrypto}=await import('node:crypto');const keys=await webcrypto.subtle.generateKey({name:'Ed25519'},true,['sign','verify']);
const verify_key=Buffer.from(await webcrypto.subtle.exportKey('raw',keys.publicKey)).toString('base64url');
const pairing=(rr)=>({installation_id:'mode-fixture',station_id:rr?'rrls':'gs',verify_key,peers:[{id:rr?'gs':'rrls',role:rr?'primary':'rrls',urls:[rr?origin:relayOrigin]}]});
let primaryOffline=false,modeUnavailable=false;
let relayMode={effective_mode:'hitl',local_build_mode:'normal',primary_station:'gs',synchronized:true,persisted:true,local_capabilities_match:false,authority:'paired_signature',error:null};
const primaryMode={...relayMode,local_build_mode:'hitl',authority:'local_primary',local_capabilities_match:true};
const stationSession=rr=>({...session,station_role:rr?'rrls':'primary',station_pairing:pairing(rr),operating_mode:rr?relayMode:primaryMode});
try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({origin,relayOrigin,primary,relay})=>localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session:primary,remember_me:true}},{host_scope:relayOrigin,updated_at_ms:Date.now(),session:{token:'ui-token',session:relay,remember_me:true}}]})),{origin,relayOrigin,primary:stationSession(false),relay:stationSession(true)});
 await context.route('**/api/station/discovery?**',async route=>{
  const u=new URL(route.request().url()),rr=u.origin===relayOrigin;
  if(!rr&&primaryOffline)return route.abort();
  const payload=Buffer.from(JSON.stringify({installation_id:'mode-fixture',station_id:rr?'rrls':'gs',role:rr?'rrls':'primary',nonce:u.searchParams.get('nonce'),scheme:'http',port:Number(u.port)})).toString('base64url');
  const signature=Buffer.from(await webcrypto.subtle.sign({name:'Ed25519'},keys.privateKey,new TextEncoder().encode('station1.'+payload))).toString('base64url');
  await route.fulfill({json:{payload,signature}});
 });
 await context.route('**/api/auth/session',route=>route.fulfill({json:stationSession(new URL(route.request().url()).origin===relayOrigin)}));
 await context.route('**/api/station_mode',route=>modeUnavailable?route.fulfill({status:503,body:'fixture offline'}):route.fulfill({json:new URL(route.request().url()).origin===relayOrigin?relayMode:primaryMode}));
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(()=>{}));
 const page=await context.newPage(),errors=[],posts=[];page.on('pageerror',e=>errors.push(e.message));page.on('request',request=>{if(request.method()==='POST')posts.push(new URL(request.url()).pathname);});
 await page.goto(origin+'/dashboard');await page.locator('.gs26-operating-mode').filter({hasText:'Operating mode: HITL'}).waitFor();
 await page.getByRole('link',{name:'Actions',exact:true}).click();
 const tabs=await page.getByRole('navigation',{name:'Mission navigation'}).innerText();
 primaryOffline=true;
 for(let i=0;i<3;i++)await page.evaluate(()=>window.GS26StationDiscovery.check());
 await page.waitForURL(relayOrigin+'/**');
 await page.goto(relayOrigin+'/dashboard');
 const notice=page.locator('.gs26-operating-mode');
 await notice.filter({hasText:'Primary operating mode: HITL'}).waitFor();
 assert.match(await notice.innerText(),/Primary operating mode: HITL.*read-only/);assert.doesNotMatch(await notice.innerText(),/Local build|Normal/,'main notice must not imply conflicting primary mode');
 assert.equal(await page.getByRole('navigation',{name:'Mission navigation'}).innerText(),tabs,'mode handover retains configured navigation');
 await page.getByRole('link',{name:'Actions',exact:true}).click();
 await page.getByText('Sequence diagnostics',{exact:true}).click();
 assert.match(await page.locator('details').filter({has:page.getByText('Sequence diagnostics',{exact:true})}).innerText(),/RRLS local backend: mode=non-HITL/,'local interlocks retain their real build context');
 relayMode={...relayMode,synchronized:false};await notice.filter({hasText:'last known; not synchronized'}).waitFor();
 await page.reload();await notice.filter({hasText:'last known; not synchronized'}).waitFor();
 relayMode={...relayMode,synchronized:true,effective_mode:'normal'};await notice.filter({hasText:'Primary operating mode: Normal'}).waitFor();
 relayMode={...relayMode,effective_mode:'testing'};await notice.filter({hasText:'Primary operating mode: Testing'}).waitFor();
 modeUnavailable=true;await notice.filter({hasText:'last known; not synchronized'}).waitFor();
 modeUnavailable=false;relayMode={...relayMode,effective_mode:null,synchronized:false,persisted:false};await notice.filter({hasText:'Primary operating mode: unavailable'}).waitFor();
 for(const width of [320,844]){await page.setViewportSize({width,height:390});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));}
 await page.screenshot({path:'/tmp/station-mode-failover-844-20261027.png'});
 assert.deepEqual(posts.filter(p=>p!=='/api/i18n/translate'),[],'display sync and discovery never mutate mode or send operations');assert.deepEqual(errors,[]);
 console.log('PASS actual Wasm signed discovery GS HITL→RRLS normal-build handover; primary mode retained, local diagnostics truthful, Testing explicit, same tabs, stale/restart/refresh/unknown and no operations');await context.close();
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));await new Promise(resolve=>relay.close(resolve));}
`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.stack||error);process.exitCode=1;}
