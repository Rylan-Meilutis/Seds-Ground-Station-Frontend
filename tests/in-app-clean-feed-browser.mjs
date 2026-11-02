// Actual Wasm against isolated synthetic services; no station commands.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8');
const boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("program_url:''","program_url:'/program?ticket=fixture'").replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture+=`try {
 session.roles=['stream_master'];
 const context=await browser.newContext({viewport:{width:1280,height:900}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(()=>{}));
 const page=await context.newPage();const errors=[],popups=[],dialogs=[];page.on('pageerror',e=>errors.push(e.message));page.on('popup',p=>popups.push(p.url()));page.on('dialog',d=>{dialogs.push(d.type());d.dismiss();});
 await page.goto(origin+'/dashboard');await page.getByRole('link',{name:'Stream Manager',exact:true}).click();await page.getByRole('heading',{name:'Broadcast studio',exact:true}).waitFor();
 await page.getByText('OBS browser source',{exact:true}).click();
 assert.equal(await page.getByRole('link',{name:'Open clean feed',exact:true}).count(),0,'feed uses an in-app control, never a new browser tab');
 const before=page.url();await page.getByRole('button',{name:'Open clean feed',exact:true}).click();
 const modal=page.getByRole('dialog',{name:'Clean broadcast feed',exact:true});await modal.waitFor();assert.equal(await modal.locator('iframe').getAttribute('src'),origin+'/program?ticket=fixture&clean=1');
 assert.equal(page.url(),before);assert.equal(context.pages().length,1);assert.deepEqual(popups,[]);assert.deepEqual(dialogs,[]);
 await modal.getByRole('button',{name:'Close clean feed',exact:true}).click();await modal.waitFor({state:'detached'});await page.getByRole('heading',{name:'Broadcast studio',exact:true}).waitFor();assert.deepEqual(errors,[]);
 await context.close();console.log('PASS actual WASM clean feed opens and closes in app; no popup, browser dialog or top-level redirect');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
