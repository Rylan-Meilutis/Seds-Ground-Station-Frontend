// Actual Wasm notification behavior; synthetic API/socket only, no hardware.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
const source=fs.readFileSync('tests/operation-workflow.mjs','utf8');
const boundary=source.indexOf('\ntry {\n const context=');
if(boundary<0)throw Error('Operation fixture boundary changed');
const module=process.env.PLAYWRIGHT_MODULE?pathToFileURL(path.resolve(process.env.PLAYWRIGHT_MODULE)).href:import.meta.resolve('playwright');
let fixture=source.slice(0,boundary).replace("process.env.PLAYWRIGHT_MODULE || 'playwright'",JSON.stringify(module));
fixture=fixture.replace("  if(pathname.startsWith('/api/')){",`  if(pathname==='/api/notifications'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(notificationHistory));return;}
  if(pathname.startsWith('/api/notifications/')){notificationDeletes++;res.end('{}');return;}
  if(pathname.startsWith('/api/')){`);
fixture=fixture.replace("const server=http.createServer",`const oldNotice={id:1,timestamp_ms:Date.now()-10000,message:'Old T clock reset confirmation',persistent:true};let notificationHistory=[oldNotice],notificationDeletes=0;
const server=http.createServer`);
fixture+=`
try {
 const context=await browser.newContext({viewport:{width:1280,height:1000}});
 await context.addInitScript(({session,origin})=>{if(!URL.parse)URL.parse=(input,base)=>{try{return new URL(input,base)}catch{return null}};localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
 const sockets=new Set();await context.routeWebSocket(url=>url.pathname==='/_dioxus',()=>{});await context.routeWebSocket(url=>url.pathname==='/ws',socket=>{sockets.add(socket);socket.onClose(()=>sockets.delete(socket));});
 const page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));
 await page.goto(origin+'/dashboard');await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 const popups=()=>page.locator('.gs26-notification-stack');
 assert.equal(await popups().count(),0,'REST history must not popup, including old persistent=true');
 for(const socket of sockets)socket.send(JSON.stringify({ty:'Notifications',data:notificationHistory}));
 await page.getByRole('link',{name:'Notifications',exact:true}).click();
 await page.getByText(oldNotice.message,{exact:true}).waitFor();
 assert.equal(await popups().count(),0);
 await page.getByRole('link',{name:'Dashboard',exact:true}).click();
 const live={id:2,timestamp_ms:Date.now(),message:'Live T clock reset confirmation',persistent:true};notificationHistory.push(live);
 for(const socket of sockets){socket.send(JSON.stringify({ty:'Notification',data:live}));socket.send(JSON.stringify({ty:'Notifications',data:notificationHistory}));}
 await popups().getByText(live.message,{exact:true}).waitFor();
 assert.equal(await popups().getByText(oldNotice.message,{exact:true}).count(),0,'old history not replayed by new event');
 for(const socket of sockets)socket.send(JSON.stringify({ty:'Notification',data:live}));
 assert.equal(await popups().getByText(live.message,{exact:true}).count(),1,'duplicate live event not duplicated');
 await popups().waitFor({state:'detached',timeout:8000});
 assert.equal(notificationDeletes,0,'auto-timeout must preserve center history for other/offline clients');
 await page.reload();await page.getByRole('navigation',{name:'Mission navigation'}).waitFor();
 assert.equal(await popups().count(),0,'reload does not replay live or persistent notices');
 const offline={id:3,timestamp_ms:Date.now(),message:'Missed while disconnected',persistent:false};notificationHistory.push(offline);
 for(const socket of sockets)socket.send(JSON.stringify({ty:'Notifications',data:notificationHistory}));
 await page.getByRole('link',{name:'Notifications',exact:true}).click();
 for(const note of notificationHistory)await page.getByText(note.message,{exact:true}).waitFor();
 assert.equal(await popups().count(),0,'missed notices go only to center');
 assert.equal(notificationDeletes,0,'opening center never deletes shared history');
 assert.deepEqual(errors,[]);await context.close();console.log('PASS actual Wasm: live-only popups, no replay on first load/reload/snapshot, duplicate suppression, 5s timeout, center includes offline/transient notices, popup timeout/opening center preserve shared history');
}finally{await browser.close();await new Promise(r=>server.close(r));}`;
try{await import('data:text/javascript;base64,'+Buffer.from(fixture).toString('base64'));}catch(error){console.error(error.name+': '+error.message);process.exitCode=1;}
