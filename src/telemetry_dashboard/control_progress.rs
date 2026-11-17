//! How an action button shows a timed physical operation the backend reports
//! for it (the QD retraction): pulsing with the time left while it runs, then
//! completed, failed or unconfirmed. Presentation only.
use super::{ControlProgress, ControlProgressState};

/// Pulse used while the operation runs, whether or not the button can be
/// pressed again. Defined in `blink::ACTION_BLINK_CSS`.
pub(super) const RUNNING_ANIMATION: &str = "animation:gs26-progress-pulse 900ms ease-in-out infinite;";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProgressView {
    /// Short badge text on the button.
    pub badge: &'static str,
    /// Second line under the button label.
    pub detail: String,
    /// Extra inline style (the running pulse), empty otherwise.
    pub animation: &'static str,
    /// Whether the button shows lit; `None` leaves the backend's readback.
    pub illuminated: Option<bool>,
    /// The `data-progress` attribute and `aria-busy`.
    pub state: &'static str,
    pub busy: bool,
}

fn seconds(ms: u32) -> u32 {
    ms.div_ceil(1000)
}

pub(super) fn view(progress: Option<&ControlProgress>) -> Option<ProgressView> {
    let progress = progress?;
    let message = progress.message.clone().unwrap_or_default();
    let make = |badge, detail: String, animation, illuminated, state, busy| {
        Some(ProgressView { badge, detail, animation, illuminated, state, busy })
    };
    match progress.state {
        ControlProgressState::Starting => make(
            "Sending",
            "Retraction requested; waiting for the actuator.".into(),
            RUNNING_ANIMATION,
            None,
            "starting",
            true,
        ),
        ControlProgressState::Running => {
            let left = progress.remaining_ms.map(seconds);
            let detail = match (progress.confirmed, left) {
                (true, Some(s)) => format!("Retracting: {s} s left"),
                (true, None) => "Retracting (actuator reports it running)".into(),
                (false, Some(s)) => format!("Retracting: about {s} s left (unconfirmed)"),
                (false, None) => "Retracting (unconfirmed)".into(),
            };
            let detail = if progress.confirmed || message.is_empty() {
                detail
            } else {
                format!("{detail}. {message}")
            };
            make(
                if progress.confirmed { "Retracting" } else { "Retracting?" },
                detail,
                RUNNING_ANIMATION,
                Some(true),
                "running",
                true,
            )
        }
        ControlProgressState::Completed => make(
            "Retracted",
            "Retraction completed".into(),
            "",
            Some(true),
            "completed",
            false,
        ),
        ControlProgressState::Failed => make(
            "Failed",
            if message.is_empty() { "Retraction failed".into() } else { message },
            "",
            Some(false),
            "failed",
            false,
        ),
        ControlProgressState::Unconfirmed => make(
            "Unconfirmed",
            if message.is_empty() { "Retraction not confirmed".into() } else { message },
            "",
            None,
            "unconfirmed",
            false,
        ),
        ControlProgressState::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(value: serde_json::Value) -> ControlProgress {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn running_retraction_pulses_and_counts_down() {
        let v = view(Some(&progress(serde_json::json!({"state":"running","confirmed":true,"remaining_ms":30_400})))).unwrap();
        assert_eq!(v.badge, "Retracting");
        assert_eq!(v.detail, "Retracting: 31 s left");
        assert_eq!(v.animation, RUNNING_ANIMATION);
        assert!(v.busy);
        assert_eq!(v.illuminated, Some(true));
    }

    #[test]
    fn unconfirmed_estimate_says_so() {
        let v = view(Some(&progress(serde_json::json!({
            "state":"running","confirmed":false,"remaining_ms":12_000,
            "message":"Retraction command sent; running, not confirmed by the actuator."})))).unwrap();
        assert_eq!(v.badge, "Retracting?");
        assert!(v.detail.starts_with("Retracting: about 12 s left (unconfirmed)"), "{}", v.detail);
        assert!(v.detail.contains("not confirmed by the actuator"));
        assert_eq!(v.animation, RUNNING_ANIMATION);
    }

    #[test]
    fn stepper_retraction_without_time_still_pulses() {
        let v = view(Some(&progress(serde_json::json!({"state":"running","confirmed":true})))).unwrap();
        assert_eq!(v.detail, "Retracting (actuator reports it running)");
        assert_eq!(v.animation, RUNNING_ANIMATION);
    }

    #[test]
    fn starting_pulses() {
        let v = view(Some(&progress(serde_json::json!({"state":"starting"})))).unwrap();
        assert_eq!((v.badge, v.state, v.busy), ("Sending", "starting", true));
        assert_eq!(v.animation, RUNNING_ANIMATION);
    }

    #[test]
    fn terminal_states_stop_pulsing() {
        let done = view(Some(&progress(serde_json::json!({"state":"completed","confirmed":true,"remaining_ms":0})))).unwrap();
        assert_eq!((done.badge, done.animation, done.illuminated, done.busy), ("Retracted", "", Some(true), false));
        let failed = view(Some(&progress(serde_json::json!({"state":"failed","message":"The actuator stopped the retraction before it completed."})))).unwrap();
        assert_eq!((failed.badge, failed.animation, failed.illuminated), ("Failed", "", Some(false)));
        assert!(failed.detail.contains("stopped"));
        let unconfirmed = view(Some(&progress(serde_json::json!({"state":"unconfirmed"})))).unwrap();
        assert_eq!((unconfirmed.badge, unconfirmed.animation, unconfirmed.illuminated), ("Unconfirmed", "", None));
    }

    #[test]
    fn no_or_unknown_progress_changes_nothing() {
        assert_eq!(view(None), None);
        assert_eq!(view(Some(&progress(serde_json::json!({"state":"paused_by_newer_backend"})))), None);
    }
}
