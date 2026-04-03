# Backend API Reference

This document describes the backend contract that the frontend currently expects.

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

Response type:

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

Schema:

- `timestamp_ms: i64`
- `data_type: string`
- `sender_id: string`
- `values: (number | null)[]`

Notes:

- the frontend uses `values[0]` and `values[1]` as latitude/longitude for `GPS`, `GPS_DATA`, or `ROCKET_GPS`
- an empty array is acceptable

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
  "sensors": [
    {
      "id": "KG50",
      "label": "50kg",
      "data_type": "KG50",
      "channel": "ch0",
      "fit_modes": ["best", "linear", "linear_zero"]
    },
    {
      "id": "IADC",
      "label": "Tank Pressure",
      "data_type": "FUEL_TANK_PRESSURE",
      "channel": "iadc",
      "fit_modes": ["best", "linear", "parabolic"]
    }
  ]
}
```

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
    "x0": null
  },
  "ch1_fit": null,
  "iadc_fit": null,
  "weights_kg": [0.0, 5.0, 10.0]
}
```

### `POST /api/calibration`

Request and response use the same shape as `GET /api/calibration`.

### `POST /api/calibration/capture_zero`

Request:

```json
{
  "sensor_id": "KG50",
  "raw": 123.4
}
```

Response:

- same shape as `GET /api/calibration`

### `POST /api/calibration/capture_span`

Request:

```json
{
  "sensor_id": "KG50",
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

This is the largest payload in the frontend contract. It controls tab visibility, actions, data labels, state widgets, theming, and some battery estimation settings.

A minimal valid example is provided in:

- [`docs/api-examples/layout.minimal.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples/layout.minimal.json)

A richer example is provided in:

- [`docs/api-examples/layout.full.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples/layout.full.json)

Important enum values used by layout:

- `connection_tab.sections[].kind`: `board_status`, `latency`
- `state_tab.states[].sections[].widgets[].kind`: `board_status`, `summary`, `chart`, `valve_state`, `map`, `actions`
- `value formatter kind`: `number`, `integer`
- `data chart scale mode`: `shared`, `per_series`

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
- `Warning` and `Error` payloads are `{ "timestamp_ms": <i64>, "message": "<string>" }`
- `BoardStatus`, `NetworkTopology`, `Notifications`, `ActionPolicy`, and `NetworkTime` reuse the same shapes as their HTTP endpoints

## Implementation Advice

- Start with empty-but-valid data for noncritical routes.
- Keep route names and casing exact.
- If auth is not implemented yet, return an anonymous session from `/api/auth/session` and accept no-op login/logout.
- If you are prototyping, implement `/api/layout`, `/api/recent`, `/flightstate`, `/api/gps`, and WebSocket first. That gives you the fastest end-to-end feedback.
