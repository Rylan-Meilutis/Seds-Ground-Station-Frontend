// Build with `dx build --platform web`, then run with Node and Playwright.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import {createHash} from 'node:crypto';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root=path.resolve('target/dx/groundstation_frontend/debug/web/public');
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
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY || (process.platform==='darwin' && fs.existsSync('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome') ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' : undefined)});
try {
  const context=await browser.newContext({viewport:{width:1440,height:1000}});
  await context.addInitScript(({session,origin})=>{
    localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));
  },{session,origin});
  const page=await context.newPage(); const errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  const old=Date.now()-1000;
  const row=(t,kind,value)=>({timestamp_ms:t,data_type:kind,sender_id:'DAQ',values:[value]});
  recentRows=[row(old,'KG50',12345),row(old,'LOADCELL_50_WEIGHT_KG',9876)];
  await page.goto(origin);
  await page.getByText('9876.00 kg',{exact:true}).waitFor();
  recentRows=[row(old,'KG50',12345),row(old,'LOADCELL_50_WEIGHT_KG',12.5)];
  broadcast({ty:'CalibrationChanged'});
  await page.getByText('12.50 kg',{exact:true}).waitFor();
  assert.equal(await page.getByText('9876.00 kg',{exact:true}).count(),0,'calibration refresh replaces old derived values');
  holdRecent=true; broadcast({ty:'CalibrationChanged'});
  for(let i=0;i<100&&!heldResponse;i++)await page.waitForTimeout(50);
  assert(heldResponse,'test must interrupt an in-flight history fetch');
  await page.getByRole('button',{name:'Clear all graphs',exact:true}).click();
  const floor=await page.evaluate(()=>Object.entries(localStorage).filter(([k])=>k.includes('graph_history_floor_v1:')).map(([,v])=>JSON.parse(v))[0]);
  assert(floor[0]>old,'clear persists backend timestamp boundary');
  holdRecent=false; heldResponse.end(JSON.stringify(recentRows));
  await page.waitForTimeout(800);
  assert.equal(await page.getByText('12.50 kg',{exact:true}).count(),0,'late response cannot restore cleared data');
  await page.reload(); await page.waitForTimeout(1500);
  assert.equal(await page.getByText('12.50 kg',{exact:true}).count(),0,'reload cannot restore pre-clear history');
  recentRows.push(row(Date.now(),'LOADCELL_50_WEIGHT_KG',0.25));
  await page.reload(); await page.getByText('0.25 kg',{exact:true}).waitFor();
  assert.equal(await page.getByText('12.50 kg',{exact:true}).count(),0);
  assert.deepEqual(errors,[]);
  console.log('PASS: calibration refresh, clear during fetch, persisted cutoff, and new data after clear');
} finally {await browser.close();for(const socket of sockets)socket.destroy();await new Promise(resolve=>server.close(resolve));}
