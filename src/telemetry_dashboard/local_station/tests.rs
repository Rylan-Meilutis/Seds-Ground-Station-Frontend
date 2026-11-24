//! The local station answers the station contract in the shapes the live
//! dashboard decodes, pushes the station's frames as its clock runs, answers
//! every write itself, and the demo scenario is internally consistent.
use super::*;
use crate::telemetry_dashboard::{AlertMsg, ActionPolicyMsg, WsInMsg, layout::LayoutConfig, mission_phase::FlightStateReply, types, vehicle_replay};

const EPOCH: i64 = 1_767_225_600_000;

fn decode<T: serde::de::DeserializeOwned>(station: &LocalStation, path: &str) -> T {
    let reply = station.request_at("GET", path, None, EPOCH);
    assert_eq!(reply.status, 200, "{path}: {}", reply.text());
    serde_json::from_slice(&reply.body).unwrap_or_else(|e| panic!("{path} does not decode with the UI type: {e}"))
}

fn frames(station: &LocalStation, wall: i64) -> Vec<WsInMsg> {
    station.frames(wall).iter().map(|f| serde_json::from_str(f).unwrap_or_else(|e| panic!("frame {f:.80} does not decode: {e}"))).collect()
}

#[test]
fn demo_routes_decode_with_the_live_dashboard_types() {
    let station = LocalStation::new(demo::session(0), EPOCH);
    let layout: LayoutConfig = decode(&station, "/api/layout");
    assert!(!layout.data_tab.tabs.is_empty());
    assert!(!layout.state_tab.states.is_empty());
    let boards: types::BoardStatusMsg = decode(&station, "/api/boards");
    assert_eq!(boards.boards.len(), 7);
    assert!(boards.boards.iter().all(|b| b.seen && b.age_ms.is_some_and(|a| a <= 1_000)), "every demo board is fresh");
    let (state, detail) = decode::<FlightStateReply>(&station, "/flightstate").into_parts();
    assert_eq!(state, "Fill", "the network state, as a station reports it");
    assert_eq!(detail.unwrap().fill_phase.as_deref(), Some("NitrousFill"));
    let recent: Vec<types::TelemetryRow> = decode(&station, "/api/recent?display=true&max_rows=12000");
    assert!(recent.len() > 100);
    assert!(recent.iter().all(|r| r.timestamp_ms <= EPOCH && r.timestamp_ms >= EPOCH - demo::OPEN_AT_MS), "history is the scenario so far, stamped as received");
    let alerts: Vec<AlertMsg> = decode(&station, "/api/alerts?minutes=20");
    assert_eq!(alerts.len(), 2, "the warnings raised before the demo opens");
    let recordings: Vec<vehicle_replay::Recording> = decode(&station, "/api/recordings");
    assert_eq!(recordings.len(), 1);
    let replay: Value = decode(&station, "/api/recordings/demo-flight.db/replay/export");
    assert_eq!(replay["format"], "seds-replay");
    let session: crate::auth::SessionStatus = decode(&station, "/api/auth/session");
    assert!(session.permissions.view_data && session.permissions.send_commands && session.anonymous);
    assert_eq!(session.station_name.as_deref(), Some("Demo rocket"));
    let policy: ActionPolicyMsg = decode(&station, "/api/action_policy");
    assert!(policy.controls.iter().any(|c| c.cmd == "Launch" && c.enabled), "the layout's actions are offered");
    let _: types::NetworkTopologyMsg = decode(&station, "/api/network_topology");
    assert!(station.request("GET", "/api/calibration", None).ok());
    let missing = station.request("GET", "/api/firmware/targets", None);
    assert_eq!(missing.status, 404, "unserved routes are not invented");
}

#[test]
fn the_clock_pushes_station_frames_in_order() {
    let station = LocalStation::new(demo::session(0), EPOCH);
    assert!(frames(&station, EPOCH).iter().any(|f| matches!(f, WsInMsg::BoardStatus(_))));
    // 40 s on: the fill completes, the station arms and Launch is pressed.
    let later = frames(&station, EPOCH + 40_000);
    let rows: usize = later.iter().map(|f| match f { WsInMsg::TelemetryBatch(rows) => rows.len(), _ => 0 }).sum();
    assert!(rows > 500, "{rows} rows in 40 s");
    let batches: Vec<&Vec<types::TelemetryRow>> = later.iter().filter_map(|f| match f { WsInMsg::TelemetryBatch(rows) => Some(rows), _ => None }).collect();
    assert!(batches.iter().flat_map(|b| b.iter()).all(|r| r.timestamp_ms > EPOCH && r.timestamp_ms <= EPOCH + 40_000), "rows are stamped as received now");
    assert!(later.iter().any(|f| matches!(f, WsInMsg::FlightState(m) if m.state == "Launch")));
    assert!(later.iter().any(|f| matches!(f, WsInMsg::MissionPhase(p) if p.phase == crate::telemetry_dashboard::mission_phase::Phase::Launch)),
        "the operator phase the dashboard's phase profiles follow");
    // In flight: the FC's detailed phase rides along as detail, never a retired state name.
    let flying = frames(&station, EPOCH + 60_000);
    assert!(flying.iter().any(|f| matches!(f, WsInMsg::FlightState(m) if m.state == "InFlight" && m.detail.fc_phase.as_ref().is_some_and(|p| p.label == "Ascent" || p.label == "Coast"))));
    assert!(flying.iter().any(|f| matches!(f, WsInMsg::MissionPhase(p) if p.phase == crate::telemetry_dashboard::mission_phase::Phase::InFlight)));
    assert!(frames(&station, EPOCH + 40_000).iter().all(|f| !matches!(f, WsInMsg::TelemetryBatch(_))), "nothing is sent twice");
    // Ten times faster: 10 s of wall time covers 100 s of the scenario.
    station.set_speed(10.0, false, EPOCH + 40_000);
    assert_eq!(station.playback(EPOCH + 50_000).position, demo::OPEN_AT_MS + 140_000);
    // The demo loops; a replay stops at its end.
    assert_eq!(station.playback(EPOCH + 90_000).position, (demo::OPEN_AT_MS + 40_000 + 500_000) % demo::LOOP_MS);
    let replay = LocalStation::new(bundle::open("flight.gsreplay", BUNDLE).unwrap(), EPOCH);
    let end = replay.session.end_ms();
    assert_eq!(replay.playback(EPOCH + 10_000_000).position, end);
    assert!(replay.playback(EPOCH + 10_000_000).paused);
    replay.seek(replay.session.start_ms() + 60_000, EPOCH);
    assert_eq!(replay.playback(EPOCH).position, replay.session.start_ms() + 60_000);
    let recent: Vec<types::TelemetryRow> = decode(&replay, "/api/recent");
    assert!(!recent.is_empty() && recent.iter().all(|r| r.timestamp_ms <= EPOCH), "history up to the new position");
    assert!(recent.iter().any(|r| r.timestamp_ms > EPOCH - 1_000));
}

#[test]
fn commands_and_writes_are_answered_here_and_never_sent() {
    let station = LocalStation::new(demo::session(0), EPOCH);
    station.command(r#"{"cmd":"Launch"}"#);
    let acks: Vec<String> = frames(&station, EPOCH).into_iter().filter_map(|f| match f { WsInMsg::Notification(n) => Some(n.message), _ => None }).collect();
    assert_eq!(acks, ["Demo: “Launch” acknowledged. Nothing was sent; no station is connected."]);
    for (method, path) in [("POST", "/api/command"), ("POST", "/api/calibration"), ("PUT", "/api/layout"), ("DELETE", "/api/recordings/x"), ("PATCH", "/api/fill_targets")] {
        let reply = station.request(method, path, Some(br#"{"cmd":"Dump"}"#));
        assert_eq!(reply.status, 409, "{method} {path} is refused, not applied");
        assert!(reply.text().contains("Nothing was sent"));
    }
    let replay = LocalStation::new(replay_file::open("f.json", include_str!("../../../tests/fixtures/offline-flight.json")).unwrap(), EPOCH);
    replay.command(r#"{"cmd":"Abort"}"#);
    let acks: Vec<String> = frames(&replay, EPOCH).into_iter().filter_map(|f| match f { WsInMsg::Notification(n) => Some(n.message), _ => None }).collect();
    assert_eq!(acks, ["Replay is read-only: “Abort” was not sent."]);
    assert!(replay.request("POST", "/api/fill_targets", None).text().contains("read-only"));
}

#[test]
fn selecting_a_local_station_reroutes_and_releases_everything() {
    use crate::telemetry_dashboard::{UrlConfig, persist, station_link};
    let before = live_sessions();
    persist::set_string("local-station-test-key", "station");
    {
        let station = std::rc::Rc::new(LocalStation::new(demo::session(0), EPOCH));
        let _selected = station_link::LocalSelection::install(station.clone());
        assert!(station_link::is_local());
        assert_eq!(UrlConfig::base_http(), station_link::LOCAL_BASE);
        assert_eq!(station_link::request("GET", "/flightstate", None).unwrap().status, 200);
        assert!(station_link::request_url("GET", "https://elsewhere/api/layout", None).is_none(), "only the local base is answered");
        persist::set_string("local-station-test-key", "demo");
        assert_eq!(persist::get_string("local-station-test-key").as_deref(), Some("demo"), "the dashboard sees its own writes");
        assert!(station_link::native_local_tile(10, 1, 1).is_some_and(|t| t.is_none()), "the map never asks a station for tiles");
    }
    assert!(!station_link::is_local());
    assert!(station_link::request("GET", "/flightstate", None).is_none());
    assert_eq!(persist::get_string("local-station-test-key").as_deref(), Some("station"), "stored preferences are untouched");
    assert!(station_link::native_local_tile(10, 1, 1).is_none());
    assert_eq!(live_sessions(), before, "the station and its rows are freed");
    persist::_remove("local-station-test-key");
}

#[test]
fn the_demo_scenario_is_consistent() {
    let s = demo::session(EPOCH);
    let order: Vec<&str> = s.states.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(order, ["Idle", "PreFill", "NitrogenFill", "NitrousFill", "Armed", "Launch", "Ascent", "Coast", "Descent", "Landed"]);
    let v = |ty: &str, index: usize, at: i64| s.latest(ty, None, at).and_then(|r| r.values[index]).unwrap();
    let m_open = v("LOADCELL_50_WEIGHT_KG", 0, demo::OPEN_AT_MS);
    assert!((5.0..10.5).contains(&m_open), "the demo opens partway through the fill: {m_open}");
    assert!((10.3..10.6).contains(&v("LOADCELL_50_WEIGHT_KG", 0, 120_000)));
    assert!(v("OX_TANK_PRESSURE", 0, 120_000) > 700.0);
    assert!(v("BAROMETER_DATA", 0, 120_000).abs() < 2.0, "on the pad before launch");
    let apogee = s.samples.iter().filter(|r| r.data_type == "BAROMETER_DATA").filter_map(|r| r.values[0]).fold(0.0f32, f32::max);
    assert!((3_000.0..3_100.0).contains(&apogee), "nominal 10k ft flight: {apogee} m");
    assert!(v("LOADCELL_50_WEIGHT_KG", 0, 200_000).abs() < 0.1, "tank empty after the burn");
    assert_eq!(s.state_at(170_000), "Descent");
    // Valve states agree with the phase.
    let valves = s.latest("VALVE_STATE", None, 60_500).unwrap();
    assert_eq!(valves.values[5], Some(1.0), "nitrous open while filling");
    let valves = s.latest("VALVE_STATE", None, 120_000).unwrap();
    assert!(valves.values[..6].iter().all(|v| *v == Some(0.0)), "armed: outputs closed");
    assert!(s.samples.iter().all(|r| r.values.iter().flatten().all(|v| v.is_finite())));
    assert!(s.track(demo::LOOP_MS).len() > 400);
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

const BUNDLE: &[u8] = include_bytes!("../../../tests/fixtures/backend-contract/replay-bundle/flight.gsreplay");
const STATION_CSV: &[u8] = include_bytes!("../../../tests/fixtures/backend-contract/replay-bundle/flight.csv");
/// An SD-resolved recording's bundle (GroundStation26 `contract_fixtures_sd`).
const SD_BUNDLE: &[u8] = include_bytes!("../../../tests/fixtures/backend-contract/replay-bundle/sd.gsreplay");

#[test]
fn a_station_bundle_opens_with_everything_and_exports_the_station_csv() {
    let s = bundle::open("flight.gsreplay", BUNDLE).unwrap();
    assert_eq!(s.mode, Mode::Flight);
    assert_eq!(s.samples.len(), 601);
    assert!(s.layout["state_tab"].is_object(), "layout from the bundle");
    assert!(s.vehicle_geometry.is_some(), "vehicle geometry from the bundle");
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
    // Served as the station served it: the recording's CSV route and its snapshots.
    let station = LocalStation::new(s, EPOCH);
    let id = station.session.manifest.as_ref().unwrap()["source"]["recording"].as_str().unwrap().to_string();
    let csv = station.request("GET", &format!("/api/recordings/{id}/csv"), None);
    assert_eq!(csv.body, STATION_CSV);
    for route in ["/api/calibration", "/api/diagrams", "/api/vehicle/geometry", &format!("/api/recordings/{id}/replay/export")] {
        assert!(station.request("GET", route, None).ok(), "{route} from the bundle");
    }
    let recordings: Vec<vehicle_replay::Recording> = decode(&station, "/api/recordings");
    assert_eq!(recordings[0].id, id);
    // Replay windows are computed from the rows, paged by id like the station.
    let snapshot: vehicle_replay::Snapshot = decode(&station, &format!("/api/recordings/{id}/replay"));
    let mid = (snapshot.start_ms.unwrap() + snapshot.end_ms.unwrap()) / 2;
    let window: Value = decode(&station, &format!("/api/recordings/{id}/replay/window?upper_id={}&after_id=0&start_ms={mid}&end_ms={}&types=%5B%22GPS_DATA%22%5D", snapshot.upper_id, snapshot.end_ms.unwrap()));
    let rows = window["rows"].as_array().unwrap();
    assert!(!rows.is_empty() && rows.iter().all(|r| r["data_type"] == "GPS_DATA" && r["timestamp_ms"].as_i64().unwrap() >= mid));
    assert_eq!(window["seeds"].as_array().unwrap().len(), 1, "the last fix before the window seeds it");
    let store: Value = decode(&station, "/api/recordings/store");
    assert_eq!(store["legacy"][0]["id"], id.as_str());
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

/// The station's answers to export requests (GroundStation26 `contract_fixtures`,
/// `replay-bundle/exports/requests.json`): the flight bundle answers the
/// `flight` case; the `cases` recording (report cases the flight lacks) is
/// assembled from the station's CSV and calibration history of that
/// recording with the flight bundle's report context.
fn export_fixture(name: &str) -> Vec<u8> {
    let path = crate::backend_contract::dir().join("replay-bundle").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn report_cases_inputs() -> exports::ExportInputs {
    let mut files = bundle::unzip(BUNDLE).unwrap();
    files.retain(|path, _| path == "config/report_context.json");
    files.insert("derived/calibration_history.json".into(), export_fixture("exports/cases-calibration-history.json"));
    let csv = std::rc::Rc::new(export_fixture("exports/cases-recording.csv"));
    exports::ExportInputs::from_bundle("groundstation_recording_2026-01-02_00-00-00_000.db", csv, &files)
}

#[test]
fn offline_exports_equal_the_station_exports_byte_for_byte() {
    let session = bundle::open("flight.gsreplay", BUNDLE).unwrap();
    let flight = session.exports.clone().expect("a bundle answers exports");
    let cases = report_cases_inputs();
    assert!(!cases.calibration_history.is_empty());
    let sd = bundle::open("sd.gsreplay", SD_BUNDLE).unwrap().exports.clone().expect("an SD-resolved bundle answers exports");
    assert!(sd.receipt_csv.is_some(), "the bundle holds the rows as received");
    let mut sd_compared = 0;
    let requests: Value = serde_json::from_slice(&export_fixture("exports/requests.json")).unwrap();
    let mut compared = 0;
    for request in requests.as_array().unwrap() {
        let path = request["path"].as_str().unwrap();
        let inputs = match request["case"].as_str() {
            Some("flight") => &flight,
            Some("sd") => {
                sd_compared += 1;
                &sd
            }
            _ => &cases,
        };
        let answer = exports::answer(inputs, path).unwrap_or_else(|| panic!("{path} is an export route"));
        let status = request["status"].as_u64().unwrap() as u16;
        match answer {
            Ok(file) => {
                assert_eq!(status, 200, "{path}: the station answered {status} {}", request["body"]);
                let file_name = request["file"].as_str().unwrap();
                let expected = export_fixture(file_name);
                if file_name.ends_with(".json") {
                    let ours: Value = serde_json::from_slice(&file.bytes).unwrap();
                    assert_eq!(ours, serde_json::from_slice::<Value>(&expected).unwrap(), "{path}: preview differs from {file_name}");
                } else {
                    assert!(file.bytes == expected, "{path}: offline bytes differ from {file_name}\n--- offline\n{}\n--- station\n{}", String::from_utf8_lossy(&file.bytes[..file.bytes.len().min(3000)]), String::from_utf8_lossy(&expected[..expected.len().min(3000)]));
                }
                assert!(!file.content_type.is_empty());
            }
            Err((code, message)) => {
                assert_eq!((code, message.as_str()), (status, request["body"].as_str().unwrap_or_default()), "{path}");
            }
        }
        compared += 1;
    }
    assert!(compared >= 30, "{compared} requests");
    assert!(sd_compared >= 12, "{sd_compared} SD-resolved requests");
    // The bundle's own CSV route and the preview served inline.
    let csv = exports::answer(&flight, "/api/recordings/groundstation_recording_2026-01-01_00-00-00_000.db/csv").unwrap().unwrap();
    assert_eq!(csv.filename, "groundstation_recording_2026-01-01_00-00-00_000.csv");
    assert_eq!(csv.bytes, STATION_CSV);
}

#[test]
fn exports_the_bundle_cannot_make_say_they_need_the_station_and_reports_are_built() {
    let flight = bundle::open("flight.gsreplay", BUNDLE).unwrap().exports.clone().unwrap();
    let id = "groundstation_recording_2026-01-01_00-00-00_000.db";
    let report = |format: &str| {
        let request = serde_json::json!({"recording": id, "format": format, "channels": null}).to_string();
        let encoded: String = request.bytes().map(|b| format!("%{b:02X}")).collect();
        exports::answer(&flight, &format!("/api/recordings/report?request={encoded}")).unwrap()
    };
    // Excel and PDF are built offline (local_station/exports/document_tests.rs compares them).
    let xlsx = report("xlsx").unwrap();
    assert_eq!((xlsx.filename.as_str(), &xlsx.bytes[..2]), ("telemetry-report.xlsx", &b"PK"[..]));
    let pdf = report("pdf").unwrap();
    assert_eq!((pdf.filename.as_str(), &pdf.bytes[..5]), ("telemetry-report.pdf", &b"%PDF-"[..]));
    assert_eq!(report("csv").unwrap().filename, "telemetry-report.csv");
    assert_eq!(report("preview").unwrap().filename, "");
    // Without a window the review export is the bundled file; a window is computed.
    let base = format!("/api/flight_review/recording/{id}/flight_export");
    let whole = exports::answer(&flight, &base).unwrap().unwrap();
    assert_eq!(whole.content_type, "application/zip");
    assert!(whole.filename.starts_with("flight-") && whole.filename.ends_with(".zip"));
    let zip = &flight.zips["flight_export"];
    let (from, to) = (zip.from_ms.unwrap(), zip.to_ms.unwrap());
    let windowed = exports::answer(&flight, &format!("{base}?from_ms={from}&to_ms={to}")).unwrap().unwrap();
    assert_eq!((windowed.filename.as_str(), windowed.content_type), (whole.filename.as_str(), "application/zip"));
    assert_eq!(exports::answer(&flight, &format!("{base}?from_ms={to}&to_ms={from}")).unwrap().unwrap_err(), (400, "The range must end after it starts.".into()));
    // A bundle without review inputs (older) sends a window to the station.
    let mut files = bundle::unzip(BUNDLE).unwrap();
    files.remove(exports::review::INPUTS_PATH);
    let older = exports::ExportInputs::from_bundle(id, std::rc::Rc::new(STATION_CSV.to_vec()), &files);
    assert_eq!(exports::answer(&older, &format!("{base}?from_ms={}&to_ms={to}", from + 5_000)).unwrap().unwrap_err().0, 501);
    assert_eq!(exports::answer(&older, &format!("/api/flight_review/recording/{id}?from_ms={from}")).unwrap().unwrap_err().0, 501);
    // Other recordings are not in the bundle; other routes are not exports.
    assert_eq!(exports::answer(&flight, "/api/recordings/other.db/csv").unwrap().unwrap_err().0, 404);
    assert!(exports::answer(&flight, "/api/recordings").is_none());
    assert!(exports::answer(&flight, "/api/flight_review/recording/x").is_none());
}

#[test]
fn the_flight_prediction_tabs_read_the_local_station() {
    // Demo: the phase-coloured track comes from the rows and the state timeline.
    let station = LocalStation::new(demo::session(0), EPOCH);
    let _ = station.frames(EPOCH + 120_000);
    let track: crate::telemetry_dashboard::map_3d::FlightTrack = serde_json::from_value(station.get_json_for_test("/api/flight_phases", EPOCH + 120_000)).unwrap();
    assert!(track.points.len() > 10);
    let phases: Vec<&str> = track.phases.iter().map(|p| p.phase.as_str()).collect();
    assert_eq!(phases.first(), Some(&"pad"));
    assert!(phases.contains(&"boost") && phases.contains(&"coast"), "{phases:?}");
    assert!(track.points.iter().all(|p| p.timestamp_ms <= EPOCH + 120_000), "stamped on the playback clock");
    let plans: crate::telemetry_dashboard::flight_plans::PlanList = decode(&station, "/api/flight_plans");
    assert!(plans.plans.is_empty(), "the demo has no plans");
    let search: crate::telemetry_dashboard::recovery_search::Status = decode(&station, "/api/recovery_search");
    assert!(!search.active);
    let simulate = station.request("POST", "/api/los_prediction/simulate", Some(b"{}"));
    assert_eq!(simulate.status, 501, "a station computation, not a write to acknowledge");
    assert_eq!(station.request("GET", "/api/terrain/heightmap?lat=1&lon=2&extent_m=2000&size=129", None).text().contains("\"available\":false"), true);
}

/// An SD-resolved recording's bundle: the resolved CSV is the recording's
/// CSV route, the rows as received answer the time range, and a bundle
/// without them (an older station's) says the range needs the station.
#[test]
fn an_sd_resolved_bundle_answers_both_timelines() {
    let session = bundle::open("sd.gsreplay", SD_BUNDLE).unwrap();
    let id = session.manifest.as_ref().unwrap()["source"]["recording"].as_str().unwrap().to_string();
    assert_eq!(session.manifest.as_ref().unwrap()["sd"]["receipt_csv"]["included"], true);
    let station = LocalStation::new(session, EPOCH);
    let csv = station.request("GET", &format!("/api/recordings/{id}/csv"), None);
    assert!(csv.body.starts_with(b"id,received_timestamp_ms,source_timestamp_ms,sender_id,data_type,values_json,payload_json,canonical_utc_ms,provenance_json"));
    let sources: Value = decode(&station, &format!("/api/recordings/{id}/sd/sources"));
    let sha = sources["sources"][0]["file_sha256"].as_str().unwrap().to_owned();
    let original = station.request("GET", &format!("/api/recordings/{id}/sd/sources/{sha}"), None);
    assert_eq!(original.status, 200, "the original SD file is bundled");
    assert!(original.body.starts_with(b"timestamp_ms,monotonic_ms,sensor"));
    let mut files = bundle::unzip(SD_BUNDLE).unwrap();
    files.remove("data/receipt_telemetry.csv");
    let older = exports::ExportInputs::from_bundle(&id, std::rc::Rc::new(files["data/telemetry.csv"].clone()), &files);
    let snapshot: vehicle_replay::Snapshot = decode(&station, &format!("/api/recordings/{id}/replay"));
    let (code, message) = exports::answer(&older, &format!("/api/recordings/csv?start_ms={}&end_ms={}", snapshot.start_ms.unwrap(), snapshot.end_ms.unwrap() + 1)).unwrap().unwrap_err();
    assert_eq!(code, 501);
    assert!(message.contains("rows as received"), "{message}");
}

/// Station records a bundle carries (checklists, recovery history) are
/// served byte for byte under their routes, the bundled checklist document
/// replaces the empty one, and the demo station answers the same routes in
/// the station's shapes.
#[test]
fn station_records_are_served_from_the_bundle_and_by_the_demo() {
    let mut session = bundle::open("flight.gsreplay", BUNDLE).unwrap();
    let records = [
        ("/api/checklists", &br#"{"revision":7,"signoffs":[],"completion_events":[],"enforce_before_launch":true,"checklists":[]}"#[..], "application/json"),
        ("/api/checklists/export", br#"{"revision":7,"signoffs":[],"completion_events":[],"enforce_before_launch":true,"checklists":[]}"#, "application/json"),
        ("/api/checklists/export.csv", b"checklist_id,checklist_title,status,signed_by,signed_at_ms,signed_revision,content_digest,invalidated_at_ms,invalidation_reason\r\n", "text/csv"),
        ("/api/recovery/export.json", br#"{"version":1,"events":[]}"#, "application/json"),
        ("/api/recovery/export.csv", b"event_id,launch_session\r\n", "text/csv"),
    ];
    let empty = LocalStation::new(bundle::open("flight.gsreplay", BUNDLE).unwrap(), EPOCH);
    assert_eq!(empty.request("GET", "/api/recovery/export.csv", None).status, 404, "a bundle without records says so");
    for (route, body, media_type) in records {
        session.routes.insert(route.to_owned(), RouteEntry { body: std::rc::Rc::new(body.to_vec()), media_type: media_type.to_owned() });
    }
    let station = LocalStation::new(session, EPOCH);
    for (route, body, _) in records {
        let reply = station.request("GET", route, None);
        assert_eq!((reply.status, reply.body.as_slice()), (200, body), "{route}");
    }
    let demo = LocalStation::new(demo::session(0), EPOCH);
    let document: Value = decode(&demo, "/api/checklists/export");
    assert_eq!(document, decode::<Value>(&demo, "/api/checklists"), "the export is the document the tab shows");
    for key in ["revision", "signoffs", "completion_events", "enforce_before_launch", "checklists"] {
        assert!(document.get(key).is_some(), "{key}");
    }
    let csv = demo.request("GET", "/api/checklists/export.csv", None).text();
    assert!(csv.starts_with("checklist_id,checklist_title,status,signed_by,signed_at_ms,signed_revision,content_digest,invalidated_at_ms,invalidation_reason\r\n"));
    assert!(csv.contains("\"pad\",\"Pad checks (demo)\",unsigned,,,,,,\r\n") && csv.contains(",\"signed_off\","), "{csv}");
    let recovery: Value = decode(&demo, "/api/recovery/export.json");
    assert_eq!(recovery["events"][0]["launch_session"], recovery["current"]["session"]);
    let csv = demo.request("GET", "/api/recovery/export.csv", None).text();
    assert!(csv.starts_with("event_id,launch_session,authority,clock_revision,t0_utc_ms,reached_utc_ms,t_relative_ms,actor,auth_session,station,primary_verified\r\n\"demo-recovery-1\""), "{csv}");
}

/// The station's custom-window reviews and review exports (`raster=false`)
/// of three bundles (GroundStation26 `contract_fixtures`,
/// `replay-bundle/review/requests.json`): the bundled flight recording, a
/// flight capture and a test-fire capture with its DAQ launch file and
/// applied plan. Offline answers are the same bytes (summaries: the same JSON).
#[test]
fn offline_review_windows_equal_the_station_byte_for_byte() {
    // The station's clock while it answered (contract_fixtures `T0`).
    const STATION_NOW: i64 = 1_767_225_600_000;
    let open = |name: &str, bytes: &[u8]| bundle::open(name, bytes).unwrap().exports.clone().expect("a bundle answers exports");
    let bundles = [
        ("flight", open("flight.gsreplay", BUNDLE)),
        ("flight_capture", open("flight_capture.gsreplay", &export_fixture("review/flight_capture.gsreplay"))),
        ("test_fire", open("test_fire.gsreplay", &export_fixture("review/test_fire.gsreplay"))),
    ];
    let requests: Value = serde_json::from_slice(&export_fixture("review/requests.json")).unwrap();
    let mut compared = 0;
    for request in requests.as_array().unwrap() {
        let path = request["path"].as_str().unwrap();
        let inputs = &bundles.iter().find(|(name, _)| request["bundle"] == *name).unwrap().1;
        let answer = exports::answer_at(inputs, path, STATION_NOW).unwrap_or_else(|| panic!("{path} is answered offline"));
        let status = request["status"].as_u64().unwrap() as u16;
        match answer {
            Ok(file) => {
                assert_eq!(status, 200, "{path}: the station answered {status} {}", request["body"]);
                let file_name = request["file"].as_str().unwrap();
                let expected = export_fixture(file_name);
                if file_name.ends_with(".json") {
                    let ours: Value = serde_json::from_slice(&file.bytes).unwrap();
                    assert_eq!(ours, serde_json::from_slice::<Value>(&expected).unwrap(), "{path}: review differs from {file_name}");
                } else {
                    if file.bytes != expected {
                        let ours = crate::telemetry_dashboard::local_station::bundle::unzip(&file.bytes).unwrap();
                        let theirs = crate::telemetry_dashboard::local_station::bundle::unzip(&expected).unwrap();
                        for (name, station) in &theirs {
                            let mine = ours.get(name).cloned().unwrap_or_default();
                            if &mine != station {
                                let at = mine.iter().zip(station.iter()).position(|(a, b)| a != b).unwrap_or(mine.len().min(station.len()));
                                let show = |b: &[u8]| String::from_utf8_lossy(&b[at.saturating_sub(200).min(b.len())..(at + 300).min(b.len())]).into_owned();
                                panic!("{path}: {name} differs at byte {at}\n--- offline\n{}\n--- station\n{}", show(&mine), show(station));
                            }
                        }
                        assert_eq!(ours.keys().collect::<Vec<_>>(), theirs.keys().collect::<Vec<_>>(), "{path}: entries");
                        panic!("{path}: entries equal but the archive bytes differ from {file_name}");
                    }
                    assert_eq!(file.content_type, "application/zip");
                }
            }
            Err((code, message)) => {
                assert_eq!((code, message.as_str()), (status, request["body"].as_str().unwrap_or_default()), "{path}");
            }
        }
        compared += 1;
    }
    assert!(compared >= 12, "{compared} requests");
}
