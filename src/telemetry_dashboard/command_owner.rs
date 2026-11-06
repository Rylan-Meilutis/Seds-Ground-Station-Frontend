//! Original gesture identity for press, confirmation, native queues and proxy.
//! Capture before the first yield. Never recapture a lease on release/dequeue.
use dioxus::prelude::*;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use super::{auth, station_policy_context, EpochFence, UrlConfig};

#[derive(Clone)]
enum Authority {
    Ordinary(station_policy_context::Lease),
    ObservedEmergency(station_policy_context::EmergencyLease),
    LegacyEmergency,
}
#[derive(Clone)]
pub(super) struct CommandOwner {
    command: String,
    identity: auth::RequestIdentity,
    base: String,
    ws_epoch: u64,
    skip_tls: bool,
    mount: u64,
    alive: Arc<AtomicBool>,
    authority: Authority,
    epoch: Option<String>,
    control_origin: Option<String>,
}
impl CommandOwner {
    pub(super) fn capture(command: &str) -> Result<Self, String> {
        let required = station_policy_context::epoch_fencing_required();
        // Exact ActionPolicy producer bytes at genuine intent creation. A form
        // origin can never substitute for a control's emergency receiver tuple.
        let control_origin = super::ACTION_POLICY_SIGNAL.read().controls.iter()
            .find(|c| c.cmd == command).and_then(|c| c.control_origin.clone());
        if station_policy_context::control_origin_required() && control_origin.as_deref().is_none_or(str::is_empty) {
            return Err("Control origin is unavailable; obtain current policy and create a new intent.".into());
        }
        let observed_emergency = super::ACTION_POLICY_SIGNAL.read().controls.iter()
            .any(|c| c.cmd == command && c.epoch_fence == EpochFence::EmergencyStop);
        let (authority, epoch) = if required && observed_emergency {
            let lease = station_policy_context::capture_emergency_current()?;
            let epoch = Some(lease.epoch().to_owned());
            (Authority::ObservedEmergency(lease), epoch)
        } else if !required && command == "Abort" {
            // Existing legacy protocol only. This exception cannot survive the
            // first supported observation and never classifies a new command.
            (Authority::LegacyEmergency, None)
        } else {
            let lease = station_policy_context::capture_current()?;
            let epoch = lease.epoch().map(str::to_owned);
            (Authority::Ordinary(lease), epoch)
        };
        let owner = Self {
            command: command.to_owned(),
            identity: auth::request_identity().ok_or("Authentication owner is unavailable")?,
            base: UrlConfig::base_http(),
            ws_epoch: *super::WS_EPOCH.read(),
            skip_tls: UrlConfig::_skip_tls_verify(),
            mount: super::dashboard_gen(),
            alive: super::dashboard_alive(),
            authority, epoch, control_origin,
        };
        owner.check()?;
        Ok(owner)
    }
    pub(super) fn check(&self) -> Result<(), String> {
        if !self.alive.load(Ordering::Acquire)
            || !Arc::ptr_eq(&self.alive, &super::dashboard_alive())
            || self.mount != super::dashboard_gen()
            || self.base != UrlConfig::base_http()
            || self.ws_epoch != *super::WS_EPOCH.read()
            || self.skip_tls != UrlConfig::_skip_tls_verify()
            || !auth::request_identity_is_current(&self.identity)
            || !auth::can_send_command(&self.command)
        {
            return Err("Command retired: original station, login or connection changed.".into());
        }
        if station_policy_context::control_origin_required() && self.control_origin.as_deref().is_none_or(str::is_empty) {
            return Err("Original control origin is missing.".into());
        }
        if let Some(original) = &self.control_origin {
            let current = super::ACTION_POLICY_SIGNAL.read().controls.iter()
                .find(|c| c.cmd == self.command).and_then(|c| c.control_origin.clone());
            if current.as_ref() != Some(original) {
                return Err("Original control context retired; create a new intent.".into());
            }
        }
        match &self.authority {
            Authority::Ordinary(lease) => lease.check_current(),
            Authority::ObservedEmergency(lease) => {
                lease.check_current()?;
                if station_policy_context::epoch_fencing_required()
                    && super::ACTION_POLICY_SIGNAL.read().controls.iter().any(|c| {
                        c.cmd == self.command && c.epoch_fence == EpochFence::EmergencyStop && c.enabled
                    })
                { Ok(()) } else { Err("Emergency classification is no longer available.".into()) }
            }
            Authority::LegacyEmergency => {
                if !station_policy_context::epoch_fencing_required() { Ok(()) }
                else { Err("Legacy command owner retired after policy support was observed.".into()) }
            }
        }
    }
    pub(super) fn is_emergency(&self) -> bool {
        matches!(self.authority, Authority::ObservedEmergency(_) | Authority::LegacyEmergency)
    }
    pub(super) fn command(&self) -> &str { &self.command }
    pub(super) fn epoch(&self) -> Option<&str> { self.epoch.as_deref() }
    pub(super) fn control_origin(&self) -> Option<&str> { self.control_origin.as_deref() }
    pub(super) fn token(&self) -> Option<&str> { self.identity.token() }
    pub(super) fn base(&self) -> &str { &self.base }
    pub(super) fn is_current(&self) -> bool { self.check().is_ok() }
    pub(super) fn wire_message(&self) -> Result<String, String> {
        self.check()?;
        let mut message = serde_json::json!({"cmd":self.command});
        if let Some(epoch) = self.epoch() { message[station_policy_context::WS_EPOCH_FIELD] = epoch.into(); }
        if let Some(origin) = self.control_origin() { message["control_origin"] = origin.into(); }
        serde_json::to_string(&message).map_err(|e| e.to_string())
    }
}

/// Input origin is presentation provenance, never server permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PressOrigin {
    Pointer { id: i32, touch: bool, button: Option<dioxus::html::input_data::MouseButton> },
    Touch { id: i32 },
    Mouse { button: dioxus::html::input_data::MouseButton },
    Keyboard,
    Accessible,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClickOrigin { Pointer, KeyboardOrAccessible, Unknown }
impl ClickOrigin {
    pub fn from_mouse_detail(detail: Option<i32>) -> Self {
        match detail {
            Some(0) => Self::KeyboardOrAccessible,
            Some(_) => Self::Pointer,
            None => Self::Unknown,
        }
    }
}
/// No eviction: forgetting an old keyboard source would let its late click
/// become a fresh accessibility activation after a different control replaced it.
#[derive(Default)]
pub(super) struct KeyboardSources {
    sources: std::collections::BTreeSet<String>,
    overflow: bool,
}
impl KeyboardSources {
    pub fn remember(&mut self, source: &str) {
        if self.sources.contains(source) { return; }
        if self.sources.len() >= 64 || source.len() > 256 { self.overflow = true; }
        else { self.sources.insert(source.into()); }
    }
    pub fn blocks_uncorrelated(&self, source: &str) -> bool {
        self.overflow || self.sources.contains(source)
    }
}
#[derive(Clone)]
pub(super) struct PendingCommandPress {
    pub source: String,
    pub command: String,
    pub started_ms: f64,
    pub owner: Option<CommandOwner>,
    pub origin: PressOrigin,
    pub proxy: bool,
    pub consumed: bool,
    pub terminal_seen: bool,
}

#[derive(Clone)]
pub(super) struct QueuedCommand {
    pub owner: CommandOwner,
    pub wire: String,
}
impl QueuedCommand {
    pub(super) fn capture(owner: CommandOwner) -> Result<Self, String> {
        let wire = owner.wire_message()?;
        Ok(Self { owner, wire })
    }
}

/// Primary proxy uses the original gesture owner, including an emergency's
/// original optional epoch. The backend alone classifies emergency authority.
pub(super) async fn post_primary(owner: &CommandOwner) -> Result<auth::CommandProxyStatus, String> {
    owner.check()?;
    let base = if owner.base().is_empty() {
        #[cfg(not(target_arch = "wasm32"))]
        { "http://localhost:3000".to_owned() }
        #[cfg(target_arch = "wasm32")]
        { web_sys::window().ok_or("Window unavailable")?.location().origin().map_err(|_| "Origin unavailable")? }
    } else { owner.base().trim_end_matches('/').to_owned() };
    let body = serde_json::to_string(owner.command()).map_err(|e| e.to_string())?;
    #[cfg(not(target_arch = "wasm32"))]
    let (status, text) = {
        let client = auth::build_native_http_client(owner.skip_tls,
            std::time::Duration::from_secs(8), std::time::Duration::from_secs(8))?;
        let mut request = client.post(format!("{base}/api/command"))
            .header("Content-Type", "application/json").body(body);
        if let Some(token) = owner.token() { request = request.bearer_auth(token); }
        if let Some(epoch) = owner.epoch() { request = request.header(station_policy_context::HTTP_EPOCH_HEADER, epoch); }
        if let Some(origin) = owner.control_origin() { request = request.header("X-Station-Control-Origin", origin); }
        owner.check()?;
        let response = super::native_session::await_owned(request.send(), || owner.is_current())
            .await.ok_or("Original command owner retired")?.map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let text = super::native_session::await_owned(response.text(), || owner.is_current())
            .await.ok_or("Original command owner retired")?.map_err(|e| e.to_string())?;
        (status, text)
    };
    #[cfg(target_arch = "wasm32")]
    let (status, text) = {
        let controller = web_sys::AbortController::new().map_err(|_| "Cancellation unavailable")?;
        let _cancel = super::HttpAbortOnDrop(controller.clone());
        let signal = controller.signal();
        let mut request = gloo_net::http::Request::post(&format!("{base}/api/command"))
            .abort_signal(Some(&signal)).redirect(web_sys::RequestRedirect::Error)
            .header("Content-Type", "application/json");
        if let Some(token) = owner.token() { request = request.header("Authorization", &format!("Bearer {token}")); }
        if let Some(epoch) = owner.epoch() { request = request.header(station_policy_context::HTTP_EPOCH_HEADER, epoch); }
        if let Some(origin) = owner.control_origin() { request = request.header("X-Station-Control-Origin", origin); }
        let request = request.body(body).map_err(|e| e.to_string())?;
        owner.check()?;
        let fetch = async {
            let response = request.send().await.map_err(|e| e.to_string())?;
            let status = response.status();
            let text = response.text().await.map_err(|e| e.to_string())?;
            Ok::<_, String>((status, text))
        };
        let bounded = futures_util::future::select(Box::pin(fetch),
            Box::pin(gloo_timers::future::TimeoutFuture::new(15_000)));
        match super::native_session::await_owned(bounded, || owner.is_current()).await
            .ok_or("Original command owner retired")? {
            futures_util::future::Either::Left((result, _)) => result?,
            futures_util::future::Either::Right(_) => return Err("Primary command request timed out".into()),
        }
    };
    owner.check()?;
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {}", text.chars().take(200).collect::<String>()));
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "station_policy_transport_tests.rs"]
mod transport_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn isolated_test_child(module: &str, name: &str) -> bool {
    transport_tests::child(module, name)
}
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) fn legacy_test_dom(base: &str) -> VirtualDom { transport_tests::dom(base) }
