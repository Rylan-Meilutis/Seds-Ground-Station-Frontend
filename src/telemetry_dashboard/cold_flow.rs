//! Explicit primary-GS start only. PILOT release stays in the authoritative run modal.
use super::layout::ThemeConfig;
use super::sequence_progress_host::StatusObservation;
use super::sequence_progress_model::RunKey;
use super::{UrlConfig, WS_EPOCH, auth};
use dioxus::prelude::*;

pub(super) fn available(
    value: &serde_json::Value,
    received: f64,
    source_epoch: u64,
    current_epoch: u64,
    now: f64,
    permission: bool,
    primary: bool,
) -> bool {
    permission
        && primary
        && source_epoch == current_epoch
        && received.is_finite()
        && now.is_finite()
        && now >= received
        && value["cold_flow"]["backend_instance"]
            .as_str()
            .is_some_and(|backend| {
                backend.len() == 32
                    && backend.bytes().all(|c| c.is_ascii_hexdigit())
                    && value["sequence_progress"]["backend_instance"].as_str() == Some(backend)
            })
        && value["cold_flow"]["reset_epoch"]
            .as_str()
            .is_some_and(|epoch| {
                epoch.len() == 16
                    && epoch.bytes().all(|c| c.is_ascii_hexdigit())
                    && value["sequence_progress"]["reset_epoch"].as_str() == Some(epoch)
            })
        && value["cold_flow"]["version"].as_u64() == Some(1)
        && value["cold_flow"]["start_available"].as_bool() == Some(true)
}
// Observation-only: permission to start is independent of an owned active run.
fn active_label(
    value: &serde_json::Value,
    received: f64,
    source_epoch: u64,
    current_epoch: u64,
    now: f64,
) -> Option<&'static str> {
    if source_epoch != current_epoch
        || !received.is_finite()
        || !now.is_finite()
        || now < received
        || value["cold_flow"]["version"].as_u64() != Some(1)
        || value["cold_flow"]["active"].as_bool() != Some(true)
        || value["sequence_progress"]["version"].as_u64() != Some(1)
    {
        return None;
    }
    let key = serde_json::from_value::<RunKey>(value["cold_flow"]["key"].clone()).ok()?;
    if !key.valid()
        || value["sequence_progress"]["backend_instance"].as_str()
            != Some(key.backend_instance.as_str())
        || value["sequence_progress"]["reset_epoch"].as_str() != Some(key.reset_epoch.as_str())
    {
        return None;
    }
    match value["cold_flow"]["phase"].as_str()? {
        "awaiting_release" => Some("Awaiting release"),
        "pause_closing" | "parked_unverified" => Some("Paused"),
        "cancel_closing" | "cancel_relieving" => Some("Cancelling"),
        "isolating" | "pressurizing" | "confirming_hold" | "releasing" | "flowing" => {
            Some("In progress")
        }
        _ => None,
    }
}
#[component]
pub(super) fn AutoColdFlow(
    status: Signal<StatusObservation>,
    theme: ThemeConfig,
    #[props(default = false)] disabled: bool,
) -> Element {
    let mut clock = use_signal(super::monotonic_now_ms);
    let mut busy = use_signal(|| false);
    let mut pending = use_signal(|| None::<RunKey>);
    let alive = super::recording_download::use_export_lifetime();
    let requests = super::station_settings_request::use_factory_selected(move || alive.load(std::sync::atomic::Ordering::Acquire));
    let mut message = use_signal(String::new);
    use_future(move || async move {
        loop {
            clock.set(super::monotonic_now_ms());
            super::sequence_progress_host::delay(250).await;
        }
    });
    use_effect(move || {
        let value = status.read();
        let waiting = pending.peek().clone();
        if let Some(key) = waiting.as_ref() {
            let actual =
                serde_json::from_value::<RunKey>(value.value["cold_flow"]["key"].clone()).ok();
            let changed = value.value["sequence_progress"]["reset_epoch"]
                .as_str()
                .is_some_and(|r| r != key.reset_epoch)
                || value.value["sequence_progress"]["backend_instance"]
                    .as_str()
                    .is_some_and(|r| r != key.backend_instance);
            if actual.as_ref() == Some(key) || changed {
                pending.set(None);
            }
        }
    });
    let observed = status();
    // A response can arrive between periodic ticks. The tick only schedules
    // rendering; it must not make a newly received observation appear future-dated.
    let _clock_tick = clock();
    let render_now = super::monotonic_now_ms();
    let primary = auth::current_status().station_role.as_deref() != Some("rrls");
    let connected = *super::WS_CONNECTED_SIGNAL.read();
    let current_binding = observed.current_binding();
    let enabled = !disabled
        && connected
        && current_binding
        && !busy()
        && pending().is_none()
        && available(
            &observed.value,
            observed.requested_at_ms,
            observed.source_epoch,
            *WS_EPOCH.read(),
            render_now,
            auth::can_send_command("AutoColdFlow"),
            primary,
        );
    let reason = observed.value["cold_flow"]["reason"]
        .as_str()
        .unwrap_or("Auto Cold Flow is unavailable on this connection")
        .to_string();
    let progress = if connected && current_binding {
        active_label(
            &observed.value,
            observed.requested_at_ms,
            observed.source_epoch,
            *WS_EPOCH.read(),
            render_now,
        )
    } else {
        None
    };
    let working = busy() || pending().is_some();
    let badge = if working {
        "Starting"
    } else {
        progress.unwrap_or(if enabled { "" } else { "Disabled" })
    };
    let badge_visibility = if badge.is_empty() {
        "hidden"
    } else {
        "visible"
    };
    let style = super::actions_tab::btn_style(
        &theme.border,
        &theme.panel_background_alt,
        &theme.text_primary,
        enabled,
        super::BlinkMode::None,
        progress.map(|_| true),
        true,
    );
    rsx! {
        div {class:"gs26-cold-flow-start",style:"display:grid;gap:6px;min-width:0;",
            button {r#type:"button",disabled:!enabled,style:"{style}","data-sequence-command":"AutoColdFlow","data-sequence-state":progress.unwrap_or(""),aria_busy:working || progress.is_some(),onclick:move|_|{
                let current=status();let epoch=*WS_EPOCH.read();
                if !requests.current()||disabled||!*super::WS_CONNECTED_SIGNAL.peek()||!current.current_binding()||busy()||pending().is_some()||!available(&current.value,current.requested_at_ms,current.source_epoch,epoch,super::monotonic_now_ms(),auth::can_send_command("AutoColdFlow"),auth::current_status().station_role.as_deref()!=Some("rrls")){return;}
                let backend=UrlConfig::base_http();let token=auth::current_token();let identity=current.identity.clone();
                let owner=requests.capture();
                let request=super::station_settings_request::post_captured_no_redirect::<serde_json::Value,serde_json::Value>("/api/gse/cold-flow/start",&serde_json::json!({}),owner);
                busy.set(true);message.set(String::new());
                spawn(async move{
                    let result=request.await;
                    if !requests.current(){return;}
                    if epoch!=*WS_EPOCH.read()||backend!=UrlConfig::base_http()||token!=auth::current_token()||identity.as_ref().is_none_or(|owner|!auth::request_identity_is_current(owner)){busy.set(false);return;}
                    match result {
                        Ok(value)=>match serde_json::from_value::<RunKey>(value["key"].clone()){
                            Ok(key)if value["accepted"].as_bool()==Some(true)=>{pending.set(Some(key));message.set("Cold Flow accepted. Watch the sequence modal; PILOT/main stays closed until explicit release.".into());},
                            _=>message.set("The station did not return an authoritative Cold Flow run.".into())
                        },Err(error)=>message.set(format!("Auto Cold Flow was refused: {error}")),
                    }
                    busy.set(false);
                });
            },
                span {style:"min-width:0;flex:1 1 auto;",
                    span {style:"display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;line-height:1.25;", "Auto Cold Flow"}
                    span {title:reason.clone(),style:"display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:2;min-height:1.3em;overflow:hidden;margin-top:2px;font-size:11px;line-height:1.3;font-weight:400;white-space:normal;overflow-wrap:anywhere;",if !enabled {"{reason}"}}
                }
                span {"aria-hidden":badge.is_empty(),style:"visibility:{badge_visibility};flex:0 0 auto;padding:0.14rem 0.42rem;border-radius:999px;border:1px solid rgba(255,255,255,0.16);background:rgba(0,0,0,0.18);color:rgba(255,255,255,0.82);font-size:0.68rem;font-weight:800;line-height:1;text-transform:uppercase;letter-spacing:0.04em;", "{badge}"}
            }
            small {style:"color:var(--gs-text-muted);line-height:1.4;","Pressurizes with nitrogen to its own Cold Flow target, then isolates and waits. No ignition or countdown."}
            if !message().is_empty(){p {role:"status",style:"margin:0;","{message}"}}
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_old_stale_wrong_source_or_permission_never_enables_automatic_start() {
        let mut value = serde_json::json!({"cold_flow":{"version":1,"start_available":true,"valid_for_ms":12345,"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222"},"sequence_progress":{"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222"}});
        assert!(available(&value, 100., 7, 7, 200., true, true));
        assert!(available(&value, 100., 7, 7, 10000., true, true));
        assert!(available(&value, 100., 7, 7, 12445., true, true));
        assert!(!available(&value, 100., 7, 7, 99.999999, true, true));
        for value in [
            serde_json::json!({}),
            serde_json::json!({"cold_flow":{"version":0,"start_available":true}}),
        ] {
            assert!(!available(&value, 100., 7, 7, 200., true, true));
        }
        assert!(available(&value, 100., 7, 7, 12446., true, true));
        assert!(!available(&value, 100., 7, 8, 200., true, true));
        assert!(!available(&value, 100., 7, 7, 200., false, true));
        assert!(!available(&value, 100., 7, 7, 200., true, false));
        assert!(!available(&value, f64::NAN, 7, 7, 200., true, true));
        value["cold_flow"]["valid_for_ms"] = serde_json::json!(0);
        assert!(available(&value, 100., 7, 7, 100., true, true));
        value["cold_flow"]["start_available"] = serde_json::json!(false);
        assert!(!available(&value, 100., 7, 7, 30000., true, true));
        value["cold_flow"]["start_available"] = serde_json::json!(true);
        value["cold_flow"]["valid_for_ms"] = serde_json::json!(12345);
        value["sequence_progress"]["reset_epoch"] = serde_json::json!("3333333333333333");
        assert!(!available(&value, 100., 7, 7, 200., true, true));
    }
    #[test]
    fn active_style_requires_fresh_current_run_and_never_uses_start_permission_as_progress() {
        let mut value = serde_json::json!({"cold_flow":{"version":1,"active":true,"phase":"pressurizing","key":{"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222","run_id":"33333333333333333333333333333333"}},"sequence_progress":{"version":1,"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222"}});
        assert_eq!(active_label(&value, 100., 7, 7, 200.), Some("In progress"));
        assert_eq!(active_label(&value, 100., 7, 7, 2100.), Some("In progress"));
        assert_eq!(
            active_label(&value, 100., 7, 7, 30000.),
            Some("In progress")
        );
        assert_eq!(active_label(&value, 100., 7, 7, 99.999999), None);
        assert_eq!(active_label(&value, f64::NAN, 7, 7, 200.), None);
        assert_eq!(active_label(&value, 100., 7, 7, 2101.), Some("In progress"));
        assert_eq!(active_label(&value, 100., 7, 8, 200.), None);
        value["cold_flow"]["phase"] = serde_json::json!("awaiting_release");
        assert_eq!(
            active_label(&value, 100., 7, 7, 200.),
            Some("Awaiting release")
        );
        value["cold_flow"]["active"] = serde_json::json!(false);
        assert_eq!(active_label(&value, 100., 7, 7, 200.), None);
        value["cold_flow"]["active"] = serde_json::json!(true);
        value["sequence_progress"]["reset_epoch"] = serde_json::json!("4444444444444444");
        assert_eq!(active_label(&value, 100., 7, 7, 200.), None);
        value["sequence_progress"]["reset_epoch"] = serde_json::json!("2222222222222222");
        value["cold_flow"]["key"]["run_id"] = serde_json::json!("invalid");
        assert_eq!(active_label(&value, 100., 7, 7, 200.), None);
    }
}
