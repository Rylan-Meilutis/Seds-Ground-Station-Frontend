// Recovery search overlay: the station's `search_coverage` MapOverlay
// (backend contract fixture recovery-search/status.json, or the document in
// $RECOVERY_SEARCH_STATUS) plus a device's local-only track, handed to a MOCK
// window.setGroundMapOverlays exactly as recovery_search::overlays_js calls
// it. The mock draws the overlay in an SVG with the contract palette, so the
// test checks the shape every renderer relies on: closed [lat, lon] rings for
// search_swath / uncovered areas, [lat, lon, alt|null] track points, a colour
// per searcher. The real 2D map draws overlays in ground_map.js (owned by the
// flight-predict map work); this harness never loads it.
//
// Areas are filled without an outline: an outline would trace the zero-width
// bridges that keep a hole visible in one ring.
//
// $SCREENSHOT writes a PNG of the drawing.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const {chromium} = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');

const fixture = new URL('./fixtures/backend-contract/recovery-search/status.json', import.meta.url);
const status = JSON.parse(fs.readFileSync(process.env.RECOVERY_SEARCH_STATUS || fixture, 'utf8'));
// The device's own track, never sent (recovery_search::local_overlay).
const first = status.overlay.tracks[0]?.points?.[0] || status.target.polygon[0];
const local = {
  id: 'search_local', kind: 'search_coverage', label: 'My track (only on this device)', predicted: false,
  tracks: [{id: 'local', phase: null, label: 'My track (not shared)', style: 'local', color: '#f8fafc',
    points: Array.from({length: 40}, (_, i) => [first[0] - 0.0004 + i * 0.00002, first[1] - 0.0012 + Math.sin(i / 4) * 0.0001, null])}],
  markers: [], areas: [],
};
const call = `window.setGroundMapOverlays?.("search", ${JSON.stringify([status.overlay, local])});`;

const mock = `
window.__drawn = null;
window.setGroundMapOverlays = (provider, overlays) => {
  const pts = [];
  for (const o of overlays) {
    for (const a of o.areas || []) pts.push(...a.polygon);
    for (const t of o.tracks || []) pts.push(...t.points);
  }
  const target = ${JSON.stringify(status.target ? status.target.polygon : [])};
  pts.push(...target);
  const lat0 = Math.min(...pts.map(p => p[0])), lat1 = Math.max(...pts.map(p => p[0]));
  const lon0 = Math.min(...pts.map(p => p[1])), lon1 = Math.max(...pts.map(p => p[1]));
  const k = Math.cos((lat0 + lat1) / 2 * Math.PI / 180);
  const W = 900, H = 560, pad = 30;
  const s = Math.min((W - 2 * pad) / ((lon1 - lon0) * k), (H - 2 * pad) / (lat1 - lat0));
  const xy = p => [pad + (p[1] - lon0) * k * s, H - pad - (p[0] - lat0) * s];
  const d = (ring, close) => ring.map((p, i) => (i ? 'L' : 'M') + xy(p).map(v => v.toFixed(1)).join(' ')).join('') + (close ? 'Z' : '');
  const style = {search_swath: ['#22c55e', 0.3], uncovered: ['#ef4444', 0.25]};
  let svg = '<path d="' + d(target, true) + '" fill="none" stroke="#a78bfa" stroke-dasharray="6 4" stroke-width="2"/>';
  const counts = {search_swath: 0, uncovered: 0, tracks: 0};
  for (const o of overlays) {
    for (const a of o.areas || []) {
      const [fill, op] = style[a.kind] || ['#64748b', 0.2];
      counts[a.kind] = (counts[a.kind] || 0) + 1;
      if (a.polygon.length < 3 || a.polygon.some(p => !Number.isFinite(p[0]) || !Number.isFinite(p[1]))) throw new Error('bad ring ' + a.id);
      svg += '<path data-kind="' + a.kind + '" d="' + d(a.polygon, true) + '" fill="' + fill + '" fill-opacity="' + op + '" fill-rule="evenodd" stroke="none"/>';
    }
    for (const t of o.tracks || []) {
      counts.tracks++;
      svg += '<path data-track="' + t.id + '" d="' + d(t.points, false) + '" fill="none" stroke="' + t.color + '" stroke-width="2.5"' + (t.style === 'local' ? ' stroke-dasharray="4 3"' : '') + '/>';
    }
  }
  const legend = overlays.map(o => o.label).join(' · ');
  document.body.innerHTML = '<div style="font:14px system-ui;color:#e2e8f0;padding:8px 12px">' + provider + ': ' + legend +
    ' · <span style="color:#22c55e">searched</span> · <span style="color:#ef4444">not yet searched</span> · <span style="color:#a78bfa">predicted landing area</span></div>' +
    '<svg width="' + W + '" height="' + H + '" style="background:#0f1b2a;display:block">' + svg + '</svg>';
  window.__drawn = {provider, counts, overlays: overlays.length};
};`;

const browser = await chromium.launch({headless: true, executablePath: process.env.CHROME_BINARY || undefined});
try {
  const page = await browser.newPage({viewportSize: {width: 920, height: 610}});
  await page.setViewportSize({width: 920, height: 610});
  await page.setContent('<!doctype html><body style="margin:0;background:#0b1220"></body>');
  // Without the map's function the guarded call is a no-op, never an error.
  await page.evaluate(call);
  await page.addScriptTag({content: mock});
  await page.evaluate(call);
  const drawn = await page.evaluate(() => window.__drawn);
  assert.equal(drawn.provider, 'search');
  assert.equal(drawn.overlays, 2);
  assert.ok(drawn.counts.search_swath >= 1, 'covered swaths drawn');
  assert.ok(drawn.counts.uncovered >= 1, 'uncovered areas drawn');
  assert.equal(drawn.counts.tracks, status.overlay.tracks.length + 1, 'every searcher track plus the local one');
  if (process.env.SCREENSHOT) await page.screenshot({path: process.env.SCREENSHOT});
  console.log('PASS: recovery search overlay draws through setGroundMapOverlays("search", …)', JSON.stringify(drawn.counts));
} finally {
  await browser.close();
}
