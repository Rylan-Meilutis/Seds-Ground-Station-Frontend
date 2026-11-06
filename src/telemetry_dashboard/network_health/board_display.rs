use dioxus::prelude::*;

pub(crate) fn network_healthy(connected: bool, server_healthy: bool) -> bool {
    // The backend owns health policy; displayed age is diagnostic only.
    connected && server_healthy
}

pub(crate) fn health_badge(healthy: bool) -> Element {
    let (label, colors) = if healthy {
        ("Healthy", "color:#86efac;background:#123b27;")
    } else {
        ("Unhealthy", "color:#fecaca;background:#471e25;")
    };
    rsx! { strong { class: "gs26-board-health", style: "{colors}padding:3px 9px;border-radius:999px;font-size:0.85em;", "{label}" } }
}
