//! Explicit primary-GS start only. PILOT release stays in the authoritative run modal.
use super::sequence_progress_host::StatusObservation;
use super::sequence_progress_model::RunKey;
use super::{UrlConfig, WS_EPOCH, auth, http_post_json};
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
        && now - received <= 2000.
        && value["cold_flow"]["version"].as_u64() == Some(1)
        && value["cold_flow"]["start_available"].as_bool() == Some(true)
}
#[component]
pub(super) fn AutoColdFlow(
    status: Signal<StatusObservation>,
    #[props(default = false)] disabled: bool,
) -> Element {
    let mut clock = use_signal(super::monotonic_now_ms);
    let mut busy = use_signal(|| false);
    let mut pending = use_signal(|| None::<RunKey>);
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
    let primary = auth::current_status().station_role.as_deref() != Some("rrls");
    let enabled = !disabled
        && !busy()
        && pending().is_none()
        && available(
            &observed.value,
            observed.requested_at_ms,
            observed.source_epoch,
            *WS_EPOCH.read(),
            clock(),
            auth::can_send_command("AutoColdFlow"),
            primary,
        );
    let reason = observed.value["cold_flow"]["reason"]
        .as_str()
        .unwrap_or("Auto Cold Flow is unavailable on this connection")
        .to_string();
    rsx! {
        div {class:"gs26-cold-flow-start",style:"display:grid;gap:6px;min-width:0;",
            button {r#type:"button",disabled:!enabled,style:"min-height:44px;width:auto;max-width:100%;padding:10px 14px;border-radius:10px;",onclick:move|_|{
                let current=status();let epoch=*WS_EPOCH.read();
                if disabled||!available(&current.value,current.requested_at_ms,current.source_epoch,epoch,super::monotonic_now_ms(),auth::can_send_command("AutoColdFlow"),auth::current_status().station_role.as_deref()!=Some("rrls")){return;}
                let backend=UrlConfig::base_http();let token=auth::current_token();busy.set(true);message.set(String::new());
                spawn(async move{
                    let result=http_post_json::<serde_json::Value,serde_json::Value>("/api/gse/cold-flow/start",&serde_json::json!({})).await;
                    if epoch!=*WS_EPOCH.read()||backend!=UrlConfig::base_http()||token!=auth::current_token(){busy.set(false);return;}
                    match result {
                        Ok(value)=>match serde_json::from_value::<RunKey>(value["key"].clone()){
                            Ok(key)if value["accepted"].as_bool()==Some(true)=>{pending.set(Some(key));message.set("Cold Flow accepted. Watch the sequence modal; PILOT/main stays closed until explicit release.".into());},
                            _=>message.set("The station did not return an authoritative Cold Flow run.".into())
                        },Err(error)=>message.set(format!("Auto Cold Flow was refused: {error}")),
                    }
                    busy.set(false);
                });
            },if busy(){"Starting Auto Cold Flow…"}else{"Auto Cold Flow"}}
            small {"Pressurizes with nitrogen, then isolates and waits. No ignition or launch countdown."}
            if !enabled&&!reason.is_empty(){small {"{reason}"}}
            if !message().is_empty(){p {role:"status",style:"margin:0;","{message}"}}
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_old_stale_wrong_source_or_permission_never_enables_automatic_start() {
        let value = serde_json::json!({"cold_flow":{"version":1,"start_available":true}});
        assert!(available(&value, 100., 7, 7, 200., true, true));
        for value in [
            serde_json::json!({}),
            serde_json::json!({"cold_flow":{"version":0,"start_available":true}}),
        ] {
            assert!(!available(&value, 100., 7, 7, 200., true, true));
        }
        assert!(!available(&value, 100., 7, 7, 2101., true, true));
        assert!(!available(&value, 100., 7, 8, 200., true, true));
        assert!(!available(&value, 100., 7, 7, 200., false, true));
        assert!(!available(&value, 100., 7, 7, 200., true, false));
        assert!(!available(&value, f64::NAN, 7, 7, 200., true, true));
    }
}
