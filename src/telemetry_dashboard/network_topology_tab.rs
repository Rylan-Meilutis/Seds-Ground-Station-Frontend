use dioxus::prelude::*;
use dioxus_signals::Signal;
use once_cell::sync::Lazy;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

const GRAPH_VIEWPORT_ID: &str = "network-topology-viewport";
const GRAPH_SURFACE_ID: &str = "network-topology-surface";
const GRAPH_CANVAS_ID: &str = "network-topology-canvas";
const GRAPH_VIEWPORT_FULLSCREEN_ID: &str = "network-topology-viewport-fullscreen";
const GRAPH_SURFACE_FULLSCREEN_ID: &str = "network-topology-surface-fullscreen";
const GRAPH_CANVAS_FULLSCREEN_ID: &str = "network-topology-canvas-fullscreen";

use super::layout::{NetworkTabLayout, ThemeConfig};
use super::types::{
    BoardStatusEntry, NetworkTopologyLink, NetworkTopologyMsg, NetworkTopologyNode,
    NetworkTopologyNodeKind, NetworkTopologyStatus,
};
use super::{js_eval, translate_text};

#[derive(Clone, Copy)]
struct NodePlacement {
    x: i32,
    y: i32,
    size: i32,
}

#[derive(Clone)]
struct GraphLayout {
    width: i32,
    height: i32,
    placements: HashMap<String, NodePlacement>,
}

#[derive(Clone, Copy)]
struct GraphViewportFocus {
    center_x: i32,
    center_y: i32,
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y: i32,
    left_extent: i32,
    right_extent: i32,
    top_extent: i32,
    bottom_extent: i32,
}

#[derive(Clone, Copy, Default)]
struct NodePacketPulse {
    last_count: u64,
    active_until_ms: u64,
    serial: u64,
}

#[derive(Clone)]
struct NodePacketStats {
    sender_id: String,
    total: u64,
}

#[derive(Clone)]
struct TopologyDerived {
    simulated: bool,
    graph_nodes: Vec<NetworkTopologyNode>,
    graph_links: Vec<NetworkTopologyLink>,
    render_placements: HashMap<String, NodePlacement>,
    node_labels: HashMap<String, String>,
    neighbor_labels_by_id: HashMap<String, Vec<String>>,
    render_width: i32,
    render_height: i32,
    viewport_focus: Option<GraphViewportFocus>,
}

#[derive(Clone)]
struct TopologyLayoutDerived {
    visible_node_ids: HashSet<String>,
    render_placements: HashMap<String, NodePlacement>,
    node_labels: HashMap<String, String>,
    neighbor_labels_by_id: HashMap<String, Vec<String>>,
    render_width: i32,
    render_height: i32,
    viewport_focus: Option<GraphViewportFocus>,
}

static NODE_PACKET_PULSES: Lazy<Mutex<HashMap<String, NodePacketPulse>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static TOPOLOGY_DERIVED_CACHE: Lazy<Mutex<Option<(u64, TopologyLayoutDerived)>>> =
    Lazy::new(|| Mutex::new(None));
static TOPOLOGY_PACKET_STATS_CACHE: Lazy<
    Mutex<Option<(u64, u64, HashMap<String, NodePacketStats>)>>,
> = Lazy::new(|| Mutex::new(None));

const GRAPH_MIN_WIDTH: i32 = 1080;
const GRAPH_MIN_HEIGHT: i32 = 720;
const EMBEDDED_GRAPH_MIN_HEIGHT: i32 = 520;
const PACKET_PULSE_MS: u64 = 3_400;
const ZOOM_MIN: f32 = 0.12;
const ZOOM_MAX: f32 = 2.2;
const ZOOM_STEP: f32 = 0.2;
const TOUCH_DRAG_FRICTION: f32 = 1.0;
const GRAPH_FIT_MARGIN_PX: i32 = 36;
const GRAPH_LINK_CHANNEL_COLOR: &str = "#243447";
const GRAPH_LINK_NODE_CLEARANCE: f32 = 7.0;

fn graph_viewport_style(
    theme: &ThemeConfig,
    min_height_px: i32,
    max_height: Option<&str>,
    fullscreen: bool,
) -> String {
    let size_constraints = if fullscreen {
        "flex:1; min-height:0;".to_string()
    } else {
        let mut style = format!("flex:1 1 auto; min-height:0; max-height:100%;");
        if let Some(max_height) = max_height {
            style.push_str(&format!(" max-height:{max_height};"));
        }
        let _ = min_height_px;
        style
    };
    format!(
        "{size_constraints} border:1px solid {border}; border-radius:20px; background:radial-gradient(circle at top, {panel_alt} 0%, {panel} 45%, {app} 100%); overflow:auto; cursor:grab; user-select:none; touch-action:none; -webkit-overflow-scrolling:touch; overscroll-behavior:contain; scrollbar-width:none; -ms-overflow-style:none; box-shadow:0 24px 60px rgba(0,0,0,0.45);",
        border = theme.border,
        panel_alt = theme.panel_background_alt,
        panel = theme.panel_background,
        app = theme.app_background,
    )
}

fn link_adjacency_map(links: &[NetworkTopologyLink]) -> HashMap<String, Vec<String>> {
    let mut adjacency = HashMap::<String, Vec<String>>::new();
    for link in links {
        adjacency
            .entry(link.source.clone())
            .or_default()
            .push(link.target.clone());
        adjacency
            .entry(link.target.clone())
            .or_default()
            .push(link.source.clone());
    }
    adjacency
}

#[component]
pub fn NetworkTopologyTab(
    topology: Signal<NetworkTopologyMsg>,
    board_status: Signal<Vec<BoardStatusEntry>>,
    layout: NetworkTabLayout,
    flow_animation_enabled: bool,
    vertical_layout: bool,
    theme: ThemeConfig,
) -> Element {
    let snapshot = topology.read();
    let board_status_snapshot = board_status.read().clone();
    let expanded_node_id = use_signal(|| None::<String>);
    let mut is_fullscreen = use_signal(|| false);
    let title = layout
        .title
        .unwrap_or_else(|| "Network Topology".to_string());
    let topology_hash = topology_cache_hash(&snapshot, vertical_layout);
    let derived = topology_derived_cached(&snapshot, vertical_layout, topology_hash);
    let packet_stats =
        packet_stats_by_node_cached(&derived.graph_nodes, &board_status_snapshot, topology_hash);
    let packet_pulses =
        node_packet_pulse_serials(&derived.graph_nodes, &packet_stats, flow_animation_enabled);
    let viewport_id = if *is_fullscreen.read() {
        GRAPH_VIEWPORT_FULLSCREEN_ID
    } else {
        GRAPH_VIEWPORT_ID
    };
    let surface_id = if *is_fullscreen.read() {
        GRAPH_SURFACE_FULLSCREEN_ID
    } else {
        GRAPH_SURFACE_ID
    };
    let canvas_id = if *is_fullscreen.read() {
        GRAPH_CANVAS_FULLSCREEN_ID
    } else {
        GRAPH_CANVAS_ID
    };
    let last_view_setup_signature = use_signal(|| None::<u64>);
    let last_view_setup_fullscreen = use_signal(|| None::<bool>);

    {
        let is_fullscreen = is_fullscreen;
        let mut last_view_setup_signature = last_view_setup_signature;
        let mut last_view_setup_fullscreen = last_view_setup_fullscreen;
        use_effect(move || {
            let fullscreen = *is_fullscreen.read();
            let viewport_id = if fullscreen {
                GRAPH_VIEWPORT_FULLSCREEN_ID
            } else {
                GRAPH_VIEWPORT_ID
            };
            let surface_id = if fullscreen {
                GRAPH_SURFACE_FULLSCREEN_ID
            } else {
                GRAPH_SURFACE_ID
            };
            let canvas_id = if fullscreen {
                GRAPH_CANVAS_FULLSCREEN_ID
            } else {
                GRAPH_CANVAS_ID
            };
            let next_signature = graph_view_setup_signature(
                fullscreen,
                viewport_id,
                surface_id,
                canvas_id,
                derived.render_width,
                derived.render_height,
                derived.viewport_focus,
            );
            let previous = *last_view_setup_signature.read();
            let previous_fullscreen = *last_view_setup_fullscreen.read();
            if previous == Some(next_signature) {
                return;
            }
            last_view_setup_signature.set(Some(next_signature));
            last_view_setup_fullscreen.set(Some(fullscreen));
            install_drag_handlers(
                fullscreen,
                viewport_id,
                surface_id,
                canvas_id,
                derived.render_width,
                derived.render_height,
                derived.viewport_focus,
                previous.is_none() || previous_fullscreen != Some(fullscreen),
            );
        });
    }

    let fullscreen_state = *is_fullscreen.read();

    let on_toggle_fullscreen = move |_| {
        let next = !*is_fullscreen.read();
        is_fullscreen.set(next);
    };

    rsx! {
        style {
            {r#"
            #network-topology-viewport::-webkit-scrollbar,
            #network-topology-viewport-fullscreen::-webkit-scrollbar {
                display: none;
                width: 0;
                height: 0;
            }
            @keyframes gs26-network-node-packet-pulse {
                0% {
                    opacity: 0;
                    transform: scale(0.88);
                    border-color: rgba(125, 211, 252, 0.1);
                    box-shadow: 0 0 10px rgba(56, 189, 248, 0.08), inset 0 0 8px rgba(14, 165, 233, 0.06);
                }
                24% {
                    opacity: 0.9;
                    transform: scale(1.02);
                    border-color: rgba(125, 211, 252, 0.95);
                    box-shadow: 0 0 44px rgba(56, 189, 248, 0.48), inset 0 0 24px rgba(14, 165, 233, 0.24);
                }
                100% {
                    opacity: 0;
                    transform: scale(1.48);
                    border-color: rgba(125, 211, 252, 0);
                    box-shadow: 0 0 8px rgba(56, 189, 248, 0), inset 0 0 6px rgba(14, 165, 233, 0);
                }
            }
            @keyframes gs26-network-flow-forward {
                from {
                    stroke-dashoffset: 0;
                }
                to {
                    stroke-dashoffset: -28;
                }
            }
            @keyframes gs26-network-flow-reverse {
                from {
                    stroke-dashoffset: -28;
                }
                to {
                    stroke-dashoffset: 0;
                }
            }
            "#}
        }
        if *is_fullscreen.read() {
            div {
                key: "network-fullscreen-{fullscreen_state}",
                style: "position:fixed; inset:0; z-index:9999; padding:16px; background:{theme.overlay_background}; display:flex; flex-direction:column; gap:12px;",
                div {
                    style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap; justify-content:space-between;",
                    h2 { style: "margin:0; color:{theme.main_tab_accents.get(\"network-topology\").map(String::as_str).unwrap_or(theme.info_accent.as_str())};", "{title}" }
                    div {
                        style: "display:flex; align-items:center; gap:10px; color:{theme.text_secondary}; flex-wrap:wrap;",
                        button {
                            style: zoom_button_style(&theme),
                            onclick: move |_| graph_zoom_delta(-ZOOM_STEP),
                            "{translate_text(\"Zoom Out\")}"
                        }
                        button {
                            style: zoom_button_style(&theme),
                            onclick: move |_| graph_zoom_reset(),
                            "{translate_text(\"Reset\")}"
                        }
                        button {
                            style: zoom_button_style(&theme),
                            onclick: move |_| graph_zoom_delta(ZOOM_STEP),
                            "{translate_text(\"Zoom In\")}"
                        }
                        button {
                            style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                            onclick: on_toggle_fullscreen,
                            "{translate_text(\"Exit Fullscreen\")}"
                        }
                    }
                }
                p {
                    style: "margin:0; color:{theme.text_muted}; font-size:0.95rem;",
                    if derived.simulated {
                        "{translate_text(\"Topology graph is running in testing-mode simulation.\")}"
                    } else {
                        "{translate_text(\"Topology graph is built from Ground Station topology and live node/link status.\")}"
                    }
                }
                div {
                    style: "{graph_viewport_style(&theme, EMBEDDED_GRAPH_MIN_HEIGHT, None, true)}",
                    id: "{viewport_id}",
                    {render_graph_surface(&theme, surface_id, canvas_id, derived.render_width, derived.render_height, &derived.graph_links, &derived.graph_nodes, &derived.node_labels, &derived.neighbor_labels_by_id, &derived.render_placements, &packet_stats, &packet_pulses, flow_animation_enabled, expanded_node_id)}
                }
            }
        } else {
            div {
                key: "network-embedded-{fullscreen_state}",
                style: "display:flex; flex:1 1 auto; flex-direction:column; gap:12px; width:100%; height:100%; max-height:100%; min-height:0; box-sizing:border-box; overflow:hidden; padding:10px 14px 14px 14px;",
                h2 { style: "margin:0; color:{theme.text_primary};", "{title}" }
                p {
                    style: "margin:0; color:{theme.text_muted}; font-size:0.95rem;",
                    if derived.simulated {
                        "{translate_text(\"Router graph is running in testing-mode simulation.\")}"
                    } else {
                        "{translate_text(\"Router graph is built from the Ground Station RAN topology and live board/link status.\")}"
                    }
                }
                div {
                    style: "display:flex; align-items:center; gap:10px; color:{theme.text_secondary}; flex-wrap:wrap;",
                    button {
                        style: zoom_button_style(&theme),
                        onclick: move |_| graph_zoom_delta(-ZOOM_STEP),
                        "{translate_text(\"Zoom Out\")}"
                    }
                    button {
                        style: zoom_button_style(&theme),
                        onclick: move |_| graph_zoom_reset(),
                        "{translate_text(\"Reset\")}"
                    }
                    button {
                        style: zoom_button_style(&theme),
                        onclick: move |_| graph_zoom_delta(ZOOM_STEP),
                        "{translate_text(\"Zoom In\")}"
                    }
                    button {
                        style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                        onclick: on_toggle_fullscreen,
                        "{translate_text(\"Fullscreen\")}"
                    }
                    span {
                        style: "font-size:0.85rem; color:{theme.text_muted};",
                        "{translate_text(\"Pinch or drag to navigate\")}"
                    }
                }

                div {
                    id: "{viewport_id}",
                    style: "padding:8px; {graph_viewport_style(&theme, EMBEDDED_GRAPH_MIN_HEIGHT, None, false)}",
                    {render_graph_surface(&theme, surface_id, canvas_id, derived.render_width, derived.render_height, &derived.graph_links, &derived.graph_nodes, &derived.node_labels, &derived.neighbor_labels_by_id, &derived.render_placements, &packet_stats, &packet_pulses, flow_animation_enabled, expanded_node_id)}
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_graph_surface(
    theme: &ThemeConfig,
    surface_id: &str,
    canvas_id: &str,
    render_width: i32,
    render_height: i32,
    graph_links: &[NetworkTopologyLink],
    graph_nodes: &[NetworkTopologyNode],
    node_labels: &HashMap<String, String>,
    neighbor_labels_by_id: &HashMap<String, Vec<String>>,
    render_placements: &HashMap<String, NodePlacement>,
    packet_stats: &HashMap<String, NodePacketStats>,
    packet_pulses: &HashMap<String, u64>,
    flow_animation_enabled: bool,
    expanded_node_id: Signal<Option<String>>,
) -> Element {
    rsx! {
        div {
            id: "{surface_id}",
            style: "position:relative; width:{render_width}px; height:{render_height}px; min-width:{render_width}px; min-height:{render_height}px;",
            div {
                id: "{canvas_id}",
                style: "position:absolute; inset:0 auto auto 0; width:{render_width}px; height:{render_height}px; transform:scale(1); transform-origin:top left; isolation:isolate;",
                svg {
                    width: "{render_width}",
                    height: "{render_height}",
                    view_box: "0 0 {render_width} {render_height}",
                    style: "position:absolute; inset:0; overflow:visible; z-index:0; pointer-events:none;",
                    for link in graph_links.iter() {
                        {render_link(link, node_labels, render_placements, flow_animation_enabled)}
                    }
                }

                for node in graph_nodes.iter() {
                    {render_node(
                        theme,
                        node,
                        neighbor_labels_by_id.get(&node.id),
                        render_placements,
                        render_width,
                        packet_stats.get(&node.id),
                        packet_pulses.get(&node.id).copied(),
                        expanded_node_id,
                    )}
                }
            }
        }
    }
}

fn zoom_button_style(theme: &ThemeConfig) -> String {
    format!(
        "padding:6px 10px; border-radius:10px; border:1px solid {}; background:{}; color:{}; font-size:0.82rem; cursor:pointer;",
        theme.button_border, theme.button_background, theme.button_text
    )
}

fn graph_zoom_delta(delta: f32) {
    js_eval(&format!(
        r#"
        (function() {{
          if (typeof window.__gs26NetworkGraphZoomDelta === "function") {{
            window.__gs26NetworkGraphZoomDelta({delta});
          }}
        }})();
        "#
    ));
}

fn graph_zoom_reset() {
    js_eval(
        r#"
        (function() {
          if (typeof window.__gs26NetworkGraphZoomReset === "function") {
            window.__gs26NetworkGraphZoomReset();
          }
        })();
        "#,
    );
}

fn graph_view_setup_signature(
    fullscreen: bool,
    viewport_id: &str,
    surface_id: &str,
    canvas_id: &str,
    render_width: i32,
    render_height: i32,
    viewport_focus: Option<GraphViewportFocus>,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    fullscreen.hash(&mut hasher);
    viewport_id.hash(&mut hasher);
    surface_id.hash(&mut hasher);
    canvas_id.hash(&mut hasher);
    render_width.hash(&mut hasher);
    render_height.hash(&mut hasher);
    viewport_focus
        .map(|focus| {
            (
                focus.center_x,
                focus.center_y,
                focus.min_x,
                focus.max_x,
                focus.min_y,
                focus.max_y,
                focus.left_extent,
                focus.right_extent,
                focus.top_extent,
                focus.bottom_extent,
            )
        })
        .hash(&mut hasher);
    hasher.finish()
}

fn install_drag_handlers(
    _fullscreen: bool,
    viewport_id: &str,
    surface_id: &str,
    canvas_id: &str,
    graph_width: i32,
    graph_height: i32,
    viewport_focus: Option<GraphViewportFocus>,
    force_fit: bool,
) {
    js_eval(&format!(
        r#"
        (function() {{
          const viewport = document.getElementById({viewport_id:?});
          const surface = document.getElementById({surface_id:?});
          const canvas = document.getElementById({canvas_id:?});
          if (!viewport || !surface || !canvas) return;
          const state = window.__gs26NetworkGraphState || {{
            scale: 1.0,
            drag: null,
            suppressNextClick: false,
            pointers: new Map(),
            pinchDistance: null,
            pinchScale: 1.0,
            pendingPanX: 0,
            pendingPanY: 0,
            panFrame: null,
            canvasLeft: 0,
            canvasTop: 0,
            padLeft: 0,
            padRight: 0,
            padTop: 0,
            padBottom: 0,
            autoFitted: false,
            userInteracted: false,
            resizeObserver: null,
            resizeObserverTarget: null,
            viewportWidth: 0,
            viewportHeight: 0,
            listenersInstalled: false,
          }};
          state.viewport = viewport;
          state.surface = surface;
          state.canvas = canvas;
          state.graphWidth = {graph_width};
          state.graphHeight = {graph_height};
          state.focusKey = {viewport_focus_key:?};
          window.__gs26NetworkGraphState = state;

          const setCursor = (value) => {{
            state.viewport.style.cursor = value;
          }};

          const isMouseLikePointer = (evt) => {{
            const pointerType = String(evt.pointerType || "");
            return pointerType === "" || pointerType === "mouse";
          }};

          const pointerIsMouseLike = (pointer) => {{
            const pointerType = String(pointer && pointer.pointerType || "");
            return pointerType === "" || pointerType === "mouse";
          }};

          const pointerDragFriction = (evt) => {{
            return isMouseLikePointer(evt) ? 1.0 : {touch_drag_friction};
          }};

          const storedPointerDragFriction = (pointer) => {{
            return pointerIsMouseLike(pointer) ? 1.0 : {touch_drag_friction};
          }};

          const withinSurface = (target) => {{
            return target === state.surface || state.surface.contains(target);
          }};

          const clamp = (value, min, max) => Math.max(min, Math.min(max, value));
          const distance = (a, b) => Math.hypot(a.x - b.x, a.y - b.y);
          const fitScale = () => {{
            const marginX = Math.min(72, Math.max({fit_margin_px}, state.viewport.clientWidth * 0.06));
            const marginY = Math.min(72, Math.max({fit_margin_px}, state.viewport.clientHeight * 0.06));
            const availW = Math.max(state.viewport.clientWidth - marginX * 2, 240);
            const availH = Math.max(state.viewport.clientHeight - marginY * 2, 240);
            return clamp(Math.min(availW / {graph_width}, availH / {graph_height}) * 0.98, {zoom_min}, {zoom_max});
          }};
          const refreshSurfaceFrame = () => {{
            const scaledWidth = Math.round({graph_width} * state.scale);
            const scaledHeight = Math.round({graph_height} * state.scale);
            const fitPadX = Math.max({fit_margin_px}, Math.round((state.viewport.clientWidth - scaledWidth) / 2));
            const fitPadY = Math.max({fit_margin_px}, Math.round((state.viewport.clientHeight - scaledHeight) / 2));
            const panPadX = Math.max(fitPadX, Math.round(state.viewport.clientWidth * 0.58));
            const panPadY = Math.max(fitPadY, Math.round(state.viewport.clientHeight * 0.58));
            state.padLeft = panPadX;
            state.padRight = panPadX;
            state.padTop = panPadY;
            state.padBottom = panPadY;
            state.surface.style.width = `${{scaledWidth + state.padLeft + state.padRight}}px`;
            state.surface.style.height = `${{scaledHeight + state.padTop + state.padBottom}}px`;
            state.surface.style.minWidth = state.surface.style.width;
            state.surface.style.minHeight = state.surface.style.height;
            state.canvasLeft = Math.round(state.padLeft);
            state.canvasTop = Math.round(state.padTop);
            state.canvas.style.left = `${{state.canvasLeft}}px`;
            state.canvas.style.top = `${{state.canvasTop}}px`;
          }};
          const setViewportScroll = (left, top) => {{
            const maxLeft = Math.max(0, state.viewport.scrollWidth - state.viewport.clientWidth);
            const maxTop = Math.max(0, state.viewport.scrollHeight - state.viewport.clientHeight);
            state.viewport.scrollLeft = clamp(left, 0, maxLeft);
            state.viewport.scrollTop = clamp(top, 0, maxTop);
          }};
          const captureViewportAnchor = () => {{
            const localX = state.viewport.clientWidth / 2;
            const localY = state.viewport.clientHeight / 2;
            return {{
              contentX: (state.viewport.scrollLeft + localX - state.canvasLeft) / state.scale,
              contentY: (state.viewport.scrollTop + localY - state.canvasTop) / state.scale,
              localX,
              localY,
            }};
          }};
          const restoreViewportAnchor = (anchor) => {{
            if (!anchor) return;
            setViewportScroll(
              anchor.contentX * state.scale + state.canvasLeft - anchor.localX,
              anchor.contentY * state.scale + state.canvasTop - anchor.localY,
            );
          }};
          const flushPendingPan = () => {{
            if (state.panFrame != null) {{
              window.cancelAnimationFrame(state.panFrame);
            }}
            state.panFrame = null;
            if (state.pendingPanX === 0 && state.pendingPanY === 0) return;
            state.viewport.scrollLeft += state.pendingPanX;
            state.viewport.scrollTop += state.pendingPanY;
            state.pendingPanX = 0;
            state.pendingPanY = 0;
          }};
          const schedulePan = (dx, dy) => {{
            state.pendingPanX += dx;
            state.pendingPanY += dy;
            if (state.panFrame != null) return;
            state.panFrame = window.requestAnimationFrame(() => {{
              flushPendingPan();
            }});
          }};
          const centerGraph = () => {{
            const scaledWidth = Math.round({graph_width} * state.scale);
            const scaledHeight = Math.round({graph_height} * state.scale);
            setViewportScroll(
              state.padLeft + Math.round((scaledWidth - state.viewport.clientWidth) / 2),
              state.padTop + Math.round((scaledHeight - state.viewport.clientHeight) / 2),
            );
          }};
          const applyScale = (nextScale, clientX, clientY) => {{
            flushPendingPan();
            const scale = clamp(nextScale, {zoom_min}, {zoom_max});
            const rect = state.viewport.getBoundingClientRect();
            const localX = clientX - rect.left;
            const localY = clientY - rect.top;
            const contentX = (state.viewport.scrollLeft + localX - state.canvasLeft) / state.scale;
            const contentY = (state.viewport.scrollTop + localY - state.canvasTop) / state.scale;
            state.scale = scale;
            state.userInteracted = true;
            state.canvas.style.transform = `scale(${{scale}})`;
            refreshSurfaceFrame();
            setViewportScroll(
              contentX * scale + state.canvasLeft - localX,
              contentY * scale + state.canvasTop - localY,
            );
          }};

          const zoomFromWheel = (evt) => {{
            const delta = Number(evt.deltaY || 0);
            if (!Number.isFinite(delta) || Math.abs(delta) < 0.01) return;
            const intensity = evt.ctrlKey ? 0.0035 : 0.0018;
            const nextScale = state.scale * Math.exp(-delta * intensity);
            applyScale(nextScale, evt.clientX, evt.clientY);
          }};

          window.__gs26NetworkGraphZoomDelta = (delta) => {{
            const rect = state.viewport.getBoundingClientRect();
            applyScale(state.scale + delta, rect.left + rect.width / 2, rect.top + rect.height / 2);
          }};

          window.__gs26NetworkGraphZoomReset = () => {{
            state.scale = fitScale();
            state.autoFitted = true;
            state.userInteracted = false;
            state.canvas.style.transform = `scale(${{state.scale}})`;
            refreshSurfaceFrame();
            centerGraph();
          }};

          window.__gs26NetworkGraphRefresh = () => {{
            const anchor = captureViewportAnchor();
            refreshSurfaceFrame();
            restoreViewportAnchor(anchor);
          }};
          const fitAndCenterGraph = () => {{
            state.viewport.scrollLeft = 0;
            state.viewport.scrollTop = 0;
            state.scale = fitScale();
            state.autoFitted = true;
            state.canvas.style.transform = `scale(${{state.scale}})`;
            refreshSurfaceFrame();
            centerGraph();
            window.requestAnimationFrame(() => {{
              refreshSurfaceFrame();
              centerGraph();
              window.requestAnimationFrame(() => {{
                refreshSurfaceFrame();
                centerGraph();
              }});
            }});
          }};

          const handleViewportResize = () => {{
            const nextWidth = Math.max(0, state.viewport.clientWidth || 0);
            const nextHeight = Math.max(0, state.viewport.clientHeight || 0);
            const changed = nextWidth !== state.viewportWidth || nextHeight !== state.viewportHeight;
            state.viewportWidth = nextWidth;
            state.viewportHeight = nextHeight;
            if (!changed || nextWidth <= 0 || nextHeight <= 0) return;
            if (!state.userInteracted || !state.autoFitted) {{
              fitAndCenterGraph();
            }} else if (typeof window.__gs26NetworkGraphRefresh === "function") {{
              window.__gs26NetworkGraphRefresh();
            }}
          }};

          if (typeof window.ResizeObserver === "function") {{
            if (!state.resizeObserver) {{
              state.resizeObserver = new window.ResizeObserver(() => {{
                handleViewportResize();
              }});
            }}
            if (state.resizeObserverTarget !== state.viewport) {{
              if (state.resizeObserverTarget) {{
                try {{
                  state.resizeObserver.unobserve(state.resizeObserverTarget);
                }} catch (_err) {{}}
              }}
              state.resizeObserver.observe(state.viewport);
              state.resizeObserverTarget = state.viewport;
            }}
          }}

          if ({force_fit}) {{
            fitAndCenterGraph();
            window.requestAnimationFrame(() => {{
              if (typeof window.__gs26NetworkGraphRefresh === "function") {{
                window.__gs26NetworkGraphRefresh();
              }}
            }});
          }} else if (typeof window.__gs26NetworkGraphRefresh === "function") {{
            window.__gs26NetworkGraphRefresh();
          }}
          if (state.listenersInstalled) return;
          state.listenersInstalled = true;

          window.addEventListener("resize", () => {{
            fitAndCenterGraph();
          }});

          document.addEventListener("wheel", (evt) => {{
            if (!withinSurface(evt.target)) return;
            const target = evt.target;
            if (target && typeof target.closest === "function" && target.closest("button")) {{
              return;
            }}
            zoomFromWheel(evt);
            evt.preventDefault();
          }}, {{ passive: false }});

          document.addEventListener("pointerdown", (evt) => {{
            if (!withinSurface(evt.target)) return;
            const target = evt.target;
            if (target && typeof target.closest === "function" && target.closest("button")) {{
              return;
            }}
            state.pointers.set(evt.pointerId, {{ x: evt.clientX, y: evt.clientY, pointerType: String(evt.pointerType || "") }});
            state.suppressNextClick = false;
            if (state.pointers.size === 1) {{
              state.drag = {{
                x: evt.clientX,
                y: evt.clientY,
                moved: false,
                friction: pointerDragFriction(evt),
              }};
            }} else if (state.pointers.size === 2) {{
              const [a, b] = Array.from(state.pointers.values());
              flushPendingPan();
              state.drag = null;
              state.pinchDistance = distance(a, b);
              state.pinchScale = state.scale;
              state.userInteracted = true;
            }}
            try {{
              state.surface.setPointerCapture(evt.pointerId);
            }} catch (_err) {{}}
            setCursor("grabbing");
          }});

          window.addEventListener("pointermove", (evt) => {{
            if (!state.pointers.has(evt.pointerId)) return;
            const previousPointer = state.pointers.get(evt.pointerId) || null;
            state.pointers.set(evt.pointerId, {{
              x: evt.clientX,
              y: evt.clientY,
              pointerType: String((previousPointer && previousPointer.pointerType) || evt.pointerType || ""),
            }});
            if (state.pointers.size >= 2) {{
              const [a, b] = Array.from(state.pointers.values());
              const nextDistance = distance(a, b);
              if (state.pinchDistance && nextDistance > 0) {{
                const centerX = (a.x + b.x) / 2;
                const centerY = (a.y + b.y) / 2;
                applyScale(state.pinchScale * (nextDistance / state.pinchDistance), centerX, centerY);
                state.suppressNextClick = true;
              }}
              evt.preventDefault();
              return;
            }}
            if (!state.drag) return;
            const dx = state.drag.x - evt.clientX;
            const dy = state.drag.y - evt.clientY;
            const friction = Number.isFinite(state.drag.friction) ? state.drag.friction : pointerDragFriction(evt);
            schedulePan(dx * friction, dy * friction);
            state.userInteracted = true;
            state.drag = {{
              x: evt.clientX,
              y: evt.clientY,
              moved: state.drag.moved || Math.abs(dx) > 2 || Math.abs(dy) > 2,
              friction,
            }};
            evt.preventDefault();
          }}, {{ passive: false }});

          window.addEventListener("pointerup", (evt) => {{
            if (!state.pointers.has(evt.pointerId)) return;
            const dragged = !!(state.drag && state.drag.moved);
            state.suppressNextClick = state.suppressNextClick || dragged;
            state.pointers.delete(evt.pointerId);
            if (state.pointers.size === 1) {{
              const [remaining] = Array.from(state.pointers.values());
              state.drag = {{
                x: remaining.x,
                y: remaining.y,
                moved: true,
                friction: storedPointerDragFriction(remaining),
              }};
              state.pinchDistance = null;
              state.pinchScale = state.scale;
            }} else if (state.pointers.size === 0) {{
              state.drag = null;
              state.pinchDistance = null;
              state.pinchScale = state.scale;
            }}
            setCursor("grab");
            try {{
              state.surface.releasePointerCapture(evt.pointerId);
            }} catch (_err) {{}}
          }});

          document.addEventListener("click", (evt) => {{
            if (!withinSurface(evt.target)) return;
            if (!state.suppressNextClick) return;
            state.suppressNextClick = false;
            evt.preventDefault();
            evt.stopPropagation();
          }}, true);
        }})();
        "#,
        viewport_id = viewport_id,
        surface_id = surface_id,
        canvas_id = canvas_id,
        zoom_min = ZOOM_MIN,
        zoom_max = ZOOM_MAX,
        touch_drag_friction = TOUCH_DRAG_FRICTION,
        fit_margin_px = GRAPH_FIT_MARGIN_PX,
        graph_width = graph_width,
        graph_height = graph_height,
        force_fit = if force_fit { "true" } else { "false" },
        viewport_focus_key = viewport_focus
            .map(|focus| format!(
                "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
                focus.center_x,
                focus.center_y,
                focus.min_x,
                focus.max_x,
                focus.min_y,
                focus.max_y,
                focus.left_extent,
                focus.right_extent,
                focus.top_extent,
                focus.bottom_extent
            ))
            .unwrap_or_default(),
    ));
}

pub(crate) fn collect_endpoint_rows(
    nodes: &[NetworkTopologyNode],
    links: &[NetworkTopologyLink],
) -> Vec<(String, Vec<String>)> {
    let mut by_endpoint = BTreeMap::<String, Vec<String>>::new();
    let adjacency = link_adjacency_map(links);

    for node in nodes {
        for endpoint in &node.endpoints {
            if let Some(owner) = endpoint_owner_label(node, endpoint) {
                by_endpoint
                    .entry(endpoint.clone())
                    .or_default()
                    .push(owner.clone());
            }
        }

        if node.kind != NetworkTopologyNodeKind::Endpoint {
            continue;
        }

        let endpoint_name = node
            .endpoints
            .first()
            .cloned()
            .unwrap_or_else(|| node.label.clone());
        for owner in endpoint_route_owners(node, nodes, &adjacency, &endpoint_name) {
            by_endpoint
                .entry(endpoint_name.clone())
                .or_default()
                .push(owner);
        }
    }

    by_endpoint
        .into_iter()
        .map(|(endpoint, mut owners)| {
            owners.sort();
            owners.dedup();
            (endpoint, owners)
        })
        .collect()
}

fn endpoint_route_owners(
    endpoint_node: &NetworkTopologyNode,
    nodes: &[NetworkTopologyNode],
    adjacency: &HashMap<String, Vec<String>>,
    endpoint_name: &str,
) -> Vec<String> {
    let mut owners = Vec::new();
    let mut queue = std::collections::VecDeque::<String>::new();
    let mut visited = HashSet::<String>::new();
    visited.insert(endpoint_node.id.clone());

    if let Some(neighbors) = adjacency.get(&endpoint_node.id) {
        for neighbor in neighbors {
            queue.push_back(neighbor.clone());
        }
    }

    while let Some(current) = queue.pop_front() {
        if !visited.insert(current.clone()) {
            continue;
        }
        let Some(node) = nodes.iter().find(|node| node.id == current) else {
            continue;
        };
        if let Some(owner) = endpoint_owner_label(node, endpoint_name) {
            owners.push(owner);
            continue;
        }
        if let Some(neighbors) = adjacency.get(&current) {
            for neighbor in neighbors {
                if !visited.contains(neighbor) {
                    queue.push_back(neighbor.clone());
                }
            }
        }
    }

    owners.sort();
    owners.dedup();
    owners
}

fn endpoint_owner_label(node: &NetworkTopologyNode, endpoint_name: &str) -> Option<String> {
    match node.kind {
        NetworkTopologyNodeKind::Router | NetworkTopologyNodeKind::Board
            if node
                .endpoints
                .iter()
                .any(|endpoint| endpoint == endpoint_name) =>
        {
            Some(node.label.clone())
        }
        NetworkTopologyNodeKind::Endpoint | NetworkTopologyNodeKind::Side => None,
        _ => None,
    }
}

fn render_link(
    link: &NetworkTopologyLink,
    node_labels: &HashMap<String, String>,
    placements: &HashMap<String, NodePlacement>,
    flow_animation_enabled: bool,
) -> Element {
    let Some(source) = placement_for(&link.source, placements) else {
        return rsx! { g {} };
    };
    let Some(target) = placement_for(&link.target, placements) else {
        return rsx! { g {} };
    };
    let (stroke, glow, dash) = link_style(link.status);
    let stroke = link_color(link, stroke);
    let glow = link_color(link, glow);
    let source_label = node_label(&link.source, node_labels);
    let target_label = node_label(&link.target, node_labels);
    let animated = flow_animation_enabled && !matches!(link.status, NetworkTopologyStatus::Offline);
    let dx = (target.x - source.x) as f32;
    let dy = (target.y - source.y) as f32;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let ux = dx / len;
    let uy = dy / len;
    let nx = -dy / len;
    let ny = dx / len;
    let source_clearance = (source.size as f32 / 2.0 + GRAPH_LINK_NODE_CLEARANCE).min(len / 2.0);
    let target_clearance = (target.size as f32 / 2.0 + GRAPH_LINK_NODE_CLEARANCE).min(len / 2.0);
    let link_x1 = source.x as f32 + ux * source_clearance;
    let link_y1 = source.y as f32 + uy * source_clearance;
    let link_x2 = target.x as f32 - ux * target_clearance;
    let link_y2 = target.y as f32 - uy * target_clearance;
    let lane_offset = if len < 220.0 { 2.2 } else { 2.4 };
    let lane1_x1 = link_x1 + nx * lane_offset;
    let lane1_y1 = link_y1 + ny * lane_offset;
    let lane1_x2 = link_x2 + nx * lane_offset;
    let lane1_y2 = link_y2 + ny * lane_offset;
    let lane2_x1 = link_x1 - nx * lane_offset;
    let lane2_y1 = link_y1 - ny * lane_offset;
    let lane2_x2 = link_x2 - nx * lane_offset;
    let lane2_y2 = link_y2 - ny * lane_offset;
    let upload_color = match link.status {
        NetworkTopologyStatus::Online => "#38bdf8",
        NetworkTopologyStatus::Offline => "#ef4444",
        NetworkTopologyStatus::Simulated => "#8b5cf6",
    };
    let download_color = match link.status {
        NetworkTopologyStatus::Online => "#22c55e",
        NetworkTopologyStatus::Offline => "#f87171",
        NetworkTopologyStatus::Simulated => "#c084fc",
    };
    let upload_dur = if matches!(link.status, NetworkTopologyStatus::Simulated) {
        "1.6s"
    } else {
        "1.1s"
    };
    let download_dur = if matches!(link.status, NetworkTopologyStatus::Simulated) {
        "1.8s"
    } else {
        "1.25s"
    };
    let lane_dash = if matches!(link.status, NetworkTopologyStatus::Simulated) {
        "12 16"
    } else {
        "10 18"
    };
    let tooltip = format!(
        "{source_label} -> {target_label}: upload lane\n{target_label} -> {source_label}: download lane"
    );

    rsx! {
        g {
            if !animated {
                line {
                    x1: "{link_x1}",
                    y1: "{link_y1}",
                    x2: "{link_x2}",
                    y2: "{link_y2}",
                    stroke: "{glow}",
                    stroke_width: "10",
                    stroke_opacity: "0.15",
                    stroke_linecap: "round",
                }
            }
            if !animated {
                line {
                    x1: "{link_x1}",
                    y1: "{link_y1}",
                    x2: "{link_x2}",
                    y2: "{link_y2}",
                    stroke: "{stroke}",
                    stroke_width: "3",
                    stroke_dasharray: "{dash}",
                    stroke_linecap: "round",
                }
            }
            if animated {
                g {
                    line {
                        x1: "{link_x1}",
                        y1: "{link_y1}",
                        x2: "{link_x2}",
                        y2: "{link_y2}",
                        stroke: "{glow}",
                        stroke_width: "8.5",
                        stroke_opacity: "0.12",
                        stroke_linecap: "round",
                    }
                    line {
                        x1: "{link_x1}",
                        y1: "{link_y1}",
                        x2: "{link_x2}",
                        y2: "{link_y2}",
                        stroke: "{GRAPH_LINK_CHANNEL_COLOR}",
                        stroke_width: "6",
                        stroke_opacity: "1.0",
                        stroke_linecap: "round",
                    }
                    line {
                        x1: "{lane1_x1}",
                        y1: "{lane1_y1}",
                        x2: "{lane1_x2}",
                        y2: "{lane1_y2}",
                        stroke: "{upload_color}",
                        stroke_width: "2.5",
                        stroke_dasharray: "{lane_dash}",
                        stroke_dashoffset: "0",
                        stroke_linecap: "round",
                        stroke_opacity: "0.92",
                        style: "animation:gs26-network-flow-forward {upload_dur} linear infinite;"
                    }
                    line {
                        x1: "{lane2_x1}",
                        y1: "{lane2_y1}",
                        x2: "{lane2_x2}",
                        y2: "{lane2_y2}",
                        stroke: "{download_color}",
                        stroke_width: "2.5",
                        stroke_dasharray: "{lane_dash}",
                        stroke_dashoffset: "-28",
                        stroke_linecap: "round",
                        stroke_opacity: "0.92",
                        style: "animation:gs26-network-flow-reverse {download_dur} linear infinite;"
                    }
                }
            }
            title { "{tooltip}" }
        }
    }
}

fn render_node(
    theme: &ThemeConfig,
    node: &NetworkTopologyNode,
    neighbor_labels: Option<&Vec<String>>,
    placements: &HashMap<String, NodePlacement>,
    graph_width: i32,
    packet_stats: Option<&NodePacketStats>,
    packet_pulse_serial: Option<u64>,
    expanded_node_id: Signal<Option<String>>,
) -> Element {
    let Some(placement) = placement_for(&node.id, placements) else {
        return rsx! { div {} };
    };
    let (ring, bg, fg, chip_bg, chip_fg, status_label) = node_style(theme, node.status);
    let neighbors = neighbor_labels.cloned().unwrap_or_default();
    let is_expanded = expanded_node_id
        .read()
        .as_ref()
        .map(|id| id == &node.id)
        .unwrap_or(false);
    let kind = match node.kind {
        NetworkTopologyNodeKind::Router => "Router",
        NetworkTopologyNodeKind::Endpoint => "Endpoint",
        NetworkTopologyNodeKind::Side => "Side",
        NetworkTopologyNodeKind::Board => "Board",
    };
    let outline = if is_expanded {
        "3px solid rgba(255,255,255,0.18)"
    } else {
        "none"
    };
    let panel_left = if placement.x > (graph_width / 2) {
        "auto"
    } else {
        "calc(100% + 14px)"
    };
    let panel_right = if placement.x > (graph_width / 2) {
        "calc(100% + 14px)"
    } else {
        "auto"
    };
    let node_z_index = if is_expanded { "20" } else { "2" };
    let packet_count_label = packet_stats.map(|stats| format_packet_count(stats.total));

    rsx! {
        div {
            "data-network-node": "true",
            style: "position:absolute; left:{placement.x}px; top:{placement.y}px; width:{placement.size}px; height:{placement.size}px; transform:translate(-50%, -50%); \
                    border-radius:999px; border:2px solid {ring}; background:{bg}; color:{fg}; box-shadow:0 24px 50px rgba(2, 6, 23, 0.48); \
                    display:flex; flex-direction:column; align-items:center; justify-content:center; text-align:center; padding:14px; gap:6px; cursor:pointer; \
                    outline:{outline}; z-index:{node_z_index};",
            onclick: {
                let node_id = node.id.clone();
                let mut expanded_node_id = expanded_node_id;
                move |_| {
                    let next = match expanded_node_id.read().as_ref() {
                        Some(current) if current == &node_id => None,
                        _ => Some(node_id.clone()),
                    };
                    expanded_node_id.set(next);
                }
            },
            if let Some(serial) = packet_pulse_serial {
                div {
                    key: "packet-pulse-{node.id}-{serial}",
                    style: "position:absolute; inset:-18px; border-radius:999px; border:3px solid rgba(125, 211, 252, 0); box-shadow:0 0 8px rgba(56, 189, 248, 0), inset 0 0 6px rgba(14, 165, 233, 0); transform-origin:center; animation:gs26-network-node-packet-pulse 3.2s linear both; pointer-events:none;",
                }
            }
            div { style: "font-size:0.95rem; font-weight:800; line-height:1.1;", "{node.label}" }
            if let Some(sender_id) = &node.sender_id {
                div { style: "font-size:0.72rem; color:#93c5fd; text-transform:uppercase; letter-spacing:0.08em;", "{sender_id}" }
            } else {
                div { style: "font-size:0.72rem; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.08em;", "{kind}" }
            }
            span {
                style: "padding:2px 8px; border-radius:999px; background:{chip_bg}; color:{chip_fg}; font-size:0.7rem; font-weight:700;",
                "{status_label}"
            }
            div {
                style: "font-size:0.68rem; color:{theme.text_muted}; max-width:100%; line-height:1.2;",
                if let Some(packet_count_label) = packet_count_label.as_ref() {
                    "{packet_count_label} packet(s)"
                } else if node.endpoints.is_empty() {
                    "Tap for details"
                } else {
                    "{node.endpoints.len()} endpoint(s)"
                }
            }
            if is_expanded {
                div {
                    "data-network-panel": "true",
                    style: "position:absolute; left:{panel_left}; right:{panel_right}; top:50%; transform:translateY(-50%); width:240px; padding:12px 14px; border-radius:14px; \
                            border:1px solid {theme.border}; background:{theme.panel_background}; box-shadow:0 20px 40px rgba(2, 6, 23, 0.55); z-index:4; text-align:left;",
                    div { style: "font-size:0.73rem; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.08em;", "{kind} details" }
                    div { style: "font-size:0.95rem; color:{theme.text_primary}; font-weight:700; margin:4px 0 10px 0;", "{node.label}" }
                    div { style: "font-size:0.73rem; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.08em; margin-bottom:8px;", "Packet stats" }
                    if let Some(stats) = packet_stats {
                        div { style: "display:flex; flex-direction:column; gap:6px; margin-bottom:12px;",
                            div {
                                style: "display:flex; justify-content:space-between; gap:10px; padding:6px 8px; border-radius:10px; border:1px solid {theme.border_soft}; background:{theme.panel_background_alt}; color:{theme.text_secondary}; font-size:0.8rem;",
                                span { "From {stats.sender_id}" }
                                span { style: "font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace; color:{theme.text_primary};", "{format_packet_count(stats.total)}" }
                            }
                        }
                    } else {
                        div { style: "font-size:0.82rem; color:{theme.text_muted}; margin-bottom:12px;", "No packets seen for this node sender yet." }
                    }
                    div { style: "font-size:0.73rem; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.08em; margin-bottom:8px;", "Connected to" }
                    if neighbors.is_empty() {
                        div { style: "font-size:0.82rem; color:{theme.text_muted}; margin-bottom:12px;", "No active links." }
                    } else {
                        div { style: "display:flex; flex-wrap:wrap; gap:6px; margin-bottom:12px;",
                            for neighbor in neighbors.iter() {
                                span {
                                    style: "padding:4px 8px; border-radius:999px; background:{theme.panel_background_alt}; border:1px solid {theme.border_soft}; color:{theme.text_secondary}; font-size:0.72rem;",
                                    "{neighbor}"
                                }
                            }
                        }
                    }
                    div { style: "font-size:0.73rem; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.08em; margin-bottom:8px;", "Endpoints" }
                    if node.endpoints.is_empty() {
                        div { style: "font-size:0.82rem; color:{theme.text_muted};", "No discovered endpoints for this node." }
                    } else {
                        div { style: "display:flex; flex-direction:column; gap:6px; max-height:240px; overflow-y:auto; padding-right:4px;",
                            for endpoint in node.endpoints.iter() {
                                div {
                                    style: "padding:6px 8px; border-radius:10px; border:1px solid {theme.border_soft}; background:{theme.panel_background_alt}; color:{theme.text_secondary}; font-size:0.8rem;",
                                    "{endpoint}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn placement_for(id: &str, placements: &HashMap<String, NodePlacement>) -> Option<NodePlacement> {
    placements.get(id).copied()
}

fn packet_stats_by_node(
    nodes: &[NetworkTopologyNode],
    board_status: &[BoardStatusEntry],
) -> HashMap<String, NodePacketStats> {
    let counts_by_sender = board_status
        .iter()
        .map(|entry| (entry.sender_id.as_str(), entry.packet_count))
        .collect::<HashMap<_, _>>();
    nodes
        .iter()
        .filter_map(|node| {
            let sender_id = node.sender_id.as_ref()?;
            let total = counts_by_sender
                .get(sender_id.as_str())
                .copied()
                .unwrap_or(0);
            Some((
                node.id.clone(),
                NodePacketStats {
                    sender_id: sender_id.clone(),
                    total,
                },
            ))
        })
        .collect()
}

fn node_packet_pulse_serials(
    nodes: &[NetworkTopologyNode],
    packet_stats: &HashMap<String, NodePacketStats>,
    animations_enabled: bool,
) -> HashMap<String, u64> {
    let node_ids = nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    let now = current_millis();
    let mut active = HashMap::new();

    let Ok(mut pulses) = NODE_PACKET_PULSES.lock() else {
        return active;
    };

    pulses.retain(|id, _| node_ids.contains(id));
    for node in nodes {
        let count = packet_stats
            .get(&node.id)
            .map(|stats| stats.total)
            .unwrap_or(0);
        let pulse = pulses.entry(node.id.clone()).or_default();
        if count > pulse.last_count {
            if animations_enabled && now >= pulse.active_until_ms {
                pulse.serial = pulse.serial.saturating_add(1);
                pulse.active_until_ms = now.saturating_add(PACKET_PULSE_MS);
                pulse.last_count = count;
            } else if !animations_enabled {
                pulse.last_count = count;
                pulse.active_until_ms = 0;
            }
        }
        if animations_enabled && now < pulse.active_until_ms {
            active.insert(node.id.clone(), pulse.serial);
        }
    }

    active
}

fn current_millis() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now().max(0.0) as u64
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }
}

fn format_packet_count(total: u64) -> String {
    if total >= 1_000_000 {
        format!("{:.1}M", total as f64 / 1_000_000.0)
    } else if total >= 10_000 {
        format!("{:.1}k", total as f64 / 1_000.0)
    } else {
        total.to_string()
    }
}

fn node_label(id: &str, node_labels: &HashMap<String, String>) -> String {
    node_labels
        .get(id)
        .cloned()
        .unwrap_or_else(|| id.to_string())
}

fn neighbor_labels_for_node(
    node: &NetworkTopologyNode,
    links: &[NetworkTopologyLink],
    node_labels: &HashMap<String, String>,
) -> Vec<String> {
    let mut labels = Vec::new();
    for link in links {
        let other = if link.source == node.id {
            Some(link.target.as_str())
        } else if link.target == node.id {
            Some(link.source.as_str())
        } else {
            None
        };
        if let Some(other) = other {
            labels.push(node_label(other, node_labels));
        }
    }
    labels.sort();
    labels.dedup();
    if labels.len() > 4 {
        let remaining = labels.len() - 4;
        labels.truncate(4);
        labels.push(format!("+{remaining} more"));
    }
    labels
}

fn topology_cache_hash(snapshot: &NetworkTopologyMsg, vertical_layout: bool) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    vertical_layout.hash(&mut hasher);
    for node in &snapshot.nodes {
        node.id.hash(&mut hasher);
        node.label.hash(&mut hasher);
        node.kind.hash(&mut hasher);
        node.sender_id.hash(&mut hasher);
        node.endpoints.hash(&mut hasher);
    }
    for link in &snapshot.links {
        link.source.hash(&mut hasher);
        link.target.hash(&mut hasher);
    }
    hasher.finish()
}

fn topology_derived_cached(
    snapshot: &NetworkTopologyMsg,
    vertical_layout: bool,
    cache_hash: u64,
) -> TopologyDerived {
    let cached_layout = if let Ok(cache) = TOPOLOGY_DERIVED_CACHE.lock()
        && let Some((cached_hash, derived)) = cache.as_ref()
        && *cached_hash == cache_hash
    {
        Some(derived.clone())
    } else {
        None
    };

    let visible_node_ids = cached_layout
        .as_ref()
        .map(|derived| derived.visible_node_ids.clone())
        .unwrap_or_else(|| {
            snapshot
                .nodes
                .iter()
                .filter(|node| {
                    !matches!(
                        node.kind,
                        NetworkTopologyNodeKind::Endpoint | NetworkTopologyNodeKind::Side
                    )
                })
                .map(|node| node.id.clone())
                .collect::<HashSet<_>>()
        });
    let graph_nodes = snapshot
        .nodes
        .iter()
        .filter(|node| visible_node_ids.contains(&node.id))
        .cloned()
        .collect::<Vec<_>>();
    let graph_links = collapse_visible_links(&snapshot.nodes, &snapshot.links, &visible_node_ids);

    let layout_derived = if let Some(cached_layout) = cached_layout {
        cached_layout
    } else {
        let graph_layout = compute_graph_layout(&graph_nodes, &graph_links, vertical_layout);
        let router_placement = graph_nodes
            .iter()
            .find(|node| node.kind == NetworkTopologyNodeKind::Router)
            .and_then(|node| graph_layout.placements.get(&node.id).copied());
        let graph_bounds = graph_layout
            .placements
            .values()
            .fold(None::<(i32, i32, i32, i32)>, |acc, placement| {
                let left = placement.x - placement.size / 2;
                let right = placement.x + placement.size / 2;
                let top = placement.y - placement.size / 2;
                let bottom = placement.y + placement.size / 2;
                match acc {
                    Some((min_x, max_x, min_y, max_y)) => Some((
                        min_x.min(left),
                        max_x.max(right),
                        min_y.min(top),
                        max_y.max(bottom),
                    )),
                    None => Some((left, right, top, bottom)),
                }
            })
            .unwrap_or((0, graph_layout.width, 0, graph_layout.height));
        let (bound_min_x, bound_max_x, bound_min_y, bound_max_y) = graph_bounds;
        let render_width = (bound_max_x - bound_min_x).max(1);
        let render_height = (bound_max_y - bound_min_y).max(1);
        let render_placements = graph_layout
            .placements
            .iter()
            .map(|(id, placement)| {
                (
                    id.clone(),
                    NodePlacement {
                        x: placement.x - bound_min_x,
                        y: placement.y - bound_min_y,
                        size: placement.size,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        let viewport_focus = router_placement.map(|router| {
            let router_radius = router.size / 2;
            GraphViewportFocus {
                center_x: router.x - bound_min_x,
                center_y: router.y - bound_min_y,
                min_x: 0,
                max_x: bound_max_x - bound_min_x,
                min_y: 0,
                max_y: bound_max_y - bound_min_y,
                left_extent: (router.x - bound_min_x).max(router_radius),
                right_extent: (bound_max_x - router.x).max(router_radius),
                top_extent: (router.y - bound_min_y).max(router_radius),
                bottom_extent: (bound_max_y - router.y).max(router_radius),
            }
        });
        let node_labels = snapshot
            .nodes
            .iter()
            .map(|node| (node.id.clone(), node.label.clone()))
            .collect::<HashMap<_, _>>();
        let neighbor_labels_by_id = graph_nodes
            .iter()
            .map(|node| {
                (
                    node.id.clone(),
                    neighbor_labels_for_node(node, &graph_links, &node_labels),
                )
            })
            .collect::<HashMap<_, _>>();

        let layout_derived = TopologyLayoutDerived {
            visible_node_ids,
            render_placements,
            node_labels,
            neighbor_labels_by_id,
            render_width,
            render_height,
            viewport_focus,
        };
        if let Ok(mut cache) = TOPOLOGY_DERIVED_CACHE.lock() {
            *cache = Some((cache_hash, layout_derived.clone()));
        }
        layout_derived
    };

    let derived = TopologyDerived {
        simulated: snapshot.simulated,
        graph_nodes,
        graph_links,
        render_placements: layout_derived.render_placements,
        node_labels: layout_derived.node_labels,
        neighbor_labels_by_id: layout_derived.neighbor_labels_by_id,
        render_width: layout_derived.render_width,
        render_height: layout_derived.render_height,
        viewport_focus: layout_derived.viewport_focus,
    };

    derived
}

fn packet_stats_cache_hash(
    nodes: &[NetworkTopologyNode],
    board_status: &[BoardStatusEntry],
) -> u64 {
    let counts_by_sender = board_status
        .iter()
        .map(|entry| (entry.sender_id.as_str(), entry.packet_count))
        .collect::<HashMap<_, _>>();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for node in nodes {
        node.id.hash(&mut hasher);
        node.sender_id.hash(&mut hasher);
        let count = node
            .sender_id
            .as_ref()
            .and_then(|sender_id| counts_by_sender.get(sender_id.as_str()))
            .copied()
            .unwrap_or(0);
        count.hash(&mut hasher);
    }
    hasher.finish()
}

fn packet_stats_by_node_cached(
    nodes: &[NetworkTopologyNode],
    board_status: &[BoardStatusEntry],
    topology_hash: u64,
) -> HashMap<String, NodePacketStats> {
    let packet_hash = packet_stats_cache_hash(nodes, board_status);
    if let Ok(cache) = TOPOLOGY_PACKET_STATS_CACHE.lock()
        && let Some((cached_topology_hash, cached_packet_hash, stats)) = cache.as_ref()
        && *cached_topology_hash == topology_hash
        && *cached_packet_hash == packet_hash
    {
        return stats.clone();
    }

    let stats = packet_stats_by_node(nodes, board_status);
    if let Ok(mut cache) = TOPOLOGY_PACKET_STATS_CACHE.lock() {
        *cache = Some((topology_hash, packet_hash, stats.clone()));
    }
    stats
}

fn link_style(status: NetworkTopologyStatus) -> (&'static str, &'static str, &'static str) {
    match status {
        NetworkTopologyStatus::Online => ("#38bdf8", "#67e8f9", ""),
        NetworkTopologyStatus::Offline => ("#ef4444", "#fca5a5", "8 8"),
        NetworkTopologyStatus::Simulated => ("#8b5cf6", "#c4b5fd", "14 10"),
    }
}

fn link_color(link: &NetworkTopologyLink, default: &'static str) -> &'static str {
    let _ = link;
    default
}

fn node_style(
    theme: &ThemeConfig,
    status: NetworkTopologyStatus,
) -> (
    &'static str,
    String,
    &'static str,
    String,
    &'static str,
    &'static str,
) {
    match status {
        NetworkTopologyStatus::Online => (
            "#22c55e",
            format!(
                "radial-gradient(circle at 30% 30%, #14532d 0%, {} 72%)",
                theme.panel_background
            ),
            "#dcfce7",
            "rgba(34, 197, 94, 0.18)".to_string(),
            "#bbf7d0",
            "Online",
        ),
        NetworkTopologyStatus::Offline => (
            "#ef4444",
            format!(
                "radial-gradient(circle at 30% 30%, #4c0519 0%, {} 72%)",
                theme.panel_background
            ),
            "#fee2e2",
            "rgba(239, 68, 68, 0.18)".to_string(),
            "#fecaca",
            "Offline",
        ),
        NetworkTopologyStatus::Simulated => (
            "#8b5cf6",
            format!(
                "radial-gradient(circle at 30% 30%, #312e81 0%, {} 72%)",
                theme.panel_background
            ),
            "#ede9fe",
            "rgba(139, 92, 246, 0.18)".to_string(),
            "#ddd6fe",
            "Simulated",
        ),
    }
}

fn node_size(kind: NetworkTopologyNodeKind) -> i32 {
    match kind {
        NetworkTopologyNodeKind::Router => 220,
        NetworkTopologyNodeKind::Side => 144,
        NetworkTopologyNodeKind::Board => 164,
        NetworkTopologyNodeKind::Endpoint => 120,
    }
}

fn estimated_label_lines(node: &NetworkTopologyNode) -> i32 {
    let chars_per_line = match node.kind {
        NetworkTopologyNodeKind::Router => 18,
        NetworkTopologyNodeKind::Side => 14,
        NetworkTopologyNodeKind::Board => 13,
        NetworkTopologyNodeKind::Endpoint => 12,
    };
    let mut lines = 1_i32;
    let mut current = 0_i32;
    for word in node.label.split_whitespace() {
        let word_len = word.chars().count() as i32;
        if current == 0 {
            current = word_len;
            continue;
        }
        if current + 1 + word_len > chars_per_line {
            lines += 1;
            current = word_len;
        } else {
            current += 1 + word_len;
        }
    }
    lines.max(1)
}

fn node_diameter(node: &NetworkTopologyNode) -> i32 {
    let base = node_size(node.kind);
    let extra_label_height = (estimated_label_lines(node) - 2).max(0) * 18;
    let endpoint_extra = if node.endpoints.is_empty() { 0 } else { 10 };
    let sender_extra = if node.sender_id.is_some() { 0 } else { 6 };
    base + extra_label_height + endpoint_extra + sender_extra
}

fn stack_height(nodes: &[&NetworkTopologyNode], node_gap: i32) -> i32 {
    if nodes.is_empty() {
        return 0;
    }
    nodes.iter().map(|node| node_diameter(node)).sum::<i32>() + node_gap * (nodes.len() as i32 - 1)
}

fn collapse_visible_links(
    nodes: &[NetworkTopologyNode],
    links: &[NetworkTopologyLink],
    visible_node_ids: &HashSet<String>,
) -> Vec<NetworkTopologyLink> {
    let visible_nodes = nodes
        .iter()
        .filter(|node| visible_node_ids.contains(&node.id))
        .cloned()
        .collect::<Vec<_>>();
    let Some(router) = visible_nodes
        .iter()
        .find(|node| node.kind == NetworkTopologyNodeKind::Router)
    else {
        return Vec::new();
    };

    let mut collapsed = BTreeMap::<(String, String), NetworkTopologyStatus>::new();

    for link in links {
        if !visible_node_ids.contains(&link.source) || !visible_node_ids.contains(&link.target) {
            continue;
        }
        let key = ordered_link_key(link.source.clone(), link.target.clone());
        collapsed
            .entry(key)
            .and_modify(|existing| *existing = existing.merged(link.status))
            .or_insert(link.status);
    }

    let side_by_board = board_side_ids(nodes, links);
    let relay_by_side = relay_board_ids(nodes, &side_by_board);

    for node in visible_nodes
        .iter()
        .filter(|node| node.kind == NetworkTopologyNodeKind::Board)
    {
        let already_connected = collapsed
            .keys()
            .any(|(source, target)| source == &node.id || target == &node.id);
        if already_connected {
            continue;
        }

        let Some(side_id) = side_by_board.get(&node.id) else {
            let key = ordered_link_key(router.id.clone(), node.id.clone());
            collapsed.entry(key).or_insert(node.status);
            continue;
        };

        if let Some(relay_id) = relay_by_side.get(side_id) {
            let relay_status = nodes
                .iter()
                .find(|candidate| candidate.id == *relay_id)
                .map(|relay| relay.status)
                .unwrap_or(node.status);

            let router_key = ordered_link_key(router.id.clone(), relay_id.clone());
            collapsed
                .entry(router_key)
                .and_modify(|existing| *existing = existing.merged(relay_status))
                .or_insert(relay_status);

            if relay_id != &node.id {
                let branch_key = ordered_link_key(relay_id.clone(), node.id.clone());
                collapsed
                    .entry(branch_key)
                    .and_modify(|existing| *existing = existing.merged(node.status))
                    .or_insert(node.status);
            }
        } else {
            let key = ordered_link_key(router.id.clone(), node.id.clone());
            collapsed
                .entry(key)
                .and_modify(|existing| *existing = existing.merged(node.status))
                .or_insert(node.status);
        }
    }

    collapsed
        .into_iter()
        .map(|((source, target), status)| NetworkTopologyLink {
            source,
            target,
            label: None,
            status,
        })
        .collect()
}

fn ordered_link_key(a: String, b: String) -> (String, String) {
    if a < b { (a, b) } else { (b, a) }
}

fn board_side_ids(
    nodes: &[NetworkTopologyNode],
    links: &[NetworkTopologyLink],
) -> HashMap<String, String> {
    let side_ids = nodes
        .iter()
        .filter(|node| node.kind == NetworkTopologyNodeKind::Side)
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    let mut out = HashMap::new();
    for link in links {
        if side_ids.contains(&link.source) {
            out.insert(link.target.clone(), link.source.clone());
        } else if side_ids.contains(&link.target) {
            out.insert(link.source.clone(), link.target.clone());
        }
    }
    out
}

fn relay_board_ids(
    nodes: &[NetworkTopologyNode],
    side_by_board: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for node in nodes
        .iter()
        .filter(|node| node.kind == NetworkTopologyNodeKind::Board)
    {
        let Some(side_id) = side_by_board.get(&node.id) else {
            continue;
        };
        out.entry(side_id.clone())
            .or_insert_with(|| node.id.clone());
    }
    out
}

fn compute_graph_layout(
    nodes: &[NetworkTopologyNode],
    links: &[NetworkTopologyLink],
    vertical_layout: bool,
) -> GraphLayout {
    let layer_gap = 320_i32;
    let node_gap = 40_i32;
    let margin_x = 160_i32;
    let margin_y = 120_i32;

    if nodes.is_empty() {
        return GraphLayout {
            width: GRAPH_MIN_WIDTH,
            height: GRAPH_MIN_HEIGHT,
            placements: HashMap::new(),
        };
    }

    let mut adjacency = link_adjacency_map(links);
    for node in nodes {
        adjacency.entry(node.id.clone()).or_default();
    }

    let root_id = nodes
        .iter()
        .find(|node| node.kind == NetworkTopologyNodeKind::Router)
        .map(|node| node.id.clone())
        .unwrap_or_else(|| nodes[0].id.clone());

    let mut layer_map = HashMap::<String, usize>::new();
    let mut first_hop_by_node = HashMap::<String, Option<String>>::new();
    let mut queue = std::collections::VecDeque::<String>::new();
    layer_map.insert(root_id.clone(), 0);
    first_hop_by_node.insert(root_id.clone(), None);
    queue.push_back(root_id.clone());

    while let Some(node_id) = queue.pop_front() {
        let current_layer = layer_map.get(&node_id).copied().unwrap_or(0);
        let current_first_hop = first_hop_by_node.get(&node_id).cloned().unwrap_or(None);
        if let Some(neighbors) = adjacency.get(&node_id) {
            for neighbor in neighbors {
                if layer_map.contains_key(neighbor) {
                    continue;
                }
                let next_layer = current_layer + 1;
                let next_first_hop = if node_id == root_id {
                    Some(neighbor.clone())
                } else {
                    current_first_hop.clone()
                };
                layer_map.insert(neighbor.clone(), next_layer);
                first_hop_by_node.insert(neighbor.clone(), next_first_hop);
                queue.push_back(neighbor.clone());
            }
        }
    }

    let mut extra_roots = nodes
        .iter()
        .filter(|node| !layer_map.contains_key(&node.id))
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();

    for (branch_idx, id) in extra_roots.drain(..).enumerate() {
        let synthetic_root = format!("__detached_{branch_idx}_{}", id);
        let start_layer = 1;
        layer_map.insert(id.clone(), start_layer);
        first_hop_by_node.insert(id.clone(), Some(synthetic_root.clone()));
        queue.push_back(id.clone());
        while let Some(node_id) = queue.pop_front() {
            let current_layer = layer_map.get(&node_id).copied().unwrap_or(start_layer);
            let current_first_hop = first_hop_by_node
                .get(&node_id)
                .cloned()
                .unwrap_or(Some(synthetic_root.clone()));
            if let Some(neighbors) = adjacency.get(&node_id) {
                for neighbor in neighbors {
                    if layer_map.contains_key(neighbor) {
                        continue;
                    }
                    layer_map.insert(neighbor.clone(), current_layer + 1);
                    first_hop_by_node.insert(neighbor.clone(), current_first_hop.clone());
                    queue.push_back(neighbor.clone());
                }
            }
        }
    }

    let mut branch_roots = first_hop_by_node
        .values()
        .filter_map(|value| value.clone())
        .collect::<Vec<_>>();
    branch_roots.sort();
    branch_roots.dedup();
    let mut branch_index_by_node = HashMap::<String, usize>::new();

    for (branch_idx, branch_root) in branch_roots.iter().enumerate() {
        for node in nodes {
            if first_hop_by_node
                .get(&node.id)
                .and_then(|value| value.as_ref())
                == Some(branch_root)
            {
                branch_index_by_node.insert(node.id.clone(), branch_idx);
            }
        }
    }

    let max_layer = layer_map.values().copied().max().unwrap_or(0);
    let mut layers = vec![Vec::<&NetworkTopologyNode>::new(); max_layer + 1];
    for node in nodes {
        let layer = layer_map.get(&node.id).copied().unwrap_or(0);
        layers[layer].push(node);
    }

    for layer_nodes in &mut layers {
        layer_nodes.sort_by(|a, b| {
            let kind_rank = |kind: NetworkTopologyNodeKind| match kind {
                NetworkTopologyNodeKind::Router => 0,
                NetworkTopologyNodeKind::Side => 1,
                NetworkTopologyNodeKind::Board => 2,
                NetworkTopologyNodeKind::Endpoint => 3,
            };
            kind_rank(a.kind)
                .cmp(&kind_rank(b.kind))
                .then_with(|| a.label.cmp(&b.label))
        });
    }

    let branch_count = branch_roots.len().max(1) as i32;
    let max_branch_stack_height = layers
        .iter()
        .map(|layer_nodes| {
            let mut counts = HashMap::<Option<usize>, Vec<&NetworkTopologyNode>>::new();
            for node in layer_nodes {
                let branch = branch_index_by_node.get(&node.id).copied();
                counts.entry(branch).or_default().push(*node);
            }
            counts
                .values()
                .map(|branch_nodes| stack_height(branch_nodes, node_gap))
                .max()
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0)
        .max(220);
    let branch_gap = max_branch_stack_height + 96;
    let content_height = ((branch_count - 1).max(0) * branch_gap) + max_branch_stack_height + 120;
    let total_height = (content_height + margin_y * 2).max(GRAPH_MIN_HEIGHT);
    let mut placements = HashMap::<String, NodePlacement>::new();
    let graph_center_y = total_height / 2;
    let branch_center_offset = (branch_count - 1) as f32 / 2.0;

    for (layer_idx, layer_nodes) in layers.iter().enumerate() {
        if layer_nodes.is_empty() {
            continue;
        }

        let mut by_branch = HashMap::<Option<usize>, Vec<&NetworkTopologyNode>>::new();
        for node in layer_nodes {
            let branch = if node.id == root_id {
                None
            } else {
                branch_index_by_node.get(&node.id).copied()
            };
            by_branch.entry(branch).or_default().push(*node);
        }

        let mut branch_keys = by_branch.keys().copied().collect::<Vec<_>>();
        branch_keys.sort_by(|a, b| match (a, b) {
            (Some(x), Some(y)) => x.cmp(y),
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });

        for branch_key in branch_keys {
            let Some(branch_nodes) = by_branch.get_mut(&branch_key) else {
                continue;
            };
            branch_nodes.sort_by(|a, b| a.label.cmp(&b.label));
            let branch_center_y = match branch_key {
                Some(branch_idx) => {
                    graph_center_y
                        + ((branch_idx as f32 - branch_center_offset) * branch_gap as f32) as i32
                }
                None => graph_center_y,
            };
            let stack_height = stack_height(branch_nodes, node_gap);
            let mut cursor_y = branch_center_y - (stack_height / 2);

            for node in branch_nodes.iter() {
                let size = node_diameter(node);
                let layer_axis = margin_x + layer_idx as i32 * layer_gap;
                let stack_axis = cursor_y + (size / 2);
                let (x, y) = if vertical_layout {
                    (stack_axis, layer_axis)
                } else {
                    (layer_axis, stack_axis)
                };
                placements.insert(node.id.clone(), NodePlacement { x, y, size });
                cursor_y += size + node_gap;
            }
        }
    }

    let rightmost = placements
        .values()
        .map(|placement| placement.x + placement.size / 2)
        .max()
        .unwrap_or(GRAPH_MIN_WIDTH - margin_x);
    let bottommost = placements
        .values()
        .map(|placement| placement.y + placement.size / 2)
        .max()
        .unwrap_or(GRAPH_MIN_HEIGHT - margin_y);
    let width = (rightmost + margin_x).max(GRAPH_MIN_WIDTH);
    let height = (bottommost + margin_y).max(GRAPH_MIN_HEIGHT);

    GraphLayout {
        width,
        height,
        placements,
    }
}
