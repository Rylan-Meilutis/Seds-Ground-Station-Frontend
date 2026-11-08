// Mode-epoch-bound presentation refresh; never restarts the session or transport.
#[derive(Clone, Copy)]
struct RuntimeLayoutSignals {
    layout_config: Signal<Option<LayoutConfig>>,
    layout_loading: Signal<bool>,
    layout_error: Signal<Option<String>>,
    layout_error_dismissed: Signal<Option<String>>,
    layout_request_base: Signal<String>,
    calibration_has_sensors: Signal<Option<bool>>,
    calibration_request_base: Signal<String>,
    action_policy: Signal<ActionPolicyMsg>,
}

/// Every layout load (initial, reconnect or runtime mode) takes the next
/// generation; a load only applies its layout while it is still the latest, so
/// a slow fetch from before a mode switch can never replace a newer layout.
static LAYOUT_GENERATION: GlobalSignal<u64> = Signal::global(|| 0);

/// Backend-announced in-process mode switches: (sequence, announced epoch).
/// The websocket `StationModeChanged` frame advances it; the runtime layout
/// hook re-reads the layout and action policy without waiting for a poll.
static STATION_MODE_CHANGE: GlobalSignal<(u64, Option<String>)> =
    Signal::global(|| (0, None));

fn next_layout_generation() -> u64 {
    let next = LAYOUT_GENERATION.peek().wrapping_add(1);
    *LAYOUT_GENERATION.write() = next;
    next
}

fn layout_generation_current(generation: u64) -> bool {
    *LAYOUT_GENERATION.peek() == generation
}

fn note_station_mode_changed(change: StationModeChangedMsg) {
    log!(
        "[mode] backend switched to {} (layout revision {}); refreshing layout",
        change
            .label
            .as_deref()
            .or(change.mode.as_deref())
            .unwrap_or("another mode"),
        change.layout_revision.unwrap_or_default()
    );
    let sequence = STATION_MODE_CHANGE.peek().0.wrapping_add(1);
    *STATION_MODE_CHANGE.write() = (sequence, change.policy_epoch);
    let refresh = (*station_mode_observer::REFRESH.peek()).saturating_add(1);
    *station_mode_observer::REFRESH.write() = refresh;
}

/// A layout refresh in flight: its generation and the epoch it will display.
type RuntimeLayoutRequest = Option<(u64, Option<String>)>;

/// Re-reads the actual backend layout and action policy whenever the backend
/// announces a mode switch, or the observed policy epoch changes.
fn use_runtime_mode_layout_refresh(signals: RuntimeLayoutSignals) {
    let mut displayed_epoch = use_signal(|| None::<String>);
    let mut handled_change = use_signal(|| STATION_MODE_CHANGE.peek().0);
    let in_flight = use_signal(|| None::<(u64, Option<String>)>);
    use_effect(move || {
        let (change, announced) = STATION_MODE_CHANGE.read().clone();
        let observed = station_mode_observer::OBSERVED.read().clone();
        if change != *handled_change.peek() {
            handled_change.set(change);
            let target = announced.or_else(|| {
                observed
                    .as_ref()
                    .and_then(|o| o.value.policy_epoch.clone())
            });
            refresh_layout_after_runtime_mode(signals, displayed_epoch, in_flight, target);
            return;
        }
        let Some(observed) = observed
            .filter(|o| o.current(monotonic_now_ms()) && o.value.admissions_open)
        else {
            return;
        };
        let epoch = observed.value.policy_epoch.clone();
        if *displayed_epoch.peek() == epoch {
            return;
        }
        if displayed_epoch.peek().is_none() {
            displayed_epoch.set(epoch);
            return;
        }
        let pending = in_flight
            .peek()
            .as_ref()
            .is_some_and(|(generation, target)| {
                layout_generation_current(*generation) && *target == epoch
            });
        if pending {
            return;
        }
        refresh_layout_after_runtime_mode(signals, displayed_epoch, in_flight, epoch);
    });
}

fn refresh_layout_after_runtime_mode(
    signals: RuntimeLayoutSignals,
    mut displayed_epoch: Signal<Option<String>>,
    mut in_flight: Signal<RuntimeLayoutRequest>,
    target_epoch: Option<String>,
) {
    let RuntimeLayoutSignals {
        layout_config,
        layout_loading,
        layout_error,
        layout_error_dismissed,
        layout_request_base,
        calibration_has_sensors,
        calibration_request_base,
        action_policy,
    } = signals;
    let identity = station_policy_context::view_generation();
    let generation = next_layout_generation();
    let Ok(owner) = recording_download::ExportOwner::capture_observer_selected(move || {
        layout_generation_current(generation)
            && station_policy_context::view_generation() == identity
    }) else {
        return;
    };
    in_flight.set(Some((generation, target_epoch.clone())));
    let base = owner.base().to_owned();
    let cache_key = layout_cache_key_for_base(&base);
    let calibration_cache_key = calibration_visibility_cache_key_for_base(&base);
    // The saved layout belongs to the previous mode: never restore it, even if
    // this view reloads before the new layout arrives.
    persist::_remove(&cache_key);
    let mut layout_config = layout_config;
    let mut layout_loading = layout_loading;
    let mut layout_error = layout_error;
    let mut layout_error_dismissed = layout_error_dismissed;
    let mut layout_request_base = layout_request_base;
    let mut calibration_has_sensors = calibration_has_sensors;
    let mut calibration_request_base = calibration_request_base;
    let mut action_policy = action_policy;

    layout_error.set(None);
    layout_error_dismissed.set(None);

    // Release only this request's in-flight marker; tolerate an unmounted view.
    let finish = move |mut in_flight: Signal<RuntimeLayoutRequest>| {
        let mine = in_flight
            .try_peek()
            .is_ok_and(|request| request.as_ref().is_some_and(|(g, _)| *g == generation));
        if mine && let Ok(mut request) = in_flight.try_write() {
            *request = None;
        }
    };
    spawn(async move {
        let result =
            recording_download::saved_json::<LayoutConfig>("/api/layout", owner.clone()).await;
        if !owner.is_current() {
            finish(in_flight);
            return;
        }
        match result {
            Ok(layout) => {
                if let Err(err) = layout.validate() {
                    log!("[layout] reconnect validation failed: {err}");
                    layout_error.set(Some(
                        "Could not load the dashboard layout. The layout file is not valid for this frontend version.".to_string(),
                    ));
                    finish(in_flight);
                    return;
                }

                let changed = layout_config
                    .read()
                    .as_ref()
                    .map(|current| current != &layout)
                    .unwrap_or(true);

                layout_request_base.set(base.clone());
                layout_error.set(None);
                layout_error_dismissed.set(None);
                if let Ok(raw) = serde_json::to_string(&layout) {
                    persist::set_string(&cache_key, &raw);
                }

                let policy = recording_download::saved_json::<ActionPolicyMsg>(
                    "/api/action_policy",
                    owner.clone(),
                )
                .await;
                finish(in_flight);
                if !owner.is_current() {
                    return;
                }
                let Ok(mut policy) = policy else {
                    layout_error.set(Some(
                        "Runtime mode changed; current action policy is not yet available.".into(),
                    ));
                    return;
                };
                BOARD_ABORT_SIGNAL
                    .read()
                    .constrain_launch_policy(&mut policy);
                set_signal_if_changed(&mut action_policy, policy);

                if !changed {
                    displayed_epoch.set(target_epoch.clone());
                    return;
                }

                configure_sender_split_data_types(&layout.data_tab.sender_split_data_types);
                rebuild_chart_cache_from_visible_rows();
                RESTORED_TELEMETRY_CACHE_NEEDS_CHART_REBUILD.store(false, Ordering::Relaxed);
                layout_config.set(Some(layout));
                displayed_epoch.set(target_epoch);
                layout_loading.set(false);
                calibration_has_sensors.set(None);
                calibration_request_base.set(String::new());
                persist::_remove(&calibration_cache_key);
            }
            Err(err) => {
                log!("[layout] reconnect refresh failed: {err}");
                layout_error.set(Some(layout_load_error_message(&err)));
                finish(in_flight);
            }
        }
    });
}
