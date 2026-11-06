// Mode-epoch-bound presentation refresh; never restarts the session or transport.
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
fn refresh_layout_after_runtime_mode(
    signals: RuntimeLayoutSignals,
    mut displayed_epoch: Signal<Option<String>>,
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
    let expected = station_mode_observer::current_view_key();
    let Ok(owner) = recording_download::ExportOwner::capture_observer_selected(move || {
        station_mode_observer::current_view_key() == expected
    }) else {
        return;
    };
    let base = owner.base().to_owned();
    let cache_key = layout_cache_key_for_base(&base);
    let calibration_cache_key = calibration_visibility_cache_key_for_base(&base);
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

    spawn(async move {
        let result =
            recording_download::saved_json::<LayoutConfig>("/api/layout", owner.clone()).await;
        if !owner.is_current() {
            return;
        }
        match result {
            Ok(layout) => {
                if let Err(err) = layout.validate() {
                    log!("[layout] reconnect validation failed: {err}");
                    layout_error.set(Some(
                        "Could not load the dashboard layout. The layout file is not valid for this frontend version.".to_string(),
                    ));
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

                let policy = recording_download::saved_json::<ActionPolicyMsg>(
                    "/api/action_policy",
                    owner.clone(),
                )
                .await;
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
                layout_config.set(Some(layout.clone()));
                displayed_epoch.set(target_epoch);
                layout_loading.set(false);
                calibration_has_sensors.set(None);
                calibration_request_base.set(String::new());
                persist::_remove(&calibration_cache_key);
                if let Ok(raw) = serde_json::to_string(&layout) {
                    persist::set_string(&cache_key, &raw);
                }
            }
            Err(err) => {
                log!("[layout] reconnect refresh failed: {err}");
                layout_error.set(Some(layout_load_error_message(&err)));
            }
        }
    });
}
