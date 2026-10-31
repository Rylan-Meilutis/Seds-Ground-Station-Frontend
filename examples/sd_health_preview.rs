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
    let mut flags = use_signal(|| 3u32);
    let mut write_age = use_signal(|| Some(200u32));
    let mut message = use_signal(String::new);
    let report = types::BoardDiagnostics {
        sd_flags:Some(flags()), sd_status_received_age_ms:Some(0),
        sd_last_write_age_ms:write_age(), sd_last_flush_age_ms:Some(1000),
        sd_accepted_records:Some(120), sd_processed_records:Some(110),
        sd_flushed_records:Some(100), sd_write_errors:Some(0),
        sd_init_failures:Some(if flags() & 2 == 0 {1} else {0}),
        sd_retry_count:Some(1), ..Default::default()
    };
    rsx! {
        div { style:"padding:12px; background:#060b13; font:16px system-ui,sans-serif; min-height:100vh; box-sizing:border-box;",
            button { onclick:move |_| {flags.set(1);write_age.set(Some(20_000));}, "Fail SD" }
            button { onclick:move |_| {flags.set(3);write_age.set(Some(20_000));}, "Stall SD" }
            network_health::NetworkHealthSection {
                rows:vec![types::NetworkBoardHealthEntry {
                    sender_id:"DAQ".into(), healthy:true, diagnostics:Some(report), ..Default::default()
                }], ws_connected:true, generated_ms:0, simulated:true,
                theme:layout::ThemeConfig::default(), sd_retry_allowed:true,
                sd_retry_message:message(), on_sd_retry:move |_| {
                    flags.set(9); message.set("Retry command queued; waiting for DAQ diagnostics.".into());
                }
            }
        }
    }
}
fn main() { dioxus::launch(App); }
