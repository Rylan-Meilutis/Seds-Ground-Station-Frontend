//! Operator-only recovery UI. Polling reads local status; it never issues queries/actions.
use super::board_recovery_guard::{ConnectionBinding, challenge_unexpired, status_age_ms};
use super::board_recovery_model::*;
use super::layout::ThemeConfig;
use dioxus::prelude::*;
use serde::Serialize;

const STATUS: &str = "/api/board-recovery/status";
#[derive(Clone)]
struct Snapshot {
    status: Status,
    received: f64,
    binding: ConnectionBinding,
}
impl Snapshot {
    fn elapsed(&self) -> u64 {
        status_age_ms(self.received, super::monotonic_now_ms())
    }
}
#[derive(Clone)]
struct Pending {
    challenge: Challenge,
    snapshot: Snapshot,
    received: f64,
}
impl Pending {
    fn valid(&self, current: &Snapshot) -> bool {
        self.snapshot.binding == ConnectionBinding::current()
            && (current.status.reset_startup_policy.is_none() || current.elapsed() <= 2_000)
            && current.binding == self.snapshot.binding
            && challenge_unexpired(
                self.received,
                self.challenge.expires_in_ms,
                super::monotonic_now_ms(),
            )
            && self
                .challenge
                .context_unchanged(&self.snapshot.status, &current.status)
    }
}
async fn read_status() -> Result<Snapshot, String> {
    let binding = ConnectionBinding::current();
    let request_started = super::monotonic_now_ms();
    let status = super::http_get_json::<Status>(STATUS).await?;
    if binding != ConnectionBinding::current() {
        return Err("Station or sign-in changed; refresh the current station.".into());
    }
    if !status.valid() {
        return Err("This backend does not provide supported recovery API v1 status.".into());
    }
    Ok(Snapshot {
        status,
        received: request_started,
        binding,
    })
}
fn display_bool(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "Yes",
        Some(false) => "No",
        None => "Unknown",
    }
}
fn acceptance_message(value: &Accepted) -> &'static str {
    if !value.accepted {
        return "Request refused / not confirmed.";
    }
    if let Some(operation) = value.operation.as_ref()
        && operation.global_startup_best_effort == Some(true)
    {
        if operation.targets.is_empty() && operation.selected_proof_count == Some(0) {
            return "Request accepted for host reset; no board proof requested.";
        }
        if !operation.targets.is_empty()
            && operation.selected_proof_count == Some(operation.targets.len())
        {
            return "Request accepted; selected board proof is required. Global broadcast is best effort.";
        }
        return "Request accepted; awaiting server result. Board proof is not confirmed.";
    }
    "Request accepted; awaiting fresh device confirmation."
}
fn operation_view(operation: &Operation) -> Element {
    rsx! {div{role:"status",class:"gs26-recovery-operation",
        strong{"{operation.label()} — {operation.phase}"}
        p{"{operation.message}"}
        if operation.completed && operation.success == Some(true) && operation.reset_applied_state == Some(0) {
            p{"Reset applied Startup. Normal initialization may advance the station to Idle."}
        }
        if operation.global_startup_best_effort == Some(true) {
            p{"Global Startup broadcast: best effort. Unselected boards remain unverified."}
            if operation.targets.is_empty() {p{"Proof targets: none. This result claims no board completion or readback."}}
            else {p{"Selected proof targets: {operation.targets.join(\", \")}."}}
        } else {p{"Targets: {operation.targets.join(\", \")}."}}
        p{"SDK acceptance alone is not device confirmation."}
    }}
}
fn challenge_scope_view(challenge: &Challenge) -> Element {
    rsx! {div{class:"gs26-recovery-confirm-scope",
                    if challenge.action=="reset_startup" && challenge.global_startup_best_effort==Some(true) {
                        p{"A GLOBAL Startup request targets ALL boards on a best-effort basis. The server summary describes this attempt; commissioning or identity reconciliation alone does not complete a reset. Only the frozen proof targets below are verified; unselected boards remain unverified."}
                        if challenge.targets.is_empty(){p{"Proof targets: NONE (explicit HITL host-only policy). No board completion or readback is claimed."}}
                        else {p{"Frozen proof targets: {challenge.targets.join(\", \")}."}}
                    }else{
                        p{"Frozen targets: {challenge.targets.join(\", \")}. Server refreshes board readbacks at confirmation; acceptance alone does not prove safe state."}
                        p{"Only the frozen targets above are requested."}
                    }
                    p{"Physical valve position is not measured by command-register readbacks."}
                    if challenge.action=="reset_startup" {p{"The reset request asks for VB vent/dump OPEN, pilot/supplies/igniter OFF and stepper asleep. Confirm the area is safe. A best-effort request does not prove unselected outputs changed."}}
    }}
}
fn begin_prepare(
    action: String,
    board: Option<String>,
    mode: Option<String>,
    snapshot: Snapshot,
    mut busy: Signal<bool>,
    mut pending: Signal<Option<Pending>>,
    mut message: Signal<String>,
) {
    if *busy.peek() || pending.peek().is_some() {
        return;
    }
    if !crate::auth::can_view_actions() {
        message.set("Command permission is required.".into());
        return;
    }
    if snapshot.binding != ConnectionBinding::current() {
        message.set("Connection changed; refresh first.".into());
        return;
    }
    // Render-time enablement is not an admission check: age/permissions may
    // have changed before the click event executes.
    let reason = match action.as_str() {
        "reset_startup" => snapshot.status.reset_reason(snapshot.elapsed()),
        "change_mode" => snapshot.status.mode_reason(mode.as_deref().unwrap_or("")),
        "recover_abort" | "resync" => board
            .as_deref()
            .and_then(|id| snapshot.status.board(id))
            .map(|board| {
                snapshot
                    .status
                    .prepare_board_reason(board, &action, snapshot.elapsed())
            })
            .unwrap_or_else(|| Some("Board context is unavailable.".into())),
        _ => Some("Unsupported recovery action.".into()),
    };
    if let Some(reason) = reason {
        message.set(reason);
        return;
    }
    busy.set(true);
    message.set("Preparing a server-bound confirmation; no action has been confirmed.".into());
    spawn(async move {
        let body = Prepare {
            action: &action,
            board: board.as_deref(),
            mode: mode.as_deref(),
            backend_instance: &snapshot.status.backend_instance,
            reset_epoch: &snapshot.status.reset_epoch,
        };
        let prepare_started = super::monotonic_now_ms();
        let result =
            super::http_post_json::<_, Challenge>("/api/board-recovery/prepare", &body).await;
        match result {
            Ok(challenge) => {
                // Preparation may explicitly refresh the board. Bind its resulting
                // immutable identity snapshot, never the older pre-query nonce.
                let received = prepare_started;
                match read_status().await {
                    Ok(fresh) if snapshot.binding==fresh.binding && challenge.matches(&fresh.status,&action,board.as_deref(),mode.as_deref()) && challenge_unexpired(received,challenge.expires_in_ms,super::monotonic_now_ms())=>{
                        pending.set(Some(Pending{challenge,snapshot:fresh,received}));message.set(String::new());
                    }
                    Ok(_)=>message.set("The prepared context changed or was unsupported. Review fresh status and prepare again.".into()),
                    Err(error)=>message.set(error),
                }
            }
            Err(error) => message.set(error),
        }
        busy.set(false);
    });
}

#[component]
pub(super) fn BoardRecoveryPanel(ws_connected: bool, theme: ThemeConfig) -> Element {
    let mut snapshot = use_signal(|| None::<Snapshot>);
    let mut pending = use_signal(|| None::<Pending>);
    let mut message = use_signal(String::new);
    let mut status_error = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut phrase = use_signal(String::new);
    let mut mode = use_signal(String::new);
    let mut serial = use_signal(|| 0_u64);
    use_future(move || async move {
        loop {
            let n = serial.peek().wrapping_add(1);
            serial.set(n);
            let result = read_status().await;
            if *serial.peek() == n {
                match result {
                    Ok(fresh) => {
                        if pending.peek().as_ref().is_some_and(|p| !p.valid(&fresh)) {
                            pending.set(None);
                            phrase.set(String::new());
                            message.set("Confirmation cancelled: board/session changed or ticket expired. No automatic retry.".into());
                        }
                        snapshot.set(Some(fresh));
                        status_error.set(String::new());
                    }
                    Err(error) => {
                        snapshot.set(None);
                        pending.set(None);
                        phrase.set(String::new());
                        status_error.set(error);
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(1000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    let refresh = move |_| {
        let n = serial.peek().wrapping_add(1);
        serial.set(n);
        spawn(async move {
            match read_status().await {
                Ok(fresh) if *serial.peek() == n => {
                    snapshot.set(Some(fresh));
                    status_error.set(String::new());
                }
                Err(error) if *serial.peek() == n => {
                    snapshot.set(None);
                    pending.set(None);
                    status_error.set(error);
                }
                _ => {}
            }
        });
    };
    let current = snapshot.read().clone();
    let dialog = pending.read().clone();
    let controls_busy = *busy.read() || dialog.is_some();
    let permitted = crate::auth::can_view_actions();
    let connected = ws_connected
        && current
            .as_ref()
            .is_some_and(|x| x.binding == ConnectionBinding::current());
    rsx! {
        section{class:"gs26-board-recovery",style:"padding:16px;margin-bottom:16px;border:1px solid {theme.border};border-radius:14px;background:{theme.panel_background};color:{theme.text_primary};overflow-wrap:anywhere;min-width:0;",
            h3{"Board state and recovery"}
            p{"Board state is a device readback, separate from GS flight state. Recovery is never retried automatically."}
            button{onclick:refresh,"Refresh local status"}
            if !status_error.read().is_empty(){p{role:"alert","{status_error}"}}
            if !message.read().is_empty(){p{role:"status","{message}"}}
            if let Some(current)=current.clone(){
                {let status=&current.status;
                    let reset_reason=status.reset_reason(current.elapsed());
                    let selected=mode.read().clone();
                    let mode_reason=status.mode_reason(&selected);
                    let operator=status.principal.username.clone().unwrap_or_else(||"Unidentified".into());
                    rsx!{
                        p{"Operator: {operator}."}
                        p{"GS state: {status.flight_state.map(|n|n.to_string()).unwrap_or_else(||\"Unknown\".into())}. Automatic actions inhibited: {status.automatic_actions_inhibited}."}
                        div{class:"gs26-recovery-mode",
                            h4{"Runtime mode"}
                            p{"Active: {status.mode_capability.current}; server build: {status.mode_capability.build_mode}."}
                            p{"{status.mode_capability.message}"}
                            if !status.mode_capability.requires_restart.is_empty(){p{"Requires another build/restart: {status.mode_capability.requires_restart.join(\", \")}."}}
                            select{style:"max-width:100%;min-height:44px;",aria_label:"Supported runtime mode",value:"{selected}",disabled:!connected||controls_busy||!permitted||!status.mode_capability.runtime_switch_supported,
                                onchange:move|e|mode.set(e.value()),
                                option{value:"","Choose an actually supported mode"}
                                for item in status.mode_capability.supported_modes.iter(){option{value:"{item}","{item}"}}
                            }
                            button{disabled:!connected||controls_busy||!permitted||mode_reason.is_some(),title:mode_reason.clone().unwrap_or_default(),
                                onclick:{let current=current.clone();let selected=selected.clone();move |_|begin_prepare("change_mode".into(),None,Some(selected.clone()),current.clone(),busy,pending,message)},
                                "Review mode change"
                            }
                            if let Some(reason)=mode_reason.clone(){p{"{reason}"}}
                        }
                        div{class:"gs26-reset-all",
                            button{disabled:!connected||controls_busy||!permitted||reset_reason.is_some(),title:reset_reason.clone().unwrap_or_default(),
                                onclick:{let current=current.clone();move |_|begin_prepare("reset_startup".into(),None,None,current.clone(),busy,pending,message)},
                                "Reset ALL to Startup…"
                            }
                            p{"Reset requests AB supplies and igniter OFF, stepper asleep; VB pilot OFF, vent and dump OPEN, plus cancellation of queued automation. Unselected boards are best effort under a scoped policy. Clock, calibration and network power are preserved."}
                            if let Some(reason)=reset_reason.clone(){p{"{reason}"}}
                        }
                        if let Some(recovery)=status.host_recovery.as_ref(){
                            if !recovery.publication_acknowledged && (recovery.pending || recovery.verified){
                                p{role:"status","Previous recovery is incomplete for {recovery.targets.join(\", \")}. Check board status and explicitly retry."}
                            }
                        }
                        if let Some(operation)=status.operation.as_ref(){{operation_view(operation)}}
                        div{style:"display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,260px),1fr));gap:12px;",
                            for id in BOARD_IDS {
                                {let board=status.board(id).cloned().unwrap_or_else(||Board{id:id.into(),label:id.into(),..Default::default()});
                                 let recover_reason=status.prepare_board_reason(&board,"recover_abort",current.elapsed());
                                 let resync_reason=status.prepare_board_reason(&board,"resync",current.elapsed());
                                 let query_allowed=status.permissions.query&&board.query_allowed();
                                 let board_id=board.id.clone();
                                 let state=if board.state.is_some(){board.state_label.clone()}else{"Unknown".into()};
                                 let network_age=board.network.age_ms.saturating_add(current.elapsed());
                                 let network_label=if connected && board.network.healthy && network_age<=12000{"Healthy"}else{"Unconfirmed / stale"};
                                 let freshness=if board.fresh(current.elapsed()){"fresh"}else{"unconfirmed/stale"};
                                 let boot_label=if board.first_uncertainty_context(){"Identity verification pending".into()}else if board.commissioning_context(){"Not yet commissioned".into()}else{board.boot.clone().unwrap_or_else(||"Unknown".into())};
                                 let epoch_label=board.epoch.map(|v|v.to_string()).unwrap_or_else(||"Unknown".into());
                                 let bits=board.commanded_outputs.map(|n|format!("0x{n:08x}")).unwrap_or_else(||"Unknown".into());
                                 rsx!{article{class:"gs26-board-recovery-card","data-board":id,style:"border:1px solid {theme.border};padding:12px;border-radius:10px;",
                                    h4{"{board.label} ({board.id})"}
                                    p{"Network: {network_label}; age {network_age}ms; disconnects {board.network.disconnects}, reconnects {board.network.reconnects}."}
                                    p{"Actual board state: {state} ({freshness})"}
                                    p{"Abort latched: {display_bool(board.abort_latched)}; commanded outputs safe: {display_bool(board.safe_outputs)}."}
                                    p{"Boot: {boot_label}; epoch: {epoch_label}."}
                                    if board.commissioning_required {p{"Storage or identity setup requires explicit confirmation; it is not a completed reset."}}
                                    p{"Output readback: {board.output_readback}; register bits: {bits}. Physical valve position is not verified by GPIO/servo command registers."}
                                    if board.physical_position_verified {p{"Server claims physical verification; these command-register fields alone do not establish valve position."}}
                                    if let Some(text)=board.message.as_ref(){p{"{text}"}}
                                    button{disabled:!connected||controls_busy||!query_allowed,title:if query_allowed{"Request fresh status through the advertised dedicated recovery endpoint"}else{"Unsupported: no verified dedicated recovery endpoint"},
                                        onclick:{let board_id=board_id.clone();let binding=current.binding.clone();move |_|{
                                            if *busy.peek()||pending.peek().is_some()||binding!=ConnectionBinding::current(){return;}
                                            busy.set(true);let board_id=board_id.clone();let binding=binding.clone();
                                            spawn(async move{
                                                #[derive(Serialize)]struct Query{board:String}
                                                let result=super::http_post_json::<_,Accepted>("/api/board-recovery/query",&Query{board:board_id}).await;
                                                if binding==ConnectionBinding::current(){match result{Ok(value)=>message.set(format!("{} {}",if value.accepted{"Query accepted; awaiting board readback."}else{"Query not accepted."},value.message)),Err(error)=>message.set(error)}}
                                                busy.set(false);
                                            });
                                        }},
                                        "Query board"
                                    }
                                    button{disabled:!connected||controls_busy||!permitted||recover_reason.is_some(),title:recover_reason.clone().unwrap_or_default(),
                                        onclick:{let current=current.clone();let id=board_id.clone();move |_|begin_prepare("recover_abort".into(),Some(id.clone()),None,current.clone(),busy,pending,message)},{super::board_recovery_model::targeted_recovery_label(&board_id).map_or("Recovery unavailable", |(label, _)| label)}}
                                    if let Some((_, description))=super::board_recovery_model::targeted_recovery_label(&board_id){p{"{description}"}}
                                    button{disabled:!connected||controls_busy||!permitted||resync_reason.is_some(),title:resync_reason.clone().unwrap_or_default(),
                                        onclick:{let current=current.clone();let id=board_id.clone();move |_|begin_prepare("resync".into(),Some(id.clone()),None,current.clone(),busy,pending,message)},"Resync…"}
                                    if let Some(reason)=recover_reason.clone(){p{"Recovery unavailable: {reason}"}}
                                    if let Some(operation)=board.operation.as_ref(){{operation_view(operation)}}
                                 }}
                                }
                            }
                        }
                    }
                }
            }else{p{"Recovery status unavailable. No control action is enabled."}}
        }
        if let Some(dialog)=dialog {
            div{style:"position:fixed;inset:0;z-index:200000;background:#000a;display:flex;align-items:center;justify-content:center;padding:20px;box-sizing:border-box;",
                onkeydown:move|e|{if e.key()==Key::Escape && !*busy.peek(){pending.set(None);phrase.set(String::new());}},
                div{role:"alertdialog","aria-modal":"true","aria-labelledby":"board-recovery-confirm-title",style:"background:var(--gs26-panel-background,#142534);color:var(--gs26-app-text,#e5e7eb);font:inherit;padding:22px;border-radius:14px;width:min(100%,620px);max-height:90vh;overflow:auto;box-sizing:border-box;overflow-wrap:anywhere;",
                    h2{id:"board-recovery-confirm-title","Confirm operator recovery action"}
                    p{"{dialog.challenge.summary}"}
                    {challenge_scope_view(&dialog.challenge)}
                    label{r#for:"board-recovery-confirm-phrase","Type {dialog.challenge.confirmation} to confirm"}
                    input{style:"width:100%;box-sizing:border-box;min-height:44px;",id:"board-recovery-confirm-phrase",value:"{phrase}",oninput:move|e|phrase.set(e.value()),disabled:*busy.read(),autocomplete:"off"}
                    button{autofocus:true,disabled:*busy.read(),onclick:move |_|{pending.set(None);phrase.set(String::new());},"Cancel"}
                    button{disabled:*busy.read()||!connected||!permitted||phrase.read().as_str()!=dialog.challenge.confirmation,
                        onclick:move |_|{
                            if *busy.peek()||!crate::auth::can_view_actions(){return;}
                            let Some(owned)=pending.write().take()else{return;};
                            let typed=phrase.peek().clone();phrase.set(String::new());
                            if typed!=owned.challenge.confirmation {message.set("Confirmation phrase did not match. No action sent.".into());return;}
                            busy.set(true);
                            spawn(async move{
                                match read_status().await{
                                    Ok(fresh)if crate::auth::can_view_actions() && owned.valid(&fresh)=>{
                                        let challenge=&owned.challenge;
                                        let body=Confirm{ticket:&challenge.ticket,backend_instance:&challenge.backend_instance,principal_context:&challenge.principal_context,reset_epoch:&challenge.reset_epoch,confirmation:&typed};
                                        match super::http_post_json::<_,Accepted>("/api/board-recovery/confirm",&body).await{
                                            Ok(value)=>{
                                                if owned.snapshot.binding==ConnectionBinding::current(){
                                                    let result=acceptance_message(&value);
                                                    let op=value.operation.map(|op|format!(" {}: {}",op.label(),op.message)).unwrap_or_default();
                                                    message.set(format!("{result} {}{op}",value.message));
                                                }
                                            }
                                            Err(error)=>message.set(error),
                                        }
                                    }
                                    Ok(_)=>message.set("Confirmation expired or its board/session context changed. No action sent; review and prepare again.".into()),
                                    Err(error)=>message.set(error),
                                }
                                busy.set(false);
                            });
                        },"Confirm recovery action"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn result(host_only: bool) -> Operation {
        serde_json::from_value(serde_json::json!({
            "action":"reset_startup","completed":true,"success":true,"phase":"completed",
            "targets":if host_only {vec![]} else {vec!["AB","VB","GB","DAQ"]},
            "reset_applied_state":0,"global_startup_best_effort":true,
            "selected_proof_count":if host_only {0} else {4},
            "unselected_board_verification":"unverified_best_effort"
        }))
        .unwrap()
    }
    fn host_result() -> Element {
        operation_view(&result(true))
    }
    fn selected_result() -> Element {
        operation_view(&result(false))
    }
    fn host_scope() -> Element {
        let ticket: Challenge = serde_json::from_value(serde_json::json!({
            "ticket":"a","backend_instance":"b","principal_context":"c","reset_epoch":"d",
            "expires_in_ms":30000,"action":"reset_startup","targets":[],"summary":"Fixture only",
            "confirmation":"RESET ALL STARTUP; VERIFY NONE","global_startup_best_effort":true,
            "required_proof_boards":[],"unselected_board_verification":"unverified_best_effort"
        }))
        .unwrap();
        challenge_scope_view(&ticket)
    }
    fn render(component: fn() -> Element) -> String {
        let mut dom = VirtualDom::new(component);
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }
    #[test]
    fn actual_host_only_result_never_renders_board_readback_confirmation() {
        let html = render(host_result);
        assert!(html.contains("Host Startup reset; no board proof"));
        assert!(html.contains("Unselected boards remain unverified"));
        assert!(!html.contains("Confirmed by board readbacks"));
    }
    #[test]
    fn actual_selected_result_keeps_global_request_separate_from_selected_proof() {
        let html = render(selected_result);
        assert!(html.contains("Selected boards verified; global broadcast best effort"));
        assert!(html.contains("Selected proof targets: AB, VB, GB, DAQ"));
        assert!(html.contains("Unselected boards remain unverified"));
    }
    #[test]
    fn actual_confirmation_scope_discloses_global_best_effort_and_host_only_none() {
        let html = render(host_scope);
        assert!(html.contains("GLOBAL Startup request targets ALL boards"));
        assert!(html.contains("NONE (explicit HITL host-only policy)"));
        assert!(!html.contains("Only the frozen targets above are requested"));
        assert!(html.contains("does not prove unselected outputs changed"));
    }
    #[test]
    fn acceptance_copy_never_promises_board_proof_for_host_only_reset() {
        let mut accepted: Accepted =
            serde_json::from_value(serde_json::json!({"accepted":true,"message":"fixture"}))
                .unwrap();
        accepted.operation = Some(result(true));
        assert_eq!(
            acceptance_message(&accepted),
            "Request accepted for host reset; no board proof requested."
        );
        accepted.operation = Some(result(false));
        assert_eq!(
            acceptance_message(&accepted),
            "Request accepted; selected board proof is required. Global broadcast is best effort."
        );
        accepted.operation = None;
        assert_eq!(
            acceptance_message(&accepted),
            "Request accepted; awaiting fresh device confirmation."
        );
        accepted.accepted = false;
        assert_eq!(
            acceptance_message(&accepted),
            "Request refused / not confirmed."
        );
    }
}
