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
const actionPolicy=example('action-policy');
actionPolicy.controls=['ToggleAvBayUnderglow','ToggleFlightBuzzer'].map(cmd=>({cmd,enabled:true,blink:'none',actuated:false}));
dashboardLayout.actions_tab.actions=[['Underglow','ToggleAvBayUnderglow'],['FC Buzzer','ToggleFlightBuzzer']].map(([label,cmd])=>({label,cmd,border:'#38bdf8',bg:'#172638',fg:'#fff',group:'Avionics'}));
const commandRequests=[];
for(const id of ['LOADCELL','DAQ']) dashboardLayout.data_tab.tabs.push({id,label:id,channels:[],subtabs:[
  {id:'large',label:'1000 kg',data_type:'KG1000',channels:['Raw'],chart:{enabled:true},chart_groups:[{title:'Fill %',data_type:'LOADCELL_FILL_PERCENT',channels:[0]}],summary_items:[{label:'Fill %',data_type:'LOADCELL_FILL_PERCENT',index:0}]},
  {id:'small',label:'50 kg',data_type:'KG50',channels:['Raw'],chart:{enabled:true},chart_groups:[],summary_items:[]}
]});

const server=http.createServer((req,res)=>{
  const pathname=new URL(req.url,'http://localhost').pathname;
  if(pathname==='/test-camera') {res.setHeader('Content-Type','text/html');res.end('<p>Live camera fixture</p>');return;}
  if(pathname==='/radio'||pathname==='/media') {res.setHeader('Content-Type','text/html');res.end(`<p>Media tool fixture</p><script>window.received=[];setTimeout(()=>addEventListener('message',e=>{if(e.source===parent)received.push(e.data)}),900)</script>`);return;}
  if(pathname==='/api/action_policy'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(actionPolicy));return;}
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
  socket.on('data',buffer=>{try{let offset=2,length=buffer[1]&127;if(length===126){length=buffer.readUInt16BE(2);offset=4;}if(length===127)return;const mask=buffer.subarray(offset,offset+4);offset+=4;const body=Buffer.from(buffer.subarray(offset,offset+length));for(let i=0;i<body.length;i++)body[i]^=mask[i%4];const msg=JSON.parse(body.toString());if(msg.cmd)commandRequests.push(msg.cmd);}catch{}});
  send(socket,{ty:'ActionPolicy',data:actionPolicy});
  send(socket,{ty:'NetworkTime',data:{timestamp_ms:Date.now()}});
});
const broadcast=msg=>{for(const socket of sockets)send(socket,msg);};
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin=`http://127.0.0.1:${server.address().port}`;

const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
const measurements=[];
try {
 const context=await browser.newContext({viewport:{width:1280,height:900}});
 await context.addInitScript(({session,origin})=>{localStorage.setItem('navigation-style','tabs');localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));page.setDefaultTimeout(8000);
 await page.goto(origin);await page.getByRole('button',{name:'Customize dashboard',exact:true}).click();
 const card=page.locator('.gs26-dashboard-card').filter({has:page.getByText('50 kg loadcell',{exact:true})});
 await card.getByRole('button',{name:'Pin to top bar',exact:true}).click();await page.getByRole('button',{name:'Done',exact:true}).click();
 const strip=page.locator('[aria-label="Pinned telemetry"]'),point=strip.locator(':scope > div').first();await strip.waitFor();
 async function reading(value){const now=Date.now();recentRows=[{data_type:'LOADCELL_50_WEIGHT_KG',sender_id:'DAQ',values:[value],timestamp_ms:now,received_timestamp_ms:now}];broadcast({ty:'TelemetryBatch',data:recentRows});await page.waitForFunction(expected=>document.querySelector('[aria-label="Pinned telemetry"] .gs26-card-value')?.textContent.trim().replace(/\s+/g,' ')===expected,value.toFixed(2)+' kg');await page.waitForTimeout(70);return point.evaluate(element=>{const value=element.querySelector('.gs26-card-value'),unit=element.querySelector('.gs26-pinned-unit'),number=element.querySelector('.gs26-pinned-number');const rect=e=>{const r=e.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height};};return{point:rect(element),value:rect(value),unit:unit?rect(unit):null,number:number?rect(number):null,text:value.textContent,scrollWidth:value.scrollWidth,clientWidth:value.clientWidth};});}
 for(const width of [1280,390,320]){
  await page.setViewportSize({width,height:900});const rows=[];
  for(const value of [0,20,0,-20,123456.78,-123456.78,0])rows.push(await reading(value));
  measurements.push({kind:'pinned',width,rows});if(process.env.GS_LAYOUT_RESULTS)fs.writeFileSync(process.env.GS_LAYOUT_RESULTS,JSON.stringify({measurements,phase:'pinned'},null,2)+'\n');
  assert(rows.every(row=>Math.abs(row.point.width-rows[0].point.width)<0.1),'pinned point width stays fixed at '+width);
  assert(rows.every(row=>row.number&&Math.abs(row.number.width-rows[0].number.width)<0.1),'numeric slot fixed at '+width);
  assert(rows.every(row=>row.unit&&Math.abs(row.unit.x-rows[0].unit.x)<0.1),'unit stays fixed at '+width);
  assert(rows.every(row=>row.number.width>0&&row.text.includes('kg')),'full precision and unit remain present');
  const beforeOverflow=rows[0];const large=2**54;const overflow=await reading(large);
  assert.equal(overflow.text.trim().replace(/\s+/g,' '),large.toFixed(2)+' kg','full finite overflow value keeps originalprecision');
  assert(Math.abs(overflow.point.width-beforeOverflow.point.width)<0.1&&Math.abs(overflow.unit.x-beforeOverflow.unit.x)<0.1,'overflow does not resize point or move unit');
  const scroll=await point.locator('.gs26-pinned-number').evaluate(element=>{const box=element.getBoundingClientRect(),text=element.firstChild;const charBox=index=>{const range=document.createRange();range.setStart(text,index);range.setEnd(text,index+1);const r=range.getBoundingClientRect();return{x:r.x,right:r.right};};element.scrollLeft=0;const first=charBox(0);element.scrollLeft=element.scrollWidth;const last=charBox(text.textContent.length-1);const result={clientWidth:element.clientWidth,scrollWidth:element.scrollWidth,firstVisible:first.x>=box.x-.1&&first.right<=box.right+.1,lastVisible:last.x>=box.x-.1&&last.right<=box.right+.1,scrollLeft:element.scrollLeft};element.scrollLeft=0;return result;});
  measurements.push({kind:'true-overflow',width,overflow,scroll});if(process.env.GS_LAYOUT_RESULTS)fs.writeFileSync(process.env.GS_LAYOUT_RESULTS,JSON.stringify({measurements,phase:'overflow'},null,2)+'\n');
  assert(scroll.scrollWidth>scroll.clientWidth&&scroll.firstVisible&&scroll.lastVisible,'overflow number is fully readable through horizontal scroll');
  await reading(0);
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'page contains pinned strip at '+width);
 }
 await page.getByRole('button',{name:/Choose tab/}).click();await page.getByRole('button',{name:'Actions',exact:true}).click();
 for(const width of [1280,390,320]){
  await page.setViewportSize({width,height:900});
  for(const state of [{actuated:false,enabled:true},{actuated:true,enabled:true},{actuated:null,enabled:true},{actuated:false,enabled:false}]){
   for(const control of actionPolicy.controls){control.actuated=state.actuated;control.enabled=state.enabled;}
   broadcast({ty:'ActionPolicy',data:actionPolicy});await page.waitForTimeout(180);
   const buttons=[];
   for(const [command,label]of[['ToggleAvBayUnderglow','Underglow'],['ToggleFlightBuzzer','FC Buzzer']]){
    const button=page.locator(`button[data-dispatch-command="${command}"]`);await button.waitFor();
    const geometry=await button.evaluate((element,label)=>{const span=[...element.querySelectorAll('span')].find(e=>e.textContent===label);const r=element.getBoundingClientRect(),s=span.getBoundingClientRect();return{button:{x:r.x,y:r.y,width:r.width,height:r.height},label:{x:s.x,y:s.y,width:s.width,height:s.height},scrollWidth:element.scrollWidth,clientWidth:element.clientWidth,text:element.textContent,whiteSpace:getComputedStyle(span).whiteSpace};},label);
    assert(geometry.label.x>=geometry.button.x&&geometry.label.x+geometry.label.width<=geometry.button.x+geometry.button.width+0.1,'label contained '+label+'/'+width);
    assert(geometry.label.y>=geometry.button.y&&geometry.label.y+geometry.label.height<=geometry.button.y+geometry.button.height+0.1,'label height contained '+label+'/'+width);
    assert.equal(geometry.whiteSpace,'nowrap','cosmetic label remains intact');assert(geometry.scrollWidth<=geometry.clientWidth+1,'button content contained '+label+'/'+width);buttons.push(geometry);
   }
   if(width===320)assert(buttons[1].button.y>=buttons[0].button.y+buttons[0].button.height,'button row wraps into full controls');
   measurements.push({kind:'buttons',width,state,buttons});
  }
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no offscreen button at '+width);
 }
 assert.deepEqual(commandRequests,[],'layout inspection sends no commands');assert.deepEqual(errors,[]);
 if(process.env.GS_LAYOUT_RESULTS)fs.writeFileSync(process.env.GS_LAYOUT_RESULTS,JSON.stringify({measurements,commands:commandRequests,errors},null,2)+'\n');
 await context.close();console.log('PASS actual pinned geometry 0→20→0/negative/longer full-precision values and mobile-contained full Underglow/FC Buzzer labels, no commands');
}finally{await browser.close();for(const socket of sockets)socket.destroy();await new Promise(resolve=>server.close(resolve));}
