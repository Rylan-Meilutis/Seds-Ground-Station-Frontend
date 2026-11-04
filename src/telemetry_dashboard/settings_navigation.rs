//! UI navigation preferences only; this module never changes station settings.
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
            Self::Loadcells => "Loadcells & calibration",
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
pub(super) fn open_calibration() {
    select_scope("station");
    select_section(StationSection::Loadcells);
    let request = (*CALIBRATION_REQUEST.peek()).saturating_add(1);
    *CALIBRATION_REQUEST.write() = request;
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
