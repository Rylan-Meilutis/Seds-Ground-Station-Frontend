//! Live P&ID-style diagrams (docs/live-diagrams.md): fill system, AV bay in
//! flight, system overview and test stand. Topologies are declarative JSON
//! (`topology.rs`), state is derived by pure rules (`derive.rs`) and drawn as
//! one SVG per diagram (`render.rs`) at 10 Hz.
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
pub(crate) const PROGRAM_SCENES: &[(&str, &str)] = &[
    ("diagram-fill", "Live diagram: fill system"),
    ("diagram-av_bay", "Live diagram: AV bay"),
    ("diagram-network", "Live diagram: system overview"),
    ("diagram-stand", "Live diagram: test stand"),
];
const SELECTED_KEY: &str = "gs26_live_diagram_selected_v1";
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
/// The bundled topologies with a station's `GET /api/diagrams` applied (the
/// offline workspace passes a replay bundle's copy).
pub(crate) fn with_station_diagrams(diagrams: Vec<serde_json::Value>) -> Vec<Topology> {
    merge_station(topology::bundled(), DiagramsReply { diagrams }).0
}

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

fn default_selection(topologies: &[Topology]) -> String {
    let stand = super::igt::current_active();
    persist_get(SELECTED_KEY)
        .filter(|id| topologies.iter().any(|t| &t.id == id))
        .unwrap_or_else(|| if stand { "stand".into() } else { "fill".into() })
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
.gs26-pid-svg { flex:1 1 auto; min-height:0; display:flex; align-items:center; justify-content:center; }
.gs26-pid-legend { display:flex; flex-wrap:wrap; gap:6px 14px; font-size:12px; color:var(--pid-text); padding-top:6px; }
.gs26-pid-legend-item { display:inline-flex; align-items:center; gap:6px; }
.gs26-pid-dot { display:inline-block; width:12px; height:12px; border-radius:3px; box-sizing:border-box; }
@media (max-width:640px) { .gs26-pid-page { padding:6px; } .gs26-pid-desc { display:none; } }
"#;

/// The diagram with its title, provenance badge and legend; shared by the
/// dashboard tab and the broadcast-size view.
#[component]
pub(crate) fn DiagramPanel(topology: Topology, state: derive::DiagramState, program: bool, provenance: String) -> Element {
    let simulated = provenance != "Live";
    rsx! {
        div { class: "gs26-pid-wrap", "data-program": program.to_string(), "data-diagram-panel": topology.id.clone(),
            div { class: "gs26-pid-title",
                strong { "{topology.title}" }
                span { class: "gs26-pid-badge", "data-sim": simulated.to_string(), "{provenance}" }
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
) -> Element {
    super::vehicle_geometry::use_station_geometry();
    let mut topologies = use_signal(topology::bundled);
    let mut source_note = use_signal(|| "Client copy".to_string());
    let mut selected = use_signal(|| default_selection(&topology::bundled()));
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
    let list = topologies.read().clone();
    let current_id = selected.read().clone();
    let Some(current) = list.iter().find(|t| t.id == current_id).or_else(|| list.first()).cloned() else {
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
                    button { key: "pick-{t.id}", r#type: "button", "aria-pressed": (t.id == current.id).to_string(),
                        onclick: { let id = t.id.clone(); move |_| { super::persist::set_string(SELECTED_KEY, &id); selected.set(id.clone()); } },
                        "{t.title}"
                    }
                }
                button { r#type: "button", "aria-pressed": is_program.to_string(),
                    title: "A 16:9 frame with larger text for the audience programme, OBS window capture or a wall display",
                    onclick: move |_| { let next = !*program.peek(); super::persist::set_string(PROGRAM_KEY, if next { "1" } else { "0" }); program.set(next); },
                    "Broadcast size"
                }
                span { class: "gs26-pid-note", "{source_note} · updates 10×/s" }
            }
            DiagramPanel { topology: current, state, program: is_program, provenance: provenance_text() }
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
        fill["title"] = "Fill (station edit)".into();
        let mut broken = fill.clone();
        broken["id"] = "broken".into();
        broken["edges"][0]["to"] = "nowhere".into();
        let (merged, problems) = merge_station(topology::bundled(), DiagramsReply { diagrams: vec![fill, broken] });
        assert_eq!(merged.iter().find(|t| t.id == "fill").unwrap().title, "Fill (station edit)");
        assert!(merged.iter().all(|t| t.id != "broken"));
        assert_eq!(problems.len(), 1);
        assert_eq!(merged.len(), topology::BUNDLED.len());
    }
}
