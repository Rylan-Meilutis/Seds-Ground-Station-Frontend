# UBSEDS Ground Station Frontend

This repository contains the Dioxus-based frontend for the UBSEDS ground station UI.

Current release:

- Frontend version: `0.3.1`
- App build: `11`
- Dioxus line: `0.7.5`
- Targets: web, macOS desktop, Android, iOS

If your goal is to build a backend that works with this frontend, start here:

- Backend contract: [`docs/backend-api.md`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/backend-api.md)
- Example payloads: [`docs/api-examples`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples)
- Build helper: [`build.py`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/build.py)
- Streaming `/api/recent` notes: [`docs/backend-recent-streaming.md`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/backend-recent-streaming.md)

## What The Frontend Expects

The app talks to a Ground Station over both HTTP and WebSocket.

- HTTP base URL: configured by the user in the app, for example `https://backend.example.com`
- WebSocket URL: derived automatically from the HTTP base as `{ws_scheme}://{host}/ws`
- HTTP auth: `Authorization: Bearer <token>` when logged in
- WebSocket auth: `?token=<token>` query parameter when logged in
- Default native backend URL when none is configured: `http://localhost:3000`

The app uses HTTP for:

- initial page and dashboard data loads
- layout/config fetches
- auth
- telemetry reseed/history loading
- launch clock snapshots
- calibration and setup editor writes
- dismissing persistent notifications

The app uses WebSocket for:

- live telemetry
- flight state updates
- warnings and errors
- network and board status
- action policy updates
- command sends from the UI to the Ground Station

## Current App Features

- App-wide built-in themes, including `default`, `light`, `sunset`, `forest`, `high_contrast`, and `backend`
- Theme presets defined in JSON and compiled into the app at build time
- Local Python theme editor for adding and editing theme presets
- Global theme propagation across the full window shell and dashboard tabs
- Native connect screen with route probing, TLS-skip testing, and WebSocket handshake diagnostics
- Ground Station colors only apply when the selected preset is `backend`
- Contrast normalization for text, buttons, alerts, and panel surfaces
- Emergency `Abort` button always rendered in red regardless of theme or Ground Station palette
- Telemetry reseed support using either a JSON array or native streamed NDJSON from `/api/recent`
- Graph-level reseed status notices for running, success, and failure
- Offline native startup with locally cached telemetry/GPS/map state when the configured Ground Station cannot be reached
- Launch clock badge and launch clock synchronization from both HTTP and WebSocket updates
- Sender-aware multi-series charts and per-series scaling for state/data graphs
- Network topology graph and endpoint ownership views
- Built-in version page showing app/build/package runtime information
- Localized UI support through translation catalog and on-demand translation routes
- Ground Station / App wording in user-facing copy instead of internal `backend` / `frontend` terminology

## Minimum Compatible Backend

A minimal backend should implement at least:

- `GET /api/layout`
- `GET /api/recent`
- `GET /api/alerts`
- `GET /api/map_config`
- `GET /tiles/{z}/{x}/{y}.jpg`
- `GET /flightstate`
- `GET /api/gps`
- `GET /api/auth/session`
- `POST /api/auth/login`
- `POST /api/auth/logout`
- `GET /ws` or equivalent WebSocket upgrade on `/ws`

For a more complete dashboard, also implement:

- `GET /api/action_policy`
- `GET /api/launch_clock`
- `GET /api/network_time`
- `GET /api/network_topology`
- `GET /api/boards`
- `GET /api/notifications`
- `POST /api/notifications/{id}/dismiss`
- `GET/POST /api/flight_setup`
- `POST /api/flight_setup/apply`
- `GET/POST /api/fill_targets`
- `GET /api/calibration_config`
- `GET/POST /api/calibration`
- `POST /api/calibration/capture_zero`
- `POST /api/calibration/capture_span`
- `POST /api/calibration/refit`
- `GET /api/i18n/catalog?lang=<code>`
- `POST /api/i18n/translate`

Layout-sensitive behavior is backend-driven. Board ids, data tab labels, graph enablement, sender-split chart types, boolean labels, state widgets, state summary fill-target metadata, and calibration channel names/colors/regressions should be supplied by `/api/layout` and `/api/calibration_config`; avoid depending on frontend hardcoded telemetry names. Layout validation rejects duplicate tab ids, invalid known chart-series indexes, invalid chart-group channel references, and incomplete fill-target metadata.

## Running The Frontend

Web:

```bash
cargo install dioxus-cli --version 0.7.5
dx serve
```

Build via helper:

```bash
python3 build.py web
```

Use `python3 build.py` for the full build helper usage.

The codebase is currently pinned to Dioxus `0.7.5`. Any compatible `0.7.x` CLI is the safe choice; the helper also contains guards for patch-level CLI/runtime skew.

## Android Build Notes

The Android build helper does more than just call `dx`:

- patches the generated Android Gradle project after `dx bundle`
- applies repo-managed Android fixes on rebuild instead of requiring manual edits under `target/dx/...`
- emits context-aware error messages so downstream Gradle or Android failures are not mislabeled as missing `dx`
- can recover from corrupted repo-local Gradle cache state in `.gradle-user-home`

If Android bundling fails, read the Gradle error block first. The helper distinguishes between:

- missing tools such as `dx`, `cargo`, or `bash`
- generated Android project failures
- Gradle cache corruption
- upstream R8 / AGP warnings that do not come from this repo

## Editing Themes

Built-in theme presets live in [`assets/themes/presets.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/assets/themes/presets.json). They are validated in `build.rs` and compiled into the app at build time, so there is no runtime file dependency.

For easier editing or adding presets, use the local Tkinter editor:

```bash
python3 scripts/theme_editor.py
```

The editor writes back to `assets/themes/presets.json`. After saving, rebuild the frontend normally and the updated theme catalog will be embedded into the app.

Theme notes:

- `backend` is the only preset that applies Ground Station-provided palette colors
- all other presets use the compiled app theme palette
- the full window shell, menus, tabs, chart panels, and major dashboard pages are intended to follow the active theme
- button and panel colors are normalized to keep borders and text readable across themes

## Telemetry Reseed Behavior

When the app reconnects or explicitly reseeds, it keeps existing chart history visible while reseed is in progress.

- `/api/recent` may return the legacy JSON array response
- native builds also accept streamed NDJSON from `/api/recent`
- WebSocket reconnects trigger telemetry reseed and preserve live rows received during reseed
- reseed status is shown directly on graphs so operators can tell whether it is running, succeeded, or failed
- if reseed fails after data was already visible, the app keeps the existing visible history instead of blanking the graphs
- native builds keep a compact local telemetry snapshot so a failed connection attempt can still open the dashboard with the last remembered data/GPS/map state

If you are implementing the backend streaming path, use [`docs/backend-recent-streaming.md`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/backend-recent-streaming.md).

## Notes For Backend Authors

- Route paths are hardcoded in the frontend. Matching them exactly is the easiest path to compatibility.
- Some routes can safely return empty data structures. For example, `/api/recent` can return `[]` and `/api/gps` can return `{ "rocket": null }`.
- `/api/recent` can also stream newline-delimited JSON rows on native builds for faster reseed startup.
- The map uses `/tiles/{z}/{x}/{y}.jpg` by default. If you expose the map tab, provide that route or an equivalent reverse-proxied path.
- The map persists the last effective tile `max_native_zoom` per tile URL and reuses it offline, so cached high-zoom tiles can still be restored after a backend disconnect.
- WebSocket message tags are case-sensitive because they are deserialized from Rust enum variant names.
- Commands are sent over WebSocket as JSON objects like `{ "cmd": "Abort" }`.
- Layout drives a large part of the UI. If your backend returns a small valid layout, the frontend can still function without the full production config.
- If you publish raw `KG1000` loadcell rows, the frontend derives `LOADCELL_WEIGHT_KG` and `LOADCELL_FILL_PERCENT` for labels and charts. Backends can also publish those derived rows directly.
- Ground Station-provided theme colors are optional and are only used when the user explicitly selects the `backend` preset.

## Regression Checks

Run the fast Rust regression suite before shipping layout, telemetry, chart, or reconnect changes:

```bash
cargo test
```

The current suite covers layout parsing/validation, sender-aware chart series, derived loadcell labels/charts, launch-clock monotonic behavior, and state/data chart regressions.

## Reference Files

- API reference: [`docs/backend-api.md`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/backend-api.md)
- Streaming notes: [`docs/backend-recent-streaming.md`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/backend-recent-streaming.md)
- Minimal layout example: [`docs/api-examples/layout.minimal.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples/layout.minimal.json)
- WebSocket examples: [`docs/api-examples/websocket-messages.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples/websocket-messages.json)
