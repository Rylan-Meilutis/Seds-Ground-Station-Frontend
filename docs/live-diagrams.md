# Live diagrams and raw streams

## Live diagrams tab

**Live diagrams** (`live-diagrams`) draws **one combined view per station
mode**, opened automatically:

| Station mode | View | Shows |
| --- | --- | --- |
| Normal, HITL, testing | **Flight: fill system and vehicle on the pad** (`flight`) | The fill plumbing with the KG50 under the motor (fill mass), and the vehicle's AV bay: boards, buses, the radio to the GS and RRLS, sensors, the estimator, pyro channels, outputs and SD logging. No KG1000, no chamber PT. |
| Test fire | **Test fire: fill system and motor on the stand** (`test_fire`) | The fill plumbing and the motor on the stand with the KG50, the KG1000 (thrust) and the chamber PT. No AV bay. |
| Integrated ground test | **Integrated ground test: stand and rocket** (`ground_test`) | The test-fire view; while the test runs, the rocket on the ground as a second body with its replaying FC. |
| Any (picked by hand) | **System overview** (`network`) | The network topology with live board and link health. |

In every view each valve shows whether it is open, closed, pending or stale,
each line shows which fluid is flowing, and pressures and masses sit where they
are measured.

The view comes from the station's layout (`live_diagram`, which GroundStation
resolves per mode). A layout without it (an older station or replay bundle)
falls back to the mode flags: `test_fire_mode`, the IGT status, else flight
(`live_diagrams::mode_view`). The operator can pick any other view from the
toolbar; the mode's view is marked **mode** and comes first. A change of
station mode returns to the new mode's view. A bundle from the old per-system
set still opens: its `fill`, `av_bay` and `stand` views are listed after the
others.

### Full screen

Every view has a **Full screen** button in its title row (live, demo, replay
and broadcast size alike).

- Browsers, WebView2, WebKitGTK and Android: the Fullscreen API on the diagram panel (prefixed on Safari).
- iPhone, or a webview that refuses the API: the panel covers the whole viewport (`data-full`, fixed, above everything, inside the safe areas).
- **Esc**, the platform's own exit and **Exit full screen** leave.
- The SVG fills the screen at its aspect ratio, so it stays crisp, and keeps updating at 10 Hz.

The AV-bay and IGT views draw the station's vehicle true to scale from
`GET /api/vehicle/geometry` (a frame's `vehicle` box); see [vehicle geometry](vehicle-geometry.md).

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

- **Live diagram: this station mode** (`diagram-mode`)
- **Live diagram: flight (fill + vehicle)**
- **Live diagram: test fire stand**
- **Live diagram: integrated ground test**
- **Live diagram: system overview**

A saved scene of the old set (`diagram-fill`, `-av_bay`, `-stand`) shows its
successor. The programme page has the same full-screen button, shown only while
the pointer moves (a captured programme never shows it); Esc leaves.

The programme page draws them with `station_ui/scripts/program-diagram.js`,
from the delayed programme snapshot, so the diagram stays in step with the
delayed video. These controls need stream-manager permission, as before.
`tests/station-program-diagram.test.cjs` checks that the programme applies the
dashboard's rules (including OVR, the per-binding staleness and full screen).
`tests/live-diagrams-browser.mjs` drives the tab in demo mode and a test-fire
replay: each mode's view, full-screen enter and exit, and a phone width.

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
