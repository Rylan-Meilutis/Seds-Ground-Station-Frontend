// Existing isolated operation fixture; no station, radio or output command.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/operation-workflow.mjs','utf8'),boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Operation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture=fixture.replace('authenticated:true,anonymous:false','authenticated:true,anonymous:false,station_role:"rrls"');
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]})),{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(()=>{}));
 const page=await context.newPage(),errors=[],posts=[];page.on('pageerror',e=>errors.push(e.message));page.on('request',request=>{if(request.method()==='POST')posts.push(request.url());});
 await page.goto(origin+'/dashboard');await page.getByRole('link',{name:'Actions',exact:true}).click();
 await page.getByText('Sequence diagnostics',{exact:true}).click();
 const diagnostics=page.locator('details').filter({has:page.getByText('Sequence diagnostics',{exact:true})});
 console.log('Rendered sequence diagnostics:',await diagnostics.innerText());
 await page.getByText('RRLS observer. Primary operating mode is unavailable; sequence diagnostics describe this RRLS backend.',{exact:true}).waitFor({timeout:5000});
 await page.waitForFunction(()=>[...document.querySelectorAll('details')].some(e=>e.textContent.includes('RRLS local backend: mode=non-HITL')));
 assert.doesNotMatch(await diagnostics.innerText(),/mode=sequenced/);
 gseStatus.request_gate.hitl_mode=true;await page.waitForFunction(()=>[...document.querySelectorAll('details')].some(e=>e.textContent.includes('RRLS local backend: mode=HITL')));
 delete gseStatus.request_gate.hitl_mode;await page.waitForFunction(()=>[...document.querySelectorAll('details')].some(e=>e.textContent.includes('RRLS local backend: mode=unknown')));
 for(const width of[320,390,844,1440]){await page.setViewportSize({width,height:1000});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'mode-source notice fits '+width);if(width===390)await page.screenshot({path:'/tmp/rrls-mode-label-390-20261027.png'});}
 gseUnavailable=true;await page.waitForFunction(()=>[...document.querySelectorAll('details')].some(e=>e.textContent.includes('Cannot read RRLS local backend GSE gate')));
 assert.deepEqual(posts.map(url=>new URL(url).pathname).filter(path=>path!=='/api/i18n/translate'),[],'only the existing text-translation request is allowed; no operations or mode mutations');
 assert.deepEqual(errors,[]);await context.close();console.log('PASS RRLS local HITL/non-HITL/unknown labels, unavailable primary mode and no mutation requests');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
