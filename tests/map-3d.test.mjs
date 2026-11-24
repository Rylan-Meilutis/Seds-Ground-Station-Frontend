// map-3d.js and the 2D track in ground_map.js: phase lookup at boundaries, phase runs, the
// CONTRACT.md palette and legend, overlay styles with a mock MapOverlay, and the 2D GeoJSON.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import {PHASE_COLORS, PHASE_ORDER, phaseAt, pointPhase, phaseRuns, legendEntries, overlayStyle, apogeeOf, trackUntil, phaseColor,
  LOS_COLOR, PLAN_ELLIPSE_COLOR, SWATH_COLOR, UNCOVERED_COLOR, SEARCHER_COLORS} from '../station_ui/rendering/three/map-3d.js';

const fixture = name => JSON.parse(fs.readFileSync(new URL(`./fixtures/backend-contract/${name}.json`, import.meta.url), 'utf8'));

// ---- the CONTRACT.md palette
assert.deepEqual({...PHASE_COLORS}, {pad: '#94a3b8', boost: '#f97316', coast: '#facc15', apogee: '#ffffff', drogue: '#38bdf8', main: '#22c55e', landed: '#a78bfa', unknown: '#64748b'});
assert.deepEqual([LOS_COLOR, PLAN_ELLIPSE_COLOR, SWATH_COLOR, UNCOVERED_COLOR], ['#f472b6', '#a78bfa', '#22c55e', '#ef4444']);
assert.deepEqual([...SEARCHER_COLORS], ['#0ea5e9', '#eab308', '#ec4899', '#14b8a6', '#f97316', '#8b5cf6']);
assert.equal(phaseColor('nonsense'), '#64748b');
console.log('PASS phase palette matches CONTRACT.md');

// ---- phase lookup, including at the boundaries (the station's [start, end) rule)
const phases = [
  {phase: 'pad', start_ms: 0, end_ms: 10}, {phase: 'boost', start_ms: 10, end_ms: 20}, {phase: 'coast', start_ms: 20, end_ms: 40},
  {phase: 'drogue', start_ms: 40, end_ms: 90}, {phase: 'main', start_ms: 90, end_ms: 200}, {phase: 'landed', start_ms: 200, end_ms: 300},
];
for (const [t, want] of [[-1, null], [0, 'pad'], [9, 'pad'], [10, 'boost'], [19, 'boost'], [20, 'coast'], [40, 'drogue'], [90, 'main'], [199, 'main'], [200, 'landed'], [300, 'landed'], [301, null]]) {
  assert.equal(phaseAt(phases, t), want, `t=${t}`);
}
assert.equal(phaseAt([{phase: 'boost', start_ms: 5, end_ms: null}], 1e12), 'boost', 'an open live segment holds');
assert.equal(pointPhase({timestamp_ms: 15, phase: 'coast'}, phases), 'coast', "the station's per-point phase wins");
assert.equal(pointPhase({timestamp_ms: 15}, phases), 'boost');
assert.equal(pointPhase({timestamp_ms: -5}, phases), 'unknown');
console.log('PASS phase lookup at every boundary');

// ---- every point of the nominal 10k flight gets the station's phase
const trajectory = fixture('replay-trajectory/flight');
for (const p of trajectory.points) assert.equal(pointPhase(p, trajectory.phases), p.phase || 'unknown', `t=${p.timestamp_ms}`);
const recomputed = trajectory.points.map(p => phaseAt(trajectory.phases, p.timestamp_ms) || undefined);
assert.deepEqual(recomputed, trajectory.points.map(p => p.phase), 'the UI lookup agrees with the station on every point');
const runs = phaseRuns(trajectory.points, trajectory.phases);
assert.deepEqual(runs.map(r => r.phase), ['pad', 'boost', 'coast', 'drogue', 'landed']);
for (let i = 1; i < runs.length; i++) assert.equal(runs[i].points[0], runs[i - 1].points.at(-1), 'runs share their boundary point');
assert.equal(runs.reduce((n, r) => n + r.points.length, 0), trajectory.points.length + runs.length - 1);
const live = fixture('flight-phases/live');
assert.deepEqual(phaseRuns(live.points, live.phases).map(r => r.phase), ['pad', 'boost', 'coast', 'drogue', 'landed']);
assert.equal(live.phases.at(-1).end_ms, null);
// A recording break starts a new run without joining across the gap.
const broken = phaseRuns([{timestamp_ms: 0, latitude: 1, longitude: 1, altitude_m: 1}, {timestamp_ms: 1, latitude: 1, longitude: 1, altitude_m: 2}, {timestamp_ms: 9, latitude: 1, longitude: 1, altitude_m: 3, break_before: true}], [{phase: 'boost', start_ms: 0, end_ms: null}]);
assert.equal(broken.length, 2);assert.equal(broken[1].points.length, 1);
console.log('PASS track phase segmentation (recorded and live)');

// ---- legend: phases present in order, the apogee marker, overlay kinds
const legend = legendEntries(trajectory, []);
assert.deepEqual(legend.map(r => r.key), ['pad', 'boost', 'coast', 'apogee', 'drogue', 'landed']);
assert(legend.every(r => r.color === PHASE_COLORS[r.key]));
assert.equal(legend.find(r => r.key === 'apogee').style, 'marker');
assert(apogeeOf(trajectory) && apogeeOf(trajectory).altitude_m > 3000);
const early = trajectory.phases[1].start_ms + 2000;
assert.deepEqual(legendEntries(trajectory, [], early).map(r => r.key), ['pad', 'boost'], 'replay cursor limits the legend');
assert.equal(trackUntil(trajectory, early).at(-1).timestamp_ms <= early, true);
assert.deepEqual(legendEntries(null, []), []);
for (const name of PHASE_ORDER) assert(PHASE_COLORS[name]);
console.log('PASS colour and legend mapping');

// ---- overlays (mock MapOverlay per CONTRACT.md)
const plan = {id: 'flight_plan:fp-1', kind: 'flight_plan', label: 'Predicted: SWEET V6 8.5 kg', predicted: true,
  tracks: [{id: 'boost', phase: 'boost', style: 'predicted', points: [[31.758, -102.046, 890], [31.7581, -102.0461, 1200]]}],
  markers: [{id: 'landing', kind: 'landing', lat_deg: 31.76, lon_deg: -102.03, alt_msl_m: 890}], areas: [{id: 'e', kind: 'landing_ellipse', polygon: [[31.76, -102.03], [31.761, -102.031], [31.759, -102.032]]}]};
const los = {id: 'los', kind: 'los_prediction', label: 'Predicted: no signal', predicted: true, tracks: [{id: 'p', phase: 'drogue', points: [[31.759, -102.04, 2500]]}],
  markers: [{id: 'n', kind: 'predicted_position', lat_deg: 31.7591, lon_deg: -102.039}, {id: 'f', kind: 'last_fix', lat_deg: 31.759, lon_deg: -102.04}], areas: [{id: 'u', kind: 'position_uncertainty', polygon: []}]};
const search = {id: 'search', kind: 'search_coverage', tracks: [{id: 'a', style: 'search', points: []}, {id: 'b', style: 'search', points: []}], areas: [{kind: 'search_swath'}, {kind: 'uncovered'}]};
assert.deepEqual(overlayStyle(plan, plan.tracks[0]), {color: '#f97316', opacity: 0.55, dashed: true}, 'predicted plan tracks: phase colour, dashed, 55 %');
assert.deepEqual(overlayStyle(plan, plan.areas[0]), {color: '#a78bfa', fill: 0.12, dashed: true, opacity: 0.9});
assert.equal(overlayStyle(los, los.tracks[0]).color, '#f472b6');assert.equal(overlayStyle(los, los.tracks[0]).dashed, true);
assert.equal(overlayStyle(los, los.markers[0]).ring, true, 'the predicted position is a hollow ring');
assert.equal(overlayStyle(los, los.markers[1]).dashed, false, 'the last real fix stays solid');
assert.equal(overlayStyle(los, los.areas[0]).fill, 0.15);
assert.deepEqual([overlayStyle(search, search.areas[0]).fill, overlayStyle(search, search.areas[1]).color], [0.30, '#ef4444']);
assert.deepEqual([overlayStyle(search, search.tracks[0], 0).color, overlayStyle(search, search.tracks[1], 1).color], ['#0ea5e9', '#eab308']);
assert.deepEqual(legendEntries(trajectory, [plan, los, search]).slice(-3).map(r => r.key), ['flight_plan', 'los_prediction', 'search_coverage']);
console.log('PASS overlay styles and legend rows for a mock overlay');

// ---- the 2D map uses the same palette and segments the same way (ground_map.js, a classic script)
const source = fs.readFileSync(new URL('../static/ground_map.js', import.meta.url), 'utf8');
const start = source.indexOf('const GS26_PHASE_COLORS'), end = source.indexOf('function syncGroundMapFlightTrack');
const sandbox = {isUsableLatLng: (a, b) => Number.isFinite(a) && Number.isFinite(b) && Math.abs(a) <= 90 && Math.abs(b) <= 180 && !(a === 0 && b === 0)};
vm.runInNewContext(source.slice(start, end) + ';this.colors=GS26_PHASE_COLORS;this.fc=flightTrackFeatureCollection;this.at=flightTrackPhaseAt;', sandbox);
assert.deepEqual({...sandbox.colors}, {...PHASE_COLORS}, 'the 2D and 3D palettes are the same');
for (const t of [-1, 0, 10, 20, 300, 301]) assert.equal(sandbox.at(phases, t), phaseAt(phases, t));
const fc = sandbox.fc(trajectory);
const lines = fc.features.filter(f => f.geometry.type === 'LineString');
assert.deepEqual(Array.from(lines, f => f.properties.phase), runs.map(r => r.phase));
assert(lines.every(f => f.properties.color === PHASE_COLORS[f.properties.phase]));
assert.equal(fc.features.filter(f => f.geometry.type === 'Point').length, 1, 'apogee point');
assert.equal(sandbox.fc(null).features.length, 0);
console.log('PASS 2D track GeoJSON coloured by phase with the shared palette');
