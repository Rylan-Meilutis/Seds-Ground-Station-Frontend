// Live telemetry ingest and render cost against a mock station, for
// before/after comparisons of the transport. Not part of the pass/fail suite.
//
//   GS_UI_BUNDLE=target/dx/groundstation_frontend/release/web/public \
//   node tests/live-ingest-benchmark.mjs [seconds] [rows_per_second] [runs]
//
// Streams TelemetryBatch frames (50 ms batches, a flight-like mix of channels)
// over /ws while the Dashboard and then the Data tab are shown, and reports the
// renderer's main-thread task and script time (Chrome Performance metrics),
// animation frames delivered and JS heap, per run and as medians.
import fs from 'node:fs';
import http from 'node:http';
import {createHash} from 'node:crypto';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const root=path.resolve(process.env.GS_UI_BUNDLE||'target/dx/groundstation_frontend/release/web/public');
const [seconds=10,rate=2000,runs=3]=process.argv.slice(2).map(Number);
const example=name=>JSON.parse(fs.readFileSync(`docs/api-examples/${name}.json`,'utf8'));
const session={...example('auth-session.anonymous'),authenticated:true,anonymous:false,username:'bench',roles:[],permissions:{view_data:true,send_commands:false}};
const layout=example('layout.full');
const sockets=new Set();
const server=http.createServer((req,res)=>{
  const pathname=new URL(req.url,'http://localhost').pathname;
  if(pathname.startsWith('/api/')){
    res.setHeader('Content-Type','application/json');
    res.end(JSON.stringify(pathname==='/api/auth/session'?session:pathname==='/api/layout'?layout:pathname==='/api/system/time'?{utc_ms:Date.now()}:pathname==='/api/vehicle'?{}:[]));return;
  }
  const relative=pathname==='/'||pathname==='/dashboard'?'index.html':pathname.slice(1);
  const file=path.resolve(root,relative);
  if(!file.startsWith(root+path.sep)||!fs.existsSync(file)||!fs.statSync(file).isFile()){res.statusCode=404;res.end();return;}
  const mime={'.html':'text/html','.js':'text/javascript','.wasm':'application/wasm','.css':'text/css','.svg':'image/svg+xml','.png':'image/png'};
  res.setHeader('Content-Type',mime[path.extname(file)]||'application/octet-stream');res.end(fs.readFileSync(file));
});
function send(socket,msg){
  const body=Buffer.from(JSON.stringify(msg));
  let header;
  if(body.length<126)header=Buffer.from([0x81,body.length]);
  else if(body.length<65536)header=Buffer.from([0x81,126,body.length>>8,body.length&255]);
  else{header=Buffer.alloc(10);header[0]=0x81;header[1]=127;header.writeBigUInt64BE(BigInt(body.length),2);}
  socket.write(Buffer.concat([header,body]));
}
server.on('upgrade',(req,socket)=>{
  if(!req.url.startsWith('/ws')){socket.destroy();return;}
  const accept=createHash('sha1').update(req.headers['sec-websocket-key']+'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
  socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
  sockets.add(socket);socket.on('close',()=>sockets.delete(socket));socket.on('error',()=>sockets.delete(socket));socket.on('data',()=>{});
  send(socket,{ty:'NetworkTime',data:{timestamp_ms:Date.now()}});
});
await new Promise(r=>server.listen(0,'127.0.0.1',r));
const origin=`http://127.0.0.1:${server.address().port}`;
// A flight-like mix: accelerometer at half the rate, the rest shared.
const mix=[['FC','ACCEL_DATA',3,0.5],['DAQ','LOADCELL_WEIGHT_KG',1,0.15],['DAQ','COMBUSTION_CHAMBER_PRESSURE',1,0.15],['FC','GPS_DATA',3,0.05],['VB','VALVE_STATE',3,0.05],['FC','BAROMETER_DATA',3,0.1]];
let seq=0;
function batch(n,now){
  const rows=[];
  for(let i=0;i<n;i++){
    let pick=(seq*0.618034)%1,acc=0,chosen=mix[0];
    for(const m of mix){acc+=m[3];if(pick<acc){chosen=m;break;}}
    const [sender,ty,count]=chosen;const t=now-50+Math.floor(i*50/n);
    rows.push({timestamp_ms:t,sender_id:sender,data_type:ty,values:Array.from({length:count},(_,k)=>ty==='GPS_DATA'?(k===0?31.758+seq*1e-7:k===1?-102.046:900+seq%400):ty==='VALVE_STATE'?(seq>>(k+6))&1:Math.sin(seq/50+k)*100)});
    seq++;
  }
  return rows;
}
const median=a=>{const s=[...a].sort((x,y)=>x-y);return s[Math.floor(s.length/2)];};
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
const results=[];
try{
  for(let run=0;run<runs;run++){
    const context=await browser.newContext({viewport:{width:1440,height:1000}});
    await context.addInitScript(({session,origin})=>{localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'bench-token',session,remember_me:true}}]}));
      window.__frames=0;const tick=()=>{window.__frames++;requestAnimationFrame(tick);};requestAnimationFrame(tick);},{session,origin});
    const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
    const cdp=await context.newCDPSession(page);await cdp.send('Performance.enable');
    await page.goto(origin);
    while(!sockets.size)await page.waitForTimeout(20);
    await page.waitForTimeout(1500);
    const phase=async(label)=>{
      const metric=async()=>Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map(m=>[m.name,m.value]));
      const before=await metric();const f0=await page.evaluate(()=>window.__frames);
      const per=Math.max(1,Math.round(rate/20));const end=Date.now()+seconds*1000;let sent=0;
      while(Date.now()<end){const rows=batch(per,Date.now());for(const s of sockets)send(s,{ty:'TelemetryBatch',data:rows});sent+=rows.length;await new Promise(r=>setTimeout(r,50));}
      await page.waitForTimeout(500);
      if(process.env.GS_BENCH_SHOT&&run===0)await page.screenshot({path:`${process.env.GS_BENCH_SHOT}-${label}.png`});
      const after=await metric();const f1=await page.evaluate(()=>window.__frames);
      return {phase:label,rows:sent,task_ms:Math.round((after.TaskDuration-before.TaskDuration)*1000),script_ms:Math.round((after.ScriptDuration-before.ScriptDuration)*1000),layout_ms:Math.round((after.LayoutDuration-before.LayoutDuration)*1000),frames:f1-f0,heap_mb:+(after.JSHeapUsedSize/1048576).toFixed(1)};
    };
    const out=[await phase('dashboard')];
    await page.getByRole('navigation').getByRole('link',{name:'Telemetry',exact:true}).first().click();
    await page.waitForTimeout(800);
    out.push(await phase('data'));
    if(errors.length)console.error('page errors',errors);
    results.push(out);console.log(JSON.stringify(out));
    await context.close();
  }
  const summary={};
  for(const label of ['dashboard','data']){
    const rows=results.map(r=>r.find(p=>p.phase===label));
    summary[label]={rows:rows[0].rows,task_ms:median(rows.map(r=>r.task_ms)),script_ms:median(rows.map(r=>r.script_ms)),layout_ms:median(rows.map(r=>r.layout_ms)),frames:median(rows.map(r=>r.frames)),heap_mb:median(rows.map(r=>r.heap_mb))};
  }
  console.log('SUMMARY '+JSON.stringify({seconds,rate,runs,summary}));
}finally{await browser.close();server.close();}
