# Backend API Reference

This document describes the backend contract that the frontend currently expects.

Verified against frontend version `0.3.1`, app build `7`, and the current Dioxus `0.7.4` codebase.

It is derived from the frontend source, not from an external OpenAPI spec. If you change the frontend route usage, update this file.

## Transport Model

Base URL example:

```text
https://backend.example.com
```

Derived URLs:

- HTTP route: `https://backend.example.com/api/layout`
- WebSocket route: `wss://backend.example.com/ws`

Authentication:

- HTTP requests include `Authorization: Bearer <token>` when logged in
- WebSocket connections append `?token=<token>` when logged in

Command uplink:

- The frontend sends commands over WebSocket
- Payload shape:

```json
{ "cmd": "Abort" }
```

## Compatibility Levels

### Required For Basic App Connectivity

| Method | Path | Notes |
| --- | --- | --- |
| `GET` | `/api/layout` | Required layout/config payload |
| `GET` | `/api/recent` | Telemetry history seed |
| `GET` | `/api/alerts` | Warning/error history |
| `GET` | `/api/map_config` | Map defaults |
| `GET` | `/flightstate` | Current flight state string |
| `GET` | `/api/gps` | Rocket GPS seed, can be null |
| `GET` | `/api/auth/session` | Session status |
| `POST` | `/api/auth/login` | Login endpoint |
| `POST` | `/api/auth/logout` | Logout endpoint |
| `GET` | `/ws` | WebSocket upgrade endpoint |

### Recommended For Full Dashboard Behavior

| Method | Path | Notes |
| --- | --- | --- |
| `GET` | `/api/action_policy` | Enables/disables action buttons |
| `GET` | `/api/network_time` | Shared network clock |
| `GET` | `/api/launch_clock` | Current T-minus/T-plus clock snapshot |
| `GET` | `/api/network_topology` | Topology tab |
| `GET` | `/api/boards` | Board presence summary |
| `GET` | `/api/notifications` | Persistent notifications |
| `POST` | `/api/notifications/{id}/dismiss` | Mark notification dismissed |
| `GET` | `/api/flight_setup` | Actions tab setup panel |
| `POST` | `/api/flight_setup` | Save setup config |
| `POST` | `/api/flight_setup/apply` | Apply selected setup |
| `GET` | `/api/fill_targets` | Actions tab fill panel |
| `POST` | `/api/fill_targets` | Save fill targets |
| `GET` | `/api/calibration_config` | Calibration tab layout |
| `GET` | `/api/calibration` | Calibration document |
| `POST` | `/api/calibration` | Save calibration document |
| `POST` | `/api/calibration/capture_zero` | Capture a zero point |
| `POST` | `/api/calibration/capture_span` | Capture a span point |
| `POST` | `/api/calibration/refit` | Recompute calibration fit |
| `GET` | `/api/i18n/catalog?lang=<code>` | Optional translation catalog |
| `POST` | `/api/i18n/translate` | Optional translation service |

## HTTP Schemas

### `GET /api/auth/session`

Response:

```json
{
  "authenticated": false,
  "username": null,
  "permissions": {
    "view_data": true,
    "send_commands": false
  },
  "expires_at_ms": null,
  "anonymous": true,
  "session_type": "anonymous",
  "allowed_commands": []
}
```

Notes:

- `allowed_commands` can be empty to mean "no command whitelist beyond permissions"
- if you support anonymous read-only mode, this route should still return `200`

### `POST /api/auth/login`

Request:

```json
{
  "username": "operator",
  "password": "secret",
  "remember_me": true
}
```

Response:

```json
{
  "token": "opaque-session-token",
  "session": {
    "authenticated": true,
    "username": "operator",
    "permissions": {
      "view_data": true,
      "send_commands": true
    },
    "expires_at_ms": 1760000000000,
    "anonymous": false,
    "session_type": "operator",
    "allowed_commands": ["Abort", "Arm", "Ignite"]
  }
}
```

### `POST /api/auth/logout`

The frontend sends a POST and only needs a success status code.

- Returning `200`, `204`, or another `2xx` is sufficient

### `GET /api/recent`

Response types:

```json
[
  {
    "timestamp_ms": 1750000000000,
    "data_type": "GPS",
    "sender_id": "FC",
    "values": [42.9586, -78.8119, 1200.0]
  }
]
```

Or streamed newline-delimited JSON on native builds:

```text
{"timestamp_ms":1750000000000,"data_type":"GPS","sender_id":"FC","values":[42.9586,-78.8119,1200.0]}
{"timestamp_ms":1750000000100,"data_type":"ACCEL","sender_id":"FC","values":[0.1,0.2,9.7]}
```

Schema:

- `timestamp_ms: i64`
- `data_type: string`
- `sender_id: string`
- `values: (number | null)[]`

Notes:

- the frontend uses `values[0]` and `values[1]` as latitude/longitude for `GPS`, `GPS_DATA`, or `ROCKET_GPS`
- an empty array is acceptable
- native builds accept either the array response or NDJSON-style streaming for faster reseed startup
- native builds advertise `Accept: application/x-ndjson` on this route
- if you stream, emit one complete telemetry row per line and start sending bytes promptly
- the native streaming path is enabled when the response content type contains `ndjson` or `json-seq`
- web builds should still be treated as requiring the array response path for compatibility
- see [`docs/backend-recent-streaming.md`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/backend-recent-streaming.md) for the current streaming-specific behavior

### `GET /api/alerts`

Response type:

```json
[
  {
    "timestamp_ms": 1750000001000,
    "severity": "warning",
    "message": "Tank pressure high"
  },
  {
    "timestamp_ms": 1750000002000,
    "severity": "error",
    "message": "Igniter continuity lost"
  }
]
```

`severity` is expected to be either:

- `warning`
- `error`

### `GET /flightstate`

Response type:

```json
"Armed"
```

This route returns a bare JSON string, not an object.

### `GET /api/launch_clock`

Response type:

```json
{
  "kind": "t_minus",
  "anchor_timestamp_ms": 1750000000000,
  "duration_ms": 10000
}
```

Schema:

- `kind: "idle" | "t_minus" | "t_plus"`
- `anchor_timestamp_ms: i64 | null`
- `duration_ms: i64 | null`

Semantics:

- `idle` means no backend-started launch clock is active.
- `t_minus` means countdown is active; remaining time is `duration_ms - (network_now_ms - anchor_timestamp_ms)`, clamped at zero.
- `t_plus` means launch has crossed T0; elapsed time is `network_now_ms - anchor_timestamp_ms`.
- Once `t_minus` starts, the backend must not reset or re-anchor it from repeated launch commands, stale flight-state packets, or reconnect/reseed flow.
- Once `t_plus` starts, the backend must not reset it to idle, restart `t_minus`, or re-anchor `t_plus`.
- Frontends should display `T- 00:00.00` after countdown completion until a backend `t_plus` update arrives.

### `GET /api/gps`

Response type:

```json
{
  "rocket": {
    "lat": 42.9586,
    "lon": -78.8119
  }
}
```

Empty GPS seed:

```json
{
  "rocket": null
}
```

### `GET /api/map_config`

Response type:

```json
{
  "max_native_zoom": 12,
  "default_center_lat": 31.0,
  "default_center_lon": -99.0,
  "default_zoom": 7.0,
  "map_title": "Recovery Map",
  "tracked_asset_label": "Rocket"
}
```

### `GET /api/boards`

Response type:

```json
{
  "boards": [
    {
      "board": "FlightComputer",
      "board_label": "Flight Computer",
      "sender_id": "FC",
      "seen": true,
      "last_seen_ms": 1750000002500,
      "age_ms": 250
    }
  ]
}
```

Known `sender_id` values inferred from the frontend:

- `GS`
- `FC`
- `RF`
- `PB`
- `VB`
- `GW`
- `AB`
- `DAQ`

### `GET /api/network_topology`

Response type:

```json
{
  "generated_ms": 1750000003000,
  "simulated": false,
  "nodes": [
    {
      "id": "fc",
      "label": "Flight Computer",
      "kind": "board",
      "status": "online",
      "group": "airframe",
      "sender_id": "FC",
      "endpoints": ["telemetry", "commands"],
      "show_in_details": true,
      "detail": "Primary telemetry source"
    }
  ],
  "links": [
    {
      "source": "fc",
      "target": "rf",
      "label": "UART",
      "status": "online"
    }
  ]
}
```

Enums:

- `kind`: `router`, `endpoint`, `side`, `board`
- `status`: `online`, `offline`, `simulated`

### `GET /api/network_time`

Response type:

```json
{
  "timestamp_ms": 1750000003500
}
```

### `GET /api/notifications`

Response type:

```json
[
  {
    "id": 101,
    "timestamp_ms": 1750000004000,
    "message": "Preflight checklist incomplete",
    "persistent": true,
    "action_label": "Open Actions",
    "action_cmd": "Abort"
  }
]
```

Notes:

- `persistent` defaults to `true` in the frontend
- `action_label` and `action_cmd` are optional

### `POST /api/notifications/{id}/dismiss`

The frontend only requires a success status code.

### `GET /api/action_policy`

Response type:

```json
{
  "key_enabled": true,
  "software_buttons_enabled": true,
  "controls": [
    {
      "cmd": "Abort",
      "enabled": true,
      "blink": "fast",
      "actuated": false
    },
    {
      "cmd": "Arm",
      "enabled": false,
      "blink": "none",
      "actuated": null
    }
  ]
}
```

`blink` values:

- `none`
- `slow`
- `fast`

### `GET /api/flight_setup`

Response type:

```json
{
  "version": 1,
  "selected_profile_id": "nominal",
  "profiles": [
    {
      "id": "nominal",
      "label": "Nominal",
      "wind_level": 2,
      "kalman": {
        "process_position_variance": 1.0,
        "process_velocity_variance": 1.0,
        "accel_variance": 0.5,
        "baro_altitude_variance": 2.0,
        "gps_altitude_variance": 5.0,
        "gps_velocity_variance": 1.5
      }
    }
  ]
}
```

### `POST /api/flight_setup`

Request and response are the same shape as `GET /api/flight_setup`.

### `POST /api/flight_setup/apply`

Request:

```json
{}
```

Response:

```json
{
  "selected_profile_id": "nominal",
  "wind_level": 2,
  "payload_bytes": 128
}
```

### `GET /api/fill_targets`

Response type:

```json
{
  "version": 1,
  "nitrogen": {
    "target_mass_kg": 12.5,
    "target_pressure_psi": 450.0
  },
  "nitrous": {
    "target_mass_kg": 18.0,
    "target_pressure_psi": 760.0
  }
}
```

### `POST /api/fill_targets`

Request and response are the same shape as `GET /api/fill_targets`.

### `GET /api/calibration_config`

Response type:

```json
{
  "capture_target_samples": 200,
  "fit_modes": [
    "best",
    "linear",
    "linear_zero",
    "parabolic",
    "parabolic_zero",
    "cubic",
    "cubic_zero",
    "quartic",
    "quartic_zero"
  ],
  "sensors": [
    {
      "id": "KG50",
      "label": "50kg",
      "data_type": "KG50",
      "channel": "ch0",
      "fit_color": "#f59e0b",
      "raw_label": "Raw",
      "expected_label": "kg",
      "fit_modes": ["best", "linear", "linear_zero", "parabolic", "parabolic_zero"]
    },
    {
      "id": "IADC",
      "label": "Tank Pressure",
      "data_type": "FUEL_TANK_PRESSURE",
      "channel": "iadc",
      "fit_color": "#a78bfa",
      "raw_label": "Raw",
      "expected_label": "psi",
      "fit_modes": ["best", "linear", "parabolic", "quartic"]
    }
  ]
}
```

The backend reads this from `backend/config/calibration_config.json` by default, or from
`GS_CALIBRATION_CONFIG_PATH` when set. `fit_modes` at the top level defines the backend-supported
regressions. A sensor can override that list with its own `fit_modes`. The frontend must use these
backend-provided modes rather than hard-coding regression choices.

Supported regression mode ids are:

- `best`
- `linear`
- `linear_zero`
- `parabolic` / `poly2`
- `parabolic_zero` / `poly2_zero`
- `cubic` / `poly3`
- `cubic_zero` / `poly3_zero`
- `quartic` / `poly4`
- `quartic_zero` / `poly4_zero`

### `GET /api/calibration`

Response type:

```json
{
  "full_mass_kg": 10.0,
  "ch0": { "m": 1.0, "b": 0.0 },
  "ch1": { "m": 1.0, "b": 0.0 },
  "iadc": { "m": 1.0, "b": 0.0 },
  "ch0_zero_raw": 0.0,
  "ch1_zero_raw": 0.0,
  "iadc_zero_raw": 0.0,
  "points": [
    { "kg": 5.0, "ch0_raw": 123.4 }
  ],
  "points_ch1": [
    { "kg": 25.0, "ch1_raw": 456.7 }
  ],
  "points_iadc": [
    { "expected": 300.0, "iadc_raw": 789.0 }
  ],
  "ch0_fit": {
    "type": "linear",
    "a": 1.0,
    "b": 0.0,
    "c": null,
    "d": null,
    "e": null,
    "x0": null
  },
  "ch1_fit": null,
  "iadc_fit": null,
  "extra_channels": {
    "aux_pressure": {
      "linear": { "m": 1.0, "b": 0.0 },
      "zero_raw": null,
      "points": [
        { "expected": 100.0, "raw": 123.4 }
      ],
      "fit": {
        "type": "linear",
        "a": null,
        "b": null,
        "c": null,
        "d": null,
        "e": null,
        "x0": null
      }
    }
  },
  "weights_kg": [0.0, 5.0, 10.0]
}
```

Legacy channels remain present for compatibility:

- `ch0` uses `points` with `{ "kg", "ch0_raw" }`
- `ch1` uses `points_ch1` with `{ "kg", "ch1_raw" }`
- `iadc` uses `points_iadc` with `{ "expected", "iadc_raw" }`

Future channels should use `extra_channels[channel_id]` with generic `{ "expected", "raw" }`
points plus `linear`, `zero_raw`, and `fit` metadata.

### `POST /api/calibration`

Request and response use the same shape as `GET /api/calibration`.

### `POST /api/calibration/capture_zero`

Request:

```json
{
  "sensor_id": "ch0",
  "raw": 123.4
}
```

Response:

- same shape as `GET /api/calibration`

### `POST /api/calibration/capture_span`

Request:

```json
{
  "sensor_id": "ch0",
  "raw": 345.6,
  "known_kg": 5.0
}
```

Response:

- same shape as `GET /api/calibration`

### `POST /api/calibration/refit`

Request:

```json
{
  "channel": "ch0",
  "mode": "best"
}
```

Response:

- same shape as `GET /api/calibration`

### `GET /api/layout`

This is the largest payload in the frontend contract. It controls tab visibility, actions, data labels, chart behavior, state widgets, board placeholders, theming, and battery estimation settings.

Theme behavior notes:

- Ground Station-provided theme colors are only used when the user selects the `backend` preset
- built-in presets such as `default`, `light`, `sunset`, `forest`, and `high_contrast` come from the app's compiled theme catalog
- operators can edit built-in theme presets in [`assets/themes/presets.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/assets/themes/presets.json), which is compiled into the app during build

A minimal valid example is provided in:

- [`docs/api-examples/layout.minimal.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples/layout.minimal.json)

A richer example is provided in:

- [`docs/api-examples/layout.full.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples/layout.full.json)

Important enum values used by layout:

- `connection_tab.sections[].kind`: `board_status`, `latency`
- `state_tab.states[].sections[].widgets[].kind`: `board_status`, `summary`, `chart`, `valve_state`, `map`, `actions`
- `value formatter kind`: `number`, `integer`
- `data chart scale mode`: `shared`, `per_series`

Generic layout behavior:

- `network_tab.expected_boards` accepts any non-empty sender id. The frontend no longer restricts this to a fixed board list.
- `data_tab.tabs[].chart.enabled` controls whether a telemetry type should render a graph. GPS or boolean telemetry should disable charts in layout instead of relying on frontend data-type names.
- `data_tab.tabs[].boolean_labels` and `channel_boolean_labels` control boolean value rendering. The frontend does not infer boolean rendering from a hardcoded telemetry id.
- `data_tab.sender_split_data_types` lists telemetry `data_type` values that should maintain separate chart caches per `sender_id`. Leave it empty for single shared charts.
- `state_tab` `valve_state` widgets must provide `data_type` and `valves`; the frontend no longer assumes a fixed valve telemetry id or fixed valve labels.
- `state_tab.states[].sections[].value_layout` can be `auto`, `horizontal`, or `vertical`. Use `horizontal` for telemetry value cards that should flow across the row.
- `state_tab` widgets can set `"full_width": true` to span the full section grid. This is intended for charts under horizontally arranged summary fields.
- state summary fill targets require explicit `fill_target_fluid` and `fill_target_kind` on each item that should show a target. The frontend does not infer targets from display labels.
- calibration sensors, labels, colors, telemetry data types, channel ids, and regression choices come from `/api/calibration_config`.

Main tab ids recognized by the frontend:

- `state`
- `connection-status`
- `map`
- `actions`
- `calibration`
- `notifications`
- `warnings`
- `errors`
- `data`
- `network-topology`
- `detailed`

### `GET /api/i18n/catalog?lang=<code>`

Response type:

```json
{
  "lang": "es",
  "translations": {
    "Warnings": "Advertencias",
    "Errors": "Errores"
  }
}
```

### `POST /api/i18n/translate`

Request:

```json
{
  "target_lang": "es",
  "texts": ["Warnings", "Errors"]
}
```

Response:

```json
{
  "lang": "es",
  "translations": {
    "Warnings": "Advertencias",
    "Errors": "Errores"
  }
}
```

## WebSocket Inbound Messages

The frontend deserializes inbound WebSocket messages as a tagged enum:

```json
{
  "ty": "Telemetry",
  "data": {
    "timestamp_ms": 1750000000000,
    "data_type": "GPS",
    "sender_id": "FC",
    "values": [42.9586, -78.8119, 1200.0]
  }
}
```

Supported `ty` values:

- `Telemetry`
- `TelemetryBatch`
- `FlightState`
- `LaunchClock`
- `Warning`
- `Error`
- `BoardStatus`
- `NetworkTopology`
- `Notifications`
- `ActionPolicy`
- `NetworkTime`

Examples are available in:

- [`docs/api-examples/websocket-messages.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples/websocket-messages.json)

Payload notes:

- `Telemetry` payload is one `TelemetryRow`
- `TelemetryBatch` payload is `TelemetryRow[]`
- `FlightState` payload is `{ "state": "<string>" }`
- `LaunchClock` payload is the same shape as `GET /api/launch_clock`
- `Warning` and `Error` payloads are `{ "timestamp_ms": <i64>, "message": "<string>" }`
- `BoardStatus`, `NetworkTopology`, `Notifications`, `ActionPolicy`, and `NetworkTime` reuse the same shapes as their HTTP endpoints

## Implementation Advice

- Start with empty-but-valid data for noncritical routes.
- Keep route names and casing exact.
- If auth is not implemented yet, return an anonymous session from `/api/auth/session` and accept no-op login/logout.
- If you are prototyping, implement `/api/layout`, `/api/recent`, `/flightstate`, `/api/gps`, and WebSocket first. That gives you the fastest end-to-end feedback.
