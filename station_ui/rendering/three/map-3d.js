// The map tab's 3D view: the station's terrain and imagery (terrain.js, the same offline-cached
// sources as the 2D map), the rocket track coloured by flight phase, map overlays (flight plans,
// loss-of-signal predictions, search coverage), the stations and the pad. Phases, overlays and
// stations all come from the station; nothing here derives physics or phases.
//
// Inputs (all optional, any order):
//   view.setTrack(doc, {at_ms})  doc = GET /api/flight_phases or GET /api/recordings/{id}/replay/trajectory
//   view.setTerrain(heightmapDoc, status)   GET /api/terrain/heightmap (+ /api/terrain/status)
//   window.__gs26_map_overlays + 'gs26-map-overlays' event   MapOverlay[] (CONTRACT.md)
//   window.__gs26_pending_stations + 'gs26-map-stations' event   [{role, lat, lon, live}]
//   window.__gs26_flight_track + 'gs26-flight-track' event   the same doc as setTrack
// Rendering is on demand (no continuous loop) with the pixel ratio capped, for phones and the Pi.
import * as T from './three.module.js';
import {gpsToLocal, validGps} from './replay-scene.js';
import {loadTerrain, groundOffset, TERRAIN_DEFAULTS} from './terrain.js';

export const PHASE_COLORS = Object.freeze({pad: '#94a3b8', boost: '#f97316', coast: '#facc15', apogee: '#ffffff', drogue: '#38bdf8', main: '#22c55e', landed: '#a78bfa', unknown: '#64748b'});
export const PHASE_LABELS = Object.freeze({pad: 'Pad', boost: 'Powered ascent', coast: 'Coast', apogee: 'Apogee', drogue: 'Drogue descent', main: 'Main descent', landed: 'Landed', unknown: 'Unknown'});
export const PHASE_ORDER = Object.freeze(['pad', 'boost', 'coast', 'apogee', 'drogue', 'main', 'landed']);
export const LOS_COLOR = '#f472b6', PLAN_ELLIPSE_COLOR = '#a78bfa', SWATH_COLOR = '#22c55e', UNCOVERED_COLOR = '#ef4444';
export const SEARCHER_COLORS = Object.freeze(['#0ea5e9', '#eab308', '#ec4899', '#14b8a6', '#f97316', '#8b5cf6']);
const VIEWS = new Set();
const PREDICTED_OPACITY = 0.55, MAX_POINTS = 8192, MAX_OVERLAYS = 24, MAX_LABELS = 32, MAX_DROP_LINES = 24;

export function phaseColor(phase) {return PHASE_COLORS[phase] || PHASE_COLORS.unknown;}

/** The phase at t: [start_ms, end_ms) segments; the last also holds its end; null before the first. */
export function phaseAt(phases, t) {
  if (!Array.isArray(phases) || !Number.isFinite(t)) return null;
  let found = -1;
  for (let i = 0; i < phases.length; i++) if (phases[i].start_ms <= t) found = i; else break;
  if (found < 0) return null;
  const s = phases[found], last = found === phases.length - 1;
  if (Number.isFinite(s.end_ms) && (t > s.end_ms || (t === s.end_ms && !last))) return null;
  return s.phase;
}

/** A point's phase: the station's per-point phase, else the segment it falls in, else unknown. */
export function pointPhase(point, phases) {return (typeof point?.phase === 'string' && point.phase) || phaseAt(phases, point?.timestamp_ms) || 'unknown';}

/** Contiguous runs of one phase; consecutive runs share their boundary point so the line is unbroken. */
export function phaseRuns(points, phases) {
  const runs = [];
  for (const p of (Array.isArray(points) ? points : []).slice(0, MAX_POINTS)) {
    if (!validGps([p?.latitude, p?.longitude, p?.altitude_m])) continue;
    const phase = pointPhase(p, phases), last = runs[runs.length - 1];
    if (p.break_before === true && last) {runs.push({phase, points: [p]}); continue;}
    if (last && last.phase === phase) last.points.push(p);
    else {if (last) runs.push({phase, points: [last.points[last.points.length - 1], p]}); else runs.push({phase, points: [p]});}
  }
  return runs;
}

/** The track up to the replay cursor (all of it when at_ms is not a number). */
export function trackUntil(doc, atMs) {
  const points = Array.isArray(doc?.points) ? doc.points : [];
  return Number.isFinite(atMs) ? points.filter(p => p.timestamp_ms <= atMs) : points;
}

/** Legend rows for what is drawn: phases present (in order), the apogee marker and overlay kinds. */
export function legendEntries(doc, overlays = [], atMs) {
  const present = new Set(phaseRuns(trackUntil(doc, atMs), doc?.phases).map(r => r.phase));
  const rows = PHASE_ORDER.filter(p => p !== 'apogee' && present.has(p)).map(p => ({key: p, label: PHASE_LABELS[p], color: PHASE_COLORS[p], style: 'solid'}));
  if (present.has('unknown')) rows.push({key: 'unknown', label: PHASE_LABELS.unknown, color: PHASE_COLORS.unknown, style: 'solid'});
  if (apogeeOf(doc, atMs)) {
    const after = rows.findIndex(r => r.key === 'drogue' || r.key === 'main' || r.key === 'landed');
    rows.splice(after < 0 ? rows.length : after, 0, {key: 'apogee', label: PHASE_LABELS.apogee, color: PHASE_COLORS.apogee, style: 'marker'});
  }
  const kinds = new Set((Array.isArray(overlays) ? overlays : []).map(o => o?.kind));
  if (kinds.has('flight_plan')) rows.push({key: 'flight_plan', label: 'Predicted flight plan', color: PHASE_COLORS.boost, style: 'dashed'});
  if (kinds.has('los_prediction')) rows.push({key: 'los_prediction', label: 'Predicted: no signal', color: LOS_COLOR, style: 'dashed'});
  if (kinds.has('search_coverage')) rows.push({key: 'search_coverage', label: 'Search coverage', color: SWATH_COLOR, style: 'area'});
  return rows.filter((r, i) => rows.findIndex(x => x.key === r.key) === i);
}

/** The apogee position: the station's apogee event (positioned), else none. */
export function apogeeOf(doc, atMs) {
  const e = (Array.isArray(doc?.events) ? doc.events : []).find(x => x?.kind === 'apogee' && Number.isFinite(x.latitude) && Number.isFinite(x.longitude) && Number.isFinite(x.altitude_m));
  return e && (!Number.isFinite(atMs) || e.t_ms <= atMs) ? e : null;
}

/** Style of one overlay element (CONTRACT.md palette). */
export function overlayStyle(overlay, element = {}, index = 0) {
  const kind = overlay?.kind;
  if (kind === 'los_prediction') return {color: LOS_COLOR, opacity: element.kind === 'last_fix' ? 1 : 0.9, dashed: element.kind !== 'last_fix', fill: 0.15, ring: element.kind === 'predicted_position'};
  if (kind === 'search_coverage') {
    if (element.kind === 'search_swath') return {color: SWATH_COLOR, fill: 0.30, dashed: false, opacity: 0.9};
    if (element.kind === 'uncovered') return {color: UNCOVERED_COLOR, fill: 0.25, dashed: false, opacity: 0.9};
    return {color: SEARCHER_COLORS[index % SEARCHER_COLORS.length], opacity: 1, dashed: false};
  }
  if (element.kind === 'landing_ellipse') return {color: PLAN_ELLIPSE_COLOR, fill: 0.12, dashed: true, opacity: 0.9};
  const predicted = overlay?.predicted !== false || element.style === 'predicted';
  return {color: phaseColor(element.phase), opacity: predicted ? PREDICTED_OPACITY : 1, dashed: predicted};
}

// ------------------------------------------------------------------ view

const color = c => new T.Color(c);
function lineMaterial(style) {
  const common = {color: color(style.color), transparent: style.opacity < 1, opacity: style.opacity ?? 1, depthWrite: style.opacity >= 1};
  return style.dashed ? new T.LineDashedMaterial({...common, dashSize: 40, gapSize: 25}) : new T.LineBasicMaterial(common);
}
function makeLine(positions, style) {
  const g = new T.BufferGeometry();g.setAttribute('position', new T.Float32BufferAttribute(positions, 3));
  const line = new T.Line(g, lineMaterial(style));if (style.dashed) line.computeLineDistances();
  return line;
}
function disposeTree(o) {o?.traverse?.(n => {n.geometry?.dispose?.(); for (const m of [].concat(n.material || [])) {m.map?.dispose?.(); m.dispose?.();}});}

export class Map3dView {
  constructor(container, opts = {}) {
    this.container = container; this.opts = opts; this.doc = null; this.atMs = undefined;
    this.overlays = []; this.stations = []; this.dropLines = opts.dropLines !== false; this.shadow = opts.shadow !== false;
    this.heightmap = null; this.status = null; this.origin = null; this.terrain = null; this.terrainKey = '';
    this.theta = -Math.PI / 2; this.phi = 1.05; this.distance = 6000; this.target = new T.Vector3();
    this.labels = [];
    const host = document.createElement('div');host.className = 'gs26-map3d';
    host.style.cssText = 'position:absolute;inset:0;overflow:hidden;background:#0f172a;touch-action:none;user-select:none';
    host.tabIndex = 0;host.setAttribute('role', 'img');host.setAttribute('aria-label', '3D map: terrain, flight track coloured by phase, predictions and stations. Drag to orbit, two fingers or right-drag to pan, pinch or scroll to zoom.');
    container.append(host);this.host = host;
    try {
      this.renderer = new T.WebGLRenderer({antialias: !opts.lowPower, alpha: false, powerPreference: 'low-power'});
    } catch {this.renderer = null;}
    if (!this.renderer) {host.textContent = '3D needs WebGL, which this device does not provide. The 2D map stays available.';host.dataset.webgl = 'false';return;}
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, opts.maxPixelRatio || 1.5));
    this.renderer.outputColorSpace = T.SRGBColorSpace;
    host.append(this.renderer.domElement);this.renderer.domElement.style.cssText = 'display:block;width:100%;height:100%';
    this.scene = new T.Scene();this.scene.background = new T.Color(0x9ec5e8);this.scene.fog = new T.Fog(0x9ec5e8, 60000, 160000);
    this.scene.add(new T.HemisphereLight(0xffffff, 0x4b5563, 2.2));const sun = new T.DirectionalLight(0xffffff, 1.4);sun.position.set(-3, 5, 2);this.scene.add(sun);
    this.camera = new T.PerspectiveCamera(50, 1, 5, 400000);
    this.world = new T.Group();this.scene.add(this.world);
    this.trackGroup = new T.Group();this.overlayGroup = new T.Group();this.stationGroup = new T.Group();this.world.add(this.trackGroup, this.overlayGroup, this.stationGroup);
    this.labelLayer = document.createElement('div');this.labelLayer.style.cssText = 'position:absolute;inset:0;pointer-events:none;font:12px/1.2 system-ui;color:#f8fafc';host.append(this.labelLayer);
    this.legend = document.createElement('div');this.legend.className = 'gs26-map3d-legend';this.legend.setAttribute('role', 'list');this.legend.setAttribute('aria-label', 'Flight phase legend');
    this.legend.style.cssText = 'position:absolute;left:8px;bottom:28px;max-width:calc(100% - 16px);display:flex;flex-wrap:wrap;gap:4px 10px;padding:6px 8px;border-radius:8px;background:#0f172ad9;color:#e2e8f0;font:12px/1.3 system-ui;pointer-events:none';
    this.info = document.createElement('div');this.info.className = 'gs26-map3d-info';this.info.setAttribute('role', 'status');
    this.info.style.cssText = 'position:absolute;right:8px;top:8px;max-width:min(320px,calc(100% - 16px));padding:5px 8px;border-radius:8px;background:#0f172ad9;color:#cbd5e1;font:11px/1.35 system-ui;pointer-events:none';
    this.attribution = document.createElement('div');this.attribution.className = 'gs26-map3d-attribution';
    this.attribution.style.cssText = 'position:absolute;right:0;bottom:0;max-width:100%;padding:2px 6px;background:#ffffffcc;color:#1e293b;font:10px/1.3 system-ui;pointer-events:none';
    host.append(this.legend, this.info, this.attribution);
    this.controls();
    this.onOverlays = () => this.setOverlays(window.__gs26_map_overlays);
    this.onStations = () => this.setStations(window.__gs26_pending_stations);
    this.onTrack = () => this.setTrack(window.__gs26_flight_track);
    window.addEventListener('gs26-map-overlays', this.onOverlays);window.addEventListener('gs26-map-stations', this.onStations);window.addEventListener('gs26-flight-track', this.onTrack);
    this.resizeObserver = typeof ResizeObserver === 'function' ? new ResizeObserver(() => this.invalidate()) : null;this.resizeObserver?.observe(host);
    if (Array.isArray(window.__gs26_map_overlays)) this.overlays = window.__gs26_map_overlays.slice(0, MAX_OVERLAYS);
    if (Array.isArray(window.__gs26_pending_stations)) this.stations = window.__gs26_pending_stations;
    if (window.__gs26_flight_track) this.doc = window.__gs26_flight_track;
    host.dataset.webgl = 'true';VIEWS.add(this);this.rebuild();
  }

  // ---- inputs
  setTrack(doc, {at_ms} = {}) {this.doc = doc && typeof doc === 'object' ? doc : null;this.atMs = at_ms;this.rebuild();}
  setOverlays(list) {this.overlays = Array.isArray(list) ? list.slice(0, MAX_OVERLAYS) : [];this.rebuildOverlays();this.renderLegend();this.invalidate();}
  setStations(list) {this.stations = Array.isArray(list) ? list : [];this.rebuildStations();this.invalidate();}
  setTerrain(heightmap, status) {this.heightmap = heightmap || null;if (status !== undefined) this.status = status;this.terrainKey = '';this.rebuild();}
  setOrigin(origin) {this.fallbackOrigin = origin;this.rebuild();}
  setDropLines(on) {this.dropLines = !!on;this.rebuildTrack();this.invalidate();}
  setShadow(on) {this.shadow = !!on;this.rebuildTrack();this.invalidate();}

  /** The scene origin: the track's first fix (the pad), else the configured fallback. */
  pickOrigin() {
    const first = (this.doc?.points || []).find(p => validGps([p.latitude, p.longitude, p.altitude_m]));
    if (first) return [first.latitude, first.longitude, first.altitude_m];
    const f = this.fallbackOrigin;
    if (Array.isArray(f) && Number.isFinite(f[0]) && Number.isFinite(f[1])) return [f[0], f[1], Number.isFinite(f[2]) ? f[2] : (this.heightmap?.min_m ?? 0)];
    return null;
  }

  async rebuild() {
    if (!this.renderer) return;
    const origin = this.pickOrigin();
    const moved = !this.origin || !origin || Math.hypot(...(gpsToLocal([origin[0], origin[1], this.origin[2]], this.origin) || [1e9, 0, 0]).filter((_, i) => i !== 1)) > 1500;
    if (origin && (moved || !this.terrain)) {this.origin = origin;this.terrainKey = '';this.resetCamera = true;}
    const key = this.origin ? `${this.origin.join(',')}:${this.heightmap ? (this.heightmap.available ? this.heightmap.lat_deg + ',' + this.heightmap.lon_deg : 'none') : 'pending'}` : '';
    if (this.origin && key !== this.terrainKey) {
      this.terrainKey = key;
      const generation = (this.generation = (this.generation || 0) + 1);
      const hm = this.heightmap?.available ? this.heightmap : null;
      const terrain = await loadTerrain(T, this.origin, {
        extent_m: this.opts.extent_m || TERRAIN_DEFAULTS.extent_m,
        // The heightmap comes from the app (it holds the station credentials); imagery uses the
        // 2D map's own tile source and caches (ground_map.js gs26FetchMapTileBitmap).
        bundle: {heightmap: hm, tiles: this.opts.tiles || null},
        fetchTile: this.opts.fetchTile || (typeof window.gs26FetchMapTileBitmap === 'function' ? (z, x, y) => window.gs26FetchMapTileBitmap(z, x, y) : null),
        fetchImage: this.opts.fetchImage,
        maxTextureSize: Math.min(this.opts.maxTextureSize || (this.opts.lowPower ? 1024 : 2048), this.renderer.capabilities.maxTextureSize || 2048),
        maxTriangles: this.opts.lowPower ? 30000 : 60000,
        maxImageryZoom: this.opts.maxImageryZoom || 17,
        onUpdate: () => this.invalidate(),
      });
      if (generation !== this.generation) {terrain.dispose();return;}
      if (this.terrain) {this.world.remove(this.terrain.group);this.terrain.dispose();}
      this.terrain = terrain;if (terrain.group) this.world.add(terrain.group);
      this.host.dataset.terrainSource = terrain.source;
      terrain.ready.then(() => {this.host.dataset.imageryReady = 'true';this.renderInfo();this.invalidate();});
    }
    this.rebuildTrack();this.rebuildOverlays();this.rebuildStations();this.renderLegend();this.renderInfo();
    if (this.resetCamera) {this.fitCamera();this.resetCamera = false;}
    this.invalidate();
  }

  heightAt(x, z) {return this.terrain ? this.terrain.heightAt(x, z) : 0;}
  local(lat, lon, alt) {return this.origin ? gpsToLocal([lat, lon, alt], this.origin) : null;}

  rebuildTrack() {
    if (!this.renderer) return;
    this.trackGroup.children.slice().forEach(c => {this.trackGroup.remove(c);disposeTree(c);});
    this.markers = [];
    if (!this.origin || !this.doc) {this.offset = 0;return;}
    const points = trackUntil(this.doc, this.atMs), runs = phaseRuns(points, this.doc.phases);
    const all = runs.flatMap(r => r.points.map(p => this.local(p.latitude, p.longitude, p.altitude_m))).filter(Boolean);
    this.offset = groundOffset(all, (x, z) => this.heightAt(x, z));
    this.host.dataset.trackRuns = String(runs.length);this.host.dataset.trackPhases = [...new Set(runs.map(r => r.phase))].join(',');
    for (const run of runs) {
      const pos = run.points.flatMap(p => {const l = this.local(p.latitude, p.longitude, p.altitude_m);return l ? [l[0], l[1] + this.offset, l[2]] : [];});
      if (pos.length < 6) continue;
      const line = makeLine(pos, {color: phaseColor(run.phase), opacity: 1});line.name = 'track-' + run.phase;line.userData.phase = run.phase;this.trackGroup.add(line);
      if (this.shadow) {
        const ground = [];for (let i = 0; i < pos.length; i += 3) ground.push(pos[i], this.heightAt(pos[i], pos[i + 2]) + 1.5, pos[i + 2]);
        const s = makeLine(ground, {color: '#0f172a', opacity: 0.45});s.name = 'ground-track';this.trackGroup.add(s);
      }
    }
    if (this.dropLines && all.length) {
      const step = Math.max(1, Math.ceil(points.length / MAX_DROP_LINES)), seg = [];
      for (let i = 0; i < points.length; i += step) {
        const l = this.local(points[i].latitude, points[i].longitude, points[i].altitude_m);if (!l) continue;
        const y = l[1] + this.offset, g = this.heightAt(l[0], l[2]);if (y - g < 20) continue;
        seg.push(l[0], y, l[2], l[0], g, l[2]);
      }
      if (seg.length) {const g = new T.BufferGeometry();g.setAttribute('position', new T.Float32BufferAttribute(seg, 3));const d = new T.LineSegments(g, new T.LineBasicMaterial({color: 0xe2e8f0, transparent: true, opacity: 0.25}));d.name = 'drop-lines';this.trackGroup.add(d);}
    }
    const apogee = apogeeOf(this.doc, this.atMs);
    if (apogee) this.addMarker(this.trackGroup, apogee.latitude, apogee.longitude, apogee.altitude_m, PHASE_COLORS.apogee, `Apogee ${Math.round(apogee.altitude_m - (this.origin?.[2] ?? 0))} m AGL`, {outline: '#0f172a', size: 1.4, name: 'apogee-marker'});
    const last = points.filter(p => validGps([p.latitude, p.longitude, p.altitude_m])).at(-1);
    if (last) this.addMarker(this.trackGroup, last.latitude, last.longitude, last.altitude_m, '#e8c77f', 'Rocket', {name: 'rocket-marker'});
  }

  /** A sphere (or ring) marker with an HTML label; msl altitude uses the track offset, null drapes it on the ground. */
  addMarker(group, lat, lon, alt, fill, label, o = {}) {
    const l = this.local(lat, lon, Number.isFinite(alt) ? alt : this.origin[2]);if (!l) return;
    const y = Number.isFinite(alt) ? l[1] + (this.offset || 0) : this.heightAt(l[0], l[2]) + 2;
    const r = 18 * (o.size || 1);
    const mesh = o.ring ? new T.Mesh(new T.TorusGeometry(r, r * 0.22, 8, 32), new T.MeshBasicMaterial({color: color(fill), transparent: true, opacity: 0.95})) : new T.Mesh(new T.SphereGeometry(r, 16, 12), new T.MeshBasicMaterial({color: color(fill)}));
    if (o.ring) mesh.rotation.x = Math.PI / 2;
    mesh.position.set(l[0], y, l[2]);mesh.name = o.name || 'marker';group.add(mesh);
    if (o.outline) {const ring = new T.Mesh(new T.SphereGeometry(r * 1.25, 16, 12), new T.MeshBasicMaterial({color: color(o.outline), side: T.BackSide}));ring.position.copy(mesh.position);group.add(ring);}
    if (label) this.markers.push({position: mesh.position.clone(), label, color: fill});
  }

  rebuildOverlays() {
    if (!this.renderer) return;
    this.overlayGroup.children.slice().forEach(c => {this.overlayGroup.remove(c);disposeTree(c);});
    this.overlayMarkers = [];
    if (!this.origin) return;
    const saved = this.markers;this.markers = this.overlayMarkers;
    this.overlays.forEach(overlay => {
      if (!overlay || typeof overlay !== 'object') return;
      (overlay.tracks || []).slice(0, 32).forEach((track, i) => {
        const style = overlayStyle(overlay, track, i), pos = [];
        for (const p of (track.points || []).slice(0, 4096)) {
          if (!Array.isArray(p) || !Number.isFinite(p[0]) || !Number.isFinite(p[1])) continue;
          const msl = Number.isFinite(p[2]), l = this.local(p[0], p[1], msl ? p[2] : this.origin[2]);if (!l) continue;
          pos.push(l[0], msl ? l[1] + (this.offset || 0) : this.heightAt(l[0], l[2]) + 3, l[2]);
        }
        if (pos.length >= 6) {const line = makeLine(pos, style);line.name = `overlay-track-${overlay.kind}`;line.userData = {overlay: overlay.id, phase: track.phase, dashed: !!style.dashed};this.overlayGroup.add(line);}
      });
      (overlay.areas || []).slice(0, 16).forEach(area => this.addArea(area, overlayStyle(overlay, area), overlay));
      (overlay.markers || []).slice(0, 16).forEach(m => {
        if (!Number.isFinite(m?.lat_deg) || !Number.isFinite(m?.lon_deg)) return;
        const style = overlayStyle(overlay, m), fill = m.kind === 'apogee' ? PHASE_COLORS.apogee : overlay.kind === 'flight_plan' && m.kind === 'landing' ? PLAN_ELLIPSE_COLOR : style.color;
        const label = overlay.kind === 'los_prediction' ? `Predicted: no signal · ${m.label || m.kind}` : (m.label || m.kind);
        this.addMarker(this.overlayGroup, m.lat_deg, m.lon_deg, m.kind === 'landing' ? null : m.alt_msl_m, fill, label, {ring: style.ring || overlay.predicted !== false && m.kind !== 'last_fix', outline: m.kind === 'apogee' ? '#0f172a' : null, name: `overlay-marker-${m.kind}`});
      });
      if (overlay.label) this.markers.push({position: this.firstOverlayPosition(overlay), label: overlay.label, color: overlay.kind === 'los_prediction' ? LOS_COLOR : '#e2e8f0'});
    });
    this.markers = saved;
    this.host.dataset.overlayCount = String(this.overlays.length);
  }

  firstOverlayPosition(overlay) {
    const p = overlay.tracks?.[0]?.points?.at?.(-1) || overlay.areas?.[0]?.polygon?.[0];
    const m = overlay.markers?.[0];
    const [lat, lon] = p || (m ? [m.lat_deg, m.lon_deg] : [this.origin[0], this.origin[1]]);
    const l = this.local(lat, lon, this.origin[2]) || [0, 0, 0];
    return new T.Vector3(l[0], this.heightAt(l[0], l[2]) + 60, l[2]);
  }

  /** A polygon draped on the terrain: a translucent fill and an outline. */
  addArea(area, style, overlay) {
    const ring = (area?.polygon || []).slice(0, 72).map(p => this.local(p[0], p[1], this.origin[2])).filter(Boolean);
    if (ring.length < 3) return;
    const contour = ring.map(l => new T.Vector2(l[0], l[2]));
    let faces = [];try {faces = T.ShapeUtils.triangulateShape(contour, []);} catch {faces = [];}
    const pos = ring.flatMap(l => [l[0], this.heightAt(l[0], l[2]) + 4, l[2]]);
    if (faces.length) {
      const g = new T.BufferGeometry();g.setAttribute('position', new T.Float32BufferAttribute(pos, 3));g.setIndex(faces.flat());
      const fill = new T.Mesh(g, new T.MeshBasicMaterial({color: color(style.color), transparent: true, opacity: style.fill ?? 0.15, depthWrite: false, side: T.DoubleSide}));
      fill.name = `overlay-area-${area.kind}`;fill.userData = {overlay: overlay.id, kind: area.kind};fill.renderOrder = 2;this.overlayGroup.add(fill);
    }
    const outline = makeLine([...pos, ...pos.slice(0, 3)], {color: style.color, opacity: style.opacity ?? 0.9, dashed: style.dashed});outline.name = `overlay-outline-${area.kind}`;this.overlayGroup.add(outline);
  }

  rebuildStations() {
    if (!this.renderer) return;
    this.stationGroup.children.slice().forEach(c => {this.stationGroup.remove(c);disposeTree(c);});
    this.stationMarkers = [];
    if (!this.origin) return;
    const saved = this.markers;this.markers = this.stationMarkers;
    // The pad: the scene origin, standing on the terrain.
    const padY = this.heightAt(0, 0);
    const pad = new T.Mesh(new T.ConeGeometry(14, 40, 16), new T.MeshLambertMaterial({color: 0x94a3b8}));pad.position.set(0, padY + 20, 0);pad.name = 'pad-marker';this.stationGroup.add(pad);
    this.markers.push({position: new T.Vector3(0, padY + 50, 0), label: 'Pad', color: PHASE_COLORS.pad});
    const names = {gs: 'Ground station', rrls: 'RRLS'};
    for (const s of this.stations.slice(0, 8)) {
      const lat = Number(s?.lat), lon = Number(s?.lon);if (!Number.isFinite(lat) || !Number.isFinite(lon)) continue;
      const l = this.local(lat, lon, this.origin[2]);if (!l) continue;
      const y = this.heightAt(l[0], l[2]);
      const mast = new T.Mesh(new T.CylinderGeometry(3, 5, 30, 8), new T.MeshLambertMaterial({color: s.live ? 0x22d3ee : 0x64748b}));mast.position.set(l[0], y + 15, l[2]);mast.name = 'station-' + (s.role || 'station');this.stationGroup.add(mast);
      this.markers.push({position: new T.Vector3(l[0], y + 40, l[2]), label: (names[s.role] || s.role || 'Station') + (s.live ? '' : ' (stale)'), color: '#67e8f9'});
    }
    this.markers = saved;
  }

  renderLegend() {
    if (!this.legend) return;
    const rows = legendEntries(this.doc, this.overlays, this.atMs);
    this.legend.replaceChildren(...rows.map(r => {
      const item = document.createElement('span');item.setAttribute('role', 'listitem');item.dataset.key = r.key;item.style.cssText = 'display:inline-flex;align-items:center;gap:5px;white-space:nowrap';
      const sw = document.createElement('span');
      sw.style.cssText = r.style === 'marker' ? `width:9px;height:9px;border-radius:50%;background:${r.color};border:2px solid #0f172a;box-shadow:0 0 0 1px #fff` : r.style === 'area' ? `width:12px;height:9px;background:${r.color}4d;border:1px solid ${r.color}` : `width:16px;height:0;border-top:3px ${r.style === 'dashed' ? 'dashed' : 'solid'} ${r.color}`;
      item.append(sw, document.createTextNode(r.label));return item;
    }));
    this.legend.style.display = rows.length ? 'flex' : 'none';
  }

  renderInfo() {
    if (!this.info) return;
    const source = this.terrain?.source, lines = [];
    lines.push(source === 'dem' ? `Terrain: station DEM (z${this.terrain.group?.userData?.zoom ?? '?'}, ${Math.round((this.terrain.group?.userData?.extent_m || 0) / 1000)} km)` : source === 'flat-imagery' ? 'Terrain: no DEM cached here; flat at pad altitude with imagery' : source === 'generic' ? 'Terrain: no DEM or imagery cached here; reference ground' : 'Terrain: loading…');
    const c = this.status?.check;
    if (c) {
      const pct = list => list?.length ? Math.round(100 * list.reduce((a, x) => a + (x.present || 0), 0) / Math.max(1, list.reduce((a, x) => a + (x.checked || 0), 0))) : 0;
      lines.push(`Offline coverage within ${Math.round(c.radius_m / 1000)} km: DEM ${pct(c.dem)} %, imagery ${pct(c.imagery)} %`);
      if (!this.status.dem?.available) lines.push('Prefetch the DEM with download_map.py before the trip.');
    }
    if (!this.doc?.points?.length) lines.push('No rocket track yet.');
    this.info.replaceChildren(...lines.map(t => {const d = document.createElement('div');d.textContent = t;return d;}));
    this.attribution.textContent = this.terrain?.attribution || '';
    this.attribution.style.display = this.attribution.textContent ? 'block' : 'none';
    // Keep the legend above the attribution, which wraps on narrow screens.
    this.legend.style.bottom = `${(this.attribution.textContent ? this.attribution.offsetHeight : 0) + 6}px`;
  }

  fitCamera() {
    const pts = (this.doc?.points || []).map(p => this.local(p.latitude, p.longitude, p.altitude_m)).filter(Boolean);
    const box = new T.Box3(new T.Vector3(-800, this.heightAt(0, 0), -800), new T.Vector3(800, this.heightAt(0, 0) + 300, 800));
    for (const p of pts) box.expandByPoint(new T.Vector3(p[0], p[1] + (this.offset || 0), p[2]));
    box.getCenter(this.target);const size = box.getSize(new T.Vector3());
    this.distance = Math.max(1500, Math.min(120000, Math.max(size.x, size.y, size.z) * 1.6));
  }

  // ---- interaction (orbit / pan / zoom, mouse, touch, keyboard)
  controls() {
    const el = this.host, pointers = new Map();let last = null, pinch = null;
    const orbit = (dx, dy) => {this.theta -= dx * 0.005;this.phi = Math.max(0.12, Math.min(1.52, this.phi - dy * 0.005));};
    const pan = (dx, dy) => {
      const s = this.distance * 0.0015, right = new T.Vector3(Math.cos(this.theta + Math.PI / 2), 0, Math.sin(this.theta + Math.PI / 2)), fwd = new T.Vector3(Math.cos(this.theta), 0, Math.sin(this.theta));
      this.target.addScaledVector(right, -dx * s).addScaledVector(fwd, dy * s);
    };
    const zoom = f => {this.distance = Math.max(80, Math.min(250000, this.distance * f));};
    el.addEventListener('pointerdown', e => {el.setPointerCapture?.(e.pointerId);pointers.set(e.pointerId, [e.clientX, e.clientY]);last = {x: e.clientX, y: e.clientY, pan: e.button === 2 || e.shiftKey};});
    el.addEventListener('pointermove', e => {
      if (!pointers.has(e.pointerId)) return;
      const prev = pointers.get(e.pointerId);pointers.set(e.pointerId, [e.clientX, e.clientY]);
      if (pointers.size >= 2) {
        const [a, b] = [...pointers.values()], d = Math.hypot(a[0] - b[0], a[1] - b[1]), mid = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
        if (pinch) {zoom(pinch.d / Math.max(1, d));pan(mid[0] - pinch.mid[0], mid[1] - pinch.mid[1]);}
        pinch = {d, mid};
      } else if (last) {const dx = e.clientX - prev[0], dy = e.clientY - prev[1];last.pan ? pan(dx, dy) : orbit(dx, dy);}
      this.invalidate();
    });
    const up = e => {pointers.delete(e.pointerId);if (pointers.size < 2) pinch = null;if (!pointers.size) last = null;};
    el.addEventListener('pointerup', up);el.addEventListener('pointercancel', up);
    el.addEventListener('contextmenu', e => e.preventDefault());
    el.addEventListener('wheel', e => {e.preventDefault();zoom(Math.exp(e.deltaY * 0.0012));this.invalidate();}, {passive: false});
    el.addEventListener('keydown', e => {
      const k = e.key, step = 30;
      if (k === 'ArrowLeft') orbit(-step, 0); else if (k === 'ArrowRight') orbit(step, 0); else if (k === 'ArrowUp') orbit(0, -step); else if (k === 'ArrowDown') orbit(0, step);
      else if (k === '+' || k === '=') zoom(0.8); else if (k === '-') zoom(1.25); else if (k === 'r' || k === 'R') this.fitCamera(); else return;
      e.preventDefault();this.invalidate();
    });
  }

  invalidate() {if (!this.renderer || this.frame) return;this.frame = requestAnimationFrame(() => {this.frame = null;this.render();});}

  render() {
    if (!this.renderer || this.disposed) return;
    const w = this.host.clientWidth || 1, h = this.host.clientHeight || 1;
    if (this.size !== `${w}x${h}`) {this.size = `${w}x${h}`;this.renderer.setSize(w, h, false);this.camera.aspect = w / h;this.camera.updateProjectionMatrix();}
    const c = Math.cos(this.phi), s = Math.sin(this.phi);
    this.camera.position.set(this.target.x + this.distance * s * Math.cos(this.theta), this.target.y + this.distance * c, this.target.z + this.distance * s * Math.sin(this.theta));
    // Never under the terrain.
    const ground = this.heightAt(this.camera.position.x, this.camera.position.z) + 20;if (this.camera.position.y < ground) this.camera.position.y = ground;
    this.camera.lookAt(this.target);
    this.renderer.render(this.scene, this.camera);
    this.renderLabels(w, h);
    this.host.dataset.rendered = String((Number(this.host.dataset.rendered) || 0) + 1);
  }

  renderLabels(w, h) {
    const all = [...(this.markers || []), ...(this.overlayMarkers || []), ...(this.stationMarkers || [])].slice(0, MAX_LABELS);
    while (this.labels.length < all.length) {const d = document.createElement('div');d.style.cssText = 'position:absolute;transform:translate(-50%,-120%);padding:1px 5px;border-radius:5px;background:#0f172acc;white-space:nowrap;font-size:11px';this.labelLayer.append(d);this.labels.push(d);}
    this.labels.forEach((d, i) => {
      const m = all[i];if (!m) {d.style.display = 'none';return;}
      const p = m.position.clone().project(this.camera);
      if (p.z > 1 || Math.abs(p.x) > 1.1 || Math.abs(p.y) > 1.1) {d.style.display = 'none';return;}
      d.style.display = 'block';d.textContent = m.label;d.style.borderLeft = `3px solid ${m.color}`;
      d.style.left = `${(p.x + 1) / 2 * w}px`;d.style.top = `${(1 - p.y) / 2 * h}px`;
    });
  }

  dispose() {
    if (this.disposed) return;
    this.disposed = true;VIEWS.delete(this);if (this.frame) cancelAnimationFrame(this.frame);
    window.removeEventListener('gs26-map-overlays', this.onOverlays);window.removeEventListener('gs26-map-stations', this.onStations);window.removeEventListener('gs26-flight-track', this.onTrack);
    this.resizeObserver?.disconnect();
    for (const g of [this.trackGroup, this.overlayGroup, this.stationGroup]) disposeTree(g);
    this.terrain?.dispose();this.renderer?.dispose();this.renderer?.forceContextLoss?.();this.host.remove();if (this.container.__gs26Map3d === this) this.container.__gs26Map3d = null;
  }
}

/** Disposes views whose element left the document (the map tab unmounted or remounted). */
export function disposeDetached() {for (const view of [...VIEWS]) if (!view.host.isConnected) view.dispose();}

/** Mounts the 3D map into a container; returns the view (idempotent per container). */
export function mountMap3d(container, opts = {}) {
  if (!container) return null;
  if (container.__gs26Map3d) return container.__gs26Map3d;
  const view = new Map3dView(container, opts);container.__gs26Map3d = view;return view;
}
export function unmountMap3d(container) {container?.__gs26Map3d?.dispose();if (container) container.__gs26Map3d = null;}

/**
 * The map tab's 2D/3D switch. mode '3d' mounts the view over the 2D map (which stays mounted
 * underneath, so switching back is instant); anything else unmounts it. When WebGL is missing
 * the view reports it and the caller keeps 2D. Returns the view or null.
 */
export function setMapViewMode(container, mode, opts = {}) {
  if (!container) return null;
  const map2d = opts.map2d || null;
  if (mode !== '3d') {
    unmountMap3d(container);container.dataset.mapView = '2d';container.style.display = 'none';
    if (map2d) map2d.style.visibility = '';
    return null;
  }
  container.style.display = 'block';container.dataset.mapView = '3d';
  const view = mountMap3d(container, opts);
  if (!view?.renderer) {container.dataset.mapView = '3d-unavailable';return view;}
  if (map2d) map2d.style.visibility = 'hidden';
  return view;
}
