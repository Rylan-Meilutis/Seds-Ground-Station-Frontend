// terrain.js: Terrarium decode, heightmap decode, mesh placement, bundle round trip, fallbacks
// and performance bounds. Synthetic data and the backend contract fixtures only; no network.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import * as T from '../station_ui/rendering/three/three.module.js';
import {gpsToLocal} from '../station_ui/rendering/three/replay-scene.js';
import {terrariumHeight, decodeTerrariumPixels, decodeHeightmap, loadTerrain, terrainGrid, groundOffset, gridSizeFor, imageryPlan, imageryZoom,
  curvatureDrop, localToGps, TERRAIN_DEFAULTS} from '../station_ui/rendering/three/terrain.js';

const fixture = name => JSON.parse(fs.readFileSync(new URL(`./fixtures/backend-contract/${name}.json`, import.meta.url), 'utf8'));
const near = (a, b, tol = 1e-6, msg = '') => assert(Math.abs(a - b) <= tol, `${msg} ${a} != ${b} (±${tol})`);

// ---- Terrarium RGB -> metres
assert.equal(terrariumHeight(128, 0, 0), 0);
assert.equal(terrariumHeight(131, 122, 0), 890);
assert.equal(terrariumHeight(128, 0, 128), 0.5);
assert.equal(terrariumHeight(0, 0, 0), -32768);
const rgba = new Uint8Array([131, 122, 0, 255, 127, 255, 0, 255, 138, 23, 64, 255, 128, 0, 0, 0]);
assert.deepEqual([...decodeTerrariumPixels(rgba, 2, 2)], [890, -1, 2583.25, 0]);
console.log('PASS Terrarium RGB decodes to metres');

// ---- the station's heightmap document (backend contract fixture, real serializer)
const doc = fixture('terrain/heightmap');
const hm = decodeHeightmap(doc);
assert(hm, 'the station heightmap decodes');
assert.equal(hm.size, 17);assert.equal(hm.heights.length, 17 * 17);
assert(hm.heights.every(h => h >= doc.min_m - doc.scale_m && h <= doc.max_m + doc.scale_m), 'heights within the stated range');
// The synthetic tile rises 0.5 m per pixel eastward: the east edge is higher than the west.
assert(hm.heights[8 * 17 + 16] > hm.heights[8 * 17], 'east is higher in the synthetic ramp');
assert.equal(decodeHeightmap(fixture('terrain/heightmap-unavailable')), null, 'an unavailable heightmap is null');
assert.equal(decodeHeightmap({...doc, values: doc.values.slice(0, 8)}), null, 'truncated values are refused');
assert.equal(decodeHeightmap({...doc, version: 2}), null);
assert.equal(decodeHeightmap('not json'), null);
console.log('PASS station heightmap decodes and malformed documents are refused');

/** A synthetic heightmap document in the station's encoding. */
function syntheticDoc(lat, lon, extent, size, metres) {
  const heights = [];
  for (let r = 0; r < size; r++) for (let c = 0; c < size; c++) heights.push(metres(-extent / 2 + c * extent / (size - 1), extent / 2 - r * extent / (size - 1)));
  const base = Math.floor(Math.min(...heights)), bytes = Buffer.alloc(size * size * 2);
  heights.forEach((h, i) => bytes.writeUInt16LE(Math.round((h - base) / 0.1), i * 2));
  return {available: true, format: 'gs26.heightmap', version: 1, lat_deg: lat, lon_deg: lon, extent_m: extent, size, spacing_m: extent / (size - 1), zoom: 12,
    min_m: Math.min(...heights), max_m: Math.max(...heights), encoding: 'u16le-base64', base_m: base, scale_m: 0.1, values: bytes.toString('base64'),
    exact_fraction: 1, coverage: 1, source: 'AWS Terrain Tiles (Terrarium)', attribution: 'Terrain: test'};
}

// ---- mesh placement: the nominal 10k flight over a sloped synthetic DEM
const trajectory = fixture('replay-trajectory/flight');
const pad = [trajectory.points[0].latitude, trajectory.points[0].longitude, trajectory.points[0].altitude_m];
// Ground rises 1 m per 100 m east and sits 6 m above the GPS pad altitude (a datum difference).
const slope = (east) => pad[2] + 6 + east * 0.01;
const slopeDoc = syntheticDoc(pad[0], pad[1], 8000, 65, slope);
const terrain = await loadTerrain(T, pad, {bundle: {heightmap: JSON.stringify(slopeDoc), offline: true}, fetchJson: () => {throw Error('offline bundle must not fetch');}});
assert.equal(terrain.source, 'dem');
assert(terrain.group instanceof T.Group && terrain.group.getObjectByName('terrain-near') && terrain.group.getObjectByName('terrain-far-field'));
assert.equal(terrain.attribution, 'Terrain: test', 'bundle terrain without imagery attributes the DEM only');
near(terrain.heightAt(0, 0), 6, 0.06, 'pad height');
near(terrain.heightAt(1000, 0) - terrain.heightAt(0, 0), 10, 0.15, 'slope eastward');
near(terrain.heightAt(0, -1000), terrain.heightAt(0, 0) + curvatureDrop(1000), 0.1, 'flat northward but curved');
const local = trajectory.points.map(p => gpsToLocal([p.latitude, p.longitude, p.altitude_m], pad));
const offset = groundOffset(local, terrain.heightAt);
const lowest = local.reduce((a, b) => (b[1] < a[1] ? b : a));
near(lowest[1] + offset, terrain.heightAt(lowest[0], lowest[2]), 1e-6, 'the lowest track point sits on the ground');
for (const p of local) assert(p[1] + offset >= terrain.heightAt(p[0], p[2]) - 12, 'no track point is buried');
assert.equal(groundOffset(local, () => 5000), 0, 'an implausible mismatch is not applied');
assert.equal(groundOffset([], terrain.heightAt), 0);
// The grid itself: corner positions follow the station's equirectangular frame.
const grid = terrainGrid(pad, decodeHeightmap(slopeDoc));
const [neLat, neLon] = localToGps(pad[0], pad[1], 4000, 4000), ne = gpsToLocal([neLat, neLon, slope(4000)], pad);
near(grid.positions[(64) * 3], ne[0], 1e-3);near(grid.positions[64 * 3 + 2], ne[2], 1e-3);
terrain.dispose();
console.log('PASS mesh placement: DEM heights, slope, curvature and the track resting on the ground');

// ---- bundle round trip: the GS bundle entry body decodes identically from bytes and from an object
const entryBytes = Buffer.from(JSON.stringify(doc));
const fromBytes = decodeHeightmap(entryBytes.toString('utf8')), fromObject = decodeHeightmap(doc);
assert.deepEqual([...fromBytes.heights], [...fromObject.heights]);
const bundled = await loadTerrain(T, [doc.lat_deg, doc.lon_deg, doc.min_m], {bundle: {heightmap: entryBytes.toString('utf8'), tiles: {}, offline: true}});
assert.equal(bundled.source, 'dem', 'a bundle heightmap gives DEM terrain with no station');
const manifest = fixture('replay-bundle/flight.manifest');
assert.equal(manifest.terrain.included, false, 'the fixture station has no DEM, so its bundle says so');
assert(!manifest.entries.some(e => e.path.startsWith('terrain/')));
bundled.dispose();
console.log('PASS bundle round trip');

// ---- fallbacks: never throws, generic ground last
const generic = await loadTerrain(T, pad, {});
assert.equal(generic.source, 'generic');assert(generic.group);assert.equal(generic.attribution, '');
near(generic.heightAt(0, 0), 0, 1e-9);near(generic.heightAt(10000, 0), curvatureDrop(10000), 1e-6);assert(generic.heightAt(10000, 0) < -7);
assert.equal((await loadTerrain(T, pad, {fetchJson: async () => fixture('terrain/heightmap-unavailable')})).source, 'generic', 'no DEM and no imagery');
assert.equal((await loadTerrain(T, pad, {fetchJson: async () => {throw Error('offline');}})).source, 'generic', 'a failed request');
assert.equal((await loadTerrain(T, pad, {fetchJson: async () => fixture('terrain/heightmap-unavailable'), fetchTile: async () => null, createCanvas: () => null})).source, 'generic', 'imagery absent too');
assert.equal((await loadTerrain(null, pad, {})).source, 'generic', 'no three.js');
assert.equal((await loadTerrain(T, [NaN, 0, 0], {})).source, 'generic', 'invalid origin');
// Flat ground with imagery: a fake canvas and tile stand in for the browser.
const drawn = [];
const fakeCanvas = (w, h) => ({width: w, height: h, getContext: () => ({fillRect() {}, drawImage: (...a) => drawn.push(a), set fillStyle(v) {}})});
const flat = await loadTerrain(T, pad, {fetchJson: async () => ({available: false}), fetchTile: async () => ({width: 256, height: 256}), createCanvas: fakeCanvas, maxTiles: 4});
assert.equal(flat.source, 'flat-imagery');
assert(flat.attribution.includes('Esri'));
await flat.ready;
assert(drawn.length >= 1 && drawn.length <= 4, 'imagery tiles drawn within the budget');
near(flat.heightAt(0, 0), 0, 1e-9, 'flat at pad altitude');
assert(flat.group.getObjectByName('terrain-flat').material.map, 'imagery attached');
flat.dispose();
console.log('PASS fallback chain: DEM -> flat imagery -> generic, never throwing');

// ---- performance bounds
assert.equal(gridSizeFor(TERRAIN_DEFAULTS.maxTriangles), 129);
assert(gridSizeFor(30000) < 129 && 2 * (gridSizeFor(30000) - 1) ** 2 + 24 * 96 * 2 <= 30000);
const big = await loadTerrain(T, pad, {bundle: {heightmap: syntheticDoc(pad[0], pad[1], 12000, 129, () => 900)}, maxTriangles: 60000});
assert(big.group.userData.triangles <= 60000, `triangles ${big.group.userData.triangles}`);
for (const extent of [2000, 12000, 60000]) {
  const plan = imageryPlan(pad[0], pad[1], extent, {maxTiles: 36, maxTextureSize: 2048, maxImageryZoom: 17});
  assert(plan.tiles.length <= 36 && plan.width <= 2048 && plan.height <= 2048, JSON.stringify({extent, n: plan.tiles.length, w: plan.width}));
}
assert(imageryZoom(pad[0], 3000, 36, 17) > imageryZoom(pad[0], 12000, 36, 17), 'finer imagery near the pad than over the whole grid');
big.dispose();
console.log('PASS bounded triangles, tiles and texture sizes');
