//! A [`Session`] served as a station: the live dashboard connects to it
//! through [`super::super::station_link`] exactly as it connects to a Ground
//! Station. It answers the HTTP routes the dashboard reads
//! ([`LocalStation::request`]) and produces the WebSocket frames a station
//! pushes ([`LocalStation::frames`]): telemetry batches as the playback clock
//! passes each row, flight state, board status, network time, warnings and
//! command acknowledgements, all in the station's own JSON.
//!
//! Nothing a control does leaves the device. A command or a write gets an
//! answer from here: in demo mode "acknowledged; nothing was sent", in a
//! replay "read-only; not sent".
use super::super::station_link::Reply;
use super::{Session, Source};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

/// Telemetry history `GET /api/recent` returns, as the station's display history.
const RECENT_MS: i64 = 10 * 60_000;
const RECENT_MAX_ROWS: usize = 12_000;

/// Playback position on the session's own time axis (sample `t_ms`).
#[derive(Clone, Copy, Debug)]
struct Clock {
    /// Wall-clock time (ms) at `anchor_pos`.
    anchor_wall: i64,
    /// Unbounded position at `anchor_wall`; a demo wraps it into its loop.
    anchor_pos: i64,
    speed: f64,
    paused: bool,
}

impl Clock {
    fn position(&self, wall: i64) -> i64 {
        if self.paused {
            self.anchor_pos
        } else {
            self.anchor_pos + ((wall - self.anchor_wall) as f64 * self.speed) as i64
        }
    }
    /// Wall-clock stamp of position `pos` (rows are stamped as if received now).
    fn wall_of(&self, pos: i64) -> i64 {
        self.anchor_wall + ((pos - self.anchor_pos) as f64 / self.speed) as i64
    }
}

/// What the offline entry shows about playback.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Playback {
    /// Position on the session's time axis, wrapped into the demo loop.
    pub position: i64,
    pub start: i64,
    pub end: i64,
    pub speed: f64,
    pub paused: bool,
    pub state: String,
}

pub(crate) struct LocalStation {
    pub session: Session,
    clock: Cell<Clock>,
    /// Unbounded position up to which rows were pushed.
    pushed_to: Cell<i64>,
    last_state: RefCell<String>,
    last_phase: Cell<Option<&'static str>>,
    last_status_wall: Cell<i64>,
    outbox: RefCell<VecDeque<String>>,
    next_notification: Cell<u64>,
}

fn reply(status: u16, body: Vec<u8>, content_type: &str) -> Reply {
    Reply { status, body, content_type: content_type.into() }
}
fn json_reply(value: &Value) -> Reply {
    reply(200, serde_json::to_vec(value).unwrap_or_default(), "application/json")
}
fn text_reply(status: u16, text: &str) -> Reply {
    reply(status, text.as_bytes().to_vec(), "text/plain; charset=utf-8")
}

/// Every command name the layout can send (`cmd` fields and widget `actions` lists).
fn layout_commands(layout: &Value, out: &mut Vec<String>) {
    match layout {
        Value::Object(map) => {
            for (key, value) in map {
                match (key.as_str(), value) {
                    ("cmd" | "action_cmd", Value::String(cmd)) if !out.contains(cmd) => out.push(cmd.clone()),
                    ("actions", Value::Array(items)) => {
                        for item in items {
                            if let Value::String(cmd) = item
                                && !out.contains(cmd)
                            {
                                out.push(cmd.clone());
                            }
                        }
                        layout_commands(value, out);
                    }
                    _ => layout_commands(value, out),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| layout_commands(item, out)),
        _ => {}
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encodes one path segment as the dashboard does.
pub(crate) fn percent_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}


/// A recorded or scenario state as the station's network contract states it:
/// `FlightState` (network state, fill sub-phase, FC phase) and the matching
/// `MissionPhase` (the operator phase the dashboard's views follow). Older
/// recordings carry the retired detailed names; the station reports those as
/// their network state with the name as detail.
fn state_wire(name: &str, previous: Option<&'static str>, mode: super::Mode, since_ms: i64) -> (Value, Value, &'static str) {
    const FC: [(&str, u8); 6] = [("Ascent", 8), ("Coast", 9), ("Apogee", 10), ("Descent", 11), ("Reefing", 12), ("ParachuteDeploy", 11)];
    let fill_label = |n: &str| match n {
        "PreFill" => Some("Pre-fill"),
        "FillTest" => Some("Fill test"),
        "NitrogenFill" => Some("Nitrogen fill"),
        "NitrousFill" => Some("Nitrous fill"),
        _ => None,
    };
    let (network, fill, fc) = if let Some(label) = fill_label(name) {
        ("Fill", Some((name, label)), None)
    } else if let Some((label, value)) = FC.iter().find(|(n, _)| *n == name) {
        ("InFlight", None, Some(json!({"value": value, "label": if *label == "ParachuteDeploy" { "Descent" } else { label }, "source": "fc_state_report", "age_ms": 0})))
    } else {
        (match name {
            "Startup" | "Unknown" => "Startup",
            "Idle" => "Idle",
            "Fill" => "Fill",
            "Armed" => "Armed",
            "Launch" => "Launch",
            "InFlight" => "InFlight",
            "Landed" | "Recovery" => "Landed",
            "Aborted" => "Aborted",
            _ => "Idle",
        }, None, None)
    };
    let phase: &'static str = match network {
        "Startup" => "startup",
        "Idle" => "idle",
        "Fill" | "Armed" => "fill",
        "Launch" => "launch",
        "InFlight" => "in_flight",
        "Landed" => "landed",
        _ => match previous {
            Some(p @ ("in_flight" | "landed")) => p,
            _ => "idle",
        },
    };
    let phase_label = match phase {
        "startup" => "Startup",
        "idle" => "Idle",
        "fill" => "Fill",
        "launch" => "Launch",
        "in_flight" => "In flight",
        _ => "Landed",
    };
    let detail = fill.map(|(_, label)| label.to_string()).or_else(|| fc.as_ref().and_then(|f| f["label"].as_str().map(str::to_owned)));
    let state_label = match network { "InFlight" => "In flight", "Armed" => "Armed", "Aborted" => "Aborted", _ => phase_label };
    let flight_state = json!({"state": network, "label": state_label, "fill_phase": fill.map(|(n, _)| n), "fill_phase_label": fill.map(|(_, l)| l), "fc_phase": fc});
    let player_mode = match mode { super::Mode::TestFire => "test_fire", super::Mode::Hitl => "hitl", _ => "flight" };
    let mission = json!({
        "phase": phase, "label": phase_label, "detail": detail, "detailed": network,
        "fill_phase": fill.map(|(n, _)| n), "fill_phase_label": fill.map(|(_, l)| l), "fc_phase": fc,
        "mode": player_mode, "manual_only": false, "aborted": network == "Aborted", "required_boards_seen": true,
        "fill_activity": fill.is_some(), "disconnect_aborts_armed": false, "comms_loss_landed": false, "since_ms": since_ms,
        "simulated": mode == super::Mode::GroundTest && matches!(phase, "in_flight" | "landed"),
    });
    (flight_state, mission, phase)
}

impl LocalStation {
    /// A station serving `session`, playing from `start_pos` at `now_wall`.
    pub(crate) fn new(session: Session, now_wall: i64) -> Self {
        let start = match session.source {
            Source::Demo => super::demo::OPEN_AT_MS,
            Source::Replay { .. } => session.start_ms(),
        };
        let clock = Clock { anchor_wall: now_wall, anchor_pos: start, speed: 1.0, paused: false };
        Self {
            session,
            clock: Cell::new(clock),
            pushed_to: Cell::new(start),
            last_state: RefCell::new(String::new()),
            last_phase: Cell::new(None),
            last_status_wall: Cell::new(i64::MIN),
            outbox: RefCell::new(VecDeque::new()),
            next_notification: Cell::new(1),
        }
    }

    /// Nothing happened before this position: the demo's first pass or the
    /// recording's first row (history never reaches into earlier loops).
    fn origin(&self) -> i64 {
        if self.looped() { -1 } else { self.session.start_ms() - 1 }
    }

    fn looped(&self) -> bool {
        self.session.is_demo()
    }
    fn loop_len(&self) -> i64 {
        super::demo::LOOP_MS
    }
    /// Unbounded position at `wall`; a replay stops at its last row.
    fn position(&self, wall: i64) -> i64 {
        let pos = self.clock.get().position(wall);
        if self.looped() { pos } else { pos.min(self.session.end_ms()) }
    }
    /// A position on the session's own axis.
    fn wrap(&self, pos: i64) -> i64 {
        if self.looped() { pos.rem_euclid(self.loop_len()) } else { pos }
    }

    pub(crate) fn playback(&self, wall: i64) -> Playback {
        let clock = self.clock.get();
        let (start, end) = if self.looped() { (0, self.loop_len()) } else { (self.session.start_ms(), self.session.end_ms()) };
        let position = self.wrap(self.position(wall));
        Playback { position, start, end, speed: clock.speed, paused: clock.paused || (!self.looped() && position >= end), state: self.session.state_at(position) }
    }

    /// Changes speed or pauses without a jump: rows keep arriving in order.
    pub(crate) fn set_speed(&self, speed: f64, paused: bool, wall: i64) {
        let pos = self.position(wall);
        self.clock.set(Clock { anchor_wall: wall, anchor_pos: pos, speed: speed.clamp(0.1, 100.0), paused });
    }

    /// Moves playback to `position` (session axis). The dashboard reconnects
    /// and reseeds its history up to the new position.
    pub(crate) fn seek(&self, position: i64, wall: i64) {
        let mut clock = self.clock.get();
        let base = if self.looped() { self.position(wall).div_euclid(self.loop_len()) * self.loop_len() } else { 0 };
        let pos = base + position;
        clock.anchor_pos = pos;
        clock.anchor_wall = wall;
        self.clock.set(clock);
        self.pushed_to.set(pos);
        self.last_state.borrow_mut().clear();
        self.last_status_wall.set(i64::MIN);
    }

    /// Rows with unbounded positions in `(from, to]`, oldest first.
    fn rows_between<'a>(&'a self, from: i64, to: i64, mut each: impl FnMut(i64, &'a super::Sample)) {
        if to <= from {
            return;
        }
        let samples = &self.session.samples;
        if !self.looped() {
            let lo = samples.partition_point(|s| s.t_ms <= from);
            let hi = samples.partition_point(|s| s.t_ms <= to);
            for s in &samples[lo..hi] {
                each(s.t_ms, s);
            }
            return;
        }
        let len = self.loop_len();
        let mut k = from.div_euclid(len);
        while k * len <= to {
            let base = k * len;
            let a = (from - base).max(-1);
            let b = (to - base).min(len - 1);
            let lo = samples.partition_point(|s| s.t_ms <= a);
            let hi = samples.partition_point(|s| s.t_ms <= b);
            for s in &samples[lo..hi] {
                each(base + s.t_ms, s);
            }
            k += 1;
        }
    }

    fn row_json(&self, clock: &Clock, pos: i64, s: &super::Sample) -> Value {
        let wall = clock.wall_of(pos);
        json!({"timestamp_ms": wall, "received_timestamp_ms": wall, "data_type": s.data_type, "sender_id": s.sender, "values": s.values})
    }

    fn board_json(&self, wall: i64) -> Value {
        let clock = self.clock.get();
        let pos = self.position(wall);
        let at = self.wrap(pos);
        let mut boards = self.session.board_status(at);
        for board in &mut boards {
            if let Some(age) = board["age_ms"].as_i64() {
                let age = (age as f64 / clock.speed) as i64;
                board["age_ms"] = age.into();
                board["last_seen_ms"] = (wall - age).into();
            }
        }
        json!({"boards": boards})
    }

    fn topology_json(&self, wall: i64) -> Value {
        let boards = self.board_json(wall);
        let mut nodes = vec![json!({"id": "gs", "label": "This device", "kind": "endpoint", "status": "online", "group": "ground", "show_in_details": true, "detail": self.mode_line()})];
        let mut links = Vec::new();
        let mut health = Vec::new();
        let observed = (self.position(wall) - self.origin()).max(0);
        for board in boards["boards"].as_array().into_iter().flatten() {
            let sender = board["sender_id"].as_str().unwrap_or_default();
            let online = board["age_ms"].as_i64().is_some_and(|age| age < 5_000);
            let id = sender.to_ascii_lowercase();
            nodes.push(json!({"id": id, "label": board["board_label"], "kind": "board", "status": if online {"online"} else {"offline"}, "group": "vehicle", "sender_id": sender, "endpoints": ["telemetry"], "show_in_details": true}));
            links.push(json!({"source": id, "target": "gs", "label": "telemetry", "status": if online {"online"} else {"offline"}}));
            if let Some(age) = board["age_ms"].as_i64() {
                health.push(json!({"sender_id": sender, "healthy": online, "age_ms": age, "last_seen_ms": board["last_seen_ms"], "disconnects": 0, "reconnects": 0,
                    "rx_observations": board["packet_count"], "observed_ms": observed, "offline_ms": 0, "longest_outage_ms": 0, "current_outage_ms": if online {0} else {age}}));
            }
        }
        json!({"generated_ms": wall, "simulated": false, "nodes": nodes, "links": links, "health": health})
    }

    /// `FlightState` and `MissionPhase` for the playback position.
    fn state_frames(&self, wall: i64) -> (Value, Value, &'static str) {
        let at = self.wrap(self.position(wall));
        let name = self.session.state_at(at);
        let since = self.session.states.iter().rev().find(|(t, _)| *t <= at).map_or(wall, |(t, _)| self.clock.get().wall_of(self.position(wall) - at + t));
        state_wire(&name, self.last_phase.get(), self.session.mode, since)
    }

    fn mode_line(&self) -> String {
        match &self.session.source {
            Source::Demo => "Demo mode — no station connected".into(),
            Source::Replay { file_name } => format!("Offline replay — {file_name}"),
        }
    }

    fn commands(&self) -> Vec<String> {
        let mut out = Vec::new();
        layout_commands(&self.session.layout, &mut out);
        if self.session.is_demo() {
            // The demo station's policy also lists the close commands its supply valves pair with.
            out.extend(super::demo::PAIRED_COMMANDS.iter().map(|c| c.to_string()));
        }
        out
    }

    fn session_status(&self) -> Value {
        json!({
            "authenticated": true, "anonymous": true, "username": null, "session_type": "local",
            "station_name": self.session.title,
            "permissions": {"view_data": true, "send_commands": true, "sign_checklists": true, "voice_transmit": false, "camera_publish": false, "set_system_time": false},
            "expires_at_ms": null, "roles": [], "allowed_commands": [],
            "can_view_calibration": true, "can_edit_calibration": false
        })
    }

    /// The acknowledgement for anything that would change station state.
    pub(crate) fn refusal(&self, what: &str) -> String {
        match self.session.source {
            Source::Demo => format!("Demo: “{what}” acknowledged. Nothing was sent; no station is connected."),
            Source::Replay { .. } => format!("Replay is read-only: “{what}” was not sent."),
        }
    }

    fn launch_clock(&self, wall: i64) -> Value {
        let at = self.wrap(self.position(wall));
        let launch = self.session.states.iter().find(|(_, s)| s == "Launch").map(|(t, _)| *t);
        let clock = self.clock.get();
        match launch {
            // The demo counts down 10 s from Launch to ignition.
            Some(t) if self.session.is_demo() && at >= t && at < t + 10_000 => json!({"kind": "t_minus", "anchor_timestamp_ms": clock.wall_of(self.position(wall) - at + t + 10_000), "duration_ms": 10_000}),
            Some(t) if at >= t => {
                let zero = t + if self.session.is_demo() { 10_000 } else { 0 };
                json!({"kind": "t_plus", "anchor_timestamp_ms": clock.wall_of(self.position(wall) - at + zero), "duration_ms": null})
            }
            _ => json!({"kind": "idle", "anchor_timestamp_ms": null, "duration_ms": null}),
        }
    }

    fn recent(&self, wall: i64) -> Value {
        let clock = self.clock.get();
        let to = self.position(wall);
        let mut rows = Vec::new();
        self.rows_between((to - RECENT_MS).max(self.origin()), to, |pos, s| rows.push((pos, s)));
        let skip = rows.len().saturating_sub(RECENT_MAX_ROWS);
        Value::Array(rows[skip..].iter().map(|(pos, s)| self.row_json(&clock, *pos, s)).collect())
    }

    /// The recording a replay file (or the demo) stands for; its replay
    /// document is served as that recording's `GET …/replay/export`.
    fn replay_id(&self) -> Option<String> {
        let document = self.session.replay_document.as_ref()?;
        Some(document["recording"].as_str().filter(|r| !r.is_empty()).unwrap_or("replay").to_string())
    }

    fn recordings(&self) -> Value {
        let clock = self.clock.get();
        let wall = |t: i64| if self.session.is_demo() { clock.wall_of(t) } else { t };
        let mut list: Vec<Value> = self
            .session
            .recordings
            .iter()
            .map(|r| json!({"id": r.id, "bytes": r.bytes, "active": false, "kind": r.kind, "label": r.label, "start_ms": wall(r.start_ms), "end_ms": wall(r.end_ms)}))
            .collect();
        if self.session.manifest.is_none()
            && let Some(id) = self.replay_id()
            && !list.iter().any(|r| r["id"] == id.as_str())
        {
            list.insert(0, json!({"id": id, "bytes": 0, "active": false, "kind": "capture", "label": format!("{} (replay file)", self.session.title),
                "start_ms": self.session.start_ms(), "end_ms": self.session.end_ms()}));
        }
        if let Some(manifest) = &self.session.manifest
            && let Some(id) = manifest["source"]["recording"].as_str()
        {
            list.insert(0, json!({"id": id, "bytes": self.session.csv.as_ref().map_or(0, |c| c.len()), "active": false, "kind": "capture",
                "label": format!("{} (replay bundle)", self.session.title), "start_ms": self.session.start_ms(), "end_ms": self.session.end_ms()}));
        }
        Value::Array(list)
    }

    /// The JSON a station returns for `GET path` at `wall`, `None` when this
    /// device does not serve it.
    fn get_json(&self, path: &str, query: &str, wall: i64) -> Option<Value> {
        let clock = self.clock.get();
        let at = self.wrap(self.position(wall));
        Some(match path {
            "/api/auth/session" => self.session_status(),
            "/flightstate" => self.state_frames(wall).0,
            "/api/recent" => self.recent(wall),
            "/api/boards" => self.board_json(wall),
            "/api/network_topology" => self.topology_json(wall),
            "/api/alerts" => {
                let pos = self.position(wall);
                let mut out = Vec::new();
                self.alerts_between((pos - 20 * 60_000).max(self.origin()), pos, |p, a| out.push(json!({"timestamp_ms": clock.wall_of(p), "message": a.message, "severity": if a.warning {"warning"} else {"error"}})));
                Value::Array(out)
            }
            "/api/notifications" | "/api/messages" => json!([]),
            "/api/action_policy" => json!({
                "key_enabled": true, "software_buttons_enabled": true,
                "controls": self.commands().into_iter().map(|cmd| json!({"cmd": cmd, "enabled": true, "blink": "none", "actuated": null})).collect::<Vec<_>>()
            }),
            "/api/launch_clock" => self.launch_clock(wall),
            "/api/recording_status" => json!({"mode": "idle", "db_path": null}),
            "/api/network_time" | "/api/system/time" => json!({"timestamp_ms": wall, "utc_ms": wall}),
            "/api/recordings" => self.recordings(),
            "/api/gps" => match self.session.track(at).last() {
                Some((lat, lon, _)) => json!({"rocket": {"lat": lat, "lon": lon}}),
                None => json!({"rocket": null}),
            },
            "/api/layout" => self.session.layout.clone(),
            _ if self.replay_id().is_some_and(|id| path == format!("/api/recordings/{}/replay/export", percent_component(&id))) => {
                self.session.replay_document.clone()?
            }
            // With no recorded trajectory (the demo, a portable replay file) the
            // track and its phases come from the rows and the state timeline.
            "/api/flight_phases" if self.session.trajectory.is_none() => self.flight_phases(wall),
            // The flight plans a bundle carries (a plan bundle's own plan and the
            // station's recent ones), listed and served as the station does.
            "/api/flight_plans" => json!({
                "plans": self.session.plans.iter().map(|plan| json!({
                    "id": plan.id, "label": plan.label, "kind": plan.kind, "created_ms": plan.created_ms, "created_by": plan.created_by,
                    "fill_kg": plan.summary.fill_kg, "apogee_m": plan.summary.apogee_m,
                    "landing": plan.landing.as_ref().map(|l| json!({"lat_deg": l.lat_deg, "lon_deg": l.lon_deg})),
                    "warnings": plan.warnings.len(),
                })).collect::<Vec<_>>(),
            }),
            _ if path.starts_with("/api/flight_plans/") && !path["/api/flight_plans/".len()..].contains('/') => {
                let id = percent_decode(&path["/api/flight_plans/".len()..]);
                serde_json::to_value(self.session.plans.iter().find(|plan| plan.id == id)?).ok()?
            }
            "/api/recovery_search" if self.session.recovery_search.is_none() => json!({"format": "gs26.recovery_search", "active": false, "overlay": null}),
            "/api/recordings/store" => {
                let legacy: Vec<Value> = self.recordings().as_array().into_iter().flatten().map(|r| json!({"id": r["id"], "bytes": r["bytes"]})).collect();
                json!({"always_on": false, "groups": [], "legacy": legacy})
            }
            // Filtered lists a bundle carries (no route of their own); empty otherwise.
            "/api/burn_reports" => json!({"reports": self.session.listed("derived/burn_reports.json")}),
            "/api/fire_archive" => json!({"fires": self.session.listed("derived/fire_archive.json")}),
            "/api/checklists" => if self.session.is_demo() { super::demo::checklists() } else { json!({"revision": 0, "enforce_before_launch": false, "checklists": []}) },
            _ if path.starts_with("/api/recordings/") && path.ends_with("/replay") => self.replay_snapshot()?,
            _ if path.starts_with("/api/recordings/") && path.ends_with("/replay/window") => self.replay_window(query)?,
            _ => {
                if let Some(value) = self.session.get(&format!("{path}?{query}"), at) {
                    return Some(value);
                }
                return None;
            }
        })
    }

    /// `GET /api/flight_phases` computed from the GPS rows and the state
    /// timeline, stamped on the wall clock like the pushed rows.
    fn flight_phases(&self, wall: i64) -> Value {
        let clock = self.clock.get();
        let to = self.position(wall);
        let base = to - self.wrap(to);
        let at = self.wrap(to);
        let phase_of = |state: &str| match state {
            "Launch" | "Ascent" => "boost",
            "Coast" | "Apogee" => "coast",
            "Descent" | "Reefing" | "ParachuteDeploy" => "drogue",
            "Landed" | "Recovery" => "landed",
            _ => "pad",
        };
        let mut phases: Vec<(String, i64)> = Vec::new();
        for (t, state) in self.session.states.iter().filter(|(t, _)| *t <= at) {
            let phase = phase_of(state).to_string();
            if phases.last().is_none_or(|(p, _)| *p != phase) {
                phases.push((phase, clock.wall_of(base + t)));
            }
        }
        let segments: Vec<Value> = phases.iter().enumerate().map(|(i, (phase, start))| json!({"phase": phase, "start_ms": start, "end_ms": phases.get(i + 1).map(|n| n.1)})).collect();
        let phase_at = |t: i64| phases.iter().rev().find(|(_, start)| *start <= t).map(|(p, _)| p.clone());
        let track = self.session.track(at);
        let gps_times: Vec<i64> = ["GPS_DATA", "GPS"].iter().flat_map(|ty| self.session.samples.iter().filter(move |s| s.data_type == *ty && s.t_ms <= at)).filter(|s| s.values.first().copied().flatten().is_some_and(|v| v.abs() > 0.0001)).map(|s| s.t_ms).collect();
        let points: Vec<Value> = track.iter().zip(gps_times).map(|((lat, lon, alt), t)| {
            let stamp = clock.wall_of(base + t);
            json!({"timestamp_ms": stamp, "latitude": lat, "longitude": lon, "altitude_m": alt, "phase": phase_at(stamp)})
        }).collect();
        json!({"points": points, "phases": segments, "events": [], "source_sender_id": "FC", "current_phase": phases.last().map(|p| p.0.clone()), "sampled": false})
    }

    /// The replay document's rows (portable replay format, with ids).
    fn replay_rows(&self) -> impl Iterator<Item = &Value> {
        self.session.replay_document.iter().flat_map(|d| d["samples"].as_array().into_iter().flatten())
    }

    /// `GET /api/recordings/{id}/replay`: the frozen range of the recording.
    fn replay_snapshot(&self) -> Option<Value> {
        let upper = self.replay_rows().filter_map(|r| r["id"].as_i64()).max()?;
        Some(json!({"upper_id": upper, "start_ms": self.session.start_ms(), "end_ms": self.session.end_ms()}))
    }

    /// `GET /api/recordings/{id}/replay/window`: rows by id within a time
    /// window and type list, paged like the station, with the last row of
    /// each source before the window as seeds on the first page.
    fn replay_window(&self, query: &str) -> Option<Value> {
        const PAGE: usize = 5_000;
        let param = |name: &str| query.split('&').find_map(|p| p.strip_prefix(name)?.strip_prefix('=')).map(percent_decode);
        let number = |name: &str, default: i64| param(name).and_then(|v| v.parse().ok()).unwrap_or(default);
        let (upper, after) = (number("upper_id", i64::MAX), number("after_id", 0));
        let (start, end) = (number("start_ms", i64::MIN), number("end_ms", i64::MAX));
        let types: Vec<String> = param("types").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let wanted = |r: &Value| r["id"].as_i64().is_some_and(|id| id <= upper) && (types.is_empty() || types.iter().any(|t| r["data_type"] == t.as_str()));
        let mut rows: Vec<&Value> = self.replay_rows().filter(|r| wanted(r) && r["id"].as_i64() > Some(after) && r["timestamp_ms"].as_i64().is_some_and(|t| t >= start && t <= end)).collect();
        rows.sort_by_key(|r| r["id"].as_i64());
        let has_more = rows.len() > PAGE;
        rows.truncate(PAGE);
        let next_id = rows.last().and_then(|r| r["id"].as_i64()).unwrap_or(after);
        let mut seeds: Vec<Value> = Vec::new();
        if after == 0 {
            let mut last: std::collections::BTreeMap<(String, String), &Value> = std::collections::BTreeMap::new();
            for r in self.replay_rows().filter(|r| wanted(r) && r["timestamp_ms"].as_i64().is_some_and(|t| t < start)) {
                let key = (r["sender_id"].as_str().unwrap_or_default().to_string(), r["data_type"].as_str().unwrap_or_default().to_string());
                if last.get(&key).is_none_or(|prev| prev["timestamp_ms"].as_i64() <= r["timestamp_ms"].as_i64()) {
                    last.insert(key, r);
                }
            }
            seeds = last.into_values().cloned().collect();
        }
        Some(json!({"upper_id": upper.min(self.replay_snapshot()?["upper_id"].as_i64()?), "next_id": next_id, "has_more": has_more, "rows": rows, "seeds": seeds}))
    }

    #[cfg(test)]
    pub(crate) fn get_json_for_test(&self, path_and_query: &str, wall: i64) -> Value {
        let (path, query) = path_and_query.split_once('?').unwrap_or((path_and_query, ""));
        self.get_json(path, query, wall).unwrap_or(Value::Null)
    }

    fn alerts_between(&self, from: i64, to: i64, mut each: impl FnMut(i64, &super::Alert)) {
        if !self.looped() {
            for a in self.session.alerts.iter().filter(|a| a.t_ms > from && a.t_ms <= to) {
                each(a.t_ms, a);
            }
            return;
        }
        let len = self.loop_len();
        let mut k = from.div_euclid(len);
        while k * len <= to {
            for a in &self.session.alerts {
                let pos = k * len + a.t_ms;
                if pos > from && pos <= to {
                    each(pos, a);
                }
            }
            k += 1;
        }
    }

    /// One HTTP request as the station would answer it. Writes are answered
    /// here and never forwarded.
    pub(crate) fn request(&self, method: &str, path_and_query: &str, body: Option<&[u8]>) -> Reply {
        self.request_at(method, path_and_query, body, super::super::current_wallclock_ms())
    }

    /// [`Self::request`] at wall-clock time `wall`.
    pub(crate) fn request_at(&self, method: &str, path_and_query: &str, _body: Option<&[u8]>, wall: i64) -> Reply {
        let (path, query) = path_and_query.split_once('?').unwrap_or((path_and_query, ""));
        if !method.eq_ignore_ascii_case("GET") {
            // Signing out of a local station leaves it.
            if path == "/api/auth/logout" {
                return json_reply(&json!({"ok": true}));
            }
            // A computation, not a write: it runs on a station.
            if path == "/api/los_prediction/simulate" {
                return text_reply(501, "Signal-loss simulation runs on a station: open this recording from a connected dashboard to simulate it.");
            }
            if path == "/api/command" {
                let cmd = _body.and_then(|b| serde_json::from_slice::<Value>(b).ok()).and_then(|v| v["cmd"].as_str().map(str::to_owned)).unwrap_or_else(|| "command".into());
                self.acknowledge(&cmd);
                return text_reply(409, &self.refusal(&cmd));
            }
            return text_reply(409, &self.refusal(&format!("{} {}", method.to_ascii_uppercase(), percent_decode(path))));
        }
        // Exports (CSV, reports, review zips) as the station writes them, from a bundle.
        if let Some(inputs) = &self.session.exports
            && let Some(answer) = super::exports::answer(inputs, path_and_query)
        {
            return match answer {
                Ok(file) => reply(200, file.bytes, file.content_type),
                Err((status, message)) => text_reply(status, &message),
            };
        }
        // Bundle entries: the station's own bytes.
        if let Some(found) = self.session.route_entry(path, query) {
            return found;
        }
        // The demo station's records exports (checklists, recovery history).
        if self.session.is_demo()
            && let Some((body, content_type)) = super::demo::record(path)
        {
            return reply(200, body, content_type);
        }
        match self.get_json(path, query, wall) {
            Some(value) => json_reply(&value),
            None => text_reply(404, "Not available offline: this route needs a Ground Station."),
        }
    }

    /// A command sent on the live socket: answered with a notification.
    pub(crate) fn command(&self, wire: &str) {
        let cmd = serde_json::from_str::<Value>(wire).ok().and_then(|v| v["cmd"].as_str().map(str::to_owned)).unwrap_or_else(|| "command".into());
        self.acknowledge(&cmd);
    }

    fn acknowledge(&self, cmd: &str) {
        let id = self.next_notification.get();
        self.next_notification.set(id + 1);
        let frame = json!({"ty": "Notification", "data": {
            "id": 9_000_000 + id, "timestamp_ms": super::super::current_wallclock_ms(), "message": self.refusal(cmd),
            "persistent": false, "action_label": null, "action_cmd": null
        }});
        self.outbox.borrow_mut().push_back(frame.to_string());
    }

    /// The frames a station would have pushed since the last call, in order.
    pub(crate) fn frames(&self, wall: i64) -> Vec<String> {
        let clock = self.clock.get();
        let to = self.position(wall);
        let from = self.pushed_to.get();
        let mut frames = Vec::new();
        if to > from {
            let mut rows = Vec::new();
            self.rows_between(from, to, |pos, s| rows.push(self.row_json(&clock, pos, s)));
            for chunk in rows.chunks(2_000) {
                frames.push(json!({"ty": "TelemetryBatch", "data": chunk}).to_string());
            }
            self.alerts_between(from, to, |pos, a| {
                let ty = if a.warning { "Warning" } else { "Error" };
                frames.push(json!({"ty": ty, "data": {"timestamp_ms": clock.wall_of(pos), "message": a.message}}).to_string());
            });
            self.pushed_to.set(to);
        }
        let state = self.session.state_at(self.wrap(to));
        if *self.last_state.borrow() != state {
            let (flight_state, mission, phase) = self.state_frames(wall);
            frames.push(json!({"ty": "FlightState", "data": flight_state}).to_string());
            frames.push(json!({"ty": "MissionPhase", "data": mission}).to_string());
            frames.push(json!({"ty": "LaunchClock", "data": self.launch_clock(wall)}).to_string());
            self.last_phase.set(Some(phase));
            *self.last_state.borrow_mut() = state;
        }
        if wall.saturating_sub(self.last_status_wall.get()) >= 1_000 {
            self.last_status_wall.set(wall);
            frames.push(json!({"ty": "NetworkTime", "data": {"timestamp_ms": wall}}).to_string());
            frames.push(json!({"ty": "BoardStatus", "data": self.board_json(wall)}).to_string());
            frames.push(json!({"ty": "NetworkTopology", "data": self.topology_json(wall)}).to_string());
        }
        frames.extend(self.outbox.borrow_mut().drain(..));
        frames
    }
}

