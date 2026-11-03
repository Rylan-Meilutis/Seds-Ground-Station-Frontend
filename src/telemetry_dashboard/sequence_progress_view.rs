use super::sequence_progress_model::{
    Evidence, Kind, Outcome, Panel, RunKey, StepState, Value, Watch,
};
use dioxus::prelude::*;

fn outcome_label(value: Outcome) -> &'static str {
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
    on_minimize: EventHandler<RunKey>,
    on_restore: EventHandler<RunKey>,
    on_cancel: EventHandler<RunKey>,
    on_dismiss: EventHandler<RunKey>,
) -> Element {
    if !panel.visible {
        return rsx! {};
    }
    let key = panel.snapshot.key.clone();
    let restore_key = key.clone();
    let minimize_key = key.clone();
    let cancel_key = key.clone();
    let dismiss_key = key.clone();
    let keyboard_key = key.clone();
    let terminal = panel.snapshot.outcome.terminal();
    let fresh = panel.fresh(now_ms) && !unavailable;
    let status = if !terminal && !fresh {
        "Progress unknown"
    } else {
        outcome_label(panel.snapshot.outcome)
    };
    let can_cancel =
        !unavailable && panel.can_cancel(now_ms, can_send_commands, cancel_api_version);
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
                div {class:"gs26-sequence-summary",role:"status",aria_live:"polite","data-outcome":format!("{:?}",panel.snapshot.outcome),
                    strong {"{status}"}if panel.snapshot.details_complete {span {"{passed} of {count} steps passed"}}
                }
                if !terminal&&!fresh {p {class:"gs26-sequence-unknown","Fresh station progress is unavailable. Confirmation is unknown."}}
                p {class:"gs26-sequence-message","{panel.snapshot.message}"}
                if !terminal {
                    section {class:"gs26-sequence-current",h3 {"Current"}
                        if let Some(step)=current {p {"{step.label}"}}else{p {"Waiting for station progress"}}
                        if let Some(step)=current {
                            if !step.watch.is_empty(){h4 {"Watching"}}
                            for watch in &step.watch {
                                {let observed=if fresh{panel.observed_value(watch,now_ms)}else{None};let reading=value_label(observed);let target=value_label(watch.target.as_ref());let evidence=if observed.is_some(){evidence_label(watch.evidence)}else{"Unknown"};
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
                ol {class:"gs26-sequence-steps","aria-label":"Sequence steps",
                    for step in &panel.snapshot.steps {
                        {let state=if !terminal&&!fresh&&matches!(step.state,StepState::Current|StepState::Watching){StepState::Unknown}else{step.state};let label=step_label(state);
                        rsx!{li {key:"{step.id}","data-step-state":label,
                            if terminal && !step.watch.is_empty() {
                                details {summary {span {"{step.label}"}small {"{label}"}}
                                    for watch in &step.watch {
                                        div {class:"gs26-sequence-watch",key:"{watch.id}",
                                            span {"{watch.label}"}strong {"{value_label(recorded_value(watch))}" if let Some(unit)=&watch.unit{" {unit}"}}
                                            small {"Last recorded while watching"}small {"{evidence_label(watch.evidence)}"}
                                        }
                                    }
                                }
                            } else {span {"{step.label}"}small {"{label}"}}
                        }}}
                    }
                }
                footer {
                    if panel.cancel_pending {p {role:"status","Cancellation requested. Waiting for the station's safe-cancel result."}}
                    if !terminal {button {class:"gs26-sequence-cancel",r#type:"button",disabled:!can_cancel,onclick:move |_|on_cancel.call(cancel_key.clone()),"Cancel sequence"}}
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
    }
    fn fixture(props: FixtureProps) -> Element {
        let mut snapshot:Snapshot=serde_json::from_value(serde_json::json!({"version":1,"key":{"backend_instance":"11111111111111111111111111111111","reset_epoch":"2222222222222222","run_id":"33333333333333333333333333333333"},"revision":1,"kind":"nitrogen_test","outcome":"running","origin_client":"origin-a","title":"Nitrogen test","message":"Watching tank pressure","current_step":"raise-0","next_step":"hold-0","steps":[{"id":"raise-0","label":"Raise pressure","state":"watching","watch":[{"id":"tank-pt","label":"Tank pressure","target":50.0,"observed":42.0,"unit":"psi","age_ms":0,"valid_for_ms":2000,"evidence":"measured"}]},{"id":"hold-0","label":"Verify hold","state":"planned","watch":[]}],"observed_age_ms":0,"valid_for_ms":2000,"cancel_allowed":true})).unwrap();
        if props.terminal {
            snapshot.outcome = Outcome::Cancelled;
            snapshot.cancel_allowed = false;
        }
        if props.summary {
            snapshot = snapshot.compact_result();
        }
        let mut model = Model::default();
        model.set_surface(true);
        model.accept(0, snapshot.clone(), 0, 0).unwrap();
        if props.minimized {
            model.minimize(&snapshot.key);
        }
        let panel = model.panels()[0].clone();
        rsx! {SequenceProgressView{panel,now_ms:props.now,can_send_commands:true,cancel_api_version:Some(1),on_minimize:move|_|{},on_restore:move|_|{},on_cancel:move|_|{},on_dismiss:move|_|{}}}
    }
    fn html(terminal: bool, minimized: bool, now: u64) -> String {
        let mut dom = VirtualDom::new_with_props(
            fixture,
            FixtureProps {
                terminal,
                minimized,
                now,
                summary: false,
            },
        );
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }
    #[test]
    fn running_actual_view_has_minimize_and_guarded_cancel_but_no_dismiss() {
        let value = html(false, false, 100);
        assert!(value.contains("Minimize sequence progress"));
        assert!(!value.contains("Close sequence result"));
        assert!(value.contains("Cancel sequence"));
        assert!(value.contains("42.00"));
        assert!(value.contains("Watching"));
    }
    #[test]
    fn stale_actual_view_blanks_readback_and_terminal_result_can_close() {
        let stale = html(false, false, 2001);
        assert!(stale.contains("Progress unknown"));
        assert!(!stale.contains("42.00"));
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
}
