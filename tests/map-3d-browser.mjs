// The map tab's 2D/3D switch in a real browser: 2D by default, 3D mounts the actual map-3d.js
// over the 2D map with terrain, the nominal 10k flight coloured by phase, a mock MapOverlay set,
// the stations and the legend; orbit/pinch work; switching back restores 2D. Synthetic data and
// the backend contract fixtures only (no station, no network). CHROME_BINARY selects Chrome.
//   GS_MAP3D_HEIGHTMAP=path  use another heightmap document (for example real DEM, for screenshots)
//   GS_MAP3D_SCREENSHOT_DIR=dir  save a screenshot per viewport width
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import os from 'node:os';
import {spawn} from 'node:child_process';
import assert from 'node:assert/strict';

const three = path.resolve('station_ui/rendering/three');
const fixture = name => fs.readFileSync(path.resolve('tests/fixtures/backend-contract', name + '.json'), 'utf8');
const trajectory = JSON.parse(fixture('replay-trajectory/flight'));
const pad = trajectory.points[0];

/** A synthetic heightmap: a gentle ridge north-west of the pad in the station's encoding. */
function syntheticHeightmap() {
  const size = 129, extent = 7000, base = Math.floor(pad.altitude_m - 30), bytes = Buffer.alloc(size * size * 2);
  for (let r = 0; r < size; r++) for (let c = 0; c < size; c++) {
    const e = -extent / 2 + c * extent / (size - 1), n = extent / 2 - r * extent / (size - 1);
    const h = pad.altitude_m + 60 * Math.exp(-(((e + 1800) / 1200) ** 2 + ((n - 1500) / 900) ** 2)) + 8 * Math.sin(e / 700) * Math.cos(n / 900);
    bytes.writeUInt16LE(Math.round((h - base) / 0.1), (r * size + c) * 2);
  }
  return {available: true, format: 'gs26.heightmap', version: 1, lat_deg: pad.latitude, lon_deg: pad.longitude, extent_m: extent, size, spacing_m: extent / (size - 1), zoom: 12,
    min_m: base, max_m: base + 110, encoding: 'u16le-base64', base_m: base, scale_m: 0.1, values: bytes.toString('base64'), exact_fraction: 1, coverage: 1,
    source: 'synthetic', attribution: 'Terrain: synthetic test grid'};
}
const heightmap = process.env.GS_MAP3D_HEIGHTMAP ? fs.readFileSync(process.env.GS_MAP3D_HEIGHTMAP, 'utf8') : JSON.stringify(syntheticHeightmap());
const lat = pad.latitude, lon = pad.longitude;
const overlays = [
  {id: 'flight_plan:fp-1', kind: 'flight_plan', label: 'Predicted: SWEET V6 8.5 kg', predicted: true,
    tracks: [{id: 'boost', phase: 'boost', style: 'predicted', points: [[lat, lon, 890], [lat + .0015, lon + .0006, 2600]]}, {id: 'coast', phase: 'coast', style: 'predicted', points: [[lat + .0015, lon + .0006, 2600], [lat + .004, lon + .0015, 3900]]},
      {id: 'drogue', phase: 'drogue', style: 'predicted', points: [[lat + .004, lon + .0015, 3900], [lat + .002, lon + .012, 1300]]}, {id: 'main', phase: 'main', style: 'predicted', points: [[lat + .002, lon + .012, 1300], [lat + .0012, lon + .0135, 890]]}],
    markers: [{id: 'landing', kind: 'landing', label: 'Predicted landing', lat_deg: lat + .0012, lon_deg: lon + .0135, alt_msl_m: 890}],
    areas: [{id: 'landing_ellipse', kind: 'landing_ellipse', label: '95% landing area', polygon: Array.from({length: 36}, (_, i) => [lat + .0012 + .0035 * Math.sin(i / 36 * 2 * Math.PI), lon + .0135 + .006 * Math.cos(i / 36 * 2 * Math.PI)])}]},
  {id: 'los', kind: 'los_prediction', label: 'Predicted: no signal', predicted: true, tracks: [{id: 'p', phase: 'drogue', style: 'predicted', points: [[lat + .003, lon + .006, 2400], [lat + .0005, lon + .016, 890]]}],
    markers: [{id: 'n', kind: 'predicted_position', label: 'Predicted position', lat_deg: lat + .002, lon_deg: lon + .01, alt_msl_m: 1700}],
    areas: [{id: 'u', kind: 'position_uncertainty', polygon: Array.from({length: 24}, (_, i) => [lat + .002 + .0012 * Math.sin(i / 24 * 2 * Math.PI), lon + .01 + .0015 * Math.cos(i / 24 * 2 * Math.PI)])}]},
  {id: 'search', kind: 'search_coverage', label: 'Search coverage', tracks: [{id: 's1', style: 'search', label: 'Crew A', points: [[lat - .002, lon + .009], [lat + .003, lon + .009], [lat + .003, lon + .012]]}],
    areas: [{id: 'sw', kind: 'search_swath', polygon: [[lat - .0025, lon + .0085], [lat + .0035, lon + .0085], [lat + .0035, lon + .0095], [lat - .0025, lon + .0095]]}]},
];
const stations = [{role: 'gs', lat: lat - .006, lon: lon - .004, live: true}, {role: 'rrls', lat: lat + .009, lon: lon + .02, live: false}];

const html = String.raw`<!doctype html><meta name="viewport" content="width=device-width,initial-scale=1"><style>body{margin:0;background:#101722;color:#eee;font:14px system-ui}#card{position:relative;height:78vh;width:100%}#ground-map{position:absolute;inset:0;background:repeating-linear-gradient(45deg,#1e293b 0 20px,#243247 20px 40px)}#result{padding:12px;white-space:pre-wrap;overflow-wrap:anywhere}</style>
<div id="card"><div id="ground-map">2D map</div><div id="gs26-map-3d" style="display:none;position:absolute;inset:0"></div></div><pre id="result">Loading…</pre>
<script type="module">
const checks={},wait=(f,ms=15000)=>new Promise((res,rej)=>{const t0=performance.now(),i=setInterval(()=>{let v;try{v=f();}catch{}if(v){clearInterval(i);res(v);}else if(performance.now()-t0>ms){clearInterval(i);rej(Error('timeout: '+f));}},40);});
const finish=()=>{document.querySelector('#result').textContent=JSON.stringify(checks,null,1);document.body.dataset.pass=String(Object.values(checks).every(v=>v===true));};
try{
 const m=await import('/map-3d.js');
 const el=document.getElementById('gs26-map-3d'),map2d=document.getElementById('ground-map');
 // Imagery tiles stand in for the station's: drawn on a canvas, so nothing is fetched.
 const fetchTile=async(z,x,y)=>{const c=new OffscreenCanvas(256,256),g=c.getContext('2d');const h=(x*7+y*13)%5;g.fillStyle=['#8a7a5c','#93835f','#7f7355','#9a8b66','#857858'][h];g.fillRect(0,0,256,256);g.strokeStyle='#6b5f45';g.lineWidth=2;for(let i=0;i<256;i+=64){g.beginPath();g.moveTo(i,0);g.lineTo(i+40,256);g.stroke();}return createImageBitmap(c);};
 checks.defaultIs2d=el.style.display==='none'&&getComputedStyle(map2d).visibility==='visible';
 m.setMapViewMode(el,'2d',{map2d});checks.explicit2dKeeps2d=el.dataset.mapView==='2d'&&!el.__gs26Map3d;
 window.__gs26_pending_stations=${JSON.stringify(stations)};
 const view=m.setMapViewMode(el,'3d',{map2d,fetchTile});
 checks.webgl=el.querySelector('.gs26-map3d')?.dataset.webgl==='true';
 checks.mounted3d=el.dataset.mapView==='3d'&&el.style.display==='block'&&getComputedStyle(map2d).visibility==='hidden';
 view.setTrack(${JSON.stringify(trajectory)});
 view.setTerrain(${heightmap},{check:{radius_m:6000,dem:[{z:12,present:9,checked:9}],imagery:[{z:16,present:0,checked:16}]},dem:{available:true}});
 window.__gs26_map_overlays=${JSON.stringify(overlays)};window.dispatchEvent(new CustomEvent('gs26-map-overlays'));
 const host=el.querySelector('.gs26-map3d');
 await wait(()=>host.dataset.terrainSource==='dem');
 await wait(()=>host.dataset.imageryReady==='true');
 await wait(()=>Number(host.dataset.rendered)>0);
 checks.terrainFromDem=host.dataset.terrainSource==='dem';
 checks.trackColouredByPhase=host.dataset.trackPhases==='pad,boost,coast,drogue,landed'&&view.trackGroup.children.filter(c=>c.name.startsWith('track-')).length===5;
 checks.trackColours=view.trackGroup.getObjectByName('track-boost').material.color.getHexString()==='f97316'&&view.trackGroup.getObjectByName('track-drogue').material.color.getHexString()==='38bdf8';
 checks.apogeeMarker=!!view.trackGroup.getObjectByName('apogee-marker');
 checks.dropLinesAndShadow=!!view.trackGroup.getObjectByName('drop-lines')&&!!view.trackGroup.getObjectByName('ground-track');
 const legend=[...host.querySelectorAll('.gs26-map3d-legend [data-key]')].map(n=>n.dataset.key);
 checks.legend=JSON.stringify(legend)===JSON.stringify(['pad','boost','coast','apogee','drogue','landed','flight_plan','los_prediction','search_coverage']);
 checks.overlays=host.dataset.overlayCount==='3'&&view.overlayGroup.children.some(c=>c.userData.dashed&&c.material.opacity===.55)&&view.overlayGroup.children.some(c=>c.name==='overlay-area-position_uncertainty'&&c.material.color.getHexString()==='f472b6');
 checks.stations=!!view.stationGroup.getObjectByName('station-gs')&&!!view.stationGroup.getObjectByName('pad-marker');
 const pad=view.stationGroup.getObjectByName('pad-marker');checks.padOnTerrain=Math.abs(pad.position.y-20-view.heightAt(0,0))<1e-6;
 checks.attribution=host.querySelector('.gs26-map3d-attribution').textContent.includes('Terrain:');
 checks.coverageInfo=host.querySelector('.gs26-map3d-info').textContent.includes('DEM 100 %');
 checks.noOverflow=document.documentElement.scrollWidth<=innerWidth;
 window.__view=view;finish();
}catch(e){checks.error=String(e&&e.stack||e);document.body.dataset.pass='false';document.querySelector('#result').textContent=JSON.stringify(checks,null,1);}
</script>`;

const server = http.createServer((req, res) => {
  const pathname = new URL(req.url, 'http://localhost').pathname;
  if (pathname === '/') {res.setHeader('Content-Type', 'text/html; charset=utf-8');res.end(html);return;}
  const file = path.resolve(three, pathname.slice(1));
  if (!file.startsWith(three + path.sep) || !fs.existsSync(file)) {res.statusCode = 404;res.end();return;}
  res.setHeader('Content-Type', 'text/javascript');res.end(fs.readFileSync(file));
});
await new Promise(r => server.listen(0, '127.0.0.1', r));
const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'gs-map3d-'));
const chrome = spawn(process.env.CHROME_BINARY || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', ['--headless', '--no-sandbox', '--no-first-run', '--disable-background-networking', '--disable-component-update', '--disable-sync', '--disable-extensions',
  '--use-gl=angle', '--use-angle=swiftshader', '--ignore-gpu-blocklist', '--enable-webgl', '--enable-unsafe-swiftshader', '--remote-debugging-port=0', '--remote-allow-origins=*', '--user-data-dir=' + profile, 'about:blank'], {stdio: ['ignore', 'ignore', 'pipe']});
let stderr = '';chrome.stderr.on('data', d => stderr = (stderr + d).slice(-65536));
let ws, next = 0;const pending = new Map(), events = [];
const sleep = ms => new Promise(r => setTimeout(r, ms));
try {
  for (let i = 0; i < 100 && !fs.existsSync(profile + '/DevToolsActivePort'); i++) await sleep(100);
  if (!fs.existsSync(profile + '/DevToolsActivePort')) throw Error('Chrome did not start: ' + stderr.slice(-1000));
  const port = fs.readFileSync(profile + '/DevToolsActivePort', 'utf8').split('\n')[0];
  const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  ws = new WebSocket(targets.find(t => t.type === 'page').webSocketDebuggerUrl);await new Promise((r, j) => {ws.onopen = r;ws.onerror = j;});
  ws.onmessage = e => {const m = JSON.parse(e.data);if (m.id) {const {resolve, reject} = pending.get(m.id) || {};pending.delete(m.id);m.error ? reject?.(Error(JSON.stringify(m.error))) : resolve?.(m.result);} else events.push(m);};
  const call = (method, params = {}) => new Promise((resolve, reject) => {const id = ++next;pending.set(id, {resolve, reject});ws.send(JSON.stringify({id, method, params}));});
  const value = expression => call('Runtime.evaluate', {expression, returnByValue: true, awaitPromise: true}).then(r => r.result.value);
  await call('Runtime.enable');await call('Page.enable');
  for (const width of [1280, 390]) {
    await call('Emulation.setDeviceMetricsOverride', {width, height: 860, deviceScaleFactor: 1, mobile: width < 500});
    await call('Page.navigate', {url: `http://127.0.0.1:${server.address().port}/`});
    let status;
    for (let i = 0; i < 300; i++) {await sleep(100);status = JSON.parse(await value('JSON.stringify({pass:document.body?.dataset.pass,result:document.querySelector("#result")?.textContent})'));if (status.pass !== undefined) break;}
    console.log(`width ${width}:`, status.result);
    if (status.pass !== 'true') {console.log('Runtime errors:', JSON.stringify(events.filter(e => e.method === 'Runtime.exceptionThrown')).slice(0, 4000));throw Error('3D map checks failed');}
    // A closer, oblique view for the screenshot; then interaction checks.
    await value('(()=>{const v=window.__view;v.theta=-2.2;v.phi=1.1;v.distance=Math.max(v.distance*0.75,4000);v.invalidate();return true;})()');
    await sleep(600);
    if (process.env.GS_MAP3D_SCREENSHOT_DIR) {
      const shot = await call('Page.captureScreenshot', {format: 'png'});
      fs.writeFileSync(path.join(process.env.GS_MAP3D_SCREENSHOT_DIR, `map-3d-${process.env.GS_MAP3D_HEIGHTMAP ? 'dem' : 'synthetic'}-${width}.png`), Buffer.from(shot.data, 'base64'));
    }
    const theta = await value('window.__view.theta');
    await call('Input.dispatchMouseEvent', {type: 'mousePressed', x: 300, y: 300, button: 'left', clickCount: 1});
    await call('Input.dispatchMouseEvent', {type: 'mouseMoved', x: 360, y: 320, button: 'left', buttons: 1});
    await call('Input.dispatchMouseEvent', {type: 'mouseReleased', x: 360, y: 320, button: 'left', clickCount: 1});
    assert.notEqual(await value('window.__view.theta'), theta, 'a pointer drag orbits');
    if (width < 500) {
      const distance = await value('window.__view.distance');
      await call('Input.dispatchTouchEvent', {type: 'touchStart', touchPoints: [{x: 120, y: 300, id: 1}, {x: 260, y: 300, id: 2}]});
      await call('Input.dispatchTouchEvent', {type: 'touchMove', touchPoints: [{x: 90, y: 300, id: 1}, {x: 300, y: 300, id: 2}]});
      await call('Input.dispatchTouchEvent', {type: 'touchEnd', touchPoints: []});
      assert.notEqual(await value('window.__view.distance'), distance, 'a two-finger pinch zooms');
    }
    const back = await value(`import('/map-3d.js').then(m=>{const el=document.getElementById('gs26-map-3d');m.setMapViewMode(el,'2d',{map2d:document.getElementById('ground-map')});return JSON.stringify({mode:el.dataset.mapView,hidden:el.style.display,visible:getComputedStyle(document.getElementById('ground-map')).visibility,children:el.children.length});})`);
    assert.deepEqual(JSON.parse(back), {mode: '2d', hidden: 'none', visible: 'visible', children: 0}, 'switching back restores the 2D map and disposes the 3D view');
    const detached = await value(`import('/map-3d.js').then(m=>{const el=document.getElementById('gs26-map-3d');const v=m.setMapViewMode(el,'3d',{map2d:document.getElementById('ground-map')});el.remove();m.disposeDetached();return v.disposed===true&&!el.__gs26Map3d;})`);
    assert.equal(detached, true, 'a view whose map left the page is disposed');
  }
  console.log('PASS map tab 2D/3D switch, phase-coloured 3D flight, overlays, stations, legend and touch at desktop and phone widths');
} catch (error) {console.log('Chrome log:', stderr.slice(-2000));throw error;}
finally {ws?.close();chrome.kill('SIGTERM');await new Promise(r => server.close(r));await Promise.race([new Promise(r => chrome.once('exit', r)), sleep(3000)]);fs.rmSync(profile, {recursive: true, force: true});}
