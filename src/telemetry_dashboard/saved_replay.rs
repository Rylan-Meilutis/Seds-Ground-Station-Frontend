//! Saved-data replay, presented in the station mode the data was recorded in.
//!
//! The view reads only the recording's own replay endpoints
//! (`/api/recordings/{id}/replay/...`): its recorded context (GS_STATION_MODE
//! rows / capture manifest), the recorded mode's layout with no actions, the
//! bounded sample windows, the recorded alerts and the burn-report thrust
//! overlay. Its samples live in this component only: they never reach the
//! live telemetry stores, charts, alerts, notifications or safety logic, and
//! it has no way to send a command. The live station mode is shown for
//! information and never changed; the HITL view picker changes only how the
//! replay is drawn.
use super::layout::{LayoutConfig, ThemeConfig};
use super::recording_download::{ExportOwner, saved_json};
use super::recording_store_view::{duration_label, utc_label};
use dioxus::prelude::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// The window API's limits (backend `recording_replay`): 30 s and 32 types.
const WINDOW_MS: i64 = 29_999;
const TYPES_PER_REQUEST: usize = 32;
const TICK_MS: u32 = 250;
pub(super) const VIEWS: [&str; 5] = ["normal", "test_fire", "testing", "hitl", "integrated_ground_test"];
const ROCKET_SENDERS: [&str; 4] = ["FC", "RF", "PB", "FC_SD"];

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct ModeSpan {
    pub from_ms: i64,
    pub mode: String,
    pub igt_active: bool,
    pub hitl_replay: bool,
    pub mock_wet_dress: bool,
    pub provenance: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct Body {
    pub key: String,
    pub label: String,
    pub senders: Vec<String>,
    pub simulated_types: Vec<String>,
    pub note: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct SeriesInfo {
    pub data_type: String,
    pub sender_id: String,
    pub rows: i64,
}

/// `GET /api/recordings/{id}/replay/context`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct ReplayContext {
    pub replay: bool,
    pub read_only: bool,
    pub recording: String,
    pub source_kind: String,
    pub capture_id: Option<String>,
    pub recorded_mode: String,
    pub mode_source: String,
    pub mode_timeline: Vec<ModeSpan>,
    pub provenance: String,
    pub simulated: bool,
    pub firmware_sim: bool,
    pub hitl: bool,
    pub hitl_replay: bool,
    pub igt: bool,
    pub test_fire: bool,
    pub default_view: String,
    pub view_options: Vec<String>,
    pub bodies: Vec<Body>,
    pub series: Vec<SeriesInfo>,
    pub sim_sample_rows: i64,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub upper_id: i64,
    pub compatibility: Value,
    pub board_sd: Vec<Value>,
    pub burn_report: Option<Value>,
    pub live_station_mode: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct Sample {
    pub id: i64,
    pub timestamp_ms: i64,
    pub sender_id: String,
    pub data_type: String,
    pub values: Vec<Option<f32>>,
    #[serde(default)]
    pub decoded: Option<Value>,
    #[serde(default)]
    pub valid_until_ms: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Page {
    next_id: i64,
    has_more: bool,
    rows: Vec<Sample>,
    #[serde(default)]
    seeds: Vec<Sample>,
}

pub(super) fn view_label(view: &str) -> &'static str {
    match view {
        "test_fire" => "Test fire",
        "testing" => "Testing",
        "hitl" => "HITL",
        "integrated_ground_test" => "Integrated ground test",
        "normal" => "Flight (normal)",
        _ => "Unknown mode",
    }
}

fn provenance_label(p: &str) -> &'static str {
    match p {
        "firmware_sim" => "simulated firmware",
        "internal_flight_sim" => "station flight simulator",
        "simulated" => "simulated",
        "hardware" => "hardware",
        _ => "not recorded",
    }
}

/// Replay ids for the composed sources the backend serves.
pub(super) fn range_id(start_ms: i64, end_ms: i64) -> String {
    format!("range.{}.{}", start_ms.max(0), end_ms.max(0))
}
pub(super) fn merge_sd_id(capture: &str) -> String {
    format!("merge-sd.{capture}")
}
pub(super) fn sd_only_id(capture: &str) -> String {
    format!("sd-only.{capture}")
}
pub(super) fn fire_id(fire: &str) -> String {
    format!("fire.{fire}")
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Banner {
    pub kind: &'static str,
    pub text: String,
}

/// What the replay announces, first to last. The REPLAY banner is always
/// first: nothing in this view is live.
pub(super) fn banners(ctx: &ReplayContext, view: &str) -> Vec<Banner> {
    let recorded = view_label(&ctx.recorded_mode);
    let mut out = vec![Banner {
        kind: "replay",
        text: format!(
            "REPLAY — saved data, not live. Recorded in {recorded} mode ({}). Read only: no command can be sent from a replay.",
            match ctx.mode_source.as_str() {
                "recording_rows" => "from the recording's station-mode rows",
                "manifest" => "from the capture manifest",
                "inferred" => "inferred from its contents; recorded before the mode was saved",
                _ => "mode unknown",
            }
        ),
    }];
    if ctx.igt || view == "integrated_ground_test" {
        out.push(Banner { kind: "igt", text: "INTEGRATED GROUND TEST — SIMULATED FLIGHT DATA (replayed). Stand (real) and Rocket flight (replayed) are shown separately.".into() });
    }
    if ctx.hitl {
        out.push(Banner {
            kind: "hitl",
            text: format!(
                "HITL — hardware in the loop, simulated flight{}.",
                if ctx.hitl_replay { " with the ground-test replay" } else { "" }
            ),
        });
    }
    match ctx.provenance.as_str() {
        "firmware_sim" => out.push(Banner { kind: "firmware_sim", text: "SIMULATED FIRMWARE — every board's real firmware ran on the station machine against a simulated plant. Not hardware data.".into() }),
        "internal_flight_sim" => out.push(Banner { kind: "flight_sim", text: "SIMULATED — recorded from the station's flight simulator. Not hardware data.".into() }),
        "simulated" => out.push(Banner { kind: "simulated", text: "SIMULATED — this fire archive entry is marked simulated.".into() }),
        _ => {}
    }
    if view != ctx.recorded_mode && ctx.recorded_mode != "unknown" {
        out.push(Banner { kind: "view", text: format!("Shown in the {} view; recorded in {recorded} mode.", view_label(view)) });
    }
    if ctx.mode_timeline.len() > 1 {
        let changes: Vec<String> = ctx.mode_timeline.iter().map(|s| format!("{} from {}", view_label(&s.mode), utc_label(s.from_ms))).collect();
        out.push(Banner { kind: "mode_change", text: format!("The station mode changed during this recording: {}.", changes.join(", ")) });
    }
    out
}

/// The tabs a replay in `view` shows (the backend's `tabs_for`).
pub(super) fn tabs_for(view: &str) -> Vec<(&'static str, &'static str)> {
    let keys: &[&str] = match view {
        "test_fire" => &["overview", "data", "gse", "thrust", "alerts"],
        "integrated_ground_test" => &["overview", "stand", "rocket", "data", "map", "estimator", "recovery", "thrust", "alerts"],
        "hitl" => &["overview", "data", "map", "estimator", "recovery", "hitl", "alerts"],
        _ => &["overview", "data", "map", "estimator", "recovery", "alerts"],
    };
    keys.iter().map(|k| (*k, tab_label(k))).collect()
}

fn tab_label(key: &str) -> &'static str {
    match key {
        "overview" => "Overview",
        "data" => "Data",
        "gse" => "Test stand (GSE)",
        "thrust" => "Thrust",
        "stand" => "Stand (real)",
        "rocket" => "Rocket flight (replayed)",
        "map" => "Map / GPS",
        "estimator" => "Estimator",
        "recovery" => "Recovery events",
        "hitl" => "HITL",
        "alerts" => "Recorded alerts",
        _ => "Other",
    }
}

fn canonical_sender(sender: &str) -> &str {
    sender.trim()
}
fn rocket_sender(sender: &str) -> bool {
    ROCKET_SENDERS.contains(&canonical_sender(sender))
}

/// How a series is labelled in the replay: which body it belongs to in an
/// integrated ground test, and whether it is simulated, from the board's SD
/// card or from simulated firmware.
pub(super) fn origin_label(ctx: &ReplayContext, view: &str, sender: &str, data_type: &str) -> String {
    let simulated_type = data_type.starts_with("SIM_")
        || ctx.bodies.iter().any(|b| b.simulated_types.iter().any(|t| t == data_type));
    let mut parts: Vec<&str> = Vec::new();
    if view == "integrated_ground_test" {
        if rocket_sender(sender) {
            parts.push(if simulated_type { "Rocket flight (replayed) · SIMULATED" } else { "Rocket (real)" });
        } else {
            parts.push("Stand (real)");
        }
    } else if simulated_type {
        parts.push("SIMULATED");
    }
    if ctx.firmware_sim {
        parts.push("SIMULATED FIRMWARE");
    } else if ctx.provenance == "internal_flight_sim" {
        parts.push("SIMULATED");
    } else if ctx.hitl && rocket_sender(sender) && view != "integrated_ground_test" {
        parts.push("HITL");
    }
    if sender.ends_with("_SD") {
        parts.push("board SD");
    }
    parts.dedup();
    parts.join(" · ")
}

fn is_gse(t: &str) -> bool {
    ["LOADCELL", "KG1000", "KG50", "PRESSURE", "COMBUSTION", "VALVE", "IGNITER", "FUEL", "UMBILICAL", "FILL", "DAQ_SD_"]
        .iter()
        .any(|k| t.contains(k))
        || t == "BURN_REPORT_THRUST"
}
fn is_estimator(t: &str) -> bool {
    ["BAROMETER", "ASCENT", "DESCENT", "ESTIMATOR", "EULER", "IMU", "ACCEL", "GYRO"].iter().any(|k| t.contains(k))
}
fn is_recovery(t: &str) -> bool {
    !t.starts_with("BOARD_RECOVERY")
        && (["FLIGHT_STATE", "ASCENT_STATE", "DESCENT_STATE", "LAUNCH_SITE", "ABORT"].contains(&t)
            || ["RECOVERY", "PYRO", "DEPLOY", "DROGUE", "MAIN_CHUTE"].iter().any(|k| t.contains(k)))
}
fn is_thrust(t: &str) -> bool {
    ["LOADCELL_WEIGHT_KG", "KG1000", "COMBUSTION_CHAMBER_PRESSURE", "BURN_REPORT_THRUST"].contains(&t)
}

/// The recorded series a tab shows.
pub(super) fn tab_series(ctx: &ReplayContext, tab: &str) -> Vec<(String, String)> {
    let stand = |s: &SeriesInfo| !rocket_sender(&s.sender_id);
    ctx.series
        .iter()
        .filter(|s| match tab {
            "data" => true,
            "gse" => is_gse(&s.data_type),
            "thrust" => is_thrust(&s.data_type),
            "stand" => stand(s) && s.data_type != "GS_STATION_MODE",
            "rocket" => rocket_sender(&s.sender_id),
            "map" => s.data_type.starts_with("GPS") || s.data_type == "LAUNCH_SITE",
            "estimator" => is_estimator(&s.data_type) || s.data_type == "SIM_SAMPLE",
            "recovery" => is_recovery(&s.data_type),
            "hitl" => s.data_type.starts_with("SIM_") || s.data_type.starts_with("GS_"),
            "overview" => s.data_type == "FLIGHT_STATE" || s.data_type.starts_with("GS_"),
            _ => false,
        })
        .map(|s| (s.data_type.clone(), s.sender_id.clone()))
        .collect()
}

/// Data types to fetch for `tab`, in window-API sized groups.
pub(super) fn fetch_groups(ctx: &ReplayContext, tab: &str) -> Vec<Vec<String>> {
    let mut types: Vec<String> = tab_series(ctx, tab).into_iter().map(|(t, _)| t).collect();
    types.sort();
    types.dedup();
    types.chunks(TYPES_PER_REQUEST).map(<[String]>::to_vec).collect()
}

/// Channel labels from the replay layout's data tabs: data type -> (tab
/// label, channel labels). The layout is the recorded mode's (or the picked
/// view's), so its labels are the ones that mode shows.
pub(super) fn layout_labels(layout: &LayoutConfig) -> BTreeMap<String, (String, Vec<String>)> {
    let mut out = BTreeMap::new();
    for tab in &layout.data_tab.tabs {
        out.insert(tab.id.clone(), (tab.label.clone(), tab.channels.clone()));
        for sub in tab.subtabs.iter().flatten() {
            if let (Some(ty), Some(channels)) = (&sub.data_type, &sub.channels) {
                out.entry(ty.clone()).or_insert_with(|| (sub.label.clone(), channels.clone()));
            }
        }
    }
    out
}

/// The latest sample of each series at `at_ms`, from a window's seeds and rows.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Frame {
    pub at_ms: i64,
    pub latest: BTreeMap<(String, String), Sample>,
}
pub(super) fn frame_at(seeds: &[Sample], rows: &[Sample], at_ms: i64) -> Frame {
    let mut latest: BTreeMap<(String, String), Sample> = BTreeMap::new();
    for s in seeds.iter().chain(rows.iter()).filter(|s| s.timestamp_ms <= at_ms) {
        let key = (s.data_type.clone(), s.sender_id.clone());
        if latest.get(&key).is_none_or(|old| (old.timestamp_ms, old.id) <= (s.timestamp_ms, s.id)) {
            latest.insert(key, s.clone());
        }
    }
    Frame { at_ms, latest }
}

fn value_text(v: &Option<f32>) -> String {
    match v {
        Some(v) if v.abs() >= 1000.0 => format!("{v:.1}"),
        Some(v) => format!("{v:.3}"),
        None => "—".into(),
    }
}

/// One line per series: label, values with channel labels, age.
fn series_lines(
    ctx: &ReplayContext,
    view: &str,
    series: &[(String, String)],
    frame: &Frame,
    labels: &BTreeMap<String, (String, Vec<String>)>,
) -> Vec<(String, String, String, String)> {
    series
        .iter()
        .map(|(ty, sender)| {
            let title = labels.get(ty).map(|(l, _)| l.clone()).unwrap_or_else(|| ty.clone());
            let origin = origin_label(ctx, view, sender, ty);
            let value = match frame.latest.get(&(ty.clone(), sender.clone())) {
                Some(s) => {
                    let channels = labels.get(ty).map(|(_, c)| c.as_slice()).unwrap_or(&[]);
                    let mut text: Vec<String> = s
                        .values
                        .iter()
                        .enumerate()
                        .take(12)
                        .map(|(i, v)| match channels.get(i) {
                            Some(c) => format!("{c} {}", value_text(v)),
                            None => value_text(v),
                        })
                        .collect();
                    if let Some(d) = &s.decoded {
                        text.push(format!("decoded v{}", d["version"]));
                    }
                    let age = frame.at_ms.saturating_sub(s.timestamp_ms);
                    format!("{} · {} ago", text.join(", "), duration_label(age))
                }
                None => "no sample yet".into(),
            };
            (format!("{title} [{sender}]"), origin, value, ty.clone())
        })
        .collect()
}

#[derive(Props, Clone, PartialEq)]
pub(super) struct StageProps {
    pub ctx: ReplayContext,
    pub view: String,
    pub tab: String,
    pub frame: Frame,
    #[props(default)]
    pub labels: BTreeMap<String, (String, Vec<String>)>,
    #[props(default)]
    pub overlay: Value,
    #[props(default)]
    pub alerts: Vec<Value>,
    pub theme: ThemeConfig,
}

/// The replay's banners, tab bar and the selected tab at the frame's time.
/// Pure: every value comes from props.
#[component]
pub(super) fn ReplayStage(props: StageProps) -> Element {
    let StageProps { ctx, view, tab, frame, labels, overlay, alerts, theme } = props;
    let banners = banners(&ctx, &view);
    let tabs = tabs_for(&view);
    let series = tab_series(&ctx, &tab);
    let lines = series_lines(&ctx, &view, &series, &frame, &labels);
    let body = |key: &str| ctx.bodies.iter().find(|b| b.key == key).cloned();
    rsx! {
        div { "data-replay-stage": "{view}", "data-recorded-mode": "{ctx.recorded_mode}",
            for b in banners.iter() {
                div { key: "{b.kind}", "data-replay-banner": "{b.kind}", role: "status",
                    style: if b.kind == "replay" { "background:repeating-linear-gradient(135deg,#78350f 0 12px,#92400e 12px 24px);color:#fffbeb;font-weight:800;padding:8px 10px;border-radius:6px;letter-spacing:.04em;" } else { "background:#3b0764;color:#f5d0fe;font-weight:700;padding:6px 10px;border-radius:6px;" },
                    "{b.text}"
                }
            }
            nav { role: "tablist", "aria-label": "Replay views", style: "display:flex;gap:6px;flex-wrap:wrap;margin:8px 0;",
                for (key, label) in tabs.iter() {
                    span { key: "{key}", role: "tab", "data-replay-tab": "{key}", "aria-selected": if *key == tab.as_str() { "true" } else { "false" },
                        style: "padding:4px 8px;border:1px solid {theme.border};border-radius:6px;", "{label}" }
                }
            }
            if tab == "overview" {
                dl { "data-replay-overview": "true",
                    dt { "Recording" } dd { "{ctx.recording} ({ctx.source_kind})" }
                    dt { "Recorded mode" } dd { "{view_label(&ctx.recorded_mode)}" }
                    dt { "Data source" } dd { "{provenance_label(&ctx.provenance)}" }
                    if let (Some(a), Some(b)) = (ctx.start_ms, ctx.end_ms) { dt { "Span" } dd { "{utc_label(a)} · {duration_label(b - a)}" } }
                    dt { "Live station" } dd { "{view_label(&ctx.live_station_mode)} — unchanged by this replay" }
                    if !ctx.board_sd.is_empty() { dt { "Board SD files" } dd { "{ctx.board_sd.len()} attached (replay with the merged or SD-only source)" } }
                    if let Some(c) = ctx.compatibility.get("loadcell_units").and_then(|v| v.as_str()) { dt { "Load cells" } dd { "{c}" } }
                }
            }
            if tab == "stand" || tab == "rocket" {
                if let Some(b) = body(&tab) {
                    section { "data-replay-body": "{b.key}",
                        h4 { "{b.label}" }
                        p { style: "color:{theme.text_muted};", "{b.note}" }
                    }
                }
            }
            if tab == "thrust" || tab == "gse" {
                ThrustOverlay { overlay: overlay.clone(), at_ms: frame.at_ms }
            }
            if tab == "hitl" {
                ul { "data-replay-hitl": "true",
                    for span in ctx.mode_timeline.iter() {
                        li { key: "{span.from_ms}", "{view_label(&span.mode)} from {utc_label(span.from_ms)}"
                            if span.hitl_replay { " · ground-test replay on" }
                            if span.mock_wet_dress { " · mock wet dress" } }
                    }
                }
            }
            if tab == "alerts" {
                if alerts.is_empty() { p { "No alerts were recorded." } }
                ul { "data-replay-alerts": "true",
                    for a in alerts.iter() {
                        li { key: "{a[\"timestamp_ms\"]}-{a[\"message\"]}", "data-recorded-alert": "true",
                            "RECORDED ({a[\"severity\"].as_str().unwrap_or(\"\")}, {utc_label(a[\"timestamp_ms\"].as_i64().unwrap_or(0))}): {a[\"message\"].as_str().unwrap_or(\"\")}" }
                    }
                }
            } else {
                table { "data-replay-series": "{tab}", style: "width:100%;border-collapse:collapse;",
                    for (title, origin, value, ty) in lines.iter() {
                        tr { key: "{title}", "data-series-type": "{ty}", "data-origin": "{origin}",
                            td { style: "padding:2px 6px;", "{title}" }
                            td { style: "padding:2px 6px;color:#e879f9;font-weight:700;", "{origin}" }
                            td { style: "padding:2px 6px;font-variant-numeric:tabular-nums;", "{value}" }
                        }
                    }
                }
                if lines.is_empty() { p { "No recorded series for this view." } }
            }
        }
    }
}

/// The burn report (or fire archive) thrust curve with the replay position.
#[component]
fn ThrustOverlay(overlay: Value, at_ms: i64) -> Element {
    let thrust = &overlay["thrust"];
    let points: Vec<(i64, f64)> = thrust["points"]
        .as_array()
        .map(|p| p.iter().filter_map(|x| Some((x[0].as_i64()?, x[1].as_f64()?))).collect())
        .unwrap_or_default();
    if points.len() < 2 {
        return rsx! { p { "data-thrust-overlay": "none", "No burn report thrust curve for this recording." } };
    }
    let (t0, t1) = (points[0].0, points[points.len() - 1].0.max(points[0].0 + 1));
    let peak = points.iter().map(|p| p.1).fold(1.0_f64, f64::max);
    let x = |t: i64| ((t - t0) as f64 / (t1 - t0) as f64 * 600.0).clamp(0.0, 600.0);
    let line: Vec<String> = points.iter().map(|(t, f)| format!("{:.1},{:.1}", x(*t), 150.0 - f / peak * 140.0)).collect();
    let cursor = x(at_ms);
    let source = thrust["source"].as_str().unwrap_or("").replace('_', " ");
    let simulated = thrust["simulated"].as_bool() == Some(true);
    rsx! {
        figure { "data-thrust-overlay": "{thrust[\"source\"].as_str().unwrap_or(\"\")}", style: "margin:8px 0;",
            svg { view_box: "0 0 600 150", width: "100%", height: "150", role: "img", "aria-label": "Thrust curve overlay",
                polyline { points: "{line.join(\" \")}", fill: "none", stroke: "#f59e0b", stroke_width: "2" }
                line { x1: "{cursor:.1}", x2: "{cursor:.1}", y1: "0", y2: "150", stroke: "#e2e8f0", stroke_dasharray: "4 3" }
            }
            figcaption { "Thrust from the {source}: peak {peak:.0} N" if simulated { " · SIMULATED" } " (overlay on the stand load cells; never part of the rocket's flight views)" }
        }
    }
}

async fn sleep_ms(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
}

fn encoded(value: &str) -> String {
    value
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Buffer {
    tab: String,
    from_ms: i64,
    to_ms: i64,
    rows: Vec<Sample>,
    seeds: Vec<Sample>,
}

async fn load_window(owner: std::rc::Rc<ExportOwner>, id: String, upper: i64, groups: Vec<Vec<String>>, from: i64, to: i64) -> Result<(Vec<Sample>, Vec<Sample>), String> {
    let (mut rows, mut seeds) = (Vec::new(), Vec::new());
    for types in groups {
        let types = encoded(&serde_json::to_string(&types).map_err(|e| e.to_string())?);
        let mut after = 0;
        loop {
            let page: Page = saved_json(
                &format!("/api/recordings/{}/replay/window?upper_id={upper}&after_id={after}&start_ms={from}&end_ms={to}&types={types}", encoded(&id)),
                owner.clone(),
            )
            .await?;
            if after == 0 {
                seeds.extend(page.seeds);
            }
            rows.extend(page.rows);
            if !page.has_more || page.next_id <= after {
                break;
            }
            after = page.next_id;
        }
    }
    Ok((rows, seeds))
}

/// The replay player: loads the recorded context, offers the view picker,
/// plays, pauses, seeks and changes speed. Never mounts live data.
#[component]
pub(super) fn SavedReplayView(recording: String, theme: ThemeConfig, on_close: EventHandler<()>) -> Element {
    let mut ctx = use_signal(|| None::<Result<ReplayContext, String>>);
    let mut view = use_signal(String::new);
    let mut labels = use_signal(BTreeMap::<String, (String, Vec<String>)>::new);
    let mut overlay = use_signal(|| Value::Null);
    let mut alerts = use_signal(Vec::<Value>::new);
    let mut tab = use_signal(|| "overview".to_string());
    let mut position = use_signal(|| 0i64);
    let mut playing = use_signal(|| false);
    let mut speed = use_signal(|| 1.0f64);
    let mut buffer = use_signal(|| None::<Buffer>);
    let mut loading = use_signal(|| false);
    let mut message = use_signal(String::new);
    let mut show_review = use_signal(|| false);
    let id = use_signal(|| recording.clone());
    if *id.peek() != recording {
        let mut id = id;
        id.set(recording.clone());
    }

    // Recorded context, overlay and alerts for this recording.
    use_effect(move || {
        let rec = id();
        ctx.set(None);
        buffer.set(None);
        position.set(0);
        playing.set(false);
        let owner = match ExportOwner::capture_selected(move || id.try_peek().is_ok_and(|r| *r == rec)) {
            Ok(owner) => owner,
            Err(e) => {
                ctx.set(Some(Err(e)));
                return;
            }
        };
        let rec = id.peek().clone();
        spawn(async move {
            let result = saved_json::<ReplayContext>(&format!("/api/recordings/{}/replay/context", encoded(&rec)), owner.clone()).await;
            if !owner.is_current() {
                return;
            }
            if let Ok(c) = &result {
                view.set(c.default_view.clone());
                tab.set("overview".into());
            }
            ctx.set(Some(result));
            if let Ok(v) = saved_json::<Value>(&format!("/api/recordings/{}/replay/overlays", encoded(&rec)), owner.clone()).await
                && owner.is_current()
            {
                overlay.set(v);
            }
            if let Ok(v) = saved_json::<Value>(&format!("/api/recordings/{}/replay/alerts", encoded(&rec)), owner.clone()).await
                && owner.is_current()
            {
                alerts.set(v["alerts"].as_array().cloned().unwrap_or_default());
            }
        });
    });

    // The layout of the chosen view (the recorded mode unless picked).
    use_effect(move || {
        let v = view();
        if v.is_empty() {
            return;
        }
        let rec = id.peek().clone();
        let Ok(owner) = ExportOwner::capture_selected(move || view.try_peek().is_ok_and(|now| *now == v)) else {
            return;
        };
        let v = view.peek().clone();
        spawn(async move {
            match saved_json::<LayoutConfig>(&format!("/api/recordings/{}/replay/layout?view={v}", encoded(&rec)), owner.clone()).await {
                Ok(layout) if owner.is_current() => labels.set(layout_labels(&layout)),
                Ok(_) => {}
                Err(e) => message.set(format!("Layout unavailable for this view: {e}")),
            }
        });
    });

    // Playback clock.
    use_future(move || async move {
        loop {
            sleep_ms(TICK_MS).await;
            if !*playing.peek() {
                continue;
            }
            let span = match ctx.peek().as_ref() {
                Some(Ok(c)) => match (c.start_ms, c.end_ms) {
                    (Some(a), Some(b)) => b - a,
                    _ => continue,
                },
                _ => continue,
            };
            let next = *position.peek() + (f64::from(TICK_MS) * *speed.peek()) as i64;
            if next >= span {
                position.set(span);
                playing.set(false);
            } else {
                position.set(next);
            }
        }
    });

    // Keep the sample window around the play position for the shown tab.
    use_effect(move || {
        let pos = position();
        let shown = tab();
        let Some(Ok(c)) = ctx.read().clone() else { return };
        let (Some(start), Some(end)) = (c.start_ms, c.end_ms) else { return };
        let at = start + pos;
        let covered = buffer.peek().as_ref().is_some_and(|b| b.tab == shown && at >= b.from_ms && at <= b.to_ms);
        if covered || *loading.peek() || shown == "alerts" {
            return;
        }
        let from = (at - 2_000).max(start);
        let to = (from + WINDOW_MS).min(end);
        let groups = fetch_groups(&c, &shown);
        let rec = id.peek().clone();
        let tab_now = shown.clone();
        let Ok(owner) = ExportOwner::capture_selected(move || tab.try_peek().is_ok_and(|t| *t == tab_now)) else { return };
        loading.set(true);
        spawn(async move {
            let result = load_window(owner.clone(), rec, c.upper_id, groups, from, to).await;
            loading.set(false);
            if !owner.is_current() {
                return;
            }
            match result {
                Ok((rows, seeds)) => buffer.set(Some(Buffer { tab: shown, from_ms: from, to_ms: to, rows, seeds })),
                Err(e) => {
                    playing.set(false);
                    message.set(format!("Replay samples unavailable: {e}"));
                }
            }
        });
    });

    let close = move |_| on_close.call(());
    let state = ctx.read().clone();
    let content = match state {
        None => rsx! { p { role: "status", "Loading the recorded context…" } },
        Some(Err(e)) => rsx! { p { role: "status", "Replay unavailable: {e}" } },
        Some(Ok(c)) => {
            let span = match (c.start_ms, c.end_ms) {
                (Some(a), Some(b)) => (b - a).max(0),
                _ => 0,
            };
            let at = c.start_ms.unwrap_or(0) + position();
            let frame = buffer.read().as_ref().map(|b| frame_at(&b.seeds, &b.rows, at)).unwrap_or(Frame { at_ms: at, latest: BTreeMap::new() });
            let current_view = view();
            let options: Vec<String> = if c.view_options.is_empty() { VIEWS.iter().map(|s| s.to_string()).collect() } else { c.view_options.clone() };
            rsx! {
                div { style: "display:flex;gap:8px;flex-wrap:wrap;align-items:center;margin:6px 0;", role: "group", "aria-label": "Replay controls",
                    button { disabled: span == 0, onclick: move |_| playing.set(!playing()), if playing() { "Pause" } else { "Play" } }
                    button { onclick: move |_| { playing.set(false); position.set(0); }, "Rewind" }
                    label { "Speed "
                        select { value: "{speed}", onchange: move |e| if let Ok(v) = e.value().parse::<f64>() { speed.set(v.clamp(0.25, 8.0)); },
                            for s in ["0.25", "0.5", "1", "2", "4", "8"] { option { key: "{s}", value: "{s}", "{s}×" } }
                        }
                    }
                    label { if c.hitl { "HITL view " } else { "View " }
                        select { "aria-label": "Replay view (changes only this replay, never the station mode)", value: "{current_view}",
                            onchange: move |e| { let v = e.value(); if VIEWS.contains(&v.as_str()) { view.set(v); tab.set("overview".into()); buffer.set(None); } },
                            for v in options.iter() { option { key: "{v}", value: "{v}", "{view_label(v)}" if *v == c.default_view { " (recorded)" } } }
                        }
                    }
                    span { style: "font-variant-numeric:tabular-nums;", "{utc_label(at)} · {duration_label(position())} / {duration_label(span)}" }
                    if loading() { span { role: "status", "Loading samples…" } }
                }
                input { r#type: "range", "aria-label": "Replay position", min: "0", max: "{span}", step: "100", value: "{position()}", style: "width:100%;",
                    oninput: move |e| if let Ok(v) = e.value().parse::<f64>() { position.set((v as i64).clamp(0, span)); } }
                nav { style: "display:flex;gap:6px;flex-wrap:wrap;margin:6px 0;",
                    for (key, label) in tabs_for(&current_view) {
                        button { key: "{key}", "aria-pressed": if tab() == key { "true" } else { "false" }, onclick: move |_| { tab.set(key.to_string()); }, "{label}" }
                    }
                }
                ReplayStage { ctx: c.clone(), view: current_view.clone(), tab: tab(), frame, labels: labels(), overlay: overlay(), alerts: alerts(), theme: theme.clone() }
            }
        }
    };
    rsx! {
        section { class: "gs26-saved-replay", "data-replay": "true", "aria-label": "Saved-data replay (not live)",
            style: "border:3px dashed #f59e0b;border-radius:10px;padding:10px;margin:8px 0;background:{theme.panel_background};color:{theme.text_primary};",
            div { style: "display:flex;justify-content:space-between;gap:8px;align-items:center;",
                h3 { style: "margin:0;", "Replay · {recording}" }
                button { onclick: close, "Close replay" }
            }
            {content}
            if !message.read().is_empty() { p { role: "status", "{message}" } }
            // The recording's computed motor values and flight summary (any
            // recorded mode), with its motor / flight exports.
            button { r#type: "button", "data-replay-review": "", "aria-expanded": show_review(), onclick: move |_| show_review.set(!show_review()),
                if show_review() { "Hide the motor & flight summary" } else { "Motor & flight summary / export" } }
            if show_review() {
                super::flight_review_view::RecordingReviewPanel { key: "{recording}", theme: theme.clone(), recording: recording.clone(), on_close: move |_| show_review.set(false) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(mode: &str, provenance: &str, extra: Value) -> ReplayContext {
        let mut v = serde_json::json!({
            "replay": true, "read_only": true, "recording": "event.manual.x.db", "source_kind": "capture",
            "recorded_mode": mode, "mode_source": "recording_rows", "provenance": provenance,
            "firmware_sim": provenance == "firmware_sim", "hitl": mode == "hitl", "igt": mode == "integrated_ground_test",
            "test_fire": mode == "test_fire", "default_view": mode, "live_station_mode": "normal",
            "start_ms": 1000, "end_ms": 9000, "upper_id": 9,
            "series": [
                {"data_type": "LOADCELL_WEIGHT_KG", "sender_id": "DAQ", "rows": 3},
                {"data_type": "LOADCELL_WEIGHT_KG", "sender_id": "DAQ_SD", "rows": 3},
                {"data_type": "COMBUSTION_CHAMBER_PRESSURE", "sender_id": "DAQ", "rows": 3},
                {"data_type": "BAROMETER_DATA", "sender_id": "FC", "rows": 3},
                {"data_type": "GPS_DATA", "sender_id": "FC", "rows": 3},
                {"data_type": "SIM_SAMPLE", "sender_id": "FC", "rows": 3},
                {"data_type": "FLIGHT_STATE", "sender_id": "FC", "rows": 3}
            ]
        });
        if mode == "integrated_ground_test" {
            v["bodies"] = serde_json::json!([
                {"key": "stand", "label": "Stand (real)", "senders": ["GS","DAQ","VB"], "simulated_types": [], "note": "stand"},
                {"key": "rocket", "label": "Rocket flight (replayed)", "senders": ["FC","RF","PB"], "simulated_types": ["SIM_SAMPLE"], "note": "rocket"}
            ]);
        }
        if let Value::Object(map) = extra {
            for (k, x) in map {
                v[k] = x;
            }
        }
        serde_json::from_value(v).unwrap()
    }

    fn sample(id: i64, ts: i64, sender: &str, ty: &str, values: &[f32]) -> Sample {
        Sample { id, timestamp_ms: ts, sender_id: sender.into(), data_type: ty.into(), values: values.iter().map(|v| Some(*v)).collect(), decoded: None, valid_until_ms: None }
    }

    fn stage(c: &ReplayContext, view: &str, tab: &str, frame: Frame, overlay: Value) -> String {
        let mut labels = BTreeMap::new();
        labels.insert("BAROMETER_DATA".to_string(), ("Barometer Data".to_string(), vec!["Altitude".into(), "Pressure".into(), "Temp".into()]));
        let props = StageProps { ctx: c.clone(), view: view.into(), tab: tab.into(), frame, labels, overlay, alerts: vec![serde_json::json!({"timestamp_ms": 2000, "severity": "critical", "message": "Chamber pressure high", "recorded": true})], theme: ThemeConfig::default() };
        let mut dom = VirtualDom::new_with_props(ReplayStage, props);
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn every_mode_opens_in_its_recorded_layout_under_a_replay_banner() {
        for (mode, provenance) in [("normal", "hardware"), ("test_fire", "hardware"), ("testing", "hardware"), ("hitl", "hardware"),
            ("integrated_ground_test", "hardware"), ("normal", "firmware_sim"), ("test_fire", "firmware_sim")] {
            let c = ctx(mode, provenance, Value::Null);
            let page = stage(&c, &c.default_view, "overview", Frame::default(), Value::Null);
            assert!(page.contains(&format!("data-replay-stage=\"{mode}\"")), "{mode}: {page}");
            assert!(page.contains("data-replay-banner=\"replay\""), "{mode}");
            assert!(page.contains("REPLAY — saved data, not live"), "{mode}");
            assert!(page.contains(&format!("Recorded in {} mode", view_label(mode))), "{mode}");
            assert!(page.contains("unchanged by this replay"));
            let first_banner = page.find("data-replay-banner=").unwrap();
            assert!(page[first_banner..].starts_with("data-replay-banner=\"replay\""), "REPLAY first");
            let tabs: Vec<&str> = tabs_for(mode).iter().map(|t| t.0).collect();
            for t in &tabs {
                assert!(page.contains(&format!("data-replay-tab=\"{t}\"")), "{mode} {t}");
            }
            match mode {
                "test_fire" => assert!(tabs.contains(&"gse") && tabs.contains(&"thrust") && !tabs.contains(&"map")),
                "integrated_ground_test" => {
                    assert!(page.contains("data-replay-banner=\"igt\"") && page.contains("SIMULATED FLIGHT DATA"));
                    assert!(tabs.contains(&"stand") && tabs.contains(&"rocket"));
                }
                "hitl" => assert!(page.contains("data-replay-banner=\"hitl\"") && tabs.contains(&"hitl")),
                _ => assert!(tabs.contains(&"map") && tabs.contains(&"estimator") && tabs.contains(&"recovery")),
            }
            assert_eq!(page.contains("data-replay-banner=\"firmware_sim\""), provenance == "firmware_sim", "{mode}");
            assert!(!page.contains("data-replay-banner=\"view\""), "recorded view: no override note");
        }
    }

    #[test]
    fn igt_replay_keeps_the_stand_and_the_replayed_rocket_apart() {
        let c = ctx("integrated_ground_test", "hardware", Value::Null);
        let rows = vec![
            sample(1, 2000, "DAQ", "LOADCELL_WEIGHT_KG", &[812.0]),
            sample(2, 2100, "FC", "SIM_SAMPLE", &[1200.0, 85.0]),
            sample(3, 2200, "FC", "BAROMETER_DATA", &[0.5, 98765.0, 21.5]),
        ];
        let frame = frame_at(&[], &rows, 3000);
        let stand = stage(&c, "integrated_ground_test", "stand", frame.clone(), Value::Null);
        assert!(stand.contains("data-replay-body=\"stand\"") && stand.contains("Stand (real)"), "{stand}");
        assert!(stand.contains("data-series-type=\"LOADCELL_WEIGHT_KG\""));
        assert!(!stand.contains("data-series-type=\"SIM_SAMPLE\""), "no rocket data on the stand");
        let rocket = stage(&c, "integrated_ground_test", "rocket", frame.clone(), Value::Null);
        assert!(rocket.contains("data-replay-body=\"rocket\"") && rocket.contains("Rocket flight (replayed)"));
        assert!(rocket.contains("data-origin=\"Rocket flight (replayed) · SIMULATED\""), "{rocket}");
        assert!(rocket.contains("data-origin=\"Rocket (real)\""), "at-rest sensors are real");
        assert!(rocket.contains("Altitude 0.500, Pressure 98765.0, Temp 21.500"), "barometer in FC order: {rocket}");
        assert!(!rocket.contains("LOADCELL_WEIGHT_KG"), "stand thrust never in the rocket's views");
        assert!(!tab_series(&c, "estimator").iter().any(|(t, _)| t.contains("LOADCELL")));
        assert!(!tab_series(&c, "recovery").iter().any(|(t, _)| t.contains("LOADCELL")));
    }

    #[test]
    fn test_fire_replay_shows_the_stand_with_its_thrust_overlay_and_sd_rows_labelled() {
        let c = ctx("test_fire", "hardware", Value::Null);
        let overlay = serde_json::json!({"thrust": {"source": "burn_report", "simulated": false, "points": [[1000, 0.0], [1500, 2000.0], [2000, 0.0]]}});
        let rows = vec![sample(1, 1500, "DAQ_SD", "LOADCELL_WEIGHT_KG", &[11.0]), sample(2, 1600, "DAQ", "LOADCELL_WEIGHT_KG", &[10.0])];
        let page = stage(&c, "test_fire", "gse", frame_at(&[], &rows, 1700), overlay.clone());
        assert!(page.contains("data-thrust-overlay=\"burn_report\"") && page.contains("peak 2000 N"), "{page}");
        assert!(page.contains("data-origin=\"board SD\""), "SD rows are their own series: {page}");
        assert!(page.contains("data-series-type=\"COMBUSTION_CHAMBER_PRESSURE\""));
        assert!(!page.contains("GPS_DATA"));
        let none = stage(&c, "test_fire", "thrust", Frame::default(), Value::Null);
        assert!(none.contains("data-thrust-overlay=\"none\""));
    }

    #[test]
    fn hitl_view_picker_redraws_only_the_replay_and_says_so() {
        let c = ctx("hitl", "hardware", serde_json::json!({"view_options": VIEWS}));
        for view in VIEWS {
            let page = stage(&c, view, "overview", Frame::default(), Value::Null);
            assert!(page.contains(&format!("data-replay-stage=\"{view}\"")));
            assert!(page.contains("data-replay-banner=\"hitl\""), "{view}: the HITL banner stays");
            assert_eq!(page.contains("data-replay-banner=\"view\""), view != "hitl", "{view}");
            assert!(page.contains("Live station") && page.contains("Flight (normal) — unchanged by this replay"));
        }
        let igt = stage(&c, "integrated_ground_test", "overview", Frame::default(), Value::Null);
        assert!(igt.contains("data-replay-tab=\"stand\"") && igt.contains("Shown in the Integrated ground test view; recorded in HITL mode."));
        let fire = stage(&c, "test_fire", "overview", Frame::default(), Value::Null);
        assert!(fire.contains("data-replay-tab=\"gse\""));
    }

    #[test]
    fn recorded_alerts_are_labelled_and_firmware_sim_series_marked() {
        let c = ctx("normal", "firmware_sim", Value::Null);
        let page = stage(&c, "normal", "alerts", Frame::default(), Value::Null);
        assert!(page.contains("data-recorded-alert=\"true\"") && page.contains("RECORDED (critical"), "{page}");
        assert!(!page.contains("role=\"alert\""), "recorded alerts never present as live alerts");
        let rows = vec![sample(1, 2000, "FC", "BAROMETER_DATA", &[1.0, 2.0, 3.0])];
        let est = stage(&c, "normal", "estimator", frame_at(&[], &rows, 2500), Value::Null);
        assert!(est.contains("data-origin=\"SIMULATED FIRMWARE\""), "{est}");
        let mode_change = ctx("normal", "hardware", serde_json::json!({"mode_timeline": [{"from_ms": 0, "mode": "normal"}, {"from_ms": 5000, "mode": "test_fire"}]}));
        assert!(banners(&mode_change, "normal").iter().any(|b| b.kind == "mode_change" && b.text.contains("Test fire from")));
    }

    #[test]
    fn frames_take_the_latest_sample_per_series_and_ids_name_composed_sources() {
        let seeds = vec![sample(1, 500, "DAQ", "KG50", &[1.0])];
        let rows = vec![sample(2, 1500, "DAQ", "KG50", &[2.0]), sample(3, 2500, "DAQ", "KG50", &[3.0])];
        assert_eq!(frame_at(&seeds, &rows, 1000).latest[&("KG50".into(), "DAQ".into())].values, vec![Some(1.0)]);
        assert_eq!(frame_at(&seeds, &rows, 2000).latest[&("KG50".into(), "DAQ".into())].values, vec![Some(2.0)]);
        assert_eq!(range_id(10, 20), "range.10.20");
        assert_eq!(merge_sd_id("event.launch.x.db"), "merge-sd.event.launch.x.db");
        assert_eq!(sd_only_id("event.launch.x.db"), "sd-only.event.launch.x.db");
        assert_eq!(fire_id("f1"), "fire.f1");
        let c = ctx("integrated_ground_test", "hardware", Value::Null);
        let groups = fetch_groups(&c, "data");
        assert!(groups.iter().all(|g| g.len() <= TYPES_PER_REQUEST));
        assert_eq!(groups.concat().len(), 6, "distinct types");
    }
}
