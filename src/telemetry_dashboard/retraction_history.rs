//! Explicit, evidence-preserving CLOSED reset; never a motion/readiness action.
use super::super::{UrlConfig, http_post_json, prelude::*};
#[derive(Clone, serde::Deserialize)]
struct Fault {
    fingerprint: Option<String>,
    revision: u64,
    resettable: bool,
}
#[derive(Clone, serde::Deserialize)]
struct Inspection {
    primary: bool,
    backend_instance: String,
    reset_epoch: String,
    retraction_history_fault: Fault,
}
#[derive(Clone)]
struct Confirmation {
    inspection: Inspection,
    source: (String, Option<String>),
}
impl Confirmation {
    fn current(&self, source: &(String, Option<String>)) -> bool {
        self.source == *source
            && self.inspection.primary
            && self.inspection.retraction_history_fault.resettable
            && self
                .inspection
                .retraction_history_fault
                .fingerprint
                .as_ref()
                .is_some_and(|fp| fp.len() == 64 && fp.bytes().all(|byte| byte.is_ascii_hexdigit()))
    }
    fn body(&self) -> serde_json::Value {
        serde_json::json!({"backend_instance":self.inspection.backend_instance,"reset_epoch":self.inspection.reset_epoch,
            "fault_fingerprint":self.inspection.retraction_history_fault.fingerprint,
            "fault_revision":self.inspection.retraction_history_fault.revision,"confirmation":"RESET RETRACTION JOURNAL"})
    }
}
fn source() -> (String, Option<String>) {
    (UrlConfig::base_http(), crate::auth::current_token())
}
#[component]
pub(super) fn RetractionHistoryRepair() -> Element {
    let mut confirmation = use_signal(|| None::<Confirmation>);
    let mut typed = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let context = confirmation.read().clone();
    let can_reset = context
        .as_ref()
        .is_some_and(|context| context.current(&source()))
        && crate::auth::can_send_command("RetractPlumbing")
        && typed() == "RESET RETRACTION JOURNAL"
        && !busy();
    rsx! {
        section {"aria-label":"Retraction history attention",
            p {"The retained retraction history needs attention. Inspection sends no motion and never makes Launch ready."}
            button {disabled:busy(),onclick:move |_|{
                let original=source();busy.set(true);message.set("Inspecting retained evidence; no motion will be sent.".into());
                spawn(async move {
                    let result=http_post_json::<serde_json::Value,Inspection>("/api/actuator/plumbing/retraction-history/inspect",&serde_json::json!({})).await;
                    busy.set(false);
                    if source()!=original{confirmation.set(None);message.set("Connection changed; inspect the current station again.".into());return;}
                    match result {Ok(inspection)=>{confirmation.set(Some(Confirmation{inspection,source:original}));typed.set(String::new());},Err(error)=>message.set(error)}
                });
            },"Inspect retraction history"}
            if let Some(context)=context {
                div {class:"gs26-modal",role:"dialog","aria-modal":"true","aria-label":"Reset retraction history to closed",
                    style:"position:fixed; inset:10% 8%; z-index:1600; overflow:auto; padding:24px; background:var(--gs26-panel-background,#17212b); color:var(--gs26-app-text,#edf3fa); border:2px solid #e0a535; border-radius:12px; box-shadow:0 0 0 100vmax #0009;",
                    h2 {"Preserve evidence and reset history to CLOSED"}
                    p {"This preserves the damaged evidence and writes a verified CLOSED record. It does not retract plumbing, change the drive, reset the T-clock, or unlock Launch. A new explicit retraction and fresh checks are still required."}
                    if !context.current(&source()){p {role:"alert","This station or fault context is unavailable or changed. Close and inspect again; no reset is authorized."}}
                    label {"Type RESET RETRACTION JOURNAL",input {value:"{typed}",disabled:busy(),oninput:move |event|typed.set(event.value())}}
                    button {class:"gs26-modal-danger",disabled:!can_reset,onclick:move |_|{
                        if !context.current(&source())||typed()!="RESET RETRACTION JOURNAL"||!crate::auth::can_send_command("RetractPlumbing"){return;}
                        let original=context.source.clone();let body=context.body();busy.set(true);
                        spawn(async move {
                            let result=http_post_json::<serde_json::Value,serde_json::Value>("/api/actuator/plumbing/retraction-history/reset",&body).await;
                            busy.set(false);confirmation.set(None);typed.set(String::new());
                            if source()!=original{message.set("Connection changed. Inspect current status; no readiness is assumed.".into());return;}
                            message.set(match result{Ok(_)=>"Retraction history is CLOSED. Make a new explicit retraction and check actual completion before Launch.".into(),Err(error)=>error});
                        });
                    },"Preserve evidence and reset to CLOSED"}
                    button {disabled:busy(),onclick:move |_|{confirmation.set(None);typed.set(String::new());},"Cancel"}
                }
            }
            if !message.read().is_empty(){p {role:"status","{message}"}}
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_reset_confirmation_is_exact_station_auth_fault_bound_and_never_a_readiness_record() {
        let current = (
            "http://fixture-primary".into(),
            Some("operator-token".into()),
        );
        let mut context = Confirmation {
            source: current.clone(),
            inspection: Inspection {
                primary: true,
                backend_instance: "0000000000000001".into(),
                reset_epoch: "0000000000000002".into(),
                retraction_history_fault: Fault {
                    fingerprint: Some("a".repeat(64)),
                    revision: 3,
                    resettable: true,
                },
            },
        };
        assert!(context.current(&current));
        assert!(!context.current(&("http://other".into(), current.1.clone())));
        assert!(!context.current(&(current.0.clone(), Some("different-token".into()))));
        let body = context.body();
        assert_eq!(body["fault_revision"], 3);
        assert_eq!(body["confirmation"], "RESET RETRACTION JOURNAL");
        assert!(body.get("ready").is_none());
        context.inspection.retraction_history_fault.resettable = false;
        assert!(!context.current(&current));
        context.inspection.retraction_history_fault.resettable = true;
        context.inspection.retraction_history_fault.fingerprint = None;
        assert!(!context.current(&current));
    }
}
