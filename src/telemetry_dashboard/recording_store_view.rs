//! Always-on recording: the status indicator, the rolling ring, and the
//! protected event captures (launch, each sequence run, aborts, snapshots).
use super::layout::ThemeConfig;
use super::neutral_request_owner;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct RingStatus {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub segments: usize,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub budget_bytes: u64,
    /// The budget applied now: the ceiling, limited by free disk above the reserve.
    #[serde(default)]
    pub effective_budget_bytes: u64,
    #[serde(default)]
    pub oldest_ms: Option<i64>,
    #[serde(default)]
    pub newest_ms: Option<i64>,
    #[serde(default)]
    pub free_bytes: Option<u64>,
    #[serde(default)]
    pub disk: String,
    #[serde(default)]
    pub warning: Option<String>,
    #[serde(default)]
    pub deleted_segments: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct Manifest {
    pub label: String,
    pub event_start_ms: i64,
    pub event_end_ms: i64,
    pub window_start_ms: i64,
    pub window_end_ms: i64,
    #[serde(default)]
    pub station_mode: String,
    #[serde(default)]
    pub operator: Option<String>,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub pre_roll_truncated: bool,
    #[serde(default)]
    pub rows: BTreeMap<String, i64>,
    #[serde(default)]
    pub board_sd: Vec<BoardSdEntry>,
}

/// A board SD launch file attached automatically to a launch capture.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct BoardSdEntry {
    pub board: String,
    pub file: String,
    pub bytes: u64,
    pub data_from_ms: i64,
    pub data_to_ms: i64,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub lead_up_file: Option<String>,
    #[serde(default)]
    pub lead_up_rows: u64,
    /// `radio` or `wired` for network downloads.
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub relay: Option<RelayProvenance>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct RelayProvenance {
    #[serde(default)]
    pub via: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(super) struct Attachment {
    pub id: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(super) struct CaptureEntry {
    pub id: String,
    pub event_type: String,
    pub title: String,
    pub label: String,
    pub bytes: u64,
    pub manifest: Manifest,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(super) struct Group {
    pub event_type: String,
    pub title: String,
    pub captures: Vec<CaptureEntry>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub(super) struct LegacyEntry {
    pub id: String,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct EventPolicy {
    pub auto: bool,
    pub pre_roll_s: u32,
    pub post_roll_s: u32,
    pub max_duration_s: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Settings {
    pub ring_budget_mb: u64,
    pub segment_mb: u64,
    pub warn_free_mb: u64,
    pub critical_free_mb: u64,
    #[serde(default)]
    pub reserve_free_mb: u64,
    pub events: BTreeMap<String, EventPolicy>,
    /// Collect the FC SD launch file automatically after landing.
    #[serde(default = "default_true")]
    pub fc_sd_auto_collect: bool,
    /// Also fetch the FC's 60 s lead-up.
    #[serde(default = "default_true")]
    pub fc_sd_leadup: bool,
}

fn default_true() -> bool {
    true
}

/// The automatic FC SD collection's active job.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct FcActive {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub bytes_done: u64,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub bytes_per_second: u64,
    #[serde(default)]
    pub paused_reason: Option<String>,
    #[serde(default)]
    pub eta_seconds: Option<u64>,
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub crc_errors: u64,
    #[serde(default)]
    pub message: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct FcJobBrief {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct PowerOff {
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub at: Option<String>,
    #[serde(default)]
    pub message: String,
}

/// `fc_sd_collect` in the store listing (older stations omit it).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct FcCollect {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub stopped: bool,
    #[serde(default)]
    pub stopped_by: Option<String>,
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub rate_bytes_per_sec: u64,
    #[serde(default)]
    pub active: Option<FcActive>,
    #[serde(default)]
    pub jobs: Vec<FcJobBrief>,
    #[serde(default)]
    pub power_off: PowerOff,
    /// RRLS -> GS forward: (file, bytes done, total).
    #[serde(default)]
    pub forward: Option<(String, u64, u64)>,
    #[serde(default)]
    pub forward_message: String,
}

/// One status line for the FC collection's active job.
pub(super) fn fc_progress_text(active: &FcActive) -> String {
    let file = active.file.clone().unwrap_or_else(|| "FC SD card".into());
    let mut text = format!("FC SD {file}: {}", if active.state.is_empty() { "waiting" } else { active.state.as_str() });
    if active.total_bytes > 0 {
        let pct = (active.bytes_done as f64 * 100.0 / active.total_bytes as f64).min(100.0);
        text.push_str(&format!(" · {} of {} ({pct:.0} %)", super::sd_board_download::human_bytes(active.bytes_done), super::sd_board_download::human_bytes(active.total_bytes)));
    }
    if active.bytes_per_second > 0 {
        text.push_str(&format!(" · {}/s", super::sd_board_download::human_bytes(active.bytes_per_second)));
    }
    if let Some(eta) = active.eta_seconds {
        text.push_str(&format!(" · about {} left", duration_label(eta as i64 * 1000)));
    }
    if !active.link.is_empty() {
        text.push_str(&format!(" · {} link", active.link));
    }
    if let Some(reason) = &active.paused_reason {
        text.push_str(&format!(" · paused: {reason}"));
    }
    if active.crc_errors > 0 {
        text.push_str(&format!(" · {} blocks re-read after a CRC error", active.crc_errors));
    }
    text
}

/// A background DAQ lead-up fetch for an attached launch file.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct LeadUpFetch {
    pub launch_file: String,
    #[serde(default)]
    pub capture: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub fetched: u64,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct StoreView {
    #[serde(default)]
    pub always_on: bool,
    #[serde(default)]
    pub ring: RingStatus,
    #[serde(default)]
    pub in_progress: Vec<super::CaptureBriefMsg>,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub captures_bytes: u64,
    #[serde(default)]
    pub legacy: Vec<LegacyEntry>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub board_sd_fetch: Vec<LeadUpFetch>,
    #[serde(default)]
    pub fc_sd_collect: Option<FcCollect>,
}

pub(super) fn size_label(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    let mib = bytes as f64 / MIB;
    if mib >= 1024.0 { format!("{:.1} GB", mib / 1024.0) } else { format!("{mib:.0} MB") }
}

pub(super) fn duration_label(ms: i64) -> String {
    let s = (ms.max(0) / 1000) as u64;
    match s {
        0..=119 => format!("{s} s"),
        120..=7199 => format!("{} min", s / 60),
        _ => format!("{:.1} h", s as f64 / 3600.0),
    }
}

pub(super) fn utc_label(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let day = seconds.div_euclid(86_400);
    let tod = seconds.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (proleptic Gregorian).
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", tod / 3600, tod % 3600 / 60, tod % 60)
}

/// One line for the header/Actions: always on, and what is being captured.
pub(super) fn indicator_text(status: &super::RecordingStatusMsg) -> (bool, String) {
    let recording = status.mode == "recording";
    let mut text = if recording && status.always_on {
        "Recording · always on".to_owned()
    } else if recording {
        "Recording".to_owned()
    } else {
        "Not recording — check the station disk".to_owned()
    };
    for capture in &status.captures {
        let what = if capture.label.is_empty() || capture.label == capture.title {
            capture.title.clone()
        } else {
            format!("{} ({})", capture.title, capture.label)
        };
        if capture.event_end_ms.is_some() {
            text.push_str(&format!(" · saving {what}"));
        } else {
            text.push_str(&format!(" · capturing {what}"));
        }
    }
    (recording, text)
}

#[component]
pub(super) fn RecordingIndicator(status: Signal<super::RecordingStatusMsg>) -> Element {
    let (ok, text) = indicator_text(&status.read());
    let capturing = !status.read().captures.is_empty();
    rsx! {
        p {
            class: "gs26-recording-indicator",
            role: "status",
            "data-recording": if ok { "on" } else { "off" },
            "data-capturing": if capturing { "true" } else { "false" },
            style: "margin:0;display:flex;align-items:center;gap:0.5rem;flex-wrap:wrap;font-weight:700;",
            span { "aria-hidden": "true", style: if ok { "color:#ef4444;" } else { "color:#f59e0b;" }, "●" }
            "{text}"
        }
    }
}

/// Recordings page: ring status, captures grouped by event type, snapshot,
/// and (administrators) settings and deletion.
#[component]
pub(super) fn RecordingStorePanel(
    theme: ThemeConfig,
    on_select: EventHandler<String>,
    /// Opens the saved-data replay of a recording id (see `saved_replay`).
    #[props(default)] on_replay: Option<EventHandler<String>>,
    /// Opens the motor & flight summary of a recording id (see `flight_review_view`).
    #[props(default)] on_review: Option<EventHandler<String>>,
) -> Element {
    let review = move |id: String| {
        if let Some(handler) = on_review { handler.call(id); }
    };
    let replay = move |id: String| {
        on_select.call(id.clone());
        if let Some(handler) = on_replay { handler.call(id); }
    };
    let mut range_from = use_signal(String::new);
    let mut range_to = use_signal(String::new);
    let authority = neutral_request_owner::use_factory();
    let mut view = use_signal(|| None::<StoreView>);
    let mut message = use_signal(String::new);
    let mut refresh = use_signal(|| 0u32);
    let mut busy = use_signal(|| false);
    let mut label = use_signal(String::new);
    let mut minutes = use_signal(|| "2".to_string());
    let mut pending_delete = use_signal(|| None::<String>);
    let mut draft = use_signal(|| None::<Settings>);
    use_effect(move || {
        let _ = refresh();
        authority.spawn(async move {
            match authority.get::<StoreView>("/api/recordings/store").await {
                Ok(next) => view.set(Some(next)),
                Err(error) => message.set(format!("Could not load recordings: {error}")),
            }
        });
    });
    let Some(current) = view.read().clone() else {
        return rsx! { section { class: "gs26-recording-store", h3 { "Recordings" } p { role: "status", "Loading recordings…{message}" } } };
    };
    let ring = current.ring.clone();
    // Older stations report only the ceiling.
    let budget_now = if ring.effective_budget_bytes > 0 { ring.effective_budget_bytes } else { ring.budget_bytes };
    let span = match (ring.oldest_ms, ring.newest_ms) {
        (Some(a), Some(b)) => format!("{} ({} → now)", duration_label(b - a), utc_label(a)),
        _ => "no data yet".to_owned(),
    };
    let snapshot = move |_| {
        let minutes_value: u32 = minutes().trim().parse().unwrap_or(2).min(240);
        let body = serde_json::json!({"label": label(), "before_s": minutes_value * 60});
        busy.set(true);
        authority.spawn(async move {
            match authority.post::<_, serde_json::Value>("/api/recordings/snapshot", &body).await {
                Ok(_) => {
                    message.set("Protected capture requested. It appears below once saved (a few seconds).".into());
                    label.set(String::new());
                }
                Err(error) => message.set(format!("Snapshot failed: {error}")),
            }
            busy.set(false);
            refresh += 1;
        });
    };
    rsx! {
        section { class: "gs26-recording-store", style: "display:flex;flex-direction:column;gap:8px;min-width:0;",
            h3 { "Recordings" }
            p { class: "gs26-recording-always-on", role: "status",
                if current.always_on { "● Recording is always on. " } else { "Recording is not running on this station. " }
                "Rolling store: {span}, {size_label(ring.bytes)} of {size_label(budget_now)} in {ring.segments} segments (up to {size_label(ring.budget_bytes)}, keeping {current.settings.reserve_free_mb} MB free for captures)"
                if let Some(free) = ring.free_bytes { ", {size_label(free)} free on disk" }
                ". When full, the oldest rolling data is overwritten; event captures below are kept until an administrator deletes them."
            }
            if let Some(warning) = ring.warning.clone() {
                p { role: "alert", style: "color:{theme.warning_text};background:{theme.warning_background};border:1px solid {theme.warning_border};padding:6px;border-radius:6px;", "{warning}" }
            }
            if !current.in_progress.is_empty() {
                ul { "aria-label": "Captures in progress",
                    for c in current.in_progress.iter() {
                        li { key: "{c.kind}-{c.event_start_ms}",
                            if c.event_end_ms.is_some() { "Saving " } else { "Capturing " }
                            "{c.title}"
                            if !c.label.is_empty() && c.label != c.title { " — {c.label}" }
                            " (since {utc_label(c.event_start_ms)})"
                        }
                    }
                }
            }
            details { class: "gs-info",
                summary { "Save a protected capture now" }
                p { "Copies the last minutes of the rolling store into a protected capture (events/manual) that is never overwritten." }
                div { style: "display:flex;gap:8px;flex-wrap:wrap;align-items:end;",
                    label { "Label " input { r#type: "text", maxlength: "80", value: "{label}", placeholder: "e.g. Leak check", oninput: move |e| label.set(e.value()) } }
                    label { "Minutes before now " input { r#type: "number", min: "1", max: "240", value: "{minutes}", oninput: move |e| minutes.set(e.value()) } }
                    button { disabled: busy() || !current.always_on, onclick: snapshot, "Save protected capture" }
                }
            }
            if current.board_sd_fetch.iter().any(|f| !f.done) {
                ul { "aria-label": "Board SD lead-up fetches", class: "gs26-leadup-fetch",
                    for fetch in current.board_sd_fetch.iter().filter(|f| !f.done) {
                        li { key: "{fetch.launch_file}", role: "status",
                            "DAQ lead-up for {fetch.launch_file} ({fetch.phase}): {fetch.message}"
                        }
                    }
                }
            }
            if on_replay.is_some() {
                details { class: "gs-info", "data-replay-range": "true",
                    summary { "Replay a time range of the rolling store" }
                    p { "Replays every rolling segment the range touches as one recording (up to 6 hours), in the station mode recorded at the time." }
                    div { style: "display:flex;gap:8px;flex-wrap:wrap;align-items:end;",
                        label { "From (UTC) " input { r#type: "datetime-local", value: "{range_from}", oninput: move |e| range_from.set(e.value()) } }
                        label { "To (UTC) " input { r#type: "datetime-local", value: "{range_to}", oninput: move |e| range_to.set(e.value()) } }
                        button { onclick: move |_| {
                            match (super::recording_download::input_utc_ms(&range_from()), super::recording_download::input_utc_ms(&range_to())) {
                                (Ok(a), Ok(b)) if b > a => replay(super::saved_replay::range_id(a, b)),
                                (Ok(_), Ok(_)) => message.set("The range must end after it starts.".into()),
                                (Err(e), _) | (_, Err(e)) => message.set(e),
                            }
                        }, "Replay range" }
                        if on_review.is_some() {
                            button { "data-review-range": "", onclick: move |_| {
                                match (super::recording_download::input_utc_ms(&range_from()), super::recording_download::input_utc_ms(&range_to())) {
                                    (Ok(a), Ok(b)) if b > a => review(super::saved_replay::range_id(a, b)),
                                    (Ok(_), Ok(_)) => message.set("The range must end after it starts.".into()),
                                    (Err(e), _) | (_, Err(e)) => message.set(e),
                                }
                            }, "Motor & flight summary of the range" }
                        }
                    }
                }
            }
            if let Some(fc) = current.fc_sd_collect.clone() {
                FcCollectPanel { fc, busy: busy(), on_action: move |path: &'static str| {
                    busy.set(true);
                    authority.spawn(async move {
                        match authority.post::<_, serde_json::Value>(path, &serde_json::json!({})).await {
                            Ok(_) => message.set(if path.ends_with("stop") { "FC SD collection stopped; it stays stopped until resumed.".into() } else { "FC SD collection resumed.".into() }),
                            Err(error) => message.set(format!("FC SD collection: {error}")),
                        }
                        busy.set(false);
                        refresh += 1;
                    });
                } }
            }
            h4 { "Event captures ({size_label(current.captures_bytes)})" }
            if current.groups.is_empty() { p { "No event captures yet. Launches, aborts and every station sequence are captured automatically." } }
            for group in current.groups.iter() {
                details { key: "{group.event_type}", open: true,
                    summary { "{group.title} ({group.captures.len()})" }
                    ul { style: "list-style:none;padding:0;margin:0;display:flex;flex-direction:column;gap:6px;",
                        for capture in group.captures.iter() {
                            li { key: "{capture.id}", class: "gs26-capture", style: "border:1px solid {theme.border};border-radius:8px;padding:6px;min-width:0;overflow-wrap:anywhere;",
                                strong { "{capture.label}" }
                                " · {utc_label(capture.manifest.event_start_ms)} · {duration_label(capture.manifest.window_end_ms - capture.manifest.window_start_ms)} · {size_label(capture.bytes)}"
                                if let Some(result) = capture.manifest.result.clone() { div { "Result: {result}" } }
                                div { style: "color:{theme.text_muted};font-size:13px;",
                                    "Mode {capture.manifest.station_mode}"
                                    if let Some(op) = capture.manifest.operator.clone() { " · by {op}" }
                                    if capture.manifest.pre_roll_truncated { " · pre-roll shorter than configured (older data had rolled off)" }
                                }
                                div { style: "display:flex;gap:6px;flex-wrap:wrap;margin-top:4px;",
                                    button { onclick: { let id = capture.id.clone(); move |_| on_select.call(id.clone()) }, "Select for CSV / report / replay" }
                                    button { "data-replay-open": "{capture.id}", onclick: { let id = capture.id.clone(); move |_| replay(id.clone()) }, "Replay" }
                                    if !capture.manifest.board_sd.is_empty() {
                                        button { onclick: { let id = super::saved_replay::merge_sd_id(&capture.id); move |_| replay(id.clone()) }, "Replay with board SD" }
                                        button { onclick: { let id = super::saved_replay::sd_only_id(&capture.id); move |_| replay(id.clone()) }, "Replay board SD only" }
                                    }
                                    if on_review.is_some() {
                                        button { "data-review-open": "{capture.id}", onclick: { let id = capture.id.clone(); move |_| review(id.clone()) }, "Motor & flight summary / export" }
                                    }
                                    button { disabled: busy(), onclick: {
                                        let id = capture.id.clone();
                                        move |_| {
                                            let id = id.clone();
                                            let owner = match super::recording_download::ExportOwner::capture() { Ok(owner) => owner, Err(error) => { message.set(error); return; } };
                                            busy.set(true);
                                            spawn(async move {
                                                let result = super::recording_download::download_csv_path_owned(&format!("/api/recordings/{id}/file"), &id, owner.clone()).await;
                                                if owner.is_current() { message.set(result.unwrap_or_else(|error| format!("Download failed: {error}"))); }
                                                busy.set(false);
                                            });
                                        }
                                    }, "Download database" }
                                    button { disabled: busy(), title: "Everything needed to replay this recording with no station: rows, layout, calibration, diagrams, vehicle geometry, summaries and the CSV. Open it with Explore offline → Open replay file.", onclick: {
                                        let id = capture.id.clone();
                                        move |_| {
                                            let id = id.clone();
                                            let owner = match super::recording_download::ExportOwner::capture() { Ok(owner) => owner, Err(error) => { message.set(error); return; } };
                                            busy.set(true);
                                            spawn(async move {
                                                let name = format!("{}.gsreplay", id.trim_end_matches(".db"));
                                                let result = super::recording_download::download_csv_path_owned(&format!("/api/recordings/{id}/bundle"), &name, owner.clone()).await;
                                                if owner.is_current() { message.set(result.unwrap_or_else(|error| format!("Download failed: {error}"))); }
                                                busy.set(false);
                                            });
                                        }
                                    }, "Download replay bundle" }
                                    if current.is_admin {
                                        if pending_delete.read().as_deref() == Some(capture.id.as_str()) {
                                            button { disabled: busy(), onclick: {
                                                let id = capture.id.clone();
                                                move |_| {
                                                    let id = id.clone();
                                                    busy.set(true);
                                                    authority.spawn(async move {
                                                        match authority.post::<_, serde_json::Value>(&format!("/api/recordings/{id}/delete"), &serde_json::json!({})).await {
                                                            Ok(_) => message.set(format!("Deleted {id}")),
                                                            Err(error) => message.set(format!("Delete failed: {error}")),
                                                        }
                                                        pending_delete.set(None);
                                                        busy.set(false);
                                                        refresh += 1;
                                                    });
                                                }
                                            }, "Confirm permanent delete" }
                                            button { onclick: move |_| pending_delete.set(None), "Keep" }
                                        } else {
                                            button { onclick: { let id = capture.id.clone(); move |_| pending_delete.set(Some(id.clone())) }, "Delete…" }
                                        }
                                    }
                                }
                                for entry in capture.manifest.board_sd.iter() {
                                    div { key: "{entry.board}-{entry.file}", class: "gs26-board-sd",
                                        "{entry.board} SD launch file {entry.file} · {size_label(entry.bytes)} · {duration_label(entry.data_to_ms - entry.data_from_ms)}"
                                        if entry.lead_up_file.is_some() { " · with {entry.lead_up_rows} lead-up rows" }
                                        if let Some(link) = entry.link.clone() { " · {link} link" }
                                        if let Some(relay) = entry.relay.clone() { " · via {relay.via}" }
                                        " "
                                        for file in std::iter::once(entry.file.clone()).chain(entry.lead_up_file.clone()) {
                                            button { key: "{file}", disabled: busy(), onclick: {
                                                let path = format!("/api/recordings/{}/board_sd/{}/{}", capture.id, entry.board, file);
                                                let save_as = format!("{}-{}", entry.board, file);
                                                move |_| {
                                                    let (path, save_as) = (path.clone(), save_as.clone());
                                                    let owner = match super::recording_download::ExportOwner::capture() { Ok(owner) => owner, Err(error) => { message.set(error); return; } };
                                                    busy.set(true);
                                                    spawn(async move {
                                                        let result = super::recording_download::download_csv_path_owned(&path, &save_as, owner.clone()).await;
                                                        if owner.is_current() { message.set(result.unwrap_or_else(|error| format!("Download failed: {error}"))); }
                                                        busy.set(false);
                                                    });
                                                }
                                            }, if file.ends_with(".leadup.csv") { "Download lead-up" } else { "Download" } }
                                        }
                                    }
                                }
                                for attachment in capture.attachments.iter() {
                                    div { key: "{attachment.id}",
                                        "Attached board SD import · {size_label(attachment.bytes)} "
                                        button { onclick: { let id = attachment.id.clone(); move |_| on_select.call(id.clone()) }, "Select" }
                                        button { onclick: { let id = attachment.id.clone(); move |_| replay(id.clone()) }, "Replay" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !current.legacy.is_empty() {
                details {
                    summary { "Legacy recordings ({current.legacy.len()})" }
                    p { "Session files recorded before the rolling store. They stay readable and exportable." }
                    ul {
                        for entry in current.legacy.iter() {
                            li { key: "{entry.id}", "{entry.id} · {size_label(entry.bytes)} "
                                button { onclick: { let id = entry.id.clone(); move |_| on_select.call(id.clone()) }, "Select" }
                                button { onclick: { let id = entry.id.clone(); move |_| replay(id.clone()) }, "Replay" }
                            }
                        }
                    }
                }
            }
            if current.is_admin {
                details { class: "gs26-recording-settings",
                    summary { "Recording settings (administrators)" }
                    {
                        let settings = draft.read().clone().unwrap_or_else(|| current.settings.clone());
                        rsx! { RecordingSettingsForm { settings, busy: busy(), on_change: move |next| draft.set(Some(next)), on_save: move |next: Settings| {
                            busy.set(true);
                            authority.spawn(async move {
                                match authority.post::<_, Settings>("/api/recordings/settings", &next).await {
                                    Ok(saved) => { draft.set(Some(saved)); message.set("Recording settings saved.".into()); }
                                    Err(error) => message.set(format!("Saving settings failed: {error}")),
                                }
                                busy.set(false);
                                refresh += 1;
                            });
                        } } }
                    }
                }
            }
            div { style: "display:flex;gap:8px;", button { disabled: busy(), onclick: move |_| refresh += 1, "Refresh" } }
            p { role: "status", style: "overflow-wrap:anywhere;", "{message}" }
        }
    }
}

/// Recordings panel: the automatic FC SD collection after landing, the
/// "AV bay can be powered off" readiness, RRLS forward progress, Stop/Resume.
#[component]
fn FcCollectPanel(fc: FcCollect, busy: bool, on_action: EventHandler<&'static str>) -> Element {
    let who = fc.stopped_by.clone().unwrap_or_else(|| "an operator".into());
    let idle = fc.active.is_none() && !fc.stopped && !fc.power_off.ready && fc.jobs.is_empty();
    if idle && fc.forward.is_none() {
        return rsx! { p { class: "gs26-fc-sd", style: "margin:0;", if fc.enabled { "FC SD: collected automatically after the next landing ({fc.link} link)." } else { "FC SD: automatic collection is turned off (Recording settings)." } } };
    }
    rsx! {
        div { class: "gs26-fc-sd", role: "status", style: "display:flex;flex-direction:column;gap:4px;",
            p { style: "margin:0;font-weight:700;", "data-ready": if fc.power_off.ready { "true" } else { "false" },
                if fc.power_off.ready { "✔ " } else { "" }
                "{fc.power_off.message}"
            }
            if let Some(active) = fc.active.clone() {
                p { style: "margin:0;", "{fc_progress_text(&active)}" }
            } else if let Some(last) = fc.jobs.first() {
                p { style: "margin:0;", "FC SD: {last.message}" }
            }
            if let Some((file, done, total)) = fc.forward.clone() {
                p { style: "margin:0;", "RRLS → GS forward: {file} · {super::sd_board_download::human_bytes(done)} of {super::sd_board_download::human_bytes(total)}" }
            } else if !fc.forward_message.is_empty() {
                p { style: "margin:0;", "RRLS → GS: {fc.forward_message}" }
            }
            if fc.stopped {
                p { style: "margin:0;", "Stopped by {who}; it stays stopped until resumed." }
            }
            div { style: "display:flex;gap:6px;flex-wrap:wrap;",
                if fc.stopped {
                    button { disabled: busy, onclick: move |_| on_action.call("/api/sd_download/fc/resume"), "Resume FC SD collection" }
                } else {
                    button { disabled: busy, onclick: move |_| on_action.call("/api/sd_download/fc/stop"), "Stop FC SD collection" }
                }
            }
        }
    }
}

fn number_input(value: u64, on: impl Fn(u64) + 'static) -> Element {
    rsx! { input { r#type: "number", min: "0", value: "{value}", style: "width:7em;", oninput: move |e| on(e.value().trim().parse().unwrap_or(0)) } }
}

#[component]
fn RecordingSettingsForm(settings: Settings, busy: bool, on_change: EventHandler<Settings>, on_save: EventHandler<Settings>) -> Element {
    let s = settings.clone();
    rsx! {
        div { style: "display:flex;flex-direction:column;gap:6px;",
            div { style: "display:flex;gap:10px;flex-wrap:wrap;",
                label { "Rolling store size (MB) " {number_input(s.ring_budget_mb, { let s = s.clone(); move |v| { let mut n = s.clone(); n.ring_budget_mb = v; on_change.call(n); } })} }
                label { "Segment size (MB) " {number_input(s.segment_mb, { let s = s.clone(); move |v| { let mut n = s.clone(); n.segment_mb = v; on_change.call(n); } })} }
                label { "Warn below free (MB) " {number_input(s.warn_free_mb, { let s = s.clone(); move |v| { let mut n = s.clone(); n.warn_free_mb = v; on_change.call(n); } })} }
                label { "Keep free for captures and the OS (MB) " {number_input(s.reserve_free_mb, { let s = s.clone(); move |v| { let mut n = s.clone(); n.reserve_free_mb = v; on_change.call(n); } })} }
                label { "Shrink ring below free (MB) " {number_input(s.critical_free_mb, { let s = s.clone(); move |v| { let mut n = s.clone(); n.critical_free_mb = v; on_change.call(n); } })} }
            }
            table { class: "gs26-capture-policies",
                thead { tr { th { "Event" } th { "Auto-capture" } th { "Pre-roll (s)" } th { "Post-roll (s)" } th { "Max length (s)" } } }
                tbody {
                    for (kind, policy) in s.events.clone().into_iter() {
                        tr { key: "{kind}",
                            td { "{kind}" }
                            td { input { r#type: "checkbox", checked: policy.auto, disabled: kind == "manual", onchange: { let s = s.clone(); let kind = kind.clone(); move |e: Event<FormData>| { let mut n = s.clone(); if let Some(p) = n.events.get_mut(&kind) { p.auto = e.checked(); } on_change.call(n); } } } }
                            td { {number_input(policy.pre_roll_s.into(), { let s = s.clone(); let kind = kind.clone(); move |v| { let mut n = s.clone(); if let Some(p) = n.events.get_mut(&kind) { p.pre_roll_s = v.min(u32::MAX.into()) as u32; } on_change.call(n); } })} }
                            td { {number_input(policy.post_roll_s.into(), { let s = s.clone(); let kind = kind.clone(); move |v| { let mut n = s.clone(); if let Some(p) = n.events.get_mut(&kind) { p.post_roll_s = v.min(u32::MAX.into()) as u32; } on_change.call(n); } })} }
                            td { {number_input(policy.max_duration_s.into(), { let s = s.clone(); let kind = kind.clone(); move |v| { let mut n = s.clone(); if let Some(p) = n.events.get_mut(&kind) { p.max_duration_s = v.min(u32::MAX.into()) as u32; } on_change.call(n); } })} }
                        }
                    }
                }
            }
            div { style: "display:flex;gap:10px;flex-wrap:wrap;",
                label { input { r#type: "checkbox", checked: s.fc_sd_auto_collect, onchange: { let s = s.clone(); move |e: Event<FormData>| { let mut n = s.clone(); n.fc_sd_auto_collect = e.checked(); on_change.call(n); } } } " Collect the FC SD launch file automatically after landing" }
                label { input { r#type: "checkbox", checked: s.fc_sd_leadup, disabled: !s.fc_sd_auto_collect, onchange: { let s = s.clone(); move |e: Event<FormData>| { let mut n = s.clone(); n.fc_sd_leadup = e.checked(); on_change.call(n); } } } " Also fetch the FC's 60 s lead-up" }
            }
            p { "Values are clamped by the station (ring at least 256 MB; pre/post-roll at most 4 h)." }
            button { disabled: busy, onclick: { let s = s.clone(); move |_| on_save.call(s.clone()) }, "Save recording settings" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_fixture() -> StoreView {
        let mut value = store_fixture_base();
        value["groups"][0]["captures"][0]["manifest"]["board_sd"] = serde_json::json!([{
            "board": "DAQ", "file": "DAQ_LAUNCH_0007.CSV", "bytes": 4096, "sha256": "ab",
            "data_from_ms": 1_800_000_000_000i64, "data_to_ms": 1_800_000_020_000i64, "rows": 100,
            "matched_by": "utc_window", "source": "network_download",
            "lead_up_file": "DAQ_LAUNCH_0007.leadup.csv", "lead_up_from_ms": 1_799_999_940_000i64, "lead_up_rows": 40}]);
        serde_json::from_value(value).unwrap()
    }

    fn store_fixture_base() -> serde_json::Value {
        serde_json::json!({
            "always_on": true,
            "ring": {"enabled": true, "segments": 3, "bytes": 200u64 * 1024 * 1024, "budget_bytes": 51200u64 * 1024 * 1024, "effective_budget_bytes": 9000u64 * 1024 * 1024,
                     "segment_bytes": 64u64 * 1024 * 1024, "oldest_ms": 1_800_000_000_000i64, "newest_ms": 1_800_003_600_000i64,
                     "active_segment": "rolling.segment_x.db", "free_bytes": 1500u64 * 1024 * 1024, "disk": "low",
                     "deleted_segments": 0, "pinned_from_ms": null, "warning": "Station disk is low (1500 MB free)."},
            "in_progress": [{"kind": "fill", "title": "Fill", "label": "Fill", "event_start_ms": 1_800_003_000_000i64, "event_end_ms": null}],
            "groups": [{"event_type": "nitrogen_test", "title": "Nitrogen pressure test", "captures": [{
                "id": "event.nitrogen_test.2027-01-15_08-00-00_000_n2.db", "event_type": "nitrogen_test", "title": "Nitrogen pressure test",
                "label": "N2 test", "bytes": 1024u64 * 1024,
                "manifest": {"format": "seds-gs-event-capture", "label": "N2 test", "event_start_ms": 1_800_000_000_000i64, "event_end_ms": 1_800_000_600_000i64,
                    "window_start_ms": 1_799_999_880_000i64, "window_end_ms": 1_800_000_720_000i64, "station_mode": "normal",
                    "operator": "alice", "result": "Passed: holds", "pre_roll_truncated": false, "rows": {"telemetry": 10},},
                "attachments": [{"id": "event.nitrogen_test.2027-01-15_08-00-00_000_n2.groundstation_recording_1.db", "bytes": 10}]}]}],
            "captures_bytes": 2u64 * 1024 * 1024,
            "legacy": [{"id": "groundstation_recording_2026-11-09_06-26-25_821.db", "bytes": 195252224u64, "active": false, "kind": "legacy"}],
            "settings": {"ring_budget_mb": 51200, "segment_mb": 64, "warn_free_mb": 2048, "critical_free_mb": 512, "reserve_free_mb": 8192,
                         "events": {"fill": {"auto": true, "pre_roll_s": 120, "post_roll_s": 120, "max_duration_s": 21600}}},
            "is_admin": true,
            "board_sd_fetch": [{"capture": "recordings/events/launch/x", "launch_file": "DAQ_LAUNCH_0007.CSV", "from_ms": 1, "to_ms": 2,
                                "source": "DAQ_1_000.CSV", "fetched": 2048, "phase": "fetching", "message": "2048 of about 6600000 bytes", "done": false}]
        })
    }

    #[test]
    fn store_listing_decodes_ring_groups_legacy_and_settings() {
        let view = store_fixture();
        assert!(view.always_on);
        assert_eq!(view.ring.segments, 3);
        assert_eq!(view.groups[0].captures[0].manifest.result.as_deref(), Some("Passed: holds"));
        assert_eq!(view.groups[0].captures[0].attachments.len(), 1);
        assert_eq!(view.legacy[0].id, "groundstation_recording_2026-11-09_06-26-25_821.db");
        assert_eq!(view.settings.events["fill"].pre_roll_s, 120);
        assert_eq!(view.settings.reserve_free_mb, 8192);
        assert_eq!(view.board_sd_fetch[0].phase, "fetching");
        assert_eq!(view.board_sd_fetch[0].fetched, 2048);
        assert_eq!(view.ring.effective_budget_bytes, 9000 * 1024 * 1024);
        assert!(view.settings.fc_sd_auto_collect && view.settings.fc_sd_leadup, "older stations: on");
        assert!(view.fc_sd_collect.is_none());
        let board = &view.groups[0].captures[0].manifest.board_sd[0];
        assert_eq!((board.board.as_str(), board.lead_up_rows), ("DAQ", 40));
        assert_eq!(board.lead_up_file.as_deref(), Some("DAQ_LAUNCH_0007.leadup.csv"));
        let round = serde_json::to_value(&view.settings).unwrap();
        assert_eq!(round["events"]["fill"]["max_duration_s"], 21600);
    }

    #[test]
    fn recordings_page_renders_ring_captures_legacy_and_admin_controls() {
        #[component]
        fn Harness() -> Element {
            let view = store_fixture();
            rsx! { RecordingSettingsForm { settings: view.settings.clone(), busy: false, on_change: |_| {}, on_save: |_| {} } }
        }
        let mut dom = VirtualDom::new(Harness);
        dom.rebuild_in_place();
        let html = dioxus::ssr::render(&dom);
        assert!(html.contains("Rolling store size (MB)"), "{html}");
        assert!(html.contains("Keep free for captures and the OS (MB)"));
        assert!(html.contains("8192"));
        assert!(html.contains("Pre-roll (s)"));
        assert!(html.contains("21600"));
    }

    #[test]
    fn fc_collection_decodes_and_renders_progress_power_off_and_stop() {
        let fc: FcCollect = serde_json::from_value(serde_json::json!({
            "enabled": true, "lead_up_enabled": true, "stopped": false, "stopped_by": null, "link": "radio", "rate_bytes_per_sec": 512,
            "active": {"id": 1, "state": "paused", "file": "irec_LAUNCH-1799999990-000.csv", "bytes_done": 1024 * 1024, "total_bytes": 2 * 1024 * 1024,
                       "bytes_per_second": 512, "paused_reason": "commands were sent recently", "eta_seconds": 2048, "link": "radio", "crc_errors": 2, "message": ""},
            "jobs": [{"id": 1, "phase": "paused", "message": "paused", "done": false}],
            "power_off": {"ready": false, "at": null, "message": "FC SD not yet collected: keep the AV bay powered"},
            "forward": ["irec_LAUNCH-1799999990-000.csv", 512, 4096], "forward_message": "", "rrls_outbox": []
        })).unwrap();
        let text = fc_progress_text(fc.active.as_ref().unwrap());
        assert!(text.contains("50 %") && text.contains("paused: commands were sent recently") && text.contains("radio link"), "{text}");
        #[component]
        fn Harness(fc: FcCollect) -> Element {
            rsx! { FcCollectPanel { fc, busy: false, on_action: |_| {} } }
        }
        let mut dom = VirtualDom::new_with_props(Harness, HarnessProps { fc: fc.clone() });
        dom.rebuild_in_place();
        let html = dioxus::ssr::render(&dom);
        assert!(html.contains("keep the AV bay powered") && html.contains("Stop FC SD collection") && html.contains("RRLS → GS forward"), "{html}");
        let ready = FcCollect { stopped: true, power_off: PowerOff { ready: true, at: Some("RRLS".into()), message: "FC SD fully received by RRLS, checksums verified: AV bay can be powered off".into() }, ..fc };
        let mut dom = VirtualDom::new_with_props(Harness, HarnessProps { fc: ready });
        dom.rebuild_in_place();
        let html = dioxus::ssr::render(&dom);
        assert!(html.contains("AV bay can be powered off") && html.contains("Resume FC SD collection"), "{html}");
        let settings: Settings = serde_json::from_value(serde_json::json!({"ring_budget_mb": 1, "segment_mb": 1, "warn_free_mb": 1, "critical_free_mb": 1, "events": {}, "fc_sd_auto_collect": false})).unwrap();
        assert!(!settings.fc_sd_auto_collect && settings.fc_sd_leadup);
        assert_eq!(serde_json::to_value(&settings).unwrap()["fc_sd_auto_collect"], false, "round-trips to the station");
    }

    #[test]
    fn indicator_says_always_on_and_names_captures() {
        let status: super::super::RecordingStatusMsg = serde_json::from_value(serde_json::json!({
            "mode": "recording", "db_path": "x", "always_on": true,
            "captures": [{"kind": "launch", "title": "Launch", "label": "Launch", "event_start_ms": 1, "event_end_ms": null},
                         {"kind": "fill", "title": "Fill", "label": "Top-off", "event_start_ms": 1, "event_end_ms": 2}]
        }))
        .unwrap();
        let (on, text) = indicator_text(&status);
        assert!(on);
        assert_eq!(text, "Recording · always on · capturing Launch · saving Fill (Top-off)");
        let old: super::super::RecordingStatusMsg = serde_json::from_value(serde_json::json!({"mode": "idle", "db_path": null})).unwrap();
        assert!(!indicator_text(&old).0, "an older station without the ring still decodes");
    }

    #[test]
    fn labels_are_readable() {
        assert_eq!(size_label(64 * 1024 * 1024), "64 MB");
        assert_eq!(size_label(4096 * 1024 * 1024), "4.0 GB");
        assert_eq!(duration_label(90_000), "90 s");
        assert_eq!(duration_label(3_600_000), "60 min");
        assert_eq!(utc_label(1_800_000_000_000), "2027-01-15 08:00:00 UTC");
    }
}
