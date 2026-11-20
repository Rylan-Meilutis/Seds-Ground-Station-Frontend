//! Integrated ground test (IGT): a real fill and motor fire on the stand while
//! the FC replays a recorded flight from its SD card (GroundStation26
//! `docs/integrated-ground-test.md`).
//!
//! This module is the client's single binding to the station's IGT contract:
//! the station mode string, the `IgtStatus` frame and `GET /api/igt`, the
//! prepare/confirm ticket routes, and how replayed (`SIM_SAMPLE`) data and the
//! `simulated` flag reach the UI. Everything here is presentation: the station
//! and the FC decide what is sent, accepted and fired.
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The station mode id (`/api/station_mode` `effective_mode`, wire code 4).
pub(crate) const MODE: &str = "integrated_ground_test";
pub(crate) const MODE_LABEL: &str = "Integrated ground test";
/// The phrase the operator types to arm LIVE PYRO (the station checks it too).
pub(crate) const LIVE_PYRO_PHRASE: &str = "LIVE PYRO";
pub(crate) const STATUS_PATH: &str = "/api/igt";
pub(crate) const PREPARE_PATH: &str = "/api/igt/prepare";
pub(crate) const CONFIRM_PATH: &str = "/api/igt/confirm";
/// Ticket-only actions: the plain command path refuses them.
pub(crate) const ENTER: &str = "IgtEnter";
pub(crate) const LIVE_PYRO: &str = "IgtLivePyro";
/// Plain command path (with `IgtDryRun`, which needs no dialog).
pub(crate) const EXIT: &str = "IgtExit";
pub(crate) const BANNER_TITLE: &str = "INTEGRATED GROUND TEST — SIMULATED FLIGHT DATA";

pub(crate) fn is_mode(effective_mode: Option<&str>) -> bool {
    effective_mode == Some(MODE)
}

/// Commands only the prepare/confirm routes may send.
pub(crate) fn ticket_only(cmd: &str) -> bool {
    matches!(cmd, ENTER | LIVE_PYRO)
}

// ---------------------------------------------------------------------------
// IgtStatus (`GET /api/igt`, websocket `IgtStatus`)
// ---------------------------------------------------------------------------

/// The station's view of the FC ground-test status (FC BOARD_HEALTH word 14).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct IgtStatus {
    /// The FC image was built with ground-test support.
    pub supported: bool,
    /// "off" | "validating" | "ready" | "sim" | "done" | "fault".
    pub fc_mode: String,
    pub live_pyro: bool,
    pub gps_simulated: bool,
    pub drogue_dry_run: bool,
    pub main_dry_run: bool,
    pub reason: u8,
    pub reason_text: String,
    pub station_mode_igt: bool,
    /// Wall time of the last FC report; null before the first one.
    pub updated_ms: Option<u64>,
}

pub(crate) static IGT_STATUS: GlobalSignal<Option<IgtStatus>> = Signal::global(|| None);

impl IgtStatus {
    pub(crate) fn mode(&self) -> &str {
        self.fc_mode.as_str()
    }
    pub(crate) fn ready(&self) -> bool {
        self.supported && self.mode() == "ready"
    }
    pub(crate) fn fault(&self) -> bool {
        self.mode() == "fault"
    }
    /// The FC's flight outputs come from the replay (or did, before landing).
    pub(crate) fn replaying_or_done(&self) -> bool {
        matches!(self.mode(), "sim" | "done")
    }
    pub(crate) fn mode_label(&self) -> &'static str {
        fc_mode_label(self.mode())
    }
    /// The reason in words: the station's text, else the documented code.
    pub(crate) fn reason_words(&self) -> Option<String> {
        let text = self.reason_text.trim();
        if !text.is_empty() {
            return Some(text.chars().filter(|c| !c.is_control()).take(240).collect());
        }
        reason_label(self.reason).map(str::to_owned)
    }
}

pub(crate) fn fc_mode_label(mode: &str) -> &'static str {
    match mode {
        "off" => "Off",
        "validating" => "Validating SD-card CSVs",
        "ready" => "Ready",
        "sim" => "SIM (replaying flight)",
        "done" => "Done (real sensors)",
        "fault" => "FAULT",
        _ => "no report",
    }
}

/// Reason codes, FC BOARD_HEALTH word 14 bits 12..15.
pub(crate) fn reason_label(code: u8) -> Option<&'static str> {
    Some(match code {
        1 => "SD card unavailable",
        2 => "CSV file missing",
        3 => "bad CSV header",
        4 => "CSV parse error or line too long",
        5 => "CSV time not increasing",
        6 => "CSV value out of range",
        7 => "CSV data too short",
        8 => "vehicle not stationary at Launch",
        9 => "real motion: replay abandoned, dry-run cleared",
        10 => "SD ring underrun (info)",
        11 => "refused in the FC's current state",
        12 => "replay data ended before landing (info)",
        _ => return None,
    })
}

/// IGT presentation applies when the station reports the IGT mode, or (with
/// no mode read yet) the IGT status says the station is in it.
pub(crate) fn active(mode_snapshot: &Value, status: Option<&IgtStatus>) -> bool {
    match mode_snapshot["effective_mode"].as_str() {
        Some(mode) => mode == MODE,
        None => status.is_some_and(|s| s.station_mode_igt),
    }
}

pub(crate) fn current_active() -> bool {
    active(&super::station_mode::current_snapshot(), IGT_STATUS.read().as_ref())
}

/// Stand presentation (thrust cards, the GSE panel at every phase): a test
/// fire, or an integrated ground test, which fires on the stand too. The AV
/// bay and flight panels follow the layout, which shows them in IGT as in
/// normal flight.
pub(crate) fn shows_stand_panels(layout_test_fire: bool) -> bool {
    layout_test_fire || current_active()
}

/// The FC is feeding (or fed) replayed data into its flight logic while the
/// station is in IGT.
pub(crate) fn current_simulated_flight() -> bool {
    current_active() && IGT_STATUS.read().as_ref().is_some_and(IgtStatus::replaying_or_done)
}

/// Store a status from the websocket or `/api/igt`, and reset the replayed
/// GPS track when a new run starts.
pub(crate) fn apply_status(status: IgtStatus) {
    if matches!(status.mode(), "off" | "validating" | "ready") {
        sim_track::clear();
    }
    sim_track::set_gps_simulated(Some(status.gps_simulated));
    if IGT_STATUS.peek().as_ref() != Some(&status) {
        *IGT_STATUS.write() = Some(status);
    }
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pyro {
    DryRun,
    Live,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Banner {
    pub title: &'static str,
    /// "FC IGT: Ready".
    pub fc: String,
    pub pyro: Pyro,
    pub pyro_text: &'static str,
    /// "GPS: simulated" / "GPS: real" / "GPS: unknown".
    pub gps: &'static str,
    /// FC fault, or a flight image without ground-test support.
    pub fault: Option<String>,
    /// Informational reason or recorded dry-run events.
    pub notes: Vec<String>,
}

/// The banner, only while IGT presentation applies.
pub(crate) fn banner(active: bool, status: Option<&IgtStatus>) -> Option<Banner> {
    if !active {
        return None;
    }
    let Some(s) = status else {
        return Some(Banner {
            title: BANNER_TITLE,
            fc: "FC IGT: no report yet".into(),
            pyro: Pyro::Unknown,
            pyro_text: "Pyros: unknown",
            gps: "GPS: unknown",
            fault: None,
            notes: Vec::new(),
        });
    };
    let pyro = if s.live_pyro { Pyro::Live } else { Pyro::DryRun };
    let fault = if !s.supported && s.updated_ms.is_some() {
        Some("FC has no ground-test support (flight image). Enter and the IGT Launch are refused.".into())
    } else if s.fault() {
        Some(format!("FC IGT fault: {}", s.reason_words().unwrap_or_else(|| format!("reason {}", s.reason))))
    } else {
        None
    };
    let mut notes = Vec::new();
    if !s.fault() && s.reason != 0 {
        if let Some(words) = s.reason_words() {
            notes.push(format!("FC note: {words}"));
        }
    }
    if s.drogue_dry_run {
        notes.push("Drogue dry-run recorded (not fired)".into());
    }
    if s.main_dry_run {
        notes.push("Main dry-run recorded (not fired)".into());
    }
    Some(Banner {
        title: BANNER_TITLE,
        fc: format!("FC IGT: {}", s.mode_label()),
        pyro,
        pyro_text: match pyro {
            Pyro::Live => "LIVE PYRO ARMED — drogue and main charges WILL FIRE",
            _ => "Pyros: DRY-RUN (logged, not fired)",
        },
        gps: if s.gps_simulated { "GPS: simulated" } else { "GPS: real" },
        fault,
        notes,
    })
}

// ---------------------------------------------------------------------------
// Prepare / confirm (IgtEnter, IgtLivePyro)
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone, PartialEq)]
pub(crate) struct PrepareRequest {
    pub action: &'static str,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub(crate) struct Prepared {
    pub ticket: String,
    #[serde(default)]
    pub expires_ms: Option<u64>,
    #[serde(default)]
    pub required_confirmation: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub(crate) struct ConfirmRequest {
    pub ticket: String,
    pub confirmation: Option<String>,
}

/// The prepare action for a ticket-only command.
pub(crate) fn prepare_action(cmd: &str) -> Option<&'static str> {
    match cmd {
        ENTER => Some("enter"),
        LIVE_PYRO => Some("live_pyro"),
        _ => None,
    }
}

/// Exactly `LIVE PYRO`: case, spacing and nothing else.
pub(crate) fn live_pyro_phrase_ok(typed: &str) -> bool {
    typed == LIVE_PYRO_PHRASE
}

/// The confirm body for a prepared ticket, or why it cannot be sent. The
/// station's required phrase is never substituted for what was typed.
pub(crate) fn confirm_request(prepared: &Prepared, typed: Option<&str>) -> Result<ConfirmRequest, String> {
    if prepared.ticket.trim().is_empty() {
        return Err("The station returned no ticket.".into());
    }
    let confirmation = match prepared.required_confirmation.as_deref() {
        None => None,
        Some(required) => {
            if typed != Some(required) {
                return Err(format!("Type {required} exactly to confirm."));
            }
            Some(required.to_owned())
        }
    };
    Ok(ConfirmRequest { ticket: prepared.ticket.clone(), confirmation })
}

/// The station's refusal text from `HTTP nnn: {"message"|"error": ...}`.
pub(crate) fn refusal(error: &str) -> String {
    let body = error.split_once(": ").map_or(error, |(_, body)| body);
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.get("message").and_then(Value::as_str).or_else(|| v.get("error").and_then(Value::as_str)).map(str::to_owned)
        })
        .unwrap_or_else(|| error.to_owned())
        .chars()
        .filter(|c| !c.is_control())
        .take(400)
        .collect()
}

/// Why the LIVE PYRO button is disabled, or None.
pub(crate) fn live_pyro_disabled_reason(
    status: Option<&IgtStatus>,
    admin: bool,
    connected: bool,
    rrls: bool,
) -> Option<String> {
    if rrls {
        return Some("LIVE PYRO is armed from the primary ground station.".into());
    }
    if !admin {
        return Some("Administrator required.".into());
    }
    if !connected {
        return Some("Not connected to the ground station.".into());
    }
    let Some(s) = status else {
        return Some("No FC ground-test status yet.".into());
    };
    if !s.supported {
        return Some("The FC has no ground-test support.".into());
    }
    if s.live_pyro {
        return Some("LIVE PYRO is already armed. Use Dry-run to disarm.".into());
    }
    if !s.ready() {
        return Some(format!("FC IGT must be Ready (now {}).", s.mode_label()));
    }
    None
}

/// Why the Enter button is disabled, or None.
pub(crate) fn enter_disabled_reason(
    status: Option<&IgtStatus>,
    permitted: bool,
    connected: bool,
    rrls: bool,
    station_igt: bool,
) -> Option<String> {
    if rrls {
        return Some("Ground test is entered from the primary ground station.".into());
    }
    if !permitted {
        return Some("Not permitted by your account.".into());
    }
    if !connected {
        return Some("Not connected to the ground station.".into());
    }
    if !station_igt {
        return Some("Switch the station to Integrated ground test first (Settings > Station).".into());
    }
    match status {
        Some(s) if s.updated_ms.is_some() && !s.supported => Some("The FC has no ground-test support (flight image).".into()),
        Some(s) if matches!(s.mode(), "validating" | "ready" | "sim") => {
            Some(format!("FC IGT is already {}.", s.mode_label()))
        }
        _ => None,
    }
}

pub(crate) const ENTER_TITLE: &str = "Enter integrated ground test?";
pub(crate) const ENTER_LINES: [&str; 4] = [
    "This is a REAL fill and a REAL motor fire on the stand, with the whole rocket powered as for flight.",
    "After Launch the FC stops using its flight sensors and replays a recorded flight from the CSV files on its SD card. Its real estimator, state machine and deployment logic run on that replayed data.",
    "Pyro outputs start DRY-RUN: drogue and main events are logged, not fired. LIVE PYRO is a separate, typed confirmation.",
    "The FC must be in Startup. It validates the CSVs now; a CSV problem shows as a fault with its file and line.",
];
pub(crate) const LIVE_PYRO_TITLE: &str = "Arm LIVE PYRO on the stand?";
pub(crate) const LIVE_PYRO_LINES: [&str; 3] = [
    "The REAL drogue (CO2) and line-cutter / main (REEF) charges WILL FIRE on the stand when the replayed flight reaches apogee and the reefing altitude.",
    "Only arm this with the charges you intend to fire installed, the stand area clear and the recovery hardware restrained.",
    "Dry-run disarms it at any time, including after Launch. Any FC reset or Abort returns the FC to Off.",
];

// ---------------------------------------------------------------------------
// SIMULATED labels
// ---------------------------------------------------------------------------

pub(crate) const SIMULATED: &str = "SIMULATED";

/// "SIMULATED · {text}", unless the text already says so.
pub(crate) fn tag_simulated(text: &str) -> String {
    if text.to_ascii_uppercase().contains(SIMULATED) {
        text.to_owned()
    } else {
        format!("{SIMULATED} · {text}")
    }
}

/// Estimator notice title while the FC runs on replayed input.
pub(crate) fn estimator_title(title: &str, simulated_input: bool) -> String {
    if !simulated_input || title.to_ascii_uppercase().contains(SIMULATED) {
        title.to_owned()
    } else {
        format!("SIMULATED input: {title}")
    }
}

/// Station notifications about a replayed flight ("SIMULATED flight: drogue
/// DRY-RUN (not fired) at 812 m") or a dry-run pulse ("IGT dry-run: ...").
pub(crate) fn is_simulated_message(message: &str) -> bool {
    let upper = message.trim_start().to_ascii_uppercase();
    upper.starts_with(SIMULATED) || upper.starts_with("IGT DRY-RUN")
}

// ---------------------------------------------------------------------------
// Replayed samples (SIM_SAMPLE)
// ---------------------------------------------------------------------------

/// The station's data type for replayed rows (`values = [kind, t, v0..v5]`).
pub(crate) const SIM_SAMPLE: &str = "SIM_SAMPLE";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SimKind {
    Imu,
    Baro,
    Gps,
    Ascent,
    Descent,
}

impl SimKind {
    pub(crate) fn from_code(code: f32) -> Option<Self> {
        if !code.is_finite() || code.fract() != 0.0 {
            return None;
        }
        Some(match code as i32 {
            1 => Self::Imu,
            2 => Self::Baro,
            3 => Self::Gps,
            4 => Self::Ascent,
            5 => Self::Descent,
            _ => return None,
        })
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Imu => "IMU (SIMULATED)",
            Self::Baro => "Barometer (SIMULATED)",
            Self::Gps => "GPS (SIMULATED)",
            Self::Ascent => "Ascent state (SIMULATED)",
            Self::Descent => "Descent state (SIMULATED)",
        }
    }
    fn from_suffix(suffix: &str) -> Option<Self> {
        Some(match suffix {
            "IMU" | "IMU_DATA" => Self::Imu,
            "BARO" | "BAROMETER" | "BAROMETER_DATA" => Self::Baro,
            "GPS" | "GPS_DATA" => Self::Gps,
            "ASCENT" | "ASCENT_STATE" => Self::Ascent,
            "DESCENT" | "DESCENT_STATE" => Self::Descent,
            _ => return None,
        })
    }
}

/// A data type that carries replayed data: `SIM_SAMPLE`, or a per-kind split
/// such as `SIM_IMU_DATA` / `SIM_GPS`.
pub(crate) fn is_simulated_type(data_type: &str) -> bool {
    data_type == SIM_SAMPLE || data_type.strip_prefix("SIM_").is_some_and(|s| SimKind::from_suffix(s).is_some())
}

/// The kind of a replayed row, and the index where its six values start.
pub(crate) fn sim_kind(data_type: &str, values: &[Option<f32>]) -> Option<(SimKind, usize)> {
    if data_type == SIM_SAMPLE {
        let kind = SimKind::from_code(values.first().copied().flatten()?)?;
        return Some((kind, 2));
    }
    let kind = SimKind::from_suffix(data_type.strip_prefix("SIM_")?)?;
    Some((kind, 0))
}

/// A replayed GPS fix (lat, lon) from a replayed row.
pub(crate) fn sim_gps(data_type: &str, values: &[Option<f32>]) -> Option<(f64, f64)> {
    let (SimKind::Gps, start) = sim_kind(data_type, values)? else {
        return None;
    };
    let lat = values.get(start).copied().flatten()? as f64;
    let lon = values.get(start + 1).copied().flatten()? as f64;
    (lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0 && (lat, lon) != (0.0, 0.0))
        .then_some((lat, lon))
}

/// A series label for a data type: replayed types say SIMULATED.
pub(crate) fn series_label(data_type: Option<&str>, label: &str) -> String {
    match data_type {
        // A bare per-kind type ("SIM_IMU_DATA") gets the kind's own name.
        Some(t) if label.trim().is_empty() || label == t => match t.strip_prefix("SIM_").and_then(SimKind::from_suffix) {
            Some(kind) => kind.label().to_owned(),
            None if t == SIM_SAMPLE => "Replayed flight (SIMULATED)".to_owned(),
            None => label.to_owned(),
        },
        Some(t) if is_simulated_type(t) && !label.to_ascii_uppercase().contains(SIMULATED) => {
            format!("{label} (SIMULATED)")
        }
        _ => label.to_owned(),
    }
}

/// Mark replayed series in a layout's data tabs (tab and subtab labels).
pub(crate) fn label_simulated_series(layout: &mut super::layout::DataTabLayout) {
    for tab in &mut layout.tabs {
        let tab_simulated = is_simulated_type(&tab.id);
        if tab_simulated {
            tab.label = series_label(Some(&tab.id), &tab.label);
        }
        for sub in tab.subtabs.iter_mut().flatten() {
            let source = sub.data_type.clone().unwrap_or_else(|| tab.id.clone());
            sub.label = series_label(Some(&source), &sub.label);
        }
    }
}

/// The replayed GPS track drawn on the map. Fed from telemetry ingestion
/// (no reactive context there), read by the map on a timer.
pub(crate) mod sim_track {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    const MAX_POINTS: usize = 4_000;

    #[derive(Default)]
    struct Track {
        points: Vec<(f64, f64)>,
        last_ms: i64,
        /// None until the station reports; then the FC's "GPS simulated" bit.
        gps_simulated: Option<bool>,
    }
    static TRACK: Mutex<Track> = Mutex::new(Track { points: Vec::new(), last_ms: i64::MIN, gps_simulated: None });
    static REVISION: AtomicU64 = AtomicU64::new(0);

    fn with<R>(f: impl FnOnce(&mut Track) -> R) -> Option<R> {
        TRACK.lock().ok().map(|mut t| f(&mut t))
    }

    pub(crate) fn revision() -> u64 {
        REVISION.load(Ordering::Acquire)
    }
    pub(crate) fn points() -> Vec<(f64, f64)> {
        with(|t| t.points.clone()).unwrap_or_default()
    }
    pub(crate) fn clear() {
        if with(|t| {
            let had = !t.points.is_empty();
            t.points.clear();
            t.last_ms = i64::MIN;
            had
        }) == Some(true)
        {
            REVISION.fetch_add(1, Ordering::AcqRel);
        }
    }
    pub(crate) fn set_gps_simulated(value: Option<bool>) {
        with(|t| t.gps_simulated = value);
    }
    /// Append a replayed fix. It stops when the FC switches GPS back to real.
    pub(crate) fn observe(timestamp_ms: i64, fix: (f64, f64)) {
        let added = with(|t| {
            if t.gps_simulated == Some(false) || timestamp_ms <= t.last_ms {
                return false;
            }
            t.last_ms = timestamp_ms;
            if t.points.last() == Some(&fix) {
                return false;
            }
            if t.points.len() >= MAX_POINTS {
                // Keep the shape: drop every other point of the older half.
                let half = t.points.len() / 2;
                let mut kept: Vec<(f64, f64)> = t.points[..half].iter().step_by(2).copied().collect();
                kept.extend_from_slice(&t.points[half..]);
                t.points = kept;
            }
            t.points.push(fix);
            true
        });
        if added == Some(true) {
            REVISION.fetch_add(1, Ordering::AcqRel);
        }
    }
    #[cfg(test)]
    pub(crate) fn reset_for_test() {
        with(|t| *t = Track { points: Vec::new(), last_ms: i64::MIN, gps_simulated: None });
    }
}

/// Telemetry ingestion hook: replayed GPS rows extend the SIMULATED track.
pub(crate) fn observe_row(row: &super::TelemetryRow) {
    if let Some(fix) = sim_gps(&row.data_type, &row.values) {
        sim_track::observe(row.timestamp_ms, fix);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status(value: Value) -> IgtStatus {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn mode_string_and_ticket_only_commands() {
        assert!(is_mode(Some("integrated_ground_test")));
        assert!(!is_mode(Some("test_fire")) && !is_mode(None));
        assert!(ticket_only("IgtEnter") && ticket_only("IgtLivePyro"));
        assert!(!ticket_only("IgtDryRun") && !ticket_only("IgtExit"));
        assert_eq!(prepare_action("IgtEnter"), Some("enter"));
        assert_eq!(prepare_action("IgtLivePyro"), Some("live_pyro"));
        assert_eq!(prepare_action("IgtExit"), None);
        let mode = json!({"effective_mode": "integrated_ground_test", "synchronized": true});
        assert_eq!(super::super::station_mode::description(&mode, false), "Operating mode: Integrated ground test");
    }

    #[test]
    fn igt_status_decodes_every_contract_field() {
        let s = status(json!({
            "supported": true, "fc_mode": "fault", "live_pyro": false, "gps_simulated": true,
            "drogue_dry_run": true, "main_dry_run": false, "reason": 3,
            "reason_text": "IGT IMU0.CSV:1 bad header", "station_mode_igt": true, "updated_ms": 1234
        }));
        assert!(s.supported && s.fault() && s.gps_simulated && s.drogue_dry_run && !s.main_dry_run);
        assert_eq!((s.reason, s.updated_ms), (3, Some(1234)));
        // Before the FC's first report the station sends updated_ms: null.
        let before = status(json!({"supported": false, "fc_mode": "off", "live_pyro": false, "gps_simulated": false,
            "drogue_dry_run": false, "main_dry_run": false, "reason": 0, "reason_text": "none",
            "station_mode_igt": true, "updated_ms": null}));
        assert_eq!(before.updated_ms, None);
        assert!(banner(true, Some(&before)).unwrap().fault.is_none(), "no report is not a flight image");
        assert_eq!(s.reason_words().as_deref(), Some("IGT IMU0.CSV:1 bad header"));
        // Missing fields default; an unknown future field is ignored.
        let sparse = status(json!({"fc_mode": "ready", "future": 1}));
        assert!(!sparse.supported && !sparse.ready(), "ready needs the support bit");
        assert_eq!(status(json!({"reason": 8})).reason_words().as_deref(), Some("vehicle not stationary at Launch"));
    }

    #[test]
    fn igt_status_websocket_frame_decodes_like_fc_estimator() {
        let frame: super::super::WsInMsg = serde_json::from_value(json!({
            "ty": "IgtStatus",
            "data": {"supported": true, "fc_mode": "sim", "live_pyro": true, "gps_simulated": true,
                     "drogue_dry_run": false, "main_dry_run": false, "reason": 0, "reason_text": "",
                     "station_mode_igt": true, "updated_ms": 99}
        }))
        .unwrap();
        let super::super::WsInMsg::IgtStatus(s) = frame else { panic!("not IgtStatus") };
        assert!(s.replaying_or_done() && s.live_pyro);
        assert!(super::super::incoming_epoch::mode_family("IgtStatus"));
    }

    #[test]
    fn banner_only_in_igt_and_says_dry_run_live_and_fault() {
        let igt = json!({"effective_mode": "integrated_ground_test"});
        let fire = json!({"effective_mode": "test_fire"});
        let ready = status(json!({"supported": true, "fc_mode": "ready", "station_mode_igt": true, "updated_ms": 1}));
        assert!(active(&igt, None));
        assert!(!active(&fire, Some(&ready)), "the station mode wins over a stale status");
        assert!(active(&Value::Null, Some(&ready)), "no mode read yet: the status says IGT");
        assert!(!active(&Value::Null, None));
        assert_eq!(banner(false, Some(&ready)), None);

        let b = banner(true, Some(&ready)).unwrap();
        assert_eq!(b.title, "INTEGRATED GROUND TEST — SIMULATED FLIGHT DATA");
        assert_eq!((b.fc.as_str(), b.pyro, b.gps), ("FC IGT: Ready", Pyro::DryRun, "GPS: real"));
        assert!(b.pyro_text.contains("DRY-RUN") && b.fault.is_none());

        let live = status(json!({"supported": true, "fc_mode": "sim", "live_pyro": true, "gps_simulated": true,
            "drogue_dry_run": false, "updated_ms": 2}));
        let b = banner(true, Some(&live)).unwrap();
        assert_eq!((b.pyro, b.gps), (Pyro::Live, "GPS: simulated"));
        assert!(b.pyro_text.starts_with("LIVE PYRO") && b.fc.contains("SIM"));

        let fault = status(json!({"supported": true, "fc_mode": "fault", "reason": 2,
            "reason_text": "IGT BARO0.CSV missing", "updated_ms": 3}));
        assert_eq!(banner(true, Some(&fault)).unwrap().fault.as_deref(), Some("FC IGT fault: IGT BARO0.CSV missing"));
        let coded = status(json!({"supported": true, "fc_mode": "fault", "reason": 9, "updated_ms": 3}));
        assert!(banner(true, Some(&coded)).unwrap().fault.unwrap().contains("real motion"));
        let flight_image = status(json!({"supported": false, "fc_mode": "off", "updated_ms": 3}));
        assert!(banner(true, Some(&flight_image)).unwrap().fault.unwrap().contains("no ground-test support"));

        let done = status(json!({"supported": true, "fc_mode": "done", "reason": 12, "drogue_dry_run": true,
            "main_dry_run": true, "updated_ms": 4}));
        let b = banner(true, Some(&done)).unwrap();
        assert!(b.fault.is_none());
        assert_eq!(b.notes, vec![
            "FC note: replay data ended before landing (info)".to_string(),
            "Drogue dry-run recorded (not fired)".to_string(),
            "Main dry-run recorded (not fired)".to_string(),
        ]);

        let unknown = banner(true, None).unwrap();
        assert_eq!((unknown.pyro, unknown.gps), (Pyro::Unknown, "GPS: unknown"));
    }

    #[test]
    fn live_pyro_phrase_is_exact() {
        assert!(live_pyro_phrase_ok("LIVE PYRO"));
        for wrong in ["live pyro", "LIVE  PYRO", " LIVE PYRO", "LIVE PYRO ", "LIVEPYRO", "LIVE PYR0", ""] {
            assert!(!live_pyro_phrase_ok(wrong), "{wrong:?}");
        }
    }

    #[test]
    fn confirm_body_carries_the_ticket_and_only_the_typed_phrase() {
        let enter: Prepared = serde_json::from_value(json!({"ticket": "t-1", "expires_ms": 15000,
            "required_confirmation": null})).unwrap();
        assert_eq!(confirm_request(&enter, None).unwrap(), ConfirmRequest { ticket: "t-1".into(), confirmation: None });
        let live: Prepared = serde_json::from_value(json!({"ticket": "t-2", "expires_ms": 15000,
            "required_confirmation": "LIVE PYRO"})).unwrap();
        assert!(confirm_request(&live, None).is_err());
        assert!(confirm_request(&live, Some("live pyro")).is_err());
        let body = confirm_request(&live, Some("LIVE PYRO")).unwrap();
        assert_eq!(serde_json::to_value(&body).unwrap(), json!({"ticket": "t-2", "confirmation": "LIVE PYRO"}));
        let empty: Prepared = serde_json::from_value(json!({"ticket": " "})).unwrap();
        assert!(confirm_request(&empty, None).is_err());
        assert_eq!(serde_json::to_value(PrepareRequest { action: "live_pyro" }).unwrap(), json!({"action": "live_pyro"}));
    }

    #[test]
    fn live_pyro_is_enabled_only_for_an_admin_when_fc_igt_is_ready() {
        let ready = status(json!({"supported": true, "fc_mode": "ready", "updated_ms": 1}));
        assert_eq!(live_pyro_disabled_reason(Some(&ready), true, true, false), None);
        assert!(live_pyro_disabled_reason(Some(&ready), false, true, false).unwrap().contains("Administrator"));
        assert!(live_pyro_disabled_reason(Some(&ready), true, false, false).is_some());
        assert!(live_pyro_disabled_reason(Some(&ready), true, true, true).is_some());
        assert!(live_pyro_disabled_reason(None, true, true, false).is_some());
        for mode in ["off", "validating", "sim", "done", "fault"] {
            let s = status(json!({"supported": true, "fc_mode": mode, "updated_ms": 1}));
            assert!(live_pyro_disabled_reason(Some(&s), true, true, false).unwrap().contains("must be Ready"), "{mode}");
        }
        let armed = status(json!({"supported": true, "fc_mode": "ready", "live_pyro": true}));
        assert!(live_pyro_disabled_reason(Some(&armed), true, true, false).unwrap().contains("already armed"));
    }

    #[test]
    fn enter_needs_the_igt_station_mode_and_an_idle_fc() {
        assert_eq!(enter_disabled_reason(None, true, true, false, true), None);
        assert!(enter_disabled_reason(None, true, true, false, false).unwrap().contains("Settings > Station"));
        assert!(enter_disabled_reason(None, false, true, false, true).is_some());
        let validating = status(json!({"supported": true, "fc_mode": "validating", "updated_ms": 1}));
        assert!(enter_disabled_reason(Some(&validating), true, true, false, true).is_some());
        let fault = status(json!({"supported": true, "fc_mode": "fault", "updated_ms": 1}));
        assert_eq!(enter_disabled_reason(Some(&fault), true, true, false, true), None, "re-enter clears a fault");
        let flight = status(json!({"supported": false, "fc_mode": "off", "updated_ms": 1}));
        assert!(enter_disabled_reason(Some(&flight), true, true, false, true).unwrap().contains("flight image"));
    }

    #[test]
    fn refusals_show_the_station_message() {
        assert_eq!(refusal(r#"HTTP 403: {"error":"Administrator required"}"#), "Administrator required");
        assert_eq!(refusal(r#"HTTP 409: {"error":"igt_refused","message":"FC IGT is not Ready"}"#), "FC IGT is not Ready");
        assert_eq!(refusal("network down"), "network down");
    }

    #[test]
    fn simulated_labels() {
        assert_eq!(tag_simulated("In flight · Descent"), "SIMULATED · In flight · Descent");
        assert_eq!(tag_simulated("SIMULATED flight"), "SIMULATED flight");
        assert_eq!(estimator_title("FC estimator: running", true), "SIMULATED input: FC estimator: running");
        assert_eq!(estimator_title("FC estimator: running", false), "FC estimator: running");
        assert_eq!(estimator_title("SIMULATED input: FC estimator", true), "SIMULATED input: FC estimator");
        assert!(is_simulated_message("SIMULATED flight: drogue DRY-RUN (not fired) at 812 m"));
        assert!(is_simulated_message("IGT dry-run: drogue not fired"));
        assert!(!is_simulated_message("Drogue deployed at 812 m"));
        assert_eq!(series_label(Some("SIM_SAMPLE"), "Replayed flight"), "Replayed flight (SIMULATED)");
        assert_eq!(series_label(Some("SIM_IMU_DATA"), "IMU"), "IMU (SIMULATED)");
        assert_eq!(series_label(Some("IMU_DATA"), "IMU"), "IMU");
        assert_eq!(series_label(Some("SIM_GPS"), "GPS (SIMULATED)"), "GPS (SIMULATED)");
        assert_eq!(series_label(Some("SIM_IMU_DATA"), "SIM_IMU_DATA"), "IMU (SIMULATED)");
        assert_eq!(series_label(Some("SIM_SAMPLE"), ""), "Replayed flight (SIMULATED)");
        assert_eq!(series_label(Some("GPS_DATA"), "GPS_DATA"), "GPS_DATA");
        assert!(!is_simulated_type("SIM_CARD_STATUS"), "only the replayed kinds");
    }

    #[test]
    fn replayed_rows_are_recognised_in_either_shape() {
        let v = |xs: &[f32]| xs.iter().copied().map(Some).collect::<Vec<_>>();
        assert_eq!(sim_kind("SIM_SAMPLE", &v(&[1.0, 0.5, 0.0, 0.0, 9.8, 0.0, 0.0, 0.0])), Some((SimKind::Imu, 2)));
        assert_eq!(sim_kind("SIM_SAMPLE", &v(&[7.0, 0.5])), None);
        assert_eq!(sim_kind("SIM_SAMPLE", &v(&[1.5])), None);
        assert_eq!(sim_kind("SIM_BAROMETER_DATA", &v(&[100.0])), Some((SimKind::Baro, 0)));
        assert_eq!(sim_kind("GPS_DATA", &v(&[31.7, -102.0])), None, "a normal type never carries replayed data");
        let fix = sim_gps("SIM_SAMPLE", &v(&[3.0, 12.0, 31.758, -102.046, 1200.0, 0.0, 0.0, 0.0])).unwrap();
        assert!((fix.0 - 31.758).abs() < 1e-4 && (fix.1 + 102.046).abs() < 1e-4);
        assert!(sim_gps("SIM_GPS_DATA", &v(&[31.758, -102.046, 1200.0])).is_some());
        assert_eq!(sim_gps("SIM_SAMPLE", &v(&[5.0, 12.0, -102.0, 31.7, 900.0, 0.0])), None, "descent state is not the GPS track");
        assert_eq!(sim_gps("SIM_SAMPLE", &v(&[3.0, 12.0, 0.0, 0.0])), None);
    }

    #[test]
    fn simulated_track_grows_until_gps_switches_back_to_real() {
        sim_track::reset_for_test();
        let before = sim_track::revision();
        sim_track::set_gps_simulated(Some(true));
        sim_track::observe(10, (31.0, -102.0));
        sim_track::observe(10, (31.1, -102.0)); // same time: a reseed duplicate
        sim_track::observe(20, (31.1, -102.0));
        assert_eq!(sim_track::points(), vec![(31.0, -102.0), (31.1, -102.0)]);
        assert!(sim_track::revision() > before);
        sim_track::set_gps_simulated(Some(false));
        sim_track::observe(30, (31.2, -102.0));
        assert_eq!(sim_track::points().len(), 2, "stops at the switch to real GPS");
        sim_track::clear();
        assert!(sim_track::points().is_empty());
        sim_track::reset_for_test();
    }
}
