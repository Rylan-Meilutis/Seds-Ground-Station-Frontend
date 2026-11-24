# Replay bundle format (`.gsreplay`), version 1

A replay bundle holds everything needed to replay one recording with no
station connected, on any device with nothing cached:

- the recorded rows;
- the configuration that labels them;
- the vehicle geometry;
- derived summaries;
- the recording's CSV export;
- optionally, map tiles.

Producers: GroundStation26 `GET /api/recordings/{id}/bundle`, available as
**Download replay bundle** in the Recordings panel. Consumers: the UI's offline
workspace (**Explore offline → Open replay file**, or drop the file on the
window).

Another station or tool can produce bundles by following this page.

## Container

- A ZIP archive with stored (method 0) or deflated (method 8) entries.
- No ZIP64, no encryption.
- Entry names are relative, `/`-separated, and contain no `..` or empty
  segments.
- Readers refuse:
  - other compression methods;
  - unsafe names;
  - archives that unpack to more than 192 MiB.
- File extension: `.gsreplay`.
- Media type: `application/vnd.seds.gsreplay+zip`.

## `manifest.json` (required)

```json
{
  "format": "gsreplay",
  "version": 1,
  "created_ms": 1767225600000,
  "generator": "groundstation_backend 0.1.0",
  "source": {"station_name": "Ground Station", "recording": "groundstation_recording_2026-01-01_00-00-00_000.db"},
  "mode": "flight",
  "recorded_mode": "normal",
  "time_range": {"start_ms": 1767225600000, "end_ms": 1767225836000},
  "rows": 601,
  "tiles": {"included": 0, "bytes": 0, "budget_bytes": 1048576, "complete": true},
  "terrain": {"included": false, "reason": "no DEM tiles are cached on this station"},
  "missing": [{"path": "derived/flight_review.json", "route": "/api/flight_review/recording/…", "status": 404}],
  "entries": [{"path": "replay.json", "bytes": 232028, "sha256": "…", "media_type": "application/json"}]
}
```

| Field | Meaning |
| --- | --- |
| `format`, `version` | Must be `"gsreplay"` and an integer. Readers refuse other formats, and versions newer than they know ("Update the app"). Add fields without bumping the version; bump it only when a reader of the old version would misread the bundle. |
| `created_ms`, `generator`, `source` | When, by what software and from which station and recording the bundle was made. |
| `mode`, `recorded_mode` | Replay scene: `mode` is `flight` or `test_fire` (3D player); `recorded_mode` is the station mode (`normal`, `test_fire`, `hitl`, `integrated_ground_test`, `testing`) from the recording's context. |
| `time_range`, `rows` | Reception-time range and row count of the replayed rows. |
| `tiles` | Map tiles included, their bytes, the budget, and whether the budget cut them short. |
| `terrain` | Whether `terrain/heightmap.json` is included: `{included: true, bytes, extent_m, size, zoom, source, attribution}`, or `{included: false, reason?}`. Absent in older bundles. |
| `missing` | Optional resources the station could not supply, with the route and HTTP status. |
| `entries` | Every other entry, with size and SHA-256 (lower-case hex). Readers verify every hash and refuse a bundle whose listed entry is absent or differs. |

## Entries

Each entry is the body of the named station route at export time. The
schemas are those in [backend-contract.md](backend-contract.md) and
[backend-api.md](backend-api.md).

| Path | Required | Source route | Used for |
| --- | --- | --- | --- |
| `replay.json` | yes | `GET /api/recordings/{id}/replay/export`, completed with `layout`, `boards`, `vehicle_geometry`, `mode` and `configuration_provenance` when the export lacks them | the rows (`samples`), flight states, replay context and the 3D player; the portable replay format of [backend-contract.md §10](backend-contract.md#10-offline-replay-files) |
| `data/telemetry.csv` | yes | `GET /api/recordings/{id}/csv`, byte for byte | the CSV export offline |
| `config/layout.json` | no | `GET /api/recordings/{id}/replay/layout?view={default_view}`, else `GET /api/layout` | dashboard, data tabs, units, actions |
| `config/boards.json` | no | `GET /api/boards` | board names |
| `config/calibration.json` | no | `GET /api/calibration` | calibration page, MCP reference and loadcell input in force |
| `config/calibration_config.json` | no | `GET /api/calibration_config` | sensor list for calibration |
| `config/diagrams.json` | no | `GET /api/diagrams` | the station's diagram topologies |
| `config/map_config.json` | no | `GET /api/map_config` | tile zoom range |
| `models/vehicle_geometry.json` | no | `GET /api/vehicle/geometry` | 3D vehicle (generic rocket without it) |
| `derived/context.json` | no | `GET /api/recordings/{id}/replay/context`, when the export has no `replay_context` | mode |
| `derived/alerts.json` | no | `GET /api/recordings/{id}/replay/alerts` | warnings list |
| `derived/trajectory.json` | no | `GET /api/recordings/{id}/replay/trajectory?upper_id=…`, with the station's flight `phases`, `events` and per-point `phase` | track (coloured by phase), tile selection, `GET /api/flight_phases` offline |
| `derived/flight_review.json` | no | `GET /api/flight_review/recording/{id}` | flight/motor summary figures |
| `derived/apogee_planner.json` | no | `GET /api/apogee_planner` (state at export) | reference |
| `derived/burn_reports.json`, `derived/fire_archive.json` | no | entries of `GET /api/burn_reports` / `GET /api/fire_archive` that name the recording | burn summary |
| `derived/flight_plans.json` | no | `{"plans": [plan document, …]}`, the five most recent flight plans (`GET /api/flight_plans/{id}`) | comparing the recording with a prediction offline (map and altitude chart) |
| `derived/flight_plan.json` | plan bundles only | `GET /api/flight_plans/{id}` | the plan itself: its landing area and markers on the map |
| `derived/stations.json` | no | `GET /api/antenna/status` at export (not written by GroundStation26 yet) | flight replay: ground-station camera and masts |
| `derived/recovery_search.json` | no | `GET /api/recovery_search/export?from_ms=<first row>&to_ms=<last row + 6 h>`, only when the recovery search has fixes in that window | the search coverage overlay (searched swaths, uncovered landing area, searcher tracks) on the replay map; also served as `GET /api/recovery_search` |
| `tiles/{z}/{x}/{y}.jpg` | no | `GET /tiles/{z}/{x}/{y}.jpg` around the track | map background, 3D terrain imagery |
| `terrain/heightmap.json` | no | `GET /api/terrain/heightmap?lat=…&lon=…&extent_m=…&size=129`, centred on the track's first fix and covering the track plus 1 km (2–30 km), at most 1 MiB, only when the station has DEM tiles there | 3D terrain ([backend-contract.md §12](backend-contract.md#12-terrain-and-the-3d-map)) |

Terrain follows the tiles: `?tiles=1` also adds the heightmap when the station
has a DEM for the area (`terrain=0` leaves it out, `terrain=1` adds it without
tiles). Without it the 3D view draws flat ground at the pad's altitude.

A plan bundle (`GET /api/flight_plans/{id}/bundle`) sets `manifest.predicted: true`
and `manifest.label`; its `replay.json` has `"predicted": true`. The flight replay
shows a Predicted badge for either. The offline workspace passes these entries to
the player as the document's `replay_extras` (`local_station/bundle.rs`).

Map tiles are opt-in. Request them with `?tiles=1&tiles_max_mb=N`; the default
budget is 8 MiB and the maximum 32 MiB. The station collects tiles from zoom
10 up to its `max_native_zoom`, starting at the lowest zoom. It skips tiles it
does not have and stops at the budget. The UI draws the deepest zoom whose
tiles cover the whole track.

Models the UI ships itself (for example the GSE site model and the generic
rocket) are not copied into bundles.

## Predicted bundles (flight plans)

`GET /api/flight_plans/{id}/bundle` is a normal version 1 bundle that plays a
flight plan like a recording. It differs from a recording bundle in:

- `manifest.json`: `"predicted": true`, `"source": {"flight_plan": "<id>", …}`
  and `"label": "Predicted: …"`;
- `replay.json`: `"predicted": true` and `"label": "Predicted: …"`. Its
  synthesized FC rows are `GPS_DATA` `[lat, lon, alt_msl]`, `BAROMETER_DATA`
  `[alt_agl, pressure_pa, temp_c]` and `FLIGHT_STATE` `[code]`, at 10 Hz in
  ascent and 2 Hz in descent, with timestamps from `created_ms` (ignition);
- `derived/flight_plan.json`: the plan document
  ([backend-contract.md §5.8.1](backend-contract.md#581-flight-plans-loss-of-signal-prediction-and-map-overlays)).

Readers show a clear "Predicted" label wherever `manifest.predicted` or
`replay.predicted` is `true` (banner, transport bar, map, charts, 3D player)
and never present the data as recorded. Only a boolean `true` marks a
prediction.

## Reading rules

- Entries a reader does not know are ignored.
- An absent optional entry degrades a page without blocking it:
  - no layout lists each recorded type under its own name;
  - no geometry draws the generic rocket;
  - no tiles draws a grid.
- A plain portable replay file (`*.seds-replay.json`) still opens. It is the
  `replay.json` entry on its own, without the CSV, calibration, diagrams or
  tiles.
- The CSV export of a bundle is `data/telemetry.csv`, unchanged. It is the
  station's own CSV export (same columns, quoting, canonical type names,
  `OVR` markers and IGT provenance column), so it cannot drift from the
  station's format. GroundStation26 and the UI both check this against the
  fixture `replay-bundle/flight.csv`.

## Sizes

| Recording | Rows | Recording database | Bundle |
| --- | --- | --- | --- |
| Contract fixture (nominal 10k ft flight, 0.5 Hz) | 601 | — | 56 KB |
| Synthetic ten-minute capture at flight rates (100 Hz IMU, 50 Hz loadcell/PT/baro, 5 Hz GPS) | 154,200 | 20.3 MB | 4.3 MB |

The second row comes from GroundStation26
`recording_bundle::tests::realistic_capture_bundle_size`
(`cargo test -p groundstation_backend -- realistic_capture_bundle_size --ignored --nocapture`).
A recording is limited by the replay export: 200,000 rows and 64 MiB of rows.

## Tests

- GroundStation26 `contract_fixtures`:
  - builds the bundle of a recorded flight twice and checks the two are
    byte-identical;
  - checks that the route answers with the bundle media type;
  - writes `replay-bundle/flight.gsreplay`, its manifest and the recording's
    own `GET /api/recordings/{id}/csv`.
- UI `local_station/tests.rs`:
  - opens the bundle with no station;
  - checks the layout, geometry, diagrams, boards, track and summaries;
  - checks that the CSV export equals the station's CSV byte for byte;
  - checks that hash, version and format errors are refused.
- UI `local_station/tests.rs`: a plan bundle opens labelled Predicted with its
  plan; `derived/flight_plans.json` plans are offered for comparison.
- UI `tests/offline-replay-predicted.test.mjs`: the 3D player's Predicted label.
- UI `offline_preview/tests.rs`: renders every page from the bundle.
- UI `tests/offline-preview-browser.mjs`: opens the bundle in a fresh browser
  profile.
