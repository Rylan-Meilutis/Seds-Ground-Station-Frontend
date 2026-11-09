use super::{AlertMsg, format_timestamp_ms_clock, prelude::*};

#[component]
pub fn ErrorsTab(
    errors: Signal<Vec<AlertMsg>>,
    ack_timestamp_ms: i64,
    theme: ThemeConfig,
    on_ack: EventHandler<()>,
    on_history_cleared: EventHandler<i64>,
) -> Element {
    let active = super::BOARD_ABORT_SIGNAL.read().active_errors.clone();
    let history_error = super::BOARD_ABORT_SIGNAL
        .read()
        .history_error_message()
        .map(str::to_owned);
    let has_errors = !errors.read().is_empty();
    let condition_view=super::ALERT_CONDITION_VIEW.read().clone();
    let ack_button_style = format!(
        "padding:0.25rem 0.65rem; border-radius:999px; border:1px solid {}; background:{}; color:{}; font-size:0.75rem; cursor:{}; opacity:{};",
        theme.button_border,
        theme.button_background,
        theme.button_text,
        if has_errors { "pointer" } else { "default" },
        if has_errors { "1" } else { "0.45" },
    );

    rsx! {
        div { class:"gs26-message-history", style: "padding:4px 0 0 0; color:{theme.text_primary}; height:100%; box-sizing:border-box;",
            div { style: "display:flex; align-items:center; gap:10px; margin:0 0 8px 0;",
                h2 { style: "margin:0; color:{theme.text_primary}; flex:1;", "{translate_text(\"Errors\")}" }
                button {
                    disabled: !has_errors,
                    style: "{ack_button_style}",
                    onclick: move |_| {
                        on_ack.call(());
                    },
                    "{translate_text(\"Acknowledge\")}"
                }
            }
            div { style: "display:flex; justify-content:flex-end; margin:0 0 8px 0;",
                ClearHistoryControl { theme: theme.clone(), on_cleared: on_history_cleared }
            }

            div { style: "display:flex; flex-direction:column; gap:6px;",
                if let Some(message)=history_error.as_deref() {
                    div {role:"alert",class:"gs26-active-history-error",
                        style:"border:1px solid {theme.error_border}; background:{theme.error_background}; color:{theme.error_text}; padding:10px; border-radius:10px;",
                        strong {"Active: Abort history unavailable"}
                        p {"{message}"}
                        small {"A current station report must confirm that saved history is healthy. Acknowledging messages does not clear this error."}
                    }
                }
                super::alert_condition_panel::ConditionRows {severity:super::alert_conditions::Severity::Error,theme:theme.clone()}
                for error in active.iter() {
                    div { key: "{error.source}", role: "alert",
                        style: "border:1px solid {theme.error_border}; background:{theme.error_background}; color:{theme.error_text}; padding:10px; border-radius:10px;",
                        strong { "Active: {error.board_label}" }
                        p { "{error.message}" }
                        small { "Clears only after a fresh confirmed report from this board. Acknowledging history does not clear it." }
                    }
                }
                for e in errors.read().iter().filter(|event|!matches!(condition_view.event_projection(event.condition.as_ref(),event.timestamp_ms<=ack_timestamp_ms),super::alert_conditions::EventProjection::Represented)) {
                    {
                        let acknowledged = e.timestamp_ms <= ack_timestamp_ms;
                        let projection=condition_view.event_projection(e.condition.as_ref(),acknowledged);
                        let row_style = if projection.history_gray(acknowledged) {
                            format!(
                                "border:1px solid {}; background:{}; color:{}; padding:8px 10px 10px 10px; border-radius:10px;",
                                theme.border, theme.panel_background, theme.text_secondary
                            )
                        } else {
                            format!(
                                "border:1px solid {}; background:{}; color:{}; padding:8px 10px 10px 10px; border-radius:10px;",
                                theme.error_border, theme.error_background, theme.error_text
                            )
                        };
                        rsx! {
                            div {
                                style: "{row_style}",
                                div { style: "font-size:12px; opacity:0.85; line-height:1.25;", "{format_timestamp_ms_clock(e.timestamp_ms)}" }
                                small {"{projection.event_label(acknowledged)}"}
                                div { style: "font-size:14px; line-height:1.3; padding-bottom:1px;", "{translate_text(&e.message)}" }
                            }
                        }
                    }
                }
                if errors.read().is_empty() && active.is_empty() && history_error.is_none() && condition_view.may_show_empty_history(super::alert_conditions::Severity::Error) {
                    div { style: "color:{theme.text_muted};", "{translate_text(\"No errors.\")}" }
                }
            }
        }
    }
}

/// Header Errors badge. The number is active (unresolved) errors only;
/// resolved and acknowledged history never inflates it.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ErrorBadge {
    pub label: String,
    pub visible: bool,
    /// Resolved/acknowledged history the operator has not dismissed yet.
    pub dismissible: usize,
}

fn capped(count: usize) -> String {
    if count >= 500 { "500+".into() } else { count.to_string() }
}

pub(super) fn error_badge(active: usize, unacknowledged_events: usize, undismissed_history: usize) -> ErrorBadge {
    let label = if unacknowledged_events > 0 {
        format!("{} · {} new", capped(active), capped(unacknowledged_events))
    } else {
        capped(active)
    };
    ErrorBadge {
        label,
        visible: active > 0 || unacknowledged_events > 0 || undismissed_history > 0,
        dismissible: undismissed_history,
    }
}

/// Resolved error conditions plus already-reviewed error events newer than
/// the operator's dismissal. Active and unknown conditions never count here.
pub(super) fn undismissed_error_history(
    records: &[super::alert_conditions::Record],
    reviewed_event_times: impl Iterator<Item = i64>,
    dismissed_through_ms: i64,
) -> usize {
    records
        .iter()
        .filter(|record| record.severity == super::alert_conditions::Severity::Error)
        .filter_map(|record| record.resolved_at_ms)
        .filter(|at| *at > dismissed_through_ms)
        .count()
        + reviewed_event_times
            .filter(|at| *at > dismissed_through_ms)
            .count()
}

/// The newest resolved/reviewed time; dismissing up to it leaves later ones visible.
pub(super) fn dismiss_cutoff(
    records: &[super::alert_conditions::Record],
    reviewed_event_times: impl Iterator<Item = i64>,
) -> i64 {
    records
        .iter()
        .filter(|record| record.severity == super::alert_conditions::Severity::Error)
        .filter_map(|record| record.resolved_at_ms)
        .chain(reviewed_event_times)
        .max()
        .unwrap_or(0)
}

/// Local to this viewer: hides resolved history from the header badge only.
#[component]
pub(super) fn DismissResolvedErrorsButton(count: usize, cutoff_ms: i64, mut dismissed_error_ts: Signal<i64>) -> Element {
    rsx! {
        button {
            class: "gs26-status-count",
            title: "Hide resolved errors from this badge. They stay in the Errors tab until history is cleared.",
            onclick: move |_| {
                let next = (*dismissed_error_ts.read()).max(cutoff_ms);
                dismissed_error_ts.set(next);
            },
            "Dismiss {count} resolved"
        }
    }
}

#[derive(serde::Serialize)]
struct ClearHistoryRequest {
    confirm: bool,
}

#[derive(serde::Deserialize)]
struct ClearHistoryReply {
    cleared_through_ms: i64,
}

/// Station-wide and permission-gated. Active errors and Abort latches stay.
#[component]
fn ClearHistoryControl(theme: ThemeConfig, on_cleared: EventHandler<i64>) -> Element {
    let mut confirming = use_signal(|| false);
    let mut pending = use_signal(|| false);
    let mut error = use_signal(String::new);
    let status = crate::auth::current_status();
    let allowed = crate::auth::can_view_actions() && status.authenticated && !status.anonymous;
    if !allowed {
        return rsx! {};
    }
    let button_style = format!(
        "padding:0.25rem 0.65rem; border-radius:999px; border:1px solid {}; background:{}; color:{}; font-size:0.75rem; cursor:pointer;",
        theme.button_border, theme.button_background, theme.button_text,
    );
    rsx! {
        if *confirming.read() {
            div { role: "alertdialog", "aria-label": "Clear alert history",
                style: "display:flex; flex-direction:column; gap:6px; padding:10px; border:1px solid {theme.warning_border}; background:{theme.warning_background}; color:{theme.warning_text}; border-radius:10px; margin:0 0 8px 0;",
                strong { "Clear error and warning history for every station viewer?" }
                small { "Resolved conditions and older error and warning events are removed. Active errors and board Abort latches stay. Your name is recorded." }
                div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                    button {
                        style: "{button_style}",
                        disabled: *pending.read(),
                        onclick: move |_| {
                            if *pending.read() { return; }
                            let owner = match super::recording_download::ExportOwner::capture_ordinary_selected(|| true) {
                                Ok(owner) => owner,
                                Err(reason) => { error.set(reason); return; }
                            };
                            let request = super::station_settings_request::post_captured::<_, ClearHistoryReply>(
                                "/api/alerts/history/clear", &ClearHistoryRequest { confirm: true }, Ok(owner.clone()));
                            pending.set(true);
                            error.set(String::new());
                            spawn(async move {
                                let result = request.await;
                                if !owner.is_current() { return; }
                                match result {
                                    Ok(reply) => {
                                        on_cleared.call(reply.cleared_through_ms);
                                        confirming.set(false);
                                    }
                                    Err(reason) => error.set(reason),
                                }
                                pending.set(false);
                            });
                        },
                        if *pending.read() { "Clearing…" } else { "Clear history" }
                    }
                    button { style: "{button_style}", disabled: *pending.read(), onclick: move |_| { confirming.set(false); error.set(String::new()); }, "Cancel" }
                }
                if !error.read().is_empty() { small { role: "alert", "Could not clear history: {error}" } }
            }
        } else {
            button { style: "{button_style}", onclick: move |_| confirming.set(true), "Clear history…" }
        }
    }
}

#[cfg(test)]
mod badge_tests {
    use super::*;
    use super::super::alert_conditions::{ConditionState, Record, Severity};

    fn record(state: ConditionState, resolved_at_ms: Option<i64>) -> Record {
        Record {
            key: "board_abort:FC".into(),
            occurrence_id: format!("epoch:{}", resolved_at_ms.unwrap_or(0)),
            authority: "epoch".into(),
            severity: Severity::Error,
            source: "FC".into(),
            message: "FC reports Abort".into(),
            state,
            raised_at_ms: 1,
            updated_at_ms: 2,
            resolved_at_ms,
        }
    }

    #[test]
    fn badge_counts_only_active_errors_never_resolved_history() {
        // Bench case: six board Aborts, all resolved, no active error.
        let records: Vec<Record> = (1..=6)
            .map(|at| record(ConditionState::Resolved, Some(100 + at)))
            .collect();
        let history = undismissed_error_history(&records, std::iter::empty(), 0);
        assert_eq!(history, 6);
        let badge = error_badge(0, 0, history);
        assert_eq!(badge.label, "0");
        assert!(badge.visible, "undismissed resolved history keeps the badge in view");
        assert_eq!(badge.dismissible, 6);
        assert_eq!(error_badge(2, 0, history).label, "2");
        assert_eq!(error_badge(1, 3, history).label, "1 · 3 new");
        assert_eq!(error_badge(600, 0, 0).label, "500+");
    }

    #[test]
    fn dismissing_resolved_history_hides_it_from_the_badge_until_a_new_resolution() {
        let mut records: Vec<Record> = (1..=6)
            .map(|at| record(ConditionState::Resolved, Some(100 + at)))
            .collect();
        records.push(record(ConditionState::Active, None));
        let reviewed_events = [90_i64, 95];
        let cutoff = dismiss_cutoff(&records, reviewed_events.iter().copied());
        assert_eq!(cutoff, 106);
        assert_eq!(undismissed_error_history(&records, reviewed_events.iter().copied(), cutoff), 0);
        let badge = error_badge(1, 0, 0);
        assert_eq!(badge.label, "1", "the active error is never dismissed");
        assert!(badge.visible);
        assert!(!error_badge(0, 0, 0).visible);
        records.push(record(ConditionState::Resolved, Some(200)));
        assert_eq!(undismissed_error_history(&records, reviewed_events.iter().copied(), cutoff), 1);
    }
}
