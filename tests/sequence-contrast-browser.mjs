// Actual sequence view in the current Wasm UI. Local synthetic observations only.
import fs from 'node:fs';
const original=fs.readFileSync('tests/sequence-progress-browser.mjs','utf8');
const boundary=original.indexOf('fixture+=`try {');if(boundary<0)throw Error('Sequence fixture boundary changed');
const setup=original.slice(0,boundary);
const body=String.raw`try {
 const observations=[];
 for(const theme of ['default','light']){
  const context=await browser.newContext({viewport:{width:390,height:900}});
  await context.addInitScript(({session,origin,theme})=>{localStorage.setItem('navigation-style','tabs');localStorage.setItem('gs_theme_preset',theme);localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin,theme});
  const page=await context.newPage();await page.goto(origin);await page.getByRole('button',{name:'Customize dashboard',exact:true}).waitFor();await page.getByRole('button',{name:/Choose tab/}).click();await page.locator('#dashboard-tab-picker').getByRole('button',{name:/Actions/}).click();
  const sample=(outcome,revision)=>({version:1,key:emptyKey,revision,kind:'nitrogen_test',outcome,origin_client:'remote-station',title:'Nitrogen test',message:'Watching tank pressure',current_step:'raise-0',next_step:'hold-0',steps:[{id:'raise-0',label:'Raise pressure',state:'watching',watch:[{id:'tank-pt',label:'Tank pressure',target:50,observed:42,unit:'psi',age_ms:0,valid_for_ms:2000,evidence:'measured'}]},{id:'hold-0',label:'Verify hold',state:'planned',watch:[]}],observed_age_ms:0,valid_for_ms:2000,cancel_allowed:true,details_complete:true});
  for(const [index,outcome]of ['running','paused'].entries()){
   terminal=[];active=[sample(outcome,index+1)];const modal=page.locator('.gs26-sequence-view');await modal.waitFor();await modal.locator('.gs26-sequence-summary').getByText(outcome==='running'?'In progress':'Paused',{exact:true}).waitFor();
   const results=await modal.evaluate(root=>{
    const parse=s=>s.match(/[\d.]+/g).map(Number);const blend=(fg,bg)=>{const a=fg[3]??1;return fg.slice(0,3).map((n,i)=>n*a+bg[i]*(1-a));};
    const background=el=>{const layers=[];for(let node=el;node;node=node.parentElement)layers.unshift(parse(getComputedStyle(node).backgroundColor));return layers.reduce((bg,fg)=>blend(fg,bg),[255,255,255]);};
    const lum=rgb=>rgb.map(n=>{n/=255;return n<=.04045?n/12.92:((n+.055)/1.055)**2.4;}).reduce((sum,n,i)=>sum+n*[.2126,.7152,.0722][i],0);
    return ['.gs26-sequence-summary strong','.gs26-sequence-summary span','.gs26-sequence-cancel','.gs26-sequence-eyebrow','.gs26-sequence-current h3','.gs26-sequence-watch small','.gs26-sequence-legend','.gs26-sequence-steps small'].map(selector=>{const el=root.querySelector(selector);if(!el)throw Error('Missing actual text '+selector);const bg=background(el),fg=blend(parse(getComputedStyle(el).color),bg),a=lum(fg),b=lum(bg);return {selector,foreground:fg,background:bg,contrast:(Math.max(a,b)+.05)/(Math.min(a,b)+.05)};});
   });
   for(const value of results)assert(value.contrast>=4.5,theme+' '+outcome+' '+value.selector+' contrast '+value.contrast);observations.push({theme,outcome,results});
   assert(await modal.getByRole('button',{name:'Cancel sequence',exact:true}).isEnabled());assert(await modal.evaluate(e=>e.getBoundingClientRect().width<=innerWidth));
  }
  await context.close();active=[];terminal=[];
 }
 assert.equal(cancelBodies.length,0);assert.equal(genericCommands,0);fs.writeFileSync(process.env.GS_SEQUENCE_CONTRAST_RESULT||'/tmp/sequence-contrast.json',JSON.stringify(observations,null,2));console.log('PASS actualWASM dark/light running+paused sequence summary/cancel/watch/legend/steps contrast>=4.5');
}catch(error){console.error('CONTRAST_CASE_ERROR '+error.message);throw error;}finally{await browser.close();server.closeAllConnections();server.close();}`;
try{await import('data:text/javascript;base64,'+Buffer.from(setup+'fixture+='+JSON.stringify(body)+';await import("data:text/javascript;base64,"+Buffer.from(fixture).toString("base64"));').toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
