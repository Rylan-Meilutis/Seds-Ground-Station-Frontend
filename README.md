# UBSEDS Ground Station Frontend

This repository contains the Dioxus-based frontend for the UBSEDS ground station UI.

Current release:

- Frontend version: `0.3.1`
- App build: `23`
- Dioxus line: `0.7.6`
- Targets: web, macOS desktop, Android, iOS

Backend compatibility note:

- The current frontend contract has been checked against `../groundstation26/backend/src/web.rs`, `../groundstation26/backend/src/state.rs`, and `../groundstation26/backend/src/types.rs`.
- In addition to the older dashboard routes, this frontend now expects backend support for shared alert acknowledgement state, message history, board `packet_count`, and the `ActionPolicy` / `FillTargets` / `RecordingStatus` WebSocket snapshots.

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
- Offline native startup with locally cached layout/telemetry/GPS/map state when the configured Ground Station cannot be reached
- Per-Ground-Station cache isolation for layout, telemetry, GPS, and map tile state
- Calibration tab local draft persistence and offline restore of the last seen calibration layout/document per Ground Station URL
- Calibration zero-point editing toggle to preserve or recalculate the active regression
- Settings tabs for general, map, telemetry, history, and maintenance controls
- Maintenance settings for rotating frontend debug logs, per-log export/share/download/view, and clearing local logs
- Settings controls for data cache, map tile cache, map tile prefetch, cache storage budget, and manual map tile prefetch
- Launch clock badge and launch clock synchronization from both HTTP and WebSocket updates
- Sender-aware multi-series charts and per-series scaling for state/data graphs
- Network topology graph with configurable flow animation and layout direction
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
- `GET /api/messages`
- `GET /api/notifications`
- `POST /api/alerts/ack`
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
cargo install dioxus-cli --version 0.7.6
dx serve
```

Build via helper:

```bash
python3 build.py web
```

Use `python3 build.py` for the full build helper usage.

The codebase is currently pinned to Dioxus `0.7.6`. Any compatible `0.7.x` CLI is the safe choice; the helper also contains guards for patch-level CLI/runtime skew.

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

Android native notes:

- the generated WebView client proxies `gs26.local/tiles/...` tile requests to the configured Ground Station and keeps an app-cache tile fallback keyed by Ground Station URL plus tile path
- Android tile cache entries for one Ground Station URL are not reused for another URL
- Android location uses both GPS and network providers with high-frequency updates, and heading uses the rotation-vector sensor

## iOS Build And Signing Notes

The build helper owns iOS packaging after `dx bundle`:

- `python3 build.py ios` creates a distribution-signed IPA
- `python3 build.py ios_dist_sign [existing]` signs an existing app bundle as a distribution IPA
- `python3 build.py ios_deploy [debug] [existing]` uses development signing for connected-device install
- `python3 build.py ios_sign [debug] [existing]` signs an existing app bundle with development signing

Provisioning profile defaults are intentionally separate:

- development/device deploy: `Groundstation_dev.mobileprovision`, or `IOS_DEVELOPMENT_MOBILEPROVISION=/path/to/profile.mobileprovision`
- distribution/TestFlight/App Store style package: `UBSEDS_GroundStation.mobileprovision`, or `IOS_DISTRIBUTION_MOBILEPROVISION=/path/to/profile.mobileprovision`

Distribution signing requires an `Apple Distribution:` identity that matches the distribution profile. Development signing requires an `Apple Development:` identity and a provisioning profile with provisioned devices.

## Settings, Cache, And Map Prefetch

The Settings overlay is split into tabs:

- `General` groups language, time format, and theme controls.
- `Map` groups map header, manual location/heading, and map tile prefetch controls.
- `Telemetry` groups network, remote alert acknowledgement, chart, and calibration capture controls.
- `History` groups recent telemetry timeline settings.
- `Maintenance` groups storage inspection, rotating debug logs, cache controls, and reset actions.

The History tab includes two operator-facing telemetry timeline controls:

- `Keep data for` sets how long recent telemetry is retained locally before older samples are dropped.
- `Visible chart range` sets how much recent telemetry the charts show at once. It is automatically capped so it cannot exceed the retained data duration.

The Maintenance tab includes separate storage, log, and cache controls:

- Used Storage shows a breakdown for frontend data cache, map tile cache, layout/settings cache, and related local storage.
- Frontend debug logs are rotated locally with a combined native cap of about `100 MB`.
- Logs are intended for frontend/runtime debugging only and exclude location coordinates, telemetry payload dumps, passwords, and auth tokens.
- The log action is per-file: operators choose an individual log artifact before using `Download Logs` on web, `Share Logs` on mobile, or `View Logs` on desktop.
- Clear Logs removes locally stored debug logs without touching cached data or saved settings.
- Cache Storage Limit defaults to `500 MB`. It is a budget/warning gate for data and map caches, not a hard filesystem quota.
- Data Cache can be disabled independently. When disabled, telemetry/layout restore data is not written or restored from the local data cache.
- Map Tile Cache can be disabled independently. When disabled, map tiles are fetched for display but not written to the persistent tile cache.
- Map Tile Prefetch can be disabled independently from normal tile caching.
- Clear Cache clears only frontend data/telemetry cache.
- Clear Cache And Map Tiles clears data/telemetry cache plus map tile cache.
- Clear All Caches clears data/telemetry cache, map tile cache, and layout/settings cache.
- Prefetch Map Tiles manually triggers the current map/user/rocket tile prefetch plan.

Map prefetch behavior:

- prefetch radius settings are separate for user and rocket
- radius values are shown and edited in the selected distance units
- the UI estimates user-radius, rocket-radius, and combined tile/cache usage from the sampled tile size
- persistent high-resolution prefetch is limited by the configured cache budget estimate instead of a fixed tile-count cap
- fixed tile-count caps remain only for memory-sensitive paths such as live tracking prefetch and DOM map rendering
- native/web tile caches are keyed by tile URL/Ground Station URL, so changing the configured URL invalidates old tile cache use for the new URL
- if the Ground Station is reachable, `/api/recent` overwrites the cached telemetry snapshot; cached telemetry is only a fallback

The dashboard reload button refetches and reapplies layout/config for the active Ground Station. On native builds it also reuses cached layout only when live layout fetch fails.

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
- the live 20-minute history window is pruned using local receive time, not packet/network timestamp, so delayed or skewed packets do not incorrectly evict visible history
- native builds keep the last valid layout per Ground Station URL plus a compact local telemetry snapshot so a failed connection attempt can still open the dashboard with the last remembered data/GPS/map state. Without a cached layout for that URL, the app shows the connection failure page.

If you are implementing the backend streaming path, use [`docs/backend-recent-streaming.md`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/backend-recent-streaming.md).

## Calibration Behavior

- The Calibration tab fetches both `/api/calibration_config` and `/api/calibration`, then caches the last successful responses per Ground Station URL for offline reuse.
- If the backend disconnects later, the tab keeps showing the last cached calibration layout/document instead of rendering blank.
- Unsaved calibration edits are stored locally as a draft per Ground Station URL, so switching tabs or remounting the page does not discard a local refit/edit before it is saved to the backend.
- Saving calibration clears the local draft and refreshes the cached backend copy.
- Zero-point changes include a `Preserve regression when changing zero point` toggle so operators can choose whether the existing fit shape should shift with the new zero or be recalculated independently.
- Calibration sensor definitions, channel ids, labels, colors, and allowed regression modes still come entirely from `/api/calibration_config`.

## Notes For Backend Authors

- Route paths are hardcoded in the frontend. Matching them exactly is the easiest path to compatibility.
- Some routes can safely return empty data structures. For example, `/api/recent` can return `[]` and `/api/gps` can return `{ "rocket": null }`.
- `/api/recent` can also stream newline-delimited JSON rows on native builds for faster reseed startup.
- The map uses `/tiles/{z}/{x}/{y}.jpg` by default. If you expose the map tab, provide that route or an equivalent reverse-proxied path.
- The map persists the last effective tile `max_native_zoom` per tile URL in browser/native storage and reuses it offline. Prefetch warms tiles around the viewport, user, and rocket from the effective minimum zoom through the remembered native max so zooming out can stay cached. Display zoom can overzoom above native tile zoom, but tile requests stay capped at native max zoom so cached high-zoom native tiles can still be restored after a backend disconnect.
- Map tile and offline data caches are isolated by configured Ground Station URL. Changing the URL prevents stale tile/data reuse from the previous URL.
- WebSocket message tags are case-sensitive because they are deserialized from Rust enum variant names.
- Commands are sent over WebSocket as JSON objects like `{ "cmd": "Abort" }`.
- Layout drives a large part of the UI. If your backend returns a small valid layout, the frontend can still function without the full production config.
- Raw `KG1000` is treated as raw-only telemetry. Backends should publish `LOADCELL_WEIGHT_KG` and `LOADCELL_FILL_PERCENT` explicitly if they want calibrated loadcell values and fill percentage to appear in labels/charts.
- `LOADCELL_FILL_PERCENT` should use the active fill target for the current flight state. Current behavior is nitrogen target during `PreFill`, `FillTest`, and `NitrogenFill`, and nitrous target otherwise.
- When `/api/fill_targets` is edited and saved, the frontend recomputes visible loadcell fill-percentage displays immediately from the latest calibrated mass and the new target snapshot.
- Ground Station-provided theme colors are optional and are only used when the user explicitly selects the `backend` preset.

## Regression Checks

Run the fast Rust regression suite before shipping layout, telemetry, chart, or reconnect changes:

```bash
cargo test
```

The current suite covers layout parsing/validation, sender-aware chart series, launch-clock monotonic behavior, and state/data chart regressions, including current loadcell display behavior.

## Reference Files

- API reference: [`docs/backend-api.md`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/backend-api.md)
- Streaming notes: [`docs/backend-recent-streaming.md`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/backend-recent-streaming.md)
- Minimal layout example: [`docs/api-examples/layout.minimal.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples/layout.minimal.json)
- WebSocket examples: [`docs/api-examples/websocket-messages.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples/websocket-messages.json)
