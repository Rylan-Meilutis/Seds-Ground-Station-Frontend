//! The firmware tab of a local station (demo mode or a replay): read-only,
//! what the recording holds about each board's firmware and health
//! (`GET /api/recordings/{id}/firmware`, a bundle's `derived/firmware.json`).
//! Flashing needs a station and is not offered.
use super::prelude::*;
use serde_json::Value;

/// The record the local station serves for its recording, or why there is none.
pub(crate) fn recorded_firmware() -> Result<Value, String> {
    let station = super::station_link::local().ok_or("No local station is selected.")?;
    let id = station.recording_id().ok_or("This session has no recording.")?;
    let path = format!("/api/recordings/{}/firmware", super::local_station::station::percent_component(&id));
    let reply = station.request("GET", &path, None);
    if !reply.ok() {
        return Err(reply.text());
    }
    serde_json::from_slice(&reply.body).map_err(|e| e.to_string())
}

fn uptime(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    match s {
        0..60 => format!("{s} s"),
        60..3600 => format!("{} min {} s", s / 60, s % 60),
        _ => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

/// One line per firmware field the record names.
pub(crate) fn firmware_lines(firmware: &Value) -> Vec<String> {
    let text = |k: &str| firmware[k].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    let mut out = Vec::new();
    if let Some(build) = text("describe").or_else(|| text("short")).or_else(|| text("commit")) {
        out.push(format!("Build {build}"));
    }
    match (text("repo"), text("ref")) {
        (Some(repo), Some(r)) => out.push(format!("{repo} @ {r}")),
        (Some(repo), None) => out.push(repo),
        _ => {}
    }
    if let Some(commit) = text("commit") {
        out.push(format!("Commit {commit}"));
    }
    if let Some(subject) = text("subject") {
        out.push(subject);
    }
    if let Some(sdk) = text("sedsnet") {
        out.push(format!("SEDSnet {}", sdk.chars().take(12).collect::<String>()));
    }
    out
}

/// Health summary lines from the record's `health` object.
pub(crate) fn health_lines(health: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let reports = health["reports"].as_u64().unwrap_or(0);
    out.push(format!("{reports} health report{}", if reports == 1 { "" } else { "s" }));
    if let (Some(first), Some(last)) = (health["first_uptime_s"].as_f64(), health["last_uptime_s"].as_f64()) {
        out.push(format!("Uptime {} → {}", uptime(first), uptime(last)));
    }
    match health["resets_observed"].as_u64() {
        Some(0) | None => {}
        Some(n) => out.push(format!("{n} reset{} during the recording", if n == 1 { "" } else { "s" })),
    }
    let last = &health["last"];
    if let Some(centi) = last["die_temperature_centi_c"].as_i64() {
        out.push(format!("MCU die {:.1} °C", centi as f64 / 100.0));
    }
    if let Some(flags) = last["reset_flags"].as_u64().filter(|f| *f != 0) {
        out.push(format!("Reset flags 0x{flags:08X}"));
    }
    if let Some(mask) = health["watchdog_missing_mask"].as_u64().filter(|m| *m != 0) {
        out.push(format!("Watchdog missing mask 0x{mask:04X}"));
    }
    if let Some(faults) = last["service_faults"].as_array().filter(|f| !f.is_empty()) {
        out.push(format!("Service faults: {}", faults.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")));
    }
    if let Some(peak) = last["heap_peak_bytes"].as_u64() {
        out.push(format!("Heap peak {:.1} KiB", peak as f64 / 1024.0));
    }
    if let Some(n) = last["allocation_failures"].as_u64().filter(|n| *n != 0) {
        out.push(format!("{n} allocation failures"));
    }
    if let Some(n) = health["undecoded_reports"].as_u64().filter(|n| *n != 0) {
        out.push(format!("{n} report{} not readable", if n == 1 { "" } else { "s" }));
    }
    out
}

#[component]
pub(crate) fn RecordedFirmware(theme: ThemeConfig) -> Element {
    let record = use_hook(recorded_firmware);
    let panel = format!(
        "padding:18px; border:1px solid {}; border-radius:12px; background:{}; color:{};",
        theme.border, theme.panel_background, theme.text_primary
    );
    let accent = theme.main_tab_accents.get("firmware-update").cloned().unwrap_or_else(|| theme.info_accent.clone());
    let provenance = record.as_ref().ok().and_then(|r| r["provenance"].as_str()).map(|p| match p {
        "hardware" => "hardware".to_string(),
        "firmware_sim" => "simulated board firmware".to_string(),
        "internal_flight_sim" => "the internal flight simulation".to_string(),
        "demo" => "the demo scenario (example builds)".to_string(),
        other => other.to_string(),
    });
    rsx! {
        div { style: "height:100%; overflow-y:auto; padding:16px; display:flex; flex-direction:column; gap:14px; background:{theme.app_background};",
            div { style: "{panel}",
                h2 { style: "margin:0 0 8px; color:{accent};", "Firmware" }
                p { style: "margin:0; color:{theme.text_secondary}; line-height:1.45;",
                    "Recorded firmware and board health, read-only. Flashing needs a station: connect to a Ground Station to update a board."
                }
                if let Some(p) = provenance.as_ref() {
                    p { style: "margin:8px 0 0; color:{theme.text_muted};", "Data from {p}." }
                }
            }
            match record.as_ref() {
                Err(message) => rsx! {
                    div { style: "{panel} color:{theme.text_secondary};", "No firmware record: {message}" }
                },
                Ok(record) => rsx! {
                    if let Some(note) = record["identity_note"].as_str() {
                        div { style: "padding:12px; border:1px solid {theme.border}; border-radius:10px; color:{theme.text_secondary}; line-height:1.45;", "{note}" }
                    }
                    if let Some(version) = record["station"]["groundstation_backend"].as_str() {
                        div { style: "color:{theme.text_muted};", "Recorded by Ground Station {version}" }
                    }
                    div { style: "display:grid; grid-template-columns:repeat(auto-fill, minmax(260px, 1fr)); gap:12px;",
                        for board in record["boards"].as_array().cloned().unwrap_or_default() {
                            div { key: "{board[\"sender_id\"]}", style: "{panel} display:grid; gap:6px;",
                                strong { "{board[\"board_label\"].as_str().unwrap_or_default()} ({board[\"sender_id\"].as_str().unwrap_or_default()})" }
                                if board["firmware"].is_object() {
                                    for line in firmware_lines(&board["firmware"]) {
                                        div { style: "font-family:monospace; font-size:0.9rem; overflow-wrap:anywhere;", "{line}" }
                                    }
                                } else {
                                    div { style: "color:{theme.text_muted};", "Firmware build not recorded" }
                                }
                                div { style: "color:{theme.text_secondary}; font-size:0.9rem;", "{board[\"rows\"].as_u64().unwrap_or(0)} rows recorded" }
                                if board["health"].is_object() {
                                    for line in health_lines(&board["health"]) {
                                        div { style: "color:{theme.text_secondary}; font-size:0.9rem;", "{line}" }
                                    }
                                } else {
                                    div { style: "color:{theme.text_muted}; font-size:0.9rem;", "No health reports recorded" }
                                }
                            }
                        }
                    }
                    if let Some(updates) = record["updates"].as_array().filter(|u| !u.is_empty()) {
                        div { style: "{panel} display:grid; gap:6px;",
                            strong { "Firmware transfers during the recording" }
                            for update in updates.iter() {
                                div { style: "color:{theme.text_secondary};", "{update[\"board_label\"].as_str().unwrap_or_default()}: {update[\"filename\"].as_str().unwrap_or_default()} — {update[\"phase\"].as_str().unwrap_or_default()}" }
                            }
                        }
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn lines_name_the_build_and_the_health() {
        let firmware = json!({"repo": "Flight-Computer", "ref": "main", "commit": "0123456789abcdef", "short": "0123456", "describe": "v1.2-3-g0123456", "sedsnet": "abcdef0123456789"});
        assert_eq!(firmware_lines(&firmware), ["Build v1.2-3-g0123456", "Flight-Computer @ main", "Commit 0123456789abcdef", "SEDSnet abcdef012345"]);
        let health = json!({"reports": 3, "first_uptime_s": 5.0, "last_uptime_s": 3725.0, "resets_observed": 1, "watchdog_missing_mask": 0,
            "last": {"die_temperature_centi_c": 2712, "reset_flags": 0, "heap_peak_bytes": 2048}});
        assert_eq!(health_lines(&health), ["3 health reports", "Uptime 5 s → 1 h 2 min", "1 reset during the recording", "MCU die 27.1 °C", "Heap peak 2.0 KiB"]);
    }
}
