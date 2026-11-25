// The programme diagram applies the same rules as the dashboard
// (src/telemetry_dashboard/live_diagrams/derive.rs) to the same topology.
'use strict';
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const vm = require('vm');

const window = {};
// Only what the full-screen handling touches outside a page.
const listeners = {};
const document = { fullscreenElement: null, addEventListener: (type, fn) => { listeners[type] = fn; }, exitFullscreen() { document.fullscreenElement = null; document.exited = (document.exited || 0) + 1; return Promise.resolve(); } };
vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../station_ui/scripts/program-diagram.js'), 'utf8'), { window, document });
const view = (id) => JSON.parse(fs.readFileSync(path.join(__dirname, `../src/telemetry_dashboard/live_diagrams/topologies/${id}.json`), 'utf8'));
// One view per station mode; the fill rules hold in each.
const fill = view('flight');
const testFire = view('test_fire');
const stand = view('ground_test');
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
for (const t of [testFire, stand]) {
  s = derive(t, input([0, 0, 0, 0, 1, 0, 0], 200));
  assert.strictEqual(s.edges.fill_line.flow, 'flowing', t.id);
  assert.strictEqual(s.valves.qd.s, 'open', t.id);
}
// The test stand: KG1000 thrust and the chamber PT, OVR never a number.
s = derive(testFire, input([1, 0, 0, 0, 0, 0, 0], 700, 100, { rows: [
  { t: 'VALVE_STATE', s: 'VB', v: [1, 0, 0, 0, 0, 0, 0], age_ms: 100 }, { t: 'PRESSURE_TRANSDUCER_CALIBRATED', s: 'VB', v: [700], age_ms: 100 },
  { t: 'COMBUSTION_CHAMBER_PRESSURE', s: 'DAQ', v: [699], age_ms: 100 },
  { t: 'LOADCELL_WEIGHT_KG', s: 'DAQ', v: [null], age_ms: 100, over_range: 1 },
  { t: 'LOADCELL_50_WEIGHT_KG', s: 'DAQ', v: [null], age_ms: 9000, over_range: 1, over_range_low: 1 }] }));
assert.strictEqual(s.edges.feed_line.flow, 'static', 'the chamber PT drives the stand');
assert.strictEqual(s.readings.kg1000_kg.text, 'OVR');
assert.ok(s.readings.kg1000_kg.ovr && s.readings.thrust.ovr, 'the scaled thrust is OVR too');
assert.strictEqual(s.readings.kg50_kg.text, '−OVR · 9 s ago');
assert.ok(s.readings.kg50_kg.stale);
assert.ok(!('kg1000_kg' in derive(fill, input([0, 0, 0, 0, 0, 0, 0], 1)).readings), 'no KG1000 in flight');
// The flight view keeps the AV bay's 10 s staleness (binding stale_ms) beside the fill's 5 s.
s = derive(fill, input([0, 0, 0, 0, 0, 0, 0], 1, 100, { boards: [{ s: 'FC', seen: true, age_ms: 7000 }, { s: 'VB', seen: true, age_ms: 7000 }] }));
assert.strictEqual(s.nodes.fc.online, 'up');
assert.strictEqual(s.nodes.vb.online, 'down');

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
// Full screen: the Fullscreen API when the holder has it, the viewport overlay always.
const holder = (api) => {
  const classes = new Set();
  const h = { classList: { contains: (c) => classes.has(c), toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)) }, querySelector: () => null, requested: 0 };
  if (api) h.requestFullscreen = () => { h.requested += 1; return Promise.resolve(); };
  return h;
};
for (const api of [true, false]) {
  const h = holder(api);
  window.gsProgramDiagram.enterFull(h);
  assert.ok(h.classList.contains('gs-pd-full'));
  assert.strictEqual(h.requested, api ? 1 : 0);
  window.gsProgramDiagram.leaveFull();
  assert.ok(!h.classList.contains('gs-pd-full'));
}
// Esc and the platform's own exit (fullscreenchange) both leave.
let h = holder(true);
window.gsProgramDiagram.enterFull(h);
document.fullscreenElement = h;
listeners.keydown({ key: 'Escape', preventDefault() {} });
assert.ok(!h.classList.contains('gs-pd-full') && document.exited === 1 && document.fullscreenElement === null);
h = holder(true);
window.gsProgramDiagram.enterFull(h);
listeners.fullscreenchange();
assert.ok(!h.classList.contains('gs-pd-full'), 'left native full screen');
console.log('station-program-diagram: ok');
