//! A station answered on this device, for use with no Ground Station
//! connected: the demo scenario ([`demo`]) and a replay file opened from disk
//! ([`replay_file`]). Both become one [`Session`]: a layout, board list and
//! time-ordered samples in the shapes a Ground Station serves. The offline
//! workspace reads them through [`Session::get`], which answers the same
//! routes and JSON as the station (`docs/backend-contract.md`), so the demo
//! is also a working reference for the contract.
//!
//! Nothing here is reachable from the live dashboard: a session exists only
//! while the offline workspace is mounted and is dropped with it. The live
//! transport (`dashboard_transport.rs`) never consults it.
pub(crate) mod bundle;
pub(crate) mod demo;
pub(crate) mod replay_file;

use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::cell::Cell;

thread_local! {
    /// Local sessions alive on this (UI) thread. Zero whenever the offline
    /// workspace is not mounted; the dashboard route asserts it in debug
    /// builds and the tests check it after unmounting.
    static LIVE_SESSIONS: Cell<usize> = const { Cell::new(0) };
}

#[cfg_attr(not(any(test, debug_assertions)), allow(dead_code))]
pub(crate) fn live_sessions() -> usize {
    LIVE_SESSIONS.with(Cell::get)
}

/// One telemetry row as the station stores and replays it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Sample {
    pub t_ms: i64,
    pub sender: String,
    pub data_type: String,
    pub values: Vec<Option<f32>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Flight,
    TestFire,
    GroundTest,
    Hitl,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Flight => "Flight",
            Mode::TestFire => "Test fire",
            Mode::GroundTest => "Integrated ground test",
            Mode::Hitl => "Hardware in the loop",
        }
    }
    /// The `mode` the offline 3D player understands.
    pub fn player_mode(self) -> &'static str {
        if self == Mode::TestFire { "test_fire" } else { "flight" }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Source {
    Demo,
    Replay { file_name: String },
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Board {
    pub sender: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Alert {
    pub t_ms: i64,
    pub warning: bool,
    pub message: String,
}

/// One plotted/displayed channel: `values[index]` of `data_type` rows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Channel {
    pub group: String,
    pub data_type: String,
    pub sender: Option<String>,
    pub index: usize,
    pub label: String,
    pub unit: String,
    pub precision: usize,
}

impl Channel {
    pub fn format(&self, value: f32) -> String {
        let unit = if self.unit.is_empty() { String::new() } else { format!(" {}", self.unit) };
        format!("{value:.prec$}{unit}", prec = self.precision)
    }
}

/// Calibration capture points (expected vs. raw input) for one channel.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CalibrationCurve {
    pub label: String,
    pub raw_unit: String,
    pub unit: String,
    pub points: Vec<(f32, f32)>,
    pub slope: f32,
    pub offset: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Recording {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub bytes: u64,
    pub start_ms: i64,
    pub end_ms: i64,
}

pub(crate) struct Session {
    pub source: Source,
    pub title: String,
    pub mode: Mode,
    pub provenance: String,
    /// `GET /api/layout` as the station (or the file) supplied it.
    pub layout: Value,
    pub boards: Vec<Board>,
    pub samples: Vec<Sample>,
    pub states: Vec<(i64, String)>,
    pub alerts: Vec<Alert>,
    pub calibration: Vec<CalibrationCurve>,
    pub recordings: Vec<Recording>,
    /// The portable replay document for the 3D player (replay files only;
    /// the demo builds one on demand).
    pub replay_document: Option<Value>,
    /// Vehicle geometry carried by the file (`GET /api/vehicle/geometry`).
    pub vehicle_geometry: Option<Value>,
    pub notes: Vec<String>,
    /// Wall-clock origin added to sample times in served JSON.
    pub epoch_ms: i64,
    /// Station diagram topologies (`GET /api/diagrams` `diagrams`), from a bundle.
    pub diagrams: Vec<Value>,
    /// Derived figures (flight review, burn report), from a bundle.
    pub derived: Vec<(String, String)>,
    /// Map tiles by (z, x, y), from a bundle.
    pub tiles: BTreeMap<(u32, u32, u32), Vec<u8>>,
    /// The recording's `GET /api/recordings/{id}/csv`, from a bundle.
    pub csv: Option<std::rc::Rc<Vec<u8>>>,
    /// A bundle's `manifest.json`.
    pub manifest: Option<Value>,
    /// Set when the file is a prediction (`replay.predicted` or
    /// `manifest.predicted`): the label the workspace shows ("Predicted: …").
    pub predicted: Option<String>,
    /// Flight plans the bundle carries (`derived/flight_plan.json`,
    /// `derived/flight_plans.json`), for drawing a prediction beside the data.
    pub plans: Vec<super::flight_plans::PlanDoc>,
    /// A bundle's `terrain/heightmap.json` (`GET /api/terrain/heightmap`).
    pub terrain: Option<Value>,
    /// A bundle's `derived/trajectory.json` with its flight phases.
    pub trajectory: Option<Value>,
    index: BTreeMap<(String, String), Vec<usize>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        LIVE_SESSIONS.with(|n| n.set(n.get().saturating_sub(1)));
    }
}

pub(crate) struct SessionParts {
    pub source: Source,
    pub title: String,
    pub mode: Mode,
    pub provenance: String,
    pub layout: Value,
    pub boards: Vec<Board>,
    pub samples: Vec<Sample>,
    pub states: Vec<(i64, String)>,
    pub alerts: Vec<Alert>,
    pub calibration: Vec<CalibrationCurve>,
    pub recordings: Vec<Recording>,
    pub replay_document: Option<Value>,
    pub vehicle_geometry: Option<Value>,
    pub notes: Vec<String>,
    pub epoch_ms: i64,
}

/// Channels a session can show, from the layout's data tabs.
const GPS_TYPES: &[&str] = &["GPS_DATA", "GPS"];

impl Session {
    pub fn new(parts: SessionParts) -> Self {
        let SessionParts { source, title, mode, provenance, layout, mut boards, mut samples, mut states, alerts, calibration, recordings, replay_document, vehicle_geometry, notes, epoch_ms } = parts;
        samples.sort_by_key(|s| s.t_ms);
        states.sort_by_key(|s| s.0);
        let mut index: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
        for (i, s) in samples.iter().enumerate() {
            index.entry((s.data_type.clone(), s.sender.clone())).or_default().push(i);
        }
        for (_, sender) in index.keys() {
            if !sender.is_empty() && !boards.iter().any(|b| &b.sender == sender) {
                boards.push(Board { sender: sender.clone(), label: sender.clone() });
            }
        }
        LIVE_SESSIONS.with(|n| n.set(n.get() + 1));
        Self {
            source, title, mode, provenance, layout, boards, samples, states, alerts, calibration, recordings, replay_document, vehicle_geometry, notes, epoch_ms,
            diagrams: Vec::new(), derived: Vec::new(), tiles: BTreeMap::new(), csv: None, manifest: None, predicted: None, plans: Vec::new(), terrain: None, trajectory: None, index,
        }
    }

    pub fn start_ms(&self) -> i64 {
        self.samples.first().map(|s| s.t_ms).unwrap_or(0)
    }
    pub fn end_ms(&self) -> i64 {
        self.samples.last().map(|s| s.t_ms).unwrap_or(0)
    }
    /// Whether any row of `data_type` was recorded.
    pub fn has_type(&self, data_type: &str) -> bool {
        self.index.keys().any(|(t, _)| t == data_type)
    }
    pub fn is_demo(&self) -> bool {
        self.source == Source::Demo
    }
    /// Marks the session as predicted data with the file's label.
    pub fn mark_predicted(&mut self, label: Option<&str>) {
        self.predicted = Some(super::flight_plans::predicted_label(label.unwrap_or("")));
        self.provenance = "Predicted (flight plan, not a recording)".into();
    }

    fn rows(&self, data_type: &str, sender: Option<&str>) -> impl Iterator<Item = &Vec<usize>> {
        self.index.iter().filter(move |((ty, s), _)| ty == data_type && sender.is_none_or(|want| want == s)).map(|(_, v)| v)
    }

    /// The newest row of `data_type` (from `sender`, if given) at or before `at`.
    pub fn latest(&self, data_type: &str, sender: Option<&str>, at: i64) -> Option<&Sample> {
        self.rows(data_type, sender)
            .filter_map(|rows| {
                let n = rows.partition_point(|&i| self.samples[i].t_ms <= at);
                n.checked_sub(1).map(|k| &self.samples[rows[k]])
            })
            .max_by_key(|s| s.t_ms)
    }

    pub fn value(&self, channel: &Channel, at: i64) -> Option<(f32, i64)> {
        let row = self.latest(&channel.data_type, channel.sender.as_deref(), at)?;
        Some((row.values.get(channel.index).copied().flatten()?, at - row.t_ms))
    }

    /// Up to `max_points` evenly thinned points of a channel in `[from, to]`.
    pub fn series(&self, channel: &Channel, from: i64, to: i64, max_points: usize) -> Vec<(i64, f32)> {
        let mut points: Vec<(i64, f32)> = Vec::new();
        for rows in self.rows(&channel.data_type, channel.sender.as_deref()) {
            let lo = rows.partition_point(|&i| self.samples[i].t_ms < from);
            let hi = rows.partition_point(|&i| self.samples[i].t_ms <= to);
            points.extend(rows[lo..hi].iter().filter_map(|&i| {
                let s = &self.samples[i];
                s.values.get(channel.index).copied().flatten().filter(|v| v.is_finite()).map(|v| (s.t_ms, v))
            }));
        }
        points.sort_by_key(|p| p.0);
        if points.len() > max_points.max(2) {
            let step = points.len() as f64 / max_points as f64;
            points = (0..max_points).map(|k| points[((k as f64) * step) as usize]).collect();
        }
        points
    }

    pub fn state_at(&self, at: i64) -> String {
        let n = self.states.partition_point(|s| s.0 <= at);
        n.checked_sub(1).map(|k| self.states[k].1.clone()).unwrap_or_else(|| "Unknown".into())
    }

    /// Every channel the layout names that has samples, then any other
    /// recorded type, so a file without a layout still shows its data.
    pub fn channels(&self) -> Vec<Channel> {
        let mut out = Vec::new();
        let has = |ty: &str| self.index.keys().any(|(t, _)| t == ty);
        let mut named = std::collections::BTreeSet::new();
        for tab in self.layout["data_tab"]["tabs"].as_array().into_iter().flatten() {
            let Some(ty) = tab["id"].as_str() else { continue };
            if !has(ty) {
                continue;
            }
            named.insert(ty.to_string());
            let group = tab["label"].as_str().unwrap_or(ty).to_string();
            // One set of channels per sender: the layout's subtabs, else the
            // recorded senders of a `sender_split_data_types` type.
            let mut senders: Vec<(Option<String>, String, &Value)> = tab["subtabs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|sub| sub["data_type"].as_str().is_none_or(|t| t == ty))
                .filter_map(|sub| Some((Some(sub["sender_id"].as_str()?.to_string()), format!("{group} · {}", sub["label"].as_str().unwrap_or("")), sub)))
                .collect();
            let split = self.layout["data_tab"]["sender_split_data_types"].as_array().is_some_and(|a| a.iter().any(|t| t == ty));
            if senders.is_empty() && split {
                for (t, sender) in self.index.keys() {
                    if t == ty {
                        let label = self.boards.iter().find(|b| &b.sender == sender).map(|b| b.label.clone()).unwrap_or_else(|| sender.clone());
                        senders.push((Some(sender.clone()), format!("{group} · {label}"), tab));
                    }
                }
            }
            if senders.is_empty() {
                senders.push((None, group.clone(), tab));
            }
            for (sender, group, spec) in senders {
                let labels = spec["channels"].as_array().or_else(|| tab["channels"].as_array());
                for (index, label) in labels.into_iter().flatten().enumerate() {
                    let formatter = if spec["channel_formatters"].is_array() { &spec["channel_formatters"][index] } else { &tab["channel_formatters"][index] };
                    out.push(Channel {
                        group: group.clone(),
                        data_type: ty.to_string(),
                        sender: sender.clone(),
                        index,
                        label: label.as_str().unwrap_or("value").to_string(),
                        unit: formatter["suffix"].as_str().unwrap_or("").trim().to_string(),
                        precision: formatter["precision"].as_u64().unwrap_or(2) as usize,
                    });
                }
            }
        }
        let mut extra: BTreeMap<&str, usize> = BTreeMap::new();
        for s in &self.samples {
            if !named.contains(&s.data_type) {
                let n = extra.entry(&s.data_type).or_default();
                *n = (*n).max(s.values.len());
            }
        }
        for (ty, count) in extra {
            for index in 0..count.min(16) {
                out.push(Channel { group: ty.to_string(), data_type: ty.to_string(), sender: None, index, label: format!("{ty}[{index}]"), unit: String::new(), precision: 2 });
            }
        }
        out
    }

    /// The board as `GET /api/boards` reports it at `at`.
    pub fn board_status(&self, at: i64) -> Vec<Value> {
        self.boards
            .iter()
            .map(|b| {
                let last = self.index.iter().filter(|((_, s), _)| s == &b.sender).filter_map(|(_, rows)| {
                    let n = rows.partition_point(|&i| self.samples[i].t_ms <= at);
                    n.checked_sub(1).map(|k| (self.samples[rows[k]].t_ms, n))
                });
                let (last, count) = last.fold((None::<i64>, 0usize), |(m, c), (t, n)| (Some(m.map_or(t, |m| m.max(t))), c + n));
                json!({
                    "board": b.sender, "board_label": b.label, "sender_id": b.sender,
                    "seen": last.is_some(), "packet_count": count,
                    "last_seen_ms": last.map(|t| t + self.epoch_ms),
                    "age_ms": last.map(|t| (at - t).max(0)),
                })
            })
            .collect()
    }

    /// The recorded GPS track up to `at` (lat, lon, altitude).
    pub fn track(&self, at: i64) -> Vec<(f64, f64, f32)> {
        for ty in GPS_TYPES {
            let rows: Vec<_> = self
                .rows(ty, None)
                .flat_map(|r| r.iter().map(|&i| &self.samples[i]))
                .filter(|s| s.t_ms <= at)
                .filter_map(|s| {
                    let lat = s.values.first().copied().flatten()?;
                    let lon = s.values.get(1).copied().flatten()?;
                    (lat.abs() > 0.0001 && lon.abs() > 0.0001).then(|| (lat as f64, lon as f64, s.values.get(2).copied().flatten().unwrap_or(0.0)))
                })
                .collect();
            if !rows.is_empty() {
                return rows;
            }
        }
        Vec::new()
    }

    pub fn first_channel(&self, data_type: &str, index: usize) -> Option<Channel> {
        self.channels().into_iter().find(|c| c.data_type == data_type && c.index == index)
    }

    /// Flight or burn figures computed from the samples.
    pub fn summary(&self) -> Vec<(String, String)> {
        let mut out = vec![
            ("Duration".to_string(), format!("{:.1} s", (self.end_ms() - self.start_ms()) as f64 / 1000.0)),
            ("Rows".to_string(), self.samples.len().to_string()),
            ("Boards".to_string(), self.boards.iter().map(|b| b.sender.as_str()).collect::<Vec<_>>().join(", ")),
        ];
        let peak = |ty: &str, index: usize| -> Option<(f32, i64)> {
            self.rows(ty, None)
                .flat_map(|r| r.iter().map(|&i| &self.samples[i]))
                .filter_map(|s| s.values.get(index).copied().flatten().filter(|v| v.is_finite()).map(|v| (v, s.t_ms)))
                .max_by(|a, b| a.0.total_cmp(&b.0))
        };
        let t = |ms: i64| (ms - self.start_ms()) as f64 / 1000.0;
        for (ty, index, label) in [
            ("BAROMETER_DATA", 0, "Apogee (barometer)"),
            ("KALMAN_FILTER", 0, "Apogee (estimator)"),
            ("GPS_DATA", 2, "Highest GPS altitude"),
            ("VEHICLE_SPEED", 0, "Peak vertical speed"),
            ("ACCEL_DATA", 2, "Peak acceleration"),
            ("COMBUSTION_CHAMBER_PRESSURE", 0, "Peak chamber pressure"),
            ("LOADCELL_1000_WEIGHT_KG", 0, "Peak thrust load"),
            ("OX_TANK_PRESSURE", 0, "Peak tank pressure"),
        ] {
            if let Some((value, at)) = peak(ty, index) {
                let channel = self.first_channel(ty, index);
                let shown = channel.map(|c| c.format(value)).unwrap_or_else(|| format!("{value:.1}"));
                let extra = if ty == "BAROMETER_DATA" { format!(" ({:.0} ft)", value * 3.28084) } else { String::new() };
                out.push((label.to_string(), format!("{shown}{extra} at {:.1} s", t(at))));
            }
        }
        if let Some(pressure) = self.first_channel("COMBUSTION_CHAMBER_PRESSURE", 0) {
            let burn: Vec<_> = self.series(&pressure, i64::MIN, i64::MAX, usize::MAX).into_iter().filter(|p| p.1 > 50.0).collect();
            if let (Some(a), Some(b)) = (burn.first(), burn.last()) {
                out.push(("Burn (chamber above 50 psi)".to_string(), format!("{:.1} s", (b.0 - a.0) as f64 / 1000.0)));
            }
        }
        out
    }

    /// A station route answered from this session at time `at`: the same
    /// JSON a Ground Station returns (status, body). `None`: not served.
    pub fn get(&self, path: &str, at: i64) -> Option<Value> {
        let path = path.split('?').next().unwrap_or(path);
        let wall = |t: i64| t + self.epoch_ms;
        Some(match path {
            "/api/auth/session" => json!({
                "authenticated": false, "username": null,
                "permissions": {"view_data": true, "send_commands": false},
                "expires_at_ms": null, "anonymous": true, "session_type": null,
                "roles": [], "allowed_commands": []
            }),
            "/api/layout" => self.layout.clone(),
            "/api/boards" => json!({"boards": self.board_status(at)}),
            "/flightstate" => json!({"state": self.state_at(at)}),
            "/api/recent" => {
                let from = at - 30_000;
                let lo = self.samples.partition_point(|s| s.t_ms < from);
                let hi = self.samples.partition_point(|s| s.t_ms <= at);
                Value::Array(self.samples[lo..hi].iter().map(|s| json!({
                    "timestamp_ms": wall(s.t_ms), "received_timestamp_ms": wall(s.t_ms),
                    "data_type": s.data_type, "sender_id": s.sender, "values": s.values,
                })).collect())
            }
            "/api/gps" => match self.track(at).last() {
                Some((lat, lon, _)) => json!({"rocket": {"lat": lat, "lon": lon}}),
                None => json!({"rocket": null}),
            },
            "/api/alerts" => Value::Array(
                self.alerts.iter().filter(|a| a.t_ms <= at).map(|a| json!({"timestamp_ms": wall(a.t_ms), "message": a.message, "severity": if a.warning {"warning"} else {"error"}})).collect(),
            ),
            "/api/recordings" => Value::Array(self.recordings.iter().map(|r| json!({
                "id": r.id, "bytes": r.bytes, "active": false, "kind": r.kind, "label": r.label,
                "start_ms": wall(r.start_ms), "end_ms": wall(r.end_ms),
            })).collect()),
            "/api/calibration" => {
                let mut channels = serde_json::Map::new();
                for c in &self.calibration {
                    channels.insert(c.label.clone(), json!({
                        "linear": {"m": c.slope, "b": c.offset},
                        "points": c.points.iter().map(|(raw, expected)| json!({"expected": expected, "raw": raw})).collect::<Vec<_>>(),
                    }));
                }
                json!({"version": 2, "extra_channels": channels})
            }
            "/api/vehicle/geometry" => self.vehicle_geometry.clone()?,
            // The bundle's DEM grid around the pad (the 3D map draws flat ground without it).
            "/api/terrain/heightmap" => self.terrain.clone().unwrap_or_else(|| json!({
                "available": false, "format": "gs26.heightmap", "version": 1, "reason": "This replay carries no terrain.",
            })),
            // The recorded track with the station's phases, up to the replay position.
            "/api/flight_phases" => {
                let trajectory = self.trajectory.as_ref()?;
                let now = wall(at);
                let points: Vec<Value> = trajectory["points"].as_array().into_iter().flatten()
                    .filter(|p| p["timestamp_ms"].as_i64().is_some_and(|t| t <= now)).cloned().collect();
                let phases = trajectory["phases"].clone();
                let current = phases.as_array().and_then(|list| list.iter().rev().find(|s| s["start_ms"].as_i64().is_some_and(|t| t <= now))).map(|s| s["phase"].clone());
                json!({
                    "source": "replay", "window_start_ms": null, "current_phase": current,
                    "phases": phases, "events": trajectory["events"].as_array().map(|e| e.iter().filter(|e| e["t_ms"].as_i64().is_some_and(|t| t <= now)).cloned().collect::<Vec<_>>()),
                    "source_sender_id": trajectory["source_sender_id"], "points": points, "sampled": trajectory["sampled"],
                })
            }
            _ => return None,
        })
    }

    /// A command as the workspace sends it. Nothing leaves the device: the
    /// demo acknowledges it, a replay is read-only.
    pub fn command(&self, cmd: &str) -> (bool, String) {
        match self.source {
            Source::Demo => (true, format!("Demo: “{cmd}” acknowledged. Nothing was sent; no station is connected.")),
            Source::Replay { .. } => (false, format!("Replay is read-only: “{cmd}” was not sent.")),
        }
    }
}

/// Live-diagram inputs at one moment of a session.
pub(crate) struct DiagramInputs<'a> {
    pub session: &'a Session,
    pub at: i64,
}

impl super::live_diagrams::derive::Inputs for DiagramInputs<'_> {
    fn telemetry(&self, data_type: &str, sender: Option<&str>, index: usize) -> Option<super::live_diagrams::derive::Observed> {
        let row = self.session.latest(data_type, sender, self.at).or_else(|| self.session.latest(data_type, None, self.at))?;
        let value = row.values.get(index).copied().flatten()?;
        Some(super::live_diagrams::derive::Observed { value: super::live_diagrams::derive::Value::Num(value), age_ms: Some(self.at - row.t_ms) })
    }
    fn control(&self, _cmd: &str) -> Option<super::live_diagrams::derive::ControlView> {
        None
    }
    fn board(&self, sender: &str) -> Option<super::live_diagrams::derive::Observed> {
        let status = self.session.board_status(self.at);
        let entry = status.iter().find(|b| b["sender_id"] == sender)?;
        Some(super::live_diagrams::derive::Observed {
            value: super::live_diagrams::derive::Value::Bool(entry["seen"].as_bool() == Some(true)),
            age_ms: entry["age_ms"].as_i64(),
        })
    }
    fn status(&self, key: &str) -> Option<super::live_diagrams::derive::Observed> {
        let text = match key {
            "flight_state" => self.session.state_at(self.at),
            "fill_phase" => {
                let state = self.session.state_at(self.at);
                if state.contains("Fill") { state } else { "Not filling".into() }
            }
            "provenance" => self.session.provenance.clone(),
            k if k.starts_with("topology:") => {
                return self.board(k.trim_start_matches("topology:"));
            }
            _ => return None,
        };
        Some(super::live_diagrams::derive::Observed { value: super::live_diagrams::derive::Value::Text(text), age_ms: None })
    }
    fn igt(&self) -> bool {
        self.session.mode == Mode::GroundTest
    }
}

#[cfg(test)]
mod tests;
