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
    /// "ground_test" | "flight" | "unknown" (word 14 bit 7). Empty from an
    /// older station.
    pub fc_image: String,
    /// Launch (and GroundStationLaunch) refused: a ground-test image was seen
    /// in normal or test fire and no flight image has been reported since.
    pub launch_latched: bool,
    pub launch_latch_reason: Option<String>,
    /// Image warning or note for every view, or null.
    pub image_notice: Option<ImageNotice>,
    /// Ground-test presentation applies: integrated_ground_test, or HITL with
    /// the HITL ground-test replay on.
    pub igt_active: bool,
    pub mock_wet_dress: bool,
    pub hitl_replay: bool,
    /// FC SD card (BOARD_HEALTH word 6 bit 30, inverted); null before a report.
    pub sd_ready: Option<bool>,
    /// An `FcSdRetry` is waiting for the FC's SD task (word 6 bit 31).
    pub sd_retry_pending: bool,
}

/// `image_notice`: `{level: "warning" | "info", text}`.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct ImageNotice {
    pub level: String,
    pub text: String,
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
            return Some(clean(text, 240));
        }
        reason_label(self.reason).map(str::to_owned)
    }
}

/// Station text shown in the UI: no control characters, bounded.
fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_owned()
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
        9 => "reason 9 (unused)",
        10 => "SD ring underrun (info)",
        11 => "refused in the FC's current state",
        12 => "replay data ended before landing (info)",
        _ => return None,
    })
}

/// Ground-test presentation applies in integrated_ground_test, and in HITL
/// when the station reports the HITL ground-test replay on (`igt_active`).
/// Any other station mode wins over a stale status; with no mode read yet the
/// status decides.
pub(crate) fn active(mode_snapshot: &Value, status: Option<&IgtStatus>) -> bool {
    match mode_snapshot["effective_mode"].as_str() {
        Some(MODE) => true,
        Some(HITL_MODE) => status.is_some_and(|s| s.igt_active),
        Some(_) => false,
        None => status.is_some_and(|s| s.igt_active || s.station_mode_igt),
    }
}

pub(crate) const HITL_MODE: &str = "hitl";

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
    let fault = if s.fc_image == "flight" || (!s.supported && s.updated_ms.is_some()) {
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
// FC image notice and the Launch latch (section 7.1)
// ---------------------------------------------------------------------------

/// The station's latch text, used when a latched status carries none.
pub(crate) const LATCH_REASON: &str =
    "FC is running a ground-test image (dry-run capable): Launch is blocked until the FC reports a flight image";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NoticeView {
    /// Warning bar; otherwise an unobtrusive note.
    pub warning: bool,
    pub text: String,
}

/// The image notice to show on every view, in any station mode. Unknown
/// levels show as a note; empty text shows nothing.
pub(crate) fn image_notice(status: Option<&IgtStatus>) -> Option<NoticeView> {
    let notice = status?.image_notice.as_ref()?;
    let text = clean(&notice.text, 400);
    (!text.is_empty()).then(|| NoticeView { warning: notice.level.eq_ignore_ascii_case("warning"), text })
}

/// Why Launch is latched closed, or None.
pub(crate) fn launch_latch(status: Option<&IgtStatus>) -> Option<String> {
    let s = status.filter(|s| s.launch_latched)?;
    let reason = s.launch_latch_reason.as_deref().map(|r| clean(r, 400)).unwrap_or_default();
    Some(if reason.is_empty() { LATCH_REASON.to_owned() } else { reason })
}

pub(crate) fn current_launch_latch() -> Option<String> {
    launch_latch(IGT_STATUS.read().as_ref())
}

/// The full disabled reason for a Launch control: the latch reason wins over
/// a shorter policy reason, so it is never lost to truncation.
pub(crate) fn launch_reason(policy_reason: Option<&str>, latch: Option<&str>) -> Option<String> {
    match (latch, policy_reason) {
        (Some(latch), Some(policy)) if policy.contains(latch) => Some(policy.to_owned()),
        (Some(latch), _) => Some(latch.to_owned()),
        (None, policy) => policy.map(str::to_owned),
    }
}

// ---------------------------------------------------------------------------
// FC SD card (section 7.5)
// ---------------------------------------------------------------------------

pub(crate) const SD_RETRY: &str = "FcSdRetry";
pub(crate) const SD_NOT_READY: &str = "FC SD card not mounted / not writable";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SdView {
    pub warning: bool,
    pub text: String,
}

/// The FC SD state for the Retry SD control.
pub(crate) fn sd_view(status: Option<&IgtStatus>) -> SdView {
    let pending = status.is_some_and(|s| s.sd_retry_pending);
    let (warning, base) = match status.and_then(|s| s.sd_ready) {
        Some(true) => (false, "FC SD card ready"),
        Some(false) => (true, SD_NOT_READY),
        None => (false, "FC SD card: no report yet"),
    };
    let text = if pending { format!("{base} · Retry pending") } else { base.to_owned() };
    SdView { warning, text }
}

/// Retry SD is refused while one is still pending on the FC.
pub(crate) fn sd_retry_blocked(status: Option<&IgtStatus>) -> Option<&'static str> {
    status.is_some_and(|s| s.sd_retry_pending).then_some("An SD retry is already pending on the FC.")
}

// ---------------------------------------------------------------------------
// Ground-test settings: mock wet dress, HITL replay (sections 7.2, 7.3)
// ---------------------------------------------------------------------------

pub(crate) const SETTINGS_PATH: &str = "/api/igt/settings";

/// `GET /api/igt/settings` (and the PUT reply, when it echoes them).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct IgtSettings {
    pub mock_wet_dress: bool,
    pub hitl_replay: bool,
}

/// `PUT /api/igt/settings`: only the field being changed.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct SettingsUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mock_wet_dress: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hitl_replay: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Toggle {
    MockWetDress,
    HitlReplay,
}

impl Toggle {
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::MockWetDress => "mock_wet_dress",
            Self::HitlReplay => "hitl_replay",
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::MockWetDress => "Mock wet dress (flight fill steps)",
            Self::HitlReplay => "HITL ground-test replay",
        }
    }
    pub(crate) fn help(self) -> &'static str {
        match self {
            Self::MockWetDress => "The ground test runs the flight fill steps (nitrogen fill and the normally-open checks) instead of the test-fire fill sequence.",
            Self::HitlReplay => "HITL keeps its own fill side; the FC's SD-card replay is the flight data source, with the Ground test controls and SIMULATED labels.",
        }
    }
    /// The station modes that offer this toggle.
    pub(crate) fn offered(self, effective_mode: Option<&str>) -> bool {
        match self {
            Self::MockWetDress => effective_mode == Some(MODE),
            Self::HitlReplay => effective_mode == Some(HITL_MODE),
        }
    }
    pub(crate) fn value(self, status: Option<&IgtStatus>) -> Option<bool> {
        let s = status?;
        Some(match self {
            Self::MockWetDress => s.mock_wet_dress,
            Self::HitlReplay => s.hitl_replay,
        })
    }
    pub(crate) fn update(self, on: bool) -> SettingsUpdate {
        match self {
            Self::MockWetDress => SettingsUpdate { mock_wet_dress: Some(on), hitl_replay: None },
            Self::HitlReplay => SettingsUpdate { mock_wet_dress: None, hitl_replay: Some(on) },
        }
    }
    /// The confirm dialog for turning it `on` or off: (title, lines, button).
    pub(crate) fn confirm_copy(self, on: bool) -> (&'static str, &'static [&'static str], &'static str) {
        match (self, on) {
            (Self::MockWetDress, true) => ("Turn on mock wet dress?", &[
                "The integrated ground test will run the FLIGHT fill steps instead of the test-fire fill sequence: the nitrogen fill step and the normally-open checks.",
                "It is a station setting: it stays on for every operator, and across restarts, until an administrator turns it off.",
            ], "Turn on mock wet dress"),
            (Self::MockWetDress, false) => ("Turn off mock wet dress?", &[
                "The integrated ground test goes back to the test-fire fill sequence.",
            ], "Turn off mock wet dress"),
            (Self::HitlReplay, true) => ("Turn on the HITL ground-test replay?", &[
                "HITL keeps its own fill rules (HITL GSE path, interlocks and the bench or simulated plant), and the FC's SD-card CSV replay becomes the flight data source.",
                "The Ground test section, the IGT commands, the SIMULATED banner and labels appear as in Integrated ground test. Launch needs FC IGT Ready, and Advance and Rewind are refused while the FC replays.",
                "The FC must be running the ground-test image.",
            ], "Turn on HITL replay"),
            (Self::HitlReplay, false) => ("Turn off the HITL ground-test replay?", &[
                "HITL goes back to its normal flight data; the ground-test controls and SIMULATED labels go away.",
            ], "Turn off HITL replay"),
        }
    }
}

/// Why a settings toggle cannot be changed now, or None.
pub(crate) fn toggle_disabled_reason(admin: bool, connected: bool, rrls: bool, inhibited: bool) -> Option<String> {
    if rrls {
        Some("Ground-test settings are changed at the primary ground station.".into())
    } else if !admin {
        Some("Administrator required.".into())
    } else if inhibited {
        Some("Actions Disabled is on (header toggle).".into())
    } else if !connected {
        Some("Not connected to the ground station.".into())
    } else {
        None
    }
}

/// Fold a settings reply into the shown status.
pub(crate) fn apply_settings(settings: IgtSettings) {
    let Some(mut next) = IGT_STATUS.peek().clone() else { return };
    if (next.mock_wet_dress, next.hitl_replay) != (settings.mock_wet_dress, settings.hitl_replay) {
        next.mock_wet_dress = settings.mock_wet_dress;
        next.hitl_replay = settings.hitl_replay;
        *IGT_STATUS.write() = Some(next);
    }
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
        return Some("Switch the station to Integrated ground test (Settings > Station), or turn on the HITL ground-test replay in HITL, first.".into());
    }
    match status {
        Some(s) if s.fc_image == "flight" || (s.updated_ms.is_some() && !s.supported) => {
            Some("The FC has no ground-test support (flight image).".into())
        }
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
    "The REAL drogue (CO2) and main (REEF) charges WILL FIRE on the stand when the replayed flight reaches apogee and the reefing altitude.",
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
        let coded = status(json!({"supported": true, "fc_mode": "fault", "reason": 6, "updated_ms": 3}));
        assert!(banner(true, Some(&coded)).unwrap().fault.unwrap().contains("value out of range"));
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
    fn igt_status_decodes_the_round_two_fields() {
        let s = status(json!({
            "supported": true, "fc_mode": "off", "live_pyro": false, "gps_simulated": false,
            "drogue_dry_run": false, "main_dry_run": false, "reason": 0, "reason_text": "",
            "station_mode_igt": false, "updated_ms": 10,
            "fc_image": "ground_test", "launch_latched": true,
            "launch_latch_reason": LATCH_REASON,
            "image_notice": {"level": "warning", "text": "FC is running a ground-test image"},
            "igt_active": false, "mock_wet_dress": true, "hitl_replay": false,
            "sd_ready": false, "sd_retry_pending": true
        }));
        assert_eq!(s.fc_image, "ground_test");
        assert!(s.launch_latched && s.mock_wet_dress && !s.hitl_replay && !s.igt_active && s.sd_retry_pending);
        assert_eq!(s.launch_latch_reason.as_deref(), Some(LATCH_REASON));
        assert_eq!(s.image_notice, Some(ImageNotice { level: "warning".into(), text: "FC is running a ground-test image".into() }));
        assert_eq!(s.sd_ready, Some(false));
        // An older station: every new field defaults.
        let old = status(json!({"supported": true, "fc_mode": "ready", "station_mode_igt": true, "updated_ms": 1}));
        assert_eq!((old.fc_image.as_str(), old.launch_latched, old.launch_latch_reason.clone()), ("", false, None));
        assert_eq!((old.image_notice.clone(), old.igt_active, old.mock_wet_dress, old.hitl_replay), (None, false, false, false));
        assert_eq!((old.sd_ready, old.sd_retry_pending), (None, false));
        // Nulls decode as absent.
        let nulls = status(json!({"launch_latch_reason": null, "image_notice": null, "sd_ready": null}));
        assert_eq!((nulls.launch_latch_reason, nulls.image_notice, nulls.sd_ready), (None, None, None));
    }

    #[test]
    fn ground_test_presentation_keys_off_igt_active() {
        let hitl = json!({"effective_mode": "hitl"});
        let replay = status(json!({"supported": true, "igt_active": true, "hitl_replay": true, "updated_ms": 1}));
        let plain = status(json!({"igt_active": false, "updated_ms": 1}));
        assert!(active(&hitl, Some(&replay)), "HITL with the replay on");
        assert!(!active(&hitl, Some(&plain)) && !active(&hitl, None));
        assert!(banner(active(&hitl, Some(&replay)), Some(&replay)).is_some());
        assert!(active(&json!({"effective_mode": "integrated_ground_test"}), Some(&plain)), "the IGT mode always");
        for mode in ["normal", "test_fire", "testing"] {
            assert!(!active(&json!({"effective_mode": mode}), Some(&replay)), "{mode}: a stale status never wins");
        }
        assert!(active(&Value::Null, Some(&replay)), "no mode read yet: the status decides");
        // Enter is open in HITL with the replay (the caller passes `active`).
        assert_eq!(enter_disabled_reason(Some(&replay), true, true, false, true), None);
        assert!(enter_disabled_reason(Some(&plain), true, true, false, false).unwrap().contains("HITL ground-test replay"));
    }

    #[test]
    fn image_notice_by_level() {
        let warn = status(json!({"image_notice": {"level": "warning",
            "text": "FC is running a flight image: integrated ground test unavailable, flash the ground-test image"}}));
        let n = image_notice(Some(&warn)).unwrap();
        assert!(n.warning && n.text.starts_with("FC is running a flight image"));
        let info = status(json!({"image_notice": {"level": "info", "text": "FC is running the ground-test image"}}));
        assert!(!image_notice(Some(&info)).unwrap().warning);
        let odd = status(json!({"image_notice": {"level": "later", "text": "x"}}));
        assert!(!image_notice(Some(&odd)).unwrap().warning, "an unknown level is a note");
        let blank = status(json!({"image_notice": {"level": "warning", "text": " \u{7} "}}));
        assert_eq!(image_notice(Some(&blank)), None);
        assert_eq!(image_notice(Some(&status(json!({})))), None);
        assert_eq!(image_notice(None), None);
        // A flight image in IGT is a banner fault too, even before `supported` arrives.
        let flight = status(json!({"supported": false, "fc_image": "flight"}));
        assert!(banner(true, Some(&flight)).unwrap().fault.unwrap().contains("flight image"));
    }

    #[test]
    fn launch_latch_reason_is_shown_in_full() {
        let long = format!("{LATCH_REASON}. Flash the flight image and power-cycle the FC; the latch clears on its next BOARD_HEALTH.");
        let latched = status(json!({"launch_latched": true, "launch_latch_reason": long}));
        assert_eq!(launch_latch(Some(&latched)).as_deref(), Some(long.as_str()), "not truncated");
        let bare = status(json!({"launch_latched": true}));
        assert_eq!(launch_latch(Some(&bare)).as_deref(), Some(LATCH_REASON));
        let clear = status(json!({"launch_latched": false, "launch_latch_reason": "stale"}));
        assert_eq!(launch_latch(Some(&clear)), None);
        // The Launch control's reason: the latch wins; a policy text that
        // already carries it is kept whole.
        assert_eq!(launch_reason(Some("Fill not complete"), Some(LATCH_REASON)).as_deref(), Some(LATCH_REASON));
        let policy = format!("Launch blocked: {LATCH_REASON}");
        assert_eq!(launch_reason(Some(&policy), Some(LATCH_REASON)).as_deref(), Some(policy.as_str()));
        assert_eq!(launch_reason(Some("Fill not complete"), None).as_deref(), Some("Fill not complete"));
        assert_eq!(launch_reason(None, None), None);
    }

    #[test]
    fn sd_state_and_retry() {
        assert_eq!(sd_view(Some(&status(json!({"sd_ready": false})))), SdView { warning: true, text: SD_NOT_READY.into() });
        let pending = status(json!({"sd_ready": false, "sd_retry_pending": true}));
        assert_eq!(sd_view(Some(&pending)).text, "FC SD card not mounted / not writable · Retry pending");
        assert!(sd_retry_blocked(Some(&pending)).unwrap().contains("already pending"));
        let ok = status(json!({"sd_ready": true}));
        assert!(!sd_view(Some(&ok)).warning && sd_retry_blocked(Some(&ok)).is_none());
        assert!(sd_view(None).text.contains("no report"));
        assert_eq!(SD_RETRY, "FcSdRetry");
    }

    #[test]
    fn settings_toggles_are_admin_only_and_send_one_field() {
        assert_eq!(serde_json::to_value(Toggle::MockWetDress.update(true)).unwrap(), json!({"mock_wet_dress": true}));
        assert_eq!(serde_json::to_value(Toggle::MockWetDress.update(false)).unwrap(), json!({"mock_wet_dress": false}));
        assert_eq!(serde_json::to_value(Toggle::HitlReplay.update(true)).unwrap(), json!({"hitl_replay": true}));
        let got: IgtSettings = serde_json::from_value(json!({"mock_wet_dress": true, "hitl_replay": false})).unwrap();
        assert_eq!(got, IgtSettings { mock_wet_dress: true, hitl_replay: false });
        assert_eq!(SETTINGS_PATH, "/api/igt/settings");
        assert!(toggle_disabled_reason(false, true, false, false).unwrap().contains("Administrator"));
        assert_eq!(toggle_disabled_reason(true, true, false, false), None);
        assert!(toggle_disabled_reason(true, false, false, false).is_some());
        assert!(toggle_disabled_reason(true, true, true, false).is_some());
        assert!(toggle_disabled_reason(true, true, false, true).is_some());
        assert!(Toggle::MockWetDress.offered(Some("integrated_ground_test")) && !Toggle::MockWetDress.offered(Some("hitl")));
        assert!(Toggle::HitlReplay.offered(Some("hitl")) && !Toggle::HitlReplay.offered(Some("integrated_ground_test")));
        assert!(!Toggle::HitlReplay.offered(Some("normal")) && !Toggle::MockWetDress.offered(None));
        let s = status(json!({"mock_wet_dress": true, "hitl_replay": false}));
        assert_eq!((Toggle::MockWetDress.value(Some(&s)), Toggle::HitlReplay.value(Some(&s))), (Some(true), Some(false)));
        assert_eq!(Toggle::MockWetDress.value(None), None);
        for toggle in [Toggle::MockWetDress, Toggle::HitlReplay] {
            for on in [true, false] {
                let (title, lines, button) = toggle.confirm_copy(on);
                assert!(!title.is_empty() && !lines.is_empty() && !button.is_empty());
            }
        }
        assert!(Toggle::MockWetDress.confirm_copy(true).1.iter().any(|l| l.contains("flight fill steps") || l.contains("FLIGHT fill steps")));
    }

    #[test]
    fn the_two_pyro_channels_are_drogue_and_main_everywhere_in_the_ui() {
        // The FC has exactly two pyro channels, drogue and main, and the UI
        // names them only that way (no third, cutter-style name).
        let needles = [["line", " cutter"].concat(), ["line", "-cutter"].concat(), ["line", "cutter"].concat()];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut stack = vec![root.join("src"), root.join("static"), root.join("assets"), root.join("station_ui")];
        let mut hits = Vec::new();
        while let Some(path) = stack.pop() {
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            if meta.is_dir() {
                for entry in std::fs::read_dir(&path).unwrap().flatten() {
                    stack.push(entry.path());
                }
            } else if let Ok(text) = std::fs::read_to_string(&path) {
                let lower = text.to_ascii_lowercase();
                if needles.iter().any(|n| lower.contains(n.as_str())) {
                    hits.push(path.display().to_string());
                }
            }
        }
        assert!(hits.is_empty(), "a third pyro name in {hits:?}");
        assert!(LIVE_PYRO_LINES[0].contains("drogue (CO2) and main (REEF)"));
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
