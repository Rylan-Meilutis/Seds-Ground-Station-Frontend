#[derive(Deserialize, Debug)]
#[serde(tag = "ty", content = "data")]
enum WsInMsg {
    DashboardReset,
    CalibrationChanged,
    Telemetry(TelemetryRow),
    TelemetryBatch(Vec<TelemetryRow>),
    FlightState(FlightStateMsg),
    MissionPhase(mission_phase::MissionPhaseView),
    FlightMaxima(mission_phase::FlightMaximaView),
    LaunchClock(LaunchClockMsg),
    Warning(AlertMsg),
    Error(AlertMsg),
    AlertAckState(AlertAckStateMsg),
    AlertConditions(alert_conditions::Snapshot),
    BoardStatus(BoardStatusMsg),
    BoardAbort(BoardAbortSnapshot),
    NetworkTopology(NetworkTopologyMsg),
    Notifications(Vec<PersistentNotification>),
    Notification(PersistentNotification),
    Messages(Vec<PersistentNotification>),
    ActionPolicy(ActionPolicyMsg),
    FillTargets(FillTargetsConfig),
    RecordingStatus(RecordingStatusMsg),
    NetworkTime(NetworkTimeMsg),
    StationModeChanged(StationModeChangedMsg),
}

/// The backend switched operating mode inside its running process. A fresh
/// `ActionPolicy` frame follows; the layout and mode status are re-read.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
struct StationModeChangedMsg {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    policy_epoch: Option<String>,
    #[serde(default)]
    layout_revision: Option<u64>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "ty", content = "data")]
enum WsTelemetryIngressMsg {
    Telemetry(TelemetryRow),
    TelemetryBatch(Vec<TelemetryRow>),
}

#[derive(Deserialize, Debug, Clone)]
struct FlightStateMsg {
    state: FlightState,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AlertMsg {
    pub timestamp_ms: i64,
    pub message: String,
    #[serde(default,flatten)]
    pub condition:Option<alert_conditions::ConditionLink>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct AlertAckStateMsg {
    pub warning_ack_timestamp_ms: i64,
    pub error_ack_timestamp_ms: i64,
    /// Station-wide history clear: events at or before this time are gone.
    #[serde(default)]
    pub history_cleared_through_ms: i64,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlinkMode {
    None,
    Slow,
    Fast,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EpochFence {
    #[default]
    Ordinary,
    EmergencyStop,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ActionControl {
    #[serde(default)]
    pub epoch_fence: EpochFence,
    /// Opaque backend producer identity; echo original bytes, never reconstruct.
    #[serde(default)]
    pub control_origin: Option<String>,
    #[serde(default)]
    pub disabled_reason: Option<String>,
    pub cmd: String,
    pub enabled: bool,
    pub blink: BlinkMode,
    pub actuated: Option<bool>,
    /// Requested setting, separate from the confirmed state.
    #[serde(default)]
    pub requested: Option<bool>,
    /// Network-variable outputs (underglow, buzzer): "Waiting for network
    /// confirmation" while the GS has seen no network value at all.
    #[serde(default)]
    pub network_note: Option<String>,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ActionPolicyMsg {
    pub key_enabled: bool,
    #[serde(default = "default_software_buttons_enabled")]
    pub software_buttons_enabled: bool,
    #[serde(default = "default_interlock_satisfied")]
    pub hitl_button_interlock_satisfied: bool,
    #[serde(default = "default_interlock_satisfied")]
    pub hitl_launch_interlock_satisfied: bool,
    #[serde(default)]
    pub local_phase_controls: bool,
    /// A Launch press held behind the opt-in loadcell zero.
    #[serde(default)]
    pub launch_zero: Option<LaunchZeroStatus>,
    pub controls: Vec<ActionControl>,
}

/// Progress of the opt-in "Zero loadcell before Launch" capture.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LaunchZeroStatus {
    pub remaining_s: u64,
    #[serde(default)]
    pub samples: u32,
    /// The zero is finished and Launch is being sent; Cancel no longer applies.
    #[serde(default)]
    pub sending: bool,
}

impl ActionPolicyMsg {
    /// Returns the startup action policy before the backend publishes a real one.
    fn default_locked() -> Self {
        Self {
            key_enabled: false,
            software_buttons_enabled: true,
            hitl_button_interlock_satisfied: true,
            hitl_launch_interlock_satisfied: true,
            local_phase_controls: false,
            launch_zero: None,
            controls: Vec::new(),
        }
    }
}

/// Provides the serde default for software action buttons.
fn default_software_buttons_enabled() -> bool {
    true
}

fn default_interlock_satisfied() -> bool {
    true
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PersistentNotification {
    pub id: u64,
    pub timestamp_ms: i64,
    pub message: String,
    #[serde(default = "default_notification_persistent")]
    pub persistent: bool,
    #[serde(default)]
    pub action_label: Option<String>,
    #[serde(default)]
    pub action_cmd: Option<String>,
}

/// Provides the serde default for notification persistence.
fn default_notification_persistent() -> bool {
    true
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct DismissedNotification {
    id: u64,
    timestamp_ms: i64,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct NetworkTimeMsg {
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct FluidFillTarget {
    pub target_mass_kg: f32,
    pub target_pressure_psi: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct FillTargetsConfig {
    #[serde(default = "default_fill_source")]
    pub fill_source: String,
    pub version: u32,
    pub nitrogen: FluidFillTarget,
    pub nitrous: FluidFillTarget,
}

fn default_fill_source() -> String {
    "kg50".into()
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LaunchClockKind {
    Idle,
    TMinus,
    TPlus,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
struct LaunchClockMsg {
    kind: LaunchClockKind,
    anchor_timestamp_ms: Option<i64>,
    duration_ms: Option<i64>,
}

const DEFAULT_LAUNCH_COUNTDOWN_DURATION_MS: i64 = 10_000;

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordingStatusMsg {
    mode: String,
    db_path: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
struct TranslationCatalogResponse {
    lang: String,
    translations: HashMap<String, String>,
}

#[derive(Serialize, Debug, Clone, Default)]
struct TranslationRequest {
    target_lang: String,
    texts: Vec<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
struct TranslationResponse {
    lang: String,
    translations: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct NetworkTimeSync {
    network_ms: i64,
    received_mono_ms: f64,
}

#[cfg(target_arch = "wasm32")]
/// Browser render and freshness deadlines must survive wall-clock corrections.
fn monotonic_now_ms() -> f64 {
    use wasm_bindgen::JsCast;
    let performance = js_sys::Reflect::get(&js_sys::global(), &"performance".into())
        .expect("browser performance clock must be available");
    let now = js_sys::Reflect::get(&performance, &"now".into())
        .expect("browser performance.now must be available")
        .unchecked_into::<js_sys::Function>();
    now.call0(&performance)
        .expect("browser performance.now must succeed")
        .as_f64()
        .expect("browser performance.now must return milliseconds")
}

#[cfg(not(target_arch = "wasm32"))]
/// Returns a monotonic timestamp source for rate calculations on native builds.
fn monotonic_now_ms() -> f64 {
    use std::sync::OnceLock;
    use std::time::Instant;

    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

#[inline]
/// Projects the last synced network time forward using monotonic elapsed time.
fn compensated_network_time_ms(sync: NetworkTimeSync) -> i64 {
    let elapsed_ms = (monotonic_now_ms() - sync.received_mono_ms)
        .max(0.0)
        .round() as i64;
    sync.network_ms.saturating_add(elapsed_ms)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn format_timestamp_ms_clock(ms_epoch: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms_epoch as f64));
    let h24 = d.get_hours();
    let m = d.get_minutes();
    let s = d.get_seconds();
    let cs = (d.get_milliseconds() / 10).clamp(0, 99);
    if *PREFERRED_CLOCK_24H.read() {
        format!("{h24:02}:{m:02}:{s:02}:{cs:02}")
    } else {
        let (h, am_pm) = match h24 {
            0 => (12, "AM"),
            1..=11 => (h24, "AM"),
            12 => (12, "PM"),
            _ => (h24 - 12, "PM"),
        };
        format!("{h:02}:{m:02}:{s:02}:{cs:02} {am_pm}")
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn format_timestamp_ms_clock(ms_epoch: i64) -> String {
    use chrono::{Local, TimeZone};
    let Some(dt) = Local.timestamp_millis_opt(ms_epoch).single() else {
        return "--:--:--:--".to_string();
    };
    let cs = dt.timestamp_subsec_millis() / 10;
    if *PREFERRED_CLOCK_24H.read() {
        format!("{}:{cs:02}", dt.format("%H:%M:%S"))
    } else {
        format!("{}:{cs:02} {}", dt.format("%I:%M:%S"), dt.format("%p"))
    }
}

/// Formats the network-synchronized wall clock for dashboard display, to the
/// second. Hundredths are reserved for the launch clock.
fn format_network_time(ms_epoch: i64) -> String {
    let full = format_timestamp_ms_clock(ms_epoch);
    // "HH:MM:SS:cc[ AM]" -> "HH:MM:SS[ AM]"
    match full.get(8..11) {
        Some(centis) if centis.starts_with(':') => format!("{}{}", &full[..8], &full[11..]),
        _ => full,
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn format_timestamp_ms_local_datetime(ms_epoch: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms_epoch as f64));
    let year = d.get_full_year();
    let month = d.get_month() + 1;
    let day = d.get_date();
    format!(
        "{year:04}-{month:02}-{day:02} {}",
        format_timestamp_ms_clock(ms_epoch)
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn format_timestamp_ms_local_datetime(ms_epoch: i64) -> String {
    use chrono::{Local, TimeZone};
    let Some(dt) = Local.timestamp_millis_opt(ms_epoch).single() else {
        return "--".to_string();
    };
    format!(
        "{} {}",
        dt.format("%Y-%m-%d"),
        format_timestamp_ms_clock(ms_epoch)
    )
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn device_timezone_label() -> String {
    js_eval(
        r#"
        (function() {
          try {
            const tz = Intl.DateTimeFormat().resolvedOptions().timeZone || "";
            const mins = -new Date().getTimezoneOffset();
            const sign = mins >= 0 ? "+" : "-";
            const abs = Math.abs(mins);
            const hh = String(Math.floor(abs / 60)).padStart(2, "0");
            const mm = String(abs % 60).padStart(2, "0");
            window.__gs26_tmp_timezone = tz ? `${tz} (UTC${sign}${hh}:${mm})` : `UTC${sign}${hh}:${mm}`;
          } catch (_) {
            window.__gs26_tmp_timezone = "Local device time";
          }
        })();
        "#,
    );
    js_read_window_string("__gs26_tmp_timezone").unwrap_or_else(|| "Local device time".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn device_timezone_label() -> String {
    use chrono::Local;
    Local::now().format("%Z (UTC%:z)").to_string()
}

// --------------------------
// DB alert DTO (/api/alerts)
// --------------------------
#[derive(Deserialize, Debug, Clone)]
struct AlertDto {
    pub timestamp_ms: i64,
    pub severity: String, // "warning" | "error"
    pub message: String,
    #[serde(default,flatten)]
    pub condition:Option<alert_conditions::ConditionLink>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MainTab {
    Checklists,
    State,
    ConnectionStatus,
    Detailed,
    NetworkTopology,
    Map,
    Actions,
    FirmwareUpdate,
    Calibration,
    Mission,
    CrewVoice,
    StreamManager,
    Media,
    Vehicle,
    Messages,
    Notifications,
    Warnings,
    Errors,
    Data,
    DataExport,
    SequenceHistory,
}

/// Active lifecycle errors are server-authoritative and separate from acknowledged history.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
struct BoardAbortSnapshot {
    version: u64,
    #[serde(default)]
    active_errors: Vec<BoardAbortError>,
    #[serde(default)]
    required_boards: Vec<String>,
    #[serde(default)]
    missing_required_state: Option<Vec<String>>,
    #[serde(default)]
    history_error: HistoryErrorReport,
    #[serde(default)]
    history_unknown_boards: Vec<String>,
    #[serde(default)]
    retraction: Option<RetractionInterlock>,
    /// Server verdict: HITL reports unreadable Abort history without blocking.
    #[serde(default = "server_launch_gate_default")]
    history_blocks_launch: bool,
    #[serde(skip)]
    source_epoch: Option<u64>,
    #[serde(skip)]
    local_revision: u64,
}
/// Missing is not a report that a previously active persistence fault cleared.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum HistoryErrorReport {
    #[default]
    Unreported,
    Reported(Option<String>),
}
impl<'de> serde::Deserialize<'de> for HistoryErrorReport {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<String>::deserialize(deserializer).map(Self::Reported)
    }
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct RetractionInterlock {
    configured: bool,
    enforced: bool,
    allowed: bool,
    phase: String,
    remaining_ms: Option<u32>,
    message: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct BoardAbortError {
    source: String,
    board: String,
    board_label: String,
    timestamp_ms: u64,
    message: String,
    /// Server verdict: false when the aborted board is no longer on the network.
    #[serde(default = "server_launch_gate_default")]
    launch_blocking: bool,
    /// Server verdict: the operator may dismiss it from the list (HITL).
    #[serde(default)]
    dismissible: bool,
}
fn server_launch_gate_default() -> bool {
    true
}
impl BoardAbortError {
    fn dismissal_key(&self) -> (String, u64) {
        (self.board.clone(), self.timestamp_ms)
    }
}
/// Dismissals survive a reload: one abort occurrence stays dismissed until the
/// board aborts again (a new abort time).
const DISMISSED_BOARD_ABORTS_STORAGE_KEY: &str = "gs_dismissed_board_aborts";
const DISMISSED_BOARD_ABORTS_KEPT: usize = 64;
fn load_dismissed_board_aborts() -> std::collections::BTreeSet<(String, u64)> {
    persist::get_string(DISMISSED_BOARD_ABORTS_STORAGE_KEY)
        .and_then(|text| serde_json::from_str::<Vec<(String, u64)>>(&text).ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}
fn dismiss_board_abort(key: (String, u64)) {
    let mut dismissed = DISMISSED_BOARD_ABORTS.write();
    dismissed.insert(key);
    // Keep the newest abort times only.
    let mut entries: Vec<_> = dismissed.iter().cloned().collect();
    entries.sort_by_key(|(_, at)| std::cmp::Reverse(*at));
    entries.truncate(DISMISSED_BOARD_ABORTS_KEPT);
    *dismissed = entries.iter().cloned().collect();
    if let Ok(text) = serde_json::to_string(&entries) {
        persist::set_string(DISMISSED_BOARD_ABORTS_STORAGE_KEY, &text);
    }
}
impl BoardAbortSnapshot {
    fn launch_blocking_errors(&self) -> impl Iterator<Item = &BoardAbortError> {
        self.active_errors.iter().filter(|error| error.launch_blocking)
    }
    fn launch_blocking_history_error(&self) -> Option<&str> {
        self.history_error_message().filter(|_| self.history_blocks_launch)
    }
    fn reconciled(mut self, previous: &Self, source_epoch: u64) -> Self {
        if previous.source_epoch == Some(source_epoch) && self.version < previous.version {
            return previous.clone();
        }
        if matches!(self.history_error, HistoryErrorReport::Unreported) {
            self.history_error = previous.history_error.clone();
        }
        if self.missing_required_state.is_none() {
            self.missing_required_state = previous.missing_required_state.clone();
        }
        self.source_epoch = Some(source_epoch);
        self.local_revision = previous.local_revision.saturating_add(1);
        self
    }
    fn history_error_message(&self) -> Option<&str> {
        match &self.history_error {
            HistoryErrorReport::Reported(Some(message)) => Some(if message.trim().is_empty() {
                "The station cannot verify saved Abort history."
            } else {
                message
            }),
            _ => None,
        }
    }
    fn active_error_count(&self) -> usize {
        self.active_error_count_visible(&Default::default())
    }
    /// Active errors still shown: dismissed AV-bay aborts are left out.
    fn visible_active_errors(&self, dismissed: &std::collections::BTreeSet<(String, u64)>) -> Vec<BoardAbortError> {
        self.active_errors.iter().filter(|error| !dismissed.contains(&error.dismissal_key())).cloned().collect()
    }
    fn active_error_count_visible(&self, dismissed: &std::collections::BTreeSet<(String, u64)>) -> usize {
        self.visible_active_errors(dismissed).len() + usize::from(self.history_error_message().is_some())
    }
    fn missing_required_state(&self) -> &[String] {
        self.missing_required_state.as_deref().unwrap_or_default()
    }
    fn constrain_launch_policy(&self, policy: &mut ActionPolicyMsg) {
        if self.launch_blocking_errors().next().is_none()
            && self.launch_blocking_history_error().is_none()
            && self.missing_required_state().is_empty()
            && !self
                .retraction
                .as_ref()
                .is_some_and(|gate| gate.configured && !gate.allowed)
        {
            return;
        }
        for control in &mut policy.controls {
            if policy.local_phase_controls && matches!(control.cmd.as_str(),"AdvanceFlightState"|"RewindFlightState") {continue;}
            if matches!(
                control.cmd.as_str(),
                "Launch" | "GroundStationLaunch" | "AdvanceFlightState" | "RewindFlightState"
            ) {
                control.enabled = false;
                control.blink = BlinkMode::None;
            }
        }
    }
    fn launch_message(&self) -> Option<String> {
        if let Some(message) = self.launch_blocking_history_error() {
            return Some(format!(
                "Launch disabled: Abort history unavailable: {message}"
            ));
        }
        if self.launch_blocking_errors().next().is_some() {
            return Some(format!(
                "Launch disabled: {} report Abort.",
                self.launch_blocking_errors()
                    .map(|e| e.board_label.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        (!self.missing_required_state().is_empty()).then(|| {
            format!(
                "Launch disabled: fresh required board state is missing for {}.",
                self.missing_required_state().join(", ")
            )
        })
    }
}
#[cfg(test)]
mod board_abort_message_tests {
    use super::*;
    #[test]
    fn dismissed_aborts_hide_until_the_board_aborts_again() {
        let snapshot: BoardAbortSnapshot = serde_json::from_value(serde_json::json!({"version":1,"active_errors":[
            {"source":"board_abort:RF","board":"RF","board_label":"RF Board","timestamp_ms":10,"message":"m"},
            {"source":"board_abort:VB","board":"VB","board_label":"Valve Board","timestamp_ms":11,"message":"m"}]})).unwrap();
        let rf = snapshot.active_errors[0].clone();
        let mut dismissed = std::collections::BTreeSet::new();
        dismissed.insert(rf.dismissal_key());
        assert_eq!(snapshot.active_error_count_visible(&dismissed), 1);
        assert_eq!(snapshot.visible_active_errors(&dismissed)[0].board, "VB");
        let mut again = snapshot.clone();
        again.active_errors[0].timestamp_ms = 99;
        assert_eq!(again.active_error_count_visible(&dismissed), 2, "a new Abort shows again");
    }
    #[test]
    fn absent_board_abort_stays_visible_without_disabling_launch() {
        let error=|blocking:bool|serde_json::json!({"source":"board_abort:VB","board":"VB","board_label":"Valve Board",
            "timestamp_ms":1,"message":"m","launch_blocking":blocking});
        let snapshot=|blocking:bool|serde_json::from_value::<BoardAbortSnapshot>(
            serde_json::json!({"version":1,"active_errors":[error(blocking)]})).unwrap();
        let absent=snapshot(false);
        assert_eq!(absent.active_error_count(),1);
        assert!(absent.launch_message().is_none());
        assert!(snapshot(true).launch_message().unwrap().contains("Valve Board"));
        // A station without the field keeps blocking.
        let legacy:BoardAbortSnapshot=serde_json::from_value(serde_json::json!({"version":1,"active_errors":[
            {"source":"s","board":"VB","board_label":"Valve Board","timestamp_ms":1,"message":"m"}]})).unwrap();
        assert!(legacy.launch_message().is_some());
    }
    #[test]
    fn hitl_history_error_is_reported_without_disabling_launch() {
        let report=|blocks:bool|serde_json::from_value::<BoardAbortSnapshot>(serde_json::json!({
            "version":1,"history_error":"Journal corrupt","history_blocks_launch":blocks})).unwrap();
        assert!(report(false).launch_message().is_none());
        assert_eq!(report(false).active_error_count(),1);
        assert!(report(true).launch_message().unwrap().contains("Journal corrupt"));
    }
    fn snapshot(value: serde_json::Value) -> BoardAbortSnapshot {
        serde_json::from_value(value).unwrap()
    }
    #[test]
    fn history_absence_stale_report_and_explicit_current_clear_are_distinct() {
        let active = snapshot(serde_json::json!({"version":9,"history_error":"Journal corrupt","missing_required_state":["FC"]}))
            .reconciled(&BoardAbortSnapshot::default(), 1);
        let absent = snapshot(serde_json::json!({"version":10})).reconciled(&active, 1);
        assert_eq!(absent.history_error_message(), Some("Journal corrupt"));
        assert_eq!(absent.missing_required_state(), ["FC"]);
        let stale = snapshot(
            serde_json::json!({"version":8,"history_error":null,"missing_required_state":[]}),
        )
        .reconciled(&absent, 1);
        assert_eq!(stale, absent);
        let clear = snapshot(
            serde_json::json!({"version":11,"history_error":null,"missing_required_state":[]}),
        )
        .reconciled(&stale, 1);
        assert!(clear.history_error_message().is_none());
        assert!(clear.missing_required_state().is_empty());
        assert!(clear.local_revision > active.local_revision);
    }
    #[test]
    fn current_new_connection_can_report_a_new_revision_but_absence_does_not_clear_fault() {
        let active = snapshot(serde_json::json!({"version":90,"history_error":"Storage failed"}))
            .reconciled(&BoardAbortSnapshot::default(), 1);
        let reconnect = snapshot(serde_json::json!({"version":1})).reconciled(&active, 2);
        assert_eq!(reconnect.version, 1);
        assert_eq!(reconnect.history_error_message(), Some("Storage failed"));
        let clear = snapshot(serde_json::json!({"version":2,"history_error":null}))
            .reconciled(&reconnect, 2);
        assert!(clear.history_error_message().is_none());
    }
    #[test]
    fn history_fault_or_applicable_missing_proof_constrains_every_launch_family_only() {
        let policy = || {
            let controls = [
                "Launch",
                "GroundStationLaunch",
                "AdvanceFlightState",
                "RewindFlightState",
                "Abort",
                "ToggleAvBayUnderglow",
            ]
            .map(
                |cmd| serde_json::json!({"cmd":cmd,"enabled":true,"blink":"slow","actuated":false}),
            );
            serde_json::from_value::<ActionPolicyMsg>(serde_json::json!({
                "key_enabled":true,"software_buttons_enabled":true,"controls":controls
            }))
            .unwrap()
        };
        for value in [
            serde_json::json!({"version":1,"history_error":"Unreadable"}),
            serde_json::json!({"version":1,"missing_required_state":["FC"]}),
        ] {
            let current = snapshot(value);
            let mut next = policy();
            current.constrain_launch_policy(&mut next);
            assert!(next.controls[..4].iter().all(|c| !c.enabled));
            assert!(next.controls[4..].iter().all(|c| c.enabled));
            assert!(current.launch_message().is_some());
            let mut local=policy(); local.local_phase_controls=true;
            local.controls[3].enabled=false; // server boundary denial stays denied
            current.constrain_launch_policy(&mut local);
            assert!(!local.controls[0].enabled && !local.controls[1].enabled);
            assert!(local.controls[2].enabled);
            assert!(!local.controls[3].enabled);
            assert!(local.controls[4..].iter().all(|c|c.enabled));
        }
        // HITL proof enforcement is server-scoped, never inferred from raw history.
        let optional = snapshot(
            serde_json::json!({"version":2,"history_unknown_boards":["FC"],"missing_required_state":[],"history_error":null}),
        );
        let mut next = policy();
        optional.constrain_launch_policy(&mut next);
        assert!(next.controls.iter().all(|c| c.enabled));
        assert!(optional.launch_message().is_none());
    }
    #[test]
    fn late_policy_cannot_override_retained_board_abort_or_incomplete_motion_and_other_controls_remain()
     {
        let mut snapshot:BoardAbortSnapshot=serde_json::from_value(serde_json::json!({"version":1,"active_errors":[
            {"source":"board_abort:AB","board":"AB","board_label":"Actuator Board","timestamp_ms":1,"message":"Abort"}]})).unwrap();
        let fresh = || {
            serde_json::from_value::<ActionPolicyMsg>(
                serde_json::json!({"key_enabled":true,"software_buttons_enabled":true,"controls":[
            {"cmd":"Launch","enabled":true,"blink":"slow","actuated":false},
            {"cmd":"Abort","enabled":true,"blink":"none","actuated":false},
            {"cmd":"ToggleAvBayUnderglow","enabled":true,"blink":"none","actuated":false}]}),
            )
            .unwrap()
        };
        let mut policy = fresh();
        snapshot.constrain_launch_policy(&mut policy);
        assert!(!policy.controls[0].enabled);
        assert!(policy.controls[1].enabled && policy.controls[2].enabled);
        // An acknowledgment changes neither the separate live snapshot nor its
        // authority, so a later enabled policy still cannot re-enable Launch.
        let mut late = fresh();
        snapshot.constrain_launch_policy(&mut late);
        assert!(!late.controls[0].enabled);
        snapshot.active_errors.clear();
        snapshot.retraction = Some(RetractionInterlock {
            configured: true,
            allowed: false,
            ..Default::default()
        });
        let mut late = fresh();
        snapshot.constrain_launch_policy(&mut late);
        assert!(!late.controls[0].enabled);
        snapshot.retraction.as_mut().unwrap().allowed = true;
        let mut fresh = fresh();
        snapshot.constrain_launch_policy(&mut fresh);
        assert!(fresh.controls[0].enabled);
    }
    #[test]
    fn all_active_board_names_remain_until_individually_cleared() {
        let mut snapshot:BoardAbortSnapshot=serde_json::from_value(serde_json::json!({"version":1,"active_errors":[
            {"source":"board_abort:AB","board":"AB","board_label":"Actuator Board","timestamp_ms":1,"message":"Abort"},
            {"source":"board_abort:DAQ","board":"DAQ","board_label":"DAQ Board","timestamp_ms":2,"message":"Abort"}]})).unwrap();
        let text = snapshot.launch_message().unwrap();
        assert!(text.contains("Actuator Board") && text.contains("DAQ Board"));
        snapshot.active_errors.remove(0);
        assert_eq!(
            snapshot.launch_message().as_deref(),
            Some("Launch disabled: DAQ Board report Abort.")
        );
        snapshot.active_errors.clear();
        assert!(snapshot.launch_message().is_none());
    }
}

#[cfg(test)]
mod cosmetic_requested_decode_tests {
    #[test]
    fn requested_field_is_optional_and_does_not_become_actuation() {
        let legacy: super::ActionControl = serde_json::from_str(
            r#"{"cmd":"ToggleAvBayUnderglow","enabled":true,"blink":"none","actuated":true}"#,
        ).unwrap();
        assert_eq!(legacy.requested, None);
        let current: super::ActionControl = serde_json::from_str(
            r#"{"cmd":"ToggleAvBayUnderglow","enabled":true,"blink":"none","actuated":null,"requested":false}"#,
        ).unwrap();
        assert_eq!(current.requested, Some(false));
        assert_eq!(current.actuated, None);
    }
}
