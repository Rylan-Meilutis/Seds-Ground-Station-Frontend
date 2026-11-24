# Replay bundle format (`.gsreplay`), version 1

A replay bundle holds everything needed to replay one recording with no
station connected, on any device with nothing cached:

- the recorded rows;
- the configuration that labels them;
- the vehicle geometry;
- derived summaries;
- the recording's CSV export;
- optionally, map tiles;
- optionally, recorded camera video and crew voice
  ([Recorded media](#recorded-media)).

Producers: GroundStation26 `GET /api/recordings/{id}/bundle`, available as
**Download replay bundle** in the Recordings panel. Consumers: the UI's live
dashboard on a local station (**Explore offline → Open replay file**, drop
the file on the window, or open it from another app;
[offline-preview.md](offline-preview.md)), and GroundStation26
`POST /api/recordings/import`.

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
  "entries": [{"path": "replay.json", "bytes": 232028, "sha256": "…", "media_type": "application/json", "route": "/api/recordings/groundstation_recording_2026-01-01_00-00-00_000.db/replay/export"}]
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
| `sd` | Only for an SD-resolved recording: `resolved_rows`; `receipt_csv` `{path, included, rows, bytes}` (or `reason` when not included); `sources` `{listed, bytes}` (the SD files the merge used); `files` `{requested, included, bytes, budget_bytes, complete}` (original SD files bundled). |
| `entries` | Every other entry, with size, SHA-256 (lower-case hex), media type and, when the entry is the body of a station route, `route`: the path and query exactly as a client requests them (the real recording id in place of `{id}`). Readers verify every hash and refuse a bundle whose listed entry is absent or differs. A reader may answer any GET whose path and query equal an entry's `route` with that entry's bytes (the UI's local station does). Entries with no `route` (`derived/burn_reports.json`, `derived/fire_archive.json`, `exports/*`, `config/report_context.json`, `derived/calibration_history.json`) are filtered lists or export inputs, not route bodies. |

## Entries

Each entry is the body of the named station route at export time. The
schemas are those in [backend-contract.md](backend-contract.md) and
[backend-api.md](backend-api.md). JSON entries are the route's JSON value with
keys sorted (`serde_json` order), not necessarily the route's bytes;
`data/telemetry.csv` is the route's bytes.

| Path | Required | Source route | Used for |
| --- | --- | --- | --- |
| `replay.json` | yes | `GET /api/recordings/{id}/replay/export`, completed with `layout`, `boards`, `view_config`, `valve_mappings`, `vehicle_geometry`, `mode` and `configuration_provenance` when the export lacks them | the rows (`samples`), flight states, replay context and the 3D player; the portable replay format of [backend-contract.md §10](backend-contract.md#10-offline-replay-files) |
| `data/telemetry.csv` | yes | `GET /api/recordings/{id}/csv`, byte for byte | the CSV export offline |
| `config/layout.json` | no | `GET /api/recordings/{id}/replay/layout?view={default_view}`, else `GET /api/layout` | dashboard, data tabs, units, actions |
| `config/boards.json` | no | `GET /api/boards` | board names |
| `config/calibration.json` | no | `GET /api/calibration` | calibration page, MCP reference and loadcell input in force |
| `config/calibration_config.json` | no | `GET /api/calibration_config` | sensor list for calibration |
| `config/diagrams.json` | no | `GET /api/diagrams` | the station's diagram topologies |
| `config/map_config.json` | no | `GET /api/map_config` | tile zoom range |
| `models/vehicle_geometry.json` | no | `GET /api/vehicle/geometry` | 3D vehicle (generic rocket without it) |
| `derived/context.json` | no | `GET /api/recordings/{id}/replay/context` | mode, replay views |
| `config/layout/{view}.json` | no | `GET /api/recordings/{id}/replay/layout?view={view}` for each replay view other than the default (`normal`, `test_fire`, `testing`, `hitl`, `integrated_ground_test`) | replay view switcher |
| `config/station_layout.json` | no | `GET /api/layout` (when `config/layout.json` is not already it) | 3D player, export fill-ins |
| `config/vehicle_visualization.json` | no | `GET /api/vehicle_visualization` | 3D player view configuration (also in `replay.json` `view_config`) |
| `derived/snapshot.json` | no | `GET /api/recordings/{id}/replay` | `upper_id` and time range for diagnostics and the 3D replay |
| `derived/flight_snapshot.json` | no | `GET /api/recordings/{id}/replay/flight` | recorded broadcast programme range |
| `derived/overlays.json` | no | `GET /api/recordings/{id}/replay/overlays` | thrust overlay |
| `derived/sd_sources.json` | no | `GET /api/recordings/{id}/sd/sources` (first page) | SD import list |
| `derived/recordings.json` | no | `GET /api/recordings`, only this recording's entry (`active: false`) | recording pickers |
| `derived/burn_reports/{id}.json` | no | `GET /api/burn_reports/{id}` for each related report | burn report detail |
| `derived/fire_archive/{id}/thrust.json` | no | `GET /api/fire_archive/{id}/thrust` for each related fire | thrust curve |
| `derived/alerts.json` | no | `GET /api/recordings/{id}/replay/alerts` | warnings list |
| `derived/trajectory.json` | no | `GET /api/recordings/{id}/replay/trajectory?upper_id=…`, with the station's flight `phases`, `events` and per-point `phase` | track (coloured by phase), tile selection, `GET /api/flight_phases` offline |
| `derived/flight_review.json` | no | `GET /api/flight_review/recording/{id}` | flight/motor summary figures |
| `derived/apogee_planner.json` | no | `GET /api/apogee_planner` (state at export) | reference |
| `derived/burn_reports.json`, `derived/fire_archive.json` | no | entries of `GET /api/burn_reports` / `GET /api/fire_archive` that name the recording | burn summary |
| `derived/flight_plans.json` | no | `{"plans": [plan document, …]}`, the five most recent flight plans (`GET /api/flight_plans/{id}`) | comparing the recording with a prediction offline (map and altitude chart) |
| `derived/flight_plan.json` | plan bundles only | `GET /api/flight_plans/{id}` | the plan itself: its landing area and markers on the map |
| `derived/stations.json` | no | `GET /api/antenna/status` at export (not written by GroundStation26 yet) | flight replay: ground-station camera and masts |
| `derived/recovery_search.json` | no | `GET /api/recovery_search/export?from_ms=<first row>&to_ms=<last row + 6 h>`, only when the recovery search has fixes in that window | the search coverage overlay (searched swaths, uncovered landing area, searcher tracks) on the replay map; also served as `GET /api/recovery_search` |
| `config/report_context.json` | no | the context the station adds to every `GET /api/recordings/report` request: `valve_mappings` and `channel_metadata` from the station layout, `recording_diagnostic_streams` | report studio offline |
| `derived/calibration_history.json` | no | the recording's `calibration_history` rows (`timestamp_ms`, `sender_id`, `sensor_id`, `config_json` as stored text), ordered by time; absent when the recording has no such table | report studio offline (recorded calibration epochs) |
| `exports/motor_export.zip`, `exports/flight_export.zip` | no | `GET /api/flight_review/recording/{id}/motor_export` / `flight_export` without a range, generated at `created_ms` and without the raster (PNG/PDF) charts; only the exports that apply to the recording | motor/flight review export offline |
| `exports/index.json` | no | `{kind: {path, filename, from_ms, to_ms, generated_ms, raster}}` for each bundled review export; `{}` when none applies | which review exports exist, their file names and windows |
| `tiles/{z}/{x}/{y}.jpg` | no | `GET /tiles/{z}/{x}/{y}.jpg` around the track | map background, 3D terrain imagery |
| `terrain/heightmap.json` | no | `GET /api/terrain/heightmap?lat=…&lon=…&extent_m=…&size=129`, centred on the track's first fix and covering the track plus 1 km (2–30 km), at most 1 MiB, only when the station has DEM tiles there | 3D terrain ([backend-contract.md §12](backend-contract.md#12-terrain-and-the-3d-map)) |

Terrain follows the tiles: `?tiles=1` also adds the heightmap when the station
has a DEM for the area (`terrain=0` leaves it out, `terrain=1` adds it without
tiles). Without it the 3D view draws flat ground at the pad's altitude.
The flight replay player gets the heightmap through `replay_extras.terrain`, and
the recovery search coverage (`derived/recovery_search.json`) as one of its
`replay_extras.overlays`, so an offline replay shows the ground and the search.

A plan bundle (`GET /api/flight_plans/{id}/bundle`) sets `manifest.predicted: true`
and `manifest.label`; its `replay.json` has `"predicted": true`. The flight replay
shows a Predicted badge for either. The offline workspace passes these entries to
the player as the document's `replay_extras` (`local_station/bundle.rs`).
| `derived/review_inputs.json` | no | what the motor/flight review reads from the recording, for its whole span (see "Review of a custom window"); left out with `?review=false` | review and review exports of any window offline |
| `models/hybrid_motor/hybrid_motor.json`, `sweet_v6_cea.json`, `sweet_v6_grain.json` | no | the station's hybrid motor model files (GroundStation26 `backend/src/apogee_planner/model/`), only when the recording has load cells and a tank PT | the burn report's predicted curve in a custom window |
| `tiles/{z}/{x}/{y}.jpg` | no | `GET /tiles/{z}/{x}/{y}.jpg` around the track | map background |
| `data/receipt_telemetry.csv` | no | SD-resolved recordings only: the rows as received (`main.telemetry`), in the CSV a recording that was never resolved exports (no route); omitted above 96 MiB (listed in `missing` with 507) | time-range CSV offline, import |
| `derived/sd_sources/after_{n}.json` | no | `GET /api/recordings/{id}/sd/sources?after_id={n}`, the SD source list's later pages (128 sources each) | SD import list |
| `sd/sources/{sha256}.csv` | no | `GET /api/recordings/{id}/sd/sources/{sha256}`, byte for byte: the original SD files, opt-in (`sd_files`) | SD source download, import |
| `records/checklists.json` | no | `GET /api/checklists`, byte for byte | Checklists tab |
| `records/checklists_export.json`, `records/checklist_signoffs.csv` | no | `GET /api/checklists/export`, `GET /api/checklists/export.csv`, byte for byte | Data Export: checklists |
| `records/recovery_history.json`, `records/recovery_history.csv` | no | `GET /api/recovery/export.json`, `GET /api/recovery/export.csv`, byte for byte | Data Export: recovery history |

Map tiles are opt-in. Request them with `?tiles=1&tiles_max_mb=N`; the default
budget is 8 MiB and the maximum 32 MiB. The station collects tiles from zoom
10 up to its `max_native_zoom`, starting at the lowest zoom. It skips tiles it
does not have and stops at the budget. The UI draws the deepest zoom whose
tiles cover the whole track.

The station records (`records/*`) are included by default; `records=false`
leaves them out. They are the station's answers at bundle time, not
filtered to the recording: the checklist document is the station's one
current document (its sign-offs have no recording link), and recovery events
name a launch session that the station does not map to recordings. The
checklist routes refuse anonymous callers, so a bundle made without signing
in lists them under `missing` (403).

The original SD files of an SD-resolved recording are opt-in:
`?sd_files=1&sd_files_max_mb=N` (default 16 MiB, at most 64), in listing
order; a file that does not fit is skipped and `sd.files.complete` is false.

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
## Recorded media

Recorded camera video and crew voice are opt-in, each with its own budget:

| Query | Default | Maximum |
| --- | --- | --- |
| `video=1&video_max_mb=N` | 32 MiB | 64 MiB |
| `voice=1&voice_max_mb=N` | 8 MiB | 16 MiB |

Flags accept `1`, `true`, `on` or `yes` (this also applies to `tiles`). The
Recordings panel offers both as checkboxes with a cap beside **Download
replay bundle** (`bundle_media_options.rs`). With every option at its
maximum a bundle stays under the reader's 192 MiB (64 MiB rows + 32 MiB tiles
+ 64 MiB video + 16 MiB voice).

The station includes whole files that overlap the recording's time range. It
does not trim them. Within each budget the smallest files go first. Entries:

| Path | Content |
| --- | --- |
| `media/video/<stream>/<file>.mp4` | a MediaMTX camera segment (fragmented MP4, one minute), stored without compression |
| `media/voice/<stream>/<start_ms>-<speaker>.wav` | one crew voice transmission (16 kHz mono PCM16 WAV); `stream` is `crew` or `radio` |
| `media/index.json` | `{version: 1, clips: [...], video: summary, voice: summary}` |

These entries have no `route`: no station route serves them to the UI. Each
clip in `clips` (in start order) has:

- `kind`: `video` or `voice`;
- `stream` and `file`;
- `path`: the entry;
- `start_ms` and `end_ms`: Unix ms, the replay rows' own clock;
- `bytes` and `media_type` (`video/mp4`, `audio/wav`);
- `timing`: how the start and end were found.

The manifest's `media` (present only when asked for) and the index carry
the same summary per requested kind:

- `requested`;
- `recorded`: false when the station does not record this kind;
- `candidates`: files overlapping the recording;
- `included`, `bytes` and `budget_bytes`;
- `complete`;
- `skipped`: `[{stream, file, start_ms, end_ms, bytes, reason}]`;
- `note`, when there is one.

Camera timing: MediaMTX names segments by the station's local time. The
station takes the one whole-quarter-hour UTC offset that puts the name less
than 15 minutes before the file's last write, which is the segment's end
(`timing: name_local_time_and_last_write`). A file whose last-write time does
not fit (for example a copied file) is read as UTC and one minute long
(`name_as_utc`). Voice bursts are named by their Unix start and their length
gives the end (`name_unix_ms_and_length`).

Crew voice is recorded only where the station opts in
(`GS_VOICE_RECORDINGS_DIR`, GroundStation26 `backend/src/voice/recorder.rs`).
Elsewhere `voice.recorded` is false and the replay says so.

Playback (`local_station/media.rs`, `recorded_media_view.rs`):

- `bundle::open` moves the clips out of the archive without copying them.
- On a local station the broadcast page (Mission tab) says "Live camera
  needs a station." and "Live voice needs a station." instead of polling
  `/api/live_streams`.
- Below that it shows one `<video>` per camera stream, with the segment that
  covers the playback position, and one `<audio>` per voice burst on air,
  with its speaker.
- Every 250 ms the elements follow `LocalStation::playback`:
  - the position in the clip is `position − start_ms`;
  - play and pause follow the clock;
  - the playback rate is the replay speed up to 16×. Faster than that,
    elements are paused and seeked on each tick (scrubbing);
  - an element is seeked when it drifts more than max(0.4 s, 0.25 s × speed),
    and by 0.05 s when paused or scrubbing;
  - video is muted above 1×, voice above 4×.
- Clip bytes reach the elements as blob URLs in the browser. Native webviews
  use the app's protocol path `local-media/<entry path>` (`main.rs`, with
  byte ranges), which serves only the selected local station's clips. No
  clip is fetched from a network.
- The Cameras & Recordings and Voice Chat tabs stay hidden offline. They are
  live station pages, and a local station grants neither camera nor voice
  permission.

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

## Routes that are not snapshot

`/api/recordings/{id}/replay/window` (playhead window, type groups,
`after_id` cursor) and `/api/recordings/report?request=` (user window and
channels) take arbitrary parameters. The UI's local station computes the
window from `replay.json` `samples` (`id <= upper_id`, `id > after_id`,
`start_ms..=end_ms`, `data_type` in `types`, id order, pages of 5,000, the
last row of each source before the window as seeds) and the report as
described below. `GET /api/burn_reports` and `GET /api/fire_archive` are
answered as `{"reports": derived/burn_reports.json}` and
`{"fires": derived/fire_archive.json}`.

## Import into a station

GroundStation26 `POST /api/recordings/import` (administrators; body: the
bundle, at most 256 MiB) applies the reading rules above and stores
`data/telemetry.csv` (plus `derived/alerts.json`) as a recording
`event.imported.<folder>.db` (kind `imported`). Its CSV export equals the
bundle's CSV byte for byte. An SD-resolved recording imports from
`data/receipt_telemetry.csv` (the rows as received) and `data/telemetry.csv`
(the resolved rows with their provenance), with the SD imports' manifests,
the resolution summary (`replay.json` `sd_provenance`), the source list and
any bundled original SD files; its CSV, time-range rows and SD source list
then equal the source station's. The SD samples left unresolved and the
coverage ends (`valid_until_ms` in replay windows) are not carried.
Responses: 201 `{id,label,rows,alerts,start_ms,end_ms,source,csv_sha256}`;
400 unreadable, unsafe or inconsistent bundle; 403 not an administrator; 409
`{error,id}` already imported (same source recording and CSV); 413 too
large; 422 newer version, or an SD-resolved recording without
`data/receipt_telemetry.csv` (a bundle from an older station).

## Exports offline

The UI answers the station's export routes for the bundle's recording from
the bundle (`local_station/exports.rs`, `exports::answer`). Each answer is
the station's answer to the same request, byte for byte.

| Export | Route | Offline |
| --- | --- | --- |
| Recording CSV | `GET /api/recordings/{id}/csv` | `data/telemetry.csv`, unchanged. |
| Time-range CSV | `GET /api/recordings/csv?start_ms=&end_ms=` | Derived from `data/telemetry.csv` (for an SD-resolved recording, from `data/receipt_telemetry.csv`, the rows as received, which the station's range export reads): the rows received in `[start_ms, end_ms)`, each record's cells unchanged behind a `recording` column. Over-range values are `OVR`/`-OVR` in both (the station's range export applies the same markers as the recording CSV). |
| Report studio preview, CSV, DAQ SD CSV/TSV | `GET /api/recordings/report?request=` with `format` `preview`, `csv`, `daq_csv`, `daq_tsv` | Computed from `data/telemetry.csv`, `derived/calibration_history.json` and `config/report_context.json` by a Rust port of the station's report worker (`exports/report.rs`, from GroundStation26 `recording_report.py`). The preview is the same JSON. For an SD-resolved recording the rows are the resolved ones on their canonical UTC time, with each row's provenance: the CSV's received time and `sample_provenance`, the units an SD import names, and SD rows copied whole (the original thirteen cells) into the DAQ SD layout, as the station writes them. The merge itself is the station's: it needs the operator's SD assignment, and the bundle carries its result. |
| Checklists and recovery history | `GET /api/checklists/export`, `/api/checklists/export.csv`, `/api/recovery/export.json`, `/api/recovery/export.csv` | The bundled `records/*` entries, byte for byte (404 "Not available offline" when the bundle has none). The demo answers them with small demo records in the station's shapes. |
| Motor and flight review exports | `GET /api/flight_review/recording/{id}/motor_export`, `.../flight_export` | The bundled ZIP, for a request without a window or with the default window (as given in `exports/index.json`, or as the export form fills it in whole seconds). |
| Time-range CSV | `GET /api/recordings/csv?start_ms=&end_ms=` | Derived from `data/telemetry.csv`: the rows received in `[start_ms, end_ms)`, each record's cells unchanged behind a `recording` column. Over-range values are `OVR`/`-OVR` in both (the station's range export applies the same markers as the recording CSV). |
| Report studio preview, CSV, DAQ SD CSV/TSV | `GET /api/recordings/report?request=` with `format` `preview`, `csv`, `daq_csv`, `daq_tsv` | Computed from `data/telemetry.csv`, `derived/calibration_history.json` and `config/report_context.json` by a Rust port of the station's report worker (`exports/report.rs`, from GroundStation26 `recording_report.py`). The preview is the same JSON. |
| Motor and flight review exports | `GET /api/flight_review/recording/{id}/motor_export`, `.../flight_export` | Without a window: the bundled ZIP. With `from_ms` and/or `to_ms`: computed from `derived/review_inputs.json` (below), the station's ZIP for the same request with `raster=false`. |
| Review of a window | `GET /api/flight_review/recording/{id}?from_ms=&to_ms=` | Computed the same way (the same JSON). Without a window the bundle's `derived/flight_review.json` answers. The dashboard asks only without a window today. |

Not offline:

- Report studio Excel and PDF (`format` `xlsx`, `pdf`): answered with 501
  "Excel and PDF reports need the station". They need openpyxl and
  matplotlib on the station.
- Motor/flight review exports of a custom window: 501. Porting the review
  (about 3,000 lines with the charts) is not worth it; the bundle carries the
  default-window files instead.
- The raster charts (PNG) and PDF report inside the review exports: the
  bundled ZIPs are made without them (SVG charts, data and the HTML report
  are kept).
- The time-range CSV of an SD-resolved recording from a bundle made before
  `data/receipt_telemetry.csv` existed: 501.
- The raster charts (PNG) and PDF report inside the review exports: they
  need matplotlib on the station. Offline ZIPs (bundled and computed) are the
  station's `raster=false` answer: SVG charts, data and the HTML report, and
  `provenance.json` notes "PNG/PDF not requested.". The export form does not
  send `raster`; a connected station renders them as before, the local
  station never does.
- A custom review window whose burn has a predicted curve, in a bundle without
  the hybrid motor model (made before the model entries, or from a recording
  without load cells and a tank PT): 501.
- Time-range CSV and reports of an SD-resolved recording (its CSV has
  `canonical_utc_ms` and `provenance_json` columns): 501. Their CSV is on the
  resolved timeline, while the station's range export reads the receipt rows.
- Other recordings: the range CSV and a report of all recordings
  (`recording` empty) cover only the bundle's recording, labelled with its
  id. The station covers every primary recording in the range.

A text cell the station guarded against spreadsheet formulas (a leading `'`
before `=`, `+`, `-` or `@`) is read back without the guard, so a sender or
type name that itself starts with `'` followed by one of those characters
would be misread. Station sender and type names never do.

Bundles made before these entries existed still answer the recording CSV,
the time-range CSV and the report formats (with default valve labels and
units, and no recorded calibration epochs); their review exports need the
station, and without `derived/review_inputs.json` a custom review window
answers 501.

### Review of a custom window

`derived/review_inputs.json` (GroundStation26 `flight_review::offline`) holds
what the station's review reads from the recording, for its whole span:

```json
{"format": "seds-gs-review-inputs", "version": 1,
 "id": "…", "source_kind": "capture", "label": "Launch", "mode": "test_fire", "provenance": "hardware", "simulated": false,
 "plan": {…}, "planner_prediction": {…},
 "station_software": "groundstation_backend 0.1.0",
 "types": ["LOADCELL_WEIGHT_KG", …], "senders": ["DAQ", …],
 "rows": [[t_ms, type_index, sender_index, [values…]], [t_ms, …, […], "payload"], …],
 "messages": [[t_ms, "text"], …], "station_states": [[t_ms, code], …],
 "extra_daq": [["kg1000", t_ms, value], …]}
```

- `rows`: the review's telemetry rows (`flight_review::data::read_pool`
  without a range) in the station's order, on network time (the source
  timestamp when the row has one). Values are numbers, `null`, or
  `"OVR"`/`"-OVR"` for over range. Only `GS_STATION_MODE` and
  `GS_FLIGHT_MAXIMA` rows keep their payload (the review reads no other).
- `messages`, `station_states`: the recording's message log and station
  flight states.
- `extra_daq`: the DAQ launch file attached to a plain event capture
  (`burn_report::daq_rows`), merged into the burn report as the station does.
- `label`, `mode`, `provenance`, `simulated`, `plan`: the event capture's
  manifest (`null`/`false` for other recordings). `planner_prediction` is the
  apogee planner's motor for `plan` (`motor::planner_prediction`), used when a
  window holds no burn.

For a request with a window the UI clips the rows as the station clips its
read (`Data::clip`), runs the station's review (burn report, hybrid model
prediction, flight summary) and writes the station's export
(`local_station/exports/review/`). The review code is GroundStation26's:
`zip.rs`, `plot.rs`, `thrust.rs`, `merge.rs` and `hybrid.rs` are copies
checked by `scripts/check_review_port.py` (and `tests/test_review_port.py`
when a GroundStation26 checkout is next to this one); `burn.rs`, `data.rs`,
`motor.rs`, `flight.rs`, `export.rs` and `review/mod.rs` are ports. The time
the station would stamp (`created_ms`, `generated_ms`) is the device's clock.
The hybrid model's floating point runs on the platform's math library: the
golden tests run natively (as the station does); a browser's results may
differ in the last digits of the predicted curve.

## Sizes

| Recording | Rows | Recording database | Bundle |
| --- | --- | --- | --- |
| Contract fixture (nominal 10k ft flight, 0.5 Hz) | 601 | — | 106 KB (13 KB of it the flight review export, 4.6 KB the review inputs, about 35 KB the five replay-view layouts) |
| Contract fixture test-fire capture (40 Hz telemetry, 200 Hz DAQ launch file) | 2,962 + 6,000 DAQ | — | 430 KB (39 KB review inputs, 183 KB hybrid motor model) |
| Contract fixture flight capture (20 Hz) | 11,471 | — | 485 KB (65 KB review inputs) |
| Synthetic ten-minute capture at flight rates (100 Hz IMU, 50 Hz loadcell/PT/baro, 5 Hz GPS) | 154,200 | 20.3 MB | 6.3 MB (about 2 MB of it `derived/review_inputs.json`; 4.3 MB with `?review=false`) |

| SD-resolved contract fixture (synthetic: 39 receipt rows, 40 resolved rows, one 2 KB SD file) | 40 | — | 96 KB (receipt CSV 2.9 KB, resolved CSV 38 KB, SD file 2 KB) |

An SD-resolved recording's bundle holds its rows twice (as received, and
resolved with per-row provenance, which is most of the resolved CSV) and the
provenance once more in `replay.json` (`sd_provenance`). The resolved CSV is
about ten times the receipt CSV for SD rows (each carries its SD record and
mapping); a ten-minute capture with SD-resolved load cells at 50 Hz (30,000
SD rows) adds about 30 MB unpacked, 3 to 4 MB packed. The replay export caps
the provenance at 64 MiB and the bundle reader at 192 MiB unpacked.

| Media fixture (`replay-bundle/media.gsreplay`: five 1 KB stand-in segments, one 0.2 s voice burst) | 601 | — | 119 KB (12 KB media) |
| Ten-minute capture, two cameras at 2 Mbit/s (20 segments of 15 MB) and 30 three-second voice bursts | — | — | media entries 32.9 MB at the default caps (2 of 20 segments, all voice), 62.9 MB at the maximum (4 segments) |

The media row comes from GroundStation26
`recording_bundle::tests::realistic_media_bundle_size` (`--ignored
--nocapture`). The caps hold only a few minutes of camera video at typical
bit rates.

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
    own `GET /api/recordings/{id}/csv`;
  - writes the station's answers to export requests to
    `replay-bundle/exports/` and lists them in `exports/requests.json`:
    range CSVs, report previews, CSVs and DAQ SD CSV/TSV (all channels, a
    window, a channel subset), error answers, and the review exports as
    bundled. A second recording (`cases`) covers what the flight lacks:
    recorded calibration epochs, an inferred regression, an over-range
    value, DAQ SD sensor names, valve labels, a recorded diagnostic stream,
    formula-like and non-ASCII senders and Python float formatting. A third
    (`sd`, `contract_fixtures_sd.rs`) is an SD-resolved recording with the
    resolver's tables and row shapes; its bundle is
    `replay-bundle/sd.gsreplay` (with the original SD file).
  - `recording_bundle_records` checks that each bundled record is the
    station's answer byte for byte, and `recording_import` that an
    SD-resolved bundle imports with both timelines.
    formula-like and non-ASCII senders and Python float formatting.
  - writes the station's reviews and review exports (`raster=false`) of
    custom windows to `replay-bundle/review/` and lists them in
    `review/requests.json`: windows of the bundled flight, of a flight
    capture (`review/flight_capture.gsreplay`: FC phases, messages, station
    states) and of a test-fire capture (`review/test_fire.gsreplay`: DAQ
    launch file, applied plan, hybrid model): the burn, a window that cuts
    the ignition, a window before the burn, and error answers. The station's
    clock is pinned so the answers are reproducible.
- UI `local_station/tests.rs`:
  - opens the bundle with no station;
  - checks the layout, geometry, diagrams, boards, track and summaries;
  - checks that the CSV export equals the station's CSV byte for byte;
  - answers every request in `exports/requests.json` offline and checks it
    equals the station's file (byte for byte; previews as JSON) or error;
  - answers every request in `review/requests.json` from its bundle at the
    station's pinned time and checks the ZIP is the same bytes (reviews: the
    same JSON) or the same error;
  - checks that hash, version and format errors are refused.
- UI `local_station/tests.rs`: a plan bundle opens labelled Predicted with its
  plan; `derived/flight_plans.json` plans are offered for comparison.
- UI `tests/offline-replay-predicted.test.mjs`: the 3D player's Predicted label.
  - checks that hash, version and format errors are refused;
  - answers the `sd` case from `sd.gsreplay`, and serves station records
    from bundle entries and from the demo.
- UI `local_station/tests.rs` also serves the bundle as a station: the
  recording's routes, replay snapshot and windows, the store listing.
- GroundStation26 `recording_bundle_media` tests: segment names and local
  time, selection by overlap and smallest-first within the cap, the index and
  summary, files still being written, voice recording off. `contract_fixtures`
  writes `replay-bundle/media.gsreplay` and `media.index.json` from synthetic
  files. `voice::recorder` tests check burst splitting, silence padding and
  the WAV header.
- UI `local_station/media.rs` and `recorded_media_view.rs`: the index
  parses, the media bundle opens with every clip's bytes, the clip at a
  position, and the clock math (offset, rate, drift tolerance, scrubbing,
  muting). `main.rs` checks the native protocol's byte ranges.
- UI `tests/recorded-media-browser.mjs` (same setup as the next test):
  - the broadcast page says live camera and voice need a station;
  - with `replay-bundle/media.gsreplay` open, the camera segment and the
    voice burst play from `blob:` URLs at the paused replay position;
  - the WAV decodes (0.2 s), and the burst leaves when playback passes it;
  - no network.
- UI `tests/offline-preview-browser.mjs`: opens the bundle in a fresh browser
  profile and drives the live dashboard's tabs on it, with zero network;
  downloads the CSV from Data Export and compares it with the station's.

## Opening files from other apps

A `.gsreplay` bundle (and, where noted, a `*.seds-replay.json` replay) opens
in the offline replay view (`/preview`) when another app hands it over. The
platform code reads the file and passes it to `src/opened_file.rs`
(`offer(name, bytes)`); the app root then navigates to the replay view, which
takes it (`take()`) and opens it like a file picked in the view. A file over
192 MiB is not read whole; the view shows a message instead.

| Platform | Registration | Delivery |
| --- | --- | --- |
| iOS | `scripts/patch_plist.sh`: exported type `com.ubseds.gsreplay` (conforms to `public.zip-archive`, `public.data`; extension `gsreplay`; media type `application/vnd.seds.gsreplay+zip`), document type (Viewer, Owner), `LSSupportsOpeningDocumentsInPlace` | Scene `openURLContexts` → Tao `Event::Opened`; vendored Tao starts security-scoped access for file URLs (`vendor-tao/GROUNDSTATION-PATCH.md`) |
| macOS | Same declarations in `platform/macos/Info.plist` | `application:openURLs:` → `Event::Opened` |
| Android | `platform/android/AndroidManifest.xml`: `VIEW` by media type; `VIEW` by name (`.*\\.gsreplay`, `.*\\.seds-replay\\.json`) for `application/zip`, `application/octet-stream`, `application/json` on `content`/`file`; `SEND`; the activity is `singleTask` | `MainActivity` (patched by `build.py`) → `OpenedFileBridge` copies the URI into the cache → Rust pulls it through JNI |
| Linux | `.desktop` `MimeType=application/vnd.seds.gsreplay+zip;`, `Exec=… %f`; shared-mime-info `usr/share/mime/packages/ubseds-groundstation.xml` (glob `*.gsreplay`) | Path as the first argument |
| Windows | NSIS installer: `HKCU\Software\Classes\.gsreplay` → `UBSEDS.GroundStation.Replay`, `shell\open\command` `"…exe" "%1"` (the IExpress fallback installer does not register it) | Path as the first argument |
| Web | Not registered (no web app manifest) | Use the file picker or drag and drop |

On Apple platforms `*.seds-replay.json` is not claimed (claiming all JSON
would put the app in every JSON share sheet); open it from the replay view.
A desktop file opened while the app is running starts a second window.

How to test on a device:

- iOS: AirDrop a `.gsreplay` to the phone, or in Files long-press it →
  Share → SEDS GS (or tap it once the app is the owner). Test a file in
  "On My iPhone", one in iCloud Drive (opened in place) and one received
  over AirDrop (copied to the app's Inbox), with the app closed and running.
- Android: copy a bundle to the device and open it from Files, or
  ```sh
  adb push flight.gsreplay /sdcard/Download/
  adb shell am start -a android.intent.action.VIEW \
    -d content://com.android.externalstorage.documents/document/primary%3ADownload%2Fflight.gsreplay \
    -t application/vnd.seds.gsreplay+zip
  ```
  (a provider URI needs a read grant; opening from the Files app always
  grants one). Also share it from Files (`SEND`), with the app closed and
  running.
- macOS: double-click a `.gsreplay` in Finder, or `open -a "UBSEDS GS"
  flight.gsreplay`.
- Linux / Windows: double-click the file after installing the package, or
  run `groundstation_frontend path/to/flight.gsreplay` (installed launcher:
  `ubseds-groundstation`, Windows: `"UBSEDS GroundStation.exe"`).

Expected: the app shows the offline replay view with the file's banner
("Offline replay — flight.gsreplay"); a file over 192 MiB or a damaged file
shows a message in the view.
