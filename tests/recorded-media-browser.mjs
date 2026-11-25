// Recorded camera video and crew voice from a replay bundle, at /preview of
// the web bundle: the broadcast page says live camera and voice need a
// station, and plays the bundle's clips from blob URLs in step with the
// replay clock, with zero network. The fixture (GroundStation26
// contract_fixtures, replay-bundle/media.gsreplay) has stand-in camera
// segments and one real 0.2 s WAV burst 30 s into the recording.
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE||'playwright');
const base=(process.env.GS_PREVIEW_URL||'http://127.0.0.1:19096').replace(/\/$/,'');
const fixture=name=>fileURLToPath(new URL(`./fixtures/backend-contract/${name}`,import.meta.url));
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY});
const shots=process.env.GS_UI_ARTIFACTS;
try {
 const page=await browser.newPage({viewport:{width:1280,height:860}});const errors=[],requests=[];
 page.on('pageerror',e=>errors.push(e.message));
 page.on('request',r=>{const u=new URL(r.url());if(u.protocol==='blob:'||u.protocol==='data:')return;
  if(['document','script','stylesheet','font','image'].includes(r.resourceType())&&u.origin===new URL(base).origin&&!u.pathname.startsWith('/api'))return;
  if(/\.(wasm|js|glb|json)$/.test(u.pathname)&&u.pathname.startsWith('/assets/'))return;
  requests.push(r.method()+' '+r.url());});
 await page.goto(base+'/preview');
 await page.getByText('Demo mode — no station connected',{exact:true}).waitFor();
 // The demo has no recorded media: the page says so and polls nothing.
 const nav=page.locator('nav[aria-label="Mission navigation"]');
 // The broadcast page is a dedicated view (no navigation rail); open it unless it is already up.
 const mission=async()=>{await page.waitForFunction(()=>document.querySelector('[data-testid=live-camera-needs-station],nav[aria-label="Mission navigation"]'),null,{timeout:30000});
  if(!await page.getByTestId('live-camera-needs-station').count())await nav.locator('a[aria-label="Mission Live"]').first().click();
  await page.getByTestId('live-camera-needs-station').waitFor();};
 await mission();
 await page.getByTestId('live-voice-needs-station').waitFor();
 await page.getByText(/This bundle has no recorded camera video/).waitFor();
 // The media bundle: paused at 30 s, the voice burst (30.0-30.2 s) is on air.
 await page.getByLabel('Open replay file').setInputFiles(fixture('replay-bundle/media.gsreplay'));
 await page.getByText('Replay: media.gsreplay',{exact:true}).waitFor();
 await mission();
 const seek=async ms=>page.getByLabel('Position').evaluate((e,v)=>{e.value=String(v);e.dispatchEvent(new Event('change',{bubbles:true}));},ms);
 if(await page.getByRole('button',{name:'Pause'}).count())await page.getByRole('button',{name:'Pause'}).click();
 await seek(30_000);await mission();
 await page.locator('[data-testid=recorded-media] video').first().waitFor({state:'attached',timeout:20000});
 assert.match(await page.locator('[data-testid=recorded-media] video').first().getAttribute('src'),/^blob:/,'camera clip from a blob URL');
 assert.match(await page.locator('[data-testid=recorded-media] figcaption').first().innerText(),/pad · segment from 00:00:00 UTC/);
 const audio=page.locator('[data-testid=recorded-media] audio');
 await audio.first().waitFor({state:'attached',timeout:20000});
 assert.match(await audio.first().getAttribute('src'),/^blob:/,'voice burst from a blob URL');
 await page.getByText(/Crew · speaker 7/).first().waitFor();
 // The element follows the clock: it plays the WAV (decodable) near the clock's offset.
 const t=await audio.first().evaluate(e=>new Promise(r=>{const done=()=>r({ready:e.readyState,dur:e.duration});if(e.readyState>=1)done();else e.addEventListener('loadedmetadata',done,{once:true});}));
 assert(t.ready>=1&&Math.abs(t.dur-0.2)<0.05,'the WAV burst decodes: '+JSON.stringify(t));
 if(shots)await page.screenshot({path:`${shots}/recorded-media.png`});
 await page.getByRole('button',{name:'Play',exact:true}).click();
 await page.getByRole('button',{name:'Pause',exact:true}).waitFor({timeout:5000});
 // After the burst: nobody on air, the next transmission is named only before it.
 await page.getByText('Nobody is transmitting at this moment.',{exact:false}).waitFor({timeout:20000});
 assert.deepEqual(requests,[],'no network requests');
 assert.deepEqual(errors,[]);
 console.log('PASS: live camera/voice labelled as needing a station; bundle camera and voice clips play from blob URLs on the replay clock; zero network');
} catch(e) {if(shots)await (await browser.contexts()[0].pages())[0].screenshot({path:`${shots}/recorded-media-failure.png`}).catch(()=>{});throw e;} finally {await browser.close();}
