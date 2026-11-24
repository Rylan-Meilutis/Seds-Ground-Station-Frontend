// Demo mode and replay files at /preview of the web bundle: the live
// dashboard's own tabs, connected to a station answered in the page, with no
// station, no credentials, an empty browser profile and zero network.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {fileURLToPath} from 'node:url';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const base=(process.env.GS_PREVIEW_URL||'http://127.0.0.1:19096').replace(/\/$/,'');
const fixture=name=>fileURLToPath(new URL(`./fixtures/backend-contract/${name}`,import.meta.url));
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
const shots=process.env.GS_UI_ARTIFACTS;
try {
 const context=await browser.newContext({acceptDownloads:true,viewport:{width:1280,height:860}});
 const page=await context.newPage();const errors=[],requests=[],ws=[];
 page.on('pageerror',e=>errors.push(e.message));
 // Everything but the page's own bundle (document, scripts, styles, wasm, fonts, images, models).
 page.on('request',r=>{const u=new URL(r.url());if(u.protocol==='blob:'||u.protocol==='data:')return;
  if(['document','script','stylesheet','font','image'].includes(r.resourceType())&&u.origin===new URL(base).origin&&!u.pathname.startsWith('/api'))return;
  if(/\.(wasm|js|glb|json)$/.test(u.pathname)&&u.pathname.startsWith('/assets/'))return;
  requests.push(r.method()+' '+r.url());});
 page.on('websocket',s=>ws.push(s.url()));
 await page.goto(base+'/preview');
 await page.getByText('Demo mode — no station connected',{exact:true}).waitFor();
 const stored=await page.evaluate(()=>JSON.stringify({...localStorage}));
 const nav=page.locator('nav[aria-label="Mission navigation"]');
 const open=async(label,needles=[])=>{await nav.locator(`a[aria-label="${label}"]`).click();
  for(const n of needles)await page.getByText(n).first().waitFor({state:'attached',timeout:20000});
  const text=await page.locator('.gs26-local-dashboard').innerText();
  assert(!/\bNaN\b|undefined/.test(text),label+' renders no NaN/undefined');
  const exit=page.getByRole('button',{name:'Exit stream mode'});if(await exit.count()&&await exit.first().isVisible())await exit.first().click();};
 // The real dashboard's tabs, populated by the demo station.
 await open('Dashboard',['Fill box battery']);
 await open('Data',['Tank pressure','Fill loadcell']);
 await open('Live diagrams',['Fill system']);
 await open('Connection Status',['Healthy']);
 await open('Checklists',['Pad checks (demo)']);
 await open('Data Export',['Demo flight (this scenario)']);
 await open('3D replay');
 await open('Map');
 // Firmware: read-only, the recorded builds and health; flashing needs a station.
 await open('Firmware Update',['Flashing needs a station','Build demo-1.0','76 health reports']);
 // Live-only panels say so in one sentence instead of an HTTP error.
 await open('Mission Live',['Live cameras need a Ground Station.']);
 assert(!/Export request failed|HTTP 404/.test(await page.locator('.gs26-local-dashboard').innerText()),'no HTTP errors on live-only panels');
 // The clock runs: playback moves the station on by itself.
 const before=await page.getByTestId('preview-state').innerText();
 await page.getByLabel('Speed').selectOption('50');
 await page.waitForFunction(b=>document.querySelector('[data-testid=preview-state]').innerText!==b,before,{timeout:30000});
 // Commands go to the local station, which acknowledges them; nothing is sent.
 await open('Actions');await page.getByRole('button',{name:/^Vent/}).first().click();
 await page.getByText(/acknowledged\. Nothing was sent; no station is connected\./).first().waitFor();
 for(const width of [390,1280]){await page.setViewportSize({width,height:844});await page.waitForTimeout(400);
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no page overflow '+width);
  if(shots)await page.screenshot({path:`${shots}/local-station-demo-${width}.png`});}
 await page.setViewportSize({width:1280,height:860});
 // A replay bundle on a fresh device: the same tabs, from the file.
 await page.getByLabel('Open replay file').setInputFiles(fixture('replay-bundle/flight.gsreplay'));
 await page.getByText('Offline replay — flight.gsreplay',{exact:true}).waitFor();
 await open('Dashboard');await open('Data');await open('Live diagrams');await open('Map');await open('3D replay');
 await open('Firmware Update',['Board firmware does not report its build','Flight Computer (FC)']);
 await open('Motor & flight');
 assert(!/Export request failed|Not available offline/.test(await page.locator('.gs26-local-dashboard').innerText()),'the recorded review, not an error');
 // Abort is answered by the replay station: read-only, nothing sent.
 await page.getByRole('button',{name:'ABORT'}).first().click();
 const confirm=page.getByRole('dialog').getByRole('button',{name:/abort/i});
 if(await confirm.count())await confirm.first().click();
 await page.getByText(/Replay is read-only: .* was not sent\./).first().waitFor();
 // The station's CSV export comes back out of the bundle byte for byte.
 const recording=JSON.parse(fs.readFileSync(fixture('replay-bundle/flight.manifest.json'),'utf8')).source.recording;
 await open('Data Export');
 const picker=page.locator(`select:has(option[value="${recording}"])`).first();
 await picker.waitFor({state:'attached',timeout:20000});
 await picker.selectOption(recording);
 const [download]=await Promise.all([page.waitForEvent('download'),page.getByRole('button',{name:'Download full recording CSV'}).click()]);
 assert(fs.readFileSync(await download.path()).equals(fs.readFileSync(fixture('replay-bundle/flight.csv'))),'bundle CSV export equals the station CSV');
 // A portable replay file too.
 await page.getByLabel('Open replay file').setInputFiles(fixture('replay-export/flight.json'));
 await page.getByText('Offline replay — flight.json',{exact:true}).waitFor();
 await open('Data');
 if(shots)await page.screenshot({path:`${shots}/local-station-replay-file.png`});
 await page.getByRole('button',{name:'Back to demo'}).click();await page.getByText('Demo mode — no station connected',{exact:true}).waitFor();
 assert.equal(await page.evaluate(()=>JSON.stringify({...localStorage})),stored,'demo and replays do not change stored preferences');
 assert.deepEqual(requests,[],'no network requests');assert.deepEqual(ws.filter(u=>new URL(u).pathname!=='/_dioxus'),[],'no WebSocket');
 assert.deepEqual(errors,[]);
 console.log('PASS: demo and replay files run the live dashboard tabs on a local station (dashboard, data, diagrams, connection, checklists, exports, 3D replay, map, actions, firmware, motor & flight), the clock runs, commands are acknowledged locally, the bundle CSV export equals the station CSV, two widths, zero network, unchanged storage');
} catch(e) {if(shots)await (await browser.contexts()[0].pages())[0].screenshot({path:`${shots}/failure.png`}).catch(()=>{});throw e;} finally {await browser.close();}
