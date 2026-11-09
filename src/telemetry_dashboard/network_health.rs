#[path = "network_health/board_display.rs"]
pub(super) mod board_display;

/// Recovery content is supplied by the production Connectivity owner. Previews
/// render health/diagnostics without gaining backend mutation capabilities.
#[derive(Clone, Copy, PartialEq)]
pub enum RecoverySlot { Tools, State, Actions, Details }
pub type RecoveryRenderer = Callback<(RecoverySlot, String, bool, ThemeConfig), Element>;

use super::{
    layout::ThemeConfig,
    types::{BoardDiagnostics, LaunchLogStatus, NetworkBoardHealthEntry},
};
use dioxus::prelude::*;
fn healthy(row: &NetworkBoardHealthEntry, connected: bool, _elapsed: u64) -> bool {
    board_display::network_healthy(connected,row.healthy)
}
fn temperature(row: &NetworkBoardHealthEntry, connected: bool, elapsed: u64) -> String {
    if !connected {
        return "Unavailable".to_owned();
    }
    if let Some(value) = row.cpu_temperature_centi_c
        && row.cpu_temperature_age_ms.saturating_add(elapsed) <= 15_000
        && (-4000..=15000).contains(&value)
    {
        return format!("CPU {:.2} °C", f64::from(value) / 100.0);
    }
    if let Some(report) = &row.diagnostics {
        let age = report
            .received_age_ms
            .saturating_add(elapsed)
            .saturating_add(u64::from(report.die_temperature_sample_age_ms));
        if let Some(value) = report.die_temperature_centi_c
            && age <= 15_000 && (-4000..=15000).contains(&value)
        {
            return format!("MCU die {:.2} °C", f64::from(value) / 100.0);
        }
    }
    "Unavailable".to_owned()
}
/// The board's own BOARD_HEALTH report. Version 0 carries only the DAQ SD and
/// launch-log status, published before or without a board health report.
fn board_health(row: &NetworkBoardHealthEntry) -> Option<&BoardDiagnostics> {
    row.diagnostics.as_ref().filter(|report| report.version != 0)
}
/// Shown in place of the SD status when the DAQ has not reported it yet.
fn daq_recording_pending(row: &NetworkBoardHealthEntry) -> Option<&'static str> {
    (row.sender_id == "DAQ" && row.diagnostics.is_none())
        .then_some("No SD recording status received from the DAQ yet. It reports every 5 s.")
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
fn launch_log_state(
    status: Option<&LaunchLogStatus>,
    connected: bool,
    elapsed: u64,
) -> (&'static str, &'static str) {
    let Some(status) = status else {
        return ("No confirmation yet", "unknown");
    };
    if !connected || status.received_age_ms.saturating_add(elapsed) > 15_000 {
        return ("Confirmation stale", "unknown");
    }
    match status.matches_expected {
        None => return ("Unverified launch clock", "unknown"),
        Some(false) => return ("Awaiting DAQ acceptance", "pending"),
        Some(true) => {}
    }
    if status.session == 0 {
        return ("Normal log mode confirmed", "good");
    }
    if status.flags & 8 != 0 {
        return if status.flags & (16 | 32 | 64) != 0 {
            ("Window ended; files draining", "pending")
        } else {
            ("Launch window closed", "idle")
        };
    }
    if status.flags & 2 == 0 {
        return ("Clock accepted; SD unavailable", "error");
    }
    if status.flags & 4 == 0 {
        return ("Clock accepted; awaiting time", "pending");
    }
    if status.flags & (16 | 32) != 0 {
        return ("Launch files open", "good");
    }
    ("Launch clock accepted", "good")
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
    let (launch_label, launch_state) =
        launch_log_state(report.launch_log.as_ref(), connected, elapsed);
    let launch_color = match launch_state {
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
            div { "data-launch-log-state":launch_state, style:"display:flex; flex-wrap:wrap; align-items:center; justify-content:space-between; gap:8px; margin-top:14px;",
                strong { "DAQ launch log" }
                span { role:"status", style:"border-radius:999px; padding:3px 9px; background:{launch_color}; color:white; font-size:0.8rem;", "{launch_label}" }
            }
            details { style:"font-size:0.85rem; margin-top:8px;",
                summary { style:"min-height:44px; display:flex; align-items:center; cursor:pointer;", "Recording details" }
                p { "Writes show successful card transfers. Flushed records show successful filesystem flushes; a write alone does not confirm a flush." }
                if report.sd_flags.is_none() { p { "Update DAQ firmware to receive recording status and enable retry." } }
                if let Some(count) = report.sd_retry_count { p { "Initialization attempts: {count} · Failures: {report.sd_init_failures.unwrap_or(0)}" } }
                if let Some(stage) = report.sd_init_stage { p { "Initialization stage: {stage}" } }
                p { "Retry preserves the recording session and launch cutoff. It does not format the card." }
                if let Some(status) = &report.launch_log {
                    p { "DAQ session: {status.session} · Revision: {status.revision} · Cutoff UTC ms: {status.deadline_utc_ms}" }
                    if status.matches_expected == Some(true) { p { "The reported session, cutoff and revision match GroundStation's current launch clock." } }
                } else { p { "Matching DAQ firmware is required for application-level launch confirmation." } }
                p { "Acceptance confirms DAQ applied the launch clock; it does not prove every sample was recorded. File-open flags and the recording cutoff are separate from write and flush progress." }

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
    #[props(default)] recovery_renderer: Option<RecoveryRenderer>,
    #[props(default)] on_recovery_focus: Option<EventHandler<(String, bool)>>,
) -> Element {
    let now = super::connection_status_tab::use_board_age_clock();
    // Anchor each server snapshot to local monotonic time; UTC adjustments do not age boards.
    let receipt = use_memo(use_reactive((&generated_ms,), move |(_generated,)| {
        super::monotonic_now_ms()
    }));
    let elapsed = (now as f64 - receipt()).max(0.0) as u64;
    let mut more_info=use_signal(||None::<String>);
    rsx! {
        section { class:"gs26-board-recovery gs26-network-health",style: "padding:16px; border:1px solid {theme.border_soft}; border-radius:12px; min-width:0; color:{theme.text_primary}; background:{theme.panel_background};",
            div{style:"display:flex;justify-content:space-between;align-items:center;gap:10px;flex-wrap:wrap;",
                h3 { style: "margin:0;", "Network health" }
                if let Some(render) = recovery_renderer { {render.call((RecoverySlot::Tools, String::new(), ws_connected, theme.clone()))} }
            }
            if simulated {p{style:"margin:8px 0;color:{theme.text_secondary};","Simulated network"}}
            if !ws_connected { p { role: "status", "GroundStation stream unavailable" } }
            div { style: "display:grid; gap:12px; margin-top:12px;grid-template-columns:repeat(auto-fit,minmax(min(100%,240px),1fr));",
                for row in &rows {
                    {let ok = healthy(row, ws_connected, elapsed);
                     let age = duration(row.age_ms.saturating_add(elapsed));
                     let observed = row.observed_ms.max(1);
                     let availability = 100.0 * observed.saturating_sub(row.offline_ms) as f64 / observed as f64;
                     let board_id=row.sender_id.clone();
                     rsx! {
                        article { key: "{row.sender_id}","data-board":"{row.sender_id}",class:"gs26-board-recovery-card", style: "padding:12px; border:1px solid {theme.border_soft}; border-radius:10px; min-width:0; overflow-wrap:anywhere;",
                            div { style: "display:flex; flex-wrap:wrap; gap:8px; justify-content:space-between; align-items:center;",
                                strong { "{row.sender_id}" }
                                {board_display::health_badge(ok)}
                            }
                            if let Some(render) = recovery_renderer { p{style:"margin:8px 0;",{render.call((RecoverySlot::State, board_id.clone(), ws_connected, theme.clone()))}} }
                            p{style:"margin:8px 0;color:{theme.text_secondary};","Disconnects: {row.disconnects}"}
                            if let Some(offset) = row.clock_offset_ms {
                                if clock_desynced(offset) {
                                    p{role:"alert",style:"margin:8px 0;padding:6px 8px;border:1px solid {theme.error_border};background:{theme.error_background};color:{theme.error_text};border-radius:8px;","Clock out of sync: {clock_offset_text(offset)}"}
                                } else {
                                    p{style:"margin:8px 0;color:{theme.text_secondary};","Clock: in sync ({clock_offset_text(offset)})"}
                                }
                            }
                            if let Some(render) = recovery_renderer { {render.call((RecoverySlot::Actions, board_id.clone(), ws_connected, theme.clone()))} }
                            button{r#type:"button",style:"min-height:44px;margin-top:8px;",onclick:{let id=board_id.clone();move |_|more_info.set(Some(id.clone()))},"More info"}
                            if more_info.read().as_deref()==Some(board_id.as_str()){
                                div{style:"position:fixed;inset:0;z-index:199999;background:#000a;display:flex;align-items:center;justify-content:center;padding:max(16px,env(safe-area-inset-top)) max(16px,env(safe-area-inset-right)) max(16px,env(safe-area-inset-bottom)) max(16px,env(safe-area-inset-left));box-sizing:border-box;",
                                    onkeydown:move|event|{event.stop_propagation();match event.key(){Key::Escape=>{event.prevent_default();more_info.set(None);},Key::Tab=>{if let Some(focus) = on_recovery_focus {event.prevent_default();focus.call(("gs26-board-more-info".into(),event.modifiers().contains(Modifiers::SHIFT)));}},_=>{}}},
                                    div{id:"gs26-board-more-info",role:"dialog","aria-modal":"true","aria-label":"Board details",style:"background:{theme.panel_background};color:{theme.text_primary};font:inherit;padding:20px;border-radius:14px;width:min(100%,1060px);height:min(88dvh,900px);max-height:100%;overflow:hidden;display:flex;flex-direction:column;gap:14px;box-sizing:border-box;overflow-wrap:anywhere;",
                                        div{style:"display:flex;flex:0 0 auto;justify-content:space-between;align-items:center;gap:12px;",h3{style:"margin:0;","{board_id} · More info"}button{r#type:"button",onmounted:move|event|async move{let _=event.data().set_focus(true).await;},onclick:move |_|more_info.set(None),"Close details"}}
                                        div{class:"gs26-modal-scroll-body gs26-board-info-columns",style:"columns:3 260px;column-gap:14px;flex:1;min-height:0;overflow-y:auto;overflow-anchor:none;overscroll-behavior:contain;padding:2px;",
                                            // Masonry-style columns: each section stacks directly under the
                                            // one above it instead of waiting for the tallest card in a row.
                                            style{".gs26-board-info-columns>section{{break-inside:avoid;display:block;width:100%;box-sizing:border-box;margin:0 0 14px;}}"}
                                            section{style:"padding:14px;border:1px solid {theme.border_soft};border-radius:10px;min-width:0;",h4{style:"margin:0;","Network history"}
                            dl { style: "display:grid; grid-template-columns:1fr auto; gap:8px; font-size:0.9rem; margin:12px 0 0;",
                                dt { "Temperature" } dd { style: "margin:0;", "{temperature(row, ws_connected, elapsed)}" }
                                dt { "Last received" } dd { style: "margin:0;", "{age} ago" }
                                dt { "Disconnects" } dd { style: "margin:0;", "{row.disconnects}" }
                                if let Some(offset) = row.clock_offset_ms {
                                    dt { "Clock offset" } dd { style: "margin:0;", "{clock_offset_text(offset)}" }
                                }
                                dt { "Reconnects" } dd { style: "margin:0;", "{row.reconnects}" }
                                dt { "Availability" } dd { style: "margin:0;", "{availability:.1}%" }
                                dt { "Longest outage" } dd { style: "margin:0;", "{duration(row.longest_outage_ms)}" }
                                if row.current_outage_ms > 0 {
                                    dt { "Current outage" } dd { style: "margin:0;", "{duration(row.current_outage_ms.saturating_add(elapsed))}" }
                                }
                            }
                                            }
                                            // Storage sits directly below network history.
                                            section{style:"padding:14px;border:1px solid {theme.border_soft};border-radius:10px;min-width:0;",h4{style:"margin:0;","Storage and recording"}
                            if row.sender_id == "DAQ" {
                                if let Some(report) = &row.diagnostics {
                                    SdHealth { report:report.clone(), connected:ws_connected, elapsed,
                                        theme:theme.clone(), retry_allowed:sd_retry_allowed,
                                        retry_busy:sd_retry_busy, retry_message:sd_retry_message.clone(),
                                        retry_token:sd_retry_token, on_retry:on_sd_retry }
                                }
                                if let Some(pending) = daq_recording_pending(row) {
                                    p { role:"status", "data-sd-state":"unknown", "{pending}" }
                                }
                            }
                                                if row.sender_id!="DAQ"{p{"No storage actions are advertised for this node."}}
                                            }
                                            if let Some(render) = recovery_renderer { {render.call((RecoverySlot::Details, board_id.clone(), ws_connected, theme.clone()))} }
                                            section{style:"padding:14px;border:1px solid {theme.border_soft};border-radius:10px;min-width:0;",h4{style:"margin:0;","Internal diagnostics"}
                                p { "Observed for {duration(row.observed_ms)} · Offline {duration(row.offline_ms)} · Receive observations {row.rx_observations}" }
                                if let Some(report) = board_health(row) {
                                    {let report_age = report.received_age_ms.saturating_add(elapsed);
                                     let uptime_ms = report.uptime_ticks.saturating_mul(1000) / u64::from(report.uptime_frequency_hz.max(1));
                                     rsx! {
                                        p { "Reported uptime {duration(uptime_ms)} · {report.uptime_frequency_hz} ticks/s" }
                                        if report_age > 15_000 { p { role: "status", "Diagnostic report stale · {duration(report_age)} ago" } }
                                        p { "Reset flags: {report.reset_flags:#010x} · Watchdog pending: {report.watchdog_missing_mask:#x}" }
                                        if let Some(faults) = service_fault_summary(report) {
                                            p { role: "alert", style: "color:{theme.warning_text};", "{faults}" }
                                        }
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
                                } else if row.diagnostics.is_some() { p { "No board health report received yet." } } else { p { "Internal diagnostics unavailable from this firmware." } }                                            }
                                        }
                                    }
                                }
                            }
                        }
                     }}
                }
            }
            if rows.is_empty() { p { "No boards observed in this mode." } }
        }
    }
}

/// Operator line for board services that failed to start. The board keeps
/// routing and retries them every 5 s; None when every service started.
fn service_fault_summary(report: &BoardDiagnostics) -> Option<String> {
    let faults: Vec<&str> = report
        .service_faults
        .iter()
        .map(|fault| fault.trim())
        .filter(|fault| !fault.is_empty())
        .collect();
    (!faults.is_empty()).then(|| {
        format!("Service faults (still routing, retrying every 5 s): {}", faults.join(", "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn daq_recording_state_is_never_silently_missing() {
        let mut row = NetworkBoardHealthEntry { sender_id: "DAQ".into(), healthy: true, ..Default::default() };
        assert!(daq_recording_pending(&row).is_some(), "an absent report is said out loud");
        row.diagnostics = Some(BoardDiagnostics { sd_flags: Some(7), sd_status_received_age_ms: Some(0), ..Default::default() });
        assert!(daq_recording_pending(&row).is_none());
        assert!(board_health(&row).is_none(), "an SD-only report is not board health");
        row.diagnostics.as_mut().unwrap().version = 1;
        assert!(board_health(&row).is_some());
        row.sender_id = "AB".into();
        row.diagnostics = None;
        assert!(daq_recording_pending(&row).is_none());
    }
    #[test]
    fn launch_confirmation_requires_current_exact_application_report() {
        assert_eq!(launch_log_state(None, true, 0).0, "No confirmation yet");
        let mut status = LaunchLogStatus {
            session: 42,
            revision: 123,
            flags: 7,
            matches_expected: Some(true),
            expected_session: Some(42),
            ..Default::default()
        };
        assert_eq!(
            launch_log_state(Some(&status), true, 0).0,
            "Launch clock accepted"
        );
        status.matches_expected = Some(false);
        assert_eq!(
            launch_log_state(Some(&status), true, 0).0,
            "Awaiting DAQ acceptance"
        );
        status.matches_expected = Some(true);
        status.flags |= 16 | 32;
        assert_eq!(
            launch_log_state(Some(&status), true, 0).0,
            "Launch files open"
        );
        status.received_age_ms = 15_001;
        assert_eq!(
            launch_log_state(Some(&status), true, 0).0,
            "Confirmation stale"
        );
        status.received_age_ms = 0;
        assert_eq!(
            launch_log_state(Some(&status), false, 0).0,
            "Confirmation stale"
        );
        status.flags |= 8;
        assert_eq!(
            launch_log_state(Some(&status), true, 0).0,
            "Window ended; files draining"
        );
        status.flags = 1 | 2 | 4 | 8;
        assert_eq!(
            launch_log_state(Some(&status), true, 0).0,
            "Launch window closed"
        );
        status.session = 0;
        status.flags = 1;
        assert_eq!(
            launch_log_state(Some(&status), true, 0).0,
            "Normal log mode confirmed"
        );
        status.session = 42;
        assert_eq!(
            launch_log_state(Some(&status), true, 0).0,
            "Clock accepted; SD unavailable"
        );
        status.flags = 3;
        assert_eq!(
            launch_log_state(Some(&status), true, 0).0,
            "Clock accepted; awaiting time"
        );
        status.matches_expected = None;
        assert_eq!(
            launch_log_state(Some(&status), true, 0).0,
            "Unverified launch clock"
        );
    }

    #[test]
    fn service_faults_are_summarised_only_when_present() {
        let mut report = BoardDiagnostics::default();
        assert_eq!(service_fault_summary(&report), None);
        report.service_faults = vec!["OTA storage unavailable".into(), " ".into()];
        assert_eq!(
            service_fault_summary(&report).as_deref(),
            Some("Service faults (still routing, retrying every 5 s): OTA storage unavailable")
        );
        let old: BoardDiagnostics = serde_json::from_str(
            r#"{"version":1,"received_age_ms":0,"uptime_ticks":0,"uptime_frequency_hz":1000,"reset_flags":0,"watchdog_missing_mask":0,"heap_live_bytes":null,"heap_peak_bytes":null,"heap_region_bytes":null,"allocation_failures":null,"admission_refusals":0,"can_rx_drops":null,"can_tx_rejected":null,"uart_rx_drops":null,"tx_queue_high_water":null,"sd_accepted_records":null,"sd_processed_records":null,"sd_rejected_records":null,"sd_flushed_records":null,"sd_write_errors":null,"sd_max_write_ms":null,"adc_overruns":null,"diagnostic_send_failures":0}"#,
        )
        .unwrap();
        assert!(old.service_faults.is_empty(), "older backends omit the field");
    }

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
    fn network_health_uses_backend_authority_even_for_an_old_displayed_age() {
        let row = NetworkBoardHealthEntry {
            healthy: true,
            age_ms: 30_000,
            ..Default::default()
        };
        assert!(healthy(&row, true, 999));
        assert!(healthy(&row, true, 1001));
        assert!(!healthy(&row, false, 0));
    }
    #[test]
    fn old_servers_default_to_an_empty_health_list() {
        let msg: super::super::types::NetworkTopologyMsg =
            serde_json::from_str(r#"{"generated_ms":1}"#).unwrap();
        assert!(msg.health.is_empty());
    }
}

/// General health stays in Detailed; individual boards live in Connectivity.
#[component]
pub fn GeneralNetworkHealthSummary(
    rows: Vec<NetworkBoardHealthEntry>,
    ws_connected: bool,
    generated_ms: u64,
    simulated: bool,
    theme: ThemeConfig,
) -> Element {
    let now = super::connection_status_tab::use_board_age_clock();
    let receipt = use_memo(use_reactive((&generated_ms,), move |(_generated,)| {
        super::monotonic_now_ms()
    }));
    let elapsed = (now as f64 - receipt()).max(0.0) as u64;
    let available = rows
        .iter()
        .filter(|row| healthy(row, ws_connected, elapsed))
        .count();
    let observed = rows.len();
    rsx! {
        section { style: "padding:16px; border:1px solid {theme.border_soft}; border-radius:12px; color:{theme.text_primary}; background:{theme.panel_background};",
            h3 { "Network health" }
            p { role: "status",
                if !ws_connected { "GroundStation stream unavailable" }
                else { "{available} of {observed} observed boards are currently communicating" }
            }
            if simulated { p { "Simulated network" } }
            p { style: "color:{theme.text_secondary};", "Per-board diagnostics and recovery are in Connectivity." }
        }
    }
}

/// Beyond radio transit delay; recovery commands need the board within 15 s.
const CLOCK_DESYNC_MS: i64 = 2_000;
fn clock_desynced(offset_ms: i64) -> bool {
    offset_ms.unsigned_abs() > CLOCK_DESYNC_MS as u64
}
fn clock_offset_text(offset_ms: i64) -> String {
    let side = if offset_ms >= 0 { "ahead of" } else { "behind" };
    let ms = offset_ms.unsigned_abs();
    let amount = if ms < 10_000 {
        format!("{ms} ms")
    } else if ms < 3_600_000 {
        format!("{:.1} min", ms as f64 / 60_000.0)
    } else if ms < 86_400_000 * 2 {
        format!("{:.1} h", ms as f64 / 3_600_000.0)
    } else {
        format!("{:.0} days", ms as f64 / 86_400_000.0)
    };
    format!("{amount} {side} the station")
}
#[cfg(test)]
mod clock_tests {
    use super::*;
    #[test]
    fn small_offsets_are_in_sync_and_uptime_clocks_are_flagged() {
        assert!(!clock_desynced(-40));
        assert!(!clock_desynced(1_500));
        assert!(clock_desynced(-1_794_183_000_000));
        assert_eq!(clock_offset_text(-25), "25 ms behind the station");
        assert!(clock_offset_text(-1_794_183_000_000).ends_with("days behind the station"));
    }
}
