#[path = "retraction_history.rs"]
mod retraction_history;
use super::{
    ACTION_POLICY_SIGNAL, BOARD_ABORT_SIGNAL, BoardAbortSnapshot, RetractionInterlock, UrlConfig,
    WS_EPOCH, WS_SENDER, prelude::*,
};
#[derive(serde::Deserialize)]
struct Reply {
    message: String,
    snapshot: BoardAbortSnapshot,
}
// Server version tracks register changes, not every asynchronous persistence fault.
// Only a request from this exact local receipt and connection may replace it.
fn accept_http_snapshot(snapshot: BoardAbortSnapshot, revision: u64, epoch: u64) -> bool {
    let previous = BOARD_ABORT_SIGNAL.read().clone();
    if *WS_EPOCH.peek() != epoch || previous.local_revision != revision {
        return false;
    }
    let next = snapshot.reconciled(&previous, epoch);
    let mut policy = ACTION_POLICY_SIGNAL.read().clone();
    next.constrain_launch_policy(&mut policy);
    *BOARD_ABORT_SIGNAL.write() = next;
    *ACTION_POLICY_SIGNAL.write() = policy;
    true
}
fn remaining_display_ms(reported: u32, received: f64, now: f64) -> Option<u32> {
    if !received.is_finite() || !now.is_finite() || now < received {
        return None;
    }
    Some(reported.saturating_sub((now - received).min(u32::MAX as f64) as u32))
}
async fn display_tick() {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(250).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
}
#[component]
pub fn BoardAbortReadiness(#[props(default = false)] show_hitl_interlocks: bool) -> Element {
    let view_key = super::station_mode_observer::view_key();
    rsx! { for key in [view_key] {
        OwnedBoardAbortReadiness { key:"{key}", show_hitl_interlocks }
    } }
}
#[component]
fn OwnedBoardAbortReadiness(show_hitl_interlocks: bool) -> Element {
    let requests = super::station_settings_request::use_factory();
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let mut request = use_signal(|| 0u64);
    let mut report = use_signal(|| None::<(RetractionInterlock, f64)>);
    let mut tick = use_signal(|| 0u64);
    use_effect(move || {
        let next = BOARD_ABORT_SIGNAL.read().retraction.clone();
        let changed = report.peek().as_ref().map(|(value, _)| value.clone()) != next;
        if changed {
            report.set(next.map(|value| (value, super::monotonic_now_ms())));
        }
    });
    use_future(move || async move {
        loop {
            display_tick().await;
            tick.set(tick().wrapping_add(1));
        }
    });
    let _ = tick();
    let current = report.read().clone();
    let mode = super::station_mode::current_snapshot();
    let hitl = mode["effective_mode"].as_str() == Some("hitl");
    let synchronized = mode["synchronized"].as_bool() == Some(true);
    let rendered_source = (UrlConfig::base_http(), crate::auth::current_token());
    let history_unknown = BOARD_ABORT_SIGNAL.read().history_unknown_boards.join(", ");
    rsx! {section {"aria-label":"Launch readiness",
        if let Some(block)=BOARD_ABORT_SIGNAL.read().launch_message(){p {role:"alert","{block}"}}
        if !BOARD_ABORT_SIGNAL.read().missing_required_state().is_empty(){p {role:"status","Fresh board state checks are required before a new Launch request."}}
        if !history_unknown.is_empty(){p {role:"status","Stored history is unconfirmed for {history_unknown}. The primary determines which current board checks apply."}}
        if let Some((gate,received))=current {
            p {role:"status","{gate.message}"}
            if gate.phase=="countdown" {
                if let Some(ms)=gate.remaining_ms.and_then(|ms|remaining_display_ms(ms,received,super::monotonic_now_ms())) {
                    p {role:"status","Retraction minimum interval: {ms.div_ceil(1000)} s remaining. A fresh actual completion check is still required."}
                } else {p {role:"status","Retraction countdown is unknown; check actual readiness."}}
            }
        } else {p {role:"status","Retraction interlock status is unavailable. This does not prove readiness."}}
        button {disabled:*busy.read(),onclick:move |_|{
            let base=UrlConfig::base_http();let token=crate::auth::current_token();let epoch=*WS_EPOCH.peek();
            let revision=BOARD_ABORT_SIGNAL.read().local_revision;
            let id=request().wrapping_add(1);request.set(id);busy.set(true);
            message.set("Checking actual board states; no Launch is being sent.".into());
            let refresh=requests.post::<serde_json::Value,Reply>("/api/board_abort/refresh",&serde_json::json!({}));
            spawn(async move {
                let result=refresh.await;
                if !requests.current()||request()!=id{return;}
                if UrlConfig::base_http()!=base||crate::auth::current_token()!=token||*WS_EPOCH.peek()!=epoch{busy.set(false);message.set("Connection changed; check current readiness again.".into());return;}
                match result {Ok(reply)=>{if accept_http_snapshot(reply.snapshot,revision,epoch){message.set(reply.message);}else{message.set("A newer station report was received; current readiness was retained.".into());}},Err(error)=>message.set(error)}
                for _ in 0..3 {
                    #[cfg(target_arch="wasm32")]
                    gloo_timers::future::TimeoutFuture::new(400).await;
                    #[cfg(not(target_arch="wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                    if !requests.current()||request()!=id{return;}
                    if UrlConfig::base_http()!=base||crate::auth::current_token()!=token||*WS_EPOCH.peek()!=epoch{busy.set(false);message.set("Connection changed; check current readiness again.".into());return;}
                    let revision=BOARD_ABORT_SIGNAL.read().local_revision;
                    if let Ok(snapshot)=requests.get::<BoardAbortSnapshot>("/api/board_abort").await
                        && requests.current()&&UrlConfig::base_http()==base&&crate::auth::current_token()==token&&*WS_EPOCH.peek()==epoch
                    {accept_http_snapshot(snapshot,revision,epoch);}
                }
                if requests.current()&&request()==id{busy.set(false);}
            });
        },"Check launch readiness"}
        if !message.read().is_empty(){p {role:"status","{message}"}}
        if BOARD_ABORT_SIGNAL.read().retraction.as_ref().is_some_and(|gate|gate.phase=="fault") {
            retraction_history::RetractionHistoryRepair {}
        }
        if show_hitl_interlocks&&hitl {
            fieldset {style:"grid-template-columns:repeat(auto-fit,minmax(min(100%,220px),1fr));",legend {"HITL enforcement at the primary"}
                p {style:"grid-column:1/-1;margin:0;font-size:var(--gs-text-sm);color:var(--gs-text-muted);","Applied by the primary. Checklists and their history are unchanged."}
                if crate::auth::current_status().station_role.as_deref()==Some("rrls") {
                    p {role:"status",style:"grid-column:1/-1;","{super::station_mode::configuration_description(&mode)}"}
                }
                for (index,(name,label)) in [("TogglePlumbingDelayInterlock","Retraction minimum 45 seconds"),("ToggleChecklistInterlock","Checklist enforcement"),("ToggleSupplyInterlock","Nitrogen/nitrous one at a time"),("ToggleBoardStateProofInterlock","Board state checks")].into_iter().enumerate() {
                    {
                        let policy=ACTION_POLICY_SIGNAL.read();
                        let control=policy.controls.iter().find(|control|control.cmd==name);
                        let current_known=control.and_then(|control|control.actuated);
                        let known=current_known.or_else(||super::station_mode::configuration_setting(&mode,index));
                        let enabled=synchronized&&current_known.is_some()&&control.is_some_and(|control|control.enabled)
                            &&crate::auth::can_send_command(name)&&WS_SENDER.read().is_some();
                        let state=known.map_or("unknown",|value|if value{"on"}else{"off"});
                        let source=rendered_source.clone();
                        rsx! {button {disabled:!enabled,onclick:move |_|{
                            if source!=(UrlConfig::base_http(),crate::auth::current_token()){
                                message.set("Connection changed; inspect current primary policy before requesting a change.".into());return;
                            }
                            if let Some(sender)=WS_SENDER.read().clone(){
                                match sender.send_cmd(name){Ok(())=>message.set("Policy change requested; awaiting primary confirmation.".into()),Err(error)=>message.set(error)}
                            }
                        },"{label}: {state}"}}
                    }
                }
            }
        }
    }}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn countdown_is_presentation_only_and_clock_rollback_never_fabricates_elapsed_time() {
        assert_eq!(remaining_display_ms(45_000, 100.0, 1_100.0), Some(44_000));
        assert_eq!(remaining_display_ms(45_000, 100.0, 50_100.0), Some(0));
        assert_eq!(remaining_display_ms(45_000, 100.0, 99.0), None);
        assert_eq!(remaining_display_ms(45_000, 100.0, f64::NAN), None);
    }
}
