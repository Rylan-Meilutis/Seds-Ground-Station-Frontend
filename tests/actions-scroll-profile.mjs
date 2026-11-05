// Actual Wasm CPU/frame profile with inert localhost APIs. Not an iPhone FPS claim.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/operation-workflow.mjs','utf8');
const boundary=source.indexOf('\ntry {\n const context=');
if(boundary<0)throw Error('Operation fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
fixture=fixture.replace('const server=http.createServer',`dashboardLayout.actions_tab.actions=Array.from({length:60},(_,index)=>({label:'Instrument control '+(index+1),cmd:'ScrollProfile'+index,group:'Instrumentation',border:'#38bdf8',bg:'#172638',fg:'#fff'}));
actionPolicy.controls=Array.from({length:60},(_,index)=>({cmd:'ScrollProfile'+index,enabled:true,actuated:null,blink:'none'}));
const server=http.createServer`);
fixture+=`
try {
 const context=await browser.newContext({viewport:{width:1280,height:900},isMobile:true,hasTouch:true});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',socket=>socket.onMessage(data=>wsCommands.push(String(data))));
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));await page.goto(origin+'/dashboard');
 await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();await page.getByRole('link',{name:'Actions',exact:true}).click();await page.setViewportSize({width:390,height:844});
 await page.getByText('Instrument control 60',{exact:true}).waitFor();await page.waitForTimeout(1200);
 const client=await context.newCDPSession(page);await client.send('Performance.enable');await client.send('Profiler.enable');await client.send('Profiler.setSamplingInterval',{interval:500});await client.send('Emulation.setCPUThrottlingRate',{rate:4});
 const before=(await client.send('Performance.getMetrics')).metrics;await client.send('Profiler.start');
 const frames=await page.evaluate(async()=>{let node=document.querySelector('[data-dispatch-command="ScrollProfile0"]');while(node&&node.scrollHeight<=node.clientHeight+1)node=node.parentElement;if(!node)throw Error('Actions scroll owner missing');const gaps=[];let prior=performance.now();for(let index=0;index<240;index++){await new Promise(requestAnimationFrame);const now=performance.now();gaps.push(now-prior);prior=now;node.scrollTop=(index%120)/119*(node.scrollHeight-node.clientHeight);}return{gaps,scrollRange:node.scrollHeight-node.clientHeight,scrollTop:node.scrollTop};});
 const profile=(await client.send('Profiler.stop')).profile;const after=(await client.send('Performance.getMetrics')).metrics;
 const metrics=Object.fromEntries(after.map(metric=>[metric.name,metric.value-(before.find(before=>before.name===metric.name)?.value||0)]));
 const names=new Map(profile.nodes.map(node=>[node.id,node.callFrame.functionName]));const actionSamples=(profile.samples||[]).filter(id=>/ActionsTab|actions_tab/.test(names.get(id)||'')).length;
 const sorted=[...frames.gaps].sort((a,b)=>a-b);const report={bundle:process.env.GS_UI_BUNDLE||'current',cpuThrottle:4,frameCount:frames.gaps.length,p50ms:sorted[Math.floor(sorted.length*.5)],p95ms:sorted[Math.floor(sorted.length*.95)],over50ms:frames.gaps.filter(gap=>gap>50).length,scriptSeconds:metrics.ScriptDuration,taskSeconds:metrics.TaskDuration,layoutSeconds:metrics.LayoutDuration,actionSamples,totalSamples:profile.samples?.length,scrollRange:frames.scrollRange};
 assert(frames.scrollRange>1000&&frames.scrollTop>500,'real long Actions surface scrolls');assert(!wsCommands.some(raw=>{try{return Boolean(JSON.parse(raw).cmd)}catch{return false}}),'scrolling must not send commands');assert.deepEqual(errors,[]);
 if(process.env.GS_SCROLL_REPORT)fs.writeFileSync(process.env.GS_SCROLL_REPORT,JSON.stringify(report,null,2)+'\\n');console.log(JSON.stringify(report));await context.close();
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
`;
if(process.env.GS_DUMP_FIXTURE)fs.writeFileSync(process.env.GS_DUMP_FIXTURE,fixture);else try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
