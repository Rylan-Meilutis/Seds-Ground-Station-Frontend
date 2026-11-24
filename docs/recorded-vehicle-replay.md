# Recorded vehicle replay

Open **3D replay**, select a recording, then use Play, Rewind, the timeline and
0.25×–4× speed. Selecting an active recording freezes its current row boundary;
Reload explicitly selects a new snapshot. Missing recorded channels remain
unavailable. The live dashboard uses cameras and 2D artwork. A stream operator can explicitly select a recorded-flight scene for the audience program; this never represents live attitude or position.

Replay reads the selected database through authenticated, read-only APIs:

- `GET /api/recordings/{id}/replay` returns the frozen maximum row ID and recorded
  reception-time range.
- `GET /api/recordings/{id}/replay/window` accepts `upper_id`, `after_id`,
  `start_ms`, `end_ms` and a JSON array in `types`. Windows are limited to 30
  seconds and 32 types, with 512 rows per page. The first page includes the last
  earlier saved value for each requested source/type so recorded latched state
  remains available when seeking.

Rendering uses saved values and saved FC phase only. It never substitutes live
telemetry, current calibration or the current flight phase. Changing the native
app's station clears the recording selection; buffers are bound to station,
recording and snapshot. Timeline dragging commits a window request on release.
Animations follow the replay clock, including pause, slow motion and seeking.
The replay timeline uses saved reception timestamps; source timestamps remain
available in API rows and are not presented as a new monotonic time source.

The real-component browser regression is `tests/saved-vehicle-replay.mjs`.
Build with `dx build --platform web`, then run it with Node and Playwright.
`GS_FRONTEND_PUBLIC`, `GS_REPLAY_RENDERING_ROOT`, `PLAYWRIGHT_MODULE` and `CHROME_BINARY`
can select external build output, backend model assets and browser tooling.
The fixture serves local APIs only and never dispatches a hardware command.

## Flight scene and camera

The replay uses the existing bundled Three.js renderer, with a Canvas2D compatibility
preview. It loads a bounded saved GPS trajectory from
`GET /api/recordings/{id}/replay/trajectory?upper_id=N` once per frozen snapshot,
independently of the exact sample windows. The response contains `upper_id`,
`points` (`timestamp_ms`, `latitude`, `longitude`, `altitude_m`, optional
`break_before`), `truncated`, `sampled` and optional `source_sender_id`.
At most 8192 sampled points are accepted. One recorded GPS sender is selected for
the snapshot (RF preferred, FC fallback), and the exact position uses that same
sender. Sources are never stitched together. The exact rocket position and attitude
continue to come only from previous fresh samples at the selected replay time.
A missing current position switches to a clearly labelled reference-attitude mode:
the model stays available at a neutral artwork origin while geographic ground and
trajectory are hidden. The artwork origin is not presented as a measured position.
Missing attitude axes remain explicitly unavailable in the readout.

The first valid saved GPS fix defines the local east/up/north coordinate origin.
It does **not** establish surveyed ground elevation. The curved ground is a
mean-radius Earth reference using the GPS altitude datum, not downloaded terrain
or a claimed measurement of local ground. The scene labels that distinction.
Trajectory segments respect saved `break_before` boundaries; without that metadata,
they do not bridge GPS gaps longer than five seconds. A sampled
trajectory is labelled; it does not replace raw recording data.

Follow tracks the recorded rocket position. Flight path frames the saved trajectory;
Reset restores the follow camera. Drag to orbit, shift/right-drag to pan, scroll to
zoom, or use two-finger pan/pinch on touch screens. Camera controls also provide
44px buttons. Focus the scene for arrow-key orbit, W/A/S/D pan, Q/E vertical
movement, F follow and R reset. Camera actions remain available while paused.
No scene feature subscribes to live telemetry or dispatches a hardware command.

`node tests/replay-scene.test.mjs` exercises coordinate and gap handling without a
Rust/UI build. `node tests/replay-scene-browser.mjs` serves the actual renderer and
GLB in an isolated local fixture to exercise high-altitude placement, reference
ground, trajectory, camera controls, reference-attitude mode during missing GPS and narrow layouts.
Set `GS_REPLAY_WEBGL=1` to require an explicit Chrome SwiftShader WebGL run;
otherwise the fixture reports the renderer actually selected by the browser.


## Flight replay

**3D replay → Flight replay** plays a flight as flight states rather than positions
over time: *Open flight replay of the selected recording* loads the whole recording
(`/replay/export`), and *Follow the live flight* follows the dashboard's own
telemetry store with the cursor on the newest data (scrub to look back, **Go
live** to return). The same player opens `.gsreplay` bundles and portable files
offline (offline workspace → Replay & summary → Load 3D replay; exported HTML).
It is `GSOfflineReplay.mount` in `station_ui/rendering/three/offline-replay.js`
with `mountFlightScene` in `replay-scene.js`; `src/telemetry_dashboard/flight_replay_panel.rs`
hosts it on a connected station. Nothing here sends a command.

**Flight state.** Phases come from the recorded `FLIGHT_STATE` rows (the `phase`
name, else the number in `values[0]`), with the shared flight-plan vocabulary and
palette:

| FlightState | phase | colour |
|---|---|---|
| Startup, Idle, PreFill/Fill, FillTest, NitrogenFill, NitrousFill, Armed | pad | `#94a3b8` |
| Launch, Ascent (InFlight with no detail) | boost | `#f97316` |
| Coast, Apogee (an apogee marker, `#ffffff`) | coast | `#facc15` |
| Descent | drogue | `#38bdf8` |
| Reefing | main | `#22c55e` |
| Landed, Recovery | landed | `#a78bfa` |
| Aborted | the previous phase | |

Events are the transitions: ignition (into boost), burnout (boost → coast), apogee
(the Apogee row), drogue, main, landing. A flight review (`flight.events`,
`flight.apogee`) or a plan (`events[].t_s` from `created_ms`) only fills an event
the rows lack; with neither, apogee is the highest recorded altitude between
ignition and the drogue. T-0 is the ignition. The T-clock reads `T+ MM:SS`,
`T+ H:MM:SS`, `T+ D:HH:MM:SS`, `T- …` before T-0 and `T --:--` without one.

**The model.** The vehicle is the station geometry (`buildVehicle`); nothing names
a vehicle. On the pad it stands on the pad and rail at the first fix. In boost the
plume's length follows the recorded thrust (a `THRUST` type, else the largest
`KG…` load cell), else the chamber pressure, each against its recorded maximum,
else nominal. Attitude: the recorded FC attitude (`view_config.attitude`) while
free-flying, else the direction of the velocity from the track (a central
difference over ±2.5 s, so a single bad fix does not flip it), else vertical.
At the drogue the airframe parts at `separations[0]` and the drogue opens at its
`recovery` diameter and line length; the main joins it at Reefing. Landed lies on
the ground. The track is coloured by phase and drawn up to the replay time, with
event markers.

**Position.** One GPS sender (the backend trajectory's, else FC, else RF), never
stitched. A fix that exactly repeats the previous one is a held fix and is dropped.
Height is the barometric altitude above the pad when it is recorded (GPS height is
the weakest axis, worst in boost), else the GPS altitude; between fixes the
position is interpolated for display.

**Ground.** The scene imports `./terrain.js` (`loadTerrain`, owned by the 3D-map
work) and places the track, pad, stations, overlays and the landing point on its
`heightAt`; the track is shifted by the terrain height at the pad. Without it
(or in a standalone file) the ground is a curved surface at the pad altitude with
a scale grid, labelled as such.

**Sensors.** The HUD and the charts bind generically: for each role the recorded
data types are matched by name and the channel by the layout's data-tab channel
names; units come from `channel_formatters[].suffix` or a `(unit)` in the channel
name. A role with no recorded data shows a dash.

| Role | Data types (first present) | Fallback |
|---|---|---|
| Altitude | `BAROMETER_DATA` "Altitude", `ASCENT_STATE`/`DESCENT_STATE` "Altitude" | GPS height above the pad |
| Velocity | `VEHICLE_SPEED`, `ASCENT_STATE`/`DESCENT_STATE` "Velocity" | speed along the track |
| Acceleration | `ACCEL_DATA`, `IMU_DATA` (magnitude of the first three) | — |
| Chamber pressure | `…CHAMBER_PRESSURE`, `DAQ_LOADCELL_CHAMBER` | — |
| Tank pressure | `…TANK_PRESSURE` | — |
| Thrust | `…THRUST…`, the largest `KG…` | — |
| Battery | `BATTERY_VOLTAGE` (FC, PB, RF first) | — |
| GPS fix | `GPS_STATUS` (module state), `GPS_SATELLITE_NUMBER` | — |
| Radio link | `…RSSI…`, `…LINK_QUALITY…`, `…SNR…` | — |

**Time.** Play/pause, the slider, the phase timeline (click a segment or an event
marker), step a frame (the next/previous recorded fix) or an event, jump to
ignition, burnout, apogee, drogue, main or landing, 0.1×–20× speed, optional slow
motion (0.25×) within 1.5 s of an event, and an A/B loop.

**Cameras.** Follow, chase (behind along the path), pad view, ground station (the
station positions from `/api/antenna/status` or a bundle's `derived/stations.json`;
press again for the next), free orbit and top-down (north up); keys 1–6. Under a
canopy the camera frames the chute too. Far views mark the vehicle with a locator
dot; the model keeps its true size.

**Predicted.** A plan bundle (`manifest.predicted` or `replay.predicted`) shows a
Predicted badge and its label on the scene and in the time readout. Plan overlays
(MapOverlay, the contract's format) are drawn dashed at 55 % with the landing
ellipse on the ground: a plan's own overlay, a recording bundle's
`derived/flight_plans.json`, or the five latest `/api/flight_plans` when connected.
`<gs-vehicle-viewer>` takes the same array in `data-overlays`.

**Test fire.** A `test_fire` replay keeps the motor on the stand with the rocket
apart; the stand plume burns while the recorded thrust (or chamber pressure) is
above 5 % of its maximum and scales with it.

**Performance.** The scene redraws only on change. The software renderer, four or
fewer cores, 2 GB or less memory, or a phone-width screen use the low budget:
pixel ratio 1, fewer segments, no plume glow or flicker.

Tests: `node tests/flight-replay.test.mjs` (timeline from rows, attitude from
velocity, plume scale, chute sizes, time controls, camera modes, overlays,
channel binding, predicted label); `node tests/flight-replay-browser.mjs` plays
the nominal 10k bundle in Chrome (WebGL and Canvas2D), with
`GS_FLIGHT_REPLAY_SCREENSHOTS=DIR` for screenshots. The fixture records no main
deployment (no Reefing row; its flight review has `main: null`), so the main
screenshot uses a copy with one Reefing row added.

## Audience recorded-flight scene

Stream Studio offers a **Recorded flight replay** panel. Select a saved recording,
then Show 3D replay. Playback, pause, seek, quarter-to-four-times speed and looping
are persisted by the server. Rewind pauses at the beginning. Stop restores the
ordinary program layout. Automatic saved-data growth does not invalidate an open scene/rundown draft and cannot be rolled back by saving that draft. Explicit replay controls use a separate authenticated control route. A competing operator's newer revision is preserved;
stale controls require a refresh rather than silently overwriting it.

Selection freezes the recording's row boundary. **Refresh saved snapshot** includes
newly saved rows while preserving the current absolute replay time. It never
reads live telemetry. **Follow newly saved data** optionally persists automatic refreshes, at most once every five seconds for all viewers together. These updates preserve playback position and pause; they never switch into a later flight in the same recording. Saved FC phases trim playback to the latest recorded ascent
through recovery plus five seconds when available. Recordings without flight
phases use their full saved range. Overlapping wall-clock epochs are rejected
rather than combining different flights.

The audience scene displays **RECORDED FLIGHT · REPLAY**, its saved timestamp,
and reference-ground qualification. The trajectory appears progressively up to
that timestamp, never revealing future saved path segments. The geometry is
allocated once per snapshot and clipped by draw range. Follow, path overview,
drag orbit, keyboard pan and touch pinch remain available while paused.

Audience requests use a narrow program media capability, not a login token.
Only the currently selected recording and frozen row boundary are readable;
window sizes remain bounded. Uploaded models receive a separate model-scoped
capability. A disconnected scene freezes and reports its status rather than
switching to current telemetry.

Additional regressions:

- `node tests/saved-replay-player.test.mjs` checks replay timing, source freshness,
  recording changes and bounded retries.
- `node tests/program-recorded-replay-browser.mjs` exercises the real audience
  page and bundled renderer at desktop, 390px and 320px widths.

Linux Chromium WebGL tests cover the actual scene, including a synthetic 9 km
apogee, visible ground and horizon, drag orbit, touch pinch, progressive trajectory,
pause and recording switches. These checks do not replace physical Raspberry Pi,
iOS or Android GPU testing. Ground is an Earth-radius reference, not terrain.

## Portable offline replay and export

In **Data Export**, explicitly select a saved recording first, load its preview,
then select channels and the time window for CSV/Excel/PDF. Valve CSV updates keep
the numeric value and include a readable channel/state label; unknowns remain
unknown. Names/state words come from the station layout at export, not a claim
about historical configuration.

**Export portable replay** saves all saved channels in that recording (up to
200,000 rows and 64 MiB; larger selections fail explicitly without truncation).
Choose standalone HTML to open the saved file directly without a server, app
login, external scripts or a connection. Choose portable JSON to import through
**3D replay → Import an offline replay** or into the standalone HTML player.
The file carries saved receive/source timestamps, decoded values, schema,
export-time vehicle configuration, layout/units and explicit valve mappings.
It does not replace historical values with live data or recalibration.

The portable player is an illustrative flight-vehicle or test-fire motor-stand
schematic, separate from the station's 3D model renderer. Valve labels update at
each saved sample; missing/stale values remain unknown. Tank progress is shown
only with a configured saved binding and valid min/max. No synthetic pressure,
fill or GPS values are generated. The file stores export-time configuration;
it cannot reconstruct configuration that was never recorded. External/custom
3D model assets are not downloaded into the portable file. No replay control
sends hardware commands.

Tests: `node tests/offline-replay.test.mjs`; real offline Chromium test
`tests/offline-replay-browser.mjs` consumes the actual Rust-produced HTML via
`GS_REPLAY_HTML_PROOF`. It tests offline file import, named valve changes,
configuration/units roundtrip, null data and flight/stand distinction.
