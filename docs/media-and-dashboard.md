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
the dashboard, or stopping sharing releases media resources. Unmounting the
dashboard also releases its screen wake-lock owners; a hidden camera tab keeps
its capture lock until sharing stops. Operating systems
may suspend capture when the app itself is backgrounded.

The voice channel holds up to 32 people by default, and the station forwards
at most four talkers at once (both set on the Ground Station; see its
`docs/backend/crew-voice.md`, "Capacity and talk floor"). The count beside
**In the channel** shows the station's cap. While you talk the page shows
**Transmitting**; when four people are already on air it shows **Waiting for a
free channel · 4 of 4 talking**, sends no audio, and goes on air by itself
when a slot frees. The roster lists people as *On air*, *Waiting to talk* or
*Listening*. An administrator's request can take the slot of the person on air
longest, and someone on air who stays quiet for 1.5 s gives the slot to the
next person waiting; either way the page says why.

**Mission Live** displays only the broadcast stream. Logged-in users with command permission can open **Checklist** beside the tab selector from any tab. Checkmarks are saved per user and station and do not bypass interlocks.

**Stream Manager** is available to users with stream-management access
(`stream_master`, `stream_admin`, or the existing `StreamControl` grant, subject
to the backend's stream-viewer restriction). It is a visual vision mixer:
Preview and Program monitors, scene thumbnails, source tiles (cameras, live
diagrams and other telemetry views, overlays, graphics) dragged into a scene's
slots, a layout editor, Cut/Fade and hotkeys; see
[broadcast studio](broadcast-studio.md#stream-manager). **Broadcast settings**
holds delay, audience crew audio, label, rundown and replay controls. Stream
admins can assign stream-master roles there. Dashboard never renders the model.
The backend enforces every change.

In **Cameras & Recordings**, permitted users choose a camera and select **Start
sharing camera**. The browser asks for camera permission; video becomes a source
that the manager can select for the broadcast. Video uses H.264 for the delayed broadcast and MP4 recorder. Camera audio is not captured;
crew audio uses the separate voice controls. Select **Stop sharing** to release
the device. Browser capture requires HTTPS or localhost. Native builds include
Apple usage descriptions and Android camera/audio permissions; device-specific
WebView support still applies.

## Crew voice in the native apps

The apps show the station's `/radio` page in an iframe, so voice runs on each
platform's web view: WKWebView (iOS, macOS), WebKitGTK (Linux), the Android
WebView and WebView2 (Windows). The dashboard's `gs26-session` message tells
the page which app it is in (`app.platform`, and on iOS the microphone
permission), so refusals name the right setting and the page never says
"this browser" inside an app.

**Noise suppression.** WebKit has no `noiseSuppression` microphone
constraint (Safari, the iOS and macOS apps and WebKitGTK all report it
unsupported; checked in WebKitGTK), so the page used to grey the control out
with "unavailable in this browser". Now the page uses the platform's own
suppression where the microphone reports it running (Chromium: browsers, the
Android WebView, WebView2) and otherwise the built-in suppressor in
`station_ui/scripts/voice-worklet.js` (spectral, 16 kHz, about 13 dB less
steady noise, 16 ms delay). The status says what runs: **Noise suppression:
on (system)**, **on (built-in)**, **off**, or **on (system; this device keeps
it on)** when a device refuses to turn its own off. The built-in one is never
stacked on a running system one.

**iOS audio.** WebKit owns the app's audio session while the microphone is
open and interrupts the page's AudioContext whenever the session changes
(the microphone opening, a call or Siri, some route changes, background). An
interrupted context stops the audio processor: the call looks connected but
nothing is sent or heard, and the old page never restarted it. The page now
resumes the context on every interruption and on the next touch, focus or
return to the page, and says "Voice audio was paused by the system. Tap this
page to resume." when WebKit wants a gesture. Separately, play-and-record
plays through the receiver (earpiece) when no headset is connected; the
app's `assets/VoiceAudioShim.m` moves output to the speaker in exactly that
case (`src/native_voice_audio.rs`, `speaker_override_wanted`) and never
changes the category, mode or a chosen headset, Bluetooth or AirPlay route.
Every route change, interruption, media-services reset and return to the
foreground is logged as `[voice-audio] ...` (category, mode, ports, sample
rate, microphone permission), and the page logs `[voice] audio context ...`
to the Web Inspector console.

### iOS device checklist

Use a release build on a phone (not the simulator), a station over HTTPS
(self-signed with TLS verification skipped is fine), and a second participant
on a laptop. Afterwards export the app's log from Settings (the log list
under maintenance) and check the `[voice-audio]` lines; Safari's Web
Inspector on a Mac shows the page's `[voice]` console lines live in a
debug build.

1. Fresh install: open Voice Chat, Join voice. iOS asks for the microphone
   once. The log shows `[voice-audio] install` then a `route
   (CategoryChange)` line with `category PlayAndRecord`.
2. No headset: the laptop talks; the phone plays it from the speaker at
   normal volume, not the earpiece. The log shows `speaker-override`.
3. Hold to talk on the phone: the laptop hears it; the input meter moves.
4. Ring/silent switch on silent: incoming voice still plays.
5. Wired or Bluetooth headset: output stays on the headset (no override line).
   Unplug it: output returns to the speaker.
6. Receive a phone call or invoke Siri during voice, then return: audio
   resumes by itself, or after one tap with the "paused by the system"
   notice. The log shows `interruption (began)` and `(ended)`.
7. Lock the phone for 10 s and unlock: voice resumes (one tap at most) or
   the page says it disconnected; it never stays silent while showing
   connected.
8. Deny the microphone in Settings → Privacy & Security → Microphone and join:
   the page says "Microphone access is off for this app" and names that
   setting. Allow it again and join.
9. Noise suppression: the status reads **Noise suppression: on (built-in)**
   (never "this browser"). With a fan near the phone, toggle it off and on
   while talking; the laptop hears the fan come and go. **off** shows when off.
10. macOS app: steps 3, 8 (System Settings wording) and 9.
11. Android app: the status reads **on (system)** on most phones; on a phone
    whose microphone ignores the setting it reads **on (built-in)**.
12. Linux and Windows apps: step 9; Windows reads **on (system)**.

`node tests/station-voice_native.test.cjs` covers the page logic above
(noise choice and labels per platform, refusal text per platform, the
resume after an interruption); `node tests/station-voice_audio.cjs` covers
the built-in suppressor; `cargo test native_voice_audio` covers the route
decision and the report parsing; `tests/test_native_media_pages.py` checks
the shim is built for iOS only and never sets the category.

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

After building the web bundle, run
`node tests/media-surface-wake-lock-browser.mjs` to check dashboard unmount,
hidden capture, remount and independent wake-lock ownership with synthetic locks.

Fill percentage in Telemetry follows the saved fill source from Actions. Its readout and chart appear only under the selected load cell in both Loadcell and DAQ. The percentage comes from backend telemetry, including calibration and source-selection rules.

Dashboard measurement extrema reuse the configured Data view's `show_min_max`: summary-item overrides subtab, subtab overrides tab, and an unspecified value defaults to true. Policies match the actual source, channel and sender scope; a sender-specific view does not configure a wildcard card. Conflicting matching views or an unconfigured source keep the existing dashboard fallback (extrema for trends only). Number, Gauge and Bar cards can show extrema without changing their display type. Binary/state cards and compact pins do not gain extrema. This adds no saved card setting. Live extrema use the existing Data history; launch extrema use only the source-timed admitted launch capture.

Live Data-history extrema summarize retained representative samples, not every raw or batched input value: the existing Data store retains one representative per source/sender receipt bucket. This presentation change does not alter that ingestion policy.

These shared visibility policies also apply to Trend extrema: a matching explicit `show_min_max:false` or boolean metadata suppresses its min/max text in both live and launch views. The Trend card, measurement and plot remain visible. This intentionally replaces the previous unconditional Trend extrema display; unconfigured or conflicting metadata retains the prior Trend-only fallback.
