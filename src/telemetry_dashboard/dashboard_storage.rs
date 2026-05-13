// Dashboard storage and cache maintenance.
//
// These keys and helpers coordinate browser/native persistence, tile/data cache
// clearing, and reseed triggers. They are kept together because most callers
// need to reason about what survives reconnects and explicit data clears.

const WARNING_ACK_STORAGE_KEY: &str = "gs_last_warning_ack_ts";
const ERROR_ACK_STORAGE_KEY: &str = "gs_last_error_ack_ts";
const MAIN_TAB_STORAGE_KEY: &str = "gs_main_tab";
const DATA_TAB_STORAGE_KEY: &str = "gs_data_tab";
const BASE_URL_STORAGE_KEY: &str = "gs_base_url";
const MAP_DISTANCE_UNITS_STORAGE_KEY: &str = "gs_map_distance_units";
const MAP_HEADER_DISTANCE_VISIBLE_STORAGE_KEY: &str = "gs_map_header_distance_visible";
const MAP_HEADER_ALTITUDE_VISIBLE_STORAGE_KEY: &str = "gs_map_header_altitude_visible";
const THEME_PRESET_STORAGE_KEY: &str = "gs_theme_preset";
const LANGUAGE_STORAGE_KEY: &str = "gs_language";
const CLOCK_24H_STORAGE_KEY: &str = "gs_clock_24h";
const NETWORK_FLOW_ANIMATION_STORAGE_KEY: &str = "gs_network_flow_animation";
const REMOTE_ALERT_ACKS_ENABLED_STORAGE_KEY: &str = "gs_remote_alert_acks_enabled";
const NETWORK_TOPOLOGY_VERTICAL_STORAGE_KEY: &str = "gs_network_topology_vertical";
const STATE_CHART_LABELS_VERTICAL_STORAGE_KEY: &str = "gs_state_chart_labels_vertical";
const CHART_INTERPOLATED_GAP_MS_STORAGE_KEY: &str = "gs_chart_interpolated_gap_ms";
const TELEMETRY_RETENTION_MS_STORAGE_KEY: &str = "gs_telemetry_retention_ms";
const TELEMETRY_VIEW_WINDOW_MS_STORAGE_KEY: &str = "gs_telemetry_view_window_ms";
const DATA_CACHE_ENABLED_STORAGE_KEY: &str = "gs_data_cache_enabled";
const MAP_TILE_CACHE_ENABLED_STORAGE_KEY: &str = "gs26_tile_cache_enabled";
const CACHE_BUDGET_MB_STORAGE_KEY: &str = "gs_cache_budget_mb";
const MAP_PREFETCH_ENABLED_STORAGE_KEY: &str = "gs_map_prefetch_enabled";
const MAP_PREFETCH_USER_RADIUS_STORAGE_KEY: &str = "gs_map_prefetch_user_radius_m";
const MAP_PREFETCH_ROCKET_RADIUS_STORAGE_KEY: &str = "gs_map_prefetch_rocket_radius_m";
const USER_LOCATION_SOURCE_STORAGE_KEY: &str = "gs_user_location_source";
const USER_MANUAL_LAT_STORAGE_KEY: &str = "gs_user_manual_lat";
const USER_MANUAL_LON_STORAGE_KEY: &str = "gs_user_manual_lon";
const USER_HEADING_SOURCE_STORAGE_KEY: &str = "gs_user_heading_source";
const USER_MANUAL_HEADING_STORAGE_KEY: &str = "gs_user_manual_heading";
const CALIBRATION_CAPTURE_SAMPLE_COUNT_STORAGE_KEY: &str = "gs_calibration_capture_sample_count";
const LAYOUT_CACHE_KEY_PREFIX: &str = "gs_layout_cache_v10_";
const CALIBRATION_VISIBILITY_CACHE_KEY_PREFIX: &str = "gs_calibration_visibility_v1_";
const DATA_SUBTAB_STORAGE_KEY_PREFIX: &str = "gs26_active_data_subtab::";
const NOTIFICATION_DISMISSED_STORAGE_KEY: &str = "gs_notification_dismissed_ids_v1";
const _SKIP_TLS_VERIFY_KEY_PREFIX: &str = "gs_skip_tls_verify_";
const TELEMETRY_CACHE_STORAGE_KEY: &str = "gs_telemetry_rows_cache_v1";
const TELEMETRY_CACHE_MAX_ROWS: usize = 5_000;
const TELEMETRY_CACHE_WRITE_INTERVAL_MS: i64 = 2_500;
const NOTIFICATION_AUTO_DISMISS_MS: u32 = 5_000;
const MAX_ACTIVE_NOTIFICATIONS: usize = 2;
const MAX_NOTIFICATION_HISTORY: usize = 500;
const MAX_MESSAGE_HISTORY: usize = 2_000;
const MAP_MAX_ZOOM_STORAGE_KEY: &str = "gs26_ground_map_max_zoom_v1";
#[cfg(target_arch = "wasm32")]
const MAP_TILE_CACHE_USAGE_BYTES_STORAGE_KEY: &str = "gs26_tile_cache_usage_bytes";
#[cfg(not(target_arch = "wasm32"))]
const TILE_CACHE_STATS_TTL_MS: i64 = 5_000;
const DEFAULT_CACHE_BUDGET_MB: u32 = 500;
const DEFAULT_PREFETCH_RADIUS_M: u32 = 1_609;
const MIN_PREFETCH_RADIUS_M: u32 = 100;
const MAX_PREFETCH_RADIUS_M: u32 = 20_000;

fn data_cache_enabled() -> bool {
    persist::get_or(DATA_CACHE_ENABLED_STORAGE_KEY, "on") != "off"
}

fn stored_cache_budget_mb() -> u32 {
    persist::get_or(
        CACHE_BUDGET_MB_STORAGE_KEY,
        &DEFAULT_CACHE_BUDGET_MB.to_string(),
    )
    .parse::<u32>()
    .ok()
    .unwrap_or(DEFAULT_CACHE_BUDGET_MB)
    .clamp(1, 100_000)
}

fn clamp_telemetry_history_ms(value_ms: u64) -> u64 {
    value_ms.clamp(MIN_TELEMETRY_HISTORY_MS, MAX_TELEMETRY_HISTORY_MS)
}

fn stored_telemetry_retention_ms() -> u64 {
    persist::get_or(
        TELEMETRY_RETENTION_MS_STORAGE_KEY,
        &DEFAULT_TELEMETRY_RETENTION_MS.to_string(),
    )
    .parse::<u64>()
    .ok()
    .map(clamp_telemetry_history_ms)
    .unwrap_or(DEFAULT_TELEMETRY_RETENTION_MS)
}

fn stored_telemetry_view_window_ms() -> u64 {
    let retention_ms = stored_telemetry_retention_ms();
    persist::get_or(
        TELEMETRY_VIEW_WINDOW_MS_STORAGE_KEY,
        &DEFAULT_TELEMETRY_VIEW_WINDOW_MS.to_string(),
    )
    .parse::<u64>()
    .ok()
    .map(clamp_telemetry_history_ms)
    .unwrap_or(DEFAULT_TELEMETRY_VIEW_WINDOW_MS)
    .min(retention_ms)
}

fn cache_budget_bytes_from_mb(mb: u32) -> u64 {
    (mb as u64).saturating_mul(1024 * 1024)
}

fn stored_cache_budget_bytes() -> u64 {
    cache_budget_bytes_from_mb(stored_cache_budget_mb())
}

fn clamp_prefetch_radius_m(value: u32) -> u32 {
    value.clamp(MIN_PREFETCH_RADIUS_M, MAX_PREFETCH_RADIUS_M)
}

fn stored_prefetch_radius_m(key: &str) -> u32 {
    persist::get_or(key, &DEFAULT_PREFETCH_RADIUS_M.to_string())
        .parse::<u32>()
        .ok()
        .map(clamp_prefetch_radius_m)
        .unwrap_or(DEFAULT_PREFETCH_RADIUS_M)
}

fn parse_manual_user_coords_strings(lat: &str, lon: &str) -> Option<(f64, f64)> {
    let lat = lat.trim().parse::<f64>().ok()?;
    let lon = lon.trim().parse::<f64>().ok()?;
    if !lat.is_finite() || !lon.is_finite() {
        return None;
    }
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    Some((lat, lon))
}

fn parse_manual_heading_string(value: &str) -> Option<f64> {
    let heading = value.trim().parse::<f64>().ok()?;
    if !heading.is_finite() {
        return None;
    }
    Some(heading.rem_euclid(360.0))
}

#[cfg(target_arch = "wasm32")]
fn browser_tile_cache_measured_bytes() -> u64 {
    persist::get_or(MAP_TILE_CACHE_USAGE_BYTES_STORAGE_KEY, "0")
        .parse::<u64>()
        .ok()
        .unwrap_or(0)
}

fn cache_storage_measured_bytes() -> u64 {
    let telemetry_bytes = persist::byte_len(TELEMETRY_CACHE_STORAGE_KEY) as u64;
    let layout_bytes = persist::byte_len_prefix(LAYOUT_CACHE_KEY_PREFIX) as u64;
    let calibration_layout_bytes =
        persist::byte_len_prefix(CALIBRATION_VISIBILITY_CACHE_KEY_PREFIX) as u64;
    let map_metadata_bytes = persist::byte_len(MAP_MAX_ZOOM_STORAGE_KEY) as u64;
    let notification_bytes = persist::byte_len(NOTIFICATION_DISMISSED_STORAGE_KEY) as u64;
    let preference_bytes = [
        WARNING_ACK_STORAGE_KEY,
        ERROR_ACK_STORAGE_KEY,
        MAIN_TAB_STORAGE_KEY,
        DATA_TAB_STORAGE_KEY,
        BASE_URL_STORAGE_KEY,
        MAP_DISTANCE_UNITS_STORAGE_KEY,
        THEME_PRESET_STORAGE_KEY,
        LANGUAGE_STORAGE_KEY,
        NETWORK_FLOW_ANIMATION_STORAGE_KEY,
        REMOTE_ALERT_ACKS_ENABLED_STORAGE_KEY,
        NETWORK_TOPOLOGY_VERTICAL_STORAGE_KEY,
        STATE_CHART_LABELS_VERTICAL_STORAGE_KEY,
        DATA_CACHE_ENABLED_STORAGE_KEY,
        MAP_TILE_CACHE_ENABLED_STORAGE_KEY,
        MAP_PREFETCH_ENABLED_STORAGE_KEY,
        MAP_PREFETCH_USER_RADIUS_STORAGE_KEY,
        MAP_PREFETCH_ROCKET_RADIUS_STORAGE_KEY,
        USER_LOCATION_SOURCE_STORAGE_KEY,
        USER_MANUAL_LAT_STORAGE_KEY,
        USER_MANUAL_LON_STORAGE_KEY,
        USER_HEADING_SOURCE_STORAGE_KEY,
        USER_MANUAL_HEADING_STORAGE_KEY,
        CALIBRATION_CAPTURE_SAMPLE_COUNT_STORAGE_KEY,
    ]
    .iter()
    .map(|key| persist::byte_len(key) as u64)
    .sum::<u64>();
    #[cfg(not(target_arch = "wasm32"))]
    let tile_bytes = native_tile_cache_stats().0;
    #[cfg(target_arch = "wasm32")]
    let tile_bytes = browser_tile_cache_measured_bytes();

    telemetry_bytes
        .saturating_add(layout_bytes)
        .saturating_add(calibration_layout_bytes)
        .saturating_add(map_metadata_bytes)
        .saturating_add(notification_bytes)
        .saturating_add(preference_bytes)
        .saturating_add(tile_bytes)
}

fn cache_storage_stats_rows() -> Vec<(String, String)> {
    let telemetry_bytes = persist::byte_len(TELEMETRY_CACHE_STORAGE_KEY) as u64;
    let layout_bytes = persist::byte_len_prefix(LAYOUT_CACHE_KEY_PREFIX) as u64;
    let calibration_layout_bytes =
        persist::byte_len_prefix(CALIBRATION_VISIBILITY_CACHE_KEY_PREFIX) as u64;
    let map_metadata_bytes = persist::byte_len(MAP_MAX_ZOOM_STORAGE_KEY) as u64;
    let notification_bytes = persist::byte_len(NOTIFICATION_DISMISSED_STORAGE_KEY) as u64;
    let preference_bytes = [
        WARNING_ACK_STORAGE_KEY,
        ERROR_ACK_STORAGE_KEY,
        MAIN_TAB_STORAGE_KEY,
        DATA_TAB_STORAGE_KEY,
        BASE_URL_STORAGE_KEY,
        MAP_DISTANCE_UNITS_STORAGE_KEY,
        THEME_PRESET_STORAGE_KEY,
        LANGUAGE_STORAGE_KEY,
        NETWORK_FLOW_ANIMATION_STORAGE_KEY,
        REMOTE_ALERT_ACKS_ENABLED_STORAGE_KEY,
        NETWORK_TOPOLOGY_VERTICAL_STORAGE_KEY,
        STATE_CHART_LABELS_VERTICAL_STORAGE_KEY,
        DATA_CACHE_ENABLED_STORAGE_KEY,
        MAP_TILE_CACHE_ENABLED_STORAGE_KEY,
        CACHE_BUDGET_MB_STORAGE_KEY,
        MAP_PREFETCH_ENABLED_STORAGE_KEY,
        MAP_PREFETCH_USER_RADIUS_STORAGE_KEY,
        MAP_PREFETCH_ROCKET_RADIUS_STORAGE_KEY,
        USER_LOCATION_SOURCE_STORAGE_KEY,
        USER_MANUAL_LAT_STORAGE_KEY,
        USER_MANUAL_LON_STORAGE_KEY,
        USER_HEADING_SOURCE_STORAGE_KEY,
        USER_MANUAL_HEADING_STORAGE_KEY,
        CALIBRATION_CAPTURE_SAMPLE_COUNT_STORAGE_KEY,
    ]
    .iter()
    .map(|key| persist::byte_len(key) as u64)
    .sum::<u64>();
    #[cfg(not(target_arch = "wasm32"))]
    let (tile_bytes, tile_files) = native_tile_cache_stats();
    #[cfg(target_arch = "wasm32")]
    let (tile_bytes, tile_files) = (browser_tile_cache_measured_bytes(), 0u64);

    let total = telemetry_bytes
        .saturating_add(layout_bytes)
        .saturating_add(calibration_layout_bytes)
        .saturating_add(map_metadata_bytes)
        .saturating_add(notification_bytes)
        .saturating_add(preference_bytes)
        .saturating_add(tile_bytes);
    vec![
        (
            "Telemetry data cache".to_string(),
            human_bytes_u64(telemetry_bytes),
        ),
        ("Layout cache".to_string(), human_bytes_u64(layout_bytes)),
        (
            "Calibration tab cache".to_string(),
            human_bytes_u64(calibration_layout_bytes),
        ),
        (
            "Notification cache".to_string(),
            human_bytes_u64(notification_bytes),
        ),
        (
            "Settings cache".to_string(),
            human_bytes_u64(preference_bytes),
        ),
        (
            "Map metadata cache".to_string(),
            human_bytes_u64(map_metadata_bytes),
        ),
        (
            "Map tile cache".to_string(),
            if tile_files > 0 {
                format!("{} / {} files", human_bytes_u64(tile_bytes), tile_files)
            } else {
                human_bytes_u64(tile_bytes)
            },
        ),
        ("Measured total".to_string(), human_bytes_u64(total)),
    ]
}

fn human_bytes_u64(bytes: u64) -> String {
    let units = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.2} {}", units[unit])
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_tile_cache_stats() -> (u64, u64) {
    static TILE_CACHE_STATS_CACHE: Lazy<Mutex<(i64, u64, u64)>> =
        Lazy::new(|| Mutex::new((0, 0, 0)));

    let now = current_wallclock_ms();
    if let Ok(cache) = TILE_CACHE_STATS_CACHE.lock() {
        let (last_ms, bytes, files) = *cache;
        if now.saturating_sub(last_ms) < TILE_CACHE_STATS_TTL_MS {
            return (bytes, files);
        }
    }

    fn walk(path: &std::path::Path) -> (u64, u64) {
        let Ok(entries) = std::fs::read_dir(path) else {
            return (0, 0);
        };
        let mut bytes = 0u64;
        let mut files = 0u64;
        for entry in entries.flatten() {
            let entry_path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                let (child_bytes, child_files) = walk(&entry_path);
                bytes = bytes.saturating_add(child_bytes);
                files = files.saturating_add(child_files);
            } else if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
                files = files.saturating_add(1);
            }
        }
        (bytes, files)
    }

    let (bytes, files) = walk(&std::env::temp_dir().join("gs26-tile-cache"));
    if let Ok(mut cache) = TILE_CACHE_STATS_CACHE.lock() {
        *cache = (now, bytes, files);
    }
    (bytes, files)
}

fn clear_cached_layout_configs() {
    persist::remove_prefix(LAYOUT_CACHE_KEY_PREFIX);
    persist::remove_prefix(CALIBRATION_VISIBILITY_CACHE_KEY_PREFIX);
    persist::remove_prefix(DATA_SUBTAB_STORAGE_KEY_PREFIX);
    persist::_remove(MAP_MAX_ZOOM_STORAGE_KEY);
}

fn clear_browser_tile_and_data_caches() {
    js_eval(
        r#"
        (async function() {
          try {
            if (typeof window !== "undefined" && window.__gs26ChartCanvasCache && typeof window.__gs26ChartCanvasCache.clear === "function") {
              window.__gs26ChartCanvasCache.clear();
            }
            if (typeof window !== "undefined") {
              window.__gs26_ground_map_cache_state = { key: "", state: "idle", pending: 0, completed: 0, failed: 0, lastStartedAt: 0, lastCompletedAt: 0 };
              window.__gs26_ground_map_cache_ready = false;
              window.__gs26_ground_map_max_zoom_json = "";
              window.__gs26_tile_cache_usage_bytes = 0;
              if (window.localStorage) {
                window.localStorage.removeItem("gs26_ground_map_max_zoom_v1");
                window.localStorage.removeItem("gs26_tile_cache_usage_bytes");
              }
              if (typeof window.clearGroundMapTileCaches === "function") {
                await window.clearGroundMapTileCaches();
              } else if (window.GS26 && typeof window.GS26.clearGroundMapTileCaches === "function") {
                await window.GS26.clearGroundMapTileCaches();
              }
            }
            if (typeof caches !== "undefined" && typeof caches.keys === "function") {
              const keys = await caches.keys();
              await Promise.all(
                keys
                  .filter((key) => key.startsWith("gs26-tiles-v1:") || key.startsWith("gs26-tiles-v2:"))
                  .map((key) => caches.delete(key))
              );
            }
          } catch (e) {
            console.warn("GS26 cache clear failed:", e);
          }
        })();
        "#,
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn clear_native_tile_cache() {
    let path = std::env::temp_dir().join("gs26-tile-cache");
    let _ = std::fs::remove_dir_all(path);
}

fn clear_frontend_data_caches() {
    charts_cache_clear_active();
    clear_telemetry_runtime_buffers();
    clear_visible_telemetry_history_without_bridge();
    if let Ok(mut q) = PENDING_WS_OPEN_EVENTS.lock() {
        q.clear();
    }
    if let Ok(mut q) = PENDING_WS_MESSAGE_EVENTS.lock() {
        q.clear();
    }
    if let Ok(mut pending) = HIDDEN_PENDING_WS_STATE.lock() {
        *pending = HiddenPendingWsState::default();
    }
    LAST_WS_ACTIVITY_MONO_MS.store(0, Ordering::Relaxed);
    LAST_TOPOLOGY_ACTIVITY_MONO_MS.store(0, Ordering::Relaxed);
    bump_frontend_data_clear_epoch();
    #[cfg(not(target_arch = "wasm32"))]
    {
        reset_frontend_network_metrics_state();
    }
}

fn clear_map_tile_caches() {
    clear_browser_tile_and_data_caches();
    #[cfg(not(target_arch = "wasm32"))]
    {
        clear_native_tile_cache();
    }
}

fn clear_frontend_caches() {
    clear_frontend_data_caches();
    clear_map_tile_caches();
    clear_cached_layout_configs();
}

fn trigger_map_prefetch_now() {
    js_eval(
        r#"
        (function() {
          try {
            if (typeof window.prefetchGroundMapTiles === "function") {
              window.prefetchGroundMapTiles();
            } else if (typeof window.scheduleHighResTilePrefetch === "function") {
              window.scheduleHighResTilePrefetch({ force: true });
            }
          } catch (e) {
            console.warn("GS26 prefetch trigger failed:", e);
          }
        })();
        "#,
    );
}

fn clear_data_caches_and_reseed() {
    clear_frontend_data_caches();
    set_reseed_status_running();
    charts_cache_request_refit();
    reconnect_and_reload_ui();
}

fn clear_current_dashboard_data_without_reseed() {
    clear_frontend_data_caches();
    set_reseed_status(0, None);
    charts_cache_request_refit();
}

fn clear_data_and_map_tile_caches_and_reseed() {
    clear_frontend_data_caches();
    clear_map_tile_caches();
    set_reseed_status_running();
    charts_cache_request_refit();
    reconnect_and_reload_ui();
}

fn clear_all_frontend_caches_and_reseed() {
    clear_frontend_caches();
    set_reseed_status_running();
    charts_cache_request_refit();
    reconnect_and_reload_ui();
}

fn reset_local_app_data() {
    clear_frontend_caches();
    auth::clear_all_stored_sessions();
    persist::clear_all();
    *PREFERRED_LANGUAGE.write() = "en".to_string();
    *APP_THEME_CONFIG.write() = localized_theme(&layout::ThemeConfig::default(), "default");
    #[cfg(target_arch = "wasm32")]
    {
        js_eval(
            r#"
            (async function() {
              try {
                if (typeof sessionStorage !== "undefined") {
                  sessionStorage.clear();
                }
              } catch (e) {
                console.warn("GS26 sessionStorage clear failed:", e);
              }
            })();
            "#,
        );
    }
}

// When this number changes, we tear down and rebuild the websocket connection.
