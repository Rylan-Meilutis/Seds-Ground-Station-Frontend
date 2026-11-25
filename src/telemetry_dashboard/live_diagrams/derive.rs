//! Pure state derivation for the live diagrams: topology + current inputs ->
//! what each valve, line and reading shows. No rendering, no globals, so the
//! rules are unit-tested directly.
//!
//! Rules (docs/live-diagrams.md):
//! * a valve is open/closed from its confirmed telemetry; a pending command
//!   (`requested` differs from the confirmed state, or a running timed
//!   operation) is drawn "pending"; telemetry older than `stale_ms` is
//!   "stale" with its age; never seen is "unknown";
//! * a pipe flows only when every valve on its path is open AND the
//!   upstream pressure exceeds the downstream pressure by `min_dp_psi`;
//!   a closed valve stops it; an unknown/stale valve or pressure makes the
//!   flow unknown (drawn dashed grey, never animated);
//! * a junction passes on what flows into it; the fluid of its outgoing pipe
//!   is the set of fluids flowing in.
use super::topology::{Binding, EdgeKind, Format, NodeKind, OpenWhen, Topology, ValveKind, When};
use std::collections::BTreeMap;

/// Station statuses a binding may name.
pub(crate) const STATUS_KEYS: &[&str] = &[
    "flight_state",
    "fill_phase",
    "fc_phase",
    "estimator",
    "estimator_rate_hz",
    "igt_mode",
    "pyro_drogue",
    "pyro_main",
    "pyro_continuity",
    "fc_sd",
    "radio_rssi",
    "provenance",
];

pub(crate) fn status_key_known(key: &str) -> bool {
    STATUS_KEYS.contains(&key) || key.strip_prefix("topology:").is_some_and(|s| !s.is_empty())
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Num(f32),
    Bool(bool),
    Text(String),
    /// An ADC over-range telemetry channel: no valid number (shown as OVR).
    OverRange(crate::telemetry_dashboard::over_range::OverRange),
}

impl Value {
    pub fn truthy(&self) -> Option<bool> {
        match self {
            Value::Num(v) => Some(*v >= 0.5),
            Value::Bool(b) => Some(*b),
            Value::Text(_) | Value::OverRange(_) => None,
        }
    }
    pub fn number(&self) -> Option<f32> {
        match self {
            Value::Num(v) => Some(*v),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::Text(_) | Value::OverRange(_) => None,
        }
    }
}

/// One observation and how old it is (None: no age, e.g. a constant or a
/// status the station republishes).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Observed {
    pub value: Value,
    pub age_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ControlView {
    pub actuated: Option<bool>,
    pub requested: Option<bool>,
    /// A timed physical operation behind the control is running (QD retraction).
    pub running: bool,
}

/// What the dashboard knows right now. Implemented over the live stores by
/// the tab and by fakes in the tests.
pub(crate) trait Inputs {
    fn telemetry(&self, data_type: &str, sender: Option<&str>, index: usize) -> Option<Observed>;
    fn control(&self, cmd: &str) -> Option<ControlView>;
    fn board(&self, sender: &str) -> Option<Observed>;
    fn status(&self, key: &str) -> Option<Observed>;
    fn igt(&self) -> bool;
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Resolved {
    pub value: Option<Value>,
    pub age_ms: Option<i64>,
    pub stale: bool,
    pub assumed: bool,
}

impl Resolved {
    fn missing() -> Self {
        Self { value: None, age_ms: None, stale: false, assumed: false }
    }
    /// A value that may drive state: present and not stale.
    pub fn fresh(&self) -> Option<&Value> {
        self.value.as_ref().filter(|_| !self.stale)
    }
}

pub(crate) fn resolve(binding: &Binding, inputs: &dyn Inputs, stale_ms: i64) -> Resolved {
    let stale_ms = binding.stale_ms.unwrap_or(stale_ms);
    let observed = if let Some(data_type) = &binding.telemetry {
        inputs.telemetry(data_type, binding.sender.as_deref(), binding.index)
    } else if let Some(value) = binding.constant {
        Some(Observed { value: Value::Num(value), age_ms: None })
    } else if let Some(board) = &binding.board {
        inputs.board(board)
    } else if let Some(cmd) = &binding.control {
        inputs
            .control(cmd)
            .and_then(|c| c.actuated)
            .map(|on| Observed { value: Value::Bool(on), age_ms: None })
    } else if let Some(key) = &binding.status {
        inputs.status(key)
    } else {
        None
    };
    let Some(Observed { value, age_ms }) = observed else {
        return Resolved::missing();
    };
    let value = match value {
        Value::Num(v) if !v.is_finite() => return Resolved::missing(),
        Value::Num(v) => Value::Num(v * binding.scale),
        other => other,
    };
    Resolved {
        stale: age_ms.is_some_and(|age| age > stale_ms),
        value: Some(value),
        age_ms,
        assumed: binding.assumed,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ValveState {
    Open,
    Closed,
    /// A command is out and the confirmed state does not match it yet.
    Pending { to_open: bool, from: Option<bool> },
    /// Last confirmed state is older than the stale limit.
    Stale { was_open: bool, age_ms: i64 },
    /// Never reported (or no fresh source).
    Unknown,
    /// A hand valve the station cannot see; assumed open for the flow rule.
    Manual,
}

impl ValveState {
    pub fn flow_gate(self) -> Gate {
        match self {
            ValveState::Open | ValveState::Manual => Gate::Open,
            ValveState::Closed => Gate::Closed,
            // A pending open from a confirmed closed valve is still closed;
            // any other pending move leaves the position uncertain.
            ValveState::Pending { from: Some(false), .. } => Gate::Closed,
            ValveState::Pending { .. } | ValveState::Stale { .. } | ValveState::Unknown => Gate::Unknown,
        }
    }
    pub fn css(self) -> &'static str {
        match self {
            ValveState::Open => "open",
            ValveState::Closed => "closed",
            ValveState::Pending { .. } => "pending",
            ValveState::Stale { .. } => "stale",
            ValveState::Unknown => "unknown",
            ValveState::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    Open,
    Closed,
    Unknown,
}

pub(crate) fn valve_state(valve: &super::topology::Valve, inputs: &dyn Inputs, stale_ms: i64) -> ValveState {
    let Some(binding) = &valve.state else {
        return ValveState::Manual;
    };
    let resolved = resolve(binding, inputs, stale_ms);
    let confirmed = resolved.value.as_ref().and_then(Value::truthy).map(|high| match valve.open_when {
        OpenWhen::High => high,
        OpenWhen::Low => !high,
    });
    let control = valve.command.as_deref().and_then(|cmd| inputs.control(cmd)).unwrap_or_default();
    // The command's own sense: RetractPlumbing requested=true means "retract",
    // which is the QD's closed (disconnected) position.
    let requested = control.requested.map(|r| match valve.open_when {
        OpenWhen::High => r,
        OpenWhen::Low => !r,
    });
    let current = confirmed.filter(|_| !resolved.stale);
    if let Some(to_open) = requested
        && current != Some(to_open)
    {
        return ValveState::Pending { to_open, from: current };
    }
    if control.running && valve.kind == ValveKind::Qd {
        return ValveState::Pending { to_open: false, from: current };
    }
    match (confirmed, resolved.stale) {
        (Some(open), true) => ValveState::Stale { was_open: open, age_ms: resolved.age_ms.unwrap_or(0) },
        (Some(true), false) => ValveState::Open,
        (Some(false), false) => ValveState::Closed,
        (None, _) => ValveState::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flow {
    /// Path open and a pressure differential: animated.
    Flowing,
    /// Path open, no differential (equalised).
    Static,
    /// A valve on the path is closed.
    Blocked,
    /// A valve or pressure on the path is unknown or stale.
    Unknown,
}

impl Flow {
    pub fn css(self) -> &'static str {
        match self {
            Flow::Flowing => "flowing",
            Flow::Static => "static",
            Flow::Blocked => "blocked",
            Flow::Unknown => "unknown",
        }
    }
    fn rank(self) -> u8 {
        match self {
            Flow::Flowing => 3,
            Flow::Unknown => 2,
            Flow::Static => 1,
            Flow::Blocked => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Link {
    Up,
    Down,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgeState {
    pub flow: Flow,
    /// Fluid ids carried (pipes); more than one for a merged line.
    pub fluids: Vec<String>,
    pub link: Option<Link>,
    pub link_age_ms: Option<i64>,
    pub dp_psi: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReadingState {
    pub text: String,
    pub stale: bool,
    pub missing: bool,
    pub warn: bool,
    pub assumed: bool,
    pub age_ms: Option<i64>,
    /// The bound channel is ADC over range; `text` is the OVR text.
    pub over_range: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NodeState {
    pub pressure: Resolved,
    pub level_pct: Option<f32>,
    pub online: Option<Link>,
    pub age_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiagramState {
    pub igt: bool,
    pub valves: BTreeMap<String, ValveState>,
    pub edges: BTreeMap<String, EdgeState>,
    pub nodes: BTreeMap<String, NodeState>,
    pub readings: BTreeMap<String, ReadingState>,
}

pub(crate) fn visible(when: Option<When>, igt: bool) -> bool {
    match when {
        None => true,
        Some(When::Igt) => igt,
        Some(When::NotIgt) => !igt,
    }
}

fn link_of(resolved: &Resolved) -> Link {
    match resolved.value.as_ref().and_then(Value::truthy) {
        None => Link::Unknown,
        Some(_) if resolved.stale => Link::Down,
        Some(true) => Link::Up,
        Some(false) => Link::Down,
    }
}

fn combine(gates: impl IntoIterator<Item = Gate>) -> Gate {
    let mut out = Gate::Open;
    for gate in gates {
        match gate {
            Gate::Closed => return Gate::Closed,
            Gate::Unknown => out = Gate::Unknown,
            Gate::Open => {}
        }
    }
    out
}

pub(crate) fn derive(topology: &Topology, inputs: &dyn Inputs) -> DiagramState {
    let stale_ms = topology.stale_ms;
    let igt = inputs.igt();
    let valves: BTreeMap<String, ValveState> = topology
        .valves
        .iter()
        .map(|v| (v.id.clone(), valve_state(v, inputs, stale_ms)))
        .collect();
    let nodes: BTreeMap<String, NodeState> = topology
        .nodes
        .iter()
        .map(|node| {
            let mut pressure = node
                .pressure
                .as_ref()
                .map(|b| resolve(b, inputs, stale_ms))
                .unwrap_or_else(Resolved::missing);
            if pressure.fresh().is_none()
                && let Some(psi) = node.fallback_psi
            {
                pressure = Resolved { value: Some(Value::Num(psi)), age_ms: None, stale: false, assumed: true };
            }
            let level_pct = node
                .level
                .as_ref()
                .map(|b| resolve(b, inputs, stale_ms))
                .and_then(|r| r.fresh().and_then(Value::number))
                .map(|v| v.clamp(0.0, 100.0));
            let status = node.status.as_ref().map(|b| resolve(b, inputs, stale_ms));
            (
                node.id.clone(),
                NodeState {
                    pressure,
                    level_pct,
                    online: status.as_ref().map(link_of),
                    age_ms: status.and_then(|s| s.age_ms),
                },
            )
        })
        .collect();
    let node_kind = |id: &str| topology.nodes.iter().find(|n| n.id == id).map(|n| n.kind);
    let fresh_psi = |id: &str| nodes.get(id).and_then(|n| n.pressure.fresh()).and_then(Value::number);
    let gate_of = |through: &[String]| combine(through.iter().map(|v| valves.get(v).map_or(Gate::Unknown, |s| s.flow_gate())));

    // Pipes into a junction are judged over the junction's outgoing pipe:
    // their valves plus its valves, against its destination's pressure.
    let junction_out: BTreeMap<&str, &super::topology::Edge> = topology
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Pipe && node_kind(&e.from) == Some(NodeKind::Junction))
        .map(|e| (e.from.as_str(), e))
        .collect();
    let mut edges: BTreeMap<String, EdgeState> = BTreeMap::new();
    let pipe_flow = |edge: &super::topology::Edge| -> (Flow, Option<f32>) {
        let (gate, downstream) = match junction_out.get(edge.to.as_str()) {
            Some(out) => (
                combine([gate_of(&edge.through), gate_of(&out.through)]),
                out.to.as_str(),
            ),
            None => (gate_of(&edge.through), edge.to.as_str()),
        };
        let dp = fresh_psi(&edge.from).zip(fresh_psi(downstream)).map(|(up, down)| up - down);
        let flow = match (gate, dp) {
            (Gate::Closed, _) => Flow::Blocked,
            (Gate::Unknown, _) | (Gate::Open, None) => Flow::Unknown,
            (Gate::Open, Some(dp)) if dp > edge.min_dp_psi => Flow::Flowing,
            (Gate::Open, Some(_)) => Flow::Static,
        };
        (flow, dp)
    };
    // Pass 1: every pipe not leaving a junction, links, and non-following pipes.
    for edge in topology.edges.iter().filter(|e| visible(e.when, igt)) {
        let state = match edge.kind {
            EdgeKind::Pipe if node_kind(&edge.from) == Some(NodeKind::Junction) || edge.follows.is_some() => continue,
            EdgeKind::Pipe => {
                let (flow, dp) = pipe_flow(edge);
                EdgeState { flow, fluids: edge.fluid.iter().cloned().collect(), link: None, link_age_ms: None, dp_psi: dp }
            }
            _ => {
                let health = edge.health.as_ref().map(|b| resolve(b, inputs, stale_ms));
                let link = health.as_ref().map_or(Link::Unknown, link_of);
                EdgeState {
                    flow: if link == Link::Up { Flow::Flowing } else { Flow::Unknown },
                    fluids: Vec::new(),
                    link: Some(link),
                    link_age_ms: health.and_then(|h| h.age_ms),
                    dp_psi: None,
                }
            }
        };
        edges.insert(edge.id.clone(), state);
    }
    // Pass 2: junction outputs take the best state of what flows in.
    for (junction, out) in &junction_out {
        if !visible(out.when, igt) {
            continue;
        }
        let incoming: Vec<&EdgeState> = topology
            .edges
            .iter()
            .filter(|e| e.to == *junction)
            .filter_map(|e| edges.get(&e.id))
            .collect();
        let flow = incoming.iter().map(|s| s.flow).max_by_key(|f| f.rank()).unwrap_or(Flow::Unknown);
        let mut fluids: Vec<String> = incoming
            .iter()
            .filter(|s| s.flow == flow && flow != Flow::Blocked)
            .flat_map(|s| s.fluids.iter().cloned())
            .collect();
        fluids.sort();
        fluids.dedup();
        let dp = fresh_psi(junction).or_else(|| incoming.iter().filter_map(|s| s.dp_psi).reduce(f32::max));
        edges.insert(out.id.clone(), EdgeState { flow, fluids, link: None, link_age_ms: None, dp_psi: dp });
    }
    // Pass 3: followers copy their leader's flow, keep their own fluid.
    for edge in topology.edges.iter().filter(|e| e.follows.is_some() && visible(e.when, igt)) {
        let leader = edge.follows.as_deref().and_then(|id| edges.get(id)).map(|s| s.flow).unwrap_or(Flow::Unknown);
        let flow = match (leader, gate_of(&edge.through)) {
            (_, Gate::Closed) => Flow::Blocked,
            (Flow::Flowing, Gate::Unknown) => Flow::Unknown,
            (leader, _) => leader,
        };
        edges.insert(
            edge.id.clone(),
            EdgeState { flow, fluids: edge.fluid.iter().cloned().collect(), link: None, link_age_ms: None, dp_psi: None },
        );
    }
    let readings = topology
        .readings
        .iter()
        .map(|reading| (reading.id.clone(), reading_state(reading, inputs, stale_ms)))
        .collect();
    DiagramState { igt, valves, edges, nodes, readings }
}

pub(crate) fn format_age(age_ms: i64) -> String {
    let s = age_ms.max(0) / 1000;
    if s < 60 {
        format!("{s} s")
    } else if s < 3600 {
        format!("{} min", s / 60)
    } else {
        format!("{} h", s / 3600)
    }
}

pub(crate) fn reading_state(reading: &super::topology::Reading, inputs: &dyn Inputs, stale_ms: i64) -> ReadingState {
    let resolved = resolve(&reading.value, inputs, reading.stale_ms.unwrap_or(stale_ms));
    let Some(value) = resolved.value.clone() else {
        return ReadingState {
            text: reading.missing_text.clone().unwrap_or_else(|| "—".into()),
            stale: false,
            missing: true,
            warn: false,
            assumed: false,
            age_ms: None,
            over_range: false,
        };
    };
    if let Value::OverRange(state) = value {
        // Never a number and never a stale number; no unit, no warn limits.
        return ReadingState {
            text: state.text().into(),
            stale: resolved.stale,
            missing: false,
            warn: false,
            assumed: false,
            age_ms: resolved.age_ms,
            over_range: true,
        };
    }
    let text = match (&value, reading.format) {
        (Value::Text(text), _) => text.clone(),
        (Value::OverRange(state), _) => state.text().into(),
        (v, Format::Bool) | (v @ Value::Bool(_), _) => {
            let on = v.truthy().unwrap_or(false);
            if on {
                reading.true_text.clone().unwrap_or_else(|| "On".into())
            } else {
                reading.false_text.clone().unwrap_or_else(|| "Off".into())
            }
        }
        (Value::Num(v), Format::Integer) => format!("{}", v.round() as i64),
        (Value::Num(v), _) => format!("{v:.prec$}", prec = reading.precision),
    };
    let text = if reading.unit.is_empty() || !matches!(value, Value::Num(_)) || reading.format == Format::Bool {
        text
    } else {
        format!("{text} {}", reading.unit)
    };
    let number = value.number().filter(|_| matches!(value, Value::Num(_)));
    let warn = number.is_some_and(|v| {
        reading.warn_above.is_some_and(|limit| v >= limit) || reading.warn_below.is_some_and(|limit| v <= limit)
    });
    ReadingState {
        text: if resolved.assumed { format!("≈ {text}") } else { text },
        stale: resolved.stale,
        missing: false,
        warn,
        assumed: resolved.assumed,
        age_ms: resolved.age_ms,
        over_range: false,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::topology;
    use super::*;
    use std::collections::HashMap;

    /// Fake station: telemetry by (type, index) with an age, controls, boards.
    #[derive(Default)]
    pub(crate) struct Fake {
        pub telemetry: HashMap<(String, usize), (f32, i64)>,
        pub controls: HashMap<String, ControlView>,
        pub boards: HashMap<String, i64>,
        pub statuses: HashMap<String, String>,
        pub igt: bool,
        pub over_range: HashMap<(String, usize), (crate::telemetry_dashboard::over_range::OverRange, i64)>,
    }
    impl Fake {
        pub fn set(&mut self, data_type: &str, index: usize, value: f32, age_ms: i64) -> &mut Self {
            self.telemetry.insert((data_type.into(), index), (value, age_ms));
            self
        }
        /// VALVE_STATE: [pilot, vent, dump, igniter, nitrogen, nitrous, retracted].
        pub fn valves(&mut self, states: [f32; 7], age_ms: i64) -> &mut Self {
            for (index, value) in states.into_iter().enumerate() {
                self.set("VALVE_STATE", index, value, age_ms);
            }
            self
        }
    }
    impl Inputs for Fake {
        fn telemetry(&self, data_type: &str, _sender: Option<&str>, index: usize) -> Option<Observed> {
            if let Some((state, age)) = self.over_range.get(&(data_type.to_string(), index)) {
                return Some(Observed { value: Value::OverRange(*state), age_ms: Some(*age) });
            }
            self.telemetry
                .get(&(data_type.to_string(), index))
                .map(|(v, age)| Observed { value: Value::Num(*v), age_ms: Some(*age) })
        }
        fn control(&self, cmd: &str) -> Option<ControlView> {
            self.controls.get(cmd).copied()
        }
        fn board(&self, sender: &str) -> Option<Observed> {
            self.boards.get(sender).map(|age| Observed { value: Value::Bool(true), age_ms: Some(*age) })
        }
        fn status(&self, key: &str) -> Option<Observed> {
            self.statuses.get(key).map(|t| Observed { value: Value::Text(t.clone()), age_ms: None })
        }
        fn igt(&self) -> bool {
            self.igt
        }
    }

    /// The flight view (normal, HITL, testing): the fill system and the vehicle.
    pub(crate) fn fill() -> Topology {
        topology::parse(topology::bundled_text("flight")).unwrap()
    }

    /// The three station-mode views; each carries the whole fill system.
    pub(crate) fn mode_views() -> Vec<Topology> {
        ["flight", "test_fire", "ground_test"].iter().map(|id| topology::parse(topology::bundled_text(id)).unwrap()).collect()
    }

    const PILOT: usize = 0;
    const VENT: usize = 1;
    const N2: usize = 4;

    #[test]
    fn open_path_with_a_differential_flows_and_a_closed_valve_blocks() {
        for topology in mode_views() {
            open_path_flows_in(&topology);
        }
    }

    fn open_path_flows_in(topology: &Topology) {
        let topology = topology.clone();
        let mut fake = Fake::default();
        // Nitrogen open, QD connected (not retracted), tank at 200 psi below
        // the 1000 psi regulator set point.
        fake.valves([0., 0., 0., 0., 1., 0., 0.], 100).set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 200.0, 100);
        let state = derive(&topology, &fake);
        assert_eq!(state.valves["nitrogen"], ValveState::Open);
        assert_eq!(state.edges["n2_branch"].flow, Flow::Flowing);
        assert_eq!(state.edges["fill_line"].flow, Flow::Flowing);
        assert_eq!(state.edges["fill_line"].fluids, vec!["n2".to_string()]);
        assert_eq!(state.edges["n2_supply"].flow, Flow::Flowing, "the bottle stub follows its branch");
        assert_eq!(state.edges["n2o_branch"].flow, Flow::Blocked);
        assert_eq!(state.edges["vent_line"].flow, Flow::Blocked);

        // Closing nitrogen blocks the whole supply path.
        fake.set("VALVE_STATE", N2, 0.0, 100);
        let state = derive(&topology, &fake);
        assert_eq!(state.edges["n2_branch"].flow, Flow::Blocked);
        assert_eq!(state.edges["fill_line"].flow, Flow::Blocked);
        assert!(state.edges["fill_line"].fluids.is_empty());
    }

    #[test]
    fn open_path_without_a_differential_is_static_not_flowing() {
        let topology = fill();
        let mut fake = Fake::default();
        fake.valves([0., 0., 0., 0., 1., 0., 0.], 100).set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 998.0, 100);
        assert_eq!(derive(&topology, &fake).edges["n2_branch"].flow, Flow::Static);
        // Vent open to the atmosphere at 500 psi: flowing vapour.
        fake.set("VALVE_STATE", VENT, 1.0, 100).set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 500.0, 100);
        let state = derive(&topology, &fake);
        assert_eq!(state.edges["vent_line"].flow, Flow::Flowing);
        // An empty tank at ambient venting: no differential.
        fake.set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 1.0, 100);
        assert_eq!(derive(&topology, &fake).edges["vent_line"].flow, Flow::Static);
    }

    #[test]
    fn retracted_qd_blocks_the_fill_line_even_with_supply_open() {
        let topology = fill();
        let mut fake = Fake::default();
        fake.valves([0., 0., 0., 0., 1., 0., 1.], 100).set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 200.0, 100);
        let state = derive(&topology, &fake);
        assert_eq!(state.valves["qd"], ValveState::Closed);
        assert_eq!(state.edges["n2_branch"].flow, Flow::Blocked);
        assert_eq!(state.edges["fill_line"].flow, Flow::Blocked);
    }

    #[test]
    fn stale_data_greys_out_with_its_age_and_never_animates() {
        let topology = fill();
        let mut fake = Fake::default();
        fake.valves([0., 0., 0., 0., 1., 0., 0.], 9_000).set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 200.0, 100);
        let state = derive(&topology, &fake);
        assert_eq!(state.valves["nitrogen"], ValveState::Stale { was_open: true, age_ms: 9_000 });
        assert_eq!(state.edges["n2_branch"].flow, Flow::Unknown);
        assert_eq!(state.edges["fill_line"].flow, Flow::Unknown);
        // A stale tank PT makes the differential unknown.
        fake.valves([0., 0., 0., 0., 1., 0., 0.], 100).set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 200.0, 12_000);
        let state = derive(&topology, &fake);
        assert_eq!(state.edges["n2_branch"].flow, Flow::Unknown);
        let pt = &state.readings["tank_pt"];
        assert!(pt.stale && pt.text.starts_with("200 psi"), "{pt:?}");
        assert_eq!(format_age(pt.age_ms.unwrap()), "12 s");
        // Never reported: unknown valves and a missing reading.
        let state = derive(&topology, &Fake::default());
        assert_eq!(state.valves["vent"], ValveState::Unknown);
        assert!(state.readings["tank_pt"].missing);
        assert_eq!(state.edges["vent_line"].flow, Flow::Unknown);
    }

    #[test]
    fn commanded_but_unconfirmed_is_pending() {
        let topology = fill();
        let mut fake = Fake::default();
        fake.valves([0., 0., 0., 0., 0., 0., 0.], 100);
        fake.controls.insert("Pilot".into(), ControlView { actuated: Some(false), requested: Some(true), running: false });
        let state = derive(&topology, &fake);
        assert_eq!(state.valves["main"], ValveState::Pending { to_open: true, from: Some(false) });
        assert_eq!(state.edges["feed_line"].flow, Flow::Blocked, "a pending open is still closed");
        // Confirmation arrives: open.
        fake.set("VALVE_STATE", PILOT, 1.0, 50);
        assert_eq!(derive(&topology, &fake).valves["main"], ValveState::Open);
        // A running QD retraction shows the QD pending towards disconnected.
        fake.controls.insert("RetractPlumbing".into(), ControlView { actuated: Some(false), requested: None, running: true });
        assert_eq!(derive(&topology, &fake).valves["qd"], ValveState::Pending { to_open: false, from: Some(true) });
        // RetractPlumbing requested=true is the QD's closed position.
        fake.controls.insert("RetractPlumbing".into(), ControlView { actuated: Some(false), requested: Some(true), running: false });
        assert_eq!(derive(&topology, &fake).valves["qd"], ValveState::Pending { to_open: false, from: Some(true) });
    }

    #[test]
    fn both_supplies_flowing_merge_into_one_line_with_both_fluids() {
        let topology = fill();
        let mut fake = Fake::default();
        fake.valves([0., 0., 0., 0., 1., 1., 0.], 100).set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 300.0, 100);
        let state = derive(&topology, &fake);
        assert_eq!(state.edges["fill_line"].flow, Flow::Flowing);
        assert_eq!(state.edges["fill_line"].fluids, vec!["n2".to_string(), "n2o_liquid".to_string()]);
    }

    #[test]
    fn chamber_without_a_pt_reading_is_assumed_ambient() {
        // The flight view has no chamber PT at all: always ambient.
        let mut fake = Fake::default();
        fake.valves([1., 0., 0., 0., 0., 0., 0.], 100)
            .set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 700.0, 100)
            .set("COMBUSTION_CHAMBER_PRESSURE", 0, 699.0, 100);
        let state = derive(&fill(), &fake);
        assert!(state.nodes["chamber"].pressure.assumed);
        assert_eq!(state.edges["feed_line"].flow, Flow::Flowing);
        let topology = topology::parse(topology::bundled_text("test_fire")).unwrap();
        let mut fake = Fake::default();
        fake.valves([1., 0., 0., 0., 0., 0., 0.], 100).set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 700.0, 100);
        let state = derive(&topology, &fake);
        assert!(state.nodes["chamber"].pressure.assumed);
        assert_eq!(state.edges["feed_line"].flow, Flow::Flowing);
        fake.set("COMBUSTION_CHAMBER_PRESSURE", 0, 699.0, 100);
        assert_eq!(derive(&topology, &fake).edges["feed_line"].flow, Flow::Static);
    }

    #[test]
    fn readings_format_warn_and_igt_visibility() {
        let topology = fill();
        let mut fake = Fake::default();
        fake.set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 900.0, 10);
        let state = derive(&topology, &fake);
        assert!(state.readings["tank_pt"].warn, "at or above the 850 psi relief");
        assert!(visible(Some(When::Igt), true) && !visible(Some(When::Igt), false));
        assert!(visible(Some(When::NotIgt), false) && visible(None, true));
    }

    #[test]
    fn links_follow_board_freshness() {
        let topology = fill();
        let mut fake = Fake::default();
        fake.boards.insert("FC".into(), 500);
        fake.boards.insert("PB".into(), 60_000);
        let state = derive(&topology, &fake);
        let fc = topology.edges.iter().find(|e| e.health.as_ref().and_then(|h| h.board.as_deref()) == Some("FC")).unwrap();
        let pb = topology.edges.iter().find(|e| e.health.as_ref().and_then(|h| h.board.as_deref()) == Some("PB")).unwrap();
        assert_eq!(state.edges[&fc.id].link, Some(Link::Up));
        assert_eq!(state.edges[&pb.id].link, Some(Link::Down));
        assert_eq!(state.edges[&pb.id].flow, Flow::Unknown);
    }

    /// One view per mode keeps each system's own staleness: the fill boards
    /// at 5 s, the AV bay's radio data at 10 s (binding `stale_ms`).
    #[test]
    fn the_flight_view_keeps_the_av_bay_radio_staleness() {
        let topology = fill();
        let mut fake = Fake::default();
        for board in ["FC", "VB"] {
            fake.boards.insert(board.into(), 7_000);
        }
        fake.set("BATTERY_CURRENT", 0, 0.8, 7_000).set("PRESSURE_TRANSDUCER_CALIBRATED", 0, 300.0, 7_000);
        let state = derive(&topology, &fake);
        assert_eq!(state.nodes["fc"].online, Some(Link::Up), "AV bay: fresh for 10 s");
        assert_eq!(state.nodes["vb"].online, Some(Link::Down), "fill: stale after 5 s");
        assert!(!state.readings["pb_i"].stale && state.readings["tank_pt"].stale);
        fake.boards.insert("FC".into(), 11_000);
        assert_eq!(derive(&topology, &fake).nodes["fc"].online, Some(Link::Down));
    }
}
