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
* **No command path through the phone.** The watch may send the phone four
  message types (`hello`, `activity`, `settings`, `tiles`). The phone ignores
  anything else; no watch message reaches a station request other than
  saving the user's own watch settings to their account. A page action is
  sent by the watch itself to the station's ordinary `POST /api/command`
  with its watch token, so the station's authorization, action policy, mode
  gating and admission are exactly those of a phone press.

## Phone side

| Part | File |
| --- | --- |
| Registry of items/pages/targets | `src/telemetry_dashboard/watch_companion/registry.rs` |
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
 "targets":[{"id":"rocket","label":"Rocket","lat":32.99,"lon":-106.97,"alt_m":1401.0,"age_ms":3000,"stale":false,"source":"Rocket GPS","fallback":false},
            {"id":"ground_station","label":"Ground station","lat":32.98,"lon":-106.98,"alt_m":null,"age_ms":0,"stale":false,"source":"Saved GroundStation (manual)","fallback":true}],
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
  ground station, RRLS. Demo targets are never centres.
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
  RRLS blue (dimmed when stale), the watch as a blue dot, a line from the
  watch to the selected target and a red north tick on the rim. The imagery
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
