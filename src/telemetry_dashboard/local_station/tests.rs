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
