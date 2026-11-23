//! SVG rendering of a topology and its derived state. The element tree is
//! fixed by the topology (stable keys), so a 10 Hz update only changes
//! attributes and text; flow animation is a CSS dash animation, not a JS
//! loop, and stops under `prefers-reduced-motion`.
use super::derive::{DiagramState, EdgeState, Flow, Link, ValveState, format_age, visible};
use super::topology::{Anchor, Edge, EdgeKind, Node, NodeKind, Reading, Topology, Valve, ValveKind};
use crate::telemetry_dashboard::layout::ThemeConfig;
use dioxus::prelude::*;

/// Fluid colours: the first three categorical slots of the dashboard's
/// validated palette (CVD-separated as a set, both modes); gas lines are
/// also dashed so colour is never the only cue.
const FLUID_LIGHT: [&str; 3] = ["#2a78d6", "#eb6834", "#1baf7a"];
const FLUID_DARK: [&str; 3] = ["#3987e5", "#d95926", "#199e70"];
const GOOD: &str = "#0ca30c";
const WARNING: &str = "#fab219";
const CRITICAL: &str = "#d03b3b";

pub(crate) const CSS: &str = r#"
.gs26-pid { width:100%; height:auto; max-height:100%; display:block; font-family:inherit; user-select:none; }
.gs26-pid text { fill:var(--pid-text); }
.gs26-pid .pid-frame { fill:none; stroke:var(--pid-border); stroke-dasharray:6 6; }
.gs26-pid .pid-frame-label { font-size:13px; fill:var(--pid-muted); letter-spacing:.04em; text-transform:uppercase; }
.gs26-pid .pid-edge { fill:none; stroke-width:6; stroke-linecap:round; stroke-linejoin:round; transition:stroke .3s, opacity .3s; }
.gs26-pid .pid-edge.flow-blocked { opacity:.28; }
.gs26-pid .pid-edge.flow-unknown { stroke:var(--pid-muted) !important; stroke-dasharray:3 9; opacity:.7; }
.gs26-pid .pid-edge.dashed { stroke-dasharray:12 6; }
.gs26-pid .pid-flow { fill:none; stroke-width:3; stroke-linecap:round; stroke:var(--pid-surface); stroke-dasharray:4 18; opacity:0; }
.gs26-pid .pid-flow.on { opacity:.95; animation:gs26-pid-flow 0.9s linear infinite; }
@keyframes gs26-pid-flow { to { stroke-dashoffset:-22; } }
.gs26-pid .pid-link { fill:none; stroke-width:3; stroke-linecap:round; stroke-linejoin:round; }
.gs26-pid .pid-link.link-up { stroke:var(--pid-text); }
.gs26-pid .pid-link.link-down { stroke:var(--pid-critical); stroke-dasharray:2 8; }
.gs26-pid .pid-link.link-unknown { stroke:var(--pid-muted); stroke-dasharray:2 8; opacity:.6; }
.gs26-pid .pid-link.kind-can { stroke-width:5; }
.gs26-pid .pid-link.kind-radio.link-up { stroke-dasharray:14 6; }
.gs26-pid .pid-link.kind-mechanical { stroke:var(--pid-muted); stroke-width:8; stroke-dasharray:none; opacity:.55; }
.gs26-pid .pid-link-flow { fill:none; stroke-width:3; stroke:var(--pid-good); stroke-dasharray:3 21; opacity:0; }
.gs26-pid .pid-link-flow.on { opacity:.9; animation:gs26-pid-flow 1.4s linear infinite; }
.gs26-pid .pid-edge-label { font-size:11px; fill:var(--pid-muted); }
.gs26-pid .pid-node rect, .gs26-pid .pid-node ellipse, .gs26-pid .pid-node path, .gs26-pid .pid-node circle { fill:var(--pid-surface-alt); stroke:var(--pid-border-strong); stroke-width:2; }
.gs26-pid .pid-node.online-up > .pid-shape { stroke:var(--pid-good); }
.gs26-pid .pid-node.online-down > .pid-shape { stroke:var(--pid-critical); stroke-dasharray:6 4; }
.gs26-pid .pid-node.online-unknown > .pid-shape { stroke:var(--pid-muted); stroke-dasharray:3 5; }
.gs26-pid .pid-node .pid-level { fill:var(--pid-fluid-1); stroke:none; opacity:.55; transition:height .4s, y .4s; }
.gs26-pid .pid-node.kind-bus .pid-shape { fill:var(--pid-text); stroke:none; }
.gs26-pid .pid-node.kind-atmosphere .pid-shape { fill:none; stroke-dasharray:4 4; }
.gs26-pid .pid-node.kind-junction .pid-shape { fill:var(--pid-text); stroke:none; }
.gs26-pid .pid-node-label { font-size:14px; font-weight:600; }
.gs26-pid .pid-node-sub { font-size:11px; fill:var(--pid-muted); }
.gs26-pid .pid-node-age { font-size:11px; fill:var(--pid-critical); }
.gs26-pid .pid-valve .pid-body { stroke-width:2.5; stroke:var(--pid-text); fill:var(--pid-surface); transition:fill .25s; }
.gs26-pid .pid-valve.state-open .pid-body { fill:var(--pid-good); stroke:var(--pid-good); }
.gs26-pid .pid-valve.state-closed .pid-body { fill:var(--pid-surface); stroke:var(--pid-text); }
.gs26-pid .pid-valve.state-pending .pid-body { fill:none; stroke:var(--pid-warning); stroke-dasharray:4 3; animation:gs26-pid-pulse 1s ease-in-out infinite; }
.gs26-pid .pid-valve.state-stale .pid-body, .gs26-pid .pid-valve.state-unknown .pid-body { fill:none; stroke:var(--pid-muted); stroke-dasharray:2 3; }
.gs26-pid .pid-valve.state-manual .pid-body { fill:none; stroke:var(--pid-muted); }
@keyframes gs26-pid-pulse { 50% { opacity:.35; } }
.gs26-pid .pid-valve-label { font-size:12px; font-weight:600; }
.gs26-pid .pid-valve-state { font-size:11px; font-weight:700; letter-spacing:.03em; }
.gs26-pid .pid-valve.state-open .pid-valve-state { fill:var(--pid-good); }
.gs26-pid .pid-valve.state-pending .pid-valve-state { fill:var(--pid-warning-text); }
.gs26-pid .pid-valve.state-stale .pid-valve-state, .gs26-pid .pid-valve.state-unknown .pid-valve-state, .gs26-pid .pid-valve.state-manual .pid-valve-state { fill:var(--pid-muted); font-weight:500; }
.gs26-pid .pid-reading-label { font-size:11px; fill:var(--pid-muted); }
.gs26-pid .pid-reading-value { font-size:17px; font-weight:650; font-variant-numeric:tabular-nums; }
.gs26-pid .pid-reading.stale .pid-reading-value, .gs26-pid .pid-reading.missing .pid-reading-value { fill:var(--pid-muted); font-weight:500; }
.gs26-pid .pid-reading.warn .pid-reading-value { fill:var(--pid-warning-text); }
.gs26-pid .pid-reading.assumed .pid-reading-value { font-style:italic; }
.gs26-pid-wrap[data-program=true] .gs26-pid .pid-reading-value { font-size:21px; }
.gs26-pid-wrap[data-program=true] .gs26-pid .pid-node-label { font-size:16px; }
@media (prefers-reduced-motion: reduce) {
  .gs26-pid .pid-flow.on, .gs26-pid .pid-link-flow.on, .gs26-pid .pid-valve.state-pending .pid-body { animation:none; }
}
"#;

fn hex_luminance(color: &str) -> Option<f32> {
    let hex = color.trim().strip_prefix('#')?;
    let (r, g, b) = match hex.len() {
        6 | 8 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ),
        3 => {
            let d = |i: usize| u8::from_str_radix(&hex[i..i + 1], 16).ok().map(|v| v * 17);
            (d(0)?, d(1)?, d(2)?)
        }
        _ => return None,
    };
    Some((0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0)
}

/// Light themes get the light-surface fluid steps; dark themes the dark ones.
pub(crate) fn theme_is_light(theme: &ThemeConfig) -> bool {
    hex_luminance(&theme.app_background)
        .or_else(|| hex_luminance(&theme.panel_background))
        .is_some_and(|l| l > 0.5)
}

pub(crate) fn theme_vars(theme: &ThemeConfig) -> String {
    let fluids = if theme_is_light(theme) { FLUID_LIGHT } else { FLUID_DARK };
    let warning_text = if theme_is_light(theme) { "#9a6700" } else { WARNING };
    format!(
        "--pid-surface:{};--pid-surface-alt:{};--pid-text:{};--pid-muted:{};--pid-border:{};--pid-border-strong:{};\
         --pid-fluid-1:{};--pid-fluid-2:{};--pid-fluid-3:{};--pid-good:{GOOD};--pid-warning:{WARNING};--pid-warning-text:{warning_text};--pid-critical:{CRITICAL};",
        theme.panel_background,
        theme.panel_background_alt,
        theme.text_primary,
        theme.text_muted,
        theme.border,
        theme.border_strong,
        fluids[0],
        fluids[1],
        fluids[2],
    )
}

fn points(edge: &Edge) -> String {
    edge.points.iter().map(|[x, y]| format!("{x},{y}")).collect::<Vec<_>>().join(" ")
}

/// Midpoint of the longest segment, for the edge label.
fn label_point(edge: &Edge) -> (f32, f32) {
    edge.points
        .windows(2)
        .max_by(|a, b| {
            let len = |s: &[[f32; 2]]| (s[1][0] - s[0][0]).abs() + (s[1][1] - s[0][1]).abs();
            len(a).total_cmp(&len(b))
        })
        .map(|s| ((s[0][0] + s[1][0]) / 2.0, (s[0][1] + s[1][1]) / 2.0))
        .unwrap_or((0.0, 0.0))
}

fn fluid_slot(topology: &Topology, id: &str) -> Option<(u8, bool)> {
    topology.fluids.iter().find(|f| f.id == id).map(|f| (f.slot, f.dashed))
}

fn fluid_label(topology: &Topology, id: &str) -> String {
    topology.fluids.iter().find(|f| f.id == id).map(|f| f.label.clone()).unwrap_or_else(|| id.to_string())
}

pub(crate) fn edge_title(topology: &Topology, edge: &Edge, state: Option<&EdgeState>) -> String {
    let Some(state) = state else { return edge.id.clone() };
    if edge.kind != EdgeKind::Pipe {
        let link = match state.link {
            Some(Link::Up) => "up".to_string(),
            Some(Link::Down) => match state.link_age_ms {
                Some(age) => format!("down (last heard {} ago)", format_age(age)),
                None => "down".into(),
            },
            _ => "unknown".into(),
        };
        return format!("{}: {link}", edge.label.clone().unwrap_or_else(|| edge.id.replace('_', " ")));
    }
    let fluids = state.fluids.iter().map(|f| fluid_label(topology, f)).collect::<Vec<_>>().join(" + ");
    let dp = state.dp_psi.map(|dp| format!(", Δp {dp:.0} psi")).unwrap_or_default();
    match state.flow {
        Flow::Flowing => format!("{fluids} flowing{dp}"),
        Flow::Static => format!("Open path, no pressure difference{dp}"),
        Flow::Blocked => "No flow: a valve on this path is closed".into(),
        Flow::Unknown => "Flow unknown: a valve or pressure on this path is stale or not reported".into(),
    }
}

#[component]
fn EdgeView(topology: Topology, edge: Edge, state: Option<EdgeState>) -> Element {
    let title = edge_title(&topology, &edge, state.as_ref());
    let pts = points(&edge);
    let (lx, ly) = label_point(&edge);
    if edge.kind != EdgeKind::Pipe {
        let link = state.as_ref().and_then(|s| s.link).unwrap_or(Link::Unknown);
        let link_css = match link {
            Link::Up => "link-up",
            Link::Down => "link-down",
            Link::Unknown => "link-unknown",
        };
        let kind = format!("{:?}", edge.kind).to_lowercase();
        // Data links show traffic; power and mechanical paths never animate
        // (an animated pyro line would read as firing).
        let animate = link == Link::Up && matches!(edge.kind, EdgeKind::Can | EdgeKind::Radio | EdgeKind::Uart | EdgeKind::Ethernet);
        return rsx! {
            g { class: "pid-edge-group", "data-edge": edge.id.clone(),
                title { "{title}" }
                polyline { class: "pid-link kind-{kind} {link_css}", points: pts.clone() }
                polyline { class: if animate { "pid-link-flow on" } else { "pid-link-flow" }, points: pts }
                if let Some(label) = edge.label.clone() {
                    text { class: "pid-edge-label", x: "{lx + 6.0}", y: "{ly - 6.0}", "{label}" }
                }
            }
        };
    }
    let flow = state.as_ref().map(|s| s.flow).unwrap_or(Flow::Unknown);
    let carried: Vec<String> = state.as_ref().map(|s| s.fluids.clone()).unwrap_or_default();
    // A blocked line keeps its declared fluid colour, faded.
    let primary = carried.first().cloned().or_else(|| edge.fluid.clone().filter(|f| f != "inherit"));
    let (slot, dashed) = primary.as_deref().and_then(|f| fluid_slot(&topology, f)).unwrap_or((0, false));
    let stroke = if slot == 0 { "var(--pid-muted)".to_string() } else { format!("var(--pid-fluid-{slot})") };
    let second = carried.get(1).and_then(|f| fluid_slot(&topology, f)).map(|(s, _)| format!("var(--pid-fluid-{s})"));
    rsx! {
        g { class: "pid-edge-group", "data-edge": edge.id.clone(), "data-flow": flow.css(),
            title { "{title}" }
            polyline {
                class: format!("pid-edge flow-{}{}", flow.css(), if dashed { " dashed" } else { "" }),
                style: "stroke:{stroke};",
                points: pts.clone(),
                marker_end: "url(#pid-arrow)",
            }
            if let Some(second) = second {
                polyline { class: "pid-edge dashed", style: "stroke:{second};stroke-width:3;", points: pts.clone() }
            }
            polyline { class: if flow == Flow::Flowing { "pid-flow on" } else { "pid-flow" }, points: pts }
        }
    }
}

fn node_shape(node: &Node) -> Element {
    let (x, y, w, h) = (node.x, node.y, node.w, node.h);
    match node.kind {
        NodeKind::Bottle => rsx! { rect { class: "pid-shape", x: "{x}", y: "{y}", width: "{w}", height: "{h}", rx: "{w / 2.6}" } },
        NodeKind::Tank => rsx! { rect { class: "pid-shape", x: "{x}", y: "{y}", width: "{w}", height: "{h}", rx: "{(w / 2.0).min(40.0)}" } },
        NodeKind::Regulator => rsx! {
            circle { class: "pid-shape", cx: "{x + w / 2.0}", cy: "{y + h / 2.0}", r: "{h / 2.0}" }
            path { d: "M{x + w / 2.0 - h / 3.0} {y + h - 6.0} L{x + w / 2.0 + h / 3.0} {y + 6.0}", style: "fill:none;" }
        },
        NodeKind::Junction => rsx! { circle { class: "pid-shape", cx: "{x + w / 2.0}", cy: "{y + h / 2.0}", r: "{w / 2.0}" } },
        NodeKind::Chamber => rsx! {
            path { class: "pid-shape",
                d: "M{x} {y} L{x + w * 0.62} {y} L{x + w * 0.74} {y + h * 0.36} L{x + w} {y + h * 0.06} L{x + w} {y + h * 0.94} L{x + w * 0.74} {y + h * 0.64} L{x + w * 0.62} {y + h} L{x} {y + h} Z" }
        },
        NodeKind::Atmosphere => rsx! { ellipse { class: "pid-shape", cx: "{x + w / 2.0}", cy: "{y + h / 2.0}", rx: "{w / 2.0}", ry: "{h / 2.0}" } },
        NodeKind::Bus => rsx! { rect { class: "pid-shape", x: "{x}", y: "{y}", width: "{w}", height: "{h}", rx: "{h / 2.0}" } },
        NodeKind::LoadCell => rsx! {
            rect { class: "pid-shape", x: "{x}", y: "{y}", width: "{w}", height: "{h}", rx: "4" }
            path { d: "M{x + 8.0} {y + h / 2.0} L{x + w - 8.0} {y + h / 2.0}", style: "fill:none;stroke-dasharray:3 3;" }
        },
        _ => rsx! { rect { class: "pid-shape", x: "{x}", y: "{y}", width: "{w}", height: "{h}", rx: "8" } },
    }
}

#[component]
fn NodeView(node: Node, state: Option<super::derive::NodeState>) -> Element {
    let online = state.as_ref().and_then(|s| s.online);
    let online_css = match online {
        Some(Link::Up) => " online-up",
        Some(Link::Down) => " online-down",
        Some(Link::Unknown) => " online-unknown",
        None => "",
    };
    let kind = format!("{:?}", node.kind).to_lowercase();
    let level = state.as_ref().and_then(|s| s.level_pct);
    let age = state.as_ref().and_then(|s| s.age_ms);
    let (x, y, w, h) = (node.x, node.y, node.w, node.h);
    let inside = !matches!(node.kind, NodeKind::Bus | NodeKind::Junction);
    let label_y = match node.kind {
        NodeKind::Tank | NodeKind::Bottle => y + 22.0,
        NodeKind::Regulator => y - 22.0,
        _ if node.sublabel.is_empty() => y + h / 2.0 + 5.0,
        _ => y + h / 2.0 - 2.0,
    };
    let status_text = match (online, age) {
        (Some(Link::Down), Some(age)) => Some(format!("not heard for {}", format_age(age))),
        (Some(Link::Down), None) | (Some(Link::Unknown), _) => Some("no report".to_string()),
        _ => None,
    };
    let title = match (online, age) {
        (Some(Link::Up), Some(age)) => format!("{}: online, heard {} ago", node.label, format_age(age)),
        _ => node.label.clone(),
    };
    rsx! {
        g { class: "pid-node kind-{kind}{online_css}", "data-node": node.id.clone(),
            title { "{title}" }
            {node_shape(&node)}
            if let Some(level) = level {
                rect { class: "pid-level", x: "{x + 4.0}", y: "{y + h - 4.0 - (h - 8.0) * level / 100.0}", width: "{w - 8.0}",
                    height: "{(h - 8.0) * level / 100.0}", rx: "6" }
            }
            if inside {
                text { class: "pid-node-label", x: "{x + w / 2.0}", y: "{label_y}", text_anchor: "middle", "{node.label}" }
                if !node.sublabel.is_empty() {
                    text { class: "pid-node-sub", x: "{x + w / 2.0}", y: "{label_y + 15.0}", text_anchor: "middle", "{node.sublabel}" }
                }
                if let Some(status) = status_text {
                    text { class: "pid-node-age", x: "{x + w / 2.0}", y: "{y + h + 14.0}", text_anchor: "middle", "{status}" }
                }
            } else if node.kind == NodeKind::Bus {
                text { class: "pid-node-sub", x: "{x}", y: "{y - 6.0}", "{node.label}" }
            }
        }
    }
}

pub(crate) fn valve_state_text(valve: &Valve, state: ValveState) -> String {
    let open = valve.open_text.clone().unwrap_or_else(|| "OPEN".into());
    let closed = valve.closed_text.clone().unwrap_or_else(|| "CLOSED".into());
    let word = |on: bool| if on { open.clone() } else { closed.clone() };
    match state {
        ValveState::Open => word(true),
        ValveState::Closed => word(false),
        ValveState::Pending { to_open, .. } if valve.kind == ValveKind::Qd && !to_open => "RETRACTING / pending".into(),
        ValveState::Pending { to_open, .. } => format!("→ {} (pending)", word(to_open)),
        ValveState::Stale { was_open, age_ms } => format!("stale {} (was {})", format_age(age_ms), word(was_open).to_lowercase()),
        ValveState::Unknown => "unknown".into(),
        ValveState::Manual => "not monitored".into(),
    }
}

#[component]
fn ValveView(valve: Valve, state: ValveState) -> Element {
    let (x, y) = (valve.x, valve.y);
    let s = 13.0_f32;
    // Bow-tie: two triangles meeting at the centre, along the pipe.
    let body = if valve.vertical {
        format!("M{} {} L{} {} L{x} {y} Z M{} {} L{} {} L{x} {y} Z", x - s, y - s, x + s, y - s, x - s, y + s, x + s, y + s)
    } else {
        format!("M{} {} L{} {} L{x} {y} Z M{} {} L{} {} L{x} {y} Z", x - s, y - s, x - s, y + s, x + s, y - s, x + s, y + s)
    };
    let actuator = match valve.kind {
        ValveKind::Solenoid => "S",
        ValveKind::Servo => "M",
        ValveKind::Qd => "QD",
        ValveKind::Manual => "H",
    };
    let text = valve_state_text(&valve, state);
    rsx! {
        g { class: "pid-valve state-{state.css()}", "data-valve": valve.id.clone(), role: "img",
            "aria-label": format!("{}: {text}", valve.label),
            title { "{valve.label}: {text}" }
            path { class: "pid-body", d: body }
            text { class: "pid-node-sub", x: "{x}", y: "{y - s - 5.0}", text_anchor: "middle", "{actuator}" }
            text { class: "pid-valve-label", x: "{x}", y: "{y + s + 15.0}", text_anchor: "middle", "{valve.label}" }
            text { class: "pid-valve-state", x: "{x}", y: "{y + s + 29.0}", text_anchor: "middle", "{text}" }
        }
    }
}

#[component]
fn ReadingView(reading: Reading, state: super::derive::ReadingState) -> Element {
    let anchor = match reading.anchor {
        Anchor::Start => "start",
        Anchor::Middle => "middle",
        Anchor::End => "end",
    };
    let mut class = String::from("pid-reading");
    for (on, name) in [(state.stale, " stale"), (state.missing, " missing"), (state.warn, " warn"), (state.assumed, " assumed")] {
        if on {
            class.push_str(name);
        }
    }
    let suffix = match (state.stale, state.age_ms) {
        (true, Some(age)) => format!(" · {} ago", format_age(age)),
        _ => String::new(),
    };
    let warn = if state.warn { " ▲" } else { "" };
    rsx! {
        g { class: class, "data-reading": reading.id.clone(),
            text { class: "pid-reading-label", x: "{reading.x}", y: "{reading.y + 11.0}", text_anchor: anchor, "{reading.label}" }
            text { class: "pid-reading-value", x: "{reading.x}", y: "{reading.y + 30.0}", text_anchor: anchor, "{state.text}{warn}{suffix}" }
        }
    }
}

/// One diagram. The caller derives `state` at its own cadence.
#[component]
pub(crate) fn DiagramSvg(topology: Topology, state: DiagramState) -> Element {
    let [min_x, min_y, width, height] = topology.view_box;
    let igt = state.igt;
    rsx! {
        svg { class: "gs26-pid", "data-diagram": topology.id.clone(), view_box: "{min_x} {min_y} {width} {height}",
            preserve_aspect_ratio: "xMidYMid meet", role: "img", "aria-label": "{topology.title}",
            defs {
                marker { id: "pid-arrow", view_box: "0 0 10 10", ref_x: "8", ref_y: "5", marker_width: "4", marker_height: "4", orient: "auto-start-reverse",
                    path { d: "M0 0 L10 5 L0 10 z", style: "fill:var(--pid-muted);" }
                }
            }
            for frame in topology.frames.iter().filter(|f| visible(f.when, igt)) {
                g { key: "frame-{frame.id}",
                    rect { class: "pid-frame", x: "{frame.x}", y: "{frame.y}", width: "{frame.w}", height: "{frame.h}", rx: "14" }
                    text { class: "pid-frame-label", x: "{frame.x + 14.0}", y: "{frame.y + 20.0}", "{frame.label}" }
                }
            }
            for edge in topology.edges.iter().filter(|e| visible(e.when, igt)) {
                EdgeView { key: "edge-{edge.id}", topology: topology.clone(), edge: edge.clone(), state: state.edges.get(&edge.id).cloned() }
            }
            for node in topology.nodes.iter().filter(|n| visible(n.when, igt)) {
                NodeView { key: "node-{node.id}", node: node.clone(), state: state.nodes.get(&node.id).cloned() }
            }
            for valve in topology.valves.iter().filter(|v| visible(v.when, igt)) {
                ValveView { key: "valve-{valve.id}", valve: valve.clone(), state: state.valves.get(&valve.id).copied().unwrap_or(ValveState::Unknown) }
            }
            for reading in topology.readings.iter().filter(|r| visible(r.when, igt)) {
                if let Some(reading_state) = state.readings.get(&reading.id).cloned() {
                    ReadingView { key: "reading-{reading.id}", reading: reading.clone(), state: reading_state }
                }
            }
        }
    }
}

/// Legend: fluids (colour and dash), valve states and flow states.
#[component]
pub(crate) fn Legend(topology: Topology) -> Element {
    let swatch = |stroke: String, dash: &str| {
        rsx! { svg { view_box: "0 0 34 10", width: "34", height: "10", "aria-hidden": "true",
            line { x1: "2", y1: "5", x2: "32", y2: "5", style: "stroke:{stroke};stroke-width:5;stroke-linecap:round;stroke-dasharray:{dash};" }
        } }
    };
    rsx! {
        div { class: "gs26-pid-legend", role: "list", "aria-label": "Legend",
            for fluid in topology.fluids.iter() {
                span { key: "fluid-{fluid.id}", role: "listitem", class: "gs26-pid-legend-item",
                    {swatch(format!("var(--pid-fluid-{})", fluid.slot), if fluid.dashed { "8 4" } else { "none" })}
                    "{fluid.label}"
                }
            }
            if !topology.fluids.is_empty() {
                span { role: "listitem", class: "gs26-pid-legend-item", {swatch("var(--pid-muted)".into(), "3 6")} "Flow unknown or stale" }
                span { role: "listitem", class: "gs26-pid-legend-item", "Moving dashes: flowing (path open and Δp)" }
                span { role: "listitem", class: "gs26-pid-legend-item", "Faded: blocked" }
            }
            if !topology.valves.is_empty() {
                span { role: "listitem", class: "gs26-pid-legend-item", i { class: "gs26-pid-dot", style: "background:var(--pid-good);" } "Valve open (confirmed)" }
                span { role: "listitem", class: "gs26-pid-legend-item", i { class: "gs26-pid-dot", style: "border:2px solid var(--pid-text);" } "Closed (confirmed)" }
                span { role: "listitem", class: "gs26-pid-legend-item", i { class: "gs26-pid-dot", style: "border:2px dashed var(--pid-warning);" } "Commanded, not yet confirmed" }
                span { role: "listitem", class: "gs26-pid-legend-item", i { class: "gs26-pid-dot", style: "border:2px dotted var(--pid-muted);" } "Stale or unknown" }
            }
            if topology.edges.iter().any(|e| e.kind != EdgeKind::Pipe && e.kind != EdgeKind::Mechanical) {
                span { role: "listitem", class: "gs26-pid-legend-item", {swatch("var(--pid-text)".into(), "none")} "Link up" }
                span { role: "listitem", class: "gs26-pid-legend-item", {swatch("var(--pid-critical)".into(), "2 6")} "Link down" }
            }
            span { role: "listitem", class: "gs26-pid-legend-item", "≈ nominal / assumed value" }
        }
    }
}
