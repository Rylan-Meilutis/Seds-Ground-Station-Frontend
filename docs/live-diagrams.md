# Live diagrams and raw streams

## Live diagrams tab

**Live diagrams** (`live-diagrams`) draws four live views:

- **Fill system**: the real plumbing. Each valve shows whether it is open, closed, pending or stale. Each line shows which fluid is flowing. Pressures and masses sit where they are measured.
- **AV bay**: boards, buses, the radio, sensors, the estimator, pyro channels, outputs and SD logging.
- **System overview**: the network topology with live link health.
- **Test stand**: load cells and thrust. In IGT the rocket appears as a second body.

The tab is available to every session with view-data permission. Split view can
place it next to Actions. **Broadcast size** frames the diagram at 16:9 with
larger text, for a wall display or OBS window capture.

- Topologies, state rules and sources: GroundStation `docs/live-diagrams.md`.
- Code:
  - `src/telemetry_dashboard/live_diagrams/topology.rs`: schema and validation.
  - `derive.rs`: pure rules, unit-tested.
  - `render.rs`: SVG.
  - `mod.rs`: the tab and its live inputs.
- Bundled fallback topologies: `live_diagrams/topologies/*.json`. Keep them identical to GroundStation `backend/layout/diagrams/`. The station's copies replace them at load.

### Rendering

- **10 Hz.** One timer, paused while the page is hidden. Inputs are read from the existing latest-row store, the action policy and board status, without subscribing.
- **Low DOM churn.**
  - The element tree is fixed by the topology (keyed), so an update changes only classes and text.
  - Flow is a CSS stroke-dash animation with no JavaScript per frame. It stops under `prefers-reduced-motion`.
- **Theme.**
  - Colours come from the dashboard theme.
  - Fluid colours are the first three categorical slots of the validated palette, with light and dark steps chosen from the theme's background.
  - Gas and vapour lines are also dashed, and every state has a text label, so colour is never the only cue.
- **Platforms.** Plain inline SVG and CSS animations (no `:has`, no SMIL, no canvas). They work in WKWebView, WebView2, WebKitGTK and Android WebView. The layout scales with the view box down to phone widths.

### Programme (audience) scenes

The Stream Manager camera layout and the rundown scene lists offer:

- **Live diagram: fill system**
- **Live diagram: AV bay**
- **Live diagram: system overview**
- **Live diagram: test stand**

The programme page draws them with `station_ui/scripts/program-diagram.js`,
from the delayed programme snapshot, so the diagram stays in step with the
delayed video. These controls need stream-manager permission, as before.
`tests/station-program-diagram.test.cjs` checks that the programme applies the
dashboard's rules.

## Raw streams tab

**Raw streams** (`raw-streams`) lists every telemetry stream the station
decodes. Each row shows the latest values with their field names and units,
the rate, the age, the provenance (live, sim or replay) and a row count. The
list polls `/api/stream/raw/catalog` once a second. Streams older than 10 s
are dimmed.

It also gives copyable connection details for the external feed: the WebSocket
URL, the NDJSON URL and a curl line, with optional type/sender filters and a
per-stream rate cap.

Administrators issue, list and revoke **stream viewer tokens** here:

- A token is shown once.
- It is read-only and reaches only the raw feed.
- Revoking it closes open feeds within about a second.

Feed schema and examples (OBS browser source, curl, Python): GroundStation
`docs/backend/raw-stream-feed.md`.
