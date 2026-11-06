//! Mounted current-domain facts. Archived records and session owners are separate.
use super::*;

#[derive(Clone)]
pub(super) struct Mounted {
    pub alive: Arc<AtomicBool>,
    pub mount: u64,
    pub flight_state: Signal<FlightState>,
    pub launch_clock: Signal<Option<LaunchClockMsg>>,
    pub board_status: Signal<Vec<BoardStatusEntry>>,
    pub network_topology: Signal<NetworkTopologyMsg>,
    pub action_policy: Signal<ActionPolicyMsg>,
    pub notifications: Signal<Vec<PersistentNotification>>,
    pub sequence_status: Signal<sequence_progress_host::StatusObservation>,
    pub sequence_progress: Signal<sequence_progress_model::Model>,
    pub fill_targets: Signal<Option<FillTargetsConfig>>,
    pub network_time: Signal<Option<NetworkTimeSync>>,
    pub rocket_gps: Signal<Option<(f64, f64)>>,
    pub rocket_gps_altitude: Signal<Option<f64>>,
}
static MOUNTED: GlobalSignal<Option<Mounted>> = Signal::global(|| None);
type RecoveryRetire = (Arc<AtomicBool>, std::rc::Rc<dyn Fn()>);
static RECOVERY: GlobalSignal<Option<RecoveryRetire>> = Signal::global(|| None);
static ANTENNA: GlobalSignal<Option<RecoveryRetire>> = Signal::global(|| None);
pub(super) fn register_antenna(alive: Arc<AtomicBool>, retire: impl Fn() + 'static) {
    *ANTENNA.write() = Some((alive, std::rc::Rc::new(retire)));
}
pub(super) fn register_recovery(alive: Arc<AtomicBool>, retire: impl Fn() + 'static) {
    // The public UI has one shared provider per Connectivity page; replacing
    // this slot does not root an unbounded list of disposed component handles.
    *RECOVERY.write() = Some((alive, std::rc::Rc::new(retire)));
}
type AppliedObservation = (bool, Option<station_policy_context::AppliedStamp>);
static LAST_APPLIED: Lazy<Mutex<Option<AppliedObservation>>> =
    Lazy::new(|| Mutex::new(None));

pub(super) fn register(view: Mounted) {
    *MOUNTED.write() = Some(view);
}

/// Called after the policy Mutex is released, before publishing its new banner.
/// Never restarts auth/WS/recorders/media, clears archived records, or consumes a
/// command press tombstone. A mode GET outage retaining the seal is no change.
pub(super) fn synchronize() {
    let context = station_policy_context::ingress_context();
    let next = (context.required, context.applied);
    let needs_clear = match LAST_APPLIED.lock() {
        Ok(mut previous) => {
            if previous.as_ref() == Some(&next) { return; }
            let needs_clear = previous.is_some() || next.0 || next.1.is_some();
            *previous = Some(next);
            needs_clear
        }
        // A poisoned presentation cache cannot turn clearing into a no-op.
        // Keep current facts unknown; this grants no operation authority.
        Err(_) => true,
    };
    if !needs_clear {
        return;
    }
    if let Ok(mut queue) = TELEMETRY_QUEUE.lock() {
        queue.clear();
    }
    if let Ok(mut queue) = PENDING_WS_MESSAGE_EVENTS.lock() {
        // Raw packets still carry producer metadata; none from an old applied
        // generation may be reinterpreted under the newly installed UI owner.
        queue.clear();
    }
    if let Ok(mut hidden) = HIDDEN_PENDING_WS_STATE.lock() {
        hidden.flight_state = None;
        hidden.launch_clock = None;
        hidden.board_status = None;
        hidden.network_topology = None;
        hidden.action_policy = None;
        hidden.fill_targets = None;
        hidden.network_time = None;
        // Persisted notifications/messages and recording session status survive.
    }
    clear_visible_telemetry_history_without_bridge();
    let recovery = RECOVERY.peek().clone();
    if let Some((alive, retire)) = recovery
        && alive.load(Ordering::Acquire) { retire(); }
    let antenna = ANTENNA.peek().clone();
    if let Some((alive, retire)) = antenna
        && alive.load(Ordering::Acquire) { retire(); }
    *BOARD_ABORT_SIGNAL.write() = BoardAbortSnapshot::default();
    ALERT_CONDITION_VIEW.write().retire_applied_context();
    *ACTION_POLICY_SIGNAL.write() = ActionPolicyMsg::default_locked();
    let Some(mut view) = MOUNTED.peek().clone() else {
        return;
    };
    if !view.alive.load(Ordering::Acquire)
        || !Arc::ptr_eq(&view.alive, &dashboard_alive())
        || view.mount != dashboard_gen()
        || view.flight_state.try_peek().is_err()
    {
        return;
    }
    set_signal_if_changed(&mut view.flight_state, "Unknown".into());
    set_signal_if_changed(&mut view.launch_clock, None);
    set_signal_if_changed(&mut view.board_status, Vec::new());
    set_signal_if_changed(&mut view.network_topology, NetworkTopologyMsg::default());
    set_signal_if_changed(&mut view.action_policy, ActionPolicyMsg::default_locked());
    set_signal_if_changed(&mut view.notifications, Vec::new());
    set_signal_if_changed(&mut view.sequence_status, sequence_progress_host::StatusObservation::default());
    view.sequence_progress.write().retire_applied_context();
    set_signal_if_changed(&mut view.fill_targets, None);
    set_signal_if_changed(&mut view.network_time, None);
    set_signal_if_changed(&mut view.rocket_gps, None);
    set_signal_if_changed(&mut view.rocket_gps_altitude, None);
}
