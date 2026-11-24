# Backend contract for the ground-station UI

This is the entry point for anyone building, replacing or extending a backend
for this UI, or reusing the UI for a different vehicle. It lists every backend
interaction the code makes (HTTP routes, WebSocket channels, station-served
pages and assets), what the UI does when one is missing, which data is already
supplied by the backend, and which vehicle-specific details are still compiled
into the frontend.

It is an index. Detailed request/response schemas live in the documents linked
from each table; a concise schema is given here only where none exists
elsewhere. The inventory was built from the source (`src/`, `watch/`,
`static/`, `station_ui/`), not from older documentation, and was cross-checked
against the GroundStation26 router (`backend/src/web.rs` and the route modules
it merges).

Contents:

1. [Overview](#1-overview)
2. [Minimum backend for a working UI](#2-minimum-backend-for-a-working-ui)
3. [Auth, session, discovery and local persistence](#3-auth-session-discovery-and-local-persistence)
4. [WebSocket channels](#4-websocket-channels)
5. [Route reference](#5-route-reference)
6. [Generic-UI rules](#6-generic-ui-rules)
7. [Hard-coding audit](#7-hard-coding-audit)
8. [Contract checks](#8-contract-checks)
9. [Related documents](#9-related-documents)
10. [Offline replay files](#10-offline-replay-files)
11. [Demo mode: a local reference station](#11-demo-mode-a-local-reference-station)
10. [Appendix: route index](#appendix-route-index)

---

## 1. Overview

### 1.1 Architecture

The UI is one Rust/Dioxus code base built for the browser (wasm), desktop
(Linux, macOS, Windows), iOS and Android. Watch apps (Apple Watch, Wear OS) are
separate native code under `watch/` and normally talk to the phone, not the
station.

Every build talks to exactly one station base URL at a time:

- HTTP JSON requests to `{base}/api/...` (plus `{base}/flightstate` and
  `{base}/tiles/...`).
- One long-lived WebSocket `{base_ws}/ws` carrying live telemetry, alerts,
  state snapshots and outbound commands.
- Two optional WebSockets: the raw stream feed `/api/stream/raw/ws` and crew
  voice `/api/voice/ws`.
- Station-served HTML pages embedded in webviews or iframes (`/media`,
  `/radio`, `/api/media-assets/...`).

Request helpers (all add `Authorization: Bearer <token>` when signed in):

| Helper | File | Notes |
| --- | --- | --- |
| `http_get_json`, `http_get_json_captured` | `src/telemetry_dashboard/dashboard_transport.rs` | GET, 15 s timeout (wasm) / 8 s (native); `/api/recent` and `/api/recordings/report` 300 s (`GS_RECENT_HTTP_TIMEOUT_SECS`, 15-600) |
| `http_post_json`, `http_post_empty` | same | POST JSON / POST without body (8 s) |
| `station_settings_request::{get, post, post_captured, put_captured, patch_captured, delete_captured, post_bytes_captured, post_captured_no_redirect}` | `src/telemetry_dashboard/station_settings_request.rs` | Settings and safety writes; PUT/PATCH/DELETE accept an empty 2xx body; `no_redirect` variants reject redirects |
| `neutral_request_owner::{get, post}` | `src/telemetry_dashboard/neutral_request_owner.rs` | Admin, media and history panels |
| `recording_download::saved_json`, `download_csv_path_owned` | `src/telemetry_dashboard/recording_download.rs` | GET JSON / file downloads |
| `auth::*` (reqwest / gloo-net) | `src/auth.rs` | Session, challenge, login, logout |
| `command_owner::post_primary` | `src/telemetry_dashboard/command_owner.rs` | `POST /api/command` |
| Direct reqwest / gloo-net | `firmware_update_tab.rs`, `plumbing_drive.rs`, `gse_config_compatibility.rs`, `native_station.rs`, `native_media.rs`, `station_mode_transport.rs`, `sd_board_download.rs` | Same headers, feature-specific bodies |

Policy-epoch headers: when the station advertises epoch fencing (via
`GET /api/station-modes`), mutating requests carry `X-Station-Policy-Epoch`
(and `X-Station-Control-Origin` where required), live reads validate the
response headers `X-Station-Epoch-Scope: mode` and `X-Station-Policy-Epoch`,
and WebSocket commands carry `policy_epoch` / `control_origin` fields. A backend
that does not advertise fencing needs none of this. Details:
[station-modes.md, "Original ownership and epoch fences"](station-modes.md#original-ownership-and-epoch-fences).

### 1.2 How the base URL is chosen

| Platform | Base URL | Code |
| --- | --- | --- |
| Browser (wasm) | The page's own origin, unless a base was saved; WebSocket uses `ws(s)://` + same host | `UrlConfig::base_http` / `base_ws` in `src/telemetry_dashboard/dashboard_connection.rs` |
| Native (desktop, iOS, Android) | The address saved on the connection screen (`gs_base_url`), filled manually or from LAN discovery; default `http://localhost:3000` when nothing is saved | same; `src/app_connect_page.rs`, `src/native_station.rs` |
| Paired failover (RRLS/primary) | Signed `GS26-STATION1` UDP handover and `GET /api/station/discovery?nonce=` verification against the pairing key from the session | `src/native_station.rs`, `static/station_discovery.js` |
| Watch direct link | `base` provisioned by the phone together with a watch token | `watch/*/Direct.*` |

HTTPS is preferred; native builds allow a per-station "skip TLS verification"
choice (`gs_skip_tls_verify_*`). See [station-discovery.md](station-discovery.md)
and GroundStation26 `docs/backend/station-discovery.md`.

### 1.3 Versioning and degradation

The frontend and GroundStation26 are developed and deployed together from
their `dev` branches; there is no old/new interoperability matrix and breaking
wire changes are allowed when both sides change together. Even so, the UI is
written to degrade rather than crash:

- **Missing route (404, 405, network error).** Background seeds and polls use
  `if let Ok(..)` and keep the previous value (alerts, notifications, boards,
  topology, launch clock, recording status, network time, fill targets, action
  policy). Feature panels show a short "unavailable" or error message, or hide
  themselves. Nothing retries faster than its normal poll interval.
- **Missing field.** Almost every DTO uses `#[serde(default)]` or `Option<T>`,
  so an absent field reads as empty/false/`None`. Required fields are few:
  `LayoutConfig.version`, `connection_tab`, `actions_tab`, `data_tab`,
  `state_tab`; `TelemetryRow.timestamp_ms`, `data_type`, `values`;
  `SessionStatus` needs nothing (all default).
- **Unknown field or unknown WebSocket `ty`.** Ignored.
- **Fail-closed areas.** Safety-relevant features treat missing or malformed
  data as "not allowed": action buttons without a policy entry, station mode
  switching, board recovery (unknown board sets are refused), proximity lock,
  ground-test tickets (IGT), firmware flashing. These show a disabled control
  with a reason instead of guessing.
- **Layout invalid.** `/api/layout` that fails `LayoutConfig::validate` shows
  "The layout file is not valid for this frontend version" and keeps the
  cached layout if one exists.
- **401.** The auth layer clears the stored session for that station and
  returns to sign-in (`src/auth.rs`).

On-failure codes used in the tables below:

| Code | Meaning |
| --- | --- |
| K | Error ignored; previous value kept; retried on next poll/reconnect |
| M | Error text shown in the panel; nothing applied |
| H | Panel or control hidden / shows "unavailable" |
| C | Fail closed: control disabled with a reason |
| P | Connection test reports the route as failed (native connect screen) |

---

## 2. Minimum backend for a working UI

The smallest backend that gives sign-in, a laid-out dashboard, live telemetry
and the board list:

| Need | Routes / messages |
| --- | --- |
| Session | `GET /api/auth/session` (anonymous allowed: `{"authenticated":true,"anonymous":true,"permissions":{"view_data":true}}`) |
| Sign-in (optional if anonymous is enough) | `POST /api/auth/challenge`, `POST /api/auth/login`, `POST /api/auth/logout` |
| Layout | `GET /api/layout` |
| History seed | `GET /api/recent` (may return `[]`), `GET /api/system/time` (optional, aligns clocks) |
| Live data | `GET /ws` upgrade; send `{"ty":"TelemetryBatch","data":[TelemetryRow...]}` |
| State | `GET /flightstate` and WS `FlightState` |
| Boards | `GET /api/boards` and WS `BoardStatus` |
| Map (if the layout shows it) | `GET /api/map_config`, `GET /tiles/{z}/{x}/{y}.jpg` (404 is fine) |
| Buttons (if the layout has actions) | `GET /api/action_policy` and WS `ActionPolicy`; commands arrive on `/ws` as `{"cmd":"<Name>"}` |

Everything else is optional; missing routes degrade as described in 1.3.
[backend-api.md, "Compatibility Levels"](backend-api.md#compatibility-levels)
has the older required/recommended split; GroundStation26
`docs/frontend/api.md` ("Startup Flow") describes the same from the backend
side.

Worked sequence (browser, signed-in operator):

1. Page loads from `https://gs.example`; base URL is the origin.
2. `GET /api/auth/session` with the saved bearer token. 401 or
   `authenticated:false` shows the login form.
3. Login: `POST /api/auth/challenge {"username":"op"}` returns
   `{challenge_id, algorithm:"pbkdf2_sha256", iterations, salt_b64, server_nonce_b64}`.
   The client derives `PBKDF2-SHA256(password, salt, iterations)` and sends
   `POST /api/auth/login {username, challenge_id, client_nonce_b64, proof_b64, remember_me}`,
   where `proof_b64` is HMAC-SHA256 over the JSON proof payload (base64url,
   no padding). The reply `{token, session}` is stored per station.
4. `GET /api/layout` (cached locally per station), `GET /api/action_policy`,
   `GET /flightstate`, `GET /api/calibration_config`.
5. Connect `wss://gs.example/ws?token=<token>`.
6. Seed: `GET /api/system/time`, `GET /api/recent?display=true&max_rows=12000`,
   `GET /api/alerts?minutes=20`, `/api/notifications`, `/api/messages`,
   `/api/action_policy`, `/api/fill_targets`, `/api/launch_clock`,
   `/api/boards`, `/api/network_topology`, `/api/recording_status`,
   `/api/network_time` (each optional).
7. Live: `TelemetryBatch` frames update charts and cards; snapshot messages
   replace their state.
8. Operator presses a button: UI checks `permissions.send_commands` /
   `allowed_commands` and the action-policy entry, then sends
   `{"cmd":"Dump"}` on `/ws`.
9. No WebSocket traffic for 15 s: the UI reconnects and repeats step 6.

---

## 3. Auth, session, discovery and local persistence

### 3.1 Session and roles

`GET /api/auth/session` returns `SessionStatus` (`src/auth.rs`); example:
[api-examples/auth-session.anonymous.json](api-examples/auth-session.anonymous.json),
schema: [backend-api.md](backend-api.md#get-apiauthsession).

| Field | Type | Used for |
| --- | --- | --- |
| `authenticated`, `anonymous` | bool | Sign-in gate; anonymous sessions never get camera/voice |
| `username` | string? | Header, audit text |
| `roles` | string[] | `admin` grants every permission client-side; `stream_master`, `stream_admin`, `stream_viewer` gate broadcast tools |
| `permissions` | object of bool: `view_data`, `send_commands`, `set_system_time`, `voice_transmit`, `camera_publish`, `sign_checklists` | Tab and control gating (unknown flags default to false) |
| `allowed_commands` | string[] | Empty = all commands; otherwise per-command allow list (`NitrogenClose`/`NitrousClose` are checked as `Nitrogen`/`Nitrous`) |
| `can_view_calibration`, `can_edit_calibration` | bool? | Calibration page and load-cell settings |
| `session_type`, `expires_at_ms` | string?, i64? | Display only |
| `station_name`, `station_role` (`primary`/`rrls`) | string? | RRLS behaviour (command proxy, labels) |
| `station_pairing` | object? | Paired failover discovery key (`installation_id`, `station_id`, `verify_key`, `peers[]`) |
| `operating_mode` | any | Shown in RRLS mode labels |

Client gates (`src/auth.rs`): `can_view_data`, `can_view_actions`
(`send_commands`), `can_send_command(cmd)`, `can_view_calibration`,
`can_edit_calibration`, `can_manage_stream`, `can_publish_camera`,
`can_join_voice`, `is_admin`. The Settings "Administration" section and the
administration console (users, pairing, physical panel, sequence access, stream
tokens, host power) are shown to `admin` only. These are presentation gates;
the backend must enforce the same rules (it returns 401/403, shown as M).

On an RRLS (`station_role:"rrls"`), commands other than `ResetTClock` and
`RadioModemCheck` are forwarded to the primary through `POST /api/command`
when `GET /api/rrls/control` reports the proxy `available` and the command in
`supported_commands`; `takeover_commands` are sent locally.
See GroundStation26 `docs/backend/rrls.md`.

### 3.2 Connection setup (native)

The native connect screen (`src/app_connect_page.rs`, `src/app_connection.rs`)
runs a connection test against: `GET /api/auth/session` (200),
`POST /api/auth/challenge`, `POST /api/auth/login` (200/400/401/403/415 all
count as reachable), `POST /api/auth/logout`, `GET /api/recent`,
`GET /api/alerts`, `GET /api/layout`, `GET /api/map_config`, `GET /flightstate`,
`GET /api/gps` (200), `GET /tiles` (200/403/404), `GET /ws` (101/400/426).
LAN discovery uses UDP `GS26-FIND1` on port 49363 (hints only, no trust).

### 3.3 What is stored locally

Browser: `localStorage` of the origin. Native: `storage.json` and
`auth_session.json` under the app data directory
([native-persistence.md](native-persistence.md)).

| Data | Key(s) | Survives disconnect | Notes |
| --- | --- | --- | --- |
| Station address | `gs_base_url`, `gs_skip_tls_verify_*` | yes | Native only |
| Session token | `auth_session_v1` (per host scope, max 20) | yes, if `remember_me` | Live authority (command proxy) is never restored from cache |
| Layout | `gs_layout_cache_v10_<base>` | yes | Shown immediately, replaced by `/api/layout` |
| Calibration layout and file | `gs_calibration_layout_v2_*`, `gs_calibration_file_v2_*`, drafts `gs_calibration_draft_v2_*` | yes | Re-read on open; drafts are client-only |
| Map config, view, tiles | `gs26_ground_map_config_v1`, `gs26_ground_map_state_v3`, tile cache (`gs26_tile_cache_enabled`, budget `gs_cache_budget_mb`) | yes | Map works offline from cached tiles |
| Telemetry rows | `gs_telemetry_rows_cache_v1` (opt-in `gs_data_cache_enabled`) | yes | Discarded on reseed; history comes from `/api/recent` |
| Alert acks, dismissed notifications, dismissed board aborts | `gs_last_warning_ack_ts`, `gs_last_error_ack_ts`, `gs_notification_dismissed_ids_v1`, `gs_dismissed_board_aborts` | yes | Shared acks come from `AlertAckState` when enabled |
| UI preferences | theme, language, tabs, units, chart options, personal dashboard, watch settings (`watch_settings_v1`), proximity prefs | yes | Watch settings also sync via `/api/account/client-settings/watch` |

Nothing persisted locally grants command authority. After a disconnect the UI
keeps showing the last values with a stale/disconnected banner, reconnects
automatically, and replaces history with a fresh `/api/recent` seed.
[offline-preview.md](offline-preview.md) describes the disconnected native
preview.

---

## 4. WebSocket channels

### 4.1 Main channel `GET /ws`

URL: `{base_ws}/ws?token=<token>` (token omitted when signed out). Handler:
`handle_ws_message` and `try_handle_ws_telemetry_message_fast` in
`src/telemetry_dashboard/dashboard_transport.rs`; enum `WsInMsg` in
`src/telemetry_dashboard/dashboard_messages.rs`. Frames are JSON
`{"ty": "<Variant>", "data": <payload>}`; unit variants have no `data`.
Examples: [api-examples/websocket-messages.json](api-examples/websocket-messages.json).

Inbound (backend to UI), every variant the UI handles:

| `ty` | Payload | UI effect |
| --- | --- | --- |
| `Telemetry` | one `TelemetryRow` | Fast path; queued for charts/cards (backend currently sends only batches) |
| `TelemetryBatch` | `TelemetryRow[]` | Fast path; main live data. Row: `timestamp_ms` i64, `received_timestamp_ms` i64?, `source_timestamp_ms` i64?, `data_type` string, `sender_id` string?, `values` (f32 or null)[], `over_range`/`over_range_low` u32 bitmasks? ([backend-recent-streaming.md](backend-recent-streaming.md#telemetryrow-shape)) |
| `FlightState` | `{state, ...StateDetail}` (fill sub-phase, FC phase flattened) | Header state, mission phase |
| `MissionPhase` | `MissionPhaseView` | Mission-phase strip |
| `FlightMaxima` | `FlightMaximaView` | Max altitude/velocity readouts |
| `FcEstimator` | `FcEstimatorView` | Estimator notice |
| `IgtStatus` | `igt::IgtStatus` (same as `GET /api/igt`) | Ground-test ticket panel |
| `ProximityLock` | `proximity_lock::Status` (same as `GET /api/proximity`) | Proximity lock banner |
| `LaunchClock` | same as `GET /api/launch_clock` | T-clock |
| `Warning`, `Error` | `{timestamp_ms, message, condition?}` (`condition` flattened `ConditionLink`) | Alert lists, badges |
| `AlertAckState` | `{warning_ack_timestamp_ms, error_ack_timestamp_ms, history_cleared_through_ms?}` | Shared acknowledgements |
| `AlertConditions` | `alert_conditions::Snapshot` | Active-condition panel; an undecodable frame invalidates the view |
| `BoardStatus` | `{boards: BoardStatusEntry[]}` (same as `GET /api/boards`) | Connection tab, board freshness |
| `BoardAbort` | `BoardAbortSnapshot` (same as `GET /api/board_abort`) | Board abort banners, launch gating |
| `NetworkTopology` | same as `GET /api/network_topology` | Topology tab |
| `Notifications` | `PersistentNotification[]` | Notification centre history (never pops up) |
| `Notification` | `PersistentNotification` | One live popup (5 s), dedupe by `(id, timestamp_ms)` |
| `Messages` | `PersistentNotification[]` | Messages tab |
| `ActionPolicy` | same as `GET /api/action_policy` | Button enable/blink/reason |
| `FillTargets` | same as `GET /api/fill_targets` | Fill panel |
| `RecordingStatus` | same as `GET /api/recording_status` | Recording indicator |
| `NetworkTime` | same as `GET /api/network_time` | Network clock offset |
| `StationModeChanged` | `{mode?, label?, policy_epoch?, layout_revision?}` | Re-read `/api/layout` and mode status; keep session |
| `DashboardReset`, `CalibrationChanged` | none | Clear chart history and reseed from `/api/recent` |

Outbound (UI to backend): commands only, as text frames
`{"cmd":"<CommandName>", "policy_epoch"?: "<epoch>", "control_origin"?: "<origin>"}`
(`CommandOwner::wire_message` in `command_owner.rs`). Command names come from
`layout.actions_tab` / `action_policy.controls[].cmd`; the UI hard-codes a few
(section 7). The UI sends no subscriptions, pings or acks on `/ws`; native
builds answer WebSocket Ping frames.

Reconnect and idle behaviour:

- No inbound frame for more than 15 s: close, reconnect, reseed
  (`ws_activity_expired`). The backend should send at least one frame
  (e.g. `NetworkTime` or a telemetry batch) every few seconds.
- Reconnect delay 250 ms when the page is visible, 1.5 s when hidden.
- Native connect timeout 8 s (`GS_WS_CONNECT_TIMEOUT_SECS`, 3-30).
- After reconnect: flight state, layout and action policy are re-read, then
  the seed list in section 2 step 6.
- Frames from a superseded connection, station or sign-in are dropped
  (`incoming_epoch.rs`).

### 4.2 Raw stream feed

`GET /api/stream/raw/ws?token=<stream token>&types=<glob,...>&senders=<id,...>&max_hz=<n>`
(and the NDJSON twin `GET /api/stream/raw`). The UI only builds these URLs for
copy/paste and lists the catalog and tokens; it does not consume the feed
itself (`src/telemetry_dashboard/raw_streams_tab.rs`). See
[live-diagrams.md, "Raw streams tab"](live-diagrams.md#raw-streams-tab) and
GroundStation26 `docs/backend/raw-stream-feed.md`.

### 4.3 Crew voice

`GET /api/voice/ws` is opened by the phone's watch voice relay
(`src/telemetry_dashboard/watch_companion/voice_relay.rs`: first text frame is
an auth message with a voice grant token and codec, then binary audio frames)
and by the watch apps directly (`watch/apple/GS26Watch/VoiceSession.swift`,
`watch/wear/core/.../Voice.kt`). The in-app voice tab embeds the station page
`/radio?embedded=1`, which opens its own socket. See
[watch-companion.md, "Voice"](watch-companion.md#voice) and GroundStation26
`docs/backend/crew-voice.md`.

---

## 5. Route reference

Columns: Method; Path (variable segments as `{name}`, query parameters listed
separately); Body / response; Used by; Permission (what the UI checks; the
backend enforces); On failure (codes in 1.3). "Detail" links point to the
schema. Where the path is built at run time, the variable segment is shown as
`{name}`.

### 5.1 Session, discovery and connection

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| GET | `/api/auth/session` | → `SessionStatus` ([detail](backend-api.md#get-apiauthsession)) | `auth.rs`, `native_station.rs` (handover check), connect test | none | sign-in screen; P |
| POST | `/api/auth/challenge` | `{username}` → `{challenge_id, algorithm, iterations, salt_b64, server_nonce_b64}` | `auth.rs` | none | M |
| POST | `/api/auth/login` | `{username, challenge_id, client_nonce_b64, proof_b64, remember_me}` → `{token, session}` ([detail](backend-api.md#post-apiauthlogin)) | `auth.rs` | none | M |
| POST | `/api/auth/logout` | empty → any | `auth.rs` | session | local session cleared anyway |
| GET | `/api/station/discovery` | query `nonce` → signed station identity (Ed25519 over nonce) | `native_station.rs`, `static/station_discovery.js` (paired failover) | none | candidate ignored |
| GET | `/api/gps` | → rocket GPS seed or null ([detail](backend-api.md#get-apigps)) | connect test only | view_data | P |
| GET | `/tiles` | probe only (any of 200/403/404) | connect test | none | P |

### 5.2 Live telemetry, history, alerts and notifications

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| GET | `/ws` | WebSocket, section 4.1 | `dashboard_transport.rs` | view_data | reconnect loop |
| GET | `/api/recent` | query `display=true`, `max_rows` → `TelemetryRow[]` (may stream) ([detail](backend-api.md#get-apirecent), [streaming](backend-recent-streaming.md)) | reseed (`dashboard_transport.rs`), connect test | view_data | K (keeps existing history) |
| GET | `/api/system/time` | → `{utc_ms, ...ClockStatus}` | reseed clock alignment; Settings → System clock | view_data | K / M |
| POST | `/api/system/time` | `{utc_ms, ...}` → `ClockStatus` | Settings → System clock (`recording_download.rs`) | `set_system_time` | M |
| GET | `/api/alerts` | query `minutes` → `[{timestamp_ms, severity:"warning"\|"error", message, condition?}]` ([detail](backend-api.md#get-apialerts)) | seed, connect test | view_data | K |
| POST | `/api/alerts/ack` | `{warning_timestamp_ms, error_timestamp_ms}` → `AlertAckStateMsg` ([detail](backend-api.md#post-apialertsack)) | alert badges | view_data | local ack only |
| POST | `/api/alerts/history/clear` | `{confirm:true}` → `{history_cleared_through_ms, ...}` | Errors tab | admin or operator per backend | M |
| GET | `/api/notifications` | → `PersistentNotification[]` ([detail](backend-api.md#get-apinotifications)) | seed | view_data | K |
| POST | `/api/notifications/{id}/dismiss` | empty → 2xx ([detail](backend-api.md#post-apinotificationsiddismiss)) | notification centre | view_data | local dismiss kept |
| GET | `/api/messages` | → `PersistentNotification[]` ([detail](backend-api.md#get-apimessages)) | seed | view_data | K |
| GET | `/api/boards` | → `{boards:[{board, board_label, sender_id, seen, packet_count, last_seen_ms?, age_ms?}]}` ([detail](backend-api.md#get-apiboards)) | seed, Connection tab | view_data | K |
| GET | `/api/network_topology` | → topology ([detail](backend-api.md#get-apinetwork_topology)) | seed, Network tab | view_data | K |
| GET | `/api/network_time` | → `NetworkTimeMsg` ([detail](backend-api.md#get-apinetwork_time)) | seed | view_data | K |
| POST | `/api/network_time` | `{source:"client", utc_ms}` → any | Settings → Station clocks (`actions_tab.rs`) | send_commands | M |
| GET | `/api/recording_status` | → `RecordingStatusMsg` | seed | view_data | K |
| GET | `/api/i18n/catalog` | query `lang` → `{lang, translations:{en:text}}` ([detail](backend-api.md#get-apii18ncataloglangcode)) | language switch | none | English shown |
| POST | `/api/i18n/translate` | `{target_lang, texts[]}` → `{translations}` ([detail](backend-api.md#post-apii18ntranslate)) | language switch | none | English shown |
| GET | `/api/stream/raw/catalog` | → streams seen with rates | Raw streams tab | view_data | M |
| GET | `/api/stream/raw` | query `token`, `types`, `senders`, `max_hz`; NDJSON | URL shown for copy only | stream token | n/a |
| GET | `/api/stream/raw/ws` | same query; WebSocket | URL shown for copy only | stream token | n/a |
| GET | `/api/admin/stream-tokens` | → `{tokens:[{id, scope, issuer?, expires_at_ms?, last_used_ms?}]}` | Raw streams tab | admin | M |
| POST | `/api/admin/stream-tokens` | `IssueRequest` → issued token (shown once) | Raw streams tab | admin | M |
| POST | `/api/admin/stream-tokens/{id}/revoke` | `{}` → `{revoked}` | Raw streams tab | admin | M |

### 5.3 Layout, configuration, map and positions

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| GET | `/api/layout` | → `LayoutConfig` ([detail](backend-api.md#get-apilayout), examples [full](api-examples/layout.full.json) / [minimal](api-examples/layout.minimal.json)) | dashboard, mode switch, vehicle export | view_data | cached layout or error banner |
| GET | `/api/diagrams` | → `{diagrams:[topology], errors:[{file, error}]}` | Live diagrams tab ([live-diagrams.md](live-diagrams.md)) | view_data | H |
| GET | `/api/vehicle_visualization` | → `VehicleVisualizationConfig` ([detail](backend-api.md#get-apivehicle_visualization), [example](api-examples/vehicle-visualization.json)) | Vehicle tab, replay export | view_data | H |
| GET | `/api/map_config` | → `{max_native_zoom, max_display_zoom, default_center_lat, default_center_lon, default_zoom, map_title, tracked_asset_label}` ([detail](backend-api.md#get-apimap_config), [example](api-examples/map-config.json)) | Map tab, watch map sync, connect test | view_data | cached config; watch plans z15 |
| GET | `/tiles/{z}/{x}/{y}.jpg` | JPEG tile ([detail](backend-api.md#get-tileszxyjpg)) | map (browser direct; native via local `gs26://` / `gs26.localhost` proxy in `main.rs`), watch direct | view_data / watch token | blank tile |
| GET | `/api/station_positions` | → `{ground_station, rrls, ...}` document | Admin → Station positions | admin | M |
| POST | `/api/station_positions` | `Update` → document | Admin → Station positions | admin | M |
| GET | `/api/antenna/status` | → `{ground_station?: StationFix, rrls?: StationFix, ...rssi}`; `StationFix` = `{latitude, longitude, altitude_m?, altitude_reference?, accuracy_m?, age_ms, stale, source, manual}` | Map markers, antenna aiming, live diagrams, custom dashboard, watch | view_data | K |
| GET | `/api/fc/launch-site` | → `{configured?: Site, compiled_default?: Site, fc_reported?: Site, confirmed, pending}`; `Site` = `{lat_deg, lon_deg, alt_msl_m}` | Settings → Mission → Launch site | view_data | M |
| PUT | `/api/fc/launch-site` | `Site` request → any | same | send_commands | M |

GroundStation26 detail: `docs/backend/station-positions.md`,
`docs/backend/offline-maps.md`, `docs/backend/video-and-models.md`.

### 5.4 Flight state, launch clock, sequences and GSE

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| GET | `/flightstate` | → `{state, ...detail}` ([detail](backend-api.md#get-flightstate)) | header, mission phase, connect test | view_data | K |
| GET | `/api/launch_clock` | → `LaunchClockMsg` ([detail](backend-api.md#get-apilaunch_clock)) | seed | view_data | K |
| GET | `/api/flight_setup` | → `FlightSetupConfig` ([detail](backend-api.md#get-apiflight_setup)) | Settings → Mission | send_commands | M |
| POST | `/api/flight_setup` | `FlightSetupConfig` → same ([detail](backend-api.md#post-apiflight_setup)) | same | send_commands | M |
| POST | `/api/flight_setup/apply` | `{}` → `FlightSetupApplyResponse` ([detail](backend-api.md#post-apiflight_setupapply)) | same | send_commands | M |
| GET | `/api/fill_targets` | → `FillTargetsConfig` ([detail](backend-api.md#get-apifill_targets), [example](api-examples/fill-targets.json)) | seed, Data tab, Loadcells settings | view_data | K |
| POST | `/api/fill_targets` | `FillTargetsConfig` → same ([detail](backend-api.md#post-apifill_targets)) | Settings | send_commands | M |
| GET | `/api/auto_zero` | → `{enabled?, before_launch?}` | Settings → Mission | send_commands | option shown off |
| POST | `/api/auto_zero` | `{enabled}` or `{before_launch}` → same | same | send_commands | M |
| POST | `/api/auto_zero/launch/cancel` | empty → 2xx | launch zero notice | `Launch` allowed | M |
| GET | `/api/gse/status` | → GSE status (sequences, pressures, qualification) | GSE panel, sequence progress, settings | view_data | H |
| GET | `/api/gse/config` | → GSE settings | Settings → Sequences (`gse_config_compatibility.rs`) | send_commands | M |
| POST | `/api/gse/config` | settings → saved settings; unknown keys are not sent (feature-detected) | same | send_commands | M |
| POST | `/api/gse/abort/clear` | `{}` → any | GSE panel, Actions | send_commands | M |
| POST | `/api/gse/self-test-confirmation` | `{confirmed}` → any | isolation checkbox (`actions_tab.rs`) | send_commands | M |
| POST | `/api/gse/cold-flow/start` | `{}` → any | cold flow, sequence host | send_commands | M |
| POST | `/api/gse/cold-flow/release` | run intent → any | sequence host | send_commands | M |
| POST | `/api/gse/cold-flow/override` | `{reason}` → any | GSE panel | send_commands | M |
| POST | `/api/gse/qualification/reset` | `{}` → any | GSE panel | send_commands | M |
| POST | `/api/gse/overpressure/acknowledge` | `{}` → any | GSE panel | send_commands | M |
| POST | `/api/gse/sequences/pause` | `RunKey` → any | sequence progress | send_commands | M |
| POST | `/api/gse/sequences/resume` | `RunKey` → any | sequence progress | send_commands | M |
| POST | `/api/gse/sequences/cancel` | `RunKey` → any | sequence progress | send_commands | M |
| POST | `/api/gse/sequences/dump` | `RunKey` → any | sequence progress | send_commands | M |
| GET | `/api/gse/sequences/results/{backend_instance}/{reset_epoch}/{run_id}` | → result snapshot | sequence progress | view_data | M |
| GET | `/api/safety/about` | → safety catalog | Settings → Safety | view_data | H |
| GET | `/api/igt` | → `IgtStatus` | ground-test ticket panel | view_data | C |
| POST | `/api/igt/prepare` | `{action}` → `Prepared` (challenge) | same | send_commands | M |
| POST | `/api/igt/confirm` | confirm request (typed text) → any | same | send_commands | M |
| PUT | `/api/igt/settings` | settings update → any | same | admin per backend | M |
| GET | `/api/checklists` | → `Document` (revision, checklists, items) | Checklists ([checklist-signoff.md](checklist-signoff.md)) | view_data | M |
| POST | `/api/checklists` | `Document` (edit) → `Document` | Checklists editor | admin per backend | M |
| POST | `/api/checklists/check` | `Check` → `Document` | Checklists | view_data | M |
| POST | `/api/checklists/sign` | `{checklist_id, revision}` → `Document` | Checklists | `sign_checklists` | M |
| POST | `/api/checklists/resolve` | `Resolve` → `Document` | paired conflict resolve | view_data | M |
| GET | `/api/checklists/sync` | → `SyncStatus` | Checklists | view_data | K |
| GET | `/api/checklists/export` | JSON file | Recordings → exports | view_data | M |
| GET | `/api/checklists/export.csv` | CSV file | Recordings → exports | view_data | M |

GroundStation26 detail: `docs/backend/gse-sequences.md`,
`docs/backend/checklists.md`, `docs/backend/checklist-signoff.md`.

### 5.5 Commands, action policy and RRLS

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| (WS) | `/ws` | `{"cmd":...}` frames, section 4.1 | every action button | `can_send_command` + policy | button feedback times out |
| POST | `/api/command` | JSON string body, e.g. `"Turbocharge"`; headers `X-Station-Policy-Epoch`, `X-Station-Control-Origin` when fenced → `CommandProxyStatus` (RRLS) or 2xx | RRLS primary proxy (`command_owner.rs`), GSE Turbocharge (`gse_panel.rs`), watch direct link | send_commands | M |
| GET | `/api/action_policy` | → `ActionPolicyMsg` ([detail](backend-api.md#get-apiaction_policy), [example](api-examples/action-policy.json)) | dashboard, mode layout, seed | view_data | C (controls without policy stay disabled) |
| GET | `/api/rrls/control` | → `CommandProxyStatus` `{supported, paired, connected, available, supported_commands[], takeover, takeover_commands[], pending?, last?, reason}` | RRLS command routing | session | proxy treated unavailable |
| GET | `/api/rrls/status` | → `{enabled, role?, forwarding, relay_requested}` | recovery status strip | view_data | H |
| GET | `/api/board_abort` | → `BoardAbortSnapshot` | board abort readiness | view_data | K |
| POST | `/api/board_abort/refresh` | `{}` → reply | same | send_commands | M |
| GET | `/api/watch/actions` | → `ActionCatalog` (commands the account may put on a watch) | watch companion settings | session | watch actions empty |

The command vocabulary is backend-defined (`layout.actions_tab`,
`action_policy.controls`). GroundStation26 `docs/frontend/api.md`
("Command Submission") describes the backend side.

### 5.6 Boards, network health, recovery, SD and bench tools

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| GET | `/api/board-recovery/status` | → recovery `Status` (`boards[]`, permissions, mode capability, operation, confirmation) | Connectivity recovery ([board-recovery-ui.md](board-recovery-ui.md)) | view_data | C |
| POST | `/api/board-recovery/prepare` | prepare request → challenge | same | send_commands | M |
| POST | `/api/board-recovery/confirm` | confirmation → `Accepted` | same | send_commands | M |
| POST | `/api/board-recovery/query` | `{board}` → `Accepted` | same; plumbing drive (`{"board":"AB"}`) | send_commands | M |
| POST | `/api/board-recovery/finish-without-absent` | body → any | same | send_commands | M |
| POST | `/api/board-recovery/recover-station` | `{confirm:true}` → any | station recover | send_commands | M |
| GET | `/api/board-config/status` | → sync status | board config sync banner | view_data | H |
| POST | `/api/board-config/resolve` | `{item, keep}` → any | same | send_commands | M |
| GET | `/api/actuator/plumbing/status` | → plumbing drive status | plumbing drive panel | view_data | H |
| POST | `/api/actuator/plumbing/query` | `{}` → any | same | send_commands | K |
| POST | `/api/actuator/plumbing/settings` | settings → any (epoch headers) | same | send_commands | M |
| POST | `/api/actuator/plumbing/retraction-history/inspect` | `{}` → `Inspection` | retraction history | send_commands | M |
| POST | `/api/actuator/plumbing/retraction-history/reset` | body → any | same | send_commands | M |
| POST | `/api/network-health/daq/sd/retry` | `{}` → any | network health (DAQ storage) | send_commands | M |
| GET | `/api/radio/modem` | → `ModemStatus` | radio modem health | view_data | H |
| POST | `/api/radio/modem/check` | `{mode}` → `CheckReply` | same | send_commands | M |
| GET | `/api/radio/reject-test` | → `RadioTestStatus` | radio reject test | view_data | H |
| POST | `/api/radio/reject-test/start` | `{variant, duration_s, confirm:true}` → any | same | send_commands | M |
| POST | `/api/radio/reject-test/stop` | `{}` → any | same | send_commands | M |
| GET | `/api/sd_download` | → `{boards[], allowed, blocked_reason?, listings[], jobs[], local[]}` | SD download panel (`sd_board_download.rs`) | view_data | H |
| POST | `/api/sd_download/{board}/list` | empty → any (`board` lower-case) | same | send_commands | M |
| POST | `/api/sd_download/{board}/start` | `{dir, name}` → any | same | send_commands | M |
| POST | `/api/sd_download/jobs/{id}/cancel` | empty → any | same | send_commands | M |
| POST | `/api/sd_download/fc/resume` | `{}` → any | recording store (FC SD collection) | admin per backend | M |
| POST | `/api/sd_download/fc/stop` | `{}` → any | same | admin per backend | M |
| GET | `/api/daq/dac` | → `DacStatus` | DAQ DAC bench panel | view_data | H |
| POST | `/api/daq/dac/config` | channel config → any | same | send_commands | M |
| POST | `/api/daq/dac/disable` | `{channel_id?}` → any | same | send_commands | M |
| POST | `/api/daq/dac/preview` | config → preview | same | send_commands | M |
| GET | `/api/daq/dac/validation` | → `Validation` | same | view_data | H |
| POST | `/api/daq/dac/validation` | `{target}` → any | same | send_commands | M |
| GET | `/api/daq/dac/sweep` | → `SweepStatus` | same | view_data | K |
| POST | `/api/daq/dac/sweep` | sweep request → any | same | send_commands | M |
| POST | `/api/daq/dac/sweep/cancel` | `{}` → any | same | send_commands | M |
| GET | `/api/firmware-sim` | → simulator status | firmware simulator panel, DAC sim patch | view_data | H |
| POST | `/api/firmware-sim/control` | control body → status | same | send_commands | M |

GroundStation26 detail: `docs/backend/board-resync.md`,
`docs/backend/sd-network-download.md`, `docs/backend/sd-transfer.md`,
`docs/backend/daq-sd-health.md`, `docs/backend/rfd-modem-check.md`,
`docs/backend/radio-reference-testing.md`.

### 5.7 Calibration

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| GET | `/api/calibration_config` | → `{capture_target_samples, fit_modes[], sensors:[{id, label, data_type, channel, fit_color?, raw_label?, expected_label?, fit_modes?, formatter?}]}` ([detail](backend-api.md#get-apicalibration_config), [example](api-examples/calibration-config.json)) | Calibration page | can_view_calibration | cached / H |
| GET | `/api/calibration` | → `CalibrationFile` ([detail](backend-api.md#get-apicalibration), [example](api-examples/calibration-file.json)) | Calibration page, Loadcells settings (MCP VREF), network health | can_view_calibration | M |
| POST | `/api/calibration` | `CalibrationSaveRequest` → `CalibrationFile` ([detail](backend-api.md#post-apicalibration)) | Calibration page | can_edit_calibration | M |
| GET | `/api/calibration/kg50_input` | → `Kg50BoardStatus` ([detail](backend-api.md#get-apicalibrationkg50_input)) | Data tab, Loadcells settings | view_data | H |
| POST | `/api/calibration/kg50_input` | `{input:"amp2"\|"p6_differential"}` → any | Loadcells settings | can_edit_calibration | M |
| POST | `/api/calibration/mcp_vref` | `{mcp_vref_v}` → any | Loadcells settings | can_edit_calibration | M |
| GET | `/api/calibration/thermal_settling` | query `sensor_id` → settling report | Calibration page | can_view_calibration | M |
| POST | `/api/calibration/capture_thermal_zero` | body → `CalibrationFile` | Calibration page | can_edit_calibration | M |
| GET | `/api/calibration/long_zero` | → long-zero status | Calibration page | can_view_calibration | K |
| POST | `/api/calibration/long_zero` | start body → status | Calibration page | can_edit_calibration | M |
| POST | `/api/calibration/long_zero/stop` | body → status | Calibration page | can_edit_calibration | M |
| POST | `/api/calibration/long_zero/apply` | body → `CalibrationFile` | Calibration page | can_edit_calibration | M |

See [temperature-compensation.md](temperature-compensation.md#acquisition-and-wire-contract)
and GroundStation26 `docs/backend/paired-calibration.md`.

### 5.8 Recordings, replay, exports, reports and flight review

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| GET | `/api/recordings` | → `Recording[]` | Recordings, vehicle replay, recorded broadcast | view_data | M |
| GET | `/api/recordings/csv` | query `start_ms`, `end_ms`; CSV | time-range export ([backend-api.md](backend-api.md#recording-downloads-all-modes)) | view_data | M |
| GET | `/api/recordings/{id}/csv` | CSV | recording export | view_data | M |
| GET | `/api/recordings/{id}/file` | database file | recording store | view_data | M |
| POST | `/api/recordings/{id}/delete` | `{}` → any | recording store | admin per backend | M |
| GET | `/api/recordings/{id}/board_sd/{board}/{file}` | file | recording store (board SD captures) | view_data | M |
| GET | `/api/recordings/store` | → `StoreView` | recording store | view_data | M |
| POST | `/api/recordings/snapshot` | body → any | recording store | send_commands | M |
| POST | `/api/recordings/settings` | `Settings` → `Settings` | recording store | admin per backend | M |
| GET | `/api/recordings/report` | query `request` (URL-encoded JSON) → report | Report studio | view_data | M |
| GET | `/api/recordings/{id}/replay` | → replay snapshot (`upper_id`, series) | diagnostics, vehicle replay | view_data | M |
| GET | `/api/recordings/{id}/replay/window` | query `upper_id`, `after_id`, `start_ms`, `end_ms`, `types` (≤32 types, ≤30 s) → rows | saved replay, vehicle replay, diagnostics | view_data | M |
| GET | `/api/recordings/{id}/replay/flight` | → `{start_ms?, end_ms?, ...}` | recorded broadcast | view_data | M |
| GET | `/api/recordings/{id}/replay/context` | → `ReplayContext` (series, mode spans) | saved replay | view_data | M |
| GET | `/api/recordings/{id}/replay/overlays` | → overlays | saved replay | view_data | K |
| GET | `/api/recordings/{id}/replay/alerts` | → alerts | saved replay | view_data | K |
| GET | `/api/recordings/{id}/replay/layout` | query `view` (`normal`, `test_fire`, `testing`, `hitl`, `integrated_ground_test`) → `LayoutConfig` | saved replay | view_data | M |
| GET | `/api/recordings/{id}/replay/trajectory` | query `upper_id` → trajectory | vehicle replay | view_data | M |
| GET | `/api/recordings/{id}/replay/export` | → portable replay file ([section 10](#10-offline-replay-files)) | vehicle replay export | view_data | M |
| GET | `/api/vehicle/geometry` | → vehicle geometry (GroundStation26 `docs/vehicle-geometry.md`) | attached to portable replay files as `vehicle_geometry` | view_data | K (file falls back to the generic rocket) |
| GET | `/api/recordings/{id}/sd/sources` | query `after_id` → `SourcePage` | SD recording import | view_data | M |
| GET | `/api/recordings/{id}/sd/sources/{sha256}` | file | SD recording import | view_data | M |
| POST | `/api/recordings/{id}/sd/imports` | `{manifest, filename}` → `{upload_id}` (`station_ui/sd-recording-upload.js`) | SD recording import | send_commands | M |
| PUT | `/api/recordings/{id}/sd/imports/{token}` | CSV body | same | send_commands | M |
| DELETE | `/api/recordings/{id}/sd/imports/{token}` | none (abort) | same | send_commands | M |
| POST | `/api/recordings/{id}/sd/imports/{token}/download` | body → result | same (network source) | send_commands | M |
| GET | `/api/recovery` | → `{recovery_marker_supported, clock_reset_request_supported, reset, clock, synchronized, clock_known?, persistence_error?, station, events[]}` | recovery marker | view_data | H |
| POST | `/api/recovery/reached` | `{launch_session}` → `Event` | recovery marker | send_commands | M |
| GET | `/api/recovery/export.{kind}` | `kind` = `csv` or `json`: `/api/recovery/export.csv`, `/api/recovery/export.json` | Recordings → exports | view_data | M |
| GET | `/api/flight_review/live` | → `LiveReview` | Flight review | view_data | M |
| GET | `/api/flight_review/recording/{id}` | → `RecordingReview` | Flight review | view_data | M |
| GET | `/api/flight_review/recording/{id}/{kind}` | `kind` = `motor_export` or `flight_export`; query `from_ms`, `to_ms`; ZIP | Flight review | view_data | M |
| GET | `/api/flight_review/recording/{id}/motor_export` | concrete form of the row above | Flight review | view_data | M |
| GET | `/api/flight_review/recording/{id}/flight_export` | concrete form of the row above | Flight review | view_data | M |
| GET | `/api/burn_reports` | → `List` | Burn reports | view_data | M |
| GET | `/api/burn_reports/{id}` | → `Report` | Burn reports | view_data | M |
| GET | `/api/fire_archive` | → `Archive` | Fire archive | view_data | M |
| PATCH | `/api/fire_archive/{id}` | patch → `Fire` | Fire archive | send_commands | M |
| GET | `/api/fire_archive/{id}/thrust` | → `Curve` | Fire archive | view_data | M |
| GET | `/api/apogee_planner` | → planner `Status` | Apogee planner | view_data | H |
| PUT | `/api/apogee_planner/settings` | settings → `Status` | same | send_commands | M |
| POST | `/api/apogee_planner/compute` | `{}` → `Plan` | same | send_commands | M |
| POST | `/api/apogee_planner/apply` | `{plan_id}` → `Applied` | same | send_commands | M |
| POST | `/api/apogee_planner/weather/fetch` | `{}` → `Weather` | same | send_commands | M |
| POST | `/api/apogee_planner/sim_environment/apply` | `{}` → any | same | send_commands | M |
| POST | `/api/apogee_planner/calibrate_flight` | `{actual_apogee_m}` → `Status` | same | send_commands | M |
| POST | `/api/apogee_planner/ork` | OpenRocket file (octet-stream) → `Status` | same | send_commands | M |
| DELETE | `/api/apogee_planner/ork` | none → `Status` | same | send_commands | M |

See [recorded-vehicle-replay.md](recorded-vehicle-replay.md) and GroundStation26
`docs/backend/data-capture.md`, `docs/backend/rolling-recording.md`.

### 5.9 Station settings, modes, system and administration

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| GET | `/api/station_mode` | → mode status (`mode`, `label`, ...) | header mode label (`station_mode.rs`) | view_data | H |
| GET | `/api/station-modes` | → mode capabilities V2 ([station-modes.md](station-modes.md), [example](api-examples/station-modes.custom.json)) | Settings → Mission | view_data | C |
| POST | `/api/station-modes/prepare` | option → challenge | same | admin per backend | M |
| POST | `/api/station-modes/confirm` | challenge answer → accepted | same | admin per backend | M |
| GET | `/api/mission-settings` | → paired settings `Status` | Settings → Network (paired settings) | view_data | M |
| GET | `/api/mission-settings/conflicts` | → `{key: Document}` | same | view_data | M |
| POST | `/api/mission-settings/resolve` | resolve request → `Resolved` | same | send_commands | M |
| GET | `/api/account/client-settings/watch` | → server watch-settings document (generic route `/api/account/client-settings/{namespace}`) | watch companion | session | local settings kept |
| POST | `/api/account/client-settings/watch` | update → document | watch companion | session | M |
| GET | `/api/system/power` | → `PowerStatus` | Admin → host power | admin | M |
| POST | `/api/system/power` | `{action, confirm:true, force?:"FORCE"}` → any | same | admin | M |
| GET | `/api/admin/users` | → `Account[]` | Admin console | admin | M |
| POST | `/api/admin/users` | `Account` → `Account` (update) | same | admin | M |
| POST | `/api/admin/users/create` | `Account` → `Account` | same | admin | M |
| POST | `/api/admin/users/delete` | `{username}` → `Deleted` | same | admin | M |
| GET | `/api/admin/physical-panel` | → `Status` | Admin → physical panel | admin | M |
| POST | `/api/admin/physical-panel` | `{panel_mode}` → `Status` | same | admin | M |
| GET | `/api/admin/physical-panel/press-test` | → `PressTest` | same | admin | M |
| GET | `/api/admin/sequence-access` | → `Status` | Admin → sequence access | admin | M |
| POST | `/api/admin/sequence-access` | `{launch_sequence_only}` → `Status` | same | admin | M |
| GET | `/api/station/pairing` | → pairing `Status` | Admin → station pairing | admin | M |
| POST | `/api/station/pairing/start` | body → any | same | admin | M |
| POST | `/api/station/pairing/confirm` | `{code}` → any | same | admin | M |
| POST | `/api/station/pairing/cancel` | `{}` → any | same | admin | M |
| POST | `/api/station/pairing/unpair` | `{}` → any | same | admin | M |

GroundStation26 detail: `docs/backend/station-modes.md`,
`docs/backend/client-settings.md`, `docs/backend/host-power.md`,
`docs/backend/system-time.md`, `docs/backend/container-accounts.md`.

### 5.10 Proximity lock

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| GET | `/api/proximity` | → `Status` (also WS `ProximityLock`) | proximity banner | view_data | C |
| POST | `/api/proximity/report` | device report → `Status` | device participation | session | retried next report |
| PUT | `/api/proximity/settings` | settings → `Status` | Settings → Safety | admin per backend | M |
| POST | `/api/proximity/override` | override request → `Status` | proximity banner | send_commands | M |
| POST | `/api/proximity/override/end` | `{}` → `Status` | same | send_commands | M |
| POST | `/api/proximity/dismiss` | `{id}` → `Status` | same | session | M |

Detail: [proximity-lock.md](proximity-lock.md).

### 5.11 Firmware update

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| GET | `/api/firmware/targets` | → `FirmwareTarget[]` (board, workflow, `ota_stream_port`, ...) | Firmware update tab | send_commands | H |
| POST | `/api/firmware/flash/{board}` | multipart/raw `.seds` artifact with filename; epoch headers → `FirmwareUpdateStatus` | same | send_commands | M |
| GET | `/api/firmware/updates/{id}` | → `FirmwareUpdateStatus` | same (poll) | send_commands | K |
| POST | `/api/firmware/updates/{id}/cancel` | empty → status | same | send_commands | M |

GroundStation26 `docs/frontend/api.md` ("Firmware OTA endpoints") and
`docs/backend/firmware-update-truth.md`.

### 5.12 Media, streams, programme and voice

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| GET | `/api/live_streams` | → `LiveStreamConfig` ([detail](backend-api.md#get-apilive_streams), [manager](api-examples/live-streams.json) / [viewer](api-examples/live-streams-viewer.json) examples) | Live stream tab, studio, custom dashboard, recorded replay | view_data | H |
| POST | `/api/live_streams/control` | `BroadcastState` → `BroadcastState` ([detail](backend-api.md#post-apilive_streamscontrol)) | studio, rundown, live stream tab | `can_manage_stream` | M |
| GET | `/api/stream-roles` | → `Account[]` ([detail](backend-api.md#get--post-apistream-roles)) | Broadcast studio | stream_admin / admin | M |
| POST | `/api/stream-roles` | `Grant` → any | same | stream_admin / admin | M |
| GET | `/api/video/transcode/hosts` | → `TranscodeHosts` | Broadcast studio | `can_manage_stream` | H |
| POST | `/api/video/transcode/hosts` | `{host_id?}` → any | same | `can_manage_stream` | M |
| GET | `/api/media-assets/recorded-vehicle/state` | → `SceneClock` | recorded replay controls | `can_manage_stream` | M |
| POST | `/api/media-assets/recorded-vehicle/control` | `BroadcastState` → `BroadcastState` | same | `can_manage_stream` | M |
| GET | `/api/voice/ws` | WebSocket, section 4.3 | watch voice relay, watch apps | `voice_transmit` | relay stops |
| GET | `/media` | station page, query `embedded=1` | Cameras tab iframe/webview (`media_tool_tab.rs`) | `can_publish_camera` or `can_manage_stream` | H |
| GET | `/radio` | station page, query `embedded=1`; native also reads its TLS certificate (`native_media.rs`) | Voice tab | `can_join_voice` | H |
| GET | `/api/media-assets/program` | station page (ticketed) | allowed in native webview (`native_navigation.rs`); URL from `/api/live_streams` | ticket | n/a |
| GET | `/api/media-assets/recorded-vehicle` | station page (ticketed) | same | ticket | n/a |
| GET | `/api/media-assets/streams/{id}` | station page (ticketed; `id` ≤ 64 chars `[A-Za-z0-9_-]`) | same | ticket | n/a |
| GET | `/assets/models/gse-site.glb` | glTF model | GSE panel scene (`gse_panel.rs`) | none | scene hidden |
| GET | `/assets/model-viewer.min.js` | script | GSE panel scene | none | scene hidden |

Native webview allow-list only (no request is made by the UI): `/program`,
`/player`, `/studio-audio`, `/recorded-vehicle`. See section 5.15.

Detail: [media-and-dashboard.md](media-and-dashboard.md),
[broadcast-studio.md](broadcast-studio.md), GroundStation26
`docs/backend/video-and-models.md`, `docs/backend/audience-studio.md`,
`docs/backend/broadcast-studio.md`, `docs/backend/crew-voice.md`.

### 5.13 Watch companion and watch direct link

The phone side (in `src/`) uses `/api/watch/actions`, `/api/auth/watch-token`,
`/api/account/client-settings/watch`, `/api/map_config`, `/api/antenna/status`
and `/api/voice/ws`. The watches talk to the station directly only in direct
mode, with a watch token issued through the phone.

| Method | Path | Body / response | Used by | Permission | On failure |
| --- | --- | --- | --- | --- | --- |
| POST | `/api/auth/watch-token` | `{device_id, commands[], voice, ...}` → `Grant` `{token, expires_at_ms, ...}` | phone (`watch_companion/mod.rs`) | session | direct link off |
| POST | `/api/auth/watch-token/revoke` | `{device_id}` → any | phone | session | ignored |
| GET | `/api/watch/status` | query `units` (`metric`/`imperial`), `stale_after_s`, `actions` (comma list) → compact status | watch direct (`watch/apple/WatchCore/.../Direct.swift`, `watch/wear/core/.../Direct.kt`) | watch token | watch shows phone route or "none" |
| POST | `/api/command` | JSON string command | watch direct | watch token (scoped commands) | accepted / refused / unknown shown |
| GET | `/tiles/{z}/{x}/{y}.jpg` | JPEG | watch compass map (`DirectLink.kt`, `Direct.swift`) | watch token | no background |

Detail: [watch-companion.md, "Direct link"](watch-companion.md#direct-link),
[wire format](watch-companion.md#wire-format-v1), GroundStation26
`docs/backend/watch-direct.md`.

### 5.14 Station pages shipped from this repository

`station_ui/` holds HTML/JS pages the GroundStation26 backend embeds and
serves (see GroundStation26 `docs/backend/browser-asset-preparation.md`).
They are not part of the Dioxus app but call the backend themselves:
`/api/auth/challenge`, `/api/auth/login`, `/api/auth/logout`,
`/api/video/streams`, `/api/video/streams/{id}/whep` (POST),
`/api/video/streams/{id}/whep/{session}` (DELETE),
`/api/video/streams/{id}/name` (POST), `/api/video/recordings` (query
`offset`, `limit`), `/api/video/publish` (POST, query `name`),
`/api/stage-models`, `/api/stage-models/{stage}/{name}` (GET/PUT),
`/api/vehicle_visualization`, `/api/live_streams`, `/api/voice/status`,
`/api/voice/ws`, `/api/program/commentary/ws`,
`/api/media-assets/program/state` and `/api/media-assets/program/audio`
(query `ticket`, `after`), `/api/media-assets/recorded-vehicle/...`, and
`/tiles/{z}/{x}/{y}.jpg`. A replacement backend that wants the media pages
must serve these as GroundStation26 does.

### 5.15 Discrepancies with the GroundStation26 router

Checked against `.route(...)` registrations in GroundStation26
`backend/src` (including constant paths in `diagrams.rs`, `raw_stream.rs`,
`radio_reject_test.rs`, `watch.rs`, `board_recovery.rs`). Every `/api` route
the UI calls is served, with the methods the UI uses. Remaining differences:

| Item | Frontend | Backend | Impact |
| --- | --- | --- | --- |
| `/program`, `/player`, `/recorded-vehicle` | In the native webview document allow-list (`src/native_navigation.rs:48`) | Not routed (programme and recorded vehicle moved to `/api/media-assets/program` and `/api/media-assets/recorded-vehicle`; `/studio-audio` and `/radio` are routed) | None; allow-list entries are dead and could be removed |
| `GET /tiles` (no tile path) | Connection-test probe | Only `/tiles/{z}/{x}/{y}` is routed, so the probe gets 404 | None; 404 is accepted as reachable |
| `GET /api/gps` | Only the native connection test | Served | Route kept only for the probe |
| `docs/backend-api.md` | Still lists `POST /api/calibration/capture_zero`, `capture_span`, `refit` and `GET /api/dashboard_status` as recommended | Served | The UI no longer calls them; treat that doc as partly stale for those rows |

Backend routes the UI does not call (not discrepancies, listed so another team
knows they are optional for this UI): `/api/alert_conditions`,
`/api/alerts/history/clears`, `/api/fc_estimator`, `/api/flight_maxima`,
`/api/mission_phase`, `/api/network_diagnostics`, `/api/dashboard_status`,
`/api/calibration/capture_zero`, `/api/calibration/capture_span`,
`/api/calibration/refit`, `/api/calibration/chamber_pt`,
`/api/loadcell_calibration/*`, `/api/firmware/updates`, `/api/sd_download/fc`,
`/api/proximity/audit`, `/api/recordings/{id}/manifest`,
`DELETE /api/recordings/{id}`, `/api/gse/cold-flow/cancel`,
`/api/fire_archive/import`, `/api/apogee_planner/sim_environment`,
`/api/media-assets/recorded-vehicle/trajectory`, `/valvestate`, `/version`.

---

## 6. Generic-UI rules

Goal: the UI stays generic and pulls vehicle data from the backend. A backend
for another vehicle should only need to supply different data on these routes.

### 6.1 Backend-configured today

| What | Source |
| --- | --- |
| Tabs, tab order, tab labels, app name, dashboard title, theme and theme presets | `/api/layout`: `main_tabs`, `branding.{app_name, dashboard_title, tab_labels}`, `theme` |
| Data tab: tabs, subtabs, channels (`data_type`, `sender_id`, index), labels, units, formatters, chart grouping, sender split | `/api/layout` `data_tab` |
| Connection/network tabs, board labels | `/api/layout` `connection_tab`, `network_tab`; `/api/boards` `board_label`; `/api/network_topology` |
| Actions: buttons, commands, labels, groups, colours | `/api/layout` `actions_tab`; enable/blink/reason from `/api/action_policy` |
| State tab content, flight-phase naming version | `/api/layout` `state_tab`, `flight_phase_version`, `test_fire_mode` |
| Battery sources and estimator thresholds | `/api/layout` `battery.{sources, estimator}` |
| Calibration channels, fit modes, labels | `/api/calibration_config` |
| Vehicle model, stages, ground systems, animations, telemetry bindings | `/api/vehicle_visualization` |
| Vehicle geometry | `GET /api/vehicle/geometry`, generated by the station from its OpenRocket design; see GroundStation26 `docs/vehicle-geometry.md` |
| Live diagrams (topology, nodes, bindings) | `/api/diagrams` |
| Map centre, zoom limits, titles, tiles | `/api/map_config`, `/tiles/...` |
| Station modes and labels, simulation pairing | `/api/station-modes`, `/api/station_mode` |
| Fill targets, flight setup, GSE sequence parameters | `/api/fill_targets`, `/api/flight_setup`, `/api/gse/config`, `/api/gse/status` |
| Camera angles, broadcast stats, programme | `/api/live_streams` |
| Watch actions | `/api/watch/actions` |
| Safety text, checklists | `/api/safety/about`, `/api/checklists` |
| Firmware targets | `/api/firmware/targets` |
| Raw stream catalog | `/api/stream/raw/catalog` |
| Translations | `/api/i18n/catalog`, `/api/i18n/translate` |
| Station identity and role | `/api/auth/session` `station_name`, `station_role` |

### 6.2 Hard-coded in the frontend (by design)

- The wire contract itself: message `ty` names, DTO field names, the
  `{"cmd":...}` command frame, permission flag names.
- Network flight states (`mission_phase::NETWORK_STATES`: `Startup`, `Idle`,
  `Fill`, `Armed`, `Launch`, `Landed`, `Aborted`, `InFlight`) and the mapping
  of retired detailed names. These are part of the SEDSnet contract; another
  vehicle reporting different states falls back to `Idle`.
- `Abort` is always shown when permitted (safety rule).
- Units conversion helpers and number formatting.
- Hardware-specific feature panels (GSE sequences, DAQ DAC, load-cell input
  selector, MCP3564R reference, igniter sense, plumbing drive, auto vent)
  appear only when their routes or data exist, but their wording and channel
  names are SEDS-specific (section 7).

Rule for new UI work: read names, labels, units, limits and channel lists from
an existing route; if none exists, add an optional field to the nearest
backend document (with `#[serde(default)]` on the UI side) and keep the
current value only as the fallback.

---

## 7. Hard-coding audit

Findings from a source search for board ids, data-type names, command names,
thresholds, geometry, diagram contents, branding, coordinates and hosts (test
modules excluded). Severity: **Blocks** = another vehicle gets wrong or missing
behaviour; **Partial** = a feature degrades for another vehicle; **Cosmetic** =
wording or sample data only. **Small-safe** marks fixes that use data the
backend already sends, with a fallback to the current value.

### 7.1 Board ids and names

| # | Location | Hard-coded | Backend already supplies? | Severity | Recommended fix |
| --- | --- | --- | --- | --- | --- |
| B1 | `src/telemetry_dashboard/board_recovery_model.rs:23-31` (`BOARD_IDS`, `canonical_targets`), `:479`, `:786-790` | Board set `RF FC PB AB VB GB DAQ`; targets outside this set are refused; reset phrase built from it | Yes: `/api/board-recovery/status` `boards[].id` (ordered) | Blocks (board recovery refuses other board sets) | Derive the canonical list and phrase from `status.boards[].id`; keep the constant as fallback |
| B2 | `src/telemetry_dashboard/board_recovery_model.rs:3-18` | Recovery button labels per board (`AB`/`VB` "Recover to Idle", `FC` "Recover to Startup", others "Reset metadata") | No | Partial (other boards get no label) | New field `boards[].recovery_label` / `recovery_description` in `/api/board-recovery/status` |
| B3 | `src/telemetry_dashboard/board_recovery_panel.rs:238`, `:512`, `:642` | `FC` startup warning; `RF FC PB` = "AV bay" boards whose queries pause | No | Partial | New field `boards[].group` (e.g. `"av_bay"`) and `boards[].startup_warning` |
| B4 | `src/telemetry_dashboard/sequence_progress_model.rs:493-501` | Sequence output index → board/channel (`VB` 1,2,4; `AB` 2,4) | Partial: `/api/gse/status` carries outputs | Blocks for GSE sequence readback on another vehicle | New `outputs[].{board, channel}` in `/api/gse/status` |
| B5 | `src/telemetry_dashboard/igniter_sense_panel.rs:74-76` | Igniter sense read from sender `AB` | No | Partial | Read `sender_id` from the layout channel or add `igniter_sense.sender_id` to `/api/igt` |
| B6 | `src/telemetry_dashboard/plumbing_drive.rs:229-231` | Board query `{"board":"AB"}` after a plumbing query | No (`/api/actuator/plumbing/status` has no board field) | Partial | Add `board` to `/api/actuator/plumbing/status` and use it |
| B7 | `src/telemetry_dashboard/network_health.rs:48`, `:589`, `:600` | `DAQ` is the only node with storage actions/diagnostics | Partial: topology nodes carry diagnostics | Partial | Advertise storage actions per node in `/api/network_topology` (e.g. `nodes[].actions[]`) |
| B8 | `src/telemetry_dashboard/telemetry_runtime.rs:1890-1902`, `calibration_tab.rs:367` | Calibration readings taken only from sender `DAQ` | **Yes**: `/api/calibration_config` `sensors[].channel` | Blocks calibration on a board not named DAQ | **Small-safe**: use the sensor's configured sender/channel instead of the literal |
| B9 | `src/telemetry_dashboard/sd_board_download.rs:248` | Initial board selection `DAQ` | **Yes**: `/api/sd_download` `boards[]` | Cosmetic (wrong default until changed) | **Small-safe**: default to `boards[0]` once the status loads |
| B10 | `src/telemetry_dashboard/saved_replay.rs:25` | Rocket senders `FC RF PB FC_SD` for the "rocket"/"stand" replay tabs | No | Partial (replay tabs mis-sorted) | New `replay/context` field `rocket_senders[]` or reuse `/api/boards` with a `location` field |
| B11 | `src/telemetry_dashboard/vehicle_tab.rs:664`, `:745` | GPS sender fallback `FC` | Partial: replay `source_sender_id` | Partial | Fall back to the vehicle-visualization GPS binding (see D3) |

### 7.2 Data types and channels

| # | Location | Hard-coded | Backend already supplies? | Severity | Recommended fix |
| --- | --- | --- | --- | --- | --- |
| D1 | `src/telemetry_dashboard/calibration_tab.rs:302-304`, `:2973`, `:2985`; `calibration_capture.rs:114-116` | Load-cell behaviour (mV units, input selector, thermal zero) only for `KG1000`/`KG50` | Partial: `sensors[].formatter`, `raw_label` | Partial | New `sensors[].kind` (`"load_cell"`) and `sensors[].raw_unit` in `/api/calibration_config` |
| D2 | `src/telemetry_dashboard/data_tab.rs:240-252` | Fill-source ids `kg50`/`kg1000_absolute` mapped to `KG50`/`KG1000` subtabs | Partial: `/api/fill_targets` `fill_source` | Partial | Add `fill_sources:[{id, data_type, label}]` to `/api/fill_targets` |
| D3 | `src/telemetry_dashboard/dashboard_component.rs:4651-4663`, `vehicle_replay.rs:134`, `vehicle_tab.rs:311-316`, `watch_companion/publisher.rs:151` | GPS data types `GPS`, `GPS_DATA`, `ROCKET_GPS` (lat=0, lon=1, alt=2) | No | Blocks map tracking for a vehicle using other names | New `gps:{data_type, sender_id?, lat_index, lon_index, alt_index}` in `/api/map_config` or `/api/vehicle_visualization` |
| D4 | `src/telemetry_dashboard/data_groups.rs:20-45` | Data-type families (Fill, Power, Flight) by name | No | Cosmetic (grouping only) | Optional `family` per channel in `/api/layout` `data_tab` |
| D5 | `src/telemetry_dashboard/saved_replay.rs:261-277` | Replay tab filters by name fragments (`LOADCELL`, `KG1000`, `PRESSURE`, `VALVE`, ...) | No | Partial | Return `tab` per series in `/api/recordings/{id}/replay/context` |
| D6 | `src/telemetry_dashboard/custom_dashboard.rs:33-216` (`defaults()`) | Personal dashboard default cards: `LOADCELL_50_WEIGHT_KG`, `LOADCELL_WEIGHT_KG`, `COMBUSTION_CHAMBER_PRESSURE`, `OX_TANK_PRESSURE`, battery cards with max 16.8 V, valve cards | **Yes**: `/api/layout` `data_tab` channels, labels, units | Partial (another vehicle sees empty cards) | **Small-safe**: build defaults from the layout's data-tab channels when present; keep the list as fallback |
| D7 | `src/telemetry_dashboard/watch_companion/registry.rs:120-240` | Watch items bound to `ASCENT_STATE`, `GPS_*`, `OX_TANK_PRESSURE`, `COMBUSTION_CHAMBER_PRESSURE`, `LOADCELL_WEIGHT_KG`, `LOADCELL_FILL_PERCENT`, `BATTERY_VOLTAGE`@`PB` | Partial: GS `/api/watch/status` mirrors the registry | Blocks watch items for another vehicle | New `watch_items[]` (id, label, channels, unit) in `/api/watch/actions` or a `/api/watch/items` route |
| D8 | `src/telemetry_dashboard/vehicle_tab.rs:968-977` | `VALVE_STATE` index → label for portable replay files (Pilot, Normally-open, Dump, Igniter, Nitrogen, Nitrous, Plumbing) | **Yes**: labels are overridden from the layout's `VALVE_STATE` items when present | Partial | **Small-safe**: emit only layout-supplied mappings when the layout has a `VALVE_STATE` block; keep the table as fallback |
| D9 | `src/telemetry_dashboard/types.rs:8-13` | Legacy rename `FUEL_TANK_PRESSURE` → `OX_TANK_PRESSURE` on ingress | n/a (mirrors backend) | Cosmetic | Keep; vehicle-neutral in effect (no-op for other names) |
| D10 | `src/telemetry_dashboard/flight_review_view.rs:607-608` | Thrust chart series labelled `KG1000`/`KG50` | Partial: series come from `/api/flight_review/...` fields `kg1000_kg`, `kg50_kg` | Cosmetic | Add `series[].label` to the review response |
| D11 | `src/telemetry_dashboard/firmware_sim_panel.rs:99`, `:213` | Simulator faults `kg50_bypass`/`kg1000_bypass`; bottle labels | No | Cosmetic (simulator only) | Let `/api/firmware-sim` list available faults and inputs |

### 7.3 Commands, states and procedures

| # | Location | Hard-coded | Backend already supplies? | Severity | Recommended fix |
| --- | --- | --- | --- | --- | --- |
| C1 | `src/telemetry_dashboard/actions_tab.rs:19-31` | Always adds `ValveSelfTest`, `NitrogenTest`, `StartFill`, `PauseFill`, `CancelFill`, `Turbocharge` buttons if the layout lacks them | **Yes**: `/api/layout` `actions_tab` and `/api/action_policy` | Blocks (foreign buttons on another vehicle) | **Small-safe**: add them only when `/api/action_policy.controls` lists the command |
| C2 | `src/telemetry_dashboard/actions_tab.rs:36-43`, `src/auth.rs:361-364` | `Nitrogen`/`Nitrous` toggles send `NitrogenClose`/`NitrousClose`; permission alias | No | Partial | Optional `close_cmd` per action in `/api/layout` `actions_tab` |
| C3 | `src/auth.rs:114-118`, `:353` | Local-only commands on RRLS: `ResetTClock`, `RadioModemCheck` | Partial: `/api/rrls/control` `takeover_commands` | Partial | Add `local_commands[]` to `/api/rrls/control` |
| C4 | `src/telemetry_dashboard/dashboard_messages.rs:645` | Board abort disables `Launch`, `GroundStationLaunch`, `AdvanceFlightState`, `RewindFlightState` | Partial: `/api/action_policy` already disables them server-side | Partial | Rely on the policy only, or add `blocked_commands[]` to `BoardAbortSnapshot` |
| C5 | `src/telemetry_dashboard/board_abort_readiness.rs:123` | Interlock list and labels (`TogglePlumbingDelayInterlock` "Retraction minimum 45 seconds", ...) | Partial: each is an `action_policy.controls[]` entry with `label` | Partial | **Small-safe**: show `controls[].label` when present; keep text as fallback |
| C6 | `src/telemetry_dashboard/watch_companion/actions.rs:19-25` | Launch/pyro command deny list | Yes: GS filters the watch catalog; `watch_safe` flag | Cosmetic (defence in depth) | Keep; document as safety rule |
| C7 | `src/telemetry_dashboard/sequence_indicator.rs:36-63`, `sequence_progress_model.rs:30-50` | Sequence kinds and phase names (`NitrogenTest`, `Turbocharge`, `raising`, `settling`, ...) | Partial: `/api/gse/status` | Partial | Return `sequences[].{kind, label, phases[]}` in `/api/gse/status` |
| C8 | `src/telemetry_dashboard/stream_rundown.rs:21-27`, `mission_phase.rs:78-110` | Detailed phase list (`PreFill`, `NitrogenFill`, `NitrousFill`, ...) for rundown profiles | Partial: `/api/layout` `flight_phase_version` | Partial | Add `phases[]` to `/api/live_streams` broadcast config |
| C9 | `src/telemetry_dashboard/checklists.rs:300-303` | Auto-complete triggers `valve_self_test`, `nitrogen_test`, `fill_target` | No | Partial | Add `complete_on_options[]` to `GET /api/checklists` |
| C10 | `src/telemetry_dashboard/layout.rs:701-706` | `FillTargetFluid` enum `nitrogen`/`nitrous` (unknown fluid fails to decode) | No | Blocks fill targets for other propellants | Make fluid a string with a label from `/api/fill_targets` |

### 7.4 Thresholds, geometry and wording

| # | Location | Hard-coded | Backend already supplies? | Severity | Recommended fix |
| --- | --- | --- | --- | --- | --- |
| T1 | `src/telemetry_dashboard/gse_config_compatibility.rs:22-25` | Turbocharge deadband must be 8-16 psi (client validation) | No | Partial | Add `turbocharge_pressure_deadband_psi_range:[min,max]` to `/api/gse/config` |
| T2 | `src/telemetry_dashboard/apogee_planner.rs:75-84` | Planner defaults `tank_volume_l: 12.0`, `meop_psi: 750`, `min_fill_kg: 3.0`, `min_ullage_frac: 0.15` used when fields are missing (current ox tank is 15.854 L) | Yes: `/api/apogee_planner` settings | Partial (wrong values only if backend omits them) | **Small-safe**: show "not set" instead of a default when the backend omits a field |
| T3 | `src/telemetry_dashboard/custom_dashboard.rs:89-106` | Battery card max 16.8 V | **Yes**: `/api/layout` `battery.sources` | Cosmetic | **Small-safe**: take card max from the battery source config |
| T4 | `src/telemetry_dashboard/gse_panel.rs:335-343` | GSE scene model `/assets/models/gse-site.glb` | **Yes**: `/api/vehicle_visualization` `ground_model_url` | Partial (wrong model for another site) | **Small-safe**: use `ground_model_url`, fall back to the constant |
| T5 | `src/telemetry_dashboard/auto_vent_notice.rs:44-55` | "Valve Board opens Vent and Dump and closes Pilot" | No | Cosmetic | Text from the backend notice or `/api/safety/about` |
| T6 | `src/telemetry_dashboard/station_settings.rs:104-116` | Settings section "Loadcells" (DAQ input, MCP3564R VREF) always present | Partial: features answer 404 when absent | Cosmetic | Hide the section when `/api/calibration/kg50_input` and VREF are unavailable |
| T7 | `src/telemetry_dashboard/paired_settings.rs:140-175` | Paired-setting summaries for `Nitrous`/`Nitrogen` targets and `fill_source` names | No | Cosmetic | Add `label`/`summary` per document in `/api/mission-settings` |
| T8 | `src/telemetry_dashboard/firmware_update_tab.rs:405`, `:440`, `:488` | ".seds firmware artifact", "SEDSnet stream" wording | Partial: `/api/firmware/targets` `workflow` | Cosmetic | Add `artifact_extension`/`description` to firmware targets |

### 7.5 Branding, hosts and coordinates

| # | Location | Hard-coded | Backend already supplies? | Severity | Recommended fix |
| --- | --- | --- | --- | --- | --- |
| N1 | `src/app.rs:8`, `src/main.rs:228`, `Dioxus.toml:3`, `:13` | App name `UBSEDS GS` (window title, connect/login headings via `auth::station_app_name`) | Partial: `/api/layout` `branding.app_name` (after sign-in) | Cosmetic | Keep as build-time name; use `branding.app_name` once the layout is cached |
| N2 | `src/auth.rs` (`station_name()`) | Returned `"GroundStation"` / `"RRLS"` | **Yes**: `/api/auth/session` `station_name` | Cosmetic | **Done**: uses `station_name` from the session when non-empty, the role default otherwise |
| N3 | `src/telemetry_dashboard/dashboard_component.rs:3409` | Navigation title fallback `"GroundStation"` | **Yes**: `/api/auth/session` `station_name` | Cosmetic | **Small-safe**: fall back to `station_name` before the literal |
| N4 | `src/telemetry_dashboard/dashboard_connection.rs:373`, `:388`, `:392`; `command_owner.rs:187`; `dashboard_transport.rs:222`, `:357`; similar in `recording_download.rs`, `station_settings_request.rs`, `firmware_update_tab.rs`, `sd_board_download.rs`, `station_mode_transport.rs`, `plumbing_drive.rs`, `gse_config_compatibility.rs`, `auth.rs:988` | Default base `http://localhost:3000` when no address is saved | n/a | Cosmetic | Keep; centralise in one constant |
| N5 | `src/telemetry_dashboard/mod.rs:1284-1315` | Native tile proxy hosts `gs26.localhost`, `gs26.local`, `gs26://local` | n/a (local proxy, not a station host) | Cosmetic | Keep |
| N6 | `src/telemetry_dashboard/local_station/demo.rs` | Demo scenario data (boards, data types, valve names, battery levels) | n/a | Cosmetic | **Done**: the offline workspace no longer names any of these; the demo supplies them as a layout and samples in the station's own shapes ([section 11](#11-demo-mode-a-local-reference-station)), so it can be replaced by another vehicle's data without touching the pages |
| N7 | `src/telemetry_dashboard/dashboard_settings.rs:60-135` | Built-in Spanish strings for SEDS-specific phrases | Partial: `/api/i18n/catalog` | Cosmetic | Move to the backend catalog |

No hard-coded station IP addresses or launch-site coordinates were found in
production code; coordinates and hosts in `src/` appear only in tests,
placeholders (`layout_settings_tab.rs:1211`) and log-redaction examples.

Small-safe candidates, in suggested order: B8, C1, D6, T4, N3, B9, C5,
D8, T2, T3 (N2 and N6 are done). They change live-dashboard behaviour in files
other work is active in, so each should land as its own reviewed change.

---

## 8. Contract checks

- [backend-contract-tests.md](backend-contract-tests.md): GroundStation26
  writes real replies and WebSocket frames to
  `backend/tests/contract-fixtures/`; this repository keeps a copy in
  `tests/fixtures/backend-contract/` and decodes each with the UI's serde
  types (`src/backend_contract.rs`, `backend_contract_tests.rs`).
  `scripts/check_backend_contract_fixtures.py` compares or refreshes the copy.
- `tests/test_backend_contract_docs.py` scans the production Rust sources
  (`src/`, skipping `#[cfg(test)]` modules and `*_tests.rs`) for string
  literals naming a station route and fails when one does not match a
  backticked path in this file. It strips the scheme/host/`{base}` prefix and
  the query string and treats any `{...}` segment (including `{}`) as one
  wildcard. Run it with `python3 -m unittest tests.test_backend_contract_docs`.
  Routes built only from constants (`format!("{LIST_PATH}/...")`) are not
  seen by the scan; keep their rows by hand.
- `src/telemetry_dashboard/local_station/tests.rs` decodes every route the
  demo serves with the live dashboard's own types (section 11), and
  `offline_preview/tests.rs` opens the backend's replay-export fixture
  (`tests/fixtures/backend-contract/replay-export/flight.json`) in every mode
  with no station or cache (section 10).
- When adding a route to the UI: add a row to the relevant table and to the
  appendix, link the schema (or give a concise one here), and ask the backend
  to add a contract fixture.

---

## 9. Related documents

This repository:

- [backend-api.md](backend-api.md): per-route schemas and WebSocket payloads
  (partly stale, see 5.15).
- [api-examples/README.md](api-examples/README.md): example payloads.
- [backend-contract-tests.md](backend-contract-tests.md): fixture decoding tests.
- [backend-recent-streaming.md](backend-recent-streaming.md): `/api/recent`
  streaming and `TelemetryRow`.
- [station-modes.md](station-modes.md), [proximity-lock.md](proximity-lock.md),
  [watch-companion.md](watch-companion.md), [live-diagrams.md](live-diagrams.md),
  [station-discovery.md](station-discovery.md),
  [native-persistence.md](native-persistence.md),
  [board-recovery-ui.md](board-recovery-ui.md),
  [checklist-signoff.md](checklist-signoff.md),
  [recorded-vehicle-replay.md](recorded-vehicle-replay.md),
  [media-and-dashboard.md](media-and-dashboard.md),
  [broadcast-studio.md](broadcast-studio.md),
  [network-traffic.md](network-traffic.md),
  [native-map-and-gse.md](native-map-and-gse.md),
  [offline-preview.md](offline-preview.md),
  [temperature-compensation.md](temperature-compensation.md).

GroundStation26 (sibling repository):

- GroundStation26 `docs/frontend/api.md`: the backend's view of this contract.
- GroundStation26 `docs/shared/contracts.md`: shared wire types.
- GroundStation26 `docs/backend/contract-fixtures.md`: how fixtures are
  produced.
- GroundStation26 `docs/vehicle-geometry.md`: `GET /api/vehicle/geometry`.
- GroundStation26 `docs/backend/*.md`: feature documents named in section 5.

---

## 10. Offline replay files

A portable replay file (`*.seds-replay.json`) opens on any device with no
station connected and nothing cached: **Explore offline** on the connection
page (native) or a failed-connection card (web, native) opens the offline
workspace; **Open replay file** (or dropping the file on the window) loads it.
Every page then reads the file only: Dashboard (the file's layout
`state_tab`), Mission (its flight states and actions, read-only), Data (all
channels, charted over the whole recording), Diagrams, Network (senders and
their packet counts), Map (GPS track on a grid, no tiles), Replay & summary
(flight or burn figures and the 3D player), Recordings (file details).

Platforms: the browser and desktop use the file picker and drag and drop; iOS
and Android use the same file input, which opens the system document picker.
Opening a file from another app (share sheet / "Open in") is not registered
yet: it needs a document type (`CFBundleDocumentTypes`/UTI on iOS, an
`ACTION_VIEW` intent filter on Android) that forwards the file to the
`/preview` route.

What the file contains. The station's
`GET /api/recordings/{id}/replay/export` produces the samples and the app
attaches its station's configuration before saving
(`vehicle_tab::ReplayFileExport`):

| Field | Source | Required | Used for | Without it |
| --- | --- | --- | --- | --- |
| `format` = `"seds-replay"`, `version` = 1 | backend | yes | recognising the file | refused with an explanation; a newer version asks for an app update |
| `samples[]`: `{id, timestamp_ms, source_timestamp_ms?, sender_id, data_type, values[], phase?}` | backend | yes | every page | refused ("no samples") |
| `samples[]` with `data_type` `FLIGHT_STATE` (FC; `phase` name or `values[0]` = `FlightState` code) | backend | no | state timeline, dashboard section choice, mission steps | state shown as Unknown; the first dashboard section is used |
| `replay_context` (`recorded_mode`, `hitl`, `igt`, `test_fire`, `provenance`, …) | backend (`GET …/replay/export` handler) | no | mode (flight, test fire, ground test, HITL) and provenance label | mode from `mode`, else FC rows imply flight, otherwise test fire |
| `mode` (`flight` / `test_fire`) | app | no | 3D player mode | as above |
| `layout` | app (`GET /api/layout`) or the capture manifest | no | dashboard sections, data-tab names, units (`channel_formatters[].suffix`), actions | every recorded type is listed under its own name; dashboard shows the first channels |
| `view_config`, `valve_mappings` | app (`GET /api/vehicle_visualization`, layout) | no | 3D player tanks and valve names | generic labels |
| `boards[]`: `{sender_id, board_label}` | app (`GET /api/boards`), new | no | board names | the sender ids |
| `vehicle_geometry` | app (`GET /api/vehicle/geometry`), new | no | 3D vehicle shape | the generic rocket |
| `alerts[]`: `{timestamp_ms, message, severity}` | not written yet | no | Network → Warnings | none listed |
| `schema`, `sd_provenance`, `snapshot`, `recording_diagnostics_schema`, `configuration_provenance` | backend / app | no | diagnostics and provenance note | not shown |

Calibration is not needed: recorded values are already calibrated. Map tiles
are never fetched offline. Files are capped at 64 MiB (as the export).
GroundStation26's contract fixture `replay-export/flight.json` is the actual
export of a recorded `nominal_10k` flight plus the attached configuration.

Proposed backend follow-up (not required for the UI): write `boards`,
`vehicle_geometry` and the recording's alerts into the export itself, so a
file exported by another client (or by `curl`) is equally complete.

## 11. Demo mode: a local reference station

With no station, the offline workspace starts in demo mode (banner "Demo
mode — no station connected"). The demo is a station implemented on the
device (`src/telemetry_dashboard/local_station/`): `Session::get(path)`
answers `/api/auth/session`, `/api/layout`, `/api/boards`, `/flightstate`,
`/api/recent`, `/api/gps`, `/api/alerts`, `/api/recordings`,
`/api/calibration` and `/api/vehicle/geometry` (when a file carries it) in
the station's JSON, and the tests decode each with the live dashboard's types.
Use it as a minimal working example of these routes.

The scenario loops every 380 s: idle, pre-fill, nitrogen and nitrous fill,
armed, a 10 s countdown, the GroundStation26 IGT `nominal_10k` flight (about
10,000 ft AGL, 2 Hz) and landing, with all seven boards reporting and four
example warnings. The dashboard, data tabs and actions come from the demo's
layout, not from code in the pages. Commands get a local acknowledgement
("Nothing was sent; no station is connected"); a replay answers that it is
read-only.

Cost when a station is connected: none. The session, its 250 ms clock task
and its data exist only while the offline workspace route is mounted and are
dropped with it; the live transport (`dashboard_transport.rs`) and telemetry
path do not reference it. A debug assertion on the dashboard route and a
render test check that no session is alive after leaving the workspace.

---

## Appendix: route index

Every backend route the frontend code uses, as `METHOD path`, grouped as in
section 5. WebSocket upgrades are listed as `GET`.

5.1 Session, discovery and connection

- `GET /api/auth/session`
- `POST /api/auth/challenge`
- `POST /api/auth/login`
- `POST /api/auth/logout`
- `GET /api/station/discovery`
- `GET /api/gps`
- `GET /tiles`

5.2 Live telemetry, history, alerts and notifications

- `GET /ws`
- `GET /api/recent`
- `GET /api/system/time`
- `POST /api/system/time`
- `GET /api/alerts`
- `POST /api/alerts/ack`
- `POST /api/alerts/history/clear`
- `GET /api/notifications`
- `POST /api/notifications/{id}/dismiss`
- `GET /api/messages`
- `GET /api/boards`
- `GET /api/network_topology`
- `GET /api/network_time`
- `POST /api/network_time`
- `GET /api/recording_status`
- `GET /api/i18n/catalog`
- `POST /api/i18n/translate`
- `GET /api/stream/raw/catalog`
- `GET /api/stream/raw`
- `GET /api/stream/raw/ws`
- `GET /api/admin/stream-tokens`
- `POST /api/admin/stream-tokens`
- `POST /api/admin/stream-tokens/{id}/revoke`

5.3 Layout, configuration, map and positions

- `GET /api/layout`
- `GET /api/diagrams`
- `GET /api/vehicle_visualization`
- `GET /api/map_config`
- `GET /tiles/{z}/{x}/{y}.jpg`
- `GET /api/station_positions`
- `POST /api/station_positions`
- `GET /api/antenna/status`
- `GET /api/fc/launch-site`
- `PUT /api/fc/launch-site`

5.4 Flight state, launch clock, sequences and GSE

- `GET /flightstate`
- `GET /api/launch_clock`
- `GET /api/flight_setup`
- `POST /api/flight_setup`
- `POST /api/flight_setup/apply`
- `GET /api/fill_targets`
- `POST /api/fill_targets`
- `GET /api/auto_zero`
- `POST /api/auto_zero`
- `POST /api/auto_zero/launch/cancel`
- `GET /api/gse/status`
- `GET /api/gse/config`
- `POST /api/gse/config`
- `POST /api/gse/abort/clear`
- `POST /api/gse/self-test-confirmation`
- `POST /api/gse/cold-flow/start`
- `POST /api/gse/cold-flow/release`
- `POST /api/gse/cold-flow/override`
- `POST /api/gse/qualification/reset`
- `POST /api/gse/overpressure/acknowledge`
- `POST /api/gse/sequences/pause`
- `POST /api/gse/sequences/resume`
- `POST /api/gse/sequences/cancel`
- `POST /api/gse/sequences/dump`
- `GET /api/gse/sequences/results/{backend_instance}/{reset_epoch}/{run_id}`
- `GET /api/safety/about`
- `GET /api/igt`
- `POST /api/igt/prepare`
- `POST /api/igt/confirm`
- `PUT /api/igt/settings`
- `GET /api/checklists`
- `POST /api/checklists`
- `POST /api/checklists/check`
- `POST /api/checklists/sign`
- `POST /api/checklists/resolve`
- `GET /api/checklists/sync`
- `GET /api/checklists/export`
- `GET /api/checklists/export.csv`

5.5 Commands, action policy and RRLS

- `POST /api/command`
- `GET /api/action_policy`
- `GET /api/rrls/control`
- `GET /api/rrls/status`
- `GET /api/board_abort`
- `POST /api/board_abort/refresh`
- `GET /api/watch/actions`

5.6 Boards, network health, recovery, SD and bench tools

- `GET /api/board-recovery/status`
- `POST /api/board-recovery/prepare`
- `POST /api/board-recovery/confirm`
- `POST /api/board-recovery/query`
- `POST /api/board-recovery/finish-without-absent`
- `POST /api/board-recovery/recover-station`
- `GET /api/board-config/status`
- `POST /api/board-config/resolve`
- `GET /api/actuator/plumbing/status`
- `POST /api/actuator/plumbing/query`
- `POST /api/actuator/plumbing/settings`
- `POST /api/actuator/plumbing/retraction-history/inspect`
- `POST /api/actuator/plumbing/retraction-history/reset`
- `POST /api/network-health/daq/sd/retry`
- `GET /api/radio/modem`
- `POST /api/radio/modem/check`
- `GET /api/radio/reject-test`
- `POST /api/radio/reject-test/start`
- `POST /api/radio/reject-test/stop`
- `GET /api/sd_download`
- `POST /api/sd_download/{board}/list`
- `POST /api/sd_download/{board}/start`
- `POST /api/sd_download/jobs/{id}/cancel`
- `POST /api/sd_download/fc/resume`
- `POST /api/sd_download/fc/stop`
- `GET /api/daq/dac`
- `POST /api/daq/dac/config`
- `POST /api/daq/dac/disable`
- `POST /api/daq/dac/preview`
- `GET /api/daq/dac/validation`
- `POST /api/daq/dac/validation`
- `GET /api/daq/dac/sweep`
- `POST /api/daq/dac/sweep`
- `POST /api/daq/dac/sweep/cancel`
- `GET /api/firmware-sim`
- `POST /api/firmware-sim/control`

5.7 Calibration

- `GET /api/calibration_config`
- `GET /api/calibration`
- `POST /api/calibration`
- `GET /api/calibration/kg50_input`
- `POST /api/calibration/kg50_input`
- `POST /api/calibration/mcp_vref`
- `GET /api/calibration/thermal_settling`
- `POST /api/calibration/capture_thermal_zero`
- `GET /api/calibration/long_zero`
- `POST /api/calibration/long_zero`
- `POST /api/calibration/long_zero/stop`
- `POST /api/calibration/long_zero/apply`

5.8 Recordings, replay, exports, reports and flight review

- `GET /api/recordings`
- `GET /api/recordings/csv`
- `GET /api/recordings/{id}/csv`
- `GET /api/recordings/{id}/file`
- `POST /api/recordings/{id}/delete`
- `GET /api/recordings/{id}/board_sd/{board}/{file}`
- `GET /api/recordings/store`
- `POST /api/recordings/snapshot`
- `POST /api/recordings/settings`
- `GET /api/recordings/report`
- `GET /api/recordings/{id}/replay`
- `GET /api/recordings/{id}/replay/window`
- `GET /api/recordings/{id}/replay/flight`
- `GET /api/recordings/{id}/replay/context`
- `GET /api/recordings/{id}/replay/overlays`
- `GET /api/recordings/{id}/replay/alerts`
- `GET /api/recordings/{id}/replay/layout`
- `GET /api/recordings/{id}/replay/trajectory`
- `GET /api/recordings/{id}/replay/export`
- `GET /api/vehicle/geometry`
- `GET /api/recordings/{id}/sd/sources`
- `GET /api/recordings/{id}/sd/sources/{sha256}`
- `POST /api/recordings/{id}/sd/imports`
- `PUT /api/recordings/{id}/sd/imports/{token}`
- `DELETE /api/recordings/{id}/sd/imports/{token}`
- `POST /api/recordings/{id}/sd/imports/{token}/download`
- `GET /api/recovery`
- `POST /api/recovery/reached`
- `GET /api/recovery/export.{kind}`
- `GET /api/recovery/export.csv`
- `GET /api/recovery/export.json`
- `GET /api/flight_review/live`
- `GET /api/flight_review/recording/{id}`
- `GET /api/flight_review/recording/{id}/{kind}`
- `GET /api/flight_review/recording/{id}/motor_export`
- `GET /api/flight_review/recording/{id}/flight_export`
- `GET /api/burn_reports`
- `GET /api/burn_reports/{id}`
- `GET /api/fire_archive`
- `PATCH /api/fire_archive/{id}`
- `GET /api/fire_archive/{id}/thrust`
- `GET /api/apogee_planner`
- `PUT /api/apogee_planner/settings`
- `POST /api/apogee_planner/compute`
- `POST /api/apogee_planner/apply`
- `POST /api/apogee_planner/weather/fetch`
- `POST /api/apogee_planner/sim_environment/apply`
- `POST /api/apogee_planner/calibrate_flight`
- `POST /api/apogee_planner/ork`
- `DELETE /api/apogee_planner/ork`

5.9 Station settings, modes, system and administration

- `GET /api/station_mode`
- `GET /api/station-modes`
- `POST /api/station-modes/prepare`
- `POST /api/station-modes/confirm`
- `GET /api/mission-settings`
- `GET /api/mission-settings/conflicts`
- `POST /api/mission-settings/resolve`
- `GET /api/account/client-settings/watch`
- `POST /api/account/client-settings/watch`
- `GET /api/system/power`
- `POST /api/system/power`
- `GET /api/admin/users`
- `POST /api/admin/users`
- `POST /api/admin/users/create`
- `POST /api/admin/users/delete`
- `GET /api/admin/physical-panel`
- `POST /api/admin/physical-panel`
- `GET /api/admin/physical-panel/press-test`
- `GET /api/admin/sequence-access`
- `POST /api/admin/sequence-access`
- `GET /api/station/pairing`
- `POST /api/station/pairing/start`
- `POST /api/station/pairing/confirm`
- `POST /api/station/pairing/cancel`
- `POST /api/station/pairing/unpair`

5.10 Proximity lock

- `GET /api/proximity`
- `POST /api/proximity/report`
- `PUT /api/proximity/settings`
- `POST /api/proximity/override`
- `POST /api/proximity/override/end`
- `POST /api/proximity/dismiss`

5.11 Firmware update

- `GET /api/firmware/targets`
- `POST /api/firmware/flash/{board}`
- `GET /api/firmware/updates/{id}`
- `POST /api/firmware/updates/{id}/cancel`

5.12 Media, streams, programme and voice

- `GET /api/live_streams`
- `POST /api/live_streams/control`
- `GET /api/stream-roles`
- `POST /api/stream-roles`
- `GET /api/video/transcode/hosts`
- `POST /api/video/transcode/hosts`
- `GET /api/media-assets/recorded-vehicle/state`
- `POST /api/media-assets/recorded-vehicle/control`
- `GET /api/voice/ws`
- `GET /media`
- `GET /radio`
- `GET /api/media-assets/program`
- `GET /api/media-assets/recorded-vehicle`
- `GET /api/media-assets/streams/{id}`
- `GET /assets/models/gse-site.glb`
- `GET /assets/model-viewer.min.js`

5.13 Watch companion and watch direct link

- `POST /api/auth/watch-token`
- `POST /api/auth/watch-token/revoke`
- `GET /api/watch/status`
- `POST /api/command`
- `GET /tiles/{z}/{x}/{y}.jpg`
