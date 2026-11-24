# Watch companion (Apple Watch and Wear OS)

Companion apps that show configurable data pages, an auto-cycle page, a
recovery compass (over a station map background) and up to two opt-in
action buttons per page.

```
Apple Watch ──WatchConnectivity──┐
                                 ├─ phone app ──HTTPS/WS (existing session)── GroundStation or RRLS
Wear OS watch ──Wear Data Layer──┘                                                  │
      │                                                                             │
      └──── direct link: HTTP polls + page actions with a scoped watch token ───────┘
            (watch Wi-Fi/LTE or the system Bluetooth proxy; only while the phone's data is not fresh)
```

* **Phone first.** The watch shows the phone's snapshots while they are
  fresh. The phone uses whichever station it is connected to (GroundStation
  or RRLS) and follows it when it switches, because the publisher lives
  inside the dashboard that is remounted on every connection change.
* **Direct link when the phone is away.** iOS suspends the phone app in the
  background, so WatchConnectivity snapshots stop. The phone provisions the
  watch with the station address and a scoped, revocable watch token; while
  the watch app is in front and the phone's data is not fresh, the watch
  polls `GET /api/watch/status` itself (see "Direct link" below).
* **Mode follows the phone.** `live` (real station), `demo` (the phone's
  offline preview; every watch page carries a DEMO banner) or `no_station`
  (phone app open on the Connect screen, checking the session, waiting for a
  sign-in or unable to reach the station; `station.link` and
  `station.address` say which, so the watch shows "Connecting to
  gs.local:3000…" rather than nothing). When neither the phone nor the
  station answers, the watch keeps the last data, marks it with its age and
  keeps the compass running from the last known target fixes.
* **No command path through the phone.** The watch may send the phone six
  message types (`hello`, `activity`, `settings`, `tiles`, and the recovery
  search's `search_share` and `search_track`). The phone ignores anything
  else. No watch message reaches a station request other than saving the
  user's own watch settings to their account, and the watch's own recovery
  search opt-in and track (below). A page action is
  sent by the watch itself to the station's ordinary `POST /api/command`
  with its watch token, so the station's authorization, action policy, mode
  gating and admission are exactly those of a phone press.

## Phone side

| Part | File |
| --- | --- |
| Registry of items/pages/targets | `src/telemetry_dashboard/watch_companion/registry.rs`; a telemetry item's channels can be rebound by the station's layout (`watch_items`, `registry::telemetry_channels`) |
| Settings model + validation | `.../watch_companion/settings.rs` |
| Wire format | `.../watch_companion/protocol.rs` |
| Geodesy (shared test vectors) | `.../watch_companion/geodesy.rs`, `watch/test-vectors.json` |
| Snapshot builder + publisher | `.../watch_companion/snapshot.rs`, `publisher.rs` |
| Phone connection phase before a dashboard | `.../watch_companion/phone_status.rs` |
| Account sync | `.../watch_companion/account_sync.rs` |
| Page actions (permitted filter, policy state) | `.../watch_companion/actions.rs` |
| Direct-link token and provisioning | `.../watch_companion/direct.rs` (runtime in `mod.rs`) |
| Map background: tile math (shared vectors) | `.../watch_companion/map_tiles.rs`, `watch/test-vectors.json` (`map_tiles`) |
| Map background: tile set, manifest, sending | `.../watch_companion/map_sync.rs`, `fetch_station_tile` in `src/main.rs` |
| Settings → Watch section | `.../watch_companion/settings_section.rs` |
| iOS bridge (WatchConnectivity) | `assets/WatchBridge.m` (compiled by `build.rs` for iOS) |
| Android bridge (Data Layer) | `platform/android/kotlin/com/ubseds/gs26/WatchBridge.kt` |
| Android foreground service while a Wear watch is connected | `platform/android/kotlin/com/ubseds/gs26/WatchLinkService.kt` |
| Station side | GroundStation26 `backend/src/watch.rs`, `docs/backend/watch-direct.md` |

Two publishers feed the watch. The app-wide status publisher
(`use_status_publisher`, mounted at the app root) activates the watch session
at launch, logs every change of the bridge bits (`[watch] link ...` in the
console and the frontend log) and, while no dashboard is mounted, sends
`no_station` snapshots carrying the phone's connection phase: Connect screen
(`offline`, no address), checking the session (`connecting`), login needed
(`sign_in`) or station unreachable (`offline` with the address). It answers
the watch's `hello`/`activity` at once. The dashboard publisher takes over
when the dashboard mounts (`connecting` until the first WebSocket
connection, then `connected`/`reconnecting`). Both share one sequence counter.

The dashboard publisher samples the dashboard's existing state (flight state, launch
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

Bridge state bits: 1 bridge present, 2 paired, 4 watch app installed,
8 reachable, 16 session activated (iOS `WCSession` activation complete;
Android first Data Layer answer). iOS only knows `isPaired` and
`isWatchAppInstalled` after activation, so until bit 16 arrives the tab stays
hidden; the session is activated at app launch and again from Settings, and
the tab is polled every 2 s, so it appears as soon as activation completes.
Settings → Maintenance shows a "Watch link" line with the raw bits, whether
the user is signed in and why the Watch tab is shown or hidden.

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

## Page configuration

Settings → Watch → Pages lists the pages in swipe order. Each page can be
turned off, moved, and opened ("Configure …") to choose:

* **Rows** (items pages): which items, in which order, at most 8. A page
  never becomes empty.
* **Actions**: up to two buttons, chosen from the actions the station offers
  this account on the watch (`GET /api/watch/actions`, refreshed with the
  account sync). Off by default. An action the station no longer offers
  (another station, a changed account, a different mode) stays in the
  document but is hidden on the watch and marked in Settings.

The auto-cycle items are chosen in the Auto-cycle card as before. The watch's
own settings screen can change rows and remove actions; adding actions is
phone-only, because only the phone knows what the station permits.

The configuration is part of the watch settings document (version 2):
`pages[].items` and `pages[].actions`, plus `direct_link`. It syncs to the
account and is gated exactly like the rest of the document (signed-in user of
the native app with the watch app installed). A version 1 document migrates
to the layout it always had: every items page gets its registry rows, no
actions, direct link on. Defaults equal the previous fixed layout.

## Page actions

* At most two per page (enforced in the editor, in `sanitize` and on the
  watch); the phone publishes only actions the GS lists for the account
  (`actions::permitted`), and refuses Launch/pyro-class names
  (`actions::LAUNCH_AND_PYRO`) unless the GS entry is explicitly
  `watch_safe`. The GS itself never lists Launch, GroundStationLaunch,
  ResetLaunchLatch, TogglePhysicalLaunchMode, ToggleLaunchInterlock, Igniter,
  IgniterSequence, DeployParachute, ExpandParachute, RecoverStation or the
  HITL bench controls (`watch::WATCH_SAFE_OVERRIDES` is empty).
* Each button shows the station's enable state and reason (from the phone's
  live action policy, or from the station's own status on the direct link)
  and is disabled while the station disables it or the shown data is stale.
  Abort is never disabled: the station admits a manual Abort under every
  policy.
* Every press needs a confirmation on the watch: Abort is tap, then "Confirm
  ABORT"; every other action is a confirm screen and a 1.5 s press-and-hold.
* The watch sends the press itself (`POST /api/command` with the watch
  token; never retried) and shows the result: **accepted** (2xx, with the
  station's message), **refused** with the station's reason (4xx; 401 means
  the token was not accepted) or **outcome unknown** (timeout, network error,
  408/429/5xx: check the station).

## Direct link

* The phone (dashboard publisher, every 2 s) asks the GS for a watch token
  when an eligible user is signed in, "Direct link" is on (Settings → Watch,
  default on) and the phone is connected to a station:
  `POST /api/auth/watch-token {"device_id": <random id of this phone install>,
  "commands": <every permitted action on an enabled page>}`. The token can
  read `/api/watch/status` and send only those commands; its other
  properties (scope, digest-only storage, expiry with the phone session,
  revocation) are in GroundStation26 `docs/backend/watch-direct.md`. The
  user's password is never copied. A new token is requested when the
  station, account or action set changes or within 3 days of expiry.
* Provisioning message (phone → watch):
  `{"v":1,"type":"direct","enabled":true,"addresses":["https://gs.local:3000"],"token":"gsw1.…","expires_at_ms":…,"username":"ada","station":"GroundStation","skip_tls":false,"commands":["Abort"]}`;
  `enabled:false` without a token makes the watch forget its token. iOS sends
  it with `transferUserInfo` (key `gs26_direct`), which is delivered even
  after the phone app is suspended and is never put in the application
  context; Android puts it in the DataItem `/gs26/direct`. The Apple Watch
  keeps the token in its keychain (this device only); the Wear watch keeps it
  encrypted with an Android Keystore key. Neither stores it anywhere else.
* Revocation: signing out on the phone (GS logout revokes the tokens that
  session issued, and the phone tells the watch to forget it); turning off
  "Direct link" or "Revoke watch token" in Settings → Watch; an
  administrator disabling the account.
* Route (`chooseRoute` in WatchCore / `Direct.chooseRoute` in the Wear core,
  `link_route` vectors): the phone's data while it is fresh (at most
  max(6 s, 3 × the foreground snapshot interval) old and the phone is
  connected to a station or in demo); otherwise the station's own status
  while that is fresh; otherwise whichever is newer.
* Polling: only while the watch app is in front, provisioned, and the
  phone's data is not fresh; every 3 s (saver 5 s, recovery 2 s), plain
  URLSession / HttpURLConnection requests, no sockets. The status asks for
  the user's units, the compass staleness and the page actions' state. The
  reply has the phone snapshot's data keys; the watch shows it with the
  pages, settings and catalog of the phone's last snapshot.
* The status line shows the path: "Via phone · updated 3s ago" or "Direct
  to GS · updated 2s ago"; with no fresh data a third line says "Contacting
  GS directly…" or "Can't reach GS directly: <error>".
* Plain-HTTP stations: the Apple Watch app allows arbitrary loads (ATS); the
  Wear app allows cleartext. With the phone's "skip TLS verification" for
  the station, the watch accepts that one host's certificate without
  verification.
* Wear OS: the phone app also runs a foreground service ("GS watch
  connected") while a watch with the GS app is reachable, so the phone keeps
  publishing in the background; Android may refuse to start it from the
  background, in which case the direct link covers it.

## Wire format v1

Transport:

| | Phone → watch | Watch → phone |
| --- | --- | --- |
| watchOS | `updateApplicationContext(["gs26": <snapshot json>])`; when reachable and the watch is foreground also `sendMessage(["gs26": json])` without reply handler | `sendMessage(["gs26_watch": json])` when reachable, else `transferUserInfo` (settings only) |
| Wear OS | DataItem `/gs26/snapshot`, DataMap keys `json` (String) and `seq` (Long); `setUrgent()` while the watch is foreground | `MessageClient` path `/gs26/watch` (UTF-8 json bytes) to nodes with capability `gs26_phone`; settings additionally as DataItem `/gs26/watch_settings` whose `json` key holds the same full message, so they sync after a disconnect |
| Map (watchOS) | manifest: `transferUserInfo(["gs26_map": json])`, replacing a queued one; tiles: `transferFile(url, metadata: ["gs26_tile": "z/x/y", "set": id])` | `tiles` report like `settings` (message when reachable, else queued `transferUserInfo`, replacing a queued report) |
| Map (Wear OS) | manifest: DataItem `/gs26/map` (`json`); tiles: DataItem `/gs26/tile/{z}/{x}/{y}` with Asset `tile` (JPEG) | `tiles` report as a `/gs26/watch` message |

Snapshot (phone → watch), abridged:

```json
{"v":1,"seq":42,"mode":"live",
 "station":{"name":"GroundStation","role":"primary","link":"connected","address":"gs.local:3000","telemetry_age_ms":350},
 "account":{"signed_in":true,"sync":"synced"},
 "items":{"flight_state":{"label":"State","text":"In flight · Descent","unit":"","age_ms":0,"stale":false,"level":"normal"},
          "t_clock":{"label":"T-clock","text":"T+ 01:23","unit":"","age_ms":0,"stale":false,"level":"normal","clock":{"kind":"tplus","ms":83000}},
          "altitude":{"label":"Altitude","text":"1234","value":1234.0,"unit":"m","age_ms":200,"stale":false,"level":"normal"}},
 "pages":[{"id":"flight","kind":"items","title":"Flight","items":["flight_state","t_clock","altitude","velocity"],
           "actions":[{"cmd":"Abort","label":"Abort","abort":true}]},
          {"id":"cycle","kind":"cycle","title":"Cycle","items":[]},
          {"id":"compass","kind":"compass","title":"Recovery","items":[]}],
 "targets":[{"id":"rocket","label":"Rocket","lat":32.99,"lon":-106.97,"alt_m":1401.0,"age_ms":3000,"stale":false,"source":"Rocket GPS","fallback":false,"predicted":false},
            {"id":"ground_station","label":"Ground station","lat":32.98,"lon":-106.98,"alt_m":null,"age_ms":0,"stale":false,"source":"Saved GroundStation (manual)","fallback":true,"predicted":false}],
 "settings":{ "...": "WatchSettings, below" },
 "catalog":{"items":[{"id":"altitude","label":"Altitude","group":"Trajectory"}],"pages":[...all pages...],"targets":[{"id":"rocket","label":"Rocket"}]},
 "actions":{"Abort":{"enabled":true}},
 "link_timeout_s":15}
```

* `pages` are the enabled pages in order with the user's rows and the
  permitted actions (`label`, `abort`); `actions` holds the station's enable
  state (and `reason` when disabled) of every action on them. Both are
  absent/empty when no page has actions.
* A direct status from the station has the same data keys plus
  `"source":"direct"` and `seq` = server time; it has no `pages`,
  `settings`, `catalog` or `account`.

* `station.link` is `connected`, `connecting` (first connection, or the
  phone checking the session), `reconnecting`, `sign_in` (the station needs a
  login on the phone) or `offline`; `station.address` is the station's
  host[:port] or empty when none is chosen. `seq` only grows, across both
  publishers.
* `items` holds every registry item; a missing value has `text:"--"`,
  `level:"unknown"`. `level` is `normal|warn|alert|unknown`.
* `clock.kind` is `idle|tminus|tplus`. The watch shows `T- MM:SS` counting
  down from `ms` (clamped at 0) or `T+ MM:SS` counting up, from the receive
  instant. From one hour the clock is `H:MM:SS` and from one day `D:HH:MM:SS`
  (`T+ 1:02:03:04`), the same text as the phone and station (`tclock_format`
  in `watch/test-vectors.json`).
* Ages (`age_ms`) are at send time; the watch adds time since it received the
  snapshot. A target is stale when `stale` is true or its current age exceeds
  `settings.compass.stale_after_s`.
* `predicted` (always sent; a watch treats it as false when absent) marks a
  model estimate rather than a fix. See "Predicted rocket position" below.
* `demo_offset:{north_m,east_m}` appears only in demo mode: the watch places
  that target at its own position plus the offset so the compass can be
  practised anywhere.
* Unknown page `kind`: show "Update the watch app". Unknown fields: ignore.

`WatchSettings`:

```json
{"version":2,"updated_ms":1730000000000,"updated_by":"phone",
 "pages":[{"id":"flight","enabled":true,"items":["flight_state","t_clock","altitude","velocity"],"actions":["Abort"]},
          {"id":"propulsion","enabled":true,"items":["tank_pressure","chamber_pressure","loadcell_mass","fill_percent"],"actions":[]},
          {"id":"trajectory","enabled":true,"items":["altitude","velocity","gps_altitude","gps_sats"],"actions":[]},
          {"id":"health","enabled":true,"items":["station_link","boards","av_battery","rocket_fix_age"],"actions":[]},
          {"id":"cycle","enabled":true,"items":[],"actions":[]},{"id":"compass","enabled":true,"items":[],"actions":[]}],
 "cycle":{"items":["flight_state","t_clock","altitude","velocity","tank_pressure"],"interval_s":5,"autostart":true},
 "compass":{"north":"true","stale_after_s":30,"targets":["rocket","ground_station","rrls"]},
 "units":"metric","power":"balanced","direct_link":true,
 "map":{"enabled":true,"cap_mb":3}}
```

Limits: `interval_s` 2–60, `stale_after_s` 5–600, `map.cap_mb` 1–5 (a
document without `map` gets `{"enabled":true,"cap_mb":3}`), ≤ 12 cycle items, ≤ 8 rows
per items page (only items pages have rows), ≤ 2 actions per page (command
names), document ≤ 8 KiB. Version 1 documents (no `items`, `actions`,
`direct_link`) migrate as described under "Page configuration". `updated_by` is `phone|watch|account`. The phone sanitizes every
document against its registry, so a watch may send any edit and the phone's
echo is authoritative.

Watch → phone:

```json
{"v":1,"type":"hello"}
{"v":1,"type":"activity","foreground":true,"page":"compass"}
{"v":1,"type":"settings","settings":{ ...WatchSettings with updated_by "watch"... }}
{"v":1,"type":"tiles","set":"9f2c4e1ab03d7c55","have":["11/449/824","13/1797/3297"]}
{"v":1,"type":"search_share","sharing":true}
{"v":1,"type":"search_track","session_id":"rs-…","points":[{"seq":12,"t_ms":1767225600000,"lat":31.76,"lon":-102.03,"acc_m":6.0}]}
```

## Watch layout

Both watches use the same pages and the same logic, checked against the
`connection_status`, `auto_cycle` and `alerts` vectors in
`watch/test-vectors.json` (`WatchCore/Status.swift`, `core/.../Status.kt`):

* Horizontal pages: **Status** (always the landing page: flight state,
  T-clock, the data path and the station connection), **Alerts** (items
  at alert, then warn level, plus link problems), the pages enabled in the
  watch settings, and **Settings** last. Settings is never selected
  automatically; a page that disappears falls back to Status.
* A page's action buttons (if any) sit below its content.
* Path line: "Via phone · updated 3s ago" or "Direct to GS · updated 2s
  ago"; on the phone path with no data for longer
  than `link_timeout_s`, "Phone reachable · no update for 2m 05s" or "Phone
  not reachable · last update 2m 05s ago". "Open the GS app on your phone"
  appears only when the phone is not reachable and there is no fresh data
  (Apple: `WCSession.isReachable`; Wear: a reachable node with `gs26_phone`).
  With the phone reachable but no snapshot yet: "Phone app open · waiting
  for data".
* Station line: "Connecting to gs.local:3000…", "Connected to GroundStation",
  "Reconnecting to …", "Sign in on the phone (…)", "Can't reach …", "Phone
  not connected to a station", "Demo data (offline preview)".
* The cycle page is a vertical pager (watchOS `.verticalPage`, Wear
  `VerticalPager`) that advances every `interval_s`. The Digital Crown (Wear:
  rotary input) is the primary control, swipe works too; a manual move
  restarts the wait so the item stays for a full interval. Tap pauses or
  resumes. A small footer shows "2/5 · next in 3s" or "2/5 · paused".

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

### Predicted rocket position

When the station loses the rocket's signal in flight, its loss-of-signal
prediction (GroundStation26 `GET /api/los_prediction`, state `predicting`)
estimates where the rocket is now. The watch gets that estimate as an extra
target:

```json
{"id":"rocket_predicted","label":"Predicted: no signal","lat":31.7591,"lon":-102.039,"alt_m":2190.0,
 "age_ms":12300,"stale":false,"source":"Prediction","fallback":false,"predicted":true}
```

* `age_ms` is the time since the last real rocket fix (the station's
  `since_fix_s`), not the age of the estimate. `stale` is false: a
  prediction is never stale by age, because its age is the reason it exists.
* The station puts it in `/api/watch/status` while predicting (direct link).
  The phone builds its targets from `/api/antenna/status`, so it also polls
  `GET /api/los_prediction` next to the antenna poll (same interval, and only
  while the compass shows the rocket) and relays it: right after `rocket`,
  only when `rocket` is a compass target, only while `state` is `predicting`
  with a valid `predicted_now`, label from the station (default
  "Predicted: no signal"). A prediction the phone received more than 30 s
  ago is dropped rather than shown frozen; a station without the route
  (404 three times in a row) is asked only every 30 s.
* `rocket_predicted` is never in `settings.compass.targets`: the compass
  lists it right after `rocket` whenever `rocket` is selected (and not
  otherwise). Tap cycles to it like any target. The rocket's own items
  (`rocket_fix_age`) keep using the real `rocket` target.
* Drawn distinctly in the LOS-prediction pink `#f472b6`: the label
  "Predicted: no signal" in pink, a hollow arrow, the age line
  "12s since last fix" (real targets read "fix 12s"), and on the map a
  hollow dashed pink ring with a dashed pink line from the watch when
  selected. The last real fix stays drawn as the normal rocket marker.
* `predicted_target` vectors cover parsing, the relayed target and the
  compass order, staleness and age line.

## Map background

The compass draws the station's map under its arrow, so it reads like a map.

**Tile source.** The station's own tiles, `GET /tiles/{z}/{x}/{y}.jpg`:
the same source as the phone's map tab, served by the GS from its offline
bundle (Esri World Imagery, downloaded beforehand by GroundStation26
`map_downloader`), so it works in the field without internet. The phone
reads them through its existing tile cache (`fetch_station_tile`), so tiles
the phone map already showed cost nothing. No phone or watch ever requests
a public tile server; there is no OpenStreetMap or Esri traffic from this
feature. Credit: "Imagery © Esri, Maxar, Earthstar Geographics", shown
small at the bottom of the compass whenever tiles are drawn.

**Phone side** (`map_sync.rs`, dashboard publisher, every 15 s). Runs only
for an eligible user (signed in, paired watch with the app installed) with
"Map background" on while the dashboard is connected to a station.

* Centres: the snapshot's targets in priority order: rocket (last fix),
  ground station, RRLS. Demo targets and the predicted rocket position are
  never centres (the rocket's last fix already is).
* Plan (`map_tiles::coverage`, `coverage` vectors): zooms 11–15 with radii
  10, 5, 2.5, 1.2 and 0.6 km (a watch face shows about 0.5–10 km across
  them), each clamped to the station's `max_native_zoom` from
  `/api/map_config`; low zoom first, centre by centre, nearest tiles first;
  at most 200 tiles.
* Budget: tiles are read in plan order and kept while the total stays under
  the size cap (Settings, 1–5 MB, default 3 MB); a tile the station does not
  have is skipped. Low zooms of every centre always fit before the detailed
  levels of the later centres.
* Rebuild (`replan` vectors) when a centre moves more than 500 m (the rocket
  landed far away), a centre appears or disappears, the cap or station
  changes, or the set is older than 7 days (then everything is resent, the
  station's tiles may have improved).
* Manifest (phone → watch): `{"v":1,"type":"map","set":"<16 hex>","tiles":["11/449/824",…],"attribution":"…","cap_bytes":3145728,"max_zoom":15,"centres":[{"id":"rocket","lat":…,"lon":…}],"built_ms":…}`.
  `set` is a content hash of `tiles`.
* Sending only what is missing: the phone keeps what the watch reported
  (`tiles` message) plus what it has sent since; `manifest_diff` gives the
  tiles to send. A sent tile is not resent for 10 minutes unless a later
  report still misses it. A report naming another `set` (watch reinstalled)
  gets the manifest again. iOS cancels queued file transfers for tiles that
  left the set; Android deletes tile DataItems once reported or out of the
  set. Tile files wait in the app's temporary directory until sent.

**Watch side** (`TileStore.swift`, `TileCache.kt`).

* Tiles live in the app container (Application Support `map_tiles/`,
  excluded from backup; Wear `filesDir/map_tiles`), capped at the manifest's
  `cap_bytes` (the user's cap). Eviction (`evict` vectors): tiles outside the
  current set first, then least recently used.
* After receiving tiles (debounced 5 s), at launch and when the phone
  becomes reachable the watch sends the `tiles` report.
* Rendering: north-up, or heading-up whenever the watch has a heading (like
  the arrow, which keeps working on top). Centred on the watch (on the
  selected target before the first GPS fix). Zoom fits the watch and the
  selected target (`auto_fit`: the target at 70 px from the centre, zoom
  11–16). The Digital Crown (Wear: rotary input) zooms 10–17 from the fitted
  zoom while the compass page is in front; it does not clash with the cycle
  page, which owns the crown only while it is on screen. Switching target
  returns to the fitted zoom. Missing levels are cut from the nearest cached
  ancestor (`ancestor` vectors), so zooms between and beyond the sent
  levels still show imagery. Markers: rocket orange, ground station green,
  RRLS blue (dimmed when stale), the predicted rocket position as a hollow
  dashed pink ring, the watch as a blue dot, a line from the watch to the
  selected target (dashed pink to a prediction) and a red north tick on the
  rim. The imagery
  is dimmed so the compass text stays readable.
* No tiles for the view (map off, nothing cached, far from every centre):
  the plain compass, exactly as before.
* Direct mode (the phone's data is not fresh and the direct link is
  provisioned): the watch fetches missing tiles of its current view from
  `GET /tiles/{z}/{x}/{y}.jpg` with its watch token (allowed read-only for
  watch tokens by the GS scope, GroundStation26 `docs/backend/watch-direct.md`),
  at most six at a time, a tile the station lacks not again for a minute.

**Settings.** Phone Settings → Watch → "Map background": on/off (default
on) and the size cap (1–5 MB), with a status line ("120 tiles (2.4 MB)
around rocket, ground station; 118 confirmed on the watch"). The watch's
own settings screen has the on/off toggle. Both edit `settings.map` in the
synced watch settings document.

## Voice

Push-to-talk crew voice on the wrist: the watch joins the same channel as
the station's Crew voice page (`/radio`) and the phone app's voice page, so
web, phone and watch users talk to each other.

```
watch ──WSS /api/voice/ws (Opus 16 kHz, 20 ms)──────────────┐
  │   (watch Wi-Fi/LTE or the system Bluetooth proxy)        ├─ GS voice relay ── browsers / phone (PCM16)
  └──phone relay: same protocol inside watch messages──phone ┘        │
                                                                      ├─ audience program buffer, RRLS radio bridge
```

### Transport and codec

* **Direct first.** The watch opens `wss://<station>/api/voice/ws` itself
  with its scoped watch token, like the direct telemetry link, so voice
  keeps working while the phone sleeps. Only when two direct attempts in a
  row fail and the phone is reachable does it relay through the phone
  (below); with no phone it keeps retrying direct (`voice.transport`
  vectors).
* **Opus, not WebRTC.** The station already runs a WebSocket voice relay
  (no WebRTC, no SFU); browsers send mono 16 kHz PCM16, 640 B per 20 ms
  (256 kbit/s). The watch says `"codec":"opus"` in its `auth` message and
  sends one Opus packet per 20 ms frame (16 kHz mono, about 24 kbit/s, 10×
  less radio time than PCM). The station decodes it to PCM16 for everything
  else (browsers, the audience program buffer, the RRLS radio bridge) and
  encodes PCM speakers once per frame for Opus listeners. A watch whose OS
  cannot encode Opus falls back to `"codec":"pcm16"`, which needs no
  station-side work.
* Frames: client → station one Opus packet (1–400 B) or one 640 B PCM16
  frame; station → client the four-byte little-endian speaker id, then the
  payload in the listener's codec (`voice.frames` vectors). Speaker
  `0xfffffffe` is the recovery radio.
* Control: `{"type":"auth","token":…,"codec":"opus"}` first (within 5 s),
  then `{"type":"transmitting","enabled":…}`, `{"type":"active","enabled":…}`
  (watch only, see "Phone and watch together"), `{"type":"radio","enabled":…}`
  and `{"type":"ping"}` every 10 s. The station sends `joined`
  (`id`, `codec`, `sample_rate`, `frame_samples`, `device`), `roster`
  (`peers[]` with `id`, `name`, `transmitting`, `device` = `web|watch`),
  `handoff` (`held`, `by`), `error` (`message`), `radio_link`,
  `radio_state` and `broadcast_state`. Unknown types are ignored.

### Access

The phone asks for crew voice in the watch token request
(`"voice": true`) while Settings → Watch → Voice is on, the Voice page is
enabled and Direct link is on. The station grants it only to an account
that has the voice transmit permission (administrators inherit it) and
rechecks it on every use; the provisioning message carries `"voice":true`
when granted. A watch token with voice reaches only `/api/voice/ws` and
`GET /api/voice/status` in addition to its other endpoints (GroundStation26
`docs/backend/watch-direct.md`); it never reaches the audience commentary
socket, the broadcast controls or any media page.

### Phone and watch together (echo avoidance)

The station groups a phone session with the watch whose token that session
issued. Within the group it never relays audio, so the watch never plays
the phone's microphone or the reverse. While the watch is *active* (it
joined; `active:false` yields), the phone is *held*: its microphone is
ignored, it receives no audio and it gets `{"type":"handoff","held":true}`;
the phone's voice page shows "Your watch has voice" and switches open mic
to listen-only. When the watch leaves or yields the phone gets
`held:false` and listens again; its microphone comes back only when the
user presses push-to-talk (never silently). A shared crew login on other
people's phones is a different group and unaffected.

### Watch behaviour

* **Voice page** (page id `voice`, kind `voice`, enabled by default and
  ordered like any other page in Settings → Watch → Pages): channel name,
  connection state and path ("Direct to GS" / "Via phone"), a big
  push-to-talk button, who is talking (roster names with
  `transmitting`, the radio as "Recovery radio"), and Join / Leave.
  Push-to-talk is press-and-hold or tap-to-toggle (`voice.ptt` vectors);
  any talk stops after 60 s (`max_talk_ms`) so a stuck button cannot hold
  the channel. Haptics: start/stop/denied when `haptics` is on, a tap when
  someone else starts talking when `talker_haptics` is on (`voice.talkers`).
* **Volume**: the Digital Crown (Wear: rotary input) on the voice page,
  0–150 % in 5 % steps (`voice.volume`), saved to `voice.volume`.
* **Playback**: a per-speaker playout buffer that waits for two frames
  (40 ms), holds at most ten (200 ms, older audio dropped) and mixes all
  speakers with integer clipping (`voice.mix`).
* **Reconnect**: after a lost connection the watch retries with
  `floor(min(1 s × 2^(n−1), 30 s) × (0.75 + 0.5 × jitter))`; a refusal
  (permission, sign-in, no Opus) is not retried, a full channel is
  (`voice.backoff`). Reconnecting never turns the microphone back on.
* **Quick start**: a watchOS Smart Stack widget / complication and a Wear
  OS tile open the app straight on the voice page and join.
* **Microphone permission** is asked when the user first joins; a denial
  shows how to allow it in the watch's settings.
* **Background**: watchOS keeps voice running with the wrist down as a
  CallKit call ("GS crew voice"), which is what lets a watch app record,
  play and use a socket in the background; Wear OS runs a foreground
  service of type `microphone`. Leaving voice ends both.
* **Audio route** (`route`): `auto` (a connected Bluetooth headset, else
  the watch), `speaker` (always the watch) or `bluetooth` (prefer the
  headset).

### Settings

`voice` in the watch settings document:

```json
"voice":{"enabled":true,"channel":"crew","ptt":"hold","haptics":true,
         "talker_haptics":false,"route":"auto","volume":100}
```

`channel` is `crew` (the station's crew channel) or `crew_radio` (the same,
plus relaying push-to-talk over the recovery radio while the station offers
it); `ptt` `hold|toggle`; `route` `auto|speaker|bluetooth`; `volume`
0–150. Unknown values fall back to the defaults. A document without
`voice` gets the defaults and the Voice page appended.

### Phone relay (fallback)

When the watch cannot reach the station itself it sends the same protocol
through the phone, which opens `/api/voice/ws` with the watch's own token
(so the station still sees the watch, with its grant and group):

```json
{"v":1,"type":"voice","op":"open","codec":"opus"}
{"v":1,"type":"voice","op":"control","msg":{"type":"transmitting","enabled":true}}
{"v":1,"type":"voice","op":"audio","frames":["<base64 frame>", …]}
{"v":1,"type":"voice","op":"close"}
```

and the phone answers with batches (at most every 60 ms):

```json
{"v":1,"type":"voice","op":"server","text":["<station json>", …],"frames":["<base64 station frame>", …]}
{"v":1,"type":"voice","op":"closed","reason":"…"}
```

`control.msg` may only be `transmitting`, `active`, `radio` or `ping`
(the phone sends `auth` itself); at most 10 frames per message. The
messages travel on their own channel so the phone can drain them every
20 ms: iOS `sendMessage` with key `gs26_watch_voice` (watch → phone) and
`gs26_voice` (phone → watch), while the phone app is reachable (watchOS
wakes it in the background for a short time); Android `MessageClient`
paths `/gs26/watch_voice` and `/gs26/voice`. The relay stops when the
phone app is suspended, which is why direct is preferred.

### Battery

Opus at 24 kbit/s keeps the radio mostly idle; nothing is sent while not
talking except a ping every 10 s. The microphone runs only while voice is
joined. Leave voice when not needed: a joined session keeps the audio
hardware and a socket alive (watchOS as a call, Wear OS as a foreground
service).

## Recovery search

During a recovery search (backend-contract.md, "Recovery search") the watch
can share its own track, relayed by the phone.

* **Opt-in on the watch.** The Status page shows a "Recovery search" section
  while the phone reports a running search. "Share watch track…" explains
  what is sent and asks for confirmation. The watch then sends
  `search_share` (`sharing: true`). The phone sends the opt-in to the
  station for the watch as its own searcher device (`<phone device id>-w`,
  "<user>'s watch", platform `watch`). The phone never opts the watch in by
  itself.
* **Snapshot.** Once the phone has seen a search status, every snapshot
  carries `"search": {"active", "session_id", "sharing", "acked_seq",
  "percent", "swath_m"}`. `sharing` is true only while the station's search
  runs and the station accepted the watch's opt-in for that session.
* **Recording.** The watch records only while its user wants to share and
  the snapshot says `active` and `sharing` for the same session. While it
  records it shows a red "● SHARING TRACK" and "Stop sharing". Fixes are
  thinned (5 m or 30 s, accuracy at most 100 m), numbered by the watch and
  kept in a persisted buffer of at most 5,000. Location runs only while the
  watch records, and then also with the wrist down or another app in front:
  * Apple Watch: background location updates (`UIBackgroundModes`
    `location`), started from the app in front when recording starts. Every
    fix goes to the recorder from the location callback (the 2 s tick does
    not run in the background) and the buffer goes to the phone at most every
    10 s; updates stop as soon as the watch no longer records (Stop sharing,
    or a snapshot that ends the search).
  * Wear OS: `SearchTrackService`, a foreground service of type `location`
    with an ongoing "Recovery search" notification and "Stop sharing",
    started from the open app while recording. It keeps the Data Layer
    listener (so it learns when the search ends), records through
    `PhoneLink.searchTick` and stops itself when the watch no longer records.
    It uses a plain ongoing notification; the Ongoing Activity API would need
    `androidx.wear:wear-ongoing`, which the build does not use yet.
  * Not a workout session (`HKWorkoutSession` / Health Services exercise):
    that needs HealthKit or Health Services permissions, records the walk as
    a workout in the user's health data and Activity rings, and App Review
    expects a fitness feature. Background location started from the app in
    front gives the same continuous GPS for a search. If device tests show
    watchOS suspending a long search anyway, a workout session is the
    fallback.
  If the phone is not reachable (or its app is suspended), fixes wait on the
  watch and go when the phone is back; the phone accepts them only for the
  watch's current opt-in and session.
* **Relay.** The watch sends `search_track` (at most 100 fixes, oldest first)
  to a reachable phone every 2 s. The phone takes custody in its own
  persisted outbox (`recovery_search_watch_outbox_v1`) and acknowledges the
  highest contiguous sequence number it holds in the snapshot's `acked_seq`.
  The watch drops what is acknowledged and resends the rest. The phone uploads
  with the watch's own sequence numbers, so the station's de-duplication
  covers a resend after a lost acknowledgement. The phone holds fixes only for
  the watch's current opt-in and session.
* **Stop.** "Stop sharing" on the watch sends `search_share`
  (`sharing: false`), and the phone forwards the opt-out at once. The watch
  discards its buffer. A new session, or the end of the late-upload window,
  ends the opt-in, and the watch drops its buffer when the session changes.
* An Apple Watch `search_share` is queued like `settings` when the phone is
  not reachable. A Wear watch repeats it every 10 s until the snapshot shows
  the station's answer.

Code: phone `watch_companion/search_relay.rs`; WatchCore `Search.swift`,
`GS26Watch/SearchPage.swift`; Wear core `Search.kt`, `PhoneLink.searchTick`
and the Status page's `SearchSection`. Tests: `search_relay` (gating,
out-of-order and repeated batches, the snapshot object), `SearchTests.swift`,
`SearchTest.kt`.

## Building

`build.py` targets `watchos`, `watchos_sim`, `watchos_sim_intel` (iOS app
with the watch app embedded; macOS) and `wearos`, `wearos_sim` (Wear OS APK);
see the README's "Watch companion targets". Manual builds: `watch/apple/README.md`
(macOS + Xcode) and `watch/wear/README.md` (Gradle). Phone-side Rust is covered
by the normal test commands.

## Deploy to a watch for testing

**Refreshing profiles.** After adding a device or regenerating a profile in the
developer portal, open Xcode > Settings > Accounts, select the team and click
**Download Manual Profiles**. The build script looks in both Xcode profile
folders (`~/Library/Developer/Xcode/UserData/Provisioning Profiles` and
`~/Library/MobileDevice/Provisioning Profiles`) and uses an installed
development profile for the bundle id when it is newer than the local
`Groundstation_dev.mobileprovision` / `Groundstation_watch_dev.mobileprovision`,
so the local copies never need to be replaced by hand.

**Optional: the push-to-talk complication.** The watchOS "Push to talk"
widget/complication is a separate app extension with its own bundle id,
`com.UBSEDS.GS26.watchkitapp.widgets`. Without a profile for it,
`watchos_deploy` (and `with_ios`, and `--final` builds) sign and install the
watch app without the widget and print one line, `watch widget skipped: no
development profile for com.UBSEDS.GS26.watchkitapp.widgets; create that App
ID + profile to get the PTT complication`; nothing else changes. To get it:

1. In the developer portal, Certificates, Identifiers & Profiles →
   Identifiers → **+** → App IDs → App, bundle id (explicit)
   `com.UBSEDS.GS26.watchkitapp.widgets`, no extra capabilities.
2. Profiles → **+** → iOS App Development (watchOS uses iOS profiles), that
   App ID, your Apple Development certificate and the watch's device (the
   same devices as the watch profile).
3. Download it as `Groundstation_watch_widgets_dev.mobileprovision` into the
   frontend folder, or use Xcode → Settings → Accounts → **Download Manual
   Profiles**, or point `IOS_WATCH_WIDGETS_DEVELOPMENT_MOBILEPROVISION` at it.
   The newest of the local file and an installed profile wins, as for the
   other profiles. Distribution builds use
   `IOS_WATCH_WIDGETS_DISTRIBUTION_MOBILEPROVISION`.

A profile that does not list the watch is skipped the same way, with its
reason. `watchos_sim_deploy` always includes the widget (simulators need no
profiles).

`build.py` builds, signs and installs the companions for testing. Like the
phone actions they mirror, the device actions only install: `watchos_deploy`
behaves like `ios_deploy` (no launch) and the Wear actions like
`android_install`, so open GS26 from the watch's app list. `watchos_sim_deploy`
launches what it installs, like `ios_sim_deploy`. The Apple Watch actions install
only the watch app unless `with_ios` is given. Each action
builds first; add `existing` to reuse the last build and `debug` for a debug
build.

Each action (and each `--target watchos*|wearos*` build) first checks its
tools and stops before any build work if one is missing, printing the install
command: `xcodegen` (`brew install xcodegen`, not needed with `existing`),
Xcode's `xcrun`/`xcodebuild` (the full Xcode must be selected, not only the
Command Line Tools), `ios-deploy` for `watchos_deploy with_ios` only
(`brew install ios-deploy`); the Android SDK and `adb` for the Wear actions.

| Command | Installs on |
| --- | --- |
| `python3 build.py watchos_deploy` | the Apple Watch only (watch app, direct install) |
| `python3 build.py watchos_deploy with_ios` | connected iPhone (phone app) + its paired Apple Watch |
| `python3 build.py watchos_sim_deploy` | the watch of an iPhone + Apple Watch simulator pair |
| `python3 build.py watchos_sim_deploy with_ios` | both simulators of the pair |
| `python3 build.py wearos_deploy` | a Wear OS watch connected over adb |
| `python3 build.py wearos_sim_deploy` | a running Wear OS emulator |

### Apple Watch

Prerequisites (macOS):

* Xcode 15 or newer with the watchOS platform installed, selected with
  `sudo xcode-select -s /Applications/Xcode.app`; `brew install xcodegen`
  (plus `ios-deploy` for `with_ios`).
* **Development** profiles with an `Apple Development:` certificate from the
  same team in the login keychain:
  * watch app `com.UBSEDS.GS26.watchkitapp` (always):
    `Groundstation_watch_dev.mobileprovision`,
    `IOS_WATCH_DEVELOPMENT_MOBILEPROVISION=...`, or an installed Xcode-managed
    profile, listing the watch's UDID;
  * iOS app `com.UBSEDS.GS26` (only with `with_ios`):
    `Groundstation_dev.mobileprovision` in the frontend directory,
    `IOS_DEVELOPMENT_MOBILEPROVISION=...`, or an installed Xcode-managed
    profile, listing the iPhone's UDID.

  Distribution profiles are refused.
* Developer Mode on the iPhone and on the watch (watch: Settings › Privacy &
  Security › Developer Mode; the switch appears after Xcode has seen the
  watch once in Window › Devices and Simulators).
* iPhone connected to the Mac (USB or the same Wi-Fi) and trusted; the watch
  paired with it, **unlocked and on its charger** next to it.

```sh
python3 build.py watchos_deploy                       # watch app only; one watch visible to Xcode
python3 build.py watchos_deploy watch="Rylan's Apple Watch"
python3 build.py watchos_deploy with_ios              # phone app + watch app (the old behaviour)
python3 build.py watchos_deploy with_ios watch="Rylan's Apple Watch" phone=<iPhone UDID>
python3 build.py watchos_deploy with_ios watch=skip   # phone install only, then Watch app › Install
```

Default steps (watch app only): list the watch with `xcrun devicectl list
devices` (fails early if none is visible or Developer Mode is off;
`watch=<name|UDID|identifier>` picks one of several), check the watch profile,
build the watch app (xcodegen + xcodebuild for watchOS), check the watch bundle
id (`com.UBSEDS.GS26.watchkitapp`, companion `com.UBSEDS.GS26`), dev-sign a
copy at `target/watch/watchos-signed/GS26Watch.app` with the watch profile and
install it with
`xcrun devicectl device install app --device <watch> target/watch/watchos-signed/GS26Watch.app`.
No `dx` build, no iOS signing, no `ios-deploy`: the phone app already on the
iPhone (from `ios_deploy` or an earlier `with_ios` run) is left as it is. The
watch app is a companion app (`WKRunsIndependentlyOfCompanionApp` is false), so
install the phone app once that way if the iPhone does not have it yet.
`watch=skip` and `phone=` need `with_ios`.

If the direct install fails, the script stops and says what to check: watch
unlocked and on its charger next to the iPhone, Developer Mode on, the watch
visible (and not still preparing) in Xcode › Window › Devices and Simulators,
Mac, iPhone and watch on the same Wi-Fi. The phone fallback is
`watchos_deploy with_ios`, which installs through the iPhone.

`with_ios` steps: as above, plus check the iOS profile, build the iOS app with
`dx`, embed the watch app, sign the watch app and then the iOS app, install the
phone app with `ios-deploy`, then install the watch app with
`xcrun devicectl device install app --device <watch> GS26.app/Watch/GS26Watch.app`.
Nothing is launched. If that direct watch install fails, the script prints the
fallback: Watch app on the iPhone › My Watch › Available Apps › Install (or
turn on General › Automatic App Install).

`watchos_sim_deploy` builds `watchos_sim` on Apple silicon or
`watchos_sim_intel` on Intel, picks a pair from `xcrun simctl list pairs`
(already booted first) or creates "GS26 iPhone" + "GS26 Apple Watch" with the
newest iOS and watchOS runtimes and pairs them, and boots both (the watch
simulator needs its paired iPhone running). By default it builds only the watch
app, ad hoc signs it, installs it on the watch simulator with `simctl install`
and launches it; the iPhone simulator gets no app. `watchos_sim_deploy with_ios`
also builds the simulator iOS app with the watch app embedded and installs and
launches it on the iPhone simulator.

### Wear OS

Prerequisites: Android SDK platform-tools (`adb`) found through
`ANDROID_HOME`/`ANDROID_SDK_ROOT` (or `~/Library/Android/sdk`,
`~/Android/Sdk`), JDK 17/21. The device is picked from `adb devices` by
`ro.build.characteristics` containing `watch`; `serial=<serial>` chooses one
of several. The APK is installed with the same fallbacks as `android_install`
(streamed, no-streaming, push + `pm install`); it is not launched.

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
