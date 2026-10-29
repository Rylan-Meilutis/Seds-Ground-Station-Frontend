// Build with `dx build --platform web`, then run with Node and Playwright.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import {createHash} from 'node:crypto';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const root=path.resolve(process.env.GS_UI_BUNDLE || 'target/dx/groundstation_frontend/debug/web/public');
const example=name=>JSON.parse(fs.readFileSync(`docs/api-examples/${name}.json`,'utf8'));
const session={...example('auth-session.anonymous'),authenticated:true,anonymous:false,username:'ui-test',roles:['stream_master'],permissions:{view_data:true,send_commands:true}};
let recentRows=[]; let recentHits=0; let holdRecent=false; let heldResponse; const sockets=new Set(); let cameraOnline=true;
let fillSource='kg50';
const dashboardLayout=example('layout.full');
for(const id of ['LOADCELL','DAQ']) dashboardLayout.data_tab.tabs.push({id,label:id,channels:[],subtabs:[
  {id:'large',label:'1000 kg',data_type:'KG1000',channels:['Raw'],chart:{enabled:true},chart_groups:[{title:'Fill %',data_type:'LOADCELL_FILL_PERCENT',channels:[0]}],summary_items:[{label:'Fill %',data_type:'LOADCELL_FILL_PERCENT',index:0}]},
  {id:'small',label:'50 kg',data_type:'KG50',channels:['Raw'],chart:{enabled:true},chart_groups:[],summary_items:[]}
]});

const server=http.createServer((req,res)=>{
  const pathname=new URL(req.url,'http://localhost').pathname;
  if(pathname==='/test-camera') {res.setHeader('Content-Type','text/html');res.end('<p>Live camera fixture</p>');return;}
  if(pathname==='/radio'||pathname==='/media') {res.setHeader('Content-Type','text/html');res.end(`<p>Media tool fixture</p><script>window.received=[];setTimeout(()=>addEventListener('message',e=>{if(e.source===parent)received.push(e.data)}),900)</script>`);return;}
  if(pathname==='/api/recent'){recentHits++;res.setHeader('Content-Type','application/json');if(holdRecent){heldResponse=res;return;}res.end(JSON.stringify(recentRows));return;}
  if(pathname==='/api/system/time'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify({utc_ms:Date.now()}));return;}
  if(pathname.startsWith('/api/')){
    res.setHeader('Content-Type','application/json');
    const data=pathname==='/api/auth/session'?session:pathname==='/api/layout'?dashboardLayout:pathname==='/api/fill_targets'?{version:1,fill_source:fillSource,nitrogen:{target_mass_kg:10,target_pressure_psi:100},nitrous:{target_mass_kg:20,target_pressure_psi:200}}:pathname==='/api/live_streams'?{...example('live-streams'),can_manage_stream:session.roles.includes('stream_master'),program_url:'',can_preview_live:session.roles.includes('stream_master'),streams:[{id:'pad-wide',label:'Pad wide',url:'/test-camera?ticket=fixture',online:cameraOnline,kind:'webrtc'}]}:pathname==='/api/vehicle'?{}:[];
    res.end(JSON.stringify(data));return;
  }
  const relative=pathname==='/'||pathname==='/dashboard'?'index.html':pathname.slice(1);
  const file=path.resolve(root,relative);
  if(!file.startsWith(root+path.sep)||!fs.existsSync(file)||!fs.statSync(file).isFile()){res.statusCode=404;res.end();return;}
  const mime={'.html':'text/html','.js':'text/javascript','.wasm':'application/wasm','.css':'text/css','.svg':'image/svg+xml','.png':'image/png'};
  res.setHeader('Content-Type',mime[path.extname(file)]||'application/octet-stream');res.end(fs.readFileSync(file));
});
function send(socket, msg) {
  const body=Buffer.from(JSON.stringify(msg));
  const header=body.length<126?Buffer.from([0x81,body.length]):Buffer.from([0x81,126,body.length>>8,body.length&255]);
  socket.write(Buffer.concat([header,body]));
}
server.on('upgrade',(req,socket)=>{
  if(!req.url.startsWith('/ws?')){socket.destroy();return;}
  const accept=createHash('sha1').update(req.headers['sec-websocket-key']+'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
  socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
  sockets.add(socket);socket.on('close',()=>sockets.delete(socket));socket.on('error',()=>sockets.delete(socket));
  socket.on('data',()=>{});
  send(socket,{ty:'NetworkTime',data:{timestamp_ms:Date.now()}});
});
const broadcast=msg=>{for(const socket of sockets)send(socket,msg);};
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin=`http://127.0.0.1:${server.address().port}`;

const browser=await chromium.launch({headless:true});
try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(origin);await page.getByRole('button',{name:'Customize dashboard',exact:true}).waitFor();
 while(!sockets.size)await page.waitForTimeout(20);
 const card=page.locator('.gs26-dashboard-card').filter({has:page.getByText('50 kg loadcell',{exact:true})});
 await card.locator('.gs26-card-value').evaluate(e=>{window.changes=[];window.watch=new MutationObserver(()=>window.changes.push({at:performance.now(),text:e.textContent}));watch.observe(e,{subtree:true,characterData:true,childList:true});});
 let seq=0;const started=Date.now();let sent=0;
 const interval=setInterval(()=>{
  const timestamp_ms=Date.now();const rows=[{data_type:'LOADCELL_50_WEIGHT_KG',sender_id:'DAQ',values:[++seq],timestamp_ms,received_timestamp_ms:timestamp_ms},{data_type:'KG50',sender_id:'DAQ',values:[seq*1000],timestamp_ms,received_timestamp_ms:timestamp_ms}];
  if(seq%2===0)rows.push({data_type:'KG1000',sender_id:'DAQ',values:[seq],timestamp_ms});
  if(seq%8===0)rows.push({data_type:'IMU_DATA',sender_id:'FC',values:[1,2,3],timestamp_ms},{data_type:'BATTERY_VOLTAGE',sender_id:'PB',values:[14],timestamp_ms});
  sent+=rows.length;broadcast({ty:'TelemetryBatch',data:rows});
 },25);
 await page.waitForTimeout(Number(process.env.GS_GRAPH_STRESS_MS||10000));clearInterval(interval);await page.waitForTimeout(300);
 const updates=await page.evaluate(()=>changes);
 assert(updates.length>=seq*0.7,'dashboard follows real40Hz source instead of independent5Hz sampler');
 assert.equal(await card.locator('.gs26-card-value').innerText(),`${seq}.00 kg`,'last source sample visible');
 {
   await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Telemetry',exact:true}).click();
   await page.getByRole('button',{name:'Loadcell',exact:true}).click();
   broadcast({ty:'TelemetryBatch',data:Array.from({length:200},(_,i)=>({data_type:'LOADCELL_WEIGHT_KG',sender_id:'DAQ',timestamp_ms:Date.now()-5000+i*25,values:[Math.sin(i/10)*10]}))});
   const canvas=page.locator('canvas[id^="gs26-chart-canvas-"]').first();await canvas.waitFor();await canvas.scrollIntoViewIfNeeded();await page.waitForTimeout(150);
   for(const width of [390,1440,844]){
     await page.setViewportSize({width,height:900});await page.waitForTimeout(100);
     const size=await canvas.evaluate(e=>({backing:e.width,css:Math.round(e.getBoundingClientRect().width),signature:e.__gs26RenderSignature}));assert(size.css>0);assert.equal(size.backing,size.css);
     assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Telemetry fits viewport');
   }
   for(let i=0;i<5;i++){
     await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Dashboard',exact:true}).click();await page.waitForTimeout(80);
     assert.equal(await page.evaluate(()=>window.__gs26ChartCanvasCache?.size||0),0,'real Wasm use_drop releases unmounted graphs');
     await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Telemetry',exact:true}).click();await page.getByRole('button',{name:'Loadcell',exact:true}).click();await page.waitForTimeout(100);
   }
   await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Dashboard',exact:true}).click();
   const before=await page.locator('.gs26-dashboard-card').count();
   await page.getByRole('button',{name:'Customize dashboard',exact:true}).click();await page.getByRole('button',{name:'Add card',exact:true}).click();
   let added=page.locator('.gs26-dashboard-card').filter({has:page.getByText('New telemetry card',{exact:true})});
   await added.getByLabel(/^Display/).selectOption('trend');
   await added.getByLabel(/^Card width/).selectOption('full');
   await added.getByLabel('Title',{exact:true}).fill('Saved live trend');
   await page.getByRole('button',{name:'Done',exact:true}).click();
   assert.equal(await page.locator('.gs26-dashboard-card').count(),before+1);
   await page.reload();await page.getByText('Saved live trend',{exact:true}).waitFor();
   assert.equal(await page.locator('.gs26-dashboard-card').count(),before+1,'saved layout survives reload');
   await page.getByRole('button',{name:'Customize dashboard',exact:true}).click();
   await page.locator('.gs26-dashboard-card').filter({has:page.getByText('Saved live trend',{exact:true})}).getByRole('button',{name:'Remove',exact:true}).click();
   await page.getByRole('button',{name:'Done',exact:true}).click();assert.equal(await page.locator('.gs26-dashboard-card').count(),before);
   for(const socket of sockets)socket.destroy();await page.waitForTimeout(1500);
   for(let retry=0;retry<50&&!sockets.size;retry++)await page.waitForTimeout(100);
   assert(sockets.size,'actual telemetry WebSocket reconnected');
   broadcast({ty:'TelemetryBatch',data:[{data_type:'LOADCELL_50_WEIGHT_KG',sender_id:'DAQ',timestamp_ms:Date.now(),values:[999]}]});
   await page.locator('.gs26-dashboard-card').filter({has:page.getByText('50 kg loadcell',{exact:true})}).getByText('999.00 kg',{exact:true}).waitFor();
   broadcast({ty:'TelemetryBatch',data:[]});broadcast({ty:'TelemetryBatch',data:[{data_type:'LOADCELL_50_WEIGHT_KG',sender_id:'DAQ',timestamp_ms:Date.now()+1,values:[null]}]});
   await page.waitForTimeout(100);assert.deepEqual(errors,[]);
 }
 console.log(JSON.stringify({elapsed_ms:Date.now()-started,telemetry_rows_sent:sent,weight_samples_sent:seq,DOM_updates:updates.length,first:updates.slice(0,3),last:updates.slice(-3),errors},null,2));
}finally{await browser.close();for(const s of sockets)s.destroy();await new Promise(resolve=>server.close(resolve));}
