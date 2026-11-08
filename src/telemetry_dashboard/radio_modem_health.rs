//! Result of the backend's RFD modem check (startup or operator request).
use super::layout::ThemeConfig;
use dioxus::prelude::*;
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct ModemRow {
    pub number: u8,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub observed: Option<i64>,
    #[serde(default)]
    pub expected: Option<i64>,
    #[serde(default)]
    pub policy: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct ModemReport {
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub trigger: String,
    #[serde(default)]
    pub identity: Option<String>,
    #[serde(default)]
    pub registers: Vec<ModemRow>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub eeprom_saved: bool,
    #[serde(default)]
    pub data_mode: String,
}

/// The RF board's own startup modem check, from its BOARD_HEALTH.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct RfModem {
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub needs_attention: bool,
    #[serde(default)]
    pub eeprom_saved: bool,
    #[serde(default)]
    pub forwarding_entries: bool,
    #[serde(default)]
    pub max_data: Option<u16>,
    #[serde(default)]
    pub max_retries: Option<u16>,
    #[serde(default)]
    pub global_retries: Option<u16>,
    #[serde(default)]
    pub serial_break_ms: Option<u16>,
    #[serde(default)]
    pub dest_id: Option<u16>,
    #[serde(default)]
    pub rtscts: Option<u16>,
}

/// Raw-UART radio receive quality reported by the backend.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct ReceiveQuality {
    #[serde(default)]
    pub good_frames_last_minute: u64,
    #[serde(default)]
    pub crc_failures_last_minute: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct ModemStatus {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub profile_name: Option<String>,
    #[serde(default)]
    pub opt_in_profile: bool,
    #[serde(default)]
    pub rolled_out_all_nodes: bool,
    #[serde(default)]
    pub configured_mode: String,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub reconfiguration_allowed: bool,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub needs_attention: bool,
    #[serde(default)]
    pub report: Option<ModemReport>,
    #[serde(default)]
    pub rf_board: Option<RfModem>,
    #[serde(default)]
    pub receive_quality: Option<ReceiveQuality>,
}

#[derive(Deserialize)]
struct CheckReply {
    accepted: bool,
}

pub(super) fn outcome_label(status: &ModemStatus) -> &'static str {
    if status.running {
        return "Checking the radio modem…";
    }
    match status.report.as_ref().map(|r| r.outcome.as_str()) {
        None => "Radio modem not checked yet",
        Some("matched") => "Radio modem settings match the profile",
        Some("applied") => "Radio modem settings corrected and verified",
        Some("reset") => "Radio modem reset and the serial port reopened",
        Some("mismatch") => "Radio modem settings differ from the profile",
        Some("skipped") => "Radio modem check skipped",
        Some("no_response") => "Radio modem did not answer; running on its current settings",
        Some(_) => "Radio modem check failed; running on its current settings",
    }
}

pub(super) fn rf_label(rf: Option<&RfModem>) -> &'static str {
    let Some(rf) = rf else {
        return "RF modem not reported";
    };
    if rf.pending {
        return "RF modem check pending";
    }
    match rf.outcome.as_str() {
        "matched" if !rf.needs_attention => "RF modem settings match the profile",
        "applied" if !rf.needs_attention => "RF modem settings corrected and verified",
        "matched" | "applied" => "RF modem settings match; see the warning",
        "mismatch" => "RF modem settings differ from the profile",
        "skipped" => "RF modem check skipped",
        "no_response" => "RF modem did not answer; running on its current settings",
        _ => "RF modem check failed; running on its current settings",
    }
}

/// S3/S4/S5/S6/S11/RTSCTS as the RF board read them.
pub(super) fn rf_values(rf: &RfModem) -> String {
    let v = |v: Option<u16>| v.map_or_else(|| "unread".to_string(), |v| v.to_string());
    format!(
        "S3 {} · S4 {} · S5 {} · S6 {} · S11 {} · RTSCTS {}",
        v(rf.max_data),
        v(rf.max_retries),
        v(rf.global_retries),
        v(rf.serial_break_ms),
        v(rf.dest_id),
        v(rf.rtscts)
    )
}

/// "Receive quality (last minute): 120 good frames, 3 CRC failures (2%)".
pub(super) fn receive_quality_line(q: &ReceiveQuality) -> Option<String> {
    let total = q.good_frames_last_minute + q.crc_failures_last_minute;
    (total > 0).then(|| {
        format!(
            "Receive quality (last minute): {} good frames, {} CRC failures ({}%)",
            q.good_frames_last_minute,
            q.crc_failures_last_minute,
            q.crc_failures_last_minute * 100 / total
        )
    })
}

/// Shown when the operator selected the opt-in low-latency profile.
pub(super) fn profile_notes(status: &ModemStatus) -> Vec<String> {
    let mut notes = Vec::new();
    if status.opt_in_profile {
        let name = status.profile_name.as_deref().unwrap_or("selected");
        notes.push(format!(
            "Opt-in profile {name}: it must be set on all three modems (GS, RRLS and the RF board via phase 2) and should first be confirmed with the hardware A/B test."
        ));
        if !status.rolled_out_all_nodes {
            notes.push(
                "Settings that must match across nodes are reported, not written, until this profile is marked as rolled out to all nodes."
                    .into(),
            );
        }
    }
    notes
}

fn value(v: Option<i64>) -> String {
    v.map_or_else(|| "unread".into(), |v| v.to_string())
}

/// Rows worth showing: anything that is not a clean match or plain info.
pub(super) fn attention_rows(report: &ModemReport) -> Vec<&ModemRow> {
    report
        .registers
        .iter()
        .filter(|r| r.status != "match" && r.status != "info")
        .collect()
}

#[component]
pub fn RadioModemHealth(ws_connected: bool, theme: ThemeConfig) -> Element {
    let mut status = use_signal(ModemStatus::default);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    use_future(move || async move {
        loop {
            // Older backends have no such route: the section stays hidden.
            if let Ok(next) = super::http_get_json::<ModemStatus>("/api/radio/modem").await {
                status.set(next);
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(2000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
    let state = status.read().clone();
    if !state.available {
        return rsx! {};
    }
    let allowed = ws_connected
        && state.reconfiguration_allowed
        && !state.running
        && !busy()
        && crate::auth::can_send_command("RadioModemCheck");
    let mut request = move |mode: &'static str| {
        if busy() {
            return;
        }
        let owner = match super::recording_download::ExportOwner::capture_ordinary_selected(|| true) {
            Ok(owner) => owner,
            Err(error) => {
                message.set(error);
                return;
            }
        };
        let call = super::station_settings_request::post_captured::<_, CheckReply>(
            "/api/radio/modem/check",
            &serde_json::json!({ "mode": mode }),
            Ok(owner.clone()),
        );
        busy.set(true);
        message.set(if mode == "reset" {
            "Requesting a radio reset; the radio pauses for a few seconds.".into()
        } else {
            "Requesting a modem check; the radio pauses for a few seconds.".into()
        });
        spawn(async move {
            let result = call.await;
            if !owner.is_current() {
                return;
            }
            match result {
                Ok(reply) if reply.accepted => message.set(if mode == "reset" { "Radio reset started.".into() } else { "Modem check started.".into() }),
                Ok(_) => message.set(if mode == "reset" { "Radio reset was not accepted.".into() } else { "Modem check was not accepted.".into() }),
                Err(error) => message.set(error),
            }
            busy.set(false);
        });
    };
    let label = outcome_label(&state);
    let warn = state.needs_attention && !state.running;
    rsx! {
        section { class: "gs26-radio-modem-health",
            style: "padding:16px; margin-bottom:12px; border:1px solid {theme.border_soft}; border-radius:12px; min-width:0; color:{theme.text_primary}; background:{theme.panel_background}; overflow-wrap:anywhere;",
            h3 { style: "margin:0;", "Radio modem" }
            p { role: if warn { "alert" } else { "status" }, style: "margin:8px 0;", strong { "{label}" } }
            if let Some(name) = &state.profile_name {
                p { style: "margin:4px 0; color:{theme.text_secondary};", "Profile: {name}" }
            }
            for note in profile_notes(&state) {
                p { style: "margin:4px 0;", "⚠ {note}" }
            }
            if let Some(report) = &state.report {
                p { style: "margin:4px 0; color:{theme.text_secondary};", "{report.summary}" }
                if let Some(identity) = &report.identity {
                    p { style: "margin:4px 0; color:{theme.text_secondary};", "Firmware: {identity}" }
                }
                if report.eeprom_saved {
                    p { style: "margin:4px 0; color:{theme.text_secondary};", "Settings were saved to the modem and it was restarted." }
                }
                for warning in &report.warnings {
                    p { style: "margin:4px 0;", "⚠ {warning}" }
                }
                {
                    let rows = attention_rows(report);
                    rsx! {
                        if !rows.is_empty() {
                            dl { style: "display:grid; grid-template-columns:auto 1fr; gap:4px 12px; font-size:0.9rem; margin:8px 0 0;",
                                for row in rows {
                                    dt { "S{row.number} {row.name.clone().unwrap_or_default()}" }
                                    dd { style: "margin:0;", "{value(row.observed)} (profile {value(row.expected)}) · {row.status}" }
                                }
                            }
                        }
                    }
                }
            }
            {
                let rf = state.rf_board.as_ref();
                let rf_warn = rf.is_some_and(|r| r.needs_attention && !r.pending);
                rsx! {
                    div { style: "margin:8px 0 0; padding-top:8px; border-top:1px solid {theme.border_soft};",
                        p { role: if rf_warn { "alert" } else { "status" }, style: "margin:4px 0;",
                            strong { "{rf_label(rf)}" } }
                        if let Some(rf) = rf {
                            p { style: "margin:4px 0; color:{theme.text_secondary};", "{rf.summary}" }
                            if !rf.pending {
                                if rf.outcome != "skipped" {
                                    p { style: "margin:4px 0; color:{theme.text_secondary}; font-size:0.9rem;", "{rf_values(rf)}" }
                                }
                                if rf.eeprom_saved {
                                    p { style: "margin:4px 0; color:{theme.text_secondary};", "The RF board saved settings to its modem and restarted it." }
                                }
                                if rf.forwarding_entries {
                                    p { style: "margin:4px 0;", "⚠ RF modem forwarding table has entries: modem-level relaying duplicates traffic beside the software RRLS relay" }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(line) = state.receive_quality.as_ref().and_then(receive_quality_line) {
                p { style: "margin:8px 0 0; color:{theme.text_secondary};", "{line}" }
            }
            if let Some(reason) = &state.blocked_reason {
                p { style: "margin:8px 0 0; color:{theme.text_secondary};", "Modem changes are unavailable: {reason}." }
            }
            div { style: "display:flex; gap:8px; flex-wrap:wrap; margin-top:8px;",
                button { r#type: "button", style: "min-height:44px;", disabled: !allowed,
                    onclick: move |_| request("verify"), "Verify modem" }
                button { r#type: "button", style: "min-height:44px;", disabled: !allowed,
                    onclick: move |_| request("fix"), "Fix modem settings" }
                button { r#type: "button", style: "min-height:44px;", disabled: !allowed,
                    title: "Reboot the radio modem (ATZ) and reopen its serial port. Settings are not changed.",
                    onclick: move |_| request("reset"), "Reset radio" }
            }
            if !message.read().is_empty() { p { role: "status", style: "margin:8px 0 0;", "{message}" } }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"available":true,"configured_mode":"fix","running":false,
        "reconfiguration_allowed":true,"blocked_reason":null,"needs_attention":true,
        "report":{"outcome":"mismatch","summary":"settings read; some differ","trigger":"startup",
        "mode":"fix","identity":"RFD ASYNC 4.03 on RFD900X2","map_source":"ati5_names",
        "registers":[{"number":2,"name":"AIR_SPEED","observed":125,"expected":64,"policy":"report_only","status":"differs"},
                     {"number":5,"name":"GLOBAL_RETRIES","observed":0,"expected":0,"policy":"fix","status":"match"},
                     {"number":12,"name":"TXPOWER","observed":30,"expected":null,"policy":"info","status":"info"}],
        "writes":[],"eeprom_saved":false,"save_attempted":false,"rebooted":false,"data_mode":"resumed",
        "forwarding":null,"warnings":["S2 is Some(125), profile expects Some(64)"],"finished_unix_ms":1,"duration_ms":2}}"#;

    #[test]
    fn backend_report_is_parsed_and_only_problem_rows_are_shown() {
        let status: ModemStatus = serde_json::from_str(SAMPLE).unwrap();
        assert!(status.available && status.needs_attention);
        let report = status.report.as_ref().unwrap();
        let rows = attention_rows(report);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].number, 2);
        assert_eq!(outcome_label(&status), "Radio modem settings differ from the profile");
    }

    #[test]
    fn running_and_unchecked_states_have_labels() {
        let mut status = ModemStatus { available: true, ..Default::default() };
        assert_eq!(outcome_label(&status), "Radio modem not checked yet");
        status.running = true;
        assert_eq!(outcome_label(&status), "Checking the radio modem…");
    }

    #[test]
    fn unknown_outcomes_are_failures_not_success() {
        let status = ModemStatus {
            available: true,
            report: Some(ModemReport { outcome: "verify_failed".into(), ..Default::default() }),
            ..Default::default()
        };
        assert!(outcome_label(&status).contains("failed"));
    }

    #[test]
    fn opt_in_profile_warns_about_all_nodes_and_the_hardware_ab() {
        let mut status = ModemStatus {
            available: true,
            profile_name: Some("rfd-async-64k-one-retry-layer".into()),
            opt_in_profile: true,
            ..Default::default()
        };
        let notes = profile_notes(&status);
        assert_eq!(notes.len(), 2);
        assert!(notes[0].contains("all three modems") && notes[0].contains("A/B"));
        status.rolled_out_all_nodes = true;
        assert_eq!(profile_notes(&status).len(), 1);
        status.opt_in_profile = false;
        assert!(profile_notes(&status).is_empty());
    }

    #[test]
    fn rf_board_result_is_shown_beside_the_station_result() {
        let status: ModemStatus = serde_json::from_str(
            r#"{"available":true,"rf_board":{"outcome":"applied","map_source":"ati5_names","mode":"fix",
            "eeprom_saved":true,"pending":false,"needs_attention":false,"max_data":3968,"max_retries":10,
            "global_retries":10,"serial_break_ms":50,"dest_id":32768,"rtscts":0,"air_speed":64,
            "summary":"RF modem settings written, saved and verified after reboot","received_unix_ms":5}}"#,
        )
        .unwrap();
        let rf = status.rf_board.as_ref().unwrap();
        assert_eq!(rf_label(Some(rf)), "RF modem settings corrected and verified");
        assert_eq!(rf_values(rf), "S3 3968 · S4 10 · S5 10 · S6 50 · S11 32768 · RTSCTS 0");
        assert_eq!(rf_label(None), "RF modem not reported");
        let failed = RfModem { outcome: "verify_failed".into(), ..Default::default() };
        assert!(rf_label(Some(&failed)).contains("failed"));
        let pending = RfModem { pending: true, ..Default::default() };
        assert!(rf_label(Some(&pending)).contains("pending"));
    }

    #[test]
    fn reset_outcome_and_receive_quality_are_shown() {
        let status: ModemStatus = serde_json::from_str(
            r#"{"available":true,"report":{"outcome":"reset","summary":"modem rebooted"},
            "receive_quality":{"good_frames_last_minute":97,"crc_failures_last_minute":3,
            "good_frames_total":1000,"crc_failures_total":30}}"#,
        )
        .unwrap();
        assert_eq!(outcome_label(&status), "Radio modem reset and the serial port reopened");
        assert_eq!(
            receive_quality_line(status.receive_quality.as_ref().unwrap()).unwrap(),
            "Receive quality (last minute): 97 good frames, 3 CRC failures (3%)"
        );
        assert!(receive_quality_line(&ReceiveQuality::default()).is_none());
    }

    #[test]
    fn empty_object_from_an_old_server_hides_the_section() {
        let status: ModemStatus = serde_json::from_str("{}").unwrap();
        assert!(!status.available);
    }
}
