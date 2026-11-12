//! Administrator setting "Launch sequence only": switches off the GSE
//! sequences that cannot be checked without sensors and valves. The station
//! decides which sequences that covers in its current mode and why.
use super::neutral_request_owner;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub(super) const SETTING_LABEL: &str = "Launch sequence only (disable fill/other sequences)";

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Status {
    launch_sequence_only: bool,
    enable_blocked_reason: Option<String>,
    #[serde(default)]
    disabled_sequences: Vec<String>,
    #[serde(default)]
    kept_sequences: Vec<String>,
    #[serde(default)]
    always_kept: Vec<String>,
    can_change: bool,
    #[serde(default)]
    preset_env: String,
}
#[derive(Serialize)]
struct Update {
    launch_sequence_only: bool,
}

/// The inline reason the toggle cannot be used now, if any. Turning the
/// setting off is never blocked; turning it on waits for running sequences.
fn toggle_block(status: &Status) -> Option<&str> {
    if !status.can_change || !status.launch_sequence_only {
        status.enable_blocked_reason.as_deref()
    } else {
        None
    }
}

#[component]
pub(super) fn SequenceAccessAdmin() -> Element {
    let authority = neutral_request_owner::use_factory();
    let mut status = use_signal(|| None::<Status>);
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    use_future(move || {
        authority.run(async move {
            loop {
                match authority.get::<Status>("/api/admin/sequence-access").await {
                    Ok(value) => status.set(Some(value)),
                    Err(error) => message.set(error),
                }
                super::sequence_progress_host::delay(3_000).await;
            }
        })
    });
    let Some(current) = status.read().clone() else {
        return rsx! {section {class:"station-positions-admin","aria-label":"Sequences", h2 {"Sequences"} p {"Loading…"} p {role:"status","{message}"}}};
    };
    let block = toggle_block(&current).map(str::to_owned);
    let disabled = *busy.read() || block.is_some();
    let on = current.launch_sequence_only;
    rsx! {
        section {class:"station-positions-admin","aria-label":"Sequences",id:"gs26-sequence-access",
            h2 {"Sequences"}
            label {style:"display:flex;gap:.6rem;align-items:flex-start;min-height:44px;",
                input {r#type:"checkbox",checked:on,disabled,style:"min-width:20px;min-height:20px;",onchange:move |e|{
                    let wanted=e.checked();
                    if !authority.current(){return;}
                    busy.set(true);
                    authority.spawn(async move{
                        let (mut status, mut message, mut busy) = (status, message, busy);
                        match authority.post::<Update,Status>("/api/admin/sequence-access",&Update{launch_sequence_only:wanted}).await {
                            Ok(value)=>{
                                message.set(if value.launch_sequence_only {"Launch sequence only is on.".into()} else {"Launch sequence only is off; every sequence is available again.".into()});
                                status.set(Some(value));
                            }
                            Err(error)=>message.set(format!("Could not change the setting. {error}")),
                        }
                        busy.set(false);
                    });
                }}
                span {strong {"{SETTING_LABEL}"}
                    if let Some(reason)=block.as_ref() {
                        span {class:"admin-note admin-error",style:"display:block;",role:"note","{reason}"}
                    }
                }
            }
            p {class:"station-location-hint",
                "For bench testing without sensors or valves. Applies in every station mode; disabled buttons say "
                em {"Disabled in admin settings: launch sequence only"}
                ". Abort, Launch and its QD wait, resets, recovery, manual controls and comms are never affected."
            }
            if !current.disabled_sequences.is_empty() {
                p {class:"station-location-hint", strong {"Disabled while on (this mode): "} "{current.disabled_sequences.join(\", \")}"}
            }
            if !current.kept_sequences.is_empty() {
                p {class:"station-location-hint", strong {"Kept: "} "{current.kept_sequences.join(\"; \")}"}
            }
            if !current.always_kept.is_empty() {
                p {class:"station-location-hint", strong {"Never affected: "} "{current.always_kept.join(\"; \")}"}
            }
            if !current.preset_env.is_empty() {
                p {class:"station-location-hint","Without a saved choice, {current.preset_env}=1 turns it on at startup."}
            }
            p {role:"status","{message}"}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(json: &str) -> Status {
        serde_json::from_str(json).unwrap()
    }
    #[test]
    fn enabling_shows_the_running_sequence_reason_but_disabling_is_never_blocked() {
        let off = parse(r#"{"launch_sequence_only":false,"enable_blocked_reason":"The nitrogen test is running or paused.","disabled_sequences":["Turbocharge"],"kept_sequences":[],"always_kept":["Abort"],"can_change":true,"preset_env":"GS_LAUNCH_SEQUENCE_ONLY"}"#);
        assert_eq!(toggle_block(&off), Some("The nitrogen test is running or paused."));
        let on = Status { launch_sequence_only: true, ..off.clone() };
        assert_eq!(toggle_block(&on), None);
        let idle = Status { enable_blocked_reason: None, ..off };
        assert_eq!(toggle_block(&idle), None);
    }
    #[test]
    fn rrls_cannot_change_it() {
        let rrls = parse(r#"{"launch_sequence_only":true,"enable_blocked_reason":"Change launch sequence only on the primary GroundStation.","can_change":false}"#);
        assert!(toggle_block(&rrls).is_some_and(|r| r.contains("primary")));
    }
}
