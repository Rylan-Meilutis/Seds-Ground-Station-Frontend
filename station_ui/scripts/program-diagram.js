// Live diagrams for the audience programme (docs/live-diagrams.md).
// The same declarative topology and the same rules as the dashboard
// (src/telemetry_dashboard/live_diagrams/derive.rs), fed by the delayed
// programme snapshot (`telemetry.diagram`), so the diagram stays in step with
// the delayed video. Builds the SVG once per topology and then only changes
// classes and text. Read-only: nothing here sends a command. A full-screen
// button (shown only while the pointer moves, so a captured programme never
// shows it) uses the Fullscreen API, else covers the viewport; Esc leaves.
'use strict';
(function () {
  const NS = 'http://www.w3.org/2000/svg';
  const FLUIDS = ['#3987e5', '#d95926', '#199e70'];
  const CSS = `
.gs-pd{width:100%;height:100%;display:flex;flex-direction:column;background:#0b1220;color:#e5ecf2;font-family:system-ui,sans-serif;box-sizing:border-box;padding:2.2vh 2.4vw}
.gs-pd-head{display:flex;gap:1.2vw;align-items:baseline;font-size:3.2vh;font-weight:700}
.gs-pd-badge{font-size:1.7vh;letter-spacing:.06em;border:1px solid currentColor;border-radius:99px;padding:.2vh .8vw}
.gs-pd-badge[data-sim=true]{color:#ff6b6b}
.gs-pd svg{flex:1 1 auto;min-height:0;width:100%}
.gs-pd text{fill:#e5ecf2}
.gs-pd .f{fill:none;stroke:#3a4a5c;stroke-dasharray:6 6}.gs-pd .fl{font-size:13px;fill:#9fb0c0;text-transform:uppercase;letter-spacing:.04em}
.gs-pd .e{fill:none;stroke-width:7;stroke-linecap:round;stroke-linejoin:round;transition:opacity .3s}
.gs-pd .e.blocked{opacity:.25}.gs-pd .e.unknown{stroke:#7b8a99!important;stroke-dasharray:3 9;opacity:.7}.gs-pd .e.dash{stroke-dasharray:12 6}
.gs-pd .a{fill:none;stroke:#0b1220;stroke-width:3;stroke-dasharray:4 18;opacity:0}.gs-pd .a.on{opacity:.95;animation:gspd 0.9s linear infinite}
@keyframes gspd{to{stroke-dashoffset:-22}}
.gs-pd .vb{fill:#1c2b3d;stroke:#e5ecf2;stroke-width:1.2;stroke-linejoin:round}.gs-pd .vf{fill:#3a4a5c;stroke:#e5ecf2;stroke-width:1}.gs-pd .vh{fill:#3987e5;fill-opacity:.55;stroke:#e5ecf2;stroke-width:1.2}
.gs-pd .l{fill:none;stroke-width:3}.gs-pd .l.up{stroke:#e5ecf2}.gs-pd .l.down{stroke:#d03b3b;stroke-dasharray:2 8}.gs-pd .l.unknown{stroke:#7b8a99;stroke-dasharray:2 8;opacity:.6}.gs-pd .l.mechanical{stroke:#7b8a99;stroke-width:8;opacity:.55;stroke-dasharray:none}
.gs-pd .n .s{fill:#152132;stroke:#8498a8;stroke-width:2}.gs-pd .n.up .s{stroke:#0ca30c}.gs-pd .n.down .s{stroke:#d03b3b;stroke-dasharray:6 4}.gs-pd .n.unknown .s{stroke:#7b8a99;stroke-dasharray:3 5}
.gs-pd .n.bus .s,.gs-pd .n.junction .s{fill:#e5ecf2;stroke:none}.gs-pd .n.atmosphere .s{fill:none;stroke-dasharray:4 4}
.gs-pd .lv{fill:#3987e5;opacity:.55;transition:all .4s}
.gs-pd .nl{font-size:15px;font-weight:650}.gs-pd .ns{font-size:12px;fill:#9fb0c0}.gs-pd .na{font-size:12px;fill:#ff6b6b}
.gs-pd .v path{stroke-width:2.5;stroke:#e5ecf2;fill:#0b1220}.gs-pd .v.open path{fill:#0ca30c;stroke:#0ca30c}
.gs-pd .v.pending path{fill:none;stroke:#fab219;stroke-dasharray:4 3}.gs-pd .v.stale path,.gs-pd .v.unknown path,.gs-pd .v.manual path{fill:none;stroke:#7b8a99;stroke-dasharray:2 3}
.gs-pd .vl{font-size:13px;font-weight:650}.gs-pd .vs{font-size:12px;font-weight:700}.gs-pd .v.open .vs{fill:#3fd13f}.gs-pd .v.pending .vs{fill:#fab219}.gs-pd .v.stale .vs,.gs-pd .v.unknown .vs,.gs-pd .v.manual .vs{fill:#9fb0c0;font-weight:500}
.gs-pd .rl{font-size:12px;fill:#9fb0c0}.gs-pd .rv{font-size:20px;font-weight:700;font-variant-numeric:tabular-nums}
.gs-pd .r.stale .rv,.gs-pd .r.missing .rv{fill:#7b8a99;font-weight:500}.gs-pd .r.warn .rv{fill:#fab219}
.gs-pd .r.ovr .rv{fill:#ff6b6b}
.gs-pd-fs{margin-left:auto;font:inherit;font-size:1.6vh;font-weight:600;padding:.6vh 1vw;border-radius:1vh;border:1px solid #3a4a5c;background:#152132;color:#e5ecf2;cursor:pointer;opacity:0;transition:opacity .3s}
.gs-pd.awake .gs-pd-fs,.gs-pd-fs:focus-visible{opacity:1}
.gs-pd-full{position:fixed!important;inset:0!important;z-index:2147483000;display:block!important}
.gs-pd-legend{display:flex;flex-wrap:wrap;gap:.6vh 1.6vw;font-size:1.6vh;color:#cbd5e1}
.gs-pd-legend i{display:inline-block;width:2.4vw;height:.6vh;margin-right:.5vw;vertical-align:middle;border-radius:3px}
@media (prefers-reduced-motion: reduce){.gs-pd .a.on{animation:none}}`;

  const el = (name, attrs, parent) => {
    const node = document.createElementNS(NS, name);
    for (const [k, v] of Object.entries(attrs || {})) node.setAttribute(k, v);
    if (parent) parent.appendChild(node);
    return node;
  };
  const visible = (when, igt) => !when || (when === 'igt' ? igt : !igt);
  const age = (ms) => { const s = Math.max(0, Math.floor(ms / 1000)); return s < 60 ? `${s} s` : s < 3600 ? `${Math.floor(s / 60)} min` : `${Math.floor(s / 3600)} h`; };

  function resolve(b, input, staleMs) {
    let value = null, ageMs = null, ovr = null;
    staleMs = b.stale_ms || staleMs;
    if (b.telemetry) {
      const rows = (input.rows || []).filter((r) => r.t === b.telemetry && (!b.sender || r.s === b.sender));
      rows.sort((x, y) => (x.age_ms ?? 1e12) - (y.age_ms ?? 1e12));
      const row = rows[0], i = b.index || 0;
      const v = row?.v?.[i];
      // ADC over range (diagrams::program_inputs): never a number, OVR or −OVR.
      if (row && ((row.over_range || 0) >>> i) & 1) { ovr = ((row.over_range_low || 0) >>> i) & 1 ? '−OVR' : 'OVR'; ageMs = row.age_ms ?? null; }
      else if (typeof v === 'number' && Number.isFinite(v)) { value = v * (b.scale ?? 1); ageMs = row.age_ms ?? null; }
    } else if (typeof b.constant === 'number') value = b.constant;
    else if (b.board) { const s = (input.boards || []).find((x) => x.s === b.board); if (s) { value = s.seen; ageMs = s.age_ms ?? null; } }
    else if (b.control) { const c = (input.controls || []).find((x) => x.cmd === b.control); if (c && c.actuated != null) value = c.actuated; }
    else if (b.status) {
      if (b.status.startsWith('topology:')) { const t = input.status?.topology?.[b.status.slice(9)]; if (t) { value = t.online; ageMs = t.age_ms ?? null; } }
      else { const s = input.status?.[b.status]; if (s != null && s !== '') value = s; }
    }
    return { value, ageMs, ovr, stale: ageMs != null && ageMs > staleMs, assumed: !!b.assumed };
  }
  const truthy = (v) => (typeof v === 'boolean' ? v : typeof v === 'number' ? v >= 0.5 : null);

  function valveState(v, input, staleMs) {
    if (!v.state) return { s: 'manual' };
    const r = resolve(v.state, input, staleMs);
    let open = truthy(r.value);
    if (open != null && v.open_when === 'low') open = !open;
    const c = v.command ? (input.controls || []).find((x) => x.cmd === v.command) : null;
    let req = c?.requested ?? null;
    if (req != null && v.open_when === 'low') req = !req;
    const cur = r.stale ? null : open;
    if (req != null && cur !== req) return { s: 'pending', to: req, from: cur };
    if (c?.running && v.kind === 'qd') return { s: 'pending', to: false, from: cur };
    if (open == null) return { s: 'unknown' };
    if (r.stale) return { s: 'stale', was: open, age: r.ageMs };
    return { s: open ? 'open' : 'closed' };
  }
  const gate = (st) => st.s === 'open' || st.s === 'manual' ? 'open' : st.s === 'closed' || (st.s === 'pending' && st.from === false) ? 'closed' : 'unknown';
  const combine = (gates) => gates.includes('closed') ? 'closed' : gates.includes('unknown') ? 'unknown' : 'open';
  const rank = { flowing: 3, unknown: 2, static: 1, blocked: 0 };

  function derive(topo, input) {
    const staleMs = topo.stale_ms || 5000, igt = !!input.status?.igt;
    const valves = {}; for (const v of topo.valves || []) valves[v.id] = valveState(v, input, staleMs);
    const nodes = {};
    for (const n of topo.nodes || []) {
      let p = n.pressure ? resolve(n.pressure, input, staleMs) : { value: null };
      if ((p.value == null || p.stale) && typeof n.fallback_psi === 'number') p = { value: n.fallback_psi, assumed: true };
      const lv = n.level ? resolve(n.level, input, staleMs) : null;
      const st = n.status ? resolve(n.status, input, staleMs) : null;
      nodes[n.id] = {
        psi: p.value != null && !p.stale && typeof p.value === 'number' ? p.value : null,
        level: lv && !lv.stale && typeof lv.value === 'number' ? Math.min(100, Math.max(0, lv.value)) : null,
        online: st ? (truthy(st.value) == null ? 'unknown' : st.stale || !truthy(st.value) ? 'down' : 'up') : null,
        age: st?.ageMs ?? null,
      };
    }
    const kind = (id) => (topo.nodes || []).find((n) => n.id === id)?.kind;
    const gateOf = (through) => combine((through || []).map((id) => (valves[id] ? gate(valves[id]) : 'unknown')));
    const jout = {}; for (const e of topo.edges || []) if ((e.kind || 'pipe') === 'pipe' && kind(e.from) === 'junction') jout[e.from] = e;
    const edges = {};
    for (const e of topo.edges || []) {
      if (!visible(e.when, igt)) continue;
      const k = e.kind || 'pipe';
      if (k === 'pipe') {
        if (kind(e.from) === 'junction' || e.follows) continue;
        const out = jout[e.to];
        const g = out ? combine([gateOf(e.through), gateOf(out.through)]) : gateOf(e.through);
        const up = nodes[e.from]?.psi, down = nodes[out ? out.to : e.to]?.psi;
        const dp = up != null && down != null ? up - down : null;
        const flow = g === 'closed' ? 'blocked' : g === 'unknown' || dp == null ? 'unknown' : dp > (e.min_dp_psi ?? 5) ? 'flowing' : 'static';
        edges[e.id] = { flow, fluids: e.fluid ? [e.fluid] : [] };
      } else {
        const h = e.health ? resolve(e.health, input, staleMs) : null;
        const t = h ? truthy(h.value) : null;
        const link = t == null ? 'unknown' : h.stale || !t ? 'down' : 'up';
        edges[e.id] = { link, flow: link === 'up' ? 'flowing' : 'unknown', fluids: [] };
      }
    }
    for (const [j, out] of Object.entries(jout)) {
      const inc = (topo.edges || []).filter((e) => e.to === j).map((e) => edges[e.id]).filter(Boolean);
      const flow = inc.map((s) => s.flow).sort((a, b) => rank[b] - rank[a])[0] || 'unknown';
      const fluids = [...new Set(inc.filter((s) => s.flow === flow && flow !== 'blocked').flatMap((s) => s.fluids))].sort();
      edges[out.id] = { flow, fluids };
    }
    for (const e of topo.edges || []) {
      if (!e.follows || !visible(e.when, igt)) continue;
      const lead = edges[e.follows]?.flow || 'unknown', g = gateOf(e.through);
      edges[e.id] = { flow: g === 'closed' ? 'blocked' : lead === 'flowing' && g === 'unknown' ? 'unknown' : lead, fluids: e.fluid ? [e.fluid] : [] };
    }
    const readings = {};
    for (const r of topo.readings || []) {
      const res = resolve(r.value, input, r.stale_ms || staleMs);
      const ago = res.stale && res.ageMs != null ? ` · ${age(res.ageMs)} ago` : '';
      if (res.ovr) { readings[r.id] = { text: res.ovr + ago, stale: res.stale, ovr: true }; continue; }
      if (res.value == null) { readings[r.id] = { text: r.missing_text || '—', missing: true }; continue; }
      let text;
      if (typeof res.value === 'string') text = res.value;
      else if (r.format === 'bool' || typeof res.value === 'boolean') text = truthy(res.value) ? (r.true_text || 'On') : (r.false_text || 'Off');
      else text = (r.format === 'integer' ? Math.round(res.value).toString() : res.value.toFixed(r.precision || 0)) + (r.unit ? ` ${r.unit}` : '');
      const n = typeof res.value === 'number' ? res.value : null;
      const warn = n != null && ((r.warn_above != null && n >= r.warn_above) || (r.warn_below != null && n <= r.warn_below));
      readings[r.id] = { text: (res.assumed ? '≈ ' : '') + text + (warn ? ' ▲' : '') + ago, stale: res.stale, warn };
    }
    return { igt, valves, nodes, edges, readings };
  }

  function valveText(v, st) {
    const word = (on) => (on ? v.open_text || 'OPEN' : v.closed_text || 'CLOSED');
    switch (st.s) {
      case 'open': return word(true);
      case 'closed': return word(false);
      case 'pending': return v.kind === 'qd' && !st.to ? 'RETRACTING / pending' : `→ ${word(st.to)} (pending)`;
      case 'stale': return `stale ${age(st.age)} (was ${word(st.was).toLowerCase()})`;
      case 'manual': return 'not monitored';
      default: return 'unknown';
    }
  }

  function build(holder, topo) {
    holder.textContent = '';
    if (!document.getElementById('gs-pd-style')) { const s = document.createElement('style'); s.id = 'gs-pd-style'; s.textContent = CSS; document.head.appendChild(s); }
    const root = document.createElement('div'); root.className = 'gs-pd'; holder.appendChild(root);
    const head = document.createElement('div'); head.className = 'gs-pd-head'; root.appendChild(head);
    const title = document.createElement('span'); title.textContent = topo.title; head.appendChild(title);
    const badge = document.createElement('span'); badge.className = 'gs-pd-badge'; head.appendChild(badge);
    const fs = document.createElement('button'); fs.type = 'button'; fs.className = 'gs-pd-fs'; head.appendChild(fs);
    fs.addEventListener('click', () => (holder.classList.contains('gs-pd-full') ? leaveFull() : enterFull(holder)));
    full.label(holder, fs);
    let sleepTimer = 0;
    const wake = () => { root.classList.add('awake'); clearTimeout(sleepTimer); sleepTimer = setTimeout(() => root.classList.remove('awake'), 2500); };
    root.addEventListener('pointermove', wake); root.addEventListener('pointerdown', wake); fs.addEventListener('focus', wake);
    const [x, y, w, h] = topo.view_box;
    const svg = el('svg', { viewBox: `${x} ${y} ${w} ${h}`, preserveAspectRatio: 'xMidYMid meet', role: 'img', 'aria-label': topo.title }, root);
    const refs = { root, badge, svg, frames: {}, edges: {}, nodes: {}, valves: {}, readings: {} };
    const pts = (e) => e.points.map((p) => p.join(',')).join(' ');
    for (const f of topo.frames || []) {
      const g = el('g', {}, svg); el('rect', { class: 'f', x: f.x, y: f.y, width: f.w, height: f.h, rx: 14 }, g); el('text', { class: 'fl', x: f.x + 14, y: f.y + 20 }, g).textContent = f.label; refs.frames[f.id] = g;
      if (f.vehicle) {
        const v = f.vehicle, sil = vehicleSilhouette(window.gsProgramVehicle, v.x, v.y, v.w, v.h, v.highlight), vg = el('g', { class: 'veh', 'data-vehicle': f.id }, g);
        el('path', { class: 'vf', d: sil.fins }, vg); el('path', { class: 'vb', d: sil.body }, vg);
        for (const x of sil.sections) el('path', { class: 'vh', d: x.d, 'data-role': x.role }, vg);
      }
    }
    for (const e of topo.edges || []) {
      const g = el('g', {}, svg);
      const base = el('polyline', { points: pts(e) }, g), second = el('polyline', { points: pts(e), class: 'e dash', style: 'display:none;stroke-width:3' }, g), anim = el('polyline', { points: pts(e), class: 'a' }, g);
      if (e.label) { const [p, q] = e.points; el('text', { class: 'ns', x: (p[0] + q[0]) / 2 + 6, y: (p[1] + q[1]) / 2 - 6 }, g).textContent = e.label; }
      refs.edges[e.id] = { g, base, second, anim };
    }
    for (const n of topo.nodes || []) {
      const g = el('g', {}, svg); let s;
      const cx = n.x + n.w / 2, cy = n.y + n.h / 2;
      if (n.kind === 'junction') s = el('circle', { cx, cy, r: n.w / 2 }, g);
      else if (n.kind === 'atmosphere' || n.kind === 'regulator') s = el('ellipse', { cx, cy, rx: n.kind === 'regulator' ? n.h / 2 : n.w / 2, ry: n.h / 2 }, g);
      else if (n.kind === 'chamber') s = el('path', { d: `M${n.x} ${n.y}L${n.x + n.w * 0.62} ${n.y}L${n.x + n.w * 0.74} ${n.y + n.h * 0.36}L${n.x + n.w} ${n.y + n.h * 0.06}L${n.x + n.w} ${n.y + n.h * 0.94}L${n.x + n.w * 0.74} ${n.y + n.h * 0.64}L${n.x + n.w * 0.62} ${n.y + n.h}L${n.x} ${n.y + n.h}Z` }, g);
      else s = el('rect', { x: n.x, y: n.y, width: n.w, height: n.h, rx: n.kind === 'tank' ? Math.min(40, n.w / 2) : n.kind === 'bottle' ? n.w / 2.6 : 8 }, g);
      s.classList.add('s');
      const lv = n.level ? el('rect', { class: 'lv', x: n.x + 4, width: n.w - 8, rx: 6 }, g) : null;
      const ly = n.kind === 'tank' || n.kind === 'bottle' ? n.y + 22 : n.kind === 'regulator' ? n.y - 22 : n.sublabel ? cy - 2 : cy + 5;
      if (n.kind === 'bus') el('text', { class: 'ns', x: n.x, y: n.y - 6 }, g).textContent = n.label;
      else if (n.kind !== 'junction') {
        el('text', { class: 'nl', x: cx, y: ly, 'text-anchor': 'middle' }, g).textContent = n.label;
        if (n.sublabel) el('text', { class: 'ns', x: cx, y: ly + 15, 'text-anchor': 'middle' }, g).textContent = n.sublabel;
      }
      const ageText = el('text', { class: 'na', x: cx, y: n.y + n.h + 14, 'text-anchor': 'middle' }, g);
      refs.nodes[n.id] = { g, lv, ageText };
    }
    for (const v of topo.valves || []) {
      const g = el('g', {}, svg), s = 13, { x: vx, y: vy } = v;
      const d = v.vertical
        ? `M${vx - s} ${vy - s}L${vx + s} ${vy - s}L${vx} ${vy}ZM${vx - s} ${vy + s}L${vx + s} ${vy + s}L${vx} ${vy}Z`
        : `M${vx - s} ${vy - s}L${vx - s} ${vy + s}L${vx} ${vy}ZM${vx + s} ${vy - s}L${vx + s} ${vy + s}L${vx} ${vy}Z`;
      el('path', { d }, g);
      el('text', { class: 'vl', x: vx, y: vy + s + 15, 'text-anchor': 'middle' }, g).textContent = v.label;
      refs.valves[v.id] = { g, state: el('text', { class: 'vs', x: vx, y: vy + s + 29, 'text-anchor': 'middle' }, g) };
    }
    for (const r of topo.readings || []) {
      const anchor = r.anchor || 'start', g = el('g', {}, svg);
      el('text', { class: 'rl', x: r.x, y: r.y + 11, 'text-anchor': anchor }, g).textContent = r.label;
      refs.readings[r.id] = { g, value: el('text', { class: 'rv', x: r.x, y: r.y + 31, 'text-anchor': anchor }, g) };
    }
    const legend = document.createElement('div'); legend.className = 'gs-pd-legend'; root.appendChild(legend);
    for (const f of topo.fluids || []) { const s = document.createElement('span'); const i = document.createElement('i'); i.style.background = FLUIDS[f.slot - 1]; s.append(i, f.label + (f.dashed ? ' (dashed)' : '')); legend.appendChild(s); }
    if ((topo.valves || []).length) for (const t of ['Green: open', 'Hollow: closed', 'Amber dashed: commanded, pending', 'Grey dotted: stale or unknown', 'Moving dashes: flowing']) { const s = document.createElement('span'); s.textContent = t; legend.appendChild(s); }
    return refs;
  }

  function update(refs, topo, input) {
    const st = derive(topo, input);
    const prov = input.status?.provenance || 'Live';
    refs.badge.textContent = prov; refs.badge.dataset.sim = String(prov !== 'Live');
    for (const f of topo.frames || []) refs.frames[f.id].style.display = visible(f.when, st.igt) ? '' : 'none';
    for (const e of topo.edges || []) {
      const r = refs.edges[e.id], s = st.edges[e.id]; r.g.style.display = s ? '' : 'none'; if (!s) continue;
      const k = e.kind || 'pipe';
      if (k === 'pipe') {
        const fl = (topo.fluids || []).find((f) => f.id === (s.fluids[0] || (e.fluid !== 'inherit' ? e.fluid : null)));
        r.base.setAttribute('class', `e ${s.flow}${fl?.dashed ? ' dash' : ''}`);
        r.base.style.stroke = fl ? FLUIDS[fl.slot - 1] : '#7b8a99';
        const second = (topo.fluids || []).find((f) => f.id === s.fluids[1]);
        r.second.style.display = second ? '' : 'none'; if (second) r.second.style.stroke = FLUIDS[second.slot - 1];
        r.anim.setAttribute('class', s.flow === 'flowing' ? 'a on' : 'a');
      } else {
        r.base.setAttribute('class', `l ${k === 'mechanical' ? 'mechanical' : s.link}`);
        r.anim.setAttribute('class', s.link === 'up' && ['can', 'radio', 'uart', 'ethernet'].includes(k) ? 'a on' : 'a');
        r.anim.style.stroke = '#0ca30c';
      }
    }
    for (const n of topo.nodes || []) {
      const r = refs.nodes[n.id], s = st.nodes[n.id];
      r.g.style.display = visible(n.when, st.igt) ? '' : 'none';
      r.g.setAttribute('class', `n ${n.kind}${s.online ? ' ' + s.online : ''}`);
      if (r.lv) { const lh = s.level == null ? 0 : (n.h - 8) * s.level / 100; r.lv.setAttribute('height', lh); r.lv.setAttribute('y', n.y + n.h - 4 - lh); }
      r.ageText.textContent = s.online === 'down' ? (s.age != null ? `not heard for ${age(s.age)}` : 'no report') : s.online === 'unknown' ? 'no report' : '';
    }
    for (const v of topo.valves || []) {
      const r = refs.valves[v.id], s = st.valves[v.id];
      r.g.style.display = visible(v.when, st.igt) ? '' : 'none';
      r.g.setAttribute('class', `v ${s.s}`); r.state.textContent = valveText(v, s);
    }
    for (const rd of topo.readings || []) {
      const r = refs.readings[rd.id], s = st.readings[rd.id];
      r.g.style.display = visible(rd.when, st.igt) ? '' : 'none';
      r.g.setAttribute('class', `r${s.stale ? ' stale' : ''}${s.missing ? ' missing' : ''}${s.warn ? ' warn' : ''}${s.ovr ? ' ovr' : ''}`);
      r.value.textContent = s.text;
    }
  }

  // The station's vehicle (GET /api/media-assets/program/vehicle-geometry, the
  // same reply as /api/vehicle/geometry; docs/vehicle-geometry.md), drawn nose
  // up and true to scale in a box. A neutral generic rocket until it arrives.
  function validVehicle(g) {
    if (!g || g.schema !== 'gs26.vehicle_geometry' || g.version !== 1 || !(g.length_m > 0.1 && g.length_m < 200) || !(g.diameter_m > 0) || !Array.isArray(g.profile) || g.profile.length < 2 || g.profile.length > 4096) return false;
    return g.profile.every((p) => Array.isArray(p) && Number.isFinite(p[0]) && Number.isFinite(p[1]) && p[1] >= 0) && (g.fins || []).every((f) => f && f.count >= 1 && f.count <= 16 && Array.isArray(f.planform) && f.planform.every((p) => Number.isFinite(p[0]) && Number.isFinite(p[1])));
  }
  const GENERIC_VEHICLE = (() => {
    const len = 3, r = 0.075, nose = 0.6, rho = (r * r + nose * nose) / (2 * r), profile = [];
    for (let i = 0; i <= 16; i++) { const x = nose * i / 16; profile.push([x, Math.max(0, Math.sqrt(Math.max(0, rho * rho - (nose - x) ** 2)) + r - rho)]); }
    profile.push([len, r]);
    return { schema: 'gs26.vehicle_geometry', version: 1, source: { kind: 'generic' }, length_m: len, diameter_m: 2 * r, span_m: 2 * (r + 0.15), profile, sections: [], separations: [],
      fins: [{ count: 4, x_m: len - 0.32, root_radius_m: r, angle_offset_deg: 0, planform: [[0, 0], [0.15, 0.15], [0.27, 0.15], [0.3, 0]] }] };
  })();
  function vehicleSilhouette(geometry, bx, by, bw, bh, highlight) {
    const g = validVehicle(geometry) ? geometry : GENERIC_VEHICLE, span = Math.max(g.span_m || 0, g.diameter_m);
    const s = Math.max(0, Math.min(bh / g.length_m, bw / span)), cx = bx + bw / 2, top = by + (bh - g.length_m * s) / 2;
    const pt = (x, r) => `${(cx + r * s).toFixed(1)} ${(top + x * s).toFixed(1)}`;
    const outline = (pts) => 'M' + pts.map((p) => pt(p[0], p[1])).join('L') + 'L' + pts.slice().reverse().map((p) => pt(p[0], -p[1])).join('L') + 'Z';
    const at = (x) => { const p = g.profile; for (let i = 1; i < p.length; i++) if (x >= p[i - 1][0] && x <= p[i][0]) return p[i][0] - p[i - 1][0] > 1e-12 ? p[i - 1][1] + (p[i][1] - p[i - 1][1]) * (x - p[i - 1][0]) / (p[i][0] - p[i - 1][0]) : Math.max(p[i - 1][1], p[i][1]); return p[p.length - 1][1]; };
    const clip = (x0, x1) => [[x0, at(x0)], ...g.profile.filter((p) => p[0] > x0 && p[0] < x1), [x1, at(x1)]];
    let fins = '';
    for (const f of g.fins || []) for (let k = 0; k < f.count; k++) {
      const c = Math.cos((f.angle_offset_deg || 0) * Math.PI / 180 + 2 * Math.PI * k / f.count);
      if (Math.abs(c) < 1e-3) continue;
      fins += 'M' + f.planform.map((p) => pt(f.x_m + p[0], (f.root_radius_m + p[1]) * c)).join('L') + 'Z';
    }
    const sections = (g.sections || []).filter((x) => (highlight || []).includes(x.role) && x.x1_m > x.x0_m).map((x) => ({ role: x.role, name: x.name, d: outline(clip(x.x0_m, x.x1_m)) }));
    const tail = g.profile[g.profile.length - 1];
    return { geometry: g, body: outline(g.profile), fins, sections, scale: s, top, cx, tailY: top + tail[0] * s, tailHalf: tail[1] * s };
  }
  window.gsVehicleSilhouette = Object.assign(vehicleSilhouette, { valid: validVehicle, generic: GENERIC_VEHICLE });

  // Full screen: the Fullscreen API (prefixed on Safari) on the diagram's
  // holder, and in any case the holder covers the viewport (`gs-pd-full`),
  // which is what an iPhone gets. Esc, the platform's exit and the button leave.
  const fsElement = () => document.fullscreenElement || document.webkitFullscreenElement || null;
  const full = {
    holder: null,
    label(holder, button) { const on = holder.classList.contains('gs-pd-full'); button.textContent = on ? 'Exit full screen' : 'Full screen'; button.setAttribute('aria-pressed', String(on)); button.title = on ? 'Leave full screen (Esc)' : 'Show the diagram full screen (Esc leaves)'; },
    set(holder, on) { holder.classList.toggle('gs-pd-full', on); this.holder = on ? holder : null; const b = holder.querySelector('.gs-pd-fs'); if (b) this.label(holder, b); },
  };
  function enterFull(holder) {
    full.set(holder, true);
    const request = holder.requestFullscreen || holder.webkitRequestFullscreen;
    if (request) { try { request.call(holder)?.catch?.(() => {}); } catch (_) { /* overlay only */ } }
  }
  function leaveFull() {
    const holder = full.holder; if (!holder) return;
    if (fsElement()) { try { (document.exitFullscreen || document.webkitExitFullscreen).call(document)?.catch?.(() => {}); } catch (_) { /* already out */ } }
    full.set(holder, false);
  }
  if (typeof document !== 'undefined') {
    const changed = () => { if (!fsElement() && full.holder) leaveFull(); };
    document.addEventListener('fullscreenchange', changed);
    document.addEventListener('webkitfullscreenchange', changed);
    document.addEventListener('keydown', (e) => { if ((e.key === 'Escape' || e.key === 'Esc') && full.holder) { e.preventDefault(); leaveFull(); } });
  }

  let current = null;
  window.gsProgramDiagram = {
    derive,
    /** Show `topology` in `holder`, driven by the delayed snapshot `input`. */
    show(holder, topology, input) {
      if (!topology || !holder) return;
      const key = JSON.stringify(topology) + '|' + (window.gsProgramVehicle?.source?.hash || '');
      if (!current || current.holder !== holder || current.key !== key) current = { holder, key, refs: build(holder, topology) };
      update(current.refs, topology, input || {});
    },
    hide(holder) { if (full.holder === holder) leaveFull(); if (current && current.holder === holder) { holder.textContent = ''; current = null; } },
    enterFull, leaveFull,
  };
})();
