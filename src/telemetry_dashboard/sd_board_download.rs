//! Compact board SD download panel. Lists a board's SD card over the network,
//! downloads selected files (resumable, cancellable, bandwidth-bounded on the
//! station) and offers completed copies to the SD recovery import above.
use super::recording_download::{ExportOwner, saved_json, use_export_lifetime};
use crate::auth;
use dioxus::prelude::*;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub(super) struct RemoteFile {
    pub dir: String,
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub open: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub(super) struct Listing {
    pub board: String,
    pub listed_at_ms: u64,
    pub files: Vec<RemoteFile>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub(super) struct Job {
    pub id: u64,
    pub board: String,
    pub dir: String,
    pub name: String,
    pub phase: String,
    pub bytes_done: u64,
    pub total_bytes: u64,
    #[serde(default)]
    pub bytes_per_second: u64,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub(super) struct LocalFile {
    pub board: String,
    pub dir: String,
    pub name: String,
    pub bytes: u64,
    pub complete: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub(super) struct Status {
    #[serde(default)]
    pub boards: Vec<String>,
    #[serde(default)]
    pub allowed: bool,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub listings: Vec<Listing>,
    #[serde(default)]
    pub jobs: Vec<Job>,
    #[serde(default)]
    pub local: Vec<LocalFile>,
}

/// A completed station-side copy chosen as the SD import source.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct DownloadedSource {
    pub board: String,
    pub dir: String,
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum RowState {
    OnCard,
    Partial { have: u64 },
    Active { done: u64, total: u64, rate: u64 },
    Local,
}

pub(super) fn active_phase(phase: &str) -> bool {
    matches!(phase, "queued" | "connecting" | "transferring" | "cancelling")
}

pub(super) fn key(dir: &str, name: &str) -> String {
    format!("{dir}/{name}")
}

pub(super) fn row_state(status: &Status, board: &str, dir: &str, name: &str) -> RowState {
    if let Some(job) = status
        .jobs
        .iter()
        .find(|j| j.board == board && j.dir == dir && j.name == name && active_phase(&j.phase))
    {
        return RowState::Active { done: job.bytes_done, total: job.total_bytes, rate: job.bytes_per_second };
    }
    let local = status.local.iter().filter(|l| l.board == board && l.dir == dir && l.name == name);
    let mut partial = None;
    for file in local {
        if file.complete {
            return RowState::Local;
        }
        partial = Some(file.bytes);
    }
    partial.map_or(RowState::OnCard, |have| RowState::Partial { have })
}

/// Files shown for a board: the last card listing plus local-only copies.
pub(super) fn rows(status: &Status, board: &str) -> Vec<RemoteFile> {
    let mut files = status
        .listings
        .iter()
        .find(|l| l.board == board)
        .map(|l| l.files.clone())
        .unwrap_or_default();
    for local in status.local.iter().filter(|l| l.board == board) {
        if !files.iter().any(|f| f.dir == local.dir && f.name == local.name) {
            files.push(RemoteFile { dir: local.dir.clone(), name: local.name.clone(), size: local.bytes, open: false });
        }
    }
    files
}

pub(super) fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

pub(super) fn eta(remaining: u64, rate: u64) -> String {
    if rate == 0 {
        return "—".into();
    }
    let secs = remaining / rate;
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        _ => format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60),
    }
}

fn percent(done: u64, total: u64) -> f64 {
    if total == 0 { 0.0 } else { (done as f64 * 100.0 / total as f64).clamp(0.0, 100.0) }
}

/// Station URL for a path, like the firmware tab.
fn station_url(owner: &ExportOwner, path: &str) -> Result<String, String> {
    owner.check()?;
    let base = owner.base();
    if !base.trim().is_empty() {
        return Ok(format!("{}{path}", base.trim_end_matches('/')));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let window = web_sys::window().ok_or("no browser window".to_string())?;
        let origin = window.location().origin().map_err(|_| "failed to read browser origin".to_string())?;
        Ok(format!("{origin}{path}"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    Ok(format!("http://localhost:3000{path}"))
}

/// Owned JSON POST for the download commands (command permission required).
async fn post_json(path: &str, body: String, owner: Rc<ExportOwner>) -> Result<String, String> {
    owner.check_ordinary()?;
    if !auth::can_send_command("SdDownload") {
        return Err("Command permission is required.".into());
    }
    let url = station_url(&owner, path)?;
    #[cfg(not(target_arch = "wasm32"))]
    let (status, text) = {
        let client = auth::build_native_http_client(
            owner.skip_tls(),
            std::time::Duration::from_secs(10),
            std::time::Duration::from_secs(300),
        )?;
        let mut request = client.post(url).header("Content-Type", "application/json").body(body);
        if let Some(epoch) = owner.policy_epoch() {
            request = request.header(super::station_policy_context::HTTP_EPOCH_HEADER, epoch);
        }
        if let Some(origin) = owner.control_origin() {
            request = request.header("X-Station-Control-Origin", origin);
        }
        if let Some(token) = owner.token() {
            request = request.bearer_auth(token.to_owned());
        }
        let response = owner.wait(request.send()).await?.map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let text = owner.wait(response.text()).await?.map_err(|e| e.to_string())?;
        (status, text)
    };
    #[cfg(target_arch = "wasm32")]
    let (status, text) = {
        let controller = web_sys::AbortController::new().map_err(|_| "Request cancellation unavailable")?;
        let _cancel = super::HttpAbortOnDrop(controller.clone());
        let signal = controller.signal();
        let mut request = gloo_net::http::Request::post(&url)
            .abort_signal(Some(&signal))
            .header("Content-Type", "application/json");
        if let Some(epoch) = owner.policy_epoch() {
            request = request.header(super::station_policy_context::HTTP_EPOCH_HEADER, epoch);
        }
        if let Some(origin) = owner.control_origin() {
            request = request.header("X-Station-Control-Origin", origin);
        }
        if let Some(token) = owner.token() {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let request = request.body(body).map_err(|e| e.to_string())?;
        let response = owner.wait(request.send()).await?.map_err(|e| e.to_string())?;
        let status = response.status();
        let text = owner.wait(response.text()).await?.map_err(|e| e.to_string())?;
        (status, text)
    };
    owner.check()?;
    if !(200..300).contains(&status) {
        return Err(text.chars().take(200).collect::<String>());
    }
    Ok(text)
}

#[cfg(target_arch = "wasm32")]
async fn pause(ms: u32) {
    gloo_timers::future::TimeoutFuture::new(ms).await;
}
#[cfg(not(target_arch = "wasm32"))]
async fn pause(ms: u32) {
    tokio::time::sleep(std::time::Duration::from_millis(ms.into())).await;
}

const PANEL: &str = "border:1px solid #334155;border-radius:10px;padding:10px 12px;margin:10px 0;display:flex;flex-direction:column;gap:8px;";
const ROW: &str = "display:grid;grid-template-columns:22px minmax(0,1fr) auto auto;gap:8px;align-items:center;padding:5px 6px;border-radius:6px;font-size:13px;";
const CHIP: &str = "font-size:11px;padding:2px 8px;border-radius:999px;white-space:nowrap;";
const BAR: &str = "height:6px;border-radius:3px;background:#1e293b;overflow:hidden;";

#[component]
pub(super) fn SdBoardDownload(busy: bool, onselect: EventHandler<DownloadedSource>) -> Element {
    let mut status = use_signal(Status::default);
    let mut board = use_signal(|| "DAQ".to_string());
    let mut selected = use_signal(BTreeSet::<String>::new);
    let mut queue = use_signal(Vec::<(String, String)>::new);
    let mut message = use_signal(String::new);
    let mut listing = use_signal(|| false);
    let mut refresh = use_signal(|| 0u64);
    let lifetime = use_export_lifetime();
    let can_command = auth::can_send_command("SdDownload");

    // Status poll: 1 s while a job runs or a queue waits, otherwise 5 s.
    let poll_life = lifetime.clone();
    use_future(move || {
        let alive = poll_life.clone();
        async move {
            loop {
                let _ = refresh();
                let probe = alive.clone();
                if let Ok(owner) = ExportOwner::capture_selected(move || probe.load(Ordering::Acquire))
                    && let Ok(next) = saved_json::<Status>("/api/sd_download", owner.clone()).await
                    && owner.is_current()
                {
                    let running = next.jobs.iter().any(|j| active_phase(&j.phase));
                    status.set(next);
                    // Start the next queued file once nothing is running.
                    if !running && let Some((dir, name)) = queue.with_mut(|q| (!q.is_empty()).then(|| q.remove(0))) {
                        let probe = alive.clone();
                        if let Ok(owner) = ExportOwner::capture_ordinary_selected(move || probe.load(Ordering::Acquire)) {
                            let body = serde_json::json!({"dir":dir,"name":name}).to_string();
                            let path = format!("/api/sd_download/{}/start", board.peek().to_ascii_lowercase());
                            if let Err(error) = post_json(&path, body, owner).await {
                                message.set(error);
                                queue.set(Vec::new());
                            }
                        }
                        continue;
                    }
                }
                if !alive.load(Ordering::Acquire) {
                    return;
                }
                let fast = status.peek().jobs.iter().any(|j| active_phase(&j.phase)) || !queue.peek().is_empty();
                pause(if fast { 1000 } else { 5000 }).await;
            }
        }
    });

    let snapshot = status();
    let current_board = board();
    let files = rows(&snapshot, &current_board);
    let active = snapshot.jobs.iter().find(|j| active_phase(&j.phase)).cloned();
    let last = snapshot
        .jobs
        .iter()
        .find(|j| j.board == current_board && !active_phase(&j.phase))
        .cloned();
    let chosen = selected();
    let selected_bytes: u64 = files.iter().filter(|f| chosen.contains(&key(&f.dir, &f.name))).map(|f| f.size).sum();
    let allowed = snapshot.allowed;
    let gate_text = snapshot.blocked_reason.clone().unwrap_or_default();
    let list_life = lifetime.clone();
    let start_life = lifetime.clone();
    let cancel_life = lifetime.clone();

    rsx! {
        section { style: PANEL, aria_label: "Board SD download",
            div { style: "display:flex;align-items:center;gap:8px;flex-wrap:wrap;",
                strong { style: "font-size:14px;", "Board SD download" }
                span {
                    style: if allowed { format!("{CHIP}background:#064e3b;color:#a7f3d0;") } else { format!("{CHIP}background:#422006;color:#fde68a;") },
                    title: "{gate_text}",
                    if allowed { "ready" } else { "blocked" }
                }
                div { style: "flex:1;" }
                if snapshot.boards.len() > 1 {
                    select { value: "{current_board}", onchange: move |e| { board.set(e.value()); selected.set(BTreeSet::new()); },
                        for b in snapshot.boards.iter() { option { value: "{b}", "{b}" } }
                    }
                } else {
                    span { style: "font-size:12px;opacity:.7;", "{current_board}" }
                }
                button {
                    disabled: !can_command || !allowed || listing() || active.is_some() || busy,
                    title: "List files on the board's SD card",
                    onclick: move |_| {
                        let alive = list_life.clone();
                        let owner = match ExportOwner::capture_ordinary_selected(move || alive.load(Ordering::Acquire)) {
                            Ok(owner) => owner, Err(error) => { message.set(error); return; }
                        };
                        let path = format!("/api/sd_download/{}/list", board().to_ascii_lowercase());
                        listing.set(true);
                        message.set(String::new());
                        spawn(async move {
                            let result = post_json(&path, String::new(), owner.clone()).await;
                            if owner.is_current() {
                                if let Err(error) = result { message.set(error); }
                                listing.set(false);
                                refresh += 1;
                            }
                        });
                    },
                    if listing() { "Listing…" } else { "↻ List" }
                }
            }
            if !allowed && !gate_text.is_empty() {
                div { style: "font-size:12px;color:#fbbf24;", "{gate_text}" }
            }
            if let Some(job) = active.clone() {
                div { style: "display:flex;flex-direction:column;gap:4px;",
                    div { style: "display:flex;gap:8px;font-size:12px;align-items:center;",
                        span { style: "overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1;", "{job.name}" }
                        span { "{human_bytes(job.bytes_done)} / {human_bytes(job.total_bytes)}" }
                        span { style: "opacity:.7;", "{human_bytes(job.bytes_per_second)}/s · {eta(job.total_bytes.saturating_sub(job.bytes_done), job.bytes_per_second)}" }
                        button {
                            disabled: !can_command || job.phase == "cancelling",
                            onclick: {
                                let id = job.id;
                                move |_| {
                                    queue.set(Vec::new());
                                    let alive = cancel_life.clone();
                                    let owner = match ExportOwner::capture_ordinary_selected(move || alive.load(Ordering::Acquire)) {
                                        Ok(owner) => owner, Err(error) => { message.set(error); return; }
                                    };
                                    spawn(async move {
                                        let result = post_json(&format!("/api/sd_download/jobs/{id}/cancel"), String::new(), owner.clone()).await;
                                        if owner.is_current() { if let Err(error) = result { message.set(error); } refresh += 1; }
                                    });
                                }
                            },
                            "Cancel"
                        }
                    }
                    div { style: BAR,
                        div { style: "height:100%;width:{percent(job.bytes_done, job.total_bytes):.1}%;background:#38bdf8;transition:width .4s;" }
                    }
                }
            }
            if files.is_empty() {
                div { style: "font-size:12px;opacity:.7;", "No file list yet. Use ↻ List while the board is in Idle, Landed or Recovery." }
            } else {
                div { style: "display:flex;flex-direction:column;max-height:260px;overflow:auto;",
                    for file in files.iter().cloned() {
                        {
                            let k = key(&file.dir, &file.name);
                            let state = row_state(&snapshot, &current_board, &file.dir, &file.name);
                            let checked = chosen.contains(&k);
                            let toggle_key = k.clone();
                            let source = DownloadedSource { board: current_board.clone(), dir: file.dir.clone(), name: file.name.clone(), bytes: file.size };
                            rsx! {
                                div { key: "{k}", style: if checked { format!("{ROW}background:#0f172a;") } else { ROW.to_string() },
                                    input { r#type: "checkbox", checked, disabled: matches!(state, RowState::Active { .. }) || (state == RowState::Local && !file.open),
                                        onchange: move |_| selected.with_mut(|s| { if !s.remove(&toggle_key) { s.insert(toggle_key.clone()); } }) }
                                    span { style: "overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", title: "{k}",
                                        span { style: "opacity:.55;", "{file.dir}/" }
                                        "{file.name}"
                                        if file.open { span { style: "{CHIP}margin-left:6px;background:#1e3a8a;color:#bfdbfe;", title: "Still being written; only the flushed part is downloaded", "live" } }
                                    }
                                    span { style: "font-size:12px;opacity:.8;", "{human_bytes(file.size)}" }
                                    match state {
                                        RowState::Local => rsx! {
                                            button { disabled: busy, title: "Use this downloaded file as the SD import source", onclick: move |_| onselect.call(source.clone()), "Use" }
                                        },
                                        RowState::Partial { have } => rsx! { span { style: "{CHIP}background:#3f3f46;color:#e4e4e7;", title: "Partial copy; downloading again resumes", "{percent(have, file.size):.0}%" } },
                                        RowState::Active { done, total, .. } => rsx! { span { style: "{CHIP}background:#0c4a6e;color:#bae6fd;", "{percent(done, total):.0}%" } },
                                        RowState::OnCard => rsx! { span { style: "{CHIP}background:#1f2937;color:#9ca3af;", "card" } },
                                    }
                                }
                            }
                        }
                    }
                }
                div { style: "display:flex;align-items:center;gap:8px;font-size:12px;",
                    span { style: "opacity:.75;flex:1;",
                        if chosen.is_empty() { "Select files to download" } else { "{chosen.len()} selected · {human_bytes(selected_bytes)}" }
                    }
                    if !queue().is_empty() { span { style: "opacity:.75;", "{queue().len()} queued" } }
                    button {
                        disabled: !can_command || !allowed || chosen.is_empty() || busy,
                        onclick: move |_| {
                            let picks = files_for_queue(&status.peek(), &board.peek(), &selected.peek());
                            if picks.is_empty() { return; }
                            selected.set(BTreeSet::new());
                            queue.with_mut(|q| q.extend(picks));
                            let _ = &start_life;
                            refresh += 1;
                        },
                        "Download"
                    }
                }
            }
            if let Some(job) = last.filter(|j| matches!(j.phase.as_str(), "paused" | "failed" | "cancelled")) {
                div { style: "display:flex;gap:8px;align-items:center;font-size:12px;",
                    span { style: "flex:1;opacity:.8;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", title: "{job.message}", "{job.name}: {job.phase}" }
                    button {
                        disabled: !can_command || !allowed || active.is_some() || busy,
                        onclick: { let item = (job.dir.clone(), job.name.clone()); move |_| { queue.with_mut(|q| q.push(item.clone())); refresh += 1; } },
                        "Resume"
                    }
                }
            }
            if !message().is_empty() { div { role: "status", style: "font-size:12px;color:#fca5a5;", "{message}" } }
        }
    }
}

/// Selected rows that still need data, in display order.
pub(super) fn files_for_queue(status: &Status, board: &str, selected: &BTreeSet<String>) -> Vec<(String, String)> {
    rows(status, board)
        .into_iter()
        .filter(|f| selected.contains(&key(&f.dir, &f.name)))
        .filter(|f| match row_state(status, board, &f.dir, &f.name) {
            RowState::OnCard | RowState::Partial { .. } => true,
            // A live file's copy can be topped up; the station resumes it.
            RowState::Local => f.open,
            RowState::Active { .. } => false,
        })
        .map(|f| (f.dir, f.name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> Status {
        serde_json::from_value(serde_json::json!({
            "boards":["DAQ"],"allowed":true,"blocked_reason":null,
            "listings":[{"board":"DAQ","listed_at_ms":1,"files":[
                {"dir":"LAUNCH","name":"L0.CSV","size":4096,"open":false},
                {"dir":"LAUNCH","name":"L1.CSV","size":2048,"open":false},
                {"dir":"UTC","name":"S.CSV","size":9000,"open":true},
                {"dir":"UTC","name":"T.CSV","size":100,"open":true}]}],
            "jobs":[{"id":2,"board":"DAQ","dir":"UTC","name":"S.CSV","phase":"transferring","bytes_done":4500,"total_bytes":9000,"bytes_per_second":1500,"message":""},
                    {"id":1,"board":"DAQ","dir":"LAUNCH","name":"L1.CSV","phase":"paused","bytes_done":1024,"total_bytes":2048,"message":"paused"}],
            "local":[{"board":"DAQ","dir":"LAUNCH","name":"L0.CSV","bytes":4096,"complete":true},
                     {"board":"DAQ","dir":"LAUNCH","name":"L1.CSV","bytes":1024,"complete":false},
                     {"board":"DAQ","dir":"NO_UTC","name":"OLD.CSV","bytes":77,"complete":true},
                     {"board":"DAQ","dir":"UTC","name":"T.CSV","bytes":100,"complete":true}]
        }))
        .unwrap()
    }

    #[test]
    fn row_states_follow_jobs_then_local_copies() {
        let s = status();
        assert_eq!(row_state(&s, "DAQ", "LAUNCH", "L0.CSV"), RowState::Local);
        assert_eq!(row_state(&s, "DAQ", "LAUNCH", "L1.CSV"), RowState::Partial { have: 1024 });
        assert_eq!(row_state(&s, "DAQ", "UTC", "S.CSV"), RowState::Active { done: 4500, total: 9000, rate: 1500 });
        assert_eq!(row_state(&s, "DAQ", "UTC", "T.CSV"), RowState::Local);
    }

    #[test]
    fn rows_include_local_only_copies_and_queue_skips_done_or_running() {
        let s = status();
        let names = rows(&s, "DAQ").into_iter().map(|f| f.name).collect::<Vec<_>>();
        assert_eq!(names, ["L0.CSV", "L1.CSV", "S.CSV", "T.CSV", "OLD.CSV"]);
        let all = names.iter().zip(rows(&s, "DAQ")).map(|(_, f)| key(&f.dir, &f.name)).collect::<BTreeSet<_>>();
        assert_eq!(
            files_for_queue(&s, "DAQ", &all),
            vec![("LAUNCH".to_string(), "L1.CSV".to_string()), ("UTC".to_string(), "T.CSV".to_string())]
        );
    }

    #[test]
    fn compact_units_and_eta() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(14_200_000), "14.2 MB");
        assert_eq!(human_bytes(20_000_000_000), "20.0 GB");
        assert_eq!(eta(100, 0), "—");
        assert_eq!(eta(30_000, 1000), "30s");
        assert_eq!(eta(7_200_000, 1000), "2h00m");
        assert!(active_phase("cancelling") && !active_phase("paused") && !active_phase("complete"));
    }
}
