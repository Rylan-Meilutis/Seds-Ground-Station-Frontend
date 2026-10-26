use dioxus::prelude::*;
use serde::Deserialize;
#[derive(Clone, Default, Deserialize)]
struct Status {
    enabled: bool,
    role: Option<String>,
    forwarding: bool,
    #[serde(default)]
    relay_requested: bool,
}
#[component]
pub(super) fn RecoveryStatus() -> Element {
    let mut status = use_signal(Status::default);
    use_future(move || async move {
        loop {
            if let Ok(next) = super::http_get_json::<Status>("/api/rrls/status").await {
                status.set(next);
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(1000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    let state = status.read();
    if !state.enabled {
        return rsx! {};
    }
    let label = match (state.role.as_deref(), state.forwarding) {
        (Some("rrls"), true) => "RRLS · recovery relay active · local controls read-only",
        (Some("rrls"), false) => "RRLS · receiving AV-bay data · local controls read-only",
        (_, true) => "GroundStation · recovery relay path",
        _ if state.relay_requested => "GroundStation · waiting for recovery relay readiness",
        _ => "GroundStation · direct AV-bay path",
    };
    rsx! {div {class:"gs26-recovery-status",role:"status",style:"flex:0 0 auto;padding:5px 12px;font-size:12px;border-bottom:1px solid #435165;", "{label}"}}
}
