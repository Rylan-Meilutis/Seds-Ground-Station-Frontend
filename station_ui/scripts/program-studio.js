// Studio scenes on the programme page (docs/broadcast-studio.md, "Stream
// manager"). The page positions camera videos and the diagram, map, rocket
// and recorded-replay holders in their slots; this script draws every other
// source of a scene in its slot: telemetry cards, plots, the T-clock, the
// transparent overlays (lower third, T-clock bug, telemetry strip), graphics
// and a "No signal" card for a camera that is not playing. Everything comes
// from the delayed programme snapshot, so it stays in step with the video.
// Sized with container units, so a source is readable at any slot size and
// at stream resolution. Read-only: nothing here sends a command.
'use strict';
(function () {
  const CSS = `
.gs-st-slot{position:absolute;overflow:hidden;container-type:size;pointer-events:none;font-family:system-ui,-apple-system,'Segoe UI',Roboto,sans-serif;color:#f4f7fa;font-variant-numeric:tabular-nums}
.gs-st-panel{position:absolute;inset:0;background:linear-gradient(160deg,#0f1e2e,#07101a);display:flex;flex-direction:column;padding:4cqmin;gap:3cqmin;box-sizing:border-box}
.gs-st-title{font-size:max(11px,5.5cqmin);font-weight:700;letter-spacing:.12em;text-transform:uppercase;color:#8fb7d6;display:flex;align-items:center;gap:2cqmin}
.gs-st-title i{display:inline-block;width:1.6cqmin;height:1.6cqmin;min-width:6px;min-height:6px;border-radius:50%;background:#2fd66b;box-shadow:0 0 1.5cqmin #2fd66b}
.gs-st-paused .gs-st-title i{background:#f5b335;box-shadow:none}
.gs-st-cards{flex:1;display:grid;gap:2.5cqmin;grid-template-columns:repeat(var(--cols,3),minmax(0,1fr));grid-auto-rows:1fr;min-height:0}
.gs-st-card{background:#ffffff0d;border:1px solid #ffffff1f;border-radius:2cqmin;padding:2.5cqmin 3cqmin;display:flex;flex-direction:column;justify-content:center;min-width:0;min-height:0;overflow:hidden}
.gs-st-card b{display:block;font-size:max(10px,4cqmin);font-weight:600;letter-spacing:.08em;text-transform:uppercase;color:#9fb6c8;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.gs-st-card span{display:block;font-size:max(14px,10cqmin);font-weight:750;line-height:1.05;margin-top:1cqmin;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.gs-st-card small{font-size:.45em;font-weight:600;color:#c9d6e2;margin-left:.3em}
.gs-st-card.none span{color:#7b8a99;font-weight:500}
@container (max-aspect-ratio: 5/4){.gs-st-cards{grid-template-columns:minmax(0,1fr)!important}.gs-st-card{flex-direction:row;align-items:baseline;justify-content:space-between;gap:3cqmin}.gs-st-card span{margin-top:0;font-size:max(14px,min(9cqw,12cqh))}}
.gs-st-clock{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:2cqmin;min-height:0}
.gs-st-clock strong{font-size:min(15cqw,40cqh);white-space:nowrap;font-weight:800;letter-spacing:.02em;line-height:1}
.gs-st-clock span{font-size:max(11px,7cqmin);color:#b8cadb;letter-spacing:.06em;text-transform:uppercase}
.gs-st-plots{flex:1;display:grid;grid-auto-rows:1fr;gap:2.5cqmin;min-height:0}
.gs-st-plot{position:relative;background:#ffffff0a;border:1px solid #ffffff1a;border-radius:2cqmin;min-height:0;overflow:hidden}
.gs-st-plot canvas{position:absolute;inset:0;width:100%;height:100%}
.gs-st-plot header{position:absolute;left:2.5cqmin;top:1.5cqmin;right:2.5cqmin;display:flex;justify-content:space-between;gap:2cqmin;font-size:max(10px,4.4cqmin);font-weight:650;z-index:1}
.gs-st-plot header b{color:#9fb6c8;text-transform:uppercase;letter-spacing:.08em;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.gs-st-lower{position:absolute;left:2.5cqw;right:2.5cqw;bottom:12cqh;height:62cqh;display:flex;align-items:stretch;border-radius:1.6cqh;overflow:hidden;box-shadow:0 1cqh 4cqh #0008}
.gs-st-lower>div{display:flex;flex-direction:column;justify-content:center;padding:0 2.4cqw;min-width:0}
.gs-st-lower .brand{background:linear-gradient(135deg,#0b63ce,#0a3f86);flex:0 1 auto;max-width:38%}
.gs-st-lower .brand b{font-size:min(15cqh,4cqw);font-weight:800;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.gs-st-lower .brand span{font-size:min(10cqh,2.6cqw);color:#d5e6ff;text-transform:uppercase;letter-spacing:.1em;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.gs-st-lower .metric{background:#07111dd9;flex:1 1 auto;border-left:1px solid #ffffff1f}
.gs-st-lower .metric b{font-size:min(10cqh,2.4cqw);color:#9fb6c8;text-transform:uppercase;letter-spacing:.1em;white-space:nowrap}
.gs-st-lower .metric span{font-size:min(24cqh,4.2cqw);font-weight:800;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.gs-st-bug{position:absolute;right:3cqw;top:6cqh;padding:1.5cqh 2.2cqw;border-radius:1.6cqh;background:#07111dd9;border:1px solid #ffffff2a;text-align:right;box-shadow:0 1cqh 3cqh #0007}
.gs-st-bug b{display:block;font-size:max(9px,min(14cqh,5cqw));letter-spacing:.12em;color:#8fb7d6;text-transform:uppercase}
.gs-st-bug span{display:block;font-size:max(14px,min(32cqh,11cqw));font-weight:800;line-height:1.05}
.gs-st-strip{position:absolute;inset:auto 2cqw 8cqh 2cqw;height:70cqh;display:flex;gap:1.2cqw}
.gs-st-strip>div{flex:1 1 0;background:#07111dd9;border:1px solid #ffffff22;border-radius:1.4cqh;display:flex;flex-direction:column;justify-content:center;padding:0 1.6cqw;min-width:0}
.gs-st-strip b{font-size:min(16cqh,2.2cqw);color:#9fb6c8;text-transform:uppercase;letter-spacing:.08em;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.gs-st-strip span{font-size:min(34cqh,4.4cqw);font-weight:800;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.gs-st-graphic{position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center;text-align:center;gap:3cqmin;padding:6cqmin;box-sizing:border-box;background:radial-gradient(ellipse at 50% 30%,#163a5f,#060d16 70%)}
.gs-st-graphic strong{font-size:min(14cqw,22cqh);font-weight:850;letter-spacing:.01em;line-height:1.05;overflow-wrap:anywhere}
.gs-st-graphic span{font-size:max(12px,6cqmin);color:#b8cadb;letter-spacing:.1em;text-transform:uppercase}
.gs-st-graphic em{font-style:normal;font-size:max(12px,7cqmin);font-weight:700;color:#e6eef5}
.gs-st-nosignal{position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:2cqmin;background:repeating-linear-gradient(135deg,#0b1520 0 3cqmin,#0d1925 3cqmin 6cqmin);color:#9fb0c0;text-align:center}
.gs-st-nosignal strong{font-size:max(12px,8cqmin);color:#dce6ef}
.gs-st-nosignal span{font-size:max(10px,5cqmin)}`;

  let styled = false;
  function style() {
    if (styled || typeof document === 'undefined') return;
    styled = true;
    const s = document.createElement('style');
    s.textContent = CSS;
    document.head.appendChild(s);
  }

  // Values as the programme banner formats them.
  function formatStat(stat) {
    if (!stat || stat.value == null || !Number.isFinite(Number(stat.value))) return null;
    if (stat.format === 'valve') return { value: Number(stat.value) > 0.5 ? 'Open' : 'Closed', unit: '' };
    if (stat.format === 'boolean') return { value: Number(stat.value) > 0.5 ? 'Yes' : 'No', unit: '' };
    return { value: Number(stat.value).toFixed(Math.max(0, Math.min(6, stat.precision | 0))), unit: stat.unit || '' };
  }
  const altitudeStat = (stats) => (stats || []).find((s) => /altitude/i.test(s.label)) || (stats || [])[0] || null;

  // A short history of each stat for the plot source, from successive
  // snapshots (the page polls twice a second).
  const HISTORY_MS = 120000;
  const history = new Map();
  let lastRecord = 0;
  function record(telemetry, at) {
    if (!telemetry || !Array.isArray(telemetry.stats) || at - lastRecord < 400) return;
    lastRecord = at;
    for (const stat of telemetry.stats) {
      if (stat.format === 'valve' || stat.format === 'boolean') continue;
      let points = history.get(stat.label);
      if (!points) history.set(stat.label, (points = []));
      const v = stat.value == null ? null : Number(stat.value);
      points.push([at, Number.isFinite(v) ? v : null]);
      while (points.length && at - points[0][0] > HISTORY_MS) points.shift();
    }
  }

  function el(tag, cls, parent, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    if (parent) parent.appendChild(e);
    return e;
  }

  function panel(holder, title, paused) {
    const p = el('div', 'gs-st-panel' + (paused ? ' gs-st-paused' : ''), holder);
    const t = el('div', 'gs-st-title', p);
    el('i', '', t);
    el('span', '', t, title);
    return p;
  }

  function statsPage(ctx, size) {
    const stats = (ctx.telemetry && ctx.telemetry.stats) || [];
    if (stats.length <= size) return stats;
    const seconds = Math.max(1, Math.min(30, Number(ctx.telemetry.cycle_seconds) || 5));
    const pages = Math.ceil(stats.length / size);
    const page = Math.floor(ctx.now / (seconds * 1000)) % pages;
    return stats.slice(page * size, page * size + size);
  }

  const VIEWS = {
    camera(holder, src, ctx) {
      const box = el('div', 'gs-st-nosignal', holder);
      el('strong', '', box, src.stream_id ? 'Connecting…' : 'No signal');
      el('span', '', box, src.label || 'Camera');
      return () => {};
    },
    stats(holder, src, ctx) {
      const p = panel(holder, 'Live telemetry', !ctx.fresh);
      const cards = el('div', 'gs-st-cards', p);
      let shown = '';
      return (c) => {
        p.classList.toggle('gs-st-paused', !c.fresh);
        const stats = statsPage(c, 6);
        const signature = JSON.stringify([c.fresh, stats.map((s) => [s.label, s.value, s.unit])]);
        if (signature === shown) return;
        shown = signature;
        cards.style.setProperty('--cols', stats.length <= 2 ? stats.length || 1 : stats.length === 4 ? 2 : 3);
        cards.replaceChildren();
        if (!stats.length) {
          const card = el('div', 'gs-st-card none', cards);
          el('b', '', card, 'Telemetry');
          el('span', '', card, c.fresh ? 'No data' : 'Paused');
        }
        for (const stat of stats) {
          const f = c.fresh ? formatStat(stat) : null;
          const card = el('div', 'gs-st-card' + (f ? '' : ' none'), cards);
          el('b', '', card, stat.label);
          const v = el('span', '', card, f ? f.value : '—');
          if (f && f.unit) el('small', '', v, f.unit);
        }
      };
    },
    tclock(holder, src, ctx) {
      const p = panel(holder, 'Mission clock', !ctx.fresh);
      const box = el('div', 'gs-st-clock', p);
      const clock = el('strong', '', box);
      const phase = el('span', '', box);
      return (c) => {
        p.classList.toggle('gs-st-paused', !c.fresh);
        clock.textContent = (c.fresh && c.telemetry && c.telemetry.t_clock) || 'T —';
        phase.textContent = c.fresh ? c.phase : 'Telemetry paused';
      };
    },
    plot(holder, src, ctx) {
      const p = panel(holder, 'Telemetry plots', !ctx.fresh);
      const rows = el('div', 'gs-st-plots', p);
      let drawn = 0;
      return (c) => {
        p.classList.toggle('gs-st-paused', !c.fresh);
        if (c.now - drawn < 250) return;
        drawn = c.now;
        const stats = ((c.telemetry && c.telemetry.stats) || []).filter((s) => s.format !== 'valve' && s.format !== 'boolean').slice(0, 3);
        while (rows.children.length > stats.length) rows.lastChild.remove();
        stats.forEach((stat, i) => {
          let row = rows.children[i];
          if (!row) {
            row = el('div', 'gs-st-plot', rows);
            const h = el('header', '', row);
            el('b', '', h);
            el('span', '', h);
            el('canvas', '', row);
          }
          const f = c.fresh ? formatStat(stat) : null;
          row.querySelector('b').textContent = stat.label;
          row.querySelector('span').textContent = f ? `${f.value} ${f.unit}`.trim() : '—';
          drawPlot(row.querySelector('canvas'), history.get(stat.label) || [], c.now, i);
        });
      };
    },
    lower_third(holder, src, ctx) {
      const bar = el('div', 'gs-st-lower', holder);
      const brand = el('div', 'brand', bar);
      const title = el('b', '', brand);
      const phase = el('span', '', brand);
      const alt = el('div', 'metric', bar);
      el('b', '', alt, 'Altitude');
      const altValue = el('span', '', alt);
      const clock = el('div', 'metric', bar);
      el('b', '', clock, 'T-clock');
      const clockValue = el('span', '', clock);
      return (c) => {
        title.textContent = c.label;
        phase.textContent = c.fresh ? c.phase : 'Telemetry paused';
        const a = altitudeStat(c.telemetry && c.telemetry.stats);
        const f = c.fresh ? formatStat(a) : null;
        alt.firstChild.textContent = a ? a.label : 'Altitude';
        altValue.textContent = f ? `${f.value} ${f.unit}`.trim() : '—';
        clockValue.textContent = (c.fresh && c.telemetry && c.telemetry.t_clock) || '—';
      };
    },
    tclock_bug(holder, src, ctx) {
      const bug = el('div', 'gs-st-bug', holder);
      el('b', '', bug, 'T-clock');
      const v = el('span', '', bug);
      return (c) => { v.textContent = (c.fresh && c.telemetry && c.telemetry.t_clock) || '—'; };
    },
    stats_bar(holder, src, ctx) {
      const strip = el('div', 'gs-st-strip', holder);
      let shown = '';
      return (c) => {
        const stats = statsPage(c, 4);
        const signature = JSON.stringify([c.fresh, stats.map((s) => [s.label, s.value, s.unit])]);
        if (signature === shown) return;
        shown = signature;
        strip.replaceChildren();
        for (const stat of stats) {
          const f = c.fresh ? formatStat(stat) : null;
          const cell = el('div', '', strip);
          el('b', '', cell, stat.label);
          el('span', '', cell, f ? `${f.value} ${f.unit}`.trim() : '—');
        }
      };
    },
    title(holder, src, ctx) {
      const g = el('div', 'gs-st-graphic', holder);
      el('span', '', g, 'Live');
      const t = el('strong', '', g);
      const phase = el('em', '', g);
      return (c) => { t.textContent = c.label; phase.textContent = c.fresh ? c.phase : ''; };
    },
    slate(holder, src, ctx) {
      const g = el('div', 'gs-st-graphic', holder);
      el('strong', '', g, 'Stand by');
      const label = el('span', '', g);
      const clock = el('em', '', g);
      return (c) => { label.textContent = c.label; clock.textContent = (c.fresh && c.telemetry && c.telemetry.t_clock) || ''; };
    },
  };
  const PAGE_VIEWS = new Set(['diagram', 'map', 'model', 'recorded_replay']);
  const PLOT_COLORS = ['#4fc3f7', '#ffb74d', '#81c784'];

  function drawPlot(canvas, points, now, index) {
    const r = canvas.getBoundingClientRect();
    const dpr = Math.min(2, (typeof devicePixelRatio === 'number' && devicePixelRatio) || 1);
    const w = Math.max(1, Math.round(r.width * dpr)), h = Math.max(1, Math.round(r.height * dpr));
    if (canvas.width !== w) canvas.width = w;
    if (canvas.height !== h) canvas.height = h;
    const g = canvas.getContext && canvas.getContext('2d');
    if (!g) return;
    g.clearRect(0, 0, w, h);
    const live = points.filter((p) => p[1] != null && now - p[0] <= HISTORY_MS);
    if (live.length < 2) return;
    let lo = Math.min(...live.map((p) => p[1])), hi = Math.max(...live.map((p) => p[1]));
    if (hi - lo < 1e-9) { lo -= 1; hi += 1; }
    const pad = (hi - lo) * 0.12, top = h * 0.3, bottom = h * 0.92;
    const x = (t) => w * (1 - (now - t) / HISTORY_MS), y = (v) => bottom - ((v - lo + pad) / (hi - lo + 2 * pad)) * (bottom - top);
    g.lineWidth = Math.max(2, h / 60);
    g.strokeStyle = PLOT_COLORS[index % PLOT_COLORS.length];
    g.beginPath();
    let pen = false;
    for (const [t, v] of points) {
      if (v == null) { pen = false; continue; }
      if (pen) g.lineTo(x(t), y(v)); else g.moveTo(x(t), y(v));
      pen = true;
    }
    g.stroke();
  }

  // layer: the absolutely positioned container over the programme viewport.
  // scene: the resolved scene (`state.studio`). ctx: {telemetry, fresh,
  // label, phase, now, cameraReady(streamId)}.
  function render(layer, scene, ctx) {
    style();
    const keep = new Set();
    for (const slot of (scene && scene.slots) || []) {
      const src = slot.source;
      if (!src || PAGE_VIEWS.has(src.view)) continue;
      if (src.view === 'camera' && src.stream_id && ctx.cameraReady(src.stream_id)) continue;
      const view = VIEWS[src.view];
      if (!view) continue;
      const key = `${slot.id}|${src.id}|${src.view === 'camera' ? src.stream_id || '' : ''}`;
      keep.add(key);
      let holder = [...layer.children].find((c) => c.dataset.key === key);
      if (!holder) {
        holder = el('div', 'gs-st-slot', layer);
        holder.dataset.key = key;
        holder.dataset.source = src.id;
        holder.dataset.view = src.view;
        holder.setAttribute('aria-label', src.label || src.id);
        holder._update = view(holder, src, ctx);
      }
      Object.assign(holder.style, { left: slot.x + '%', top: slot.y + '%', width: slot.width + '%', height: slot.height + '%', zIndex: String(3 + (slot.z | 0) * 3) });
      holder._update(ctx);
    }
    for (const child of [...layer.children]) if (!keep.has(child.dataset.key)) child.remove();
  }
  function clear(layer) { layer.replaceChildren(); }

  window.gsProgramStudio = { render, clear, record, formatStat, altitudeStat, views: Object.keys(VIEWS).concat([...PAGE_VIEWS]) };
})();
