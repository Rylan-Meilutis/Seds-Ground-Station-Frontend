//! Navigation preferences and the dashboard-owned input-selection workflow.
use dioxus::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum StationSection {
    Mission,
    Loadcells,
    Sequences,
    Network,
    Administration,
}
impl StationSection {
    pub(super) fn parse(value: &str) -> Self {
        match value {
            "loadcells" => Self::Loadcells,
            "sequences" => Self::Sequences,
            "network" => Self::Network,
            "administration" => Self::Administration,
            _ => Self::Mission,
        }
    }
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Mission => "mission",
            Self::Loadcells => "loadcells",
            Self::Sequences => "sequences",
            Self::Network => "network",
            Self::Administration => "administration",
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Mission => "Mission",
            Self::Loadcells => "Loadcells",
            Self::Sequences => "Sequences & limits",
            Self::Network => "Network & clocks",
            Self::Administration => "Administration",
        }
    }
}
pub(super) fn normalize_scope(value: &str) -> &'static str {
    if value == "station" {
        "station"
    } else {
        "client"
    }
}
pub(super) static SCOPE: GlobalSignal<String> = Signal::global(|| {
    normalize_scope(&super::persist::get_string("settings-scope").unwrap_or_default()).into()
});
pub(super) static SECTION: GlobalSignal<StationSection> = Signal::global(|| {
    StationSection::parse(
        &super::persist::get_string("station-settings-section").unwrap_or_default(),
    )
});
pub(super) static CALIBRATION_REQUEST: GlobalSignal<u64> = Signal::global(|| 0);
pub(super) fn select_scope(scope: &str) {
    let scope = normalize_scope(scope);
    super::persist::set_string("settings-scope", scope);
    *SCOPE.write() = scope.into();
}
pub(super) fn select_section(section: StationSection) {
    super::persist::set_string("station-settings-section", section.key());
    *SECTION.write() = section;
}
/// Open the procedure page without changing remembered settings preferences.
pub(super) fn open_calibration() {
    let request = (*CALIBRATION_REQUEST.peek()).saturating_add(1);
    *CALIBRATION_REQUEST.write() = request;
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) enum InputSelectionStatus {
    #[default]
    Ready,
    Pending(String),
    Confirmed(String),
    Unconfirmed {
        requested: String,
        detail: String,
    },
    Retired(String),
}
impl InputSelectionStatus {
    pub(super) fn captures_blocked(&self) -> bool {
        !matches!(self, Self::Ready | Self::Confirmed(_))
    }
    pub(super) fn pending(&self) -> bool {
        matches!(self, Self::Pending(_))
    }
}
#[derive(Clone, Copy, PartialEq)]
pub(super) struct CalibrationContext {
    pub(super) input_epoch: Signal<u64>,
    pub(super) selection: Signal<InputSelectionStatus>,
    pub(super) locked: Signal<bool>,
    pub(super) request: EventHandler<String>,
    retained_owner: Signal<Option<std::rc::Rc<super::recording_download::ExportOwner>>>,
}
impl CalibrationContext {
    pub(super) fn owner_current(self) -> bool {
        self.retained_owner
            .read()
            .as_ref()
            .is_none_or(|owner| owner.is_current())
    }
    pub(super) fn captures_blocked(self) -> bool {
        self.selection.read().captures_blocked() || !self.owner_current()
    }
    /// A fresh read can replace a KNOWN prior selection, never acknowledge an
    /// uncertain or pending write. Called only by explicit page Reload.
    pub(super) fn can_retry_read(self) -> bool {
        matches!(
            &*self.selection.peek(),
            InputSelectionStatus::Ready | InputSelectionStatus::Confirmed(_)
        )
    }
    pub(super) fn prepare_read_retry(mut self) -> bool {
        if !self.can_retry_read() {
            return false;
        }
        self.retained_owner.set(None);
        self.selection.set(InputSelectionStatus::Ready);
        true
    }
}
/// Local editor lifetime only; this is not backend or command authority.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct CalibrationEditorOwner {
    input_epoch: Signal<u64>,
    pub(super) revision: Signal<u64>,
    pub(super) retired: Signal<Option<u64>>,
}
pub(super) fn use_calibration_editor_owner(input_epoch: Signal<u64>) -> CalibrationEditorOwner {
    CalibrationEditorOwner {
        input_epoch,
        revision: use_signal(|| 0),
        retired: use_signal(|| None),
    }
}
impl CalibrationEditorOwner {
    pub(super) fn is_retired(self) -> bool {
        *self.retired.read() == Some(*self.input_epoch.read())
    }
    pub(super) fn token(self) -> (u64, u64) {
        (*self.input_epoch.read(), *self.revision.read())
    }
    pub(super) fn retire(mut self, original: (u64, u64)) -> bool {
        let current = (*self.input_epoch.peek(), *self.revision.peek());
        if original != current || current.0 == u64::MAX || current.1 == u64::MAX {
            return false;
        }
        self.retired.set(Some(original.0));
        true
    }
    pub(super) fn invalidate(mut self) -> bool {
        let next = self.revision.peek().saturating_add(1);
        self.revision.set(next);
        next != u64::MAX
    }
    pub(super) fn reload(mut self) -> bool {
        if !self.invalidate() {
            return false;
        }
        self.retired.set(None);
        true
    }
}
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "calibration_editor_owner_tests.rs"]
mod calibration_editor_owner_tests;

struct InputSelectionRequest {
    requested: String,
    epoch: u64,
    owner: std::rc::Rc<super::recording_download::ExportOwner>,
}
pub(super) fn known_input(value: &str) -> bool {
    matches!(value, "amp2" | "p6_differential")
}
/// One parent-owned operation, captured only when the operator acts. Closing
/// Settings does not destroy its owner; changing station/auth/session still does.
pub(super) fn use_calibration_context(abort_only: Signal<bool>) -> CalibrationContext {
    use futures_util::StreamExt;
    use std::sync::atomic::Ordering;
    let alive = super::recording_download::use_export_lifetime();
    let mut input_epoch = use_signal(|| 0_u64);
    let mut selection = use_signal(InputSelectionStatus::default);
    let locked = use_signal(|| false);
    // The result keeps its ORIGINAL owner too: a completed unknown/confirmed
    // operation must never become authority for a replacement station/login.
    let mut retained_owner =
        use_signal(|| None::<std::rc::Rc<super::recording_download::ExportOwner>>);
    let operation_alive = alive.clone();
    let operations = use_coroutine(move |mut rx: UnboundedReceiver<InputSelectionRequest>| {
        let alive = operation_alive.clone();
        async move {
            while let Some(operation) = rx.next().await {
                if !alive.load(Ordering::Acquire) {
                    return;
                }
                if *input_epoch.peek() != operation.epoch {
                    continue;
                }
                let body = serde_json::json!({"input":operation.requested});
                let result =
                    super::station_settings_request::post_captured::<_, serde_json::Value>(
                        "/api/calibration/kg50_input",
                        &body,
                        Ok(operation.owner.clone()),
                    )
                    .await;
                if !alive.load(Ordering::Acquire) {
                    return;
                }
                if *input_epoch.peek() != operation.epoch {
                    continue;
                }
                if !operation.owner.is_current() {
                    // This exact original epoch has settled; a fresh station
                    // confirmation need not rediscover this already-retired Rc.
                    retained_owner.set(None);
                    selection.set(InputSelectionStatus::Retired(
                        "The station, account or connection changed. Confirm the desired input for this station before calibrating.".into()));
                    continue;
                }
                match result {
                    Ok(saved) if saved.get("kg50_input").and_then(|v|v.as_str()) == Some(operation.requested.as_str()) =>
                        selection.set(InputSelectionStatus::Confirmed(operation.requested)),
                    Ok(_) => selection.set(InputSelectionStatus::Unconfirmed {
                        requested:operation.requested,
                        detail:"The station did not confirm the requested input. Confirm that same selection again before calibrating.".into(),
                    }),
                    Err(error) => selection.set(InputSelectionStatus::Unconfirmed {
                        requested:operation.requested,
                        detail:format!("Input selection was not confirmed: {error}. Confirm that same selection again before calibrating."),
                    }),
                }
            }
        }
    });
    let request = use_callback(move |requested: String| {
        if !known_input(&requested)
            || !crate::auth::can_edit_calibration()
            || *abort_only.peek()
            || *locked.peek()
            || *input_epoch.peek() == u64::MAX
        {
            return;
        }
        // Do not enqueue a replacement while the original bounded await is live.
        if selection.peek().pending() {
            return;
        }
        let previous_owner_retired = retained_owner
            .peek()
            .as_ref()
            .is_some_and(|owner| !owner.is_current());
        if previous_owner_retired {
            retained_owner.set(None);
            selection.set(InputSelectionStatus::Retired(
                "The previous input operation belongs to a retired station or account. Confirm this station's current input with a new explicit action.".into()));
            return;
        }
        match &*selection.peek() {
            InputSelectionStatus::Pending(_) => return,
            InputSelectionStatus::Unconfirmed {
                requested: previous,
                ..
            } if previous != &requested => return,
            _ => {}
        }
        let request_alive = alive.clone();
        let owner = match super::recording_download::ExportOwner::capture_selected(move || {
            request_alive.load(Ordering::Acquire)
        }) {
            Ok(owner) => owner,
            Err(_) => {
                selection.set(InputSelectionStatus::Retired(
                    "The input request has no current station/account owner. Reconnect, then confirm the desired input.".into()));
                return;
            }
        };
        let epoch = next_input_epoch(*input_epoch.peek());
        if epoch == u64::MAX {
            return;
        }
        retained_owner.set(Some(owner.clone()));
        input_epoch.set(epoch);
        selection.set(InputSelectionStatus::Pending(requested.clone()));
        if operations
            .tx()
            .unbounded_send(InputSelectionRequest {
                requested,
                epoch,
                owner,
            })
            .is_err()
        {
            selection.set(InputSelectionStatus::Retired("Input request worker is unavailable. Reload this view before confirming the input.".into()));
        }
    });
    CalibrationContext {
        input_epoch,
        selection,
        locked,
        request,
        retained_owner,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_preferences_fall_back_without_new_configuration() {
        assert_eq!(normalize_scope("unknown"), "client");
        assert_eq!(normalize_scope("station"), "station");
        assert_eq!(StationSection::parse("unknown"), StationSection::Mission);
        for section in [
            StationSection::Mission,
            StationSection::Loadcells,
            StationSection::Sequences,
            StationSection::Network,
            StationSection::Administration,
        ] {
            assert_eq!(StationSection::parse(section.key()), section);
        }
    }
}

/// A local view epoch never wraps into an older calibration selection.
pub(super) fn next_input_epoch(current: u64) -> u64 {
    current.saturating_add(1)
}
#[cfg(test)]
mod input_epoch_tests {
    #[test]
    fn exhaustion_never_reuses_an_old_selection() {
        assert_eq!(super::next_input_epoch(0), 1);
        assert_eq!(super::next_input_epoch(u64::MAX - 1), u64::MAX);
        assert_eq!(super::next_input_epoch(u64::MAX), u64::MAX);
    }
}
