// Dashboard component tree and operator command interaction.

// ============================================================================
// OUTER component: owns “real mount” lifetime & publishes it into DASHBOARD_LIFE
// INNER component is keyed for native “reload UI” without tripping outer Drop.
// ============================================================================
#[component]
/// Outer dashboard component that owns the real mount lifetime.
pub fn TelemetryDashboard() -> Element {
    // Create once per real mount
    *DASHBOARD_LIFE.write() = DashboardLife::new_alive();
    let frontend_data_clear_epoch = frontend_data_clear_epoch();

    log!(
        "[UI] TelemetryDashboard mounted (alive=true, gen={})",
        dashboard_gen()
    );

    rsx! {
        TelemetryDashboardInner {
            key: "dashboard-clear-{frontend_data_clear_epoch}"
        }
    }
}

// ---------- INNER dashboard (this is what we remount on native reload) ----------
#[component]
/// Inner dashboard component that owns the live UI state and background tasks.
fn TelemetryDashboardInner() -> Element {
    // Always valid; becomes “real” once outer publishes it.
    let alive = dashboard_alive();
    let _restored_cached_rows = use_signal(restore_cached_telemetry_rows_if_needed);
    let frontend_data_clear_epoch = frontend_data_clear_epoch();

    // ----------------------------
    // Persistent values (strings)
    // ----------------------------
    let st_warn_ack = use_signal(|| persist::get_or(WARNING_ACK_STORAGE_KEY, "0"));
    let st_err_ack = use_signal(|| persist::get_or(ERROR_ACK_STORAGE_KEY, "0"));
    let st_main_tab = use_signal(|| "state".to_string());
    let st_data_tab = use_signal(|| persist::get_or(DATA_TAB_STORAGE_KEY, "GYRO_DATA"));
    let st_base_url = use_signal(|| persist::get_or(BASE_URL_STORAGE_KEY, ""));
    let dashboard_customization = use_signal(load_dashboard_customization);
    let dashboard_edit_mode = use_signal(|| false);
    let streamer_mode = use_signal(|| persist::get_or(&streamer_mode_key(), "off") == "on");
    let ground_station_view = use_signal(|| persist::get_or(&ground_station_view_key(), "off") == "on");
    let distance_units_metric = use_signal(|| {
        persist::get_string(MAP_DISTANCE_UNITS_STORAGE_KEY)
            .map(|v| v == "metric")
            .unwrap_or(false)
    });
    let map_header_distance_visible =
        use_signal(|| persist::get_or(MAP_HEADER_DISTANCE_VISIBLE_STORAGE_KEY, "on") != "off");
    let map_header_altitude_visible =
        use_signal(|| persist::get_or(MAP_HEADER_ALTITUDE_VISIBLE_STORAGE_KEY, "off") != "off");
    let user_location_manual =
        use_signal(|| persist::get_or(USER_LOCATION_SOURCE_STORAGE_KEY, "sensor") == "manual");
    let manual_user_lat = use_signal(|| persist::get_or(USER_MANUAL_LAT_STORAGE_KEY, ""));
    let manual_user_lon = use_signal(|| persist::get_or(USER_MANUAL_LON_STORAGE_KEY, ""));
    let user_heading_manual =
        use_signal(|| persist::get_or(USER_HEADING_SOURCE_STORAGE_KEY, "sensor") == "manual");
    let manual_user_heading = use_signal(|| persist::get_or(USER_MANUAL_HEADING_STORAGE_KEY, ""));
    let theme_preset = use_signal(|| {
        let stored = persist::get_or(THEME_PRESET_STORAGE_KEY, "default");
        if stored == "layout" {
            "backend".to_string()
        } else {
            stored
        }
    });
    let language_code = use_signal(|| persist::get_or(LANGUAGE_STORAGE_KEY, "en"));
    let clock_24h = use_signal(|| persist::get_or(CLOCK_24H_STORAGE_KEY, "off") == "on");
    let network_flow_animation_enabled =
        use_signal(|| persist::get_or(NETWORK_FLOW_ANIMATION_STORAGE_KEY, "on") != "off");
    let remote_alert_acks_enabled =
        use_signal(|| persist::get_or(REMOTE_ALERT_ACKS_ENABLED_STORAGE_KEY, "on") != "off");
    let network_topology_vertical =
        use_signal(|| persist::get_or(NETWORK_TOPOLOGY_VERTICAL_STORAGE_KEY, "off") == "on");
    let state_chart_labels_vertical =
        use_signal(|| persist::get_or(STATE_CHART_LABELS_VERTICAL_STORAGE_KEY, "off") == "on");
    let chart_interpolated_gap_ms = use_signal(|| {
        let stored = persist::get_or(CHART_INTERPOLATED_GAP_MS_STORAGE_KEY, "1200000");
        let parsed = stored.parse::<u64>().ok().unwrap_or(HISTORY_MS as u64);
        if matches!(stored.trim(), "5000" | "15000") {
            HISTORY_MS as u64
        } else {
            parsed.clamp(0, HISTORY_MS as u64)
        }
    });
    let data_filter_overrides = use_signal(HashMap::<String, String>::new);
    let telemetry_retention_ms = use_signal(stored_telemetry_retention_ms);
    let telemetry_view_window_ms = use_signal(stored_telemetry_view_window_ms);
    let data_cache_enabled =
        use_signal(|| persist::get_or(DATA_CACHE_ENABLED_STORAGE_KEY, "on") != "off");
    let map_tile_cache_enabled =
        use_signal(|| persist::get_or(MAP_TILE_CACHE_ENABLED_STORAGE_KEY, "on") != "off");
    let cache_budget_mb = use_signal(stored_cache_budget_mb);
    let map_prefetch_enabled =
        use_signal(|| persist::get_or(MAP_PREFETCH_ENABLED_STORAGE_KEY, "on") != "off");
    let map_prefetch_user_radius_m =
        use_signal(|| stored_prefetch_radius_m(MAP_PREFETCH_USER_RADIUS_STORAGE_KEY));
    let map_prefetch_rocket_radius_m =
        use_signal(|| stored_prefetch_radius_m(MAP_PREFETCH_ROCKET_RADIUS_STORAGE_KEY));
    let calibration_capture_sample_count = use_signal(|| {
        persist::get_or(CALIBRATION_CAPTURE_SAMPLE_COUNT_STORAGE_KEY, "200")
            .parse::<usize>()
            .ok()
            .unwrap_or(200)
            .clamp(1, 5_000)
    });

    {
        let cache_budget_mb = cache_budget_mb;
        use_effect(move || {
            let budget_mb = (*cache_budget_mb.read()).clamp(1, 100_000);
            let budget_bytes = cache_budget_bytes_from_mb(budget_mb);
            persist::set_string(CACHE_BUDGET_MB_STORAGE_KEY, &budget_mb.to_string());
            js_eval(&format!(
                r#"
                (function() {{
                  try {{
                    window.__gs26_cache_budget_bytes = {budget_bytes};
                    if (window.localStorage) {{
                      window.localStorage.setItem("gs_cache_budget_mb", "{budget_mb}");
                    }}
                    const api = window.GS26 || window;
                    if (typeof api.setCacheBudgetBytes === "function") {{
                      api.setCacheBudgetBytes({budget_bytes});
                    }}
                  }} catch (e) {{
                    console.warn("GS26 cache budget sync failed:", e);
                  }}
                }})();
                "#
            ));
        });
    }

    let layout_config = use_signal(|| None::<LayoutConfig>);
    let layout_loading = use_signal(|| true);
    let layout_error = use_signal(|| None::<String>);
    let layout_error_dismissed = use_signal(|| None::<String>);
    let layout_request_base = use_signal(String::new);
    let calibration_has_sensors = use_signal(|| None::<bool>);
    let calibration_request_base = use_signal(String::new);
    let startup_seed_ready = use_signal(|| false);

    let parse_i64 = |s: &str| s.parse::<i64>().unwrap_or(0);

    // ----------------------------
    // Live app state
    // ----------------------------
    let active_data_tab = use_signal(|| st_data_tab.read().clone());
    let warnings = use_signal(Vec::<AlertMsg>::new);
    let errors = use_signal(Vec::<AlertMsg>::new);
    let notifications = use_signal(Vec::<PersistentNotification>::new);
    let notification_history = use_signal(Vec::<PersistentNotification>::new);
    let message_history = use_signal(Vec::<PersistentNotification>::new);
    let dismissed_notifications = use_signal(load_dismissed_notifications);
    let unread_notification_ids = use_signal(Vec::<u64>::new);
    let action_policy = use_signal(ActionPolicyMsg::default_locked);
    let recording_status = use_signal(|| RecordingStatusMsg {
        mode: "idle".to_string(),
        db_path: None,
    });
    let fill_targets = use_signal(|| None::<FillTargetsConfig>);
    let network_time = use_signal(|| None::<NetworkTimeSync>);
    let launch_clock = use_signal(|| None::<LaunchClockMsg>);
    let flight_state = use_signal(|| "Startup".to_string());
    let board_status = use_signal(Vec::<BoardStatusEntry>::new);
    let network_topology = use_signal(NetworkTopologyMsg::default);
    let frontend_network_metrics = use_signal(FrontendNetworkMetrics::default);
    let abort_only_mode = use_signal(|| false);
    let tabs_expanded = use_signal(|| false);
    let header_actions_expanded = use_signal(|| false);
    let last_applied_disable_actions_default = use_signal(|| None::<bool>);
    let show_settings_overlay = use_signal(|| false);
    let show_version_overlay = use_signal(|| false);

    let active_main_tab = use_signal(|| _main_tab_from_str(st_main_tab.read().as_str()));

    {
        let streamer_mode = streamer_mode;
        let mut active_main_tab = active_main_tab;
        use_effect(move || {
            let enabled = *streamer_mode.read();
            persist::set_string(&streamer_mode_key(), if enabled { "on" } else { "off" });
            if enabled && *active_main_tab.read() != MainTab::Mission {
                active_main_tab.set(MainTab::Mission);
            }
        });
    }

    {
        use_effect(move || {
            log!(
                "[UI] active_main_tab={}",
                _main_tab_to_str(*active_main_tab.read())
            );
        });
    }
    {
        use_effect(move || {
            log!(
                "[UI] settings_overlay_open={}",
                if *show_settings_overlay.read() {
                    "true"
                } else {
                    "false"
                }
            );
        });
    }

    {
        let mut warnings = warnings;
        let mut errors = errors;
        let mut notifications = notifications;
        let mut notification_history = notification_history;
        let mut message_history = message_history;
        let mut dismissed_notifications = dismissed_notifications;
        let mut unread_notification_ids = unread_notification_ids;
        let mut network_time = network_time;
        let mut launch_clock = launch_clock;
        let mut flight_state = flight_state;
        let mut board_status = board_status;
        let mut network_topology = network_topology;
        let mut frontend_network_metrics = frontend_network_metrics;
        use_effect(move || {
            let _ = frontend_data_clear_epoch;
            warnings.set(Vec::new());
            errors.set(Vec::new());
            notifications.set(Vec::new());
            notification_history.set(Vec::new());
            message_history.set(Vec::new());
            dismissed_notifications.set(Vec::new());
            unread_notification_ids.set(Vec::new());
            network_time.set(None);
            launch_clock.set(None);
            flight_state.set("Startup".to_string());
            board_status.set(Vec::new());
            network_topology.set(NetworkTopologyMsg::default());
            frontend_network_metrics.set(FrontendNetworkMetrics::default());
        });
    }

    {
        let mut active_data_tab = active_data_tab;
        let layout_config = layout_config;
        use_effect(move || {
            let Some(layout) = layout_config.read().clone() else {
                return;
            };
            if layout.data_tab.tabs.is_empty() {
                return;
            }
            let current = active_data_tab.read().clone();
            if !layout.data_tab.tabs.iter().any(|t| t.id == current) {
                active_data_tab.set(layout.data_tab.tabs[0].id.clone());
            }
        });
    }

    {
        let mut frontend_network_metrics = frontend_network_metrics;
        let alive = alive.clone();
        let active_main_tab = active_main_tab;
        use_effect(move || {
            reset_frontend_network_metrics_state();
            let alive = alive.clone();
            let active_main_tab = active_main_tab;
            let epoch = *WS_EPOCH.read();
            spawn(async move {
                let mut last_snapshot = FrontendNetworkMetrics::default();
                while alive.load(Ordering::Relaxed) && *WS_EPOCH.read() == epoch {
                    let snapshot = frontend_network_metrics_snapshot();
                    if snapshot != last_snapshot {
                        last_snapshot = snapshot.clone();
                        frontend_network_metrics.set(snapshot);
                    }

                    let sleep_ms = if !dashboard_page_visible() {
                        5_000
                    } else if *active_main_tab.read() == MainTab::ConnectionStatus {
                        500
                    } else {
                        2_000
                    };

                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(sleep_ms).await;
                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(sleep_ms as u64)).await;
                }
            });
        });
    }

    {
        let action_policy = action_policy;
        use_effect(move || {
            let snapshot = action_policy.read().clone();
            let enabled = snapshot.software_buttons_enabled;
            *SOFTWARE_BUTTONS_ENABLED_SIGNAL.write() = enabled;
            *ACTION_POLICY_SIGNAL.write() = snapshot;
        });
    }

    {
        let mut action_policy = action_policy;
        use_effect(move || {
            let epoch = *ACTION_POLICY_RESYNC_EPOCH.read();
            if epoch == 0 {
                return;
            }
            spawn(async move {
                for delay_ms in [250_u32, 800_u32, 1_500_u32] {
                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(delay_ms).await;
                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms as u64)).await;

                    if let Ok(policy) = http_get_json::<ActionPolicyMsg>("/api/action_policy").await
                    {
                        set_signal_if_changed(&mut action_policy, policy);
                    }
                }
            });
        });
    }

    {
        let mut active_main_tab = active_main_tab;
        let layout_config = layout_config;
        let abort_only_mode = abort_only_mode;
        let calibration_has_sensors = calibration_has_sensors;
        let dashboard_customization = dashboard_customization;
        use_effect(move || {
            let Some(layout) = layout_config.read().clone() else {
                return;
            };
            let current = *active_main_tab.read();
            let configured = _configured_main_tabs(
                &layout,
                *abort_only_mode.read(),
                *calibration_has_sensors.read(),
                &dashboard_customization.read(),
            );
            if !configured.contains(&current) {
                let next = configured.into_iter().next().unwrap_or(MainTab::State);
                active_main_tab.set(next);
            }
        });
    }

    {
        let active_main_tab = active_main_tab;
        use_effect(move || {
            if matches!(*active_main_tab.read(), MainTab::Data | MainTab::State) {
                bump_chart_render_epoch();
                if CHART_RENDER_DIRTY.load(Ordering::Acquire) {
                    schedule_dashboard_runtime_pump();
                }
            }
        });
    }

    {
        let active_main_tab = active_main_tab;
        let mut tabs_expanded = tabs_expanded;
        use_effect(move || {
            let _ = *active_main_tab.read();
            tabs_expanded.set(false);
        });
    }

    {
        let mut calibration_has_sensors = calibration_has_sensors;
        let mut calibration_request_base = calibration_request_base;
        use_effect(move || {
            if !auth::can_view_calibration() {
                calibration_has_sensors.set(Some(false));
                calibration_request_base.set(String::new());
                return;
            }
            let base = UrlConfig::base_http();
            let current_calibration_request_base = calibration_request_base.read().clone();
            if current_calibration_request_base == base {
                return;
            }
            calibration_request_base.set(base.clone());
            let visibility_cache_key = calibration_visibility_cache_key_for_base(&base);
            if let Some(cached) = persist::get_string(&visibility_cache_key) {
                match cached.as_str() {
                    "true" => calibration_has_sensors.set(Some(true)),
                    "false" => calibration_has_sensors.set(Some(false)),
                    _ => calibration_has_sensors.set(None),
                }
            } else {
                calibration_has_sensors.set(None);
            }
            spawn(async move {
                match http_get_json::<CalibrationTabLayout>("/api/calibration_config").await {
                    Ok(layout) => {
                        let has_sensors = !layout.sensors.is_empty();
                        calibration_has_sensors.set(Some(has_sensors));
                        persist::set_string(
                            &visibility_cache_key,
                            if has_sensors { "true" } else { "false" },
                        );
                    }
                    Err(_) => {
                        if persist::get_string(&visibility_cache_key).is_none() {
                            calibration_has_sensors.set(None);
                        }
                    }
                }
            });
        });
    }

    {
        let layout_config = layout_config;
        let mut abort_only_mode = abort_only_mode;
        let mut last_applied_disable_actions_default = last_applied_disable_actions_default;
        use_effect(move || {
            let Some(layout) = layout_config.read().clone() else {
                return;
            };
            let default_disabled = layout.actions_tab.disable_actions_by_default;
            let current_disable_actions_default = *last_applied_disable_actions_default.read();
            if current_disable_actions_default == Some(default_disabled) {
                return;
            }
            last_applied_disable_actions_default.set(Some(default_disabled));
            abort_only_mode.set(default_disabled);
        });
    }

    let ack_warning_ts = use_signal(|| parse_i64(st_warn_ack.read().as_str()));
    let ack_error_ts = use_signal(|| parse_i64(st_err_ack.read().as_str()));
    let warning_event_counter = use_signal(|| 0u64);
    let error_event_counter = use_signal(|| 0u64);

    let rocket_gps = use_signal(latest_rocket_gps_from_store);
    let rocket_gps_altitude_m = use_signal(latest_rocket_gps_altitude_m_from_store);
    let user_gps = use_signal(|| None::<(f64, f64)>);
    let user_gps_altitude_m = use_signal(|| None::<f64>);

    {
        let user_location_manual = user_location_manual;
        let manual_user_lat = manual_user_lat;
        let manual_user_lon = manual_user_lon;
        let mut user_gps = user_gps;
        let mut user_gps_altitude_m = user_gps_altitude_m;
        use_effect(move || {
            if *user_location_manual.read() {
                let next = parse_manual_user_coords_strings(
                    manual_user_lat.read().as_str(),
                    manual_user_lon.read().as_str(),
                );
                user_gps.set(next);
                user_gps_altitude_m.set(None);
            } else if user_gps.read().is_none() {
                user_gps_altitude_m.set(None);
            }
        });
    }
    {
        let user_heading_manual = user_heading_manual;
        let manual_user_heading = manual_user_heading;
        use_effect(move || {
            let heading = if *user_heading_manual.read() {
                parse_manual_heading_string(manual_user_heading.read().as_str())
            } else {
                None
            };
            let heading_js = heading
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string());
            js_eval(&format!(
                r#"
                (function() {{
                  try {{
                    window.__gs26_manual_user_heading = {heading_js};
                    window.__gs26_manual_heading_enabled = {enabled};
                    if ({enabled} && Number.isFinite(window.__gs26_manual_user_heading) && typeof window.setGroundMapUserHeading === "function") {{
                      window.setGroundMapUserHeading(window.__gs26_manual_user_heading);
                    }}
                  }} catch (e) {{
                    console.warn("GS26 manual heading sync failed:", e);
                  }}
                }})();
                "#,
                heading_js = heading_js,
                enabled = if *user_heading_manual.read() {
                    "true"
                } else {
                    "false"
                },
            ));
        });
    }

    {
        let rocket_gps = rocket_gps;
        let user_gps = user_gps;
        let map_prefetch_user_radius_m = map_prefetch_user_radius_m;
        let map_prefetch_rocket_radius_m = map_prefetch_rocket_radius_m;
        use_effect(move || {
            let tiles_url = map_tiles_url();
            let tiles_js = serde_json::to_string(&tiles_url).unwrap_or_else(|_| "\"\"".to_string());
            let user_radius = clamp_prefetch_radius_m(*map_prefetch_user_radius_m.read());
            let rocket_radius = clamp_prefetch_radius_m(*map_prefetch_rocket_radius_m.read());
            let coord_js = |value: Option<(f64, f64)>| -> (String, String) {
                if let Some((lat, lon)) = value
                    && lat.is_finite()
                    && lon.is_finite()
                {
                    return (lat.to_string(), lon.to_string());
                }
                ("null".to_string(), "null".to_string())
            };
            let (rocket_lat, rocket_lon) = coord_js(*rocket_gps.read());
            let (user_lat, user_lon) = coord_js(*user_gps.read());
            js_eval(&format!(
                r#"
                (function() {{
                  try {{
                    window.__gs26_prefetch_user_radius_m = {user_radius};
                    window.__gs26_prefetch_rocket_radius_m = {rocket_radius};
                    if (typeof window.setGroundMapPrefetchContext === "function") {{
                      window.setGroundMapPrefetchContext(
                        {tiles_js},
                        null,
                        {rocket_lat},
                        {rocket_lon},
                        {user_lat},
                        {user_lon}
                      );
                    }} else {{
                      window.__gs26_tiles_url = {tiles_js};
                    }}
                  }} catch (e) {{
                    console.warn("GS26 prefetch context sync failed:", e);
                  }}
                }})();
                "#,
                tiles_js = tiles_js,
                user_radius = user_radius,
                rocket_radius = rocket_radius,
                rocket_lat = rocket_lat,
                rocket_lon = rocket_lon,
                user_lat = user_lat,
                user_lon = user_lon,
            ));
        });
    }

    // ---------------------------------------------------------
    // Base URL sync
    // ---------------------------------------------------------
    {
        let mut last_applied_base = use_signal(String::new);

        use_effect(move || {
            let base = st_base_url.read().clone();
            if *last_applied_base.read() == base {
                return;
            }

            last_applied_base.set(base.clone());

            UrlConfig::set_base_url_and_persist(base);
            log!("[GS26] Base URL changed; bumping ws epoch.");
            bump_ws_epoch();
        });
    }

    // ---------------------------------------------------------
    // Layout config fetch + cache
    // ---------------------------------------------------------
    {
        let mut layout_config = layout_config;
        let mut layout_loading = layout_loading;
        let mut layout_error = layout_error;
        let mut layout_error_dismissed = layout_error_dismissed;
        let mut layout_request_base = layout_request_base;

        use_effect(move || {
            let base = UrlConfig::base_http();
            let current_layout_request_base = layout_request_base.read().clone();
            if current_layout_request_base == base {
                return;
            }
            layout_request_base.set(base.clone());
            layout_loading.set(true);
            layout_error.set(None);
            layout_error_dismissed.set(None);

            let cache_key = layout_cache_key_for_base(&base);
            if let Some(cached) = persist::get_string(&cache_key)
                && let Ok(layout) = serde_json::from_str::<LayoutConfig>(&cached)
                && let Ok(()) = layout.validate()
            {
                configure_sender_split_data_types(&layout.data_tab.sender_split_data_types);
                rebuild_chart_cache_from_visible_rows();
                RESTORED_TELEMETRY_CACHE_NEEDS_CHART_REBUILD.store(false, Ordering::Relaxed);
                layout_config.set(Some(layout));
                layout_loading.set(false);
            }

            spawn(async move {
                match http_get_json::<LayoutConfig>("/api/layout").await {
                    Ok(layout) => {
                        if let Err(err) = layout.validate() {
                            log!("[layout] validation failed: {err}");
                            layout_error.set(Some(
                                "Could not load the dashboard layout. The layout file is not valid for this frontend version.".to_string(),
                            ));
                            let has_layout_config = layout_config.read().is_some();
                            if !has_layout_config {
                                layout_loading.set(false);
                            }
                            return;
                        }
                        configure_sender_split_data_types(&layout.data_tab.sender_split_data_types);
                        rebuild_chart_cache_from_visible_rows();
                        RESTORED_TELEMETRY_CACHE_NEEDS_CHART_REBUILD
                            .store(false, Ordering::Relaxed);
                        layout_config.set(Some(layout.clone()));
                        layout_loading.set(false);
                        layout_error.set(None);
                        layout_error_dismissed.set(None);
                        if let Ok(raw) = serde_json::to_string(&layout) {
                            persist::set_string(&cache_key, &raw);
                        }
                    }
                    Err(err) => {
                        log!("[layout] load failed: {err}");
                        layout_error.set(Some(layout_load_error_message(&err)));
                        let has_layout_config = layout_config.read().is_some();
                        if !has_layout_config {
                            layout_loading.set(false);
                        }
                    }
                }
            });
        });
    }

    // Delay the first DB seed until initial UI/layout load has settled.
    // Subsequent reseeds (button/reconnect) remain immediate.
    {
        let mut startup_seed_ready = startup_seed_ready;
        let mut startup_seed_scheduled = use_signal(|| false);
        let layout_loading = layout_loading;
        let alive = alive.clone();

        use_effect(move || {
            let seed_ready = *startup_seed_ready.read();
            let loading = *layout_loading.read();
            let already_scheduled = *startup_seed_scheduled.read();
            if seed_ready || loading || already_scheduled {
                return;
            }
            startup_seed_scheduled.set(true);

            let alive = alive.clone();
            spawn(async move {
                if ui_telemetry_rows_snapshot().is_empty() {
                    set_reseed_status_running();
                }

                let delay_ms: u64 = std::env::var("GS_UI_STARTUP_SEED_DELAY_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(STARTUP_SEED_DELAY_MS)
                    .clamp(0, 15_000);

                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::TimeoutFuture::new(delay_ms as u32).await;

                #[cfg(not(target_arch = "wasm32"))]
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;

                if !alive.load(Ordering::Relaxed) {
                    return;
                }
                startup_seed_ready.set(true);
                bump_seed_epoch();
            });
        });
    }

    // Persist UI state changes
    {
        let mut st_main_tab = st_main_tab;
        use_effect(move || {
            let s = _main_tab_to_str(*active_main_tab.read()).to_string();
            st_main_tab.set(s.clone());
            persist::set_string(&scoped_main_tab_key(), &s);
        });
    }
    {
        let alive = alive.clone();
        let startup_seed_ready = startup_seed_ready;
        use_effect(move || {
            let alive = alive.clone();
            let epoch = *WS_EPOCH.read();
            spawn(async move {
                let mut was_visible = dashboard_page_visible();
                while alive.load(Ordering::Relaxed) && *WS_EPOCH.read() == epoch {
                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(2_000).await;

                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(2_000)).await;

                    if !alive.load(Ordering::Relaxed) || *WS_EPOCH.read() != epoch {
                        break;
                    }

                    let visible = dashboard_page_visible();
                    if visible && !was_visible && *startup_seed_ready.read() {
                        log!("[seed] dashboard returned to foreground; reseeding recent data");
                        bump_seed_epoch();
                    }
                    was_visible = visible;
                }
            });
        });
    }
    {
        use_effect(move || {
            if *active_main_tab.read() == MainTab::Map {
                js_eval(
                    r#"
                    (function() {
                      try {
                        if (typeof window.__gs26_map_size_hook_update === "function") {
                          window.__gs26_map_size_hook_update();
                        }
                      } catch (e) {}
                    })();
                    "#,
                );
            }
        });
    }
    {
        let mut st_data_tab = st_data_tab;
        use_effect(move || {
            let v = active_data_tab.read().clone();
            st_data_tab.set(v.clone());
            persist::set_string(DATA_TAB_STORAGE_KEY, &v);
        });
    }
    {
        let distance_units_metric = distance_units_metric;
        use_effect(move || {
            let value = if *distance_units_metric.read() {
                "metric"
            } else {
                "imperial"
            };
            persist::set_string(MAP_DISTANCE_UNITS_STORAGE_KEY, value);
        });
    }
    {
        let map_header_distance_visible = map_header_distance_visible;
        use_effect(move || {
            persist::set_string(
                MAP_HEADER_DISTANCE_VISIBLE_STORAGE_KEY,
                if *map_header_distance_visible.read() {
                    "on"
                } else {
                    "off"
                },
            );
        });
    }
    {
        let map_header_altitude_visible = map_header_altitude_visible;
        use_effect(move || {
            persist::set_string(
                MAP_HEADER_ALTITUDE_VISIBLE_STORAGE_KEY,
                if *map_header_altitude_visible.read() {
                    "on"
                } else {
                    "off"
                },
            );
        });
    }
    {
        let user_location_manual = user_location_manual;
        use_effect(move || {
            persist::set_string(
                USER_LOCATION_SOURCE_STORAGE_KEY,
                if *user_location_manual.read() {
                    "manual"
                } else {
                    "sensor"
                },
            );
        });
    }
    {
        let manual_user_lat = manual_user_lat;
        use_effect(move || {
            persist::set_string(USER_MANUAL_LAT_STORAGE_KEY, manual_user_lat.read().as_str());
        });
    }
    {
        let manual_user_lon = manual_user_lon;
        use_effect(move || {
            persist::set_string(USER_MANUAL_LON_STORAGE_KEY, manual_user_lon.read().as_str());
        });
    }
    {
        let user_heading_manual = user_heading_manual;
        use_effect(move || {
            persist::set_string(
                USER_HEADING_SOURCE_STORAGE_KEY,
                if *user_heading_manual.read() {
                    "manual"
                } else {
                    "sensor"
                },
            );
        });
    }
    {
        let manual_user_heading = manual_user_heading;
        use_effect(move || {
            persist::set_string(
                USER_MANUAL_HEADING_STORAGE_KEY,
                manual_user_heading.read().as_str(),
            );
        });
    }
    {
        let theme_preset = theme_preset;
        use_effect(move || {
            let value = theme_preset.read().clone();
            persist::set_string(THEME_PRESET_STORAGE_KEY, &value);
        });
    }
    {
        let language_code = language_code;
        use_effect(move || {
            let value = language_code.read().clone();
            *PREFERRED_LANGUAGE.write() = value.clone();
            persist::set_string(LANGUAGE_STORAGE_KEY, &value);
        });
    }
    {
        let clock_24h = clock_24h;
        use_effect(move || {
            let enabled = *clock_24h.read();
            *PREFERRED_CLOCK_24H.write() = enabled;
            persist::set_string(CLOCK_24H_STORAGE_KEY, if enabled { "on" } else { "off" });
        });
    }
    {
        let network_flow_animation_enabled = network_flow_animation_enabled;
        use_effect(move || {
            let value = if *network_flow_animation_enabled.read() {
                "on"
            } else {
                "off"
            };
            persist::set_string(NETWORK_FLOW_ANIMATION_STORAGE_KEY, value);
        });
    }
    {
        let remote_alert_acks_enabled = remote_alert_acks_enabled;
        use_effect(move || {
            let value = if *remote_alert_acks_enabled.read() {
                "on"
            } else {
                "off"
            };
            persist::set_string(REMOTE_ALERT_ACKS_ENABLED_STORAGE_KEY, value);
        });
    }
    {
        let network_topology_vertical = network_topology_vertical;
        use_effect(move || {
            let value = if *network_topology_vertical.read() {
                "on"
            } else {
                "off"
            };
            persist::set_string(NETWORK_TOPOLOGY_VERTICAL_STORAGE_KEY, value);
        });
    }
    {
        let state_chart_labels_vertical = state_chart_labels_vertical;
        use_effect(move || {
            let value = if *state_chart_labels_vertical.read() {
                "on"
            } else {
                "off"
            };
            persist::set_string(STATE_CHART_LABELS_VERTICAL_STORAGE_KEY, value);
        });
    }
    {
        let chart_interpolated_gap_ms = chart_interpolated_gap_ms;
        use_effect(move || {
            let value = (*chart_interpolated_gap_ms.read()).clamp(0, 60_000);
            persist::set_string(CHART_INTERPOLATED_GAP_MS_STORAGE_KEY, &value.to_string());
            data_chart::set_interpolated_gap_threshold_ms(value);
        });
    }
    {
        let telemetry_retention_ms = telemetry_retention_ms;
        let mut telemetry_view_window_ms = telemetry_view_window_ms;
        use_effect(move || {
            let retention_ms = clamp_telemetry_history_ms(*telemetry_retention_ms.read());
            let view_window_ms =
                clamp_telemetry_history_ms(*telemetry_view_window_ms.read()).min(retention_ms);
            if *telemetry_view_window_ms.read() != view_window_ms {
                telemetry_view_window_ms.set(view_window_ms);
            }
            persist::set_string(
                TELEMETRY_RETENTION_MS_STORAGE_KEY,
                &retention_ms.to_string(),
            );
            persist::set_string(
                TELEMETRY_VIEW_WINDOW_MS_STORAGE_KEY,
                &view_window_ms.to_string(),
            );
            apply_telemetry_history_settings(retention_ms, view_window_ms);
        });
    }
    {
        let data_cache_enabled = data_cache_enabled;
        use_effect(move || {
            let enabled = *data_cache_enabled.read();
            persist::set_string(
                DATA_CACHE_ENABLED_STORAGE_KEY,
                if enabled { "on" } else { "off" },
            );
            if !enabled {
                persist::_remove(TELEMETRY_CACHE_STORAGE_KEY);
                LAST_TELEMETRY_CACHE_PERSIST_MS.store(0, Ordering::Relaxed);
            }
        });
    }
    {
        let map_tile_cache_enabled = map_tile_cache_enabled;
        use_effect(move || {
            let enabled = *map_tile_cache_enabled.read();
            persist::set_string(
                MAP_TILE_CACHE_ENABLED_STORAGE_KEY,
                if enabled { "on" } else { "off" },
            );
            js_eval(&format!(
                r#"
                (function() {{
                  try {{
                    window.__gs26_tile_cache_enabled = {enabled};
                    window.__gs26_tile_cache_disabled = !{enabled};
                    if (window.localStorage) {{
                      window.localStorage.setItem("gs26_tile_cache_enabled", {enabled} ? "on" : "off");
                    }}
                    const api = window.GS26 || window;
                    if (typeof api.setTileCacheEnabled === "function") {{
                      api.setTileCacheEnabled({enabled});
                    }}
                  }} catch (e) {{
                    console.warn("GS26 tile cache toggle sync failed:", e);
                  }}
                }})();
                "#
            ));
        });
    }
    {
        let map_prefetch_enabled = map_prefetch_enabled;
        use_effect(move || {
            let enabled = *map_prefetch_enabled.read();
            persist::set_string(
                MAP_PREFETCH_ENABLED_STORAGE_KEY,
                if enabled { "on" } else { "off" },
            );
            js_eval(&format!(
                r#"
                (function() {{
                  try {{
                    window.__gs26_prefetch_enabled = {enabled};
                    if ({enabled}) {{
                      if (typeof window.scheduleHighResTilePrefetch === "function") {{
                        window.scheduleHighResTilePrefetch();
                      }}
                    }} else {{
                      window.__gs26_ground_map_cache_state = {{ key: "", state: "idle", pending: 0, completed: 0, failed: 0, lastStartedAt: 0, lastCompletedAt: 0 }};
                      window.__gs26_ground_map_cache_ready = false;
                    }}
                  }} catch (e) {{
                    console.warn("GS26 prefetch toggle sync failed:", e);
                  }}
                }})();
                "#
            ));
        });
    }
    {
        let map_prefetch_user_radius_m = map_prefetch_user_radius_m;
        let map_prefetch_rocket_radius_m = map_prefetch_rocket_radius_m;
        use_effect(move || {
            let user_radius = clamp_prefetch_radius_m(*map_prefetch_user_radius_m.read());
            let rocket_radius = clamp_prefetch_radius_m(*map_prefetch_rocket_radius_m.read());
            persist::set_string(
                MAP_PREFETCH_USER_RADIUS_STORAGE_KEY,
                &user_radius.to_string(),
            );
            persist::set_string(
                MAP_PREFETCH_ROCKET_RADIUS_STORAGE_KEY,
                &rocket_radius.to_string(),
            );
            js_eval(&format!(
                r#"
                (function() {{
                  try {{
                    window.__gs26_prefetch_user_radius_m = {user_radius};
                    window.__gs26_prefetch_rocket_radius_m = {rocket_radius};
                    if (typeof window.scheduleHighResTilePrefetch === "function") {{
                      window.scheduleHighResTilePrefetch({{ force: true }});
                    }}
                  }} catch (e) {{
                    console.warn("GS26 prefetch radius sync failed:", e);
                  }}
                }})();
                "#
            ));
        });
    }
    {
        let calibration_capture_sample_count = calibration_capture_sample_count;
        use_effect(move || {
            let count = (*calibration_capture_sample_count.read()).clamp(1, 5_000);
            persist::set_string(
                CALIBRATION_CAPTURE_SAMPLE_COUNT_STORAGE_KEY,
                &count.to_string(),
            );
        });
    }
    {
        let language_code = language_code;
        let alive = alive.clone();
        use_effect(move || {
            let lang = language_code.read().clone();
            *TRANSLATION_CATALOG.write() = HashMap::new();
            if let Ok(mut pending) = TRANSLATION_MISS_QUEUE.lock() {
                pending.clear();
            }
            let alive = alive.clone();
            spawn(async move {
                if !alive.load(Ordering::Relaxed) {
                    return;
                }
                let path = format!("/api/i18n/catalog?lang={lang}");
                if let Ok(response) = http_get_json::<TranslationCatalogResponse>(&path).await
                    && alive.load(Ordering::Relaxed)
                    && response.lang == lang
                {
                    *TRANSLATION_CATALOG.write() = response.translations;
                }
            });
        });
    }
    {
        let alive = alive.clone();
        use_effect(move || {
            let alive = alive.clone();
            let epoch = *WS_EPOCH.read();
            spawn(async move {
                while alive.load(Ordering::Relaxed) && *WS_EPOCH.read() == epoch {
                    let has_pending_translation_misses = TRANSLATION_MISS_QUEUE
                        .lock()
                        .map(|pending| !pending.is_empty())
                        .unwrap_or(false);
                    let sleep_ms = if has_pending_translation_misses {
                        300
                    } else if dashboard_page_visible() {
                        2_000
                    } else {
                        5_000
                    };

                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(sleep_ms).await;

                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(sleep_ms as u64)).await;

                    if !alive.load(Ordering::Relaxed) || *WS_EPOCH.read() != epoch {
                        break;
                    }

                    if TRANSLATION_REQUEST_ACTIVE
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                        .is_err()
                    {
                        continue;
                    }

                    let lang = current_language();
                    let catalog = TRANSLATION_CATALOG.read().clone();
                    let batch = drain_translation_misses(64, &catalog);
                    if batch.is_empty() {
                        TRANSLATION_REQUEST_ACTIVE.store(false, Ordering::Release);
                        continue;
                    }

                    let result = http_post_json::<TranslationRequest, TranslationResponse>(
                        "/api/i18n/translate",
                        &TranslationRequest {
                            target_lang: lang.clone(),
                            texts: batch,
                        },
                    )
                    .await;

                    if let Ok(response) = result
                        && alive.load(Ordering::Relaxed)
                        && response.lang == lang
                    {
                        merge_translation_map(response.translations);
                    }

                    TRANSLATION_REQUEST_ACTIVE.store(false, Ordering::Release);
                }
            });
        });
    }
    {
        let mut st_warn_ack = st_warn_ack;
        use_effect(move || {
            let v = ack_warning_ts.read().to_string();
            st_warn_ack.set(v.clone());
            persist::set_string(WARNING_ACK_STORAGE_KEY, &v);
        });
    }
    {
        let mut st_err_ack = st_err_ack;
        use_effect(move || {
            let v = ack_error_ts.read().to_string();
            st_err_ack.set(v.clone());
            persist::set_string(ERROR_ACK_STORAGE_KEY, &v);
        });
    }
    {
        use_effect(move || {
            let v = st_base_url.read().clone();
            persist::set_string(BASE_URL_STORAGE_KEY, &v);
        });
    }
    {
        let alive = alive.clone();
        let notifications = notifications;
        let notification_history = notification_history;
        let unread_notification_ids = unread_notification_ids;
        use_effect(move || {
            let alive = alive.clone();
            let mut notifications = notifications;
            let mut notification_history = notification_history;
            let mut unread_notification_ids = unread_notification_ids;
            let epoch = *WS_EPOCH.read();
            spawn(async move {
                while alive.load(Ordering::Relaxed) && *WS_EPOCH.read() == epoch {
                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(2_000).await;

                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(2_000)).await;

                    if !alive.load(Ordering::Relaxed) || *WS_EPOCH.read() != epoch {
                        break;
                    }

                    if !platform_network_available() {
                        if frontend_network_metrics_snapshot().ws_connected {
                            note_ws_connection_notification(
                                &mut notifications,
                                &mut notification_history,
                                &mut unread_notification_ids,
                                &auth_ws_url(&UrlConfig::base_ws()),
                                "platform network reported offline",
                            );
                            reconnect_and_reload_ui();
                        }
                        continue;
                    }

                    if !frontend_network_metrics_snapshot().ws_connected {
                        continue;
                    }

                    let last_activity_ms = LAST_WS_ACTIVITY_MONO_MS.load(Ordering::Relaxed);
                    if last_activity_ms <= 0 {
                        continue;
                    }

                    let idle_ms = (monotonic_now_ms() as i64).saturating_sub(last_activity_ms);
                    if idle_ms < WS_STALE_RECONNECT_MS {
                        continue;
                    }

                    note_ws_connection_notification(
                        &mut notifications,
                        &mut notification_history,
                        &mut unread_notification_ids,
                        &auth_ws_url(&UrlConfig::base_ws()),
                        "websocket activity timed out",
                    );
                    reconnect_and_reload_ui();
                }
            });
        });
    }

    // ------------------------------------------------------------------------
    // Event-driven runtime pump: coalesces telemetry flushes and render wakeups
    // only when data actually changes.
    // ------------------------------------------------------------------------
    {
        let alive = alive.clone();
        let active_main_tab = active_main_tab;
        let rocket_gps_flush = rocket_gps;
        let notifications_flush = notifications;
        let notification_history_flush = notification_history;
        let message_history_flush = message_history;
        let dismissed_notifications_flush = dismissed_notifications;
        let unread_notification_ids_flush = unread_notification_ids;
        let flight_state_flush = flight_state;
        let board_status_flush = board_status;
        let network_topology_flush = network_topology;
        let action_policy_flush = action_policy;
        let fill_targets_flush = fill_targets;
        let recording_status_flush = recording_status;
        let network_time_flush = network_time;
        let launch_clock_flush = launch_clock;

        use_effect(move || {
            let alive = alive.clone();
            let active_main_tab = active_main_tab;
            let mut rocket_gps_flush = rocket_gps_flush;
            let mut rocket_gps_altitude_flush = rocket_gps_altitude_m;
            let mut notifications_flush = notifications_flush;
            let mut notification_history_flush = notification_history_flush;
            let message_history_flush = message_history_flush;
            let dismissed_notifications_flush = dismissed_notifications_flush;
            let mut unread_notification_ids_flush = unread_notification_ids_flush;
            let mut flight_state_flush = flight_state_flush;
            let mut board_status_flush = board_status_flush;
            let mut network_topology_flush = network_topology_flush;
            let mut action_policy_flush = action_policy_flush;
            let mut fill_targets_flush = fill_targets_flush;
            let mut recording_status_flush = recording_status_flush;
            let mut network_time_flush = network_time_flush;
            let mut launch_clock_flush = launch_clock_flush;
            let epoch = *WS_EPOCH.read();
            let (tx, mut rx) = futures_channel::mpsc::unbounded::<DashboardRuntimeEvent>();
            if let Ok(mut slot) = DASHBOARD_RUNTIME_TX.lock() {
                *slot = Some(tx);
            }

            spawn(async move {
                use futures_util::StreamExt;

                while let Some(DashboardRuntimeEvent::Pump) = rx.next().await {
                    DASHBOARD_RUNTIME_PUMP_SCHEDULED.store(false, Ordering::Release);

                    if !alive.load(Ordering::Relaxed) || *WS_EPOCH.read() != epoch {
                        break;
                    }
                    let now_ms = current_wallclock_ms();

                    let ws_open_events: Vec<(u64, String)> =
                        if let Ok(mut q) = PENDING_WS_OPEN_EVENTS.lock() {
                            std::mem::take(&mut *q).into_iter().collect()
                        } else {
                            Vec::new()
                        };
                    for (event_epoch, ws_url) in ws_open_events {
                        if event_epoch != epoch {
                            continue;
                        }
                        note_ws_connected_and_restore_data_flow(
                            ws_url,
                            epoch,
                            &mut notifications_flush,
                            &mut notification_history_flush,
                            &mut unread_notification_ids_flush,
                        );
                        refresh_flight_state_after_ws_reconnect(flight_state_flush, epoch);
                        refresh_layout_after_ws_reconnect(
                            layout_config,
                            layout_loading,
                            layout_error,
                            layout_error_dismissed,
                            layout_request_base,
                            calibration_has_sensors,
                            calibration_request_base,
                            action_policy_flush,
                        );
                    }

                    let ws_messages: Vec<(u64, String)> =
                        if let Ok(mut q) = PENDING_WS_MESSAGE_EVENTS.lock() {
                            std::mem::take(&mut *q).into_iter().collect()
                        } else {
                            Vec::new()
                        };
                    for (event_epoch, payload) in ws_messages {
                        if event_epoch != epoch {
                            continue;
                        }
                        handle_ws_message(
                            &payload,
                            warnings,
                            errors,
                            ack_warning_ts,
                            ack_error_ts,
                            remote_alert_acks_enabled,
                            notifications_flush,
                            notification_history_flush,
                            message_history_flush,
                            dismissed_notifications_flush,
                            unread_notification_ids_flush,
                            action_policy_flush,
                            recording_status_flush,
                            fill_targets_flush,
                            network_time_flush,
                            launch_clock_flush,
                            network_topology_flush,
                            warning_event_counter,
                            error_event_counter,
                            flight_state_flush,
                            board_status_flush,
                            rocket_gps_flush,
                            rocket_gps_altitude_flush,
                            user_gps,
                            user_gps_altitude_m,
                        );
                    }

                    flush_hidden_pending_ws_state(
                        &mut flight_state_flush,
                        &mut launch_clock_flush,
                        &mut board_status_flush,
                        &mut network_topology_flush,
                        notifications_flush,
                        notification_history_flush,
                        message_history_flush,
                        dismissed_notifications_flush,
                        unread_notification_ids_flush,
                        &mut action_policy_flush,
                        &mut fill_targets_flush,
                        &mut recording_status_flush,
                        &mut network_time_flush,
                    );

                    let drained = drain_telemetry_queue_for_frame();

                    let charts_live_enabled =
                        matches!(*active_main_tab.read(), MainTab::Data | MainTab::State);
                    if let Some((gps, altitude_m)) =
                        ingest_telemetry_rows_into_runtime_caches_with_chart_mode(
                            drained,
                            charts_live_enabled,
                        )
                    {
                        if let Some(gps) = gps {
                            set_signal_if_changed(&mut rocket_gps_flush, Some(gps));
                        }
                        if let Some(altitude_m) = altitude_m {
                            set_signal_if_changed(&mut rocket_gps_altitude_flush, Some(altitude_m));
                        }
                        persist_cached_telemetry_snapshot_if_due(false);
                    }

                    if TELEMETRY_RENDER_DIRTY.load(Ordering::Acquire)
                        && dashboard_page_visible()
                        && telemetry_render_flush_due(now_ms)
                        && matches!(
                            *active_main_tab.read(),
                            MainTab::State | MainTab::Data | MainTab::Calibration
                        )
                    {
                        TELEMETRY_RENDER_DIRTY.store(false, Ordering::Release);
                        LAST_TELEMETRY_RENDER_FLUSH_MS.store(now_ms, Ordering::Relaxed);
                        bump_telemetry_render_epoch();
                    }

                    if CHART_RENDER_DIRTY.load(Ordering::Acquire)
                        && chart_render_flush_due(now_ms)
                        && matches!(*active_main_tab.read(), MainTab::Data | MainTab::State)
                    {
                        CHART_RENDER_DIRTY.store(false, Ordering::Release);
                        LAST_CHART_RENDER_FLUSH_MS.store(now_ms, Ordering::Relaxed);
                        bump_chart_render_epoch();
                    }

                    let active_tab = active_main_tab.read().clone();
                    let page_visible = dashboard_page_visible();
                    if telemetry_queue_has_rows() {
                        TELEMETRY_RENDER_DIRTY.store(true, Ordering::Release);
                        CHART_RENDER_DIRTY.store(true, Ordering::Release);
                    }
                    let pending_ws_work =
                        pending_ws_open_events_exist() || pending_ws_message_events_exist();
                    let pending_hidden_state = hidden_pending_ws_state_exists();
                    let telemetry_backlog = telemetry_queue_has_rows();
                    let mut delayed_pump_ms: Option<u32> = None;

                    if TELEMETRY_RENDER_DIRTY.load(Ordering::Acquire)
                        && page_visible
                        && matches!(
                            active_tab,
                            MainTab::State | MainTab::Data | MainTab::Calibration
                        )
                    {
                        let elapsed = now_ms
                            .saturating_sub(LAST_TELEMETRY_RENDER_FLUSH_MS.load(Ordering::Relaxed));
                        let remaining = TELEMETRY_RENDER_MIN_INTERVAL_MS.saturating_sub(elapsed);
                        delayed_pump_ms = Some(remaining.max(1) as u32);
                    }

                    if CHART_RENDER_DIRTY.load(Ordering::Acquire)
                        && matches!(active_tab, MainTab::Data | MainTab::State)
                    {
                        let elapsed = now_ms
                            .saturating_sub(LAST_CHART_RENDER_FLUSH_MS.load(Ordering::Relaxed));
                        let remaining = CHART_RENDER_MIN_INTERVAL_MS.saturating_sub(elapsed);
                        let remaining = remaining.max(1) as u32;
                        delayed_pump_ms = Some(
                            delayed_pump_ms.map_or(remaining, |current| current.min(remaining)),
                        );
                    }

                    if pending_ws_work || pending_hidden_state {
                        schedule_dashboard_runtime_pump();
                    } else if telemetry_backlog || delayed_pump_ms.is_some() {
                        let delay_ms =
                            delayed_pump_ms.unwrap_or(TELEMETRY_RENDER_MIN_INTERVAL_MS as u32);
                        schedule_dashboard_runtime_pump_after(delay_ms);
                    }
                }

                DASHBOARD_RUNTIME_PUMP_SCHEDULED.store(false, Ordering::Release);
                if let Ok(mut slot) = DASHBOARD_RUNTIME_TX.lock() {
                    slot.take();
                }
            });
        });
    }

    // ------------------------------------------------------------------------
    // Shared header clock: replaces per-badge timer loops.
    // ------------------------------------------------------------------------
    {
        let alive = alive.clone();
        let launch_clock = launch_clock;
        let network_time = network_time;

        use_effect(move || {
            let alive = alive.clone();
            let launch_clock = launch_clock;
            let network_time = network_time;
            let epoch = *WS_EPOCH.read();

            spawn(async move {
                while alive.load(Ordering::Relaxed) && *WS_EPOCH.read() == epoch {
                    let launch_clock_active = launch_clock.read().as_ref().is_some_and(|clock| {
                        matches!(clock.kind, LaunchClockKind::TMinus | LaunchClockKind::TPlus)
                    });
                    let network_time_active = network_time.read().is_some();
                    let effective_tick_ms = if !dashboard_page_visible() {
                        5_000
                    } else if launch_clock_active || network_time_active {
                        ACTIVE_LAUNCH_CLOCK_REFRESH_MS
                    } else {
                        NETWORK_TIME_BADGE_REFRESH_MS
                    };

                    if dashboard_page_visible() {
                        if telemetry_queue_has_rows() {
                            mark_live_dashboard_data_dirty();
                        } else if TELEMETRY_RENDER_DIRTY.load(Ordering::Acquire)
                            || CHART_RENDER_DIRTY.load(Ordering::Acquire)
                            || hidden_pending_ws_state_exists()
                        {
                            schedule_dashboard_runtime_pump();
                        }
                    }

                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(effective_tick_ms).await;

                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(effective_tick_ms as u64))
                        .await;

                    if !alive.load(Ordering::Relaxed) || *WS_EPOCH.read() != epoch {
                        break;
                    }

                    let mut tick = HEADER_CLOCK_TICK.write();
                    *tick = tick.wrapping_add(1);
                }
            });
        });
    }

    // Seed from DB (HTTP) on mount
    {
        let mut warnings_s = warnings;
        let mut errors_s = errors;
        let mut board_status_s = board_status;
        let mut rocket_gps_s = rocket_gps;
        let mut rocket_gps_altitude_s = rocket_gps_altitude_m;
        let mut user_gps_s = user_gps;
        let mut user_gps_altitude_s = user_gps_altitude_m;
        let mut ack_warning_ts_s = ack_warning_ts;
        let mut ack_error_ts_s = ack_error_ts;
        let mut notifications_s = notifications;
        let mut notification_history_s = notification_history;
        let mut message_history_s = message_history;
        let mut dismissed_notifications_s = dismissed_notifications;
        let mut unread_notification_ids_s = unread_notification_ids;
        let mut action_policy_s = action_policy;
        let mut recording_status_s = recording_status;
        let mut fill_targets_s = fill_targets;
        let mut network_time_s = network_time;
        let mut launch_clock_s = launch_clock;
        let mut network_topology_s = network_topology;

        let alive = alive.clone();
        let startup_seed_ready = startup_seed_ready;

        use_effect(move || {
            let alive = alive.clone();
            spawn(async move {
                use futures_util::StreamExt;

                let mut handled_seed_epoch: Option<u64> = None;
                let (tx, mut rx) = futures_channel::mpsc::unbounded::<()>();
                if let Ok(mut slot) = SEED_WATCHER_TX.lock() {
                    *slot = Some(tx);
                }
                if *startup_seed_ready.read() || SEED_WATCHER_PENDING.load(Ordering::Acquire) {
                    schedule_seed_watcher();
                }

                while alive.load(Ordering::Relaxed) {
                    let Some(()) = rx.next().await else { break };
                    if !alive.load(Ordering::Relaxed) {
                        break;
                    }
                    if !*startup_seed_ready.read() {
                        continue;
                    }

                    let seed_epoch = *SEED_EPOCH.read();
                    if handled_seed_epoch == Some(seed_epoch) {
                        SEED_WATCHER_PENDING.store(false, Ordering::Release);
                        continue;
                    }
                    SEED_WATCHER_PENDING.store(false, Ordering::Release);
                    handled_seed_epoch = Some(seed_epoch);
                    log!("[seed] watcher picked up epoch={seed_epoch}");

                    // Keep current in-memory rows visible until reseed data arrives.
                    // This avoids visible graph "blanking" during reconnect/reseed.
                    let mut last_err: Option<String> = None;
                    const RESEED_ATTEMPTS: usize = 3;
                    for attempt in 1..=RESEED_ATTEMPTS {
                        log!("[seed] epoch={seed_epoch} attempt={attempt} starting seed_from_db");
                        let res = seed_from_db(
                            &mut warnings_s,
                            &mut errors_s,
                            &mut notifications_s,
                            &mut notification_history_s,
                            &mut message_history_s,
                            &mut dismissed_notifications_s,
                            &mut unread_notification_ids_s,
                            &mut action_policy_s,
                            &mut recording_status_s,
                            &mut fill_targets_s,
                            &mut network_time_s,
                            &mut launch_clock_s,
                            &mut network_topology_s,
                            &mut board_status_s,
                            &mut rocket_gps_s,
                            &mut rocket_gps_altitude_s,
                            &mut user_gps_s,
                            &mut user_gps_altitude_s,
                            &mut ack_warning_ts_s,
                            &mut ack_error_ts_s,
                            alive.clone(),
                        )
                        .await;

                        match res {
                            Ok(()) => {
                                log!("[seed] epoch={seed_epoch} attempt={attempt} completed");
                                last_err = None;
                                break;
                            }
                            Err(e) => {
                                log!("[seed] epoch={seed_epoch} attempt={attempt} failed: {e}");
                                last_err = Some(e);
                                if attempt < RESEED_ATTEMPTS
                                    && alive.load(Ordering::Relaxed)
                                    && *SEED_EPOCH.read() == seed_epoch
                                {
                                    #[cfg(target_arch = "wasm32")]
                                    gloo_timers::future::TimeoutFuture::new(400 * attempt as u32)
                                        .await;

                                    #[cfg(not(target_arch = "wasm32"))]
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        400 * attempt as u64,
                                    ))
                                    .await;
                                }
                            }
                        }
                    }

                    if let Some(e) = last_err
                        && alive.load(Ordering::Relaxed)
                        && *SEED_EPOCH.read() == seed_epoch
                    {
                        log!("seed_from_db failed after retries: {e}");
                        set_reseed_status_failed(reseed_error_message(false, &e));
                    }
                }

                if let Ok(mut slot) = SEED_WATCHER_TX.lock() {
                    slot.take();
                }
            });
        });
    }

    // Derived state
    let warn_count = warnings.read().len();
    let err_count = errors.read().len();
    let format_capped_alert_count = |count: usize| {
        if count >= 500 {
            "500+".to_string()
        } else {
            count.to_string()
        }
    };
    let warn_count_label = format_capped_alert_count(warn_count);
    let err_count_label = format_capped_alert_count(err_count);
    let alert_pulse_high_signal = use_signal(|| true);

    let latest_warning_ts = warnings
        .read()
        .iter()
        .map(|w| w.timestamp_ms)
        .max()
        .unwrap_or(0);
    let latest_error_ts = errors
        .read()
        .iter()
        .map(|e| e.timestamp_ms)
        .max()
        .unwrap_or(0);

    let has_warnings = warn_count > 0;
    let has_errors = err_count > 0;
    let has_active_notifications = !notifications.read().is_empty();
    let has_unread_notifications = !unread_notification_ids.read().is_empty();

    let has_unacked_warnings = latest_warning_ts > *ack_warning_ts.read();
    let has_unacked_errors = latest_error_ts > *ack_error_ts.read();
    let active_error_alert = has_unacked_errors && has_errors;
    let active_warning_alert = has_unacked_warnings && has_warnings;
    let alert_pulse_high = *alert_pulse_high_signal.read();

    {
        let alive = alive.clone();
        let mut alert_pulse_high_signal = alert_pulse_high_signal;
        use_effect(move || {
            let alive = alive.clone();
            let epoch = *WS_EPOCH.read();
            spawn(async move {
                while alive.load(Ordering::Relaxed) && *WS_EPOCH.read() == epoch {
                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(600).await;

                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(600)).await;

                    if !alive.load(Ordering::Relaxed) || *WS_EPOCH.read() != epoch {
                        break;
                    }

                    let next = !*alert_pulse_high_signal.read();
                    alert_pulse_high_signal.set(next);
                }
            });
        });
    }

    let border_style = "1px solid transparent";
    let app_alert_effect = if active_error_alert && alert_pulse_high {
        "inset 0 0 0 2px #ef4444"
    } else if active_warning_alert && !active_error_alert && alert_pulse_high {
        "inset 0 0 0 2px #facc15"
    } else {
        "none"
    };
    let app_alert_animation = "";
    let warnings_tab_icon_opacity = if alert_pulse_high {
        "1"
    } else if has_unacked_warnings && has_warnings {
        "0.28"
    } else {
        "1"
    };
    let errors_tab_icon_opacity = if alert_pulse_high {
        "1"
    } else if has_unacked_errors && has_errors {
        "0.28"
    } else {
        "1"
    };
    let warnings_tab_icon_style = if active_warning_alert {
        format!(
            "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#facc15; opacity:{warnings_tab_icon_opacity};"
        )
    } else if has_warnings {
        "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#94a3b8; opacity:1; animation:none;".to_string()
    } else {
        "display:none; animation:none;".to_string()
    };
    let errors_tab_icon_style = if active_error_alert {
        format!(
            "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#ef4444; opacity:{errors_tab_icon_opacity};"
        )
    } else if has_errors {
        "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#94a3b8; opacity:1; animation:none;".to_string()
    } else {
        "display:none; animation:none;".to_string()
    };
    let notifications_tab_icon_style = if has_unread_notifications {
        "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#bfdbfe; opacity:1;".to_string()
    } else if has_active_notifications {
        "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#94a3b8; opacity:1;".to_string()
    } else {
        "display:none;".to_string()
    };
    let status_label = if !has_warnings && !has_errors {
        translate_text("Nominal")
    } else {
        translate_text("Attention")
    };
    let status_label_style = if !has_warnings && !has_errors {
        "display:inline-flex; align-items:center; min-width:12ch; color:#22c55e; font-weight:600; flex:0 0 auto;"
    } else {
        "display:inline-flex; align-items:center; min-width:12ch; color:#e5e7eb; font-weight:600; flex:0 0 auto;"
    };
    let errors_status_style = format!(
        "display:inline-flex; align-items:center; min-width:12ch; color:#fecaca; opacity:{}; flex:0 0 auto;",
        if has_errors { "1" } else { "0" }
    );
    let warnings_status_style = format!(
        "display:inline-flex; align-items:center; min-width:13ch; color:#fde68a; opacity:{}; flex:0 0 auto;",
        if has_warnings { "1" } else { "0" }
    );
    // Initial flightstate (HTTP)
    {
        let mut flight_state = flight_state;
        let alive = alive.clone();

        use_effect(move || {
            let alive = alive.clone();
            let epoch = *WS_EPOCH.read();
            spawn(async move {
                if !alive.load(Ordering::Relaxed) || *WS_EPOCH.read() != epoch {
                    return;
                }

                if let Ok(state) = http_get_json::<FlightState>("/flightstate").await
                    && alive.load(Ordering::Relaxed)
                    && *WS_EPOCH.read() == epoch
                {
                    flight_state.set(state);
                }
            });
        });
    }

    // Opening the Notifications tab dismisses active toasts while preserving history.
    {
        let notifications = notifications;
        let dismissed_notifications = dismissed_notifications;
        let unread_notification_ids = unread_notification_ids;
        use_effect(move || {
            if *active_main_tab.read() == MainTab::Notifications {
                dismiss_all_active_notifications_local_and_remote(
                    notifications,
                    dismissed_notifications,
                    unread_notification_ids,
                );
            }
        });
    }

    // WebSocket supervisor (spawn ONCE per epoch)
    {
        let alive = alive.clone();
        let mut last_started_epoch = use_signal(|| None::<u64>);

        use_effect(move || {
            let epoch = *WS_EPOCH.read();

            // IMPORTANT: if dashboard has been "logically" disabled (CONNECT pressed),
            // do not spawn a supervisor for the new epoch.
            if !alive.load(Ordering::Relaxed) {
                return;
            }
            if !web_dashboard_runtime_allowed() {
                log!("[WS] supervisor skipped on non-dashboard route");
                return;
            }

            let current_started_epoch = *last_started_epoch.read();
            if current_started_epoch == Some(epoch) {
                return;
            }
            last_started_epoch.set(Some(epoch));

            log!("[WS] supervisor spawn (epoch={epoch})");
            let alive = alive.clone();
            spawn(async move {
                if !alive.load(Ordering::Relaxed) {
                    log!("[WS] early exit (alive=false) epoch={epoch}");
                    return;
                }

                if let Err(e) = connect_ws_supervisor(
                    epoch,
                    warnings,
                    errors,
                    ack_warning_ts,
                    ack_error_ts,
                    remote_alert_acks_enabled,
                    notifications,
                    notification_history,
                    message_history,
                    dismissed_notifications,
                    unread_notification_ids,
                    action_policy,
                    recording_status,
                    fill_targets,
                    network_time,
                    launch_clock,
                    network_topology,
                    warning_event_counter,
                    error_event_counter,
                    flight_state,
                    board_status,
                    rocket_gps,
                    rocket_gps_altitude_m,
                    user_gps,
                    user_gps_altitude_m,
                    layout_config,
                    layout_loading,
                    layout_error,
                    layout_error_dismissed,
                    layout_request_base,
                    calibration_has_sensors,
                    calibration_request_base,
                    alive.clone(),
                )
                .await
                    && alive.load(Ordering::Relaxed)
                {
                    log!("[WS] supervisor ended: {e}");
                }
            });
        });
    }

    let base_theme = layout_config
        .read()
        .as_ref()
        .map(|cfg| cfg.theme.clone())
        .unwrap_or_default();
    let language_snapshot = language_code.read().clone();
    let theme_preset_value = theme_preset.read().clone();
    let theme = localized_theme(&base_theme, theme_preset_value.as_str());
    let use_layout_theme_overrides = theme_preset_uses_backend_colors(theme_preset_value.as_str());
    {
        let layout_config = layout_config;
        let theme_preset = theme_preset;
        use_effect(move || {
            let base_theme = layout_config
                .read()
                .as_ref()
                .map(|cfg| cfg.theme.clone())
                .unwrap_or_default();
            let theme = localized_theme(&base_theme, theme_preset.read().as_str());
            *APP_THEME_CONFIG.write() = theme.clone();
            apply_window_theme(&theme);
        });
    }
    let main_tab_accent = |tab_id: &str, fallback: &str| {
        theme
            .main_tab_accents
            .get(tab_id)
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    };
    // Button styles
    let tab_style_active = |color: &str| {
        format!(
            "padding:0.25rem 0.65rem 0.3rem 0.65rem; border-radius:0.5rem;\
             display:inline-flex; align-items:center; justify-content:center; gap:0.35rem;\
             font:inherit;\
             min-width:0; max-width:100%; text-align:center; line-height:1.15;\
             white-space:normal; overflow-wrap:anywhere; word-break:break-word;\
             border:1px solid {color}; background:{};\
             color:{color}; cursor:pointer;",
            theme.button_background
        )
    };
    let tab_style_inactive = format!(
        "padding:0.25rem 0.65rem 0.3rem 0.65rem; border-radius:0.5rem;\
         display:inline-flex; align-items:center; justify-content:center; gap:0.35rem;\
         font:inherit;\
         min-width:0; max-width:100%; text-align:center; line-height:1.15;\
         white-space:normal; overflow-wrap:anywhere; word-break:break-word;\
         border:1px solid {}; background:{};\
         color:{}; cursor:pointer;",
        theme.button_border, theme.button_background, theme.button_text
    );
    let dashboard_font_stack = "system-ui, -apple-system, BlinkMacSystemFont";

    // Native-only CONNECT button
    let connect_button: Element = {
        #[cfg(not(target_arch = "wasm32"))]
        use dioxus_router::use_navigator;
        #[cfg(not(target_arch = "wasm32"))]
        let nav = use_navigator();

        #[cfg(target_arch = "wasm32")]
        {
            rsx! { div {} }
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let alive_for_click = alive.clone();
            let mut header_actions_expanded = header_actions_expanded;
            let connect_button_label =
                localized_copy(&current_language(), "CONNECT", "CONECTAR", "CONNECTER");

            rsx! {

                button {
                    style: format!("
                        padding:0.45rem 0.85rem;
                        border-radius:0.75rem;
                        border:1px solid {};
                        background:{};
                        color:{};
                        font-weight:800;
                        cursor:pointer;
                    ", theme.button_border, theme.button_background, theme.button_text),
                    onclick: move |_| {
                        header_actions_expanded.set(false);
                        // KEY CHANGE:
                        // Mark dashboard "not alive" *before* bumping WS_EPOCH.
                        // That prevents the dashboard's WS supervisor effect from spawning
                        // a new epoch while we're navigating away.
                        let was_alive = alive_for_click.swap(false, Ordering::Relaxed);
                        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
                        gps::stop_gps_updates();
                        _set_dashboard_alive(false);
                        if was_alive {
                            bump_ws_epoch();
                            log!("[UI] CONNECT pressed -> alive=false + bump epoch");
                        }

                        let _ = nav.push(Route::Connect {});
                    },
                    "{connect_button_label}"
                }
            }
        }
    };

    let version_button: Element = {
        let show_version_overlay = show_version_overlay;
        let mut header_actions_expanded = header_actions_expanded;
        rsx! {
            button {
                style: format!("
                    padding:0.45rem 0.85rem;
                    border-radius:0.75rem;
                    border:1px solid {};
                    background:{};
                    color:{};
                    font-weight:800;
                    cursor:pointer;
                ", theme.button_border, theme.button_background, theme.button_text),
                onclick: {
                    let mut show_version_overlay = show_version_overlay;
                    move |_| {
                        header_actions_expanded.set(false);
                        show_version_overlay.set(true);
                    }
                },
                ontouchend: {
                    let mut show_version_overlay = show_version_overlay;
                    move |_| {
                        header_actions_expanded.set(false);
                        show_version_overlay.set(true);
                    }
                },
                {translate_text("VERSION")}
            }
        }
    };

    let settings_button: Element = {
        let show_settings_overlay = show_settings_overlay;
        let mut header_actions_expanded = header_actions_expanded;
        rsx! {
            button {
                style: format!("
                    padding:0.45rem 0.85rem;
                    border-radius:0.75rem;
                    border:1px solid {};
                    background:{};
                    color:{};
                    font-weight:800;
                    cursor:pointer;
                ", theme.button_border, theme.button_background, theme.button_text),
                onclick: {
                    let mut show_settings_overlay = show_settings_overlay;
                    move |_| {
                        header_actions_expanded.set(false);
                        show_settings_overlay.set(true);
                    }
                },
                ontouchend: {
                    let mut show_settings_overlay = show_settings_overlay;
                    move |_| {
                        header_actions_expanded.set(false);
                        show_settings_overlay.set(true);
                    }
                },
                {translate_text("SETTINGS")}
            }
        }
    };

    let reload_button_label = translate_text("RELOAD");
    let close_button_label = translate_text("Close");
    let _version_title = localized_copy(&language_snapshot, "UBSEDS GS", "UBSEDS GS", "UBSEDS GS");
    let settings_title = localized_copy(&language_snapshot, "Settings", "Ajustes", "Parametres");
    let sign_in_label = localized_copy(
        &language_snapshot,
        "SIGN IN",
        "INICIAR SESIÓN",
        "SE CONNECTER",
    );
    let sign_out_prefix = localized_copy(
        &language_snapshot,
        "SIGN OUT",
        "CERRAR SESIÓN",
        "SE DECONNECTER",
    );
    let auth_label = auth::current_session()
        .and_then(|session| session.session.username)
        .map(|username| format!("{sign_out_prefix} {username}"))
        .unwrap_or(sign_in_label);
    let disable_actions_label = if *abort_only_mode.read() {
        translate_text("Actions Disabled")
    } else {
        translate_text("Actions Enabled")
    };

    let auth_button: Element = {
        use dioxus_router::use_navigator;
        let nav = use_navigator();
        let base = UrlConfig::base_http();
        let skip_tls = UrlConfig::_skip_tls_verify();
        let mut header_actions_expanded = header_actions_expanded;
        rsx! {
            button {
                style: format!("
                    padding:0.45rem 0.85rem;
                    border-radius:0.75rem;
                    border:1px solid {};
                    background:{};
                    color:{};
                    font-weight:800;
                    cursor:pointer;
                ", theme.button_border, theme.button_background, theme.button_text),
                onclick: move |_| {
                    header_actions_expanded.set(false);
                    if auth::current_session().is_some() {
                        let base = base.clone();
                        spawn(async move {
                            let _ = auth::logout(&base, skip_tls).await;
                            auth::clear_current_session();
                            _set_dashboard_alive(false);
                            bump_ws_epoch();
                            reconnect_and_reseed_after_auth_change();
                            let _ = nav.replace(Route::Login {});
                        });
                    } else {
                        _set_dashboard_alive(false);
                        bump_ws_epoch();
                        auth::clear_current_session();
                        let _ = nav.replace(Route::Login {});
                    }
                },
                "{auth_label}"
            }
        }
    };

    let layout_config = layout_config;
    let mut layout_loading = layout_loading;
    let mut layout_error = layout_error;
    let mut layout_error_dismissed = layout_error_dismissed;
    let mut layout_request_base = layout_request_base;
    let calibration_has_sensors = calibration_has_sensors;
    let mut calibration_request_base = calibration_request_base;
    let mut _refresh_layout = move || {
        let base = UrlConfig::base_http();
        let cache_key = layout_cache_key_for_base(&base);
        layout_request_base.set(String::new());
        calibration_request_base.set(String::new());
        layout_loading.set(true);
        layout_error.set(None);
        layout_error_dismissed.set(None);
        let mut layout_config = layout_config;
        let mut layout_loading = layout_loading;
        let mut layout_error = layout_error;
        let mut layout_error_dismissed = layout_error_dismissed;
        let mut layout_request_base = layout_request_base;
        spawn(async move {
            match http_get_json::<LayoutConfig>("/api/layout").await {
                Ok(layout) => {
                    if let Err(err) = layout.validate() {
                        log!("[layout] validation failed: {err}");
                        layout_error.set(Some(
                            "Could not load the dashboard layout. The layout file is not valid for this frontend version.".to_string(),
                        ));
                        let has_layout_config = layout_config.read().is_some();
                        if !has_layout_config {
                            layout_loading.set(false);
                        }
                        return;
                    }
                    configure_sender_split_data_types(&layout.data_tab.sender_split_data_types);
                    rebuild_chart_cache_from_visible_rows();
                    RESTORED_TELEMETRY_CACHE_NEEDS_CHART_REBUILD.store(false, Ordering::Relaxed);
                    layout_request_base.set(base.clone());
                    layout_config.set(Some(layout.clone()));
                    layout_loading.set(false);
                    layout_error.set(None);
                    layout_error_dismissed.set(None);
                    if let Ok(raw) = serde_json::to_string(&layout) {
                        persist::set_string(&cache_key, &raw);
                    }
                }
                Err(err) => {
                    log!("[layout] load failed: {err}");
                    layout_error.set(Some(layout_load_error_message(&err)));
                    let has_layout_config = layout_config.read().is_some();
                    if !has_layout_config {
                        layout_loading.set(false);
                    }
                }
            }
        });
    };

    let reload_button: Element = {
        let mut header_actions_expanded = header_actions_expanded;
        rsx! {
            button {
                style: format!("
                padding:0.45rem 0.85rem;
                border-radius:0.75rem;
                border:1px solid {};
                background:{};
                color:{};
                font-weight:800;
                cursor:pointer;
            ", theme.button_border, theme.button_background, theme.button_text),
                onclick: move |_| {
                    header_actions_expanded.set(false);
                    _refresh_layout();
                    hard_reload_dashboard_data();
                },
                "{reload_button_label}"
            }
        }
    };

    fn start_gps_js() -> bool {
        // Only needed if you want to gate geolocation until the JS is ready on wasm:
        #[cfg(target_arch = "wasm32")]
        return js_is_ground_map_ready();

        #[cfg(not(target_arch = "wasm32"))]
        true
    }

    let layout_snapshot = layout_config.read().clone();
    let layout_error_snapshot = layout_error.read().clone();
    let layout_error_dismissed_snapshot = layout_error_dismissed.read().clone();
    let layout_cached_error_banner = layout_error_snapshot.clone().and_then(|msg| {
        if layout_snapshot.is_some()
            && layout_error_dismissed_snapshot.as_deref() != Some(msg.as_str())
        {
            Some(msg)
        } else {
            None
        }
    });
    let layout_loading_snapshot = *layout_loading.read();
    let version_overlay_open = *show_version_overlay.read();
    let version_overlay: Element = {
        if version_overlay_open {
            rsx! {
                div {
                    style: "
                        position:fixed;
                        inset:0;
                        z-index:3000;
                        display:flex;
                        flex-wrap:wrap;
                        align-items:flex-start;
                        justify-content:center;
                        padding:24px 16px;
                        overflow-y:auto;
                        overflow-x:hidden;
                        background:{theme.app_background};
                        font-family:{dashboard_font_stack};
                        backdrop-filter:blur(6px);
                        overscroll-behavior:contain;
                        -webkit-overflow-scrolling:touch;
                    ",
                    onclick: {
                        let mut show_version_overlay = show_version_overlay;
                        move |_| show_version_overlay.set(false)
                    },
                    div {
                        style: "
                            width:min(900px, 100%);
                            padding:24px;
                            color:{theme.text_primary};
                            border:1px solid {theme.tab_shell_border};
                            border-radius:16px;
                            background:{theme.tab_shell_background};
                            font-family:{dashboard_font_stack};
                            box-shadow:0 12px 30px rgba(0,0,0,0.5);
                        ",
                        onclick: move |evt| evt.stop_propagation(),
                        ontouchend: move |evt| evt.stop_propagation(),
                        div {
                            style: "display:flex; align-items:flex-start; justify-content:space-between; gap:12px; margin-bottom:12px; flex-wrap:wrap;",
                            h1 { style: "margin:0; font-size:20px;", "{_version_title}" }
                            button {
                                style: "
                                    padding:10px 14px;
                                    border-radius:12px;
                                    border:1px solid {theme.button_border};
                                    background:{theme.button_background};
                                    color:{theme.button_text};
                                    font-family:{dashboard_font_stack};
                                    font-weight:700;
                                    cursor:pointer;
                                ",
                                onclick: {
                                    let mut show_version_overlay = show_version_overlay;
                                    move |_| show_version_overlay.set(false)
                                },
                                ontouchend: {
                                    let mut show_version_overlay = show_version_overlay;
                                    move |_| show_version_overlay.set(false)
                                },
                                "{close_button_label}"
                            }
                        }
                        VersionTab { theme: theme.clone() }
                    }
                }
            }
        } else {
            rsx! { div {} }
        }
    };
    let settings_overlay_open = *show_settings_overlay.read();
    let settings_overlay: Element = {
        if settings_overlay_open {
            rsx! {
                div {
                    style: "
                        position:fixed;
                        inset:0;
                        z-index:3000;
                        display:flex;
                        align-items:flex-start;
                        justify-content:center;
                        padding:24px 16px;
                        overflow-y:auto;
                        overflow-x:hidden;
                        background:{theme.app_background};
                        font-family:{dashboard_font_stack};
                        backdrop-filter:blur(6px);
                        overscroll-behavior:contain;
                        -webkit-overflow-scrolling:touch;
                    ",
                    onclick: {
                        let mut show_settings_overlay = show_settings_overlay;
                        move |_| show_settings_overlay.set(false)
                    },
                    div {
                        style: "
                            width:min(980px, 100%);
                            padding:24px;
                            color:{theme.text_primary};
                            border:1px solid {theme.tab_shell_border};
                            border-radius:16px;
                            background:{theme.tab_shell_background};
                            font-family:{dashboard_font_stack};
                            box-shadow:0 12px 30px rgba(0,0,0,0.5);
                        ",
                        onclick: move |evt| evt.stop_propagation(),
                        ontouchend: move |evt| evt.stop_propagation(),
                        div {
                            style: "display:flex; align-items:flex-start; justify-content:space-between; gap:12px; margin-bottom:12px; flex-wrap:wrap;",
                            h1 { style: "margin:0; font-size:20px;", "{settings_title}" }
                            button {
                                style: "
                                    padding:10px 14px;
                                    border-radius:12px;
                                    border:1px solid {theme.button_border};
                                    background:{theme.button_background};
                                    color:{theme.button_text};
                                    font-family:{dashboard_font_stack};
                                    font-weight:700;
                                    cursor:pointer;
                                ",
                                onclick: {
                                    let mut show_settings_overlay = show_settings_overlay;
                                    move |_| show_settings_overlay.set(false)
                                },
                                ontouchend: {
                                    let mut show_settings_overlay = show_settings_overlay;
                                    move |_| show_settings_overlay.set(false)
                                },
                                "{close_button_label}"
                            }
                        }
                        SettingsPage {
                            distance_units_metric: distance_units_metric,
                            map_header_distance_visible: map_header_distance_visible,
                            map_header_altitude_visible: map_header_altitude_visible,
                            user_location_manual: user_location_manual,
                            manual_user_lat: manual_user_lat,
                            manual_user_lon: manual_user_lon,
                            user_heading_manual: user_heading_manual,
                            manual_user_heading: manual_user_heading,
                            theme_preset: theme_preset,
                            language_code: language_code,
                            clock_24h: clock_24h,
                            network_flow_animation_enabled: network_flow_animation_enabled,
                            remote_alert_acks_enabled: remote_alert_acks_enabled,
                            network_topology_vertical: network_topology_vertical,
                            state_chart_labels_vertical: state_chart_labels_vertical,
                            chart_interpolated_gap_ms: chart_interpolated_gap_ms,
                            data_filter_rows: data_filter_settings_rows(layout_snapshot.as_ref()),
                            data_filter_overrides: data_filter_overrides,
                            telemetry_retention_ms: telemetry_retention_ms,
                            telemetry_view_window_ms: telemetry_view_window_ms,
                            data_cache_enabled: data_cache_enabled,
                            map_tile_cache_enabled: map_tile_cache_enabled,
                            cache_budget_mb: cache_budget_mb,
                            map_prefetch_enabled: map_prefetch_enabled,
                            map_prefetch_user_radius_m: map_prefetch_user_radius_m,
                            map_prefetch_rocket_radius_m: map_prefetch_rocket_radius_m,
                            calibration_capture_sample_count: calibration_capture_sample_count,
                            streamer_mode: streamer_mode,
                            ground_station_view,
                            storage_breakdown: cache_storage_stats_rows(),
                            measured_cache_bytes: cache_storage_measured_bytes(),
                            theme: theme.clone(),
                            on_clear_data_cache: move |_| {
                                debug_log::append("[settings] clear_data_cache requested");
                                clear_data_caches_and_reseed();
                            },
                            on_clear_current_data: move |_| {
                                debug_log::append("[settings] clear_current_data requested");
                                clear_current_dashboard_data_without_reseed();
                            },
                            on_clear_data_and_map_cache: move |_| {
                                debug_log::append("[settings] clear_data_and_map_cache requested");
                                clear_data_and_map_tile_caches_and_reseed();
                            },
                            on_clear_all_caches: move |_| {
                                debug_log::append("[settings] clear_all_caches requested");
                                clear_all_frontend_caches_and_reseed();
                            },
                            on_prefetch_map_tiles: move |_| {
                                debug_log::append("[settings] map_prefetch requested");
                                trigger_map_prefetch_now();
                            },
                            on_reset_app_data: {
                                let mut st_warn_ack = st_warn_ack;
                                let mut st_err_ack = st_err_ack;
                                let mut st_main_tab = st_main_tab;
                                let mut st_data_tab = st_data_tab;
                                let mut st_base_url = st_base_url;
                                let mut distance_units_metric = distance_units_metric;
                                let mut map_header_distance_visible = map_header_distance_visible;
                                let mut map_header_altitude_visible = map_header_altitude_visible;
                                let mut user_location_manual = user_location_manual;
                                let mut manual_user_lat = manual_user_lat;
                                let mut manual_user_lon = manual_user_lon;
                                let mut user_heading_manual = user_heading_manual;
                                let mut manual_user_heading = manual_user_heading;
                                let mut theme_preset = theme_preset;
                                let mut language_code = language_code;
                                let mut clock_24h = clock_24h;
                                let mut network_flow_animation_enabled = network_flow_animation_enabled;
                                let mut remote_alert_acks_enabled = remote_alert_acks_enabled;
                                let mut network_topology_vertical = network_topology_vertical;
                                let mut state_chart_labels_vertical = state_chart_labels_vertical;
                                let mut chart_interpolated_gap_ms = chart_interpolated_gap_ms;
                                let mut telemetry_retention_ms = telemetry_retention_ms;
                                let mut telemetry_view_window_ms = telemetry_view_window_ms;
                                let mut data_cache_enabled = data_cache_enabled;
                                let mut map_tile_cache_enabled = map_tile_cache_enabled;
                                let mut cache_budget_mb = cache_budget_mb;
                                let mut map_prefetch_enabled = map_prefetch_enabled;
                                let mut map_prefetch_user_radius_m = map_prefetch_user_radius_m;
                                let mut map_prefetch_rocket_radius_m = map_prefetch_rocket_radius_m;
                                let mut calibration_capture_sample_count = calibration_capture_sample_count;
                                let mut streamer_mode = streamer_mode;
                                move |_| {
                                    debug_log::append("[settings] reset_app_data requested");
                                    reset_local_app_data();
                                    st_warn_ack.set("0".to_string());
                                    st_err_ack.set("0".to_string());
                                    st_main_tab.set("state".to_string());
                                    st_data_tab.set("GYRO_DATA".to_string());
                                    st_base_url.set(String::new());
                                    distance_units_metric.set(false);
                                    map_header_distance_visible.set(true);
                                    map_header_altitude_visible.set(false);
                                    user_location_manual.set(false);
                                    manual_user_lat.set(String::new());
                                    manual_user_lon.set(String::new());
                                    user_heading_manual.set(false);
                                    manual_user_heading.set(String::new());
                                    theme_preset.set("default".to_string());
                                    language_code.set("en".to_string());
                                    clock_24h.set(false);
                                    network_flow_animation_enabled.set(true);
                                    remote_alert_acks_enabled.set(true);
                                    network_topology_vertical.set(false);
                                    state_chart_labels_vertical.set(false);
                                    chart_interpolated_gap_ms.set(HISTORY_MS as u64);
                                    telemetry_retention_ms.set(DEFAULT_TELEMETRY_RETENTION_MS);
                                    telemetry_view_window_ms.set(DEFAULT_TELEMETRY_VIEW_WINDOW_MS);
                                    data_cache_enabled.set(true);
                                    map_tile_cache_enabled.set(true);
                                    cache_budget_mb.set(DEFAULT_CACHE_BUDGET_MB);
                                    map_prefetch_enabled.set(true);
                                    map_prefetch_user_radius_m.set(DEFAULT_PREFETCH_RADIUS_M);
                                    map_prefetch_rocket_radius_m.set(DEFAULT_PREFETCH_RADIUS_M);
                                    calibration_capture_sample_count.set(200);
                                    streamer_mode.set(false);
                                }
                            },
                            title: settings_title.clone(),
                        }
                    }
                }
            }
        } else {
            rsx! { div {} }
        }
    };

    // MAIN UI
    rsx! {
            gps::GpsDriver {
                user_gps: user_gps,
                user_altitude_m: Some(user_gps_altitude_m),
                // Only needed if you want to gate geolocation until the JS is ready on wasm:
                js_ready: Some(start_gps_js()),
            }
                style {
                    "@keyframes gs26-blink-slow-off {{ 0%, 100% {{ opacity: 0.2; }} 18% {{ opacity: 1.0; }} }}
             @keyframes gs26-blink-slow-on  {{ 0%, 100% {{ opacity: 1.0; }} 82% {{ opacity: 0.25; }} }}
             @keyframes gs26-blink-fast-off {{ 0%, 100% {{ opacity: 0.15; }} 45% {{ opacity: 1.0; }} }}
             @keyframes gs26-blink-fast-on  {{ 0%, 100% {{ opacity: 1.0; }} 55% {{ opacity: 0.2; }} }}
             @keyframes gs26-alert-pulse {{
               0%, 49.999% {{ box-shadow: var(--gs26-alert-shadow-high, none); opacity: var(--gs26-alert-opacity-high, 1); }}
               50%, 100% {{ box-shadow: var(--gs26-alert-shadow-low, none); opacity: var(--gs26-alert-opacity-low, 1); }}
             }}
             .gs26-tab-shell {{ min-width:260px; }}
             .gs26-dashboard-shell[data-streamer="true"] {{ padding:0 !important; }}
             .gs26-dashboard-shell[data-streamer="true"] > .gs26-header-row,
             .gs26-dashboard-shell[data-streamer="true"] > .gs26-header-secondary {{ display:none !important; }}
             .gs26-dashboard-shell[data-streamer="true"] .gs26-mission-shell {{ padding:0; overflow:hidden; }}
             .gs26-dashboard-shell[data-streamer="true"] .gs26-mission-hero {{ min-height:var(--gs26-app-height); border:0 !important; border-radius:0; }}
             .gs26-dashboard-shell[data-streamer="true"] .gs26-mission-header,
             .gs26-dashboard-shell[data-streamer="true"] .gs26-mission-editor,
             .gs26-dashboard-shell[data-streamer="true"] .gs26-angle-strip,
             .gs26-dashboard-shell[data-streamer="true"] .gs26-vehicle-header {{ display:none !important; }}
             .gs26-dashboard-shell[data-streamer="true"] .gs26-vehicle-shell {{ padding:0 !important; overflow:hidden; }}
             .gs26-dashboard-shell[data-streamer="true"] .gs26-vehicle-grid {{ grid-template-columns:1fr; gap:0; height:100%; }}
             .gs26-dashboard-shell[data-streamer="true"] .gs26-vehicle-grid > :first-child {{ height:100%; min-height:var(--gs26-app-height); border:0 !important; border-radius:0 !important; }}
             .gs26-dashboard-shell[data-streamer="true"] .gs26-vehicle-grid > :nth-child(2) {{ display:none !important; }}
             .gs26-tab-toggle {{ display:none; }}
             .gs26-tab-nav {{ display:flex; gap:0.5rem; flex-wrap:nowrap; overflow-x:auto; max-width:100%; }}
             .gs26-tab-nav > * {{ flex-shrink:0; }}
             .gs26-status-shell {{ flex:1000 1 520px; display:grid; grid-template-columns:minmax(0, 1fr) max-content; grid-template-rows:auto auto; align-items:center; column-gap:0.75rem; row-gap:0; padding:0.16rem 0.6rem 0.24rem 0.6rem; border-radius:1rem; min-width:260px; overflow:hidden; container-type:inline-size; align-self:start; }}
             .gs26-status-row {{ display:flex; align-items:center; flex-wrap:wrap; gap:0.5rem; min-width:0; line-height:1.08; margin:0; }}
             .gs26-status-row {{ grid-column:1; grid-row:1; }}
             .gs26-status-flight {{ display:flex; align-items:baseline; gap:0.35rem; min-width:0; width:fit-content; max-width:100%; flex-wrap:nowrap; white-space:nowrap; line-height:1.1; margin:0; padding:0.04rem 0.12rem 0.07rem 0; }}
             .gs26-status-flight {{ grid-column:1; grid-row:2; }}
             .gs26-launch-clock {{ display:inline-flex; align-items:baseline; line-height:1; white-space:nowrap; vertical-align:baseline; color:#f8fafc; font-weight:800; }}
             .gs26-launch-clock-value {{ display:inline-flex; align-items:baseline; width:9ch; padding-left:0.35ch; line-height:1; text-align:left; font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace; font-variant-numeric:tabular-nums; }}
             .gs26-status-network {{ grid-column:2; grid-row:1; justify-self:end; line-height:1; }}
             .gs26-status-launch {{ grid-column:2; grid-row:2; justify-self:end; line-height:1; }}
             .gs26-status-ack {{ grid-column:2; grid-row:3; justify-self:end; }}
             .gs26-status-ack[data-active=\"false\"] {{ display:none !important; }}
             .gs26-status-count[data-active=\"false\"] {{ opacity:0; }}
             @container (max-width: 520px) {{
               .gs26-status-shell {{
                 grid-template-columns:minmax(0, 1fr);
                 grid-template-rows:auto auto auto auto;
                 justify-content:center;
                 justify-items:center;
                 align-items:center;
                 column-gap:0;
                 text-align:center;
                 row-gap:0.02rem;
                 padding:0.12rem 0.45rem;
               }}
               .gs26-status-row {{
                 grid-column:1;
                 grid-row:1;
                 justify-content:center;
                 justify-self:center;
                 width:fit-content;
                 max-width:100%;
                 text-align:center;
               }}
               .gs26-status-row .gs26-status-value {{
                 min-width:0 !important;
                 flex:0 1 auto !important;
               }}
               .gs26-status-flight {{
                 grid-column:1;
                 grid-row:2;
                 justify-content:center;
                 width:fit-content;
                 max-width:100%;
                 text-align:center;
               }}
               .gs26-status-network {{
                 grid-column:1;
                 grid-row:3;
                 justify-self:center;
                 max-width:100%;
               }}
               .gs26-status-launch {{
                 grid-column:1;
                 grid-row:4;
                 justify-self:center;
                 max-width:100%;
               }}
               .gs26-status-ack {{
                 grid-column:1;
                 grid-row:5;
                 justify-self:center;
               }}
               .gs26-status-count[data-active=\"false\"] {{
                 display:none !important;
               }}
             }}
             @media (max-width: 860px) {{
               .gs26-status-shell {{
                 grid-template-columns:minmax(0, 1fr);
                 grid-template-rows:auto auto auto auto;
                 justify-content:center;
                 justify-items:center;
                 align-items:center;
                 column-gap:0;
                 text-align:center;
                 row-gap:0.02rem;
                 padding:0.12rem 0.45rem;
               }}
               .gs26-status-row {{
                 grid-column:1;
                 grid-row:1;
                 justify-content:center;
                 justify-self:center;
                 width:fit-content;
                 max-width:100%;
                 text-align:center;
               }}
               .gs26-status-row .gs26-status-value {{
                 min-width:0 !important;
                 flex:0 1 auto !important;
               }}
               .gs26-status-flight {{
                 grid-column:1;
                 grid-row:2;
                 justify-content:center;
                 justify-self:center;
                 width:fit-content;
                 max-width:100%;
                 text-align:center;
               }}
               .gs26-status-network {{
                 grid-column:1;
                 grid-row:3;
                 justify-self:center;
                 max-width:100%;
               }}
               .gs26-status-launch {{
                 grid-column:1;
                 grid-row:4;
                 justify-self:center;
                 max-width:100%;
               }}
               .gs26-status-ack {{
                 grid-column:1;
                 grid-row:5;
                 justify-self:center;
                 align-self:center !important;
                 margin-left:0 !important;
               }}
               .gs26-status-ack[data-active=\"false\"] {{
                 display:none !important;
               }}
               .gs26-status-value {{
                 min-width:0 !important;
               }}
               .gs26-status-count[data-active=\"false\"] {{
                 display:none !important;
               }}
             }}
             .gs26-header-row {{
               position:relative;
             }}
             .gs26-header-title {{
               flex:0 1 auto;
             }}
             .gs26-header-actions-shell {{ margin-left:auto; position:relative; z-index:2000; }}
             .gs26-header-actions-list {{ display:flex; align-items:center; gap:10px; flex-wrap:wrap; }}
             .gs26-header-menu-toggle {{ display:none; }}
             .gs26-header-abort-mobile {{ display:none; }}
             .gs26-header-secondary {{
               display:flex;
               align-items:center;
               gap:12px;
               width:100%;
               margin-bottom:12px;
               flex-wrap:wrap;
             }}
             @media (max-width: 900px) {{
               .gs26-header-row {{
                 display:grid !important;
                 grid-template-columns:minmax(0, auto) minmax(0, 1fr) minmax(0, auto);
                 align-items:center;
                 gap:4px;
                 flex-wrap:nowrap !important;
                 min-height:32px;
                 margin-bottom:8px !important;
               }}
               .gs26-header-title {{
                 grid-column:2;
                 justify-self:center;
                 text-align:center;
                 min-width:0;
                 max-width:100%;
                 width:100%;
                 padding:0 3.9rem;
                 font-size:clamp(10px, 3.4vw, 14px) !important;
                 line-height:0.95;
                 white-space:nowrap;
                 overflow:hidden;
                 text-overflow:ellipsis;
                 pointer-events:none;
               }}
               .gs26-header-actions-shell {{
                 grid-column:1 / 4;
                 position:absolute;
                 inset:0;
                 display:flex;
                 align-items:center;
                 justify-content:space-between;
                 gap:6px;
                 width:100%;
                 margin-left:0;
                 pointer-events:none;
               }}
               .gs26-header-actions-shell[data-expanded=\"true\"] {{
                 pointer-events:auto;
               }}
               .gs26-header-abort-mobile {{
                 display:inline-flex;
                 align-items:center;
                 justify-content:center;
                 margin-left:0 !important;
                 flex:0 0 auto;
                 order:0;
                 pointer-events:auto;
               }}
               .gs26-header-menu-toggle {{
                 display:inline-flex;
                 align-items:center;
                 justify-content:center;
                 padding:0.24rem 0.52rem;
                 border-radius:0.58rem;
                 border:1px solid var(--gs26-header-menu-border);
                 background:var(--gs26-header-menu-background);
                 color:var(--gs26-header-menu-text);
                 font:inherit;
                 font-weight:800;
                 font-size:0.76rem;
                 cursor:pointer;
                 margin-left:auto;
                 order:2;
                 pointer-events:auto;
               }}
               .gs26-header-actions-list {{
                 display:none;
                 position:absolute;
                 top:calc(100% + 6px);
                 right:0;
                 z-index:60;
                 min-width:min(320px, calc(100vw - 32px));
                 max-width:calc(100vw - 32px);
                 padding:0.45rem;
                 border-radius:0.8rem;
                 border:1px solid var(--gs26-header-menu-border);
                 background:var(--gs26-header-menu-background);
                 box-shadow:0 18px 40px rgba(0,0,0,0.4);
                 flex-direction:column;
                 align-items:stretch;
                 gap:5px;
                 pointer-events:auto;
               }}
               .gs26-header-actions-shell[data-expanded=\"true\"] .gs26-header-actions-list {{
                 display:flex;
               }}
               .gs26-header-actions-list button {{
                 width:100%;
                 margin-left:0 !important;
                 padding:0.3rem 0.62rem !important;
                 font-size:0.78rem !important;
                }}
               .gs26-header-actions-list .gs26-header-abort-menu {{
                 display:none;
               }}
               .gs26-header-secondary {{
                 gap:6px !important;
                 margin-bottom:6px !important;
               }}
             }}
             @media (max-width: 720px), (max-height: 780px) {{
               .gs26-dashboard-shell {{
                 padding:5px 7px 7px 7px !important;
               }}
               .gs26-header-row {{
                 min-height:30px;
               }}
               .gs26-header-secondary {{
                 gap:5px !important;
                 margin-bottom:5px !important;
               }}
               .gs26-tab-shell {{
                 flex:1 1 100%;
                 min-width:0;
                 display:grid !important;
                 width:100% !important;
                 justify-content:stretch !important;
                 align-items:center !important;
                 justify-items:center !important;
                 row-gap:0.35rem;
                 padding:0.32rem;
               }}
               .gs26-tab-shell[data-expanded=\"false\"] {{
                 grid-template-columns:minmax(0, 1fr);
                 justify-content:stretch;
                 justify-items:stretch;
               }}
               .gs26-tab-shell[data-expanded=\"true\"] {{
                 grid-template-columns:minmax(0, 1fr);
                 column-gap:0;
                 row-gap:0.45rem;
                 justify-content:stretch;
               }}
               .gs26-tab-shell[data-expanded=\"true\"] .gs26-tab-toggle {{
                 grid-column:1;
               }}
               .gs26-tab-shell[data-expanded=\"true\"] .gs26-tab-nav {{
                 grid-column:1;
               }}
               .gs26-tab-toggle {{
                 display:inline-flex;
                 align-items:center;
                 justify-content:center;
                 font:inherit;
                 width:100%;
                 max-width:100%;
                 align-self:center;
                 justify-self:stretch;
                 text-align:center;
                 line-height:1.2;
                 white-space:normal;
                 overflow-wrap:anywhere;
                 word-break:break-word;
                 padding:0.22rem 0.55rem 0.26rem 0.55rem;
                 border-radius:0.65rem;
                 border:1px solid var(--gs26-header-menu-border);
                 background:var(--gs26-header-menu-background);
                 color:var(--gs26-header-menu-text);
                 font-weight:800;
                 font-size:0.8rem;
                 cursor:pointer;
               }}
               .gs26-tab-nav {{
                 display:none;
                 width:auto;
               }}
               .gs26-tab-shell[data-expanded=\"true\"] .gs26-tab-nav {{
                 display:grid;
                 grid-template-columns:repeat(2, minmax(0, 1fr));
                 align-items:stretch;
                 justify-items:stretch;
                 justify-self:stretch;
                 width:100%;
                 gap:0.28rem;
                 margin-top:0;
               }}
               .gs26-tab-shell[data-expanded=\"true\"] .gs26-tab-nav button {{
                 display:flex !important;
                 width:100%;
                 max-width:100%;
                 min-width:0;
                 justify-content:center !important;
                 align-items:center !important;
                 text-align:center !important;
                 padding:0.22rem 0.55rem 0.26rem 0.55rem !important;
                 margin-left:0;
                 margin-right:0;
               }}
               .gs26-tab-shell[data-expanded=\"true\"] .gs26-tab-nav button span[data-active=\"false\"] {{
                 display:none !important;
               }}
             }}
             @media (max-width: 360px) {{
               .gs26-tab-shell[data-expanded=\"true\"] .gs26-tab-nav {{
                 grid-template-columns:1fr;
               }}
             }}
             .gs26-tab-toggle {{ display:none !important; }}
             .gs26-tab-shell .gs26-tab-nav {{ display:flex !important; flex-wrap:nowrap !important; overflow-x:auto; width:100%; max-width:100%; }}
             .gs26-tab-shell .gs26-tab-nav button {{ width:auto !important; flex:0 0 auto; white-space:nowrap; }}"
                }
                if layout_loading_snapshot && layout_snapshot.is_none() {
                    div {
                        style: "
                    height:var(--gs26-app-height);
                    padding:clamp(8px, 2.5vw, 24px);
                    color:var(--gs26-app-text);
                    font-family:system-ui, -apple-system, BlinkMacSystemFont;
                    background:var(--gs26-app-background);
                    display:flex;
                    align-items:center;
                    justify-content:center;
                    border:{border_style};
                    --gs26-alert-frame-shadow:none;
                    box-sizing:border-box;
                ",
                        div { style: "text-align:center; display:flex; flex-direction:column; gap:10px; align-items:center;",
                            div { style: "font-size:22px; font-weight:800; color:{theme.info_accent};", "Loading layout..." }
                            div { style: "font-size:14px; color:{theme.text_muted};", "Waiting for layout from Ground Station" }
                            div { style: "display:flex; gap:10px; flex-wrap:wrap; justify-content:center; margin-top:4px;",
                                {version_button}
                                {connect_button}
                            }
                        }
                    }
                } else if layout_snapshot.is_none() {
                    div {
                        style: "
                    height:var(--gs26-app-height);
                    padding:clamp(8px, 2.5vw, 24px);
                    color:var(--gs26-app-text);
                    font-family:system-ui, -apple-system, BlinkMacSystemFont;
                    background:var(--gs26-app-background);
                    display:flex;
                    align-items:center;
                    justify-content:center;
                    border:{border_style};
                    --gs26-alert-frame-shadow:none;
                    box-sizing:border-box;
                ",
                        div { style: "text-align:center; display:flex; flex-direction:column; gap:12px; align-items:center;",
                            div { style: "font-size:20px; font-weight:800; color:{theme.error_text};", "Dashboard layout unavailable" }
                            if let Some(msg) = layout_error_snapshot.clone() {
                                div { style: "font-size:13px; color:{theme.text_muted};", "{msg}" }
                            }
                            div { style: "display:flex; gap:10px; flex-wrap:wrap; justify-content:center;",
                                {reload_button}
                                {version_button}
                                {connect_button}
                            }
                        }
                    }
                } else if let Some(layout) = layout_snapshot {
                div {
                    class: "gs26-dashboard-shell",
                    "data-streamer": if *streamer_mode.read() { "true" } else { "false" },

                    style: "
                height:var(--gs26-app-height);
                padding:clamp(8px, 2.5vw, 24px);
                color:var(--gs26-app-text);
                font-family:system-ui, -apple-system, BlinkMacSystemFont;
                background:var(--gs26-app-background);
                display:flex;
                flex-direction:column;
                width:100%;
                max-width:100%;
                border:{border_style};
                --gs26-alert-frame-shadow:{app_alert_effect};
                box-shadow:{app_alert_effect};
                {app_alert_animation}
                box-sizing:border-box;
                overflow:hidden;
            ",

                    if *streamer_mode.read() {
                        button { title:"Exit streamer mode", style:"position:fixed;top:8px;right:8px;z-index:1000;opacity:0.65;border:1px solid #526071;border-radius:8px;background:#101923;color:white;padding:6px;cursor:pointer;",
                            onclick:move |_| {let mut streamer_mode=streamer_mode;streamer_mode.set(false);let mut active_main_tab=active_main_tab;active_main_tab.set(MainTab::State);}, "Exit streamer"
                        }
                    }
                    if !*streamer_mode.read() {
                    // Header row 1
                    div {
                        class: "gs26-header-row",
                        style: "
                    display:flex;
                    align-items:center;
                    justify-content:space-between;
                    gap:16px;
                    width:100%;
                    margin-bottom:12px;
                    flex-wrap:wrap;
                    position:relative;
                    z-index:2000;
                ",
                        h1 {
                            class: "gs26-header-title",
                            style: "color:{theme.info_accent}; margin:0; font-size:22px; font-weight:800;",
                            "{_dashboard_title(&layout)}"
                        }

                        {
                            let show_disable_actions = _actions_tab_has_visible_actions(&layout, *abort_only_mode.read());
                            rsx! {
                        div {
                            class: "gs26-header-actions-shell",
                            "data-expanded": if *header_actions_expanded.read() { "true" } else { "false" },
                            style: "
                                margin-left:auto;
                                --gs26-header-menu-background:{theme.button_background};
                                --gs26-header-menu-border:{theme.button_border};
                                --gs26-header-menu-text:{theme.button_text};
                            ",
                            button {
                                class: "gs26-header-menu-toggle",
                                onclick: {
                                    let mut header_actions_expanded = header_actions_expanded;
                                    move |_| {
                                        let next = {
                                            let current = *header_actions_expanded.read();
                                            !current
                                        };
                                        header_actions_expanded.set(next);
                                    }
                                },
                                {if *header_actions_expanded.read() { translate_text("Close menu") } else { translate_text("Menu") }}
                            }
                            {
                                let abort_control = action_policy
                                    .read()
                                    .controls
                                    .iter()
                                    .find(|c| c.cmd == "Abort")
                                    .cloned();
                                let abort_visible = auth::can_send_command("Abort");
                                let abort_allowed =
                                    abort_visible && action_policy_control_enabled(&action_policy.read(), "Abort");
                                let abort_active = abort_control
                                    .as_ref()
                                    .and_then(|c| c.actuated)
                                    .unwrap_or(false)
                                    || command_feedback_active("Abort");
                                let abort_style = if abort_allowed {
                                    if abort_active {
                                        "
                                margin-left:clamp(20px, 6vw, 96px);
                                padding:0.45rem 0.85rem;
                                border-radius:0.75rem;
                                border:1px solid #fca5a5;
                                background:#7f1d1d;
                                color:#fee2e2;
                                box-shadow:0 0 0 1px rgba(252,165,165,0.3), 0 10px 28px rgba(127,29,29,0.5);
                                font-weight:900;
                                cursor:pointer;
                            "
                                    } else {
                                        "
                                margin-left:clamp(20px, 6vw, 96px);
                                padding:0.45rem 0.85rem;
                                border-radius:0.75rem;
                                border:1px solid #ef4444;
                                background:#450a0a;
                                color:#fecaca;
                                box-shadow:0 0 0 1px rgba(239,68,68,0.16), 0 10px 24px rgba(69,10,10,0.35);
                                font-weight:900;
                                cursor:pointer;
                            "
                                    }
                                } else {
                                    "
                                margin-left:clamp(20px, 6vw, 96px);
                                padding:0.45rem 0.85rem;
                                border-radius:0.75rem;
                                border:1px solid #991b1b;
                                background:#2b0b0b;
                                color:#fca5a5;
                                font-weight:900;
                                cursor:not-allowed;
                                opacity:0.72;
                                pointer-events:none;
                            "
                                };
                                rsx! {
                                    if abort_visible {
                                        button {
                                            class: "gs26-header-abort-mobile",
                                            style: "{abort_style} touch-action:manipulation;",
                                            disabled: !abort_allowed,
                                            onmousedown: move |_| {
                                                if abort_allowed {
                                                    send_cmd_from_press("Abort");
                                                }
                                            },
                                            ontouchstart: move |_| {
                                                if abort_allowed {
                                                    send_cmd_from_press("Abort");
                                                }
                                            },
                                            onclick: move |_| {
                                                if abort_allowed {
                                                    send_cmd_from_click("Abort");
                                                }
                                            },
                                            span { style: "display:inline-flex; align-items:center; gap:8px;",
                                                span { "{translate_text(\"ABORT\")}" }
                                                if !abort_allowed {
                                                    span {
                                                        style: "flex:0 0 auto; padding:0.14rem 0.42rem; border-radius:999px; border:1px solid rgba(255,255,255,0.16); background:rgba(0,0,0,0.18); color:rgba(255,255,255,0.82); font-size:0.68rem; font-weight:800; line-height:1; text-transform:uppercase; letter-spacing:0.04em;",
                                                        "{translate_text(\"Disabled\")}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        div { class: "gs26-header-actions-list",
                            if show_disable_actions {
                            button {
                                style: if *abort_only_mode.read() {
                                    "
                                        padding:0.45rem 0.85rem;
                                        border-radius:0.75rem;
                                        border:1px solid {theme.error_border};
                                        background:{theme.error_background};
                                        color:{theme.error_text};
                                        box-shadow:0 0 0 1px rgba(239,68,68,0.15), 0 8px 20px rgba(76,5,25,0.35);
                                        font-weight:800;
                                        cursor:pointer;
                                    "
                                } else {
                                    "
                                        padding:0.45rem 0.85rem;
                                        border-radius:0.75rem;
                                        border:1px solid {theme.button_border};
                                        background:{theme.button_background};
                                        color:{theme.button_text};
                                        font-weight:800;
                                        cursor:pointer;
                                    "
                                },
                                onclick: {
                                    let mut abort_only_mode = abort_only_mode;
                                    let mut header_actions_expanded = header_actions_expanded;
                                    move |_| {
                                        let next = {
                                            let current = *abort_only_mode.read();
                                            !current
                                        };
                                        abort_only_mode.set(next);
                                        header_actions_expanded.set(false);
                                    }
                                },
                                "{disable_actions_label}"
                            }
                            }

                            {reload_button}
                            {settings_button}
                            {auth_button}
                            {version_button}
                            {connect_button}

                            {
                                let abort_control = action_policy
                                    .read()
                                    .controls
                                    .iter()
                                    .find(|c| c.cmd == "Abort")
                                    .cloned();
                                let abort_visible = auth::can_send_command("Abort");
                                let abort_allowed =
                                    abort_visible && action_policy_control_enabled(&action_policy.read(), "Abort");
                                let abort_active = abort_control
                                    .as_ref()
                                    .and_then(|c| c.actuated)
                                    .unwrap_or(false)
                                    || command_feedback_active("Abort");
                                let abort_style = if abort_allowed {
                                    if abort_active {
                                        "
                                margin-left:clamp(20px, 6vw, 96px);
                                padding:0.45rem 0.85rem;
                                border-radius:0.75rem;
                                border:1px solid #fca5a5;
                                background:#7f1d1d;
                                color:#fee2e2;
                                box-shadow:0 0 0 1px rgba(252,165,165,0.3), 0 10px 28px rgba(127,29,29,0.5);
                                font-weight:900;
                                cursor:pointer;
                            "
                                    } else {
                                        "
                                margin-left:clamp(20px, 6vw, 96px);
                                padding:0.45rem 0.85rem;
                                border-radius:0.75rem;
                                border:1px solid #ef4444;
                                background:#450a0a;
                                color:#fecaca;
                                box-shadow:0 0 0 1px rgba(239,68,68,0.16), 0 10px 24px rgba(69,10,10,0.35);
                                font-weight:900;
                                cursor:pointer;
                            "
                                    }
                                } else {
                                    "
                                margin-left:clamp(20px, 6vw, 96px);
                                padding:0.45rem 0.85rem;
                                border-radius:0.75rem;
                                border:1px solid #991b1b;
                                background:#2b0b0b;
                                color:#fca5a5;
                                font-weight:900;
                                cursor:not-allowed;
                                opacity:0.72;
                                pointer-events:none;
                            "
                                };
                                rsx! {
                                    if abort_visible {
                                        button {
                                            class: "gs26-header-abort-menu",
                                            style: "{abort_style} touch-action:manipulation;",
                                            disabled: !abort_allowed,
                                            onmousedown: {
                                                let mut header_actions_expanded = header_actions_expanded;
                                                move |_| {
                                                    if abort_allowed {
                                                        send_cmd_from_press("Abort");
                                                    }
                                                    header_actions_expanded.set(false);
                                                }
                                            },
                                            ontouchstart: {
                                                let mut header_actions_expanded = header_actions_expanded;
                                                move |_| {
                                                    if abort_allowed {
                                                        send_cmd_from_press("Abort");
                                                    }
                                                    header_actions_expanded.set(false);
                                                }
                                            },
                                            onclick: {
                                                let mut header_actions_expanded = header_actions_expanded;
                                                move |_| {
                                                    if abort_allowed {
                                                        send_cmd_from_click("Abort");
                                                    }
                                                    header_actions_expanded.set(false);
                                                }
                                            },
                                            span { style: "display:inline-flex; align-items:center; gap:8px;",
                                                span { "{translate_text(\"ABORT\")}" }
                                                if !abort_allowed {
                                                    span {
                                                        style: "flex:0 0 auto; padding:0.14rem 0.42rem; border-radius:999px; border:1px solid rgba(255,255,255,0.16); background:rgba(0,0,0,0.18); color:rgba(255,255,255,0.82); font-size:0.68rem; font-weight:800; line-height:1; text-transform:uppercase; letter-spacing:0.04em;",
                                                        "{translate_text(\"Disabled\")}"
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        }
                            }
                        }
                    }

                    if let Some(msg) = layout_cached_error_banner.clone() {
                        div { style: "margin-bottom:12px; padding:10px 12px; border-radius:10px; border:1px solid {theme.error_border}; background:{theme.error_background}; color:{theme.error_text}; font-size:12px; display:flex; align-items:center; gap:10px; flex-wrap:wrap;",
                            span { style: "flex:1 1 220px; min-width:0;", "{msg}" }
                            button {
                                style: format!("
                                    padding:0.3rem 0.7rem;
                                    border-radius:0.6rem;
                                    border:1px solid {};
                                    background:{};
                                    color:{};
                                    font-weight:800;
                                    cursor:pointer;
                                ", theme.button_border, theme.button_background, theme.button_text),
                                onclick: {
                                    let mut layout_error_dismissed = layout_error_dismissed;
                                    let msg = msg.clone();
                                    move |_| {
                                        layout_error_dismissed.set(Some(msg.clone()));
                                    }
                                },
                                "Dismiss"
                            }
                        }
                    }

                    if !action_policy.read().software_buttons_enabled {
                        div { style: "margin-bottom:12px; padding:10px 12px; border-radius:10px; border:1px solid {theme.warning_border}; background:{theme.warning_background}; color:{theme.warning_text}; font-size:12px;",
                            "{action_buttons_disabled_message(&action_policy.read())}"
                        }
                    }
                    // Header row 2
                    div {
                        class: "gs26-header-secondary",

                        div {
                            class: "gs26-tab-shell",
                            "data-expanded": if *tabs_expanded.read() { "true" } else { "false" },
                            style: "
                        flex:1 1 100%;
                        width:100%;
                        max-width:100%;
                        --gs26-header-menu-background:{theme.button_background};
                        --gs26-header-menu-border:{theme.button_border};
                        --gs26-header-menu-text:{theme.button_text};
                        display:flex;
                        flex-wrap:wrap;
                        align-items:center;
                        padding:0.85rem;
                        border-radius:0.75rem;
                        background:{theme.tab_shell_background};
                        border:1px solid {theme.tab_shell_border};
                        box-shadow:0 10e0px 25px rgba(0,0,0,0.45);
                        min-width:0;
                    ",
                            button {
                                class: "gs26-tab-toggle",
                                onclick: {
                                    let mut tabs_expanded = tabs_expanded;
                                    move |_| {
                                        let next = {
                                            let current = *tabs_expanded.read();
                                            !current
                                        };
                                        tabs_expanded.set(next);
                                    }
                                },
                                {
                                if *tabs_expanded.read() {
                                    "Hide tabs".to_string()
                                } else {
                                    format!("Show tabs ({})", _main_tab_label(&layout, *active_main_tab.read()))
                                }
                                }
                            }
                            button {
                                class: "gs26-tab-toggle",
                                title: "Customize tab visibility and order for this Ground Station",
                                onclick: {
                                    let mut dashboard_edit_mode = dashboard_edit_mode;
                                    move |_| {
                                        let next = !*dashboard_edit_mode.read();
                                        dashboard_edit_mode.set(next);
                                    }
                                },
                                if *dashboard_edit_mode.read() { "Done editing" } else { "Edit layout" }
                            }
                            nav { class: "gs26-tab-nav",
                                for tab in _configured_main_tabs(&layout, *abort_only_mode.read(), *calibration_has_sensors.read(), &dashboard_customization.read()).into_iter() {
                                    match tab {
                                        MainTab::State => rsx! {
                                            button {
                                                key: "{\"main-tab-state\"}",
                                                style: if *active_main_tab.read() == MainTab::State { tab_style_active(&main_tab_accent("state", "#38bdf8")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| {
                                                        t.set(MainTab::State);
                                                        tabs_expanded.set(false);
                                                    }
                                                },
                                                "{_main_tab_label(&layout, MainTab::State)}"
                                            }
                                        },
                                        MainTab::ConnectionStatus => rsx! {
                                            button {
                                                key: "{\"main-tab-connection-status\"}",
                                                style: if *active_main_tab.read() == MainTab::ConnectionStatus { tab_style_active(&main_tab_accent("connection-status", "#06b6d4")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| {
                                                        t.set(MainTab::ConnectionStatus);
                                                        tabs_expanded.set(false);
                                                    }
                                                },
                                                "{_main_tab_label(&layout, MainTab::ConnectionStatus)}"
                                            }
                                        },
                                        MainTab::Detailed => rsx! {
                                            button {
                                                key: "{\"main-tab-detailed\"}",
                                                style: if *active_main_tab.read() == MainTab::Detailed { tab_style_active(&main_tab_accent("detailed", "#0ea5e9")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| {
                                                        t.set(MainTab::Detailed);
                                                        tabs_expanded.set(false);
                                                    }
                                                },
                                                "{_main_tab_label(&layout, MainTab::Detailed)}"
                                            }
                                        },
                                        MainTab::Map => rsx! {
                                            button {
                                                key: "{\"main-tab-map\"}",
                                                style: if *active_main_tab.read() == MainTab::Map { tab_style_active(&main_tab_accent("map", "#22c55e")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| {
                                                        t.set(MainTab::Map);
                                                        tabs_expanded.set(false);
                                                    }
                                                },
                                                "{_main_tab_label(&layout, MainTab::Map)}"
                                            }
                                        },
                                        MainTab::Actions => rsx! {
                                            button {
                                                key: "{\"main-tab-actions\"}",
                                                style: if *active_main_tab.read() == MainTab::Actions { tab_style_active(&main_tab_accent("actions", "#a78bfa")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| {
                                                        t.set(MainTab::Actions);
                                                        tabs_expanded.set(false);
                                                    }
                                                },
                                                "{_main_tab_label(&layout, MainTab::Actions)}"
                                            }
                                        },
                                        MainTab::FirmwareUpdate => rsx! {
                                            button {
                                                key: "{\"main-tab-firmware-update\"}",
                                                style: if *active_main_tab.read() == MainTab::FirmwareUpdate { tab_style_active(&main_tab_accent("firmware-update", "#22d3ee")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| {
                                                        t.set(MainTab::FirmwareUpdate);
                                                        tabs_expanded.set(false);
                                                    }
                                                },
                                                "{_main_tab_label(&layout, MainTab::FirmwareUpdate)}"
                                            }
                                        },
                                        MainTab::Calibration => rsx! {
                                            button {
                                                key: "{\"main-tab-calibration\"}",
                                                style: if *active_main_tab.read() == MainTab::Calibration { tab_style_active(&main_tab_accent("calibration", "#14b8a6")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| {
                                                        t.set(MainTab::Calibration);
                                                        tabs_expanded.set(false);
                                                    }
                                                },
                                                "{_main_tab_label(&layout, MainTab::Calibration)}"
                                            }
                                        },
                                        MainTab::Mission => rsx! {
                                            button {
                                                key: "{\"main-tab-mission\"}",
                                                style: if *active_main_tab.read() == MainTab::Mission { tab_style_active(&main_tab_accent("mission", "#ef4444")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| { t.set(MainTab::Mission); tabs_expanded.set(false); }
                                                },
                                                "{_main_tab_label(&layout, MainTab::Mission)}"
                                            }
                                        },
                                        MainTab::Vehicle => rsx! {
                                            button {
                                                key: "{\"main-tab-vehicle\"}",
                                                style: if *active_main_tab.read() == MainTab::Vehicle { tab_style_active(&main_tab_accent("vehicle", "#38bdf8")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| { t.set(MainTab::Vehicle); tabs_expanded.set(false); }
                                                },
                                                "{_main_tab_label(&layout, MainTab::Vehicle)}"
                                            }
                                        },
                                        MainTab::Messages => rsx! {
                                            button {
                                                key: "{\"main-tab-messages\"}",
                                                style: if *active_main_tab.read() == MainTab::Messages { tab_style_active(&main_tab_accent("messages", "#2563eb")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| {
                                                        t.set(MainTab::Messages);
                                                        tabs_expanded.set(false);
                                                    }
                                                },
                                                "{_main_tab_label(&layout, MainTab::Messages)}"
                                            }
                                        },
                                        MainTab::Notifications => rsx! {
                                            button {
                                                key: "{\"main-tab-notifications\"}",
                                                style: if *active_main_tab.read() == MainTab::Notifications { tab_style_active(&main_tab_accent("notifications", "#3b82f6")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    let notifications = notifications;
                                                    let dismissed_notifications = dismissed_notifications;
                                                    let unread_notification_ids = unread_notification_ids;
                                                    move |_| {
                                                        t.set(MainTab::Notifications);
                                                        tabs_expanded.set(false);
                                                        dismiss_all_active_notifications_local_and_remote(
                                                            notifications,
                                                            dismissed_notifications,
                                                            unread_notification_ids,
                                                        );
                                                    }
                                                },
                                                span { "{_main_tab_label(&layout, MainTab::Notifications)}" }
                                                span {
                                                    "data-active": if has_unread_notifications { "true" } else { "false" },
                                                    style: "{notifications_tab_icon_style}",
                                                    "●"
                                                }
                                            }
                                        },
                                        MainTab::Warnings => rsx! {
                                            button {
                                                key: "{\"main-tab-warnings\"}",
                                                style: if *active_main_tab.read() == MainTab::Warnings { tab_style_active(&main_tab_accent("warnings", "#facc15")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| {
                                                        t.set(MainTab::Warnings);
                                                        tabs_expanded.set(false);
                                                    }
                                                },
                                                span { "{_main_tab_label(&layout, MainTab::Warnings)}" }
                                                span {
                                                    "data-active": if has_warnings { "true" } else { "false" },
                                                    style: "{warnings_tab_icon_style}",
                                                    "⚠"
                                                }
                                            }
                                        },
                                        MainTab::Errors => rsx! {
                                            button {
                                                key: "{\"main-tab-errors\"}",
                                                style: if *active_main_tab.read() == MainTab::Errors { tab_style_active(&main_tab_accent("errors", "#ef4444")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| {
                                                        t.set(MainTab::Errors);
                                                        tabs_expanded.set(false);
                                                    }
                                                },
                                                span { "{_main_tab_label(&layout, MainTab::Errors)}" }
                                                span {
                                                    "data-active": if has_errors { "true" } else { "false" },
                                                    style: "{errors_tab_icon_style}",
                                                    "⛔"
                                                }
                                            }
                                        },
                                        MainTab::Data => rsx! {
                                            button {
                                                key: "{\"main-tab-data\"}",
                                                style: if *active_main_tab.read() == MainTab::Data { tab_style_active(&main_tab_accent("data", "#f97316")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| {
                                                        t.set(MainTab::Data);
                                                        tabs_expanded.set(false);
                                                    }
                                                },
                                                "{_main_tab_label(&layout, MainTab::Data)}"
                                            }
                                        },
                                        MainTab::NetworkTopology => rsx! {
                                            button {
                                                key: "{\"main-tab-network-topology\"}",
                                                style: if *active_main_tab.read() == MainTab::NetworkTopology { tab_style_active(&main_tab_accent("network-topology", "#8b5cf6")) } else { tab_style_inactive.to_string() },
                                                onclick: {
                                                    let mut t = active_main_tab;
                                                    let mut tabs_expanded = tabs_expanded;
                                                    move |_| {
                                                        t.set(MainTab::NetworkTopology);
                                                        tabs_expanded.set(false);
                                                    }
                                                },
                                                "{_main_tab_label(&layout, MainTab::NetworkTopology)}"
                                            }
                                        },
                    }
                }
                if *dashboard_edit_mode.read() {
                    div {
                        style: "flex:1 0 100%; display:flex; flex-direction:column; gap:8px; margin-top:10px; padding:10px; box-sizing:border-box; border:1px dashed {theme.info_accent}; border-radius:12px; background:{theme.info_background};",
                        div { style: "display:flex; align-items:center; justify-content:space-between; gap:8px; flex-wrap:wrap;",
                            div {
                                div { style: "font-size:13px; font-weight:800; color:{theme.info_text};", "Dashboard layout" }
                                div { style: "font-size:11px; color:{theme.text_muted};", "Changes are saved only for this Ground Station. Backend layout remains the default." }
                            }
                            button {
                                style: "padding:5px 9px; border:1px solid {theme.button_border}; border-radius:9px; background:{theme.button_background}; color:{theme.button_text}; cursor:pointer;",
                                onclick: {
                                    let mut dashboard_customization = dashboard_customization;
                                    move |_| {
                                        let next = DashboardCustomization::default();
                                        save_dashboard_customization(&next);
                                        dashboard_customization.set(next);
                                    }
                                },
                                "Use Ground Station default"
                            }
                        }
                        div { style: "display:grid; grid-template-columns:repeat(auto-fill,minmax(220px,1fr)); gap:6px;",
                            for (index, tab) in _available_main_tabs(&layout, *abort_only_mode.read(), *calibration_has_sensors.read()).into_iter().enumerate() {
                                {
                                    let tab_id = _main_tab_to_str(tab).to_string();
                                    let visible = !dashboard_customization.read().hidden.contains(&tab_id);
                                    rsx! {
                                        div { style: "display:grid; grid-template-columns:minmax(0,1fr) auto auto; align-items:center; gap:5px; padding:7px; border:1px solid {theme.border_soft}; border-radius:10px; background:{theme.panel_background_alt};",
                                            label { style: "display:flex; align-items:center; gap:7px; min-width:0; font-size:12px;",
                                                input {
                                                    r#type: "checkbox",
                                                    checked: visible,
                                                    onchange: {
                                                        let mut dashboard_customization = dashboard_customization;
                                                        let tab_id = tab_id.clone();
                                                        move |event| {
                                                            let mut next = dashboard_customization.read().clone();
                                                            next.hidden.retain(|id| id != &tab_id);
                                                            if !event.checked() { next.hidden.push(tab_id.clone()); }
                                                            save_dashboard_customization(&next);
                                                            dashboard_customization.set(next);
                                                        }
                                                    }
                                                }
                                                span { style: "overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", "{_main_tab_label(&layout, tab)}" }
                                            }
                                            button {
                                                title: "Move earlier",
                                                disabled: index == 0,
                                                style: "padding:3px 7px; border:1px solid {theme.button_border}; border-radius:7px; background:{theme.button_background}; color:{theme.button_text}; cursor:pointer;",
                                                onclick: {
                                                    let mut dashboard_customization = dashboard_customization;
                                                    let available = _available_main_tabs(&layout, *abort_only_mode.read(), *calibration_has_sensors.read());
                                                    let tab_id = tab_id.clone();
                                                    move |_| {
                                                        let mut next = dashboard_customization.read().clone();
                                                        let mut order = available.iter().map(|tab| _main_tab_to_str(*tab).to_string()).collect::<Vec<_>>();
                                                        if !next.order.is_empty() { order.sort_by_key(|id| next.order.iter().position(|saved| saved == id).unwrap_or(usize::MAX)); }
                                                        if let Some(pos) = order.iter().position(|id| id == &tab_id) && pos > 0 { order.swap(pos, pos - 1); }
                                                        next.order = order;
                                                        save_dashboard_customization(&next);
                                                        dashboard_customization.set(next);
                                                    }
                                                },
                                                "↑"
                                            }
                                            button {
                                                title: "Move later",
                                                style: "padding:3px 7px; border:1px solid {theme.button_border}; border-radius:7px; background:{theme.button_background}; color:{theme.button_text}; cursor:pointer;",
                                                onclick: {
                                                    let mut dashboard_customization = dashboard_customization;
                                                    let available = _available_main_tabs(&layout, *abort_only_mode.read(), *calibration_has_sensors.read());
                                                    let tab_id = tab_id.clone();
                                                    move |_| {
                                                        let mut next = dashboard_customization.read().clone();
                                                        let mut order = available.iter().map(|tab| _main_tab_to_str(*tab).to_string()).collect::<Vec<_>>();
                                                        if !next.order.is_empty() { order.sort_by_key(|id| next.order.iter().position(|saved| saved == id).unwrap_or(usize::MAX)); }
                                                        if let Some(pos) = order.iter().position(|id| id == &tab_id) && pos + 1 < order.len() { order.swap(pos, pos + 1); }
                                                        next.order = order;
                                                        save_dashboard_customization(&next);
                                                        dashboard_customization.set(next);
                                                    }
                                                },
                                                "↓"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
        }
    }

                        div {
                            class: "gs26-status-shell",
                            style: "background:{theme.button_background}; border:1px solid {theme.tab_shell_border};",
                            div { class: "gs26-status-row",
                                span { style: "color:{theme.text_soft};", {localized_copy(&language_snapshot, "Status:", "Estado:", "Statut:")} }
                                span { class: "gs26-status-value", style: "{status_label_style}", "{status_label}" }
                                span {
                                    class: "gs26-status-count",
                                    "data-active": if has_errors { "true" } else { "false" },
                                    style: "{errors_status_style}",
                                    {format!("{}: {}", translate_text("Errors"), err_count_label)}
                                }
                                span {
                                    class: "gs26-status-count",
                                    "data-active": if has_warnings { "true" } else { "false" },
                                    style: "{warnings_status_style}",
                                    {format!("{}: {}", translate_text("Warnings"), warn_count_label)}
                                }
                            }
                            div { class: "gs26-status-flight", style: "color:{theme.info_text};",
                                span { style: "color:{theme.text_soft}; flex:0 0 auto;", "{localized_copy(&language_snapshot, \"Flight state\", \"Estado de vuelo\", \"Etat de vol\")}: " }
                                span {
                                    style: "display:inline-flex; align-items:baseline; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;",
                                    "{translate_text(&display_flight_state(&flight_state.read()))}"
                                }
                            }
                            div { class: "gs26-status-network",
                                NetworkTimeBadge { network_time: network_time, language: language_snapshot.clone() }
                            }
                            div { class: "gs26-status-launch",
                                LaunchClockBadge { launch_clock: launch_clock, network_time: network_time }
                            }
                        }
                    }

                    }
                    div { style: "flex:1 1 auto; min-height:0; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow:hidden;",
                        match if *streamer_mode.read() {MainTab::Mission}else{*active_main_tab.read()} {
                            MainTab::State if !*ground_station_view.read() => rsx! {
                                model_dashboard::ModelDashboard { theme:theme.clone(), action_policy, abort_only_mode:*abort_only_mode.read(), flight_state, rocket_gps, rocket_altitude_m:rocket_gps_altitude_m }
                            },
                            MainTab::State => rsx! {
                                div { style: "height:100%; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow-y:auto; overflow-x:hidden; -webkit-overflow-scrolling:auto;",
                                        StateTab {
                                            flight_state: flight_state,
                                            board_status: board_status,
                                            rocket_gps: rocket_gps,
                                            user_gps: user_gps,
                                            fill_targets: fill_targets,
                                            layout: layout.state_tab.clone(),
                                            data_layout: layout.data_tab.clone(),
                                            actions: layout.actions_tab.clone(),
                                            action_policy: action_policy,
                                            default_valve_labels: None,
                                            abort_only_mode: *abort_only_mode.read(),
                                            state_chart_labels_vertical: *state_chart_labels_vertical.read(),
                                            theme: theme.clone(),
                                            use_layout_theme_overrides: use_layout_theme_overrides,
                                        }
                                    }
                            },
                            MainTab::ConnectionStatus => rsx! {
                                div {
                                    key: "connection-status-clear-{frontend_data_clear_epoch}",
                                    style: "height:100%; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow:hidden;",
                                    ConnectionStatusTab {
                                        boards: board_status,
                                        ws_connected: *WS_CONNECTED_SIGNAL.read(),
                                        expected_boards: layout.network_tab.expected_boards.clone(),
                                        layout: layout.connection_tab.clone(),
                                        title: _main_tab_label(&layout, MainTab::ConnectionStatus),
                                        theme: theme.clone(),
                                    }
                                }
                            },
                            MainTab::Detailed => rsx! {
                                DetailedTab {
                                    metrics: frontend_network_metrics,
                                    ws_connected: *WS_CONNECTED_SIGNAL.read(),
                                    board_status: board_status,
                                    network_topology: network_topology,
                                    flight_state: flight_state,
                                    rocket_gps: rocket_gps,
                                    user_gps: user_gps,
                                    rocket_altitude_m: rocket_gps_altitude_m,
                                    user_altitude_m: user_gps_altitude_m,
                                    distance_units_metric: *distance_units_metric.read(),
                                    warnings: warnings,
                                    errors: errors,
                                    notifications: notifications,
                                    network_time: network_time,
                                    cache_stats: cache_storage_stats_rows(),
                                    theme: theme.clone(),
                                }
                            },
                            MainTab::NetworkTopology => rsx! {
                                div { key: "network-topology-clear-{frontend_data_clear_epoch}", style: "height:100%; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow:hidden;",
                                    NetworkTopologyTab {
                                        topology: network_topology,
                                        ws_connected: *WS_CONNECTED_SIGNAL.read(),
                                        layout: layout.network_tab.clone(),
                                        flow_animation_enabled: *network_flow_animation_enabled.read(),
                                        vertical_layout: *network_topology_vertical.read(),
                                        theme: theme.clone(),
                                    }
                                }
                            },
                            MainTab::Map => rsx! {
                                MapTab {
                                    rocket_gps: rocket_gps,
                                    user_gps: user_gps,
                                    user_location_manual: *user_location_manual.read(),
                                    user_heading_manual: *user_heading_manual.read(),
                                    rocket_altitude_m: Some(rocket_gps_altitude_m),
                                    user_altitude_m: Some(user_gps_altitude_m),
                                    show_header_distance: *map_header_distance_visible.read(),
                                    show_header_altitude: *map_header_altitude_visible.read(),
                                    distance_units_metric: *distance_units_metric.read(),
                                    theme: theme.clone(),
                                    title: _main_tab_label(&layout, MainTab::Map),
                                }
                            },
                            MainTab::Actions => rsx! {
                                div { style: "height:100%; width:100%; max-width:100%; box-sizing:border-box; overflow-y:auto; overflow-x:hidden;",
                                ActionsTab {
                                    layout: layout.actions_tab.clone(),
                                    action_policy: action_policy,
                                    recording_status: recording_status,
                                    backend_fill_targets: fill_targets,
                                    abort_only_mode: *abort_only_mode.read(),
                                    theme: theme.clone(),
                                }
                                }
                            },
                            MainTab::FirmwareUpdate => rsx! {
                                FirmwareUpdateTab { theme: theme.clone() }
                            },
                            MainTab::Calibration => rsx! {
                                div { style: "height:100%; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow-y:auto; overflow-x:hidden;",
                                    CalibrationTab {
                                        theme: theme.clone(),
                                        can_edit: auth::can_edit_calibration(),
                                        capture_sample_count: *calibration_capture_sample_count.read(),
                                    }
                                }
                            },
                            MainTab::Mission => rsx! {
                                LiveStreamTab {
                                    theme: theme.clone(),
                                    program_only: *streamer_mode.read(),
                                    action_policy,
                                    abort_only_mode:*abort_only_mode.read(),
                                    flight_state,
                                    launch_clock,
                                    network_time,
                                    rocket_gps,
                                    rocket_altitude_m: rocket_gps_altitude_m,
                                }
                            },
                            MainTab::Vehicle => rsx! {
                                VehicleTab {
                                    theme: theme.clone(),
                                    flight_state,
                                    rocket_gps,
                                    rocket_altitude_m: rocket_gps_altitude_m,
                                }
                            },
                            MainTab::Messages => rsx! {
                                div { key: "messages-clear-{frontend_data_clear_epoch}", style: "height:100%; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow-y:auto; overflow-x:hidden;",
                                    MessagesTab {
                                        history: message_history,
                                        theme: theme.clone(),
                                    }
                                }
                            },
                            MainTab::Notifications => rsx! {
                                div { style: "height:100%; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow-y:auto; overflow-x:hidden;",
                                    NotificationsTab {
                                        history: notification_history,
                                        theme: theme.clone(),
                                        on_clear: {
                                            let notifications = notifications;
                                            let notification_history = notification_history;
                                            let dismissed_notifications = dismissed_notifications;
                                            let unread_notification_ids = unread_notification_ids;
                                            move |_| {
                                                clear_all_notifications_local_and_remote(
                                                    notifications,
                                                    notification_history,
                                                    dismissed_notifications,
                                                    unread_notification_ids,
                                                );
                                            }
                                        }
                                    }
                                }
                            },
                            MainTab::Warnings => rsx! {
                                div { style: "height:100%; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow-y:auto; overflow-x:hidden;",
                                    WarningsTab {
                                        warnings: warnings,
                                        ack_timestamp_ms: *ack_warning_ts.read(),
                                        theme: theme.clone(),
                                        on_ack: {
                                            let mut ack_warning_ts = ack_warning_ts;
                                            let mut ack_error_ts = ack_error_ts;
                                            let latest_warning_ts = latest_warning_ts;
                                            let latest_error_ts = latest_error_ts;
                                            let remote_alert_acks_enabled = remote_alert_acks_enabled;
                                            move |_| {
                                                if *remote_alert_acks_enabled.read() {
                                                    ack_warning_ts.set(latest_warning_ts);
                                                    ack_error_ts.set(latest_error_ts);
                                                    spawn(async move {
                                                        let _ = post_remote_alert_ack(
                                                            latest_warning_ts,
                                                            latest_error_ts,
                                                        )
                                                        .await;
                                                    });
                                                } else {
                                                    ack_warning_ts.set(latest_warning_ts);
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            MainTab::Errors => rsx! {
                                div { style: "height:100%; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow-y:auto; overflow-x:hidden;",
                                    ErrorsTab {
                                        errors: errors,
                                        ack_timestamp_ms: *ack_error_ts.read(),
                                        theme: theme.clone(),
                                        on_ack: {
                                            let mut ack_warning_ts = ack_warning_ts;
                                            let mut ack_error_ts = ack_error_ts;
                                            let latest_warning_ts = latest_warning_ts;
                                            let latest_error_ts = latest_error_ts;
                                            let remote_alert_acks_enabled = remote_alert_acks_enabled;
                                            move |_| {
                                                if *remote_alert_acks_enabled.read() {
                                                    ack_warning_ts.set(latest_warning_ts);
                                                    ack_error_ts.set(latest_error_ts);
                                                    spawn(async move {
                                                        let _ = post_remote_alert_ack(
                                                            latest_warning_ts,
                                                            latest_error_ts,
                                                        )
                                                        .await;
                                                    });
                                                } else {
                                                    ack_error_ts.set(latest_error_ts);
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            MainTab::Data => rsx! {
                                DataTab {
                                    active_tab: active_data_tab,
                                    layout: layout.data_tab.clone(),
                                    state_chart_labels_vertical: *state_chart_labels_vertical.read(),
                                    theme: theme.clone(),
                                }
                            },
                        }
                    }
                }
                }
                if *active_main_tab.read() != MainTab::Notifications && !notifications.read().is_empty() {
                    div {
                        style: "position:fixed; top:88px; right:16px; z-index:2000; width:min(420px, calc(100vw - 32px)); max-height:min(40vh, 280px); display:flex; flex-direction:column; gap:8px; overflow-y:auto; overflow-x:hidden; -webkit-overflow-scrolling:auto; pointer-events:none; font-family:{dashboard_font_stack};",
                        for n in notifications.read().iter() {
                            div {
                                style: "display:flex; align-items:center; gap:10px; padding:10px 12px; border:1px solid {theme.notification_border}; border-radius:10px; background:{theme.notification_background}; color:{theme.notification_text}; min-width:0; box-shadow:0 16px 40px rgba(0,0,0,0.35); pointer-events:auto; font-family:{dashboard_font_stack};",
                                span { style: "flex:1 1 auto; min-width:0; overflow-wrap:anywhere; word-break:break-word;", {translate_text(&n.message)} }
                                if let (Some(action_label), Some(action_cmd)) = (n.action_label.as_deref(), n.action_cmd.as_deref())
                                    && auth::can_send_command(action_cmd)
                                {
                                    button {
                                        style: "padding:0.2rem 0.65rem; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.75rem; font-family:{dashboard_font_stack}; cursor:pointer; touch-action:manipulation;",
                                        onmousedown: {
                                            let cmd = action_cmd.to_string();
                                            move |_| {
                                                send_cmd_from_press(&cmd);
                                            }
                                        },
                                        ontouchstart: {
                                            let cmd = action_cmd.to_string();
                                            move |_| {
                                                send_cmd_from_press(&cmd);
                                            }
                                        },
                                        onclick: {
                                            let cmd = action_cmd.to_string();
                                            move |_| {
                                                send_cmd_from_click(&cmd);
                                            }
                                        },
                                        {translate_text(action_label)}
                                    }
                                }
                                button {
                                    style: "padding:0.2rem 0.55rem; border-radius:999px; border:1px solid {theme.button_border}; background:{theme.button_background}; color:{theme.button_text}; font-size:0.75rem; font-family:{dashboard_font_stack}; cursor:pointer;",
                                    onclick: {
                                        let id = n.id;
                                        let ts = n.timestamp_ms;
                                        let mut notifications = notifications;
                                        let mut dismissed_notifications = dismissed_notifications;
                                        let mut unread_notification_ids = unread_notification_ids;
                                        move |_| {
                                            let mut v = notifications.read().clone();
                                            v.retain(|x| x.id != id);
                                            notifications.set(v);
                                            let mut unread = unread_notification_ids.read().clone();
                                            unread.retain(|x| *x != id);
                                            unread_notification_ids.set(unread);
                                            let mut ids = dismissed_notifications.read().clone();
                                            let item = DismissedNotification {
                                                id,
                                                timestamp_ms: ts,
                                            };
                                            if !ids.contains(&item) {
                                                ids.push(item);
                                                ids.sort_by_key(|x| (x.id, x.timestamp_ms));
                                                dismissed_notifications.set(ids.clone());
                                                persist_dismissed_notifications(&ids);
                                            }
                                            spawn_detached(async move {
                                                let _ = dismiss_notification_remote(id).await;
                                            });
                                        }
                                    },
                                    {translate_text("Dismiss")}
                                }
                            }
                        }
                    }
                }
                {settings_overlay}
                {version_overlay}
            }
}

fn send_cmd(cmd: &str) {
    if !action_policy_control_enabled(&ACTION_POLICY_SIGNAL.read(), cmd)
        || !auth::can_send_command(cmd)
    {
        log!("[CMD] blocked cmd='{cmd}'");
        return;
    }
    if let Some(sender) = WS_SENDER.read().clone()
        && let Err(e) = sender.send_cmd(cmd)
    {
        log!("[CMD] ws send failed for '{cmd}': {e}");
    } else {
        log!("[CMD] dispatched cmd='{cmd}'");
        schedule_action_policy_resync();
    }
}

fn schedule_action_policy_resync() {
    let mut epoch = ACTION_POLICY_RESYNC_EPOCH.write();
    *epoch = epoch.wrapping_add(1);
}

fn clear_command_feedback_latches() {
    if let Ok(mut pending) = PENDING_COMMAND_PRESS.lock() {
        pending.take();
    }
    if let Ok(mut last) = LAST_COMMAND_ACTIVATION.lock() {
        last.take();
    }
}

fn should_send_command_activation(cmd: &str) -> bool {
    let now = monotonic_now_ms();
    let Ok(mut last) = LAST_COMMAND_ACTIVATION.lock() else {
        return true;
    };
    if let Some((last_cmd, last_ts)) = last.as_ref()
        && last_cmd == cmd
        && now - *last_ts <= COMMAND_ACTIVATION_DEDUP_MS
    {
        return false;
    }
    *last = Some((cmd.to_string(), now));
    true
}

pub(crate) fn send_cmd_from_press(cmd: &str) {
    if !action_policy_control_enabled(&ACTION_POLICY_SIGNAL.read(), cmd)
        || !auth::can_send_command(cmd)
    {
        log!("[CMD] press ignored cmd='{cmd}'");
        return;
    }
    let Ok(mut pending) = PENDING_COMMAND_PRESS.lock() else {
        return;
    };
    *pending = Some((cmd.to_string(), monotonic_now_ms()));
}

pub(crate) fn command_feedback_active(cmd: &str) -> bool {
    let now = monotonic_now_ms();
    let pending_active = PENDING_COMMAND_PRESS
        .lock()
        .ok()
        .and_then(|pending| pending.clone())
        .is_some_and(|(pending_cmd, started_ms)| {
            pending_cmd == cmd && now - started_ms <= COMMAND_MAX_PRESS_RELEASE_MS
        });
    if pending_active {
        return true;
    }
    LAST_COMMAND_ACTIVATION
        .lock()
        .ok()
        .and_then(|last| last.clone())
        .is_some_and(|(last_cmd, last_ts)| {
            last_cmd == cmd && now - last_ts <= COMMAND_VISUAL_FEEDBACK_MS
        })
}

fn should_send_command_release(cmd: &str) -> bool {
    let now = monotonic_now_ms();
    let Ok(mut pending) = PENDING_COMMAND_PRESS.lock() else {
        return false;
    };
    let armed = pending.take().is_some_and(|(pending_cmd, started_ms)| {
        pending_cmd == cmd && now - started_ms <= COMMAND_MAX_PRESS_RELEASE_MS
    });
    drop(pending);

    armed && should_send_command_activation(cmd)
}

pub(crate) fn send_cmd_from_click(cmd: &str) {
    if !action_policy_control_enabled(&ACTION_POLICY_SIGNAL.read(), cmd)
        || !auth::can_send_command(cmd)
    {
        log!("[CMD] click ignored cmd='{cmd}'");
        let Ok(mut pending) = PENDING_COMMAND_PRESS.lock() else {
            return;
        };
        pending.take();
        return;
    }
    if should_send_command_release(cmd) || should_send_command_activation(cmd) {
        send_cmd(cmd);
    }
}

fn row_to_gps(row: &TelemetryRow) -> Option<(f64, f64)> {
    let is_gps_type = matches!(row.data_type.as_str(), "GPS" | "GPS_DATA" | "ROCKET_GPS");
    if !is_gps_type {
        return None;
    }
    Some((
        row.values.first().copied().flatten()? as f64,
        row.values.get(1).copied().flatten()? as f64,
    ))
}

fn row_to_gps_altitude_m(row: &TelemetryRow) -> Option<f64> {
    let is_gps_type = matches!(row.data_type.as_str(), "GPS" | "GPS_DATA" | "ROCKET_GPS");
    if !is_gps_type {
        return None;
    }
    row.values
        .get(2)
        .copied()
        .flatten()
        .map(|value| value as f64)
}

// ---------- Web vs Native logging ----------
fn log(msg: &str) {
    debug_log::append(msg);
}
