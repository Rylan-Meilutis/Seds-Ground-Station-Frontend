# UBSEDS Ground Station Frontend

This repository contains the Dioxus-based frontend for the UBSEDS ground station UI.

If your goal is to build a backend that works with this frontend, start here:

- Backend contract: [`docs/backend-api.md`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/backend-api.md)
- Example payloads: [`docs/api-examples`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples)
- Build helper: [`build.py`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/build.py)

## What The Frontend Expects

The app talks to a backend over both HTTP and WebSocket.

- HTTP base URL: configured by the user in the app, for example `https://backend.example.com`
- WebSocket URL: derived automatically from the HTTP base as `{ws_scheme}://{host}/ws`
- HTTP auth: `Authorization: Bearer <token>` when logged in
- WebSocket auth: `?token=<token>` query parameter when logged in
- Default native backend URL when none is configured: `http://localhost:3000`

The frontend uses HTTP for:

- initial page and dashboard data loads
- layout/config fetches
- auth
- calibration and setup editor writes
- dismissing persistent notifications

The frontend uses WebSocket for:

- live telemetry
- flight state updates
- warnings and errors
- network and board status
- action policy updates
- command sends from the UI to the backend

## Minimum Compatible Backend

A minimal backend should implement at least:

- `GET /api/layout`
- `GET /api/recent`
- `GET /api/alerts`
- `GET /api/map_config`
- `GET /flightstate`
- `GET /api/gps`
- `GET /api/auth/session`
- `POST /api/auth/login`
- `POST /api/auth/logout`
- `GET /ws` or equivalent WebSocket upgrade on `/ws`

For a more complete dashboard, also implement:

- `GET /api/action_policy`
- `GET /api/network_time`
- `GET /api/network_topology`
- `GET /api/boards`
- `GET /api/notifications`
- `POST /api/notifications/{id}/dismiss`
- `GET/POST /api/flight_setup`
- `POST /api/flight_setup/apply`
- `GET/POST /api/fill_targets`
- `GET /api/calibration_config`
- `GET/POST /api/calibration`
- `POST /api/calibration/capture_zero`
- `POST /api/calibration/capture_span`
- `POST /api/calibration/refit`
- `GET /api/i18n/catalog?lang=<code>`
- `POST /api/i18n/translate`

## Running The Frontend

Web:

```bash
cargo install dioxus-cli
dx serve
```

Build via helper:

```bash
python3 build.py web
```

Use `python3 build.py` for the full build helper usage.

## Notes For Backend Authors

- Route paths are hardcoded in the frontend. Matching them exactly is the easiest path to compatibility.
- Some routes can safely return empty data structures. For example, `/api/recent` can return `[]` and `/api/gps` can return `{ "rocket": null }`.
- WebSocket message tags are case-sensitive because they are deserialized from Rust enum variant names.
- Commands are sent over WebSocket as JSON objects like `{ "cmd": "Abort" }`.
- Layout drives a large part of the UI. If your backend returns a small valid layout, the frontend can still function without the full production config.

## Reference Files

- API reference: [`docs/backend-api.md`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/backend-api.md)
- Minimal layout example: [`docs/api-examples/layout.minimal.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples/layout.minimal.json)
- WebSocket examples: [`docs/api-examples/websocket-messages.json`](/Users/rylan/Documents/GitKraken/Seds-Ground-Station-Frontend/docs/api-examples/websocket-messages.json)
