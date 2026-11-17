//! Actions-tab sections. The backend layout gives each action its section
//! (GroundStation `sequences::control_section`, beside the per-mode command
//! catalogs); this module only decides, per operating mode, which sections
//! lead, which start open, and remembers each operator's own open/closed
//! choices. Presentation only: it never enables, gates or sends a command.
use std::collections::{BTreeMap, BTreeSet};

pub use super::layout::ActionSection;

impl ActionSection {
    pub const ALL: [Self; 9] = [
        Self::Safety,
        Self::FillSystem,
        Self::Sequences,
        Self::FlightComputer,
        Self::AvBay,
        Self::Recovery,
        Self::DaqRecording,
        Self::Testing,
        Self::Other,
    ];

    pub fn of(section: Option<Self>) -> Self {
        section.unwrap_or(Self::Other)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Safety => "Safety / Abort",
            Self::FillSystem => "Fill system",
            Self::Sequences => "Sequences & launch",
            Self::FlightComputer => "Flight computer",
            Self::AvBay => "AV bay",
            Self::Recovery => "Recovery / reset",
            Self::DaqRecording => "DAQ / recording",
            Self::Testing => "Testing / HITL",
            Self::Other => "Other controls",
        }
    }

    /// Stable storage and DOM id.
    pub fn id(self) -> &'static str {
        match self {
            Self::Safety => "safety",
            Self::FillSystem => "fill_system",
            Self::Sequences => "sequences",
            Self::FlightComputer => "flight_computer",
            Self::AvBay => "av_bay",
            Self::Recovery => "recovery",
            Self::DaqRecording => "daq_recording",
            Self::Testing => "testing",
            Self::Other => "other",
        }
    }

    /// Safety (Abort and the abort/recovery status) is never collapsible.
    pub fn collapsible(self) -> bool {
        self != Self::Safety
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingMode {
    Normal,
    Hitl,
    TestFire,
    Testing,
}

impl OperatingMode {
    /// The station's effective mode; when it is unknown, the served layout's
    /// test-fire flag, else Normal.
    pub fn from_effective(effective_mode: Option<&str>, test_fire_layout: bool) -> Self {
        match effective_mode {
            Some("hitl") => Self::Hitl,
            Some("test_fire") => Self::TestFire,
            Some("testing") => Self::Testing,
            Some("normal") => Self::Normal,
            _ if test_fire_layout => Self::TestFire,
            _ => Self::Normal,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Hitl => "hitl",
            Self::TestFire => "test_fire",
            Self::Testing => "testing",
        }
    }

    /// Section order and default open state after Safety, which always leads.
    pub fn sections(self) -> &'static [(ActionSection, bool)] {
        use ActionSection::*;
        match self {
            // Flight: launch path first, then the FC, then the fill system.
            Self::Normal => &[
                (Sequences, true),
                (FlightComputer, true),
                (FillSystem, true),
                (AvBay, true),
                (Recovery, true),
                (DaqRecording, true),
                (Testing, false),
            ],
            // Test fire has no flight computer: FC and AV bay go last, closed.
            Self::TestFire => &[
                (Sequences, true),
                (FillSystem, true),
                (DaqRecording, true),
                (Recovery, true),
                (FlightComputer, false),
                (AvBay, false),
                (Testing, false),
            ],
            // HITL bench: everything, open, grouped.
            Self::Hitl => &[
                (Sequences, true),
                (FillSystem, true),
                (FlightComputer, true),
                (AvBay, true),
                (Recovery, true),
                (DaqRecording, true),
                (Testing, true),
            ],
            // Simulator: the simulator controls lead.
            Self::Testing => &[
                (Testing, true),
                (Sequences, true),
                (FlightComputer, true),
                (FillSystem, true),
                (AvBay, true),
                (Recovery, true),
                (DaqRecording, true),
            ],
        }
    }
}

/// The sections to show, in order, with their default open state: Safety
/// first, always; every other section only when it holds a control. A
/// section the mode table does not list is appended open, so no control is
/// ever hidden by the mode.
pub fn arrange(mode: OperatingMode, present: &BTreeSet<ActionSection>) -> Vec<(ActionSection, bool)> {
    let mut out = vec![(ActionSection::Safety, true)];
    for &(section, open) in mode.sections() {
        if present.contains(&section) {
            out.push((section, open));
        }
    }
    for &section in present {
        if !out.iter().any(|(listed, _)| *listed == section) {
            out.push((section, true));
        }
    }
    out
}

/// Whether a section renders open. Safety always is. An operator's explicit
/// choice wins; otherwise the mode default, except that a section holding a
/// control that is running, pending or prompting (now or earlier this
/// session) is never collapsed on its own.
pub fn is_open(section: ActionSection, default_open: bool, explicit: Option<bool>, active: bool) -> bool {
    if !section.collapsible() {
        return true;
    }
    explicit.unwrap_or(default_open || active)
}

/// One control's contribution to its section header.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ControlActivity {
    pub enabled: bool,
    /// Running, pending a confirmation or a sent press, or a backend-timed
    /// operation (the QD retraction pulse).
    pub busy: bool,
    /// The backend is prompting this control (blinking).
    pub prompt: bool,
    /// Blocked or failed: a failed/unconfirmed operation or a rejected request.
    pub attention: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SectionSummary {
    pub total: usize,
    pub enabled: usize,
    pub busy: bool,
    pub prompt: bool,
    pub attention: bool,
}

impl SectionSummary {
    pub fn add(&mut self, control: ControlActivity) {
        self.total += 1;
        self.enabled += usize::from(control.enabled);
        self.busy |= control.busy;
        self.prompt |= control.prompt;
        self.attention |= control.attention;
    }

    /// Never auto-collapse while any of these holds.
    pub fn active(&self) -> bool {
        self.busy || self.prompt || self.attention
    }

    /// The header's warning badge, most urgent first.
    pub fn badge(&self) -> Option<&'static str> {
        if self.attention {
            Some("Attention")
        } else if self.busy {
            Some("In progress")
        } else if self.prompt {
            Some("Action prompted")
        } else {
            None
        }
    }

    pub fn count_label(&self) -> String {
        format!("{} of {} enabled", self.enabled, self.total)
    }
}

/// The operator's explicit open/closed choices for one mode.
pub type Choices = BTreeMap<String, bool>;

const STORAGE_PREFIX: &str = "gs26_actions_sections_v1";

pub fn storage_key(user: Option<&str>, mode: OperatingMode) -> String {
    let user = user.map(str::trim).filter(|user| !user.is_empty()).unwrap_or("anonymous");
    format!("{STORAGE_PREFIX}_user_{user}_mode_{}", mode.id())
}

/// Unreadable or malformed storage falls back to the mode defaults.
pub fn decode(raw: Option<&str>) -> Choices {
    raw.and_then(|raw| serde_json::from_str::<Choices>(raw).ok())
        .map(|mut choices| {
            choices.retain(|id, _| ActionSection::ALL.iter().any(|s| s.collapsible() && s.id() == id));
            choices
        })
        .unwrap_or_default()
}

pub fn encode(choices: &Choices) -> String {
    serde_json::to_string(choices).unwrap_or_else(|_| "{}".into())
}

pub fn explicit(choices: &Choices, section: ActionSection) -> Option<bool> {
    section.collapsible().then(|| choices.get(section.id()).copied()).flatten()
}

/// A header click: flips what the operator currently sees and records it.
pub fn toggle(choices: &mut Choices, section: ActionSection, currently_open: bool) {
    if section.collapsible() {
        choices.insert(section.id().to_string(), !currently_open);
    }
}

pub fn load(user: Option<&str>, mode: OperatingMode) -> Choices {
    // Storage access never fails the page: a missing, blocked or corrupt
    // store reads as "no choices" and the mode defaults apply.
    let key = storage_key(user, mode);
    let raw = std::panic::catch_unwind(|| super::persist::get_string(&key)).ok().flatten();
    decode(raw.as_deref())
}

pub fn save(user: Option<&str>, mode: OperatingMode, choices: &Choices) {
    let key = storage_key(user, mode);
    let raw = encode(choices);
    let _ = std::panic::catch_unwind(|| super::persist::set_string(&key, &raw));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn present(sections: &[ActionSection]) -> BTreeSet<ActionSection> {
        sections.iter().copied().collect()
    }

    fn every_section() -> BTreeSet<ActionSection> {
        ActionSection::ALL.iter().copied().filter(|s| *s != ActionSection::Safety).collect()
    }

    fn order(mode: OperatingMode, sections: &BTreeSet<ActionSection>) -> Vec<ActionSection> {
        arrange(mode, sections).into_iter().map(|(section, _)| section).collect()
    }

    #[test]
    fn every_mode_table_lists_each_section_once_and_never_safety() {
        for mode in [OperatingMode::Normal, OperatingMode::Hitl, OperatingMode::TestFire, OperatingMode::Testing] {
            let listed: Vec<_> = mode.sections().iter().map(|(s, _)| *s).collect();
            let unique: BTreeSet<_> = listed.iter().copied().collect();
            assert_eq!(unique.len(), listed.len(), "{mode:?} lists a section twice");
            assert!(!unique.contains(&ActionSection::Safety));
            for section in ActionSection::ALL {
                if !matches!(section, ActionSection::Safety | ActionSection::Other) {
                    assert!(unique.contains(&section), "{mode:?} omits {section:?}");
                }
            }
        }
    }

    #[test]
    fn modes_lead_with_their_relevant_sections() {
        use ActionSection::*;
        let all = every_section();
        assert_eq!(
            order(OperatingMode::TestFire, &all)[..5],
            [Safety, Sequences, FillSystem, DaqRecording, Recovery]
        );
        assert_eq!(
            order(OperatingMode::Normal, &all)[..7],
            [Safety, Sequences, FlightComputer, FillSystem, AvBay, Recovery, DaqRecording]
        );
        // Test fire: FC and AV bay start closed, after the fire-relevant groups.
        let test_fire = arrange(OperatingMode::TestFire, &all);
        for (section, open) in &test_fire {
            assert_eq!(*open, !matches!(section, FlightComputer | AvBay | Testing), "{section:?}");
        }
        // HITL: every section, open.
        let hitl = arrange(OperatingMode::Hitl, &all);
        assert_eq!(hitl.len(), ActionSection::ALL.len());
        assert!(hitl.iter().all(|(_, open)| *open));
        assert_eq!(order(OperatingMode::Testing, &all)[..2], [Safety, Testing]);
    }

    #[test]
    fn empty_sections_are_hidden_but_safety_always_shows_first() {
        use ActionSection::*;
        assert_eq!(order(OperatingMode::Normal, &BTreeSet::new()), [Safety]);
        assert_eq!(
            order(OperatingMode::TestFire, &present(&[FlightComputer, FillSystem, Sequences])),
            [Safety, Sequences, FillSystem, FlightComputer]
        );
        // A control with no known section still shows, open, at the end.
        let with_other = arrange(OperatingMode::Normal, &present(&[Other, FillSystem]));
        assert_eq!(with_other, [(Safety, true), (FillSystem, true), (Other, true)]);
        for mode in [OperatingMode::Normal, OperatingMode::Hitl, OperatingMode::TestFire, OperatingMode::Testing] {
            for section in ActionSection::ALL {
                let shown = order(mode, &present(&[section]));
                assert_eq!(shown[0], Safety);
                assert!(shown.contains(&section), "{mode:?} drops {section:?}");
            }
        }
    }

    #[test]
    fn safety_is_never_collapsed_even_by_a_stored_choice() {
        let mut choices = Choices::new();
        choices.insert("safety".into(), false);
        assert!(is_open(ActionSection::Safety, false, Some(false), false));
        assert_eq!(explicit(&choices, ActionSection::Safety), None);
        toggle(&mut choices, ActionSection::Safety, true);
        assert_eq!(decode(Some(&encode(&choices))).get("safety"), None);
        assert!(!ActionSection::Safety.collapsible());
        assert!(ActionSection::ALL.iter().filter(|s| !s.collapsible()).eq([&ActionSection::Safety]));
    }

    #[test]
    fn an_active_section_is_never_collapsed_by_defaults() {
        use ActionSection::FillSystem;
        // Default closed, but a QD retraction is running: shown open.
        assert!(is_open(FillSystem, false, None, true));
        assert!(!is_open(FillSystem, false, None, false));
        // The operator's own collapse stands; the header badge still signals.
        assert!(!is_open(FillSystem, true, Some(false), true));
        let mut summary = SectionSummary::default();
        summary.add(ControlActivity { enabled: true, ..Default::default() });
        assert!(!summary.active());
        assert_eq!(summary.badge(), None);
        summary.add(ControlActivity { busy: true, ..Default::default() });
        assert!(summary.active());
        assert_eq!(summary.badge(), Some("In progress"));
        summary.add(ControlActivity { attention: true, ..Default::default() });
        assert_eq!(summary.badge(), Some("Attention"));
        assert_eq!(summary.count_label(), "1 of 3 enabled");
        let prompted = SectionSummary { prompt: true, ..Default::default() };
        assert!(prompted.active());
        assert_eq!(prompted.badge(), Some("Action prompted"));
    }

    #[test]
    fn choices_persist_per_operator_and_mode_and_corrupt_storage_falls_back() {
        assert_ne!(storage_key(Some("alice"), OperatingMode::Hitl), storage_key(Some("bob"), OperatingMode::Hitl));
        assert_ne!(storage_key(Some("alice"), OperatingMode::Hitl), storage_key(Some("alice"), OperatingMode::TestFire));
        assert_eq!(storage_key(None, OperatingMode::Normal), storage_key(Some("  "), OperatingMode::Normal));
        let mut choices = Choices::new();
        let open = is_open(ActionSection::FlightComputer, false, explicit(&choices, ActionSection::FlightComputer), false);
        toggle(&mut choices, ActionSection::FlightComputer, open);
        toggle(&mut choices, ActionSection::FillSystem, true);
        let restored = decode(Some(&encode(&choices)));
        assert_eq!(restored, choices);
        assert_eq!(explicit(&restored, ActionSection::FlightComputer), Some(true));
        assert_eq!(explicit(&restored, ActionSection::FillSystem), Some(false));
        assert_eq!(explicit(&restored, ActionSection::AvBay), None);
        for corrupt in [None, Some(""), Some("not json"), Some("[1,2]"), Some("{\"fill_system\":\"yes\"}")] {
            assert!(decode(corrupt).is_empty(), "{corrupt:?}");
        }
        assert!(decode(Some("{\"unknown_section\":true}")).is_empty());
    }

    #[test]
    fn mode_comes_from_the_station_then_the_layout() {
        assert_eq!(OperatingMode::from_effective(Some("hitl"), true), OperatingMode::Hitl);
        assert_eq!(OperatingMode::from_effective(Some("test_fire"), false), OperatingMode::TestFire);
        assert_eq!(OperatingMode::from_effective(Some("testing"), false), OperatingMode::Testing);
        assert_eq!(OperatingMode::from_effective(None, true), OperatingMode::TestFire);
        assert_eq!(OperatingMode::from_effective(Some("unknown"), false), OperatingMode::Normal);
    }

    #[test]
    fn backend_section_names_deserialize_and_unknown_ones_become_other() {
        for section in ActionSection::ALL.iter().filter(|s| **s != ActionSection::Other) {
            let parsed: ActionSection = serde_json::from_value(serde_json::json!(section.id())).unwrap();
            assert_eq!(parsed, *section);
        }
        let future: ActionSection = serde_json::from_value(serde_json::json!("station_network")).unwrap();
        assert_eq!(future, ActionSection::Other);
    }
}
