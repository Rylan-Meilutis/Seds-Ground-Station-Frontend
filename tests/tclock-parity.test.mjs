// The classic-script offline player keeps its own copy of the T-clock
// formatter (it cannot import ES modules); it must print exactly what the
// shared station_ui/rendering/three/tclock.js prints.
import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import {formatTClock} from '../station_ui/rendering/three/tclock.js';
vm.runInThisContext(fs.readFileSync(new URL('../station_ui/rendering/three/offline-replay.js', import.meta.url), 'utf8'));
const local = globalThis.GSOfflineReplay.formatTClock;
const H = 3600e3, D = 24 * H;
for (const ms of [0, 1, 999, 1000, 59_999, -59_999, -1, -1000, 60_000, H - 1, H, -H, 99 * H, 100 * H + 61_000, D, D + 2 * H + 3 * 60e3 + 4e3, -400 * D, 2 ** 53, -(2 ** 53), NaN, Infinity, -Infinity, null, undefined, '12']) {
  assert.equal(local(ms), formatTClock(ms), `T-clock parity at ${String(ms)}`);
}
console.log('tclock parity ok');
