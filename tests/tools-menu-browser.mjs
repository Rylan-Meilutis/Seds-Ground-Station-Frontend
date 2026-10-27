// Reuse the isolated navigation fixture; never contacts a station.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8'),boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture+=`try {
 session.username='launch_operator_'+'a'.repeat(40);
 const failures=[];
 for(const [width,height] of[[1440,1000],[844,390],[720,360],[320,568]]){
  const context=await browser.newContext({viewport:{width,height}});
  await context.addInitScript(({session,origin})=>localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]})),{session,origin});
  await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(()=>{}));
  const page=await context.newPage();await page.goto(origin+'/dashboard');
  const account=page.locator('.gs26-utility-menu button').filter({hasText:/SIGN OUT/});await account.waitFor({state:'attached'});
  const menu=page.getByRole('button',{name:'Menu',exact:true});if(await menu.isVisible())await menu.click();
  const tools=page.locator('.gs26-utility-menu');if(!await tools.evaluate(e=>e.open))await tools.locator('summary').click();
  await account.waitFor({state:'visible'});await account.scrollIntoViewIfNeeded();
  const bounds=await tools.locator(':scope > div').evaluate(e=>{const r=e.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom,scrollWidth:e.scrollWidth,clientWidth:e.clientWidth};});
  const button=await account.boundingBox(),content=await account.evaluate(e=>({text:e.textContent,scrollHeight:e.scrollHeight,clientHeight:e.clientHeight}));console.log(JSON.stringify({viewport:[width,height],bounds,account:button,content}));
  if(content.scrollHeight>content.clientHeight+1)failures.push('Account label clipped vertically '+width+'x'+height);
  if(button.x<0||button.x+button.width>width||button.y<0||button.y+button.height>height)failures.push('Account action outside viewport '+width+'x'+height);
  if(bounds.scrollWidth>bounds.clientWidth+1)failures.push('Tools content overflows '+width+'x'+height);
  assert.equal(await page.getByText(/queue drops/i).count(),0);
  await page.screenshot({path:'/tmp/tools-menu-'+width+'x'+height+'-20261027.png'});await context.close();
 }
 assert.deepEqual(failures,[]);console.log('PASS Tools account action remains visible and fits desktop/landscape/mobile with a long account name');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
