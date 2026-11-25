// Opening a replay file straight from the connection screen of the web
// bundle, on a fresh browser profile with no station: the connection-failure
// card offers Open replay file; picking (or dropping) a bundle goes directly
// to the replay, never through demo mode; the replay's tabs render; nothing
// leaves the page once the file is chosen; Leave replay goes back.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {fileURLToPath} from 'node:url';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const base=(process.env.GS_PREVIEW_URL||'http://127.0.0.1:19096').replace(/\/$/,'');
const fixture=name=>fileURLToPath(new URL(`./fixtures/backend-contract/${name}`,import.meta.url));
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
const shots=process.env.GS_UI_ARTIFACTS;
try {
 const context=await browser.newContext({viewport:{width:1280,height:860}});
 const page=await context.newPage();const errors=[];let requests=null,ws=null;
 page.on('pageerror',e=>errors.push(e.message));
 // After the file is chosen: everything but blob/data URLs and the page's own
 // bundle (scripts, styles, wasm, fonts, images, models) counts.
 page.on('request',r=>{if(!requests)return;const u=new URL(r.url());if(u.protocol==='blob:'||u.protocol==='data:')return;
  if(['script','stylesheet','font','image'].includes(r.resourceType())&&u.origin===new URL(base).origin&&!u.pathname.startsWith('/api'))return;
  if(/\.(wasm|js|glb|json)$/.test(u.pathname)&&u.pathname.startsWith('/assets/'))return;
  requests.push(r.method()+' '+r.url());});
 page.on('websocket',s=>{if(ws)ws.push(s.url());});
 // A fresh device: no station configured, nothing stored.
 await page.goto(base+'/');
 const card=page.locator('.gs26-connection-failure');
 await card.getByText('Failed to Connect',{exact:true}).waitFor({timeout:60000});
 const stored=await page.evaluate(()=>JSON.stringify({...localStorage}));
 const picker=card.getByLabel('Open replay file');
 assert.equal(await picker.count(),1,'the connection screen offers Open replay file');
 await card.getByText('or drop a .gsreplay file here').waitFor();
 if(shots)await page.screenshot({path:`${shots}/open-replay-connect.png`});
 // Pick the bundle: straight into the replay, no demo on the way.
 requests=[];ws=[];
 const sawDemo=page.waitForFunction(()=>/Demo mode — no station connected/.test(document.body.innerText),null,{timeout:15000,polling:'raf'}).then(()=>true,()=>false);
 await picker.setInputFiles(fixture('replay-bundle/flight.gsreplay'));
 await page.getByText('Replay: flight.gsreplay',{exact:true}).waitFor({timeout:60000});
 assert.equal(new URL(page.url()).pathname,'/preview');
 assert.equal(await page.locator('.gs26-local-station').getAttribute('data-source'),'replay');
 assert.equal(await page.getByRole('button',{name:'Back to demo'}).count(),0,'no demo offered to someone who never entered it');
 const nav=page.locator('nav[aria-label="Mission navigation"]');
 const open=async(label,needles=[])=>{await nav.locator(`a[aria-label="${label}"]`).click();
  for(const n of needles)await page.getByText(n).first().waitFor({state:'attached',timeout:20000});
  const text=await page.locator('.gs26-local-dashboard').innerText();
  assert(!/\bNaN\b|undefined/.test(text),label+' renders no NaN/undefined');
  assert(!/Export request failed|HTTP 404/.test(text),label+' renders no HTTP error');};
 await open('Dashboard');await open('Data');await open('Live diagrams');await open('Map');await open('3D replay');
 await open('Firmware Update',['Board firmware does not report its build']);
 await open('Motor & flight');await open('Connection Status',['Replay: flight.gsreplay']);
 if(shots)await page.screenshot({path:`${shots}/open-replay-replay.png`});
 assert.equal(await sawDemo,false,'demo mode never showed');
 assert.deepEqual(requests,[],'no network requests once the file is chosen');
 assert.deepEqual(ws.filter(u=>new URL(u).pathname!=='/_dioxus'),[],'no WebSocket');
 assert.equal(await page.evaluate(()=>JSON.stringify({...localStorage})),stored,'the replay changes nothing stored in the browser');
 // Back to the connection screen.
 await page.getByRole('button',{name:'Leave replay'}).click();
 await card.getByText('Failed to Connect',{exact:true}).waitFor({timeout:60000});
 assert.equal(await page.evaluate(()=>window.location.pathname),'/');
 // Drop a portable replay file on the connection screen.
 requests=[];ws=[];
 const text=fs.readFileSync(fixture('replay-export/flight.json'));
 const transfer=await page.evaluateHandle(bytes=>{const t=new DataTransfer();t.items.add(new File([new Uint8Array(bytes)],'dropped.seds-replay.json',{type:'application/json'}));return t;},[...text]);
 await card.dispatchEvent('dragover',{dataTransfer:transfer});
 await card.dispatchEvent('drop',{dataTransfer:transfer});
 await page.getByText('Replay: dropped.seds-replay.json',{exact:true}).waitFor({timeout:60000});
 await open('Data');
 assert.deepEqual(requests,[],'no network requests after the drop');
 // A file that is not a replay: said on the replay view, with the way back; no demo.
 await page.getByRole('button',{name:'Leave replay'}).click();
 await card.getByText('Failed to Connect',{exact:true}).waitFor({timeout:60000});
 await card.getByLabel('Open replay file').setInputFiles({name:'notes.gsreplay',mimeType:'application/octet-stream',buffer:Buffer.from([0xff,0xfe,0x00])});
 await page.getByText(/^Could not open notes\.gsreplay/).waitFor({timeout:30000});
 await page.getByText('No replay open',{exact:true}).waitFor();
 assert.equal(await page.getByText('Demo mode — no station connected').count(),0);
 await page.getByRole('button',{name:'Leave replay'}).click();
 await card.getByText('Failed to Connect',{exact:true}).waitFor({timeout:60000});
 assert.deepEqual(errors,[]);
 console.log('PASS: Open replay file on the connection screen of a fresh browser opens the bundle straight into its replay (no demo), the replay tabs render with zero network, Leave replay returns, a dropped replay file opens too, and a damaged file is reported without a demo');
} catch(e) {if(shots)await (await browser.contexts()[0].pages())[0].screenshot({path:`${shots}/open-replay-failure.png`}).catch(()=>{});throw e;} finally {await browser.close();}
