// Actual app, synthetic V2 station-modes backend. Switching modes takes one
// click on Confirm in the review: nothing is typed. A current backend issues
// no phrase; an older one still gets its own phrase echoed back. The review
// must also stay open when the station answers within one UI clock tick.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8'),boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Shared browser fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
fixture=fixture.replace('let lists={',String.raw`
const contract=name=>JSON.parse(fs.readFileSync('tests/fixtures/backend-contract/'+name+'.json','utf8'));
let mode=contract('station-modes/snapshot'),legacyPhrase=null;const prepares=[],confirms=[];
let lists={`);
fixture=fixture.replace("  if(pathname==='/api/checklists'||",String.raw`
  if(pathname==='/api/station-modes'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(mode));return;}
  if(pathname==='/api/station-modes/prepare'){
    let text='';for await(const data of req)text+=data;const binding=JSON.parse(text);prepares.push(binding);
    const challenge={...contract('station-modes/prepare-in-process'),...binding,version:2,ticket:'ticket-'+prepares.length,transition_id:'switch-'+prepares.length,required_confirmation:legacyPhrase};
    res.setHeader('Content-Type','application/json');res.end(JSON.stringify(challenge));return;
  }
  if(pathname==='/api/station-modes/confirm'){
    let text='';for await(const data of req)text+=data;const body=JSON.parse(text);confirms.push(body);
    const accepted={...contract('station-modes/confirm-accepted'),transition:{...contract('station-modes/confirm-accepted').transition,id:body.transition_id}};
    res.setHeader('Content-Type','application/json');res.end(JSON.stringify(accepted));return;
  }
  if(pathname==='/api/checklists'||`);
fixture+=String.raw`try {
 const context=await browser.newContext({viewport:{width:390,height:844}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(a,b)=>{try{return new URL(a,b)}catch{return null}};localStorage.setItem('navigation-style','sidebar');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',()=>{});
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));page.setDefaultTimeout(12000);
 async function open(){await page.goto(origin+'/dashboard');await page.locator('.gs26-dashboard-shell').waitFor();const settings=page.getByRole('link',{name:'Settings',exact:true}).or(page.getByRole('button',{name:'Settings',exact:true}));if(!await settings.isVisible())await page.getByRole('button',{name:'Show navigation sidebar',exact:true}).click();await settings.click();await page.getByRole('button',{name:'Ground Station',exact:true}).click();await page.getByRole('combobox',{name:'Station operating mode',exact:true}).waitFor();}
 async function review(){
   const select=page.getByRole('combobox',{name:'Station operating mode',exact:true});
   await page.waitForFunction(()=>!document.querySelector('#station-operating-mode')?.disabled);await select.selectOption('hitl');
   await page.getByRole('button',{name:'Review mode change',exact:true}).click();
   const dialog=page.getByRole('alertdialog',{name:'Review station mode change'});await dialog.waitFor();
   assert.equal(await dialog.locator('input:not([type=checkbox]),textarea').count(),0,'nothing to type in the review');
   // The fixture answers at once; the review must survive several UI clock ticks.
   await page.waitForTimeout(1500);assert(await dialog.isVisible(),'review stays open for the operator');
   const confirm=dialog.getByRole('button',{name:'Confirm mode change',exact:true});assert(await confirm.isEnabled(),'Confirm is enabled without typing');
   await confirm.click();await dialog.waitFor({state:'detached'});
   await page.waitForFunction(n=>document.body.innerText.includes('Mode request:'),null);
   return confirms.at(-1);
 }
 await open();
 let body=await review();
 assert.equal(confirms.length,1);assert.equal(body.ticket,'ticket-1');assert(!body.confirmation,'current backend: no phrase is sent: '+JSON.stringify(body));
 for(const key of ['backend_instance','principal_context','authority_id','revision','policy_epoch','current_id','target_id'])assert.equal(body[key],prepares.at(-1)[key],key);
 // An older backend still issuing a phrase gets it echoed; the operator still types nothing.
 legacyPhrase='SWITCH TO HITL';await open();body=await review();
 assert.equal(confirms.length,2);assert.equal(body.confirmation,'SWITCH TO HITL');
 for(const width of [320,1280]){await page.setViewportSize({width,height:900});assert(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth+1),'no horizontal scroll at '+width);}
 assert.deepEqual(errors,[]);await context.close();console.log('PASS station mode switch: one-click confirm, no typed phrase, legacy phrase echoed, review survives a fast backend');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
