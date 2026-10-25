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
