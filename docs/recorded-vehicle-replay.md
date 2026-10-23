# Recorded vehicle replay

Open **3D replay**, select a recording, then use Play, Rewind, the timeline and
0.25×–4× speed. Selecting an active recording freezes its current row boundary;
Reload explicitly selects a new snapshot. Missing recorded channels remain
unavailable. The live dashboard and audience program use cameras and 2D artwork.

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
