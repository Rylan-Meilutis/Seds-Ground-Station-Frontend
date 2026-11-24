// The programme diagram applies the same rules as the dashboard
// (src/telemetry_dashboard/live_diagrams/derive.rs) to the same topology.
'use strict';
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const vm = require('vm');

const window = {};
vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../station_ui/scripts/program-diagram.js'), 'utf8'), { window });
const fill = JSON.parse(fs.readFileSync(path.join(__dirname, '../src/telemetry_dashboard/live_diagrams/topologies/fill.json'), 'utf8'));
const stand = JSON.parse(fs.readFileSync(path.join(__dirname, '../src/telemetry_dashboard/live_diagrams/topologies/stand.json'), 'utf8'));
const derive = (t, i) => JSON.parse(JSON.stringify(window.gsProgramDiagram.derive(t, i)));
// VALVE_STATE: [pilot, vent, dump, igniter, nitrogen, nitrous, retracted].
const input = (valves, psi, age = 100, extra = {}) => ({
  rows: [{ t: 'VALVE_STATE', s: 'VB', v: valves, age_ms: age }, { t: 'PRESSURE_TRANSDUCER_CALIBRATED', s: 'VB', v: [psi], age_ms: 100 }],
  boards: [], controls: [], status: { igt: false }, ...extra,
});

let s = derive(fill, input([0, 0, 0, 0, 1, 0, 0], 200));
assert.strictEqual(s.valves.nitrogen.s, 'open');
assert.strictEqual(s.edges.n2_branch.flow, 'flowing');
assert.strictEqual(s.edges.fill_line.flow, 'flowing');
assert.deepStrictEqual(s.edges.fill_line.fluids, ['n2']);
assert.strictEqual(s.edges.n2_supply.flow, 'flowing');
assert.strictEqual(s.edges.vent_line.flow, 'blocked');

s = derive(fill, input([0, 0, 0, 0, 1, 0, 0], 998));
assert.strictEqual(s.edges.n2_branch.flow, 'static', 'no differential');

s = derive(fill, input([0, 0, 0, 0, 1, 0, 1], 200));
assert.strictEqual(s.valves.qd.s, 'closed', 'retracted QD');
assert.strictEqual(s.edges.fill_line.flow, 'blocked');

s = derive(fill, input([0, 0, 0, 0, 1, 0, 0], 200, 9000));
assert.strictEqual(s.valves.nitrogen.s, 'stale');
assert.strictEqual(s.edges.fill_line.flow, 'unknown', 'stale data never animates');

s = derive(fill, input([0, 0, 0, 0, 0, 0, 0], 200, 100, { controls: [{ cmd: 'Pilot', actuated: false, requested: true }] }));
assert.deepStrictEqual(s.valves.main, { s: 'pending', to: true, from: false });

s = derive(fill, input([1, 0, 0, 0, 0, 0, 0], 700));
assert.strictEqual(s.edges.feed_line.flow, 'flowing', 'chamber assumed ambient without a PT');

s = derive(fill, input([0, 0, 0, 0, 1, 1, 0], 300));
assert.deepStrictEqual(s.edges.fill_line.fluids, ['n2', 'n2o_liquid']);
assert.ok(s.readings.tank_pt.text.startsWith('300 psi'));

assert.strictEqual(derive(stand, input([0, 0, 0, 0, 0, 0, 0], 1)).igt, false);
// The vehicle silhouette: the station's geometry true to scale, else the generic rocket.
const geometry = JSON.parse(fs.readFileSync(path.join(__dirname, 'fixtures/backend-contract/vehicle/geometry.json'), 'utf8'));
const sil = window.gsVehicleSilhouette;
assert.ok(sil.valid(geometry) && !sil.valid({ ...geometry, version: 2 }));
const frame = stand.frames.find((f) => f.vehicle).vehicle;
const drawn = sil(geometry, frame.x, frame.y, frame.w, frame.h, frame.highlight);
assert.strictEqual(drawn.geometry.source.rocket, 'SWEET_V6');
assert.ok(Math.abs(drawn.scale * geometry.length_m - frame.h) < 0.5, 'fills the box height');
assert.ok(geometry.length_m / geometry.diameter_m > 27, 'about 28:1, as drawn');
assert.deepStrictEqual(drawn.sections.map((x) => x.role), ['avionics']);
assert.strictEqual((drawn.fins.match(/Z/g) || []).length, 3, 'three fins, all visible from the side');
assert.strictEqual(sil({ ...geometry, version: 9 }, 0, 0, 100, 100).geometry.source.kind, 'generic');
console.log('station-program-diagram: ok');
