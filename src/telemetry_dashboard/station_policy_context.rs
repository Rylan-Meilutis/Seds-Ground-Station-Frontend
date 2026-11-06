//! Actual UI/auth/connection adapter for the maintained pure policy model.
pub(super) use super::station_policy_model::{
    AppliedStamp, Context, IngressContext, Lease, ObservationTicket,
};
use super::{
    station_mode_contract::Snapshot,
    station_policy_model::{self, ModeFacts, ModePair, ObservationFacts},
};
use std::sync::{LazyLock, Mutex};
fn mode_facts(snapshot: &Snapshot) -> ModeFacts {
    ModeFacts {
        backend_instance: snapshot.backend_instance.clone(),
        authority_id: snapshot.authority_id.clone(),
        principal_context: snapshot.principal_context.clone(),
        policy_epoch: snapshot.policy_epoch.clone(),
        authority_current: snapshot.authority_current,
        valid_for_ms: snapshot.valid_for_ms,
        epoch_fencing_supported: snapshot.epoch_fencing_supported,
        control_origin_required: snapshot.control_origin_required,
        control_origin: snapshot.control_origin.clone(),
        admissions_open: snapshot.admissions_open,
        current: snapshot.current.as_ref().map(|pair| ModePair {
            id: pair.id.clone(),
            mode_id: pair.mode_id.clone(),
            simulation: pair.simulation,
        }),
    }
}
pub(super) fn observation_is_current(
    snapshot: &Snapshot,
    received: f64,
    now: f64,
    verified: bool,
    original_owner_current: bool,
) -> bool {
    station_policy_model::observation_is_current(
        ObservationFacts {
            verified,
            original_owner_current,
            authority_current: snapshot.authority_current,
            valid_for_ms: snapshot.valid_for_ms,
            epoch_fencing_supported: snapshot.epoch_fencing_supported,
            epoch_known: snapshot.policy_epoch.is_some(),
            current_known: snapshot.current.is_some(),
        },
        received,
        now,
        epoch_fencing_required,
    )
}
impl AppliedStamp {
    /// Pure Send-safe installed-identity prefilter, NOT an auth/lifetime grant.
    /// A native queue may discard superseded work here; its final UI reducer
    /// must still call check_current under the original UI ownership context.
    pub fn matches_installed(&self) -> bool {
        CONTEXT
            .lock()
            .is_ok_and(|context| context.matches_installed(self))
    }
    /// UI-runtime-thread only: consults the original Rc observer proof. Moving
    /// this data token to a native queue does not make that UI predicate Send.
    pub fn check_current(&self) -> Result<(), String> {
        let current = self.matches_installed() && self.origin() == super::UrlConfig::base_http();
        // The retained proof owns its ORIGINAL auth/base/WS/lifetime. Never
        // recapture that ownership just because the epoch text still matches.
        if current && super::station_mode_observer::applied_owner_current(self) {
            Ok(())
        } else {
            Err("Original applied station domain retired or is unverified.".into())
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct EmergencyLease(AppliedStamp);
impl EmergencyLease {
    pub fn epoch(&self) -> &str {
        self.0.epoch()
    }
    pub fn check_current(&self) -> Result<(), String> {
        self.0.check_current()
    }
}
impl Lease {
    pub fn check_form_current(&self) -> Result<(), String> {
        self.check_current()?;
        self.check_form_origin(control_origin_required())
    }
    pub fn check_current(&self) -> Result<(), String> {
        let mut context = CONTEXT
            .lock()
            .map_err(|_| "Policy identity is unavailable")?;
        if context.current(
            self,
            &super::UrlConfig::base_http(),
            super::monotonic_now_ms(),
        ) {
            Ok(())
        } else {
            Err(
                "Operation cancelled: its original station policy retired. Create a new operation."
                    .into(),
            )
        }
    }
}
static CONTEXT: LazyLock<Mutex<Context>> = LazyLock::new(|| Mutex::new(Context::default()));
/// Capture before awaiting a mode response. An intervening accepted context or
/// retirement invalidates that response even if its auth owner is still current.
pub(super) fn observation_ticket(origin: &str) -> Result<ObservationTicket, String> {
    CONTEXT
        .lock()
        .map_err(|_| "Policy identity is unavailable")?
        .observation_ticket(origin)
}
pub(super) fn observation_current(ticket: &ObservationTicket, origin: &str) -> bool {
    CONTEXT
        .lock()
        .is_ok_and(|context| context.observation_current(ticket, origin))
}
/// Pure installed-domain snapshot. The observer separately binds its original
/// authenticated response owner before this can authorize ingress or Stop.
pub(super) fn applied_stamp() -> Option<AppliedStamp> {
    CONTEXT.lock().ok()?.applied_stamp()
}
pub(super) fn retire_applied_owner() {
    if let Ok(mut context) = CONTEXT.lock() {
        context.retire_applied_owner();
    }
}
pub(super) fn capture_emergency_current() -> Result<EmergencyLease, String> {
    let stamp = applied_stamp().ok_or(
        "No verified applied domain: obtain fresh status and make a new explicit Stop gesture",
    )?;
    stamp.check_current()?;
    Ok(EmergencyLease(stamp))
}
pub(super) fn ingress_context() -> IngressContext {
    let origin = super::UrlConfig::base_http();
    let Ok(context) = CONTEXT.lock() else {
        return IngressContext {
            required: true,
            applied: None,
        };
    };
    let IngressContext { required, applied } = context.ingress_context(&origin);
    drop(context);
    let applied = applied.filter(|stamp| stamp.check_current().is_ok());
    IngressContext { required, applied }
}
pub(super) fn capture_current() -> Result<Lease, String> {
    CONTEXT
        .lock()
        .map_err(|_| "Policy identity is unavailable".to_owned())?
        .capture(&super::UrlConfig::base_http(), super::monotonic_now_ms())
}
pub(super) fn capture_form_current() -> Result<Lease, String> {
    let lease = capture_current()?;
    lease.check_form_current()?;
    Ok(lease)
}
pub(super) fn control_origin_required() -> bool {
    CONTEXT.lock().map_or(true, |context| context.control_origin_required_for(&super::UrlConfig::base_http()))
}
pub(super) fn declare_control_origin_required(origin: &str, authority: Option<&str>) -> Result<(), String> {
    CONTEXT.lock().map_err(|_| "Policy identity is unavailable".to_owned())?
        .declare_control_origin_required(origin, authority);
    Ok(())
}
pub(super) fn declare_supported(origin: &str, authority: Option<&str>) -> Result<(), String> {
    CONTEXT
        .lock()
        .map_err(|_| "Policy identity is unavailable".to_owned())?
        .declare_supported(origin, authority);
    Ok(())
}
pub(super) fn publish(
    origin: &str,
    snapshot: Snapshot,
    received: f64,
    now: f64,
) -> Result<(), String> {
    let mut context = CONTEXT
        .lock()
        .map_err(|_| "Policy identity is unavailable".to_owned())?;
    context.observe(origin, mode_facts(&snapshot), received, now);
    match context.protocol_error() {
        Some(error) => Err(error.to_owned()),
        None => Ok(()),
    }
}
pub(super) fn unavailable(origin: &str) -> Result<(), String> {
    CONTEXT
        .lock()
        .map_err(|_| "Policy identity is unavailable".to_owned())?
        .unavailable(origin);
    Ok(())
}
pub(super) fn tick(now: f64) -> Result<(), String> {
    CONTEXT
        .lock()
        .map_err(|_| "Policy identity is unavailable".to_owned())?
        .tick(now);
    Ok(())
}
pub(super) const HTTP_EPOCH_HEADER: &str = "X-Station-Policy-Epoch";
pub(super) const WS_EPOCH_FIELD: &str = "policy_epoch";
/// Missing observation is never evidence that a previously supported authority
/// returned to legacy behavior. This query grants no operation authority.
pub(super) fn epoch_fencing_required() -> bool {
    CONTEXT.lock().map_or(true, |context| {
        context.required_for(&super::UrlConfig::base_http())
    })
}
/// Rendering reads identity only: never captures a lease or advances retirement.
/// Legacy bootstrap is deliberately stable so its first normal operation does
/// not remount/cancel itself merely by binding the initial configured origin.
pub(super) fn view_generation() -> String {
    let Ok(context) = CONTEXT.lock() else {
        return "policy-identity-unavailable".into();
    };
    context.generation_label(&super::UrlConfig::base_http())
}
/// Root enables this only after every producer and real backend isolation qualifies.
/// A capable custom backend cannot bypass an incomplete client integration.
pub(super) const CLIENT_EPOCH_INTEGRATION_READY: bool = false;

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        super::super::station_mode_contract::tests::example_snapshot()
    }
    #[test]
    fn descriptor_renames_do_not_retire_ordinary_or_applied_owners() {
        let mut c = Context::default();
        let mut s = snapshot();
        c.observe("station", mode_facts(&s), 0., 0.);
        let ordinary = c.capture("station", 1.).unwrap();
        let seal = c.applied_stamp().unwrap();
        s.current.as_mut().unwrap().label = "New display label".into();
        s.current.as_mut().unwrap().output_domain = "New descriptive wording".into();
        c.observe("station", mode_facts(&s), 2., 2.);
        assert!(c.current(&ordinary, "station", 3.));
        assert_eq!(c.applied_stamp().as_ref(), Some(&seal));
    }
    #[test]
    fn availability_revision_does_not_cancel_ordinary_but_epoch_does() {
        let mut c = Context::default();
        let mut s = snapshot();
        c.observe("station", mode_facts(&s), 0., 0.);
        let lease = c.capture("station", 1.).unwrap();
        s.revision = "choices-only".into();
        c.observe("station", mode_facts(&s), 2., 2.);
        assert!(c.current(&lease, "station", 3.));
        s.policy_epoch = Some("owner-A:epoch-2".into());
        c.observe("station", mode_facts(&s), 4., 4.);
        assert!(!c.current(&lease, "station", 5.));
    }
}
