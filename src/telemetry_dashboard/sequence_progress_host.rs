//! One dashboard-lifetime observer. Never sends a generic sequence command.
use super::{
    auth,
    prelude::*,
    sequence_progress_model::{Control, Envelope, Model, Panel, RunKey, Snapshot},
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
    pub(super) read_context: Option<super::incoming_epoch::ReadContext>,
}
impl StatusObservation {
    pub(super) fn current_binding(&self) -> bool {
        self.read_context.as_ref().is_some_and(|context| context.check_current().is_ok())
            && self.data_epoch == super::frontend_data_clear_epoch()
            && self.binding.as_ref()
                == Some(&super::board_recovery_guard::ConnectionBinding::current())
            && self
                .identity
                .as_ref()
                .is_some_and(crate::auth::request_identity_is_current)
    }
}
/// Recent sequence observations for this station, shown as a dashboard tab.
#[component]
pub(super) fn SequenceHistoryTab(mut state: Signal<Model>, mut open_result: Signal<Option<RunKey>>) -> Element {
    let history = state
        .read()
        .history()
        .into_iter()
        .map(|(key, title, outcome, timestamp)| {
            (format!("{}:{}:{}", key.backend_instance, key.reset_epoch, key.run_id), key, title, outcome, timestamp)
        })
        .collect::<Vec<_>>();
    rsx! {
        section {class:"gs26-sequence-history-tab", "aria-label":"Sequence history",
            header {class:"gs-section-header",
                h2 {"Sequence history"}
                details {class:"gs-info", summary {"About"} p {"Recent observations in this session. This is not a permanent sequence archive."}}
            }
            if !auth::can_view_data() {
                p {class:"gs-notice","Sequence history requires permission to view data."}
            } else if history.is_empty() {
                p {class:"gs-notice","No sequence observations are retained for this station."}
            } else {
                div {class:"gs26-sequence-history-list",
                    for (identity,key,title,outcome,timestamp) in history {
                        article {key:"{identity}",class:"gs-card gs26-sequence-history-item",
                            div {
                                strong {"{title}"}
                                div {class:"gs26-sequence-history-meta",
                                    span {class:"gs-badge","{super::sequence_progress_view::outcome_label(outcome)}"}
                                    if let Some(timestamp)=timestamp { span {"Completed {super::format_timestamp_ms_local_datetime(timestamp)}"} }
                                }
                            }
                            button {r#type:"button",class:"gs-btn",onclick:move |_|{if outcome.terminal(){open_result.set(Some(key.clone()));}else{state.write().restore(&key);}},
                                if outcome.terminal(){"Open result"}else{"Open progress"}
                            }
                        }
                    }
                }
            }
        }
    }
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
    let mut observed_policy = use_signal(|| None::<super::station_policy_context::Lease>);
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
                    observed_policy.set(None);
                    transient_transport_error.set(false);
                    state.write().retire_source();
                    status.set(StatusObservation::default());
                }
                if auth::can_view_data() {
                    let policy = super::station_policy_context::capture_current().ok();
                    if policy.is_none() { observed_policy.set(None); }
                    let read_context = match super::incoming_epoch::ReadContext::capture() {
                        Ok(context) => context,
                        Err(_) => {
                            observed_policy.set(None); state.write().unavailable = true;
                            status.set(StatusObservation::default()); cancel_api.set(None);
                            delay(1000).await; continue;
                        }
                    };
                    let started = super::monotonic_now_ms();
                    let observation = super::telemetry_reseed_continuity::begin_observation();
                    let request_alive = alive.clone();
                    let owner =
                        super::recording_download::ExportOwner::capture_selected(move || {
                            request_alive.load(std::sync::atomic::Ordering::Acquire)
                        });
                    let response = match &owner {
                        Ok(owner) => {
                            super::recording_download::saved_json_classified_in_context::<serde_json::Value>(
                                "/api/gse/status",
                                owner.clone(), read_context.clone(),
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
                    if read_context.check_current().is_err()
                        || owner.as_ref().is_ok_and(|owner| !owner.is_current())
                        || source() != origin
                        || observation
                            .as_ref()
                            .is_none_or(|request| !request.current())
                    {
                        delay(1000).await; continue;
                    }
                    // Preserve the original in-flight policy before any new
                    // status/readback projection can become action evidence.
                    // Read-only status remains valid during Closing. A retired
                    // ordinary owner only disables mutators; never spin/rebind it.
                    observed_policy.set(policy.filter(|lease| lease.check_current().is_ok()));
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
                                            read_context:Some(read_context.clone()),
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
                                    read_context: None,
                                });
                            }
                            state.write().unavailable = true;
                            cancel_api.set(None);
                        }
                    }
                } else {
                    observed_policy.set(None);
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
            let model = state.read();
            let controls = model.controls(&panel.snapshot.key).cloned();
            let key = &panel.snapshot.key;
            let can_pause = model.can_control(key, Control::Pause, render_now, auth::can_send_command("PauseFill"));
            let can_resume = model.can_control(key, Control::Resume, render_now, auth::can_send_command(resume_command(panel.snapshot.kind)));
            drop(model);
            (
                panel.clone(),
                (can_release, controls, can_pause, can_resume),
                format!(
                    "{}:{}:{}",
                    panel.snapshot.key.backend_instance,
                    panel.snapshot.key.reset_epoch,
                    panel.snapshot.key.run_id
                ),
            )
        })
        .collect();
    let ordinary_current = observed_policy.read().as_ref().is_some_and(|lease| lease.check_current().is_ok());
    let unavailable = state.read().unavailable || !status.read().current_binding();
    let panel_policy_key = super::station_mode_observer::view_key();
    let transient_transport_error = transient_transport_error();
    let result_error = state.read().result_error.clone();
    rsx! {
        div {class:"gs26-sequence-progress-host",
            if let Some(error)=result_error {
                div {class:"gs26-sequence-unknown",role:"alert",
                    p {"{error}"}
                    button {r#type:"button",onclick:move |_|state.write().result_error=None,"Close result notice"}
                }
            }
            for (index,(panel,(can_release_cold_flow,controls,can_pause,can_resume),identity)) in panels.into_iter().enumerate() {
                div {key:"{identity}:{panel_policy_key}",style:"--sequence-panel-index:{index};",
                    OwnedSequenceProgressView {
                        panel,now_ms:render_now,can_send_commands:ordinary_current&&auth::can_send_command("CancelFill"),cancel_api_version:cancel_api(),unavailable,
                        can_release_cold_flow:ordinary_current&&can_release_cold_flow,transient_transport_error,
                        controls,can_pause:ordinary_current&&can_pause,can_resume:ordinary_current&&can_resume,
                        on_control:move |(owned,control):(OwnedRunIntent,Control)|{
                            let OwnedRunIntent {key, owner}=owned;
                            if !owner.is_current(){return;}
                            let origin=source();
                            let permission=match control {Control::Pause=>auth::can_send_command("PauseFill"),Control::Resume=>state.read().panels().iter().find(|p|p.snapshot.key==key).is_some_and(|p|auth::can_send_command(resume_command(p.snapshot.kind)))};
                            let intent=state.write().request_control(&key,control,now_ms(),permission);
                            if let Ok(intent)=intent {
                                spawn(async move {
                                    let path=match control {Control::Pause=>"/api/gse/sequences/pause",Control::Resume=>"/api/gse/sequences/resume"};
                                    let result=super::station_settings_request::post_captured::<RunKey,serde_json::Value>(path,&intent,Ok(owner.clone())).await;
                                    if !owner.is_current()||source()!=origin {return;}
                                    if let Err(error)=result{state.write().control_failed(&intent);state.write().result_error=Some(format!("{} refused: {error}",match control{Control::Pause=>"Pause",Control::Resume=>"Resume"}));}
                                    // Acceptance is not completion; the next station revision confirms it.
                                });
                            }
                        },
                        board_display:state.read().board_display.clone(),
                        output_display:state.read().output_display(),
                        on_release_cold_flow:move |owned:OwnedRunIntent|{
                            let OwnedRunIntent {key, owner}=owned;
                            if !owner.is_current(){return;}
                            let origin=source();
                            let release = { state.write().request_release(&key,now_ms(),auth::can_send_command("ColdFlow")) };
                            if let Ok(intent)=release {
                                spawn(async move {
                                    let result=super::station_settings_request::post_captured::<_,serde_json::Value>("/api/gse/cold-flow/release",&intent,Ok(owner.clone())).await;
                                    if !owner.is_current()||source()!=origin{return;}
                                    if let Err(error)=result{state.write().release_failed(&intent);state.write().result_error=Some(format!("Cold Flow release refused: {error}"));}
                                    // Acceptance is not reported output or measured fluid flow.
                                });
                            }
                        },
                        on_minimize:move |key|state.write().minimize(&key),
                        on_restore:move |key|state.write().restore(&key),
                        on_dismiss:move |key|{state.write().dismiss(&key);},
                        on_cancel:move |owned:OwnedRunIntent| {
                            let OwnedRunIntent {key, owner}=owned;
                            if !owner.is_current(){return;}
                            let origin=source();
                            let intent=state.write().request_cancel(&key,now_ms(),auth::can_send_command("CancelFill"),cancel_api());
                            if let Ok(intent)=intent {
                                spawn(async move {
                                    let result=super::station_settings_request::post_captured::<RunKey,serde_json::Value>("/api/gse/sequences/cancel",&intent,Ok(owner.clone())).await;
                                    if !owner.is_current()||source()!=origin {return;}
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

/// Each actual run panel retains its first mounted ordinary owner. The global
/// observer/results model survives; changing policy never rebinds this owner.
/// The command permission a Resume needs, mirroring the station's check.
fn resume_command(kind: super::sequence_progress_model::Kind) -> &'static str {
    use super::sequence_progress_model::Kind;
    match kind {
        Kind::Fill => "StartFill",
        Kind::ValveSelfTest => "ValveSelfTest",
        Kind::NitrogenTest => "NitrogenTest",
        Kind::ColdFlow => "AutoColdFlow",
        _ => "Turbocharge",
    }
}
#[derive(Clone)]
struct OwnedRunIntent {
    key: RunKey,
    owner: std::rc::Rc<super::recording_download::ExportOwner>,
}
#[component]
fn OwnedSequenceProgressView(
    panel: Panel,
    now_ms: u64,
    can_send_commands: bool,
    cancel_api_version: Option<u16>,
    unavailable: bool,
    can_release_cold_flow: bool,
    transient_transport_error: bool,
    board_display: super::sequence_progress_model::BoardDisplayMap,
    output_display: super::sequence_progress_model::OutputDisplayMap,
    on_minimize: EventHandler<RunKey>,
    on_restore: EventHandler<RunKey>,
    on_dismiss: EventHandler<RunKey>,
    on_cancel: EventHandler<OwnedRunIntent>,
    on_release_cold_flow: EventHandler<OwnedRunIntent>,
    controls: Option<super::sequence_progress_model::RunControls>,
    can_pause: bool,
    can_resume: bool,
    on_control: EventHandler<(OwnedRunIntent, Control)>,
) -> Element {
    let original = super::station_settings_request::use_factory();
    rsx! {SequenceProgressView {
        panel, now_ms, can_send_commands, cancel_api_version,
        unavailable, can_release_cold_flow, transient_transport_error, board_display, output_display,
        on_minimize, on_restore, on_dismiss, controls, can_pause, can_resume,
        on_pause: move |key:RunKey| {
            if let Ok(owner)=original.capture() { on_control.call((OwnedRunIntent {key,owner},Control::Pause)); }
        },
        on_resume: move |key:RunKey| {
            if let Ok(owner)=original.capture() { on_control.call((OwnedRunIntent {key,owner},Control::Resume)); }
        },
        on_cancel: move |key:RunKey| {
            if let Ok(owner)=original.capture() { on_cancel.call(OwnedRunIntent {key,owner}); }
        },
        on_release_cold_flow: move |key:RunKey| {
            if let Ok(owner)=original.capture() { on_release_cold_flow.call(OwnedRunIntent {key,owner}); }
        },
    }}
}
