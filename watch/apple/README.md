# GS26 Apple Watch companion

Read-only watchOS 10+ companion for the GS26 phone app. It shows the phone's
data pages, an auto-cycle page and a recovery compass, and lets you edit the
watch settings. Protocol and behaviour: `docs/watch-companion.md`.

```
watch/apple/
├── project.yml          XcodeGen spec (watchOS app target + scheme)
├── GS26Watch/           SwiftUI app sources + Info.plist
└── WatchCore/           Swift package: wire models, geodesy, settings,
                         compass/link/clock logic, XCTests (Foundation only)
```

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

The project has no asset catalog; add an `AppIcon` before any TestFlight or
App Store upload.

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
* **Development:** install the phone app with the repo's usual
  `python3 build.py ios`, then install the watch app from Xcode directly
  onto the paired watch (step 5 above). If the watch keeps showing
  "Open the GS app on your iPhone" while the phone app is open, check that
  both are signed by the same team and that the phone app was reinstalled
  after the watch app (`WCSession.isWatchAppInstalled` is evaluated by the
  phone).
* **Distribution:** the watch app must be embedded in the iOS app bundle as
  `GS26.app/Watch/GS26Watch.app` and signed (inner app first) before the iOS
  app is signed and uploaded. `build.py` does not automate this yet.
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
* `power = recovery`: `kCLLocationAccuracyBest`, 2 m distance filter.
  Otherwise `kCLLocationAccuracyNearestTenMeters`, 5 m filter.
  Heading filter 2°.
* Clocks and ages tick locally once per second; the phone sets the snapshot
  rate from the watch's `activity` messages.
