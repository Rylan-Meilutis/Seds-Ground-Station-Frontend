import fs from 'node:fs';
import assert from 'node:assert/strict';
import {formatTClock, tclockBody, TCLOCK_UNKNOWN, TCLOCK_UNKNOWN_BODY} from '../station_ui/rendering/three/tclock.js';

const vectors = JSON.parse(fs.readFileSync(new URL('../watch/test-vectors.json', import.meta.url), 'utf8')).tclock_format;
assert.ok(vectors.length >= 10);
let checked = 0;
for (const {ms, fraction_digits: fractionDigits, text} of vectors) {
  // JSON numbers past 2^53 are not exact in JS; the Rust/Kotlin/Swift cores cover those.
  if (!Number.isSafeInteger(ms)) continue;
  assert.equal(formatTClock(ms, {fractionDigits}), text, `vector ${ms} ms`);
  checked++;
}
assert.ok(checked >= 18);

assert.equal(formatTClock(0), 'T+ 00:00');
assert.equal(formatTClock(-0), 'T+ 00:00');
assert.equal(formatTClock(59999.9, {fractionDigits: 2}), 'T+ 00:59.99');
assert.equal(formatTClock(-59999.9, {fractionDigits: 1}), 'T- 00:59.9');
assert.equal(formatTClock(3600000), 'T+ 1:00:00');
assert.equal(formatTClock(99 * 3600000), 'T+ 4:03:00:00');
assert.equal(formatTClock(93784000), 'T+ 1:02:03:04');
assert.equal(formatTClock(0, {beforeT0: true, fractionDigits: 2}), 'T- 00:00.00');
assert.equal(formatTClock(9500, {beforeT0: true}), 'T- 00:09');
assert.equal(formatTClock(Number.MAX_SAFE_INTEGER), 'T+ 104249991:08:59:00');
assert.equal(formatTClock(Number.MAX_VALUE).startsWith('T+ '), true);
assert.doesNotMatch(formatTClock(Number.MAX_VALUE), /NaN|Infinity|e\+/);
assert.doesNotMatch(formatTClock(Number.MAX_VALUE, {fractionDigits: 3}), /NaN|Infinity|e\+|\.-/);
for (const bad of [NaN, Infinity, -Infinity, undefined, null, '1000', {}]) {
  assert.equal(formatTClock(bad), TCLOCK_UNKNOWN);
  assert.equal(tclockBody(bad), TCLOCK_UNKNOWN_BODY);
}
assert.equal(TCLOCK_UNKNOWN, 'T --:--');
assert.equal(tclockBody(1234, 9), '00:01.234');
assert.equal(tclockBody(1234, -1), '00:01');
console.log('tclock: MM:SS / H:MM:SS / D:HH:MM:SS, sign, fractions, shared vectors, overflow and invalid input PASS');
