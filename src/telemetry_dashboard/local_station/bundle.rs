//! Self-contained replay bundles (`*.gsreplay`, `docs/replay-bundle.md`).
//!
//! A bundle is a ZIP file written by the station's
//! `GET /api/recordings/{id}/bundle`. Every entry is the body of one station
//! route, and `manifest.json` lists them with their SHA-256. This module reads
//! the archive (stored or deflated entries, no ZIP64), checks the manifest and
//! the hashes, and builds the same [`Session`] a replay file gives. The extra
//! entries (calibration, alerts, diagrams, map tiles, derived summaries and
//! the recording CSV) fill the pages a replay file leaves empty.
use super::{CalibrationCurve, Session, replay_file};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(crate) const FORMAT: &str = "gsreplay";
pub(crate) const VERSION: u64 = 1;
/// Unpacked size ceiling: the replay export's 64 MiB plus configuration, CSV and tiles.
const MAX_UNPACKED: usize = 192 << 20;

pub(crate) fn is_bundle(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

fn u16_at(b: &[u8], i: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?) as usize)
}
fn u32_at(b: &[u8], i: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize)
}

/// Every entry of a ZIP archive, by name.
pub(crate) fn unzip(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let bad = || "The bundle is not a readable ZIP archive.".to_string();
    // End of central directory: the last 0x06054b50 within the trailing 64 KiB.
    let tail = bytes.len().saturating_sub(65_557);
    let end = (tail..bytes.len().saturating_sub(21)).rev().find(|&i| u32_at(bytes, i) == Some(0x0605_4b50)).ok_or_else(bad)?;
    let count = u16_at(bytes, end + 10).ok_or_else(bad)?;
    let mut at = u32_at(bytes, end + 16).ok_or_else(bad)?;
    let mut files = BTreeMap::new();
    let mut unpacked = 0usize;
    for _ in 0..count {
        if u32_at(bytes, at) != Some(0x0201_4b50) {
            return Err(bad());
        }
        let method = u16_at(bytes, at + 10).ok_or_else(bad)?;
        let packed = u32_at(bytes, at + 20).ok_or_else(bad)?;
        let size = u32_at(bytes, at + 24).ok_or_else(bad)?;
        let (name_len, extra_len, comment_len) = (u16_at(bytes, at + 28).ok_or_else(bad)?, u16_at(bytes, at + 30).ok_or_else(bad)?, u16_at(bytes, at + 32).ok_or_else(bad)?);
        let local = u32_at(bytes, at + 42).ok_or_else(bad)?;
        let name = String::from_utf8(bytes.get(at + 46..at + 46 + name_len).ok_or_else(bad)?.to_vec()).map_err(|_| bad())?;
        at += 46 + name_len + extra_len + comment_len;
        if name.ends_with('/') {
            continue;
        }
        if name.starts_with('/') || name.split('/').any(|part| part == ".." || part.is_empty()) {
            return Err(format!("The bundle has an unsafe entry name: {name}"));
        }
        unpacked = unpacked.saturating_add(size);
        if unpacked > MAX_UNPACKED {
            return Err("The bundle unpacks to more than 192 MiB.".into());
        }
        if u32_at(bytes, local) != Some(0x0403_4b50) {
            return Err(bad());
        }
        let start = local + 30 + u16_at(bytes, local + 26).ok_or_else(bad)? + u16_at(bytes, local + 28).ok_or_else(bad)?;
        let body = bytes.get(start..start + packed).ok_or_else(bad)?;
        let data = match method {
            0 => body.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(body, size.max(1)).map_err(|_| format!("Entry {name} is damaged."))?,
            other => return Err(format!("Entry {name} uses ZIP method {other}; only stored and deflated entries are read.")),
        };
        if data.len() != size {
            return Err(format!("Entry {name} is damaged."));
        }
        files.insert(name, data);
    }
    Ok(files)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Checks the manifest and every listed hash; returns the manifest.
pub(crate) fn verify(files: &BTreeMap<String, Vec<u8>>) -> Result<Value, String> {
    let manifest: Value = serde_json::from_slice(files.get("manifest.json").ok_or("The bundle has no manifest.json.")?).map_err(|e| format!("manifest.json is not JSON: {e}"))?;
    if manifest["format"] != FORMAT {
        return Err("Not a ground-station replay bundle (manifest format is not \"gsreplay\").".into());
    }
    let version = manifest["version"].as_u64().unwrap_or(0);
    if version == 0 || version > VERSION {
        return Err(format!("Replay bundle version {version} is newer than this app understands ({VERSION}). Update the app."));
    }
    for entry in manifest["entries"].as_array().into_iter().flatten() {
        let path = entry["path"].as_str().unwrap_or_default();
        let data = files.get(path).ok_or_else(|| format!("The bundle lists {path} but does not contain it."))?;
        if entry["sha256"].as_str() != Some(sha256_hex(data).as_str()) {
            return Err(format!("{path} does not match its manifest hash; the bundle is damaged or was edited."));
        }
    }
    Ok(manifest)
}

fn json(files: &BTreeMap<String, Vec<u8>>, path: &str) -> Option<Value> {
    serde_json::from_slice(files.get(path)?).ok()
}

/// Calibration curves from `config/calibration.json` (`extra_channels`, and
/// the load-cell `points`) as the station serves `GET /api/calibration`.
fn calibration(document: &Value) -> Vec<CalibrationCurve> {
    let mut curves = Vec::new();
    for (name, channel) in document["extra_channels"].as_object().into_iter().flatten() {
        let points: Vec<(f32, f32)> = channel["points"].as_array().into_iter().flatten().filter_map(|p| Some((p["raw"].as_f64()? as f32, p["expected"].as_f64()? as f32))).collect();
        curves.push(CalibrationCurve {
            label: name.clone(),
            raw_unit: if document["version"].as_u64() == Some(2) { "V".into() } else { "raw".into() },
            unit: "value".into(),
            points,
            slope: channel["linear"]["m"].as_f64().unwrap_or(1.0) as f32,
            offset: channel["linear"]["b"].as_f64().unwrap_or(0.0) as f32,
        });
    }
    for (key, raw, label, fit) in [("points", "ch0_raw", "Loadcell channel 0", "ch0"), ("points_ch1", "ch1_raw", "Loadcell channel 1", "ch1")] {
        let points: Vec<(f32, f32)> = document[key].as_array().into_iter().flatten().filter_map(|p| Some((p[raw].as_f64()? as f32, p["kg"].as_f64()? as f32))).collect();
        if !points.is_empty() {
            curves.push(CalibrationCurve {
                label: label.into(),
                raw_unit: "V".into(),
                unit: "kg".into(),
                points,
                slope: document[fit]["m"].as_f64().unwrap_or(1.0) as f32,
                offset: document[fit]["b"].as_f64().unwrap_or(0.0) as f32,
            });
        }
    }
    curves
}

/// Scalar figures of a derived document, for the summary page.
fn figures(prefix: &str, value: &Value, out: &mut Vec<(String, String)>) {
    for (key, item) in value.as_object().into_iter().flatten() {
        let label = format!("{prefix}{}", key.replace('_', " "));
        match item {
            Value::Number(n) => out.push((label, n.as_f64().map(|f| format!("{f:.6}").trim_end_matches('0').trim_end_matches('.').to_string()).unwrap_or_default())),
            Value::String(s) if s.len() <= 80 => out.push((label, s.clone())),
            Value::Bool(b) => out.push((label, b.to_string())),
            Value::Object(_) if prefix.is_empty() && matches!(key.as_str(), "summary" | "flight" | "motor" | "maxima") => figures(&format!("{} · ", key.replace('_', " ")), item, out),
            _ => {}
        }
    }
}

pub(crate) fn open(file_name: &str, bytes: &[u8]) -> Result<Session, String> {
    let files = unzip(bytes)?;
    let manifest = verify(&files)?;
    let replay = files.get("replay.json").ok_or("The bundle has no replay.json.")?;
    let mut document: Value = serde_json::from_slice(replay).map_err(|e| format!("replay.json is not JSON: {e}"))?;
    // Configuration the replay document lacks comes from the bundle's own snapshot.
    for (field, path) in [("layout", "config/layout.json"), ("vehicle_geometry", "models/vehicle_geometry.json")] {
        if !document[field].is_object() && let Some(value) = json(&files, path) {
            document[field] = value;
        }
    }
    if !document["boards"].is_array() && let Some(boards) = json(&files, "config/boards.json") {
        document["boards"] = boards["boards"].clone();
    }
    if let Some(alerts) = json(&files, "derived/alerts.json") {
        document["alerts"] = alerts.get("alerts").cloned().unwrap_or(alerts);
    }
    // What the flight replay player uses beyond the rows: the backend track, the
    // flight review's events, a plan bundle's plan and the predicted flag, stations.
    let mut extras = serde_json::Map::new();
    for (key, path) in [
        ("trajectory", "derived/trajectory.json"),
        ("flight_review", "derived/flight_review.json"),
        ("flight_plan", "derived/flight_plan.json"),
        ("flight_plans", "derived/flight_plans.json"),
        ("stations", "derived/stations.json"),
    ] {
        if let Some(value) = json(&files, path) {
            extras.insert(key.into(), value);
        }
    }
    extras.insert("manifest".into(), serde_json::json!({"predicted": manifest["predicted"].as_bool() == Some(true), "label": manifest["label"].clone(), "mode": manifest["mode"].clone()}));
    if let Some(plans) = extras.get("flight_plans").and_then(|p| p["plans"].as_array()) {
        let overlays: Vec<Value> = plans.iter().filter_map(|p| p.get("overlay").filter(|o| o.is_object()).cloned()).take(5).collect();
        extras.insert("overlays".into(), Value::Array(overlays));
    }
    if !document["replay_extras"].is_object() {
        document["replay_extras"] = Value::Object(extras);
    }
    let text = serde_json::to_string(&document).map_err(|e| e.to_string())?;
    let mut session = replay_file::open(file_name, &text)?;
    session.title = manifest["source"]["recording"].as_str().unwrap_or(file_name).to_string();
    if let Some(cal) = json(&files, "config/calibration.json") {
        session.calibration = calibration(&cal);
    }
    if let Some(diagrams) = json(&files, "config/diagrams.json") {
        session.diagrams = diagrams["diagrams"].as_array().cloned().unwrap_or_default();
    }
    let mut derived = Vec::new();
    // Station settings that shaped the recorded values.
    if let Some(cal) = json(&files, "config/calibration.json") {
        if let Some(vref) = cal["mcp_vref_v"].as_f64() {
            derived.push(("DAQ MCP3564R reference (at export)".to_string(), format!("{vref} V")));
        }
        if let Some(input) = cal["kg50_input"].as_str() {
            derived.push(("50 kg loadcell input (at export)".to_string(), input.to_string()));
        }
    }
    for (path, prefix) in [("derived/flight_review.json", ""), ("derived/burn_reports.json", "burn report · ")] {
        if let Some(value) = json(&files, path) {
            let value = value.as_array().and_then(|a| a.first().cloned()).unwrap_or(value);
            figures(prefix, &value, &mut derived);
        }
    }
    session.derived = derived.into_iter().take(24).collect();
    for (path, data) in &files {
        let Some(rest) = path.strip_prefix("tiles/").and_then(|p| p.strip_suffix(".jpg")) else { continue };
        let parts: Vec<u32> = rest.split('/').filter_map(|p| p.parse().ok()).collect();
        if let [z, x, y] = parts[..] {
            session.tiles.insert((z, x, y), data.clone());
        }
    }
    session.csv = files.get("data/telemetry.csv").cloned().map(std::rc::Rc::new);
    session.notes.push(format!(
        "Bundle from {} ({}), {} entries, made {}.",
        manifest["source"]["station_name"].as_str().unwrap_or("a station"),
        manifest["generator"].as_str().unwrap_or("unknown version"),
        manifest["entries"].as_array().map_or(0, Vec::len),
        manifest["created_ms"].as_i64().map(|ms| format!("at {ms} ms (Unix)")).unwrap_or_default()
    ));
    session.manifest = Some(manifest);
    Ok(session)
}
