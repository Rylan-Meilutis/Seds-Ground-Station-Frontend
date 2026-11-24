//! The offline workspace: demo mode and replay files with no station
//! connected. It renders a [`local_station::Session`] read through the same
//! routes and JSON a Ground Station serves (`Session::get`), so pages are
//! driven by the session's layout (`GET /api/layout` `state_tab`, `data_tab`,
//! `actions_tab`), board list and samples, never by vehicle names in code.
//!
//! It never mounts the live dashboard, creates a station transport, writes
//! station state or sends a command: commands get a local acknowledgement
//! (demo) or a read-only notice (replay). The session, its clock task and
//! everything it allocated are dropped when this component unmounts, so a
//! connected station pays nothing for it.
use super::{
    layout::ThemeConfig,
    live_diagrams::{self, derive, topology},
    local_station::{self, Channel, Mode, Session, demo, replay_file},
    navigation_rail::{NavigationItem, NavigationRail},
};
use dioxus::html::HasFileData;
use dioxus::prelude::*;
use serde_json::Value;
use std::rc::Rc;

const PAGES: &[(&str, &str)] = &[
    ("state", "Dashboard"),
    ("mission", "Mission"),
    ("map", "Map"),
    ("vehicle", "Replay & summary"),
    ("checklists", "Checklists"),
    ("data", "Data"),
    ("detailed", "Diagrams"),
    ("network-topology", "Network"),
    ("crew-voice", "Voice Chat"),
    ("stream-manager", "Stream Control"),
    ("calibration", "Calibration"),
    ("data-export", "Recordings"),
];
const CHECKS: &[&str] = &[
    "Pressure transducer reading checked",
    "Gas lines and connections inspected",
    "Communications and abort procedure checked",
];
const TICK_MS: u32 = 250;
/// The loss-of-signal prediction runs on the station.
const OFFLINE_LOS_REASON: &str = "Signal-loss simulation needs a connected station: the prediction runs on the station. Open this recording from the 3D replay tab of a connected dashboard.";

fn known_page(id: &str) -> &str {
    if id == "settings" || PAGES.iter().any(|(key, _)| *key == id) { id } else { "state" }
}
fn title(id: &str) -> &str {
    PAGES.iter().find_map(|(key, label)| (*key == id).then_some(*label)).unwrap_or("Settings")
}

async fn sleep_ms(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
}

/// Demo clock as the mission header shows it: T- before ignition, T+ after.
fn mission_clock(session: &Session, at: i64) -> String {
    let launch = session.states.iter().find(|(_, s)| s == "Launch").map(|(t, _)| *t + if session.is_demo() { 10_000 } else { 0 });
    let Some(launch) = launch else {
        return format!("{:.1} s", (at - session.start_ms()) as f64 / 1000.0);
    };
    super::tclock::format_tclock_ms(at.saturating_sub(launch), 1)
}

fn format_item(value: f32, formatter: &Value) -> String {
    if !value.is_finite() {
        return "—".into();
    }
    let prefix = formatter["prefix"].as_str().unwrap_or("");
    let suffix = formatter["suffix"].as_str().unwrap_or("");
    let body = if formatter["kind"] == "integer" {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.prec$}", prec = formatter["precision"].as_u64().unwrap_or(2) as usize)
    };
    format!("{prefix}{body}{suffix}")
}

thread_local! {
    /// A bundle handed over by the live dashboard ("Play in replay" on a
    /// flight plan), taken once by the `/preview` route.
    static PENDING_BUNDLE: std::cell::RefCell<Option<(String, Vec<u8>)>> = const { std::cell::RefCell::new(None) };
}

/// Queue a replay bundle for the next `/preview` mount.
pub(crate) fn queue_bundle(name: String, bytes: Vec<u8>) {
    PENDING_BUNDLE.with(|slot| *slot.borrow_mut() = Some((name, bytes)));
}

/// The queued bundle, once.
pub(crate) fn take_pending_bundle() -> Option<(String, Vec<u8>)> {
    PENDING_BUNDLE.with(|slot| slot.borrow_mut().take())
}

/// A replay bundle (ZIP) or a portable JSON replay file.
fn open_bytes(name: &str, bytes: &[u8]) -> Result<Session, String> {
    if local_station::bundle::is_bundle(bytes) {
        local_station::bundle::open(name, bytes)
    } else {
        let text = std::str::from_utf8(bytes).map_err(|_| "The file is neither a .gsreplay bundle nor UTF-8 JSON.".to_string())?;
        replay_file::open(name, text)
    }
}

/// Saves bytes as a download (web) or into Downloads/Documents (native).
fn save_file(name: &str, bytes: &[u8], media_type: &str) -> Result<String, String> {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let array = js_sys::Uint8Array::from(bytes);
        let parts = js_sys::Array::of1(&array);
        let _ = media_type;
        let blob = web_sys::Blob::new_with_u8_array_sequence(&parts).map_err(|_| "Cannot create the file")?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|_| "Cannot create the download")?;
        let document = web_sys::window().and_then(|w| w.document()).ok_or("No document")?;
        let link: web_sys::HtmlAnchorElement = document.create_element("a").map_err(|_| "Cannot create the download")?.dyn_into().map_err(|_| "Cannot create the download")?;
        link.set_href(&url);
        link.set_download(name);
        link.click();
        gloo_timers::callback::Timeout::new(60_000, move || { let _ = web_sys::Url::revoke_object_url(&url); }).forget();
        Ok(format!("{name} downloaded."))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = media_type;
        let directory = dirs::download_dir().or_else(dirs::document_dir).ok_or("No Downloads or Documents directory")?;
        let path = directory.join(name);
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok(format!("Saved {}", path.display()))
    }
}

fn text_of(value: &Value) -> String {
    value.as_str().unwrap_or("").to_string()
}

fn age_text(age_ms: i64) -> String {
    if age_ms < 1_000 { format!("{age_ms} ms ago") } else { format!("{:.1} s ago", age_ms as f64 / 1000.0) }
}

/// The widgets of a dashboard section worth drawing. A replay omits
/// widgets for data types the file never recorded (they could only ever say
/// "no data"); the demo and a live station show every widget.
fn shown_widgets(session: &Session, widgets: &Value) -> Vec<Value> {
    widgets
        .as_array()
        .into_iter()
        .flatten()
        .filter(|w| {
            if session.is_demo() {
                return true;
            }
            let types: Vec<&str> = match w["kind"].as_str() {
                Some("summary" | "valve_state") => w["data_type"].as_str().into_iter().collect(),
                Some("chart") => w["chart_series"].as_array().into_iter().flatten().filter_map(|c| c["data_type"].as_str()).collect(),
                Some("actions") => return false,
                _ => return true,
            };
            types.iter().any(|t| session.has_type(t))
        })
        .cloned()
        .collect()
}

/// The `state_tab` sections the layout shows in `state` (first entry when
/// none names it).
fn sections_for(layout: &Value, state: &str) -> Vec<Value> {
    let states = layout["state_tab"]["states"].as_array().cloned().unwrap_or_default();
    states
        .iter()
        .find(|s| s["states"].as_array().is_some_and(|a| a.iter().any(|x| x == state)))
        .or_else(|| states.first())
        .and_then(|s| s["sections"].as_array().cloned())
        .unwrap_or_default()
}

struct Lane {
    label: String,
    current: String,
    points: Vec<(i64, f32)>,
}

/// Stacked lanes, one per series, each on its own scale. Labels are HTML so
/// they keep their size at any width; only the traces are scaled.
fn lane_chart(aria: String, lanes: Vec<Lane>, from: i64, to: i64, cursor: Option<i64>) -> Element {
    const W: f64 = 1000.0;
    const H: f64 = 100.0;
    let span = (to - from).max(1) as f64;
    let cursor_x = cursor.map(|c| ((c - from) as f64 / span * W).clamp(0.0, W));
    let lanes: Vec<(String, String, String, String)> = lanes
        .into_iter()
        .map(|lane| {
            let (mut lo, mut hi) = lane.points.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
            if !lo.is_finite() {
                (lo, hi) = (0.0, 1.0);
            }
            let range = if lane.points.is_empty() { "no samples".to_string() } else { format!("{lo:.1} … {hi:.1}") };
            if (hi - lo).abs() < 1e-6 {
                lo -= 1.0;
                hi += 1.0;
            }
            let path = lane
                .points
                .iter()
                .enumerate()
                .map(|(k, (t, v))| {
                    let x = ((*t - from) as f64 / span * W).clamp(0.0, W);
                    let y = 6.0 + (1.0 - ((v - lo) / (hi - lo)) as f64) * (H - 12.0);
                    format!("{}{x:.1} {y:.1}", if k == 0 { "M" } else { "L" })
                })
                .collect::<String>();
            (lane.label, lane.current, path, range)
        })
        .collect();
    rsx! {
        div { class: "gs26-preview-lanes", role: "img", "aria-label": "{aria}",
            for (label, current, path, range) in lanes {
                div { class: "gs26-preview-lane",
                    div { class: "gs26-preview-lane-head", span { "{label}: " strong { "{current}" } } small { "{range}" } }
                    svg { view_box: "0 0 {W} {H}", preserve_aspect_ratio: "none", "aria-hidden": "true",
                        path { d: "{path}", fill: "none", stroke: "#67d5c8", stroke_width: "2", vector_effect: "non-scaling-stroke" }
                        if let Some(x) = cursor_x {
                            line { x1: "{x}", x2: "{x}", y1: "0", y2: "{H}", stroke: "#e8c77f", stroke_width: "1.5", stroke_dasharray: "4 3", vector_effect: "non-scaling-stroke" }
                        }
                    }
                }
            }
        }
    }
}

/// Web Mercator world pixels (256 px tiles) of a coordinate at zoom `z`.
fn world_px(lat: f64, lon: f64, z: f64) -> (f64, f64) {
    let size = 256.0 * 2f64.powf(z);
    let rad = lat.clamp(-85.05, 85.05).to_radians();
    ((lon + 180.0) / 360.0 * size, (1.0 - (rad.tan() + 1.0 / rad.cos()).ln() / std::f64::consts::PI) / 2.0 * size)
}

/// The shared flight-phase palette (flight-predict contract).
fn phase_color(phase: &str) -> &'static str {
    match phase {
        "pad" => "#94a3b8",
        "boost" => "#f97316",
        "coast" => "#facc15",
        "drogue" => "#38bdf8",
        "main" => "#22c55e",
        "landed" => "#a78bfa",
        _ => "#64748b",
    }
}

/// Recorded altitude above the pad and a plan's predicted altitude, both
/// against seconds from launch (the first Launch/Ascent state, else the
/// start). `None` when the session has no barometer altitude.
fn altitude_comparison(session: &Session, plan: &super::flight_plans::PlanDoc) -> Option<Vec<super::flight_plans_view::Series>> {
    let channel = Channel { group: String::new(), data_type: "BAROMETER_DATA".into(), sender: None, index: 0, label: String::new(), unit: String::new(), precision: 1 };
    let recorded = session.series(&channel, i64::MIN, i64::MAX, 600);
    let predicted = plan.altitude_curve();
    if recorded.is_empty() || predicted.is_empty() {
        return None;
    }
    let launch = session.states.iter().find(|(_, s)| s == "Launch" || s == "Ascent").map(|(t, _)| *t).unwrap_or_else(|| session.start_ms());
    Some(vec![
        super::flight_plans_view::Series { label: "Recorded".into(), color: "#67d5c8".into(), dashed: false, points: recorded.iter().map(|(t, v)| [(*t - launch) as f64 / 1000.0, f64::from(*v)]).collect() },
        super::flight_plans_view::Series { label: super::flight_plans::predicted_label(&plan.label), color: "#a78bfa".into(), dashed: true, points: predicted },
    ])
}

/// The recorded GPS track in Web Mercator, over a bundle's map tiles when it
/// carries them and over a tile-sized grid otherwise. Nothing is fetched.
fn track_map(session: &Session, at: i64, plan: Option<&super::flight_plans::PlanDoc>) -> Element {
    use base64::Engine as _;
    let track = session.track(at);
    let all = session.track(i64::MAX);
    if all.is_empty() {
        return rsx! { p { class: "gs26-preview-note", role: "status", "No GPS position in this data, so there is no track to draw." } };
    }
    let overlay = plan.and_then(|p| p.map_overlay());
    // A predicted session's own plan adds only its areas and markers: its
    // track is the replay track itself.
    let own_plan = session.predicted.is_some();
    let (mut la0, mut la1, mut lo0, mut lo1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    let extra = overlay.into_iter().flat_map(|o| {
        o.areas.iter().flat_map(|a| a.polygon.iter().map(|p| (p[0], p[1]))).chain(o.markers.iter().map(|m| (m.lat_deg, m.lon_deg)))
    });
    for (la, lo) in all.iter().map(|p| (p.0, p.1)).chain(extra) {
        (la0, la1, lo0, lo1) = (la0.min(la), la1.max(la), lo0.min(lo), lo1.max(lo));
    }
    // The deepest zoom whose tiles all exist in the bundle, else one that fits the track.
    let covered = |z: u32| {
        let (x0, y0) = world_px(la1, lo0, z as f64);
        let (x1, y1) = world_px(la0, lo1, z as f64);
        let (tx0, ty0, tx1, ty1) = ((x0 / 256.0) as u32, (y0 / 256.0) as u32, (x1 / 256.0) as u32, (y1 / 256.0) as u32);
        (tx0..=tx1).all(|x| (ty0..=ty1).all(|y| session.tiles.contains_key(&(z, x, y))))
    };
    let tiled = (1..=19u32).rev().find(|z| covered(*z));
    let z = tiled.map(f64::from).unwrap_or_else(|| {
        let (x0, y0) = world_px(la1, lo0, 0.0);
        let (x1, y1) = world_px(la0, lo1, 0.0);
        (600.0 / (x1 - x0).max(1e-9)).min(280.0 / (y1 - y0).max(1e-9)).log2().clamp(1.0, 19.0)
    });
    let (x0, y0) = world_px(la1, lo0, z);
    let (x1, y1) = world_px(la0, lo1, z);
    let pad = ((x1 - x0).max(y1 - y0) * 0.12).max(24.0);
    let (vx, vy, vw, vh) = (x0 - pad, y0 - pad, (x1 - x0) + 2.0 * pad, (y1 - y0) + 2.0 * pad);
    let line = |pts: &[(f64, f64, f32)]| pts.iter().enumerate().map(|(i, p)| {
        let (x, y) = world_px(p.0, p.1, z);
        format!("{}{x:.1} {y:.1}", if i == 0 { "M" } else { "L" })
    }).collect::<String>();
    let images: Vec<(f64, f64, String)> = match tiled {
        Some(zt) => session.tiles.iter().filter(|((tz, _, _), _)| *tz == zt).map(|((_, x, y), data)| {
            (f64::from(*x) * 256.0, f64::from(*y) * 256.0, format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(data)))
        }).collect(),
        None => Vec::new(),
    };
    let (sx, sy) = world_px(all[0].0, all[0].1, z);
    let here = track.last().map(|p| world_px(p.0, p.1, z));
    let (full, flown) = (line(&all), line(&track));
    let r = vw.max(vh) / 90.0;
    let coslat = ((la0 + la1) / 2.0).to_radians().cos();
    let drift = ((all.last().unwrap().0 - all[0].0) * 110_540.0).hypot((all.last().unwrap().1 - all[0].1) * 111_320.0 * coslat);
    let grid_m = 40_075_016.0 * coslat / 2f64.powf(z);
    let position = track.last().map(|p| format!("{:.5}°, {:.5}° · {:.0} m", p.0, p.1, p.2)).unwrap_or_else(|| "On the pad".into());
    let note = if tiled.is_some() { "Map tiles from the replay bundle.".to_string() } else { format!("No map tiles offline; grid squares are {grid_m:.0} m.") };
    let xy = |lat: f64, lon: f64| world_px(lat, lon, z);
    let plan_tracks: Vec<(String, String)> = match overlay {
        Some(o) if !own_plan => o.tracks.iter().filter(|t| t.points.len() >= 2).map(|t| {
            let d = t.points.iter().filter_map(|p| Some((p.first().copied().flatten()?, p.get(1).copied().flatten()?))).enumerate().map(|(i, (la, lo))| {
                let (x, y) = xy(la, lo);
                format!("{}{x:.1} {y:.1}", if i == 0 { "M" } else { "L" })
            }).collect::<String>();
            (d, phase_color(&t.phase).to_string())
        }).collect(),
        _ => Vec::new(),
    };
    let plan_areas: Vec<String> = overlay.map(|o| o.areas.iter().map(|a| {
        a.polygon.iter().enumerate().map(|(i, p)| { let (x, y) = xy(p[0], p[1]); format!("{}{x:.1} {y:.1}", if i == 0 { "M" } else { "L" }) }).collect::<String>() + "Z"
    }).collect()).unwrap_or_default();
    let plan_markers: Vec<(f64, f64, String)> = overlay.map(|o| o.markers.iter().map(|m| { let (x, y) = xy(m.lat_deg, m.lon_deg); (x, y, m.label.clone()) }).collect()).unwrap_or_default();
    let plan_label = overlay.map(|o| super::flight_plans::predicted_label(&o.label));
    let predicted_track = session.predicted.is_some();
    rsx! {
        svg { class: "gs26-preview-map", view_box: "{vx} {vy} {vw} {vh}", role: "img", "aria-label": "Recorded GPS track",
            defs { pattern { id: "preview-map-grid", x: "0", y: "0", width: "256", height: "256", pattern_units: "userSpaceOnUse", path { d: "M256 0H0V256", fill: "none", stroke: "#24364b", vector_effect: "non-scaling-stroke" } } }
            rect { x: "{vx}", y: "{vy}", width: "{vw}", height: "{vh}", fill: "#101d2d" }
            rect { x: "{vx}", y: "{vy}", width: "{vw}", height: "{vh}", fill: "url(#preview-map-grid)" }
            for (ix, iy, href) in images { image { x: "{ix}", y: "{iy}", width: "256", height: "256", href: "{href}" } }
            for d in plan_areas.iter() { path { d: "{d}", fill: "#a78bfa", fill_opacity: "0.12", stroke: "#a78bfa", stroke_width: "2", stroke_dasharray: "6 4", vector_effect: "non-scaling-stroke" } }
            for (d, color) in plan_tracks.iter() { path { "data-plan-track": "", d: "{d}", fill: "none", stroke: "{color}", stroke_opacity: "0.55", stroke_width: "3", stroke_dasharray: "7 5", vector_effect: "non-scaling-stroke" } }
            path { d: "{full}", fill: "none", stroke: "#94a3b8", stroke_width: "2", stroke_dasharray: "5 4", vector_effect: "non-scaling-stroke" }
            path { d: "{flown}", fill: "none", stroke: if predicted_track { "#a78bfa" } else { "#67d5c8" }, stroke_width: "3", stroke_dasharray: if predicted_track { "7 4" } else { "none" }, vector_effect: "non-scaling-stroke" }
            for (x, y, label) in plan_markers.iter() { circle { cx: "{x}", cy: "{y}", r: "{r * 0.8}", fill: "#a78bfa", stroke: "#111827", stroke_width: "2", vector_effect: "non-scaling-stroke", title { "{label}" } } }
            circle { cx: "{sx}", cy: "{sy}", r: "{r}", fill: "#8bc8ff" }
            if let Some((x, y)) = here { circle { cx: "{x}", cy: "{y}", r: "{r}", fill: if predicted_track { "#a78bfa" } else { "#e8c77f" } } }
        }
        if predicted_track {
            p { class: "gs26-preview-note", "data-predicted-map": "", strong { "Predicted track" } " (dashed purple): the station's flight simulation, not a recording." }
        }
        if let Some(label) = plan_label.filter(|_| !own_plan) {
            p { class: "gs26-preview-note", "data-plan-compare-map": "", strong { "{label}" } " drawn dashed in phase colours with its landing area; the recorded track is solid." }
        }
        p { class: "gs26-preview-note", "Rocket: {position} · landing {drift:.0} m from the pad (blue). {note}" }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Clock {
    Playing,
    Paused,
}

#[component]
pub fn OfflinePreview(
    on_exit: EventHandler<()>,
    theme: ThemeConfig,
    /// Page to open on (a `PAGES` id); Dashboard by default.
    #[props(default)]
    page: Option<String>,
    /// A replay file to open instead of the demo: (file name, contents).
    #[props(default)]
    replay: Option<(String, String)>,
    /// A replay bundle (`.gsreplay`) to open instead of the demo.
    #[props(default)]
    bundle: Option<(String, Vec<u8>)>,
) -> Element {
    #[cfg(not(target_arch = "wasm32"))]
    super::watch_companion::use_demo_publisher();
    // The session is built here, on entry, and dropped with this scope.
    let mut session = use_signal(|| {
        let opened = replay.as_ref().and_then(|(name, text)| replay_file::open(name, text).ok());
        let opened = opened.or_else(|| bundle.as_ref().and_then(|(name, bytes)| local_station::bundle::open(name, bytes).ok()));
        Rc::new(opened.unwrap_or_else(|| demo::session(super::current_wallclock_ms() - demo::OPEN_AT_MS)))
    });
    let mut at = use_signal(|| { let s = session.peek(); if s.is_demo() { demo::OPEN_AT_MS } else { s.start_ms() } });
    let mut clock = use_signal(|| if session.peek().is_demo() { Clock::Playing } else { Clock::Paused });
    let mut speed = use_signal(|| 1i64);
    let mut active = use_signal(|| known_page(page.as_deref().unwrap_or("state")).to_string());
    let mut collapsed = use_signal(|| false);
    let mut sidebar = use_signal(|| true);
    let mut checks = use_signal(|| vec![false; CHECKS.len()]);
    let mut toast = use_signal(|| None::<(bool, String)>);
    let mut data_group = use_signal(String::new);
    let mut diagram = use_signal(|| if session.peek().mode == Mode::Flight && !session.peek().is_demo() { "av_bay".to_string() } else { "fill".to_string() });
    let mut file_status = use_signal(String::new);
    let mut compare = use_signal(String::new);
    let mut player_loaded = use_signal(|| false);
    let player_id = use_hook(|| format!("offline-3d-{}", super::current_wallclock_ms()));
    use_future(move || async move {
        loop {
            sleep_ms(TICK_MS).await;
            if *clock.peek() != Clock::Playing {
                continue;
            }
            let s = session.peek().clone();
            let next = *at.peek() + TICK_MS as i64 * *speed.peek();
            if s.is_demo() {
                at.set(next % demo::LOOP_MS);
            } else if next >= s.end_ms() {
                at.set(s.end_ms());
                clock.set(Clock::Paused);
            } else {
                at.set(next);
            }
        }
    });

    let open_file = move |file: dioxus::html::FileData| {
        spawn(async move {
            file_status.set(format!("Opening {}…", file.name()));
            let result = async {
                if file.size() as usize > replay_file::MAX_BYTES {
                    return Err("Replay exceeds 64 MiB.".to_string());
                }
                let bytes = file.read_bytes().await.map_err(|e| e.to_string())?;
                open_bytes(&file.name(), &bytes)
            }
            .await;
            match result {
                Ok(next) => {
                    at.set(next.start_ms());
                    diagram.set(if next.mode == Mode::Flight { "av_bay".into() } else { "fill".into() });
                    session.set(Rc::new(next));
                    compare.set(String::new());
                    toast.set(None);
                    clock.set(Clock::Paused);
                    data_group.set(String::new());
                    player_loaded.set(false);
                    file_status.set(String::new());
                }
                Err(error) => file_status.set(format!("Could not open {}: {error}", file.name())),
            }
        });
    };

    let s = session.read().clone();
    let now = at();
    let state = s.state_at(now);
    let demo_mode = s.is_demo();
    let mut send = move |cmd: String| {
        let (ok, message) = session.peek().command(&cmd);
        toast.set(Some((ok, message)));
    };
    let current = active();
    let items: Vec<NavigationItem> = PAGES.iter().map(|(id, label)| NavigationItem { id: (*id).into(), label: (*label).into() }).collect();
    let shell_style = format!(
        "--gs26-app-height:100dvh;--gs26-app-background:{};--gs26-app-text:{};--gs26-panel-background:{};--gs26-panel-alt-background:{};--gs26-border:{};--gs26-text-secondary:{};--gs26-text-muted:{};--gs-control-bg:{};--gs-control-border:{};--gs-control-text:{};",
        theme.app_background, theme.text_primary, theme.panel_background, theme.panel_background_alt, theme.border, theme.text_secondary, theme.text_muted, theme.button_background, theme.button_border, theme.button_text
    );
    let banner_title = match (&s.source, &s.predicted) {
        (local_station::Source::Demo, _) => "Demo mode — no station connected".to_string(),
        (local_station::Source::Replay { .. }, Some(label)) => format!("Predicted replay — {label}"),
        (local_station::Source::Replay { file_name }, None) => format!("Offline replay — {file_name}"),
    };
    // A plan to draw beside the data: a predicted session's own plan, else
    // the one chosen from the plans a recording bundle carries.
    let compare_plan: Option<super::flight_plans::PlanDoc> = if s.predicted.is_some() {
        s.plans.first().cloned()
    } else {
        s.plans.iter().find(|p| p.id == compare()).cloned()
    };
    let chart_tag = if s.predicted.is_some() { " · Predicted" } else { "" };
    let plan_picker: Element = if s.predicted.is_none() && !s.plans.is_empty() {
        let choices: Vec<(String, String)> = s.plans.iter().filter(|p| p.map_overlay().is_some()).map(|p| (p.id.clone(), super::flight_plans::predicted_label(&p.label))).collect();
        rsx! {
            label { class: "gs26-preview-plan-picker", "data-plan-picker": "",
                "Compare with a flight plan "
                select { value: compare(), onchange: move |e| compare.set(e.value()),
                    option { value: "", "None" }
                    for (id, label) in choices { option { value: "{id}", "{label}" } }
                }
            }
        }
    } else {
        rsx! {}
    };
    let banner_detail = if demo_mode {
        "Example data from a demo scenario. Commands are acknowledged here and never sent.".to_string()
    } else {
        format!("{} · {} · no station connected · read-only", s.mode.label(), s.provenance)
    };
    let (from, to) = (s.start_ms(), s.end_ms().max(s.start_ms() + 1));
    let window = if demo_mode { ((now - 120_000).max(0), now.max(1)) } else { (from, to) };
    let cursor = (!demo_mode).then_some(now);
    let actions: Vec<Value> = s.layout["actions_tab"]["actions"].as_array().cloned().unwrap_or_default();
    let action_label = |cmd: &str| actions.iter().find(|a| a["cmd"] == cmd).and_then(|a| a["label"].as_str()).unwrap_or(cmd).to_string();
    let boards: Vec<Value> = s.get("/api/boards", now).and_then(|b| b["boards"].as_array().cloned()).unwrap_or_default();
    let alerts: Vec<Value> = s.get("/api/alerts", now).and_then(|a| a.as_array().cloned()).unwrap_or_default();
    let channels = s.channels();

    let widget = |w: &Value| -> Element {
        let kind = w["kind"].as_str().unwrap_or("");
        let ty = w["data_type"].as_str().unwrap_or("").to_string();
        match kind {
            "summary" => {
                let cards: Vec<(String, String, String)> = w["items"].as_array().into_iter().flatten().map(|item| {
                    let sender = item["sender_id"].as_str();
                    let index = item["index"].as_u64().unwrap_or(0) as usize;
                    match s.latest(&ty, sender, now) {
                        Some(row) => (
                            item["label"].as_str().unwrap_or("").to_string(),
                            row.values.get(index).copied().flatten().map(|v| format_item(v, &item["formatter"])).unwrap_or_else(|| "—".into()),
                            format!("{} · {}", row.sender, age_text(now - row.t_ms)),
                        ),
                        None => (item["label"].as_str().unwrap_or("").to_string(), "—".into(), "No data yet".into()),
                    }
                }).collect();
                rsx! { for (label, value, meta) in cards { article { class: "gs26-preview-card", span { "{label}" } strong { "{value}" } small { "{meta}" } } } }
            }
            "valve_state" => {
                let row = s.latest(&ty, None, now);
                let chips: Vec<(String, String, &'static str)> = w["valves"].as_array().into_iter().flatten().enumerate().map(|(k, v)| {
                    let index = v["index"].as_u64().unwrap_or(k as u64) as usize;
                    let labels = &w["valve_labels"][k];
                    let pick = |key: &str, fallback: &str| v[key].as_str().or_else(|| labels[key].as_str()).unwrap_or(fallback).to_string();
                    let (text, class) = match row.and_then(|r| r.values.get(index).copied().flatten()) {
                        Some(x) if x >= 0.5 => (pick("true_label", "Open"), "gs-badge gs-badge--warn"),
                        Some(_) => (pick("false_label", "Closed"), "gs-badge gs-badge--ok"),
                        None => (pick("unknown_label", "Unknown"), "gs-badge"),
                    };
                    (v["label"].as_str().unwrap_or("").to_string(), text, class)
                }).collect();
                rsx! { div { class: "gs26-preview-valves gs26-preview-wide", for (name, text, class) in chips { div { class: "gs26-preview-valve", span { "{name}" } span { class: class, "{text}" } } } } }
            }
            "chart" => {
                let lanes: Vec<Lane> = w["chart_series"].as_array().into_iter().flatten().filter_map(|series| {
                    let ty = series["data_type"].as_str()?;
                    let index = series["index"].as_u64().unwrap_or(0) as usize;
                    let channel = Channel { group: String::new(), data_type: ty.into(), sender: series["sender_id"].as_str().map(str::to_string), index, label: String::new(), unit: String::new(), precision: 1 };
                    let points = s.series(&channel, window.0, window.1.min(now), 240);
                    let current = points.last().map(|p| format!("{:.1}", p.1)).unwrap_or_else(|| "—".into());
                    Some(Lane { label: series["label"].as_str().unwrap_or(ty).to_string(), current, points })
                }).collect();
                let aria = w["chart_title"].as_str().unwrap_or("Chart").to_string();
                rsx! { div { class: "gs26-preview-wide", h3 { class: "gs-section-label", "{aria}{chart_tag}" } {lane_chart(format!("{aria}{chart_tag}"), lanes, window.0, window.1, cursor)} } }
            }
            "board_status" => {
                let chips: Vec<(String, String, &'static str, String)> = boards.iter().map(|b| {
                    let (badge, text) = match b["age_ms"].as_i64() { Some(a) if a < 2_000 => ("gs-badge gs-badge--ok", age_text(a)), Some(a) => ("gs-badge gs-badge--warn", age_text(a)), None => ("gs-badge", "Not seen".into()) };
                    (text_of(&b["board_label"]), text_of(&b["sender_id"]), badge, text)
                }).collect();
                rsx! { div { class: "gs26-preview-boards gs26-preview-wide",
                    for (label, sender, badge, text) in chips { div { class: "gs26-preview-board", span { strong { "{label}" } small { " {sender}" } } span { class: badge, "{text}" } } }
                } }
            }
            "actions" => {
                let cmds: Vec<String> = w["actions"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
                rsx! { div { class: "gs26-preview-actions gs26-preview-wide",
                    for cmd in cmds { button { r#type: "button", class: if cmd == "Abort" { "gs26-preview-abort" } else { "" }, onclick: { let cmd = cmd.clone(); move |_| send(cmd.clone()) }, "{action_label(&cmd)}" } }
                } }
            }
            "map" => rsx! { div { class: "gs26-preview-wide", {track_map(&s, now, compare_plan.as_ref())} } },
            other => rsx! { p { class: "gs26-preview-note gs26-preview-wide", "A connected station shows its “{other}” widget here." } },
        }
    };

    rsx! {
        style {{include_str!("../design_system.css")}}
        style {{include_str!("navigation_rail.css")}}
        style {{include_str!("controls.css")}}
        style {{live_diagrams::render::CSS}}
        style {{live_diagrams::PAGE_CSS}}
        style {{include_str!("offline_preview.css")}}
        div { class: "gs26-offline-preview", style: shell_style, "data-source": if demo_mode { "demo" } else { "replay" },
            ondragover: move |e| e.prevent_default(),
            ondrop: move |e| { e.prevent_default(); if let Some(file) = e.files().into_iter().next() { open_file(file); } },
            header { class: "gs26-preview-header", role: "banner",
                div { class: "gs26-preview-banner",
                    strong { "{banner_title}" }
                    span { "{banner_detail}" }
                    if s.predicted.is_some() {
                        span { class: "gs-badge gs-badge--warn", "data-predicted-banner": "", "Predicted · simulated, not recorded" }
                    }
                }
                div { class: "gs26-preview-header-actions",
                    label { class: "gs26-preview-file-button",
                        // No `accept` filter: phone pickers cannot match the .gsreplay extension, and the content is checked on open.
                        input { r#type: "file", "aria-label": "Open replay file",
                            onchange: move |e| { if let Some(file) = e.files().into_iter().next() { open_file(file); } } }
                        "Open replay file"
                    }
                    if !demo_mode {
                        button { r#type: "button", onclick: move |_| {
                            session.set(Rc::new(demo::session(super::current_wallclock_ms() - demo::OPEN_AT_MS)));
                            at.set(demo::OPEN_AT_MS); clock.set(Clock::Playing); toast.set(None); data_group.set(String::new()); player_loaded.set(false);
                        }, "Back to demo" }
                    }
                    button { r#type: "button", onclick: move |_| on_exit.call(()), "Exit" }
                }
            }
            if !file_status().is_empty() { p { class: "gs-notice gs-notice--warn gs26-preview-file-status", role: "status", "{file_status}" } }
            div { class: "gs26-preview-transport", role: "toolbar", "aria-label": "Playback",
                button { r#type: "button", "aria-label": if clock() == Clock::Playing { "Pause" } else { "Play" },
                    onclick: move |_| {
                        let s = session.peek().clone();
                        if clock() == Clock::Playing { clock.set(Clock::Paused); } else {
                            if !s.is_demo() && *at.peek() >= s.end_ms() { at.set(s.start_ms()); }
                            clock.set(Clock::Playing);
                        }
                    },
                    if clock() == Clock::Playing { "Pause" } else { "Play" }
                }
                span { class: "gs26-preview-clock", "{mission_clock(&s, now)}" }
                if let Some(label) = s.predicted.as_ref() {
                    span { class: "gs-badge gs-badge--warn", "data-testid": "preview-predicted", "{label}" }
                }
                span { class: "gs-badge gs-badge--info", "data-testid": "preview-state", "{super::types::display_flight_state(&state)}" }
                input { r#type: "range", "aria-label": "Position", min: "{from}", max: "{to}", step: "250", value: "{now}",
                    oninput: move |e| { if let Ok(v) = e.value().parse::<i64>() { at.set(v); } } }
                select { "aria-label": "Speed", value: "{speed}", onchange: move |e| speed.set(e.value().parse().unwrap_or(1)),
                    for x in [1, 2, 5, 10] { option { value: "{x}", "{x}×" } }
                }
            }
            if let Some((ok, message)) = toast() {
                div { class: if ok { "gs-notice gs26-preview-toast" } else { "gs-notice gs-notice--warn gs26-preview-toast" }, role: "status",
                    span { "{message}" }
                    button { r#type: "button", "aria-label": "Dismiss", onclick: move |_| toast.set(None), "OK" }
                }
            }
            div { class: "gs26-preview-workspace", "data-sidebar": sidebar().to_string(),
                if sidebar() {
                    NavigationRail { items, active: current.clone(), title: if demo_mode { "Demo station".to_string() } else { "Replay".to_string() },
                        collapsed: collapsed(), on_toggle: move |_| collapsed.toggle(),
                        on_navigate: move |id: String| active.set(known_page(&id).to_string()),
                        on_settings: move |_| active.set("settings".into()), warning_alert: !alerts.is_empty(), error_alert: false }
                }
                main { class: "gs26-preview-main",
                    div { class: "gs26-preview-page-title",
                        h1 { "{title(&current)}" }
                        div { class: "gs26-preview-page-picker",
                            label { r#for: "preview-page", "Page" }
                            select { id: "preview-page", value: current.clone(), onchange: move |e| active.set(known_page(&e.value()).to_string()),
                                for (id, label) in PAGES { option { value: *id, "{label}" } }
                                option { value: "settings", "Settings" }
                            }
                        }
                    }
                    if current == "state" {
                        {
                            let sections = sections_for(&s.layout, &state);
                            if sections.is_empty() {
                                let cards: Vec<(String, String, String)> = channels.iter().take(12).map(|c| match s.value(c, now) {
                                    Some((v, age)) => (format!("{} · {}", c.group, c.label), c.format(v), age_text(age)),
                                    None => (format!("{} · {}", c.group, c.label), "—".into(), "No data yet".into()),
                                }).collect();
                                rsx! {
                                    p { class: "gs26-preview-note", "This file has no dashboard layout; showing the first recorded channels." }
                                    div { class: "gs26-preview-cards", for (label, value, meta) in cards { article { class: "gs26-preview-card", span { "{label}" } strong { "{value}" } small { "{meta}" } } } }
                                }
                            } else {
                                rsx! {
                                    for (heading, widgets) in sections.iter().map(|section| (text_of(&section["title"]), shown_widgets(&s, &section["widgets"]))).filter(|(_, w)| !w.is_empty()) {
                                        section { class: "gs26-preview-panel",
                                            h2 { "{heading}" }
                                            div { class: "gs26-preview-cards",
                                                for w in widgets.iter() { {widget(w)} }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if current == "mission" {
                        {
                            let mut steps: Vec<String> = Vec::new();
                            for (_, name) in &s.states { if !steps.contains(name) { steps.push(name.clone()); } }
                            let fill = s.first_channel("LOADCELL_50_WEIGHT_KG", 0).and_then(|c| s.value(&c, now).map(|v| (c.format(v.0), v.0.max(0.0)))).filter(|_| state.contains("Fill"));
                            let groups: Vec<String> = { let mut g: Vec<String> = Vec::new(); for a in &actions { let name = a["group"].as_str().unwrap_or("Actions").to_string(); if !g.contains(&name) { g.push(name); } } g };
                            rsx! {
                                section { class: "gs26-preview-panel",
                                    h2 { "{s.mode.label()} procedure" }
                                    if steps.is_empty() { p { class: "gs26-preview-note", "This recording has no flight-state rows." } }
                                    ol { class: "gs26-preview-steps", for name in steps.iter() { li { "data-active": (*name == state).to_string(), "{super::types::display_flight_state(name)}" } } }
                                    if let Some((shown, mass)) = fill {
                                        div { class: "gs26-preview-sequence-progress", role: "group", "aria-label": "Fill progress",
                                            span { "Oxidizer loaded: {shown}" }
                                            progress { max: "10.5", value: "{mass}" }
                                        }
                                    }
                                    for group in groups.iter() {
                                        h3 { class: "gs-section-label", "{group}" }
                                        div { class: "gs26-preview-actions",
                                            for a in actions.iter().filter(|a| a["group"].as_str().unwrap_or("Actions") == group) {
                                                {
                                                    let cmd = a["cmd"].as_str().unwrap_or("").to_string();
                                                    let label = a["label"].as_str().unwrap_or(&cmd).to_string();
                                                    let style = format!("border-color:{};background:{};color:{};", a["border"].as_str().unwrap_or(""), a["bg"].as_str().unwrap_or(""), a["fg"].as_str().unwrap_or(""));
                                                    rsx! { button { r#type: "button", style: style, onclick: move |_| send(cmd.clone()), "{label}" } }
                                                }
                                            }
                                        }
                                    }
                                    if actions.is_empty() { p { class: "gs26-preview-note", "The layout defines no actions." } }
                                    p { class: "gs26-preview-note", if demo_mode { "Buttons and labels come from the layout's actions. In demo mode a command is acknowledged on this device and never sent; a connected station checks permissions, interlocks and sensor freshness first." } else { "Replays are read-only; commands are not sent." } }
                                }
                            }
                        }
                    } else if current == "data" {
                        {
                            let mut groups: Vec<String> = Vec::new();
                            for c in &channels { if !groups.contains(&c.group) { groups.push(c.group.clone()); } }
                            let selected = if groups.contains(&data_group()) { data_group() } else { groups.first().cloned().unwrap_or_default() };
                            let lanes: Vec<Lane> = channels.iter().filter(|c| c.group == selected).map(|c| {
                                let points = s.series(c, window.0, window.1.min(if demo_mode { now } else { to }), 300);
                                let current = s.value(c, now).map(|(v, _)| c.format(v)).unwrap_or_else(|| "—".into());
                                Lane { label: c.label.clone(), current, points }
                            }).collect();
                            rsx! {
                                section { class: "gs26-preview-panel",
                                    div { class: "gs26-preview-data-tabs", role: "tablist", "aria-label": "Data type",
                                        for g in groups.iter() {
                                            button { r#type: "button", role: "tab", "aria-selected": (*g == selected).to_string(), class: if *g == selected { "selected" } else { "" },
                                                onclick: { let g = g.clone(); move |_| data_group.set(g.clone()) }, "{g}" }
                                        }
                                    }
                                    if lanes.is_empty() { p { class: "gs26-preview-note", "No channels." } } else {
                                        {lane_chart(format!("{selected} history{chart_tag}"), lanes, window.0, window.1, cursor)}
                                    }
                                    if s.predicted.is_some() { p { class: "gs26-preview-note", "data-predicted-chart": "", strong { "Predicted data" } ": synthesized from a flight plan, not recorded." } }
                                    p { class: "gs26-preview-note", if demo_mode { "Last two minutes. Tabs, channel names and units come from the layout's data tabs." } else { "Whole recording; the dashed line is the replay position." } }
                                }
                            }
                        }
                    } else if current == "detailed" {
                        {
                            let list = if s.diagrams.is_empty() { topology::bundled() } else { live_diagrams::with_station_diagrams(s.diagrams.clone()) };
                            let chosen = list.iter().find(|t| t.id == diagram()).or_else(|| list.first()).cloned();
                            match chosen {
                                Some(t) => {
                                    let state = derive::derive(&t, &local_station::DiagramInputs { session: &s, at: now });
                                    let vars = live_diagrams::render::theme_vars(&theme);
                                    rsx! {
                                        div { class: "gs26-pid-page", style: "{vars}",
                                            div { class: "gs26-pid-toolbar", role: "toolbar", "aria-label": "Diagram",
                                                for d in list.iter() { button { r#type: "button", "aria-pressed": (d.id == t.id).to_string(), onclick: { let id = d.id.clone(); move |_| diagram.set(id.clone()) }, "{d.title}" } }
                                            }
                                            live_diagrams::DiagramPanel { topology: t, state, program: false, provenance: s.provenance.clone() }
                                        }
                                    }
                                }
                                None => rsx! { p { "No diagram topology is available." } },
                            }
                        }
                    } else if current == "network-topology" {
                        section { class: "gs26-preview-panel", h2 { "Boards" }
                            div { class: "gs26-preview-boards",
                                for b in boards.iter() {
                                    {
                                        use super::network_health::board_display::{Health, health_line};
                                        let age = b["age_ms"].as_i64();
                                        let health = match age { Some(a) if a < 2_000 => Health::Healthy, Some(a) if a < 10_000 => Health::Unhealthy, _ => Health::Stale };
                                        let detail = age.map(age_text).unwrap_or_else(|| "not seen".into());
                                        let packets = format!("{} packets", b["packet_count"].as_u64().unwrap_or(0));
                                        let (label, sender) = (text_of(&b["board_label"]), text_of(&b["sender_id"]));
                                        rsx! { div { class: "gs26-preview-board",
                                            span { strong { "{label}" } small { " {sender}" } }
                                            {health_line("network", "Last packet", health, &detail)}
                                            small { "{packets}" }
                                        } }
                                    }
                                }
                            }
                        }
                        section { class: "gs26-preview-panel", h2 { "Warnings" }
                            if alerts.is_empty() { p { class: "gs26-preview-note", "No warnings yet." } }
                            ul { class: "gs26-preview-alerts",
                                for (when, message) in alerts.iter().rev().map(|a| (mission_clock(&s, a["timestamp_ms"].as_i64().unwrap_or(0) - s.epoch_ms), text_of(&a["message"]))) {
                                    li { span { class: "gs-badge gs-badge--warn", "{when}" } span { "{message}" } }
                                }
                            }
                        }
                    } else if current == "map" {
                        section { class: "gs26-preview-panel", h2 { "Recovery map" }
                            {plan_picker.clone()}
                            {track_map(&s, now, compare_plan.as_ref())}
                            if !demo_mode {
                                div { class: "gs26-preview-los",
                                    button { r#type: "button", disabled: true, "data-simulate-los": "", title: "{OFFLINE_LOS_REASON}", "Simulate signal loss here" }
                                    small { class: "gs26-preview-note", "{OFFLINE_LOS_REASON}" }
                                }
                            }
                        }
                    } else if current == "vehicle" {
                        section { class: "gs26-preview-panel", h2 { if s.mode == Mode::TestFire { "Burn summary" } else { "Flight summary" } }
                            dl { class: "gs26-preview-summary", for (k, v) in s.summary().into_iter().chain(s.derived.iter().cloned()) { div { dt { "{k}" } dd { "{v}" } } } }
                            for note in s.notes.iter() { p { class: "gs26-preview-note", "{note}" } }
                        }
                        if s.predicted.is_none() && !s.plans.is_empty() {
                            section { class: "gs26-preview-panel", h2 { "Predicted vs recorded" }
                                {plan_picker.clone()}
                                if let Some(series) = compare_plan.as_ref().and_then(|p| altitude_comparison(&s, p)) {
                                    super::flight_plans_view::CurvePlot { title: "Altitude above pad, from launch".to_string(), unit: "m".to_string(), series, theme: theme.clone() }
                                } else if compare_plan.is_some() {
                                    p { class: "gs26-preview-note", "This recording has no barometer altitude to compare." }
                                }
                            }
                        }
                        section { class: "gs26-preview-panel", h2 { "3D replay" }
                            p { class: "gs26-preview-note", "The same offline player as an exported replay: samples, valve names and units come from the file; the vehicle is the generic rocket unless the file carries vehicle geometry." }
                            if !player_loaded() {
                                button { r#type: "button", onclick: {
                                    let s = s.clone(); let id = player_id.clone();
                                    move |_| {
                                        let document = s.replay_document.clone().unwrap_or_else(|| demo::replay_document(&s));
                                        let id = id.clone();
                                        player_loaded.set(true);
                                        spawn(async move {
                                            let result = async {
                                                let text = serde_json::to_string(&document).map_err(|e| e.to_string())?;
                                                let encoded = serde_json::to_string(&text).map_err(|e| e.to_string())?;
                                                let script = format!("{}\ntry{{const root=document.getElementById({});root.__player?.dispose();root.__player=GSOfflineReplay.mount(root,{encoded});return 'ok';}}catch(e){{return String(e&&e.message||e);}}", super::recording_download::offline_replay_script()?, serde_json::to_string(&id).unwrap_or_default());
                                                document::eval(&script).join::<String>().await.map_err(|e| e.to_string())
                                            }.await;
                                            if let Err(e) = result { file_status.set(format!("3D replay unavailable: {e}")); }
                                        });
                                    }
                                }, "Load 3D replay" }
                            }
                            div { id: "{player_id}", class: "gs26-preview-player" }
                        }
                    } else if current == "calibration" {
                        section { class: "gs26-preview-panel", h2 { "Calibration" }
                            if s.calibration.is_empty() {
                                p { class: "gs26-preview-note", "This file carries recorded, already-calibrated values and no calibration document. Calibration is edited on a connected station." }
                            }
                            for c in s.calibration.iter() {
                                h3 { class: "gs-section-label", {c.label.clone()} }
                                {
                                    let (rmin, rmax) = c.points.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
                                    let (emin, emax) = c.points.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
                                    let x = move |r: f32| 40.0 + (r - rmin) / (rmax - rmin).max(1e-6) * 520.0;
                                    let y = move |e: f32| 170.0 - (e - emin) / (emax - emin).max(1e-6) * 150.0;
                                    let fit = format!("M{} {}L{} {}", x(rmin), y(c.slope * rmin + c.offset), x(rmax), y(c.slope * rmax + c.offset));
                                    let dots: Vec<(f32, f32)> = c.points.iter().map(|p| (x(p.0), y(p.1))).collect();
                                    let rows: Vec<(String, String, String)> = c.points.iter().map(|p| (format!("{:.3}", p.0), format!("{:.2}", p.1), format!("{:.2}", c.slope * p.0 + c.offset))).collect();
                                    let equation = format!("Fit: {} = {:.2} × {} {:+.2}", c.unit, c.slope, c.raw_unit, c.offset);
                                    let (label, raw_unit, unit) = (c.label.clone(), c.raw_unit.clone(), c.unit.clone());
                                    let has_points = !c.points.is_empty();
                                    rsx! {
                                        if !has_points { p { class: "gs26-preview-note", "No capture points saved for this channel." } }
                                        if has_points { svg { class: "gs26-preview-chart", view_box: "0 0 600 190", role: "img", "aria-label": "{label} calibration points",
                                            path { d: "M40 10V170H580", fill: "none", stroke: "#64748b" }
                                            path { d: "{fit}", stroke: "#e8c77f", stroke_width: "2", fill: "none" }
                                            for (cx, cy) in dots { circle { cx: "{cx}", cy: "{cy}", r: "5", fill: "#67d5c8" } }
                                            text { x: "44", y: "186", fill: "#94a3b8", font_size: "11", "Input ({raw_unit}) →" }
                                        } }
                                        if has_points { table { class: "gs26-preview-table",
                                            thead { tr { th { "Input ({raw_unit})" } th { "Reference ({unit})" } th { "Fit ({unit})" } } }
                                            tbody { for (a, b, f) in rows { tr { td { "{a}" } td { "{b}" } td { "{f}" } } } }
                                        } }
                                        p { class: "gs26-preview-note", "{equation}" }
                                    }
                                }
                                if demo_mode { div { class: "gs26-preview-actions",
                                    button { r#type: "button", onclick: move |_| send("Capture zero".into()), "Capture zero" }
                                    button { r#type: "button", onclick: move |_| send("Capture span".into()), "Capture span" }
                                } }
                            }
                        }
                    } else if current == "data-export" {
                        section { class: "gs26-preview-panel", h2 { "Recordings" }
                            if demo_mode {
                                table { class: "gs26-preview-table",
                                    thead { tr { th { "Recording" } th { "Kind" } th { "Size" } th {} } }
                                    tbody {
                                        for (id, label, kind, size) in s.recordings.iter().map(|r| (r.id.clone(), r.label.clone(), r.kind.clone(), format!("{:.1} MB", r.bytes as f64 / 1e6))) {
                                            tr { td { "{label}" } td { "{kind}" } td { "{size}" }
                                                td { button { r#type: "button", onclick: move |_| send(format!("Download {id}")), "Download" } } }
                                        }
                                    }
                                }
                                p { class: "gs26-preview-note", "A connected station lists its recordings here; export one as a portable replay and open it with “Open replay file” on any device, with no station." }
                            } else {
                                if let Some(csv) = s.csv.clone() {
                                    {
                                        let name = format!("{}.csv", s.title.trim_end_matches(".db"));
                                        rsx! {
                                            div { class: "gs26-preview-actions",
                                                button { r#type: "button", onclick: move |_| {
                                                    let result = save_file(&name, &csv, "text/csv");
                                                    toast.set(Some((result.is_ok(), result.unwrap_or_else(|e| format!("CSV export failed: {e}")))));
                                                }, "Export CSV (as the station exports it)" }
                                            }
                                        }
                                    }
                                }
                                if let Some(layout) = s.layout.as_object().filter(|o| !o.is_empty()).map(|o| serde_json::to_vec_pretty(o).unwrap_or_default()) {
                                    div { class: "gs26-preview-actions",
                                        button { r#type: "button", onclick: move |_| {
                                            let result = save_file("layout.json", &layout, "application/json");
                                            toast.set(Some((result.is_ok(), result.unwrap_or_else(|e| format!("Export failed: {e}")))));
                                        }, "Export layout.json" }
                                    }
                                }
                                dl { class: "gs26-preview-summary",
                                    div { dt { "File" } dd { "{s.title}" } }
                                    div { dt { "Format" } dd { if s.manifest.is_some() { "Replay bundle (.gsreplay)" } else { "Portable replay (.json)" } } }
                                    div { dt { "Map tiles" } dd { "{s.tiles.len()}" } }
                                    div { dt { "Mode" } dd { "{s.mode.label()}" } }
                                    div { dt { "Rows" } dd { "{s.samples.len()}" } }
                                    div { dt { "Channels" } dd { "{channels.len()}" } }
                                    div { dt { "Station layout" } dd { if s.layout.as_object().is_some_and(|o| !o.is_empty()) { "Included" } else { "Not included" } } }
                                    div { dt { "Vehicle geometry" } dd { if s.vehicle_geometry.is_some() { "Included" } else { "Generic rocket" } } }
                                }
                            }
                        }
                    } else if current == "checklists" {
                        section { class: "gs26-preview-panel", h2 { "Practice checklist" }
                            p { class: "gs26-preview-note", "These practice checks stay here and are discarded when you exit. They never complete a real station checklist." }
                            for (index, label) in CHECKS.iter().enumerate() {
                                label { class: "gs26-preview-check", input { r#type: "checkbox", checked: checks()[index], onchange: move |_| { let mut next = checks(); next[index] = !next[index]; checks.set(next); } } span { "{label}" } }
                            }
                            button { r#type: "button", onclick: move |_| checks.set(vec![false; CHECKS.len()]), "Reset practice checks" }
                        }
                    } else if current == "crew-voice" || current == "stream-manager" {
                        section { class: "gs26-preview-panel", h2 { if current == "crew-voice" { "Crew channels" } else { "Audience program" } }
                            p { class: "gs26-preview-note", if current == "crew-voice" { "Join a channel after connecting and granting microphone access. No microphone or audio devices are opened offline." } else { "Stream operators arrange cameras and replay recorded moments on a connected station. Nothing is published offline." } }
                            button { r#type: "button", onclick: { let cmd = if current == "crew-voice" { "Join channel" } else { "Open stream controls" }; move |_| send(cmd.to_string()) }, if current == "crew-voice" { "Join channel" } else { "Open stream controls" } }
                        }
                    } else {
                        section { class: "gs26-preview-panel", h2 { "Offline preferences" }
                            label { class: "gs26-preview-check", input { r#type: "checkbox", checked: sidebar(), onchange: move |_| sidebar.toggle() } span { "Use sidebar navigation" } }
                            p { class: "gs26-preview-note", "These choices affect this workspace only. Station settings, saved layouts and credentials remain unchanged." }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
