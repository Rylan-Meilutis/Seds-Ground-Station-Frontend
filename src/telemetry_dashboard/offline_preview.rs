//! The entry point for demo mode and replay files: the live dashboard
//! connected to a station on this device.
//!
//! This route builds a [`LocalStation`] (the demo scenario, or a replay file
//! or bundle the user opens), selects it as the dashboard's station
//! ([`station_link::LocalSelection`]) and mounts the same
//! [`TelemetryDashboard`] a Ground Station gets. Every page, card and control
//! is the live one; the local station answers the station's routes and
//! pushes its frames. Above the dashboard a slim bar says what is playing
//! and offers playback controls, opening another file and leaving.
//!
//! Leaving the route drops the selection, the station, its rows and the
//! scratch preferences: a connected station pays nothing for it.
use super::{
    TelemetryDashboard,
    layout::ThemeConfig,
    local_station::{self, LocalStation, demo, replay_file},
    station_link,
};
use dioxus::html::HasFileData;
use dioxus::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    static PENDING_BUNDLE: std::cell::RefCell<Option<(String, Vec<u8>)>> = const { std::cell::RefCell::new(None) };
}

/// A bundle to open when this route mounts (a flight plan's predicted
/// replay, handed over from the dashboard).
pub(crate) fn queue_bundle(name: String, bytes: Vec<u8>) {
    PENDING_BUNDLE.with(|slot| *slot.borrow_mut() = Some((name, bytes)));
}

pub(crate) fn take_pending_bundle() -> Option<(String, Vec<u8>)> {
    PENDING_BUNDLE.with(|slot| slot.borrow_mut().take())
}

/// Playback speeds offered for replays and the demo.
const SPEEDS: [u32; 5] = [1, 2, 5, 10, 50];

/// A replay bundle (ZIP) or a portable JSON replay file.
pub(crate) fn open_bytes(name: &str, bytes: &[u8]) -> Result<local_station::Session, String> {
    if local_station::bundle::is_bundle(bytes) {
        local_station::bundle::open(name, bytes)
    } else {
        if bytes.len() > replay_file::MAX_BYTES {
            return Err("Replay exceeds 64 MiB.".into());
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "The file is neither a .gsreplay bundle nor UTF-8 JSON.".to_string())?;
        replay_file::open(name, text)
    }
}

async fn sleep_ms(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
}

/// Time into the session as the bar shows it beside the T clock (`mm:ss.s`).
fn position_text(position: i64, start: i64) -> String {
    let d = (position - start).max(0);
    let secs = d / 1000;
    format!("{:02}:{:02}.{}", secs / 60, secs % 60, (d % 1000) / 100)
}

fn new_station(session: local_station::Session) -> Rc<LocalStation> {
    Rc::new(LocalStation::new(session, super::current_wallclock_ms()))
}

/// What the route opens on: the file it was handed or, with none, the demo.
/// A file that does not open is reported and nothing is opened in its place:
/// opening a replay never falls back to demo mode.
pub(crate) fn starting_session(
    replay: Option<&(String, String)>,
    bundle: Option<&(String, Vec<u8>)>,
    opened: Option<Result<(String, Vec<u8>), String>>,
) -> Result<local_station::Session, String> {
    if let Some((name, text)) = replay {
        return replay_file::open(name, text).map_err(|error| format!("Could not open {name}: {error}"));
    }
    if let Some((name, bytes)) = bundle {
        return local_station::bundle::open(name, bytes).map_err(|error| format!("Could not open {name}: {error}"));
    }
    match opened {
        Some(Ok((name, bytes))) => open_bytes(&name, &bytes).map_err(|error| format!("Could not open {name}: {error}")),
        Some(Err(message)) => Err(message),
        None => Ok(demo::session(0)),
    }
}

/// The waiting opened file (another app, or the connection page's **Open
/// replay file**), or why it could not be read. The location of a desktop
/// file is remembered once it opens.
fn take_opened() -> Option<(Result<(String, Vec<u8>), String>, Option<std::path::PathBuf>)> {
    if let Some(message) = crate::opened_file::take_rejection() {
        return Some((Err(message), None));
    }
    crate::opened_file::take_opened().map(|opened| (Ok((opened.name, opened.bytes)), opened.path))
}

/// Adds a replay that opened from a desktop path to the connection page's
/// recent list.
#[cfg_attr(any(target_arch = "wasm32", target_os = "android", target_os = "ios"), allow(unused_variables))]
fn remember(name: &str, path: Option<&std::path::Path>) {
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    if let Some(path) = path {
        crate::recent_replays::record(name, path);
    }
}

/// The banner's headline for a session.
pub(crate) fn banner_title(session: &local_station::Session) -> String {
    match (&session.source, &session.predicted) {
        (local_station::Source::Demo, _) => "Demo mode — no station connected".to_string(),
        (local_station::Source::Replay { .. }, Some(label)) => format!("Predicted replay — {label}"),
        (local_station::Source::Replay { file_name }, None) => format!("Replay: {file_name}"),
    }
}

/// Holds the selection for as long as this route is mounted.
struct Selected(RefCell<Option<station_link::LocalSelection>>);

#[component]
pub fn OfflinePreview(
    on_exit: EventHandler<()>,
    theme: ThemeConfig,
    /// Dashboard tab to open on (a main tab id such as "vehicle").
    #[props(default)]
    page: Option<String>,
    /// A replay file to open instead of the demo: (file name, contents).
    #[props(default)]
    replay: Option<(String, String)>,
    /// A replay bundle (`.gsreplay`) to open instead of the demo.
    #[props(default)]
    bundle: Option<(String, Vec<u8>)>,
    /// The leave button's label (where it goes).
    #[props(default = "Exit".to_string())]
    exit_label: String,
) -> Element {
    // The station is built on entry and replaced when a file is opened. A
    // file handed over before this route mounted opens directly; the demo is
    // built only when there is no file at all.
    // Held only until the signals below take it: a replaced station must not
    // stay alive for the life of the route.
    let initial = use_hook(|| Rc::new(RefCell::new(Some({
        let (opened, path) = match (&replay, &bundle) {
            (None, None) => take_opened().map_or((None, None), |(opened, path)| (Some(opened), path)),
            _ => (None, None),
        };
        match starting_session(replay.as_ref(), bundle.as_ref(), opened) {
            Ok(session) => {
                if let local_station::Source::Replay { file_name } = &session.source {
                    remember(file_name, path.as_deref());
                }
                (Some(new_station(session)), String::new())
            }
            Err(message) => (None, message),
        }
    }))));
    let mut file_status = use_signal(|| initial.borrow().as_ref().map(|(_, message)| message.clone()).unwrap_or_default());
    let mut station = use_signal(|| initial.borrow_mut().take().and_then(|(station, _)| station));
    // Demo mode is offered as a way back only to someone who entered it.
    let entered_demo = use_hook(|| station.peek().as_ref().is_some_and(|s| s.session.is_demo()));
    let selected = use_hook(|| Rc::new(Selected(RefCell::new(None))));
    {
        let selected = selected.clone();
        use_drop(move || {
            selected.0.borrow_mut().take();
            // A real station's dashboard starts from its own data, not this one's.
            super::reset_dashboard_for_new_station();
        });
    }
    // Each (re)start selects the current station, clears what the previous
    // one showed, reads the local station's session (its permissions and
    // name) and then mounts a fresh dashboard.
    let mut ready = use_signal(|| 0u64);
    let mut generation = use_signal(|| 1u64);
    use_effect(move || {
        let generation = generation();
        let Some(current) = station.peek().clone() else { return };
        {
            let mut slot = selected.0.borrow_mut();
            if !station_link::local().is_some_and(|s| Rc::ptr_eq(&s, &current)) {
                slot.take();
                *slot = Some(station_link::LocalSelection::install(current));
            }
        }
        super::reset_dashboard_for_new_station();
        if let Some(page) = page.as_deref() {
            // Written to the local station's scratch preferences, never the station's.
            super::persist::set_string(&super::scoped_main_tab_key(), page);
        }
        spawn(async move {
            let _ = crate::auth::fetch_session_status(station_link::LOCAL_BASE, false).await;
            ready.set(generation);
        });
    });
    let mut playback = use_signal(|| station.peek().as_ref().map(|s| s.playback(super::current_wallclock_ms())));
    use_future(move || async move {
        loop {
            // The header's T clock moves on every local-station tick (100 ms); the bar keeps pace.
            sleep_ms(100).await;
            let next = station.peek().as_ref().map(|s| s.playback(super::current_wallclock_ms()));
            if *playback.peek() != next {
                playback.set(next);
            }
        }
    });
    let mut replace = move |session: local_station::Session| {
        let current = new_station(session);
        playback.set(Some(current.playback(super::current_wallclock_ms())));
        station.set(Some(current));
        generation.with_mut(|g| *g += 1);
        file_status.set(String::new());
    };
    let open_file = move |file: dioxus::html::FileData| {
        spawn(async move {
            file_status.set(format!("Opening {}…", file.name()));
            let result = async {
                if file.size() as usize > crate::opened_file::MAX_BYTES {
                    return Err("The file is larger than 192 MiB.".to_string());
                }
                let bytes = file.read_bytes().await.map_err(|e| e.to_string())?;
                open_bytes(&file.name(), &bytes)
            }
            .await;
            match result {
                Ok(session) => {
                    remember(&file.name(), picked_path(&file).as_deref());
                    replace(session)
                }
                Err(error) => file_status.set(format!("Could not open {}: {error}", file.name())),
            }
        });
    };

    // A replay opened from another app (crate::opened_file), or from the
    // connection page, whenever another arrives while this view is open.
    use_effect(move || {
        let _revision = crate::opened_file::OPENED_FILE_REVISION();
        match take_opened() {
            Some((Err(message), _)) => file_status.set(message),
            Some((Ok((name, bytes)), path)) => match open_bytes(&name, &bytes) {
                Ok(session) => {
                    remember(&name, path.as_deref());
                    replace(session)
                }
                Err(error) => file_status.set(format!("Could not open {name}: {error}")),
            },
            None => {}
        }
    });

    let shell_style = format!(
        "--gs26-local-bar-bg:{};--gs26-local-bar-border:{};--gs26-local-bar-text:{};",
        theme.panel_background, theme.border, theme.text_primary
    );
    let file_button = rsx! {
        label { class: "gs26-local-file-button",
            // No `accept` filter: phone pickers cannot match the .gsreplay extension, and the content is checked on open.
            input { r#type: "file", "aria-label": "Open replay file",
                onchange: move |e| { if let Some(file) = e.files().into_iter().next() { open_file(file); } } }
            "Open replay file"
        }
    };
    let exit_button = rsx! { button { r#type: "button", onclick: move |_| on_exit.call(()), "{exit_label}" } };

    // The handed-over file did not open: say why, offer another file and the
    // way back. Nothing runs and no demo is started.
    let (Some(current), Some(p)) = (station.read().clone(), playback()) else {
        return rsx! {
            style {{include_str!("offline_preview.css")}}
            div { class: "gs26-local-station", style: shell_style, "data-source": "none",
                ondragover: move |e| e.prevent_default(),
                ondrop: move |e| { e.prevent_default(); if let Some(file) = e.files().into_iter().next() { open_file(file); } },
                header { class: "gs26-local-bar", role: "banner", "aria-label": "Local station",
                    div { class: "gs26-local-bar-title",
                        strong { "No replay open" }
                        span { "Open a replay file, or go back." }
                    }
                    div { class: "gs26-local-bar-actions", {file_button} {exit_button} }
                }
                if !file_status().is_empty() { p { class: "gs-notice gs-notice--warn gs26-local-file-status", role: "status", "{file_status}" } }
            }
        };
    };
    let demo_mode = current.session.is_demo();
    let title = banner_title(&current.session);
    let detail = if demo_mode {
        "Example data. Controls answer here; nothing is sent.".to_string()
    } else {
        format!("{} · {} · read-only", current.session.mode.label(), current.session.provenance)
    };
    let span = (p.end - p.start).max(1);
    let station_for_play = current.clone();
    let station_for_speed = current.clone();
    let station_for_seek = current.clone();
    let key = format!("local-{}", generation());

    rsx! {
        style {{include_str!("offline_preview.css")}}
        div { class: "gs26-local-station", style: shell_style, "data-source": if demo_mode { "demo" } else { "replay" },
            ondragover: move |e| e.prevent_default(),
            ondrop: move |e| { e.prevent_default(); if let Some(file) = e.files().into_iter().next() { open_file(file); } },
            header { class: "gs26-local-bar", role: "banner", "aria-label": "Local station",
                div { class: "gs26-local-bar-title",
                    strong { "{title}" }
                    span { "{detail}" }
                }
                div { class: "gs26-local-bar-transport", role: "toolbar", "aria-label": "Playback",
                    button { r#type: "button", "aria-label": if p.paused { "Play" } else { "Pause" },
                        onclick: move |_| {
                            let now = super::current_wallclock_ms();
                            let at = station_for_play.playback(now);
                            if at.paused && !station_for_play.session.is_demo() && at.position >= at.end {
                                station_for_play.seek(at.start, now);
                                generation.with_mut(|g| *g += 1);
                            }
                            station_for_play.set_speed(at.speed, !at.paused, now);
                        },
                        if p.paused { "▶" } else { "❚❚" }
                    }
                    // The station's own T clock (the header shows the same), then the position.
                    span { class: "gs26-local-bar-clock", "data-testid": "local-clock", "{current.tclock_text(p.position)}" }
                    span { class: "gs26-local-bar-position", "data-testid": "local-position", "{position_text(p.position, p.start)}" }
                    span { class: "gs-badge gs-badge--info", "data-testid": "preview-state", "{super::types::display_flight_state(&p.state)}" }
                    input { r#type: "range", "aria-label": "Position", min: "0", max: "{span}", step: "250", value: "{p.position - p.start}",
                        onchange: move |e| {
                            if let Ok(v) = e.value().parse::<i64>() {
                                let now = super::current_wallclock_ms();
                                station_for_seek.seek(station_for_seek.playback(now).start + v, now);
                                generation.with_mut(|g| *g += 1);
                            }
                        }
                    }
                    select { "aria-label": "Speed", value: "{p.speed as u32}",
                        onchange: move |e| {
                            let now = super::current_wallclock_ms();
                            let paused = station_for_speed.playback(now).paused;
                            station_for_speed.set_speed(e.value().parse().unwrap_or(1.0), paused, now);
                        },
                        for x in SPEEDS { option { value: "{x}", "{x}×" } }
                    }
                }
                div { class: "gs26-local-bar-actions",
                    {file_button}
                    if !demo_mode && entered_demo {
                        button { r#type: "button", onclick: move |_| replace(demo::session(0)), "Back to demo" }
                    }
                    {exit_button}
                }
            }
            if !file_status().is_empty() { p { class: "gs-notice gs-notice--warn gs26-local-file-status", role: "status", "{file_status}" } }
            div { class: "gs26-local-dashboard",
                if ready() == generation() {
                    TelemetryDashboard { key: "{key}" }
                }
            }
        }
    }
}

/// Where a picked or dropped file lives, on a desktop (the recent list).
pub(crate) fn picked_path(_file: &dioxus::html::FileData) -> Option<std::path::PathBuf> {
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    {
        let path = _file.path();
        if path.is_absolute() && path.is_file() {
            return std::fs::canonicalize(path).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests;
