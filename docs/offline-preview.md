# Disconnected native preview

On the native app’s connection page, choose **Explore offline**. It is also
available after a connection failure. **Exit preview** returns to connection
setup; connecting to a real station still uses the normal session and permission
checks.

The persistent preview header labels every page as example data with controls
disabled. A bundled sample layout covers Dashboard, Mission, Data, Network, Map,
Checklists, Voice Chat, Stream Control and Settings. Real stations can configure
different pages and permissions. Preview supports sidebar/collapsed-sidebar and
page-picker navigation, flight/test-fire examples, example phase and sensor
selection, and a practice checklist.

Preview is a separate component. It does not mount the live dashboard, start
HTTP/WebSocket station clients, read live telemetry, request map tiles or
location/microphone access, or open cameras. Operational and media buttons are
disabled. Its local training choices and checklist checks are discarded on exit;
they do not change saved station addresses, login credentials, dashboard layouts,
calibration, qualification or shared checklists. It is an orientation aid, not
simulator or procedure qualification.

The shipped preview route is native-only. Web users retain the station’s normal
UI and authentication. The `offline_preview` example mounts the exact same
component for browser acceptance testing without a backend.

Validation:

```sh
cargo test --bin groundstation_frontend
cargo check --bin groundstation_frontend --target wasm32-unknown-unknown
cargo build --release --example offline_preview --target wasm32-unknown-unknown
```

After generating a web bootstrap with the wasm-bindgen CLI version matching
Cargo.lock and serving it on loopback, run
`node tests/offline-preview-browser.mjs`. Set `GS_PREVIEW_URL` for another test
port and `PLAYWRIGHT_MODULE` / `CHROME_BINARY` for local browser tooling.
`GS_UI_ARTIFACTS` optionally saves the mobile and desktop screenshots. Use a
release fixture for the no-network assertion: debug Dioxus builds poll their dev
server independently of the preview.

The browser check covers local navigation, practice-check reset, mode-sensitive
examples, 320/390/844/1280-pixel layouts, sidebar spacing, disabled operational
buttons, zero application network traffic and unchanged browser storage. Native
compilation includes the Connect and failed-connection route integration. Real
phone/native-window input and accessibility remain device checks.
