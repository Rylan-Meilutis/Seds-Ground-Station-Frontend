# Personnel proximity lock (app side)

The station's optional lock that refuses arming and pressurizing while people
are near the rocket. The station side (what is refused, stale policy,
override, RRLS, audit, API) is in GroundStation26 `docs/proximity-lock.md`.
This page covers what the app does on each device.

## Privacy model

- The device compares **its own location** with the **rocket's GPS fix** it
  already receives over telemetry (`GPS_DATA`), or, without a fix received in
  the last 2 minutes, the station's **configured launch site** (`pad` in the
  status). With neither it cannot tell.
- It sends the station only `participating`, `in_range` (true / false /
  null), an accuracy class (`high`/`medium`/`low`), which reference it used
  (`rocket`/`pad`), whether it reports in the background, why it does not
  participate (`user`/`permission_denied`/`unsupported`), a heartbeat counter
  and its clock. The report type (`proximity_lock::Report`) has no field that
  could carry a place, and the station refuses any extra field. A test
  serializes a report and checks that no coordinate, distance or bearing
  appears.
- Coordinates handed to the native layers (the reference point for the OS
  geofence, the device's fixes) stay on the device. The native code sends
  nothing; the Rust reporter sends the flag over the existing signed-in
  session.
- The opt-out and the device name are stored on the device only
  (`proximity_lock_prefs_v1`, per station and account); the device id is a
  random `prox-…` (`proximity_device_id_v1`).
- The recovery search (Map tab) is separate and is the only feature that
  sends a location track. It has its own explicit opt-in on each device, for
  one running search, with a visible "Sharing" indicator and "Stop sharing"
  (backend-contract.md, "Recovery search"). The proximity lock never uses
  it, and opting in to one never opts in to the other.

## Decision on the device

`src/telemetry_dashboard/proximity_lock.rs`:

- distance: spherical haversine (`watch_companion::geodesy::distance_m`, the
  watch compass's, checked against `watch/test-vectors.json`);
- **conservative accuracy**: in range whenever `distance − accuracy ≤ radius`;
  an unknown accuracy counts as 100 m;
- **hysteresis**: once in range, out only when `distance − accuracy > radius +
  exit margin` (both from the station's settings);
- a fix older than 5 minutes, or no reference, reports `in_range: null`
  (the station treats it as unknown).

The reporter (`proximity_lock_view::ProximityRuntime`, mounted once in the
dashboard) evaluates every second and at once on an OS geofence wake-up,
sends on every change and every 15 s (opted-out devices every 5 minutes),
and backs off 5 s after an error.

## Taking part

- When the station opts in, every signed-in device takes part by default.
- Before asking for location the app shows an explanation (banner): what the
  lock does, that only "in range"/"out of range" is sent, and that the user
  can opt out. "Allow location" asks the OS; "Opt out on this device" opts
  out. Until the prompt is answered the device reports "cannot tell".
- Settings > Proximity (this device): "Take part on this device", the
  location permission, "Your device reports: in range / out of range / not
  participating / cannot tell (why)", the background note, the device name
  shown to the station, and the last report error.
- A denied OS permission counts as opting out, and the UI says so.

## Platforms

| Platform | How | In the background |
| --- | --- | --- |
| iOS | `assets/ProximityShim.m`: its own CLLocationManager; "When In Use" then "Always" on request; while taking part: background location updates (`UIBackgroundModes` `location`, blue indicator), significant-change monitoring, and a `CLCircularRegion` (radius + exit margin, at least 100 m) around the reference that wakes the app on entry/exit; the region moves when the rocket fix moves more than 25 m. | Yes with "Always". With "While Using", until iOS suspends or kills the app. Region monitoring can relaunch a killed app in the background, but the reporter runs only if the app finishes launching. |
| Android | `ProximityBridge`, `ProximityService` (foreground service type `location`, notification "Proximity lock"), `ProximityGeofenceReceiver`: fused location every 10 s into the same Rust fix the map uses, and a Geofencing API fence (needs "Allow all the time"). Permissions: FINE/COARSE first, then ACCESS_BACKGROUND_LOCATION after an in-app rationale (Android 11+ opens Settings). | Yes while the service runs ("Allow all the time" for the fence; a service started while the app is open keeps location "while in use"). If Android kills the app, the service stops too. |
| macOS, Windows, Linux | The desktop location (CoreLocation, Windows location, GeoClue) every 15 s while the app is open. | No: best effort while open. |
| Web | `navigator.geolocation.watchPosition` while the page is open. | No: browsers throttle or stop hidden pages. |
| Watches | Do not take part. | |

When a device stops reporting, the station's stale policy decides (warn or
block); see the station doc.

Play Store and App Store: background location needs the store's declaration
(Google Play's background location form with the rationale; App Store review
notes explaining the "Always" use). The usage strings say the location stays
on the phone.

## UI

- **Banner on every view** (`ProximityBanner`): red "PROXIMITY LOCK" with the
  reason while it refuses; purple "PROXIMITY LOCK OVERRIDDEN by … (m:ss left)";
  amber for unknown devices; otherwise a one-line indicator ("Proximity lock:
  4 devices reporting, 1 in range (Alex's iPhone), 2 opted out").
- **Actions tab, Safety section** (`ProximityActionsNotice`): the banner, who
  is in range and unknown, what is refused and what never is, "Override
  proximity lock…" (administrators, minutes and reason, confirm dialog) and
  "End override now" (anyone who may send commands).
- **Controls**: the station disables blocked controls with the reason, shown
  as each control's disabled reason/tooltip.
- **Settings > Safety (station)** (`ProximityAdminSettings`): on/off, radius,
  exit margin, heartbeat timeout, blocked classes, stale policy (warn
  recommended / block), the device list (administrators remove unknown
  devices). Saved with a confirm dialog listing each change; read-only for
  non-administrators.
- **About safety**: the station's catalog has a "Personnel proximity lock"
  section with the live state.

## Tests

- Rust: `proximity_lock/tests.rs` (haversine vectors, hysteresis, the
  accuracy rule, the reference choice, participation, the serialized report,
  banners, settings and override requests, preferences), `proximity_lock_view`
  (banners, the permission explanation, settings rendering, heartbeats),
  `proximity_native` (fence movement tolerance).
- Kotlin (JVM, offline with the Wear build's cache):
  `watch/wear/gradlew -p platform/android/unit-tests --offline test` runs
  `ProximityPolicyTest` (permission states, the request order, fence radius,
  user-facing text).
- Python: `tests/test_proximity_lock_platform.py` (manifest, Play Services
  location dependency, iOS usage strings and background mode added once,
  the shim's symbols and that it has no networking).
- The iOS shim is Objective-C and is not compiled on Linux; there is no Swift
  in the phone app.

## Device test checklist

Station: lock on, radius 150 m, a launch site configured; a rocket GPS fix on
the map (or note "pad").

1. iOS and Android, fresh install, signed in: the explanation appears; "Allow
   location" shows the OS prompt; the station lists the device as reporting.
2. Walk from 300 m to 100 m from the rocket: the station shows "in range"
   within a few seconds; Fill and supply opens are disabled with the device
   name; Abort, vent, dump and supply close stay enabled; a Fill press via
   API answers 409 with the name.
3. Walk back out: "out of range" only beyond 180 m (radius + margin), with no
   flapping at 150–180 m.
4. Settings > Proximity: turn "Take part" off: the device becomes "not
   participating" at once and the lock clears; on again: it reports again.
5. Deny location in the OS: "not participating (location permission denied)"
   on the station and in the app.
6. Background (Always / Allow all the time): lock the phone, walk into range:
   the station shows "in range" without opening the app. Android shows the
   "Proximity lock" notification while taking part.
7. "While using" only: background the app for several minutes: the station
   shows the device unknown after 60 s; with "warn" a warning lists it, with
   "block" Fill is refused.
8. Force-quit the app: unknown after 60 s (same as 7).
9. Turn the rocket GPS off (or wait 2 minutes): the device reports against the
   pad ("measured against the configured launch site").
10. Override as an administrator (5 min, reason): purple banner on every view
    and the Actions tab; Fill allowed; "End override now" ends it; letting it
    expire restores the lock; the audit lists each step.
11. Enter range during a fill: a warning and notification name the device;
    the fill is not paused or vented; Pause and Cancel still work.
12. Web: the browser prompt; reporting stops when the tab is hidden for more
    than the timeout.
13. RRLS: a device signed in to the RRLS in range: commands sent from the RRLS
    are refused there (409) before forwarding.
