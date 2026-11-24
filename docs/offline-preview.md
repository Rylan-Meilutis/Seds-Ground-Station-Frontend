# Offline workspace: demo mode and replay files

With no station connected, the app opens an offline workspace at `/preview`:

- native: **Explore offline** on the connection page or after a connection
  failure;
- web: **Explore offline** on the connection-failure card, or open
  `/preview` directly.

**Exit** returns to connection setup. Connecting to a real station still uses
the normal session and permission checks.

## Demo mode

The workspace starts in demo mode. Its banner reads "Demo mode — no station
connected". The demo is a looping 380 s scenario:

- fill;
- arm;
- a 10 s countdown;
- the GroundStation26 IGT `nominal_10k` mock flight to about 10,000 ft;
- landing.

Every page shows data from it:

- the Dashboard, built from the demo layout's `state_tab` for the current
  state;
- Mission steps and actions;
- Data charts for every channel;
- live Diagrams;
- Network, with seven boards and four example warnings;
- the Map, as a GPS track on a grid;
- Replay & summary, with flight figures and the 3D player;
- Calibration points and fit;
- Recordings.

Use Play/Pause, the position slider and 1–10× speed to move through it.

Commands are acknowledged in the workspace ("Nothing was sent; no station is
connected"). Nothing leaves the device.

## Replay files

**Open replay file** loads a replay bundle (`*.gsreplay`, from the Recordings
panel's **Download replay bundle**) or a portable `*.seds-replay.json` (from
3D replay), as does dropping the file on the window. A bundle also brings the
station's calibration, diagrams, summaries, optional map tiles and the
recording's CSV. **Recordings → Export CSV** saves that CSV exactly as the
station exports it; **Export layout.json** saves the layout. Format:
[replay-bundle.md](replay-bundle.md). Opening one needs no station, no sign-in and no cache. All pages read
the file, in flight, test-fire, ground-test (IGT) and HITL recordings. The
banner names the file and the mode.

Commands answer that a replay is read-only. **Back to demo** returns to the
scenario.

The file format, and what each page needs from it, are in
[backend-contract.md, section 10](backend-contract.md#10-offline-replay-files).

## Isolation

The workspace does not mount the live dashboard and does not start station
HTTP/WebSocket clients. It does not request map tiles or location or
microphone access, and does not open cameras.

Its checklist ticks and choices are discarded on exit. They do not change:

- saved station addresses;
- credentials;
- layouts;
- calibration;
- qualification;
- shared checklists.

The workspace is an orientation and review aid. It is not a simulator or
procedure qualification.

The demo's data, layout and session are created when the route mounts and
dropped when it unmounts. A connected station pays nothing for them (see
[backend-contract.md, section 11](backend-contract.md#11-demo-mode-a-local-reference-station)).

## Validation

```sh
cargo test --bin groundstation_frontend -- local_station offline_preview
cargo check --bin groundstation_frontend --target wasm32-unknown-unknown
dx build --platform web --release
```

Serve the release bundle on loopback, for example
`node scripts/serve-static.mjs target/dx/groundstation_frontend/release/web/public 19096`,
then run `node tests/offline-preview-browser.mjs`.

- `GS_PREVIEW_URL` selects another port.
- `PLAYWRIGHT_MODULE` and `CHROME_BINARY` select local browser tooling.
- `GS_UI_ARTIFACTS` saves screenshots at four widths.

The browser test checks:

- the demo pages and the moving clock;
- the local command acknowledgement;
- opening the backend's replay-export fixture, including the 3D player;
- the read-only command notice;
- widths of 320, 390, 844 and 1280 px;
- no application network traffic;
- unchanged browser storage.
