// Build with `dx build --platform web`, then run with Node and Playwright.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const root=path.resolve('target/dx/groundstation_frontend/debug/web/public');
const example=name=>JSON.parse(fs.readFileSync(`docs/api-examples/${name}.json`,'utf8'));
const session={...example('auth-session.anonymous'),authenticated:true,anonymous:false,username:'ui-test',roles:['stream_master'],permissions:{view_data:true,send_commands:true}};
let cameraOnline=true;
let fillSource='kg50';
const dashboardLayout=example('layout.full');
for(const id of ['LOADCELL','DAQ']) dashboardLayout.data_tab.tabs.push({id,label:id,channels:[],subtabs:[
  {id:'large',label:'1000 kg',data_type:'KG1000',channels:['Raw'],chart:{enabled:true},chart_groups:[{title:'Fill %',data_type:'LOADCELL_FILL_PERCENT',channels:[0]}],summary_items:[{label:'Fill %',data_type:'LOADCELL_FILL_PERCENT',index:0}]},
  {id:'small',label:'50 kg',data_type:'KG50',channels:['Raw'],chart:{enabled:true},chart_groups:[],summary_items:[]}
]});

const server=http.createServer((req,res)=>{
  const pathname=new URL(req.url,'http://localhost').pathname;
  if(pathname==='/test-camera') {res.setHeader('Content-Type','text/html');res.end('<p>Live camera fixture</p>');return;}
  if(pathname==='/radio'||pathname==='/media') {res.setHeader('Content-Type','text/html');res.end(`<p>Media tool fixture</p><script>window.received=[];addEventListener('message',e=>{if(e.source===parent)received.push(e.data)})</script>`);return;}
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
  assert.equal((await page.locator('.gs26-title-tab-toggle span').first().innerText()),'Dashboard');
  await page.getByRole('button',{name:'Checklist',exact:true}).click();
  await page.getByRole('checkbox',{name:'Pressure transducer reading checked'}).check();
  await choose();await page.getByRole('button',{name:'Mission Live',exact:true}).click();
  assert.equal(await page.locator('.gs26-program-header, .gs26-program-editor').count(),0,'Mission Live is stream only');
  assert.equal(await page.getByText('Ground setup',{exact:true}).count(),0,'no ground controls in stream');
  assert.equal(await page.locator('#ground-checklist-panel').count(),0,'checklist stays collapsed');
  await page.getByRole('button',{name:'Checklist',exact:true}).click();
  assert(await page.getByRole('checkbox',{name:'Pressure transducer reading checked'}).isChecked(),'checklist persists across tabs');
  await page.getByRole('button',{name:'Checklist',exact:true}).click();
  await choose();await page.getByRole('button',{name:'Dashboard',exact:true}).click();

  assert.equal(await page.getByRole('heading',{name:'Dashboard',exact:true}).count(),0,'no duplicate dashboard heading');
  await choose();
  await page.getByRole('button',{name:'Stream Manager',exact:true}).click();
  await page.getByRole('heading',{name:'Broadcast studio'}).waitFor();
  assert.equal((await page.locator('.gs26-title-tab-toggle span').first().innerText()),'Stream Manager');
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
  await choose();await page.getByRole('button',{name:'Voice Chat',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('#gs26-crew-voice')?.contentWindow.received.some(m=>m.token==='ui-token'&&m.visible));
  await page.frameLocator('#gs26-crew-voice').getByText('Media tool fixture').waitFor({state:'visible'});
  assert((await page.locator('#gs26-crew-voice').boundingBox()).height>300,'voice fills the tab');
  await page.evaluate(()=>document.querySelector('#gs26-crew-voice').contentWindow.instanceMarker=42);
  await choose();await page.getByRole('button',{name:'Dashboard',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('#gs26-crew-voice')?.contentWindow.received.some(m=>m.visible===false));
  assert.equal(await page.evaluate(()=>document.querySelector('#gs26-crew-voice').contentWindow.instanceMarker),42,'media document persists when hidden');
  assert.equal(await page.locator('#gs26-crew-voice').isVisible(),false);
  await choose();await page.getByRole('button',{name:'Cameras & Recordings',exact:true}).click();
  await page.frameLocator('#gs26-media').getByText('Media tool fixture').waitFor({state:'visible'});
  assert((await page.locator('#gs26-media').boundingBox()).height>300,'cameras fill the tab');
  await page.evaluate(()=>document.querySelector('#gs26-media').contentWindow.instanceMarker=43);
  await choose();await page.getByRole('button',{name:'Voice Chat',exact:true}).click();
  await page.frameLocator('#gs26-crew-voice').getByText('Media tool fixture').waitFor({state:'visible'});
  assert.equal(await page.locator('#gs26-media').isVisible(),false);
  await choose();await page.getByRole('button',{name:'Cameras & Recordings',exact:true}).click();
  await page.frameLocator('#gs26-media').getByText('Media tool fixture').waitFor({state:'visible'});
  assert.equal(await page.evaluate(()=>document.querySelector('#gs26-media').contentWindow.instanceMarker),43,'camera document persists when reopened');
  await choose();await page.getByRole('button',{name:'Dashboard',exact:true}).click();
  await choose();await page.getByRole('button',{name:'Customize',exact:true}).click();
  await page.getByRole('checkbox',{name:'Voice Chat',exact:true}).uncheck();
  await page.getByRole('button',{name:'Done editing',exact:true}).click();
  await choose();assert.equal(await page.getByRole('button',{name:'Voice Chat',exact:true}).count(),0);
  await page.getByRole('button',{name:'Close tab picker',exact:true}).click();
  await page.reload();
  await page.frameLocator('iframe[title="Live camera"]').getByText('Live camera fixture').waitFor();
  await page.locator('[aria-label="Pinned telemetry"]').getByText('Pinned load',{exact:true}).waitFor();
  await choose();assert.equal(await page.getByRole('button',{name:'Voice Chat',exact:true}).count(),0);
  await page.getByRole('button',{name:'Close tab picker',exact:true}).click();
  await page.locator('iframe[title="Live camera"]').scrollIntoViewIfNeeded();
  await page.screenshot({path:'/tmp/gs-dashboard-desktop.png',fullPage:true});
  await choose();
  await page.setViewportSize({width:390,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no horizontal page scrolling on mobile');
  const nav=page.locator('#dashboard-tab-picker');assert(await nav.evaluate(el=>el.scrollWidth<=el.clientWidth),'tab picker does not scroll sideways');
  await page.getByRole('button',{name:'Close tab picker',exact:true}).click();
  await choose();
  const titleToggle=page.locator('.gs26-title-tab-toggle').first();
  assert((await titleToggle.boundingBox()).height>=44,'mobile title has a full touch target');
  await page.screenshot({path:'/tmp/gs-dashboard-mobile-menu.png',fullPage:true});
  await page.getByRole('button',{name:'Close tab picker',exact:true}).click();
  await page.locator('iframe[title="Live camera"]').scrollIntoViewIfNeeded();
  await page.screenshot({path:'/tmp/gs-dashboard-mobile.png',fullPage:true});
  await page.getByRole('button',{name:'Checklist',exact:true}).click();
  await page.getByRole('checkbox',{name:'Pressure transducer reading checked'}).waitFor({state:'visible'});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'checklist fits mobile');
  await page.screenshot({path:'/tmp/gs-checklist-mobile.png',fullPage:true});
  await page.getByRole('button',{name:'Checklist',exact:true}).click();
  session.roles=[]; session.username='other-user'; session.permissions.send_commands=false;
  await page.reload();await choose();
  assert.equal(await page.getByRole('button',{name:'Stream Manager',exact:true}).count(),0,'manager is permission gated');
  assert.equal(await page.getByRole('button',{name:'Checklist',exact:true}).count(),0,'checklist is permission gated');
  assert.equal(await page.locator('[aria-label="Pinned telemetry"]').count(),0,'another user starts with their own pins');
  assert.equal(await page.getByRole('button',{name:'Voice Chat',exact:true}).count(),1,'another user has independent tab visibility');
  await page.getByRole('button',{name:'Voice Chat',exact:true}).click();
  await page.frameLocator('#gs26-crew-voice').getByText('Media tool fixture').waitFor({state:'visible'});
  assert((await page.locator('#gs26-crew-voice').boundingBox()).height>300,'voice fills mobile tab');
  await choose();await page.getByRole('button',{name:'Cameras & Recordings',exact:true}).click();
  await page.frameLocator('#gs26-media').getByText('Media tool fixture').waitFor({state:'visible'});
  assert((await page.locator('#gs26-media').boundingBox()).height>300,'cameras fill mobile tab');
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'media fits mobile width');
  for(const source of ['kg50','kg1000_absolute']) {
    fillSource=source;
    await page.reload();await choose();await page.getByRole('button',{name:'Telemetry',exact:true}).click();
    for(const tab of ['LOADCELL','DAQ']) {
      await page.getByRole('button',{name:/Show data tabs/}).click();
      await page.getByRole('button',{name:tab,exact:true}).click();
      for(const cell of ['1000 kg','50 kg']) {
        await page.getByRole('button',{name:/Show subtabs/}).click();
        await page.getByRole('button',{name:cell,exact:true}).click();
        const expected=cell===(source==='kg50'?'50 kg':'1000 kg');
        if(expected) await page.getByText('Fill %',{exact:true}).first().waitFor({state:'visible'});
        else assert.equal(await page.getByText('Fill %',{exact:true}).count(),0,`${tab} ${cell} must not show other cell's fill`);
      }
    }
  }
  assert.deepEqual(errors,[]);
  console.log('PASS: no dashboard model, persistent/resizable camera cards, offline recovery, stream manager permissions, card editing, live pins, per-user persistence, hidden tabs, iframe session handoff/lifetime, desktop and mobile tab picker, permission-gated checklist, stream-only Mission Live, selected-loadcell fill in Loadcell and DAQ');
}finally{await browser.close();await new Promise(resolve=>server.close(resolve));}
