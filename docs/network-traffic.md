# Router traffic diagnostics

Detailed Info distinguishes **Browser WebSocket traffic** from **Backend router traffic**.
The backend normally flushes telemetry batches every 20 ms (about 50 WebSocket
messages/s, plus status updates). Each batch can contain many readings. That
message rate does not measure incoming device traffic or limit acquisition.

Backend traffic uses the local SEDSnet router's cumulative packet and byte counters.
Each named router side shows RX/TX message rates, bytes/s, average bytes/message
for the sample window, and lifetime packet/byte totals. RX/TX are relative to
Ground Station; totals include protocol/control traffic and are side observations,
not unique sensor readings. A packet crossing two sides is observed on both.
Remote routers' individual internal sides are not reported by this API.

Rates use counter differences and actual monotonic backend elapsed time (at
least one second), shared across all clients. Idle links show zero. Initial,
new, or reset sides need a second sample; stale/disconnected clients show `--`
for live rates while retaining the last totals. Packet bytes are SEDSnet's
accounting, excluding physical-link framing; this is not raw CAN/serial/RF wire
utilization. Browser bandwidth is separate JSON WebSocket payload traffic.

The authenticated `/api/network_topology` response and `network_topology`
WebSocket messages include an optional `traffic` object with `interval_ms` and
`sides`. Each side has `side_id`, `name`, direction flags, cumulative `totals`,
and nullable `delta`. Rate = delta × 1000 / interval_ms. Older backends omit
this object and the UI reports backend traffic as unavailable.

The topology view follows observed nodes and links, including direct and relayed
paths. Zoom and pan use the current graph dimensions as nodes appear or disappear.
Resizing the window preserves manual zoom; **Reset** fits the graph again.
Interrupted touch gestures end when the pointer is cancelled, capture is lost,
or the window loses focus. Leaving the tab releases its event handlers before
another graph is mounted.

`node tests/network-topology-controls-browser.mjs` exercises the production graph
controller with local browser events. After `dx build --platform web`,
`node tests/network-topology-transitions-browser.mjs` checks the real Wasm UI
against isolated topology snapshots, including fullscreen and tab remounts.
Both accept `PLAYWRIGHT_MODULE` and `CHROME_BINARY` for local browser paths.
