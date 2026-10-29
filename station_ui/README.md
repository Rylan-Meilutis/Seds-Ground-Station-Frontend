# Station browser assets

Voice, stream control/program pages, browser audio, rendering modules and GLB
models are frontend-owned. Original vendor licenses remain beside their assets.
`provenance.json` records the byte-preserving migration from GroundStation.

`manifest.json` exports static bodies to the backend compile-time bridge.
Backend Rust retains routing, authentication, configuration and media services;
runtime URLs remain stable as browser assets evolve. `station_ui` stays outside the
native client's `assets/**/*` resource glob to avoid packaging server-only
scripts and models in every client.

GroundStation build tools select one checkout for WebUI and backend exports;
`--frontend-dev` selects dev for both. Reusing the WebUI verifies its saved
station-asset digest. Direct Cargo builds use its `ui` submodule, or an explicit
`GS_FRONTEND_SOURCE_DIR` checkout.

Browser and renderer fixtures are in `tests/station-*`. They use synthetic
media/radios and no physical outputs. The crew-voice fixtures use a native Rust
test server from GroundStation; see its `docs/backend/crew-voice.md`. Set
`GS_BROWSER_TEST_ORIGIN` to the printed loopback address when using an ephemeral
port. Chromium's fake audio-output stream keeps the actual audio graph running
on a headless host without a sound device; PCM relay/worklet assertions remain
active.

Regenerate replay GLBs with `node scripts/build_gse_models.mjs` from the frontend
checkout, or pass `--output-dir PATH` for a separate build directory. Verify them
with `node tests/station-model-structure.cjs` and the software-renderer test.
The legacy CDP broadcast/animation fixtures are also frontend-owned; their
loopback-only server setup remains documented by GroundStation. The obsolete
live-3D streamer fixture is superseded by `station-program_browser.mjs`, which
requires live2D artwork and rejects live3D asset requests.

`station-player-browser.mjs` checks standalone WHEP camera audio/video negotiation,
combined incoming tracks, session cleanup and restored-page reconnection using
synthetic WebRTC tracks. `station-program_browser.mjs` plays real synthetic H.264
and checks that restored pages restart video and crew audio while retaining an
explicit mute choice. Page lifecycle callbacks are dispatched by these fixtures;
physical iOS background behavior still requires testing on a device.

`concurrent-broadcast-browser.mjs` exercises the real loopback MediaMTX/backend
fixture with original or remotely encoded feeds. It checks decoded camera PCM,
continuous frames for one/two viewers, restored clean OBS audio and saved geometry;
encoded runs reject silent fallback to original camera URLs. Set
`GS_MEDIA_TEST_ORIGIN` to the isolated backend fixture.
