// Demo mode and replay files at /preview of the web bundle: the live
// dashboard's own tabs, connected to a station answered in the page, with no
// station, no credentials, an empty browser profile and zero network.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import crypto from 'node:crypto';
import zlib from 'node:zlib';
import {fileURLToPath} from 'node:url';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const base=(process.env.GS_PREVIEW_URL||'http://127.0.0.1:19096').replace(/\/$/,'');
const fixture=name=>fileURLToPath(new URL(`./fixtures/backend-contract/${name}`,import.meta.url));
// The contract bundle with derived/stations.json (GET /api/antenna/status at export) added.
function bundleWithStations(){
 const zip=fs.readFileSync(fixture('replay-bundle/flight.gsreplay'));const files=new Map();
 const end=zip.lastIndexOf(Buffer.from([0x50,0x4b,0x05,0x06]));let at=zip.readUInt32LE(end+16);
 for(let i=0;i<zip.readUInt16LE(end+10);i++){
  const method=zip.readUInt16LE(at+10),size=zip.readUInt32LE(at+20),nameLen=zip.readUInt16LE(at+28),extra=zip.readUInt16LE(at+30),comment=zip.readUInt16LE(at+32),local=zip.readUInt32LE(at+42);
  const name=zip.toString('utf8',at+46,at+46+nameLen);at+=46+nameLen+extra+comment;
  const start=local+30+zip.readUInt16LE(local+26)+zip.readUInt16LE(local+28),data=zip.subarray(start,start+size);
  files.set(name,method===8?zlib.inflateRawSync(data):Buffer.from(data));
 }
 const stations=Buffer.from(JSON.stringify({gpsd_configured:true,ground_station:{latitude:31.7535,longitude:-102.0505,altitude_m:884,age_ms:800,stale:false,source:'GroundStation GPS',manual:false},
  rocket:{latitude:31.76,longitude:-102.04,age_ms:400,stale:false,source:'RF',manual:false},rrls:{latitude:31.7552,longitude:-102.0418,age_ms:0,stale:false,source:'Saved RRLS (manual)',manual:true},rssi:null}));
 const manifest=JSON.parse(files.get('manifest.json'));
 manifest.entries.push({path:'derived/stations.json',route:'/api/antenna/status',bytes:stations.length,media_type:'application/json',sha256:crypto.createHash('sha256').update(stations).digest('hex')});
 files.set('manifest.json',Buffer.from(JSON.stringify(manifest)));files.set('derived/stations.json',stations);
 const parts=[],central=[];let offset=0;
 for(const [name,data] of files){
  const n=Buffer.from(name),h=Buffer.alloc(30),c=Buffer.alloc(46),crc=zlib.crc32(data);
  h.writeUInt32LE(0x04034b50,0);h.writeUInt16LE(20,4);h.writeUInt32LE(crc,14);h.writeUInt32LE(data.length,18);h.writeUInt32LE(data.length,22);h.writeUInt16LE(n.length,26);
  c.writeUInt32LE(0x02014b50,0);c.writeUInt16LE(20,4);c.writeUInt16LE(20,6);c.writeUInt32LE(crc,16);c.writeUInt32LE(data.length,20);c.writeUInt32LE(data.length,24);c.writeUInt16LE(n.length,28);c.writeUInt32LE(offset,42);
  parts.push(h,n,data);central.push(c,n);offset+=30+n.length+data.length;
 }
 const dir=Buffer.concat(central),e=Buffer.alloc(22);
 e.writeUInt32LE(0x06054b50,0);e.writeUInt16LE(files.size,8);e.writeUInt16LE(files.size,10);e.writeUInt32LE(dir.length,12);e.writeUInt32LE(offset,16);
 return Buffer.concat([...parts,dir,e]);
}
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
 await open('Live diagrams',['Flight: fill system and vehicle on the pad']);
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
 // A bundle with the station positions at export: the map, the 3D map and the
 // 3D replay place the ground station and RRLS from it.
 await page.getByLabel('Open replay file').setInputFiles({name:'stations.gsreplay',mimeType:'application/octet-stream',buffer:bundleWithStations()});
 await page.getByText('Offline replay — stations.gsreplay',{exact:true}).waitFor();
 await open('Map');
 await page.locator('.gs26-map-stations [data-station="GS"]').waitFor({timeout:20000});
 assert.match(await page.locator('.gs26-map-stations').innerText(),/RRLS 31\.75520, -102\.04180 · saved, manual entry/);
 assert.deepEqual(await page.evaluate(()=>window.__gs26_pending_stations),[{role:'gs',lat:31.7535,lon:-102.0505,live:true},{role:'rrls',lat:31.7552,lon:-102.0418,live:false}]);
 await open('3D replay');
 const stationsRecording=JSON.parse(fs.readFileSync(fixture('replay-bundle/flight.manifest.json'),'utf8')).source.recording;
 const recordingPicker=page.locator(`select:has(option[value="${stationsRecording}"])`).first();
 if(await recordingPicker.count())await recordingPicker.selectOption(stationsRecording);
 await page.getByRole('button',{name:'Open flight replay of the selected recording'}).click();
 // The station camera is offered only when the replay has station positions.
 await page.locator('[data-camera="station"]').first().waitFor({state:'attached',timeout:60000});
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
