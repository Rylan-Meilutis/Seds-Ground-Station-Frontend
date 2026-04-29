use dioxus::prelude::*;
use dioxus_signals::Signal;
use std::collections::{BTreeMap, HashSet};

use super::network_topology_tab::collect_endpoint_rows;
use super::types::{
    display_flight_state, BoardStatusEntry, FlightState, NetworkTopologyMsg,
    NetworkTopologyNodeKind, NetworkTopologyStatus,
};
use super::{
    compensated_network_time_ms, current_language, current_wallclock_ms, format_network_time,
    format_timestamp_ms_clock, js_eval, layout::ThemeConfig, localized_copy,
    monotonic_now_ms, translate_text, AlertMsg, FrontendNetworkMetrics, NetworkTimeSync,
    PersistentNotification, device_timezone_label,
};
use crate::telemetry_dashboard::map_tab::{format_elevation, format_precise_distance};

#[component]
pub fn DetailedTab(
    metrics: Signal<FrontendNetworkMetrics>,
    board_status: Signal<Vec<BoardStatusEntry>>,
    network_topology: Signal<NetworkTopologyMsg>,
    flight_state: Signal<FlightState>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    user_gps: Signal<Option<(f64, f64)>>,
    rocket_altitude_m: Signal<Option<f64>>,
    user_altitude_m: Signal<Option<f64>>,
    #[props(default = false)] distance_units_metric: bool,
    warnings: Signal<Vec<AlertMsg>>,
    errors: Signal<Vec<AlertMsg>>,
    notifications: Signal<Vec<PersistentNotification>>,
    network_time: Signal<Option<NetworkTimeSync>>,
    cache_stats: Vec<(String, String)>,
    theme: ThemeConfig,
) -> Element {
    use_effect(move || {
        js_eval(
            r#"
            (function() {
              if (window.__gs26_prefetch_detail_timer) return;
              const fmtTime = (ms) => {
                const n = Number(ms);
                if (!Number.isFinite(n) || n <= 0) return "--";
                return new Date(n).toLocaleTimeString();
              };
              const setText = (id, value) => {
                const el = document.getElementById(id);
                if (el && el.textContent !== value) el.textContent = value;
              };
              const update = () => {
                if (!document.getElementById("gs26-prefetch-state")) {
                  clearInterval(window.__gs26_prefetch_detail_timer);
                  window.__gs26_prefetch_detail_timer = null;
                  return;
                }
                const state = window.__gs26_ground_map_cache_state || {};
                const context = window.__gs26_ground_map_prefetch_context || {};
                const prefetchEnabled = typeof window.__gs26_prefetch_enabled === "boolean"
                  ? window.__gs26_prefetch_enabled
                  : true;
                const completed = Number(state.completed);
                const failed = Number(state.failed);
                const grabbed = Number.isFinite(completed) ? Math.max(0, completed - (Number.isFinite(failed) ? failed : 0)) : 0;
                const stateName = !prefetchEnabled
                  ? "disabled"
                  : (state.state ? String(state.state) : "idle");
                const stateDetail = state.detail ? ` (${String(state.detail)})` : "";
                setText("gs26-prefetch-state", `${stateName}${stateDetail}`);
                setText("gs26-prefetch-last-started", fmtTime(state.lastStartedAt));
                setText("gs26-prefetch-last-completed", fmtTime(state.lastCompletedAt));
                setText("gs26-prefetch-tiles-grabbed", String(grabbed));
                setText("gs26-prefetch-tiles-failed", Number.isFinite(failed) ? String(failed) : "0");
                setText("gs26-prefetch-tiles-pending", Number.isFinite(Number(state.pending)) ? String(Number(state.pending)) : "0");
                setText("gs26-prefetch-user-context", context.userMessage ? String(context.userMessage) : "--");
                setText("gs26-prefetch-rocket-context", context.rocketMessage ? String(context.rocketMessage) : "--");
                setText("gs26-prefetch-context-summary", context.summaryMessage ? String(context.summaryMessage) : "--");
              };
              window.__gs26_prefetch_detail_timer = window.setInterval(update, 2000);
              update();
            })();
            "#,
        );
    });
    use_effect(move || {
        js_eval(
            r#"
            (function() {
              const GRID_ID = "gs26-detailed-summary-grid";
              const ITEM_SELECTOR = ".gs26-detailed-summary-item";
              const ROW_SIZE = 10;
              const GAP = 14;
              if (!window.__gs26_detailed_summary_layout) {
                window.__gs26_detailed_summary_layout = () => {
                  const grid = document.getElementById(GRID_ID);
                  if (!grid) return;
                  const items = Array.from(grid.querySelectorAll(ITEM_SELECTOR));
                  for (const item of items) {
                    const inner = item.firstElementChild || item;
                    const height = Math.ceil(inner.getBoundingClientRect().height);
                    const span = Math.max(1, Math.ceil((height + GAP) / ROW_SIZE));
                    item.style.gridRowEnd = `span ${span}`;
                  }
                };
              }
              const layout = window.__gs26_detailed_summary_layout;
              const grid = document.getElementById(GRID_ID);
              if (!grid) return;
              if (!window.__gs26_detailed_summary_resize_observer && typeof ResizeObserver === "function") {
                window.__gs26_detailed_summary_resize_observer = new ResizeObserver(() => {
                  layout();
                });
              }
              const observer = window.__gs26_detailed_summary_resize_observer;
              if (observer) {
                observer.disconnect();
                observer.observe(grid);
                for (const item of grid.querySelectorAll(ITEM_SELECTOR)) {
                  observer.observe(item);
                }
              }
              window.requestAnimationFrame(() => {
                layout();
                window.requestAnimationFrame(layout);
              });
            })();
            "#,
        );
    });
    let metrics_snapshot = metrics.read().clone();
    let boards = board_status.read().clone();
    let seen_boards = boards
        .iter()
        .filter(|board| board.seen)
        .cloned()
        .collect::<Vec<_>>();
    let topology = network_topology.read().clone();
    let network_time_snapshot = *network_time.read();
    let visible_topology_nodes = visible_topology_nodes(&topology.nodes);
    let visible_topology_links = collapse_visible_links(&topology.nodes, &topology.links);
    let warnings_count = warnings.read().len();
    let errors_count = errors.read().len();
    let notifications_count = notifications.read().len();
    let now_ms = current_wallclock_ms();
    let rocket_coords = *rocket_gps.read();
    let user_coords = *user_gps.read();
    let rocket_altitude = super::map_tab::sanitize_altitude_m(*rocket_altitude_m.read());
    let user_altitude = super::map_tab::sanitize_altitude_m(*user_altitude_m.read());
    let precise_distance_to_rocket = match (rocket_coords, user_coords) {
        (Some((rocket_lat, rocket_lon)), Some((user_lat, user_lon))) => {
            Some(format_precise_distance(
                super::map_tab::haversine_meters(rocket_lat, rocket_lon, user_lat, user_lon),
                distance_units_metric,
            ))
        }
        _ => None,
    };

    let board_seen = seen_boards.len();
    let online_nodes = visible_topology_nodes
        .iter()
        .filter(|node| node.status == NetworkTopologyStatus::Online)
        .count();
    let offline_nodes = visible_topology_nodes
        .iter()
        .filter(|node| node.status == NetworkTopologyStatus::Offline)
        .count();
    let simulated_nodes = visible_topology_nodes
        .iter()
        .filter(|node| node.status == NetworkTopologyStatus::Simulated)
        .count();
    let online_links = visible_topology_links
        .iter()
        .filter(|link| link.status == NetworkTopologyStatus::Online)
        .count();
    let offline_links = visible_topology_links
        .iter()
        .filter(|link| link.status == NetworkTopologyStatus::Offline)
        .count();
    let router_nodes = visible_topology_nodes
        .iter()
        .filter(|node| node.kind == NetworkTopologyNodeKind::Router)
        .count();
    let board_nodes = visible_topology_nodes
        .iter()
        .filter(|node| node.kind == NetworkTopologyNodeKind::Board)
        .count();
    let max_board_age_ms = seen_boards.iter().filter_map(|board| board.age_ms).max();
    let min_board_age_ms = seen_boards.iter().filter_map(|board| board.age_ms).min();
    let avg_bytes_per_msg = if metrics_snapshot.ws_messages_total > 0 {
        Some(metrics_snapshot.ws_bytes_total as f64 / metrics_snapshot.ws_messages_total as f64)
    } else {
        None
    };
    let avg_rows_per_batch = if metrics_snapshot.telemetry_batches_total > 0 {
        Some(
            metrics_snapshot.telemetry_rows_total as f64
                / metrics_snapshot.telemetry_batches_total as f64,
        )
    } else {
        None
    };
    let ws_idle_ms = metrics_snapshot
        .last_ws_message_wall_ms
        .map(|ts| now_ms.saturating_sub(ts));
    let ws_connected_for_ms = if metrics_snapshot.ws_connected {
        metrics_snapshot
            .last_connect_wall_ms
            .map(|ts| now_ms.saturating_sub(ts))
    } else {
        None
    };
    let network_time_display = network_time_snapshot
        .map(compensated_network_time_ms)
        .map(format_network_time);
    let device_timezone = device_timezone_label();
    let network_clock_delta_ms = network_time_snapshot
        .map(compensated_network_time_ms)
        .map(|ms| current_wallclock_ms().saturating_sub(ms));
    let network_time_age_ms = network_time_snapshot.map(|sync| {
        (monotonic_now_ms() - sync.received_mono_ms)
            .max(0.0)
            .round() as i64
    });
    let topology_age_ms = if topology.generated_ms > 0 {
        Some(now_ms.saturating_sub(topology.generated_ms as i64))
    } else {
        None
    };
    let topology_links_preview = visible_topology_links
        .iter()
        .take(12)
        .map(|link| (link.source.clone(), link.target.clone(), link.status))
        .collect::<Vec<_>>();
    let endpoint_rows = collect_endpoint_rows(&topology.nodes, &topology.links);
    let board_route_rows =
        collect_board_route_rows(&visible_topology_nodes, &visible_topology_links);
    let language = current_language();
    let app_ground_station_title = localized_copy(
        &language,
        "App ↔ Ground Station",
        "Aplicacion ↔ Estacion terrestre",
        "Application ↔ Station au sol",
    );

    rsx! {
        div { style: "padding:18px; height:100%; overflow-y:auto; overflow-x:hidden; color:{theme.text_primary}; background:{theme.app_background};",
            style {
                r#"
                #gs26-detailed-summary-grid {{
                    display:grid;
                    grid-template-columns:repeat(auto-fit, minmax(min(100%, 320px), 1fr));
                    grid-auto-rows:10px;
                    grid-auto-flow:dense;
                    gap:14px;
                    align-items:start;
                    margin-bottom:14px;
                }}
                .gs26-detailed-summary-item {{
                    min-width:0;
                }}
                "#
            }
            div { id: "gs26-detailed-summary-grid",
                div { class: "gs26-detailed-summary-item",
                    {metric_card(
                        &theme,
                        &app_ground_station_title,
                        vec![
                            ("Status", if metrics_snapshot.ws_connected { translate_text("Connected") } else { translate_text("Disconnected") }),
                            ("Base URL", metrics_snapshot.base_http.clone()),
                            ("WebSocket", metrics_snapshot.ws_url.clone()),
                            ("HTTP RTT", opt_ms(metrics_snapshot.http_rtt_ms)),
                            ("HTTP RTT EMA", opt_ms(metrics_snapshot.http_rtt_ema_ms)),
                        ],
                    )}
                }
                div { class: "gs26-detailed-summary-item",
                    {metric_card(
                        &theme,
                        "Traffic",
                        vec![
                            ("Inbound messages", metrics_snapshot.ws_messages_total.to_string()),
                            ("Inbound bytes", human_bytes(metrics_snapshot.ws_bytes_total)),
                            ("Telemetry rows", metrics_snapshot.telemetry_rows_total.to_string()),
                            ("Telemetry batches", metrics_snapshot.telemetry_batches_total.to_string()),
                            ("Msg rate", format!("{:.1}/s", metrics_snapshot.msgs_per_sec)),
                            ("Bandwidth", format!("{}/s", human_bytes_f64(metrics_snapshot.bytes_per_sec))),
                            ("Avg bytes/msg", avg_bytes_per_msg.map(|v| format!("{v:.1} B")).unwrap_or_else(|| "--".to_string())),
                            ("Avg rows/batch", avg_rows_per_batch.map(|v| format!("{v:.1}")).unwrap_or_else(|| "--".to_string())),
                        ],
                    )}
                }
                div { class: "gs26-detailed-summary-item",
                    {metric_card(
                        &theme,
                        "Session",
                        vec![
                            ("Rows per second", format!("{:.1}/s", metrics_snapshot.rows_per_sec)),
                            ("WS disconnects", metrics_snapshot.ws_disconnects_total.to_string()),
                            ("Connected for", opt_i64_ms(ws_connected_for_ms)),
                            ("WS idle", opt_i64_ms(ws_idle_ms)),
                            ("Last WS message", opt_timestamp(metrics_snapshot.last_ws_message_wall_ms)),
                            ("Last disconnect", metrics_snapshot.last_disconnect_reason.clone().map(|v| translate_text(&v)).unwrap_or_else(|| translate_text("None"))),
                            ("Last connect", opt_timestamp(metrics_snapshot.last_connect_wall_ms)),
                        ],
                    )}
                }
                div { class: "gs26-detailed-summary-item",
                    {metric_card(
                        &theme,
                        "Mission State",
                        vec![
                            ("Flight state", translate_text(&display_flight_state(&flight_state.read()))),
                            ("Rocket time", network_time_display.unwrap_or_else(|| translate_text("Unavailable"))),
                            ("Device timezone", device_timezone),
                            ("Clock delta", opt_signed_ms(network_clock_delta_ms)),
                            ("Server time age", opt_i64_ms(network_time_age_ms)),
                            ("Warnings", warnings_count.to_string()),
                            ("Errors", errors_count.to_string()),
                            ("Notifications", notifications_count.to_string()),
                        ],
                    )}
                }
                div { class: "gs26-detailed-summary-item",
                    {metric_card(
                        &theme,
                        "Board Timing",
                        vec![
                            ("Fastest board", opt_u64_ms(min_board_age_ms)),
                            ("Slowest board", opt_u64_ms(max_board_age_ms)),
                        ],
                    )}
                }
                div { class: "gs26-detailed-summary-item",
                    {metric_card_owned(
                        &theme,
                        "Positioning",
                        vec![
                            ("Distance to rocket".to_string(), precise_distance_to_rocket.unwrap_or_else(|| "--".to_string())),
                            ("Rocket coordinates".to_string(), format_coords(rocket_coords)),
                            (
                                "Rocket elevation".to_string(),
                                rocket_altitude
                                    .map(|value| format_elevation(Some(value), distance_units_metric))
                                    .unwrap_or_else(|| "Not available".to_string()),
                            ),
                            ("User coordinates".to_string(), format_coords(user_coords)),
                            (
                                "User elevation".to_string(),
                                user_altitude
                                    .map(|value| format_elevation(Some(value), distance_units_metric))
                                    .unwrap_or_else(|| "Not available".to_string()),
                            ),
                        ],
                    )}
                }
                div { class: "gs26-detailed-summary-item",
                    {metric_card(
                        &theme,
                        "Topology",
                        vec![
                            ("Boards seen", board_seen.to_string()),
                            ("Visible nodes", visible_topology_nodes.len().to_string()),
                            ("Visible links", visible_topology_links.len().to_string()),
                            ("Routers", router_nodes.to_string()),
                            ("Boards", board_nodes.to_string()),
                            ("Online nodes", online_nodes.to_string()),
                            ("Offline nodes", offline_nodes.to_string()),
                            ("Simulated nodes", simulated_nodes.to_string()),
                            ("Online links", online_links.to_string()),
                            ("Offline links", offline_links.to_string()),
                            ("Topology age", opt_i64_ms(topology_age_ms)),
                            ("Topology simulated", translate_text(&yes_no(topology.simulated))),
                        ],
                    )}
                }
                div { class: "gs26-detailed-summary-item",
                    {metric_card_owned(&theme, "Cache Storage", cache_stats.clone())}
                }
                div { class: "gs26-detailed-summary-item",
                    {prefetch_status_card(&theme)}
                }
            }

            div { style: "display:grid; gap:14px; grid-template-columns:repeat(auto-fit, minmax(min(100%, 340px), 1fr)); align-items:start; width:100%;",
                div { style: "display:flex; flex-direction:column; gap:14px; min-width:0;",
                    div { style: "{section_style(&theme)}",
                    h3 { style: "{section_title_style(&theme)}", "Board Latency Detail" }
                    div { style: "width:100%; overflow-x:auto;",
                    table { style: "{table_style()}",
                        thead {
                            tr {
                                th { style: "{th_style(&theme)}", "Board" }
                                th { style: "{th_style(&theme)}", "Sender" }
                                th { style: "{th_style(&theme)}", "Seen" }
                                th { style: "{th_style(&theme)}", "Age" }
                                th { style: "{th_style(&theme)}", "Last Seen" }
                            }
                        }
                        tbody {
                            for board in seen_boards.iter() {
                                tr {
                                    td { style: "{td_style(&theme)}", "{board.display_name()}" }
                                    td { style: "{td_style_mono(&theme)}", "{board.sender_id}" }
                                    td { style: "{td_style(&theme)}", "yes" }
                                    td { style: "{td_style_mono(&theme)}", "{opt_i64_ms(board.age_ms.map(|v| v as i64))}" }
                                    td { style: "{td_style_mono(&theme)}", "{board.last_seen_ms.map(|ts| format_timestamp_ms_clock(ts as i64)).unwrap_or_else(|| \"--\".to_string())}" }
                                }
                            }
                            if seen_boards.is_empty() {
                                tr {
                                    td { style: "{td_style(&theme)}", colspan: "5", "No boards have been observed yet." }
                                }
                            }
                        }
                    }
                    }
                }
                    div { style: "{section_style(&theme)}",
                    h3 { style: "{section_title_style(&theme)}", "Board Routes" }
                    div { style: "width:100%; overflow-x:auto;",
                    table { style: "{table_style()}",
                        thead {
                            tr {
                                th { style: "{th_style(&theme)}", "Board" }
                                th { style: "{th_style(&theme)}", "Upstream" }
                                th { style: "{th_style(&theme)}", "Status" }
                                th { style: "{th_style(&theme)}", "Sender" }
                            }
                        }
                        tbody {
                            for (label, upstream, status, sender_id) in board_route_rows.iter() {
                                tr {
                                    td { style: "{td_style(&theme)}", "{label}" }
                                    td { style: "{td_style(&theme)}", "{upstream}" }
                                    td { style: "{td_style(&theme)}", "{format_status(*status)}" }
                                    td { style: "{td_style_mono(&theme)}", "{sender_id}" }
                                }
                            }
                            if board_route_rows.is_empty() {
                                tr {
                                    td { style: "{td_style(&theme)}", colspan: "4", "No board routes are visible yet." }
                                }
                            }
                        }
                        }
                    }
                    }
                }
                div { style: "display:flex; flex-direction:column; gap:14px; min-width:0;",
                    div { style: "{section_style(&theme)}",
                        h3 { style: "{section_title_style(&theme)}", "Endpoint Ownership" }
                        div { style: "width:100%; overflow-x:auto;",
                        table { style: "{table_style()}",
                            thead {
                                tr {
                                    th { style: "{th_style(&theme)}", "Endpoint" }
                                    th { style: "{th_style(&theme)}", "Host" }
                                }
                            }
                            tbody {
                                for (endpoint, owners) in endpoint_rows.iter() {
                                    tr {
                                        td { style: "{td_style_mono(&theme)}", "{endpoint}" }
                                        td { style: "{td_style(&theme)}", "{owners.join(\", \")}" }
                                    }
                                }
                                if endpoint_rows.is_empty() {
                                    tr {
                                        td { style: "{td_style(&theme)}", colspan: "2", "No endpoint ownership data available." }
                                    }
                                }
                            }
                        }
                        }
                    }
                    div { style: "{section_style(&theme)}",
                        h3 { style: "{section_title_style(&theme)}", "Topology Links" }
                        div { style: "width:100%; overflow-x:auto;",
                        table { style: "{table_style()}",
                            thead {
                                tr {
                                    th { style: "{th_style(&theme)}", "Path" }
                                    th { style: "{th_style(&theme)}", "Status" }
                                }
                            }
                            tbody {
                                for (source, target, status) in topology_links_preview.iter() {
                                    tr {
                                        td { style: "{td_style_mono(&theme)}", "{node_label(source, &visible_topology_nodes)} -> {node_label(target, &visible_topology_nodes)}" }
                                        td { style: "{td_style(&theme)}", "{format_status(*status)}" }
                                    }
                                }
                                if topology_links_preview.is_empty() {
                                    tr {
                                        td { style: "{td_style(&theme)}", colspan: "2", "No topology links are visible yet." }
                                    }
                                }
                            }
                        }
                        }
                    }
                }
            }
        }
    }
}

fn metric_card(theme: &ThemeConfig, title: &str, rows: Vec<(&'static str, String)>) -> Element {
    rsx! {
        div { style: "border:1px solid {theme.border}; border-radius:16px; padding:14px; background:linear-gradient(180deg, {theme.panel_background} 0%, {theme.panel_background_alt} 100%); box-shadow:0 14px 30px rgba(2, 6, 23, 0.28); min-width:0;",
            h3 { style: "margin:0 0 10px 0; color:{theme.text_primary}; font-size:15px; letter-spacing:0.02em;", "{title}" }
            div { style: "display:flex; flex-direction:column; gap:8px;",
                for (label, value) in rows {
                    div { style: "display:flex; justify-content:space-between; gap:16px; align-items:flex-start; min-width:0;",
                        span { style: "color:{theme.text_muted}; font-size:12px; flex:0 0 auto;", "{label}" }
                        span { style: "color:{theme.text_primary}; font-size:13px; text-align:right; font-family: ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace; font-variant-numeric:tabular-nums; flex:1 1 auto; min-width:0; overflow-wrap:anywhere; word-break:break-word;", "{value}" }
                    }
                }
            }
        }
    }
}

fn metric_card_owned(theme: &ThemeConfig, title: &str, rows: Vec<(String, String)>) -> Element {
    rsx! {
        div { style: "border:1px solid {theme.border}; border-radius:16px; padding:14px; background:linear-gradient(180deg, {theme.panel_background} 0%, {theme.panel_background_alt} 100%); box-shadow:0 14px 30px rgba(2, 6, 23, 0.28); min-width:0;",
            h3 { style: "margin:0 0 10px 0; color:{theme.text_primary}; font-size:15px; letter-spacing:0.02em;", "{title}" }
            div { style: "display:flex; flex-direction:column; gap:8px;",
                for (label, value) in rows {
                    div { style: "display:flex; justify-content:space-between; gap:16px; align-items:flex-start; min-width:0;",
                        span { style: "color:{theme.text_muted}; font-size:12px; flex:0 0 auto;", "{label}" }
                        span { style: "color:{theme.text_primary}; font-size:13px; text-align:right; font-family: ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace; font-variant-numeric:tabular-nums; flex:1 1 auto; min-width:0; overflow-wrap:anywhere; word-break:break-word;", "{value}" }
                    }
                }
            }
        }
    }
}

fn prefetch_status_card(theme: &ThemeConfig) -> Element {
    let rows = [
        ("State", "gs26-prefetch-state"),
        ("Context", "gs26-prefetch-context-summary"),
        ("User context", "gs26-prefetch-user-context"),
        ("Rocket context", "gs26-prefetch-rocket-context"),
        ("Last prefetch", "gs26-prefetch-last-started"),
        ("Completed", "gs26-prefetch-last-completed"),
        ("Tiles grabbed", "gs26-prefetch-tiles-grabbed"),
        ("Tiles pending", "gs26-prefetch-tiles-pending"),
        ("Tiles failed", "gs26-prefetch-tiles-failed"),
    ];

    rsx! {
        div { style: "border:1px solid {theme.border}; border-radius:16px; padding:14px; background:linear-gradient(180deg, {theme.panel_background} 0%, {theme.panel_background_alt} 100%); box-shadow:0 14px 30px rgba(2, 6, 23, 0.28); min-width:0;",
            h3 { style: "margin:0 0 10px 0; color:{theme.text_primary}; font-size:15px; letter-spacing:0.02em;", "Map Prefetch" }
            div { style: "display:flex; flex-direction:column; gap:8px;",
                for (label, id) in rows {
                    div { style: "display:flex; justify-content:space-between; gap:16px; align-items:flex-start; min-width:0;",
                        span { style: "color:{theme.text_muted}; font-size:12px; flex:0 0 auto;", "{label}" }
                        span { id: "{id}", style: "color:{theme.text_primary}; font-size:13px; text-align:right; font-family: ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace; font-variant-numeric:tabular-nums; flex:1 1 auto; min-width:0; overflow-wrap:anywhere; word-break:break-word;", "--" }
                    }
                }
            }
        }
    }
}

fn opt_ms(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.1} ms"))
        .unwrap_or_else(|| "--".to_string())
}

fn opt_signed_ms(value: Option<i64>) -> String {
    value
        .map(|v| format!("{v:+} ms"))
        .unwrap_or_else(|| "--".to_string())
}

fn opt_i64_ms(value: Option<i64>) -> String {
    value
        .map(|v| format!("{v} ms"))
        .unwrap_or_else(|| "--".to_string())
}

fn opt_u64_ms(value: Option<u64>) -> String {
    value
        .map(|v| format!("{v} ms"))
        .unwrap_or_else(|| "--".to_string())
}

fn opt_timestamp(value: Option<i64>) -> String {
    value
        .map(format_timestamp_ms_clock)
        .unwrap_or_else(|| "--".to_string())
}

fn human_bytes(bytes: u64) -> String {
    human_bytes_f64(bytes as f64)
}

fn human_bytes_f64(bytes: f64) -> String {
    let units = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes.max(0.0);
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.2} {}", units[unit])
    }
}

fn format_coords(coords: Option<(f64, f64)>) -> String {
    let Some((lat, lon)) = coords else {
        return "--".to_string();
    };
    format!("{lat:.6}, {lon:.6}")
}

fn section_style(theme: &ThemeConfig) -> String {
    format!(
        "border:1px solid {}; border-radius:16px; padding:14px; background:{}; min-width:0;",
        theme.border, theme.panel_background
    )
}

fn section_title_style(theme: &ThemeConfig) -> String {
    format!(
        "margin:0 0 12px 0; color:{}; font-size:15px;",
        theme.text_primary
    )
}

fn table_style() -> &'static str {
    "width:100%; border-collapse:collapse; font-size:13px; table-layout:fixed;"
}

fn th_style(theme: &ThemeConfig) -> String {
    format!(
        "text-align:left; color:{}; border-bottom:1px solid {}; padding:8px 6px;",
        theme.text_muted, theme.border
    )
}

fn td_style(theme: &ThemeConfig) -> String {
    format!(
        "padding:8px 6px; border-bottom:1px solid {}; color:{};",
        theme.border_soft, theme.text_secondary
    )
}

fn td_style_mono(theme: &ThemeConfig) -> String {
    format!(
        "padding:8px 6px; border-bottom:1px solid {}; color:{}; font-family: ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace; font-variant-numeric:tabular-nums; white-space:normal; overflow-wrap:anywhere; word-break:break-word;",
        theme.border_soft, theme.text_secondary
    )
}

fn format_status(status: NetworkTopologyStatus) -> &'static str {
    match status {
        NetworkTopologyStatus::Online => "online",
        NetworkTopologyStatus::Offline => "offline",
        NetworkTopologyStatus::Simulated => "simulated",
    }
}

fn node_label(id: &str, nodes: &[super::types::NetworkTopologyNode]) -> String {
    nodes
        .iter()
        .find(|node| node.id == id)
        .map(|node| node.label.clone())
        .unwrap_or_else(|| id.to_string())
}

fn visible_topology_nodes(
    nodes: &[super::types::NetworkTopologyNode],
) -> Vec<super::types::NetworkTopologyNode> {
    nodes
        .iter()
        .filter(|node| {
            matches!(
                node.kind,
                NetworkTopologyNodeKind::Router | NetworkTopologyNodeKind::Board
            )
        })
        .cloned()
        .collect()
}

fn collapse_visible_links(
    nodes: &[super::types::NetworkTopologyNode],
    links: &[super::types::NetworkTopologyLink],
) -> Vec<super::types::NetworkTopologyLink> {
    let visible = visible_topology_nodes(nodes);
    let visible_ids = visible
        .iter()
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    let mut collapsed = BTreeMap::<(String, String), NetworkTopologyStatus>::new();
    for link in links {
        if !visible_ids.contains(&link.source) || !visible_ids.contains(&link.target) {
            continue;
        }
        let key = if link.source < link.target {
            (link.source.clone(), link.target.clone())
        } else {
            (link.target.clone(), link.source.clone())
        };
        collapsed
            .entry(key)
            .and_modify(|existing| *existing = existing.merged(link.status))
            .or_insert(link.status);
    }

    collapsed
        .into_iter()
        .map(
            |((source, target), status)| super::types::NetworkTopologyLink {
                source,
                target,
                label: None,
                status,
            },
        )
        .collect()
}

fn collect_board_route_rows(
    nodes: &[super::types::NetworkTopologyNode],
    links: &[super::types::NetworkTopologyLink],
) -> Vec<(String, String, NetworkTopologyStatus, String)> {
    let labels = nodes
        .iter()
        .map(|node| (node.id.clone(), node.label.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut adjacency = BTreeMap::<String, Vec<(String, NetworkTopologyStatus)>>::new();
    for link in links {
        adjacency
            .entry(link.source.clone())
            .or_default()
            .push((link.target.clone(), link.status));
        adjacency
            .entry(link.target.clone())
            .or_default()
            .push((link.source.clone(), link.status));
    }

    let mut rows = nodes
        .iter()
        .filter(|node| node.kind == NetworkTopologyNodeKind::Board)
        .map(|node| {
            let upstream = adjacency.get(&node.id).and_then(|neighbors| {
                neighbors
                    .iter()
                    .find(|(neighbor, _)| {
                        nodes.iter().any(|candidate| {
                            candidate.id == *neighbor
                                && matches!(
                                    candidate.kind,
                                    NetworkTopologyNodeKind::Router
                                        | NetworkTopologyNodeKind::Board
                                )
                        })
                    })
                    .cloned()
            });
            let (upstream_label, status) = upstream
                .map(|(neighbor, status)| {
                    (labels.get(&neighbor).cloned().unwrap_or(neighbor), status)
                })
                .unwrap_or_else(|| ("--".to_string(), node.status));
            (
                node.label.clone(),
                upstream_label,
                status,
                node.sender_id.clone().unwrap_or_else(|| "--".to_string()),
            )
        })
        .collect::<Vec<_>>();

    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

fn yes_no(value: bool) -> String {
    if value {
        "yes".to_string()
    } else {
        "no".to_string()
    }
}
