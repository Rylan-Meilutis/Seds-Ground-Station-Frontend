//! Presentation continuity, never command authority or per-row boot attestation.
use super::*;

#[derive(Clone)]
struct LocalOwner {
    identity: crate::auth::RequestIdentity,
    base: String,
    graph: u64,
    clear: u64,
    mount: u64,
    alive: Arc<AtomicBool>,
}
impl LocalOwner {
    fn capture() -> Option<Self> {
        Some(Self {
            identity: crate::auth::request_identity()?,
            base: UrlConfig::base_http(),
            graph: GRAPH_HISTORY_GENERATION.load(Ordering::Relaxed),
            clear: frontend_data_clear_epoch(),
            mount: dashboard_gen(),
            alive: dashboard_alive(),
        })
    }
    fn identity_current(&self) -> bool {
        self.alive.load(Ordering::Acquire)
            && Arc::ptr_eq(&self.alive, &dashboard_alive())
            && self.mount == dashboard_gen()
            && self.base == UrlConfig::base_http()
            && crate::auth::request_identity_is_current(&self.identity)
    }
    fn current(&self) -> bool {
        self.identity_current()
            && self.graph == GRAPH_HISTORY_GENERATION.load(Ordering::Relaxed)
            && self.clear == frontend_data_clear_epoch()
    }
}
#[derive(Default)]
struct Continuity {
    owner: Option<LocalOwner>,
    revision: u64,
    /// Validated observation only; no stored row acquires this identity.
    authority: Option<(String, String)>,
    last_observation: u64,
}
impl Continuity {
    fn advance(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }
    fn accept(&mut self, serial: u64, identity: (String, String)) -> Option<bool> {
        if serial <= self.last_observation || self.revision == u64::MAX {
            return None;
        }
        self.last_observation = serial;
        if self.authority.as_ref() == Some(&identity) {
            return None;
        }
        let replacement = self.authority.is_some();
        self.authority = Some(identity);
        self.advance();
        Some(replacement)
    }
}
static CONTINUITY: Lazy<Mutex<Continuity>> = Lazy::new(|| Mutex::new(Continuity::default()));
static NEXT_OBSERVATION: AtomicU64 = AtomicU64::new(1);

/// Call before queue drain: identity changes retire old queued data. Explicit
/// cal/clear/clock owners have already cleared or filtered their own queues.
pub(super) fn synchronize() {
    {
        let Ok(state) = CONTINUITY.lock() else { return };
        if state.owner.as_ref().is_some_and(LocalOwner::current) {
            return;
        }
    }
    let now = LocalOwner::capture();
    let Ok(mut state) = CONTINUITY.lock() else {
        return;
    };
    if state.owner.as_ref().is_some_and(LocalOwner::current) {
        return;
    }
    let had_owner = state.owner.is_some();
    let identity_changed = state
        .owner
        .as_ref()
        .is_some_and(|owner| !owner.identity_current());
    state.owner = now;
    state.authority = None;
    state.advance();
    drop(state);
    if had_owner {
        retire_display(identity_changed);
    }
}
fn retire_display(clear_queue: bool) {
    if clear_queue && let Ok(mut queue) = TELEMETRY_QUEUE.lock() {
        queue.clear();
    }
    if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
        store.dashboard_history.reset_display_epoch();
    }
    // This clears only telemetry/graphs/GPS, not queued alerts or commands.
    clear_visible_telemetry_history_without_bridge();
}
#[derive(Clone)]
pub(super) struct Seed {
    owner: LocalOwner,
    revision: u64,
}
impl Seed {
    pub(super) fn current(&self) -> bool {
        self.owner.current()
            && CONTINUITY
                .lock()
                .ok()
                .is_some_and(|state| state.revision == self.revision && state.revision != u64::MAX)
    }
}
pub(super) fn begin_seed() -> Option<Seed> {
    synchronize();
    let state = CONTINUITY.lock().ok()?;
    if state.revision == u64::MAX {
        return None;
    }
    Some(Seed {
        owner: state.owner.clone()?,
        revision: state.revision,
    })
}
/// Original HTTP owner fences auth ABA, actual reconnect counters, mount and
/// clear epochs. The serial additionally rejects delayed obsolete observers.
pub(super) struct Observation {
    owner: std::rc::Rc<recording_download::ExportOwner>,
    local: LocalOwner,
    serial: u64,
}
impl Observation {
    pub(super) fn current(&self) -> bool {
        self.owner.is_current() && self.local.current()
    }
}
pub(super) fn begin_observation() -> Option<Observation> {
    synchronize();
    Some(Observation {
        owner: recording_download::ExportOwner::capture().ok()?,
        local: LocalOwner::capture()?,
        serial: NEXT_OBSERVATION
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .ok()?,
    })
}
pub(super) fn observe(request: &Observation, backend: String, reset: String) {
    if !request.current() {
        return;
    }
    let identity = (backend, reset);
    let Ok(mut state) = CONTINUITY.lock() else {
        return;
    };
    let Some(replacement) = state.accept(request.serial, identity) else {
        return;
    };
    drop(state);
    // First identity refines pending seed ownership only. Earlier local rows
    // remain unbound local observations with their ORIGINAL age, never relabelled.
    if replacement {
        retire_display(true);
    }
    bump_seed_epoch();
}

#[cfg(test)]
mod tests {
    use super::*;
    fn authority(instance: &str, reset: &str) -> (String, String) {
        (instance.into(), reset.into())
    }
    #[test]
    fn first_identity_fences_seeds_without_retiring_unbound_observations() {
        let mut state = Continuity::default();
        assert_eq!(state.accept(1, authority("A", "0")), Some(false));
        assert_eq!(state.revision, 1);
        assert_eq!(state.accept(2, authority("A", "0")), None);
        assert_eq!(
            state.revision, 1,
            "same observation cannot renew or invalidate samples"
        );
    }
    #[test]
    fn confirmed_process_or_reset_replacement_retires_old_domain() {
        let mut state = Continuity::default();
        state.accept(1, authority("A", "0"));
        assert_eq!(state.accept(2, authority("B", "0")), Some(true));
        assert_eq!(state.accept(3, authority("B", "1")), Some(true));
        assert_eq!(state.revision, 3);
    }
    #[test]
    fn obsolete_or_exhausted_observers_cannot_revert_authority() {
        let mut state = Continuity::default();
        state.accept(9, authority("B", "1"));
        assert_eq!(state.accept(8, authority("A", "0")), None);
        assert_eq!(state.authority, Some(authority("B", "1")));
        state.revision = u64::MAX;
        assert_eq!(state.accept(10, authority("C", "0")), None);
        assert_eq!(state.authority, Some(authority("B", "1")));
    }
}
