// The programme's studio sources format values as the banner does, find the
// altitude for the lower third, and cover every view the station offers.
'use strict';
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const vm = require('vm');

const window = {};
vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../station_ui/scripts/program-studio.js'), 'utf8'), { window });
const studio = window.gsProgramStudio;
assert.deepStrictEqual({ ...studio.formatStat({ value: 12.345, precision: 1, unit: 'm' }) }, { value: '12.3', unit: 'm' });
assert.deepStrictEqual({ ...studio.formatStat({ value: 1, format: 'valve' }) }, { value: 'Open', unit: '' });
assert.deepStrictEqual({ ...studio.formatStat({ value: 0, format: 'boolean' }) }, { value: 'No', unit: '' });
assert.strictEqual(studio.formatStat({ value: null }), null, 'missing data is never a number');
assert.strictEqual(studio.formatStat({ value: 'NaN' }), null);
assert.strictEqual(studio.formatStat(null), null);
assert.strictEqual(studio.altitudeStat([{ label: 'Tank' }, { label: 'GPS altitude' }]).label, 'GPS altitude');
assert.strictEqual(studio.altitudeStat([{ label: 'Tank' }]).label, 'Tank', 'falls back to the first stat');
assert.strictEqual(studio.altitudeStat([]), null);
// Every view in the station's source catalogue (contract fixture) is drawn here or by the page.
const view = JSON.parse(fs.readFileSync(path.join(__dirname, 'fixtures/backend-contract/studio/view.json'), 'utf8'));
for (const source of view.sources) assert(studio.views.includes(source.view), source.view);
console.log('program studio sources: formatting, altitude, catalogue views');
