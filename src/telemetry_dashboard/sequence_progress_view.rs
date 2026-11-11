use super::sequence_progress_model::{
    Evidence, Kind, Outcome, Panel, RunControls, RunKey, StepState, Value, Watch,
};
use dioxus::prelude::*;

pub(super) fn outcome_label(value: Outcome) -> &'static str {
    match value {
        Outcome::Running => "In progress",
        Outcome::Paused => "Paused",
        Outcome::Cancelling => "Cancelling safely",
        Outcome::Passed => "Passed",
        Outcome::Failed => "Failed",
        Outcome::Cancelled => "Warning: safely cancelled",
        Outcome::Interrupted => "Warning: interrupted",
        Outcome::Unknown => "Progress unknown",
    }
}
fn step_label(value: StepState) -> &'static str {
    match value {
        StepState::Planned => "Next",
        StepState::Current => "Current",
        StepState::Watching => "Watching",
        StepState::Passed => "Passed",
        StepState::Failed => "Failed",
        StepState::Skipped => "Skipped",
        StepState::Unknown => "Unknown",
    }
}
fn step_glyph(value: StepState) -> &'static str {
    match value {
        StepState::Planned => "○",
        StepState::Current | StepState::Watching => "◌",
        StepState::Passed => "✓",
        StepState::Failed => "×",
        StepState::Skipped => "–",
        StepState::Unknown => "?",
    }
}
fn evidence_label(value: Evidence) -> &'static str {
    match value {
        Evidence::Planned => "Planned",
        Evidence::Commanded => "Command accepted",
        Evidence::ElectricalConfirmed => "Electrical measured",
        Evidence::BoardReported => "Board reported",
        Evidence::Measured => "Measured",
        Evidence::Unknown => "Unknown",
    }
}
fn value_label(value: Option<&Value>) -> String {
    match value {
        Some(Value::Boolean(v)) => if *v { "On" } else { "Off" }.into(),
        Some(Value::Number(v)) if v.is_finite() => {
            if v.abs() >= 1_000_000. || (*v != 0. && v.abs() < 0.01) {
                format!("{v:.3e}")
            } else {
                format!("{v:.2}")
            }
        }
        _ => "Unknown".into(),
    }
}

// Recorded step readings keep their capture-time validity. They are never
// presented as a fresh live observation when a historical result is reopened.
fn recorded_value(watch: &Watch) -> Option<&Value> {
    if watch.evidence == Evidence::Unknown {
        return None;
    }
    if matches!(
        watch.evidence,
        Evidence::Measured | Evidence::BoardReported | Evidence::ElectricalConfirmed
    ) && watch.age_ms.is_none_or(|age| age > watch.valid_for_ms)
    {
        return None;
    }
    watch.observed.as_ref()
}

/// Pure view: only run-key intents leave this component. No command or HTTP fallback.
#[component]
pub fn SequenceProgressView(
    panel: Panel,
    now_ms: u64,
    can_send_commands: bool,
    cancel_api_version: Option<u16>,
    #[props(default = false)] unavailable: bool,
    #[props(default = false)] transient_transport_error: bool,
    on_minimize: EventHandler<RunKey>,
    on_restore: EventHandler<RunKey>,
    #[props(default)] board_display: super::sequence_progress_model::BoardDisplayMap,
    #[props(default)] output_display: super::sequence_progress_model::OutputDisplayMap,
    on_cancel: EventHandler<RunKey>,
    on_dismiss: EventHandler<RunKey>,
    #[props(default = false)] can_release_cold_flow: bool,
    #[props(default)] on_release_cold_flow: EventHandler<RunKey>,
    /// Live station admission for Pause/Resume/Cancel, with refusal reasons.
    #[props(default)] controls: Option<RunControls>,
    #[props(default = false)] can_pause: bool,
    #[props(default = false)] can_resume: bool,
    #[props(default)] on_pause: EventHandler<RunKey>,
    #[props(default)] on_resume: EventHandler<RunKey>,
    /// Cold flow chosen at a nitrogen-test pressure hold.
    #[props(default)] on_hold_cold_flow: EventHandler<RunKey>,
    /// Dump chosen at a nitrogen-test pressure hold (the run stays open).
    #[props(default)] on_hold_dump: EventHandler<RunKey>,
) -> Element {
    if !panel.visible {
        return rsx! {};
    }
    let key = panel.snapshot.key.clone();
    let restore_key = key.clone();
    let minimize_key = key.clone();
    let cancel_key = key.clone();
    let release_key = key.clone();
    let pause_key = key.clone();
    let resume_key = key.clone();
    let hold_cold_key = key.clone();
    let hold_dump_key = key.clone();
    let dismiss_key = key.clone();
    let keyboard_key = key.clone();
    let terminal = panel.snapshot.outcome.terminal();
    let fresh = panel.fresh(now_ms) && (!unavailable || transient_transport_error);
    let current_authority = panel.fresh(now_ms) && !unavailable;
    let display_authority = !terminal
        && !panel.historical
        && (!unavailable || transient_transport_error)
        && board_display
            .values()
            .any(|board| board.connected && board.known);
    let status = if !terminal && !fresh && display_authority {
        "Last confirmed progress"
    } else if !terminal && !fresh {
        "Progress unknown"
    } else {
        outcome_label(panel.snapshot.outcome)
    };
    let can_cancel = !unavailable
        && panel.can_cancel(now_ms, can_send_commands, cancel_api_version)
        && controls.as_ref().is_none_or(|c| c.cancel.allowed);
    let paused = panel.snapshot.outcome == Outcome::Paused;
    let can_pause = can_pause && !unavailable && current_authority;
    let can_resume = can_resume && !unavailable && current_authority;
    // Server-provided reasons first; then local reasons the station cannot see.
    let local_reason = if unavailable || !current_authority {
        Some("Current station status is unavailable".to_string())
    } else if panel.control_pending.is_some() || panel.cancel_pending {
        Some("Waiting for the station to apply the previous request".to_string())
    } else if !can_send_commands {
        Some("This session cannot send sequence commands".to_string())
    } else {
        None
    };
    let reason = |gate: Option<&super::sequence_progress_model::Gate>| {
        gate.and_then(|g| (!g.allowed).then(|| g.reason.clone()).flatten())
            .or_else(|| local_reason.clone())
            .unwrap_or_else(|| "Waiting for the station's control status".into())
    };
    let pause_reason = (!can_pause).then(|| reason(controls.as_ref().map(|c| &c.pause)));
    let resume_reason = (!can_resume).then(|| reason(controls.as_ref().map(|c| &c.resume)));
    let cancel_reason = (!can_cancel).then(|| reason(controls.as_ref().map(|c| &c.cancel)));
    let current = panel
        .snapshot
        .current_step
        .as_ref()
        .and_then(|id| panel.snapshot.steps.iter().find(|s| &s.id == id));
    let next = panel
        .snapshot
        .next_step
        .as_ref()
        .and_then(|id| panel.snapshot.steps.iter().find(|s| &s.id == id));
    let passed = panel
        .snapshot
        .steps
        .iter()
        .filter(|s| s.state == StepState::Passed)
        .count();
    let count = panel.snapshot.steps.len();
    let kind = match panel.snapshot.kind {
        Kind::ValveSelfTest => "valve-self-test",
        Kind::NitrogenTest => "nitrogen-test",
        Kind::Fill => "fill",
        Kind::Turbocharge => "turbocharge",
        Kind::ColdFlow => "cold-flow",
        Kind::ContinueFillSequence => "fill-sequence",
        Kind::IgniterSequence => "igniter-sequence",
    };
    rsx! {
        style {{include_str!("sequence_progress.css")}}
        if panel.minimized {
            button {r#type:"button",class:"gs26-sequence-chip", "data-sequence-kind":kind, "data-run-id":key.run_id.clone(),
                onclick:move |_|on_restore.call(restore_key.clone()),
                span {class:"gs26-sequence-dot", "aria-hidden":"true"}
                span {strong {"{panel.snapshot.title}"} span {"{status}"}}
                span {"aria-hidden":"true","↗"}
            }
        } else {
            article {class:"gs26-sequence-view", role:"dialog", aria_modal:"false", "aria-label":format!("{} progress",panel.snapshot.title),
                "data-sequence-kind":kind,"data-run-id":key.run_id.clone(),tabindex:"0",
                onkeydown:move |event|{if event.key().to_string()=="Escape" {event.prevent_default();event.stop_propagation();if terminal{on_dismiss.call(keyboard_key.clone());}else{on_minimize.call(keyboard_key.clone());}}},
                header {
                    div {span {class:"gs26-sequence-eyebrow",if panel.historical{"Sequence result"}else{"Shared sequence"}} h2 {"{panel.snapshot.title}"}}
                    if terminal {button {r#type:"button","aria-label":"Close sequence result",onclick:move |_|on_dismiss.call(dismiss_key.clone()),"×"}}
                    else {button {r#type:"button","aria-label":"Minimize sequence progress",onclick:move |_|on_minimize.call(minimize_key.clone()),"−"}}
                }
                div {class:"gs26-sequence-body",
                div {class:"gs26-sequence-summary",role:"status",aria_live:"polite","data-outcome":format!("{:?}",panel.snapshot.outcome),
                    strong {"{status}"}if panel.snapshot.details_complete {span {"{passed} of {count} steps passed"}}
                }
                if !terminal&&!fresh {p {class:"gs26-sequence-unknown","Current sensor confirmation is unavailable. Last reported board state is shown separately when connected."}}
                if !terminal&&transient_transport_error&&fresh {p{class:"gs26-sequence-refreshing","Last confirmed progress retained while reconnecting. Commands remain unavailable."}}
                p {class:"gs26-sequence-message","{panel.snapshot.message}"}
                if !terminal {
                    section {class:"gs26-sequence-current",h3 {"Current"}
                        if let Some(step)=current {p {"{step.label}"}}else{p {"Waiting for station progress"}}
                        if let Some(step)=current {
                            if !step.watch.is_empty(){h4 {"Watching"}}
                            for watch in &step.watch {
                                {let displayed=if !terminal&&!panel.historical&&(!unavailable||transient_transport_error){super::sequence_progress_model::board_output_display(watch,&output_display)}else{None};let metadata_present=!terminal&&!panel.historical&&(!unavailable||transient_transport_error)&&super::sequence_progress_model::board_output_metadata_present(watch,&board_display);let observed=displayed.as_ref().or_else(||if fresh&&!metadata_present{panel.observed_value(watch,now_ms)}else{None});let reading=value_label(observed);let target=value_label(watch.target.as_ref());let evidence=if displayed.is_some(){"Board-reported electrical state"}else if observed.is_some(){evidence_label(watch.evidence)}else{"Unknown"};
                                rsx!{div {class:"gs26-sequence-watch",key:"{watch.id}",
                                    span {"{watch.label}"}strong {"{reading}" if let Some(unit)=&watch.unit{" {unit}"}}
                                    small {"{evidence}"}small {if watch.target.is_some(){"Planned target: {target}"}}
                                }}}
                            }
                        }
                        if let Some(step)=next {p {class:"gs26-sequence-next",strong {"Next: "}"{step.label}"}}
                    }
                }
                if terminal && !panel.snapshot.details_complete {p {class:"gs26-sequence-message",role:"status","Loading recorded step details…"}}
                if terminal && panel.snapshot.details_complete {p {class:"gs26-sequence-legend","Step readings below were recorded while watching each step. They are not current live readings."}}
                if panel.snapshot.watch_history_truncated {p {class:"gs26-sequence-legend","Some earlier step readings are not retained. All step results are retained."}}
                if terminal&&!panel.snapshot.details_complete&&!panel.loading_steps.is_empty(){
                    ol{class:"gs26-sequence-steps","aria-label":"Recorded steps loading",for (id,label) in &panel.loading_steps{li{key:"{id}",span{"{label}"}small{"Loading final result…"}}}}
                }
                ol {class:"gs26-sequence-steps","aria-label":"Sequence steps",
                    for step in &panel.snapshot.steps {
                        {let state=if !terminal&&!fresh&&!display_authority&&matches!(step.state,StepState::Current|StepState::Watching){StepState::Unknown}else{step.state};let label=step_label(state);let glyph=step_glyph(state);
                        rsx!{li {key:"{step.id}","data-step-state":label,
                            if terminal && !step.watch.is_empty() {
                                details {summary {span {"{step.label}"}small {span {class:"gs26-sequence-step-glyph","aria-hidden":"true","{glyph}"} span {class:"gs26-sequence-step-label","{label}"}}}
                                    for watch in &step.watch {
                                        div {class:"gs26-sequence-watch",key:"{watch.id}",
                                            span {"{watch.label}"}strong {"{value_label(recorded_value(watch))}" if let Some(unit)=&watch.unit{" {unit}"}}
                                            small {"Last recorded while watching"}small {"{evidence_label(watch.evidence)}"}
                                        }
                                    }
                                }
                            } else {span {"{step.label}"}small {span {class:"gs26-sequence-step-glyph","aria-hidden":"true","{glyph}"} span {class:"gs26-sequence-step-label","{label}"}}}
                        }}}
                    }
                }
                }
                footer {
                    if panel.snapshot.kind==Kind::ColdFlow && !terminal {
                        p {class:"gs26-sequence-legend","Cold Flow opens PILOT/main only. No igniter or launch countdown. Cancel closes supplies/main, then dump and vent."}
                        if panel.snapshot.cold_flow_release_allowed {
                            button {class:"gs26-sequence-cancel",r#type:"button",disabled:!current_authority||!can_release_cold_flow,onclick:move |_|on_release_cold_flow.call(release_key.clone()),"Cold Flow · open PILOT/main"}
                            if !current_authority||!can_release_cold_flow {
                                p {class:"gs26-sequence-control-reason","data-control":"cold-flow-release","Release unavailable: {local_reason.clone().unwrap_or_else(|| \"This session cannot release Cold Flow\".into())}"}
                            }
                        }
                    }
                    if let Some(choice)=controls.as_ref().and_then(|c|c.hold_choice.clone()).filter(|_|!terminal) {
                        {let cold_ok=choice.cold_flow.allowed&&current_authority&&can_send_commands&&!unavailable;
                        let dump_ok=choice.dump.allowed&&current_authority&&can_send_commands&&!unavailable&&panel.control_pending.is_none();
                        let cold_reason=(!cold_ok).then(||reason(Some(&choice.cold_flow)));
                        let dump_reason=(!dump_ok).then(||reason(Some(&choice.dump)));
                        rsx!{section {class:"gs26-sequence-hold-choice","data-hold-verdict":if choice.passed{"passed"}else{"failed"},
                            p {role:"status",class:"gs26-sequence-message","{choice.message}"}
                            div {class:"gs26-sequence-controls",
                                button {class:"gs26-sequence-resume",r#type:"button","data-hold-choice":"cold-flow",disabled:!cold_ok,title:cold_reason.clone().unwrap_or_default(),onclick:move |_|on_hold_cold_flow.call(hold_cold_key.clone()),"Cold flow · held pressure"}
                                button {class:"gs26-sequence-cancel",r#type:"button","data-hold-choice":"dump",disabled:!dump_ok,title:dump_reason.clone().unwrap_or_default(),onclick:move |_|on_hold_dump.call(hold_dump_key.clone()),"Dump"}
                            }
                            p {class:"gs26-sequence-legend","Cold flow releases the pressure this test holds, not the standalone Auto Cold Flow target, and never re-pressurises; PILOT/main opens only on explicit release (panel Launch can release it). Dump opens dump and vent, never PILOT, and this window stays open until the tank is empty. Cancel opens dump and vent, ends the test and closes this window; venting continues."}
                            if let Some(text)=&cold_reason {p {class:"gs26-sequence-control-reason","data-control":"cold-flow","Cold flow unavailable: {text}"}}
                            if let Some(text)=&dump_reason {p {class:"gs26-sequence-control-reason","data-control":"dump","Dump unavailable: {text}"}}
                        }}}
                    }
                    if panel.cancel_pending {p {role:"status","Cancellation requested. Waiting for the station's safe-cancel result."}}
                    if panel.control_pending.is_some() {p {role:"status","Request sent. Waiting for the station to confirm it."}}
                    if !terminal {
                        div {class:"gs26-sequence-controls",
                            if paused {
                                button {class:"gs26-sequence-resume",r#type:"button",disabled:!can_resume,title:resume_reason.clone().unwrap_or_default(),onclick:move |_|on_resume.call(resume_key.clone()),"Resume sequence"}
                            } else {
                                button {class:"gs26-sequence-pause",r#type:"button",disabled:!can_pause,title:pause_reason.clone().unwrap_or_default(),onclick:move |_|on_pause.call(pause_key.clone()),"Pause sequence"}
                            }
                            button {class:"gs26-sequence-cancel",r#type:"button",disabled:!can_cancel,title:cancel_reason.clone().unwrap_or_default(),onclick:move |_|on_cancel.call(cancel_key.clone()),"Cancel sequence"}
                        }
                        if paused { if let Some(text)=&resume_reason {p {class:"gs26-sequence-control-reason","data-control":"resume","Resume unavailable: {text}"}} }
                        else if let Some(text)=&pause_reason {p {class:"gs26-sequence-control-reason","data-control":"pause","Pause unavailable: {text}"}}
                        if let Some(text)=&cancel_reason {p {class:"gs26-sequence-control-reason","data-control":"cancel","Cancel unavailable: {text}"}}
                    }
                    p {class:"gs26-sequence-legend","Planned · Command accepted · Board reported · Measured · Unknown"}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::sequence_progress_model::{Model, Snapshot};
    use super::*;
    #[derive(Props, Clone, PartialEq)]
    struct FixtureProps {
        terminal: bool,
        minimized: bool,
        now: u64,
        summary: bool,
        unavailable: bool,
        transient: bool,
        board_meta: Option<bool>,
    }
    fn fixture(props: FixtureProps) -> Element {
        let mut snapshot:Snapshot=serde_json::from_value(serde_json::json!({"version":1,"key":{"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222","run_id":"33333333333333333333333333333333"},"revision":1,"kind":"nitrogen_test","outcome":"running","origin_client":"origin-a","title":"Nitrogen test","message":"Watching tank pressure","current_step":"raise-0","next_step":"hold-0","steps":[{"id":"raise-0","label":"Raise pressure","state":"watching","watch":[{"id":"tank-pt","label":"Tank pressure","target":50.0,"observed":42.0,"unit":"psi","age_ms":0,"valid_for_ms":2000,"evidence":"measured"}]},{"id":"hold-0","label":"Verify hold","state":"planned","watch":[]}],"observed_age_ms":0,"valid_for_ms":2000,"cancel_allowed":true})).unwrap();
        let mut board_display = super::super::sequence_progress_model::BoardDisplayMap::new();
        if let Some(connected) = props.board_meta {
            let watch = &mut snapshot.steps[0].watch[0];
            watch.id = "board-output-0".into();
            watch.observed = Some(Value::Boolean(true));
            watch.unit = None;
            watch.evidence = Evidence::BoardReported;
            board_display.insert(
                "VB".into(),
                super::super::sequence_progress_model::BoardDisplay {
                    connected,
                    known: connected,
                    boot: Some("0000000000000002".into()),
                    epoch: Some(3),
                    outputs: Some(1),
                    outputs_age_ms: Some(0),
                },
            );
        }
        if props.terminal {
            snapshot.outcome = Outcome::Cancelled;
            snapshot.cancel_allowed = false;
        }
        let mut model = Model::default();
        model.set_surface(true);
        if props.terminal {
            // Use the same explicit immutable-result opening path as Notifications.
            model.open_result(0, snapshot.clone(), 0).unwrap();
        } else {
            model.accept(0, snapshot.clone(), 0, 0).unwrap();
        }
        if props.minimized {
            model.minimize(&snapshot.key);
        }
        let mut panel = model.panels()[0].clone();
        if props.summary {
            // This component-only case checks compact/loading presentation;
            // the real notification opener still requires complete result details.
            panel.snapshot = panel.snapshot.compact_result();
        }
        let output_display = if props.board_meta == Some(true) {
            [(0, true)].into_iter().collect()
        } else {
            Default::default()
        };
        rsx! {SequenceProgressView{panel,board_display,output_display,now_ms:props.now,unavailable:props.unavailable,transient_transport_error:props.transient,can_send_commands:true,cancel_api_version:Some(1),on_minimize:move|_|{},on_restore:move|_|{},on_cancel:move|_|{},on_dismiss:move|_|{}}}
    }
    fn html(terminal: bool, minimized: bool, now: u64) -> String {
        let mut dom = VirtualDom::new_with_props(
            fixture,
            FixtureProps {
                terminal,
                minimized,
                now,
                summary: false,
                unavailable: false,
                transient: false,
                board_meta: None,
            },
        );
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }
    #[test]
    fn transient_transport_failure_retains_valid_display_but_never_command_authority() {
        for (now, transient, known) in [(100, true, true), (100, false, false), (2001, true, false)]
        {
            let mut dom = VirtualDom::new_with_props(
                fixture,
                FixtureProps {
                    terminal: false,
                    minimized: false,
                    now,
                    summary: false,
                    unavailable: true,
                    transient,
                    board_meta: None,
                },
            );
            dom.rebuild_in_place();
            let html = dioxus::ssr::render(&dom);
            assert_eq!(html.contains("42.00"), known);
            assert!(html.contains("disabled"));
            assert_eq!(html.contains("Progress unknown"), !known);
        }
    }
    #[test]
    fn explicit_disconnect_overrides_fresh_producer_output_but_never_rewrites_history() {
        for terminal in [false, true] {
            for connected in [false, true] {
                let mut dom = VirtualDom::new_with_props(
                    fixture,
                    FixtureProps {
                        terminal,
                        minimized: false,
                        now: 100,
                        summary: false,
                        unavailable: false,
                        transient: false,
                        board_meta: Some(connected),
                    },
                );
                dom.rebuild_in_place();
                let html = dioxus::ssr::render(&dom);
                assert_eq!(
                    html.contains(">On</strong>"),
                    terminal || connected,
                    "terminal={terminal} connected={connected}"
                );
            }
        }
    }
    #[test]
    fn running_actual_view_has_minimize_and_guarded_cancel_but_no_dismiss() {
        let value = html(false, false, 100);
        assert!(value.contains("Minimize sequence progress"));
        assert!(!value.contains("Close sequence result"));
        assert!(value.contains("Cancel sequence"));
        assert!(value.contains("42.00"));
        assert!(value.contains("Watching"));
        assert!(value.contains("gs26-sequence-step-glyph"));
        assert!(value.contains("◌"));
        assert!(value.contains("○"));
        assert!(value.contains("aria-hidden=\"true\""));
    }
    #[test]
    fn stale_actual_view_blanks_readback_and_terminal_result_can_close() {
        let stale = html(false, false, 2001);
        assert!(stale.contains("Progress unknown"));
        assert!(!stale.contains("42.00"));
        assert!(
            !stale.contains("◌"),
            "stale work must not keep a working glyph"
        );
        assert!(stale.contains("?"));
        let result = html(true, false, 100);
        assert!(result.contains("Close sequence result"));
        assert!(!result.contains("Cancel sequence"));
        assert!(result.contains("not current live readings"));
        assert!(result.contains("42.00"));
    }
    #[test]
    fn compact_terminal_view_loads_details_without_synthetic_zero_steps() {
        let mut dom = VirtualDom::new_with_props(
            fixture,
            FixtureProps {
                terminal: true,
                minimized: false,
                now: 100,
                summary: true,
                unavailable: false,
                transient: false,
                board_meta: None,
            },
        );
        dom.rebuild_in_place();
        let html = dioxus::ssr::render(&dom);
        assert!(html.contains("Loading recorded step details"));
        assert!(!html.contains("0 of 0"));
        assert!(!html.contains("Cancel sequence"));
        assert!(html.contains("Warning: safely cancelled"));
    }
    #[test]
    fn minimized_actual_view_is_a_reopen_chip_with_no_cancel_or_dismiss() {
        let value = html(false, true, 100);
        assert!(value.contains("gs26-sequence-chip"));
        assert!(!value.contains("role=\"dialog\""));
        assert!(!value.contains("Cancel sequence"));
    }
    fn cold_fixture() -> Element {
        let snapshot:Snapshot=serde_json::from_value(serde_json::json!({"version":1,"key":{"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222","run_id":"33333333333333333333333333333333"},"revision":1,"kind":"cold_flow","outcome":"running","title":"Auto Cold Flow","message":"Isolated hold; explicit release required","current_step":"cold-await","next_step":"cold-release","steps":[{"id":"cold-await","label":"Await operator release","state":"watching","watch":[{"id":"pt","label":"Tank pressure","target":100.,"observed":100.,"unit":"psi","age_ms":0,"valid_for_ms":2000,"evidence":"measured"}]},{"id":"cold-release","label":"Open PILOT/main","state":"planned","watch":[]}],"observed_age_ms":0,"valid_for_ms":2000,"cancel_allowed":true,"cold_flow_release_allowed":true})).unwrap();
        let mut model = Model::default();
        model.set_surface(true);
        model.accept(0, snapshot, 0, 0).unwrap();
        let panel = model.panels()[0].clone();
        rsx! {SequenceProgressView{panel,now_ms:0,can_send_commands:true,cancel_api_version:Some(1),can_release_cold_flow:true,on_release_cold_flow:move|_|{},on_minimize:move|_|{},on_restore:move|_|{},on_cancel:move|_|{},on_dismiss:move|_|{}}}
    }
    #[test]
    fn actual_cold_flow_modal_has_explicit_release_and_safe_cancel_without_launch_control() {
        let mut dom = VirtualDom::new(cold_fixture);
        dom.rebuild_in_place();
        let value = dioxus::ssr::render(&dom);
        assert!(value.contains("Cold Flow · open PILOT/main"));
        assert!(value.contains("Cancel sequence"));
        assert!(value.contains("No igniter or launch countdown"));
        assert!(value.contains("100.00"));
        assert!(!value.contains("Close sequence result"));
        assert!(value.contains("Minimize sequence progress"));
    }
    fn controlled(outcome: &str, pause: bool, resume: bool) -> String {
        let snapshot:Snapshot=serde_json::from_value(serde_json::json!({"version":1,"key":{"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222","run_id":"33333333333333333333333333333333"},"revision":1,"kind":"nitrogen_test","outcome":outcome,"origin_client":null,"title":"Nitrogen test","message":"Watching","current_step":null,"next_step":null,"steps":[],"observed_age_ms":0,"valid_for_ms":2000,"cancel_allowed":true})).unwrap();
        let controls = super::super::sequence_progress_model::RunControls {
            key: snapshot.key.clone(),
            pause: super::super::sequence_progress_model::Gate { allowed: pause, reason: (!pause).then(|| "Already paused".into()) },
            resume: super::super::sequence_progress_model::Gate { allowed: resume, reason: (!resume).then(|| "Earlier valve commands are still being delivered".into()) },
            cancel: super::super::sequence_progress_model::Gate { allowed: false, reason: Some("Vehicle is not in a prelaunch state".into()) },
            hold_choice: None,
        };
        #[derive(Props, Clone, PartialEq)]
        struct P { snapshot: Snapshot, controls: super::super::sequence_progress_model::RunControls, pause: bool, resume: bool }
        fn view(p: P) -> Element {
            let mut model = Model::default();
            model.set_surface(true);
            model.accept(0, p.snapshot.clone(), 0, 0).unwrap();
            let panel = model.panels()[0].clone();
            rsx! {SequenceProgressView{panel,now_ms:0,can_send_commands:true,cancel_api_version:Some(1),controls:Some(p.controls),can_pause:p.pause,can_resume:p.resume,on_minimize:move|_|{},on_restore:move|_|{},on_cancel:move|_|{},on_dismiss:move|_|{}}}
        }
        let mut dom = VirtualDom::new_with_props(view, P { snapshot, controls, pause, resume });
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }
    #[test]
    fn running_view_offers_pause_and_paused_view_offers_resume_with_station_reasons() {
        let running = controlled("running", true, false);
        assert!(running.contains("Pause sequence"));
        assert!(!running.contains("Resume sequence"));
        assert!(!running.contains("Pause unavailable"));
        assert!(running.contains("Cancel unavailable: Vehicle is not in a prelaunch state"));
        let paused = controlled("paused", false, false);
        assert!(paused.contains("Resume sequence"));
        assert!(!paused.contains("Pause sequence"));
        assert!(paused.contains("Resume unavailable: Earlier valve commands are still being delivered"));
        let ready = controlled("paused", false, true);
        assert!(!ready.contains("Resume unavailable"));
    }

    fn held(passed: bool, cold: bool, cold_reason: Option<&str>) -> String {
        let snapshot:Snapshot=serde_json::from_value(serde_json::json!({"version":1,"key":{"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222","run_id":"33333333333333333333333333333333"},"revision":1,"kind":"nitrogen_test","outcome":"paused","origin_client":null,"title":"Nitrogen test","message":"Holding","current_step":null,"next_step":null,"steps":[],"observed_age_ms":0,"valid_for_ms":2000,"cancel_allowed":true})).unwrap();
        let controls: super::super::sequence_progress_model::RunControls = serde_json::from_value(serde_json::json!({
            "key": snapshot.key.clone(),
            "pause": {"allowed": false, "reason": "Already paused"},
            "resume": {"allowed": false, "reason": "Resume is available once the test is paused"},
            "cancel": {"allowed": true},
            "hold_choice": {
                "message": if passed {"Nitrogen test passed: holding top-step pressure. Choose Cold flow or Dump."} else {"Nitrogen test failed: leak. Holding pressure for leak search. Dump when ready."},
                "passed": passed,
                "cold_flow": {"allowed": cold, "reason": cold_reason},
                "dump": {"allowed": true}
            }
        })).unwrap();
        #[derive(Props, Clone, PartialEq)]
        struct P { snapshot: Snapshot, controls: super::super::sequence_progress_model::RunControls }
        fn view(p: P) -> Element {
            let mut model = Model::default();
            model.set_surface(true);
            model.accept(0, p.snapshot.clone(), 0, 0).unwrap();
            let panel = model.panels()[0].clone();
            rsx! {SequenceProgressView{panel,now_ms:0,can_send_commands:true,cancel_api_version:Some(1),controls:Some(p.controls),on_minimize:move|_|{},on_restore:move|_|{},on_cancel:move|_|{},on_dismiss:move|_|{}}}
        }
        let mut dom = VirtualDom::new_with_props(view, P { snapshot, controls });
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }
    #[test]
    fn nitrogen_hold_offers_cold_flow_and_dump_with_station_reasons() {
        let passed = held(true, true, None);
        assert!(passed.contains("Choose Cold flow or Dump"));
        assert!(passed.contains("data-hold-choice=\"cold-flow\""));
        assert!(passed.contains("data-hold-choice=\"dump\""));
        assert!(passed.contains("never PILOT"));
        assert!(passed.contains("Cold flow · held pressure"));
        assert!(passed.contains("not the standalone Auto Cold Flow target"));
        assert!(passed.contains("stays open until the tank is empty"));
        assert!(passed.contains("closes this window"));
        assert!(!passed.contains("Cold flow unavailable"));
        let failed = held(false, false, Some("Another ground sequence owns the valves"));
        assert!(failed.contains("Holding pressure for leak search. Dump when ready."));
        assert!(failed.contains("data-hold-verdict=\"failed\""));
        assert!(failed.contains("Cold flow unavailable: Another ground sequence owns the valves"));
        assert!(!failed.contains("Dump unavailable"));
    }
    #[test]
    fn older_station_controls_without_a_hold_choice_still_parse() {
        let controls: super::super::sequence_progress_model::RunControls = serde_json::from_value(serde_json::json!({
            "key": {"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222","run_id":"33333333333333333333333333333333"},
            "pause": {"allowed": true}, "resume": {"allowed": false}, "cancel": {"allowed": true}
        })).unwrap();
        assert!(controls.hold_choice.is_none());
        assert!(!controlled("running", true, false).contains("data-hold-choice"));
    }
}
