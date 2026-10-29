# Current contract examples

These are illustrative JSON payloads, not live telemetry or credentials. `EXAMPLE_*`
media tickets and the login token must be replaced with server-issued values.

- [Vehicle visualization](vehicle-visualization.json): current stock single-stage
  rocket, aft fins, no upper fins/airbrakes, bundled offline named-node renderer.
  Phase motions are illustrative state displays, not confirmed physical deployment.
- [Manager streams](live-streams.json): live WebRTC previews, authoritative capability
  flags and delayed program URL; a customized two-field stats profile.
- [Viewer streams](live-streams-viewer.json): no live preview URLs or management access.
- [Dashboard status](dashboard-status.json): backend-resolved live phase/T clock and
  the six default fields; null means missing/stale, not zero.
- [Program state](program-state.json): delayed telemetry with current editorial camera
  state. `telemetry` may be null while warming; never substitute dashboard data.
- [Role list](stream-roles.json) and [role change](stream-role-change.request.json):
  stream-admin-only API, no hardware permission escalation.
- [Login](auth-login.response.json) and [anonymous session](auth-session.anonymous.json):
  `roles` is distinct from `session_type` (`session`, `remembered`, or null).

The media examples are mirrored in the backend repository's `docs/frontend/examples/`.
The backend's `docs/backend/presentation.example.json` illustrates file-based channel
configuration. Empty/missing `stats` inherits defaults; a nonempty array replaces them.
Do not copy a complete sample profile over an existing production configuration:
merge intended changes and preserve vehicle selection, broadcast revision and labels.

Backend mappings identify data type, optional sender, index, scale and offset. GPS
defaults use RF channel 2 for altitude and channels 0/1 for latitude/longitude. Fill
data uses existing calibrated/derived channels; it is not inferred from tank pressure.
No fin/gimbal sensor data is invented for the stock rocket. Custom multi-stage models
remain supported with suitable backend profiles and named GLB nodes.
