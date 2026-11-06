//! Pure station-policy ownership model; no UI, IO, clock source or serde.
//! Adapters provide validated facts and actual original-owner observations.
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_TTL_MS: u64 = 30_000;
pub fn elapsed(received: f64, now: f64, ttl: u64) -> bool {
    let age = now - received;
    ttl > 0 && ttl <= MAX_TTL_MS && age.is_finite() && (0. ..=ttl as f64).contains(&age)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModePair {
    pub id: String,
    pub mode_id: String,
    pub simulation: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModeFacts {
    pub backend_instance: String,
    pub authority_id: String,
    pub principal_context: String,
    pub policy_epoch: Option<String>,
    pub authority_current: bool,
    pub valid_for_ms: u64,
    pub epoch_fencing_supported: bool,
    pub control_origin_required: bool,
    pub control_origin: Option<String>,
    pub admissions_open: bool,
    pub current: Option<ModePair>,
}
impl ModeFacts {
    pub fn fresh(&self, received: f64, now: f64) -> bool {
        elapsed(received, now, self.valid_for_ms)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ObservationFacts {
    pub verified: bool,
    pub original_owner_current: bool,
    pub authority_current: bool,
    pub valid_for_ms: u64,
    pub epoch_fencing_supported: bool,
    pub epoch_known: bool,
    pub current_known: bool,
}
pub fn observation_is_current(
    facts: ObservationFacts,
    received: f64,
    now: f64,
    required: impl FnOnce() -> bool,
) -> bool {
    facts.verified
        && facts.original_owner_current
        && facts.authority_current
        && elapsed(received, now, facts.valid_for_ms)
        && (!required()
            || (facts.epoch_fencing_supported && facts.epoch_known && facts.current_known))
}
/// Installed-domain identity. Availability and ordinary admission are excluded:
/// Closing does not change the applied domain or prevent reading its telemetry.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AppliedIdentity {
    backend: String,
    authority: String,
    principal: String,
    epoch: String,
    target: String,
    mode: String,
    simulation: bool,
}
type EpochKey = (String, String, String, String);
type LineageKey = (String, String, String);
const MAX_RETAINED_EPOCH_BINDINGS: usize = 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
struct PairIdentity {
    target: String,
    mode: String,
    simulation: bool,
}
struct EpochBinding {
    pair: PairIdentity,
    incoherent: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppliedStamp {
    origin: String,
    generation: u64,
    identity: AppliedIdentity,
}
#[derive(Clone, Debug)]
pub struct IngressContext {
    pub required: bool,
    pub applied: Option<AppliedStamp>,
}
#[derive(Clone, Debug)]
pub struct ObservationTicket {
    origin: String,
    revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lease {
    origin: String,
    generation: u64,
    epoch: Option<String>,
    control_origin: Option<String>,
}
impl AppliedStamp {
    pub fn epoch(&self) -> &str {
        &self.identity.epoch
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
}
impl Lease {
    /// Field-presence rule for ordinary forms only; not command authority.
    /// Callers must independently validate the original epoch/auth owner.
    pub fn check_form_origin(&self, required: bool) -> Result<(), String> {
        if required && self.control_origin.is_none() {
            Err("Ordinary form origin is unverified; obtain fresh station metadata.".into())
        } else {
            Ok(())
        }
    }
    pub fn control_origin(&self) -> Option<&str> {
        self.control_origin.as_deref()
    }
    pub fn epoch(&self) -> Option<&str> {
        self.epoch.as_deref()
    }
}
#[derive(Default)]
pub struct Context {
    origin: String,
    authority: Option<String>,
    supported: BTreeSet<(String, String)>,
    authorities: BTreeMap<String, String>,
    unbound_support: BTreeSet<String>,
    origin_required: BTreeSet<(String, String)>,
    unbound_origin_required: BTreeSet<String>,
    generation: u64,
    exhausted: bool,
    observed: Option<(ModeFacts, f64)>,
    expired: bool,
    applied: Option<AppliedStamp>,
    applied_generation: u64,
    applied_exhausted: bool,
    observation_revision: u64,
    observation_exhausted: bool,
    // Never evict known epoch identities: latest-only storage lets an old
    // epoch return with another domain after an intervening epoch or lineage.
    epoch_bindings: BTreeMap<EpochKey, EpochBinding>,
    quarantined_lineages: BTreeSet<LineageKey>,
    protocol_exhausted: bool,
    protocol_error: Option<String>,
}
impl Context {
    fn coherent_pair(&mut self, origin: &str, s: &ModeFacts, received: f64, now: f64) -> bool {
        if self.protocol_exhausted {
            return false;
        }
        let lineage = (
            origin.to_owned(),
            s.backend_instance.clone(),
            s.authority_id.clone(),
        );
        let active_quarantine = self.quarantined_lineages.contains(&lineage);
        let complete = s.authority_current && s.fresh(received, now) && s.epoch_fencing_supported;
        let Some(epoch) = &s.policy_epoch else {
            return !active_quarantine;
        };
        let key = (
            lineage.0.clone(),
            lineage.1.clone(),
            lineage.2.clone(),
            epoch.clone(),
        );
        if self
            .epoch_bindings
            .get(&key)
            .is_some_and(|known| known.incoherent)
        {
            self.quarantined_lineages.insert(lineage);
            return false;
        }
        let Some(pair) = &s.current else {
            return !active_quarantine;
        };
        if !complete {
            return !active_quarantine;
        }
        let identity = PairIdentity {
            target: pair.id.clone(),
            mode: pair.mode_id.clone(),
            simulation: pair.simulation,
        };
        if let Some(known) = self.epoch_bindings.get_mut(&key) {
            if known.pair != identity {
                known.incoherent = true;
                self.quarantined_lineages.insert(lineage);
                return false;
            }
            // Recovery cannot merely restore an old known descriptor/epoch.
            // A trusted genuinely new epoch is required for this lineage.
            return !active_quarantine;
        }
        if self.epoch_bindings.len() >= MAX_RETAINED_EPOCH_BINDINGS {
            self.protocol_exhausted = true;
            return false;
        }
        self.epoch_bindings.insert(
            key,
            EpochBinding {
                pair: identity,
                incoherent: false,
            },
        );
        self.quarantined_lineages.remove(&lineage);
        true
    }
    fn advance_observation(&mut self) {
        match self.observation_revision.checked_add(1) {
            Some(next) => self.observation_revision = next,
            None => self.observation_exhausted = true,
        }
    }
    fn retire_applied(&mut self) {
        self.applied = None;
        match self.applied_generation.checked_add(1) {
            Some(next) => self.applied_generation = next,
            None => self.applied_exhausted = true,
        }
    }
    fn observe_applied(&mut self, origin: &str, s: &ModeFacts, received: f64, now: f64) {
        let changed = self.applied.as_ref().is_some_and(|old| {
            old.origin != origin
                || old.identity.backend != s.backend_instance
                || old.identity.authority != s.authority_id
                || old.identity.principal != s.principal_context
                || s.policy_epoch
                    .as_ref()
                    .is_some_and(|epoch| *epoch != old.identity.epoch)
                || s.current.as_ref().is_some_and(|pair| {
                    pair.id != old.identity.target
                        || pair.mode_id != old.identity.mode
                        || pair.simulation != old.identity.simulation
                })
        });
        if changed {
            self.retire_applied();
        }
        if self.applied_exhausted
            || !s.authority_current
            || !s.epoch_fencing_supported
            || !s.fresh(received, now)
        {
            return;
        }
        let (Some(epoch), Some(pair)) = (&s.policy_epoch, &s.current) else {
            return;
        };
        self.applied = Some(AppliedStamp {
            origin: origin.into(),
            generation: self.applied_generation,
            identity: AppliedIdentity {
                backend: s.backend_instance.clone(),
                authority: s.authority_id.clone(),
                principal: s.principal_context.clone(),
                epoch: epoch.clone(),
                target: pair.id.clone(),
                mode: pair.mode_id.clone(),
                simulation: pair.simulation,
            },
        });
    }
    fn retire(&mut self) {
        self.advance_observation();
        match self.generation.checked_add(1) {
            Some(next) => self.generation = next,
            None => self.exhausted = true,
        }
    }
    fn supported(&self) -> bool {
        self.unbound_support.contains(&self.origin)
            || self
                .authority
                .as_ref()
                .is_some_and(|a| self.supported.contains(&(self.origin.clone(), a.clone())))
    }
    /// A successful, current auth-owned response may tighten support before full
    /// decoding. Malformed descriptors never grant an epoch or operation lease.
    pub fn declare_supported(&mut self, origin: &str, authority: Option<&str>) {
        let authority = authority
            .filter(|a| !a.trim().is_empty() && a.len() <= 256 && !a.chars().any(char::is_control));
        if self.origin == origin
            && authority == self.authority.as_deref()
            && authority.is_some_and(|a| self.supported.contains(&(origin.into(), a.into())))
            && !self.unbound_support.contains(origin)
        {
            return;
        }
        self.retire();
        if self.applied.as_ref().is_some_and(|old| {
            old.origin != origin || authority.is_some_and(|a| a != old.identity.authority)
        }) {
            self.retire_applied();
        }
        if self.origin != origin {
            self.origin = origin.into();
            self.authority = self.authorities.get(origin).cloned();
        }
        self.unbound_support.insert(origin.into());
        if let Some(authority) = authority {
            self.supported.insert((origin.into(), authority.into()));
        }
        self.observed = None;
        self.expired = true;
    }
    /// Origin support can only tighten fallback, never install an epoch/seal.
    pub fn declare_control_origin_required(&mut self, origin: &str, authority: Option<&str>) {
        self.declare_supported(origin, authority);
        let authority = authority
            .filter(|a| !a.trim().is_empty() && a.len() <= 256 && !a.chars().any(char::is_control));
        let known = authority
            .is_some_and(|a| self.origin_required.contains(&(origin.into(), a.into())))
            && !self.unbound_origin_required.contains(origin);
        if known {
            return;
        }
        self.retire();
        self.unbound_origin_required.insert(origin.into());
        if let Some(authority) = authority {
            self.origin_required
                .insert((origin.into(), authority.into()));
        }
    }
    pub fn control_origin_required_for(&self, origin: &str) -> bool {
        self.unbound_origin_required.contains(origin)
            || self.authorities.get(origin).is_some_and(|authority| {
                self.origin_required
                    .contains(&(origin.into(), authority.clone()))
            })
    }
    pub fn generation_label(&self, origin: &str) -> String {
        let required = self.unbound_support.contains(origin)
            || self
                .authorities
                .get(origin)
                .is_some_and(|a| self.supported.contains(&(origin.into(), a.clone())));
        if required {
            format!("fenced:{}:{}", self.generation, self.exhausted)
        } else {
            "legacy".into()
        }
    }
    pub fn observe(&mut self, origin: &str, snapshot: ModeFacts, received: f64, now: f64) {
        self.advance_observation();
        self.protocol_error = None;
        if !self.coherent_pair(origin, &snapshot, received, now) {
            self.retire();
            self.retire_applied();
            self.observed = None;
            self.expired = true;
            self.protocol_error = Some(if self.protocol_exhausted {
                "Station policy identity history exhausted; restart this client before creating operations.".into()
            } else {
                "Station policy is incoherent: an applied epoch was reused for a different mode or execution domain. A fresh coherent epoch is required.".into()
            });
            return;
        }
        self.observe_applied(origin, &snapshot, received, now);
        self.tick(now);
        let changed = self.origin != origin
            || self.authority.as_ref() != Some(&snapshot.authority_id)
            || self.observed.as_ref().is_some_and(|(old, _)| {
                old.principal_context != snapshot.principal_context
                    || old.backend_instance != snapshot.backend_instance
                    || old.policy_epoch != snapshot.policy_epoch
                    || old.control_origin != snapshot.control_origin
                    || (old.admissions_open && !snapshot.admissions_open)
                    || (old.current.is_some()
                        && snapshot.current.is_none()
                        && (self.supported() || snapshot.epoch_fencing_supported))
            });
        // Refinement into a fenced authority retires pre-protocol untagged work.
        let newly_supported = snapshot.epoch_fencing_supported
            && !self
                .supported
                .contains(&(origin.to_owned(), snapshot.authority_id.clone()));
        let newly_origin_required = snapshot.control_origin_required
            && !self
                .origin_required
                .contains(&(origin.into(), snapshot.authority_id.clone()));
        if changed || newly_supported || newly_origin_required {
            self.retire();
        }
        // An unbound first declaration refines to this identity without legacy
        // reopening. A proven different previously known authority is a new lineage.
        let different = self
            .authorities
            .get(origin)
            .is_some_and(|old| old != &snapshot.authority_id);
        if self.unbound_support.remove(origin) && !different {
            self.supported
                .insert((origin.into(), snapshot.authority_id.clone()));
        }
        if self.unbound_origin_required.remove(origin) && !different {
            self.origin_required
                .insert((origin.into(), snapshot.authority_id.clone()));
        }
        if snapshot.control_origin_required {
            self.origin_required
                .insert((origin.into(), snapshot.authority_id.clone()));
        }
        self.origin = origin.into();
        self.authority = Some(snapshot.authority_id.clone());
        self.authorities
            .insert(origin.into(), snapshot.authority_id.clone());
        if snapshot.epoch_fencing_supported {
            self.supported
                .insert((origin.into(), snapshot.authority_id.clone()));
        }
        self.expired = false;
        self.observed = Some((snapshot, received));
        self.tick(now);
    }
    pub fn unavailable(&mut self, origin: &str) {
        self.advance_observation();
        if self
            .applied
            .as_ref()
            .is_some_and(|old| old.origin != origin)
        {
            self.retire_applied();
        }
        if self.origin != origin {
            self.retire();
            self.origin = origin.into();
            self.authority = self.authorities.get(origin).cloned();
        }
        // A failed probe cannot erase a known authority or its sticky support.
        if self.supported() && !self.expired {
            self.retire();
            self.expired = true;
        }
        self.observed = None;
    }
    pub fn tick(&mut self, now: f64) {
        let stale = self
            .observed
            .as_ref()
            .is_some_and(|(s, t)| !s.fresh(*t, now) || !s.authority_current);
        if self.supported() && stale && !self.expired {
            self.retire();
            self.expired = true;
        }
    }
    pub fn capture(&mut self, origin: &str, now: f64) -> Result<Lease, String> {
        self.tick(now);
        if self.exhausted || self.protocol_exhausted {
            return Err("Policy owner identity exhausted; reconnect with a new client.".into());
        }
        if self.origin != origin {
            self.retire();
            self.origin = origin.into();
            self.authority = self.authorities.get(origin).cloned();
            self.observed = None;
            self.expired = false;
        }
        if !self.supported() {
            return Ok(Lease {
                origin: origin.into(),
                generation: self.generation,
                epoch: None,
                control_origin: None,
            });
        }
        let Some((s, t)) = &self.observed else {
            return Err("Refresh station policy before creating an operation.".into());
        };
        if self.expired
            || !s.fresh(*t, now)
            || !s.authority_current
            || !s.epoch_fencing_supported
            || !s.admissions_open
            || s.current.is_none()
        {
            return Err("Station policy admission is unavailable or closed.".into());
        }
        Ok(Lease {
            origin: origin.into(),
            generation: self.generation,
            epoch: Some(s.policy_epoch.clone().ok_or("Policy epoch is unknown")?),
            control_origin: s.control_origin.clone(),
        })
    }
    pub fn current(&mut self, lease: &Lease, origin: &str, now: f64) -> bool {
        self.capture(origin, now)
            .is_ok_and(|current| current == *lease)
    }
}

impl Context {
    pub fn observation_ticket(&self, origin: &str) -> Result<ObservationTicket, String> {
        if self.observation_exhausted {
            return Err("Mode observation identity exhausted".into());
        }
        Ok(ObservationTicket {
            origin: origin.into(),
            revision: self.observation_revision,
        })
    }
    pub fn observation_current(&self, ticket: &ObservationTicket, origin: &str) -> bool {
        !self.observation_exhausted
            && ticket.origin == origin
            && ticket.revision == self.observation_revision
    }
    pub fn applied_stamp(&self) -> Option<AppliedStamp> {
        (!self.applied_exhausted)
            .then(|| self.applied.clone())
            .flatten()
    }
    pub fn matches_installed(&self, stamp: &AppliedStamp) -> bool {
        !self.applied_exhausted && self.applied.as_ref() == Some(stamp)
    }
    pub fn retire_applied_owner(&mut self) {
        if self.applied.is_some() {
            self.retire_applied();
            self.advance_observation();
        }
    }
    pub fn protocol_error(&self) -> Option<&str> {
        self.protocol_error.as_deref()
    }
    #[cfg(test)]
    pub fn observed_epoch(&self, origin: &str) -> Option<String> {
        (self.origin == origin)
            .then(|| self.observed.as_ref()?.0.policy_epoch.clone())
            .flatten()
    }
    pub fn required_for(&self, origin: &str) -> bool {
        self.unbound_support.contains(origin)
            || self.authorities.get(origin).is_some_and(|authority| {
                self.supported
                    .contains(&(origin.to_owned(), authority.clone()))
            })
    }
    pub fn ingress_context(&self, origin: &str) -> IngressContext {
        IngressContext {
            required: self.required_for(origin),
            applied: self.applied_stamp(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> ModeFacts {
        ModeFacts {
            backend_instance: "process-A".into(),
            authority_id: "custom-station".into(),
            principal_context: "operator-session".into(),
            policy_epoch: Some("owner-A:epoch-1".into()),
            authority_current: true,
            valid_for_ms: 5000,
            epoch_fencing_supported: true,
            control_origin_required: false,
            control_origin: None,
            admissions_open: true,
            current: Some(ModePair {
                id: "payload-live".into(),
                mode_id: "payload/checkout".into(),
                simulation: false,
            }),
        }
    }
    #[test]
    fn first_reuse_conflict_is_detected_after_epoch_and_lineage_round_trips() {
        for other_lineage in [false, true] {
            let mut c = Context::default();
            let mut original = snapshot();
            original.current.as_mut().unwrap().simulation = false;
            c.observe("station", original.clone(), 0., 0.);
            let mut intermediate = original.clone();
            if other_lineage {
                intermediate.backend_instance = "different-backend".into();
            } else {
                intermediate.policy_epoch = Some("owner-A:intermediate-coherent-epoch".into());
            }
            c.observe("station", intermediate, 1., 1.);
            assert!(c.protocol_error.is_none());
            assert_eq!(
                c.epoch_bindings.len(),
                2,
                "history retains both coherent identities"
            );
            let mut reused = original.clone();
            reused.current.as_mut().unwrap().simulation = true;
            c.observe("station", reused, 2., 2.);
            assert!(
                c.protocol_error.is_some(),
                "the FIRST conflict must be detected even after another epoch/lineage"
            );
            assert!(c.applied.is_none());
            assert!(c.capture("station", 3.).is_err());
            c.observe("station", original.clone(), 4., 4.);
            assert!(
                c.protocol_error.is_some(),
                "restoration cannot remint known reused epoch"
            );
            original.policy_epoch = Some("owner-A:never-seen-coherent-recovery".into());
            c.observe("station", original, 5., 5.);
            assert!(c.protocol_error.is_none());
            assert!(c.capture("station", 6.).is_ok());
        }
    }
    #[test]
    fn same_epoch_domain_reuse_is_quarantined_until_a_genuinely_new_epoch() {
        let mut c = Context::default();
        let mut s = snapshot();
        c.observe("station", s.clone(), 0., 0.);
        let old = c.capture("station", 1.).unwrap();
        let seal = c.applied.clone().unwrap();
        let original_pair = s.current.clone();
        let original_simulation = s.current.as_ref().unwrap().simulation;
        s.current.as_mut().unwrap().simulation = !original_simulation;
        c.observe("station", s.clone(), 2., 2.);
        assert!(c.protocol_error.is_some());
        assert!(c.applied.is_none());
        assert!(!c.current(&old, "station", 3.));
        assert!(c.capture("station", 3.).is_err());
        s.current = original_pair;
        c.observe("station", s.clone(), 4., 4.);
        assert!(
            c.protocol_error.is_some(),
            "restoring the descriptor cannot rehabilitate reused epoch"
        );
        assert!(c.capture("station", 5.).is_err());
        s.current = None;
        s.policy_epoch = None;
        c.observe("station", s.clone(), 6., 6.);
        assert!(
            c.protocol_error.is_some(),
            "partial status does not clear quarantine"
        );
        let mut fresh = snapshot();
        fresh.policy_epoch = Some("owner-A:coherent-new-epoch".into());
        c.observe("station", fresh, 7., 7.);
        assert!(c.protocol_error.is_none());
        assert!(c.capture("station", 8.).is_ok());
        assert_ne!(c.applied.as_ref(), Some(&seal));
        c.observe("station", snapshot(), 9., 9.);
        assert!(
            c.protocol_error.is_some(),
            "old quarantined epoch never returns after new epoch"
        );
    }
    #[test]
    fn nullable_current_preserves_readonly_seal_but_ordinary_intent_never_revives() {
        let mut c = Context::default();
        let s = snapshot();
        c.observe("station", s.clone(), 0., 0.);
        let ordinary = c.capture("station", 1.).unwrap();
        let seal = c.applied.clone().unwrap();
        let mut partial = s.clone();
        partial.current = None;
        c.observe("station", partial, 2., 2.);
        assert_eq!(c.applied.as_ref(), Some(&seal));
        assert!(c.capture("station", 3.).is_err());
        c.observe("station", s, 4., 4.);
        assert!(!c.current(&ordinary, "station", 5.));
        assert!(c.capture("station", 5.).is_ok());
    }
    #[test]
    fn quarantine_survives_lineage_round_trip_and_capacity_never_falls_back() {
        let mut c = Context::default();
        let original = snapshot();
        c.observe("station", original.clone(), 0., 0.);
        let mut changed = original.clone();
        changed.current.as_mut().unwrap().id = "different-pair".into();
        c.observe("station", changed, 1., 1.);
        assert!(c.protocol_error.is_some());
        let mut other = original.clone();
        other.authority_id = "fresh-authority".into();
        c.observe("station", other, 2., 2.);
        assert!(c.protocol_error.is_none());
        c.observe("station", original.clone(), 3., 3.);
        assert!(c.protocol_error.is_some());
        let mut bounded = Context::default();
        bounded.observe("station", original.clone(), 0., 0.);
        for n in 0..MAX_RETAINED_EPOCH_BINDINGS - 1 {
            bounded.epoch_bindings.insert(
                (
                    "other".into(),
                    "process".into(),
                    "authority".into(),
                    n.to_string(),
                ),
                EpochBinding {
                    pair: PairIdentity {
                        target: "safe".into(),
                        mode: "safe".into(),
                        simulation: true,
                    },
                    incoherent: false,
                },
            );
        }
        let original_key = (
            "station".to_owned(),
            original.backend_instance.clone(),
            original.authority_id.clone(),
            original.policy_epoch.clone().unwrap(),
        );
        let mut new_epoch = original;
        new_epoch.policy_epoch = Some("genuinely-new-after-capacity".into());
        bounded.observe("station", new_epoch, 1., 1.);
        assert!(bounded.protocol_exhausted);
        assert_eq!(
            bounded.epoch_bindings.len(),
            MAX_RETAINED_EPOCH_BINDINGS,
            "known proof is never silently evicted"
        );
        assert!(bounded.applied.is_none());
        assert!(
            bounded.epoch_bindings.contains_key(&original_key),
            "overflow retains the original known descriptor"
        );
        assert!(
            bounded.capture("new-legacy-origin", 2.).is_err(),
            "capacity exhaustion never silently forgets a known epoch"
        );
    }
    #[test]
    fn applied_identity_survives_closure_expiry_and_unknown_probe_but_not_commit_or_aba() {
        let mut c = Context::default();
        let mut s = snapshot();
        c.observe("station", s.clone(), 0., 0.);
        let original = c.applied.clone().unwrap();
        let ordinary = c.capture("station", 1.).unwrap();
        s.admissions_open = false;
        c.observe("station", s.clone(), 2., 2.);
        assert_eq!(c.applied.as_ref(), Some(&original));
        assert!(!c.current(&ordinary, "station", 3.));
        c.tick(10000.);
        c.unavailable("station");
        assert_eq!(c.applied.as_ref(), Some(&original));
        let mut unknown = s.clone();
        unknown.policy_epoch = None;
        c.observe("station", unknown, 10001., 10001.);
        assert_eq!(c.applied.as_ref(), Some(&original));
        s.policy_epoch = Some("owner-A:epoch-2".into());
        c.observe("station", s.clone(), 10002., 10002.);
        assert_ne!(c.applied.as_ref(), Some(&original));
        s.policy_epoch = Some(original.epoch().into());
        c.observe("station", s, 10003., 10003.);
        assert_ne!(c.applied.as_ref(), Some(&original));
    }
    #[test]
    fn retired_observer_proof_cannot_reissue_the_same_stamp_in_a_new_ui_runtime() {
        let mut c = Context::default();
        let s = snapshot();
        c.observe("station", s.clone(), 0., 0.);
        let old = c.applied.clone().unwrap();
        // The observer retires missing/retired original Rc proof BEFORE accepting
        // a fresh response, even when the backend repeats the same identity.
        c.retire_applied();
        c.observe("station", s, 1., 1.);
        assert_ne!(c.applied.as_ref(), Some(&old));
        assert_eq!(c.applied.as_ref().unwrap().epoch(), old.epoch());
    }
    #[test]
    fn applied_partial_known_authority_change_retires_without_minting_unknown_seal() {
        let mut c = Context::default();
        let mut s = snapshot();
        c.observe("station", s.clone(), 0., 0.);
        s.principal_context = "new-principal".into();
        s.policy_epoch = None;
        c.observe("station", s, 1., 1.);
        assert!(c.applied.is_none());
        c.applied_generation = u64::MAX;
        c.retire_applied();
        c.observe("station", snapshot(), 2., 2.);
        assert!(c.applied_exhausted && c.applied.is_none());
    }
    #[test]
    fn observation_revision_changes_even_for_same_epoch_new_snapshot_and_closure() {
        let mut c = Context::default();
        let mut s = snapshot();
        c.observe("station", s.clone(), 0., 0.);
        let before = c.observation_revision;
        c.observe("station", s.clone(), 1., 1.);
        assert_ne!(c.observation_revision, before);
        let before = c.observation_revision;
        s.admissions_open = false;
        c.observe("station", s, 2., 2.);
        assert_ne!(c.observation_revision, before);
    }
    #[test]
    fn ordinary_owners_never_revive_after_closure_or_expiry() {
        let mut c = Context::default();
        let s = snapshot();
        c.observe("station", s.clone(), 0., 0.);
        let lease = c.capture("station", 1.).unwrap();
        assert_eq!(lease.epoch(), Some("owner-A:epoch-1"));
        let mut closed = s.clone();
        closed.admissions_open = false;
        c.observe("station", closed, 10., 10.);
        assert!(!c.current(&lease, "station", 11.));
        c.observe("station", s.clone(), 12., 12.);
        assert!(!c.current(&lease, "station", 13.));
        let renewed = c.capture("station", 13.).unwrap();
        c.tick(10000.);
        c.observe("station", s, 10001., 10001.);
        assert!(!c.current(&renewed, "station", 10002.));
    }
    #[test]
    fn supported_authority_never_downgrades_after_reconnect_error() {
        let mut c = Context::default();
        c.unavailable("station");
        let legacy = c.capture("station", 0.).unwrap();
        assert_eq!(legacy.epoch(), None);
        let s = snapshot();
        c.observe("station", s.clone(), 1., 1.);
        assert!(!c.current(&legacy, "station", 2.));
        c.unavailable("station");
        assert!(c.capture("station", 3.).is_err());
        let mut downgrade = s;
        downgrade.epoch_fencing_supported = false;
        downgrade.policy_epoch = None;
        c.observe("station", downgrade, 4., 4.);
        assert!(c.capture("station", 5.).is_err());
    }
    #[test]
    fn supported_unknown_epoch_stays_closed_after_missing_capability() {
        let mut c = Context::default();
        let mut s = snapshot();
        s.policy_epoch = None;
        c.observe("station", s, 0., 0.);
        assert!(c.capture("station", 1.).is_err());
        c.unavailable("station");
        assert!(c.capture("station", 2.).is_err());
    }
    #[test]
    fn malformed_first_declaration_refines_without_legacy_fallback() {
        let mut c = Context::default();
        let legacy = c.capture("station", 0.).unwrap();
        c.declare_supported("station", None);
        assert!(c.capture("station", 1.).is_err());
        c.unavailable("station");
        assert!(c.capture("station", 2.).is_err());
        let mut legacy_report = snapshot();
        legacy_report.epoch_fencing_supported = false;
        legacy_report.policy_epoch = None;
        c.observe("station", legacy_report.clone(), 3., 3.);
        assert!(c.capture("station", 4.).is_err());
        assert!(!c.current(&legacy, "station", 4.));
        // A proven different authority is an explicit new lineage, not an auth
        // refresh. An unrelated origin can also remain a genuine legacy backend.
        legacy_report.authority_id = "explicit-other-authority".into();
        c.observe("station", legacy_report, 5., 5.);
        assert_eq!(c.capture("station", 6.).unwrap().epoch(), None);
        assert_eq!(c.capture("other-origin", 7.).unwrap().epoch(), None);
        c.capture("station", 8.).unwrap();
    }
    #[test]
    fn principal_context_retirement_is_independent_of_token_and_epoch() {
        let mut c = Context::default();
        let mut s = snapshot();
        c.observe("station", s.clone(), 0., 0.);
        let old = c.capture("station", 1.).unwrap();
        s.principal_context = "same-token-new-permissions".into();
        c.observe("station", s, 2., 2.);
        assert!(!c.current(&old, "station", 3.));
        assert_eq!(
            c.capture("station", 3.).unwrap().epoch(),
            Some("owner-A:epoch-1")
        );
    }
    #[test]
    fn view_identity_is_read_only_stable_for_legacy_and_distinct_after_coalesced_reopen() {
        let mut c = Context::default();
        let legacy_key = c.generation_label("station");
        c.capture("station", 0.).unwrap();
        assert_eq!(c.generation_label("station"), legacy_key);
        let mut s = snapshot();
        c.observe("station", s.clone(), 1., 1.);
        let before = c.generation_label("station");
        let generation = c.generation;
        assert_eq!(c.generation_label("station"), before);
        assert_eq!(c.generation, generation);
        s.admissions_open = false;
        c.observe("station", s.clone(), 2., 2.);
        s.admissions_open = true;
        c.observe("station", s, 3., 3.);
        assert_ne!(c.generation_label("station"), before);
        c.generation = u64::MAX;
        c.retire();
        assert!(c.generation_label("station").ends_with(":true"));
        assert!(c.capture("station", 4.).is_err());
    }

    #[test]
    fn same_descriptor_roundtrip_is_valid_but_old_owner_never_resurrects() {
        for other_lineage in [false, true] {
            let mut c = Context::default();
            let original = snapshot();
            c.observe("station", original.clone(), 0., 0.);
            let first = c.capture("station", 1.).unwrap();
            let first_stamp = c.applied_stamp().unwrap();
            c.observe("station", original.clone(), 2., 2.);
            assert!(c.current(&first, "station", 3.));
            assert_eq!(c.applied_stamp(), Some(first_stamp.clone()));
            let mut middle = original.clone();
            if other_lineage {
                middle.backend_instance = "other-backend".into();
            } else {
                middle.policy_epoch = Some("owner-A:other-epoch".into());
            }
            c.observe("station", middle, 4., 4.);
            c.observe("station", original, 5., 5.);
            assert!(c.protocol_error().is_none());
            assert!(c.capture("station", 6.).is_ok());
            assert!(!c.current(&first, "station", 6.));
            assert_ne!(c.applied_stamp(), Some(first_stamp));
        }
    }
    #[test]
    fn origins_do_not_share_descriptor_or_quarantine_authority() {
        let mut c = Context::default();
        let a = snapshot();
        let mut b = a.clone();
        b.current.as_mut().unwrap().simulation = true;
        c.observe("origin-A", a.clone(), 0., 0.);
        let old_a = c.capture("origin-A", 1.).unwrap();
        c.observe("origin-B", b.clone(), 2., 2.);
        assert!(c.protocol_error().is_none());
        assert!(c.capture("origin-B", 3.).is_ok());
        b.current.as_mut().unwrap().simulation = false;
        c.observe("origin-B", b, 4., 4.);
        assert!(c.protocol_error().is_some());
        c.observe("origin-A", a, 5., 5.);
        assert!(c.protocol_error().is_none());
        assert!(c.capture("origin-A", 6.).is_ok());
        assert!(!c.current(&old_a, "origin-A", 6.));
    }
    #[test]
    fn actual_observation_predicate_requires_latest_owner_verified_fresh_context() {
        let base = ObservationFacts {
            verified: true,
            original_owner_current: true,
            authority_current: true,
            valid_for_ms: 5000,
            epoch_fencing_supported: true,
            epoch_known: true,
            current_known: true,
        };
        assert!(observation_is_current(base, 10., 5010., || true));
        assert!(!observation_is_current(base, 10., 5010.001, || true));
        assert!(!observation_is_current(base, 10., 9.999, || true));
        assert!(!observation_is_current(base, 10., f64::NAN, || true));
        for index in 0..6 {
            let mut facts = base;
            match index {
                0 => facts.verified = false,
                1 => facts.original_owner_current = false,
                2 => facts.authority_current = false,
                3 => facts.epoch_fencing_supported = false,
                4 => facts.epoch_known = false,
                _ => facts.current_known = false,
            }
            assert!(
                !observation_is_current(facts, 10., 11., || true),
                "no raw snapshot TTL can replace a missing original observation proof"
            );
        }
        let calls = std::cell::Cell::new(0);
        let mut retired = base;
        retired.original_owner_current = false;
        assert!(!observation_is_current(retired, 10., 11., || {
            calls.set(calls.get() + 1);
            true
        }));
        assert_eq!(
            calls.get(),
            0,
            "retired observation cannot query/rebind a fresh policy scope"
        );
        let mut legacy = base;
        legacy.epoch_fencing_supported = false;
        legacy.epoch_known = false;
        legacy.current_known = false;
        assert!(
            observation_is_current(legacy, 10., 11., || false),
            "never-supported readonly legacy observation remains separate"
        );
    }
    #[test]
    fn pure_adapter_queries_preserve_original_ticket_and_applied_context() {
        fn require_send_sync<T: Send + Sync>() {}
        require_send_sync::<AppliedStamp>();
        require_send_sync::<Lease>();
        require_send_sync::<ObservationTicket>();
        let mut c = Context::default();
        let ticket = c.observation_ticket("station").unwrap();
        assert!(c.observation_current(&ticket, "station"));
        c.observe("station", snapshot(), 0., 0.);
        assert!(!c.observation_current(&ticket, "station"));
        let stamp = c.applied_stamp().unwrap();
        assert_eq!(stamp.origin(), "station");
        assert!(c.matches_installed(&stamp));
        assert_eq!(
            c.observed_epoch("station").as_deref(),
            Some("owner-A:epoch-1")
        );
        assert!(c.observed_epoch("other").is_none());
        assert!(c.required_for("station"));
        assert!(!c.required_for("other"));
        let incoming = c.ingress_context("station");
        assert!(incoming.required);
        assert_eq!(incoming.applied, Some(stamp.clone()));
        c.retire_applied_owner();
        assert!(!c.matches_installed(&stamp));
        assert!(c.applied_stamp().is_none());
    }
    #[test]
    fn actual_observation_path_reaches_capacity_without_eviction_or_reopening() {
        let mut c = Context::default();
        for n in 0..MAX_RETAINED_EPOCH_BINDINGS {
            let mut facts = snapshot();
            facts.policy_epoch = Some(format!("owner-A:epoch-{n}"));
            c.observe("station", facts, n as f64, n as f64);
            assert!(c.protocol_error().is_none());
            assert!(c.capture("station", n as f64).is_ok());
        }
        let keys: Vec<_> = c.epoch_bindings.keys().cloned().collect();
        let mut extra = snapshot();
        extra.policy_epoch = Some("owner-A:epoch-overflow".into());
        c.observe("station", extra, 2000., 2000.);
        assert!(c.protocol_error().is_some());
        assert!(c.applied_stamp().is_none());
        assert!(c.capture("station", 2001.).is_err());
        assert_eq!(c.epoch_bindings.keys().cloned().collect::<Vec<_>>(), keys);
        c.observe("station", snapshot(), 2002., 2002.);
        assert!(c.protocol_error().is_some());
        assert!(c.capture("station", 2003.).is_err());
        assert!(c.applied_stamp().is_none());
    }
    #[test]
    fn ordinary_origin_rotation_never_revives_old_lease_or_retires_applied_read_seal() {
        let mut c = Context::default();
        let mut s = snapshot();
        s.control_origin_required = true;
        s.control_origin = Some("form-origin-A".into());
        c.observe("station", s.clone(), 0., 0.);
        let old = c.capture("station", 1.).unwrap();
        let seal = c.applied_stamp().unwrap();
        assert_eq!(old.control_origin(), Some("form-origin-A"));
        old.check_form_origin(true).unwrap();
        c.observe("station", s.clone(), 2., 2.);
        assert!(c.current(&old, "station", 3.));
        s.control_origin = Some("form-origin-B".into());
        c.observe("station", s.clone(), 4., 4.);
        assert!(!c.current(&old, "station", 5.));
        assert_eq!(c.applied_stamp(), Some(seal.clone()));
        s.control_origin = Some("form-origin-A".into());
        c.observe("station", s, 6., 6.);
        assert!(
            !c.current(&old, "station", 7.),
            "same opaque token text cannot revive old operation generation"
        );
        assert_eq!(c.applied_stamp(), Some(seal));
    }
    #[test]
    fn required_null_form_origin_does_not_block_epoch_commands_or_readonly_context() {
        let mut c = Context::default();
        let mut s = snapshot();
        s.control_origin_required = true;
        c.observe("station", s, 0., 0.);
        let command_epoch = c.capture("station", 1.).unwrap();
        assert!(c.current(&command_epoch, "station", 2.));
        assert!(c.applied_stamp().is_some());
        assert!(c.control_origin_required_for("station"));
        assert!(
            command_epoch.check_form_origin(true).is_err(),
            "only ordinary form capture requires GET origin"
        );
        assert_eq!(
            command_epoch.control_origin(),
            None,
            "GET origin is never synthesized from a command seal"
        );
    }
    #[test]
    fn required_origin_stays_sticky_through_unknown_and_false_reports_per_lineage() {
        let mut c = Context::default();
        let mut s = snapshot();
        s.control_origin_required = true;
        s.control_origin = Some("form-A".into());
        c.observe("station", s.clone(), 0., 0.);
        let old = c.capture("station", 1.).unwrap();
        c.unavailable("station");
        assert!(c.control_origin_required_for("station"));
        s.control_origin_required = false;
        s.control_origin = None;
        c.observe("station", s.clone(), 2., 2.);
        assert!(c.control_origin_required_for("station"));
        assert!(!c.current(&old, "station", 3.));
        assert!(
            c.capture("station", 3.)
                .unwrap()
                .check_form_origin(c.control_origin_required_for("station"))
                .is_err()
        );
        let mut other = s.clone();
        other.authority_id = "different-authority".into();
        other.epoch_fencing_supported = false;
        c.observe("station", other, 4., 4.);
        assert!(!c.control_origin_required_for("station"));
        assert!(
            c.capture("station", 5.)
                .unwrap()
                .check_form_origin(false)
                .is_ok()
        );
        c.observe("station", s, 6., 6.);
        assert!(c.control_origin_required_for("station"));
        assert!(!c.control_origin_required_for("other-origin"));
    }
    #[test]
    fn malformed_origin_declaration_only_tightens_support_and_fences_old_ticket() {
        let mut c = Context::default();
        let old = c.capture("station", 0.).unwrap();
        let ticket = c.observation_ticket("station").unwrap();
        c.declare_control_origin_required("station", None);
        assert!(c.required_for("station"));
        assert!(c.control_origin_required_for("station"));
        assert!(!c.observation_current(&ticket, "station"));
        assert!(!c.current(&old, "station", 1.));
        assert!(c.applied_stamp().is_none());
        c.observe("station", snapshot(), 2., 2.);
        assert!(
            c.control_origin_required_for("station"),
            "first valid refinement cannot erase unbound required origin"
        );
        let epoch = c.capture("station", 3.).unwrap();
        assert!(epoch.check_form_origin(true).is_err());
    }
    #[test]
    fn required_origin_activation_retires_previously_missing_or_optional_lease() {
        for token in [None, Some("stable-form-token".to_string())] {
            let mut c = Context::default();
            let mut s = snapshot();
            s.control_origin = token;
            c.observe("station", s.clone(), 0., 0.);
            let old = c.capture("station", 1.).unwrap();
            c.declare_control_origin_required("station", Some(&s.authority_id));
            s.control_origin_required = true;
            c.observe("station", s, 2., 2.);
            assert!(!c.current(&old, "station", 3.));
        }
    }
}
