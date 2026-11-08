//! Backend-defined policy modes and orthogonal execution domains (wire V2).
//! Opaque identifiers never become commands, URLs or inferred hardware policy.
use super::station_policy_model::{MAX_TTL_MS, elapsed};
use serde::{Deserialize, Serialize};
fn nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}
fn text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}
fn prose(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= maximum
        && !value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Mode {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Current {
    pub id: String,
    pub mode_id: String,
    pub label: String,
    pub simulation: bool,
    pub output_domain: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct OptionEntry {
    pub id: String,
    pub mode_id: String,
    pub simulation: bool,
    pub label: String,
    pub available: bool,
    #[serde(deserialize_with = "nullable")]
    pub reason: Option<String>,
    pub transition_kind: String,
    pub output_domain: String,
    /// Newer backends hold every mode in one process and switch while idle,
    /// without restarting. Absent on older backends, which restart instead.
    #[serde(default)]
    pub in_process: bool,
}
impl OptionEntry {
    /// The backend fences this switch itself and needs no per-request epoch
    /// protocol: either a controlled restart into another installed build, or
    /// (with `in_process`) an idle-only switch inside the running backend that
    /// the backend still advertises as `restart` for older clients.
    pub fn restarts(&self) -> bool {
        self.transition_kind == "restart"
    }
    /// The server-fenced switch happens inside the running backend: sign-in
    /// and connection are kept and the layout and controls refresh in place.
    pub fn switches_in_process(&self) -> bool {
        self.restarts() && self.in_process
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Permissions {
    pub can_change: bool,
    pub can_cancel_transition: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Transition {
    pub id: String,
    pub target_id: String,
    pub state: String,
    pub phase: String,
    #[serde(deserialize_with = "nullable")]
    pub error: Option<String>,
}
impl Transition {
    fn valid(&self) -> bool {
        text(&self.id, 256)
            && text(&self.target_id, 256)
            && text(&self.phase, 512)
            && matches!(self.state.as_str(), "pending" | "applied" | "failed")
            && self.error.as_ref().is_none_or(|v| prose(v, 4096))
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub version: u8,
    pub backend_instance: String,
    pub principal_context: String,
    pub authority_id: String,
    pub revision: String,
    #[serde(deserialize_with = "nullable")]
    pub policy_epoch: Option<String>,
    pub authority_current: bool,
    pub valid_for_ms: u64,
    pub epoch_fencing_supported: bool,
    #[serde(default)]
    pub control_origin_required: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub control_origin: Option<String>,
    pub admissions_open: bool,
    #[serde(deserialize_with = "nullable")]
    pub current: Option<Current>,
    pub modes: Vec<Mode>,
    pub permissions: Permissions,
    pub change_supported: bool,
    pub cancel_supported: bool,
    pub options: Vec<OptionEntry>,
    #[serde(deserialize_with = "nullable")]
    pub transition: Option<Transition>,
    #[serde(deserialize_with = "nullable")]
    pub reason: Option<String>,
}
impl Snapshot {
    pub fn decode(value: serde_json::Value) -> Result<Self, String> {
        if !value.as_object().is_some_and(|v| {
            ["current", "transition", "policy_epoch", "reason"]
                .iter()
                .all(|key| v.contains_key(*key))
        }) {
            return Err("Mode capability is missing required nullable status.".into());
        }
        if value["control_origin_required"].as_bool() == Some(true)
            && !value.as_object().is_some_and(|fields| fields.contains_key("control_origin")) {
            return Err("Mode capability is missing nullable ordinary control origin.".into());
        }
        let result: Self =
            serde_json::from_value(value).map_err(|_| "Unsupported mode capability.".to_owned())?;
        if !result.valid() {
            return Err("Invalid mode capability.".into());
        }
        Ok(result)
    }
    fn valid(&self) -> bool {
        self.version == 2
            && [
                &self.backend_instance,
                &self.principal_context,
                &self.authority_id,
                &self.revision,
            ]
            .iter()
            .all(|v| text(v, 256))
            && (1..=MAX_TTL_MS).contains(&self.valid_for_ms)
            && self.policy_epoch.as_ref().is_none_or(|v| text(v, 512))
            && (!self.control_origin_required || self.epoch_fencing_supported)
            && self.control_origin.as_ref().is_none_or(|v| text(v, 4096))
            && self.reason.as_ref().is_none_or(|v| prose(v, 4096))
            && self.modes.len() <= 64
            && self.options.len() <= 128
            && self.modes.iter().enumerate().all(|(i, m)| {
                text(&m.id, 256)
                    && text(&m.label, 512)
                    && !self.modes[..i].iter().any(|p| p.id == m.id)
            })
            && self.current.as_ref().is_none_or(|c| {
                text(&c.id, 256)
                    && text(&c.label, 512)
                    && text(&c.output_domain, 512)
                    && self.modes.iter().any(|m| m.id == c.mode_id)
            })
            && self.options.iter().enumerate().all(|(i, o)| {
                text(&o.id, 256)
                    && text(&o.label, 512)
                    && text(&o.output_domain, 512)
                    && matches!(o.transition_kind.as_str(), "in_place" | "restart")
                    && self.modes.iter().any(|m| m.id == o.mode_id)
                    && o.reason.as_ref().is_none_or(|v| prose(v, 4096))
                    && !self.options[..i].iter().any(|p| {
                        p.id == o.id || (p.mode_id == o.mode_id && p.simulation == o.simulation)
                    })
            })
            && self.transition.as_ref().is_none_or(Transition::valid)
    }
    pub fn fresh(&self, received: f64, now: f64) -> bool {
        elapsed(received, now, self.valid_for_ms)
    }
    pub fn option(&self, id: &str) -> Option<&OptionEntry> {
        self.options.iter().find(|o| o.id == id)
    }
    pub fn pair(&self, mode_id: &str, simulation: bool) -> Option<&OptionEntry> {
        self.options
            .iter()
            .find(|o| o.mode_id == mode_id && o.simulation == simulation)
    }
    pub fn reason(&self, target: &str, received: f64, now: f64) -> Option<String> {
        if !self.fresh(received, now) {
            return Some("Refresh the station's mode capability first.".into());
        }
        if !self.authority_current {
            return Some(
                "Mode authority is not current; this is a last-reported observation.".into(),
            );
        }
        if !self.change_supported {
            return Some(
                self.reason
                    .clone()
                    .unwrap_or_else(|| "Mode changes are unavailable on this backend.".into()),
            );
        }
        if self.policy_epoch.is_none() {
            return Some("The station's actual policy epoch is unknown.".into());
        }
        let restarts = self.option(target).is_some_and(OptionEntry::restarts);
        // Mode repair has its own exact capability/ticket authority. Ordinary
        // admission closure alone cannot forbid a backend-advertised repair.
        if !self.epoch_fencing_supported && !restarts {
            return Some("The station's original-epoch protocol is not available.".into());
        }
        if !self.permissions.can_change {
            return Some("This session cannot change station mode.".into());
        }
        if self
            .transition
            .as_ref()
            .is_some_and(|t| t.state == "pending")
        {
            return Some("A mode transition is pending.".into());
        }
        let Some(current) = &self.current else {
            return Some("The actual current mode is unknown.".into());
        };
        let Some(option) = self.option(target) else {
            return Some("Choose an explicitly supported mode and Simulation combination.".into());
        };
        if !option.available {
            return Some(
                option
                    .reason
                    .clone()
                    .unwrap_or_else(|| "This combination is unavailable.".into()),
            );
        }
        if current.id == option.id && self.admissions_open {
            return Some("This combination is already reported active.".into());
        }
        None
    }
    pub fn binding(&self, target_id: &str) -> Option<Binding> {
        self.option(target_id)?;
        Some(Binding {
            version: 2,
            backend_instance: self.backend_instance.clone(),
            principal_context: self.principal_context.clone(),
            authority_id: self.authority_id.clone(),
            revision: self.revision.clone(),
            policy_epoch: self.policy_epoch.clone()?,
            current_id: self.current.as_ref()?.id.clone(),
            target_id: target_id.into(),
        })
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Binding {
    #[serde(skip_serializing)]
    pub version: u8,
    pub backend_instance: String,
    pub principal_context: String,
    pub authority_id: String,
    pub revision: String,
    pub policy_epoch: String,
    pub current_id: String,
    pub target_id: String,
}
#[derive(Clone, Debug, Deserialize)]
pub(super) struct Challenge {
    #[serde(flatten)]
    pub binding: Binding,
    pub ticket: String,
    pub transition_id: String,
    pub current: Current,
    pub target: OptionEntry,
    pub summary: String,
    #[serde(deserialize_with = "nullable")]
    pub required_confirmation: Option<String>,
    pub expires_in_ms: u64,
}
impl Challenge {
    pub fn matches(&self, binding: &Binding) -> bool {
        &self.binding == binding
            && text(&self.ticket, 512)
            && text(&self.transition_id, 256)
            && self.current.id == binding.current_id
            && self.target.id == binding.target_id
            && prose(&self.summary, 8192)
            && self
                .required_confirmation
                .as_ref()
                .is_none_or(|v| text(v, 256))
            && (1..=15000).contains(&self.expires_in_ms)
    }
    pub fn fresh(&self, received: f64, now: f64) -> bool {
        elapsed(received, now, self.expires_in_ms) && self.expires_in_ms <= 15000
    }
    pub fn confirms(&self, value: &str) -> bool {
        self.required_confirmation.as_deref().unwrap_or("") == value
    }
}
#[derive(Clone, Serialize)]
pub(super) struct Confirmation {
    #[serde(flatten)]
    pub binding: Binding,
    pub ticket: String,
    pub transition_id: String,
    pub confirmation: String,
}
#[derive(Clone, Debug, Deserialize)]
pub(super) struct Accepted {
    pub version: u8,
    pub backend_instance: String,
    pub principal_context: String,
    pub authority_id: String,
    pub policy_epoch: String,
    pub current_id: String,
    pub target_id: String,
    pub accepted: bool,
    pub transition: Transition,
}
impl Accepted {
    pub fn matches(&self, request: &Confirmation) -> bool {
        self.version == 2
            && self.accepted
            && self.backend_instance == request.binding.backend_instance
            && self.principal_context == request.binding.principal_context
            && self.authority_id == request.binding.authority_id
            && self.policy_epoch == request.binding.policy_epoch
            && self.current_id == request.binding.current_id
            && self.target_id == request.binding.target_id
            && self.transition.id == request.transition_id
            && self.transition.target_id == request.binding.target_id
            && self.transition.state == "pending"
            && self.transition.valid()
            && self.transition.error.is_none()
    }
}
/// The prepared operation is retained even if Confirm's response is lost.
/// A receipt alone is never an applied readback.
pub(super) fn applied_by(
    request: &Confirmation,
    target: &OptionEntry,
    snapshot: &Snapshot,
    received: f64,
    now: f64,
) -> bool {
    snapshot.fresh(received, now)
        && snapshot.authority_current
        && snapshot.backend_instance == request.binding.backend_instance
        && snapshot.authority_id == request.binding.authority_id
        && snapshot.principal_context == request.binding.principal_context
        && snapshot
            .policy_epoch
            .as_ref()
            .is_some_and(|p| p != &request.binding.policy_epoch)
        && snapshot.current.as_ref().is_some_and(|c| {
            c.id == request.binding.target_id
                && c.mode_id == target.mode_id
                && c.simulation == target.simulation
        })
        && snapshot.transition.as_ref().is_some_and(|t| {
            t.id == request.transition_id
                && t.target_id == request.binding.target_id
                && t.state == "applied"
        })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(crate) fn example_snapshot() -> Snapshot {
        Snapshot::decode(example()).unwrap()
    }
    fn example() -> serde_json::Value {
        serde_json::json!({"version":2,"backend_instance":"process-A","authority_id":"custom-station","principal_context":"operator-session","revision":"choices-r9","policy_epoch":"owner-A:epoch-1","authority_current":true,"valid_for_ms":5000,"epoch_fencing_supported":true,"admissions_open":true,"change_supported":true,"cancel_supported":false,"permissions":{"can_change":true,"can_cancel_transition":false},"current":{"id":"payload-live","mode_id":"payload/checkout","label":"Payload checkout · actual outputs","simulation":false,"output_domain":"Backend controlled outputs"},"modes":[{"id":"payload/checkout","label":"Payload checkout"},{"id":"custom α","label":"Third team's test"}],"options":[{"id":"third-sim","mode_id":"custom α","label":"Third team simulated","simulation":true,"available":true,"reason":null,"transition_kind":"in_place","output_domain":"Isolated simulated actions and telemetry"}],"transition":null,"reason":null})
    }
    fn challenge(s: &Snapshot) -> Challenge {
        Challenge {
            binding: s.binding("third-sim").unwrap(),
            ticket: "once".into(),
            transition_id: "known-before-confirm".into(),
            current: s.current.clone().unwrap(),
            target: s.option("third-sim").unwrap().clone(),
            summary: "Apply this backend-owned policy and isolated domain".into(),
            required_confirmation: Some("SIMULATE".into()),
            expires_in_ms: 15000,
        }
    }
    #[test]
    fn arbitrary_modes_and_orthogonal_pairs_are_explicit_not_inferred() {
        let s = example_snapshot();
        assert!(s.pair("custom α", true).is_some());
        assert!(s.pair("custom α", false).is_none());
        assert!(s.reason("third-sim", 100., 5100.).is_none());
        assert!(s.reason("third-sim", 100., 5100.01).is_some());
        assert!(s.reason("third-sim", 100., 99.).is_some());
        assert!(s.reason("third-sim", 100., f64::NAN).is_some());
        let b = serde_json::to_value(s.binding("third-sim").unwrap()).unwrap();
        assert_eq!(b["policy_epoch"], "owner-A:epoch-1");
        assert_eq!(b["target_id"], "third-sim");
        assert!(!b.as_object().unwrap().contains_key("simulation"));
        assert!(!b.as_object().unwrap().contains_key("version"));
        assert_eq!(s.current.unwrap().id, "payload-live"); // current need not be selectable
    }
    #[test]
    fn duplicates_missing_pair_fields_unknown_modes_and_v1_fail_closed() {
        for field in [
            "current",
            "transition",
            "policy_epoch",
            "modes",
            "epoch_fencing_supported",
            "admissions_open",
            "cancel_supported",
        ] {
            let mut v = example();
            v.as_object_mut().unwrap().remove(field);
            assert!(Snapshot::decode(v).is_err(), "{field}");
        }
        let mut v = example();
        v["version"] = 1.into();
        assert!(Snapshot::decode(v).is_err());
        let mut v = example();
        v["options"][0]["mode_id"] = "not-supplied".into();
        assert!(Snapshot::decode(v).is_err());
        let mut v = example();
        v["options"][0]
            .as_object_mut()
            .unwrap()
            .remove("simulation");
        assert!(Snapshot::decode(v).is_err());
        let mut v = example();
        let mut duplicate = v["options"][0].clone();
        duplicate["id"] = "different-id-same-pair".into();
        v["options"].as_array_mut().unwrap().push(duplicate);
        assert!(Snapshot::decode(v).is_err());
        let mut v = example();
        v["current"]["mode_id"] = "unknown".into();
        assert!(Snapshot::decode(v).is_err());
    }
    #[test]
    fn original_ticket_and_readback_require_pair_new_epoch_and_exact_transition() {
        let s = example_snapshot();
        let c = challenge(&s);
        assert!(c.matches(&c.binding));
        assert!(c.confirms("SIMULATE"));
        assert!(!c.confirms("simulate"));
        assert!(c.fresh(100., 15100.));
        assert!(!c.fresh(100., 15100.01));
        let request = Confirmation {
            binding: c.binding.clone(),
            ticket: c.ticket.clone(),
            transition_id: c.transition_id.clone(),
            confirmation: "SIMULATE".into(),
        };
        let mut readback = s.clone();
        readback.current = Some(Current {
            id: c.target.id.clone(),
            mode_id: c.target.mode_id.clone(),
            label: c.target.label.clone(),
            simulation: true,
            output_domain: c.target.output_domain.clone(),
        });
        readback.transition = Some(Transition {
            id: c.transition_id.clone(),
            target_id: c.target.id.clone(),
            state: "applied".into(),
            phase: "Applied".into(),
            error: None,
        });
        assert!(!applied_by(&request, &c.target, &readback, 0., 1.));
        readback.policy_epoch = Some("owner-A:epoch-2".into());
        assert!(applied_by(&request, &c.target, &readback, 0., 1.));
        readback.current.as_mut().unwrap().simulation = false;
        assert!(!applied_by(&request, &c.target, &readback, 0., 1.));
        readback.current.as_mut().unwrap().simulation = true;
        readback.transition.as_mut().unwrap().id = "another-request".into();
        assert!(!applied_by(&request, &c.target, &readback, 0., 1.));
        readback.transition.as_mut().unwrap().id = c.transition_id;
        readback.backend_instance = "restarted-owner".into();
        assert!(!applied_by(&request, &c.target, &readback, 0., 1.));
    }
    #[test]
    fn supported_unknown_epoch_is_a_valid_but_unusable_observation() {
        let mut raw = example();
        raw["policy_epoch"] = serde_json::Value::Null;
        let s = Snapshot::decode(raw).unwrap();
        assert!(s.epoch_fencing_supported);
        assert!(s.binding("third-sim").is_none());
        assert!(s.reason("third-sim", 0., 1.).is_some());
    }
    #[test]
    fn ordinary_closure_does_not_invent_or_deny_mode_repair_availability() {
        let mut s = example_snapshot();
        s.admissions_open = false;
        assert!(s.reason("third-sim", 0., 1.).is_none());
        // The backend may explicitly offer the applied pair for failed-closed
        // reconciliation. A fresh epoch and matching transition still prove it.
        s.current = Some(Current {
            id: "third-sim".into(),
            mode_id: "custom α".into(),
            label: "Third team · Simulation".into(),
            simulation: true,
            output_domain: "Isolated simulated actions and telemetry".into(),
        });
        assert!(s.reason("third-sim", 0., 1.).is_none());
        s.admissions_open = true;
        assert!(s.reason("third-sim", 0., 1.).is_some());
        s.admissions_open = false;
        s.options[0].available = false;
        assert!(s.reason("third-sim", 0., 1.).is_some());
        s.options[0].available = true;
        s.permissions.can_change = false;
        assert!(s.reason("third-sim", 0., 1.).is_some());
        s.permissions.can_change = true;
        s.transition = Some(Transition {
            id: "other-pending".into(),
            target_id: "third-sim".into(),
            state: "pending".into(),
            phase: "Closing".into(),
            error: None,
        });
        assert!(s.reason("third-sim", 0., 1.).is_some());
    }
    #[test]
    fn restart_options_need_no_epoch_fencing_but_in_place_options_still_do() {
        let mut raw = example();
        raw["epoch_fencing_supported"] = false.into();
        raw["options"][0]["transition_kind"] = "restart".into();
        let s = Snapshot::decode(raw.clone()).unwrap();
        assert!(s.option("third-sim").unwrap().restarts());
        assert!(s.reason("third-sim", 0., 1.).is_none());
        let mut closed = s.clone();
        closed.options[0].available = false;
        closed.options[0].reason = Some("Station is not idle: the T clock is running".into());
        assert_eq!(closed.reason("third-sim", 0., 1.).as_deref(), Some("Station is not idle: the T clock is running"));
        let mut unknown = s.clone();
        unknown.policy_epoch = None;
        assert!(unknown.reason("third-sim", 0., 1.).is_some());
        raw["options"][0]["transition_kind"] = "in_place".into();
        let in_place = Snapshot::decode(raw.clone()).unwrap();
        assert!(in_place.reason("third-sim", 0., 1.).is_some());
        raw["options"][0]["transition_kind"] = "reboot".into();
        assert!(Snapshot::decode(raw).is_err());
    }
    #[test]
    fn in_process_flag_is_optional_and_keeps_server_fenced_gating() {
        // Older backends never send the flag: they restart into another build.
        let old = example_snapshot();
        assert!(!old.options[0].in_process);
        assert!(!old.options[0].switches_in_process());
        let mut raw = example();
        raw["epoch_fencing_supported"] = false.into();
        raw["options"][0]["transition_kind"] = "restart".into();
        let restart_only = Snapshot::decode(raw.clone()).unwrap();
        assert!(restart_only.options[0].restarts());
        assert!(!restart_only.options[0].switches_in_process());
        raw["options"][0]["in_process"] = true.into();
        raw["policy_epoch"] = "boot-1a2b:epoch-3".into();
        let s = Snapshot::decode(raw.clone()).unwrap();
        let option = s.option("third-sim").unwrap();
        assert!(option.in_process && option.restarts() && option.switches_in_process());
        assert!(s.reason("third-sim", 0., 1.).is_none());
        let mut busy = s.clone();
        busy.options[0].available = false;
        busy.options[0].reason = Some("Station is not idle: a sequence is running".into());
        assert_eq!(
            busy.reason("third-sim", 0., 1.).as_deref(),
            Some("Station is not idle: a sequence is running")
        );
        // The flag never relaxes the original-epoch requirement of in_place.
        raw["options"][0]["transition_kind"] = "in_place".into();
        let in_place = Snapshot::decode(raw.clone()).unwrap();
        assert!(in_place.options[0].in_process);
        assert!(!in_place.options[0].switches_in_process());
        assert!(in_place.reason("third-sim", 0., 1.).is_some());
        raw["options"][0]["in_process"] = "yes".into();
        assert!(Snapshot::decode(raw).is_err());
    }
    #[test]
    fn origin_capability_is_optional_for_older_v2_but_required_flag_never_invents_form_token() {
        let old=Snapshot::decode(example()).unwrap();
        assert!(!old.control_origin_required);assert!(old.control_origin.is_none());
        let mut value=example();value["control_origin_required"]=true.into();
        assert!(Snapshot::decode(value.clone()).is_err(),"required capability must carry explicit nullable origin");
        value["control_origin"]=serde_json::Value::Null;
        let unknown=Snapshot::decode(value.clone()).unwrap();assert!(unknown.control_origin_required);assert!(unknown.control_origin.is_none());
        value["control_origin"]="opaque-form-origin".into();assert_eq!(Snapshot::decode(value.clone()).unwrap().control_origin.as_deref(),Some("opaque-form-origin"));
        value["control_origin"]="invalid\nheader-origin".into();assert!(Snapshot::decode(value).is_err());
    }

}
