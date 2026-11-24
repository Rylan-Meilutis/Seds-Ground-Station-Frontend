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
  - the recorded station mode (`/api/station_mode`, `/api/station-modes`
    with changes not supported), the recorded flight review as
    `/api/flight_review/live`, the 3D view configuration of a replay file,
    the firmware record (below) and an empty translation catalog;
  - a live-only route: 404 with one plain sentence the panel shows as is,
    for example "Live cameras need a Ground Station." or "Flashing needs a
    station." (`local_station::station::live_only`). A station setting the
    session does not hold says "Demo mode has no station settings…" or
    "This replay does not include the station's settings…". Any other
    route: "Not available offline: this route needs a Ground Station.";
    `saved_json` passes the local station's sentence through unwrapped.
- One T clock: the playback bar and the dashboard header both show the
  local station's launch clock (`LocalStation::tclock_at`), formatted by
  `tclock::launch_clock_text`. The station sends it as held samples
  (`anchor_timestamp_ms: null`, `duration_ms` = time left or elapsed), one
  per 100 ms tick while it runs, which the header shows without advancing
  them on the wall clock; so it follows playback at any speed, after a seek
  and stops when paused. Before the countdown both show the countdown length
  (`T- 00:10.00`); the bar shows the position into the session beside it.
  `tests/tclock-agreement-browser.mjs` checks the two agree at 1x, 10x,
  after a seek and paused.
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

## Firmware tab

In demo mode and replays the Firmware tab is read-only
(`firmware_recorded.rs`): "Recorded firmware and board health, read-only.
Flashing needs a station: connect to a Ground Station to update a board." It
shows `GET /api/recordings/{id}/firmware` for the session's recording (a
bundle's `derived/firmware.json`): per board the rows recorded, the firmware
build where the recording names it, and the health from its `BOARD_HEALTH`
reports (report count, uptime at the first and last report, resets during
the recording, die temperature, reset flags, watchdog mask, service faults,
heap peak), plus firmware transfers during the recording.

Board firmware does not report its build over SEDSnet, so a hardware
recording names no build; the record's `identity_note` says so. Firmware
simulation captures name each board's repository, ref, commit and SEDSnet
commit (the capture manifest's `versions.simulated_firmware`). A portable
replay file lists the boards it heard without health. The demo shows example
builds and health.

## Offline inventory

Every GET the dashboard sends to a local station that it did not serve,
found by visiting every tab, every Settings section (Client and Ground
Station) and the Tools menu with the release web build in demo mode and with
the contract-fixture bundle (`window.__gs26LocalMisses` lists them in the
browser; `LocalStation::misses` in tests). "Before" is the state before this
sweep; "owned elsewhere" routes are handled by other work.

| Route | Where | Before | Now |
| --- | --- | --- | --- |
| `/api/recordings/{id}/firmware` | Firmware tab | (tab showed live flashing; `/api/firmware/targets` failed) | bundle `derived/firmware.json`; demo data; replay file: boards heard |
| `/api/firmware/targets`, `/api/firmware/updates*`, `/api/firmware/flash/*` | Firmware tab | 404 error | not requested offline; the tab says "Flashing needs a station" (live only) |
| `/api/station_mode` | Settings, operating-mode line | "Operating mode: unavailable" | computed: the recorded mode |
| `/api/station-modes` | Settings → Ground Station, mode change | "Mode observation: Not available offline … (HTTP 404 …)" | computed: recorded mode only, `change_supported: false` with the reason "…changing mode needs a Ground Station." |
| `/api/flight_review/live` | Motor & flight | "Cannot read the motor and flight summary: Export request failed (404)…" | computed from the bundle's `/api/flight_review/recording/{id}`; demo: empty flight section |
| `/api/i18n/catalog` | every page | silent 404 | bundle `config/station/i18n_catalog_en.json` (English); otherwise an empty catalog (the UI's own wording) |
| `/api/vehicle_visualization` | 3D replay | silent 404 (demo) | bundle entry; else the replay document's `view_config` |
| `/api/map_config` | Map | silent 404 (demo) | demo map around its launch site; bundle entry |
| `/api/fill_targets`, `/api/flight_setup`, `/api/auto_zero`, `/api/calibration/kg50_input`, `/api/gse/config`, `/api/fc/launch-site`, `/api/safety/about` | Settings → Ground Station (Mission, Loadcells, Sequences & limits, About safety) | "… unavailable: Export request failed (404): Not available offline…" | bundle `config/station/*` (station settings at export, read-only); demo: "Demo mode has no station settings; connect to a Ground Station to see them." |
| `/api/apogee_planner` | Settings → Mission | 404 error (demo) | bundle entry; demo: the settings sentence |
| `/api/calibration_config`, `/api/diagrams`, `/api/vehicle/geometry` | Dashboard, Live diagrams, 3D replay | silent 404 (demo) | bundle entries; demo: built-in defaults (diagrams from the layout, generic rocket) |
| `/api/live_streams`, `/api/media-assets/*`, `/api/video/*` | Mission Live, media | "Camera service unavailable: Export request failed (404)…" | live only: "Live cameras need a Ground Station." (recorded camera/voice playback: owned elsewhere) |
| `/api/stream/raw/catalog` | Raw streams | "Raw stream catalog unavailable: Export request failed (404)…" | live only: "The raw packet feed is live only; it needs a Ground Station." |
| `/api/gse/status` | Dashboard (GSE sequences) | silent 404 | live only: "GSE sequences run on a Ground Station." |
| `/api/actuator/plumbing/status` | Settings → Sequences & limits | "Actuator configuration unavailable: Export request failed (404)…" | live only: "Actuator status is live only; it needs a Ground Station." |
| `/api/mission-settings` | Settings → Network & clocks | "Shared settings are unavailable on this station" | live only (inter-station sync), unchanged wording |
| `/api/firmware-sim` | Settings → Mission | silent 404 | live only: "Firmware simulation runs on a Ground Station." |
| `/api/igt` | Dashboard | silent 404 | live only: "The integrated ground test runs on a Ground Station." |
| `/api/proximity` | Dashboard | silent 404 | live only |
| `/api/recovery` | Dashboard | silent 404 | live only |
| `/api/radio/reject-test`, `/api/radio/modem` | Dashboard, Connection Status | silent 404 | live only |
| `/api/board-recovery/status` | Connection Status | silent 404 | live only |
| `/api/antenna/status` | Map, Settings | silent 404 | live only |
| `/api/rrls/status` | Actions | silent 404 | live only |
| `/api/checklists/sync` | Checklists | silent 404 | live only (checklists come from `/api/checklists`) |
| report, motor/flight custom windows, SD-resolved exports, checklist and recovery exports | Data Export, Report Studio, Motor & flight | — | owned elsewhere |

Writes (commands, settings saves, uploads, flashing, admin and user
management, pairing, power, SD downloads) never leave the device: the demo
acknowledges them, a replay refuses them ("Replay is read-only: … was not
sent.").

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
