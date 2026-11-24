// Flight plans and loss-of-signal prediction in a real browser: the real
// static/ground_map.js and MapLibre draw the mock LOS and flight-plan
// overlays beside the rocket's last fix, the viewer and the stations, and the
// server-rendered Map tab card and flight simulation panel are shown with
// the app's design CSS. Data: the backend contract fixtures and
// tests/fixtures/flight-predict-mock (not real flights). Synthetic tiles; no station is contacted.
//
//   GS_FLIGHT_PREDICT_PREVIEW_DIR=/tmp/fp cargo test --bin groundstation_frontend -- \
//     flight_plans_view::tests::write_browser_preview_fragments los_prediction::tests::card_renders
//   GS_FLIGHT_PREDICT_PREVIEW_DIR=/tmp/fp GS_UI_ARTIFACTS=out/ PLAYWRIGHT_MODULE=… CHROME_BINARY=… \
//     node tests/flight-predict-browser.mjs
//
// Without the fragments only the map part runs.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import zlib from 'node:zlib';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const ui=process.env.FRONTEND_ROOT || new URL('..',import.meta.url).pathname;
const fragments=process.env.GS_FLIGHT_PREDICT_PREVIEW_DIR;
const artifacts=process.env.GS_UI_ARTIFACTS;
const mock=name=>JSON.parse(fs.readFileSync(path.join(ui,'tests/fixtures/flight-predict-mock',name),'utf8'));
const fragment=name=>{const file=fragments&&path.join(fragments,name);return file&&fs.existsSync(file)?fs.readFileSync(file,'utf8'):'';};
// The map uses the backend's own contract fixtures (a stored plan and a
// simulated cut); the panel fragments are rendered from the mocks, whose plan
// carries burn curves.
const real=name=>JSON.parse(fs.readFileSync(path.join(ui,'tests/fixtures/backend-contract',name),'utf8'));
const plan=real('flight-plans/plan.json');
const los=real('los-prediction/simulate.json');
const css=['src/design_system.css'].map(f=>fs.readFileSync(path.join(ui,f),'utf8')).join('\n');
const page_html=(body)=>`<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="/maplibre-gl.css"><style>${css}
body{margin:0;background:#0b1220;color:#e2e8f0;font:14px system-ui,sans-serif}
#map-card{display:flex;flex-direction:column;gap:8px;width:100%;height:100vh;box-sizing:border-box;padding:8px;background:#0f172a}
#map-card h2{margin:0 10px;font-size:1rem}
#ground-map{flex:1;min-height:320px;border-radius:10px;overflow:hidden}
.panel{max-width:1100px;margin:0 auto;padding:16px}</style></head><body>${body}</body></html>`;
const browser=await chromium.launch({headless:true,executablePath:process.env.CHROME_BINARY,args:['--enable-unsafe-swiftshader','--use-angle=swiftshader']});
let failures=0;
function crc32(buf){let c=~0;for(const b of buf){c^=b;for(let k=0;k<8;k++)c=(c>>>1)^(0xedb88320&-(c&1));}return ~c>>>0;}
function chunk(type,data){const len=Buffer.alloc(4);len.writeUInt32BE(data.length);const td=Buffer.concat([Buffer.from(type),data]);const crc=Buffer.alloc(4);crc.writeUInt32BE(crc32(td));return Buffer.concat([len,td,crc]);}
function tilePng(x,y){
 const raw=Buffer.alloc(256*(1+256*3));
 for(let r=0;r<256;r++){raw[r*(769)]=0;for(let c=0;c<256;c++){const grid=r===0||c===0;const o=r*769+1+c*3;
  const base=[118+(x%3)*8,112+(y%3)*8,86];for(let k=0;k<3;k++)raw[o+k]=grid?base[k]-24:base[k]+((r*7+c*13)%11);}}
 const ihdr=Buffer.alloc(13);ihdr.writeUInt32BE(256,0);ihdr.writeUInt32BE(256,4);ihdr[8]=8;ihdr[9]=2;
 return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',ihdr),chunk('IDAT',zlib.deflateSync(raw)),chunk('IEND',Buffer.alloc(0))]);
}
async function tileRoute(route){
 const url=new URL(route.request().url());
 if(url.pathname.startsWith('/tiles/')){
  // Synthetic terrain-coloured PNG tile (deterministic).
  const [,x,y]=url.pathname.split('/').slice(2).map(v=>parseInt(v,10));
  return route.fulfill({status:200,contentType:'image/png',body:tilePng(x,y),headers:{'access-control-allow-origin':'*'}});
 }
 if(url.pathname==='/maplibre-gl.css')return route.fulfill({contentType:'text/css',body:fs.readFileSync(path.join(ui,'static/vendor/maplibre-gl/maplibre-gl.css'))});
 return route.fulfill({contentType:'text/html',body:route.request().url().includes('/plan')?mapPage(false):mapPage(true)});
}
function mapPage(withLos){
 return page_html(`<div id="map-card"><h2>Map</h2>${withLos?fragment('los-card.html'):fragment('plan-legend.html')}<div id="ground-map"></div></div>`);
}
async function mapScene(name,withLos){
 const page=await browser.newPage({viewport:{width:1280,height:860}});
 const errors=[];page.on('pageerror',e=>errors.push(String(e)));if(process.env.GS_DEBUG)page.on('console',m=>console.log('console:',m.text().slice(0,300)));
 await page.route('http://station.test/**',tileRoute);
 await page.goto(withLos?'http://station.test/map':'http://station.test/plan');
 await page.addScriptTag({path:path.join(ui,'static/vendor/maplibre-gl/maplibre-gl.js')});
 await page.addScriptTag({path:path.join(ui,'static/ground_map.js')});
 const center=withLos?los.predicted_now:plan.landing;
 await page.evaluate(({center,plan,los,withLos})=>{
  window.__gs26_tile_cache_enabled=false;window.__gs26_tile_cache_disabled=true;
  window.initGroundMap('http://station.test/tiles/{z}/{x}/{y}.png',center.lat_deg,center.lon_deg,withLos?14.6:13.6,18,'Rocket');
  // The real track ends at the last fix; the viewer stands south-west of the pad.
  const fix=withLos?los.last_fix:plan.points[60];
  window.updateGroundMapMarkers(fix.lat_deg,fix.lon_deg,31.7515,-102.049);
  window.setGroundMapStations([{role:'gs',lat:31.7535,lon:-102.0505,live:true},{role:'rrls',lat:31.7552,lon:-102.0418,live:false}]);
  if(withLos)window.setGroundMapOverlays('los',[los.overlay]);
  window.setGroundMapOverlays('flight_plan',[plan.overlay]);
 },{center,plan,los,withLos});
 await page.waitForFunction(()=>{const m=window.__gs26_ground_map;const s=m?.getSource?.('gs26-map-overlays-source');return !!(s&&m.loaded&&m.isStyleLoaded());},null,{timeout:20000}).catch(()=>{});
 await page.waitForTimeout(2500);
 const result=await page.evaluate(()=>{
  const m=window.__gs26_ground_map;
  if(!m)return {error:window.__gs26_map_runtime_error||'no map',trace:JSON.stringify(window.__gs26_map_init_trace||null).slice(0,800),fallback:!!document.querySelector('#ground-map img')};
  const features=m.querySourceFeatures('gs26-map-overlays-source');
  return {overlays:(window.__gs26_map_overlays||[]).map(o=>o.kind),features:features.length,
   labels:[...document.querySelectorAll('.gs26-map-overlay-label')].map(e=>e.textContent),
   layers:m.getStyle().layers.map(l=>l.id)};
 });
 try{
  assert.deepEqual(errors,[]);
  assert.deepEqual(result.overlays,withLos?['los_prediction','flight_plan']:['flight_plan']);
  assert.ok(result.layers.includes('gs26-map-overlays-track-dash'));
  assert.ok(result.features>0,'overlay features are drawn');
  assert.ok(result.labels.some(l=>/landing/i.test(l)));
  if(withLos){
   assert.ok(result.labels.some(l=>/^Predicted: no signal/.test(l)));
   assert.ok(result.labels.some(l=>/^Last fix/.test(l)));
   if(fragment('los-card.html'))assert.ok(await page.locator('[data-los-state="predicting"]').isVisible());
  }
  console.log('PASS',name,JSON.stringify({features:result.features,labels:result.labels.length}));
 }catch(e){failures++;console.error('FAIL',name,e.message,JSON.stringify(result));}
 if(artifacts){fs.mkdirSync(artifacts,{recursive:true});await page.screenshot({path:path.join(artifacts,name+'.png')});}
 await page.close();
}
await mapScene('map-tab-los-prediction',true);
await mapScene('map-flight-plan-overlay',false);

const sim=fragment('flight-sim.html');
if(sim){
 for(const width of [1280,390]){
  const page=await browser.newPage({viewport:{width,height:900}});
  await page.setContent(page_html(`<div class="panel">${sim}</div>`));
  try{
   assert.ok(await page.locator('[data-flight-plan-result]').isVisible());
   assert.ok(await page.locator('[data-predicted-label]').first().isVisible());
   assert.equal(await page.locator('[data-flight-plan]').count(),2);
   const overflow=await page.evaluate(()=>document.documentElement.scrollWidth-document.documentElement.clientWidth);
   assert.ok(overflow<=1,`no horizontal scroll at ${width}px (${overflow})`);
   console.log('PASS flight-sim-panel',width);
  }catch(e){failures++;console.error('FAIL flight-sim-panel',width,e.message);}
  if(artifacts)await page.screenshot({path:path.join(artifacts,`flight-sim-panel-${width}.png`),fullPage:true});
  await page.close();
 }
}else console.log('SKIP flight-sim-panel (no GS_FLIGHT_PREDICT_PREVIEW_DIR fragments)');
await browser.close();
if(failures)process.exit(1);
