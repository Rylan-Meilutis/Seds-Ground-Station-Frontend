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

### Settings sync

One local copy on the phone (`watch_settings_v1`) plus the account revision it
was last synced with. Edits come from Settings → Watch on the phone or from the
watch's settings screen and are stamped `updated_ms` (monotonic per document).

* Signed in: the account document is the source of truth. On sign-in and
  every 60 s the phone reads `GET /api/account/client-settings/watch`. If the
  local copy has an unsynced edit based on the current account revision it is
  pushed; otherwise the account copy is adopted. A push that loses a race
  (HTTP 409) re-reads and keeps whichever copy has the newer `updated_ms`.
* Not signed in, demo, or station without the endpoint: local only
  (`account.sync = "local_only"`).
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
 "items":{"flight_state":{"label":"Flight state","text":"Descent","unit":"","age_ms":0,"stale":false,"level":"normal"},
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

See `watch/apple/README.md` (macOS + Xcode) and `watch/wear/README.md`
(Gradle). Phone-side Rust is covered by the normal test commands.
