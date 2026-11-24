//! The demo and replay sessions answer the station contract in the shapes the
//! live dashboard decodes, and the demo scenario is internally consistent.
use super::*;
use crate::telemetry_dashboard::{AlertMsg, layout::LayoutConfig, mission_phase::FlightStateReply, types, vehicle_replay};

const EPOCH: i64 = 1_767_225_600_000;

fn decode<T: serde::de::DeserializeOwned>(session: &Session, path: &str, at: i64) -> T {
    let value = session.get(path, at).unwrap_or_else(|| panic!("{path} not served"));
    serde_json::from_value(value).unwrap_or_else(|e| panic!("{path} does not decode with the UI type: {e}"))
}

#[test]
fn demo_routes_decode_with_the_live_dashboard_types() {
    let s = demo::session(EPOCH);
    let at = demo::OPEN_AT_MS;
    let layout: LayoutConfig = decode(&s, "/api/layout", at);
    assert!(!layout.data_tab.tabs.is_empty());
    assert!(!layout.state_tab.states.is_empty());
    let boards: types::BoardStatusMsg = decode(&s, "/api/boards", at);
    assert_eq!(boards.boards.len(), 7);
    assert!(boards.boards.iter().all(|b| b.seen && b.age_ms.is_some_and(|a| a <= 1_000)), "every demo board is fresh");
    let (state, _) = decode::<FlightStateReply>(&s, "/flightstate", at).into_parts();
    assert_eq!(state, "NitrousFill");
    let recent: Vec<types::TelemetryRow> = decode(&s, "/api/recent", at);
    assert!(recent.len() > 100);
    assert!(recent.iter().all(|r| r.timestamp_ms >= EPOCH && r.timestamp_ms <= EPOCH + at));
    let alerts: Vec<AlertMsg> = decode(&s, "/api/alerts", 400_000);
    assert_eq!(alerts.len(), 4);
    let recordings: Vec<vehicle_replay::Recording> = decode(&s, "/api/recordings", at);
    assert_eq!(recordings.len(), 3);
    let session: crate::auth::SessionStatus = decode(&s, "/api/auth/session", at);
    assert!(!session.authenticated);
    assert!(s.get("/api/gps", at).unwrap()["rocket"]["lat"].as_f64().is_some());
    assert!(s.get("/api/calibration", at).unwrap()["extra_channels"].as_object().is_some_and(|c| !c.is_empty()));
    assert!(s.get("/api/firmware/targets", at).is_none(), "unserved routes are not invented");
}

#[test]
fn every_dashboard_widget_in_the_demo_layout_has_data() {
    let s = demo::session(EPOCH);
    for state in s.layout["state_tab"]["states"].as_array().unwrap() {
        for section in state["sections"].as_array().unwrap() {
            for w in section["widgets"].as_array().unwrap() {
                let types: Vec<&str> = match w["kind"].as_str().unwrap() {
                    "summary" | "valve_state" => vec![w["data_type"].as_str().unwrap()],
                    "chart" => w["chart_series"].as_array().unwrap().iter().map(|c| c["data_type"].as_str().unwrap()).collect(),
                    _ => vec![],
                };
                for ty in types {
                    assert!(s.latest(ty, None, demo::LOOP_MS).is_some(), "{ty} in the demo layout has no samples");
                }
            }
        }
    }
    for c in s.channels() {
        assert!(s.series(&c, 0, demo::LOOP_MS, 10).len() > 1, "{} {} has no history", c.group, c.label);
    }
}

#[test]
fn the_demo_scenario_is_consistent() {
    let s = demo::session(EPOCH);
    let order: Vec<&str> = s.states.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(order, ["Idle", "PreFill", "NitrogenFill", "NitrousFill", "Armed", "Launch", "Ascent", "Coast", "Descent", "Landed"]);
    let mass = s.first_channel("LOADCELL_50_WEIGHT_KG", 0).unwrap();
    let pressure = s.first_channel("OX_TANK_PRESSURE", 0).unwrap();
    let altitude = s.first_channel("BAROMETER_DATA", 0).unwrap();
    let (m_open, _) = s.value(&mass, demo::OPEN_AT_MS).unwrap();
    assert!((5.0..10.5).contains(&m_open), "the demo opens partway through the fill: {m_open}");
    let (m_armed, _) = s.value(&mass, 120_000).unwrap();
    assert!((10.3..10.6).contains(&m_armed));
    assert!(s.value(&pressure, 120_000).unwrap().0 > 700.0);
    assert!(s.value(&altitude, 120_000).unwrap().0.abs() < 2.0, "on the pad before launch");
    let apogee = s.series(&altitude, 0, demo::LOOP_MS, usize::MAX).into_iter().map(|p| p.1).fold(0.0f32, f32::max);
    assert!((3_000.0..3_100.0).contains(&apogee), "nominal 10k ft flight: {apogee} m");
    assert!(s.value(&mass, 200_000).unwrap().0.abs() < 0.1, "tank empty after the burn");
    assert_eq!(s.state_at(170_000), "Descent");
    // Valve states agree with the phase.
    let valves = s.latest("VALVE_STATE", None, 60_500).unwrap();
    assert_eq!(valves.values[5], Some(1.0), "nitrous open while filling");
    let valves = s.latest("VALVE_STATE", None, 120_000).unwrap();
    assert!(valves.values[..6].iter().all(|v| *v == Some(0.0)), "armed: outputs closed");
    assert!(s.samples.iter().all(|r| r.values.iter().flatten().all(|v| v.is_finite())));
    let track = s.track(demo::LOOP_MS);
    assert!(track.len() > 400);
    let summary = s.summary();
    assert!(summary.iter().any(|(k, v)| k == "Apogee (barometer)" && v.contains("ft")), "{summary:?}");
    assert!(summary.iter().any(|(k, _)| k.starts_with("Burn")));
}

#[test]
fn demo_commands_are_acknowledged_and_replays_are_read_only() {
    let s = demo::session(EPOCH);
    let (ok, message) = s.command("Launch");
    assert!(ok && message.contains("Nothing was sent"));
    let r = replay_file::open("f.json", include_str!("../../../tests/fixtures/offline-flight.json")).unwrap();
    let (ok, message) = r.command("Launch");
    assert!(!ok && message.contains("read-only"));
}

#[test]
fn sessions_are_counted_and_released() {
    let before = live_sessions();
    let s = demo::session(EPOCH);
    let r = replay_file::open("f.json", include_str!("../../../tests/fixtures/offline-flight.json")).unwrap();
    assert_eq!(live_sessions(), before + 2);
    drop((s, r));
    assert_eq!(live_sessions(), before);
}

#[test]
fn replay_files_open_in_every_mode_and_degrade_without_config() {
    let text = include_str!("../../../tests/fixtures/offline-flight.json");
    let mut file: Value = serde_json::from_str(text).unwrap();
    for (context, expected) in [
        (json!({"recorded_mode": "normal"}), Mode::Flight),
        (json!({"recorded_mode": "test_fire"}), Mode::TestFire),
        (json!({"recorded_mode": "integrated_ground_test", "igt": true}), Mode::GroundTest),
        (json!({"recorded_mode": "hitl", "hitl": true}), Mode::Hitl),
        (json!({"recorded_mode": "normal", "hitl": true}), Mode::Hitl),
    ] {
        file["replay_context"] = context;
        let s = replay_file::open("r.json", &file.to_string()).unwrap();
        assert_eq!(s.mode, expected);
        assert_eq!(s.replay_document.as_ref().unwrap()["mode"], s.mode.player_mode());
    }
    // An older file: no layout, mode, boards or geometry.
    for field in ["layout", "view_config", "valve_mappings", "mode", "replay_context"] {
        file.as_object_mut().unwrap().remove(field);
    }
    let s = replay_file::open("old.json", &file.to_string()).unwrap();
    assert!(!s.channels().is_empty(), "recorded types are listed under their own names");
    assert!(s.boards.iter().any(|b| b.sender == "VB"));
    assert!(s.vehicle_geometry.is_none());
    assert!(s.notes.iter().any(|n| n.contains("generic rocket")));
    let document = s.replay_document.as_ref().unwrap();
    assert!(document["view_config"]["stages"].is_array() && document["valve_mappings"].is_array() && document["layout"].is_object(), "the 3D player gets the sections it requires");
    assert!(replay_file::open("x.json", "{\"format\":\"csv\"}").err().unwrap().contains("Not a SEDS replay"));
    assert!(replay_file::open("x.json", "{\"format\":\"seds-replay\",\"version\":2,\"samples\":[]}").err().unwrap().contains("newer"));
    assert!(replay_file::open("x.json", "not json").is_err());
}

#[test]
fn replay_flight_state_rows_become_the_state_timeline() {
    let file = json!({"format": "seds-replay", "version": 1, "samples": [
        {"id": 1, "timestamp_ms": 1000, "sender_id": "FC", "data_type": "FLIGHT_STATE", "values": [6.0], "phase": null},
        {"id": 2, "timestamp_ms": 2000, "sender_id": "FC", "data_type": "FLIGHT_STATE", "values": [7.0], "phase": "Launch"},
        {"id": 3, "timestamp_ms": 2500, "sender_id": "FC", "data_type": "FLIGHT_STATE", "values": [7.0], "phase": "Launch"},
        {"id": 4, "timestamp_ms": 9000, "sender_id": "FC", "data_type": "FLIGHT_STATE", "values": [13.0]}
    ]});
    let s = replay_file::open("r.json", &file.to_string()).unwrap();
    assert_eq!(s.states, vec![(1000, "Armed".to_string()), (2000, "Launch".to_string()), (9000, "Landed".to_string())]);
    assert_eq!(s.mode, Mode::Flight, "FC rows imply a flight");
}

#[test]
fn diagram_inputs_follow_the_session_clock() {
    use crate::telemetry_dashboard::live_diagrams::{derive, topology};
    let s = demo::session(EPOCH);
    let fill = topology::bundled().into_iter().find(|t| t.id == "fill").unwrap();
    let filling = derive::derive(&fill, &DiagramInputs { session: &s, at: 60_500 });
    let armed = derive::derive(&fill, &DiagramInputs { session: &s, at: 120_000 });
    assert_ne!(format!("{filling:?}"), format!("{armed:?}"), "the fill diagram changes between filling and armed");
}

/// Live telemetry ingest cost: decode WebSocket-sized batches with the live
/// `TelemetryRow` type and intern their ids, as every live row is. Used to
/// show the offline workspace adds nothing to the live path:
/// `cargo test --release --bin groundstation_frontend live_row_ingest_cost -- --ignored --nocapture`
#[test]
#[ignore]
fn live_row_ingest_cost() {
    let batch: Vec<Value> = (0..256)
        .map(|i: i64| {
            let (data_type, sender) = [("OX_TANK_PRESSURE", "VB"), ("VALVE_STATE", "VB"), ("GPS_DATA", "FC"), ("BATTERY_VOLTAGE", "PB")][i as usize % 4];
            json!({"timestamp_ms": EPOCH + i * 20, "received_timestamp_ms": EPOCH + i * 20, "data_type": data_type, "sender_id": sender, "values": [1.5, 2.5, 3.5]})
        })
        .collect();
    let text = Value::Array(batch).to_string();
    let rounds = 2_000;
    let started = std::time::Instant::now();
    let mut rows_seen = 0usize;
    for _ in 0..rounds {
        let mut rows: Vec<types::TelemetryRow> = serde_json::from_str(&text).unwrap();
        for row in &mut rows {
            row.refresh_interned_ids();
        }
        rows_seen += rows.len();
    }
    let per_row_ns = started.elapsed().as_nanos() as f64 / rows_seen as f64;
    println!("live_row_ingest_cost: {rows_seen} rows, {per_row_ns:.0} ns/row; offline sessions alive: {}", live_sessions());
    assert_eq!(live_sessions(), 0);
}

const BUNDLE: &[u8] = include_bytes!("../../../tests/fixtures/backend-contract/replay-bundle/flight.gsreplay");
const STATION_CSV: &[u8] = include_bytes!("../../../tests/fixtures/backend-contract/replay-bundle/flight.csv");

#[test]
fn a_station_bundle_opens_with_everything_and_exports_the_station_csv() {
    let s = bundle::open("flight.gsreplay", BUNDLE).unwrap();
    assert_eq!(s.mode, Mode::Flight);
    assert_eq!(s.samples.len(), 601);
    assert!(s.layout["state_tab"].is_object(), "layout from the bundle");
    assert!(s.vehicle_geometry.is_some(), "vehicle geometry from the bundle");
    assert!(!s.diagrams.is_empty(), "station diagrams from the bundle");
    assert!(!s.derived.is_empty(), "flight review figures from the bundle");
    assert!(s.boards.iter().any(|b| b.sender == "FC" && b.label != "FC"), "board names from the bundle");
    assert!(s.track(i64::MAX).len() > 100);
    // The flight replay player gets the backend track, the flight review and the predicted flag.
    let extras = &s.replay_document.as_ref().unwrap()["replay_extras"];
    assert!(extras["trajectory"]["points"].as_array().is_some_and(|p| p.len() > 100));
    assert!(extras["flight_review"]["flight"]["events"].is_array());
    assert_eq!(extras["manifest"]["predicted"], false);
    assert!(extras["flight_plan"].is_null(), "a recording bundle carries no plan");
    // Round trip: the CSV exported offline is the station's own CSV export of the recording.
    assert_eq!(s.csv.as_deref().map(Vec::as_slice), Some(STATION_CSV));
    assert!(std::str::from_utf8(STATION_CSV).unwrap().starts_with("id,"));
    assert_eq!(s.manifest.as_ref().unwrap()["version"], 1);
}

#[test]
fn a_bundle_serves_its_phased_track_and_says_when_it_has_no_terrain() {
    let s = bundle::open("flight.gsreplay", BUNDLE).unwrap();
    let all: crate::telemetry_dashboard::map_3d::FlightTrack = serde_json::from_value(s.get("/api/flight_phases", i64::MAX / 4).unwrap()).unwrap();
    assert!(all.points.len() > 100);
    assert_eq!(all.phases.iter().map(|p| p.phase.as_str()).collect::<Vec<_>>(), ["pad", "boost", "coast", "drogue", "landed"]);
    for point in &all.points {
        assert_eq!(point.phase.as_deref(), all.phase_at(point.timestamp_ms));
    }
    let boost = all.phases[1].start_ms;
    let early: crate::telemetry_dashboard::map_3d::FlightTrack = serde_json::from_value(s.get("/api/flight_phases", boost - s.epoch_ms).unwrap()).unwrap();
    assert!(early.points.iter().all(|p| p.timestamp_ms <= boost));
    assert_eq!(early.current_phase.as_deref(), Some("boost"));
    // The fixture station had no DEM: the bundle carries none and the route says so.
    assert!(s.terrain.is_none());
    assert_eq!(s.get("/api/terrain/heightmap", 0).unwrap()["available"], false);
    assert_eq!(s.manifest.as_ref().unwrap()["terrain"]["included"], false);
}

#[test]
fn bundles_are_verified_and_newer_versions_refused() {
    let mut files = bundle::unzip(BUNDLE).unwrap();
    assert!(bundle::verify(&files).is_ok());
    let mut tampered = files.clone();
    tampered.get_mut("data/telemetry.csv").unwrap().push(b'x');
    assert!(bundle::verify(&tampered).err().unwrap().contains("does not match its manifest hash"));
    let mut manifest: Value = serde_json::from_slice(&files["manifest.json"]).unwrap();
    manifest["version"] = 2.into();
    files.insert("manifest.json".into(), serde_json::to_vec(&manifest).unwrap());
    assert!(bundle::verify(&files).err().unwrap().contains("newer than this app understands"));
    manifest["format"] = "zip".into();
    files.insert("manifest.json".into(), serde_json::to_vec(&manifest).unwrap());
    assert!(bundle::verify(&files).err().unwrap().contains("Not a ground-station replay bundle"));
    assert!(bundle::open("x.gsreplay", b"PK\x03\x04 not really").is_err());
    assert!(!bundle::is_bundle(b"{\"format\":\"seds-replay\"}"));
}

/// A stored (uncompressed) ZIP of `files` with a manifest listing their hashes.
fn stored_bundle(mut manifest: serde_json::Value, files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let hex = |b: &[u8]| Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect::<String>();
    manifest["entries"] = files.iter().map(|(p, d)| serde_json::json!({"path": p, "sha256": hex(d)})).collect();
    let mut all: Vec<(String, Vec<u8>)> = vec![("manifest.json".into(), serde_json::to_vec(&manifest).unwrap())];
    all.extend(files.iter().map(|(p, d)| (p.to_string(), d.clone())));
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, data) in &all {
        let offset = out.len() as u32;
        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend((data.len() as u32).to_le_bytes());
        out.extend((data.len() as u32).to_le_bytes());
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(name.as_bytes());
        out.extend(data);
        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        central.extend((data.len() as u32).to_le_bytes());
        central.extend((data.len() as u32).to_le_bytes());
        central.extend((name.len() as u16).to_le_bytes());
        central.extend([0u8; 12]);
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let at = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((all.len() as u16).to_le_bytes());
    out.extend((all.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(at.to_le_bytes());
    out.extend([0u8; 2]);
    out
}

/// A stored (uncompressed) ZIP of `files`, as a station writes one.
fn stored_zip(files: &std::collections::BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, data) in files {
        let offset = out.len() as u32;
        let header = |sig: u32, central_entry: bool| {
            let mut h = sig.to_le_bytes().to_vec();
            if central_entry {
                h.extend(20u16.to_le_bytes());
            }
            h.extend(20u16.to_le_bytes());
            h.extend([0u8; 4]); // flags, method 0 (stored)
            h.extend([0u8; 8]); // time, date, crc32 (not checked by the reader)
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes());
            if central_entry {
                h.extend([0u8; 6]); // comment, disk, internal attributes
                h.extend(0u32.to_le_bytes()); // external attributes
                h.extend(offset.to_le_bytes());
            }
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend(name.as_bytes());
        out.extend(data);
        central.extend(header(0x0201_4b50, true));
        central.extend(name.as_bytes());
    }
    let start = out.len() as u32;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

#[test]
fn a_flight_plan_bundle_is_labelled_predicted_and_carries_its_plan() {
    let plan: serde_json::Value = serde_json::from_str(include_str!("../../../tests/fixtures/flight-predict-mock/plan.json")).unwrap();
    let replay = serde_json::json!({"format": "seds-replay", "version": 1, "mode": "flight", "predicted": true, "label": "Predicted: 8.5 kg nominal",
        "samples": [{"timestamp_ms": 1_767_225_600_000i64, "data_type": "GPS_DATA", "sender_id": "FC", "values": [31.758, -102.046, 890.0]},
                    {"timestamp_ms": 1_767_225_600_000i64, "data_type": "FLIGHT_STATE", "sender_id": "FC", "values": [7]}]});
    let manifest = serde_json::json!({"format": "gsreplay", "version": 1, "predicted": true, "label": "Predicted: 8.5 kg nominal", "source": {"flight_plan": plan["id"]}});
    let bytes = stored_bundle(manifest, &[("replay.json", serde_json::to_vec(&replay).unwrap()), ("derived/flight_plan.json", serde_json::to_vec(&plan).unwrap())]);
    let session = bundle::open("plan.gsreplay", &bytes).unwrap();
    assert_eq!(session.predicted.as_deref(), Some("Predicted: 8.5 kg nominal"));
    assert!(session.provenance.starts_with("Predicted"));
    assert_eq!(session.plans.len(), 1);
    assert!(session.plans[0].map_overlay().is_some());
    assert_eq!(session.title, "Flight plan fp-1767225600000-1a2b3c4d");

    // A recording bundle with recent plans is not predicted; its plans are offered for comparison.
    let mut recording = replay.clone();
    recording["predicted"] = false.into();
    let manifest = serde_json::json!({"format": "gsreplay", "version": 1, "source": {"recording": "flight.db"}});
    let plans = serde_json::json!({"plans": [plan.clone(), plan]});
    let bytes = stored_bundle(manifest, &[("replay.json", serde_json::to_vec(&recording).unwrap()), ("derived/flight_plans.json", serde_json::to_vec(&plans).unwrap())]);
    let session = bundle::open("flight.gsreplay", &bytes).unwrap();
    assert_eq!(session.predicted, None);
    assert_eq!(session.plans.len(), 1, "duplicate plan ids are dropped");
}

#[test]
fn a_bundle_with_recovery_search_tracks_serves_the_coverage_overlay() {
    let mut files = bundle::unzip(BUNDLE).unwrap();
    assert!(bundle::open("flight.gsreplay", BUNDLE).unwrap().recovery_search.is_none());
    let search = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/backend-contract/recovery-search/stopped.json")).unwrap();
    files.insert("derived/recovery_search.json".into(), search);
    let bytes = stored_zip(&files);
    assert_eq!(bundle::unzip(&bytes).unwrap(), files, "the test writer round-trips");
    let s = bundle::open("flight.gsreplay", &bytes).unwrap();
    let served = s.get("/api/recovery_search", s.end_ms()).expect("served like the station's route");
    assert_eq!(served["overlay"]["kind"], "search_coverage");
    let status: super::super::recovery_search::Status = serde_json::from_value(served).unwrap();
    assert!(!status.active);
    assert!(status.overlay["areas"].as_array().unwrap().iter().any(|a| a["kind"] == "uncovered"));
}

#[test]
fn a_bundles_terrain_and_search_reach_the_flight_replay() {
    let mut files = bundle::unzip(BUNDLE).unwrap();
    let terrain = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/backend-contract/terrain/heightmap.json")).unwrap();
    let search = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/backend-contract/recovery-search/stopped.json")).unwrap();
    files.insert("terrain/heightmap.json".into(), terrain);
    files.insert("derived/recovery_search.json".into(), search);
    let s = bundle::open("flight.gsreplay", &stored_zip(&files)).unwrap();
    let extras = &s.replay_document.as_ref().unwrap()["replay_extras"];
    assert_eq!(extras["terrain"]["format"], "gs26.heightmap");
    assert!(extras["overlays"].as_array().unwrap().iter().any(|o| o["kind"] == "search_coverage"));
}
