//! Mission phase (Startup, Idle, Fill, Launch, In flight, Landed) from the
//! ground station, current-flight maxima, per-phase dashboard profiles and the
//! one-shot "switch to Map after landing" decision.
//!
//! Phase and flight state are one concept: the operator sees one state, the
//! phase, with the detailed flight state only as its secondary detail
//! ("Fill · Nitrous fill"). `StateView::resolve` is the single place that turns
//! the station's `MissionPhase` message (or, from a station without it, the
//! bare detailed state) into what every view shows.
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
    /// The phase of a flight state name, with no station context. Retired
    /// names from older stations map as the network contract says.
    /// Aborted keeps a previous In flight or Landed, otherwise reads Idle.
    pub(crate) fn of_detailed(detailed: &str, previous: Option<Phase>) -> Option<Phase> {
        Some(match network_state(detailed)?.0 {
            "Startup" => Phase::Startup,
            "Idle" => Phase::Idle,
            "Fill" | "Armed" => Phase::Fill,
            "Launch" => Phase::Launch,
            "InFlight" => Phase::InFlight,
            "Landed" => Phase::Landed,
            _ => match previous {
                Some(Phase::InFlight) => Phase::InFlight,
                Some(Phase::Landed) => Phase::Landed,
                _ => Phase::Idle,
            },
        })
    }
}

/// The network FLIGHT_STATE values the station may report, by number.
pub(crate) const NETWORK_STATES: [(u8, &str); 8] = [
    (0, "Startup"),
    (1, "Idle"),
    (2, "Fill"),
    (6, "Armed"),
    (7, "Launch"),
    (13, "Landed"),
    (15, "Aborted"),
    (16, "InFlight"),
];

/// A flight state name as its network state, plus the retired detailed name
/// it came from. Retired names (older stations) map to the nearest network
/// value: fill steps to Fill, Ascent..Reefing to InFlight, Recovery to Landed.
pub(crate) fn network_state(name: &str) -> Option<(&'static str, Option<&'static str>)> {
    if let Some((_, network)) = NETWORK_STATES.iter().find(|(_, network)| *network == name) {
        return Some((network, None));
    }
    let (network, retired) = match name {
        "PreFill" => ("Fill", "PreFill"),
        "FillTest" => ("Fill", "FillTest"),
        "NitrogenFill" => ("Fill", "NitrogenFill"),
        "NitrousFill" => ("Fill", "NitrousFill"),
        "Ascent" => ("InFlight", "Ascent"),
        "Coast" => ("InFlight", "Coast"),
        "Apogee" => ("InFlight", "Apogee"),
        "Descent" => ("InFlight", "Descent"),
        "ParachuteDeploy" => ("InFlight", "Descent"),
        "Reefing" => ("InFlight", "Reefing"),
        "Recovery" => ("Landed", "Recovery"),
        _ => return None,
    };
    note_retired_state(name);
    Some((network, Some(retired)))
}

/// The network state of a FLIGHT_STATE number, retired numbers mapped as
/// [`network_state`] maps their names.
pub(crate) fn network_state_value(value: u8) -> Option<&'static str> {
    const RETIRED: [(u8, &str); 9] = [
        (3, "FillTest"),
        (4, "NitrogenFill"),
        (5, "NitrousFill"),
        (8, "Ascent"),
        (9, "Coast"),
        (10, "Apogee"),
        (11, "Descent"),
        (12, "Reefing"),
        (14, "Recovery"),
    ];
    if let Some((_, network)) = NETWORK_STATES.iter().find(|(number, _)| *number == value) {
        return Some(network);
    }
    let (_, name) = RETIRED.iter().find(|(number, _)| *number == value)?;
    network_state(name).map(|(network, _)| network)
}

/// Retired values are a contract violation by the station; log each once.
fn note_retired_state(name: &str) {
    if name == "PreFill" {
        // The old name of network value 2 (now Fill), not a retired value.
        return;
    }
    static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let Ok(mut seen) = SEEN.lock() else { return };
    if seen.iter().any(|known| known == name) {
        return;
    }
    seen.push(name.to_string());
    crate::debug_log::append(&format!(
        "Flight state contract violation: the station reported retired state {name}; shown as its network state."
    ));
}

/// Operator label of a flight state name. Matches the station's labels.
pub(crate) fn detailed_label(detailed: &str) -> String {
    match detailed {
        "InFlight" => "In flight".into(),
        "PreFill" => "Pre-fill".into(),
        "FillTest" => "Fill test".into(),
        "NitrogenFill" => "Nitrogen fill".into(),
        "NitrousFill" => "Nitrous fill".into(),
        "ParachuteDeploy" => "Descent".into(),
        other => super::types::display_flight_state(other),
    }
}

/// The FC's own detailed phase, from its state report or flight state.
#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
pub(crate) struct FcPhase {
    #[serde(default)]
    pub value: Option<u8>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub age_ms: Option<u64>,
}

/// Secondary detail the station sends with the network state: the GS fill
/// sub-phase and the FC's detailed phase. Absent from older stations.
#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
pub(crate) struct StateDetail {
    #[serde(default)]
    pub fill_phase: Option<String>,
    #[serde(default)]
    pub fill_phase_label: Option<String>,
    #[serde(default)]
    pub fc_phase: Option<FcPhase>,
}

impl StateDetail {
    fn fill_label(&self) -> Option<String> {
        self.fill_phase_label
            .clone()
            .filter(|label| !label.trim().is_empty())
            .or_else(|| self.fill_phase.as_deref().filter(|p| !p.is_empty()).map(detailed_label))
    }
    fn fc_label(&self) -> Option<String> {
        self.fc_phase
            .as_ref()
            .map(|phase| phase.label.clone())
            .filter(|label| !label.trim().is_empty())
    }
    fn is_empty(&self) -> bool {
        self.fill_label().is_none() && self.fc_label().is_none()
    }
}

/// The detail from the WS `FlightState` message, beside the state signal.
pub(crate) static FLIGHT_STATE_DETAIL: GlobalSignal<StateDetail> = Signal::global(StateDetail::default);

/// The one operator-visible state: the network state plus optional secondary
/// detail ("In flight · Descent", "Fill · Nitrous fill", "Landed · Recovery").
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StateView {
    pub phase: Option<Phase>,
    /// Network state name ("InFlight", "Fill", "Aborted"), empty when unknown.
    pub detailed: String,
    /// The fill sub-phase or FC phase shown beside the state.
    pub detail: Option<String>,
    pub aborted: bool,
    pub comms_loss_landed: bool,
}

impl StateView {
    /// Prefer the station's `MissionPhase` message; its detailed state and
    /// detail fields win over the separate state signal. A station without
    /// the message (or before its first one) gets the bare state and the
    /// `FlightState` message's detail.
    pub(crate) fn resolve(
        message: Option<&MissionPhaseView>,
        flight_state: &str,
        signal_detail: &StateDetail,
    ) -> StateView {
        let (phase, raw, detail_fields, aborted, comms_loss_landed) = match message {
            Some(message) => {
                let raw = if message.detailed.is_empty() { flight_state } else { &message.detailed };
                let fields = if message.state_detail.is_empty() { signal_detail } else { &message.state_detail };
                (
                    Some(message.phase),
                    raw,
                    fields,
                    message.aborted || raw == "Aborted",
                    message.comms_loss_landed,
                )
            }
            None => (
                Phase::of_detailed(flight_state, None),
                flight_state,
                signal_detail,
                flight_state == "Aborted",
                false,
            ),
        };
        let (detailed, detail) = match network_state(raw) {
            Some((network, retired)) => {
                let sent = match network {
                    "Fill" => detail_fields.fill_label(),
                    "InFlight" => detail_fields.fc_label(),
                    // Recovery is the FC's detail of Landed.
                    "Landed" => detail_fields.fc_label().filter(|label| label != "Landed"),
                    _ => None,
                };
                let composed = message
                    .and_then(|message| message.detail.as_deref())
                    .and_then(|detail| detail.split_once(" · "))
                    .map(|(_, detail)| detail.to_string());
                let detail = match network {
                    "Fill" | "InFlight" | "Landed" => {
                        sent.or(retired.map(detailed_label)).or(composed)
                    }
                    _ => None,
                };
                (network.to_string(), detail)
            }
            None => (raw.to_string(), None),
        };
        StateView { phase, detailed, detail, aborted, comms_loss_landed }
    }

    /// The network state as the operator reads it ("In flight") or "Unknown".
    pub(crate) fn label(&self) -> String {
        match self.detailed.as_str() {
            "" | "Unknown" => "Unknown".into(),
            detailed => detailed_label(detailed),
        }
    }

    /// The state with its detail: "In flight · Descent", "Aborted".
    pub(crate) fn text(&self) -> String {
        match &self.detail {
            Some(detail) => format!("{} · {detail}", self.label()),
            None => self.label(),
        }
    }
}

/// The current operator state from the global phase message, the state
/// signal and the `FlightState` message's detail.
pub(crate) fn current_state(flight_state: &str) -> StateView {
    StateView::resolve(MISSION_PHASE.read().as_ref(), flight_state, &FLIGHT_STATE_DETAIL.read())
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct MissionPhaseView {
    pub phase: Phase,
    #[serde(default)]
    pub label: String,
    /// Composed operator string ("Fill · Nitrous fill"); absent from older stations.
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub detailed: String,
    #[serde(default, flatten)]
    pub state_detail: StateDetail,
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

    fn message(json: serde_json::Value) -> MissionPhaseView {
        serde_json::from_value(json).unwrap()
    }

    fn none() -> StateDetail {
        StateDetail::default()
    }

    #[test]
    fn state_is_the_current_flight_state() {
        let fill = message(serde_json::json!({
            "phase":"fill","label":"Fill","detail":"Nitrous fill","detailed":"NitrousFill"
        }));
        // The phase message's detailed state wins over the separate signal.
        let view = StateView::resolve(Some(&fill), "Idle", &none());
        assert_eq!(view.text(), "Fill · Nitrous fill");
        let overlay = message(serde_json::json!({
            "phase":"fill","label":"Fill","detail":"GSE fill active","detailed":"Idle"
        }));
        assert_eq!(StateView::resolve(Some(&overlay), "Idle", &none()).text(), "Idle");
        let idle = message(serde_json::json!({"phase":"idle","detail":null,"detailed":"Idle"}));
        assert_eq!(StateView::resolve(Some(&idle), "Idle", &none()).text(), "Idle");
        let aborted = message(serde_json::json!({
            "phase":"in_flight","detail":"Aborted","detailed":"Aborted","aborted":true
        }));
        let view = StateView::resolve(Some(&aborted), "Aborted", &none());
        assert!(view.aborted);
        assert_eq!(view.text(), "Aborted");
    }

    #[test]
    fn contract_states_show_the_state_with_the_fc_phase_or_fill_step_beside_it() {
        let in_flight = message(serde_json::json!({
            "phase":"in_flight","label":"In flight","detail":"In flight · Descent","detailed":"InFlight",
            "fill_phase":null,
            "fc_phase":{"value":11,"label":"Descent","source":"fc_state_report","age_ms":1200}
        }));
        let view = StateView::resolve(Some(&in_flight), "InFlight", &none());
        assert_eq!(view.phase, Some(Phase::InFlight));
        assert_eq!(view.label(), "In flight");
        assert_eq!(view.detail.as_deref(), Some("Descent"));
        assert_eq!(view.text(), "In flight · Descent");

        let fill = message(serde_json::json!({
            "phase":"fill","detail":"Fill · Nitrous fill","detailed":"Fill",
            "fill_phase":"NitrousFill","fill_phase_label":"Nitrous fill","fc_phase":null
        }));
        assert_eq!(StateView::resolve(Some(&fill), "Fill", &none()).text(), "Fill · Nitrous fill");
        // Without the label the sub-phase name is labelled here.
        let fill = message(serde_json::json!({"phase":"fill","detailed":"Fill","fill_phase":"FillTest"}));
        assert_eq!(StateView::resolve(Some(&fill), "Fill", &none()).text(), "Fill · Fill test");

        let landed = message(serde_json::json!({
            "phase":"landed","detailed":"Landed",
            "fc_phase":{"value":14,"label":"Recovery","source":"fc_state_report","age_ms":10}
        }));
        assert_eq!(StateView::resolve(Some(&landed), "Landed", &none()).text(), "Landed · Recovery");
        let landed = message(serde_json::json!({
            "phase":"landed","detailed":"Landed",
            "fc_phase":{"value":13,"label":"Landed","source":"fc_flight_state","age_ms":10}
        }));
        assert_eq!(StateView::resolve(Some(&landed), "Landed", &none()).text(), "Landed");

        // A FC phase never decorates a state it does not belong to.
        let idle = message(serde_json::json!({
            "phase":"idle","detailed":"Idle","fc_phase":{"value":0,"label":"Startup"}
        }));
        assert_eq!(StateView::resolve(Some(&idle), "Idle", &none()).text(), "Idle");
        let aborted = message(serde_json::json!({
            "phase":"in_flight","detailed":"Aborted","aborted":true,"fc_phase":{"label":"Coast"}
        }));
        assert_eq!(StateView::resolve(Some(&aborted), "Aborted", &none()).text(), "Aborted");
    }

    #[test]
    fn without_a_phase_message_the_flight_state_message_supplies_the_detail() {
        let detail: StateDetail = serde_json::from_value(serde_json::json!({
            "state":"InFlight","fill_phase":null,
            "fc_phase":{"value":9,"label":"Coast","source":"fc_state_report","age_ms":300}
        }))
        .unwrap();
        let view = StateView::resolve(None, "InFlight", &detail);
        assert_eq!(view.phase, Some(Phase::InFlight));
        assert_eq!(view.text(), "In flight · Coast");
        assert_eq!(StateView::resolve(None, "InFlight", &none()).text(), "In flight");
        let fill: StateDetail = serde_json::from_value(serde_json::json!({
            "state":"Fill","fill_phase":"NitrogenFill","fill_phase_label":"Nitrogen fill","fc_phase":null
        }))
        .unwrap();
        assert_eq!(StateView::resolve(None, "Fill", &fill).text(), "Fill · Nitrogen fill");
        assert_eq!(StateView::resolve(None, "Fill", &none()).text(), "Fill");
        // A stale fill step from the previous message does not label Launch.
        assert_eq!(StateView::resolve(None, "Launch", &fill).text(), "Launch");
    }

    #[test]
    fn older_stations_still_show_the_flight_state() {
        // Phase message without `detail`.
        let old = message(serde_json::json!({"phase":"in_flight","detailed":"Coast"}));
        assert_eq!(StateView::resolve(Some(&old), "Coast", &none()).text(), "In flight · Coast");
        let old = message(serde_json::json!({"phase":"launch","detailed":"Launch"}));
        assert_eq!(StateView::resolve(Some(&old), "Launch", &none()).text(), "Launch");
        // No phase message at all: retired values map to their network state.
        for (detailed, shown, phase) in [
            ("Startup", "Startup", Phase::Startup),
            ("Idle", "Idle", Phase::Idle),
            ("Fill", "Fill", Phase::Fill),
            ("PreFill", "Fill · Pre-fill", Phase::Fill),
            ("FillTest", "Fill · Fill test", Phase::Fill),
            ("NitrogenFill", "Fill · Nitrogen fill", Phase::Fill),
            ("NitrousFill", "Fill · Nitrous fill", Phase::Fill),
            ("Armed", "Armed", Phase::Fill),
            ("Launch", "Launch", Phase::Launch),
            ("InFlight", "In flight", Phase::InFlight),
            ("Ascent", "In flight · Ascent", Phase::InFlight),
            ("Coast", "In flight · Coast", Phase::InFlight),
            ("Apogee", "In flight · Apogee", Phase::InFlight),
            ("Descent", "In flight · Descent", Phase::InFlight),
            ("ParachuteDeploy", "In flight · Descent", Phase::InFlight),
            ("Reefing", "In flight · Reefing", Phase::InFlight),
            ("Landed", "Landed", Phase::Landed),
            ("Recovery", "Landed · Recovery", Phase::Landed),
            ("Aborted", "Aborted", Phase::Idle),
        ] {
            let view = StateView::resolve(None, detailed, &none());
            assert_eq!(view.text(), shown, "{detailed}");
            assert_eq!(view.phase, Some(phase), "{detailed}");
        }
        assert_eq!(StateView::resolve(None, "Unknown", &none()).text(), "Unknown");
        assert_eq!(StateView::resolve(None, "", &none()).text(), "Unknown");
    }

    #[test]
    fn network_state_numbers_follow_the_contract() {
        for (value, name) in [
            (0, "Startup"), (1, "Idle"), (2, "Fill"), (6, "Armed"), (7, "Launch"),
            (13, "Landed"), (15, "Aborted"), (16, "InFlight"),
            // Retired values map to the nearest network value.
            (3, "Fill"), (4, "Fill"), (5, "Fill"),
            (8, "InFlight"), (9, "InFlight"), (10, "InFlight"), (11, "InFlight"), (12, "InFlight"),
            (14, "Landed"),
        ] {
            assert_eq!(network_state_value(value), Some(name), "{value}");
        }
        assert_eq!(network_state_value(17), None);
        assert_eq!(network_state_value(255), None);
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
