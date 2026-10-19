//! Deterministic browser preview of the real health cards, with no hardware actions.
#![allow(dead_code)]
use dioxus::prelude::*;
#[path = "../src/telemetry_dashboard/layout.rs"]
mod layout;
#[path = "../src/telemetry_dashboard/network_health.rs"]
mod network_health;
#[path = "../src/telemetry_dashboard/types.rs"]
mod types;
fn monotonic_now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0.0
    }
}
mod connection_status_tab {
    use dioxus::prelude::*;
    pub fn use_board_age_clock() -> i64 {
        let mut now = use_signal(|| super::monotonic_now_ms() as i64);
        use_future(move || async move {
            loop {
                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::TimeoutFuture::new(100).await;
                #[cfg(not(target_arch = "wasm32"))]
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                now.set(super::monotonic_now_ms() as i64);
            }
        });
        *now.read()
    }
}
#[component]
fn App() -> Element {
    let mut connected = use_signal(|| true);
    let mut empty = use_signal(|| false);
    let rows = if empty() {
        vec![]
    } else {
        vec![
            types::NetworkBoardHealthEntry {
                sender_id: "VB".into(),
                healthy: true,
                observed_ms: 100_000,
                rx_observations: 100,
                diagnostics: Some(types::BoardDiagnostics {
                    version: 1,
                    received_age_ms: 20_000,
                    uptime_ticks: 12_345_000,
                    uptime_frequency_hz: 1000,
                    reset_flags: 0x20000000,
                    heap_live_bytes: Some(40960),
                    heap_peak_bytes: Some(81920),
                    heap_region_bytes: Some(131072),
                    allocation_failures: Some(0),
                    can_rx_drops: Some(17),
                    can_tx_rejected: Some(4),
                    uart_rx_drops: Some(2),
                    tx_queue_high_water: Some(24),
                    sd_accepted_records: Some(12345),
                    sd_processed_records: Some(12000),
                    sd_rejected_records: Some(345),
                    sd_flushed_records: Some(11000),
                    sd_write_errors: Some(1),
                    adc_overruns: Some(15),
                    diagnostic_send_failures: 2,
                    ..Default::default()
                }),
                ..Default::default()
            },
            types::NetworkBoardHealthEntry {
                sender_id: "New-runtime-sensor-with-long-name".into(),
                healthy: false,
                age_ms: 20_000,
                disconnects: 3,
                reconnects: 2,
                observed_ms: 100_000,
                offline_ms: 18_000,
                longest_outage_ms: 10_000,
                current_outage_ms: 8000,
                ..Default::default()
            },
        ]
    };
    rsx! {main {style:"padding:12px; font-family:system-ui;",
        button { onclick:move |_|connected.toggle(), "Toggle stream" }
        button { onclick:move |_|empty.toggle(), "Toggle empty" }
        network_health::NetworkHealthSection {rows, ws_connected:connected(), generated_ms:1, simulated:true, theme:layout::ThemeConfig::default()}
    }}
}
fn main() {
    dioxus::launch(App);
}
