//! The flight replay engine (`GSOfflineReplay` in `offline-replay.js`) on a
//! connected station: a whole saved recording, or the live flight followed as
//! it arrives. Reads only; it sends no command.
//!
//! - Recording: `GET /api/recordings/{id}/replay/export` (the rows), plus the
//!   optional `/replay` snapshot and `/replay/trajectory`, the flight review,
//!   `/api/antenna/status` (station positions), the heightmap under the track
//!   (`/api/terrain/heightmap`, see `replay_terrain`) and the latest flight plans as
//!   overlays (`/api/flight_plans`). A missing optional route only removes its part.
//! - Live: the dashboard's own telemetry store (no extra request for rows); the
//!   player follows the newest data until the operator scrubs.
use super::{UrlConfig, http_get_json, js_eval};
use dioxus::prelude::*;
use serde_json::{Value, json};

static PANEL_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Data types the live view follows: GPS, flight state and the HUD channels.
/// The player binds them through the layout; absent types show a dash.
pub(super) const LIVE_TYPES: [&str; 14] = [
    "GPS_DATA", "GPS", "ROCKET_GPS", "FLIGHT_STATE", "BAROMETER_DATA", "VEHICLE_SPEED", "ACCEL_DATA",
    "COMBUSTION_CHAMBER_PRESSURE", "OX_TANK_PRESSURE", "KG1000", "BATTERY_VOLTAGE", "GPS_STATUS",
    "ASCENT_STATE", "DESCENT_STATE",
];

fn encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The export completed into a document the player accepts (it requires these sections).
pub(super) fn complete_document(mut file: Value, layout: Option<Value>, geometry: Option<Value>) -> Value {
    if !file.is_object() {
        file = json!({});
    }
    if file["layout"].is_null() && let Some(layout) = layout {
        file["layout"] = layout;
    }
    for (field, empty) in [("schema", json!({})), ("view_config", json!({"stages": [], "ground_systems": []})), ("layout", json!({})), ("valve_mappings", json!([]))] {
        if !file[field].is_object() && !file[field].is_array() {
            file[field] = empty;
        }
    }
    if file["vehicle_geometry"].is_null() && let Some(g) = geometry {
        file["vehicle_geometry"] = g;
    }
    if file["mode"].is_null() {
        file["mode"] = if file["layout"]["test_fire_mode"].as_bool() == Some(true) { "test_fire" } else { "flight" }.into();
    }
    file
}

/// Overlays of up to five flight plans (their MapOverlay), for comparison.
pub(super) fn plan_overlays(plans: &[Value]) -> Vec<Value> {
    plans.iter().filter_map(|p| p.get("overlay").filter(|o| o.is_object()).cloned()).take(5).collect()
}

async fn optional(path: &str) -> Option<Value> {
    http_get_json::<Value>(path).await.ok()
}

async fn recording_document(id: &str) -> Result<(Value, Value), String> {
    let base = format!("/api/recordings/{}", encode(id));
    let export = http_get_json::<Value>(&format!("{base}/replay/export")).await?;
    let layout = if export["layout"].is_null() { optional("/api/layout").await } else { None };
    let geometry = super::vehicle_geometry::station_json_now().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let document = complete_document(export, layout, geometry);
    let mut extras = serde_json::Map::new();
    if let Some(snapshot) = optional(&format!("{base}/replay")).await
        && let Some(upper) = snapshot["upper_id"].as_i64()
        && let Some(track) = optional(&format!("{base}/replay/trajectory?upper_id={upper}")).await
    {
        extras.insert("trajectory".into(), track);
    }
    if let Some(review) = optional(&format!("/api/flight_review/recording/{}", encode(id))).await {
        extras.insert("flight_review".into(), review);
    }
    if let Some(stations) = optional("/api/antenna/status").await {
        extras.insert("stations".into(), stations);
    }
    // The ground under the track, as a replay bundle snapshots it (flat ground without it).
    if let Some(path) = super::replay_terrain::heightmap_path(&document)
        && let Some(terrain) = optional(&path).await.filter(|t| t["available"] == true)
    {
        extras.insert("terrain".into(), terrain);
    }
    extras.insert("overlays".into(), Value::Array(plan_overlays(&latest_plans().await)));
    Ok((document, Value::Object(extras)))
}

async fn latest_plans() -> Vec<Value> {
    let Some(list) = optional("/api/flight_plans").await else { return Vec::new() };
    let mut plans = Vec::new();
    for summary in list["plans"].as_array().into_iter().flatten().filter(|p| p["kind"] == "flight").take(5) {
        if let Some(id) = summary["id"].as_str()
            && let Some(plan) = optional(&format!("/api/flight_plans/{}", encode(id))).await
        {
            plans.push(plan);
        }
    }
    plans
}

fn mount_script(root: &str, document: &Value, options: &Value) -> Result<String, String> {
    let doc = serde_json::to_string(document).map_err(|e| e.to_string())?;
    let opts = serde_json::to_string(options).map_err(|e| e.to_string())?;
    let id = serde_json::to_string(root).map_err(|e| e.to_string())?;
    Ok(format!(
        "{}\ntry{{const root=document.getElementById({id});if(!root)return 'gone';root.__player?.dispose();root.__player=GSOfflineReplay.mount(root,{doc},{opts});return 'ok';}}catch(e){{return String(e&&e.message||e);}}",
        super::recording_download::offline_replay_script()?
    ))
}

/// Rows of the live store as the player takes them.
fn live_rows(rows: impl IntoIterator<Item = super::types::TelemetryRow>) -> Vec<Value> {
    rows.into_iter()
        .filter(|r| LIVE_TYPES.contains(&r.data_type.as_str()) || super::vehicle_bindings::is_gps_type(&r.data_type))
        .map(|r| json!({"timestamp_ms": r.timestamp_ms, "data_type": r.data_type, "sender_id": r.sender_id, "values": r.values}))
        .collect()
}

#[component]
pub(super) fn FlightReplayPanel(recording: String) -> Element {
    let id = use_hook(|| PANEL_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let root = format!("gs26-flight-replay-{id}");
    let mut status = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut live = use_signal(|| false);
    let mut live_marks = use_signal(std::collections::BTreeMap::<String, i64>::new);
    // Live: push the newest row of each followed type when it changes.
    let live_root = root.clone();
    use_future(move || {
        let live_root = live_root.clone();
        async move {
            loop {
                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::TimeoutFuture::new(500).await;
                #[cfg(not(target_arch = "wasm32"))]
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if !*live.read() {
                    continue;
                }
                let mut fresh = Vec::new();
                let mut marks = live_marks.read().clone();
                // Plus the GPS types the layout names, when they differ.
                let gps = super::vehicle_bindings::gps_types();
                let kinds = LIVE_TYPES.iter().copied().chain(gps.iter().map(String::as_str).filter(|t| !LIVE_TYPES.contains(t)));
                for kind in kinds {
                    if let Some(row) = super::latest_telemetry_row(kind, None)
                        && marks.get(kind).is_none_or(|t| *t < row.timestamp_ms)
                    {
                        marks.insert(kind.to_string(), row.timestamp_ms);
                        fresh.push(row);
                    }
                }
                if fresh.is_empty() {
                    continue;
                }
                live_marks.set(marks);
                let rows = serde_json::to_string(&live_rows(fresh)).unwrap_or_else(|_| "[]".into());
                let target = serde_json::to_string(&live_root).unwrap_or_default();
                js_eval(&format!("(()=>{{const r=document.getElementById({target});r?.__player?.append?.({rows});}})();"));
            }
        }
    });
    let open_recording = {
        let root = root.clone();
        let recording = recording.clone();
        move |_| {
            let root = root.clone();
            let recording = recording.clone();
            live.set(false);
            busy.set(true);
            status.set("Loading the whole recording…".into());
            spawn(async move {
                let origin = UrlConfig::base_http();
                let result = async {
                    let (document, extras) = recording_document(&recording).await?;
                    if UrlConfig::base_http() != origin {
                        return Err("Station changed; open the replay again".to_string());
                    }
                    let script = mount_script(&root, &document, &json!({"extras": extras}))?;
                    document::eval(&script).join::<String>().await.map_err(|e| e.to_string())
                }
                .await;
                status.set(match result {
                    Ok(text) if text == "ok" => "Flight replay loaded.".into(),
                    Ok(text) | Err(text) => format!("Flight replay unavailable: {text}"),
                });
                busy.set(false);
            });
        }
    };
    let open_live = {
        let root = root.clone();
        move |_| {
            let root = root.clone();
            busy.set(true);
            spawn(async move {
                let layout = optional("/api/layout").await;
                let geometry = super::vehicle_geometry::station_json_now().and_then(|t| serde_json::from_str::<Value>(&t).ok());
                let mut document = complete_document(json!({"format": "seds-replay", "version": 1, "recording": "Live telemetry", "samples": live_rows(super::ui_telemetry_rows_snapshot().into_iter().rev().take(20_000).collect::<Vec<_>>().into_iter().rev())}), layout, geometry);
                if let Some(rows) = document["samples"].as_array_mut() {
                    rows.sort_by_key(|r| r["timestamp_ms"].as_i64().unwrap_or(0));
                    for (i, row) in rows.iter_mut().enumerate() {
                        row["id"] = json!(i + 1);
                    }
                }
                let stations = optional("/api/antenna/status").await;
                let overlays = plan_overlays(&latest_plans().await);
                let result = match mount_script(&root, &document, &json!({"live": true, "extras": {"stations": stations, "overlays": overlays}})) {
                    Ok(script) => document::eval(&script).join::<String>().await.map_err(|e| e.to_string()),
                    Err(e) => Err(e),
                };
                match result {
                    Ok(text) if text == "ok" => {
                        live_marks.set(Default::default());
                        live.set(true);
                        status.set("Following the live flight. Scrub to look back; Go live returns.".into());
                    }
                    Ok(text) | Err(text) => status.set(format!("Live flight view unavailable: {text}")),
                }
                busy.set(false);
            });
        }
    };
    rsx! {
        section { style: "padding:12px;border:1px solid #64748b;border-radius:10px;margin-bottom:16px;min-width:0;",
            h3 { "Flight replay" }
            p { "The flight as flight states: the vehicle with its plume and chutes, a phase timeline with events, the recorded sensors in a HUD and charts, and camera modes. Reads only." }
            div { style: "display:flex;gap:8px;flex-wrap:wrap;",
                button { disabled: recording.is_empty() || busy(), onclick: open_recording, "Open flight replay of the selected recording" }
                button { disabled: busy(), onclick: open_live, if live() { "Restart live flight view" } else { "Follow the live flight" } }
                if live() { button { onclick: move |_| live.set(false), "Stop following" } }
            }
            p { role: "status", "{status}" }
            div { id: "{root}", style: "min-width:0;" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_export_without_sections_becomes_a_playable_document() {
        let doc = complete_document(json!({"format": "seds-replay", "version": 1, "samples": []}), Some(json!({"test_fire_mode": true})), Some(json!({"schema": "gs26.vehicle_geometry"})));
        assert!(doc["schema"].is_object() && doc["view_config"]["stages"].is_array() && doc["valve_mappings"].is_array());
        assert_eq!(doc["mode"], "test_fire");
        assert_eq!(doc["vehicle_geometry"]["schema"], "gs26.vehicle_geometry");
        let kept = complete_document(json!({"layout": {"a": 1}, "mode": "flight"}), Some(json!({"b": 2})), None);
        assert_eq!(kept["layout"], json!({"a": 1}), "the export's own layout wins");
    }

    #[test]
    fn plan_overlays_take_the_plans_map_overlays() {
        let plans = vec![json!({"overlay": {"id": "flight_plan:a"}}), json!({"overlay": null}), json!({})];
        assert_eq!(plan_overlays(&plans), vec![json!({"id": "flight_plan:a"})]);
    }

    #[test]
    fn live_rows_keep_only_followed_types() {
        let row = |t: &str| serde_json::from_value::<super::super::types::TelemetryRow>(json!({"data_type": t, "sender_id": "FC", "timestamp_ms": 5, "values": [1.0]})).unwrap();
        let rows = live_rows([row("GPS_DATA"), row("HEARTBEAT"), row("FLIGHT_STATE")]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["data_type"], "GPS_DATA");
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
    }
}
