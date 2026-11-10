//! Backend-defined mode selection and an orthogonal Simulation draft.
//! A draft or HTTP receipt never changes the actual mode presentation.
use super::{
    layout::ThemeConfig, recording_download::ExportOwner, station_mode_contract as contract,
    station_mode_observer as observer, station_mode_transport as transport,
};
use dioxus::prelude::*;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Clone)]
struct Prepared {
    challenge: contract::Challenge,
    received: f64,
    owner: std::rc::Rc<super::recording_download::ExportOwner>,
}
impl Prepared {
    fn valid(&self, now: f64) -> bool {
        self.owner.is_current()
            && self.challenge.fresh(self.received, now)
            && observer::OBSERVED.peek().as_ref().is_some_and(|o| {
                o.current(now)
                    && o.value.binding(&self.challenge.target.id).as_ref()
                        == Some(&self.challenge.binding)
                    && o.value.current.as_ref() == Some(&self.challenge.current)
                    && o.value.option(&self.challenge.target.id) == Some(&self.challenge.target)
                    && o.value
                        .reason(&self.challenge.target.id, o.received, now)
                        .is_none()
            })
    }
}
#[derive(Clone)]
struct Notice {
    identity: crate::auth::RequestIdentity,
    base: String,
    skip_tls: bool,
    request: contract::Confirmation,
    target: contract::OptionEntry,
    message: String,
}
impl Notice {
    fn same_viewer(&self) -> bool {
        self.base == super::UrlConfig::base_http()
            && self.skip_tls == super::UrlConfig::_skip_tls_verify()
            && crate::auth::request_identity_is_current(&self.identity)
    }
}
static NOTICE: GlobalSignal<Option<Notice>> = Signal::global(|| None);
#[component]
pub(super) fn ModeRequestNotice(theme: ThemeConfig) -> Element {
    let notice = NOTICE.read().clone().filter(Notice::same_viewer);
    let Some(notice) = notice else {
        return rsx! {};
    };
    let observed = observer::OBSERVED.read();
    let now = *observer::CLOCK.read();
    let applied = observed.as_ref().is_some_and(|o| {
        o.current(now)
            && contract::applied_by(&notice.request, &notice.target, &o.value, o.received, now)
    });
    rsx! {section{role:"status",style:"padding:12px;border:1px solid {theme.border};border-radius:10px;font:inherit;overflow-wrap:anywhere;",
        strong{"Mode request: {notice.target.label}"}
        if applied {p{"The backend reported this exact transition, requested combination and new policy epoch as applied. This is not physical-output proof."}}
        else {p{"{notice.message}"}}
        button{r#type:"button",onclick:move|_|*NOTICE.write()=None,"Dismiss mode request notice"}
    }}
}
#[derive(Clone)]
struct Controls {
    alive: Arc<AtomicBool>,
    selected: Signal<String>,
    applied_selection: Signal<Option<String>>,
    simulation: Signal<Option<bool>>,
    dialog: Signal<Option<Prepared>>,
    busy: Signal<bool>,
    operation_owner: Signal<Option<Rc<ExportOwner>>>,
    serial: Signal<u64>,
    message: Signal<String>,
}
impl Controls {
    fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
    fn target(&self) -> Option<contract::OptionEntry> {
        let mode = self.selected.peek().clone();
        let simulation = (*self.simulation.peek())?;
        observer::OBSERVED
            .peek()
            .as_ref()?
            .value
            .pair(&mode, simulation)
            .cloned()
    }
    fn reason(&self, inhibited: bool) -> Option<String> {
        if inhibited {
            return Some("Actions are disabled on this screen. Select Actions Disabled in the header to enable actions, then review the mode change.".into());
        }
        let target = self.target()?;
        // A server-fenced option (a restart, or an idle-only in-process switch)
        // does not wait for the in-place epoch-fenced transport.
        if !super::station_policy_context::CLIENT_EPOCH_INTEGRATION_READY && !target.restarts() {
            return Some("Mode switching is unavailable until this client and backend complete epoch-fenced transport qualification.".into());
        }
        let o = observer::OBSERVED.peek().clone()?;
        if !o.owner.is_current() {
            return Some("The original connection retired; refresh mode status.".into());
        }
        let now = super::monotonic_now_ms();
        if !o.current(now) {
            return Some(
                "Refresh the original station mode capability before reviewing a change.".into(),
            );
        }
        o.value.reason(&target.id, o.received, now)
    }
    fn next(&mut self) -> Option<u64> {
        let n = (*self.serial.peek()).checked_add(1)?;
        self.serial.set(n);
        Some(n)
    }
    fn prepare(mut self, inhibited: bool) {
        if !self.alive() || *self.busy.peek() || self.dialog.peek().is_some() {
            return;
        }
        if let Some(reason) = self.reason(inhibited) {
            self.message.set(reason);
            return;
        }
        let Some(target) = self.target() else {
            return;
        };
        let Some(o) = observer::OBSERVED.peek().clone() else {
            return;
        };
        let Some(binding) = o.value.binding(&target.id) else {
            return;
        };
        let Ok(owner) = observer::owner(self.alive.clone()) else {
            return;
        };
        let Some(serial) = self.next() else {
            return;
        };
        let started = super::monotonic_now_ms();
        let request = transport::prepare(Ok(owner.clone()), &binding);
        self.operation_owner.set(Some(owner.clone()));
        self.busy.set(true);
        spawn(async move {
            let result = request.await;
            if !self.alive() || !owner.is_current() {
                return;
            }
            if *self.serial.peek() != serial {
                return;
            }
            self.operation_owner.set(None);
            self.busy.set(false);
            match result.and_then(|v| {
                serde_json::from_value::<contract::Challenge>(v)
                    .map_err(|_| "Unsupported mode challenge".into())
            }) {
                Ok(challenge)
                    if challenge.matches(&binding)
                        && challenge.current == o.value.current.clone().unwrap()
                        && challenge.target == target =>
                {
                    let prepared = Prepared {
                        challenge,
                        received: started,
                        owner,
                    };
                    if prepared.valid(super::monotonic_now_ms()) {
                        self.dialog.set(Some(prepared));
                    } else {
                        self.message.set(
                            "Mode review retired or expired; no confirmation was sent.".into(),
                        );
                    }
                }
                Ok(_) => self
                    .message
                    .set("Mode review does not match its original context and target.".into()),
                Err(e) => self.message.set(format!("Mode review unavailable: {e}")),
            }
        });
    }
    fn confirm(mut self, inhibited: bool) {
        if !self.alive() || *self.busy.peek() {
            return;
        }
        let Some(prepared) = self.dialog.peek().clone() else {
            return;
        };
        if self.reason(inhibited).is_some() || !prepared.valid(super::monotonic_now_ms()) {
            self.dialog.set(None);
            self.message
                .set("Mode confirmation retired or expired; nothing was sent.".into());
            return;
        }
        let Some(identity) = crate::auth::request_identity() else {
            return;
        };
        let echoed = prepared.challenge.echo();
        let request_body = contract::Confirmation {
            binding: prepared.challenge.binding,
            ticket: prepared.challenge.ticket,
            transition_id: prepared.challenge.transition_id,
            // Nothing is typed. An older backend that still issues a phrase
            // gets it echoed back; the click on Confirm is the confirmation.
            confirmation: echoed,
        };
        let owner = prepared.owner;
        let restarts = prepared.challenge.target.restarts();
        let in_process = prepared.challenge.target.switches_in_process();
        let request = transport::confirm(Ok(owner.clone()), &request_body);
        *NOTICE.write()=Some(Notice{identity,base:super::UrlConfig::base_http(),skip_tls:super::UrlConfig::_skip_tls_verify(),request:request_body.clone(),target:prepared.challenge.target,message:"Submitted; actual mode is unconfirmed. Status readback will identify this exact transition. This request is never resent automatically.".into()});
        self.dialog.set(None);
        self.operation_owner.set(Some(owner.clone()));
        self.busy.set(true);
        spawn(async move {
            let result = request.await;
            if !self.alive() || !owner.is_current() {
                return;
            }
            self.operation_owner.set(None);
            self.busy.set(false);
            let message=match result.and_then(|v|serde_json::from_value::<contract::Accepted>(v).map_err(|_|"Unsupported receipt".to_owned())) {
                Ok(receipt) if receipt.matches(&request_body)&&in_process=>"Backend accepted the switch and changes mode in place once the station is still idle. Your sign-in and connection are kept; the layout and controls refresh on their own, and status readback will confirm the new mode or report why it was refused.".into(),
                Ok(receipt) if receipt.matches(&request_body)&&restarts=>"Backend accepted the switch and is restarting into the selected build. Live data pauses for a few seconds and reconnects on its own; status readback will confirm the new mode.".into(),
                Ok(receipt) if receipt.matches(&request_body)=>"Backend accepted the transition as pending. Awaiting actual combination and new-epoch readback.".into(),
                Ok(_)=>"Receipt mismatched; outcome remains unconfirmed. Read status; do not resend automatically.".into(),
                Err(e)=>format!("Outcome remains unconfirmed: {e}. Read status; no automatic resend."),
            };
            if let Some(notice) = NOTICE.write().as_mut().filter(|n| {
                n.same_viewer() && n.request.transition_id == request_body.transition_id
            }) {
                notice.message = message;
            }
            let next = (*observer::REFRESH.peek()).saturating_add(1);
            *observer::REFRESH.write() = next;
        });
    }
}
#[component]
pub(super) fn StationModeSettings(theme: ThemeConfig, inhibited: bool) -> Element {
    let alive = super::recording_download::use_export_lifetime();
    let controls = Controls {
        alive,
        selected: use_signal(String::new),
        applied_selection: use_signal(|| None::<String>),
        simulation: use_signal(|| None::<bool>),
        dialog: use_signal(|| None::<Prepared>),
        busy: use_signal(|| false),
        operation_owner: use_signal(|| None::<Rc<ExportOwner>>),
        serial: use_signal(|| 0_u64),
        message: use_signal(String::new),
    };
    let observed = observer::OBSERVED.read().clone();
    let now = *observer::CLOCK.read();
    let mut sync = controls.clone();
    use_effect(move || {
        let observed = observer::OBSERVED.read().clone();
        // CLOCK only wakes this effect (every 100 ms). Judge the review with
        // the actual monotonic time: CLOCK can trail a review prepared since
        // its last tick, and a review "received in the future" reads as
        // expired, closing it before the operator can confirm.
        let _tick = *observer::CLOCK.read();
        let now = super::monotonic_now_ms();
        if !sync.alive() {
            return;
        }
        // Retire only the original stale operation. Its late completion cannot
        // clear a replacement operation or rebind a confirmation to new auth.
        let retired_operation = sync
            .operation_owner
            .peek()
            .as_ref()
            .is_some_and(|owner| !owner.is_current());
        if retired_operation {
            let next = (*sync.serial.peek()).saturating_add(1);
            sync.serial.set(next);
            sync.operation_owner.set(None);
            sync.busy.set(false);
            sync.dialog.set(None);
            sync.message
                .set("Original mode request owner retired; no automatic resend.".into());
        }
        // A backend-applied identity change resets the draft to actual state.
        // Availability/Closing polls preserve the user draft and active review.
        // These are server descriptors, never client-inferred mode names.
        if let Some(o) = observed.as_ref().filter(|o| o.current(now))
            && let Some(c) = o.value.current.as_ref()
        {
            let applied = serde_json::to_string(&(
                &o.value.backend_instance,
                &o.value.authority_id,
                &o.value.principal_context,
                &o.value.policy_epoch,
                &c.id,
                &c.mode_id,
                c.simulation,
            ))
            .ok();
            let changed = applied.is_some() && *sync.applied_selection.peek() != applied;
            if changed || sync.selected.peek().is_empty() {
                sync.selected.set(c.mode_id.clone());
                sync.simulation.set(Some(c.simulation));
                sync.applied_selection.set(applied);
            }
        }
        if sync.dialog.peek().as_ref().is_some_and(|d| !d.valid(now)) {
            sync.dialog.set(None);
            sync.message
                .set("Mode confirmation retired or expired; no automatic resend.".into());
        }
    });
    let selected = controls.selected.read().clone();
    let simulation = *controls.simulation.read();
    let busy = *controls.busy.read();
    let dialog = controls.dialog.read().clone();
    let target = controls.target();
    let reason = controls.reason(inhibited);
    let can_draft = !inhibited
        && !busy
        && dialog.is_none()
        && observed.as_ref().is_some_and(|o| {
            o.current(now) && o.value.change_supported && o.value.permissions.can_change
        });
    let mut choose = controls.clone();
    let mut toggle = controls.clone();
    let review = controls.clone();
    let mut cancel = controls.clone();
    let confirm = controls.clone();
    let current = observed.as_ref().and_then(|o| o.value.current.clone());
    let changed = target.as_ref().is_some_and(|t| current.as_ref().is_none_or(|c| c.id != t.id));
    let how = if target.as_ref().is_some_and(contract::OptionEntry::switches_in_process) {
        "Switches in place while the station is idle. Sign-in and connection are kept; layout and controls refresh."
    } else if target.as_ref().is_some_and(contract::OptionEntry::restarts) {
        "The station restarts into the selected mode. Live data pauses for a few seconds and reconnects."
    } else {
        "Applied by the station after review and confirmation."
    };
    rsx! {section{class:"gs26-station-mode gs-card",style:"gap:var(--gs-space-3);overflow-wrap:anywhere;",
        div{class:"gs-section-header",h3{"Operating mode"}
            button{r#type:"button",class:"gs-btn gs-btn--ghost",onclick:move|_|{let n=(*observer::REFRESH.peek()).saturating_add(1);*observer::REFRESH.write()=n;},"Refresh mode status"}
        }
        if let Some(o)=observed.as_ref(){
            p{style:"margin:0;",if o.current(now){"Current: "}else{"Last reported (unverified): "}strong{"{o.value.current.as_ref().map_or(\"Unknown\",|c|c.label.as_str())}"}}
            if let Some(t)=&o.value.transition {p{role:"status",class:"gs-notice","Transition {t.state}: {t.phase}."}if let Some(e)=&t.error {p{role:"alert",class:"gs-notice gs-notice--danger","{e}"}}}
            if let Some(reason)=&o.value.reason {p{class:"gs-notice","{reason}"}}
        }else{p{role:"status",class:"gs-notice","This backend has not provided a usable V2 mode capability. Existing fixed-mode operation is unchanged."}}
        div{class:"gs-form-row",
            label{r#for:"station-operating-mode","Mode"}
            select{id:"station-operating-mode",aria_label:"Station operating mode",style:"font:inherit;min-height:44px;max-width:100%;",value:"{selected}",disabled:!can_draft,
                onchange:move|e|{if !choose.alive()||*choose.busy.peek()||choose.dialog.peek().is_some(){return;}let mode=e.value();if observer::OBSERVED.peek().as_ref().is_some_and(|o|o.current(super::monotonic_now_ms())&&o.value.modes.iter().any(|m|m.id==mode)){choose.selected.set(mode);}},
                option{value:"","Choose a backend mode"}
                if let Some(o)=&observed {for mode in &o.value.modes {option{value:"{mode.id}","{mode.label}"}}}
            }
        }
        label{style:"display:flex;gap:var(--gs-space-2);align-items:center;",input{r#type:"checkbox",aria_label:"Simulate telemetry and actions",checked:simulation==Some(true),disabled:!can_draft||simulation.is_none(),onchange:move|e|{if toggle.alive()&&!*toggle.busy.peek()&&toggle.dialog.peek().is_none(){toggle.simulation.set(Some(e.checked()));}}} "Simulation (incoming data and actions)"}
        if simulation.is_none(){p{class:"gs-notice","Execution domain unknown; no hardware domain is assumed."}}
        if let Some(target)=target.as_ref(){if changed{p{style:"margin:0;","Selected: {target.label}"}}}else{p{class:"gs-notice","This mode/Simulation combination is not advertised. No target is inferred."}}
        if changed {if let Some(reason)=reason.as_ref(){p{role:"status",class:"gs-notice gs-notice--warn","{reason}"}}}
        div{style:"display:flex;gap:var(--gs-space-2);flex-wrap:wrap;align-items:center;",
            button{r#type:"button",class:"gs-btn gs-btn--primary",disabled:busy||dialog.is_some()||reason.is_some()||target.is_none(),onclick:move|_|review.clone().prepare(inhibited),"Review mode change"}
            details{class:"gs-info",summary{"How switching works"}p{"{how} The station checks it is idle at review, at confirm and at the switch. Changing the selection sends nothing."}}
        }
        if !observer::ERROR.read().is_empty(){p{role:"status",class:"gs-notice","Mode observation: {observer::ERROR}"}}
        if !controls.message.read().is_empty(){p{role:"status",class:"gs-notice","{controls.message}"}}
        if let Some(prepared)=dialog.as_ref() {
            div{style:"position:fixed;inset:0;z-index:200100;background:#000a;display:flex;align-items:center;justify-content:center;padding:var(--gs-space-4);box-sizing:border-box;",
                div{class:"gs26-modal",role:"alertdialog",aria_modal:true,aria_label:"Review station mode change",style:"font:inherit;max-width:560px;width:100%;max-height:85vh;overflow:auto;padding:20px;border-radius:12px;background:{theme.panel_background};color:{theme.text_primary};border:1px solid {theme.border};display:grid;gap:var(--gs-space-3);",
                    h3{style:"margin:0;","Switch to {prepared.challenge.target.label}?"}
                    p{style:"margin:0;","{prepared.challenge.summary}"}
                    p{style:"margin:0;color:var(--gs-text-muted);","Output domain: {prepared.challenge.target.output_domain}"}
                    div{style:"display:flex;gap:var(--gs-space-2);flex-wrap:wrap;justify-content:flex-end;",
                        button{onmounted:move|event|async move{let _=event.data().set_focus(true).await;},onclick:move|_|cancel.dialog.set(None),"Cancel review"}
                        button{class:"gs26-modal-primary",disabled:busy||!prepared.valid(super::monotonic_now_ms()),onclick:move|_|confirm.clone().confirm(inhibited),"Confirm mode change"}
                    }
                }
            }
        }
    }}
}
