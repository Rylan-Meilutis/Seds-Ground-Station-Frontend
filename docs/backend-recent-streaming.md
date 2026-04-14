# `/api/recent` Streaming Notes

This document matches the current frontend implementation in version `0.3.1` (build `7`).

## Compatibility Summary

- Web builds expect `GET /api/recent` to return a normal JSON array.
- Native builds accept either:
  - a normal JSON array, or
  - streamed NDJSON / JSON text sequence style responses

If you only implement one format, use the JSON array. If you want faster native reseed startup, add streaming for native clients.

## What The Native Frontend Sends

The native app issues:

- `GET /api/recent`
- `Accept: application/x-ndjson`
- `Authorization: Bearer <token>` when logged in

The frontend then inspects the response `Content-Type`.

Streaming mode is enabled when the content type contains either:

- `ndjson`
- `json-seq`

Anything else is treated as a normal JSON array response.

## Streaming Format

Send one telemetry row per line:

```text
{"timestamp_ms":1750000000000,"data_type":"GPS","sender_id":"FC","values":[42.9586,-78.8119,1200.0]}
{"timestamp_ms":1750000000100,"data_type":"ACCEL","sender_id":"FC","values":[0.1,0.2,9.7]}
```

Rules:

- Each line must be a complete JSON object matching `TelemetryRow`.
- Blank lines are ignored.
- A trailing newline is optional.
- The frontend also accepts a final non-empty line without a newline terminator.

## `TelemetryRow` Shape

```json
{
  "timestamp_ms": 1750000000000,
  "data_type": "GPS",
  "sender_id": "FC",
  "values": [42.9586, -78.8119, 1200.0]
}
```

Field expectations:

- `timestamp_ms`: integer
- `data_type`: string
- `sender_id`: string
- `values`: array of numbers or nulls

## Practical Advice

- Start sending bytes promptly; the native app reads incrementally.
- Keep web compatibility by preserving the JSON array response path for browser clients.
- If you cannot provide streaming yet, returning `[]` or a normal array is still valid.
