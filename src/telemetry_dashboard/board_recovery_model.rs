//! API v1 recovery DTOs. Missing capabilities/identity always fail closed.
/// Presentation only. Server capability and fresh proof still decide admission.
pub(super) fn targeted_recovery_label(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "AB" | "VB" => Some((
            "Recover to Idle…",
            "Verified safe-output application recovery; no physical-position proof.",
        )),
        "FC" => Some((
            "Recover to Startup…",
            "Verified FC evaluator and safe-output reset. Deployment history is preserved; a separate new flight is required.",
        )),
        "RF" | "PB" | "GB" | "DAQ" => Some((
            "Reset metadata to Startup…",
            "Cached application metadata only; no physical recovery, power cycling, acquisition reset or resync.",
        )),
        _ => None,
    }
}

use serde::{Deserialize, Serialize};

pub(super) const BOARD_IDS: [&str; 7] = ["RF", "FC", "PB", "AB", "VB", "GB", "DAQ"];
fn canonical_targets(targets: &[String]) -> bool {
    targets.len() <= BOARD_IDS.len()
        && BOARD_IDS
            .iter()
            .copied()
            .filter(|id| targets.iter().any(|target| target == id))
            .eq(targets.iter().map(String::as_str))
}
#[derive(Clone, Default, Deserialize, PartialEq)]
pub(super) struct ResetStartupPolicy {
    pub version: u8,
    pub global_startup_best_effort: bool,
    pub mode: String,
    pub base_required_boards: Vec<String>,
    pub optional_proofs: bool,
}
impl ResetStartupPolicy {
    fn valid_for(&self, mode: &ModeCapability) -> bool {
        // A reset covers whichever boards are available, so none are required.
        if !matches!(self.mode.as_str(), "normal" | "testing" | "test_fire" | "hitl") {
            return false;
        }
        self.version == 1
            && self.global_startup_best_effort
            && self.optional_proofs
            && self.mode == mode.current
            && self.mode == mode.build_mode
            && mode.supported_modes.contains(&self.mode)
            && self.base_required_boards.is_empty()
    }
}
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
    /// Opens preparation/query only; actual recovery still requires fresh board proof.
    pub prepare_recover: Option<bool>,
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
    pub global_startup_best_effort: Option<bool>,
    pub selected_proof_count: Option<usize>,
    pub unselected_board_verification: Option<String>,
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
                if matches!(self.action.as_str(), "reset_startup" | "reset_startup_all7")
                    && self.reset_applied_state != Some(0) =>
            {
                "Reset result is not confirmed"
            }
            (true, Some(true), _)
                if matches!(self.action.as_str(), "reset_startup" | "reset_startup_all7") =>
            {
                if self.global_startup_best_effort == Some(true) {
                    if self.selected_proof_count != Some(self.targets.len())
                        || !canonical_targets(&self.targets)
                        || self.unselected_board_verification.as_deref()
                            != Some("unverified_best_effort")
                    {
                        "Reset result is not confirmed"
                    } else if self.targets.is_empty() {
                        "Host Startup reset; no board proof"
                    } else {
                        "Selected boards verified; global broadcast best effort"
                    }
                } else if self.global_startup_best_effort.is_none()
                    && self.selected_proof_count.is_none()
                    && self.unselected_board_verification.is_none()
                    && self.targets.len() == 7
                    && canonical_targets(&self.targets)
                {
                    "Confirmed by board readbacks"
                } else {
                    "Reset result is not confirmed"
                }
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
    /// Backend presentation authority; distinct from the mutation proof lease.
    pub display_connected: Option<bool>,
    pub display_known: Option<bool>,
    pub state: Option<u8>,
    pub state_label: String,
    pub boot: Option<String>,
    pub epoch: Option<u32>,
    pub nonce: Option<String>,
    pub age_ms: Option<u64>,
    /// Remaining server-issued readback lease at response time.
    pub readback_valid_for_ms: Option<u64>,
    #[serde(default)]
    pub recovery_required: bool,
    #[serde(default)]
    pub recovery_reason: Option<String>,
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
    /// Server's plain-language reason Resync is unavailable for this board.
    pub resync_blocked_reason: Option<String>,
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value.bytes().any(|byte| byte != b'0')
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
impl Board {
    pub fn recovery_notice(&self) -> Option<&'static str> {
        if !self.recovery_required { return None; }
        Some(match self.recovery_reason.as_deref() {
            Some("abort_latched") => "This board reports Abort. Review recovery before starting another launch.",
            Some("unfinished_recovery") => "The previous recovery for this board is incomplete. Review its result and retry recovery.",
            _ => "GroundStation reports that this board requires recovery.",
        })
    }
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
                .readback_valid_for_ms
                .is_some_and(|remaining| elapsed < remaining)
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
    pub reset_startup_policy: Option<ResetStartupPolicy>,
    pub boards: Vec<Board>,
    pub operation: Option<Operation>,
    pub confirmation: Option<ConfirmationIdentity>,
    pub host_recovery: Option<HostRecovery>,
    /// Server's plain-language reason Reset ALL is not permitted right now.
    #[serde(default)]
    pub reset_startup_blocked_reason: Option<String>,
    /// Station phase a Resync would set (fill phases and Armed only).
    #[serde(default)]
    pub resync_target_state: Option<u8>,
    #[serde(default)]
    pub resync_target_label: Option<String>,
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
    /// Only an advertised mutation owns recovery admission. Background status
    /// queries may run while preparation asks the backend for fresh proof.
    pub fn mutating_pending(&self) -> bool {
        let mutation = |operation: &Operation| operation.action != "query" && operation.pending();
        self.operation.as_ref().is_some_and(mutation)
            || self.boards.iter().any(|board| board.operation.as_ref().is_some_and(mutation))
    }
    /// GS is the server's global state, not a recoverable MCU identity.
    pub fn global_state_label(&self) -> &'static str {
        const LABELS: [&str; 16] = [
            "Startup", "Idle", "Pre Fill", "Fill Test", "Nitrogen Fill", "Nitrous Fill",
            "Armed", "Launch", "Ascent", "Coast", "Apogee", "Descent", "Reefing", "Landed",
            "Recovery", "Aborted",
        ];
        self.flight_state
            .and_then(|value| LABELS.get(value as usize).copied())
            .unwrap_or("Unknown")
    }
    pub fn reset_reason(&self, elapsed: u64) -> Option<String> {
        if !self.valid() {
            return Some("Reset ALL is not permitted by this server/session.".into());
        }
        if !self.permissions.reset_startup {
            return Some(match self.reset_startup_blocked_reason.as_deref() {
                Some(reason) if !reason.trim().is_empty() => format!("Reset all boards is blocked: {reason}"),
                _ => "Reset ALL is not permitted by this server/session.".into(),
            });
        }
        if self.mutating_pending() {
            return Some("Another operation is pending; wait for its actual result.".into());
        }
        if let Some(policy) = self.reset_startup_policy.as_ref() {
            if !policy.valid_for(&self.mode_capability) {
                return Some(
                    "This server advertised an unsupported or inconsistent reset policy.".into(),
                );
            }
            // Capability permits preparation/query only. The returned server
            // ticket must still bind exact selected proof targets and policy.
            return None;
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
        if self.mutating_pending() {
            return Some("Another operation is pending; wait for its actual result.".into());
        }
        None
    }
    /// Preparation can refresh expired board proof. This never grants confirmation.
    pub fn prepare_board_reason(
        &self,
        board: &Board,
        action: &str,
        elapsed: u64,
    ) -> Option<String> {
        if action == "resync" {
            return self.resync_reason(board);
        }
        if action != "recover_abort" {
            return self.board_reason(board, action, elapsed);
        }
        if !self.valid() || !self.permissions.recover {
            return Some("Recovery is not permitted by this server/session.".into());
        }
        if !matches!(self.flight_state, Some(0 | 1 | 15)) {
            return Some("Recovery requires GS Startup, Idle or Aborted.".into());
        }
        if board.capabilities.prepare_recover != Some(true) || !board.query_allowed() {
            return Some("Recovery preparation is not advertised by this server/board.".into());
        }
        if self.mutating_pending() {
            return Some("Another operation is pending.".into());
        }
        if self.host_recovery.as_ref().is_some_and(|recovery| {
            !recovery.publication_acknowledged && recovery.targets.iter().any(|id| id != &board.id)
        }) {
            return Some("Finish recovery of all previously selected boards first.".into());
        }
        None
    }
    /// Resync has its own window (fill phases and Armed), so it does not use the
    /// Startup/Idle/Aborted recovery permission. PREPARE takes fresh proof.
    pub fn resync_reason(&self, board: &Board) -> Option<String> {
        if !self.valid() {
            return Some("Resync is not permitted by this server/session.".into());
        }
        if board.capabilities.resync != Some(true) {
            return Some("This board's firmware does not support Resync.".into());
        }
        if let Some(reason) = board.resync_blocked_reason.as_deref().filter(|r| !r.trim().is_empty()) {
            return Some(reason.to_string());
        }
        if self.resync_target_state.is_none_or(|state| !(2..=6).contains(&state)) {
            return Some("Resync is allowed only in the fill phases and Armed.".into());
        }
        if !board.query_allowed() {
            return Some("The board's recovery endpoint is not verified.".into());
        }
        if self.mutating_pending() {
            return Some("Another operation is pending.".into());
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
        if self.mutating_pending() {
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
        if self.mutating_pending() {
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
    #[serde(default)]
    pub global_startup_best_effort: Option<bool>,
    #[serde(default)]
    pub required_proof_boards: Option<Vec<String>>,
    #[serde(default)]
    pub unselected_board_verification: Option<String>,
    #[serde(default)]
    pub resync_target_state: Option<u8>,
    #[serde(default)]
    pub resync_target_label: Option<String>,
    #[serde(default)]
    pub outputs_unchanged: Option<bool>,
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
            || (action == "reset_startup" && !status.permissions.reset_startup)
        {
            return false;
        }
        if action == "reset_startup" && board.is_none() && mode.is_none() {
            if let Some(policy) = status.reset_startup_policy.as_ref() {
                if !policy.valid_for(&status.mode_capability)
                    || self.global_startup_best_effort != Some(true)
                    || self.required_proof_boards.as_ref() != Some(&policy.base_required_boards)
                    || self.unselected_board_verification.as_deref()
                        != Some("unverified_best_effort")
                    || !canonical_targets(&self.targets)
                    || !policy
                        .base_required_boards
                        .iter()
                        .all(|id| self.targets.contains(id))
                    || !self.targets.iter().all(|id| status.board(id).is_some())
                {
                    return false;
                }
                let scope = if self.targets.is_empty() {
                    "NONE".to_string()
                } else {
                    self.targets.join(" ")
                };
                let phrase = format!("RESET ALL STARTUP; VERIFY {scope}");
                return self.confirmation == phrase
                    || (!self.targets.is_empty()
                        && self.confirmation == format!("{phrase} COMMISSION"));
            }
            // An old server has no scoped-reset contract. Do not interpret
            // partial/new metadata or a narrowed phrase as legacy authority.
            if self.global_startup_best_effort.is_some()
                || self.required_proof_boards.is_some()
                || self.unselected_board_verification.is_some()
            {
                return false;
            }
        }
        let phrase = match (action, board, mode) {
            ("recover_abort", Some(id), None)
                if targeted_recovery_label(id).is_some() && self.targets == [id] =>
            {
                format!("RECOVER {id}")
            }
            // The phrase names the board and the phase frozen at PREPARE; the
            // ticket must still match the station's current phase and promise
            // that no outputs move.
            ("resync", Some(id), None)
                if self.targets == [id]
                    && self.outputs_unchanged == Some(true)
                    && self.resync_target_state.is_some()
                    && self.resync_target_state == status.resync_target_state =>
            {
                match self.resync_target_label.as_deref() {
                    Some(label) if !label.is_empty() => {
                        return self.confirmation == format!("RESYNC {id} TO {}", label.to_ascii_uppercase())
                            && status.board(id).is_some();
                    }
                    _ => return false,
                }
            }
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
                .is_some_and(|op| op.action != "query" && op.pending() && op.phase != "awaiting_confirmation")
            && original.permissions == current.permissions
            && original.flight_state == current.flight_state
            && original.mode_capability == current.mode_capability
            && original.reset_startup_policy == current.reset_startup_policy
            && (self.action != "reset_startup" || current.permissions.reset_startup)
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
                "FC" => {
                    assert_eq!(label, "Recover to Startup…");
                    assert!(description.contains("history is preserved"));
                }
                _ => {
                    assert_eq!(label, "Reset metadata to Startup…");
                    assert!(description.contains("no physical recovery"));
                }
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

    /// Resync follows the server's own window and reason, not the Startup/Idle/
    /// Aborted recovery permission, and accepts only a phrase naming the board
    /// and the frozen phase with the no-output promise.
    #[test]
    fn resync_uses_server_reason_and_phase_bound_phrase() {
        let mut s = all_boards();
        s.flight_state = Some(5);
        s.permissions.recover = false;
        s.resync_target_state = Some(5);
        s.resync_target_label = Some("NitrousFill".into());
        let ab = s.board("AB").unwrap().clone();
        assert_eq!(s.prepare_board_reason(&ab, "resync", 0), None);
        let mut blocked = ab.clone();
        blocked.resync_blocked_reason = Some("A tank overpressure is latched; use Review recovery or Reset all boards.".into());
        assert_eq!(s.prepare_board_reason(&blocked, "resync", 0).as_deref(),
            Some("A tank overpressure is latched; use Review recovery or Reset all boards."));
        let mut unsupported = ab.clone();
        unsupported.capabilities.resync = Some(false);
        assert!(s.prepare_board_reason(&unsupported, "resync", 0).unwrap().contains("does not support Resync"));
        let mut idle = s.clone();
        idle.resync_target_state = None;
        assert!(idle.prepare_board_reason(&ab, "resync", 0).unwrap().contains("fill phases and Armed"));

        let mut ticket = challenge();
        ticket.action = "resync".into();
        ticket.targets = vec!["AB".into()];
        ticket.confirmation = "RESYNC AB TO NITROUSFILL".into();
        ticket.resync_target_state = Some(5);
        ticket.resync_target_label = Some("NitrousFill".into());
        ticket.outputs_unchanged = Some(true);
        let identity = s.confirmation.as_mut().unwrap();
        identity.action = ticket.action.clone();
        identity.targets = ticket.targets.clone();
        assert!(ticket.matches(&s, "resync", Some("AB"), None));
        let mut other = ticket.clone();
        other.confirmation = "RESYNC AB".into();
        assert!(!other.matches(&s, "resync", Some("AB"), None), "phrase must name the phase");
        let mut other = ticket.clone();
        other.outputs_unchanged = None;
        assert!(!other.matches(&s, "resync", Some("AB"), None), "no-output promise required");
        let mut moved = s.clone();
        moved.resync_target_state = Some(6);
        assert!(!ticket.matches(&moved, "resync", Some("AB"), None), "station left the reviewed phase");
        assert!(!ticket.matches(&s, "resync", Some("VB"), None));
    }

    #[test]
    fn preparation_capability_refreshes_stale_proof_without_authorizing_recovery() {
        let mut s = status();
        let board = s.boards.iter_mut().find(|b| b.id == "AB").unwrap();
        board.capabilities.prepare_recover = Some(true);
        board.capabilities.recover = Some(false);
        board.age_ms = Some(20_000);
        board.readback_valid_for_ms = Some(0);
        let board = s.board("AB").unwrap();
        assert!(!board.fresh(60_000));
        assert!(
            s.prepare_board_reason(board, "recover_abort", 60_000)
                .is_none()
        );
        assert!(s.board_reason(board, "recover_abort", 0).is_some());
        assert!(s.prepare_board_reason(board, "resync", 0).is_some());
        assert!(
            s.confirmation.is_none(),
            "Preparation capability is not a challenge/ticket"
        );
    }

    #[test]
    fn missing_or_denied_preparation_capability_stays_closed_even_with_fresh_recovery() {
        for capability in [None, Some(false)] {
            let mut s = status();
            let board = s.boards.iter_mut().find(|b| b.id == "AB").unwrap();
            board.capabilities.prepare_recover = capability;
            assert!(board.fresh(0));
            assert_eq!(board.capabilities.recover, Some(true));
            assert!(
                s.prepare_board_reason(s.board("AB").unwrap(), "recover_abort", 0)
                    .is_some()
            );
        }
    }

    #[test]
    fn preparation_capability_retains_session_permission_safe_state_and_query_guards() {
        for rejected in 0..7 {
            let mut s = status();
            s.boards
                .iter_mut()
                .find(|b| b.id == "AB")
                .unwrap()
                .capabilities
                .prepare_recover = Some(true);
            match rejected {
                0 => s.permissions.recover = false,
                1 => s.flight_state = Some(8),
                2 => s.backend_instance = "0".repeat(32),
                3 => {
                    s.boards
                        .iter_mut()
                        .find(|b| b.id == "AB")
                        .unwrap()
                        .capabilities
                        .endpoint_verified = false
                }
                4 => {
                    s.boards
                        .iter_mut()
                        .find(|b| b.id == "AB")
                        .unwrap()
                        .capabilities
                        .query = Some(false)
                }
                5 => {
                    s.host_recovery = Some(HostRecovery {
                        pending: true,
                        verified: false,
                        publication_acknowledged: false,
                        targets: vec!["AB".into(), "VB".into()],
                    })
                }
                _ => {
                    s.operation = Some(Operation {
                        action: "recover_abort".into(),
                        phase: "querying".into(),
                        ..Default::default()
                    })
                }
            }
            assert!(
                s.prepare_board_reason(s.board("AB").unwrap(), "recover_abort", 0)
                    .is_some(),
                "guard {rejected}"
            );
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
        assert!(s.board("AB").unwrap().fresh(4949));
        assert!(!s.board("AB").unwrap().fresh(4950));
        assert!(s.reset_reason(0).unwrap().contains("RF"));
        assert!(!serde_json::from_str::<Status>("{}").unwrap().valid());
    }
    #[test]
    fn board_readback_uses_only_the_server_issued_remaining_lease() {
        let mut board = status().board("AB").unwrap().clone();
        board.readback_valid_for_ms = Some(123);
        assert!(board.fresh(122));
        assert!(!board.fresh(123));
        // The server owns TTL and transport-age accounting; the browser must
        // neither impose its own fixed window nor re-add diagnostic age.
        board.age_ms = Some(90_000);
        board.readback_valid_for_ms = Some(12_345);
        assert!(board.fresh(12_344));
        assert!(!board.fresh(12_345));
        board.readback_valid_for_ms = Some(0);
        assert!(!board.fresh(0));
        board.readback_valid_for_ms = None;
        assert!(!board.fresh(0));
        board.readback_valid_for_ms = Some(u64::MAX);
        assert!(!board.fresh(u64::MAX));
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
        s.boards[3].age_ms = Some(5001);
        s.boards[3].readback_valid_for_ms = Some(0);
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
    fn thirty_second_confirmation_does_not_claim_expired_readback_stays_fresh() {
        let original = all_boards();
        let c = challenge();
        let mut later = original.clone();
        for board in &mut later.boards {
            board.age_ms = Some(15_000);
            board.readback_valid_for_ms = Some(0);
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
            board.readback_valid_for_ms = Some(0);
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
        assert!(b.fresh(4_949));
        assert!(!b.fresh(4_950));
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
        s.permissions.reset_startup = false;
        assert!(
            s.reset_reason(0).is_some(),
            "Server must withhold reset permission for an active operation"
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
    fn scoped_reset(mode: &str) -> (Status, Challenge) {
        let mut status = all_boards();
        status.mode_capability.current = mode.into();
        status.mode_capability.build_mode = mode.into();
        status.mode_capability.supported_modes = vec![mode.into()];
        let required: Vec<String> = match mode {
            "normal" | "testing" => BOARD_IDS.iter().map(|x| (*x).into()).collect(),
            "test_fire" => ["AB", "VB", "GB", "DAQ"]
                .iter()
                .map(|x| (*x).into())
                .collect(),
            "hitl" => vec![],
            _ => panic!("test mode"),
        };
        status.reset_startup_policy = Some(
            serde_json::from_value(serde_json::json!({
                "version":1,"global_startup_best_effort":true,"mode":mode,
                "base_required_boards":[],"optional_proofs":true
            }))
            .unwrap(),
        );
        let mut ticket = challenge();
        ticket.targets = required.clone();
        ticket.global_startup_best_effort = Some(true);
        ticket.required_proof_boards = Some(Vec::new());
        ticket.unselected_board_verification = Some("unverified_best_effort".into());
        ticket.confirmation = format!(
            "RESET ALL STARTUP; VERIFY {}",
            if ticket.targets.is_empty() {
                "NONE".into()
            } else {
                ticket.targets.join(" ")
            }
        );
        status.confirmation.as_mut().unwrap().targets = ticket.targets.clone();
        (status, ticket)
    }
    #[test]
    fn recovery_notice_requires_explicit_server_metadata_not_age_state_or_health() {
        let mut board = status().board("VB").unwrap().clone();
        board.state = Some(15);
        board.abort_latched = Some(true);
        board.age_ms = Some(u64::MAX);
        board.network.healthy = false;
        assert!(board.recovery_notice().is_none());
        board.recovery_required = true;
        board.recovery_reason = Some("abort_latched".into());
        assert!(board.recovery_notice().unwrap().contains("reports Abort"));
        board.recovery_reason = Some("unfinished_recovery".into());
        assert!(board.recovery_notice().unwrap().contains("incomplete"));
        board.recovery_required = false;
        assert!(board.recovery_notice().is_none());
    }
    #[test]
    fn global_station_state_does_not_require_a_board_identity_or_other_board_agreement() {
        let mut current = status();
        current.boards.clear();
        current.flight_state = Some(7);
        assert_eq!(current.global_state_label(), "Launch");
        current.flight_state = Some(1);
        assert_eq!(current.global_state_label(), "Idle");
        current.flight_state = Some(255);
        assert_eq!(current.global_state_label(), "Unknown");
    }
    #[test]
    fn server_granted_launch_reset_keeps_review_and_confirmation_available() {
        let (mut current, ticket) = scoped_reset("hitl");
        current.flight_state = Some(7);
        assert!(current.reset_reason(0).is_none());
        assert!(ticket.matches(&current, "reset_startup", None, None));
        assert!(ticket.context_unchanged(&current, &current));
        // A grant to reset host metadata is not a grant for targeted recovery.
        assert!(current
            .prepare_board_reason(current.board("VB").unwrap(), "recover_abort", 0)
            .is_some());
        let original = current.clone();
        current.permissions.reset_startup = false;
        assert!(current.reset_reason(0).is_some());
        assert!(!ticket.matches(&current, "reset_startup", None, None));
        assert!(!ticket.context_unchanged(&original, &current));
    }
    #[test]
    fn background_queries_do_not_lock_preparation_but_real_mutations_do() {
        let (mut status, ticket) = scoped_reset("hitl");
        let original = status.clone();
        status.boards[0].operation = Some(Operation { action:"query".into(),phase:"querying".into(),..Default::default() });
        status.operation = status.boards[0].operation.clone();
        assert!(!status.mutating_pending());
        assert!(status.reset_reason(20_000).is_none());
        let board = status.boards.iter_mut().find(|board|board.id=="AB").unwrap();
        board.capabilities.prepare_recover=Some(true);
        assert!(status.prepare_board_reason(status.board("AB").unwrap(),"recover_abort",0).is_none());
        assert!(ticket.context_unchanged(&original,&status));
        status.operation.as_mut().unwrap().action="recover_abort".into();
        assert!(status.mutating_pending());
        assert!(status.reset_reason(0).is_some());
        assert!(status.prepare_board_reason(status.board("AB").unwrap(),"recover_abort",0).is_some());
        assert!(!ticket.context_unchanged(&original,&status));
        status.operation=None;
        status.boards[0].operation.as_mut().unwrap().action="reset_startup".into();
        assert!(status.mutating_pending());
    }
    #[test]
    fn explicit_mode_policy_allows_preparation_then_exact_scoped_confirmation() {
        for mode in ["normal", "test_fire", "hitl", "testing"] {
            let (mut status, ticket) = scoped_reset(mode);
            for board in &mut status.boards {
                board.age_ms = None;
                board.readback_valid_for_ms = None;
                board.capabilities.reset_startup = None;
            }
            assert!(
                status.reset_reason(0).is_none(),
                "prepare queries must be allowed under explicit {mode} capability"
            );
            assert!(status.reset_reason(20_000).is_none(),
                "server permission and scoped policy govern preparation, not a client age threshold");
            assert!(ticket.matches(&status, "reset_startup", None, None));
            assert!(ticket.context_unchanged(&status, &status));
        }
        let mut legacy = all_boards();
        legacy.boards[0].age_ms = None;
        legacy.boards[0].readback_valid_for_ms = None;
        assert!(
            legacy.reset_reason(0).is_some(),
            "legacy all-seven freshness stays required"
        );
    }
    #[test]
    fn scoped_optional_abort_proofs_are_exact_unique_canonical_supersets() {
        let (mut status, mut ticket) = scoped_reset("test_fire");
        ticket.targets = vec![
            "FC".into(),
            "AB".into(),
            "VB".into(),
            "GB".into(),
            "DAQ".into(),
        ];
        status.confirmation.as_mut().unwrap().targets = ticket.targets.clone();
        ticket.confirmation = format!("RESET ALL STARTUP; VERIFY {}", ticket.targets.join(" "));
        assert!(ticket.matches(&status, "reset_startup", None, None));
        for targets in [
            vec!["AB", "VB", "GB", "DAQ", "DAQ"],
            vec!["VB", "AB", "GB", "DAQ"],
            vec!["AB", "VB", "GB", "UNKNOWN"],
        ] {
            ticket.targets = targets.into_iter().map(str::to_string).collect();
            status.confirmation.as_mut().unwrap().targets = ticket.targets.clone();
            ticket.confirmation = format!("RESET ALL STARTUP; VERIFY {}", ticket.targets.join(" "));
            assert!(!ticket.matches(&status, "reset_startup", None, None));
        }
    }
    #[test]
    fn incomplete_or_changed_policy_never_falls_back_to_legacy() {
        for change in 0..9 {
            let (mut status, mut ticket) = scoped_reset("normal");
            match change {
                0 => status.reset_startup_policy.as_mut().unwrap().version = 2,
                1 => {
                    status
                        .reset_startup_policy
                        .as_mut()
                        .unwrap()
                        .global_startup_best_effort = false
                }
                2 => {
                    status
                        .reset_startup_policy
                        .as_mut()
                        .unwrap()
                        .optional_proofs = false
                }
                3 => status.reset_startup_policy.as_mut().unwrap().mode = "unknown".into(),
                4 => {
                    status
                        .reset_startup_policy
                        .as_mut()
                        .unwrap()
                        .base_required_boards
                        .push("AB".into());
                }
                5 => status.mode_capability.build_mode = "hitl".into(),
                6 => ticket.global_startup_best_effort = None,
                7 => ticket.required_proof_boards = None,
                _ => ticket.unselected_board_verification = Some("verified".into()),
            }
            assert!(
                !ticket.matches(&status, "reset_startup", None, None),
                "mutation {change}"
            );
        }
        let (status, mut ticket) = scoped_reset("normal");
        for phrase in [
            "RESET ALL STARTUP",
            "RESET STARTUP RF FC PB AB VB GB DAQ",
            "RESET ALL STARTUP; VERIFY NONE",
            "RESET ALL STARTUP; VERIFY RF FC PB AB VB GB DAQ EXTRA",
        ] {
            ticket.confirmation = phrase.into();
            assert!(!ticket.matches(&status, "reset_startup", None, None));
        }
    }
    #[test]
    fn empty_scope_is_only_explicit_hitl_none_and_no_phantom_c11_space() {
        let (status, mut ticket) = scoped_reset("hitl");
        assert!(ticket.matches(&status, "reset_startup", None, None));
        for phrase in [
            "RESET ALL STARTUP; VERIFY ",
            "RESET ALL STARTUP; VERIFY",
            "RESET ALL STARTUP; VERIFY NONE COMMISSION",
            "anything",
        ] {
            ticket.confirmation = phrase.into();
            assert!(!ticket.matches(&status, "reset_startup", None, None));
        }
        let (mut status, mut ticket) = scoped_reset("hitl");
        status.reset_startup_policy = None;
        assert!(!ticket.matches(&status, "reset_startup", None, None));
        ticket.global_startup_best_effort = None;
        ticket.required_proof_boards = None;
        ticket.unselected_board_verification = None;
        ticket.confirmation = "RESET ALL STARTUP".into();
        assert!(!ticket.matches(&status, "reset_startup", None, None));
    }
    #[test]
    fn scoped_policy_remains_bound_to_current_identity_auth_mode_and_safe_state() {
        let (original, ticket) = scoped_reset("test_fire");
        for change in 0..7 {
            let mut current = original.clone();
            match change {
                0 => current.backend_instance = "ffffffffffffffffffffffffffffffff".into(),
                1 => current.principal.context = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
                2 => current.reset_epoch = "0000000000000002".into(),
                3 => current.permissions.reset_startup = false,
                4 => current.mode_capability.current = "normal".into(),
                5 => {
                    current
                        .reset_startup_policy
                        .as_mut()
                        .unwrap()
                        .optional_proofs = false
                }
                _ => {
                    current.flight_state = Some(8);
                    current.permissions.reset_startup = false;
                }
            }
            assert!(!ticket.matches(&current, "reset_startup", None, None));
            assert!(!ticket.context_unchanged(&original, &current));
        }
    }
    #[test]
    fn host_only_and_selected_results_never_claim_unselected_board_readbacks() {
        let mut operation: Operation = serde_json::from_value(serde_json::json!({
            "action":"reset_startup","completed":true,"success":true,"phase":"completed",
            "targets":[],"reset_applied_state":0,"global_startup_best_effort":true,
            "selected_proof_count":0,"unselected_board_verification":"unverified_best_effort"
        }))
        .unwrap();
        assert_eq!(operation.label(), "Host Startup reset; no board proof");
        operation.targets = vec!["AB".into(), "VB".into(), "GB".into(), "DAQ".into()];
        operation.selected_proof_count = Some(4);
        assert_eq!(
            operation.label(),
            "Selected boards verified; global broadcast best effort"
        );
        operation.selected_proof_count = Some(0);
        assert_eq!(operation.label(), "Reset result is not confirmed");
        operation.targets.clear();
        operation.global_startup_best_effort = None;
        operation.selected_proof_count = None;
        operation.unselected_board_verification = None;
        assert_eq!(operation.label(), "Reset result is not confirmed");
    }
    #[test]
    fn present_hitl_capability_requires_every_field_without_guessing_empty_base() {
        let complete = serde_json::json!({"version":1,"global_startup_best_effort":true,"mode":"hitl","base_required_boards":[],"optional_proofs":true});
        for field in [
            "base_required_boards",
            "version",
            "global_startup_best_effort",
            "mode",
            "optional_proofs",
        ] {
            let mut partial = complete.clone();
            partial.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<ResetStartupPolicy>(partial).is_err(),
                "present capability must explicitly include {field}"
            );
        }
    }
}
