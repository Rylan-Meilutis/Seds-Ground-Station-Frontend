//! Personnel proximity lock (docs/proximity-lock.md; GroundStation26
//! docs/proximity-lock.md): the device side.
//!
//! The station opts in (admin). Each signed-in device then participates by
//! default unless its user opts out here, or the operating system's location
//! permission is denied (which counts as opting out). A participating device
//! measures its own distance to the rocket's GPS fix (or the station's
//! configured launch site when there is no fresh fix) ON THE DEVICE and sends
//! the station only [`Report`]: participating, in range / out of range /
//! cannot tell, an accuracy class and a heartbeat. No latitude, longitude,
//! distance or bearing ever leaves the device.
//!
//! This module is the pure part: wire types, the geofence with hysteresis and
//! the accuracy-conservative rule, the reference choice, the participation
//! state, and the report. `proximity_lock_view` runs it; `proximity_native`
//! keeps it running in the background on iOS and Android.
use super::watch_companion::geodesy::{LatLon, distance_m};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub(crate) const STATUS_PATH: &str = "/api/proximity";
pub(crate) const REPORT_PATH: &str = "/api/proximity/report";
pub(crate) const SETTINGS_PATH: &str = "/api/proximity/settings";
pub(crate) const OVERRIDE_PATH: &str = "/api/proximity/override";
pub(crate) const OVERRIDE_END_PATH: &str = "/api/proximity/override/end";
pub(crate) const DISMISS_PATH: &str = "/api/proximity/dismiss";

/// The station's rocket fix counts as the reference while this fresh.
pub(crate) const ROCKET_FIX_FRESH_MS: i64 = 120_000;
/// An accuracy the device did not report is taken as this (metres): wide,
/// so an unknown accuracy errs towards "in range".
pub(crate) const UNKNOWN_ACCURACY_M: f64 = 100.0;
/// The device's own fix is used while this fresh. The operating system
/// delivers a new fix when the device moves, so a stationary device keeps a
/// valid older one.
pub(crate) const OWN_FIX_FRESH_MS: i64 = 300_000;
pub(crate) const HEARTBEAT_MS: i64 = 15_000;

// ---------------------------------------------------------------------------
// Wire: the station's status (GET /api/proximity, websocket ProximityLock)

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StalePolicy {
    #[default]
    Warn,
    Block,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Blocks {
    pub arming: bool,
    pub fill: bool,
    pub launch: bool,
}

impl Default for Blocks {
    fn default() -> Self {
        Self { arming: true, fill: true, launch: true }
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct Settings {
    pub enabled: bool,
    pub radius_m: u32,
    pub exit_margin_m: u32,
    pub blocks: Blocks,
    pub stale_policy: StalePolicy,
    pub heartbeat_timeout_s: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            radius_m: 150,
            exit_margin_m: 30,
            blocks: Blocks::default(),
            stale_policy: StalePolicy::Warn,
            heartbeat_timeout_s: 60,
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeviceState {
    InRange,
    OutOfRange,
    Unknown,
    OptedOut,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Accuracy {
    High,
    Medium,
    Low,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reference {
    Rocket,
    Pad,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OptOut {
    User,
    PermissionDenied,
    Unsupported,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Platform {
    Ios,
    Android,
    Web,
    Desktop,
    Watch,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub(crate) struct DeviceView {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub user: String,
    pub platform: Platform,
    pub state: DeviceState,
    #[serde(default)]
    pub accuracy: Option<Accuracy>,
    #[serde(default)]
    pub reference: Option<Reference>,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub opt_out: Option<OptOut>,
    #[serde(default)]
    pub last_report_ms: i64,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub(crate) struct Counts {
    pub reporting: usize,
    pub in_range: usize,
    pub unknown: usize,
    pub opted_out: usize,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub(crate) struct OverrideView {
    pub by: String,
    pub reason: String,
    pub expires_ms: i64,
    #[serde(default)]
    pub remaining_s: u64,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pad {
    pub lat_deg: f64,
    pub lon_deg: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Status {
    pub settings: Settings,
    pub heartbeat_interval_s: u32,
    pub active: bool,
    pub condition: bool,
    pub reason: Option<String>,
    pub warning: Option<String>,
    pub summary: String,
    pub counts: Counts,
    pub devices: Vec<DeviceView>,
    #[serde(rename = "override")]
    pub override_view: Option<OverrideView>,
    pub pad: Option<Pad>,
    pub blocked_commands: Vec<String>,
}

impl Status {
    pub(crate) fn names(&self, state: DeviceState) -> Vec<String> {
        self.devices.iter().filter(|d| d.state == state).map(|d| d.name.clone()).collect()
    }
}

/// The station's latest status (websocket frame, poll, or report reply).
pub(crate) static PROXIMITY_STATUS: GlobalSignal<Option<Status>> = Signal::global(|| None);

pub(crate) fn apply_status(status: Status) {
    if PROXIMITY_STATUS.peek().as_ref() != Some(&status) {
        *PROXIMITY_STATUS.write() = Some(status);
    }
}

// ---------------------------------------------------------------------------
// Wire: the device's report (POST /api/proximity/report)

/// Exactly what a device tells the station. The station refuses any other
/// field. There is deliberately no field that could carry a place.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct Report {
    pub v: u8,
    pub device_id: String,
    pub device_name: String,
    pub platform: Platform,
    pub participating: bool,
    pub in_range: Option<bool>,
    pub accuracy: Option<Accuracy>,
    pub reference: Option<Reference>,
    pub background: bool,
    pub opt_out: Option<OptOut>,
    pub seq: u64,
    pub sent_ms: i64,
}

// ---------------------------------------------------------------------------
// Geofence

/// The device's own fix, as the operating system or browser gives it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OwnFix {
    pub lat: f64,
    pub lon: f64,
    /// Radius of the accuracy circle in metres, when known.
    pub accuracy_m: Option<f64>,
    /// Wall-clock time of the fix.
    pub at_ms: i64,
}

pub(crate) fn accuracy_class(accuracy_m: Option<f64>) -> Accuracy {
    match accuracy_m.filter(|a| a.is_finite() && *a >= 0.0) {
        Some(a) if a <= 20.0 => Accuracy::High,
        Some(a) if a <= 100.0 => Accuracy::Medium,
        _ => Accuracy::Low,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geofence {
    pub radius_m: f64,
    pub exit_margin_m: f64,
}

impl Geofence {
    pub(crate) fn of(settings: &Settings) -> Self {
        Self { radius_m: f64::from(settings.radius_m), exit_margin_m: f64::from(settings.exit_margin_m) }
    }

    /// In range or not, given the last answer (hysteresis), the distance to
    /// the reference and the fix's accuracy:
    ///
    /// * conservative: whenever the accuracy circle reaches the radius
    ///   (`distance - accuracy <= radius`) the device is in range;
    /// * hysteresis: a device in range leaves only once the near edge of its
    ///   accuracy circle is beyond `radius + exit_margin`, so it is out by at
    ///   least the margin or its accuracy, whichever is larger.
    pub(crate) fn in_range(&self, previous: Option<bool>, distance_m: f64, accuracy_m: Option<f64>) -> bool {
        let accuracy = accuracy_m.filter(|a| a.is_finite() && *a >= 0.0).unwrap_or(UNKNOWN_ACCURACY_M);
        let near_edge = distance_m - accuracy;
        if near_edge <= self.radius_m {
            return true;
        }
        previous == Some(true) && near_edge <= self.radius_m + self.exit_margin_m
    }
}

/// What the device measures against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ReferencePoint {
    Rocket(LatLon),
    Pad(LatLon),
    None,
}

impl ReferencePoint {
    pub(crate) fn point(self) -> Option<LatLon> {
        match self {
            ReferencePoint::Rocket(p) | ReferencePoint::Pad(p) => Some(p),
            ReferencePoint::None => None,
        }
    }
    pub(crate) fn kind(self) -> Option<Reference> {
        match self {
            ReferencePoint::Rocket(_) => Some(Reference::Rocket),
            ReferencePoint::Pad(_) => Some(Reference::Pad),
            ReferencePoint::None => None,
        }
    }
}

/// The rocket's fix while fresh, else the station's configured launch site,
/// else nothing (the device then cannot tell). A null-island or out-of-range
/// point is ignored.
pub(crate) fn choose_reference(rocket: Option<((f64, f64), i64)>, pad: Option<Pad>, now_ms: i64) -> ReferencePoint {
    let usable = |p: LatLon| p.valid() && !(p.lat.abs() < 1e-6 && p.lon.abs() < 1e-6);
    if let Some(((lat, lon), at_ms)) = rocket {
        let p = LatLon::new(lat, lon);
        if usable(p) && now_ms.saturating_sub(at_ms) <= ROCKET_FIX_FRESH_MS && at_ms <= now_ms + 5_000 {
            return ReferencePoint::Rocket(p);
        }
    }
    if let Some(pad) = pad {
        let p = LatLon::new(pad.lat_deg, pad.lon_deg);
        if usable(p) {
            return ReferencePoint::Pad(p);
        }
    }
    ReferencePoint::None
}

/// Why the device cannot tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unknown {
    NoFix,
    NoReference,
}

/// One evaluation: in range / out of range, or why it cannot tell.
pub(crate) fn evaluate(
    geofence: &Geofence,
    previous: Option<bool>,
    fix: Option<OwnFix>,
    reference: ReferencePoint,
    now_ms: i64,
) -> Result<bool, Unknown> {
    let target = reference.point().ok_or(Unknown::NoReference)?;
    let fix = fix
        .filter(|f| now_ms.saturating_sub(f.at_ms) <= OWN_FIX_FRESH_MS)
        .ok_or(Unknown::NoFix)?;
    let distance = distance_m(LatLon::new(fix.lat, fix.lon), target).ok_or(Unknown::NoFix)?;
    Ok(geofence.in_range(previous, distance, fix.accuracy_m))
}

// ---------------------------------------------------------------------------
// Participation

/// The operating system's location permission for this app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Permission {
    /// Not asked yet (or the browser will prompt).
    NotDetermined,
    Denied,
    /// Granted while the app is open only.
    Foreground,
    /// Granted in the background too.
    Background,
    /// No location on this device.
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Participation {
    /// The station has the lock off: nothing is sent.
    StationOff,
    /// Signed out (or anonymous): nothing is sent.
    SignedOut,
    OptedOut,
    PermissionDenied,
    Unsupported,
    /// Participating; the permission prompt is still to be answered.
    AwaitingPermission,
    Participating,
}

impl Participation {
    pub(crate) fn of(station_enabled: bool, signed_in: bool, opted_out: bool, permission: Permission) -> Self {
        if !station_enabled {
            Participation::StationOff
        } else if !signed_in {
            Participation::SignedOut
        } else if opted_out {
            Participation::OptedOut
        } else {
            match permission {
                Permission::Denied => Participation::PermissionDenied,
                Permission::Unsupported => Participation::Unsupported,
                Permission::NotDetermined => Participation::AwaitingPermission,
                Permission::Foreground | Permission::Background => Participation::Participating,
            }
        }
    }

    /// Something is sent to the station.
    pub(crate) fn reports(self) -> bool {
        !matches!(self, Participation::StationOff | Participation::SignedOut)
    }

    /// The device affects the lock.
    pub(crate) fn participating(self) -> bool {
        matches!(self, Participation::Participating | Participation::AwaitingPermission)
    }

    pub(crate) fn opt_out(self) -> Option<OptOut> {
        match self {
            Participation::OptedOut => Some(OptOut::User),
            Participation::PermissionDenied => Some(OptOut::PermissionDenied),
            Participation::Unsupported => Some(OptOut::Unsupported),
            _ => None,
        }
    }
}

/// The report for one evaluation. Only flags: see [`Report`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_report(
    device_id: &str,
    device_name: &str,
    platform: Platform,
    participation: Participation,
    result: Option<Result<bool, Unknown>>,
    fix: Option<OwnFix>,
    reference: ReferencePoint,
    background: bool,
    seq: u64,
    now_ms: i64,
) -> Report {
    let participating = participation.participating();
    Report {
        v: 1,
        device_id: device_id.to_owned(),
        device_name: device_name.to_owned(),
        platform,
        participating,
        in_range: if participating { result.and_then(Result::ok) } else { None },
        accuracy: fix.filter(|_| participating).map(|f| accuracy_class(f.accuracy_m)),
        reference: if participating { reference.kind() } else { None },
        background: participating && background,
        opt_out: participation.opt_out(),
        seq,
        sent_ms: now_ms,
    }
}

/// Send now: something the station shows changed, or the heartbeat is due.
pub(crate) fn report_due(last: Option<(&Report, i64)>, next: &Report, now_ms: i64) -> bool {
    let Some((last, at_ms)) = last else { return true };
    let same = Report { seq: 0, sent_ms: 0, ..last.clone() } == Report { seq: 0, sent_ms: 0, ..next.clone() };
    !same || now_ms.saturating_sub(at_ms) >= HEARTBEAT_MS
}

/// "Your device reports: …" for the Settings line.
pub(crate) fn own_state_text(participation: Participation, result: Option<Result<bool, Unknown>>, reference: ReferencePoint) -> String {
    match participation {
        Participation::StationOff => "The station has the proximity lock off. Nothing is sent.".into(),
        Participation::SignedOut => "Sign in to take part. Nothing is sent.".into(),
        Participation::OptedOut => "Not participating (you opted out on this device).".into(),
        Participation::PermissionDenied => {
            "Not participating: location permission is denied for this app. That counts as opting out.".into()
        }
        Participation::Unsupported => "Not participating: this device has no location.".into(),
        Participation::AwaitingPermission => {
            "Participating, waiting for location permission: the station sees \"cannot tell\".".into()
        }
        Participation::Participating => {
            let against = match reference {
                ReferencePoint::Rocket(_) => "the rocket's GPS fix",
                ReferencePoint::Pad(_) => "the configured launch site (no fresh rocket fix)",
                ReferencePoint::None => "nothing",
            };
            match result {
                Some(Ok(true)) => format!("In range (measured against {against})."),
                Some(Ok(false)) => format!("Out of range (measured against {against})."),
                Some(Err(Unknown::NoFix)) | None => "Cannot tell: no recent location fix on this device.".into(),
                Some(Err(Unknown::NoReference)) => {
                    "Cannot tell: no fresh rocket fix and no launch site configured at the station.".into()
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Local preferences (this device only)

pub(crate) const PREFS_KEY: &str = "proximity_lock_prefs_v1";
pub(crate) const DEVICE_ID_KEY: &str = "proximity_device_id_v1";

/// Per station and account, on this device: opted out, and the device name.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Prefs {
    pub accounts: std::collections::BTreeMap<String, AccountPrefs>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct AccountPrefs {
    pub opted_out: bool,
    pub device_name: Option<String>,
    /// The background-location explanation was shown and answered.
    pub explained: bool,
}

impl Prefs {
    pub(crate) fn parse(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }
    pub(crate) fn account(&self, key: &str) -> AccountPrefs {
        self.accounts.get(key).cloned().unwrap_or_default()
    }
}

pub(crate) fn platform() -> Platform {
    if cfg!(target_arch = "wasm32") {
        Platform::Web
    } else if cfg!(target_os = "ios") {
        Platform::Ios
    } else if cfg!(target_os = "android") {
        Platform::Android
    } else {
        Platform::Desktop
    }
}

pub(crate) fn default_device_name(user: &str, platform: Platform) -> String {
    let kind = match platform {
        Platform::Ios => "iPhone",
        Platform::Android => "Android phone",
        Platform::Web => "browser",
        Platform::Desktop if cfg!(target_os = "macos") => "Mac",
        Platform::Desktop => "computer",
        Platform::Watch => "watch",
    };
    let name = format!("{user}'s {kind}");
    name.chars().take(48).collect()
}

/// A device name the station accepts: 1–48 printable characters.
pub(crate) fn valid_device_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty() && name.chars().count() <= 48 && !name.chars().any(char::is_control)
}

pub(crate) fn new_device_id(random: u128) -> String {
    format!("prox-{random:032x}")
}

/// Whether this platform keeps reporting with the app in the background.
pub(crate) fn background_capable(platform: Platform, permission: Permission) -> bool {
    matches!(platform, Platform::Ios | Platform::Android) && permission == Permission::Background
}

pub(crate) fn platform_note(platform: Platform) -> &'static str {
    match platform {
        Platform::Ios | Platform::Android => {
            "With \"Always\" / \"Allow all the time\" location this phone keeps reporting in the background. With \"While using the app\" it reports only while the app is open; when it stops, the station sees it as unknown."
        }
        Platform::Web => {
            "A browser reports only while this page is open and visible. When it stops, the station sees this device as unknown."
        }
        Platform::Desktop => {
            "A desktop app reports only while it is open (best effort). When it stops, the station sees this device as unknown."
        }
        Platform::Watch => "",
    }
}

// ---------------------------------------------------------------------------
// Admin requests

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub(crate) struct SettingsUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius_m: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_margin_m: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Blocks>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_policy: Option<StalePolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_timeout_s: Option<u32>,
}

impl SettingsUpdate {
    /// Only the fields that differ from `current`.
    pub(crate) fn between(current: &Settings, next: &Settings) -> Self {
        fn pick<T: PartialEq>(a: T, b: T) -> Option<T> {
            (a != b).then_some(b)
        }
        Self {
            enabled: pick(current.enabled, next.enabled),
            radius_m: pick(current.radius_m, next.radius_m),
            exit_margin_m: pick(current.exit_margin_m, next.exit_margin_m),
            blocks: (current.blocks != next.blocks).then_some(next.blocks),
            stale_policy: (current.stale_policy != next.stale_policy).then_some(next.stale_policy),
            heartbeat_timeout_s: pick(current.heartbeat_timeout_s, next.heartbeat_timeout_s),
        }
    }
    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// The station's limits, checked before sending.
pub(crate) fn validate_settings(s: &Settings) -> Result<(), String> {
    if !(25..=2000).contains(&s.radius_m) {
        return Err("The radius must be 25–2000 m.".into());
    }
    if !(5..=500).contains(&s.exit_margin_m) {
        return Err("The exit margin must be 5–500 m.".into());
    }
    if !(30..=600).contains(&s.heartbeat_timeout_s) {
        return Err("The heartbeat timeout must be 30–600 s.".into());
    }
    if s.enabled && !(s.blocks.arming || s.blocks.fill || s.blocks.launch) {
        return Err("Choose at least one action class to block.".into());
    }
    Ok(())
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub(crate) struct OverrideRequest {
    pub minutes: u32,
    pub reason: String,
    pub confirm: bool,
}

pub(crate) fn validate_override(minutes: u32, reason: &str) -> Result<OverrideRequest, String> {
    if !(1..=30).contains(&minutes) {
        return Err("An override lasts 1–30 minutes.".into());
    }
    let reason = reason.trim();
    if reason.chars().count() < 3 || reason.chars().count() > 200 {
        return Err("Give a reason (3–200 characters).".into());
    }
    Ok(OverrideRequest { minutes, reason: reason.to_owned(), confirm: true })
}

/// The status banner, by severity: what blocks, what is overridden, what is
/// only a warning, or the quiet indicator.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Banner {
    Active(String),
    Overridden { text: String, by: String, reason: String, expires_ms: i64 },
    Warning(String),
    Indicator(String),
}

pub(crate) fn banner(status: Option<&Status>) -> Option<Banner> {
    let status = status?;
    if !status.settings.enabled {
        return None;
    }
    if status.active {
        return Some(Banner::Active(status.reason.clone().unwrap_or_else(|| "Proximity lock active".into())));
    }
    if let Some(o) = &status.override_view {
        return Some(Banner::Overridden {
            text: status.reason.clone().unwrap_or_else(|| status.summary.clone()),
            by: o.by.clone(),
            reason: o.reason.clone(),
            expires_ms: o.expires_ms,
        });
    }
    if let Some(warning) = &status.warning {
        return Some(Banner::Warning(warning.clone()));
    }
    Some(Banner::Indicator(status.summary.clone()))
}

pub(crate) fn remaining_text(expires_ms: i64, now_ms: i64) -> String {
    let s = (expires_ms - now_ms).max(0) / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests;
