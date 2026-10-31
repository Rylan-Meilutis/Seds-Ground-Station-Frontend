use super::{
    layout::ThemeConfig,
    types::{BoardDiagnostics, NetworkBoardHealthEntry},
};
use dioxus::prelude::*;
const TIMEOUT_MS: u64 = 12_000;
fn healthy(row: &NetworkBoardHealthEntry, connected: bool, elapsed: u64) -> bool {
    connected && row.healthy && row.age_ms.saturating_add(elapsed) <= TIMEOUT_MS
}
fn temperature(row: &NetworkBoardHealthEntry, connected: bool, elapsed: u64) -> String {
    if !connected {
        return "Unavailable".to_owned();
    }
    if let Some(value) = row.cpu_temperature_centi_c {
        if row.cpu_temperature_age_ms.saturating_add(elapsed) <= 15_000
            && (-4000..=15000).contains(&value)
        {
            return format!("CPU {:.2} °C", f64::from(value) / 100.0);
        }
    }
    if let Some(report) = &row.diagnostics {
        let age = report
            .received_age_ms
            .saturating_add(elapsed)
            .saturating_add(u64::from(report.die_temperature_sample_age_ms));
        if let Some(value) = report.die_temperature_centi_c {
            if age <= 15_000 && (-4000..=15000).contains(&value) {
                return format!("MCU die {:.2} °C", f64::from(value) / 100.0);
            }
        }
    }
    "Unavailable".to_owned()
}
fn duration(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
    }
}
// Status is based on successful device I/O, never just enqueue acceptance.
fn sd_state(
    report: &BoardDiagnostics,
    connected: bool,
    elapsed: u64,
) -> (&'static str, &'static str) {
    let Some(age) = report.sd_status_received_age_ms else {
        return ("Status unavailable", "unknown");
    };
    if !connected || age.saturating_add(elapsed) > 15_000 {
        return ("Status stale", "unknown");
    }
    let Some(flags) = report.sd_flags else {
        return ("Status unavailable", "unknown");
    };
    if flags & 8 != 0 {
        return ("Retry pending", "pending");
    }
    if flags & 4 != 0 {
        return ("Recording complete", "idle");
    }
    if flags & 1 == 0 {
        return ("Starting", "pending");
    }
    if flags & 2 == 0 {
        return if report.sd_init_failures.unwrap_or(0) > 0 {
            ("Card unavailable", "error")
        } else {
            ("Initializing", "pending")
        };
    }
    let write_age = report
        .sd_last_write_age_ms
        .map(|value| u64::from(value).saturating_add(age).saturating_add(elapsed));
    if report.sd_processed_records.unwrap_or(0) > 0
        && write_age.is_some_and(|value| value <= 15_000)
    {
        return ("Writing", "good");
    }
    let backlog = report
        .sd_accepted_records
        .zip(report.sd_processed_records)
        .is_some_and(|(accepted, processed)| accepted.wrapping_sub(processed) > 0);
    if backlog {
        ("Write stalled", "error")
    } else {
        ("Idle", "idle")
    }
}
fn sd_io_age(value: Option<u32>, report_age: u64, elapsed: u64) -> String {
    value
        .map(|age| {
            format!(
                "{} ago",
                duration(
                    u64::from(age)
                        .saturating_add(report_age)
                        .saturating_add(elapsed)
                )
            )
        })
        .unwrap_or_else(|| "Not yet reported".into())
}
#[component]
fn SdHealth(
    report: BoardDiagnostics,
    connected: bool,
    elapsed: u64,
    theme: ThemeConfig,
    retry_allowed: bool,
    retry_busy: bool,
    retry_message: String,
    retry_token: Option<u32>,
    on_retry: EventHandler<()>,
) -> Element {
    let (label, state) = sd_state(&report, connected, elapsed);
    let color = match state {
        "good" => "#146c43",
        "error" => "#9f1239",
        "pending" => "#815b12",
        _ => "#475569",
    };
    let age = report.sd_status_received_age_ms.unwrap_or(0);
    let fresh = connected
        && report
            .sd_status_received_age_ms
            .is_some_and(|age| age.saturating_add(elapsed) <= 15_000);
    let pending = report.sd_flags.is_some_and(|flags| flags & 8 != 0);
    let message = if fresh
        && retry_token.is_some_and(|token| report.sd_retry_completed_token == Some(token))
    {
        if report.sd_flags.is_some_and(|flags| flags & 2 != 0) {
            "Retry completed; card ready".to_owned()
        } else {
            "Retry completed; card unavailable".to_owned()
        }
    } else {
        retry_message
    };
    rsx! {
        section { "data-sd-state": state, style: "margin-top:12px; padding-top:12px; border-top:1px solid {theme.border_soft}; min-width:0;",
            div { style:"display:flex; flex-wrap:wrap; align-items:center; justify-content:space-between; gap:8px;",
                strong { "SD recording" }
                span { role:"status", style:"border-radius:999px; padding:3px 9px; background:{color}; color:white; font-size:0.8rem;", "{label}" }
            }
            if fresh {
                dl { style:"display:grid; grid-template-columns:minmax(0,1fr) auto; gap:6px; font-size:0.85rem; margin:12px 0;",
                    dt { "Last write" } dd { style:"margin:0;", "{sd_io_age(report.sd_last_write_age_ms, age, elapsed)}" }
                    dt { "Last flush" } dd { style:"margin:0;", "{sd_io_age(report.sd_last_flush_age_ms, age, elapsed)}" }
                    if let Some(flushed) = report.sd_flushed_records {
                        dt { "Samples flushed" } dd { style:"margin:0;", "{flushed}" }
                    }
                    dt { "Write errors" } dd { style:"margin:0;", "{report.sd_write_errors.unwrap_or(0)}" }
                }
            }
            if retry_allowed && report.sd_flags.is_some() {
                button { disabled:retry_busy || !fresh || pending || report.sd_flags.is_none_or(|flags| flags & 1 == 0),
                    style:"min-height:44px; margin-top:8px; padding:10px 16px; border:1px solid {theme.button_border}; border-radius:10px; background:{theme.button_background}; color:{theme.button_text}; font:inherit; cursor:pointer;", onclick:move |_| on_retry.call(()),
                    if retry_busy || pending { "Retry pending…" } else { "Retry SD" }
                }
            }
            if !message.is_empty() { p { role:"status", style:"font-size:0.85rem; margin:8px 0 0; overflow-wrap:anywhere;", "{message}" } }
            details { style:"font-size:0.85rem; margin-top:8px;",
                summary { style:"min-height:44px; display:flex; align-items:center; cursor:pointer;", "Recording details" }
                p { "Writes show successful card transfers. Flushed records show successful filesystem flushes; a write alone does not confirm a flush." }
                if report.sd_flags.is_none() { p { "Update DAQ firmware to receive recording status and enable retry." } }
                if let Some(count) = report.sd_retry_count { p { "Initialization attempts: {count} · Failures: {report.sd_init_failures.unwrap_or(0)}" } }
                if let Some(stage) = report.sd_init_stage { p { "Initialization stage: {stage}" } }
                p { "Retry preserves the recording session and launch cutoff. It does not format the card." }
            }
        }
    }
}
#[component]
pub fn NetworkHealthSection(
    rows: Vec<NetworkBoardHealthEntry>,
    ws_connected: bool,
    generated_ms: u64,
    simulated: bool,
    theme: ThemeConfig,
    #[props(default = false)] sd_retry_allowed: bool,
    #[props(default = false)] sd_retry_busy: bool,
    #[props(default)] sd_retry_message: String,
    #[props(default)] sd_retry_token: Option<u32>,
    #[props(default)] on_sd_retry: EventHandler<()>,
) -> Element {
    let now = super::connection_status_tab::use_board_age_clock();
    // Anchor each server snapshot to local monotonic time; UTC adjustments do not age boards.
    let receipt = use_memo(use_reactive((&generated_ms,), move |(_generated,)| {
        super::monotonic_now_ms()
    }));
    let elapsed = (now as f64 - receipt()).max(0.0) as u64;
    rsx! {
        section { style: "padding:16px; border:1px solid {theme.border_soft}; border-radius:12px; min-width:0; color:{theme.text_primary}; background:{theme.panel_background};",
            h3 { style: "margin:0 0 6px;", "Network health" }
            p { style: "margin:0 0 12px; color:{theme.text_secondary}; font-size:0.85rem;",
                if simulated { "Simulated network · " }
                "Observed boards · history since GroundStation started"
            }
            if !ws_connected { p { role: "status", "GroundStation stream unavailable" } }
            div { style: "display:grid; gap:12px; grid-template-columns:repeat(auto-fit,minmax(min(100%,240px),1fr));",
                for row in &rows {
                    {let ok = healthy(row, ws_connected, elapsed);
                     let color = if ok { "#146c43" } else { "#9f1239" };
                     let age = duration(row.age_ms.saturating_add(elapsed));
                     let observed = row.observed_ms.max(1);
                     let availability = 100.0 * observed.saturating_sub(row.offline_ms) as f64 / observed as f64;
                     rsx! {
                        article { key: "{row.sender_id}", style: "padding:12px; border:1px solid {theme.border_soft}; border-radius:10px; min-width:0; overflow-wrap:anywhere;",
                            div { style: "display:flex; flex-wrap:wrap; gap:8px; justify-content:space-between; align-items:center;",
                                strong { "{row.sender_id}" }
                                span { style: "font-size:0.8rem; border:1px solid {color}; background:{color}; color:#ffffff; border-radius:999px; padding:3px 9px;",
                                    if ok { "Healthy" } else { "Unhealthy" }
                                }
                            }
                            dl { style: "display:grid; grid-template-columns:1fr auto; gap:8px; font-size:0.9rem; margin:12px 0 0;",
                                dt { "Temperature" } dd { style: "margin:0;", "{temperature(row, ws_connected, elapsed)}" }
                                dt { "Last received" } dd { style: "margin:0;", "{age} ago" }
                                dt { "Disconnects" } dd { style: "margin:0;", "{row.disconnects}" }
                                dt { "Reconnects" } dd { style: "margin:0;", "{row.reconnects}" }
                                dt { "Availability" } dd { style: "margin:0;", "{availability:.1}%" }
                                dt { "Longest outage" } dd { style: "margin:0;", "{duration(row.longest_outage_ms)}" }
                                if row.current_outage_ms > 0 {
                                    dt { "Current outage" } dd { style: "margin:0;", "{duration(row.current_outage_ms.saturating_add(elapsed))}" }
                                }
                            }
                            if row.sender_id == "DAQ" {
                                if let Some(report) = &row.diagnostics {
                                    SdHealth { report:report.clone(), connected:ws_connected, elapsed,
                                        theme:theme.clone(), retry_allowed:sd_retry_allowed,
                                        retry_busy:sd_retry_busy, retry_message:sd_retry_message.clone(),
                                        retry_token:sd_retry_token, on_retry:on_sd_retry }
                                }
                            }
                            details { style: "margin-top:12px; font-size:0.85rem;",
                                summary { style: "min-height:44px; display:flex; align-items:center; cursor:pointer;", "Health details" }
                                p { "Observed for {duration(row.observed_ms)} · Offline {duration(row.offline_ms)} · Receive observations {row.rx_observations}" }
                                if let Some(report) = &row.diagnostics {
                                    {let report_age = report.received_age_ms.saturating_add(elapsed);
                                     let uptime_ms = report.uptime_ticks.saturating_mul(1000) / u64::from(report.uptime_frequency_hz.max(1));
                                     rsx! {
                                        p { "Reported uptime {duration(uptime_ms)} · {report.uptime_frequency_hz} ticks/s" }
                                        if report_age > 15_000 { p { role: "status", "Diagnostic report stale · {duration(report_age)} ago" } }
                                        p { "Reset flags: {report.reset_flags:#010x} · Watchdog pending: {report.watchdog_missing_mask:#x}" }
                                        if let (Some(live),Some(peak),Some(region))=(report.heap_live_bytes,report.heap_peak_bytes,report.heap_region_bytes) {
                                            p { "Allocator live / peak / region: {live} / {peak} / {region} bytes" }
                                        }
                                        if let Some(failures)=report.allocation_failures { p { "Allocation failures: {failures}" } }
                                        p { "Memory admission refusals: {report.admission_refusals}" }
                                        if let Some(drops)=report.can_rx_drops { p { "CAN receive drops: {drops}" } }
                                        if let Some(drops)=report.can_tx_rejected { p { "CAN transmit refusals: {drops}" } }
                                        if let Some(drops)=report.uart_rx_drops { p { "UART receive drops: {drops}" } }
                                        if let Some(peak)=report.tx_queue_high_water { p { "Transmit queue peak: {peak}" } }
                                        if let (Some(accepted),Some(processed),Some(rejected),Some(flushed))=(report.sd_accepted_records,report.sd_processed_records,report.sd_rejected_records,report.sd_flushed_records) {
                                            p { "SD accepted / writer-processed / flushed: {accepted} / {processed} / {flushed}" }
                                            p { "SD rejected records: {rejected} · Write errors: {report.sd_write_errors.unwrap_or(0)}" }
                                        }
                                        if let Some(overruns)=report.adc_overruns { p { "ADC queue overruns: {overruns}" } }
                                        p { "Diagnostic send refusals: {report.diagnostic_send_failures}" }
                                     }}
                                } else { p { "Internal diagnostics unavailable from this firmware." } }
                            }
                        }
                     }}
                }
            }
            if rows.is_empty() { p { "No boards observed in this mode." } }
            details { style: "margin-top:12px; font-size:0.85rem; color:{theme.text_secondary};",
                summary { style: "min-height:44px; display:flex; align-items:center; cursor:pointer;", "About health" }
                p { "Healthy means traffic was received within 12 seconds. Silence counts once as a disconnect; fresh traffic counts once as a reconnect. This measures network liveness, not internal sensor or CPU health. Receive observations may include multiple handlers for one packet. History resets when the backend restarts or simulation is reset." }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sd_status_requires_fresh_successful_io_and_preserves_idle_completion() {
        let mut report = BoardDiagnostics {
            sd_status_received_age_ms: Some(1000),
            sd_flags: Some(3),
            sd_accepted_records: Some(100),
            sd_processed_records: Some(90),
            sd_last_write_age_ms: Some(200),
            ..Default::default()
        };
        assert_eq!(sd_state(&report, true, 0).0, "Writing");
        assert_eq!(sd_state(&report, false, 0).0, "Status stale");
        assert_eq!(sd_state(&report, true, 14_001).0, "Status stale");
        report.sd_status_received_age_ms = Some(0);
        report.sd_last_write_age_ms = Some(16_000);
        assert_eq!(sd_state(&report, true, 0).0, "Write stalled");
        report.sd_processed_records = Some(100);
        assert_eq!(sd_state(&report, true, 0).0, "Idle");
        report.sd_flags = Some(7);
        assert_eq!(sd_state(&report, true, 0).0, "Recording complete");
        report.sd_flags = Some(9);
        assert_eq!(sd_state(&report, true, 0).0, "Retry pending");
        report.sd_flags = Some(1);
        report.sd_init_failures = Some(1);
        assert_eq!(sd_state(&report, true, 0).0, "Card unavailable");
        report.sd_status_received_age_ms = None;
        assert_eq!(sd_state(&report, true, 0).0, "Status unavailable");
    }
    #[test]
    fn queued_records_and_old_board_health_cannot_claim_sd_is_writing() {
        let mut report = BoardDiagnostics {
            received_age_ms: 0,
            sd_accepted_records: Some(100),
            sd_processed_records: Some(0),
            sd_flushed_records: Some(0),
            ..Default::default()
        };
        assert_eq!(sd_state(&report, true, 0).0, "Status unavailable");
        report.sd_status_received_age_ms = Some(0);
        report.sd_flags = Some(3);
        report.sd_last_write_age_ms = Some(0);
        assert_ne!(sd_state(&report, true, 0).0, "Writing");
        report.sd_status_received_age_ms = Some(16_000);
        assert_eq!(sd_state(&report, true, 0).0, "Status stale");
        assert_eq!(sd_io_age(None, 0, 0), "Not yet reported");
        assert_eq!(sd_io_age(Some(1000), 1000, 1000), "3s ago");
    }
    #[test]
    fn temperature_is_signed_unit_labelled_and_expires_independently() {
        let mut row = NetworkBoardHealthEntry {
            diagnostics: Some(super::super::types::BoardDiagnostics {
                die_temperature_centi_c: Some(-1234),
                die_temperature_sample_age_ms: 200,
                received_age_ms: 5000,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(temperature(&row, true, 0), "MCU die -12.34 °C");
        assert_eq!(temperature(&row, true, 9801), "Unavailable");
        assert_eq!(temperature(&row, false, 0), "Unavailable");
        row.cpu_temperature_centi_c = Some(5550);
        row.cpu_temperature_age_ms = 1;
        assert_eq!(temperature(&row, true, 0), "CPU 55.50 °C");
        row.cpu_temperature_centi_c = Some(20000);
        assert_eq!(temperature(&row, true, 0), "MCU die -12.34 °C");
    }
    #[test]
    fn disconnected_stream_and_aged_snapshot_cannot_show_healthy() {
        let row = NetworkBoardHealthEntry {
            healthy: true,
            age_ms: 11_000,
            ..Default::default()
        };
        assert!(healthy(&row, true, 999));
        assert!(!healthy(&row, true, 1001));
        assert!(!healthy(&row, false, 0));
    }
    #[test]
    fn old_servers_default_to_an_empty_health_list() {
        let msg: super::super::types::NetworkTopologyMsg =
            serde_json::from_str(r#"{"generated_ms":1}"#).unwrap();
        assert!(msg.health.is_empty());
    }
}
