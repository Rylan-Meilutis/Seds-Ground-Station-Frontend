//! API v1 recovery DTOs. Missing capabilities/identity always fail closed.
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
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value.bytes().any(|byte| byte != b'0')
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
impl Board {
    pub fn known_context(&self) -> bool {
        self.state.is_some_and(|state| state <= 15)
            && self.epoch.is_some_and(|epoch| epoch != 0)
            && self.boot.as_deref().is_some_and(|x| hex(x, 16))
            && self.nonce.as_deref().is_some_and(|x| hex(x, 16))
            && self.capabilities.endpoint_verified
    }
    pub fn fresh(&self, elapsed: u64) -> bool {
        self.known_context()
            && self
                .age_ms
                .is_some_and(|age| age.saturating_add(elapsed) <= 2_000)
    }
    pub fn same_context(&self, other: &Self) -> bool {
        self.known_context()
            && other.known_context()
            && self.id == other.id
            && self.boot == other.boot
            && self.epoch == other.epoch
            && self.nonce == other.nonce
            && self.state == other.state
            && self.abort_latched == other.abort_latched
            && self.safe_outputs == other.safe_outputs
            && self.commanded_outputs == other.commanded_outputs
            && self.capabilities == other.capabilities
    }
    pub fn query_allowed(&self) -> bool {
        self.capabilities.endpoint_verified && self.capabilities.query == Some(true)
    }
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
        if !matches!(self.flight_state, Some(0 | 1)) {
            return Some("Recovery/resync requires GS Startup or Idle.".into());
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
        {
            return false;
        }
        let phrase = match (action, board, mode) {
            ("recover_abort", Some(id), None) if self.targets == [id] => format!("RECOVER {id}"),
            ("resync", Some(id), None) if self.targets == [id] => format!("RESYNC {id}"),
            ("reset_startup", None, None)
                if self.targets.len() == 7
                    && BOARD_IDS.iter().all(|id| {
                        self.targets.iter().filter(|x| x.as_str() == *id).count() == 1
                    }) =>
            {
                "RESET ALL STARTUP".into()
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
        self.confirmation == phrase
            && self
                .targets
                .iter()
                .all(|id| status.board(id).is_some_and(Board::known_context))
    }
    pub fn context_unchanged(&self, original: &Status, current: &Status) -> bool {
        current.valid()
            && self.backend_instance == current.backend_instance
            && self.principal_context == current.principal.context
            && self.reset_epoch == current.reset_epoch
            && !current
                .operation
                .as_ref()
                .is_some_and(|op| op.pending() && op.phase != "awaiting_confirmation")
            && original.permissions == current.permissions
            && original.mode_capability == current.mode_capability
            && self.targets.iter().all(|id| {
                original
                    .board(id)
                    .zip(current.board(id))
                    .is_some_and(|(a, b)| a.same_context(b))
            })
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
            Board { boot: Some("0".repeat(16)), ..original.clone() },
            Board { nonce: Some("0".repeat(16)), ..original.clone() },
            Board { epoch: Some(0), ..original.clone() },
            Board { state: Some(255), ..original.clone() },
        ] {
            assert!(!invalid.known_context());
            assert!(!invalid.fresh(0));
            assert!(status().board_reason(&invalid, "recover_abort", 0).is_some());
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
        s
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
    fn confirmation_rejects_identity_reset_nonce_state_and_capability_changes() {
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
        for field in 0..7 {
            changed = original.clone();
            let b = &mut changed.boards[0];
            match field {
                0 => b.boot = Some("000000000000000a".into()),
                1 => b.epoch = Some(11),
                2 => b.nonce = Some("0000000000000012".into()),
                3 => b.state = Some(4),
                4 => b.safe_outputs = Some(false),
                5 => b.commanded_outputs = Some(7),
                _ => b.boot = None,
            };
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
        assert_eq!(op.label(), "Confirmed by board readbacks");
        op.phase = "cancelled".into();
        assert_eq!(op.label(), "Cancelled");
    }
}
