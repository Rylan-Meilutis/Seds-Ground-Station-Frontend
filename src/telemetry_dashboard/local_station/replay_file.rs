//! A portable replay file (`*.seds-replay.json`) opened with no station.
//!
//! The station's `GET /api/recordings/{id}/replay/export` writes the samples;
//! the app attaches `layout`, `view_config`, `valve_mappings` and, from this
//! version, `boards` and `vehicle_geometry` before saving
//! (`vehicle_tab::ReplayFileExport`). Every attached field is optional so
//! older files still open: without a layout every recorded type is shown
//! under its own name, without boards the senders are listed, without a mode
//! it is inferred from the senders (`docs/backend-contract.md`, "Offline
//! replay files").
use super::{Alert, Board, Mode, Sample, Session, SessionParts, Source};
use serde_json::Value;

/// Same ceiling as the station export and the in-dashboard importer.
pub(crate) const MAX_BYTES: usize = 64 * 1024 * 1024;

fn mode_of(file: &Value, samples: &[Sample]) -> Mode {
    let recorded = file["replay_context"]["recorded_mode"].as_str().or_else(|| file["mode"].as_str()).unwrap_or("");
    match recorded {
        "test_fire" => Mode::TestFire,
        "hitl" => Mode::Hitl,
        "integrated_ground_test" | "igt" | "testing" => Mode::GroundTest,
        "normal" | "flight" => {
            if file["replay_context"]["hitl"].as_bool() == Some(true) {
                Mode::Hitl
            } else if file["replay_context"]["igt"].as_bool() == Some(true) {
                Mode::GroundTest
            } else {
                Mode::Flight
            }
        }
        _ => {
            if samples.iter().any(|s| s.sender == "FC") || file["layout"]["test_fire_mode"].as_bool() == Some(false) {
                Mode::Flight
            } else {
                Mode::TestFire
            }
        }
    }
}

fn state_name(sample: &Value) -> Option<String> {
    if let Some(phase) = sample["phase"].as_str() {
        return Some(phase.to_string());
    }
    // FC FLIGHT_STATE codes as the station numbers them (GroundStation26 `types::FlightState`).
    const NAMES: [&str; 16] = [
        "Startup", "Idle", "PreFill", "FillTest", "NitrogenFill", "NitrousFill", "Armed", "Launch", "Ascent", "Coast", "Apogee", "Descent", "Reefing", "Landed", "Recovery", "Aborted",
    ];
    let code = sample["values"][0].as_f64()?;
    NAMES.get(code as usize).map(|s| s.to_string())
}

pub(crate) fn open(file_name: &str, text: &str) -> Result<Session, String> {
    if text.len() > MAX_BYTES {
        return Err("Replay exceeds 64 MiB.".into());
    }
    let file: Value = serde_json::from_str(text).map_err(|e| format!("Not a JSON replay file: {e}"))?;
    if file["format"] != "seds-replay" {
        return Err("Not a SEDS replay file (expected \"format\": \"seds-replay\"). Export one from Data → Recordings or 3D replay.".into());
    }
    let version = file["version"].as_u64().unwrap_or(1);
    if version > 1 {
        return Err(format!("Replay format version {version} is newer than this app understands (1). Update the app."));
    }
    let rows = file["samples"].as_array().ok_or("The replay has no samples.")?;
    let mut samples = Vec::with_capacity(rows.len());
    let mut states = Vec::new();
    for row in rows {
        let Some(t_ms) = row["timestamp_ms"].as_i64() else { continue };
        let data_type = row["data_type"].as_str().unwrap_or_default().to_string();
        let sender = row["sender_id"].as_str().unwrap_or_default().to_string();
        if data_type == "FLIGHT_STATE"
            && let Some(state) = state_name(row)
            && states.last().is_none_or(|(_, s): &(i64, String)| s != &state)
        {
            states.push((t_ms, state));
        }
        let values = row["values"].as_array().map(|v| v.iter().map(|x| x.as_f64().map(|f| f as f32)).collect()).unwrap_or_default();
        samples.push(Sample { t_ms, sender, data_type, values });
    }
    if samples.is_empty() {
        return Err("The replay has no samples.".into());
    }
    let mode = mode_of(&file, &samples);
    let boards = file["boards"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|b| {
            let sender = b["sender_id"].as_str()?;
            Some(Board { sender: sender.into(), label: b["board_label"].as_str().filter(|l| !l.is_empty()).unwrap_or(sender).into() })
        })
        .collect();
    let mut notes = Vec::new();
    let layout = if file["layout"].is_object() {
        file["layout"].clone()
    } else {
        notes.push("This file carries no station layout: every recorded type is listed under its own name.".to_string());
        Value::Object(Default::default())
    };
    if let Some(provenance) = file["configuration_provenance"].as_str() {
        notes.push(provenance.to_string());
    }
    let vehicle_geometry = file["vehicle_geometry"].as_object().map(|g| Value::Object(g.clone()));
    if vehicle_geometry.is_none() {
        notes.push("No vehicle geometry in this file: the 3D view uses the generic rocket.".to_string());
    }
    let alerts = file["alerts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| Some(Alert { t_ms: a["timestamp_ms"].as_i64()?, warning: a["severity"] != "error", message: a["message"].as_str()?.to_string() }))
        .collect();
    let provenance = file["replay_context"]["provenance"].as_str().map(|p| match p {
        "hardware" => "Recorded".to_string(),
        "firmware_sim" | "internal_flight_sim" | "simulated" => "Recorded (simulated)".to_string(),
        other => format!("Recorded ({other})"),
    });
    let mut document = file.clone();
    document["mode"] = mode.player_mode().into();
    Ok(Session::new(SessionParts {
        source: Source::Replay { file_name: file_name.to_string() },
        title: file["recording"].as_str().filter(|r| !r.is_empty()).unwrap_or(file_name).to_string(),
        mode,
        provenance: provenance.unwrap_or_else(|| "Recorded".into()),
        layout,
        boards,
        samples,
        states,
        alerts,
        calibration: Vec::new(),
        recordings: Vec::new(),
        replay_document: Some(document),
        vehicle_geometry,
        notes,
        epoch_ms: 0,
    }))
}
