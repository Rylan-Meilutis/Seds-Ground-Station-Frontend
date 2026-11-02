// Actual Wasm against isolated synthetic services; no station commands.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const commands=[],errors=[];await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(data=>commands.push(data)));
 const page=await context.newPage();page.on('pageerror',e=>errors.push(e.message));await page.goto(origin+'/dashboard');
 await page.getByRole('button',{name:'Customize dashboard',exact:true}).click();
 let editors=page.locator('.gs26-card-editor');await editors.first().waitFor();
 for(const editor of await editors.all()) {
  assert.equal(await editor.locator('input').count(),1,'only the user label accepts text');
  assert.equal(await editor.getByLabel('Title',{exact:true}).count(),1);
  for(const name of['Section','Display','Card width','Data type','Board','Channel','Unit label','Decimal places','Display range'])assert.equal(await editor.getByLabel(name,{exact:true}).locator('option').count()>0,true,name+' has choices');
  assert.equal(await editor.locator('input[type=number],textarea').count(),0,'no numeric or arbitrary source fields');
 }
 const first=editors.first();await first.getByLabel('Title',{exact:true}).fill('Chamber monitor');
 const card=page.locator('.gs26-dashboard-card').filter({has:page.locator('.gs26-card-label').filter({hasText:'Chamber monitor'})});const editor=card.locator('.gs26-card-editor');
 await editor.getByLabel('Data type',{exact:true}).selectOption('COMBUSTION_CHAMBER_PRESSURE');
 assert.equal(await editor.getByLabel('Board',{exact:true}).inputValue(),'DAQ');assert.equal(await editor.getByLabel('Channel',{exact:true}).inputValue(),'0');assert.equal(await editor.getByLabel('Unit label',{exact:true}).inputValue(),'psig');
 await editor.getByLabel('Decimal places',{exact:true}).selectOption('3');await editor.getByLabel('Display range',{exact:true}).selectOption({label:'0 to 500'});
 await editor.getByLabel('Section',{exact:true}).selectOption('Avionics');
 assert.equal(await editor.getByLabel('Title',{exact:true}).inputValue(),'Chamber monitor');
 await page.getByRole('button',{name:'Done',exact:true}).click();await page.reload();await page.locator('.gs26-card-label').getByText('Chamber monitor',{exact:true}).waitFor();
 await page.getByRole('button',{name:'Customize dashboard',exact:true}).click();await editor.waitFor();
 assert.equal(await editor.getByLabel('Data type',{exact:true}).inputValue(),'COMBUSTION_CHAMBER_PRESSURE');assert.equal(await editor.getByLabel('Section',{exact:true}).inputValue(),'Avionics');assert.equal(await editor.getByLabel('Decimal places',{exact:true}).inputValue(),'3');assert.equal(await editor.getByLabel('Display range',{exact:true}).locator('option:checked').innerText(),'0 to 500');
 await page.getByRole('button',{name:'Add camera',exact:true}).click();const camera=page.locator('.gs26-card-editor').filter({has:page.getByLabel('Camera source',{exact:true})});assert.equal(await camera.locator('input').count(),1);assert.equal(await camera.getByLabel('Title',{exact:true}).count(),1);
 for(const width of[320,390,844,1440]){await page.setViewportSize({width,height:1000});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'editor fits '+width);}
 assert.deepEqual(commands,[],'card choices never send hardware commands');assert.deepEqual(errors,[]);await context.close();console.log('PASS actual Wasm: dropdown-only card choices except labels, source-dependent defaults, persisted section/channel/unit/range, camera editor and responsive widths');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
