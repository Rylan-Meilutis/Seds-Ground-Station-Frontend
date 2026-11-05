// Actual Wasm on inert local station APIs; no physical command or hardware.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/operation-workflow.mjs','utf8');
const boundary=source.indexOf('\ntry {\n const context=');
if(boundary<0)throw Error('Operation fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
fixture=fixture.replace("roles:['admin'],permissions:{view_data:true,send_commands:true", "roles:['viewer'],permissions:{view_data:true,send_commands:false");
fixture=fixture.replace("const dashboardLayout=example('layout.full');", "const dashboardLayout=example('layout.full');const oldTab=JSON.parse(fs.readFileSync('src/telemetry_dashboard/igniter_sense_tab.json','utf8'));oldTab.channels.push('Current (unavailable)');oldTab.subtabs[0].channels.push('Current (unavailable)');oldTab.channel_formatters.push({suffix:' A'});dashboardLayout.data_tab.tabs.push(oldTab);let senseHistory=[];");
fixture=fixture.replace("  if(pathname.startsWith('/api/')){",`  if(pathname==='/api/recent'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(senseHistory));return;}
  if(pathname.startsWith('/api/')){`);
fixture+=`
try {
 const context=await browser.newContext({viewport:{width:1440,height:1000}});
 await context.addInitScript(({session,origin})=>{
  if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};
  localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));
 },{session,origin});
 const sockets=new Set(),commands=[],errors=[];
 await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});
 await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{sockets.add(socket);socket.onClose(()=>sockets.delete(socket));socket.onMessage(data=>commands.push(String(data)));});
 const page=await context.newPage();page.on('pageerror',error=>errors.push(error.message));
 const limbs=(value)=>[0n,16n,32n,48n].map(shift=>Number((BigInt(value)>>shift)&65535n));
 const rows=(sequence,stamp=Date.now(),counts=2048,volts=1.6504)=>{
  const owner=[4,0,0,0,sequence,0];
  return [{timestamp_ms:stamp,data_type:'IGNITER_SENSE_SIGNAL',sender_id:'AB',values:[counts,volts,null,...owner]},
   {timestamp_ms:stamp,data_type:'IGNITER_SENSE_STATUS',sender_id:'AB',values:[1,0,0,1,...owner,...limbs(stamp)]}];
 };
 const push=data=>{assert(sockets.size>0);for(const socket of sockets)socket.send(JSON.stringify({ty:'TelemetryBatch',data}));};
 async function open(){await page.getByRole('navigation',{name:'Mission navigation'}).waitFor({timeout:15000});await page.getByRole('link',{name:dashboardLayout.branding.tab_labels.data||'Data',exact:true}).click();await page.getByRole('button',{name:'Igniter current sense',exact:true}).click();await page.getByTestId('igniter-sense-panel').waitFor({timeout:10000}).catch(async error=>{console.log((await page.locator('body').innerText()).slice(0,3500));console.log(errors);throw error;});}
 await page.goto(origin+'/dashboard');await open();
 const panel=page.getByTestId('igniter-sense-panel');
 assert((await panel.innerText()).includes('ADC: —'),'missing source cannot fabricate zero');assert((await panel.innerText()).includes('No igniter ADC report received'),'missing report is explicit');assert.equal(await page.getByText('Current (unavailable)',{exact:true}).count(),0,'old empty amperes card removed; capability stays visible');
 push(rows(1));await panel.getByText('Live measured signal',{exact:true}).waitFor();
 assert((await panel.innerText()).includes('ADC: 2048 counts'));
 assert((await panel.innerText()).includes('Signal: 1.6504 V'));
 assert((await panel.innerText()).includes('Current: unavailable'));
 assert((await panel.innerText()).includes('no supported current conversion'));
 await page.screenshot({path:process.env.GS_IGNITER_SCREENSHOT?.replace('.png','-live.png')||'/tmp/igniter-sense-live.png',fullPage:true});
 await panel.getByText('Stale · last signal retained',{exact:true}).waitFor({timeout:4000});
 push([rows(2)[1]]);await panel.getByText('Waiting for matching ADC sample · last signal retained',{exact:true}).waitFor();
 push(rows(2,Date.now(),1111,0.8953));await panel.getByText('Live measured signal',{exact:true}).waitFor();
 assert((await panel.innerText()).includes('1111 counts'));
 push(rows(3,Date.now()-5000));await panel.getByText('Stale · last signal retained',{exact:true}).waitFor();
 push(rows(4,Date.now()+5000));await panel.getByText('Source clock unavailable · last signal retained',{exact:true}).waitFor();
 senseHistory=rows(5,Date.now()-5000);await page.reload();await open();
 await panel.getByText('ADC: 2048 counts',{exact:true}).waitFor({timeout:15000});
 await panel.getByText('Recorded or unavailable · no fresh live source',{exact:true}).waitFor();
 assert(!commands.some(command=>{try{const message=JSON.parse(command);return ['Abort','Launch','IgniterOn','DeployChutes'].includes(message.cmd);}catch{return false;}}),'read-only viewing emitted command');
 assert.deepEqual(errors,[]);
 await page.screenshot({path:process.env.GS_IGNITER_SCREENSHOT||'/tmp/igniter-sense-browser.png',fullPage:true});
 await context.close();console.log('PASS actualWasm igniter readout: legacy layout fallback, view-only user, raw/refvolts, amperesUnavailable, source pairing, stale/wire delay/clock skew/history, no commands');
} finally {await browser.close();await new Promise(resolve=>server.close(resolve));}
`;
try {await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
