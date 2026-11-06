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
    pub(super) binding: Option<super::board_recovery_guard::ConnectionBinding>,
    pub(super) identity: Option<crate::auth::RequestIdentity>,
    pub(super) data_epoch: u64,
}
impl StatusObservation {
    pub(super) fn current_binding(&self) -> bool {
        self.data_epoch == super::frontend_data_clear_epoch()
            && self.binding.as_ref()
                == Some(&super::board_recovery_guard::ConnectionBinding::current())
            && self
                .identity
                .as_ref()
                .is_some_and(crate::auth::request_identity_is_current)
    }
}
pub(super) static HISTORY_OPEN: GlobalSignal<bool> = Signal::global(|| false);
pub(super) fn open_history() {
    *HISTORY_OPEN.write() = true;
}
fn now_ms() -> u64 {
    super::monotonic_now_ms().max(0.0) as u64
}
pub(super) async fn delay(ms: u32) {
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
    identity: Option<crate::auth::RequestIdentity>,
}
fn source() -> Source {
    Source {
        epoch: *super::WS_EPOCH.read(),
        backend: super::UrlConfig::base_http(),
        token: auth::current_token(),
        identity: auth::request_identity(),
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
    let mut transient_transport_error = use_signal(|| false);
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
    let status_alive = super::recording_download::use_export_lifetime();
    use_future(move || {
        let alive = status_alive.clone();
        async move {
            let mut previous_source = None::<Source>;
            loop {
                if !alive.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                let origin = source();
                let changed = previous_source.as_ref().is_some_and(|old| old != &origin);
                previous_source = Some(origin.clone());
                state.write().set_source(origin.epoch);
                if changed {
                    transient_transport_error.set(false);
                    state.write().retire_source();
                    status.set(StatusObservation::default());
                    *HISTORY_OPEN.write() = false;
                }
                if auth::can_view_data() {
                    let started = super::monotonic_now_ms();
                    let observation = super::telemetry_reseed_continuity::begin_observation();
                    let request_alive = alive.clone();
                    let owner =
                        super::recording_download::ExportOwner::capture_selected(move || {
                            request_alive.load(std::sync::atomic::Ordering::Acquire)
                        });
                    let response = match &owner {
                        Ok(owner) => {
                            super::recording_download::saved_json_classified::<serde_json::Value>(
                                "/api/gse/status",
                                owner.clone(),
                            )
                            .await
                        }
                        Err(error) => Err(super::recording_download::SavedJsonError::from(
                            error.clone(),
                        )),
                    };
                    if !alive.load(std::sync::atomic::Ordering::Acquire) {
                        return;
                    }
                    if owner.as_ref().is_ok_and(|owner| !owner.is_current())
                        || source() != origin
                        || observation
                            .as_ref()
                            .is_none_or(|request| !request.current())
                    {
                        continue;
                    }
                    match response {
                        Ok(value) => {
                            transient_transport_error.set(false);

                            let envelope = serde_json::from_value::<Envelope>(
                                value["sequence_progress"].clone(),
                            );
                            match envelope {
                                Ok(envelope) => {
                                    let authority = (
                                        envelope.backend_instance.clone(),
                                        envelope.reset_epoch.clone(),
                                    );
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
                                        status.set(StatusObservation {
                                            value: value.clone(),
                                            requested_at_ms: started,
                                            source_epoch: origin.epoch,
                                            error: None,
                                            binding:Some(super::board_recovery_guard::ConnectionBinding::current()),
                                            identity:crate::auth::request_identity(),
                                            data_epoch:super::frontend_data_clear_epoch(),
                                        });
                                        if let Some(request) = observation.as_ref() {
                                            super::telemetry_reseed_continuity::observe(
                                                request,
                                                authority.0,
                                                authority.1,
                                            );
                                        }
                                        cancel_api.set(Some(version));
                                        let needed = state.write().needed_result_details();
                                        for key in needed {
                                            let captured = origin.clone();
                                            spawn(async move {
                                                let path = format!(
                                                    "/api/gse/sequences/results/{}/{}/{}",
                                                    key.backend_instance,
                                                    key.reset_epoch,
                                                    key.run_id
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
                                        status.set(StatusObservation::default());
                                        cancel_api.set(None);
                                    }
                                }
                                Err(_) => {
                                    state.write().unavailable = true;
                                    status.set(StatusObservation::default());
                                    cancel_api.set(None);
                                }
                            }
                        }
                        Err(error) => {
                            transient_transport_error.set(error.transient);
                            if error.transient {
                                // Preserve original request timestamp: a retry cannot mint validity.
                                status.write().error = Some(error.message);
                            } else {
                                status.set(StatusObservation {
                                    value: serde_json::json!({"unavailable":true}),
                                    requested_at_ms: f64::NAN,
                                    source_epoch: origin.epoch,
                                    error: Some(error.message),
                                    binding: None,
                                    identity: None,
                                    data_epoch: 0,
                                });
                            }
                            state.write().unavailable = true;
                            cancel_api.set(None);
                        }
                    }
                } else {
                    *HISTORY_OPEN.write() = false;
                    transient_transport_error.set(false);
                    state.write().unavailable = true;
                    cancel_api.set(None);
                    status.set(StatusObservation::default());
                }
                delay(1000).await;
            }
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
    // The periodic signal schedules expiry updates. Sample the actual clock after
    // reading a new response, rather than comparing it against an older tick.
    let _clock_tick = clock();
    let render_now = now_ms();
    let panels: Vec<_> = state
        .read()
        .panels()
        .iter()
        .filter(|panel| panel.visible)
        .map(|panel| {
            let can_release = state.read().can_release(
                &panel.snapshot.key,
                render_now,
                auth::can_send_command("ColdFlow"),
            );
            (
                panel.clone(),
                can_release,
                format!(
                    "{}:{}:{}",
                    panel.snapshot.key.backend_instance,
                    panel.snapshot.key.reset_epoch,
                    panel.snapshot.key.run_id
                ),
            )
        })
        .collect();
    let unavailable = state.read().unavailable;
    let transient_transport_error = transient_transport_error();
    let result_error = state.read().result_error.clone();
    let history = if *HISTORY_OPEN.read() {
        state
            .read()
            .history()
            .into_iter()
            .map(|(key, title, outcome, timestamp)| {
                (
                    format!(
                        "{}:{}:{}",
                        key.backend_instance, key.reset_epoch, key.run_id
                    ),
                    key,
                    title,
                    outcome,
                    timestamp,
                )
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    rsx! {
        div {class:"gs26-sequence-progress-host",
            if *HISTORY_OPEN.read()&&auth::can_view_data(){
                div{class:"gs26-sequence-history-overlay",style:"position:fixed;inset:0;z-index:6200;background:#000a;padding:16px;display:flex;align-items:center;justify-content:center;box-sizing:border-box;",onkeydown:move|event|{event.stop_propagation();match event.key(){Key::Escape=>{event.prevent_default();*HISTORY_OPEN.write()=false;},Key::Tab=>{event.prevent_default();super::board_recovery_panel::cycle_modal_focus("gs26-sequence-history",event.modifiers().contains(Modifiers::SHIFT));},_=>{}}},
                    section{id:"gs26-sequence-history",role:"dialog","aria-modal":"true","aria-label":"Sequence history",style:"background:var(--gs26-panel-background,#12202e);color:var(--gs26-app-text,#e7eef6);border:1px solid #496079;border-radius:14px;padding:20px;width:min(100%,960px);height:min(86dvh,860px);max-height:100%;display:flex;flex-direction:column;gap:14px;box-sizing:border-box;",
                        header{style:"display:flex;justify-content:space-between;align-items:center;gap:12px;",h2{style:"margin:0;","Sequence history"}button{r#type:"button",onmounted:move|event|async move{let _=event.data().set_focus(true).await;},onclick:move |_|*HISTORY_OPEN.write()=false,"Close history"}}
                        p{style:"margin:0;","Recent observations in this session. This is not a permanent sequence archive."}
                        div{class:"gs26-modal-scroll-body",style:"flex:1;min-height:0;overflow-y:auto;overflow-anchor:none;display:grid;gap:10px;align-content:start;",
                            if history.is_empty(){p{"No sequence observations are retained for this station."}}
                            for (identity,key,title,outcome,timestamp) in history {article{key:"{identity}",style:"border:1px solid #496079;border-radius:10px;padding:12px;display:flex;justify-content:space-between;align-items:center;gap:12px;flex-wrap:wrap;",div{strong{"{title}"}p{style:"margin:4px 0;","{super::sequence_progress_view::outcome_label(outcome)}"}if let Some(timestamp)=timestamp{p{style:"margin:4px 0;","Reported completion: {super::format_timestamp_ms_local_datetime(timestamp)}"}}}button{r#type:"button",onclick:move |_|{if outcome.terminal(){open_result.set(Some(key.clone()));}else{state.write().restore(&key);}*HISTORY_OPEN.write()=false;},if outcome.terminal(){"Open result"}else{"Open progress"}}}}
                        }
                    }
                }
            }
            if let Some(error)=result_error {
                div {class:"gs26-sequence-unknown",role:"alert",
                    p {"{error}"}
                    button {r#type:"button",onclick:move |_|state.write().result_error=None,"Close result notice"}
                }
            }
            for (index,(panel,can_release_cold_flow,identity)) in panels.into_iter().enumerate() {
                div {key:"{identity}",style:"--sequence-panel-index:{index};",
                    SequenceProgressView {
                        panel,now_ms:render_now,can_send_commands:auth::can_send_command("CancelFill"),cancel_api_version:cancel_api(),unavailable,
                        can_release_cold_flow,transient_transport_error,
                        board_display:state.read().board_display.clone(),
                        output_display:state.read().output_display(),
                        on_release_cold_flow:move |key:RunKey|{
                            let origin=source();
                            let release = { state.write().request_release(&key,now_ms(),auth::can_send_command("ColdFlow")) };
                            if let Ok(intent)=release {
                                spawn(async move {
                                    let result=super::http_post_json::<RunKey,serde_json::Value>("/api/gse/cold-flow/release",&intent).await;
                                    if source()!=origin{return;}
                                    if let Err(error)=result{state.write().release_failed(&intent);state.write().result_error=Some(format!("Cold Flow release refused: {error}"));}
                                    // Acceptance is not reported output or measured fluid flow.
                                });
                            }
                        },
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
