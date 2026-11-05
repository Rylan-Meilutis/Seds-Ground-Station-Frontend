// Actual Wasm with synthetic chart/history APIs; no station or output commands.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/operation-workflow.mjs','utf8');
const boundary=source.indexOf('\ntry {\n const context=');
if(boundary<0)throw Error('Operation fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
fixture=fixture.replace("const dashboardLayout=example('layout.full');",`const dashboardLayout=example('layout.full');
const titles=['AV Bay battery','Fill box battery','Valve board battery','RF battery','Flight battery','Power board battery'];
const chartGroups=titles.map((title,index)=>({title,data_type:'PACKING_'+index,sender_id:'DAQ',channels:[0],labels:['Voltage']}));
dashboardLayout.data_tab.tabs.unshift({id:'PACKING',label:'Battery packing',channels:['Voltage'],chart:{enabled:true},chart_groups:chartGroups});
const packingHistory=[1,2,4,5].flatMap(index=>Array.from({length:40},(_,point)=>({timestamp_ms:Date.now()-4000+point*100,data_type:'PACKING_'+index,sender_id:'DAQ',values:[14+Math.sin(point/5)]})));`);
fixture=fixture.replace("  if(pathname.startsWith('/api/')){",`  if(pathname==='/api/recent'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(packingHistory));return;}
  if(pathname.startsWith('/api/')){`);
fixture+=`
try {
 const context=await browser.newContext({viewport:{width:1280,height:1000}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});const commands=[];await context.routeWebSocket(url=>url.pathname==='/ws',s=>s.onMessage(message=>commands.push(message)));
 const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 async function open(){await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();await page.getByRole('link',{name:dashboardLayout.branding.tab_labels.data||'Data',exact:true}).click();await page.getByRole('button',{name:'Battery packing',exact:true}).click();await page.locator('.gs26-info-panel canvas').first().waitFor();}
 await open();const panels=page.locator('.gs26-info-panel');await panels.filter({hasText:'No chart data yet.'}).first().waitFor();
 assert.equal(await panels.count(),6);const initial=await panels.evaluateAll(nodes=>nodes.map(n=>n.dataset.panelId));
 for(const width of[320,390,844,1280]){
  await page.setViewportSize({width,height:1000});await page.waitForTimeout(150);
  const boxes=await panels.evaluateAll(nodes=>nodes.map(n=>{const b=n.getBoundingClientRect();return{x:b.x,y:b.y,width:b.width,height:b.height}}));
  const columns=new Map();for(const box of boxes){const key=Math.round(box.x);if(!columns.has(key))columns.set(key,[]);columns.get(key).push(box);assert(box.x>=0&&box.x+box.width<=width+1,'card fits '+width);}
  for(const column of columns.values()){column.sort((a,b)=>a.y-b.y);for(let i=1;i<column.length;i++){const gap=column[i].y-column[i-1].y-column[i-1].height;assert(gap>=-1&&gap<=24,'no row-height gap at '+width+': '+gap);}}
  if(width===1280)assert(columns.size>=2,'desktop exercises multiple columns');
  assert.deepEqual(await panels.evaluateAll(nodes=>nodes.map(n=>n.dataset.panelId)),initial,'viewport must not reorder DOM');
  assert(await page.locator('.gs26-info-panels').evaluate(n=>n.scrollWidth<=n.clientWidth+1),'panel toolbar fits '+width);
  await page.screenshot({path:'/tmp/info-panel-packing-'+width+'.png'});
 }
 await page.getByRole('button',{name:'Arrange panels',exact:true}).click();await page.getByRole('button',{name:'Move Fill box battery up',exact:true}).click();await page.getByRole('checkbox',{name:/^RF battery/}).uncheck();await page.getByRole('button',{name:'Done arranging',exact:true}).click();
 const arranged=await panels.evaluateAll(nodes=>nodes.map(n=>n.dataset.panelId));assert.equal(arranged.length,5);assert.equal(arranged[0],initial[1]);
 await open();assert.deepEqual(await panels.evaluateAll(nodes=>nodes.map(n=>n.dataset.panelId)),arranged,'saved arrangement and hidden cards survive reload');
 assert.equal(commands.length,0,'viewing/arranging does not emit commands');assert.deepEqual(errors,[]);await context.close();console.log('PASS actual Wasm: mixed empty/tall charts pack without row gaps;320/390/844/1280 fit; DOM order stable; saved Arrange/hide survives reload; no commands');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
