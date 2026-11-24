//! Declarative live-diagram topology (`docs/live-diagrams.md`).
//!
//! A topology says what is drawn where and which telemetry drives it; it holds
//! no rendering code. The station serves the editable copies from its
//! `backend/layout/diagrams/` (`GET /api/diagrams`); the copies bundled here
//! are the fallback for a station that does not serve them yet.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const SCHEMA: u32 = 1;

fn default_stale_ms() -> i64 {
    5_000
}
fn default_node_w() -> f32 {
    90.0
}
fn default_node_h() -> f32 {
    50.0
}
fn default_min_dp() -> f32 {
    5.0
}
fn default_scale() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Topology {
    pub schema: u32,
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// `[min_x, min_y, width, height]` of the SVG coordinate space.
    pub view_box: [f32; 4],
    /// Readings older than this are drawn stale (greyed with their age).
    #[serde(default = "default_stale_ms")]
    pub stale_ms: i64,
    #[serde(default)]
    pub fluids: Vec<Fluid>,
    #[serde(default)]
    pub frames: Vec<Frame>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub valves: Vec<Valve>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub readings: Vec<Reading>,
    /// Where the topology was derived from (repo paths), for reviewers.
    #[serde(default)]
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fluid {
    pub id: String,
    pub label: String,
    /// Categorical colour slot 1..=3 (validated colour-blind safe as a set).
    pub slot: u8,
    /// Gas/vapour lines are dashed as a second, colour-independent cue.
    #[serde(default)]
    pub dashed: bool,
}

/// When an element is shown. Absent means always.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum When {
    /// The station is in an integrated ground test (motor on the stand, FC replaying).
    Igt,
    NotIgt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Frame {
    pub id: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub when: Option<When>,
    /// Draws the station's vehicle (GET /api/vehicle/geometry) true to scale in this box.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vehicle: Option<FrameVehicle>,
}

/// The vehicle's side profile, nose up, fitted into `x, y, w, h` at its real
/// proportions; `highlight` names section roles to emphasise (`avionics`,
/// `recovery`, `tank`, `motor`, `payload`, `nose`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FrameVehicle {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub highlight: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NodeKind {
    Bottle,
    Regulator,
    /// A tee where branches merge; exactly one outgoing pipe.
    Junction,
    Tank,
    Chamber,
    Atmosphere,
    Board,
    Sensor,
    Battery,
    Station,
    /// A data bus (CAN) drawn as a bar.
    Bus,
    LoadCell,
    Pyro,
    Output,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
    #[serde(default)]
    pub sublabel: String,
    pub x: f32,
    pub y: f32,
    #[serde(default = "default_node_w")]
    pub w: f32,
    #[serde(default = "default_node_h")]
    pub h: f32,
    /// Pressure (psig) used for the flow rule. A `constant` with `assumed`
    /// marks a nominal value (a regulator set point, a vapour pressure).
    #[serde(default)]
    pub pressure: Option<Binding>,
    /// Pressure used only when `pressure` has no fresh value (the chamber is
    /// open to the atmosphere through the nozzle).
    #[serde(default)]
    pub fallback_psi: Option<f32>,
    /// Liquid level 0..100 % (tanks).
    #[serde(default)]
    pub level: Option<Binding>,
    /// Online/health (boards, stations): truthy and fresh means up.
    #[serde(default)]
    pub status: Option<Binding>,
    #[serde(default)]
    pub fluid: Option<String>,
    #[serde(default)]
    pub when: Option<When>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ValveKind {
    Solenoid,
    Servo,
    /// The quick disconnect: open = connected.
    Qd,
    /// A hand valve the station does not monitor (bottle valves).
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OpenWhen {
    /// Value >= 0.5 means open.
    #[default]
    High,
    /// Value < 0.5 means open (the QD state reports "retracted").
    Low,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Valve {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub sublabel: String,
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub vertical: bool,
    pub kind: ValveKind,
    /// Confirmed position. Absent only for `manual` valves.
    #[serde(default)]
    pub state: Option<Binding>,
    #[serde(default)]
    pub open_when: OpenWhen,
    /// Action-policy command whose `requested` (pending) and `progress`
    /// give the commanded state.
    #[serde(default)]
    pub command: Option<String>,
    /// Text for the open/closed states (QD: Connected/Disconnected).
    #[serde(default)]
    pub open_text: Option<String>,
    #[serde(default)]
    pub closed_text: Option<String>,
    #[serde(default)]
    pub when: Option<When>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EdgeKind {
    #[default]
    Pipe,
    Can,
    Radio,
    Uart,
    Ethernet,
    Power,
    Mechanical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub kind: EdgeKind,
    /// Valves on the path; flow needs every one open.
    #[serde(default)]
    pub through: Vec<String>,
    /// A fluid id, or `inherit` (the fluids flowing into `from`).
    #[serde(default)]
    pub fluid: Option<String>,
    /// Polyline in view-box coordinates.
    pub points: Vec<[f32; 2]>,
    /// Minimum upstream-downstream difference (psi) that counts as flow.
    #[serde(default = "default_min_dp")]
    pub min_dp_psi: f32,
    /// Takes the flow state of another pipe (the bottle-to-regulator stub
    /// follows the branch it feeds).
    #[serde(default)]
    pub follows: Option<String>,
    /// Links: truthy and fresh means up.
    #[serde(default)]
    pub health: Option<Binding>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub when: Option<When>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Format {
    #[default]
    Number,
    Integer,
    Bool,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Anchor {
    #[default]
    Start,
    Middle,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reading {
    pub id: String,
    pub label: String,
    pub x: f32,
    pub y: f32,
    pub value: Binding,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub precision: usize,
    #[serde(default)]
    pub format: Format,
    #[serde(default)]
    pub true_text: Option<String>,
    #[serde(default)]
    pub false_text: Option<String>,
    /// Text when the source has never reported (default "—").
    #[serde(default)]
    pub missing_text: Option<String>,
    #[serde(default)]
    pub stale_ms: Option<i64>,
    #[serde(default)]
    pub warn_above: Option<f32>,
    #[serde(default)]
    pub warn_below: Option<f32>,
    #[serde(default)]
    pub anchor: Anchor,
    #[serde(default)]
    pub when: Option<When>,
}

/// Where a value comes from. Exactly one of `telemetry`, `constant`,
/// `board`, `control` and `status` is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    /// A telemetry data type; `sender` narrows it, `index` picks the channel.
    #[serde(default)]
    pub telemetry: Option<String>,
    #[serde(default)]
    pub sender: Option<String>,
    #[serde(default)]
    pub index: usize,
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default)]
    pub constant: Option<f32>,
    /// Board sender id: up while the station hears it.
    #[serde(default)]
    pub board: Option<String>,
    /// Action-policy command: its confirmed `actuated` state.
    #[serde(default)]
    pub control: Option<String>,
    /// A named station status (see `derive::STATUS_KEYS`).
    #[serde(default)]
    pub status: Option<String>,
    /// The value is nominal, not measured (drawn with "≈").
    #[serde(default)]
    pub assumed: bool,
}

impl Binding {
    fn source_count(&self) -> usize {
        [
            self.telemetry.is_some(),
            self.constant.is_some(),
            self.board.is_some(),
            self.control.is_some(),
            self.status.is_some(),
        ]
        .into_iter()
        .filter(|set| *set)
        .count()
    }
}

/// Every problem found, so an editor can fix a file in one pass.
pub(crate) fn validate(topology: &Topology) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let mut err = |message: String| errors.push(message);
    if topology.schema != SCHEMA {
        err(format!("schema {} is not supported (expected {SCHEMA})", topology.schema));
    }
    if topology.id.trim().is_empty() || topology.title.trim().is_empty() {
        err("id and title are required".into());
    }
    let [_, _, width, height] = topology.view_box;
    if !(width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite()) {
        err("view_box width and height must be positive".into());
    }
    if topology.stale_ms <= 0 {
        err("stale_ms must be positive".into());
    }
    let mut ids = BTreeSet::new();
    let mut unique = |kind: &str, id: &str, err: &mut dyn FnMut(String)| {
        if id.trim().is_empty() {
            err(format!("{kind} with an empty id"));
        } else if !ids.insert(id.to_string()) {
            err(format!("duplicate id `{id}`"));
        }
    };
    let fluids: BTreeSet<&str> = topology.fluids.iter().map(|f| f.id.as_str()).collect();
    for fluid in &topology.fluids {
        unique("fluid", &fluid.id, &mut err);
        if !(1..=3).contains(&fluid.slot) {
            err(format!("fluid `{}`: slot must be 1..=3", fluid.id));
        }
        if fluid.id == "inherit" {
            err("`inherit` is reserved".into());
        }
    }
    let check_binding = |owner: &str, binding: &Binding, err: &mut dyn FnMut(String)| {
        if binding.source_count() != 1 {
            err(format!("{owner}: a binding needs exactly one of telemetry, constant, board, control, status"));
        }
        if binding.sender.is_some() && binding.telemetry.is_none() {
            err(format!("{owner}: sender is only valid with telemetry"));
        }
        if !binding.scale.is_finite() || binding.scale == 0.0 {
            err(format!("{owner}: scale must be finite and non-zero"));
        }
        if let Some(key) = &binding.status
            && !super::derive::status_key_known(key)
        {
            err(format!("{owner}: unknown status `{key}`"));
        }
    };
    for frame in &topology.frames {
        unique("frame", &frame.id, &mut err);
        if let Some(v) = &frame.vehicle
            && !([v.x, v.y, v.w, v.h].iter().all(|n| n.is_finite()) && v.w > 0.0 && v.h > 0.0)
        {
            err(format!("frame `{}`: vehicle w and h must be positive", frame.id));
        }
    }
    let mut nodes: BTreeMap<&str, &Node> = BTreeMap::new();
    for node in &topology.nodes {
        unique("node", &node.id, &mut err);
        nodes.insert(node.id.as_str(), node);
        if !(node.w > 0.0 && node.h > 0.0) {
            err(format!("node `{}`: w and h must be positive", node.id));
        }
        for (what, binding) in [("pressure", &node.pressure), ("level", &node.level), ("status", &node.status)] {
            if let Some(binding) = binding {
                check_binding(&format!("node `{}` {what}", node.id), binding, &mut err);
            }
        }
        if let Some(fluid) = &node.fluid
            && !fluids.contains(fluid.as_str())
        {
            err(format!("node `{}`: unknown fluid `{fluid}`", node.id));
        }
    }
    let mut valves = BTreeSet::new();
    for valve in &topology.valves {
        unique("valve", &valve.id, &mut err);
        valves.insert(valve.id.as_str());
        match (&valve.state, valve.kind) {
            (None, ValveKind::Manual) => {}
            (None, _) => err(format!("valve `{}`: state is required unless kind is manual", valve.id)),
            (Some(binding), _) => check_binding(&format!("valve `{}`", valve.id), binding, &mut err),
        }
    }
    let edge_ids: BTreeSet<&str> = topology.edges.iter().map(|e| e.id.as_str()).collect();
    let mut junction_out: BTreeMap<&str, usize> = BTreeMap::new();
    for edge in &topology.edges {
        unique("edge", &edge.id, &mut err);
        for end in [&edge.from, &edge.to] {
            if !nodes.contains_key(end.as_str()) {
                err(format!("edge `{}`: unknown node `{end}`", edge.id));
            }
        }
        if edge.points.len() < 2 {
            err(format!("edge `{}`: needs at least two points", edge.id));
        }
        for valve in &edge.through {
            if !valves.contains(valve.as_str()) {
                err(format!("edge `{}`: unknown valve `{valve}`", edge.id));
            }
        }
        match (edge.kind, &edge.fluid) {
            (EdgeKind::Pipe, None) => err(format!("edge `{}`: a pipe needs a fluid", edge.id)),
            (EdgeKind::Pipe, Some(fluid)) if fluid != "inherit" && !fluids.contains(fluid.as_str()) => {
                err(format!("edge `{}`: unknown fluid `{fluid}`", edge.id))
            }
            (EdgeKind::Pipe, _) => {}
            (_, Some(_)) => err(format!("edge `{}`: only pipes carry a fluid", edge.id)),
            (_, None) => {}
        }
        if edge.kind != EdgeKind::Pipe && !edge.through.is_empty() {
            err(format!("edge `{}`: only pipes pass through valves", edge.id));
        }
        if let Some(follows) = &edge.follows
            && (!edge_ids.contains(follows.as_str()) || follows == &edge.id)
        {
            err(format!("edge `{}`: follows unknown edge `{follows}`", edge.id));
        }
        if let Some(health) = &edge.health {
            check_binding(&format!("edge `{}` health", edge.id), health, &mut err);
        }
        if edge.kind == EdgeKind::Pipe
            && nodes.get(edge.from.as_str()).is_some_and(|n| n.kind == NodeKind::Junction)
        {
            *junction_out.entry(edge.from.as_str()).or_default() += 1;
        }
        if edge.fluid.as_deref() == Some("inherit")
            && !nodes.get(edge.from.as_str()).is_some_and(|n| n.kind == NodeKind::Junction)
        {
            err(format!("edge `{}`: `inherit` needs a junction upstream", edge.id));
        }
    }
    for node in topology.nodes.iter().filter(|n| n.kind == NodeKind::Junction) {
        if junction_out.get(node.id.as_str()).copied().unwrap_or(0) != 1 {
            err(format!("junction `{}`: needs exactly one outgoing pipe", node.id));
        }
    }
    for reading in &topology.readings {
        unique("reading", &reading.id, &mut err);
        check_binding(&format!("reading `{}`", reading.id), &reading.value, &mut err);
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

pub(crate) fn parse(text: &str) -> Result<Topology, Vec<String>> {
    let topology: Topology = serde_json::from_str(text).map_err(|e| vec![e.to_string()])?;
    validate(&topology)?;
    Ok(topology)
}

/// The diagrams shipped with the client, in tab order.
pub(crate) const BUNDLED: &[(&str, &str)] = &[
    ("fill", include_str!("topologies/fill.json")),
    ("av_bay", include_str!("topologies/av_bay.json")),
    ("network", include_str!("topologies/network.json")),
    ("stand", include_str!("topologies/stand.json")),
];

pub(crate) fn bundled() -> Vec<Topology> {
    BUNDLED
        .iter()
        .filter_map(|(_, text)| parse(text).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_topology_parses_and_validates() {
        for (id, text) in BUNDLED {
            let topology = parse(text).unwrap_or_else(|errors| panic!("{id}: {errors:#?}"));
            assert_eq!(&topology.id, id);
        }
    }

    #[test]
    fn validation_reports_every_broken_reference() {
        let mut topology = parse(BUNDLED[0].1).unwrap();
        topology.edges[0].through.push("no_such_valve".into());
        topology.edges[1].to = "nowhere".into();
        topology.readings[0].value.constant = Some(1.0); // two sources
        topology.valves[1].state = None; // a monitored valve without state
        topology.nodes.push(topology.nodes[0].clone()); // duplicate id
        let errors = validate(&topology).unwrap_err();
        for needle in ["no_such_valve", "unknown node `nowhere`", "exactly one of", "state is required", "duplicate id"] {
            assert!(errors.iter().any(|e| e.contains(needle)), "{needle} missing in {errors:#?}");
        }
    }

    #[test]
    fn junction_rules_and_fluids_are_enforced() {
        let mut topology = parse(BUNDLED[0].1).unwrap();
        let out = topology.edges.iter().position(|e| e.fluid.as_deref() == Some("inherit")).unwrap();
        let mut second = topology.edges[out].clone();
        second.id = "second_out".into();
        topology.edges.push(second);
        topology.fluids[0].slot = 7;
        let errors = validate(&topology).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("exactly one outgoing pipe")), "{errors:#?}");
        assert!(errors.iter().any(|e| e.contains("slot must be 1..=3")), "{errors:#?}");
    }

    #[test]
    fn unknown_fields_and_schema_versions_are_refused() {
        let text = BUNDLED[0].1.replacen("\"schema\": 1", "\"schema\": 2", 1);
        assert!(parse(&text).unwrap_err()[0].contains("schema 2"));
        let text = BUNDLED[0].1.replacen("{", "{\"bogus\": true,", 1);
        assert!(parse(&text).unwrap_err()[0].contains("bogus"));
        let text = BUNDLED[0].1.replacen("\"status\": \"fill_phase\"", "\"status\": \"made_up\"", 1);
        assert!(parse(&text).unwrap_err().iter().any(|e| e.contains("unknown status `made_up`")));
    }
}
