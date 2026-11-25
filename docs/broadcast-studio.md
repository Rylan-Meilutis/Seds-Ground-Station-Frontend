# Broadcast studio

Stream managers and application administrators configure the audience programme. These controls do not operate valves, deployment outputs or ignition. Operational Dashboard pins remain unchanged when flight phase changes.

## Stream manager

The Stream Manager tab is a vision mixer. Everything it lists comes from the
station (`/api/studio`, [contract](backend-contract.md#5121-stream-manager-layouts-scenes-and-sources));
the station checks stream-manager access on every request.

- **Preview / Program.** Two monitors run the actual programme page: Program
  is what viewers see (muted; **Listen** unmutes), Preview draws the scene you
  picked. Click a scene thumbnail to load it into Preview, then **Cut** or
  **Fade** it to Program. The fade duration (100 ms–5 s) is the slider beside
  them. After a take, Program and Preview swap. Double-clicking a scene fades
  it straight to Program. Preview is yours alone; Program is shared.
- **Hotkeys** (outside text fields): `1`–`9` load scenes 1–9 into Preview,
  `Enter` or `C` cuts, `Space` or `T` fades.
- **Tally.** Red is Program, green is Preview, on the monitors, the scene
  thumbnails and the source tiles. An automatic camera also lights the camera
  it shows.
- **Scenes.** Each thumbnail shows its layout with each slot's source and a
  live value (T-clock, altitude). A dot marks unsaved changes. The scene in
  Preview is edited below: name, layout (pick a layout thumbnail), whether the
  programme telemetry bar stays, and the source in each slot. Save, revert,
  duplicate or delete; the scene on Program cannot be deleted.
- **Sources.** Tiles for every camera, telemetry view, overlay and graphic,
  with their state (live, no signal, stale, ready). Drag a tile onto a slot,
  or tap a tile and then a slot (phones and tablets). **Live camera
  thumbnails** shows each live camera's preview. **Hide/Show** keeps a camera
  out of the automatic cameras. Selecting a non-camera source shows its
  browser-source address for OBS (overlays are transparent there).
- **Layout editor.** A canvas with a 2.5 % grid: drag a slot to move it, its
  corner to resize, or select it and use the arrow keys (Shift resizes) or type
  its position, size and layer. Aspect presets: full, picture in picture, side
  by side, 2 × 2, lower third. A layout or slot that a scene fills cannot be
  removed.

### Telemetry views as sources

The programme page composes every scene in the browser (the station never
encodes video), so a telemetry view is a live element of the programme, fed
by the same delayed snapshot as the cameras: the live diagrams (each
per-mode view and **Live diagram (station mode)**, which follows the station's
mode), key telemetry cards, telemetry plots (the last two minutes), the
T-clock, the recovery map, the rocket graphic and the recorded flight replay.
Overlays (T-clock + altitude lower third, T-clock corner bug, telemetry strip)
have transparent backgrounds. Text is sized to its slot for 720p/1080p
viewing. A scene shows one live diagram, one map, one rocket graphic and one
recorded replay. During an instant camera replay the telemetry sources show
the replay's saved telemetry; the recorded flight replay is itself a source,
so a saved flight can be streamed as the programme. A scene's backmost
camera carries its audio.

The classic layouts (featured camera, grid, interview, rundown per-phase
scenes) still apply until the first take; a take switches the programme to
studio scenes, and the rundown's per-phase scene then no longer overrides it.

## Rundown

Open **Audience rundown** in Stream Manager. Choose a stage profile, its scene, ordered metrics, page size and cycling period. Metric details select a telemetry sender/type/channel or the currently selected calibrated fill-mass source. Save explicitly. Per-phase assignments can override the normal stage selection. Reload after a concurrent-edit conflict; unsuccessful saves preserve the local draft and never overwrite another operator's settings.

Defaults:

- Ground, including filling and Armed: calibrated fill mass, tank pressure, fill percentage, fill-system batteries, valve states, AV-bay satellite count and GPS-lock telemetry when supplied. Missing telemetry remains unavailable.
- Flight: altitude, velocity, AV-bay battery and GPS coordinates/status; fill-system metrics are excluded.
- Recovery: flight/recovery metrics; the map can be selected independently before or after landing.
- Test fire: fill metrics and thrust from the calibrated 1000 kg loadcell. Fire does not cause test-fire data to switch to the flight profile.

The backend persists the rundown with its presentation revision. It validates stage IDs, phases, finite scales/offsets, channel bounds, timing, sizes and scene names. Unknown phase assignments and invalid values are rejected.

## Interview and audio

Interview main and corner picture-in-picture feeds are selected from registered camera/participant feeds. A guest uses the existing authenticated camera-sharing page; this feature does not introduce a new guest-invitation or calling system. Camera microphones are off by default and require the publisher to select **Include this camera's microphone for interviews**. Each interview feed also has an independent audience-audio choice.

**Include programme commentary microphones** enables the audience mix only. It never requests a microphone on any device. The operator must open the audio desk and select **Start commentary microphone**. Commentary uses a separate authenticated programme-only WebSocket and is never registered as a crew/radio participant.

**Monitor crew voice** receives VC without requesting a microphone. **Hold to speak to VC** temporarily routes the microphone exclusively to crew, pausing commentary; release resumes commentary. Destination changes discard partial and queued microphone packets. Use headphones. Noise suppression is selectable before capture. Stop all audio, logout, lost permissions or closing the desk releases capture. The separate **Include crew voice** mix determines whether crew audio reaches the audience. Camera clip replay retains the existing muted-crew/commentary policy.

## Map and recorded replay

The map displays canonical rocket, GroundStation and RRLS coordinates from the delayed programme snapshot. Saved station positions are labelled saved, stale live positions remain marked stale, and absent positions remain unavailable. The map does not request device location or invent GPS lock. Administrators edit station fallback locations in Settings. Offline raster tiles use the normal map database.

Recorded vehicle replay is a separate saved-data scene, explicitly labelled recorded. See [recorded replay](recorded-vehicle-replay.md). It cannot masquerade as live vehicle telemetry. Orbit/follow controls and playback operate on the selected immutable recording snapshot.

## Live diagrams

Each rundown scene and any slot of a studio scene can be a live diagram: the view of a station mode (flight, test fire, ground test), the station's current mode, or the system overview. The diagram uses the delayed programme snapshot, so it matches the delayed video. It shows confirmed valve states, pending commands, flow, pressures and stale ages exactly as the dashboard's **Live diagrams** tab does. See [live diagrams](live-diagrams.md).

## Qualification limits

Browser acceptance uses generated video/audio, fake cameras and temporary HTTP/WebSocket fixtures. It exercises the actual programme, AudioWorklet, MapLibre and Wasm editor on Linux Chromium, including 320 px layouts. It does not certify real microphone acoustics, radio throughput, deployed MediaMTX codec conversion, native mobile permission dialogs or physical sequence outputs.
