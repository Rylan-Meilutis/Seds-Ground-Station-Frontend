# Broadcast studio

Stream managers and application administrators configure the audience programme. These controls do not operate valves, deployment outputs or ignition. Operational Dashboard pins remain unchanged when flight phase changes.

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

## Qualification limits

Browser acceptance uses generated video/audio, fake cameras and temporary HTTP/WebSocket fixtures. It exercises the actual programme, AudioWorklet, MapLibre and Wasm editor on Linux Chromium, including 320 px layouts. It does not certify real microphone acoustics, radio throughput, deployed MediaMTX codec conversion, native mobile permission dialogs or physical sequence outputs.
