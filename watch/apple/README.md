# GS26 Apple Watch companion

watchOS 10+ companion for the GS26 phone app. It shows the configured data
pages, an auto-cycle page, a recovery compass over a cached station map
(tiles sent by the phone with `transferFile`, the Digital Crown zooms) and up
to two opt-in action buttons per page, and lets you edit the watch settings. Data comes from the
phone, or from the station directly (URLSession, scoped watch token in the
keychain) while the phone app is suspended. Protocol and behaviour:
`docs/watch-companion.md`.

```
watch/apple/
├── project.yml          XcodeGen spec (watchOS app target + scheme)
├── GS26Watch/           SwiftUI app sources + Info.plist
└── WatchCore/           Swift package: wire models, geodesy, settings,
                         compass/link/clock logic, direct-link route and
                         action logic, XCTests (Foundation only)
```

## Crew voice

The Voice page (and the "Push to talk" complication / Smart Stack widget,
target `GS26WatchWidgets`, which opens `gs26://voice`) joins the station's
crew voice channel; protocol in `docs/watch-companion.md` "Voice".

* `GS26Watch/VoiceSession.swift`: a CallKit outgoing call ("GS crew voice")
  keeps audio, the microphone and the socket alive with the wrist down
  (watchOS 9+; `UIBackgroundModes` `audio` + `voip`). Transport is
  `URLSessionWebSocketTask` to `/api/voice/ws` with the watch token, or the
  phone relay when the station cannot be reached directly. Opus through
  `AVAudioConverter` (16 kHz mono, 20 ms packets), PCM16 if the watch has no
  Opus encoder. Playback mixes speakers with WatchCore `VoiceMixer`.
* `GS26Watch/VoicePage.swift`: push-to-talk button (hold or tap-to-toggle,
  from the phone's watch settings), talkers, Join/Leave, Digital Crown
  volume.
* Logic shared with Wear OS (state machine, backoff, framing, mixer) is in
  `WatchCore/Sources/WatchCore/Voice.swift`, tested against the `voice`
  vectors (`swift test`).

Signing: the widget extension has its own bundle id
(`com.UBSEDS.GS26.watchkitapp.widgets`). It is optional: `build.py` keeps it
on a device only when a profile for that id exists and otherwise installs
the watch app without it (see `docs/watch-companion.md`, "Optional: the
push-to-talk complication"); simulators always include it.
The `voip` background mode is reviewed by App Store review; voice here is a
real two-way call, which is what it requires. Things to check on a device:
CallKit on the watch, whether watchOS offers the Opus encoder at 16 kHz in
`AVAudioConverter` (otherwise it falls back to PCM16, 256 kbit/s), and which
route (watch speaker vs. AirPods) the system picks: watchOS routes calls
itself, so the "speaker"/"bluetooth" route setting is best effort.

## Build (macOS)

1. Install Xcode 15 or newer (16 recommended) from the App Store, open it
   once and install the watchOS platform when asked
   (Xcode › Settings › Platforms).
2. Install XcodeGen: `brew install xcodegen`
3. Generate the project:
   ```sh
   cd watch/apple
   xcodegen generate
   open GS26Watch.xcodeproj
   ```
4. Select the `GS26Watch` target › Signing & Capabilities › choose your
   Team (or set `DEVELOPMENT_TEAM` in `project.yml` and regenerate). It must
   be the same team that signs the iOS app.
5. Pick the `GS26Watch` scheme and your paired Apple Watch (or a watchOS
   simulator) as the destination, then Run. A real watch needs Developer
   Mode enabled on both the iPhone and the watch.

The app icon is `GS26Watch/Assets.xcassets/AppIcon.appiconset`: one
1024x1024 opaque PNG (watchOS 9+ single-size icon) made from the iOS icon by
`python3 scripts/generate_watch_icons.py`, which also writes the Wear OS
launcher foregrounds. Rerun it after changing `assets/icon_1024x1024.png`.

## Tests

The core logic is a plain Swift package and runs on macOS or Linux:

```sh
swift test --package-path watch/apple/WatchCore
```

The tests read `watch/test-vectors.json` (the same vectors as the Rust test
`shared_test_vectors_match` and the Wear OS tests) and also cover near-pole
and short distances, staleness, link loss, settings editing, decoding the
documented example snapshot and unknown page kinds. In Xcode, open
`WatchCore/Package.swift` directly to run them with ⌘U.

## Companion relationship

* The iOS app's bundle id is `com.UBSEDS.GS26`. The watch app's
  `Info.plist` sets `WKCompanionAppBundleIdentifier` to exactly that, and its
  own bundle id is `com.UBSEDS.GS26.watchkitapp`. Both apps must be signed by
  the same team or WatchConnectivity will not pair them.
* **Development:** `python3 build.py watchos_deploy` dev-signs the phone
  app with the watch app embedded, installs it on the iPhone and the watch
  app on the paired watch (`watchos_sim_deploy` for simulators; see
  `docs/watch-companion.md`, "Deploy to a watch for testing"). Or install
  the phone app with `python3 build.py ios_deploy` and the watch app from
  Xcode directly onto the paired watch (step 5 above). If the watch keeps showing
  "Open the GS app on your phone" while the phone app is open, check that
  both are signed by the same team, and look at Settings › Maintenance ›
  "Watch link" on the phone: the bits must include `activated`, `paired` and
  `app installed` (`WCSession.isWatchAppInstalled` is evaluated by the
  phone).
* **Builds with the phone app:** `python3 build.py --target watchos`
  (device, `arm64_32` + `arm64`), `--target watchos_sim` (Apple silicon
  simulator) or `--target watchos_sim_intel` build the iOS app, generate this
  project with XcodeGen, build the watch app and embed it as
  `GS26.app/Watch/GS26Watch.app`. `--dry-run` prints the commands.
* **Distribution:** a signed `watchos` build signs the embedded watch app
  first, with the profile in `IOS_WATCH_DISTRIBUTION_MOBILEPROVISION` (bundle
  id `com.UBSEDS.GS26.watchkitapp`, same team), then the iOS app.
  `--no-sign` leaves both unsigned. `--final` does not cover the watch yet.
* The phone side (`assets/WatchBridge.m`) needs `WatchConnectivity.framework`
  linked; `build.rs` already does that for iOS builds.

## What the watch sends

Only three messages, as JSON strings under the key `gs26_watch`:

| Message | When | Transport |
| --- | --- | --- |
| `{"v":1,"type":"hello"}` | session activated, phone became reachable | `sendMessage` (dropped if unreachable) |
| `{"v":1,"type":"activity","foreground":…,"page":…}` | scene phase change, page change, every 15 s while active | `sendMessage` (dropped if unreachable) |
| `{"v":1,"type":"settings","settings":{…}}` | each settings edit | `sendMessage` when reachable, otherwise `transferUserInfo` (older queued settings are cancelled) |

`sendMessage` is called without a reply handler, so the phone delegate must
implement `session:didReceiveMessage:` (and `session:didReceiveUserInfo:` for
queued settings). The watch receives snapshots under the key `gs26` from both
`updateApplicationContext` and `sendMessage`, stores the last one with its
receive time, and keeps showing it (with "Phone link lost · last data …")
when updates stop for `link_timeout_s`.

## Battery

* Location and heading run only while the compass page is visible and the
  app is active; they stop when you swipe away or lower your wrist.
* Map tiles arrive in the background as queued file transfers and stay in
  Application Support/map_tiles (capped at the "Map background" size cap,
  1–5 MB); the watch only fetches tiles itself in direct mode while the
  compass is on screen.
* `power = recovery`: `kCLLocationAccuracyBest`, 2 m distance filter.
  Otherwise `kCLLocationAccuracyNearestTenMeters`, 5 m filter.
  Heading filter 2°.
* Clocks and ages tick locally once per second; the phone sets the snapshot
  rate from the watch's `activity` messages.
