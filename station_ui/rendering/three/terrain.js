// Terrain for the 3D map and the 3D replay (flight-predict CONTRACT.md, "Terrain module").
//
// loadTerrain(T, origin, opts) builds the ground around the scene origin (the pad) from the
// station's offline-cached DEM (GET /api/terrain/heightmap, a decoded Terrarium grid) and drapes
// the station's map imagery (GET /tiles/{z}/{x}/{y}.jpg, the map tab's source) over it, finer near
// the pad and coarser outward. Fallbacks, in order: DEM -> flat at pad altitude with imagery ->
// the plain reference ground. It never throws and never blocks: the mesh is returned as soon as
// the heightmap is decoded and imagery is drawn into the texture as tiles arrive.
//
// Local frame: gpsToLocal's (east +X, up +Y, north -Z, metres), which keeps the Earth-curvature
// drop, so far terrain and the far-field ring curve away like the existing reference ground.
// Sources: DEM = AWS Terrain Tiles (Terrarium), imagery = Esri World Imagery; see
// docs/backend-contract.md "Terrain and the 3D map".
import {EARTH_RADIUS_M, gpsToLocal} from './replay-scene.js';

export const TERRAIN_DEFAULTS = Object.freeze({
  extent_m: 12000,      // DEM grid side (the far field continues beyond it)
  gridSize: 129,        // DEM samples per side (128 x 128 cells)
  maxTriangles: 60000,  // DEM grid + far-field ring
  maxTextureSize: 2048, // per imagery texture (two: near and far)
  maxTiles: 36,         // imagery tiles per texture (6 x 6)
  maxImageryZoom: 17,
  concurrency: 4,
});
export const IMAGERY_ATTRIBUTION = 'Imagery: Esri, Maxar, Earthstar Geographics and the GIS User Community';
export const DEM_ATTRIBUTION = 'Terrain: Mapzen/Tilezen Terrain Tiles via AWS Open Data (SRTM, USGS 3DEP, GMTED2010, ETOPO1 and others)';
const GROUND_COLOR = 0x415e55;
const FAR_RINGS = 24, FAR_SLICES = 96, FAR_RADIUS_M = 150000;

// ---------------------------------------------------------------- decoding

/** Terrarium RGB to metres: R*256 + G + B/256 - 32768. */
export function terrariumHeight(r, g, b) {return r * 256 + g + b / 256 - 32768;}

/** Decodes RGBA pixels of a Terrarium tile into metres (row-major, row 0 north). */
export function decodeTerrariumPixels(rgba, width, height) {
  const out = new Float32Array(width * height);
  for (let i = 0; i < out.length; i++) out[i] = terrariumHeight(rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2]);
  return out;
}

function base64Bytes(text) {
  if (typeof atob === 'function') {const s = atob(text), b = new Uint8Array(s.length); for (let i = 0; i < s.length; i++) b[i] = s.charCodeAt(i); return b;}
  return new Uint8Array(Buffer.from(text, 'base64'));
}

/**
 * Decodes a `gs26.heightmap` document (GET /api/terrain/heightmap or a bundle's
 * terrain/heightmap.json). Returns null when it is unavailable or malformed.
 */
export function decodeHeightmap(doc) {
  try {
    if (typeof doc === 'string') doc = JSON.parse(doc);
    if (!doc || doc.available !== true || doc.format !== 'gs26.heightmap' || doc.version !== 1 || doc.encoding !== 'u16le-base64') return null;
    const size = doc.size, extent = doc.extent_m;
    if (!Number.isInteger(size) || size < 2 || size > 257 || !(extent > 0) || !Number.isFinite(doc.base_m) || !(doc.scale_m > 0)) return null;
    if (!Number.isFinite(doc.lat_deg) || !Number.isFinite(doc.lon_deg)) return null;
    const bytes = base64Bytes(doc.values);
    if (bytes.length !== size * size * 2) return null;
    const heights = new Float32Array(size * size);
    for (let i = 0; i < heights.length; i++) heights[i] = doc.base_m + (bytes[2 * i] | (bytes[2 * i + 1] << 8)) * doc.scale_m;
    return {size, extent_m: extent, lat: doc.lat_deg, lon: doc.lon_deg, heights, min: doc.min_m, max: doc.max_m,
      zoom: doc.zoom, attribution: typeof doc.attribution === 'string' ? doc.attribution : DEM_ATTRIBUTION};
  } catch {return null;}
}

// ---------------------------------------------------------------- geometry helpers

/** The station's grid frame: equirectangular metres east/north of the grid centre. */
export function localToGps(lat0, lon0, east, north) {
  const rad = Math.PI / 180;
  return [lat0 + north / EARTH_RADIUS_M / rad, lon0 + east / (EARTH_RADIUS_M * Math.max(1e-6, Math.cos(lat0 * rad))) / rad];
}

/** Ground drop of the curved Earth at horizontal distance r from the origin. */
export function curvatureDrop(r) {return Math.sqrt(Math.max(0, EARTH_RADIUS_M * EARTH_RADIUS_M - r * r)) - EARTH_RADIUS_M;}

/** World pixel (256 px tiles) of a coordinate at zoom z. */
export function worldPixel(lat, lon, z) {
  const n = 256 * 2 ** z, rad = Math.max(-85.05, Math.min(85.05, lat)) * Math.PI / 180;
  return [(lon + 180) / 360 * n, (1 - Math.log(Math.tan(rad) + 1 / Math.cos(rad)) / Math.PI) / 2 * n];
}

/** The deepest imagery zoom whose tiles over a square of side extent_m stay within maxTiles. */
export function imageryZoom(lat, extent_m, maxTiles, maxZoom) {
  const across = Math.max(1, Math.floor(Math.sqrt(maxTiles)));
  for (let z = Math.min(22, maxZoom); z > 0; z--) {
    const mpp = 2 * Math.PI * EARTH_RADIUS_M * Math.cos(lat * Math.PI / 180) / (256 * 2 ** z);
    if (extent_m / (256 * mpp) + 1 <= across) return z;
  }
  return 0;
}

/** Grid size that fits the triangle budget (a far-field ring included). */
export function gridSizeFor(maxTriangles, wanted = TERRAIN_DEFAULTS.gridSize) {
  const budget = Math.max(2, (maxTriangles || TERRAIN_DEFAULTS.maxTriangles) - FAR_RINGS * FAR_SLICES * 2);
  return Math.max(9, Math.min(wanted, 257, Math.floor(Math.sqrt(budget / 2)) + 1));
}

/**
 * Local positions of the DEM grid and a height sampler. `hm` from decodeHeightmap;
 * returns {positions: Float32Array (x, y, z per sample), gps: [[lat, lon]], heightAt}.
 */
export function terrainGrid(origin, hm) {
  const n = hm.size, E = hm.extent_m, step = E / (n - 1);
  const positions = new Float32Array(n * n * 3), gps = new Array(n * n);
  // Local metres of the grid centre (the station centres on the pad; tolerate a shift).
  for (let row = 0; row < n; row++) for (let col = 0; col < n; col++) {
    const i = row * n + col, [lat, lon] = localToGps(hm.lat, hm.lon, -E / 2 + col * step, E / 2 - row * step);
    const p = gpsToLocal([lat, lon, hm.heights[i]], origin) || [0, 0, 0];
    positions.set(p, i * 3); gps[i] = [lat, lon];
  }
  const centre = gpsToLocal([hm.lat, hm.lon, origin[2]], origin) || [0, 0, 0];
  const edge = [];
  for (let k = 0; k < n; k++) for (const i of [k, (n - 1) * n + k, k * n, k * n + n - 1]) {
    const x = positions[i * 3], z = positions[i * 3 + 2];
    edge.push(positions[i * 3 + 1] - curvatureDrop(Math.hypot(x, z)));
  }
  const farBase = edge.reduce((a, b) => a + b, 0) / edge.length;
  const gridY = (col, row) => positions[(row * n + col) * 3 + 1];
  const sampleGrid = (x, z) => {
    const fc = (x - centre[0] + E / 2) / step, fr = (z - centre[2] + E / 2) / step;
    const c0 = Math.max(0, Math.min(n - 2, Math.floor(fc))), r0 = Math.max(0, Math.min(n - 2, Math.floor(fr)));
    const tc = Math.max(0, Math.min(1, fc - c0)), tr = Math.max(0, Math.min(1, fr - r0));
    const top = gridY(c0, r0) * (1 - tc) + gridY(c0 + 1, r0) * tc, bottom = gridY(c0, r0 + 1) * (1 - tc) + gridY(c0 + 1, r0 + 1) * tc;
    return {y: top * (1 - tr) + bottom * tr, inside: fc >= 0 && fc <= n - 1 && fr >= 0 && fr <= n - 1};
  };
  const heightAt = (x, z) => {
    if (!Number.isFinite(x) || !Number.isFinite(z)) return 0;
    const s = sampleGrid(x, z);
    return s.inside ? s.y : farBase + curvatureDrop(Math.hypot(x, z));
  };
  return {positions, gps, heightAt, sampleGrid, farBase, centre, step};
}

/** Flat ground at pad altitude (or a given altitude) following the Earth's curvature. */
export function flatHeightAt(origin, altitude_m = origin?.[2] ?? 0) {
  const offset = altitude_m - (origin?.[2] ?? 0);
  return (x, z) => Number.isFinite(x) && Number.isFinite(z) ? offset + curvatureDrop(Math.hypot(x, z)) : offset;
}

/**
 * Vertical shift that puts a track's lowest point on the ground: GPS altitude and the DEM
 * disagree by metres to tens of metres. Bounded (0 beyond maxShift: a mismatch that large
 * is not a datum difference). positions: [[x, y, z]] local metres.
 */
export function groundOffset(positions, heightAt, maxShift = 300) {
  let low = null;
  for (const p of positions || []) if (p && Number.isFinite(p[0]) && Number.isFinite(p[1]) && Number.isFinite(p[2]) && (!low || p[1] < low[1])) low = p;
  if (!low) return 0;
  const d = heightAt(low[0], low[2]) - low[1];
  return Number.isFinite(d) && Math.abs(d) <= maxShift ? d : 0;
}

// ---------------------------------------------------------------- imagery

function makeCanvas(opts, w, h) {
  if (typeof opts.createCanvas === 'function') return opts.createCanvas(w, h);
  if (typeof OffscreenCanvas === 'function') return new OffscreenCanvas(w, h);
  if (typeof document !== 'undefined') {const c = document.createElement('canvas'); c.width = w; c.height = h; return c;}
  return null;
}

async function bundleImage(value) {
  if (!value) return null;
  if (typeof value === 'object' && ('width' in value) && ('height' in value) && !(value instanceof ArrayBuffer) && !ArrayBuffer.isView(value)) return value;
  if (typeof createImageBitmap !== 'function') return null;
  const blob = typeof Blob === 'function' && value instanceof Blob ? value : new Blob([value], {type: 'image/jpeg'});
  return createImageBitmap(blob);
}

/** Resolves one imagery tile from the bundle or the station; null when unavailable. */
function tileLoader(opts) {
  const tiles = opts.bundle?.tiles;
  return async (z, x, y) => {
    try {
      if (tiles) {
        const key = `${z}/${x}/${y}`, value = tiles instanceof Map ? tiles.get(key) : tiles[key];
        if (value) return await bundleImage(value);
        if (!(opts.fetchImage || opts.fetchTile) || opts.bundle?.offline) return null;
      }
      if (opts.fetchTile) return await opts.fetchTile(z, x, y);
      return opts.fetchImage ? await opts.fetchImage(`/tiles/${z}/${x}/${y}.jpg`) : null;
    } catch {return null;}
  };
}

/** Plans one texture: the mercator window around a square of side extent_m. */
export function imageryPlan(lat, lon, extent_m, opts) {
  const maxTiles = opts.maxTiles ?? TERRAIN_DEFAULTS.maxTiles;
  const z = imageryZoom(lat, extent_m, maxTiles, opts.maxImageryZoom ?? TERRAIN_DEFAULTS.maxImageryZoom);
  const [nLat, wLon] = localToGps(lat, lon, -extent_m / 2, extent_m / 2), [sLat, eLon] = localToGps(lat, lon, extent_m / 2, -extent_m / 2);
  const [x0, y0] = worldPixel(nLat, wLon, z), [x1, y1] = worldPixel(sLat, eLon, z);
  const tiles = [];
  for (let tx = Math.floor(x0 / 256); tx <= Math.floor(x1 / 256); tx++) for (let ty = Math.floor(y0 / 256); ty <= Math.floor(y1 / 256); ty++) tiles.push([z, tx, ty]);
  // Nearest tiles first, so the pad fills in before the edges.
  const [cx, cy] = worldPixel(lat, lon, z);
  tiles.sort((a, b) => Math.hypot(a[1] * 256 + 128 - cx, a[2] * 256 + 128 - cy) - Math.hypot(b[1] * 256 + 128 - cx, b[2] * 256 + 128 - cy));
  const max = opts.maxTextureSize ?? TERRAIN_DEFAULTS.maxTextureSize;
  const scale = Math.min(1, max / Math.max(x1 - x0, y1 - y0));
  return {z, x0, y0, x1, y1, tiles: tiles.slice(0, maxTiles), width: Math.max(1, Math.ceil((x1 - x0) * scale)), height: Math.max(1, Math.ceil((y1 - y0) * scale)), scale};
}

/** UVs for coordinates against a plan (flipY textures: v = 0 at the bottom). */
export function planUv(plan, coords) {
  const uv = new Float32Array(coords.length * 2);
  coords.forEach(([lat, lon], i) => {
    const [px, py] = worldPixel(lat, lon, plan.z);
    uv[2 * i] = (px - plan.x0) / (plan.x1 - plan.x0); uv[2 * i + 1] = 1 - (py - plan.y0) / (plan.y1 - plan.y0);
  });
  return uv;
}

/** Draws a plan's tiles into a canvas texture as they arrive. Resolves the count drawn. */
async function paintImagery(T, plan, material, opts, state) {
  const canvas = makeCanvas(opts, plan.width, plan.height);
  const ctx = canvas?.getContext?.('2d');
  if (!ctx) return 0;
  ctx.fillStyle = '#415e55'; ctx.fillRect(0, 0, plan.width, plan.height);
  const texture = new T.CanvasTexture(canvas);
  texture.colorSpace = T.SRGBColorSpace; texture.anisotropy = 4;
  const load = tileLoader(opts);
  let drawn = 0, next = 0, attached = false;
  const worker = async () => {
    while (next < plan.tiles.length && !state.disposed) {
      const [z, x, y] = plan.tiles[next++];
      const image = await load(z, x, y);
      if (!image || state.disposed) continue;
      ctx.drawImage(image, (x * 256 - plan.x0) * plan.scale, (y * 256 - plan.y0) * plan.scale, 256 * plan.scale + .5, 256 * plan.scale + .5);
      image.close?.();
      drawn++;
      if (!attached) {material.map = texture; material.color.set(0xffffff); material.needsUpdate = true; attached = true;}
      texture.needsUpdate = true;
      opts.onUpdate?.();
      await new Promise(r => setTimeout(r, 0));
    }
  };
  await Promise.all(Array.from({length: Math.max(1, opts.concurrency ?? TERRAIN_DEFAULTS.concurrency)}, worker));
  if (!attached) texture.dispose(); else state.textures.push(texture);
  return drawn;
}

// ---------------------------------------------------------------- meshes

function farRing(T, heightAt, edgeSquare, origin, material) {
  // Inner boundary on the DEM square (or the pad for flat ground); rings out to FAR_RADIUS_M.
  const half = edgeSquare / 2, positions = [], index = [];
  for (let ring = 0; ring <= FAR_RINGS; ring++) for (let s = 0; s < FAR_SLICES; s++) {
    const a = s / FAR_SLICES * Math.PI * 2, c = Math.cos(a), sn = Math.sin(a);
    const inner = half > 0 ? half / Math.max(Math.abs(c), Math.abs(sn)) : 1;
    const r = ring === 0 ? inner : inner * Math.exp(Math.log(FAR_RADIUS_M / inner) * ring / FAR_RINGS);
    const x = r * c, z = r * sn;
    // Slightly under the grid edge so the seam never shows sky.
    positions.push(x, heightAt(x, z) - (ring === 0 ? 0.5 : 2), z);
  }
  for (let ring = 0; ring < FAR_RINGS; ring++) for (let s = 0; s < FAR_SLICES; s++) {
    const a = ring * FAR_SLICES + s, b = ring * FAR_SLICES + (s + 1) % FAR_SLICES, c = a + FAR_SLICES, d = b + FAR_SLICES;
    index.push(a, b, c, b, d, c);
  }
  const geometry = new T.BufferGeometry();
  geometry.setAttribute('position', new T.Float32BufferAttribute(positions, 3));
  geometry.setIndex(index); geometry.computeVertexNormals();
  const mesh = new T.Mesh(geometry, material);
  mesh.name = 'terrain-far-field'; mesh.userData.referenceGround = true;
  return mesh;
}

function gridIndices(n, inside) {
  const a = [], b = [];
  for (let r = 0; r < n - 1; r++) for (let c = 0; c < n - 1; c++) {
    const i = r * n + c, quad = [i, i + n, i + 1, i + 1, i + n, i + n + 1];
    (inside(r, c) ? a : b).push(...quad);
  }
  return [a, b];
}

function groundMaterial(T, color = GROUND_COLOR) {
  return new T.MeshLambertMaterial({color, side: T.DoubleSide});
}

function genericGround(T, origin, heightAt) {
  // The plain reference ground: the curved surface at pad altitude (no fabricated terrain).
  const group = new T.Group();group.name = 'terrain-generic';
  group.add(farRing(T, heightAt, 0, origin, new T.MeshBasicMaterial({color: GROUND_COLOR, side: T.DoubleSide})));
  return group;
}

// ---------------------------------------------------------------- loader

function result(T, group, heightAt, source, attribution, state) {
  return {group, heightAt, source, attribution, ready: state.ready || Promise.resolve(), dispose() {
    state.disposed = true;
    group.traverse(o => {o.geometry?.dispose?.(); const m = o.material; for (const x of Array.isArray(m) ? m : m ? [m] : []) x.dispose?.();});
    for (const t of state.textures) t.dispose();
    group.removeFromParent?.();
  }};
}

/**
 * origin: [lat, lon, alt_msl_m] of the scene origin (the pad).
 * opts: {extent_m, fetchJson(path), fetchImage(path), fetchTile?(z, x, y), bundle?: {heightmap, tiles: {"z/x/y": Blob|bytes|image}, offline?},
 *        maxTriangles?, maxTextureSize?, maxTiles?, maxImageryZoom?, imagery?: false, createCanvas?, onUpdate?}
 * Returns {group, heightAt(x, z), source: 'dem' | 'flat-imagery' | 'generic', attribution, ready, dispose()}.
 */
export async function loadTerrain(T, origin, opts = {}) {
  const state = {disposed: false, textures: []};
  const valid = Array.isArray(origin) && origin.length >= 3 && origin.slice(0, 3).every(Number.isFinite);
  const pad = valid ? origin : [0, 0, 0];
  const fallbackHeight = flatHeightAt(pad);
  try {
    if (!valid || !T?.Group) throw Error('invalid origin');
    const extent = Math.max(500, Math.min(60000, Number(opts.extent_m) || TERRAIN_DEFAULTS.extent_m));
    const size = gridSizeFor(opts.maxTriangles, opts.gridSize);
    let hm = null;
    if (opts.bundle?.heightmap) hm = decodeHeightmap(opts.bundle.heightmap);
    if (!hm && opts.fetchJson && !opts.bundle?.offline) {
      try {
        hm = decodeHeightmap(await opts.fetchJson(`/api/terrain/heightmap?lat=${pad[0]}&lon=${pad[1]}&extent_m=${extent}&size=${size}`));
      } catch {hm = null;}
    }
    const imageryWanted = opts.imagery !== false && !!(opts.fetchImage || opts.fetchTile || opts.bundle?.tiles);
    const group = new T.Group();group.name = 'terrain';
    if (hm) {
      const grid = terrainGrid(pad, hm), n = hm.size;
      const position = new T.Float32BufferAttribute(grid.positions, 3);
      // Inner quarter (around the pad) gets its own finer imagery texture.
      const nearCells = Math.max(2, Math.round((n - 1) / 4)), lo = Math.floor((n - 1 - nearCells) / 2), hi = lo + nearCells;
      const [inner, outer] = gridIndices(n, (r, c) => r >= lo && r < hi && c >= lo && c < hi);
      const nearExtent = hm.extent_m * nearCells / (n - 1);
      const plans = imageryWanted ? [imageryPlan(hm.lat, hm.lon, nearExtent, opts), imageryPlan(hm.lat, hm.lon, hm.extent_m, opts)] : [null, null];
      const meshes = [[inner, plans[0], 'terrain-near'], [outer, plans[1], 'terrain-far']].map(([idx, plan, name]) => {
        const g = new T.BufferGeometry();
        g.setAttribute('position', position); g.setIndex(idx);
        if (plan) g.setAttribute('uv', new T.Float32BufferAttribute(planUv(plan, grid.gps), 2));
        g.computeVertexNormals();
        const mesh = new T.Mesh(g, groundMaterial(T, 0x5f7356));mesh.name = name;mesh.userData.terrain = true;
        group.add(mesh); return [mesh, plan];
      });
      group.add(farRing(T, grid.heightAt, hm.extent_m, pad, groundMaterial(T)));
      group.userData = {triangles: (n - 1) * (n - 1) * 2 + FAR_RINGS * FAR_SLICES * 2, gridSize: n, extent_m: hm.extent_m, zoom: hm.zoom,
        imageryZooms: plans.filter(Boolean).map(p => p.z), textureSizes: plans.filter(Boolean).map(p => [p.width, p.height])};
      state.ready = Promise.all(meshes.map(([mesh, plan]) => plan ? paintImagery(T, plan, mesh.material, opts, state) : 0)).catch(() => 0);
      const attribution = [hm.attribution, imageryWanted ? (opts.imageryAttribution || IMAGERY_ATTRIBUTION) : null].filter(Boolean).join(' · ');
      return result(T, group, grid.heightAt, 'dem', attribution, state);
    }
    if (imageryWanted) {
      // No DEM: flat ground at pad altitude with the imagery, if any tile loads.
      const flatExtent = Math.min(extent, 8000), plan = imageryPlan(pad[0], pad[1], flatExtent, opts);
      const probe = await tileLoader(opts)(...plan.tiles[0]);
      if (probe && makeCanvas(opts, 1, 1)) {
        probe.close?.();
        const n = 33, step = flatExtent / (n - 1), positions = new Float32Array(n * n * 3), coords = [];
        for (let r = 0; r < n; r++) for (let c = 0; c < n; c++) {
          const east = -flatExtent / 2 + c * step, north = flatExtent / 2 - r * step, i = r * n + c;
          positions.set([east, fallbackHeight(east, -north), -north], i * 3); coords.push(localToGps(pad[0], pad[1], east, north));
        }
        const g = new T.BufferGeometry();
        g.setAttribute('position', new T.Float32BufferAttribute(positions, 3)); g.setIndex(gridIndices(n, () => true)[0]);
        g.setAttribute('uv', new T.Float32BufferAttribute(planUv(plan, coords), 2)); g.computeVertexNormals();
        const mesh = new T.Mesh(g, groundMaterial(T, 0x5f7356));mesh.name = 'terrain-flat';mesh.userData.terrain = true;group.add(mesh);
        group.add(farRing(T, fallbackHeight, flatExtent, pad, groundMaterial(T)));
        group.userData = {triangles: (n - 1) * (n - 1) * 2 + FAR_RINGS * FAR_SLICES * 2, imageryZooms: [plan.z], textureSizes: [[plan.width, plan.height]]};
        state.ready = paintImagery(T, plan, mesh.material, opts, state).catch(() => 0);
        return result(T, group, fallbackHeight, 'flat-imagery', opts.imageryAttribution || IMAGERY_ATTRIBUTION, state);
      }
    }
    return result(T, genericGround(T, pad, fallbackHeight), fallbackHeight, 'generic', '', state);
  } catch {
    try {return result(T, genericGround(T, pad, fallbackHeight), fallbackHeight, 'generic', '', state);}
    catch {return {group: null, heightAt: fallbackHeight, source: 'generic', attribution: '', ready: Promise.resolve(), dispose() {}};}
  }
}
