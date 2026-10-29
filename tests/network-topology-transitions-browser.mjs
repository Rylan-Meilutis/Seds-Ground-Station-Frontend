// Actual Wasm graph transitions through isolated HTTP/WebSocket snapshots; no station contact.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/navigation-checklists.mjs','utf8'),boundary=source.indexOf('try {\n const context=');
if(boundary<0)throw Error('Navigation fixture boundary changed');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright')));
fixture=fixture.replace('const server=http.createServer',`let topology={generated_ms:Date.now(),nodes:[],links:[]};
const server=http.createServer`).replace("if(pathname==='/api/checklists'",`if(pathname==='/api/network_topology'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(topology));return;}
  if(pathname==='/api/checklists'`);
fixture+=`try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]})),{session,origin});
 const sockets=new Set();await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{sockets.add(socket);socket.onClose(()=>sockets.delete(socket));socket.onMessage(()=>{});socket.send(JSON.stringify({ty:'NetworkTopology',data:topology}));});
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));
 const node=(id,label)=>({id,label,kind:'board',status:'online',group:'av',sender_id:id.toUpperCase(),age_ms:20,last_seen_ms:Date.now()-20,endpoints:[],detail:null});
 const nodes=[node('gs','Ground Station'),node('rrls','RRLS'),node('rf','AV RF'),node('fc','Flight Computer'),{...node('pb','Power Board'),status:'offline',age_ms:null,last_seen_ms:null}];
 const link=(source,target,status='online',label='direct')=>({source,target,status,label});
 topology={generated_ms:Date.now(),nodes,links:[link('gs','rf'),link('gs','rrls','online','standby'),link('rf','fc')]};
 await page.goto(origin+'/dashboard');await page.getByRole('link',{name:'Topology',exact:true}).click();
 const graph=page.locator('#network-topology-canvas');await graph.waitFor({state:'visible'});
 const edgeNames=async()=>graph.locator('svg > g > title').evaluateAll(titles=>titles.map(e=>e.textContent.split(': upload lane')[0].split(' -> ').sort().join(' ↔ ')));
 const expected=pairs=>pairs.map(pair=>[...pair].sort().join(' ↔ ')).sort();
 const waitEdges=async(pairs)=>{const value=expected(pairs);await page.waitForFunction(({value})=>JSON.stringify([...document.querySelectorAll('#network-topology-canvas svg > g > title')].map(e=>e.textContent.split(': upload lane')[0].split(' -> ').sort().join(' ↔ ')).sort())===JSON.stringify(value),{value});assert.deepEqual((await edgeNames()).sort(),value);};
 const update=async(snapshot)=>{topology={...snapshot,generated_ms:Date.now()};assert.ok(sockets.size,'actual dashboard WebSocket connected');for(const socket of sockets)socket.send(JSON.stringify({ty:'NetworkTopology',data:topology}));};
 const record=async(stage)=>{const edges=await edgeNames();console.log(JSON.stringify({stage,edges,nodes:await graph.locator('[data-network-node]').allTextContents()}));await page.screenshot({path:'/tmp/network-topology-'+stage+'-20261027.png'});};
 await waitEdges([['Ground Station','AV RF'],['Ground Station','RRLS'],['AV RF','Flight Computer']]);await record('direct');
 await update({...topology,links:[link('gs','rrls','online','relay'),link('rrls','rf','online','relay'),link('rf','fc')]});
 await waitEdges([['Ground Station','RRLS'],['RRLS','AV RF'],['AV RF','Flight Computer']]);await record('relay-only');
 assert.ok(!(await edgeNames()).some(edge=>edge==='AV RF ↔ Ground Station'),'relay graph must remove direct AV-GS edge');
 // Same IDs and links: status/age-only updates must not remain trapped in cached layout.
 await update({...topology,nodes:nodes.map(n=>n.id==='rf'?{...n,status:'offline',age_ms:40000,last_seen_ms:Date.now()-40000}:n),links:topology.links.map(l=>l.target==='rf'?{...l,status:'offline'}:l)});
 const rf=graph.locator('[data-network-node]').filter({hasText:'AV RF'}),pb=graph.locator('[data-network-node]').filter({hasText:'Power Board'});
 await page.waitForFunction(()=>[...document.querySelectorAll('#network-topology-canvas [data-network-node]')].some(e=>e.textContent.includes('AV RF')&&e.textContent.includes('Offline')&&e.textContent.includes('Seen 40s ago')));
 assert.match(await pb.textContent(),/Never observed/);const staleEdges=await graph.locator('svg > g').evaluateAll(groups=>groups.filter(g=>g.querySelector('title')?.textContent.includes('RRLS')&&g.querySelector('title')?.textContent.includes('AV RF')).map(g=>[...g.querySelectorAll('line')].map(line=>({stroke:line.getAttribute('stroke'),dash:line.getAttribute('stroke-dasharray')}))));assert.ok(staleEdges.flat().some(line=>line.dash==='8 8'&&line.stroke==='#ef4444'),'offline observed edge must become red and dashed');await record('stale-and-unknown');
 await update({...topology,nodes,links:[link('gs','rf'),link('gs','rrls','online','standby'),link('rf','fc')]});await waitEdges([['Ground Station','AV RF'],['Ground Station','RRLS'],['AV RF','Flight Computer']]);await record('restored');
 assert.ok(!(await edgeNames()).some(edge=>edge==='AV RF ↔ RRLS'),'restored direct snapshot must remove old relay edge');
 await update({...topology,links:[]});await waitEdges([]);await record('unknown-no-links');assert.equal(await graph.locator('[data-network-node]').count(),5,'isolated nodes remain without fabricated links');
 // Exercise mounted Dioxus controls and teardown, in addition to isolated controller tests.
 await page.getByRole('button',{name:'Zoom In',exact:true}).click();
 const zoomed=await page.evaluate(()=>window.__gs26NetworkGraphState.scale);
 await page.setViewportSize({width:1280,height:900});await page.waitForTimeout(100);
 assert.equal(await page.evaluate(()=>window.__gs26NetworkGraphState.scale),zoomed,'resize retains operator zoom');
 await page.getByRole('button',{name:'Fullscreen',exact:true}).click();
 await page.locator('#network-topology-canvas-fullscreen').waitFor({state:'visible'});
 await page.waitForFunction(()=>window.__gs26NetworkGraphState?.canvas?.id==='network-topology-canvas-fullscreen');
 await page.getByRole('button',{name:'Exit Fullscreen',exact:true}).click();
 await graph.waitFor({state:'visible'});await page.waitForFunction(()=>window.__gs26NetworkGraphState?.canvas?.id==='network-topology-canvas');
 await page.getByRole('link',{name:'Telemetry',exact:true}).click();
 await page.waitForFunction(()=>!window.__gs26NetworkGraphState);
 await page.getByRole('link',{name:'Topology',exact:true}).click();await graph.waitFor({state:'visible'});
 await page.waitForFunction(()=>!!window.__gs26NetworkGraphState?.listenersInstalled);
 await page.getByRole('button',{name:'Zoom In',exact:true}).click();
 assert.ok(Number.isFinite(await page.evaluate(()=>window.__gs26NetworkGraphState.scale)),'remounted controls remain valid');
 assert.deepEqual(errors,[]);console.log('PASS actual Wasm direct→relay-only→stale/unknown→direct restore→no-links graph transitions without page reload');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
