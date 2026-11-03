//! One dashboard-lifetime observer. Never sends a generic sequence command.
use super::{
    auth,
    prelude::*,
    sequence_progress_model::{Envelope, Model, RunKey, Snapshot},
    sequence_progress_view::SequenceProgressView,
};

/// Also consumed by Actions so the dashboard has one GSE status poll.
#[derive(Clone, Default, PartialEq)]
pub struct StatusObservation {
    pub value: serde_json::Value,
    pub requested_at_ms: f64,
    pub source_epoch: u64,
    pub error: Option<String>,
}
fn now_ms() -> u64 {
    super::monotonic_now_ms().max(0.0) as u64
}
async fn delay(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
}
#[derive(Clone, PartialEq)]
struct Source {
    epoch: u64,
    backend: String,
    token: Option<String>,
}
fn source() -> Source {
    Source {
        epoch: *super::WS_EPOCH.read(),
        backend: super::UrlConfig::base_http(),
        token: auth::current_token(),
    }
}

#[component]
pub fn SequenceProgressHost(
    mut state: Signal<Model>,
    mut status: Signal<StatusObservation>,
    mut open_result: Signal<Option<RunKey>>,
    on_actions: bool,
) -> Element {
    let mut clock = use_signal(now_ms);
    let mut cancel_api = use_signal(|| None::<u16>);
    use_effect(use_reactive((&on_actions,), move |(on_actions,)| {
        state.write().set_surface(on_actions)
    }));
    use_future(move || async move {
        loop {
            let now = now_ms();
            clock.set(now);
            state.write().tick(now);
            delay(250).await;
        }
    });
    use_future(move || async move {
        loop {
            let origin = source();
            state.write().set_source(origin.epoch);
            if auth::can_view_data() {
                let started = super::monotonic_now_ms();
                let response = super::http_get_json::<serde_json::Value>("/api/gse/status").await;
                if source() != origin {
                    state.write().set_source(source().epoch);
                    cancel_api.set(None);
                    continue;
                }
                match response {
                    Ok(value) => {
                        status.set(StatusObservation {
                            value: value.clone(),
                            requested_at_ms: started,
                            source_epoch: origin.epoch,
                            error: None,
                        });
                        let envelope =
                            serde_json::from_value::<Envelope>(value["sequence_progress"].clone());
                        match envelope {
                            Ok(envelope) => {
                                let version = envelope.cancel_api_version;
                                if state
                                    .write()
                                    .accept_envelope(
                                        origin.epoch,
                                        envelope,
                                        started.max(0.0) as u64,
                                        now_ms(),
                                    )
                                    .is_ok()
                                {
                                    cancel_api.set(Some(version));
                                    let needed = state.write().needed_result_details();
                                    for key in needed {
                                        let captured = origin.clone();
                                        spawn(async move {
                                            let path = format!(
                                                "/api/gse/sequences/results/{}/{}/{}",
                                                key.backend_instance, key.reset_epoch, key.run_id
                                            );
                                            let result =
                                                super::http_get_json::<Snapshot>(&path).await;
                                            if source() != captured {
                                                return;
                                            }
                                            match result {
                                                Ok(snapshot)
                                                    if snapshot.key == key
                                                        && snapshot.details_complete =>
                                                {
                                                    let received = now_ms();
                                                    if state
                                                        .write()
                                                        .accept(
                                                            captured.epoch,
                                                            snapshot,
                                                            received,
                                                            received,
                                                        )
                                                        .is_err()
                                                    {
                                                        state.write().unavailable = true;
                                                    }
                                                }
                                                _ => {
                                                    state.write().result_error=Some("Recorded sequence details are unavailable. Reopen the result from Notifications to retry.".into());
                                                }
                                            }
                                        });
                                    }
                                } else {
                                    state.write().unavailable = true;
                                    cancel_api.set(None);
                                }
                            }
                            Err(_) => {
                                state.write().unavailable = true;
                                cancel_api.set(None);
                            }
                        }
                    }
                    Err(error) => {
                        status.set(StatusObservation {
                            value: serde_json::json!({"unavailable":true}),
                            requested_at_ms: f64::NAN,
                            source_epoch: origin.epoch,
                            error: Some(error.to_string()),
                        });
                        state.write().unavailable = true;
                        cancel_api.set(None);
                    }
                }
            } else {
                state.write().unavailable = true;
                cancel_api.set(None);
                status.set(StatusObservation::default());
            }
            delay(1000).await;
        }
    });
    use_effect(move || {
        let Some(key) = open_result.read().clone() else {
            return;
        };
        open_result.set(None);
        let origin = source();
        let cached = state.read().cached_result(&key).cloned();
        if let Some(value) = cached {
            let _ = state.write().open_result(origin.epoch, value, now_ms());
            return;
        }
        spawn(async move {
            // Validated identifiers are the only path components. No command fallback.
            if !key.valid() {
                state.write().unavailable = true;
                return;
            }
            let path = format!(
                "/api/gse/sequences/results/{}/{}/{}",
                key.backend_instance, key.reset_epoch, key.run_id
            );
            let response = super::http_get_json::<Snapshot>(&path).await;
            if source() != origin {
                return;
            }
            match response {
                Ok(snapshot) if snapshot.key == key => {
                    if state
                        .write()
                        .open_result(origin.epoch, snapshot, now_ms())
                        .is_err()
                    {
                        state.write().unavailable = true;
                    }
                }
                _ => {
                    state.write().result_error=Some("This sequence result is unavailable: it was pruned, the backend restarted, or this station does not retain it.".into());
                }
            }
        });
    });
    let panels = state.read().panels().to_vec();
    let unavailable = state.read().unavailable;
    let result_error = state.read().result_error.clone();
    rsx! {
        div {class:"gs26-sequence-progress-host",
            if let Some(error)=result_error {
                div {class:"gs26-sequence-unknown",role:"alert",
                    p {"{error}"}
                    button {r#type:"button",onclick:move |_|state.write().result_error=None,"Close result notice"}
                }
            }
            for (index,panel) in panels.into_iter().enumerate() {
                div {key:"{panel.snapshot.key.run_id}",style:"--sequence-panel-index:{index};",
                    SequenceProgressView {
                        panel,now_ms:clock(),can_send_commands:auth::can_send_command("CancelFill"),cancel_api_version:cancel_api(),unavailable,
                        on_minimize:move |key|state.write().minimize(&key),
                        on_restore:move |key|state.write().restore(&key),
                        on_dismiss:move |key|{state.write().dismiss(&key);},
                        on_cancel:move |key:RunKey| {
                            let origin=source();
                            let intent=state.write().request_cancel(&key,now_ms(),auth::can_send_command("CancelFill"),cancel_api());
                            if let Ok(intent)=intent {
                                spawn(async move {
                                    let result=super::http_post_json::<RunKey,serde_json::Value>("/api/gse/sequences/cancel",&intent).await;
                                    if source()!=origin {return;}
                                    if result.is_err(){state.write().cancel_failed(&intent);state.write().unavailable=true;}
                                    // HTTP acceptance never means cancelled; actual producer observation must confirm.
                                });
                            }
                        },
                    }
                }
            }
        }
    }
}
