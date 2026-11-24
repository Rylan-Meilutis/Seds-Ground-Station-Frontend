#[path = "network_health/board_display.rs"]
pub(super) mod board_display;

/// Recovery content is supplied by the production Connectivity owner. Previews
/// render health/diagnostics without gaining backend mutation capabilities.
#[derive(Clone, Copy, PartialEq)]
pub enum RecoverySlot { Tools, State, Actions, Details }
pub type RecoveryRenderer = Callback<(RecoverySlot, String, bool, ThemeConfig), Element>;

use super::{
    layout::ThemeConfig,
    types::{BoardDiagnostics, DaqInputHealthStatus, LaunchLogStatus, NetworkBoardHealthEntry},
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
/// The card's SD line, only for a board whose diagnostics carry storage
/// health. A fault or dropped/failed writes are unhealthy; a status that is
/// stale, starting or pending is stale.
fn sd_health(row: &NetworkBoardHealthEntry, connected: bool, elapsed: u64) -> Option<(board_display::Health, String)> {
    use board_display::Health;
    let report = row.diagnostics.as_ref()?;
    if report.sd_flags.is_none() && report.sd_status_received_age_ms.is_none() {
        return None;
    }
    let (label, state) = sd_state(report, connected, elapsed);
    let dropped = report.sd_rejected_records.unwrap_or(0).saturating_add(report.sd_write_errors.unwrap_or(0));
    Some(match state {
        "error" => (Health::Unhealthy, label.to_string()),
        _ if dropped > 0 && state != "unknown" => (Health::Unhealthy, format!("{label} · {dropped} dropped")),
        "good" | "idle" => (Health::Healthy, label.to_string()),
        _ => (Health::Stale, label.to_string()),
    })
}
/// The DAQ input-health report is fresh: reports arrive every second.
fn daq_input_fresh(status: &DaqInputHealthStatus, connected: bool, elapsed: u64) -> bool {
    connected && status.received_age_ms.saturating_add(elapsed) <= 5_000
}
/// The card's Inputs line, for a DAQ reporting input health. Any active fault
/// is unhealthy; a fault seen only briefly in the last second is named but
/// does not turn the line red. These are warnings: no action is blocked.
fn daq_input_health(row: &NetworkBoardHealthEntry, connected: bool, elapsed: u64) -> Option<(board_display::Health, String)> {
    use board_display::Health;
    let status = row.diagnostics.as_ref()?.daq_input.as_ref()?;
    if !daq_input_fresh(status, connected, elapsed) {
        return Some((Health::Stale, "Input report stale".into()));
    }
    let active: Vec<&str> = status.faults.iter().filter(|f| f.active).map(|f| f.label.as_str()).collect();
    if let Some(first) = active.first() {
        let more = if active.len() > 1 { format!(" (+{} more)", active.len() - 1) } else { String::new() };
        return Some((Health::Unhealthy, format!("{first}{more}")));
    }
    let over: Vec<&str> = status.adc_channels.iter().filter(|c| c.over_range_now).map(|c| c.label.as_str()).collect();
    if let Some(first) = over.first() {
        let more = if over.len() > 1 { format!(" (+{} more)", over.len() - 1) } else { String::new() };
        return Some((Health::Unhealthy, format!("ADC over range: {first}{more}")));
    }
    match status.faults.iter().find(|f| f.latched) {
        Some(brief) => Some((Health::Healthy, format!("OK · brief: {}", brief.label))),
        None => Some((Health::Healthy, "OK".into())),
    }
}
fn millivolts(mv: Option<u16>) -> String {
    match mv {
        Some(mv) if mv < 100 => format!("{:.3} V", f32::from(mv) / 1000.0),
        Some(mv) => format!("{:.2} V", f32::from(mv) / 1000.0),
        None => "No reading".into(),
    }
}
/// How the +3V3A (VDDA) reading should look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VddaLevel { Ok, Warn, Danger }

/// VDDA outside 3.15–3.45 V warns; above 3.6 V (the STM32's absolute
/// maximum) is danger. The highest reading of the interval drives the high
/// checks so a brief spike is not hidden by a normal latest value.
fn vdda_level(vdda: Option<f32>, vdda_max: Option<f32>) -> VddaLevel {
    let high = vdda.into_iter().chain(vdda_max).fold(f32::NEG_INFINITY, f32::max);
    if high > 3.6 {
        VddaLevel::Danger
    } else if high > 3.45 || vdda.is_some_and(|v| v < 3.15) {
        VddaLevel::Warn
    } else {
        VddaLevel::Ok
    }
}

/// The "+3V3A (VDDA)" line, or None when the backend does not report it.
fn vdda_text(status: &DaqInputHealthStatus) -> Option<(String, VddaLevel)> {
    let vdda = status.vdda_v.map(|v| v.0).filter(|v| v.is_finite());
    let max = status.vdda_max_v.map(|v| v.0).filter(|v| v.is_finite());
    if vdda.is_none() && max.is_none() {
        return None;
    }
    let now = vdda.map_or_else(|| "No reading".to_string(), |v| format!("{v:.2} V"));
    let text = match max {
        Some(max) => format!("{now} (max {max:.2} V)"),
        None => now,
    };
    Some((text, vdda_level(vdda, max)))
}

/// The MCP3564R reference line: "2.200 V (station)", and why it warns.
/// Nominal is 2.4 V ±2 %; outside 2.35–2.45 V is a drifted part (or a wrong
/// setting). A DAQ on its default while the station has another value has
/// not received the station's setting.
fn mcp_vref_line(status: &DaqInputHealthStatus, station_v: Option<f32>) -> Option<(String, Option<String>)> {
    let value = status.mcp_vref_v.map(|v| v.0).filter(|v| v.is_finite())?;
    let source = status.mcp_vref_source.as_deref().filter(|s| !s.is_empty());
    let text = match source {
        Some(source) => format!("{value:.3} V ({source})"),
        None => format!("{value:.3} V"),
    };
    let warning = if source == Some("default") && station_v.is_some_and(|station| (station - value).abs() >= 0.0005) {
        Some(format!(
            "The DAQ is on its default reference, not the station's {:.3} V setting.",
            station_v.unwrap_or_default()
        ))
    } else if !(2.35..=2.45).contains(&value) {
        Some("Outside 2.35–2.45 V (nominal 2.4 V ±2 %): a drifted part or a wrong setting. Re-measure +VREF.".into())
    } else {
        None
    };
    Some((text, warning))
}

/// One over-range channel line of the DAQ panel.
#[derive(Clone, Debug, PartialEq)]
struct AdcChannelLine {
    key: String,
    label: String,
    /// "Over range now" / "Over range (low) now" / "Recently over range".
    state: &'static str,
    now: bool,
    counts: String,
    episode: Option<String>,
}

/// Channels over range now first, then recent ones, each with its counts and
/// the current episode's duration.
fn adc_channel_lines(status: &DaqInputHealthStatus, now_ms: i64) -> Vec<AdcChannelLine> {
    let mut channels: Vec<_> = status.adc_channels.iter().collect();
    channels.sort_by_key(|c| (!c.over_range_now, c.bit));
    channels
        .into_iter()
        .map(|c| AdcChannelLine {
            key: c.key.clone(),
            label: c.label.clone(),
            state: match (c.over_range_now, c.low) {
                (true, true) => "Over range (low) now",
                (true, false) => "Over range now",
                (false, _) if c.latched => "Latched: recently over range",
                (false, _) => "Recently over range",
            },
            now: c.over_range_now,
            counts: format!("{} in the last 60 s · {} since DAQ boot", c.count_recent, c.count_total),
            episode: c
                .first_over_range_ms
                .filter(|_| c.over_range_now)
                .map(|first| format!("Current episode: {}", duration(now_ms.saturating_sub(first).max(0) as u64))),
        })
        .collect()
}

#[component]
fn DaqInputHealth(status: DaqInputHealthStatus, connected: bool, elapsed: u64, theme: ThemeConfig) -> Element {
    let fresh = daq_input_fresh(&status, connected, elapsed);
    let adc_lines = adc_channel_lines(&status, super::current_wallclock_ms());
    let vdda = vdda_text(&status);
    let station_vref = *super::station_settings::STATION_MCP_VREF_V.read();
    let mcp_vref = mcp_vref_line(&status, station_vref);
    let reports_vref = status.mcp_vref_v.is_some();
    let settings_owner = super::station_settings_request::use_factory();
    use_effect(move || {
        // The station's setting, once, so a DAQ on its default can be compared.
        if !reports_vref || super::station_settings::STATION_MCP_VREF_V.peek().is_some() {
            return;
        }
        spawn(async move {
            if let Ok(raw) = settings_owner.get::<serde_json::Value>("/api/calibration").await
                && let Some(value) = super::station_settings::mcp_vref_from_calibration(&raw)
            {
                *super::station_settings::STATION_MCP_VREF_V.write() = Some(value);
            }
        });
    });
    rsx! {
        section { "data-daq-input": if fresh { "fresh" } else { "stale" }, style:"padding:14px;border:1px solid {theme.border_soft};border-radius:10px;min-width:0;",
            h4 { style:"margin:0;", "DAQ input health" }
            if !fresh { p { role:"status", "Input report stale · {duration(status.received_age_ms.saturating_add(elapsed))} ago" } }
            ul { style:"list-style:none;padding:0;margin:12px 0 0;display:grid;gap:8px;",
                for fault in status.faults.iter() {
                    li { key:"{fault.key}", "data-fault":"{fault.key}", "data-active": if fault.active { "true" } else { "false" }, style:"min-width:0;",
                        div { style:"display:flex;flex-wrap:wrap;justify-content:space-between;gap:8px;",
                            span { "{fault.label}" }
                            if fault.active {
                                span { role:"status", style:"border-radius:999px;padding:3px 9px;background:#9f1239;color:white;font-size:0.8rem;", "Active" }
                            } else if fault.latched {
                                span { role:"status", style:"border-radius:999px;padding:3px 9px;background:#815b12;color:white;font-size:0.8rem;", "Brief" }
                            } else {
                                span { style:"border-radius:999px;padding:3px 9px;background:#146c43;color:white;font-size:0.8rem;", "OK" }
                            }
                        }
                        if fault.active || fault.latched { p { style:"margin:4px 0 0;font-size:0.85rem;color:{theme.text_secondary};", "{fault.advice}" } }
                        if fault.episodes > 0 { p { style:"margin:4px 0 0;font-size:0.8rem;color:{theme.text_secondary};", "Episodes since DAQ boot: {fault.episodes}" } }
                    }
                }
            }
            if !adc_lines.is_empty() {
                h5 { style:"margin:12px 0 0;", "ADC over range" }
                ul { "data-adc-over-range": "", style:"list-style:none;padding:0;margin:8px 0 0;display:grid;gap:8px;",
                    for line in adc_lines {
                        li { key:"{line.key}", "data-adc-channel":"{line.key}", "data-now": if line.now { "true" } else { "false" }, style:"min-width:0;",
                            div { style:"display:flex;flex-wrap:wrap;justify-content:space-between;gap:8px;",
                                span { "{line.label}" }
                                span { role:"status", class: if line.now { "gs-badge gs-badge--danger" } else { "gs-badge gs-badge--warn" }, "{line.state}" }
                            }
                            p { style:"margin:4px 0 0;font-size:0.8rem;color:{theme.text_secondary};", "{line.counts}" }
                            if let Some(episode) = line.episode { p { style:"margin:4px 0 0;font-size:0.8rem;color:{theme.text_secondary};", "{episode}" } }
                        }
                    }
                }
                p { style:"font-size:0.8rem;margin:4px 0 0;color:{theme.text_secondary};", "Over-range readings are not valid: they show as OVR on the data and charts." }
            }
            dl { style:"display:grid;grid-template-columns:minmax(0,1fr) auto;gap:6px;font-size:0.85rem;margin:12px 0 0;",
                dt { "Supply (VMAIN)" } dd { style:"margin:0;", "{millivolts(status.vmain_mv)}" }
                dt { "Lowest in last second" } dd { style:"margin:0;", "{millivolts(status.vmain_min_mv)}" }
                dt { "Undervoltage level" } dd { style:"margin:0;", "{millivolts(Some(status.supply_low_mv))}" }
                if status.chamber_mv.is_some() || status.chamber_min_mv.is_some() {
                    dt { "Chamber transducer" } dd { style:"margin:0;", "{millivolts(status.chamber_mv)}" }
                    dt { "No-signal level" } dd { style:"margin:0;", "{millivolts(Some(status.chamber_floor_mv))}" }
                }
                if status.kg50_p6 {
                    dt { "P6 leg difference (max)" } dd { style:"margin:0;", "{millivolts(status.p6_imbalance_max_mv)}" }
                    dt { "P6 leg mean" } dd { style:"margin:0;", "{millivolts(status.p6_common_mv)}" }
                }
                if let Some((text, warning)) = mcp_vref {
                    dt { "MCP3564R reference" }
                    dd { style:"margin:0;", "data-mcp-vref": if warning.is_some() { "warn" } else { "ok" },
                        if let Some(reason) = warning.clone() {
                            span { class:"gs-badge gs-badge--warn", title:"{reason}", "{text}" }
                            p { style:"margin:4px 0 0;font-size:0.8rem;color:{theme.text_secondary};", "{reason}" }
                        } else { "{text}" }
                    }
                }
                if let Some((text, level)) = vdda {
                    dt { "+3V3A (VDDA)" }
                    dd { style:"margin:0;", "data-vdda": match level { VddaLevel::Ok => "ok", VddaLevel::Warn => "warn", VddaLevel::Danger => "danger" },
                        match level {
                            VddaLevel::Ok => rsx! { "{text}" },
                            VddaLevel::Warn => rsx! { span { class:"gs-badge gs-badge--warn", title:"VDDA outside 3.15–3.45 V: ADC readings may be off", "{text}" } },
                            VddaLevel::Danger => rsx! { span { class:"gs-badge gs-badge--danger", title:"VDDA above 3.6 V: over the STM32 limit", "{text}" } },
                        }
                    }
                }
            }
            p { style:"font-size:0.8rem;margin:8px 0 0;color:{theme.text_secondary};", "Warnings only: these checks never block a command." }
        }
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
    // Same verdict as the board card: samples refused before the card count
    // as dropped even while the card itself is writing.
    let rejected = report.sd_rejected_records.unwrap_or(0);
    let dropping = rejected.saturating_add(report.sd_write_errors.unwrap_or(0)) > 0 && state != "unknown";
    let label = if dropping && state != "error" { format!("{label} · dropping samples") } else { label.to_string() };
    let color = match state {
        "error" => "#9f1239",
        _ if dropping => "#9f1239",
        "good" => "#146c43",
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
                    dt { "Dropped before the card" } dd { style:"margin:0;", "{rejected}" }
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
                            strong { "{row.sender_id}" }
                            // One indicator per aspect: a reachable board that has aborted is
                            // Network healthy but State unhealthy, never one "Healthy" badge.
                            div { class: "gs26-health-lines", style: "display:grid;gap:var(--gs-space-1,4px);margin:var(--gs-space-2,8px) 0;min-width:0;",
                                {board_display::health_line("network", "Network", if ok {board_display::Health::Healthy} else {board_display::Health::Unhealthy}, &format!("seen {age} ago"))}
                                if let Some(render) = recovery_renderer { {render.call((RecoverySlot::State, board_id.clone(), ws_connected, theme.clone()))} }
                                if let Some((health, value)) = sd_health(row, ws_connected, elapsed) {
                                    {board_display::health_line("sd", "SD", health, &value)}
                                }
                                if let Some((health, value)) = daq_input_health(row, ws_connected, elapsed) {
                                    {board_display::health_line("inputs", "Inputs", health, &value)}
                                }
                            }
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
                                        div{class:"gs26-modal-scroll-body gs26-board-info-columns",style:"columns:2 340px;column-gap:14px;flex:1;min-height:0;overflow-y:auto;overflow-anchor:none;overscroll-behavior:contain;padding:2px;",
                                            // Two masonry columns: each block stacks under the one above it and
                                            // is never split across columns (the recovery block is not a direct
                                            // section, and three columns cut it in two at the top of the third).
                                            style{".gs26-board-info-columns>*{{break-inside:avoid;display:block;width:100%;box-sizing:border-box;margin:0 0 14px;}} .gs26-board-info-columns>style{{display:none;}}"}
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
                                            if let Some(status) = row.diagnostics.as_ref().and_then(|report| report.daq_input.clone()) {
                                                DaqInputHealth { status, connected:ws_connected, elapsed, theme:theme.clone() }
                                            }
                                            if let Some(render) = recovery_renderer { div{class:"gs26-board-info-recovery",{render.call((RecoverySlot::Details, board_id.clone(), ws_connected, theme.clone()))}} }
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
                                        for (line, warn) in power_monitor_lines(report) {
                                            if warn { p { role: "status", style: "color:{theme.warning_text};", "{line}" } } else { p { "{line}" } }
                                        }
                                        for (line, warn) in current_sense_lines(report) {
                                            if warn { p { role: "status", style: "color:{theme.warning_text};", "{line}" } } else { p { "{line}" } }
                                        }
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

/// GB/PB LTC2990 power-monitor lines (read-only); `true` marks a warning.
/// An absent monitor is named on its own line: its readings are not trusted.
fn power_monitor_lines(report: &BoardDiagnostics) -> Vec<(String, bool)> {
    let Some(power) = report.power_monitors.as_ref() else { return Vec::new() };
    let mut lines = Vec::new();
    for (name, present) in [("Voltage", power.voltage_present), ("Current", power.current_present)] {
        lines.push(if present {
            (format!("{name} monitor (LTC2990): answering"), false)
        } else {
            (format!("{name} monitor (LTC2990): monitor absent, readings not trusted"), true)
        });
    }
    if power.retry_delay_ms > 0 {
        lines.push((format!("Power monitor I2C retry every {} s", power.retry_delay_ms.div_ceil(1000)), true));
    }
    lines.push(match (power.bus_clears, power.failed_bus_clears) {
        (Some(clears), Some(failed)) => (
            format!("Power monitor I2C retries / bus clears / failed clears: {} / {clears} / {failed}", power.retries),
            failed > 0,
        ),
        _ => (format!("Power monitor I2C retries: {}", power.retries), false),
    });
    lines
}

/// AB Hall current-sense lines (read-only); `true` marks a warning.
fn current_sense_lines(report: &BoardDiagnostics) -> Vec<(String, bool)> {
    let Some(sense) = report.current_sense.as_ref() else { return Vec::new() };
    let untrusted = if sense.untrusted.is_empty() { "none".to_string() } else { sense.untrusted.join(", ") };
    let mut lines = vec![
        (format!("Current sense untrusted (not acted on): {untrusted}"), false),
        (format!("Hall mismatch abort: {}", if sense.mismatch_abort_enabled { "compiled in" } else { "off" }), false),
    ];
    if !sense.mismatch_latched.is_empty() {
        lines.push((format!("Current mismatch seen since boot: {}", sense.mismatch_latched.join(", ")), true));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_monitor_and_current_sense_lines_follow_the_backend_report() {
        let mut report = BoardDiagnostics::default();
        assert!(power_monitor_lines(&report).is_empty() && current_sense_lines(&report).is_empty());
        report = serde_json::from_str(r#"{"version":1,"received_age_ms":0,"uptime_ticks":0,"uptime_frequency_hz":1000,"reset_flags":0,"watchdog_missing_mask":0,"heap_live_bytes":null,"heap_peak_bytes":null,"heap_region_bytes":null,"allocation_failures":null,"admission_refusals":0,"can_rx_drops":null,"can_tx_rejected":null,"uart_rx_drops":null,"tx_queue_high_water":null,"sd_accepted_records":null,"sd_processed_records":null,"sd_rejected_records":null,"sd_flushed_records":null,"sd_write_errors":null,"sd_max_write_ms":null,"adc_overruns":null,"diagnostic_send_failures":0,"power_monitors":{"voltage_present":true,"current_present":false,"retry_delay_ms":4000,"retries":9,"bus_clears":2,"failed_bus_clears":0},"current_sense":{"untrusted":["N2O","N2","Igniter","H-bridge"],"mismatch_abort_enabled":false,"mismatch_latched":["N2"]}}"#).unwrap();
        assert_eq!(power_monitor_lines(&report), vec![
            ("Voltage monitor (LTC2990): answering".to_string(), false),
            ("Current monitor (LTC2990): monitor absent, readings not trusted".to_string(), true),
            ("Power monitor I2C retry every 4 s".to_string(), true),
            ("Power monitor I2C retries / bus clears / failed clears: 9 / 2 / 0".to_string(), false),
        ]);
        // PB: no bus-clear counters.
        let pb: BoardDiagnostics = serde_json::from_str(&serde_json::to_string(&BoardDiagnostics {
            power_monitors: Some(crate::telemetry_dashboard::types::PowerMonitorHealth {
                voltage_present: false, current_present: true, retry_delay_ms: 0, retries: 3,
                bus_clears: None, failed_bus_clears: None }),
            ..Default::default()
        }).unwrap()).unwrap();
        assert_eq!(power_monitor_lines(&pb), vec![
            ("Voltage monitor (LTC2990): monitor absent, readings not trusted".to_string(), true),
            ("Current monitor (LTC2990): answering".to_string(), false),
            ("Power monitor I2C retries: 3".to_string(), false),
        ]);
        assert_eq!(current_sense_lines(&report), vec![
            ("Current sense untrusted (not acted on): N2O, N2, Igniter, H-bridge".to_string(), false),
            ("Hall mismatch abort: off".to_string(), false),
            ("Current mismatch seen since boot: N2".to_string(), true),
        ]);
    }
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
    fn daq_input_line_names_active_faults_and_never_hides_staleness() {
        use super::board_display::Health;
        use crate::telemetry_dashboard::types::{DaqInputFaultStatus, DaqInputHealthStatus};
        let fault = |key: &str, label: &str, active, latched| DaqInputFaultStatus {
            key: key.into(), label: label.into(), active, latched, episodes: 1, advice: String::new(),
        };
        let mut row = NetworkBoardHealthEntry { sender_id: "DAQ".into(), ..Default::default() };
        assert!(daq_input_health(&row, true, 0).is_none(), "no block without a report");
        let mut status = DaqInputHealthStatus {
            faults: vec![fault("supply_low", "DAQ supply (VMAIN) low", false, false),
                         fault("chamber_no_signal", "Chamber pressure transducer no signal", true, true)],
            ..Default::default()
        };
        row.diagnostics = Some(BoardDiagnostics { daq_input: Some(status.clone()), ..Default::default() });
        assert_eq!(daq_input_health(&row, true, 0), Some((Health::Unhealthy, "Chamber pressure transducer no signal".into())));
        assert_eq!(daq_input_health(&row, true, 6_000).unwrap().0, Health::Stale);
        assert_eq!(daq_input_health(&row, false, 0).unwrap().0, Health::Stale);
        status.faults[1].active = false;
        status.faults[1].latched = false;
        status.faults[0].latched = true;
        row.diagnostics = Some(BoardDiagnostics { daq_input: Some(status), ..Default::default() });
        assert_eq!(daq_input_health(&row, true, 0), Some((Health::Healthy, "OK · brief: DAQ supply (VMAIN) low".into())));
        assert_eq!(millivolts(Some(12)), "0.012 V");
        assert_eq!(millivolts(Some(11_060)), "11.06 V");
        // The backend's JSON for the block.
        let decoded: BoardDiagnostics = serde_json::from_str(r#"{"version":1,"received_age_ms":0,"uptime_ticks":0,"uptime_frequency_hz":1000,"reset_flags":0,"watchdog_missing_mask":0,"heap_live_bytes":null,"heap_peak_bytes":null,"heap_region_bytes":null,"allocation_failures":null,"admission_refusals":0,"can_rx_drops":null,"can_tx_rejected":null,"uart_rx_drops":null,"tx_queue_high_water":null,"sd_accepted_records":null,"sd_processed_records":null,"sd_rejected_records":null,"sd_flushed_records":null,"sd_write_errors":null,"sd_max_write_ms":null,"adc_overruns":null,"diagnostic_send_failures":0,"daq_input":{"received_age_ms":250,"faults":[{"key":"supply_low","label":"DAQ supply (VMAIN) low","active":true,"latched":true,"episodes":14,"advice":"Check the DAQ power/ground connector."}],"kg50_p6":true,"vmain_mv":11100,"vmain_min_mv":11030,"chamber_mv":null,"chamber_min_mv":null,"p6_imbalance_max_mv":4,"p6_common_mv":1137,"supply_low_mv":12500,"chamber_floor_mv":50}}"#).unwrap();
        let input = decoded.daq_input.unwrap();
        assert_eq!((input.faults[0].episodes, input.vmain_min_mv, input.supply_low_mv), (14, Some(11_030), 12_500));
    }

    fn render_daq_input(status: DaqInputHealthStatus) -> String {
        let mut dom = VirtualDom::new_with_props(
            |status: DaqInputHealthStatus| rsx! { DaqInputHealth { status, connected: true, elapsed: 0, theme: ThemeConfig::default() } },
            status,
        );
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    fn v2_status() -> DaqInputHealthStatus {
        serde_json::from_str(r#"{"received_age_ms":250,"faults":[{"key":"supply_low","label":"DAQ supply (VMAIN) low","active":false,"latched":false,"episodes":0,"advice":""}],"kg50_p6":false,"vmain_mv":12100,"vmain_min_mv":12050,"chamber_mv":null,"chamber_min_mv":null,"p6_imbalance_max_mv":null,"p6_common_mv":null,"supply_low_mv":11500,"chamber_floor_mv":50}"#).unwrap()
    }

    #[test]
    fn v2_daq_input_status_renders_without_adc_or_vdda_lines() {
        let status = v2_status();
        assert!(status.adc_channels.is_empty() && status.vdda_v.is_none() && status.vdda_max_v.is_none());
        let html = render_daq_input(status.clone());
        assert!(!html.contains("ADC over range") && !html.contains("VDDA") && !html.contains("data-adc"), "{html}");
        assert!(html.contains("Supply (VMAIN)") && html.contains("DAQ supply (VMAIN) low"));
        let row = NetworkBoardHealthEntry {
            sender_id: "DAQ".into(),
            diagnostics: Some(BoardDiagnostics { daq_input: Some(status), ..Default::default() }),
            ..Default::default()
        };
        assert_eq!(daq_input_health(&row, true, 0), Some((board_display::Health::Healthy, "OK".into())));
    }

    #[test]
    fn daq_input_lists_over_range_channels_and_vdda() {
        use crate::telemetry_dashboard::types::{DaqAdcChannelStatus, Volts};
        let mut status: DaqInputHealthStatus = serde_json::from_str(r#"{"received_age_ms":250,"faults":[{"key":"vdda_high","label":"DAQ +3V3A (VDDA) high","active":false,"latched":true,"episodes":1,"advice":""}],"supply_low_mv":11500,"chamber_floor_mv":50,
            "adc_channels":[
              {"key":"kg50","label":"KG50 load cell","bit":2,"over_range_now":false,"low":false,"latched":true,"count_total":40,"count_recent":3,"first_over_range_ms":null,"last_over_range_ms":1000},
              {"key":"kg1000","label":"KG1000 load cell","bit":0,"over_range_now":true,"low":true,"latched":true,"count_total":120,"count_recent":95,"first_over_range_ms":10000,"last_over_range_ms":74000}],
            "vdda_v":3.31,"vdda_max_v":3.48}"#).unwrap();
        let lines = adc_channel_lines(&status, 75_000);
        assert_eq!(lines.iter().map(|l| l.key.as_str()).collect::<Vec<_>>(), ["kg1000", "kg50"], "over range now first");
        assert_eq!(lines[0].state, "Over range (low) now");
        assert_eq!(lines[0].counts, "95 in the last 60 s · 120 since DAQ boot");
        assert_eq!(lines[0].episode.as_deref(), Some("Current episode: 1m 5s"));
        assert_eq!(lines[1].state, "Latched: recently over range");
        assert_eq!(lines[1].episode, None, "no current episode once it has ended");

        assert_eq!(vdda_text(&status), Some(("3.31 V (max 3.48 V)".into(), VddaLevel::Warn)));
        assert_eq!(vdda_level(Some(3.30), Some(3.31)), VddaLevel::Ok);
        assert_eq!(vdda_level(Some(3.10), Some(3.20)), VddaLevel::Warn);
        assert_eq!(vdda_level(Some(3.30), Some(3.61)), VddaLevel::Danger);
        assert_eq!(vdda_level(Some(3.62), None), VddaLevel::Danger);
        assert_eq!(vdda_level(None, Some(3.40)), VddaLevel::Ok);

        let html = render_daq_input(status.clone());
        assert!(html.contains("data-adc-channel=\"kg1000\""), "{html}");
        assert!(html.contains("Over range (low) now") && html.contains("gs-badge--danger"));
        assert!(html.contains("KG50 load cell") && html.contains("3 in the last 60 s · 40 since DAQ boot"));
        assert!(html.contains("+3V3A (VDDA)") && html.contains("3.31 V (max 3.48 V)"));
        assert!(html.contains("data-vdda=\"warn\""));
        status.vdda_max_v = Some(Volts(3.65));
        assert!(render_daq_input(status.clone()).contains("data-vdda=\"danger\""));
        status.vdda_v = Some(Volts(3.30));
        status.vdda_max_v = Some(Volts(3.32));
        assert!(render_daq_input(status.clone()).contains("data-vdda=\"ok\""));

        // The card's Inputs line names a channel over range now.
        let row = NetworkBoardHealthEntry {
            sender_id: "DAQ".into(),
            diagnostics: Some(BoardDiagnostics { daq_input: Some(status.clone()), ..Default::default() }),
            ..Default::default()
        };
        assert_eq!(daq_input_health(&row, true, 0), Some((board_display::Health::Unhealthy, "ADC over range: KG1000 load cell".into())));
        status.adc_channels = vec![DaqAdcChannelStatus { key: "kg50".into(), label: "KG50".into(), latched: true, ..Default::default() }];
        let row = NetworkBoardHealthEntry {
            sender_id: "DAQ".into(),
            diagnostics: Some(BoardDiagnostics { daq_input: Some(status), ..Default::default() }),
            ..Default::default()
        };
        assert_eq!(daq_input_health(&row, true, 0).unwrap().0, board_display::Health::Healthy, "recent only: named in the panel, not red");
    }

    #[test]
    fn daq_input_shows_the_mcp_reference_and_warns_on_drift_or_default() {
        use crate::telemetry_dashboard::types::Volts;
        let mut status = v2_status();
        assert!(status.mcp_vref_v.is_none() && status.mcp_vref_source.is_none(), "serde default when absent");
        assert_eq!(mcp_vref_line(&status, Some(2.2)), None);
        let decoded: DaqInputHealthStatus = serde_json::from_str(
            r#"{"received_age_ms":0,"supply_low_mv":0,"chamber_floor_mv":0,"mcp_vref_v":2.2,"mcp_vref_source":"station"}"#,
        ).unwrap();
        assert_eq!(decoded.mcp_vref_source.as_deref(), Some("station"));
        let (text, warning) = mcp_vref_line(&decoded, Some(2.2)).unwrap();
        assert_eq!(text, "2.200 V (station)");
        assert!(warning.unwrap().contains("2.35–2.45 V"), "2.2 V is a drifted part");
        status.mcp_vref_v = Some(Volts(2.4));
        status.mcp_vref_source = Some("station".into());
        assert_eq!(mcp_vref_line(&status, Some(2.4)), Some(("2.400 V (station)".into(), None)));
        status.mcp_vref_source = Some("default".into());
        assert_eq!(mcp_vref_line(&status, Some(2.4)).unwrap().1, None, "default equal to the station setting");
        assert!(mcp_vref_line(&status, Some(2.2)).unwrap().1.unwrap().contains("not the station's 2.200 V"));
        assert_eq!(mcp_vref_line(&status, None).unwrap().1, None, "station setting unknown");
        let html = render_daq_input(decoded);
        assert!(html.contains("MCP3564R reference") && html.contains("2.200 V (station)"), "{html}");
        assert!(html.contains("data-mcp-vref=\"warn\""));
        assert!(!render_daq_input(v2_status()).contains("MCP3564R"));
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

/// The offset is against the station's network time in the frame's own time
/// base (a network clock step is not a board offset), from the least-delayed
/// frame in 10 s, so it is clock error plus the best transit seen. A healthy radio carries a frame well inside 500 ms;
/// more means the clock or the link needs attention (2 s let a board read
/// "in sync" at 1.9 s behind).
const CLOCK_DESYNC_MS: i64 = 500;
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
    format!("{amount} {side} network time")
}
#[cfg(test)]
mod clock_tests {
    use super::*;
    #[test]
    fn small_offsets_are_in_sync_and_uptime_clocks_are_flagged() {
        assert!(!clock_desynced(-40));
        assert!(!clock_desynced(400));
        assert!(clock_desynced(-1_000));
        assert!(clock_desynced(1_500));
        assert!(clock_desynced(-1_794_183_000_000));
        assert_eq!(clock_offset_text(-25), "25 ms behind network time");
        assert!(clock_offset_text(-1_794_183_000_000).ends_with("days behind network time"));
    }
}
