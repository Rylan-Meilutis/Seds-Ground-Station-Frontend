//! Presentation only. Cached readbacks never supply command capabilities or tickets.
use super::{
    board_recovery_guard::{ConnectionBinding, status_age_ms},
    board_recovery_model::{BOARD_IDS, Board, Status},
};
use std::collections::BTreeMap;
#[derive(Clone, PartialEq, Eq)]
struct Owner {
    binding: ConnectionBinding,
    backend: String,
    reset: String,
    principal: String,
}
#[derive(Clone)]
struct Retained {
    board: Board,
    requested: f64,
}
#[derive(Clone, Default)]
pub(super) struct DisplayCache {
    owner: Option<Owner>,
    boards: BTreeMap<String, Retained>,
}
#[derive(Clone, Default)]
pub(super) struct ReadbackDisplay {
    pub state: String,
    pub boot: String,
    pub epoch: String,
    pub nonce: String,
    pub age_ms: Option<u64>,
    pub lease_ms: Option<u64>,
    pub abort_latched: Option<bool>,
    pub safe_outputs: Option<bool>,
    pub outputs: String,
    pub output_source: String,
    pub commissioning: bool,
    pub physical_verified: bool,
    pub message: Option<String>,
    pub retained: bool,
    pub fresh: bool,
}
fn owner(status: &Status, binding: &ConnectionBinding) -> Owner {
    Owner {
        binding: binding.clone(),
        backend: status.backend_instance.clone(),
        reset: status.reset_epoch.clone(),
        principal: status.principal.context.clone(),
    }
}
fn pending(board: &Board) -> bool {
    board.result == Some(1)
        || board
            .operation
            .as_ref()
            .is_some_and(|operation| operation.phase == "querying")
}
fn display_authorized(board: &Board, elapsed: u64) -> bool {
    match (board.display_connected, board.display_known) {
        (Some(connected), Some(known)) => connected && known,
        (None, None) => board.network.healthy && board.result == Some(0) && board.fresh(elapsed),
        _ => false,
    }
}
fn view(board: &Board, requested: f64, now: f64, retained: bool) -> ReadbackDisplay {
    let elapsed = status_age_ms(requested, now);
    ReadbackDisplay {
        state: board
            .state
            .as_ref()
            .map(|_| board.state_label.clone())
            .unwrap_or_else(|| "Unknown".into()),
        boot: if board.first_uncertainty_context() {
            "Identity verification pending".into()
        } else if board.commissioning_context() {
            "Not yet commissioned".into()
        } else {
            board.boot.clone().unwrap_or_else(|| "Unknown".into())
        },
        epoch: board
            .epoch
            .map(|value| value.to_string())
            .unwrap_or_else(|| "Unknown".into()),
        nonce: board.nonce.clone().unwrap_or_else(|| "Unknown".into()),
        age_ms: board.age_ms.map(|age| age.saturating_add(elapsed)),
        lease_ms: board
            .readback_valid_for_ms
            .map(|lease| lease.saturating_sub(elapsed)),
        abort_latched: board.abort_latched,
        safe_outputs: board.safe_outputs,
        outputs: board
            .commanded_outputs
            .map(|value| format!("0x{value:08x}"))
            .unwrap_or_else(|| "Unknown".into()),
        output_source: board.output_readback.clone(),
        commissioning: board.commissioning_required,
        physical_verified: board.physical_position_verified,
        message: board.message.clone(),
        retained,
        fresh: board.fresh(elapsed),
    }
}
impl DisplayCache {
    pub fn observe(
        &mut self,
        status: &Status,
        binding: &ConnectionBinding,
        requested: f64,
        now: f64,
    ) {
        let next = owner(status, binding);
        if self.owner.as_ref() != Some(&next) {
            self.boards.clear();
            self.owner = Some(next);
        }
        for id in BOARD_IDS {
            let Some(board) = status.board(id) else {
                self.boards.remove(id);
                continue;
            };
            if self.boards.get(id).is_some_and(|old| {
                board
                    .boot
                    .as_ref()
                    .is_some_and(|boot| Some(boot) != old.board.boot.as_ref())
                    || board
                        .epoch
                        .is_some_and(|epoch| Some(epoch) != old.board.epoch)
            }) {
                self.boards.remove(id);
            }
            if display_authorized(board, status_age_ms(requested, now)) && board.state.is_some() {
                self.boards.insert(
                    id.into(),
                    Retained {
                        board: board.clone(),
                        requested,
                    },
                );
            } else if board.display_connected == Some(false)
                || board.display_known == Some(false)
                || board.state.is_none() && !pending(board)
            {
                self.boards.remove(id);
            }
        }
    }
    pub fn display(
        &self,
        status: Option<(&Status, f64)>,
        binding: &ConnectionBinding,
        id: &str,
        now: f64,
    ) -> ReadbackDisplay {
        let fallback = || {
            self.owner
                .as_ref()
                .filter(|owner| owner.binding == *binding)
                .and_then(|_| self.boards.get(id))
                .filter(|old| display_authorized(&old.board, status_age_ms(old.requested, now)))
                .map(|old| view(&old.board, old.requested, now, true))
        };
        if let Some((status, requested)) = status {
            if self.owner.as_ref() != Some(&owner(status, binding)) {
                return ReadbackDisplay::default();
            }
            if let Some(board) = status.board(id) {
                if display_authorized(board, status_age_ms(requested, now)) && board.state.is_some()
                {
                    return view(board, requested, now, false);
                }
                if board.display_connected != Some(false)
                    && board.display_known != Some(false)
                    && board.network.healthy
                    && pending(board)
                {
                    return fallback().unwrap_or_default();
                }
            }
            return ReadbackDisplay::default();
        }
        fallback().unwrap_or_default()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> ConnectionBinding {
        ConnectionBinding {
            backend: "https://fixture.invalid".into(),
            token: Some("fake-display-token".into()),
            principal: Some("fixture".into()),
        }
    }
    fn status() -> Status {
        let mut status: Status = serde_json::from_str(include_str!(
            "../../tests/fixtures/board-recovery/status-example.json"
        ))
        .unwrap();
        for board in &mut status.boards {
            if board.state.is_some() {
                board.result = Some(0);
                board.display_connected = Some(true);
                board.display_known = Some(true);
            }
        }
        status
    }
    #[test]
    fn healthy_display_persists_beyond_mutation_proof_lease() {
        let mut cache = DisplayCache::default();
        let status = status();
        cache.observe(&status, &binding(), 100., 100.);
        assert_eq!(cache.display(None, &binding(), "AB", 200.).state, "Startup");
        assert!(cache.display(None, &binding(), "AB", 200.).retained);
        assert_eq!(
            cache.display(None, &binding(), "AB", 30000.).state,
            "Startup"
        );
        assert!(!cache.display(None, &binding(), "AB", 30000.).fresh);
    }
    #[test]
    fn pending_reply_cannot_extend_old_proof_or_grant_capabilities() {
        let mut cache = DisplayCache::default();
        let mut status = status();
        cache.observe(&status, &binding(), 100., 100.);
        let board = status.boards.iter_mut().find(|b| b.id == "AB").unwrap();
        board.state = None;
        board.result = Some(1);
        board.readback_valid_for_ms = Some(999999);
        cache.observe(&status, &binding(), 1000., 1000.);
        assert_eq!(
            cache
                .display(Some((&status, 1000.)), &binding(), "AB", 1000.)
                .state,
            "Startup"
        );
        assert_eq!(
            cache
                .display(Some((&status, 1000.)), &binding(), "AB", 30000.)
                .state,
            "Startup"
        );
        assert!(
            !cache
                .display(Some((&status, 1000.)), &binding(), "AB", 30000.)
                .fresh
        );
        assert!(status.board("AB").unwrap().state.is_none());
    }
    #[test]
    fn actual_unknown_and_identity_changes_discard_retained_display() {
        for field in ["unknown", "boot", "epoch", "backend", "reset", "principal"] {
            let mut cache = DisplayCache::default();
            let mut status = status();
            cache.observe(&status, &binding(), 100., 100.);
            let board = status.boards.iter_mut().find(|b| b.id == "AB").unwrap();
            board.state = None;
            board.result = Some(1);
            match field {
                "backend" => status.backend_instance = "new".into(),
                "reset" => status.reset_epoch = "new".into(),
                "principal" => status.principal.context = "new".into(),
                _ => {
                    let board = status.boards.iter_mut().find(|b| b.id == "AB").unwrap();
                    board.state = None;
                    board.result = Some(if field == "unknown" { 0 } else { 1 });
                    if field == "unknown" {
                        board.display_known = Some(false);
                    }
                    if field == "boot" {
                        board.boot = Some("000000000000000a".into());
                    }
                    if field == "epoch" {
                        board.epoch = Some(9);
                    }
                }
            }
            cache.observe(&status, &binding(), 200., 200.);
            assert!(
                cache
                    .display(Some((&status, 200.)), &binding(), "AB", 200.)
                    .state
                    .is_empty(),
                "{field}"
            );
        }
    }
    #[test]
    fn old_backend_without_display_authority_uses_only_original_fresh_proof() {
        let mut status = status();
        for board in &mut status.boards {
            board.display_connected = None;
            board.display_known = None;
        }
        let mut cache = DisplayCache::default();
        cache.observe(&status, &binding(), 100., 100.);
        assert_eq!(
            cache
                .display(Some((&status, 100.)), &binding(), "AB", 200.)
                .state,
            "Startup"
        );
        assert!(
            cache
                .display(None, &binding(), "AB", 30000.)
                .state
                .is_empty()
        );
    }
    #[test]
    fn source_or_auth_change_cannot_reuse_display() {
        let mut cache = DisplayCache::default();
        cache.observe(&status(), &binding(), 100., 100.);
        let mut other = binding();
        other.token = Some("other-fake-token".into());
        assert!(cache.display(None, &other, "AB", 200.).state.is_empty());
        other = binding();
        other.backend = "https://other.invalid".into();
        assert!(cache.display(None, &other, "AB", 200.).state.is_empty());
    }
    #[test]
    fn arbitrary_server_lease_is_preserved_without_client_ttl() {
        let mut cache = DisplayCache::default();
        let mut status = status();
        let b = status.boards.iter_mut().find(|b| b.id == "AB").unwrap();
        b.readback_valid_for_ms = Some(12345);
        cache.observe(&status, &binding(), 100., 100.);
        assert_eq!(
            cache.display(None, &binding(), "AB", 10000.).state,
            "Startup"
        );
        assert_eq!(
            cache.display(None, &binding(), "AB", 30000.).state,
            "Startup"
        );
        let board = status.boards.iter_mut().find(|b| b.id == "AB").unwrap();
        board.display_connected = Some(false);
        cache.observe(&status, &binding(), 30000., 30000.);
        assert!(
            cache
                .display(Some((&status, 30000.)), &binding(), "AB", 30000.)
                .state
                .is_empty()
        );
    }
}
