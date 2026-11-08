//! Mission phase (Startup, Idle, Fill, Launch, In flight, Landed) from the
//! ground station, current-flight maxima, per-phase dashboard profiles and the
//! one-shot "switch to Map after landing" decision.
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Startup,
    Idle,
    Fill,
    Launch,
    InFlight,
    Landed,
}

impl Phase {
    pub(crate) const ALL: [Phase; 6] = [
        Phase::Startup,
        Phase::Idle,
        Phase::Fill,
        Phase::Launch,
        Phase::InFlight,
        Phase::Landed,
    ];
    pub(crate) fn label(self) -> &'static str {
        match self {
            Phase::Startup => "Startup",
            Phase::Idle => "Idle",
            Phase::Fill => "Fill",
            Phase::Launch => "Launch",
            Phase::InFlight => "In flight",
            Phase::Landed => "Landed",
        }
    }
    pub(crate) fn key(self) -> &'static str {
        match self {
            Phase::Startup => "startup",
            Phase::Idle => "idle",
            Phase::Fill => "fill",
            Phase::Launch => "launch",
            Phase::InFlight => "in_flight",
            Phase::Landed => "landed",
        }
    }
    pub(crate) fn from_key(key: &str) -> Option<Phase> {
        Phase::ALL.into_iter().find(|phase| phase.key() == key)
    }
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct MissionPhaseView {
    pub phase: Phase,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub detailed: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub manual_only: bool,
    #[serde(default)]
    pub aborted: bool,
    #[serde(default)]
    pub fill_activity: bool,
    #[serde(default)]
    pub disconnect_aborts_armed: bool,
    #[serde(default)]
    pub comms_loss_landed: bool,
    #[serde(default)]
    pub since_ms: i64,
}

pub(crate) static MISSION_PHASE: GlobalSignal<Option<MissionPhaseView>> = Signal::global(|| None);

#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
pub(crate) struct Extreme {
    pub value: f64,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub t_plus_ms: Option<i64>,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
pub(crate) struct FlightMaximaView {
    #[serde(default)]
    pub tracking: bool,
    #[serde(default)]
    pub altitude: Vec<Extreme>,
    #[serde(default)]
    pub speed: Vec<Extreme>,
    #[serde(default)]
    pub acceleration: Vec<Extreme>,
    #[serde(default)]
    pub fc_altitude: Option<Extreme>,
    #[serde(default)]
    pub fc_speed: Option<Extreme>,
    #[serde(default)]
    pub fc_acceleration: Option<Extreme>,
}

pub(crate) static FLIGHT_MAXIMA: GlobalSignal<FlightMaximaView> =
    Signal::global(FlightMaximaView::default);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MaxQuantity {
    Altitude,
    Speed,
    Acceleration,
}

impl MaxQuantity {
    pub(crate) fn from_index(index: usize) -> MaxQuantity {
        match index {
            0 => MaxQuantity::Altitude,
            1 => MaxQuantity::Speed,
            _ => MaxQuantity::Acceleration,
        }
    }
}

/// One display line: "2810 m · T+25.0 s · FC Kalman altitude".
pub(crate) fn format_extreme(quantity: MaxQuantity, extreme: &Extreme) -> String {
    let value = match quantity {
        MaxQuantity::Altitude => format!("{:.0} m", extreme.value),
        MaxQuantity::Speed => format!("{:.1} m/s", extreme.value),
        MaxQuantity::Acceleration => format!(
            "{:.1} m/s² ({:.1} g)",
            extreme.value,
            extreme.value / 9.80665
        ),
    };
    let t_plus = extreme
        .t_plus_ms
        .map(|ms| format!(" · T+{:.1} s", ms as f64 / 1000.0))
        .unwrap_or_default();
    format!("{value}{t_plus} · {}", extreme.source)
}

impl FlightMaximaView {
    /// (authoritative FC value, GS values preferred first).
    pub(crate) fn quantity(&self, quantity: MaxQuantity) -> (Option<&Extreme>, &[Extreme]) {
        match quantity {
            MaxQuantity::Altitude => (self.fc_altitude.as_ref(), &self.altitude),
            MaxQuantity::Speed => (self.fc_speed.as_ref(), &self.speed),
            MaxQuantity::Acceleration => (self.fc_acceleration.as_ref(), &self.acceleration),
        }
    }
}

// ---------------------------------------------------------------------------
// Per-phase dashboard profiles
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub(crate) struct PhaseProfile {
    /// Card ids shown first, in this order, whatever their base visibility.
    #[serde(default)]
    pub top: Vec<String>,
    /// Card ids hidden in this phase.
    #[serde(default)]
    pub hidden: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct PhaseProfiles {
    #[serde(default = "default_true")]
    pub auto_switch: bool,
    #[serde(default)]
    pub profiles: BTreeMap<String, PhaseProfile>,
}

fn default_true() -> bool {
    true
}

pub(crate) const FLIGHT_CARD_IDS: [&str; 10] = [
    "flight-altitude",
    "flight-speed",
    "flight-accel",
    "flight-gyro-x",
    "flight-gyro-y",
    "flight-gyro-z",
    "flight-max-altitude",
    "flight-max-speed",
    "flight-max-accel",
    "flight-mini-map",
];

impl Default for PhaseProfiles {
    fn default() -> Self {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            Phase::InFlight.key().to_string(),
            PhaseProfile {
                top: FLIGHT_CARD_IDS.iter().map(|id| id.to_string()).collect(),
                hidden: Vec::new(),
            },
        );
        profiles.insert(
            Phase::Landed.key().to_string(),
            PhaseProfile {
                top: [
                    "flight-mini-map",
                    "flight-max-altitude",
                    "flight-max-speed",
                    "flight-max-accel",
                ]
                .iter()
                .map(|id| id.to_string())
                .collect(),
                hidden: Vec::new(),
            },
        );
        Self {
            auto_switch: true,
            profiles,
        }
    }
}

impl PhaseProfiles {
    pub(crate) fn profile(&self, phase: Phase) -> Option<&PhaseProfile> {
        self.profiles.get(phase.key())
    }
    pub(crate) fn profile_mut(&mut self, phase: Phase) -> &mut PhaseProfile {
        self.profiles.entry(phase.key().to_string()).or_default()
    }
    pub(crate) fn valid(&self) -> bool {
        self.profiles.len() <= Phase::ALL.len()
            && self.profiles.keys().all(|key| Phase::from_key(key).is_some())
            && self
                .profiles
                .values()
                .all(|p| p.top.len() <= 32 && p.hidden.len() <= 32)
    }
}

/// Display order for one phase: profile `top` cards first (forced visible),
/// then every other card in base order with base visibility unless hidden in
/// this phase. Returns (base index, visible).
pub(crate) fn profile_order(
    ids: &[&str],
    base_visible: &[bool],
    profile: Option<&PhaseProfile>,
) -> Vec<(usize, bool)> {
    let mut out = Vec::with_capacity(ids.len());
    let mut used = vec![false; ids.len()];
    if let Some(profile) = profile {
        for id in &profile.top {
            if let Some(index) = ids.iter().position(|card| card == id)
                && !used[index]
            {
                used[index] = true;
                out.push((index, true));
            }
        }
    }
    for (index, id) in ids.iter().enumerate() {
        if used[index] {
            continue;
        }
        let hidden = profile.is_some_and(|p| p.hidden.iter().any(|h| h == id));
        out.push((index, base_visible[index] && !hidden));
    }
    out
}

// ---------------------------------------------------------------------------
// Switch to Map after landing
// ---------------------------------------------------------------------------

/// Switch exactly once per landing episode (keyed by the phase's `since_ms`),
/// only on a live transition into Landed seen while the dashboard is open.
pub(crate) fn should_switch_to_map(
    previous: Option<Phase>,
    current: &MissionPhaseView,
    setting_enabled: bool,
    dashboard_open: bool,
    already_switched_for: Option<i64>,
) -> bool {
    setting_enabled
        && dashboard_open
        && current.phase == Phase::Landed
        && previous.is_some_and(|p| p != Phase::Landed)
        && already_switched_for != Some(current.since_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(phase: Phase, since_ms: i64) -> MissionPhaseView {
        serde_json::from_value(serde_json::json!({
            "phase": phase, "label": phase.label(), "detailed": "Landed", "since_ms": since_ms
        }))
        .unwrap()
    }

    #[test]
    fn decodes_backend_phase_message() {
        let msg: MissionPhaseView = serde_json::from_str(
            r#"{"phase":"in_flight","label":"In flight","detailed":"Descent","mode":"flight","manual_only":false,"aborted":false,"required_boards_seen":true,"fill_activity":false,"disconnect_aborts_armed":false,"comms_loss_landed":false,"since_ms":5}"#,
        )
        .unwrap();
        assert_eq!(msg.phase, Phase::InFlight);
        assert_eq!(msg.detailed, "Descent");
        for phase in Phase::ALL {
            assert_eq!(Phase::from_key(phase.key()), Some(phase));
        }
    }

    #[test]
    fn in_flight_default_puts_flight_data_on_top_of_fill_data() {
        let ids = ["fill-a", "fill-b", "flight-altitude", "flight-accel", "flight-gyro-x"];
        let visible = [true, true, false, false, false];
        let profiles = PhaseProfiles::default();
        let order = profile_order(&ids, &visible, profiles.profile(Phase::InFlight));
        assert_eq!(
            order,
            vec![(2, true), (3, true), (4, true), (0, true), (1, true)]
        );
        // Fill keeps the base dashboard; hidden flight cards stay hidden.
        let order = profile_order(&ids, &visible, profiles.profile(Phase::Fill));
        assert_eq!(
            order,
            vec![(0, true), (1, true), (2, false), (3, false), (4, false)]
        );
    }

    #[test]
    fn profile_hides_and_ignores_unknown_or_duplicate_ids() {
        let ids = ["a", "b", "c"];
        let profile = PhaseProfile {
            top: vec!["c".into(), "missing".into(), "c".into()],
            hidden: vec!["a".into()],
        };
        assert_eq!(
            profile_order(&ids, &[true, true, true], Some(&profile)),
            vec![(2, true), (0, false), (1, true)]
        );
    }

    #[test]
    fn profiles_round_trip_and_validate() {
        let profiles = PhaseProfiles::default();
        let json = serde_json::to_string(&profiles).unwrap();
        let back: PhaseProfiles = serde_json::from_str(&json).unwrap();
        assert_eq!(back, profiles);
        assert!(back.valid());
        let bad: PhaseProfiles =
            serde_json::from_str(r#"{"auto_switch":true,"profiles":{"orbit":{}}}"#).unwrap();
        assert!(!bad.valid());
    }

    #[test]
    fn map_switch_happens_once_per_landing_and_only_live() {
        let landed = view(Phase::Landed, 100);
        assert!(should_switch_to_map(Some(Phase::InFlight), &landed, true, true, None));
        assert!(!should_switch_to_map(Some(Phase::InFlight), &landed, true, true, Some(100)));
        assert!(!should_switch_to_map(Some(Phase::InFlight), &landed, false, true, None));
        assert!(!should_switch_to_map(Some(Phase::InFlight), &landed, true, false, None));
        // A reload while already Landed is not a landing.
        assert!(!should_switch_to_map(None, &landed, true, true, None));
        assert!(!should_switch_to_map(Some(Phase::Landed), &landed, true, true, None));
        // A later landing episode switches again.
        assert!(should_switch_to_map(Some(Phase::InFlight), &view(Phase::Landed, 200), true, true, Some(100)));
    }

    #[test]
    fn formats_maxima_with_time_and_source() {
        let e = Extreme {
            value: 98.0665,
            source: "Raw accelerometer".into(),
            t_plus_ms: Some(2_500),
        };
        assert_eq!(
            format_extreme(MaxQuantity::Acceleration, &e),
            "98.1 m/s² (10.0 g) · T+2.5 s · Raw accelerometer"
        );
    }
}
