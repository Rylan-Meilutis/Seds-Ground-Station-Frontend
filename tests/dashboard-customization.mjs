// Build with `dx build --platform web`, then run with Node and Playwright.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root=path.resolve('target/dx/groundstation_frontend/debug/web/public');
const example=name=>JSON.parse(fs.readFileSync(`docs/api-examples/${name}.json`,'utf8'));
const session={...example('auth-session.anonymous'),authenticated:true,anonymous:false,username:'ui-test',roles:['stream_master']};
let cameraOnline=true;
const server=http.createServer((req,res)=>{
  const pathname=new URL(req.url,'http://localhost').pathname;
  if(pathname==='/test-camera') {res.setHeader('Content-Type','text/html');res.end('<p>Live camera fixture</p>');return;}
  if(pathname==='/radio'||pathname==='/media') {res.setHeader('Content-Type','text/html');res.end(`<p>Media tool fixture</p><script>window.received=[];addEventListener('message',e=>{if(e.source===parent)received.push(e.data)})</script>`);return;}
  if(pathname.startsWith('/api/')){
    res.setHeader('Content-Type','application/json');
    const data=pathname==='/api/auth/session'?session:pathname==='/api/layout'?example('layout.full'):pathname==='/api/live_streams'?{...example('live-streams'),can_manage_stream:session.roles.includes('stream_master'),program_url:'',can_preview_live:session.roles.includes('stream_master'),streams:[{id:'pad-wide',label:'Pad wide',url:'/test-camera?ticket=fixture',online:cameraOnline,kind:'webrtc'}]}:pathname==='/api/vehicle'?{}:[];
    res.end(JSON.stringify(data));return;
  }
  const relative=pathname==='/'||pathname==='/dashboard'?'index.html':pathname.slice(1);
  const file=path.resolve(root,relative);
  if(!file.startsWith(root+path.sep)||!fs.existsSync(file)||!fs.statSync(file).isFile()){res.statusCode=404;res.end();return;}
  const mime={'.html':'text/html','.js':'text/javascript','.wasm':'application/wasm','.css':'text/css','.svg':'image/svg+xml','.png':'image/png'};
  res.setHeader('Content-Type',mime[path.extname(file)]||'application/octet-stream');res.end(fs.readFileSync(file));
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin=`http://127.0.0.1:${server.address().port}`;
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
try{
  const context=await browser.newContext({viewport:{width:1440,height:1000}});
  await context.addInitScript(({session,origin})=>{if(!localStorage.getItem('auth_session_v1'))localStorage.setItem('auth_session_v1',JSON.stringify({entries:[{host_scope:origin,updated_at_ms:Date.now(),session:{token:'ui-token',session,remember_me:true}}]}));},{session,origin});
  const page=await context.newPage();const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.goto(origin);
  await page.waitForTimeout(1000);
  if (!(await page.getByRole('button',{name:/Choose tab/}).count())) console.error('Startup:', await page.locator('body').innerText(), errors);
  const choose=()=>page.getByRole('button',{name:/Choose tab/}).click();
  assert.equal(await page.locator('model-viewer, gs-vehicle-viewer').count(),0,'Dashboard has no 3D model');
  await page.getByRole('heading',{name:'Dashboard',exact:true}).waitFor();
  await choose();
  await page.getByRole('button',{name:'Stream Manager',exact:true}).click();
  await page.getByRole('heading',{name:'Broadcast studio'}).waitFor();
  await choose();await page.getByRole('button',{name:'Dashboard',exact:true}).click();
  await page.getByRole('button',{name:'Pin to top bar',exact:true}).first().click();
  await page.locator('[aria-label="Pinned telemetry"]').getByText('50 kg loadcell',{exact:true}).waitFor();
  await page.getByRole('button',{name:'Customize dashboard',exact:true}).click();
  await page.getByLabel('Title',{exact:true}).first().fill('Pinned load');
  await page.getByRole('button',{name:'Add camera',exact:true}).click();
  await page.getByLabel(/^Camera source/).selectOption('pad-wide');
  await page.getByLabel(/^Card width/).last().selectOption('full');
  await page.getByRole('button',{name:'Done',exact:true}).click();
  await page.frameLocator('iframe[title="Live camera"]').getByText('Live camera fixture').waitFor();
  assert.equal(await page.locator('.gs26-dashboard-card').last().getAttribute('data-width'),'full');
  cameraOnline=false; await page.getByText('Camera offline — waiting for pad-wide').waitFor();
  assert.equal(await page.locator('iframe[title="Live camera"]').count(),0);
  cameraOnline=true; await page.frameLocator('iframe[title="Live camera"]').getByText('Live camera fixture').waitFor();
  await page.locator('[aria-label="Pinned telemetry"]').getByText('Pinned load',{exact:true}).waitFor();
  await choose();await page.getByRole('button',{name:'Crew Voice',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('#gs26-crew-voice')?.contentWindow.received.some(m=>m.token==='ui-token'&&m.visible));
  await page.evaluate(()=>document.querySelector('#gs26-crew-voice').contentWindow.instanceMarker=42);
  await choose();await page.getByRole('button',{name:'Dashboard',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('#gs26-crew-voice')?.contentWindow.received.some(m=>m.visible===false));
  assert.equal(await page.evaluate(()=>document.querySelector('#gs26-crew-voice').contentWindow.instanceMarker),42,'media document persists when hidden');
  await page.getByRole('button',{name:'Customize',exact:true}).click();
  await page.getByRole('checkbox',{name:'Crew Voice',exact:true}).uncheck();
  await page.getByRole('button',{name:'Done editing',exact:true}).click();
  await choose();assert.equal(await page.getByRole('button',{name:'Crew Voice',exact:true}).count(),0);
  await page.getByRole('button',{name:'Close tab picker',exact:true}).click();
  await page.reload();
  await page.frameLocator('iframe[title="Live camera"]').getByText('Live camera fixture').waitFor();
  await page.locator('[aria-label="Pinned telemetry"]').getByText('Pinned load',{exact:true}).waitFor();
  await choose();assert.equal(await page.getByRole('button',{name:'Crew Voice',exact:true}).count(),0);
  await page.getByRole('button',{name:'Close tab picker',exact:true}).click();
  await page.locator('iframe[title="Live camera"]').scrollIntoViewIfNeeded();
  await page.screenshot({path:'/tmp/gs-dashboard-desktop.png',fullPage:true});
  await choose();
  await page.setViewportSize({width:390,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no horizontal page scrolling on mobile');
  const nav=page.locator('#dashboard-tab-picker');assert(await nav.evaluate(el=>el.scrollWidth<=el.clientWidth),'tab picker does not scroll sideways');
  await page.getByRole('button',{name:'Close tab picker',exact:true}).click();
  await page.locator('iframe[title="Live camera"]').scrollIntoViewIfNeeded();
  await page.screenshot({path:'/tmp/gs-dashboard-mobile.png',fullPage:true});
  session.roles=[]; session.username='other-user';
  await page.reload();await choose();
  assert.equal(await page.getByRole('button',{name:'Stream Manager',exact:true}).count(),0,'manager is permission gated');
  assert.equal(await page.locator('[aria-label="Pinned telemetry"]').count(),0,'another user starts with their own pins');
  assert.equal(await page.getByRole('button',{name:'Crew Voice',exact:true}).count(),1,'another user has independent tab visibility');
  assert.deepEqual(errors,[]);
  console.log('PASS: no dashboard model, persistent/resizable camera cards, offline recovery, stream manager permissions, card editing, live pins, per-user persistence, hidden tabs, iframe session handoff/lifetime, desktop and mobile tab picker');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
