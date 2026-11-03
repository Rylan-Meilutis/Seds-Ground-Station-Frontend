use super::{
    alert_conditions::{RowTone, Severity},
    format_timestamp_ms_clock,
    prelude::*,
};
#[component]
pub(super) fn ConditionRows(severity: Severity, theme: ThemeConfig) -> Element {
    let view = super::ALERT_CONDITION_VIEW.read();
    let board_abort = super::BOARD_ABORT_SIGNAL.read().active_errors.clone();
    let records = view
        .snapshot()
        .map(|snapshot| {
            snapshot
                .records
                .iter()
                .filter(|record| {
                    record.severity == severity
                        && !(record.state == super::alert_conditions::ConditionState::Active
                            && board_abort.iter().any(|error| {
                                error.source == record.key && error.board == record.source
                            }))
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let summary = view.summary(false, false);
    rsx! {
        if severity==Severity::Warning && !summary.snapshot_current {
            div {class:"gs26-condition-status-unconfirmed",role:"status",style:"border:1px solid {theme.warning_border};color:{theme.warning_text};padding:10px;border-radius:10px;",
                "Current condition status is not confirmed yet. Acknowledging events does not confirm recovery."
            }
        }
        if severity==Severity::Error && view.typed_history_unconfirmed() {
            div {class:"gs26-condition-history-unconfirmed",role:"status",style:"color:{theme.text_muted};padding:10px;",
                "Condition history is not confirmed yet. No complete current status is available."
            }
        }
        for record in records {
            {let tone=record.tone();let resolved=tone==RowTone::Resolved;
            let (border,background,color)=match tone {
                RowTone::Resolved=>(theme.border.clone(),theme.panel_background.clone(),theme.text_secondary.clone()),
                RowTone::Error=>(theme.error_border.clone(),theme.error_background.clone(),theme.error_text.clone()),
                RowTone::Warning|RowTone::Unknown=>(theme.warning_border.clone(),theme.warning_background.clone(),theme.warning_text.clone()),
            };
            rsx! {div {key:"{record.occurrence_id}",class:"gs26-condition-row","data-condition-key":"{record.key}","data-occurrence-id":"{record.occurrence_id}","data-condition-state":record.state_label(),
                role:if resolved {"status"}else{"alert"},
                style:"border:1px solid {border};background:{background};color:{color};padding:10px;border-radius:10px;",
                strong {"{record.state_label()} · {record.source}"}
                div {"{record.message}"}
                small {"Raised {format_timestamp_ms_clock(record.raised_at_ms)}"}
                if let Some(at)=record.resolved_at_ms {small {style:"display:block;","Resolved {format_timestamp_ms_clock(at)}"}}
            }}}
        }
    }
}
