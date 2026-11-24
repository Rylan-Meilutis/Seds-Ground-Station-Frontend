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


impl LocalStation {
    /// A station serving `session`, playing from `start_pos` at `now_wall`.
    pub(crate) fn new(session: Session, now_wall: i64) -> Self {
        let start = match session.source {
            Source::Demo => super::demo::OPEN_AT_MS,
            Source::Replay { .. } => session.start_ms(),
        };
        let clock = Clock { anchor_wall: now_wall, anchor_pos: start, speed: 1.0, paused: false };
        let state = session.state_at(start);
        Self {
            session,
            clock: Cell::new(clock),
            pushed_to: Cell::new(start),
            last_state: RefCell::new(state),
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
        *self.last_state.borrow_mut() = self.session.state_at(self.wrap(pos));
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
        for board in boards["boards"].as_array().into_iter().flatten() {
            let sender = board["sender_id"].as_str().unwrap_or_default();
            let online = board["age_ms"].as_i64().is_some_and(|age| age < 5_000);
            let id = sender.to_ascii_lowercase();
            nodes.push(json!({"id": id, "label": board["board_label"], "kind": "board", "status": if online {"online"} else {"offline"}, "group": "vehicle", "sender_id": sender, "endpoints": ["telemetry"], "show_in_details": true}));
            links.push(json!({"source": id, "target": "gs", "label": "telemetry", "status": if online {"online"} else {"offline"}}));
        }
        json!({"generated_ms": wall, "simulated": self.session.is_demo(), "nodes": nodes, "links": links})
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
            "/flightstate" => json!({"state": self.session.state_at(at)}),
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
            _ => {
                if let Some(value) = self.session.get(&format!("{path}?{query}"), at) {
                    return Some(value);
                }
                return None;
            }
        })
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
            if path == "/api/command" {
                let cmd = _body.and_then(|b| serde_json::from_slice::<Value>(b).ok()).and_then(|v| v["cmd"].as_str().map(str::to_owned)).unwrap_or_else(|| "command".into());
                self.acknowledge(&cmd);
                return text_reply(409, &self.refusal(&cmd));
            }
            return text_reply(409, &self.refusal(&format!("{} {}", method.to_ascii_uppercase(), percent_decode(path))));
        }
        // Bundle entries and exports: the station's own bytes.
        if let Some(found) = self.session.route_entry(path, query) {
            return found;
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
            frames.push(json!({"ty": "FlightState", "data": {"state": state}}).to_string());
            frames.push(json!({"ty": "LaunchClock", "data": self.launch_clock(wall)}).to_string());
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

