//! Recover station: clear the station's own Abort latch without resetting any
//! board, once every board has been recovered on its own (GroundStation
//! `board_recovery::coordinator::station_recover`). The station decides; the
//! button shows its reason and asks for a confirmation that says plainly that
//! no board is reset.
use super::ActionPolicyMsg;
use dioxus::prelude::*;
use serde::Deserialize;

/// The Actions-tab entry the station's layout lists in "Recovery / reset".
pub(super) const COMMAND: &str = "RecoverStation";
const PATH: &str = "/api/board-recovery/recover-station";

/// The station's view, from the action policy (or the board-recovery status).
#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct StationRecoveryPolicy {
    /// Why it cannot run now; None when it can.
    pub disabled_reason: Option<String>,
    /// Where an Aborted station goes: Startup, or Idle in HITL.
    pub target_state_label: String,
}

/// Listed only while the station reports its own Abort latch set.
pub(super) fn listed(policy: &ActionPolicyMsg) -> bool {
    policy.station_recovery.is_some()
}

/// Why the button is disabled here, or None. Local reasons first, then the
/// station's own.
pub(super) fn disabled_reason(server: Option<&str>, rrls: bool, permitted: bool, connected: bool) -> Option<String> {
    if rrls {
        Some("Recover station runs on the primary ground station.".into())
    } else if !permitted {
        Some("Not permitted by your account.".into())
    } else if !connected {
        Some("Not connected to the ground station.".into())
    } else {
        server.map(str::to_owned)
    }
}

pub(super) const CONFIRM_TITLE: &str = "Recover the station without resetting boards?";

/// The confirmation's text: what changes, and that no board is reset.
pub(super) fn confirm_lines(target: &str) -> [String; 3] {
    let target = if target.is_empty() { "Startup" } else { target };
    [
        format!("This clears the station's own Abort latch. An Aborted station returns to {target}, and fill-side controls are no longer limited to the safe direction."),
        "No board is reset. Every board must already be recovered on its own; nothing is sent to any board except the station's own flight state.".into(),
        "The action is recorded in the launch journal under your name. Reset all boards is still the way to reset every board.".into(),
    ]
}

use super::board_recovery_panel::server_refusal;

/// What the operator reads after the request.
pub(super) fn outcome_message(result: Result<serde_json::Value, String>) -> String {
    match result {
        Ok(value) => {
            let state = value["flight_state_label"].as_str().unwrap_or("its recovered state");
            let mut text = format!("Station recovered: its Abort latch is cleared and it is in {state}. No board was reset.");
            if value["published"] == false {
                text.push_str(" Its new state could not be published or saved; a restart returns it to Aborted.");
            }
            text
        }
        Err(error) => format!("Station not recovered: {}", server_refusal(&error)),
    }
}

/// The button, its reason and its confirmation dialog.
#[component]
pub(super) fn RecoverStationControl(view: StationRecoveryPolicy) -> Element {
    let mut confirming = use_signal(|| false);
    let mut sending = use_signal(|| false);
    let mut result = use_signal(String::new);
    let rrls = crate::auth::current_status().station_role.as_deref() == Some("rrls");
    let permitted = crate::auth::can_view_actions();
    let connected = *super::WS_CONNECTED_SIGNAL.read();
    let reason = disabled_reason(view.disabled_reason.as_deref(), rrls, permitted, connected);
    let disabled = reason.is_some() || sending();
    let lines = confirm_lines(&view.target_state_label);
    rsx! {
        div{class:"gs26-recover-station","data-recover-station":"",style:"display:grid;gap:var(--gs-space-2);justify-items:start;min-width:0;",
            button{r#type:"button",class:"gs-btn","data-dispatch-command":COMMAND,disabled,title:reason.clone().unwrap_or_default(),
                onclick:move |_|{result.set(String::new());confirming.set(true);},
                "Recover station…"
            }
            p{style:"margin:0;font-size:var(--gs-text-sm);","Clears the station's own Abort latch once every board is recovered. No board is reset."}
            if let Some(reason)=reason.clone(){p{role:"status",class:"gs-notice gs-notice--warn","data-disabled-reason":"","Unavailable: {reason}"}}
            if !result.read().is_empty(){p{role:"status",class:"gs-notice","{result}"}}
        }
        if confirming() {
            div{style:"position:fixed;inset:0;z-index:200000;background:#000a;display:flex;align-items:center;justify-content:center;padding:20px;box-sizing:border-box;",
                onkeydown:move|event|{event.stop_propagation();match event.key(){Key::Escape if !*sending.peek()=>{event.prevent_default();confirming.set(false);},Key::Tab=>{event.prevent_default();super::board_recovery_panel::cycle_modal_focus("gs26-recover-station-confirm",event.modifiers().contains(Modifiers::SHIFT));},_=>{}}},
                div{id:"gs26-recover-station-confirm",role:"alertdialog","aria-modal":"true","aria-labelledby":"recover-station-title",style:"background:var(--gs26-panel-background,#142534);color:var(--gs26-app-text,#e5e7eb);font:inherit;padding:var(--gs-space-5);border-radius:var(--gs-radius-lg);width:min(100%,640px);max-height:calc(100dvh - 40px);display:flex;flex-direction:column;gap:var(--gs-space-3);overflow-y:auto;box-sizing:border-box;overflow-wrap:anywhere;",
                    h2{id:"recover-station-title",style:"margin:0;font-size:var(--gs-text-lg);","{CONFIRM_TITLE}"}
                    p{style:"margin:0;","{lines[0]}"}
                    p{style:"margin:0;font-weight:700;","{lines[1]}"}
                    p{style:"margin:0;","{lines[2]}"}
                    div{style:"display:flex;gap:var(--gs-space-2);flex-wrap:wrap;justify-content:flex-end;",
                        button{r#type:"button",class:"gs-btn",autofocus:true,disabled:sending(),onclick:move |_|confirming.set(false),"Cancel"}
                        button{r#type:"button",class:"gs-btn gs-btn--primary",disabled:sending(),
                            onclick:move |_|{
                                if *sending.peek(){return;}
                                let owner=match super::recording_download::ExportOwner::capture_ordinary_selected(||true){
                                    Ok(owner)=>owner,
                                    Err(_)=>{result.set("No current station or sign-in. Reconnect and try again.".into());confirming.set(false);return;}
                                };
                                sending.set(true);
                                spawn(async move{
                                    let outcome=super::station_settings_request::post_captured::<_,serde_json::Value>(PATH,&serde_json::json!({"confirm":true}),Ok(owner.clone())).await;
                                    if !owner.is_current(){return;}
                                    result.set(outcome_message(outcome));
                                    sending.set(false);
                                    confirming.set(false);
                                });
                            },
                            if sending() {"Recovering…"} else {"Recover station"}
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(value: serde_json::Value) -> ActionPolicyMsg {
        let mut base = serde_json::json!({"key_enabled":true,"software_buttons_enabled":true,"controls":[]});
        if !value.is_null() {
            base["station_recovery"] = value;
        }
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn listed_only_while_the_station_reports_its_latch() {
        assert!(!listed(&policy(serde_json::Value::Null)));
        let ready = policy(serde_json::json!({"target_state_label":"Startup"}));
        assert!(listed(&ready));
        assert_eq!(ready.station_recovery.unwrap().disabled_reason, None);
        let blocked = policy(serde_json::json!({"disabled_reason":"VB still reports Abort.","target_state_label":"Startup"}));
        assert_eq!(blocked.station_recovery.unwrap().disabled_reason.as_deref(), Some("VB still reports Abort."));
    }

    #[test]
    fn disabled_reason_names_the_local_cause_first_then_the_stations() {
        assert_eq!(disabled_reason(None, false, true, true), None);
        assert_eq!(disabled_reason(Some("VB still reports Abort."), false, true, true).as_deref(), Some("VB still reports Abort."));
        assert!(disabled_reason(None, true, true, true).unwrap().contains("primary ground station"));
        assert!(disabled_reason(Some("x"), false, false, true).unwrap().contains("Not permitted"));
        assert!(disabled_reason(None, false, true, false).unwrap().contains("Not connected"));
    }

    #[test]
    fn the_confirmation_says_no_board_is_reset_and_names_the_target() {
        let lines = confirm_lines("Idle");
        assert!(lines[0].contains("returns to Idle"), "{}", lines[0]);
        assert!(lines[1].starts_with("No board is reset."), "{}", lines[1]);
        assert!(confirm_lines("")[0].contains("returns to Startup"));
        assert!(CONFIRM_TITLE.contains("without resetting boards"));
    }

    #[test]
    fn outcome_shows_the_new_state_or_the_stations_refusal() {
        let ok = outcome_message(Ok(serde_json::json!({"recovered":true,"flight_state_label":"Startup","published":true})));
        assert_eq!(ok, "Station recovered: its Abort latch is cleared and it is in Startup. No board was reset.");
        let unsaved = outcome_message(Ok(serde_json::json!({"recovered":true,"flight_state_label":"Startup","published":false})));
        assert!(unsaved.contains("restart returns it to Aborted"), "{unsaved}");
        let refused = outcome_message(Err(r#"HTTP 409: {"error":"recover_station_refused","message":"VB still reports Abort."}"#.into()));
        assert_eq!(refused, "Station not recovered: VB still reports Abort.");
        assert_eq!(outcome_message(Err("network down".into())), "Station not recovered: network down");
    }
}
