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
pub(crate) mod exports;
pub(crate) mod media;
pub(crate) mod replay_file;
pub(crate) mod station;
pub(crate) use station::LocalStation;

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
    /// A bundle's `derived/recovery_search.json` (`GET /api/recovery_search`).
    pub recovery_search: Option<Value>,
    /// Bundle entries by the station route whose body they are (path
    /// without query), served as the station served them.
    pub routes: BTreeMap<String, RouteEntry>,
    /// Bundle entries that are filtered lists rather than route bodies.
    pub lists: BTreeMap<String, Value>,
    /// What a bundle supplies to answer the station's export routes ([`exports::answer`]).
    pub exports: Option<std::rc::Rc<exports::ExportInputs>>,
    /// Recorded camera video and crew voice a bundle carries ([`media`]).
    pub media: Option<std::rc::Rc<media::Media>>,
    index: BTreeMap<(String, String), Vec<usize>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        LIVE_SESSIONS.with(|n| n.set(n.get().saturating_sub(1)));
    }
}

/// One bundle entry served under its station route.
#[derive(Clone, Debug)]
pub(crate) struct RouteEntry {
    pub body: std::rc::Rc<Vec<u8>>,
    pub media_type: String,
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

/// Data types whose rows carry a GPS fix (lat, lon, alt).
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
            csv: None, manifest: None, predicted: None, plans: Vec::new(), terrain: None, trajectory: None, recovery_search: None, routes: BTreeMap::new(), lists: BTreeMap::new(), exports: None, media: None, index,
        }
    }

    pub fn start_ms(&self) -> i64 {
        self.samples.first().map(|s| s.t_ms).unwrap_or(0)
    }
    pub fn end_ms(&self) -> i64 {
        self.samples.last().map(|s| s.t_ms).unwrap_or(0)
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
    #[cfg(test)]
    pub fn latest(&self, data_type: &str, sender: Option<&str>, at: i64) -> Option<&Sample> {
        self.rows(data_type, sender)
            .filter_map(|rows| {
                let n = rows.partition_point(|&i| self.samples[i].t_ms <= at);
                n.checked_sub(1).map(|k| &self.samples[rows[k]])
            })
            .max_by_key(|s| s.t_ms)
    }



    pub fn state_at(&self, at: i64) -> String {
        let n = self.states.partition_point(|s| s.0 <= at);
        n.checked_sub(1).map(|k| self.states[k].1.clone()).unwrap_or_else(|| "Unknown".into())
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
            "/api/recovery_search" => self.recovery_search.clone()?,
            _ => return None,
        })
    }

    /// A bundle's filtered list entry (`derived/burn_reports.json`), else empty.
    pub(crate) fn listed(&self, path: &str) -> Value {
        self.lists.get(path).cloned().unwrap_or_else(|| Value::Array(Vec::new()))
    }

    /// Map tiles carried by a bundle, "z/x/y" -> bytes.
    pub(crate) fn tiles(&self) -> super::station_link::Tiles {
        self.routes
            .iter()
            .filter_map(|(route, entry)| Some((route.strip_prefix("/tiles/")?.strip_suffix(".jpg")?.to_string(), entry.body.clone())))
            .collect()
    }

    /// A bundle entry (or a map tile) for `GET path`, as the station sent it.
    pub(crate) fn route_entry(&self, path: &str, query: &str) -> Option<super::station_link::Reply> {
        let full = format!("{path}?{query}");
        let prefix = format!("{path}?");
        let entry = (!query.is_empty()).then(|| self.routes.get(&full)).flatten()
            .or_else(|| self.routes.get(path))
            .or_else(|| self.routes.range(prefix.clone()..).next().filter(|(k, _)| k.starts_with(&prefix)).map(|(_, v)| v))?;
        Some(super::station_link::Reply { status: 200, body: entry.body.as_ref().clone(), content_type: entry.media_type.clone() })
    }

}

#[cfg(test)]
mod tests;
