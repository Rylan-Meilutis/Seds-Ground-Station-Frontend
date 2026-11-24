//! Recovery search coverage (docs/backend-contract.md "Recovery search";
//! GroundStation26 docs/backend/recovery-search.md): the device side.
//!
//! An operator starts a search on the station. A user may then opt in on
//! this device to share its GPS track while the search runs. The track is
//! thinned, buffered on the device (persisted and bounded, so it survives
//! losing signal or closing the app) and uploaded in sequence-numbered
//! batches that the station de-duplicates. A user who does not opt in can
//! still draw their own track on their own map; that track never leaves the
//! device.
//!
//! This module is the pure part: wire types, the sharing gate, the recorder's
//! thinning, the outbox (buffer, batching, retry and refusal handling) and
//! the map overlays. `recovery_search_view` runs it.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::VecDeque;

pub(crate) const STATUS_PATH: &str = "/api/recovery_search";
pub(crate) const START_PATH: &str = "/api/recovery_search/start";
pub(crate) const STOP_PATH: &str = "/api/recovery_search/stop";
pub(crate) const SETTINGS_PATH: &str = "/api/recovery_search/settings";
pub(crate) const OPT_IN_PATH: &str = "/api/recovery_search/opt_in";
pub(crate) const TRACK_PATH: &str = "/api/recovery_search/track";
pub(crate) const TRACKS_PATH: &str = "/api/recovery_search/tracks";
/// Sources of a predicted landing area (the flight-predict contract).
pub(crate) const LOS_PATH: &str = "/api/los_prediction";
pub(crate) const FLIGHT_PLANS_PATH: &str = "/api/flight_plans";
/// The map overlay provider id (`window.setGroundMapOverlays`).
pub(crate) const PROVIDER: &str = "search";

/// A new fix is kept after this much movement...
pub(crate) const MIN_MOVE_M: f64 = 5.0;
/// ...or this long after the last kept fix.
pub(crate) const MAX_INTERVAL_MS: i64 = 30_000;
/// Fixes worse than this are not recorded (the station refuses them too).
pub(crate) const MAX_ACCURACY_M: f64 = 100.0;
/// The upload buffer keeps at most this many fixes (oldest dropped first).
pub(crate) const MAX_PENDING: usize = 20_000;
pub(crate) const MAX_BATCH: usize = 200;
/// The local-only track keeps at most this many fixes.
pub(crate) const MAX_LOCAL: usize = 5_000;
pub(crate) const SWATH_MIN_M: f64 = 2.0;
pub(crate) const SWATH_MAX_M: f64 = 500.0;
pub(crate) const OUTBOX_KEY: &str = "recovery_search_outbox_v1";
pub(crate) const DEVICE_ID_KEY: &str = "recovery_search_device_id_v1";
pub(crate) const LOCAL_TRACK_COLOR: &str = "#f8fafc";

// ---------------------------------------------------------------------------
// Wire

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub(crate) struct Settings {
    pub swath_width_m: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { swath_width_m: 30.0 }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Target {
    pub polygon: Vec<[f64; 2]>,
    pub source: String,
    pub label: Option<String>,
    pub set_ms: Option<i64>,
    pub set_by: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Stats {
    pub percent_covered: Option<f64>,
    pub target_m2: Option<f64>,
    pub covered_in_target_m2: Option<f64>,
    pub uncovered_m2: Option<f64>,
    pub swath_m2: f64,
    pub cell_m: Option<f64>,
    pub target_cell_m: Option<f64>,
    pub searchers: usize,
    pub sharing: usize,
    pub points: usize,
    pub distance_m: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct SearcherView {
    pub id: String,
    pub name: String,
    pub username: String,
    pub platform: String,
    pub color: String,
    pub sharing: bool,
    pub points: usize,
    pub distance_m: f64,
    pub first_ms: Option<i64>,
    pub last_ms: Option<i64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct You {
    pub device_id: String,
    pub id: String,
    pub sharing: bool,
    pub opt_in_id: Option<String>,
    pub points: usize,
    pub max_seq: Option<u64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Limits {
    pub swath_width_m: Option<[f64; 2]>,
    pub max_batch_points: Option<usize>,
    pub late_upload_grace_s: Option<i64>,
}

/// `GET /api/recovery_search`.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Status {
    pub format: String,
    pub active: bool,
    pub session_id: Option<String>,
    pub started_ms: Option<i64>,
    pub started_by: Option<String>,
    pub stopped_ms: Option<i64>,
    pub stopped_by: Option<String>,
    pub accepting_uploads: bool,
    pub late_upload_until_ms: Option<i64>,
    pub settings: Settings,
    pub limits: Limits,
    pub target: Option<Target>,
    pub stats: Stats,
    pub searchers: Vec<SearcherView>,
    pub overlay: Value,
    pub you: Option<You>,
}

/// `POST /api/recovery_search/opt_in` reply.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct OptInReply {
    pub session_id: Option<String>,
    pub device_id: String,
    pub sharing: bool,
    pub opt_in_id: Option<String>,
    pub opted_in_ms: Option<i64>,
    pub points: usize,
    pub max_seq: Option<u64>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub(crate) struct OptInRequest {
    pub device_id: String,
    pub device_name: String,
    pub platform: String,
    pub sharing: bool,
    pub delete_my_track: bool,
}

/// `POST /api/recovery_search/track` reply.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct UploadReply {
    pub accepted: usize,
    pub duplicates: usize,
    pub rejected: usize,
    pub full: bool,
    pub stored_points: usize,
    pub max_seq: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fix {
    pub seq: u64,
    pub t_ms: i64,
    pub lat: f64,
    pub lon: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acc_m: Option<f64>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub(crate) struct Batch {
    pub v: u32,
    pub session_id: String,
    pub opt_in_id: String,
    pub device_id: String,
    pub points: Vec<Fix>,
}

// ---------------------------------------------------------------------------
// The sharing gate

/// What this device does with its location right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Nothing is recorded.
    Off,
    /// Recorded and drawn on this device's map only; never sent.
    LocalOnly,
    /// Opted in to the running search: recorded, buffered and uploaded.
    Sharing,
}

/// Sharing needs a running search on the station AND this device's opt-in for
/// that session; anything else records locally at most.
pub(crate) fn mode(status: Option<&Status>, outbox: &Outbox, local_track: bool) -> Mode {
    let running = status.is_some_and(|s| s.active && s.session_id.is_some() && s.session_id == outbox.session_id);
    if running && outbox.armed() {
        Mode::Sharing
    } else if local_track {
        Mode::LocalOnly
    } else {
        Mode::Off
    }
}

// ---------------------------------------------------------------------------
// Recorder thinning

/// Distance in metres between two points (equirectangular; short ranges).
pub(crate) fn distance_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let k = 6_371_008.8f64.to_radians();
    let dx = (b.1 - a.1) * k * ((a.0 + b.0) / 2.0).to_radians().cos();
    let dy = (b.0 - a.0) * k;
    dx.hypot(dy)
}

pub(crate) fn usable(lat: f64, lon: f64, acc_m: Option<f64>) -> bool {
    lat.is_finite()
        && lon.is_finite()
        && lat.abs() <= 90.0
        && lon.abs() <= 180.0
        && !(lat.abs() < 1e-6 && lon.abs() < 1e-6)
        && acc_m.is_none_or(|a| a.is_finite() && (0.0..=MAX_ACCURACY_M).contains(&a))
}

/// Keeps a fix after [`MIN_MOVE_M`] of movement or [`MAX_INTERVAL_MS`].
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Thinner {
    last: Option<(f64, f64, i64)>,
}

impl Thinner {
    pub(crate) const fn new() -> Self {
        Self { last: None }
    }
    pub(crate) fn keep(&mut self, lat: f64, lon: f64, t_ms: i64, acc_m: Option<f64>) -> bool {
        if !usable(lat, lon, acc_m) {
            return false;
        }
        let keep = match self.last {
            None => true,
            Some((la, lo, t)) => t_ms > t && (distance_m((la, lo), (lat, lon)) >= MIN_MOVE_M || t_ms - t >= MAX_INTERVAL_MS),
        };
        if keep {
            self.last = Some((lat, lon, t_ms));
        }
        keep
    }
    pub(crate) fn reset(&mut self) {
        self.last = None;
    }
}

// ---------------------------------------------------------------------------
// Outbox: the persisted upload buffer

/// How a failed upload is handled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The station refused sharing (search off, other session, not opted
    /// in): stop sharing and discard the buffer.
    Refused,
    /// The batch itself is invalid: drop it, keep sharing.
    BadBatch,
    /// Offline, timeout, rate limit, server error: keep it and retry later.
    Retry,
}

/// Classifies an upload error (`HTTP <status>: <body>` or a transport error).
pub(crate) fn classify(error: &str) -> Failure {
    let refused = ["\"search_off\"", "\"session_mismatch\"", "\"not_opted_in\"", "\"sign_in\""];
    if refused.iter().any(|code| error.contains(code)) {
        Failure::Refused
    } else if error.starts_with("HTTP 400") || error.starts_with("HTTP 413") {
        Failure::BadBatch
    } else {
        Failure::Retry
    }
}

/// Retry delay after `failures` consecutive failures: 2 s doubling to 60 s.
pub(crate) fn backoff_ms(failures: u32) -> i64 {
    (2_000i64 << failures.saturating_sub(1).min(5)).min(60_000)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Outbox {
    /// Station URL and account the buffer belongs to.
    pub owner: String,
    pub device_id: String,
    pub session_id: Option<String>,
    pub opt_in_id: Option<String>,
    pub next_seq: u64,
    pub pending: VecDeque<Fix>,
    /// Fixes dropped because the buffer was full.
    pub dropped: u64,
    pub failures: u32,
    pub retry_at_ms: i64,
    pub last_error: Option<String>,
    /// The user stopped sharing in this session; resent if the station
    /// still lists the device as sharing (the opt-out was sent offline).
    pub opted_out_session: Option<String>,
    pub uploaded: u64,
}

impl Outbox {
    pub(crate) fn new(owner: &str, device_id: &str) -> Self {
        Self { owner: owner.into(), device_id: device_id.into(), next_seq: 1, ..Self::default() }
    }
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        serde_json::from_str(raw).ok()
    }
    pub(crate) fn armed(&self) -> bool {
        self.session_id.is_some() && self.opt_in_id.is_some()
    }
    /// After the station accepted this device's opt-in. A different session
    /// starts empty; sequence numbers only grow.
    pub(crate) fn arm(&mut self, session_id: &str, opt_in_id: &str, station_max_seq: Option<u64>) {
        if self.session_id.as_deref() != Some(session_id) {
            self.pending.clear();
            self.failures = 0;
            self.retry_at_ms = 0;
        }
        self.session_id = Some(session_id.into());
        self.opt_in_id = Some(opt_in_id.into());
        self.opted_out_session = None;
        self.last_error = None;
        self.next_seq = self.next_seq.max(station_max_seq.map_or(1, |s| s + 1)).max(1);
    }
    /// Stops sharing: nothing more is recorded or sent, the buffer is discarded.
    pub(crate) fn disarm(&mut self) {
        self.opt_in_id = None;
        self.pending.clear();
        self.failures = 0;
        self.retry_at_ms = 0;
    }
    /// Buffers a fix while armed. Returns its sequence number.
    pub(crate) fn push(&mut self, t_ms: i64, lat: f64, lon: f64, acc_m: Option<f64>) -> Option<u64> {
        if !self.armed() || !usable(lat, lon, acc_m) {
            return None;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.pending.push_back(Fix { seq, t_ms, lat, lon, acc_m });
        while self.pending.len() > MAX_PENDING {
            self.pending.pop_front();
            self.dropped += 1;
        }
        Some(seq)
    }
    /// The next batch to send, oldest first, once any retry delay has passed.
    /// Uploading continues after the search stops (the station's late grace)
    /// as long as the device is still armed.
    pub(crate) fn batch(&self, now_ms: i64) -> Option<Batch> {
        if !self.armed() || self.pending.is_empty() || now_ms < self.retry_at_ms {
            return None;
        }
        Some(Batch {
            v: 1,
            session_id: self.session_id.clone()?,
            opt_in_id: self.opt_in_id.clone()?,
            device_id: self.device_id.clone(),
            points: self.pending.iter().take(MAX_BATCH).copied().collect(),
        })
    }
    /// The station stored (or already had) the batch: drop it from the buffer.
    pub(crate) fn sent(&mut self, batch: &Batch) {
        let seqs: std::collections::HashSet<u64> = batch.points.iter().map(|p| p.seq).collect();
        let before = self.pending.len();
        self.pending.retain(|p| !seqs.contains(&p.seq));
        self.uploaded += (before - self.pending.len()) as u64;
        self.failures = 0;
        self.retry_at_ms = 0;
        self.last_error = None;
    }
    pub(crate) fn failed(&mut self, batch: &Batch, error: &str, now_ms: i64) -> Failure {
        let failure = classify(error);
        match failure {
            Failure::Refused => self.disarm(),
            Failure::BadBatch => {
                let seqs: std::collections::HashSet<u64> = batch.points.iter().map(|p| p.seq).collect();
                self.pending.retain(|p| !seqs.contains(&p.seq));
            }
            Failure::Retry => {
                self.failures = self.failures.saturating_add(1);
                self.retry_at_ms = now_ms + backoff_ms(self.failures);
            }
        }
        self.last_error = Some(error.chars().take(200).collect());
        failure
    }
    /// Follows the station's status: arms from a still-valid opt-in (after a
    /// reload), disarms when the station no longer takes uploads or no
    /// longer lists this device as sharing.
    pub(crate) fn reconcile(&mut self, status: &Status) {
        let you = status.you.as_ref();
        if !status.accepting_uploads || status.session_id.is_none() {
            self.disarm();
            return;
        }
        if status.session_id != self.session_id {
            // A new session never inherits a buffer or an opt-in.
            self.disarm();
            self.session_id = None;
            return;
        }
        if status.active {
            match you {
                Some(you) if you.sharing => {
                    if self.opted_out_session.is_none()
                        && let Some(id) = you.opt_in_id.as_deref()
                    {
                        let session = status.session_id.clone().unwrap_or_default();
                        self.arm(&session, id, you.max_seq);
                    }
                }
                _ => self.disarm(),
            }
        }
    }
    /// Whether an opt-out must be (re)sent: the station still lists the
    /// device as sharing after the user stopped.
    pub(crate) fn opt_out_due(&self, status: &Status) -> bool {
        self.opted_out_session.is_some()
            && self.opted_out_session == status.session_id
            && status.you.as_ref().is_some_and(|y| y.sharing)
    }
}

// ---------------------------------------------------------------------------
// Local-only track

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LocalTrack {
    pub points: VecDeque<(f64, f64, i64)>,
}

impl LocalTrack {
    pub(crate) fn push(&mut self, lat: f64, lon: f64, t_ms: i64) {
        self.points.push_back((lat, lon, t_ms));
        while self.points.len() > MAX_LOCAL {
            self.points.pop_front();
        }
    }
    pub(crate) fn distance_m(&self) -> f64 {
        self.points.iter().zip(self.points.iter().skip(1)).map(|(a, b)| distance_m((a.0, a.1), (b.0, b.1))).sum()
    }
}

// ---------------------------------------------------------------------------
// Overlays

/// This device's own track as a MapOverlay drawn only here.
pub(crate) fn local_overlay(track: &LocalTrack) -> Option<Value> {
    if track.points.is_empty() {
        return None;
    }
    let points: Vec<Value> = track.points.iter().map(|p| json!([p.0, p.1, Value::Null])).collect();
    Some(json!({
        "id": "search_local",
        "kind": "search_coverage",
        "label": "My track (only on this device)",
        "predicted": false,
        "tracks": [{"id": "local", "phase": null, "label": "My track (not shared)", "style": "local", "color": LOCAL_TRACK_COLOR, "points": points}],
        "markers": [],
        "areas": [],
    }))
}

/// The overlays this provider draws: the station's coverage (while a session
/// exists) and the local-only track.
pub(crate) fn overlays(status: Option<&Status>, local: Option<Value>) -> Vec<Value> {
    let mut list = Vec::new();
    if let Some(status) = status
        && status.session_id.is_some()
        && status.overlay.is_object()
    {
        list.push(status.overlay.clone());
    }
    list.extend(local);
    list
}

/// The guarded call into the map (the map owner defines the function).
pub(crate) fn overlays_js(overlays: &[Value]) -> String {
    let json = serde_json::to_string(overlays).unwrap_or_else(|_| "[]".into());
    format!("window.setGroundMapOverlays?.({PROVIDER:?}, {json});")
}

// ---------------------------------------------------------------------------
// Predicted landing area

fn polygon_of(value: &Value) -> Option<Vec<[f64; 2]>> {
    let polygon: Vec<[f64; 2]> = value
        .as_array()?
        .iter()
        .filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?]))
        .collect();
    (polygon.len() >= 3).then_some(polygon)
}

/// The landing ellipse from `GET /api/los_prediction` (its overlay's
/// `landing_ellipse` area).
pub(crate) fn los_landing_polygon(los: &Value) -> Option<Vec<[f64; 2]>> {
    los["overlay"]["areas"]
        .as_array()?
        .iter()
        .find(|a| a["kind"] == "landing_ellipse")
        .and_then(|a| polygon_of(&a["polygon"]))
}

/// The landing ellipse of a flight plan document.
pub(crate) fn plan_landing_polygon(plan: &Value) -> Option<Vec<[f64; 2]>> {
    polygon_of(&plan["dispersion"]["landing_ellipse"]["polygon"])
}

pub(crate) fn parse_swath(text: &str) -> Result<f64, String> {
    let value: f64 = text.trim().parse().map_err(|_| "Enter the swath width in metres.".to_string())?;
    if !(SWATH_MIN_M..=SWATH_MAX_M).contains(&value) {
        return Err(format!("The swath width must be {SWATH_MIN_M:.0}–{SWATH_MAX_M:.0} m."));
    }
    Ok(value)
}

pub(crate) fn area_text(m2: f64) -> String {
    if m2 >= 100_000.0 { format!("{:.2} km²", m2 / 1e6) } else { format!("{m2:.0} m²") }
}

pub(crate) fn source_label(source: &str) -> &str {
    match source {
        "los_prediction" => "loss-of-signal prediction",
        "flight_plan" => "flight plan",
        "manual" => "set by an operator",
        other => other,
    }
}

pub(crate) fn new_device_id(random: u128) -> String {
    format!("srch-{random:032x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(active: bool, session: Option<&str>, you: Option<You>) -> Status {
        Status {
            active,
            session_id: session.map(Into::into),
            accepting_uploads: session.is_some(),
            you,
            overlay: json!({"id": "search_coverage:x", "kind": "search_coverage", "tracks": [], "areas": [], "markers": []}),
            ..Status::default()
        }
    }

    fn sharing_you(id: &str, max_seq: Option<u64>) -> Option<You> {
        Some(You { device_id: "d".into(), sharing: true, opt_in_id: Some(id.into()), max_seq, ..You::default() })
    }

    #[test]
    fn nothing_is_buffered_or_sent_without_an_opt_in_and_a_running_search() {
        let mut outbox = Outbox::new("gs|ada", "dev");
        assert_eq!(outbox.push(1, 31.0, -102.0, Some(5.0)), None, "not opted in");
        assert!(outbox.batch(0).is_none());
        // Station off: local only at most.
        assert_eq!(mode(None, &outbox, false), Mode::Off);
        assert_eq!(mode(Some(&status(false, None, None)), &outbox, true), Mode::LocalOnly);
        // A running search but no opt-in: still not shared.
        let running = status(true, Some("rs-1"), None);
        outbox.reconcile(&running);
        assert_eq!(mode(Some(&running), &outbox, true), Mode::LocalOnly);
        assert_eq!(outbox.push(2, 31.0, -102.0, None), None);
        // Opted in: shared.
        outbox.arm("rs-1", "opt", None);
        assert_eq!(mode(Some(&running), &outbox, false), Mode::Sharing);
        assert_eq!(outbox.push(3, 31.0, -102.0, None), Some(1));
        // The search stops: no more recording for sharing (uploads may finish).
        let stopped = Status { active: false, accepting_uploads: true, ..running.clone() };
        assert_eq!(mode(Some(&stopped), &outbox, false), Mode::Off);
        assert!(outbox.batch(0).is_some(), "buffered fixes still go during the late grace");
        // The grace ends: the buffer is discarded.
        outbox.reconcile(&Status { accepting_uploads: false, ..stopped });
        assert!(!outbox.armed() && outbox.pending.is_empty());
    }

    #[test]
    fn opting_out_or_a_refusal_discards_the_buffer() {
        let mut outbox = Outbox::new("gs|ada", "dev");
        outbox.arm("rs-1", "opt", None);
        for i in 0..5 {
            outbox.push(i, 31.0 + i as f64 * 1e-4, -102.0, None);
        }
        let batch = outbox.batch(0).unwrap();
        assert_eq!(outbox.failed(&batch, r#"HTTP 403: {"code":"not_opted_in","error":"x"}"#, 0), Failure::Refused);
        assert!(!outbox.armed() && outbox.pending.is_empty());
        // The station no longer lists the device as sharing.
        outbox.arm("rs-1", "opt", None);
        outbox.push(9, 31.0, -102.0, None);
        outbox.reconcile(&status(true, Some("rs-1"), Some(You { sharing: false, ..You::default() })));
        assert!(!outbox.armed() && outbox.pending.is_empty());
        // A new session never inherits an opt-in.
        outbox.arm("rs-1", "opt", None);
        outbox.reconcile(&status(true, Some("rs-2"), sharing_you("opt", None)));
        assert!(!outbox.armed());
    }

    #[test]
    fn offline_fixes_are_kept_retried_with_backoff_and_sent_in_order() {
        let mut outbox = Outbox::new("gs|ada", "dev");
        outbox.arm("rs-1", "opt", None);
        for i in 0..450 {
            outbox.push(1_000 + i, 31.0 + i as f64 * 1e-4, -102.0, Some(4.0));
        }
        let first = outbox.batch(0).unwrap();
        assert_eq!(first.points.len(), MAX_BATCH);
        assert_eq!(first.points.first().unwrap().seq, 1);
        // Offline: kept, retried after 2 s, then 4 s.
        assert_eq!(outbox.failed(&first, "error sending request: connection refused", 10_000), Failure::Retry);
        assert!(outbox.batch(11_000).is_none());
        let again = outbox.batch(12_000).unwrap();
        assert_eq!(again, first, "the same fixes, the same sequence numbers");
        assert_eq!(outbox.failed(&again, "HTTP 503: busy", 12_000), Failure::Retry);
        assert!(outbox.batch(15_999).is_none() && outbox.batch(16_000).is_some());
        assert_eq!(backoff_ms(10), 60_000);
        // Back in range: everything goes, oldest first.
        outbox.sent(&again);
        let second = outbox.batch(16_000).unwrap();
        assert_eq!(second.points.first().unwrap().seq, 201);
        outbox.sent(&second);
        let third = outbox.batch(16_000).unwrap();
        assert_eq!(third.points.len(), 50);
        outbox.sent(&third);
        assert!(outbox.pending.is_empty() && outbox.uploaded == 450);
    }

    #[test]
    fn the_buffer_is_bounded_and_survives_a_reload() {
        let mut outbox = Outbox::new("gs|ada", "dev");
        outbox.arm("rs-1", "opt", Some(41));
        assert_eq!(outbox.push(0, 31.0, -102.0, None), Some(42), "continues after the station's last seq");
        for i in 0..MAX_PENDING as i64 + 10 {
            outbox.push(i, 31.0, -102.0, None);
        }
        assert_eq!(outbox.pending.len(), MAX_PENDING);
        assert_eq!(outbox.dropped, 11);
        let raw = serde_json::to_string(&outbox).unwrap();
        let back = Outbox::parse(&raw).unwrap();
        assert_eq!(back, outbox);
        // A reload that re-reads the station's opt-in keeps the buffer and
        // never reuses a sequence number.
        let mut back = back;
        back.reconcile(&status(true, Some("rs-1"), sharing_you("opt", Some(10))));
        assert_eq!(back.pending.len(), MAX_PENDING);
        assert!(back.push(1, 31.0, -102.0, None).unwrap() > outbox.pending.back().unwrap().seq);
    }

    #[test]
    fn an_invalid_batch_is_dropped_but_sharing_continues() {
        let mut outbox = Outbox::new("gs|ada", "dev");
        outbox.arm("rs-1", "opt", None);
        outbox.push(1, 31.0, -102.0, None);
        let batch = outbox.batch(0).unwrap();
        assert_eq!(outbox.failed(&batch, r#"HTTP 400: {"code":"invalid"}"#, 0), Failure::BadBatch);
        assert!(outbox.armed() && outbox.pending.is_empty());
    }

    #[test]
    fn an_offline_opt_out_is_resent() {
        let mut outbox = Outbox::new("gs|ada", "dev");
        outbox.arm("rs-1", "opt", None);
        outbox.disarm();
        outbox.opted_out_session = Some("rs-1".into());
        let still = status(true, Some("rs-1"), sharing_you("opt", None));
        assert!(outbox.opt_out_due(&still));
        outbox.reconcile(&still);
        assert!(!outbox.armed(), "a pending opt-out is never undone by the station's stale state");
    }

    #[test]
    fn thinning_keeps_movement_and_a_heartbeat() {
        let mut thin = Thinner::default();
        assert!(thin.keep(31.0, -102.0, 0, Some(5.0)));
        assert!(!thin.keep(31.00001, -102.0, 1_000, Some(5.0)), "1 m");
        assert!(thin.keep(31.0001, -102.0, 2_000, Some(5.0)), "11 m");
        assert!(!thin.keep(31.0001, -102.0, 20_000, None));
        assert!(thin.keep(31.0001, -102.0, 32_000, None), "30 s heartbeat");
        assert!(!thin.keep(31.01, -102.0, 40_000, Some(250.0)), "poor accuracy");
        assert!(!thin.keep(0.0, 0.0, 50_000, None), "null island");
        assert!(!thin.keep(31.01, -102.0, 1_000, None), "older than the last fix");
    }

    #[test]
    fn overlays_hold_the_station_layer_and_the_local_track() {
        let mut local = LocalTrack::default();
        assert!(local_overlay(&local).is_none());
        local.push(31.0, -102.0, 0);
        local.push(31.001, -102.0, 1);
        assert!((local.distance_m() - 111.2).abs() < 1.0);
        let st = status(true, Some("rs-1"), None);
        let list = overlays(Some(&st), local_overlay(&local));
        assert_eq!(list.len(), 2);
        assert_eq!(list[1]["tracks"][0]["style"], "local");
        assert_eq!(overlays(Some(&status(false, None, None)), None), Vec::<Value>::new());
        let js = overlays_js(&list);
        assert!(js.starts_with("window.setGroundMapOverlays?.(\"search\", ["), "{js}");
        assert_eq!(overlays_js(&[]), "window.setGroundMapOverlays?.(\"search\", []);");
        for _ in 0..MAX_LOCAL + 5 {
            local.push(31.0, -102.0, 2);
        }
        assert_eq!(local.points.len(), MAX_LOCAL);
    }

    #[test]
    fn landing_polygons_come_from_the_prediction_or_a_plan() {
        let los = json!({"overlay": {"areas": [
            {"kind": "position_uncertainty", "polygon": [[1, 1], [1, 2], [2, 2]]},
            {"kind": "landing_ellipse", "polygon": [[31.76, -102.03], [31.761, -102.031], [31.762, -102.03]]}]}});
        assert_eq!(los_landing_polygon(&los).unwrap()[0], [31.76, -102.03]);
        assert!(los_landing_polygon(&json!({"overlay": null})).is_none());
        let plan = json!({"dispersion": {"landing_ellipse": {"polygon": [[1.0, 2.0], [1.0, 3.0], [2.0, 3.0]]}}});
        assert_eq!(plan_landing_polygon(&plan).unwrap().len(), 3);
        assert!(plan_landing_polygon(&json!({"dispersion": null})).is_none());
    }

    #[test]
    fn swath_and_labels() {
        assert_eq!(parse_swath(" 30 "), Ok(30.0));
        assert!(parse_swath("1").is_err() && parse_swath("x").is_err() && parse_swath("501").is_err());
        assert_eq!(area_text(5_000.0), "5000 m²");
        assert_eq!(area_text(250_000.0), "0.25 km²");
        assert_eq!(classify("HTTP 409: {\"code\":\"search_off\"}"), Failure::Refused);
        assert_eq!(classify("HTTP 429: slow down"), Failure::Retry);
    }
}
