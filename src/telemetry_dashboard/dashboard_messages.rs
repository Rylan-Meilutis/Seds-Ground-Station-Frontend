#[derive(Deserialize, Debug)]
#[serde(tag = "ty", content = "data")]
enum WsInMsg {
    Telemetry(TelemetryRow),
    TelemetryBatch(Vec<TelemetryRow>),
    FlightState(FlightStateMsg),
    LaunchClock(LaunchClockMsg),
    Warning(AlertMsg),
    Error(AlertMsg),
    BoardStatus(BoardStatusMsg),
    NetworkTopology(NetworkTopologyMsg),
    Notifications(Vec<PersistentNotification>),
    Messages(Vec<PersistentNotification>),
    ActionPolicy(ActionPolicyMsg),
    FillTargets(FillTargetsConfig),
    RecordingStatus(RecordingStatusMsg),
    NetworkTime(NetworkTimeMsg),
}

#[derive(Deserialize, Debug, Clone)]
struct FlightStateMsg {
    state: FlightState,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AlertMsg {
    pub timestamp_ms: i64,
    pub message: String,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlinkMode {
    None,
    Slow,
    Fast,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ActionControl {
    pub cmd: String,
    pub enabled: bool,
    pub blink: BlinkMode,
    pub actuated: Option<bool>,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ActionPolicyMsg {
    pub key_enabled: bool,
    #[serde(default = "default_software_buttons_enabled")]
    pub software_buttons_enabled: bool,
    pub controls: Vec<ActionControl>,
}

impl ActionPolicyMsg {
    /// Returns the startup action policy before the backend publishes a real one.
    fn default_locked() -> Self {
        Self {
            key_enabled: false,
            software_buttons_enabled: true,
            controls: Vec::new(),
        }
    }
}

/// Provides the serde default for software action buttons.
fn default_software_buttons_enabled() -> bool {
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
    pub version: u32,
    pub nitrogen: FluidFillTarget,
    pub nitrous: FluidFillTarget,
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
/// Returns a monotonic-ish timestamp source for rate calculations in the browser.
fn monotonic_now_ms() -> f64 {
    js_sys::Date::now()
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
    let (h, am_pm) = match h24 {
        0 => (12, "AM"),
        1..=11 => (h24, "AM"),
        12 => (12, "PM"),
        _ => (h24 - 12, "PM"),
    };
    let m = d.get_minutes();
    let s = d.get_seconds();
    let cs = (d.get_milliseconds() / 10).clamp(0, 99);
    format!("{h:02}:{m:02}:{s:02}:{cs:02} {am_pm}")
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn format_timestamp_ms_clock(ms_epoch: i64) -> String {
    use chrono::{Local, TimeZone};
    let Some(dt) = Local.timestamp_millis_opt(ms_epoch).single() else {
        return "--:--:--:--".to_string();
    };
    let cs = dt.timestamp_subsec_millis() / 10;
    format!("{}:{cs:02} {}", dt.format("%I:%M:%S"), dt.format("%p"))
}

/// Formats the network-synchronized wall clock for dashboard display.
fn format_network_time(ms_epoch: i64) -> String {
    format_timestamp_ms_clock(ms_epoch)
}

// --------------------------
// DB alert DTO (/api/alerts)
// --------------------------
#[derive(Deserialize, Debug, Clone)]
struct AlertDto {
    pub timestamp_ms: i64,
    pub severity: String, // "warning" | "error"
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MainTab {
    State,
    ConnectionStatus,
    Detailed,
    NetworkTopology,
    Map,
    Actions,
    Calibration,
    Messages,
    Notifications,
    Warnings,
    Errors,
    Data,
}
