use super::{layout::ThemeConfig, types::NetworkBoardHealthEntry};
use dioxus::prelude::*;
const TIMEOUT_MS: u64 = 12_000;
fn healthy(row: &NetworkBoardHealthEntry, connected: bool, elapsed: u64) -> bool {
    connected && row.healthy && row.age_ms.saturating_add(elapsed) <= TIMEOUT_MS
}
fn duration(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
    }
}
#[component]
pub fn NetworkHealthSection(
    rows: Vec<NetworkBoardHealthEntry>,
    ws_connected: bool,
    generated_ms: u64,
    simulated: bool,
    theme: ThemeConfig,
) -> Element {
    let now = super::connection_status_tab::use_board_age_clock();
    // Anchor each server snapshot to local monotonic time; UTC adjustments do not age boards.
    let receipt = use_memo(use_reactive((&generated_ms,), move |(_generated,)| {
        super::monotonic_now_ms()
    }));
    let elapsed = (now as f64 - receipt()).max(0.0) as u64;
    rsx! {
        section { style: "padding:16px; border:1px solid {theme.border_soft}; border-radius:12px; min-width:0; color:{theme.text_primary}; background:{theme.panel_background};",
            h3 { style: "margin:0 0 6px;", "Network health" }
            p { style: "margin:0 0 12px; color:{theme.text_secondary}; font-size:0.85rem;",
                if simulated { "Simulated network · " }
                "Observed boards · history since GroundStation started"
            }
            if !ws_connected { p { role: "status", "GroundStation stream unavailable" } }
            div { style: "display:grid; gap:12px; grid-template-columns:repeat(auto-fit,minmax(min(100%,240px),1fr));",
                for row in &rows {
                    {let ok = healthy(row, ws_connected, elapsed);
                     let color = if ok { "#146c43" } else { "#9f1239" };
                     let age = duration(row.age_ms.saturating_add(elapsed));
                     let observed = row.observed_ms.max(1);
                     let availability = 100.0 * observed.saturating_sub(row.offline_ms) as f64 / observed as f64;
                     rsx! {
                        article { key: "{row.sender_id}", style: "padding:12px; border:1px solid {theme.border_soft}; border-radius:10px; min-width:0; overflow-wrap:anywhere;",
                            div { style: "display:flex; flex-wrap:wrap; gap:8px; justify-content:space-between; align-items:center;",
                                strong { "{row.sender_id}" }
                                span { style: "font-size:0.8rem; border:1px solid {color}; background:{color}; color:#ffffff; border-radius:999px; padding:3px 9px;",
                                    if ok { "Healthy" } else { "Unhealthy" }
                                }
                            }
                            dl { style: "display:grid; grid-template-columns:1fr auto; gap:8px; font-size:0.9rem; margin:12px 0 0;",
                                dt { "Last received" } dd { style: "margin:0;", "{age} ago" }
                                dt { "Disconnects" } dd { style: "margin:0;", "{row.disconnects}" }
                                dt { "Reconnects" } dd { style: "margin:0;", "{row.reconnects}" }
                                dt { "Availability" } dd { style: "margin:0;", "{availability:.1}%" }
                                dt { "Longest outage" } dd { style: "margin:0;", "{duration(row.longest_outage_ms)}" }
                                if row.current_outage_ms > 0 {
                                    dt { "Current outage" } dd { style: "margin:0;", "{duration(row.current_outage_ms.saturating_add(elapsed))}" }
                                }
                            }
                            details { style: "margin-top:12px; font-size:0.85rem;",
                                summary { style: "min-height:44px; display:flex; align-items:center; cursor:pointer;", "Health details" }
                                p { "Observed for {duration(row.observed_ms)} · Offline {duration(row.offline_ms)} · Receive observations {row.rx_observations}" }
                            }
                        }
                     }}
                }
            }
            if rows.is_empty() { p { "No boards observed in this mode." } }
            details { style: "margin-top:12px; font-size:0.85rem; color:{theme.text_secondary};",
                summary { style: "min-height:44px; display:flex; align-items:center; cursor:pointer;", "About health" }
                p { "Healthy means traffic was received within 12 seconds. Silence counts once as a disconnect; fresh traffic counts once as a reconnect. This measures network liveness, not internal sensor or CPU health. Receive observations may include multiple handlers for one packet. History resets when the backend restarts or simulation is reset." }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disconnected_stream_and_aged_snapshot_cannot_show_healthy() {
        let row = NetworkBoardHealthEntry {
            healthy: true,
            age_ms: 11_000,
            ..Default::default()
        };
        assert!(healthy(&row, true, 999));
        assert!(!healthy(&row, true, 1001));
        assert!(!healthy(&row, false, 0));
    }
    #[test]
    fn old_servers_default_to_an_empty_health_list() {
        let msg: super::super::types::NetworkTopologyMsg =
            serde_json::from_str(r#"{"generated_ms":1}"#).unwrap();
        assert!(msg.health.is_empty());
    }
}
