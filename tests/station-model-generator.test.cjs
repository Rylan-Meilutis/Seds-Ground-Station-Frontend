const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const test = require('node:test');

test('replay models reproduce without depending on the working directory', () => {
  const root = path.resolve(__dirname, '..');
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'station-replay-models-'));
  try {
    const generated = spawnSync(process.execPath, [
      path.join(root, 'scripts/build_gse_models.mjs'), '--output-dir', directory,
    ], { cwd: os.tmpdir(), encoding: 'utf8' });
    assert.equal(generated.status, 0, generated.stderr);
    for (const name of ['vehicle.glb', 'gse-site.glb']) {
      assert.deepEqual(
        fs.readFileSync(path.join(directory, name)),
        fs.readFileSync(path.join(root, 'station_ui/rendering/models', name)),
        `${name} differs from its source-controlled generator`,
      );
    }
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});
