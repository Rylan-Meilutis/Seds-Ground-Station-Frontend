// The offline workspace at /preview of the web bundle: demo mode and a replay
// file opened with no station, no credentials and an empty browser profile.
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const base=(process.env.GS_PREVIEW_URL||'http://127.0.0.1:19096').replace(/\/$/,'');
const replay=fileURLToPath(new URL('./fixtures/backend-contract/replay-export/flight.json',import.meta.url));
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
const shots=process.env.GS_UI_ARTIFACTS;
try {
 const page=await (await browser.newContext()).newPage();const errors=[],requests=[],ws=[];
 page.on('pageerror',e=>errors.push(e.message));
 page.on('request',r=>{if(['xhr','fetch','media'].includes(r.resourceType())&&!r.url().endsWith('.wasm'))requests.push(r.url());});
 page.on('websocket',s=>ws.push(s.url()));
 await page.goto(base+'/preview');
 await page.getByText('Demo mode — no station connected',{exact:true}).waitFor();
 const stored=await page.evaluate(()=>JSON.stringify({...localStorage}));
 const nav=page.getByRole('navigation',{name:'Mission navigation'});
 const open=async label=>{await nav.getByRole('link',{name:label,exact:true}).click();await page.getByRole('heading',{name:label,exact:true,level:1}).waitFor();};
 const pages={
  'Dashboard':['Oxidizer mass','Valves'],'Mission':['Flight procedure','Abort'],'Map':['Map tiles are not loaded'],
  'Replay & summary':['Flight summary','Apogee (barometer)'],'Data':['Tank pressure'],'Diagrams':['Fill system'],
  'Network':['Flight Computer','Warnings'],'Calibration':['Fill loadcell (KG50)'],'Recordings':['Demo flight (this scenario)'],
 };
 for(const [label,needles] of Object.entries(pages)){await open(label);for(const n of needles)await page.getByText(n).first().waitFor();
  assert(!/\bNaN\b|undefined/.test(await page.locator('main').innerText()),label+' renders no NaN');}
 // The clock runs: the demo moves on by itself.
 const before=await page.getByTestId('preview-state').innerText();
 await page.getByLabel('Speed').selectOption('10');await page.waitForFunction(b=>document.querySelector('[data-testid=preview-state]').innerText!==b,before,{timeout:20000});
 // Commands are acknowledged locally, never sent.
 await open('Mission');await page.getByRole('button',{name:'Abort'}).first().click();
 await page.getByText(/acknowledged\. Nothing was sent/).waitFor();
 // A replay file on a fresh device.
 await page.getByLabel('Open replay file').setInputFiles(replay);
 await page.getByText('Offline replay — flight.json',{exact:true}).waitFor();
 for(const label of ['Dashboard','Data','Map','Replay & summary','Network','Diagrams']){await open(label);
  assert(!/\bNaN\b|undefined/.test(await page.locator('main').innerText()),label+' (replay) renders no NaN');}
 await open('Replay & summary');await page.getByText('Apogee (barometer)').waitFor();
 await page.getByRole('button',{name:'Load 3D replay'}).click();await page.getByRole('img',{name:'Flight vehicle and named valve states'}).waitFor({timeout:30000});
 await open('Mission');await page.getByText('Replays are read-only; commands are not sent.').waitFor();
 await open('Calibration');await page.getByRole('button',{name:'Capture zero'}).count().then(n=>assert.equal(n,0,'no calibration actions in a replay'));
 for(const width of [320,390,844,1280]){await page.setViewportSize({width,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no page overflow '+width);
  if(shots)await page.screenshot({path:`${shots}/offline-workspace-${width}.png`});}
 await page.getByRole('button',{name:'Back to demo'}).click();await page.getByText('Demo mode — no station connected',{exact:true}).waitFor();
 assert.equal(await page.evaluate(()=>JSON.stringify({...localStorage})),stored,'the workspace does not change stored preferences');
 assert.deepEqual(requests,[],'no HTTP API requests');assert.deepEqual(ws.filter(u=>new URL(u).pathname!=='/_dioxus'),[],'no WebSocket');
 assert.deepEqual(errors,[]);
 console.log('PASS: demo pages populated and animated, local command acknowledgement, replay file opened offline (all pages, 3D), four widths, zero network, unchanged storage');
} finally {await browser.close();}
