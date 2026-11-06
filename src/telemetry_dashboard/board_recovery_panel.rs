//! Operator-only recovery UI. Polling reads local status; it never issues queries/actions.
use super::board_recovery_guard::{ConnectionBinding, challenge_unexpired, status_age_ms};
use super::board_recovery_model::*;
use super::layout::ThemeConfig;
use dioxus::prelude::*;
use serde::Serialize;

const STATUS: &str = "/api/board-recovery/status";
#[derive(Clone)]
struct Snapshot {
    status: std::sync::Arc<Status>,
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
    action: String,
    board: Option<String>,
    mode: Option<String>,
}
impl Pending {
    fn locally_live(&self) -> bool {
        self.snapshot.binding == ConnectionBinding::current()
            && challenge_unexpired(
                self.received,
                self.challenge.expires_in_ms,
                super::monotonic_now_ms(),
            )
    }
    fn same_origin(&self, current: &Snapshot) -> bool {
        self.snapshot.binding == ConnectionBinding::current()
            && current.binding == self.snapshot.binding
            && self.snapshot.status.backend_instance == current.status.backend_instance
            && self.snapshot.status.principal.context == current.status.principal.context
            && self.snapshot.status.reset_epoch == current.status.reset_epoch
            && self.snapshot.status.mode_capability.current == current.status.mode_capability.current
            && self.snapshot.status.mode_capability.build_mode == current.status.mode_capability.build_mode
    }
    fn valid(&self, current: &Snapshot) -> bool {
        self.snapshot.binding == ConnectionBinding::current()
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
// A request started before PREPARE cannot revoke the challenge returned by a newer
// request. Authority changes are never discarded by this ordering fence.
fn older_same_authority(incoming: &Snapshot, current: &Snapshot) -> bool {
    incoming.received < current.received
        && incoming.binding == current.binding
        && incoming.status.backend_instance == current.status.backend_instance
        && incoming.status.reset_epoch == current.status.reset_epoch
        && incoming.status.principal.context == current.status.principal.context
}
async fn read_status() -> Result<Snapshot, String> {
    read_status_classified()
        .await
        .map_err(|error| error.message)
}
async fn read_status_classified() -> Result<Snapshot, super::recording_download::SavedJsonError> {
    let binding = ConnectionBinding::current();
    let request_started = super::monotonic_now_ms();
    let owner = super::recording_download::ExportOwner::capture_selected(|| true)?;
    let status = super::recording_download::saved_json_classified::<Status>(STATUS, owner).await?;
    if binding != ConnectionBinding::current() {
        return Err("Station or sign-in changed; refresh the current station.".into());
    }
    if !status.valid() {
        return Err("This backend does not provide supported recovery API v1 status.".into());
    }
    Ok(Snapshot {
        status: std::sync::Arc::new(status),
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
                        p{"Frozen targets: {challenge.targets.join(\", \")}. Server verifies board proof at confirmation; acceptance alone does not prove safe state."}
                        p{"Only the frozen targets above are requested."}
                    }
                    p{"Physical valve position is not measured by command-register readbacks."}
                    if challenge.action=="reset_startup" {p{"The reset request asks for VB vent/dump OPEN, pilot/supplies/igniter OFF and stepper asleep. Confirm the area is safe. A best-effort request does not prove unselected outputs changed."}}
    }}
}
fn begin_prepare(
    action: String, board: Option<String>, mode: Option<String>, snapshot: Snapshot,
    busy: Signal<bool>, pending: Signal<Option<Pending>>, message: Signal<String>,
) {
    begin_prepare_internal(action, board, mode, snapshot, busy, pending, message, false);
}
fn begin_prepare_internal(
    action: String,
    board: Option<String>,
    mode: Option<String>,
    snapshot: Snapshot,
    mut busy: Signal<bool>,
    mut pending: Signal<Option<Pending>>,
    mut message: Signal<String>,
    replace_pending: bool,
) {
    if *busy.peek() || (!replace_pending && pending.peek().is_some()) {
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
                        pending.set(Some(Pending{challenge,snapshot:fresh,received,action:action.clone(),board:board.clone(),mode:mode.clone()}));message.set(String::new());
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

pub(super) fn network_healthy(connected: bool, server_healthy: bool) -> bool {
    // The backend owns health policy; displayed age is diagnostic only.
    connected && server_healthy
}

pub(super) fn health_badge(healthy: bool) -> Element {
    let (label, colors) = if healthy {
        ("Healthy", "color:#86efac;background:#123b27;")
    } else {
        ("Unhealthy", "color:#fecaca;background:#471e25;")
    };
    rsx! { strong { class: "gs26-board-health", style: "{colors}padding:3px 9px;border-radius:999px;font-size:0.85em;", "{label}" } }
}

#[derive(Clone, Copy)]
struct RecoveryUi {
    snapshot: Signal<Option<Snapshot>>,
    pending: Signal<Option<Pending>>,
    message: Signal<String>,
    status_error: Signal<String>,
    busy: Signal<bool>,
    mode: Signal<String>,
    serial: Signal<u64>,
    display: Signal<super::board_readback_display::DisplayCache>,
}
// One owner per Connectivity page, independent of card count or presentation.
pub(super) fn use_recovery_provider() {
    let mut snapshot = use_signal(|| None::<Snapshot>);
    let mut display = use_signal(super::board_readback_display::DisplayCache::default);
    let mut pending = use_signal(|| None::<Pending>);
    let mut message = use_signal(String::new);
    let mut status_error = use_signal(String::new);
    let busy = use_signal(|| false);
    let mode = use_signal(String::new);
    let mut serial = use_signal(|| 0_u64);
    use_future(move || async move {
        loop {
            let n = serial.peek().wrapping_add(1);
            serial.set(n);
            let result = read_status_classified().await;
            if *serial.peek() == n {
                match result {
                    Ok(fresh) => {
                        let older = snapshot
                            .peek()
                            .as_ref()
                            .is_some_and(|current| older_same_authority(&fresh, current))
                            || pending.peek().as_ref().is_some_and(|current| {
                                older_same_authority(&fresh, &current.snapshot)
                            });
                        if older {
                            #[cfg(target_arch = "wasm32")]
                            gloo_timers::future::TimeoutFuture::new(1000).await;
                            #[cfg(not(target_arch = "wasm32"))]
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            continue;
                        }
                        display.write().observe(
                            &fresh.status,
                            &fresh.binding,
                            fresh.received,
                            super::monotonic_now_ms(),
                        );
                        if pending.peek().as_ref().is_some_and(|p| !p.valid(&fresh)
                            && (p.locally_live() || !p.same_origin(&fresh))) {
                            pending.set(None);
                            message.set("Confirmation cancelled: board/session changed or ticket expired. No automatic retry.".into());
                        }
                        snapshot.set(Some(fresh));
                        status_error.set(String::new());
                    }
                    Err(error) => {
                        if !error.transient {
                            snapshot.set(None);
                            display.set(Default::default());
                            pending.set(None);
                        }
                        if pending
                            .peek()
                            .as_ref()
                            .is_some_and(|pending| pending.snapshot.binding != ConnectionBinding::current())
                        {
                            pending.set(None);
                        }
                        status_error.set(error.message);
                    }
                }
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(1000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    use_context_provider(|| RecoveryUi {
        snapshot,
        pending,
        message,
        status_error,
        busy,
        mode,
        serial,
        display,
    });
}
pub(super) fn cycle_modal_focus(id: &str, backwards: bool) {
    let id = serde_json::to_string(id).expect("static modal id");
    let shift = if backwards { "-1" } else { "1" };
    super::js_eval(&format!(
        "(()=>{{const d=document.getElementById({id});if(!d)return;const a=[...d.querySelectorAll('button:not([disabled]),a[href],summary,input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]')].filter(e=>e.tabIndex>=0&&e.getClientRects().length);if(a.length){{const i=a.indexOf(document.activeElement);a[(i+{shift}+a.length)%a.length].focus();}}}})()"
    ));
}
fn connected(ui: RecoveryUi, ws_connected: bool) -> bool {
    ws_connected
        && ui
            .snapshot
            .read()
            .as_ref()
            .is_some_and(|snapshot| snapshot.binding == ConnectionBinding::current())
}
#[component]
pub(super) fn RecoveryBoardState(board_id: String) -> Element {
    let ui = use_context::<RecoveryUi>();
    let _ = *super::HEADER_CLOCK_TICK.read();
    let current = ui.snapshot.read().clone();
    let display = ui.display.read().display(
        current
            .as_ref()
            .map(|snapshot| (&*snapshot.status, snapshot.received)),
        &ConnectionBinding::current(),
        &board_id,
        super::monotonic_now_ms(),
    );
    let local_gs = board_id == "GS"
        && crate::auth::current_status().station_role.as_deref() != Some("rrls");
    let label = if local_gs {
        current.as_ref()
            .filter(|snapshot| snapshot.binding == ConnectionBinding::current())
            .map_or("Unknown", |snapshot| snapshot.status.global_state_label())
    } else if display.state.is_empty() {
        "Unknown"
    } else {
        display.state.as_str()
    };
    rsx! {span{class:"gs26-recovery-state","{label}"}}
}
#[component]
pub(super) fn RecoveryBoardActions(board_id: String, ws_connected: bool) -> Element {
    let ui = use_context::<RecoveryUi>();
    let RecoveryUi {
        snapshot,
        mut busy,
        pending,
        mut message,
        ..
    } = ui;
    if !BOARD_IDS.contains(&board_id.as_str()) {
        return rsx! {};
    }
    let unavailable = || rsx! {div{class:"gs26-recovery-actions",style:"display:flex;flex-wrap:wrap;gap:8px;margin-top:10px;",button{disabled:true,"Query board"}button{disabled:true,{targeted_recovery_label(&board_id).map_or("Recovery unavailable",|(label,_)|label)}}button{disabled:true,"Resync…"}}};
    let Some(current) = snapshot.read().clone() else {
        return unavailable();
    };
    let Some(board) = current.status.board(&board_id).cloned() else {
        return unavailable();
    };
    let status = &current.status;
    let connected = connected(ui, ws_connected);
    let permitted = crate::auth::can_view_actions();
    let controls_busy = *busy.read() || pending.read().is_some();
    let query_allowed = status.permissions.query && board.query_allowed();
    let recover_reason = status.prepare_board_reason(&board, "recover_abort", current.elapsed());
    let resync_reason = status.prepare_board_reason(&board, "resync", current.elapsed());
    rsx! {div{class:"gs26-recovery-actions",style:"display:flex;flex-wrap:wrap;gap:8px;margin-top:10px;",
                                    if board.recovery_notice().is_some() {
                                        span { role:"status",class:"gs26-recovery-required",style:"display:inline-flex;align-items:center;min-height:28px;padding:3px 9px;border:1px solid #b77b18;border-radius:999px;background:#3b2c11;color:#ffdc8a;font-size:0.85rem;font-weight:700;line-height:1.3;", "Recovery required" }
                                    }
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
                                        onclick:{let current=current.clone();let id=board_id.clone();move |_|begin_prepare("recover_abort".into(),Some(id.clone()),None,current.clone(),busy,pending,message)},{if board.recovery_notice().is_some() {"Review recovery…"} else {super::board_recovery_model::targeted_recovery_label(&board_id).map_or("Recovery unavailable", |(label, _)| label)}}}
                                    button{disabled:!connected||controls_busy||!permitted||resync_reason.is_some(),title:resync_reason.clone().unwrap_or_default(),
                                        onclick:{let current=current.clone();let id=board_id.clone();move |_|begin_prepare("resync".into(),Some(id.clone()),None,current.clone(),busy,pending,message)},"Resync…"}
    }}
}
#[component]
pub(super) fn RecoveryBoardDetails(
    board_id: String,
    ws_connected: bool,
    theme: ThemeConfig,
) -> Element {
    let ui = use_context::<RecoveryUi>();
    let _ = *super::HEADER_CLOCK_TICK.read();
    let current = ui.snapshot.read().clone();
    let raw = current
        .as_ref()
        .and_then(|snapshot| snapshot.status.board(&board_id));
    let display = ui.display.read().display(
        current
            .as_ref()
            .map(|snapshot| (&*snapshot.status, snapshot.received)),
        &ConnectionBinding::current(),
        &board_id,
        super::monotonic_now_ms(),
    );
    let recover_reason = current
        .as_ref()
        .and_then(|snapshot| {
            raw.map(|board| {
                snapshot
                    .status
                    .prepare_board_reason(board, "recover_abort", snapshot.elapsed())
            })
        })
        .unwrap_or_else(|| {
            Some("Current server proof is unavailable; controls remain disabled.".into())
        });
    let state = if display.state.is_empty() {
        "Unknown"
    } else {
        display.state.as_str()
    };
    let freshness = if display.retained {
        "last confirmed; refresh pending"
    } else if display.fresh {
        "fresh"
    } else {
        "unconfirmed/stale"
    };
    let boot_label = if display.boot.is_empty() {
        "Unknown"
    } else {
        display.boot.as_str()
    };
    let epoch_label = if display.epoch.is_empty() {
        "Unknown"
    } else {
        display.epoch.as_str()
    };
    let bits = if display.outputs.is_empty() {
        "Unknown"
    } else {
        display.outputs.as_str()
    };
    let _ = ws_connected;
    rsx! {section{class:"gs26-board-proof",style:"padding:14px;border:1px solid {theme.border_soft};border-radius:10px;min-width:0;",
    h4{style:"margin:0;","Device readback and recovery"}
    if let Some(notice)=raw.and_then(|board|board.recovery_notice()) {
        p{role:"status",style:"padding:10px;border:1px solid {theme.warning_border};border-radius:8px;background:{theme.warning_background};color:{theme.warning_text};line-height:1.45;overflow-wrap:anywhere;",strong{"Recovery required. "}"{notice}"}
    }
    if display.retained{p{class:"gs26-retained-readback","Last confirmed readback retained while refreshing. Commands still require current server proof."}}
                                            dl{style:"display:grid;grid-template-columns:minmax(0,1fr) minmax(0,2fr);gap:8px 12px;margin:18px 0;",
                                                dt{"Actual board state"}dd{style:"margin:0;","{state} ({freshness})"}
                                                dt{"Abort latched"}dd{style:"margin:0;","{display_bool(display.abort_latched)}"}
                                                dt{"Safe output registers"}dd{style:"margin:0;","{display_bool(display.safe_outputs)}"}
                                                dt{"Boot identity"}dd{style:"margin:0;","{boot_label}"}
                                                dt{"Recovery epoch"}dd{style:"margin:0;","{epoch_label}"}
                                                dt{"Query nonce"}dd{style:"margin:0;",{if display.nonce.is_empty(){"Unknown"}else{display.nonce.as_str()}}}
                                                dt{"Readback age"}dd{style:"margin:0;",{display.age_ms.map(|age|format!("{age} ms")).unwrap_or_else(||"Unknown".into())}}
                                                dt{"Remaining server lease"}dd{style:"margin:0;",{display.lease_ms.map(|lease|format!("{lease} ms")).unwrap_or_else(||"Not issued".into())}}
                                                dt{"Output source"}dd{style:"margin:0;","{display.output_source}"}
                                                dt{"Output bits"}dd{style:"margin:0;","{bits}"}
                                            }
                                if display.commissioning {p{"Storage or identity setup requires explicit confirmation; it is not a completed reset."}}
                                            p{"Physical valve position is not verified by GPIO/servo command registers."}
                                if display.physical_verified {p{"Server claims physical verification; these command-register fields alone do not establish valve position."}}
                                if let Some(text)=display.message.as_ref(){p{"{text}"}}
                                            if let Some((_, description))=super::board_recovery_model::targeted_recovery_label(&board_id){p{"{description}"}}
                                            if let Some(reason)=recover_reason.clone(){p{"Recovery unavailable: {reason}"}}
                                            if let Some(operation)=raw.and_then(|board|board.operation.as_ref()){{operation_view(operation)}}    }}
}
#[component]
pub(super) fn RecoveryTools(ws_connected: bool, theme: ThemeConfig) -> Element {
    let ui = use_context::<RecoveryUi>();
    let RecoveryUi {
        mut snapshot,
        mut pending,
        message,
        mut status_error,
        busy,
        mut mode,
        mut serial,
        mut display,
        ..
    } = ui;
    let mut open = use_signal(|| false);
    let refresh = move |_| {
        let n = serial.peek().wrapping_add(1);
        serial.set(n);
        spawn(async move {
            match read_status_classified().await {
                Ok(fresh) if *serial.peek() == n => {
                    display.write().observe(
                        &fresh.status,
                        &fresh.binding,
                        fresh.received,
                        super::monotonic_now_ms(),
                    );
                    snapshot.set(Some(fresh));
                    status_error.set(String::new());
                }
                Err(error) if *serial.peek() == n => {
                    if !error.transient {
                        snapshot.set(None);
                        display.set(Default::default());
                        pending.set(None);
                    }
                    if pending
                        .peek()
                        .as_ref()
                        .is_some_and(|pending| pending.snapshot.binding != ConnectionBinding::current())
                    {
                        pending.set(None);
                    }
                    status_error.set(error.message);
                }
                _ => {}
            }
        });
    };
    let current = snapshot.read().clone();
    let controls_busy = *busy.read() || pending.read().is_some();
    let permitted = crate::auth::can_view_actions();
    let connected = connected(ui, ws_connected);
    rsx! {
        button{r#type:"button",onclick:move |_|open.set(true),"Recovery tools"}
        if open(){
            div{style:"position:fixed;inset:0;z-index:199998;background:#000a;display:flex;align-items:center;justify-content:center;padding:16px;box-sizing:border-box;",onkeydown:move|event|{event.stop_propagation();match event.key(){Key::Escape=>{event.prevent_default();open.set(false);},Key::Tab=>{event.prevent_default();cycle_modal_focus("gs26-recovery-tools",event.modifiers().contains(Modifiers::SHIFT));},_=>{}}},
                div{id:"gs26-recovery-tools",role:"dialog","aria-modal":"true","aria-label":"Recovery tools",style:"background:{theme.panel_background};color:{theme.text_primary};padding:20px;border-radius:14px;width:min(100%,960px);height:min(88dvh,900px);max-height:calc(100dvh - 32px);display:flex;flex-direction:column;gap:12px;overflow:hidden;box-sizing:border-box;overflow-wrap:anywhere;font:inherit;",
                    h3{"Recovery tools"}
                    button{r#type:"button",onmounted:move|event|async move{let _=event.data().set_focus(true).await;},onclick:move |_|open.set(false),"Close tools"}
                    button{onclick:refresh,"Refresh local status"}
                    div{class:"gs26-modal-scroll-body",style:"flex:1;min-height:0;overflow-y:auto;overflow-anchor:none;overscroll-behavior:contain;padding:2px;",
                    if !status_error.read().is_empty(){p{role:"alert","{status_error}"}}
                    section{style:"padding:14px;border:1px solid var(--gs26-border,#405269);border-radius:10px;margin-bottom:14px;",h4{style:"margin:0 0 10px;","Launch readiness"}super::board_abort_readiness::BoardAbortReadiness{}}
                    if let Some(current)=current.clone(){
                        {let status=&current.status;let reset_reason=status.reset_reason(current.elapsed());let selected=mode.read().clone();let mode_reason=status.mode_reason(&selected);let operator=status.principal.username.clone().unwrap_or_else(||"Unidentified".into());rsx!{
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
                        }}
                    }else{p{"Recovery status unavailable. No control action is enabled."}}
                    }
                }
            }
        }
    }
}
#[component]
pub(super) fn RecoveryDialogs(ws_connected: bool) -> Element {
    let ui = use_context::<RecoveryUi>();
    let RecoveryUi {
        mut pending,
        mut busy,
        mut message,
        ..
    } = ui;
    let _ = *super::HEADER_CLOCK_TICK.read();
    let dialog = pending.read().clone().filter(|pending| pending.snapshot.binding == ConnectionBinding::current());
    let connected = connected(ui, ws_connected);
    let permitted = crate::auth::can_view_actions();
    rsx! {
        if !message.read().is_empty(){p{role:"status","{message}"}}
        if let Some(dialog)=dialog {
            div{style:"position:fixed;inset:0;z-index:200000;background:#000a;display:flex;align-items:center;justify-content:center;padding:20px;box-sizing:border-box;",
                onkeydown:move|event|{event.stop_propagation();match event.key(){Key::Escape if !*busy.peek()=>{event.prevent_default();pending.set(None);},Key::Tab=>{event.prevent_default();cycle_modal_focus("gs26-recovery-confirm",event.modifiers().contains(Modifiers::SHIFT));},_=>{}}},
                div{id:"gs26-recovery-confirm",role:"alertdialog","aria-modal":"true","aria-labelledby":"board-recovery-confirm-title",style:"background:var(--gs26-panel-background,#142534);color:var(--gs26-app-text,#e5e7eb);font:inherit;padding:22px;border-radius:14px;width:min(100%,960px);height:min(86dvh,860px);max-height:calc(100dvh - 40px);display:flex;flex-direction:column;gap:12px;overflow:hidden;box-sizing:border-box;overflow-wrap:anywhere;",
                    h2{id:"board-recovery-confirm-title","Confirm operator recovery action"}
                    div{class:"gs26-modal-scroll-body",style:"flex:1;min-height:0;overflow-y:auto;overflow-anchor:none;overscroll-behavior:contain;padding:2px;",
                        p{"{dialog.challenge.summary}"}
                        {challenge_scope_view(&dialog.challenge)}
                        if !message.read().is_empty() {p{role:"status",style:"padding:10px;border:1px solid #b77b18;border-radius:8px;line-height:1.45;overflow-wrap:anywhere;","{message}"}}
                        if !dialog.locally_live() {
                            p{role:"status",style:"padding:10px;border:1px solid #b77b18;border-radius:8px;background:#3b2c11;color:#ffdc8a;line-height:1.4;", "This review expired. Refresh the review to request fresh board proof, then confirm again. No action was sent."}
                        }
                    }
                    if !dialog.locally_live() {
                        button{r#type:"button",disabled:*busy.read()||!connected||!permitted,
                            onclick:{let original=dialog.clone();move |_|{
                                if *busy.peek()||!crate::auth::can_view_actions(){return;}
                                busy.set(true);message.set("Refreshing review; no recovery has been confirmed.".into());
                                let original=original.clone();
                                spawn(async move{
                                    match read_status().await {
                                        Ok(fresh) if original.same_origin(&fresh) => {
                                            busy.set(false);
                                            begin_prepare_internal(original.action,original.board,original.mode,fresh,busy,pending,message,true);
                                        }
                                        Ok(_)=>{pending.set(None);message.set("Station or session changed. Start a new recovery review.".into());busy.set(false);}
                                        Err(error)=>{message.set(error);busy.set(false);}
                                    }
                                });
                            }},"Refresh review"}
                    }
                    button{r#type:"button",onmounted:move|event|async move{let _=event.data().set_focus(true).await;},disabled:*busy.read(),onclick:move |_|pending.set(None),"Cancel"}
                    button{r#type:"button",disabled:*busy.read()||!connected||!permitted||!dialog.locally_live(),
                        onclick:move |_|{
                            if *busy.peek()||!crate::auth::can_view_actions(){return;}
                            let Some(owned)=pending.write().take()else{return;};
                            busy.set(true);
                            spawn(async move{
                                match read_status().await{
                                    Ok(fresh)if crate::auth::can_view_actions() && owned.valid(&fresh)=>{
                                        let challenge=&owned.challenge;
                                        let body=Confirm{ticket:&challenge.ticket,backend_instance:&challenge.backend_instance,principal_context:&challenge.principal_context,reset_epoch:&challenge.reset_epoch,confirmation:&challenge.confirmation};
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
    #[test]
    fn older_status_cannot_rewind_post_prepare_context_but_authority_change_is_seen() {
        let status: Status = serde_json::from_str(include_str!(
            "../../tests/fixtures/board-recovery/status-example.json"
        ))
        .unwrap();
        let binding = ConnectionBinding {
            backend: "https://fixture.invalid".into(),
            token: Some("fake-confirmation-token".into()),
            principal: Some("fixture".into()),
        };
        let current = Snapshot {
            status: std::sync::Arc::new(status.clone()),
            received: 200.,
            binding: binding.clone(),
        };
        let mut incoming = Snapshot {
            status: std::sync::Arc::new(status),
            received: 100.,
            binding,
        };
        assert!(older_same_authority(&incoming, &current));
        incoming.received = 201.;
        assert!(!older_same_authority(&incoming, &current));
        incoming.received = 100.;
        std::sync::Arc::make_mut(&mut incoming.status).reset_epoch = "different".into();
        assert!(!older_same_authority(&incoming, &current));
        incoming.status = current.status.clone();
        incoming.binding.token = Some("other-fake-token".into());
        assert!(!older_same_authority(&incoming, &current));
    }
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
    fn network_health_follows_server_policy_not_displayed_age() {
        let mut network = Network {
            healthy: true,
            age_ms: 30000,
            ..Default::default()
        };
        assert!(network_healthy(true, network.healthy));
        assert!(!network_healthy(false, network.healthy));
        network.healthy = false;
        assert!(!network_healthy(true, network.healthy));
    }
    fn healthy_badge() -> Element {
        health_badge(true)
    }
    fn unhealthy_badge() -> Element {
        health_badge(false)
    }
    #[test]
    fn actual_network_health_badges_are_explicit_and_distinct() {
        let healthy = render(healthy_badge);
        let unhealthy = render(unhealthy_badge);
        assert!(healthy.contains("Healthy"));
        assert!(healthy.contains("#86efac"));
        assert!(!healthy.contains("Unhealthy"));
        assert!(unhealthy.contains("Unhealthy"));
        assert!(unhealthy.contains("#fecaca"));
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
