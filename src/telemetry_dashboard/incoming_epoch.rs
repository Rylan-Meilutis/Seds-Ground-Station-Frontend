//! Read-only receive ownership. Incoming data never establishes station policy.
use super::station_policy_context::{self as policy, AppliedStamp};
use dioxus::prelude::ReadableExt;

pub(super) const SCOPE_HEADER: &str = "X-Station-Epoch-Scope";

/// Immutable installed-context capture made before a live request or packet is
/// queued. Ordinary admission closure is deliberately not part of this owner.
#[derive(Clone)]
pub(super) struct ReadOwner {
    origin: String,
    identity: crate::auth::RequestIdentity,
    ws_epoch: u64,
    mount: u64,
    alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl PartialEq for ReadOwner {
    fn eq(&self, other: &Self) -> bool {
        self.origin == other.origin && self.identity == other.identity
            && self.ws_epoch == other.ws_epoch && self.mount == other.mount
            && std::sync::Arc::ptr_eq(&self.alive, &other.alive)
    }
}
impl Eq for ReadOwner {}
impl std::fmt::Debug for ReadOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadOwner").finish_non_exhaustive()
    }
}
impl ReadOwner {
    fn capture() -> Result<Self, String> {
        Ok(Self { origin: super::UrlConfig::base_http(),
            identity: crate::auth::request_identity().ok_or("Read authentication identity is unavailable")?,
            ws_epoch: *super::WS_EPOCH.read(), mount: super::dashboard_gen(), alive: super::dashboard_alive() })
    }
    fn check_current(&self) -> Result<(), String> {
        if self.origin != super::UrlConfig::base_http()
            || self.ws_epoch != *super::WS_EPOCH.read()
            || !crate::auth::request_identity_is_current(&self.identity)
            || !self.alive.load(std::sync::atomic::Ordering::Acquire)
            || !std::sync::Arc::ptr_eq(&self.alive, &super::dashboard_alive())
            || self.mount != super::dashboard_gen()
        { Err("Original read station, sign-in, connection or view retired.".into()) }
        else { Ok(()) }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ReadContext {
    owner: ReadOwner,
    applied: Option<AppliedStamp>,
}
impl ReadContext {
    pub(super) fn capture() -> Result<Self, String> {
        let context = policy::ingress_context();
        if context.required && context.applied.is_none() {
            return Err("Live data requires a verified original applied station domain.".into());
        }
        Ok(Self {
            owner: ReadOwner::capture()?,
            applied: context.applied,
        })
    }
    pub(super) fn epoch(&self) -> Option<&str> {
        self.applied.as_ref().map(AppliedStamp::epoch)
    }
    /// UI runtime only: original authenticated observer proof is checked here.
    pub(super) fn check_current(&self) -> Result<(), String> {
        self.owner.check_current()?;
        match &self.applied {
            Some(stamp) => stamp.check_current(),
            None if !policy::epoch_fencing_required() => Ok(()),
            None => Err("Legacy live-data owner retired after epoch support was observed.".into()),
        }
    }
    /// Native/background discard prefilter only, never an auth or apply grant.
    pub(super) fn matches_installed(&self) -> bool {
        match &self.applied {
            Some(stamp) => stamp.matches_installed(),
            // A background worker cannot inspect UI globals. A legacy item
            // must reach the original UI owner before it can be accepted.
            None => true,
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn validate_native_headers(&self, headers: &reqwest::header::HeaderMap) -> Result<(), String> {
        let scope = headers.get_all(SCOPE_HEADER).iter().collect::<Vec<_>>();
        let epoch = headers.get_all(policy::HTTP_EPOCH_HEADER).iter().collect::<Vec<_>>();
        if scope.len() > 1 || epoch.len() > 1 { return Err("Duplicate live response epoch metadata".into()); }
        let scope = scope.first().map(|v| v.to_str()).transpose().map_err(|_| "Invalid live response scope")?;
        let epoch = epoch.first().map(|v| v.to_str()).transpose().map_err(|_| "Invalid live response epoch")?;
        self.validate_mode_headers(scope, epoch)
    }
    /// UI-runtime await: reuse the published original-owner cancellation loop.
    /// Its existing wakeups also retire an old installed domain during IO.
    pub(super) async fn wait<F: std::future::Future>(
        &self, owner: &super::recording_download::ExportOwner, future: F,
    ) -> Result<F::Output, String> {
        super::native_session::await_owned(owner.wait(future), || self.check_current().is_ok())
            .await.ok_or("Original live read domain retired during IO")?
    }
    pub(super) fn validate_mode_headers(
        &self,
        scope: Option<&str>,
        epoch: Option<&str>,
    ) -> Result<(), String> {
        self.check_current()?;
        match (self.epoch(), scope, epoch) {
            (Some(expected), Some("mode"), Some(actual)) if expected == actual => Ok(()),
            (None, None, None) if !policy::epoch_fencing_required() => Ok(()),
            _ => Err(
                "Live response is missing or disagrees with its ORIGINAL station domain.".into(),
            ),
        }
    }
}

/// Every currently recognized UI WS variant is a current envelope. Archive,
/// auth, mode observation and neutral media use their separate owner/channels;
/// an unknown type or an `independent` tag cannot exempt current UI facts.
pub(super) fn mode_family(ty: &str) -> bool {
    matches!(ty, "DashboardReset" | "CalibrationChanged" | "Telemetry" | "TelemetryBatch" | "FlightState"
        | "LaunchClock" | "AlertConditions" | "BoardStatus" | "BoardAbort"
        | "NetworkTopology" | "ActionPolicy" | "FillTargets" | "NetworkTime"
        | "Warning" | "Error" | "AlertAckState" | "Notifications" | "Notification"
        | "Messages" | "RecordingStatus" | "MissionPhase" | "FlightMaxima"
        | "StationModeChanged")
}

#[cfg(test)]
mod mode_family_tests {
    #[test]
    fn mission_phase_and_flight_maxima_are_current_station_messages() {
        for ty in ["MissionPhase", "FlightMaxima", "FlightState", "StationModeChanged"] {
            assert!(super::mode_family(ty), "{ty}");
        }
        assert!(!super::mode_family("UnknownFamily"));
    }
}

#[derive(serde::Deserialize)]
struct Metadata {
    ty: String,
    epoch_scope: Option<String>,
    policy_epoch: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct PacketPermit { context: ReadContext }

/// One homogeneous installed-domain owner for the entire telemetry FIFO.
/// Binding and enqueue happen under its single Mutex without an intervening
/// await; retaining the owner here prevents a later frame drain from treating
/// old rows as newly received. Row payloads and queue-budget math stay unchanged.
#[derive(Default)]
pub(super) struct TelemetryQueue {
    rows: std::collections::VecDeque<super::TelemetryRow>,
    original: Option<ReadContext>,
}
impl TelemetryQueue {
    pub(super) fn bind(&mut self, permit: &PacketPermit) -> bool {
        let context = &permit.context;
        if context.check_current().is_err() { return false; }
        if self.original.as_ref() != Some(context) {
            self.rows.clear();
            self.original = Some(context.clone());
        }
        true
    }
    pub(super) fn clear(&mut self) {
        self.rows.clear();
        self.original = None;
    }
    pub(super) fn current(&self) -> bool {
        self.original.as_ref().map_or_else(
            || !policy::epoch_fencing_required(),
            |context| context.check_current().is_ok(),
        )
    }
}
impl std::ops::Deref for TelemetryQueue {
    type Target = std::collections::VecDeque<super::TelemetryRow>;
    fn deref(&self) -> &Self::Target { &self.rows }
}
impl std::ops::DerefMut for TelemetryQueue {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.rows }
}
impl PacketPermit {
    pub(super) fn capture(payload: &str) -> Result<Self, String> {
        // Serde rejects duplicate recognized metadata fields. Value would
        // silently replace duplicate epoch fields before validation.
        let meta: Metadata = serde_json::from_str(payload).map_err(|e| e.to_string())?;
        if !mode_family(&meta.ty) { return Err("Unknown incoming station message family".into()); }
        let context = ReadContext::capture()?;
        context.validate_mode_headers(meta.epoch_scope.as_deref(), meta.policy_epoch.as_deref())?;
        Ok(Self { context })
    }
    pub(super) fn check_current(&self) -> Result<(), String> { self.context.check_current() }
    pub(super) fn matches_installed(&self) -> bool { self.context.matches_installed() }
}

/// Explicit live GET families; no guessed path prefix or HTTP-verb policy.
/// A mutable domain draft already has a sealed ordinary owner and also reads
/// its original installed context. Independent auth/history/mode owners do not.
pub(super) fn context_for_path(path: &str, ordinary: bool) -> Result<Option<ReadContext>, String> {
    let route = path.split('?').next().unwrap_or(path);
    let live = matches!(route,
        "/flightstate" | "/api/action_policy" | "/api/recent" | "/api/fill_targets"
        | "/api/launch_clock" | "/api/boards" | "/api/network_topology" | "/api/network_time"
        | "/api/board-recovery/status" | "/api/actuator/plumbing/status" | "/api/gse/status"
        | "/api/antenna/status" | "/api/recovery" | "/api/rrls/control"
        | "/api/recording_status" | "/api/notifications" | "/api/messages" | "/api/alerts");
    if ordinary || live { ReadContext::capture().map(Some) } else { Ok(None) }
}
