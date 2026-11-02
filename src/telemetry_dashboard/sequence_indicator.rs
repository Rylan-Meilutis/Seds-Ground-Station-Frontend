//! Sequence progress is an observation, independent of permission to press again.
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Progress {
    Unknown,
    Inactive,
    Running,
    Paused,
    Complete,
}

impl Progress {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Status unavailable",
            Self::Inactive => "",
            Self::Running => "In progress",
            Self::Paused => "Paused",
            Self::Complete => "Complete",
        }
    }
    pub fn illuminated(self) -> bool {
        matches!(self, Self::Running | Self::Paused | Self::Complete)
    }
}

pub(super) fn progress(
    command: &str,
    status: &Value,
    local_age_ms: u64,
    rrls: bool,
) -> Option<Progress> {
    if !matches!(
        command,
        "ValveSelfTest" | "NitrogenTest" | "StartFill" | "PauseFill" | "CancelFill" | "Turbocharge"
    ) {
        return None;
    }
    let primary = &status["primary_observation"];
    let fresh = local_age_ms <= 2_000
        && status["unavailable"].as_bool() != Some(true)
        && (!rrls
            || (primary["fresh"].as_bool() == Some(true)
                && primary["age_ms"]
                    .as_u64()
                    .is_some_and(|age| age <= 1_500 && age.saturating_add(local_age_ms) <= 2_000)));
    let Some(phase) = status["phase"].as_str().filter(|_| fresh) else {
        return Some(Progress::Unknown);
    };
    let state = match (command, phase) {
        (
            "ValveSelfTest",
            "self_setup" | "self_open" | "self_hold" | "self_close" | "self_finish",
        )
        | ("NitrogenTest", "nitrogen_setup" | "raising" | "settling" | "holding" | "dumping")
        | ("StartFill", "fill_setup" | "filling")
        | ("PauseFill", "pause_setup")
        | ("CancelFill", "cancel_setup")
        | ("Turbocharge", "turbocharge") => Progress::Running,
        ("PauseFill", "paused") => Progress::Paused,
        ("CancelFill", "cancelled") => Progress::Complete,
        ("NitrogenTest", _) if status["nitrogen_passed"].as_bool() == Some(true) => {
            Progress::Complete
        }
        ("ValveSelfTest", _) if status["qualification"]["self_test"].as_bool() == Some(true) => {
            Progress::Complete
        }
        ("StartFill", "paused")
            if status["qualification"]["fill_target"].as_bool() == Some(true) =>
        {
            Progress::Complete
        }
        (
            _,
            "idle" | "baseline_setup" | "baseline" | "self_setup" | "self_open" | "self_hold"
            | "self_close" | "self_finish" | "nitrogen_setup" | "raising" | "settling" | "holding"
            | "dumping" | "passed" | "fill_setup" | "filling" | "pause_setup" | "paused"
            | "cancel_setup" | "cancelled" | "fault" | "turbocharge",
        ) => Progress::Inactive,
        _ => Progress::Unknown,
    };
    Some(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn running_sequence_illuminates_without_a_button_press_or_output_claim() {
        for (command, phase) in [
            ("ValveSelfTest", "self_hold"),
            ("NitrogenTest", "holding"),
            ("StartFill", "filling"),
            ("PauseFill", "pause_setup"),
            ("CancelFill", "cancel_setup"),
            ("Turbocharge", "turbocharge"),
        ] {
            let state = progress(command, &json!({"phase":phase}), 0, false).unwrap();
            assert_eq!(state, Progress::Running);
            assert!(state.illuminated());
            assert_eq!(state.label(), "In progress");
        }
        assert_eq!(
            progress("Pilot", &json!({"phase":"self_open"}), 0, false),
            None
        );
    }
    #[test]
    fn baseline_does_not_guess_which_test_was_requested_and_override_is_not_a_pass() {
        for command in ["ValveSelfTest", "NitrogenTest"] {
            assert_eq!(
                progress(command, &json!({"phase":"baseline"}), 0, false),
                Some(Progress::Inactive)
            );
        }
        assert_eq!(
            progress(
                "NitrogenTest",
                &json!({"phase":"passed","nitrogen_overridden":true,"nitrogen_passed":false}),
                0,
                false
            ),
            Some(Progress::Inactive)
        );
    }
    #[test]
    fn stale_primary_or_lost_status_cannot_keep_a_running_indicator() {
        let mut status =
            json!({"phase":"filling","primary_observation":{"fresh":true,"age_ms":400}});
        assert_eq!(
            progress("StartFill", &status, 500, true),
            Some(Progress::Running)
        );
        assert_eq!(
            progress("StartFill", &status, 2_001, false),
            Some(Progress::Unknown)
        );
        status["primary_observation"]["fresh"] = json!(false);
        assert_eq!(
            progress("StartFill", &status, 0, true),
            Some(Progress::Unknown)
        );
        assert_eq!(
            progress("StartFill", &Value::Null, 0, false),
            Some(Progress::Unknown)
        );
    }
    #[test]
    fn pause_and_completion_follow_specific_server_evidence() {
        assert_eq!(
            progress("PauseFill", &json!({"phase":"paused"}), 0, false),
            Some(Progress::Paused)
        );
        assert_eq!(
            progress("StartFill", &json!({"phase":"paused"}), 0, false),
            Some(Progress::Inactive)
        );
        assert_eq!(
            progress(
                "StartFill",
                &json!({"phase":"paused","qualification":{"fill_target":true}}),
                0,
                false
            ),
            Some(Progress::Complete)
        );
        assert_eq!(
            progress(
                "ValveSelfTest",
                &json!({"phase":"idle","dry_self_test_confirmed":true}),
                0,
                false
            ),
            Some(Progress::Inactive)
        );
    }
}
