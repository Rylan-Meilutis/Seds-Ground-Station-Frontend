# Vehicle geometry

The UI holds no vehicle-specific geometry. The station generates the vehicle from
its OpenRocket design and serves it at `GET /api/vehicle/geometry`; the schema,
sources and named parts are in GroundStation26 `docs/vehicle-geometry.md`. A team
with a different rocket imports its `.ork` at the station; nothing here changes.

## What uses it

- **3D vehicle** (`gs-vehicle-viewer`, `station_ui/rendering/three/vehicle-renderer.js`).
  The viewer takes the reply in `data-geometry`. When the configured model is the
  bundled `/assets/models/vehicle.glb`, it builds the vehicle from the geometry instead
  (`buildVehicle` in `replay-scene.js`): lathe-turned nose, tubes and transitions from
  the radius profile, extruded fins with their thickness and rounded edges, the nozzle,
  rail buttons, section colours from the design (else by material), and parachutes at
  their separation sized by their real diameters and lines. The nodes keep the GLB's
  names (`stage-1`, `nose`, `airframe-upper/-join/-lower`, `gimbal-pivot`, `engine`,
  `motor-flame`, `fin-pivot-N` > `fin`, `drogue-parachute`, `main-parachute`), so the
  configured motions keep working. An uploaded stage model is still loaded as a GLB.
- **Saved replays** (`createReplayAssembly(T, mode, geometry)`): flight mode shows the
  translucent airframe with its motor mount and tank inside; test-fire mode puts the
  motor (its tank and chamber from the geometry) on the stand with the rocket lying
  apart. Exported offline HTML replays embed the station geometry at export time
  (`vehicle_geometry`); the saved-replay page gets it with its state.
- **Live diagrams**: a frame with a `vehicle` box draws the side profile nose up, true to
  scale (about 28:1 for a 15 ft by 6 in rocket), highlighting the sections whose roles
  are listed (`src/telemetry_dashboard/live_diagrams/render.rs`, `VehicleView`). The
  ground-test view's rocket frame (IGT only) highlights the AV bay; the flight view
  has a `rocket` frame highlighting the AV bay and recovery bay, with the separation marked.
- **Programme**: the rocket inset and the diagram scenes draw the same silhouette
  (`window.gsVehicleSilhouette` in `station_ui/scripts/program-diagram.js`).

## Loading, cache and fallback

`src/telemetry_dashboard/vehicle_geometry.rs` loads the cached reply for the current
station (`gs_vehicle_geometry_v1_<station>`), then asks the station and caches a good
reply, so offline use and replays keep the last vehicle. A reply with another
`schema`/`version`, or an impossible profile, is refused (Rust `parse`, JS
`vehicleGeometry`) and the neutral generic rocket is drawn: 3 m, 150 mm, ogive nose,
four fins. The bundled `vehicle.glb` stays as the model with no geometry at all.

## Tests

- `vehicle_geometry::tests` (Rust): the station fixture parses true to the design
  (length, diameter, fins, aspect ratio of the drawing), bad versions are refused, the
  generic rocket is valid.
- `backend_contract`: `vehicle/geometry` is a covered backend contract fixture
  (`tests/fixtures/backend-contract/vehicle/geometry.json`).
- `live_diagrams::render_tests`: the ground-test and flight silhouettes render to scale.
- `node tests/vehicle-geometry.test.mjs`: parse/refuse, true-scale 3D with the node
  names, the generic fallback, flight and stand replays.
- `node tests/station-program-diagram.test.cjs`: the programme silhouette.
