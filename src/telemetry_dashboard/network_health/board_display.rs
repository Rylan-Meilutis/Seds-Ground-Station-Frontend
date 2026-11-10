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

/// One aspect of a board's health: reachable on the network, its actual
/// state, or its storage. Stale means the evidence is too old (or paused) to
/// call it either way; it is never shown as healthy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Health {
    Healthy,
    Unhealthy,
    Stale,
}
impl Health {
    fn key(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Unhealthy => "unhealthy",
            Self::Stale => "stale",
        }
    }
}

/// A short "Network · State · SD" line: name, its own indicator and a value.
pub(crate) fn health_line(key: &'static str, name: &str, health: Health, value: &str) -> Element {
    let badge = match health {
        Health::Healthy => health_badge(true),
        Health::Unhealthy => health_badge(false),
        Health::Stale => rsx! { strong { class: "gs26-board-health", style: "color:#ffdc8a;background:#3b2c11;padding:3px 9px;border-radius:999px;font-size:0.85em;", "Stale" } },
    };
    rsx! {
        div { class: "gs26-health-line", "data-health-line": key, "data-health": health.key(),
            style: "display:grid;grid-template-columns:4.5em auto minmax(0,1fr);align-items:center;gap:var(--gs-space-2,8px);min-width:0;font-size:var(--gs-text-sm,13px);",
            span { style: "color:var(--gs-text-muted,#9fb0c7);", "{name}" }
            {badge}
            span { style: "min-width:0;overflow-wrap:anywhere;", "{value}" }
        }
    }
}
