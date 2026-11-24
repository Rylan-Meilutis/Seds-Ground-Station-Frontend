# Demo mode and replay files: the live dashboard on a local station

With no station connected, the app runs its normal dashboard against a
station answered on the device, at `/preview`:

- native: **Explore offline** on the connection page or after a connection
  failure, or opening a `.gsreplay` file from another app
  ([replay-bundle.md](replay-bundle.md#opening-files-from-other-apps));
- web: **Explore offline** on the connection-failure card, or open
  `/preview` directly.

There is one UI. Every tab, card, chart, diagram and control is the live
dashboard's own code, driven by a local station instead of a Ground Station.
A slim bar above the dashboard says what is playing and offers play/pause, a
position slider, 1–50× speed, **Open replay file**, **Back to demo** and
**Exit** (back to connection setup).

## How it works

The dashboard's station is chosen once, when its connection starts
(`src/telemetry_dashboard/station_link.rs`):

| Selection | HTTP | Live frames | Commands and writes |
| --- | --- | --- | --- |
| Remote (default) | `{base}/api/...` over reqwest / fetch | `/ws` WebSocket | sent to the station |
| Local (this route) | answered by `LocalStation::request` | `LocalStation::frames`, fed to the same message handlers | answered by the local station; nothing is sent |

- Every HTTP helper asks `station_link` before it builds a request (auth,
  `saved_json`, settings writes, downloads, commands, mode transport,
  firmware, plumbing, GSE configuration, SD download, the `/api/recent`
  seed). The WebSocket supervisor asks once per connection.
- While a local station is selected the base URL is
  `local-station://this-device`, which no HTTP client or WebSocket can
  reach: a call site that missed the local answer fails instead of sending.
- `LocalStation` (`local_station/station.rs`) serves a session as a station:
  - a bundle entry under the route it snapshots, or an export computed from
    the bundle (`local_station/exports.rs`);
  - routes computed from the rows: `/api/recent`, `/flightstate`, boards,
    topology with per-board health, alerts, action policy (the layout's
    commands), launch clock, recordings and the recording store, replay
    snapshot and windows, checklists (demo);
  - frames as its playback clock passes them: `TelemetryBatch`,
    `FlightState`, `LaunchClock`, `Warning`, `BoardStatus`,
    `NetworkTopology`, `NetworkTime`, and a `Notification` answering each
    command;
  - any other route: 404 "Not available offline: this route needs a Ground
    Station" (camera, voice, firmware and similar panels say so).
- Rows are stamped with the wall-clock time they are pushed, so the live
  charts, ages and staleness checks behave as with a station. A seek, file
  change or restart remounts the dashboard and reseeds its history from the
  new position.

## Demo mode

A looping 380 s scenario: fill, arm, a 10 s countdown, the GroundStation26
IGT `nominal_10k` mock flight to about 10,000 ft, landing. Seven boards
report and four example warnings fire. Its layout, boards, calibration and
checklists are data in `local_station/demo.rs` in the station's shapes;
nothing in the pages names them.

Commands and writes are acknowledged by the demo station: "Demo: “Vent”
acknowledged. Nothing was sent; no station is connected." (a notification for
socket commands, HTTP 409 with the same text for writes).

## Replay files

**Open replay file** (or dropping the file on the window, or opening it from
another app) loads a replay bundle (`*.gsreplay`) or a portable
`*.seds-replay.json`. The replay plays from its first row. Every route the
bundle snapshots is served from it, the exports are produced from it byte for
byte as the station produces them ([replay-bundle.md](replay-bundle.md)),
and the 3D replay, Motor & flight, Data Export and Report Studio work on the
bundle's recording. Commands answer "Replay is read-only: “Abort” was not
sent."

## Isolation and cost

- Preferences and caches the dashboard writes while a local station is
  selected go to a scratch overlay (native: `persist`; browser: a
  `localStorage` overlay in page memory). Saved station addresses,
  credentials, layouts, calibration and checklists are untouched.
- The map draws only the bundle's tiles (or none) and fetches nothing; the
  native tile protocol handler never contacts a station while local.
- The watch companion marks local data as demo.
- Leaving the route drops the selection, the station, its rows and the
  overlay. With a station connected nothing of this runs: one thread-local
  read per HTTP request, nothing per row. The dashboard route asserts in
  debug builds that no local session is alive.

## Validation

```sh
cargo test --bin groundstation_frontend -- local_station offline_preview
dx build --platform web --release
node scripts/serve-static.mjs target/dx/groundstation_frontend/release/web/public 19096 &
node tests/offline-preview-browser.mjs
```

The browser test drives the live tabs (Dashboard, Data, Live diagrams,
Connection Status, Checklists, Data Export, 3D replay, Map, Actions) in demo
mode, from a bundle and from a portable replay file; checks that the clock
runs, that commands are acknowledged locally, that the bundle's CSV export
equals the station's, two widths without overflow, zero network requests, no
WebSocket and unchanged browser storage. `GS_PREVIEW_URL`,
`PLAYWRIGHT_MODULE`, `CHROME_BINARY` and `GS_UI_ARTIFACTS` (screenshots) work
as in the other browser tests.

`tests/live-ingest-benchmark.mjs` measures live telemetry ingest and render
against a mock station, for before/after comparisons of the transport.
