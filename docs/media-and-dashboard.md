# Media tabs and personal dashboard

Use the boxed active-tab name to open the wrapping tab picker. **Customize** controls tab
visibility and order. The primary Dashboard remains available. **Dashboard**
lets each user add, remove and reorder telemetry cards, choose sections, sources,
board IDs, channel indices, units, precision, and number/bar/gauge/trend/state
formats. Add live-camera cards, choose a camera, and set normal, wide, or full-width sizes. Camera previews require operator or stream-management access; offline cameras show an explicit status rather than a model. Up to eight cards can be pinned to the status bar. Pins retain missing
and stale data indicators. Preferences are local to the user and Ground Station.

**Voice Chat** and **Cameras & Recordings** reuse the dashboard login. Their
embedded pages remain mounted after their first visit, so switching dashboard
tabs does not end voice or camera capture. Hiding voice releases push-to-talk;
open mic continues until muted or left. Logging out, changing sessions, closing
the dashboard, or stopping sharing releases media resources. Operating systems
may suspend capture when the app itself is backgrounded.

**Mission Live** displays only the broadcast stream. Logged-in users with command permission can open **Checklist** beside the tab selector from any tab. Checkmarks are saved per user and station and do not bypass interlocks.

**Stream Manager** is available to users with stream-management access
(`stream_master`, `stream_admin`, or the existing `StreamControl` grant, subject
to the backend's stream-viewer restriction). It exposes delay, audience crew
audio, label, layout, featured camera and feed visibility controls. Stream admins
can assign stream-master roles there. Choose **3D model** for a model-only program or **Cameras + 3D model** for a mixed grid. Otherwise the model appears only when there are no available cameras; Dashboard never renders the model. The backend enforces every change.

In **Cameras & Recordings**, permitted users choose a camera and select **Start
sharing camera**. The browser asks for camera permission; video becomes a source
that the manager can select for the broadcast. Video uses H.264 for the delayed broadcast and MP4 recorder. Camera audio is not captured;
crew audio uses the separate voice controls. Select **Stop sharing** to release
the device. Browser capture requires HTTPS or localhost. Native builds include
Apple usage descriptions and Android camera/audio permissions; device-specific
WebView support still applies.

## Backend coordination

Deploy the matching GroundStation26 backend changes with this frontend. Embedded
`/radio?embedded=1` and `/media?embedded=1` accept a `gs26-session` message only
from their parent window. The frontend uses the configured backend origin as
`postMessage`'s target origin; bearer tokens never enter iframe URLs or storage.
The message contains `token` and `visible`. Session changes clear the old media
resources; repeated messages with the same token retain them.

Camera publishing uses authenticated, same-origin endpoints:

- `POST /api/video/publish`: `Content-Type: application/sdp`; returns an SDP answer
  and a relative `Location: /api/video/publish/{id}/{session}`.
- `POST` to that location renews the owner's 45-second lease. The client renews
  every 10 seconds and the server checks permission on every renewal.
- `DELETE` to that location stops the owner's publisher. Expired leases are
  cleaned up every five seconds.

The server generates `browser-…` stream names, caps publishing at two per user
and 64 total, and proxies WHIP using private MediaMTX camera credentials. It never
returns relay credentials to clients or permits callers to overwrite physical
camera stream names. Live stream discovery and recording use the existing relay
configuration.

## Regression checks

`cargo test --offline --bin groundstation_frontend` covers tab order/visibility,
legacy saved layouts, pin limits and telemetry cards. Build the web UI with
`dx build --platform web`, then run `node tests/dashboard-customization.mjs` for
the dashboard interaction and desktop/mobile layout checks.

In GroundStation26, run `cargo test -p groundstation_backend media::publishing::tests`
and `node tests/embedded_media.mjs`. The browser media test uses fake devices and
a local WebRTC receiver. Set `PLAYWRIGHT_MODULE` and `CHROME_BINARY` if Playwright
and Chrome are installed outside their default locations. These tests do not
connect to hardware or start a production Ground Station.

Fill percentage in Telemetry follows the saved fill source from Actions. Its readout and chart appear only under the selected load cell in both Loadcell and DAQ. The percentage comes from backend telemetry, including calibration and source-selection rules.
