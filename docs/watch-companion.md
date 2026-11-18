# Watch companion (Apple Watch and Wear OS)

Read-only companion apps that show live data pages, an auto-cycle page and a
recovery compass. Watches never talk to a station and never send commands.

```
Apple Watch ──WatchConnectivity──┐
                                 ├─ phone app ──HTTPS/WS (existing session)── GroundStation or RRLS
Wear OS watch ──Wear Data Layer──┘
```

* **Topology is fixed.** The watch only talks to the phone app. The phone uses
  whichever station it is connected to (GroundStation or RRLS) and follows it
  when it switches, because the publisher lives inside the dashboard that is
  remounted on every connection change.
* **Mode follows the phone.** `live` (real station), `demo` (the phone's
  offline preview; every watch page carries a DEMO banner) or `no_station`
  (phone app open on the connection screen). When the phone app stops sending,
  the watch keeps the last snapshot, marks it with the age of the link loss and
  keeps the compass running from the last known target fixes.
* **Read-only.** The watch may send three message types (`hello`,
  `activity`, `settings`). The phone ignores anything else; there is no path
  from a watch message to a station request other than saving the user's own
  watch settings to their account.

## Phone side

| Part | File |
| --- | --- |
| Registry of items/pages/targets | `src/telemetry_dashboard/watch_companion/registry.rs` |
| Settings model + validation | `.../watch_companion/settings.rs` |
| Wire format | `.../watch_companion/protocol.rs` |
| Geodesy (shared test vectors) | `.../watch_companion/geodesy.rs`, `watch/test-vectors.json` |
| Snapshot builder + publisher | `.../watch_companion/snapshot.rs`, `publisher.rs` |
| Account sync | `.../watch_companion/account_sync.rs` |
| Settings → Watch section | `.../watch_companion/settings_section.rs` |
| iOS bridge (WatchConnectivity) | `assets/WatchBridge.m` (compiled by `build.rs` for iOS) |
| Android bridge (Data Layer) | `platform/android/kotlin/com/ubseds/gs26/WatchBridge.kt` |

The publisher samples the dashboard's existing state (flight state, launch
clock, latest telemetry rows, board status, WebSocket state) and polls
`GET /api/antenna/status` (already used by antenna aiming) for the rocket,
GroundStation and RRLS fixes. That endpoint labels saved positions with
`manual: true`; the watch shows them as "saved position". No new telemetry
endpoint is needed.

### Rates (battery)

`settings.power` picks the cadence. While the watch app is foreground (it sends
`activity` every 15 s) the phone sends a snapshot every 1 s (`recovery`), 2 s
(`balanced`) or 5 s (`saver`) and polls antenna status at the same rate when a
compass page is enabled. When the watch is backgrounded or unreachable the
phone only refreshes the latest-state context every 10/30/60 s and does not
poll antenna status faster than 10 s. The watch ticks the T-clock and fix ages
locally between snapshots.

### Who gets watch settings

Settings → Watch, account sync of the watch document and applying watch-side
edits are only for a **signed-in user of the native iOS or Android app whose
watch has the companion app** (`watch_companion::settings_eligible`):

* signed in: a named, non-anonymous session;
* native phone build: iOS or Android (`COMPANION_PLATFORM`); never the
  web/wasm frontend or desktop builds;
* companion present: the bridge reports a paired watch with the GS app
  installed (iOS `WCSession.isPaired` and `isWatchAppInstalled`; Android a
  known node advertising `gs26_watch`, reachable or not).

Everyone else sees no Watch tab. The persisted local copy is kept but not
sent or applied: snapshots carry the default `WatchSettings`, settings
messages from the watch are answered with that default, and nothing is read
from or pushed to the account. The offline preview (no sign-in) behaves the
same. Becoming eligible triggers an immediate account sync.

### Settings sync

One local copy on the phone (`watch_settings_v1`) plus the account revision it
was last synced with. Edits come from Settings → Watch on the phone or from the
watch's settings screen and are stamped `updated_ms` (monotonic per document).

* Eligible (above): the account document is the source of truth. On sign-in and
  every 60 s the phone reads `GET /api/account/client-settings/watch`. If the
  local copy has an unsynced edit based on the current account revision it is
  pushed; otherwise the account copy is adopted. A push that loses a race
  (HTTP 409) re-reads and keeps whichever copy has the newer `updated_ms`.
* Station without the endpoint: local only (`account.sync = "local_only"`).
  Not eligible: defaults only, as described above.
* Phone → watch: settings travel in every snapshot. Watch → phone: a
  `settings` message; the phone applies it if newer and echoes the result.

Backend: `GET`/`POST /api/account/client-settings/{namespace}` (namespace
`watch` only), signed-in accounts only, stores the caller's own opaque
document (≤ 8 KiB) with a revision. It touches no telemetry, command or radio
state.

## Wire format v1

Transport:

| | Phone → watch | Watch → phone |
| --- | --- | --- |
| watchOS | `updateApplicationContext(["gs26": <snapshot json>])`; when reachable and the watch is foreground also `sendMessage(["gs26": json])` without reply handler | `sendMessage(["gs26_watch": json])` when reachable, else `transferUserInfo` (settings only) |
| Wear OS | DataItem `/gs26/snapshot`, DataMap keys `json` (String) and `seq` (Long); `setUrgent()` while the watch is foreground | `MessageClient` path `/gs26/watch` (UTF-8 json bytes) to nodes with capability `gs26_phone`; settings additionally as DataItem `/gs26/watch_settings` whose `json` key holds the same full message, so they sync after a disconnect |

Snapshot (phone → watch), abridged:

```json
{"v":1,"seq":42,"mode":"live",
 "station":{"name":"GroundStation","role":"primary","link":"connected","telemetry_age_ms":350},
 "account":{"signed_in":true,"sync":"synced"},
 "items":{"flight_state":{"label":"State","text":"In flight · Descent","unit":"","age_ms":0,"stale":false,"level":"normal"},
          "t_clock":{"label":"T-clock","text":"T+ 01:23","unit":"","age_ms":0,"stale":false,"level":"normal","clock":{"kind":"tplus","ms":83000}},
          "altitude":{"label":"Altitude","text":"1234","value":1234.0,"unit":"m","age_ms":200,"stale":false,"level":"normal"}},
 "pages":[{"id":"flight","kind":"items","title":"Flight","items":["flight_state","t_clock","altitude","velocity"]},
          {"id":"cycle","kind":"cycle","title":"Cycle","items":[]},
          {"id":"compass","kind":"compass","title":"Recovery","items":[]}],
 "targets":[{"id":"rocket","label":"Rocket","lat":32.99,"lon":-106.97,"alt_m":1401.0,"age_ms":3000,"stale":false,"source":"Rocket GPS","fallback":false},
            {"id":"ground_station","label":"Ground station","lat":32.98,"lon":-106.98,"alt_m":null,"age_ms":0,"stale":false,"source":"Saved GroundStation (manual)","fallback":true}],
 "settings":{ "...": "WatchSettings, below" },
 "catalog":{"items":[{"id":"altitude","label":"Altitude","group":"Trajectory"}],"pages":[...all pages...],"targets":[{"id":"rocket","label":"Rocket"}]},
 "link_timeout_s":15}
```

* `items` holds every registry item; a missing value has `text:"--"`,
  `level:"unknown"`. `level` is `normal|warn|alert|unknown`.
* `clock.kind` is `idle|tminus|tplus`. The watch shows `T- mm:ss` counting
  down from `ms` (clamped at 0) or `T+ mm:ss` counting up, from the receive
  instant.
* Ages (`age_ms`) are at send time; the watch adds time since it received the
  snapshot. A target is stale when `stale` is true or its current age exceeds
  `settings.compass.stale_after_s`.
* `demo_offset:{north_m,east_m}` appears only in demo mode: the watch places
  that target at its own position plus the offset so the compass can be
  practised anywhere.
* Unknown page `kind`: show "Update the watch app". Unknown fields: ignore.

`WatchSettings`:

```json
{"version":1,"updated_ms":1730000000000,"updated_by":"phone",
 "pages":[{"id":"flight","enabled":true},{"id":"propulsion","enabled":true},{"id":"trajectory","enabled":true},
          {"id":"health","enabled":true},{"id":"cycle","enabled":true},{"id":"compass","enabled":true}],
 "cycle":{"items":["flight_state","t_clock","altitude","velocity","tank_pressure"],"interval_s":5,"autostart":true},
 "compass":{"north":"true","stale_after_s":30,"targets":["rocket","ground_station","rrls"]},
 "units":"metric","power":"balanced"}
```

Limits: `interval_s` 2–60, `stale_after_s` 5–600, ≤ 12 cycle items, document
≤ 8 KiB. `updated_by` is `phone|watch|account`. The phone sanitizes every
document against its registry, so a watch may send any edit and the phone's
echo is authoritative.

Watch → phone:

```json
{"v":1,"type":"hello"}
{"v":1,"type":"activity","foreground":true,"page":"compass"}
{"v":1,"type":"settings","settings":{ ...WatchSettings with updated_by "watch"... }}
```

## Compass

* Distance: haversine on R = 6 371 008.8 m. Bearing: initial great-circle
  bearing, true north. No bearing within 1 m or from a pole.
* Arrow rotation = bearing(true) − heading(true). The `north` setting only
  changes the printed bearing/heading numbers (magnetic = true − declination).
* watchOS: `CLLocationManager` heading; uses `trueHeading` when valid, else
  `magneticHeading` (declination = trueHeading − magneticHeading when both are
  valid). `headingAccuracy < 0` or > 25° shows a figure-8 calibration prompt.
* Wear OS: rotation-vector sensor azimuth (magnetic) plus
  `GeomagneticField` declination; sensor accuracy `UNRELIABLE`/`LOW` shows the
  calibration prompt.
* Expected accuracy: watch GPS ±5–15 m and compass ±5–15° after calibration.
  Within ~30 m of a target the arrow is not reliable; the watch says "Within
  N m — look around" instead of trusting the arrow.
* `watch/test-vectors.json` is checked by the Rust, Swift and Kotlin tests.

## Building

`build.py` targets `watchos`, `watchos_sim`, `watchos_sim_intel` (iOS app
with the watch app embedded; macOS) and `wearos`, `wearos_sim` (Wear OS APK);
see the README's "Watch companion targets". Manual builds: `watch/apple/README.md`
(macOS + Xcode) and `watch/wear/README.md` (Gradle). Phone-side Rust is covered
by the normal test commands.

## Deploy to a watch for testing

`build.py` builds, signs, installs and launches the companions for testing.
Each action builds first; add `existing` to reuse the last build and `debug`
for a debug build.

| Command | Installs on |
| --- | --- |
| `python3 build.py watchos_deploy` | connected iPhone (phone app) + its paired Apple Watch |
| `python3 build.py watchos_sim_deploy` | an iPhone + Apple Watch simulator pair |
| `python3 build.py wearos_deploy` | a Wear OS watch connected over adb |
| `python3 build.py wearos_sim_deploy` | a running Wear OS emulator |

### Apple Watch

Prerequisites (macOS):

* Xcode 15 or newer with the watchOS platform installed, selected with
  `sudo xcode-select -s /Applications/Xcode.app`; `brew install xcodegen ios-deploy`.
* Two **development** profiles from the same team with the same
  `Apple Development:` certificate in the login keychain:
  * iOS app `com.UBSEDS.GS26`: `Groundstation_dev.mobileprovision` in the
    frontend directory, `IOS_DEVELOPMENT_MOBILEPROVISION=...`, or an installed
    Xcode-managed profile;
  * watch app `com.UBSEDS.GS26.watchkitapp`:
    `Groundstation_watch_dev.mobileprovision`,
    `IOS_WATCH_DEVELOPMENT_MOBILEPROVISION=...`, or an installed Xcode-managed
    profile.

  Both must list the device UDIDs (iPhone and watch). Distribution profiles are
  refused.
* Developer Mode on the iPhone and on the watch (watch: Settings › Privacy &
  Security › Developer Mode; the switch appears after Xcode has seen the
  watch once in Window › Devices and Simulators).
* iPhone connected to the Mac (USB or the same Wi-Fi) and trusted; the watch
  paired with it, **unlocked and on its charger** next to it.

```sh
python3 build.py watchos_deploy                       # one watch visible to Xcode
python3 build.py watchos_deploy watch="Rylan's Apple Watch" phone=<iPhone UDID>
python3 build.py watchos_deploy watch=skip            # phone install only, then Watch app › Install
```

Steps: list the watch with `xcrun devicectl list devices` (fails early if none
is visible or Developer Mode is off; `watch=<name|UDID|identifier>` picks one
of several), check both profiles, build the iOS app and the watch app, check
the watch bundle id (`com.UBSEDS.GS26.watchkitapp`, companion
`com.UBSEDS.GS26`), sign the watch app and then the iOS app, install the
phone app with `ios-deploy`, install the watch app with
`xcrun devicectl device install app --device <watch> GS26.app/Watch/GS26Watch.app`
and launch it. If the direct watch install fails, the script prints the
fallback: Watch app on the iPhone › My Watch › Available Apps › Install (or
turn on General › Automatic App Install).

`watchos_sim_deploy` builds `watchos_sim` on Apple silicon or
`watchos_sim_intel` on Intel, picks a pair from `xcrun simctl list pairs`
(already booted first) or creates "GS26 iPhone" + "GS26 Apple Watch" with the
newest iOS and watchOS runtimes and pairs them, boots both, installs the phone
app and the watch app with `simctl install` and launches both.

### Wear OS

Prerequisites: Android SDK platform-tools (`adb`) found through
`ANDROID_HOME`/`ANDROID_SDK_ROOT` (or `~/Library/Android/sdk`,
`~/Android/Sdk`), JDK 17/21. The device is picked from `adb devices` by
`ro.build.characteristics` containing `watch`; `serial=<serial>` chooses one
of several. The APK is installed with the same fallbacks as `android_install`
(streamed, no-streaming, push + `pm install`) and then launched.

Physical watch over Wi-Fi debugging:

1. Watch: Settings › System › About › Versions, tap Build number 7 times.
2. Settings › Developer options: ADB debugging and Wireless debugging on (watch
   and computer on the same Wi-Fi).
3. Wireless debugging › Pair new device: `adb pair <ip>:<pairing-port>` and
   enter the code.
4. `adb connect <ip>:<port>` (the address on the Wireless debugging screen).
5. `python3 build.py wearos_deploy debug` (add `serial=<ip>:<port>` if several
   devices are connected).

Emulator: start a Wear OS AVD (Android Studio › Device Manager, or
`emulator -avd <name>`), then `python3 build.py wearos_sim_deploy debug`. If no
Wear emulator is running the script lists the Wear AVDs it found. To see live
data, pair the emulator with a phone (emulator or device) running the phone app
with Android Studio's Wear OS pairing assistant.

The Data Layer only connects apps signed with the same key: deploy the phone app
and the watch app both as debug or both as release. A release Wear APK needs
the phone's `ANDROID_KEYSTORE_PATH`/`ANDROID_KEY_ALIAS`/`ANDROID_KEYSTORE_PASSWORD`;
without them the deploy stops and asks for `debug`.
