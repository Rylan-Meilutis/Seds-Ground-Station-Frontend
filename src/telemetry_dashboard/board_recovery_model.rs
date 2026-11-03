//! API v1 recovery DTOs. Missing capabilities/identity always fail closed.
/// Presentation only. Server capability and fresh proof still decide admission.
pub(super) fn targeted_recovery_label(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "AB" | "VB" => Some(("Recover to Idle…", "Verified safe-output application recovery; no physical-position proof.")),
        "FC" => Some(("Recover to Startup…", "Verified FC evaluator and safe-output reset. Deployment history is preserved; a separate new flight is required.")),
        "RF" | "PB" | "GB" | "DAQ" => Some(("Reset metadata to Startup…", "Cached application metadata only; no physical recovery, power cycling, acquisition reset or resync.")),
        _ => None,
    }
}

use serde::{Deserialize, Serialize};

pub(super) const BOARD_IDS: [&str; 7] = ["RF", "FC", "PB", "AB", "VB", "GB", "DAQ"];
#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct Principal {
    pub username: Option<String>,
    pub context: String,
}
#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct Permissions {
    pub query: bool,
    pub recover: bool,
    pub reset_startup: bool,
    pub change_mode: bool,
}
#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct ModeCapability {
    pub current: String,
    pub build_mode: String,
    pub supported_modes: Vec<String>,
    pub runtime_switch_supported: bool,
    pub requires_restart: Vec<String>,
    pub message: String,
}
#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct Network {
    pub healthy: bool,
    pub age_ms: u64,
    pub disconnects: u64,
    pub reconnects: u64,
}
#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct Capabilities {
    pub query: Option<bool>,
    pub recover: Option<bool>,
    pub resync: Option<bool>,
    pub reset_startup: Option<bool>,
    pub endpoint_verified: bool,
    pub hardened_transport: Option<bool>,
}
#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct Operation {
    pub id: String,
    pub action: String,
    pub phase: String,
    pub targets: Vec<String>,
    pub completed: bool,
    pub success: Option<bool>,
    pub age_ms: u64,
    pub message: String,
    pub reset_applied_state: Option<u8>,
}
impl Operation {
    pub fn pending(&self) -> bool {
        !self.completed
            && !matches!(
                self.phase.as_str(),
                "idle" | "completed" | "failed" | "cancelled"
            )
    }
    pub fn label(&self) -> &'static str {
        match (self.completed, self.success, self.phase.as_str()) {
            (_, _, "cancelled") => "Cancelled",
            (_, Some(false), _) | (_, _, "failed") => "Failed",
            (true, Some(true), _)
                if self.action == "reset_startup" && self.reset_applied_state != Some(0) =>
            {
                "Reset result is not confirmed"
            }
            (true, Some(true), _) => "Confirmed by board readbacks",
            _ => "Pending / not confirmed",
        }
    }
}
#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct Board {
    pub id: String,
    pub label: String,
    pub network: Network,
    pub state: Option<u8>,
    pub state_label: String,
    pub boot: Option<String>,
    pub epoch: Option<u32>,
    pub nonce: Option<String>,
    pub age_ms: Option<u64>,
    pub abort_latched: Option<bool>,
    pub safe_outputs: Option<bool>,
    pub commanded_outputs: Option<u32>,
    pub output_readback: String,
    pub physical_position_verified: bool,
    pub capabilities: Capabilities,
    pub operation: Option<Operation>,
    pub message: Option<String>,
    pub result: Option<u8>,
    pub commissioning_required: bool,
    pub identity_kind: String,
    pub first_uncertainty_reconcilable: bool,
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value.bytes().any(|byte| byte != b'0')
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
impl Board {
    pub fn first_uncertainty_context(&self) -> bool {
        self.first_uncertainty_reconcilable
            && self.identity_kind == "retained_first_uncertainty"
            && self.boot.as_deref() == Some("0000000000000000")
            && self.result == Some(7)
            && self.commissioning_required
            && self.capabilities.hardened_transport == Some(true)
            && self.state.is_none_or(|state| state <= 15)
    }
    pub fn commissioning_context(&self) -> bool {
        self.identity_kind == "commissioning_sentinel"
            && self.boot.as_deref() == Some("ffffffffffffffff")
            && self.result == Some(7)
            && self.commissioning_required
            && self.capabilities.hardened_transport == Some(true)
            && self.state.is_none_or(|state| state <= 15)
    }
    pub fn known_context(&self) -> bool {
        (self.commissioning_context()
            || self.first_uncertainty_context()
            || (self.boot.as_deref() != Some("ffffffffffffffff")
                && self.state.is_some_and(|state| state <= 15)))
            && self
                .epoch
                .is_some_and(|epoch| epoch != 0 && epoch < u32::MAX - 1)
            && (self.first_uncertainty_context()
                || self.boot.as_deref().is_some_and(|x| hex(x, 16)))
            && self.nonce.as_deref().is_some_and(|x| hex(x, 16))
            && self.capabilities.endpoint_verified
    }
    pub fn fresh(&self, elapsed: u64) -> bool {
        self.known_context()
            && self
                .age_ms
                .is_some_and(|age| age.saturating_add(elapsed) <= 2_000)
    }
    pub fn query_allowed(&self) -> bool {
        self.capabilities.endpoint_verified && self.capabilities.query == Some(true)
    }
}
/// Server-owned confirmation identity is separate from expiring board samples.
/// The server refreshes and checks actual board identities at confirmation.
#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct ConfirmationIdentity {
    pub ticket: String,
    pub backend_instance: String,
    pub principal_context: String,
    pub reset_epoch: String,
    pub action: String,
    pub targets: Vec<String>,
    pub expires_in_ms: u64,
}
#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct HostRecovery {
    pub pending: bool,
    pub verified: bool,
    pub publication_acknowledged: bool,
    pub targets: Vec<String>,
}
#[derive(Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct Status {
    pub version: u8,
    pub backend_instance: String,
    pub principal: Principal,
    pub reset_epoch: String,
    pub flight_state: Option<u8>,
    pub automatic_actions_inhibited: bool,
    pub permissions: Permissions,
    pub mode_capability: ModeCapability,
    pub boards: Vec<Board>,
    pub operation: Option<Operation>,
    pub confirmation: Option<ConfirmationIdentity>,
    pub host_recovery: Option<HostRecovery>,
}
impl Status {
    pub fn valid(&self) -> bool {
        self.version == 1
            && hex(&self.backend_instance, 32)
            && hex(&self.principal.context, 32)
            && hex(&self.reset_epoch, 16)
    }
    pub fn board(&self, id: &str) -> Option<&Board> {
        self.boards.iter().find(|b| b.id == id)
    }
    pub fn pending(&self) -> bool {
        self.operation.as_ref().is_some_and(Operation::pending)
            || self
                .boards
                .iter()
                .any(|b| b.operation.as_ref().is_some_and(Operation::pending))
    }
    pub fn reset_reason(&self, elapsed: u64) -> Option<String> {
        if !self.valid() || !self.permissions.reset_startup {
            return Some("Reset ALL is not permitted by this server/session.".into());
        }
        if !matches!(self.flight_state, Some(0 | 1 | 15)) {
            return Some("Stop the active operation before resetting the boards.".into());
        }
        let missing: Vec<_> = BOARD_IDS
            .iter()
            .filter(|id| {
                !self.board(id).is_some_and(|b| {
                    b.fresh(elapsed)
                        && b.query_allowed()
                        && b.capabilities.reset_startup == Some(true)
                })
            })
            .copied()
            .collect();
        if !missing.is_empty() {
            return Some(format!(
                "Reset ALL requires fresh supported readbacks from: {}.",
                missing.join(", ")
            ));
        }
        if self.pending() {
            return Some("Another operation is pending; wait for its actual result.".into());
        }
        None
    }
    pub fn board_reason(&self, board: &Board, action: &str, elapsed: u64) -> Option<String> {
        if !self.valid() || !self.permissions.recover {
            return Some("Recovery is not permitted by this server/session.".into());
        }
        if !matches!(self.flight_state, Some(0 | 1 | 15)) {
            return Some("Recovery requires GS Startup, Idle or Aborted.".into());
        }
        if !board.fresh(elapsed) {
            return Some("Fresh board boot/epoch/nonce readback is required.".into());
        }
        let capability = match action {
            "recover_abort" => board.capabilities.recover,
            "resync" => board.capabilities.resync,
            _ => None,
        };
        if capability != Some(true) {
            return Some("Unsupported / not advertised by this board.".into());
        }
        if self.pending() {
            return Some("Another operation is pending.".into());
        }
        if self.host_recovery.as_ref().is_some_and(|recovery| {
            !recovery.publication_acknowledged && recovery.targets.iter().any(|id| id != &board.id)
        }) {
            return Some("Finish recovery of all previously selected boards first.".into());
        }
        None
    }
    pub fn mode_reason(&self, mode: &str) -> Option<String> {
        if !self.valid() || !self.permissions.change_mode {
            return Some("Mode changes are not permitted by this server/session.".into());
        }
        if !self.mode_capability.runtime_switch_supported
            || !self
                .mode_capability
                .supported_modes
                .iter()
                .any(|x| x == mode)
        {
            return Some(
                "This server cannot switch to that mode at runtime. Use a supported build/restart."
                    .into(),
            );
        }
        if self.mode_capability.current == mode {
            return Some("This mode is already active.".into());
        }
        if self.pending() {
            return Some("Another operation is pending.".into());
        }
        None
    }
}
#[derive(Clone, Deserialize)]
pub(super) struct Challenge {
    pub ticket: String,
    pub backend_instance: String,
    pub principal_context: String,
    pub reset_epoch: String,
    pub expires_in_ms: u64,
    pub action: String,
    pub targets: Vec<String>,
    pub summary: String,
    pub confirmation: String,
}
impl Challenge {
    fn identity_matches(&self, status: &Status) -> bool {
        status.confirmation.as_ref().is_some_and(|identity| {
            identity.ticket == self.ticket
                && identity.backend_instance == self.backend_instance
                && identity.principal_context == self.principal_context
                && identity.reset_epoch == self.reset_epoch
                && identity.action == self.action
                && identity.targets == self.targets
                && identity.expires_in_ms > 0
                && identity.expires_in_ms <= self.expires_in_ms
        })
    }
    pub fn matches(
        &self,
        status: &Status,
        action: &str,
        board: Option<&str>,
        mode: Option<&str>,
    ) -> bool {
        if !status.valid()
            || !hex(&self.ticket, 32)
            || self.backend_instance != status.backend_instance
            || self.principal_context != status.principal.context
            || self.reset_epoch != status.reset_epoch
            || self.action != action
            || self.expires_in_ms == 0
            || self.expires_in_ms > 30_000
            || !self.identity_matches(status)
        {
            return false;
        }
        let phrase = match (action, board, mode) {
            ("recover_abort", Some(id), None)
                if targeted_recovery_label(id).is_some() && self.targets == [id] =>
            {
                format!("RECOVER {id}")
            }
            ("resync", Some(id), None) if self.targets == [id] => format!("RESYNC {id}"),
            ("reset_startup", None, None)
                if self.targets.len() == 7
                    && BOARD_IDS.iter().all(|id| {
                        self.targets.iter().filter(|x| x.as_str() == *id).count() == 1
                    }) =>
            {
                format!("RESET STARTUP {}", BOARD_IDS.join(" "))
            }
            ("change_mode", None, Some(mode))
                if status
                    .mode_capability
                    .supported_modes
                    .iter()
                    .any(|x| x == mode) =>
            {
                format!("CHANGE MODE {}", mode.to_ascii_uppercase())
            }
            _ => return false,
        };
        // Keep compatibility with an explicitly scoped older reset phrase;
        // never construct or substitute the phrase submitted to the server.
        (self.confirmation == phrase
            || self.confirmation == format!("{phrase} COMMISSION")
            || (action == "reset_startup" && self.confirmation == "RESET ALL STARTUP"))
            && self
                .targets
                .iter()
                .all(|id| BOARD_IDS.contains(&id.as_str()) && status.board(id).is_some())
    }
    pub fn context_unchanged(&self, original: &Status, current: &Status) -> bool {
        original.valid()
            && current.valid()
            && self.backend_instance == current.backend_instance
            && self.principal_context == current.principal.context
            && self.reset_epoch == current.reset_epoch
            && !current
                .operation
                .as_ref()
                .is_some_and(|op| op.pending() && op.phase != "awaiting_confirmation")
            && original.permissions == current.permissions
            && original.mode_capability == current.mode_capability
            && self.identity_matches(original)
            && self.identity_matches(current)
            && original.mode_capability.current == current.mode_capability.current
    }
}
#[derive(Serialize)]
pub(super) struct Prepare<'a> {
    pub action: &'a str,
    pub board: Option<&'a str>,
    pub mode: Option<&'a str>,
    pub backend_instance: &'a str,
    pub reset_epoch: &'a str,
}
#[derive(Serialize)]
pub(super) struct Confirm<'a> {
    pub ticket: &'a str,
    pub backend_instance: &'a str,
    pub principal_context: &'a str,
    pub reset_epoch: &'a str,
    pub confirmation: &'a str,
}
#[derive(Deserialize)]
pub(super) struct Accepted {
    pub accepted: bool,
    pub operation: Option<Operation>,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn status() -> Status {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/board-recovery/status-example.json"
        ))
        .unwrap()
    }
    fn challenge() -> Challenge {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/board-recovery/prepare-example.json"
        ))
        .unwrap()
    }
    #[test]
    fn targeted_recovery_labels_and_confirmation_remain_exact_board_scopes() {
        for id in BOARD_IDS {
            let (label, description) = targeted_recovery_label(id).unwrap();
            match id {
                "AB" | "VB" => assert_eq!(label, "Recover to Idle…"),
                "FC" => {assert_eq!(label, "Recover to Startup…"); assert!(description.contains("history is preserved"));},
                _ => {assert_eq!(label, "Reset metadata to Startup…"); assert!(description.contains("no physical recovery"));},
            }
            let mut status = all_boards();
            let mut ticket = challenge();
            ticket.action = "recover_abort".into();
            ticket.targets = vec![id.into()];
            ticket.confirmation = format!("RECOVER {id}");
            let identity = status.confirmation.as_mut().unwrap();
            identity.action = ticket.action.clone();
            identity.targets = ticket.targets.clone();
            assert!(ticket.matches(&status, "recover_abort", Some(id), None));
            assert!(!ticket.matches(&status, "recover_abort", Some("GS"), None));
            ticket.confirmation.push_str(" COMMISSION");
            assert!(ticket.matches(&status, "recover_abort", Some(id), None));
            ticket.targets.push("AB".into());
            assert!(!ticket.matches(&status, "recover_abort", Some(id), None));
        }
        for id in ["GS", "GW", "DAQB", "", "fc", "unknown"] {
            assert!(targeted_recovery_label(id).is_none());
        }
    }

    #[test]
    fn actual_v1_unknown_boards_do_not_inherit_global_startup_or_capabilities() {
        let s = status();
        assert!(s.valid());
        assert_eq!(s.boards.len(), 7);
        assert_eq!(s.flight_state, Some(0));
        let rf = s.board("RF").unwrap();
        assert!(rf.state.is_none());
        assert!(!rf.known_context());
        assert!(!rf.query_allowed());
        assert!(s.board("AB").unwrap().query_allowed());
        assert!(s.board("AB").unwrap().fresh(1950));
        assert!(!s.board("AB").unwrap().fresh(1951));
        assert!(s.reset_reason(0).unwrap().contains("RF"));
        assert!(!serde_json::from_str::<Status>("{}").unwrap().valid());
    }
    #[test]
    fn unpublished_or_invalid_board_identity_never_authorizes_recovery() {
        let original = status().board("AB").unwrap().clone();
        assert!(original.known_context());
        for invalid in [
            Board {
                boot: Some("0".repeat(16)),
                ..original.clone()
            },
            Board {
                nonce: Some("0".repeat(16)),
                ..original.clone()
            },
            Board {
                epoch: Some(0),
                ..original.clone()
            },
            Board {
                state: Some(255),
                ..original.clone()
            },
        ] {
            assert!(!invalid.known_context());
            assert!(!invalid.fresh(0));
            assert!(
                status()
                    .board_reason(&invalid, "recover_abort", 0)
                    .is_some()
            );
        }
        let mut invalid = status();
        invalid.reset_epoch = "0".repeat(16);
        assert!(!invalid.valid());
        invalid = status();
        invalid.backend_instance = "0".repeat(32);
        assert!(!invalid.valid());
    }

    #[test]
    fn query_requires_dedicated_endpoint_and_explicit_advertisement() {
        let mut b = status().board("AB").unwrap().clone();
        assert!(b.query_allowed());
        b.capabilities.endpoint_verified = false;
        assert!(!b.query_allowed());
        b.capabilities.endpoint_verified = true;
        b.capabilities.query = None;
        assert!(!b.query_allowed());
        b.capabilities.query = Some(false);
        assert!(!b.query_allowed());
    }
    #[test]
    fn mode_capabilities_never_relabel_compiled_normal_as_hitl() {
        let mut s = status();
        assert!(s.mode_reason("hitl").is_some());
        assert!(s.mode_reason("normal").is_some());
        s.mode_capability.supported_modes.push("testing".into());
        assert!(s.mode_reason("testing").is_some());
        s.mode_capability.runtime_switch_supported = true;
        assert!(s.mode_reason("testing").is_none());
        s.permissions.change_mode = false;
        assert!(s.mode_reason("testing").is_some());
    }
    fn all_boards() -> Status {
        let mut s = status();
        let ab = s.board("AB").unwrap().clone();
        s.boards = BOARD_IDS
            .into_iter()
            .map(|id| {
                let mut b = ab.clone();
                b.id = id.into();
                b
            })
            .collect();
        s.confirmation = Some(identity(&challenge()));
        s
    }
    fn identity(c: &Challenge) -> ConfirmationIdentity {
        ConfirmationIdentity {
            ticket: c.ticket.clone(),
            backend_instance: c.backend_instance.clone(),
            principal_context: c.principal_context.clone(),
            reset_epoch: c.reset_epoch.clone(),
            action: c.action.clone(),
            targets: c.targets.clone(),
            expires_in_ms: c.expires_in_ms,
        }
    }
    #[test]
    fn reset_all_requires_every_board_actual_context_not_partial_ack() {
        let mut s = all_boards();
        assert!(s.reset_reason(0).is_none());
        s.boards[0].nonce = None;
        assert!(s.reset_reason(0).is_some());
        s = all_boards();
        s.boards[3].age_ms = Some(2001);
        assert!(s.reset_reason(0).is_some());
        s = all_boards();
        s.boards[1].capabilities.endpoint_verified = false;
        assert!(s.reset_reason(0).is_some());
        s = all_boards();
        s.boards[1].capabilities.reset_startup = None;
        assert!(s.boards[1].query_allowed());
        assert!(s.reset_reason(0).is_some());
        s.boards[1].capabilities.reset_startup = Some(false);
        assert!(s.reset_reason(0).is_some());
    }
    #[test]
    fn confirmation_rejects_server_ticket_identity_auth_and_generation_changes() {
        let original = all_boards();
        let c = challenge();
        assert!(c.matches(&original, "reset_startup", None, None));
        assert!(c.context_unchanged(&original, &original));
        let mut changed = original.clone();
        changed.backend_instance = "ffffffffffffffffffffffffffffffff".into();
        assert!(!c.context_unchanged(&original, &changed));
        changed = original.clone();
        changed.principal.context = "ffffffffffffffffffffffffffffffff".into();
        assert!(!c.context_unchanged(&original, &changed));
        changed = original.clone();
        changed.reset_epoch = "0000000000000002".into();
        assert!(!c.context_unchanged(&original, &changed));
        for field in 0..6 {
            changed = original.clone();
            let ticket = changed.confirmation.as_mut().unwrap();
            match field {
                0 => ticket.ticket = "f".repeat(32),
                1 => ticket.action = "recover_abort".into(),
                2 => ticket.targets = vec!["AB".into()],
                3 => ticket.expires_in_ms = 0,
                4 => ticket.expires_in_ms = 30_001,
                _ => {
                    changed.confirmation = None;
                }
            }
            assert!(!c.context_unchanged(&original, &changed), "field {field}");
        }
        changed = original.clone();
        changed.permissions.reset_startup = false;
        assert!(!c.context_unchanged(&original, &changed));
    }
    #[test]
    fn thirty_second_confirmation_does_not_claim_two_second_readback_stays_fresh() {
        let original = all_boards();
        let c = challenge();
        let mut later = original.clone();
        for board in &mut later.boards {
            board.age_ms = Some(15_000);
            board.state = None;
            board.abort_latched = None;
            board.safe_outputs = None;
            board.commanded_outputs = None;
            board.result = None;
            board.capabilities.recover = Some(false);
            board.capabilities.reset_startup = Some(false);
        }
        assert!(!later.board("AB").unwrap().fresh(0));
        assert!(c.context_unchanged(&original, &later));
        // Server must issue a new query/revalidate after Confirm; UI ticket
        // identity validity does not certify this stale output readback.
        assert!(super::super::board_recovery_guard::challenge_unexpired(
            100.0, 30_000, 30_099.0
        ));
        assert!(!super::super::board_recovery_guard::challenge_unexpired(
            100.0, 30_000, 30_100.0
        ));
    }
    #[test]
    fn all_declared_modes_require_actual_runtime_capability_and_frozen_phrase() {
        for mode in ["normal", "hitl", "test_fire", "testing"] {
            let mut s = all_boards();
            s.mode_capability.current = if mode == "normal" {
                "testing"
            } else {
                "normal"
            }
            .into();
            s.mode_capability.supported_modes = ["normal", "hitl", "test_fire", "testing"]
                .map(str::to_owned)
                .into();
            assert!(s.mode_reason(mode).is_some());
            s.mode_capability.runtime_switch_supported = true;
            assert!(s.mode_reason(mode).is_none());
            let mut c = challenge();
            c.action = "change_mode".into();
            c.confirmation = format!("CHANGE MODE {}", mode.to_ascii_uppercase());
            s.confirmation = Some(identity(&c));
            assert!(c.matches(&s, "change_mode", None, Some(mode)));
            c.confirmation = "CHANGE MODE UNSUPPORTED".into();
            assert!(!c.matches(&s, "change_mode", None, Some(mode)));
        }
    }
    #[test]
    fn server_challenge_cannot_silently_retarget_or_change_action() {
        let s = all_boards();
        let mut c = challenge();
        c.targets.pop();
        assert!(!c.matches(&s, "reset_startup", None, None));
        c = challenge();
        c.action = "recover_abort".into();
        assert!(!c.matches(&s, "reset_startup", None, None));
        c = challenge();
        c.confirmation = "ABORT".into();
        assert!(!c.matches(&s, "reset_startup", None, None));
        c = challenge();
        c.expires_in_ms = 30_001;
        assert!(!c.matches(&s, "reset_startup", None, None));
    }
    #[test]
    fn confirm_body_contains_only_frozen_ticket_context_and_phrase() {
        let c = challenge();
        let body = Confirm {
            ticket: &c.ticket,
            backend_instance: &c.backend_instance,
            principal_context: &c.principal_context,
            reset_epoch: &c.reset_epoch,
            confirmation: &c.confirmation,
        };
        let json = serde_json::to_value(body).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 5);
        assert!(json.get("board").is_none());
        assert!(json.get("mode").is_none());
        assert!(json.get("action").is_none());
    }
    #[test]
    fn acceptance_and_pending_never_claim_confirmed_safe_outputs() {
        let mut op: Operation = serde_json::from_str(include_str!(
            "../../tests/fixtures/board-recovery/operation-example.json"
        ))
        .unwrap();
        assert!(op.pending());
        assert_eq!(op.label(), "Pending / not confirmed");
        op.completed = true;
        assert_eq!(op.label(), "Pending / not confirmed");
        op.success = Some(true);
        assert_eq!(op.label(), "Reset result is not confirmed");
        op.reset_applied_state = Some(0);
        assert_eq!(op.label(), "Confirmed by board readbacks");
        op.phase = "cancelled".into();
        assert_eq!(op.label(), "Cancelled");
    }
    #[test]
    fn actual_server_readback_expiry_must_not_cancel_live_confirmation_identity() {
        let original = all_boards();
        let c = challenge();
        let mut later = original.clone();
        for board in &mut later.boards {
            board.age_ms = Some(15_000);
            board.state = None;
            board.abort_latched = None;
            board.safe_outputs = None;
            board.commanded_outputs = None;
            board.capabilities.recover = Some(false);
            board.capabilities.reset_startup = Some(false);
        }
        assert!(!later.board("AB").unwrap().fresh(0));
        assert!(
            c.context_unchanged(&original, &later),
            "Transient actual readback expiry is not a change to the server-owned 30s ticket"
        );
    }
    #[test]
    fn commissioning_sentinel_is_explicit_and_never_an_ordinary_boot_proof() {
        let mut b = status().board("AB").unwrap().clone();
        b.boot = Some("ffffffffffffffff".into());
        assert!(!b.known_context());
        b.identity_kind = "commissioning_sentinel".into();
        b.result = Some(7);
        b.commissioning_required = true;
        b.capabilities.hardened_transport = Some(true);
        b.state = None;
        assert!(b.commissioning_context());
        assert!(b.known_context());
        assert!(b.fresh(0));
        for field in 0..6 {
            let mut bad = b.clone();
            match field {
                0 => bad.result = Some(0),
                1 => bad.commissioning_required = false,
                2 => bad.capabilities.hardened_transport = None,
                3 => bad.boot = Some("0000000000000000".into()),
                4 => bad.epoch = Some(u32::MAX),
                _ => bad.identity_kind = "unknown".into(),
            }
            assert!(!bad.known_context(), "field {field}");
        }
    }

    #[test]
    fn zero_generation_needs_explicit_retained_first_binding_and_never_becomes_normal() {
        let mut b = status().board("AB").unwrap().clone();
        b.boot = Some("0000000000000000".into());
        b.result = Some(7);
        b.commissioning_required = true;
        b.capabilities.hardened_transport = Some(true);
        b.identity_kind = "retained_first_uncertainty".into();
        b.state = None;
        assert!(!b.known_context());
        b.first_uncertainty_reconcilable = true;
        assert!(b.known_context());
        assert!(b.first_uncertainty_context());
        assert!(!b.commissioning_context());
        assert!(b.fresh(0));
        assert!(!b.fresh(2_001));
        for field in 0..5 {
            let mut bad = b.clone();
            match field {
                0 => bad.result = Some(0),
                1 => bad.identity_kind = "unknown".into(),
                2 => bad.commissioning_required = false,
                3 => bad.capabilities.hardened_transport = None,
                _ => bad.nonce = None,
            }
            assert!(!bad.known_context(), "field {field}");
        }
    }

    #[test]
    fn aborted_station_can_review_reset_but_partial_recovery_cannot_drop_old_targets() {
        let mut s = all_boards();
        s.flight_state = Some(15);
        assert!(s.reset_reason(0).is_none());
        assert!(
            s.board_reason(s.board("AB").unwrap(), "recover_abort", 0)
                .is_none()
        );
        s.host_recovery = Some(HostRecovery {
            pending: true,
            verified: false,
            publication_acknowledged: false,
            targets: vec!["AB".into(), "VB".into()],
        });
        assert!(
            s.board_reason(s.board("AB").unwrap(), "recover_abort", 0)
                .is_some()
        );
        assert!(
            s.reset_reason(0).is_none(),
            "Explicit ALL includes all retained targets"
        );
        s.flight_state = Some(8);
        assert!(
            s.reset_reason(0).is_some(),
            "Active operation is not a reset context"
        );
    }

    #[test]
    fn server_commission_phrase_is_preserved_and_bound_to_exact_target_identity() {
        let mut s = all_boards();
        let mut c = challenge();
        c.confirmation = format!("RESET STARTUP {} COMMISSION", BOARD_IDS.join(" "));
        assert!(c.matches(&s, "reset_startup", None, None));
        let phrase = c.confirmation.clone();
        s.boards[0].state = None;
        assert!(c.matches(&s, "reset_startup", None, None));
        assert_eq!(
            c.confirmation, phrase,
            "Never substitute a new confirmation string"
        );
        s.confirmation.as_mut().unwrap().targets.pop();
        assert!(!c.matches(&s, "reset_startup", None, None));
    }

    #[test]
    fn all_compiled_modes_can_reset_without_optimistic_mode_switch_or_clock_claim() {
        for mode in ["normal", "hitl", "test_fire", "testing"] {
            let mut s = all_boards();
            s.mode_capability.current = mode.into();
            s.mode_capability.build_mode = mode.into();
            s.mode_capability.supported_modes = vec![mode.into()];
            assert!(s.reset_reason(0).is_none());
            assert!(!s.mode_capability.runtime_switch_supported);
            assert!(s.mode_reason("hitl").is_some());
        }
    }
}
