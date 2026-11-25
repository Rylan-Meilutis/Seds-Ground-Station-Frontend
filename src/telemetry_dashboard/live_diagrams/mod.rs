//! Live P&ID-style diagrams (docs/live-diagrams.md): one combined view per
//! station mode (flight, test fire, integrated ground test), chosen from the
//! station's layout (`live_diagram`), plus the system overview. Topologies are
//! declarative JSON (`topology.rs`), state is derived by pure rules
//! (`derive.rs`) and drawn as one SVG per diagram (`render.rs`) at 10 Hz. Each
//! view can be shown full screen.
pub(crate) mod derive;
pub(crate) mod render;
pub(crate) mod topology;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod render_tests;

use super::layout::ThemeConfig;
use super::types::{BoardStatusEntry, NetworkTopologyMsg, NetworkTopologyStatus};
use derive::{ControlView, Inputs, Observed, Value};
use dioxus::prelude::*;
use serde::Deserialize;
use topology::Topology;

const RENDER_INTERVAL_MS: u64 = 100;

/// Audience programme scenes (GroundStation `diagrams::is_scene`): the
/// station draws the same topology, delayed with the video.
/// `diagram-mode` follows the station mode's view; the older per-system
/// scenes (`diagram-fill`, `-av_bay`, `-stand`) still resolve on the station.
pub(crate) const PROGRAM_SCENES: &[(&str, &str)] = &[
    ("diagram-mode", "Live diagram: this station mode"),
    ("diagram-flight", "Live diagram: flight (fill + vehicle)"),
    ("diagram-test_fire", "Live diagram: test fire stand"),
    ("diagram-ground_test", "Live diagram: integrated ground test"),
    ("diagram-network", "Live diagram: system overview"),
];
const PROGRAM_KEY: &str = "gs26_live_diagram_program_v1";

async fn sleep_ms(ms: u64) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

/// `GET /api/diagrams`: the station's editable topologies.
#[derive(Deserialize, Default)]
struct DiagramsReply {
    #[serde(default)]
    diagrams: Vec<serde_json::Value>,
}

/// Station topologies that validate; any that do not keep the bundled copy.
fn merge_station(bundled: Vec<Topology>, reply: DiagramsReply) -> (Vec<Topology>, Vec<String>) {
    let mut out = bundled;
    let mut problems = Vec::new();
    for value in reply.diagrams {
        let parsed: Result<Topology, Vec<String>> = serde_json::from_value(value)
            .map_err(|e| vec![e.to_string()])
            .and_then(|t: Topology| topology::validate(&t).map(|()| t));
        match parsed {
            Ok(t) => match out.iter_mut().find(|o| o.id == t.id) {
                Some(slot) => *slot = t,
                None => out.push(t),
            },
            Err(errors) => problems.push(errors.join("; ")),
        }
    }
    (out, problems)
}

/// The live stores, read without subscribing (the 10 Hz tick drives renders).
pub(crate) struct LiveInputs {
    pub boards: Vec<BoardStatusEntry>,
    pub topology: NetworkTopologyMsg,
    pub flight_state: String,
    pub rssi: Option<String>,
    pub now_wall_ms: i64,
    pub now_mono_ms: i64,
}

fn fc_phase_label() -> Option<String> {
    let detail = super::mission_phase::FLIGHT_STATE_DETAIL.peek().clone();
    let label = detail.fc_phase.map(|p| p.label).filter(|l| !l.trim().is_empty())?;
    Some(if detail.simulated { format!("{label} (SIMULATED)") } else { label })
}

/// Drogue/main channel text from what the station actually knows: the FC's
/// phase (deployment), the network flight state (armed after Launch) and the
/// ground-test dry-run flags. Continuity is not reported by the FC.
pub(crate) fn pyro_text(main: bool, flight_state: &str, fc_phase: Option<&str>, dry_run: Option<bool>) -> String {
    let phase = fc_phase.unwrap_or("");
    let fired = if main {
        ["Reef", "Landed", "Recovery"].iter().any(|p| phase.contains(p))
    } else {
        ["Descent", "Reef", "Landed", "Recovery"].iter().any(|p| phase.contains(p))
    };
    let base = if fired {
        format!("Fired (FC phase {})", phase.trim_end_matches(" (SIMULATED)"))
    } else if matches!(flight_state, "Launch" | "Ascent" | "Coast" | "Apogee" | "Descent" | "InFlight" | "Reefing") {
        "Armed by the FC in flight".into()
    } else {
        "Safe (FC arms after Launch)".into()
    };
    match dry_run {
        Some(true) => format!("{base} · DRY-RUN"),
        _ => base,
    }
}

pub(crate) fn provenance_text() -> String {
    let current = super::station_mode_observer::OBSERVED
        .peek()
        .as_ref()
        .and_then(|o| o.value.current.clone());
    if current.as_ref().is_some_and(|c| c.firmware_sim) {
        "SIMULATED FIRMWARE".into()
    } else if current.as_ref().is_some_and(|c| c.simulation) {
        "SIMULATED".into()
    } else if super::igt::current_simulated_flight() {
        "IGT: FC flight data SIMULATED (replay)".into()
    } else if super::igt::current_active() {
        "IGT: live fill and stand".into()
    } else {
        "Live".into()
    }
}

impl Inputs for LiveInputs {
    fn telemetry(&self, data_type: &str, sender: Option<&str>, index: usize) -> Option<Observed> {
        let row = super::latest_telemetry_row(data_type, sender)?;
        let received = super::telemetry_row_received_ms(&row);
        let age_ms = Some(self.now_wall_ms.saturating_sub(received).max(0));
        if let Some(state) = row.over_range_at(index) {
            return Some(Observed { value: Value::OverRange(state), age_ms });
        }
        let value = row.values.get(index).copied().flatten()?;
        Some(Observed { value: Value::Num(value), age_ms })
    }
    fn control(&self, cmd: &str) -> Option<ControlView> {
        let policy = super::ACTION_POLICY_SIGNAL.peek();
        let control = policy.controls.iter().find(|c| c.cmd == cmd)?;
        Some(ControlView {
            actuated: control.actuated,
            requested: control.requested,
            running: control.progress.as_ref().is_some_and(|p| {
                matches!(p.state, super::ControlProgressState::Starting | super::ControlProgressState::Running)
            }),
        })
    }
    fn board(&self, sender: &str) -> Option<Observed> {
        let entry = self.boards.iter().find(|b| b.sender_id == sender || b.board == sender)?;
        if !entry.seen && entry.last_seen_ms.is_none() {
            return None;
        }
        Some(Observed {
            value: Value::Bool(entry.seen),
            age_ms: entry.current_age_ms(self.now_mono_ms).map(|a| a as i64),
        })
    }
    fn status(&self, key: &str) -> Option<Observed> {
        let text = |t: String| Some(Observed { value: Value::Text(t), age_ms: None });
        if let Some(sender) = key.strip_prefix("topology:") {
            let node = self.topology.nodes.iter().find(|n| n.sender_id.as_deref() == Some(sender))?;
            return Some(Observed {
                value: Value::Bool(node.status != NetworkTopologyStatus::Offline),
                age_ms: node.current_age_ms(self.now_mono_ms).map(|a| a as i64),
            });
        }
        let igt = super::igt::IGT_STATUS.peek().clone();
        match key {
            "flight_state" => text(super::types::display_flight_state(&self.flight_state)),
            "fill_phase" => {
                let detail = super::mission_phase::FLIGHT_STATE_DETAIL.peek().clone();
                detail
                    .fill_phase_label
                    .filter(|l| !l.trim().is_empty())
                    .or_else(|| detail.fill_phase.filter(|p| !p.is_empty()).map(|p| super::types::display_flight_state(&p)))
                    .and_then(text)
            }
            "fc_phase" => fc_phase_label().and_then(text),
            "estimator" => super::fc_estimator_notice::FC_ESTIMATOR
                .peek()
                .as_ref()
                .map(|e| if e.label.is_empty() { e.state.clone() } else { e.label.trim_start_matches("FC estimator: ").to_string() })
                .filter(|l| !l.is_empty())
                .and_then(text),
            "estimator_rate_hz" => super::fc_estimator_notice::FC_ESTIMATOR
                .peek()
                .as_ref()
                .and_then(|e| e.rate_hz)
                .map(|r| format!("{r:.0} cycles/s"))
                .and_then(text),
            "igt_mode" => igt
                .filter(|_| super::igt::current_active())
                .map(|s| {
                    let pyro = if s.live_pyro { "LIVE PYRO" } else { "pyro DRY-RUN" };
                    format!("{} · {pyro}", s.mode_label())
                })
                .and_then(text),
            "pyro_drogue" | "pyro_main" => {
                let main = key == "pyro_main";
                let dry = igt
                    .filter(|s| s.supported && super::igt::current_active())
                    .map(|s| if main { s.main_dry_run } else { s.drogue_dry_run });
                text(pyro_text(main, &self.flight_state, fc_phase_label().as_deref(), dry))
            }
            "pyro_continuity" => None,
            "fc_sd" => igt.and_then(|s| {
                s.sd_ready.map(|ready| match (ready, s.sd_retry_pending) {
                    (true, _) => "Ready".to_string(),
                    (false, true) => "Not ready (retry pending)".into(),
                    (false, false) => "Not ready".into(),
                })
            }).and_then(text),
            "radio_rssi" => self.rssi.clone().and_then(text),
            "provenance" => text(provenance_text()),
            _ => None,
        }
    }
    fn igt(&self) -> bool {
        super::igt::current_active()
    }
}

#[derive(Deserialize)]
struct AntennaRssi {
    #[serde(default)]
    rssi: Option<RssiValue>,
}
#[derive(Deserialize)]
struct RssiValue {
    value: f64,
    unit: String,
    #[serde(default)]
    source: String,
}

/// The view the station mode shows: the layout's `live_diagram` (the station
/// resolves it per mode), else, for a layout without one (an older station or
/// replay bundle), from the mode flags: test fire, integrated ground test or
/// flight (normal, HITL, testing).
pub(crate) fn mode_view(layout_view: Option<&str>, test_fire: bool, igt: bool) -> String {
    match layout_view.map(str::trim).filter(|v| !v.is_empty()) {
        Some(view) => legacy_view(view).unwrap_or(view).to_string(),
        None if igt => "ground_test".into(),
        None if test_fire => "test_fire".into(),
        None => "flight".into(),
    }
}

/// The per-system views the mode views replaced.
pub(crate) fn legacy_view(id: &str) -> Option<&'static str> {
    match id {
        "fill" | "av_bay" => Some("flight"),
        "stand" => Some("ground_test"),
        _ => None,
    }
}

/// The topology to draw: the operator's pick if it exists, else the mode's
/// view, else the first available.
fn pick<'a>(list: &'a [Topology], chosen: Option<&str>, mode: &str) -> Option<&'a Topology> {
    let find = |id: &str| list.iter().find(|t| t.id == id);
    chosen.and_then(find).or_else(|| find(mode)).or_else(|| list.first())
}

/// Tab order: the mode's view first, then the other mode views, then the
/// rest (the system overview, and any per-system view an older bundle serves).
fn ordered(list: &[Topology], mode: &str) -> Vec<Topology> {
    let rank = |t: &Topology| match t.id.as_str() {
        id if id == mode => 0,
        id if topology::BUNDLED.iter().any(|(b, _)| *b == id) => 1,
        _ => 2,
    };
    let mut out = list.to_vec();
    out.sort_by_key(|t| (rank(t), topology::BUNDLED.iter().position(|(b, _)| *b == t.id).unwrap_or(usize::MAX)));
    out
}

fn persist_get(key: &str) -> Option<String> {
    super::persist::get_string(key)
}

pub(crate) const PAGE_CSS: &str = r#"
.gs26-pid-page { height:100%; min-height:0; display:flex; flex-direction:column; gap:8px; padding:10px 12px; box-sizing:border-box; overflow:auto; }
.gs26-pid-toolbar { display:flex; flex-wrap:wrap; gap:8px; align-items:center; }
.gs26-pid-toolbar button { font:inherit; min-height:40px; padding:6px 12px; border-radius:10px; border:1px solid var(--pid-border); background:transparent; color:var(--pid-text); cursor:pointer; }
.gs26-pid-toolbar button[aria-pressed=true] { background:var(--pid-surface-alt); border-color:var(--pid-text); font-weight:650; }
.gs26-pid-toolbar .gs26-pid-note { font-size:12px; color:var(--pid-muted); }
.gs26-pid-wrap { flex:1 1 auto; min-height:260px; display:flex; flex-direction:column; border:1px solid var(--pid-border); border-radius:14px; background:var(--pid-surface); padding:8px; box-sizing:border-box; }
.gs26-pid-wrap[data-program=true] { aspect-ratio:16/9; flex:0 0 auto; width:min(100%, calc((100vh - 140px) * 16 / 9)); margin:0 auto; padding:16px 20px; }
.gs26-pid-title { display:flex; gap:10px; align-items:baseline; flex-wrap:wrap; color:var(--pid-text); }
.gs26-pid-title strong { font-size:18px; }
.gs26-pid-wrap[data-program=true] .gs26-pid-title strong { font-size:26px; }
.gs26-pid-badge { font-size:12px; font-weight:700; letter-spacing:.05em; padding:2px 8px; border-radius:999px; border:1px solid currentColor; }
.gs26-pid-badge[data-sim=true] { color:#d03b3b; }
.gs26-pid-desc { font-size:12px; color:var(--pid-muted); margin:2px 0 6px; }
.gs26-pid-tag { font-size:11px; font-weight:600; color:var(--pid-muted); margin-left:6px; }
.gs26-pid-full-btn { margin-left:auto; font:inherit; font-size:13px; min-height:36px; padding:4px 12px; border-radius:10px; border:1px solid var(--pid-border); background:var(--pid-surface-alt); color:var(--pid-text); cursor:pointer; }
.gs26-pid-svg { flex:1 1 auto; min-height:0; display:flex; align-items:center; justify-content:center; }
.gs26-pid-wrap[data-program=false][data-full=false] { flex:0 0 auto; }
.gs26-pid-wrap[data-program=false][data-full=false] .gs26-pid-svg > .gs26-pid { max-height:calc(100vh - 96px); }
.gs26-pid-wrap[data-full=true] { position:fixed; inset:0; z-index:2147483000; width:auto; height:auto; max-width:none; aspect-ratio:auto; margin:0; border:0; border-radius:0; box-sizing:border-box;
  padding:max(10px, env(safe-area-inset-top)) max(12px, env(safe-area-inset-right)) max(10px, env(safe-area-inset-bottom)) max(12px, env(safe-area-inset-left)); overflow:hidden; }
.gs26-pid-wrap[data-full=true] .gs26-pid-svg > .gs26-pid { width:100%; height:100%; max-height:none; }
.gs26-pid-wrap[data-full=true] .gs26-pid-desc { display:none; }
.gs26-pid-wrap:fullscreen { width:100vw; height:100vh; }
.gs26-pid-legend { display:flex; flex-wrap:wrap; gap:6px 14px; font-size:12px; color:var(--pid-text); padding-top:6px; }
.gs26-pid-legend-item { display:inline-flex; align-items:center; gap:6px; }
.gs26-pid-dot { display:inline-block; width:12px; height:12px; border-radius:3px; box-sizing:border-box; }
@media (max-width:640px) { .gs26-pid-page { padding:6px; } .gs26-pid-desc { display:none; } .gs26-pid-wrap[data-full=true] .gs26-pid-legend { display:none; } }
"#;

/// The panel shown full screen (`FULLSCREEN_JS`).
const PANEL_ID: &str = "gs26-pid-panel";

/// Full screen: the Fullscreen API where the platform has it for elements
/// (browsers, WebView2, WebKitGTK; prefixed on Safari), and in any case the
/// panel covers the viewport (`data-full`), which is all an iPhone, or a
/// webview that refuses the API, gets. Esc, the platform's own exit and the
/// panel's button all leave it; the script reports `exit` once.
const FULLSCREEN_JS: &str = r#"
const el = document.getElementById('gs26-pid-panel');
let done = false;
const current = () => document.fullscreenElement || document.webkitFullscreenElement || null;
const finish = () => {
  if (done) return; done = true;
  document.removeEventListener('fullscreenchange', onChange);
  document.removeEventListener('webkitfullscreenchange', onChange);
  document.removeEventListener('keydown', onKey, true);
  if (current()) { try { (document.exitFullscreen || document.webkitExitFullscreen).call(document)?.catch?.(() => {}); } catch (_) {} }
  delete window.__gs26PidExit;
  dioxus.send('exit');
};
const onChange = () => { if (!current()) finish(); };
const onKey = (e) => { if (e.key === 'Escape' || e.key === 'Esc') { e.preventDefault(); finish(); } };
window.__gs26PidExit?.();
window.__gs26PidExit = finish;
done = false;
document.addEventListener('fullscreenchange', onChange);
document.addEventListener('webkitfullscreenchange', onChange);
document.addEventListener('keydown', onKey, true);
const request = el && (el.requestFullscreen || el.webkitRequestFullscreen);
let native = false;
if (request) {
  try { const p = request.call(el); native = true; p?.catch?.(() => { native = false; }); } catch (_) { native = false; }
}
dioxus.send(native ? 'native' : 'overlay');
el?.focus?.({ preventScroll: true });
"#;

/// Full-screen state of a diagram view and the handlers that change it.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Fullscreen {
    pub on: Signal<bool>,
}

impl Fullscreen {
    pub(crate) fn enter(mut self) {
        if *self.on.peek() {
            return;
        }
        self.on.set(true);
        let mut on = self.on;
        spawn(async move {
            let mut script = document::eval(FULLSCREEN_JS);
            while let Ok(message) = script.recv::<String>().await {
                if message == "exit" {
                    break;
                }
            }
            on.set(false);
        });
    }
    pub(crate) fn exit(mut self) {
        document::eval("window.__gs26PidExit?.()");
        self.on.set(false);
    }
}

/// The diagram with its title, provenance badge, full-screen button and
/// legend; shared by the dashboard tab and the broadcast-size view.
#[component]
pub(crate) fn DiagramPanel(
    topology: Topology,
    state: derive::DiagramState,
    program: bool,
    provenance: String,
    #[props(default)] fullscreen: Option<Fullscreen>,
    #[props(default)] mode_view: bool,
) -> Element {
    let simulated = provenance != "Live";
    let full = fullscreen.is_some_and(|f| *f.on.read());
    rsx! {
        div { class: "gs26-pid-wrap", id: PANEL_ID, tabindex: "-1", "data-program": program.to_string(), "data-full": full.to_string(),
            "data-diagram-panel": topology.id.clone(), role: if full { "dialog" } else { "group" },
            "aria-label": "{topology.title}", "aria-modal": if full { "true" } else { "false" },
            onkeydown: move |e: KeyboardEvent| {
                if e.key() == Key::Escape && let Some(f) = fullscreen { f.exit(); }
            },
            div { class: "gs26-pid-title",
                strong { "{topology.title}" }
                if mode_view { span { class: "gs26-pid-tag", "station mode view" } }
                span { class: "gs26-pid-badge", "data-sim": simulated.to_string(), "{provenance}" }
                if let Some(f) = fullscreen {
                    if full {
                        button { class: "gs26-pid-full-btn", r#type: "button", "data-fullscreen": "exit",
                            title: "Leave full screen (Esc)", onclick: move |_| f.exit(), "Exit full screen" }
                    } else {
                        button { class: "gs26-pid-full-btn", r#type: "button", "data-fullscreen": "enter",
                            title: "Show this diagram full screen (Esc leaves)", onclick: move |_| f.enter(), "Full screen" }
                    }
                }
            }
            if !program { p { class: "gs26-pid-desc", "{topology.description}" } }
            div { class: "gs26-pid-svg", render::DiagramSvg { topology: topology.clone(), state } }
            render::Legend { topology: topology.clone() }
        }
    }
}

#[component]
pub(crate) fn LiveDiagramsTab(
    theme: ThemeConfig,
    board_status: Signal<Vec<BoardStatusEntry>>,
    network_topology: Signal<NetworkTopologyMsg>,
    flight_state: Signal<String>,
    /// The layout's `live_diagram` (the station's view for its mode).
    #[props(default)]
    layout_view: Option<String>,
    #[props(default)]
    test_fire_mode: bool,
) -> Element {
    super::vehicle_geometry::use_station_geometry();
    let mut topologies = use_signal(topology::bundled);
    let mut source_note = use_signal(|| "Client copy".to_string());
    // The operator's pick, with the mode view it was made in: a change of
    // station mode returns to the new mode's view.
    let mut chosen = use_signal(|| None::<(String, String)>);
    let fullscreen = Fullscreen { on: use_signal(|| false) };
    let mode = mode_view(layout_view.as_deref(), test_fire_mode, super::igt::current_active());
    let mut program = use_signal(|| persist_get(PROGRAM_KEY).as_deref() == Some("1"));
    let mut tick = use_signal(|| 0u64);
    let mut rssi = use_signal(|| None::<String>);
    use_future(move || async move {
        match super::http_get_json::<DiagramsReply>("/api/diagrams").await {
            Ok(reply) => {
                let (merged, problems) = merge_station(topology::bundled(), reply);
                source_note.set(if problems.is_empty() {
                    "Station topology".into()
                } else {
                    format!("Station topology ({} invalid file(s) ignored)", problems.len())
                });
                topologies.set(merged);
            }
            Err(_) => source_note.set("Client copy (station serves no topology)".into()),
        }
    });
    use_future(move || async move {
        loop {
            sleep_ms(RENDER_INTERVAL_MS).await;
            if super::dashboard_page_visible() {
                let next = tick.peek().wrapping_add(1);
                tick.set(next);
            }
        }
    });
    use_future(move || async move {
        loop {
            if let Ok(reply) = super::http_get_json::<AntennaRssi>("/api/antenna/status").await {
                rssi.set(reply.rssi.map(|r| {
                    if r.source.is_empty() { format!("{:.1} {}", r.value, r.unit) } else { format!("{:.1} {} ({})", r.value, r.unit, r.source) }
                }));
            }
            sleep_ms(2_000).await;
        }
    });
    let _ = *tick.read();
    let list = ordered(&topologies.read(), &mode);
    let pick_now = chosen.read().as_ref().filter(|(m, _)| *m == mode).map(|(_, id)| id.clone());
    let Some(current) = pick(&list, pick_now.as_deref(), &mode).cloned() else {
        return rsx! { p { role: "status", "No diagram topology is available." } };
    };
    let inputs = LiveInputs {
        boards: board_status.peek().clone(),
        topology: network_topology.peek().clone(),
        flight_state: flight_state.peek().clone(),
        rssi: rssi.peek().clone(),
        now_wall_ms: super::current_wallclock_ms(),
        now_mono_ms: super::monotonic_now_ms() as i64,
    };
    let state = derive::derive(&current, &inputs);
    let vars = render::theme_vars(&theme);
    let is_program = *program.read();
    rsx! {
        style { {render::CSS} {PAGE_CSS} }
        div { class: "gs26-pid-page", style: "{vars}color:{theme.text_primary};",
            div { class: "gs26-pid-toolbar", role: "toolbar", "aria-label": "Diagram",
                for t in list.iter() {
                    button { key: "pick-{t.id}", r#type: "button", "aria-pressed": (t.id == current.id).to_string(), "data-view": t.id.clone(),
                        title: if t.id == mode { "The view for the station's current mode" } else { "Show this view instead of the mode's" },
                        onclick: { let id = t.id.clone(); let mode = mode.clone(); move |_| chosen.set((id != mode).then(|| (mode.clone(), id.clone()))) },
                        "{t.title}"
                        if t.id == mode { span { class: "gs26-pid-tag", "mode" } }
                    }
                }
                button { r#type: "button", "aria-pressed": is_program.to_string(),
                    title: "A 16:9 frame with larger text for the audience programme, OBS window capture or a wall display",
                    onclick: move |_| { let next = !*program.peek(); super::persist::set_string(PROGRAM_KEY, if next { "1" } else { "0" }); program.set(next); },
                    "Broadcast size"
                }
                span { class: "gs26-pid-note", "{source_note} · updates 10×/s" }
            }
            DiagramPanel { mode_view: current.id == mode, topology: current, state, program: is_program, provenance: provenance_text(), fullscreen }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pyro_text_reports_only_what_the_station_knows() {
        assert_eq!(pyro_text(false, "PadIdle", None, None), "Safe (FC arms after Launch)");
        assert_eq!(pyro_text(false, "InFlight", Some("Coast"), None), "Armed by the FC in flight");
        assert_eq!(pyro_text(false, "InFlight", Some("Descent"), None), "Fired (FC phase Descent)");
        assert_eq!(pyro_text(true, "InFlight", Some("Descent"), None), "Armed by the FC in flight");
        assert_eq!(pyro_text(true, "InFlight", Some("Reefing (SIMULATED)"), Some(true)), "Fired (FC phase Reefing) · DRY-RUN");
    }

    #[test]
    fn station_topologies_replace_bundled_ones_and_invalid_files_are_ignored() {
        let mut fill: serde_json::Value = serde_json::from_str(topology::BUNDLED[0].1).unwrap();
        fill["title"] = "Flight (station edit)".into();
        let mut broken = fill.clone();
        broken["id"] = "broken".into();
        broken["edges"][0]["to"] = "nowhere".into();
        let (merged, problems) = merge_station(topology::bundled(), DiagramsReply { diagrams: vec![fill, broken] });
        assert_eq!(merged.iter().find(|t| t.id == "flight").unwrap().title, "Flight (station edit)");
        assert!(merged.iter().all(|t| t.id != "broken"));
        assert_eq!(problems.len(), 1);
        assert_eq!(merged.len(), topology::BUNDLED.len());
    }

    #[test]
    fn each_station_mode_gets_its_view_and_old_layouts_fall_back_to_the_mode_flags() {
        // The station's layout names the view (GroundStation `live_diagram`).
        assert_eq!(mode_view(Some("test_fire"), true, false), "test_fire");
        assert_eq!(mode_view(Some("flight"), false, true), "flight", "the layout wins");
        // A layout without it (an older station or replay bundle).
        assert_eq!(mode_view(None, false, false), "flight");
        assert_eq!(mode_view(Some(" "), true, false), "test_fire");
        assert_eq!(mode_view(None, false, true), "ground_test");
        // A layout naming a per-system view of the old set.
        assert_eq!(mode_view(Some("stand"), true, false), "ground_test");
        assert_eq!(mode_view(Some("fill"), false, false), "flight");
        for (_, id) in [("normal", "flight"), ("test_fire", "test_fire"), ("igt", "ground_test")] {
            assert!(topology::BUNDLED.iter().any(|(b, _)| *b == id), "{id} is bundled");
        }
    }

    #[test]
    fn the_mode_view_comes_first_and_an_operator_pick_wins_while_it_exists() {
        let mut list = topology::bundled();
        // An old replay bundle also serves the per-system views.
        let mut legacy = list[0].clone();
        legacy.id = "fill".into();
        list.push(legacy);
        let order: Vec<String> = ordered(&list, "test_fire").into_iter().map(|t| t.id).collect();
        assert_eq!(order, ["test_fire", "flight", "ground_test", "network", "fill"]);
        assert_eq!(pick(&list, None, "flight").unwrap().id, "flight");
        assert_eq!(pick(&list, Some("network"), "flight").unwrap().id, "network");
        assert_eq!(pick(&list, Some("gone"), "test_fire").unwrap().id, "test_fire");
        assert_eq!(pick(&list, None, "missing").unwrap().id, "flight", "first available");
    }
}
