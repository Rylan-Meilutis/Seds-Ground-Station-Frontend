// Real Wasm UI with the synthetic board-recovery backend of
// board-recovery-browser.mjs. No hardware. Reset all boards opens its one
// server-issued confirmation directly (no intermediate dialog), and Recovery
// tools groups its commands into Boards / Reset / Launch readiness sections.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const base=fs.readFileSync('tests/board-recovery-browser.mjs','utf8'),cut=base.indexOf('\nfixture+=`try {');
if(cut<0)throw Error('Board recovery fixture boundary changed');
let prefix=base.slice(0,cut);
if(process.env.PLAYWRIGHT_MODULE)prefix=prefix.replace("import.meta.resolve('playwright')",JSON.stringify(pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href));
const body=String.raw`
fixture+=${'`'}try {
 const context=await browser.newContext({viewport:{width:1280,height:1000}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>{const beat=setInterval(()=>s.send(JSON.stringify({ty:'NetworkTime',data:{timestamp_ms:Date.now()}})),500);s.onClose(()=>clearInterval(beat));s.send(JSON.stringify({ty:'NetworkTopology',data:topology()}));});
 const page=await context.newPage(),errors=[],dialogs=[];page.on('pageerror',e=>errors.push(e.message));page.on('dialog',async d=>{dialogs.push(d.type());await d.dismiss();});
 // Every board reads back fresh and resettable, as AB does in the fixture.
 const template=JSON.parse(JSON.stringify(boardRecovery.boards.find(b=>b.id==='AB')));for(const b of boardRecovery.boards){Object.assign(b,JSON.parse(JSON.stringify(template)),{id:b.id,label:b.label});}
 const nav=async name=>{await page.evaluate(n=>[...document.querySelectorAll('a')].find(a=>(a.getAttribute('aria-label')||a.textContent.trim())===n)?.click(),name);};
 await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 await nav('Actions');const reset=page.getByRole('button',{name:'Reset all boards to Idle…',exact:true});await reset.waitFor();
 assert.equal(prepared.length,0,'opening Actions prepares nothing');
 // One click: straight to the single confirmation. No "Reset all boards" or "Recovery tools" dialog in between.
 await reset.click();const confirm=page.getByRole('alertdialog');await confirm.waitFor();
 assert.equal(await page.getByRole('dialog').count(),0,'no intermediate dialog');assert.equal(await page.getByRole('alertdialog').count(),1,'exactly one confirmation');
 assert.equal(prepared.length,1);assert.equal(prepared[0].action,'reset_startup');assert.equal(await confirm.locator('input').count(),0,'nothing to type');
 assert.match(await confirm.innerText(),/Outputs move/);
 for(const width of[320,390,1280]){await page.setViewportSize({width,height:900});assert(await confirm.evaluate(e=>{const r=e.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth&&e.scrollWidth<=e.clientWidth+1;}),'confirmation fits '+width);}
 await page.setViewportSize({width:1280,height:1000});
 await confirm.getByRole('button',{name:'Cancel',exact:true}).click();await confirm.waitFor({state:'detached'});
 await page.getByText('Reset cancelled; nothing was sent.',{exact:true}).waitFor();assert.equal(confirmed.length,0,'Cancel sends nothing');
 await reset.click();await confirm.waitFor();await confirm.getByRole('button',{name:'Confirm recovery action'}).click();
 await page.getByText(/Request accepted; awaiting fresh device confirmation/).waitFor();assert.equal(confirmed.length,1,'one confirmation, one reset');
 boardRecovery.operation=null;boardRecovery.confirmation=null;
 // A refused reset says why, inline, and opens nothing.
 const savedPolicy=boardRecovery.permissions.reset_startup;boardRecovery.permissions.reset_startup=false;const before=prepared.length;
 await reset.click();await page.getByText(/^Reset unavailable: /).waitFor();assert.equal(await page.getByRole('alertdialog').count(),0);assert.equal(prepared.length,before,'a refused reset prepares nothing');
 boardRecovery.permissions.reset_startup=savedPolicy;
 // Recovery tools: commands grouped per section.
 await nav('Connection Status');await nav('Connection');const panel=page.locator('.gs26-network-health');await panel.getByRole('button',{name:'Recovery tools',exact:true}).click();
 const tools=page.getByRole('dialog',{name:'Recovery tools',exact:true});await tools.waitFor();
 const tabs=await tools.getByRole('tab').allInnerTexts();assert.deepEqual(tabs,['Boards','Reset','Launch readiness']);
 const ab=tools.locator('[data-recovery-board="AB"]');await ab.waitFor();assert.match(await ab.innerText(),/abort latched/i);assert(await ab.getByRole('button',{name:'Query board'}).isVisible(),'per-board commands live under Boards');
 assert.equal(await tools.getByRole('button',{name:'Reset all boards to Idle…'}).count(),0,'reset is its own section');
 await tools.getByRole('tab',{name:'Reset',exact:true}).click();const toolsReset=tools.getByRole('button',{name:'Reset all boards to Idle…',exact:true});await toolsReset.waitFor();
 await page.waitForFunction(()=>![...document.querySelectorAll('#gs26-recovery-tools button')].find(b=>b.textContent==='Reset all boards to Idle…')?.disabled);
 await toolsReset.click();await confirm.waitFor();assert.equal(await page.getByRole('alertdialog').count(),1);await confirm.getByRole('button',{name:'Cancel',exact:true}).click();await confirm.waitFor({state:'detached'});
 await tools.getByRole('tab',{name:'Launch readiness',exact:true}).click();await tools.getByRole('button',{name:'Check launch readiness',exact:true}).waitFor();
 for(const width of[320,390]){await page.setViewportSize({width,height:850});assert(await tools.evaluate(e=>e.scrollWidth<=e.clientWidth+1),'tools fit '+width);}
 assert.deepEqual(dialogs,[]);assert.deepEqual(errors,[]);await context.close();
 console.log('PASS reset all boards: direct single confirmation from Actions, cancel sends nothing, refusal explained inline; Recovery tools grouped into Boards / Reset / Launch readiness');
}finally{await browser.close();await new Promise(r=>server.close(r));}${'`'};
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
`;
const moduleText=prefix+body;
await import('data:text/javascript;base64,'+Buffer.from(moduleText).toString('base64'));
