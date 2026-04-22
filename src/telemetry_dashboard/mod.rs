#![allow(clippy::redundant_locals)]

// frontend/src/telemetry_dashboard/mod.rs

mod actions_tab;
mod calibration_tab;
mod connection_status_tab;
pub mod data_chart;
pub mod data_tab;
mod detailed_tab;
pub mod errors_tab;
mod gps;
pub(crate) mod gps_android;
mod gps_webview;
pub mod layout;
mod layout_settings_tab;
mod network_topology_tab;
mod notifications_tab;
pub mod types;
#[cfg(not(target_arch = "wasm32"))]
pub mod version_page;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod gps_apple;

pub mod map_tab;
pub mod state_tab;
pub mod warnings_tab;

use crate::app::Route;
use crate::auth;
use data_chart::charts_cache_request_refit;
use data_chart::{
    charts_cache_begin_reseed_build, charts_cache_cancel_reseed_build, charts_cache_clear_active,
    charts_cache_finish_reseed_build, charts_cache_ingest_row, charts_cache_reseed_ingest_row,
    configure_sender_split_data_types,
};

use crate::telemetry_dashboard::actions_tab::ActionsTab;
use calibration_tab::{CalibrationTab, CalibrationTabLayout};
use connection_status_tab::ConnectionStatusTab;
use data_tab::DataTab;
use detailed_tab::DetailedTab;
use dioxus::prelude::*;
use dioxus_signals::Signal;
use errors_tab::ErrorsTab;
use layout::LayoutConfig;
use layout_settings_tab::SettingsPage;
use map_tab::MapTab;
use network_topology_tab::NetworkTopologyTab;
use notifications_tab::NotificationsTab;
use serde::{Deserialize, Serialize};
use state_tab::StateTab;
use types::{
    display_flight_state, BoardStatusEntry, BoardStatusMsg, FlightState, NetworkTopologyMsg,
    TelemetryRow,
};
#[cfg(not(target_arch = "wasm32"))]
use version_page::VersionTab;
use warnings_tab::WarningsTab;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{
    atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicU8, Ordering}, Arc,
    Mutex,
};

use once_cell::sync::Lazy;

// ============================================================================
// Telemetry queue: decouple high-rate telemetry ingest from UI re-render cadence.
// - WS ingest becomes O(1) and never does large Vec rebuilds.
// - UI flush loop drains at ~120Hz (or as fast as runtime allows).
// ============================================================================
static TELEMETRY_QUEUE: Lazy<Mutex<VecDeque<TelemetryRow>>> =
    Lazy::new(|| Mutex::new(VecDeque::new()));
static RESEED_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static RESEED_LIVE_BUFFER: Lazy<Mutex<Vec<TelemetryRow>>> = Lazy::new(|| Mutex::new(Vec::new()));
static RESEED_HISTORY_BRIDGE: Lazy<Mutex<Vec<TelemetryRow>>> = Lazy::new(|| Mutex::new(Vec::new()));
static RESEED_STATUS: AtomicU8 = AtomicU8::new(0);
static RESEED_STATUS_TOKEN: AtomicU64 = AtomicU64::new(0);
static RESEED_STATUS_DETAIL: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));
static DASHBOARD_HAS_CONNECTED: AtomicBool = AtomicBool::new(false);
static LAST_WS_CONNECT_WARNING: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));
static FRONTEND_NETWORK_METRICS_STATE: Lazy<Mutex<FrontendNetworkMetrics>> =
    Lazy::new(|| Mutex::new(FrontendNetworkMetrics::default()));
static TRANSLATION_MISS_QUEUE: Lazy<Mutex<HashSet<String>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));
static TRANSLATION_REQUEST_ACTIVE: AtomicBool = AtomicBool::new(false);
static LAST_COMMAND_ACTIVATION: Lazy<Mutex<Option<(String, f64)>>> = Lazy::new(|| Mutex::new(None));
static PENDING_COMMAND_PRESS: Lazy<Mutex<Option<(String, f64)>>> = Lazy::new(|| Mutex::new(None));

const COMMAND_ACTIVATION_DEDUP_MS: f64 = 450.0;
const COMMAND_MAX_PRESS_RELEASE_MS: f64 = 650.0;

// ============================================================================
// Dashboard lifetime: STATIC + ALWAYS PRESENT (never Option)
// - Solves: Inner reads before Outer writes -> false Arc -> tasks early-exit
//
// CHANGE: we make "unmount" idempotent (swap) and we also let the CONNECT button
//         explicitly flip alive=false *before* bumping WS_EPOCH, so the WS
//         supervisor won't spawn a new epoch while we're leaving the dashboard.
// ============================================================================
#[derive(Clone)]
struct DashboardLife {
    alive: Arc<AtomicBool>,
    // bumps on every REAL mount of outer dashboard
    r#gen: u64,
}

impl DashboardLife {
    /// Creates a dashboard lifetime marker that is already considered torn down.
    fn _new_dead() -> Self {
        Self {
            alive: Arc::new(AtomicBool::new(false)),
            r#gen: 0,
        }
    }
    /// Creates a dashboard lifetime marker for a freshly mounted dashboard.
    fn new_alive() -> Self {
        Self {
            alive: Arc::new(AtomicBool::new(true)),
            r#gen: 0,
        }
    }
}

static DASHBOARD_LIFE: GlobalSignal<DashboardLife> = Signal::global(DashboardLife::new_alive);

#[inline]
/// Returns the current shared dashboard-alive flag.
fn dashboard_alive() -> Arc<AtomicBool> {
    DASHBOARD_LIFE.read().alive.clone()
}

#[inline]
/// Replaces the dashboard lifetime flag and bumps the mount generation.
fn _set_dashboard_alive(alive: bool) {
    let alive = Arc::new(AtomicBool::new(alive));
    *DASHBOARD_LIFE.write() = DashboardLife {
        alive,
        r#gen: dashboard_gen() + 1,
    };
}

#[inline]
/// Returns the current dashboard mount generation.
fn dashboard_gen() -> u64 {
    DASHBOARD_LIFE.read().r#gen
}

mod blink;
mod network_metrics;

// ----------------------------
// Cross-platform persistence
// ----------------------------
mod persist;

include!("dashboard_messages.rs");

const LAUNCH_TMINUS_ZERO_SNAP_MS: i64 = 20;
const LAUNCH_TMINUS_RESET_ZERO_LATCH_MS: i64 = 250;
const DASHBOARD_CLOCK_REFRESH_MS: u32 = 16;

pub(crate) use network_metrics::FrontendNetworkMetrics;
use network_metrics::{
    clear_ws_connection_notification, frontend_network_metrics_snapshot, note_http_rtt_ms,
    note_incoming_telemetry_rows, note_incoming_ws_message, note_ws_connection_notification,
    note_ws_connection_state, reset_frontend_network_metrics_state,
};

/// Returns the current wall-clock time in milliseconds since the Unix epoch.
pub(crate) fn current_wallclock_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}

macro_rules! log {
    ($($t:tt)*) => {{
        let s = format!($($t)*);
        crate::telemetry_dashboard::log(&s);
    }}
}

pub const HISTORY_MS: i64 = 60_000 * 20; // 20 minutes
const UI_ROW_BUCKET_MS: i64 = 20; // Match chart bucket width in data_chart.rs.
const STARTUP_SEED_DELAY_MS: u64 = 1_200;
const MAX_TELEMETRY_QUEUE: usize = 120_000;

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct UiRowKey {
    bucket: i64,
    data_type: String,
    sender_id: String,
}

#[derive(Clone, Eq, PartialEq, Hash)]
struct LatestTelemetryKey {
    data_type: String,
    sender_id: String,
}

#[derive(Clone)]
struct LatestTelemetrySample {
    timestamp_ms: i64,
    data_type: String,
    sender_id: String,
    values: Arc<[Option<f32>]>,
}

impl LatestTelemetryKey {
    /// Builds the cache key used for latest-row tracking.
    fn new(data_type: &str, sender_id: &str) -> Self {
        Self {
            data_type: data_type.to_string(),
            sender_id: sender_id.to_string(),
        }
    }
}

#[derive(Default)]
struct UiTelemetryStore {
    rows: BTreeMap<UiRowKey, TelemetryRow>,
}

impl UiTelemetryStore {
    /// Replaces the compacted UI store with a fresh telemetry snapshot.
    fn replace_from_rows(&mut self, rows: &[TelemetryRow]) {
        self.rows.clear();
        self.apply_rows(rows.iter().cloned());
    }

    /// Inserts rows into the compacted UI store, keeping only the newest row per bucket.
    fn apply_rows<I>(&mut self, rows: I)
    where
        I: IntoIterator<Item = TelemetryRow>,
    {
        for row in rows {
            // The UI only needs one representative row per bucket/sender/type tuple.
            let key = UiRowKey {
                bucket: row.timestamp_ms.div_euclid(UI_ROW_BUCKET_MS),
                data_type: row.data_type.clone(),
                sender_id: row.sender_id.clone(),
            };
            self.rows.insert(key, row);
        }

        self.prune_history();
    }

    /// Drops buckets that are older than the retained history window.
    fn prune_history(&mut self) {
        let Some((&newest_bucket, _)) = self.rows.last_key_value().map(|(k, v)| (&k.bucket, v))
        else {
            return;
        };
        let min_bucket =
            (newest_bucket * UI_ROW_BUCKET_MS - HISTORY_MS).div_euclid(UI_ROW_BUCKET_MS);
        while self
            .rows
            .first_key_value()
            .is_some_and(|(key, _)| key.bucket < min_bucket)
        {
            self.rows.pop_first();
        }
    }

    /// Returns the compacted UI store as a sorted vector.
    fn snapshot(&self) -> Vec<TelemetryRow> {
        self.rows.values().cloned().collect()
    }
}

static UI_TELEMETRY_STORE: Lazy<Mutex<UiTelemetryStore>> =
    Lazy::new(|| Mutex::new(UiTelemetryStore::default()));
static LATEST_TELEMETRY: Lazy<Mutex<HashMap<LatestTelemetryKey, LatestTelemetrySample>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static LATEST_TELEMETRY_BY_TYPE: Lazy<Mutex<HashMap<String, LatestTelemetrySample>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static LAST_TELEMETRY_CACHE_PERSIST_MS: AtomicI64 = AtomicI64::new(0);
static RESTORED_TELEMETRY_CACHE_NEEDS_CHART_REBUILD: AtomicBool = AtomicBool::new(false);

/// Sorts telemetry rows into a stable UI presentation order.
fn sort_rows(rows: &mut [TelemetryRow]) {
    rows.sort_by(|a, b| {
        a.timestamp_ms
            .cmp(&b.timestamp_ms)
            .then_with(|| a.sender_id.cmp(&b.sender_id))
            .then_with(|| a.data_type.cmp(&b.data_type))
    });
}

/// Trims a telemetry vector down to the retained history window.
fn prune_history(rows: &mut Vec<TelemetryRow>) {
    if let Some(last) = rows.last() {
        let cutoff = last.timestamp_ms - HISTORY_MS;
        let start = rows.partition_point(|r| r.timestamp_ms < cutoff);
        if start > 0 {
            rows.drain(0..start);
        }
    }
}

/// Compacts raw telemetry rows down to the newest row per UI bucket.
fn compact_rows_for_ui(rows: Vec<TelemetryRow>) -> Vec<TelemetryRow> {
    let mut by_key: HashMap<(String, String, i64), TelemetryRow> = HashMap::new();
    for row in rows {
        let bucket = row.timestamp_ms.div_euclid(UI_ROW_BUCKET_MS);
        let key = (row.data_type.clone(), row.sender_id.clone(), bucket);
        by_key.insert(key, row);
    }
    let mut out: Vec<TelemetryRow> = by_key.into_values().collect();
    sort_rows(&mut out);
    prune_history(&mut out);
    out
}

/// Rebuilds the latest-row indexes from a full telemetry snapshot.
fn reset_latest_telemetry(rows: &[TelemetryRow]) {
    if let Ok(mut latest) = LATEST_TELEMETRY.lock()
        && let Ok(mut latest_by_type) = LATEST_TELEMETRY_BY_TYPE.lock()
    {
        latest.clear();
        latest_by_type.clear();
        for row in rows {
            update_latest_telemetry_locked(&mut latest, &mut latest_by_type, row);
        }
    }
}

/// Inserts a single row into the latest-row indexes.
fn update_latest_telemetry(row: &TelemetryRow) {
    if let Ok(mut latest) = LATEST_TELEMETRY.lock()
        && let Ok(mut latest_by_type) = LATEST_TELEMETRY_BY_TYPE.lock()
    {
        update_latest_telemetry_locked(&mut latest, &mut latest_by_type, row);
    }
}

/// Inserts a batch of rows into the latest-row indexes under a single lock.
fn update_latest_telemetry_batch(rows: &[TelemetryRow]) {
    if let Ok(mut latest) = LATEST_TELEMETRY.lock()
        && let Ok(mut latest_by_type) = LATEST_TELEMETRY_BY_TYPE.lock()
    {
        for row in rows {
            update_latest_telemetry_locked(&mut latest, &mut latest_by_type, row);
        }
    }
}

/// Applies latest-row replacement rules while both latest-row maps are already locked.
fn update_latest_telemetry_locked(
    latest: &mut HashMap<LatestTelemetryKey, LatestTelemetrySample>,
    latest_by_type: &mut HashMap<String, LatestTelemetrySample>,
    row: &TelemetryRow,
) {
    let key = LatestTelemetryKey::new(&row.data_type, &row.sender_id);
    let should_replace = latest
        .get(&key)
        .is_none_or(|existing| existing.timestamp_ms <= row.timestamp_ms);
    if should_replace {
        latest.insert(
            key,
            LatestTelemetrySample {
                timestamp_ms: row.timestamp_ms,
                data_type: row.data_type.clone(),
                sender_id: row.sender_id.clone(),
                values: Arc::<[Option<f32>]>::from(row.values.clone()),
            },
        );
    }

    let should_replace_type = latest_by_type
        .get(&row.data_type)
        .is_none_or(|existing| existing.timestamp_ms <= row.timestamp_ms);
    if should_replace_type {
        latest_by_type.insert(
            row.data_type.clone(),
            LatestTelemetrySample {
                timestamp_ms: row.timestamp_ms,
                data_type: row.data_type.clone(),
                sender_id: row.sender_id.clone(),
                values: Arc::<[Option<f32>]>::from(row.values.clone()),
            },
        );
    }
}

/// Returns the latest telemetry row for a given data type and optional sender.
pub(crate) fn latest_telemetry_row(
    data_type: &str,
    sender_id: Option<&str>,
) -> Option<TelemetryRow> {
    match sender_id {
        Some(sender_id) => {
            if let Ok(latest) = LATEST_TELEMETRY.lock() {
                latest
                    .get(&LatestTelemetryKey::new(data_type, sender_id))
                    .map(|sample| TelemetryRow {
                        timestamp_ms: sample.timestamp_ms,
                        data_type: sample.data_type.clone(),
                        sender_id: sample.sender_id.clone(),
                        values: sample.values.as_ref().to_vec(),
                    })
            } else {
                None
            }
        }
        None => {
            if let Ok(latest_by_type) = LATEST_TELEMETRY_BY_TYPE.lock() {
                latest_by_type.get(data_type).map(|sample| TelemetryRow {
                    timestamp_ms: sample.timestamp_ms,
                    data_type: sample.data_type.clone(),
                    sender_id: sample.sender_id.clone(),
                    values: sample.values.as_ref().to_vec(),
                })
            } else {
                None
            }
        }
    }
}

/// Returns a single channel from the latest telemetry row for the given key.
pub(crate) fn latest_telemetry_value(
    data_type: &str,
    sender_id: Option<&str>,
    index: usize,
) -> Option<f32> {
    latest_telemetry_value_direct(data_type, sender_id, index)
        .or_else(|| fallback_latest_telemetry_value(data_type, sender_id, index))
}

fn latest_telemetry_value_direct(
    data_type: &str,
    sender_id: Option<&str>,
    index: usize,
) -> Option<f32> {
    match sender_id {
        Some(sender_id) => {
            if let Ok(latest) = LATEST_TELEMETRY.lock() {
                latest
                    .get(&LatestTelemetryKey::new(data_type, sender_id))
                    .and_then(|row| row.values.get(index).copied().flatten())
            } else {
                None
            }
        }
        None => {
            if let Ok(latest_by_type) = LATEST_TELEMETRY_BY_TYPE.lock() {
                latest_by_type
                    .get(data_type)
                    .and_then(|row| row.values.get(index).copied().flatten())
            } else {
                None
            }
        }
    }
}

fn fallback_latest_telemetry_value(
    data_type: &str,
    sender_id: Option<&str>,
    index: usize,
) -> Option<f32> {
    const DEFAULT_LOADCELL_FULL_MASS_KG: f32 = 10.0;

    match (data_type, index) {
        ("LOADCELL_WEIGHT_KG", 0) => latest_telemetry_value_direct("KG1000", sender_id, 0),
        ("LOADCELL_FILL_PERCENT", 0) => {
            let mass_kg = latest_telemetry_value_direct("LOADCELL_WEIGHT_KG", sender_id, 0)
                .or_else(|| latest_telemetry_value_direct("KG1000", sender_id, 0))?;
            Some(((mass_kg / DEFAULT_LOADCELL_FULL_MASS_KG) * 100.0).clamp(0.0, 100.0))
        }
        _ => None,
    }
}

#[cfg(test)]
mod latest_telemetry_tests {
    use super::{latest_telemetry_value, reset_latest_telemetry, TelemetryRow};

    #[test]
    fn derives_latest_loadcell_labels_from_kg1000_samples() {
        reset_latest_telemetry(&[TelemetryRow {
            timestamp_ms: 1_700_000_030_000,
            data_type: "KG1000".to_string(),
            sender_id: "DAQ".to_string(),
            values: vec![Some(9.5754)],
        }]);

        assert_eq!(latest_telemetry_value("KG1000", None, 0), Some(9.5754));
        assert_eq!(
            latest_telemetry_value("LOADCELL_WEIGHT_KG", None, 0),
            Some(9.5754)
        );
        let fill_percent =
            latest_telemetry_value("LOADCELL_FILL_PERCENT", None, 0).expect("derived fill percent");
        assert!((fill_percent - 95.754).abs() < 0.001);

        reset_latest_telemetry(&[]);
    }
}

/// Returns the compacted UI telemetry store as a snapshot vector.
pub(crate) fn ui_telemetry_rows_snapshot() -> Vec<TelemetryRow> {
    if let Ok(store) = UI_TELEMETRY_STORE.lock() {
        store.snapshot()
    } else {
        Vec::new()
    }
}

fn persist_cached_telemetry_rows(rows: &[TelemetryRow]) {
    if rows.is_empty() {
        return;
    }
    let start = rows.len().saturating_sub(TELEMETRY_CACHE_MAX_ROWS);
    if let Ok(raw) = serde_json::to_string(&rows[start..]) {
        persist::set_string(TELEMETRY_CACHE_STORAGE_KEY, &raw);
    }
}

fn persist_cached_telemetry_snapshot_if_due(force: bool) {
    let now_ms = current_wallclock_ms();
    let last_ms = LAST_TELEMETRY_CACHE_PERSIST_MS.load(Ordering::Relaxed);
    if !force && now_ms.saturating_sub(last_ms) < TELEMETRY_CACHE_WRITE_INTERVAL_MS {
        return;
    }

    let rows = ui_telemetry_rows_snapshot();
    if rows.is_empty() {
        return;
    }

    persist_cached_telemetry_rows(&rows);
    LAST_TELEMETRY_CACHE_PERSIST_MS.store(now_ms, Ordering::Relaxed);
}

fn restore_cached_telemetry_rows_if_needed() -> usize {
    if !ui_telemetry_rows_snapshot().is_empty() {
        return 0;
    }

    let Some(raw) = persist::get_string(TELEMETRY_CACHE_STORAGE_KEY) else {
        return 0;
    };
    let Ok(mut rows) = serde_json::from_str::<Vec<TelemetryRow>>(&raw) else {
        return 0;
    };
    if rows.is_empty() {
        return 0;
    }

    sort_rows(&mut rows);
    prune_history(&mut rows);
    rows = compact_rows_for_ui(rows);
    if rows.is_empty() {
        return 0;
    }

    for row in &rows {
        charts_cache_ingest_row(row);
    }
    if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
        store.replace_from_rows(&rows);
    }
    reset_latest_telemetry(&rows);
    RESTORED_TELEMETRY_CACHE_NEEDS_CHART_REBUILD.store(true, Ordering::Relaxed);
    bump_render_epoch();
    bump_chart_render_epoch();
    rows.len()
}

fn rebuild_chart_cache_from_visible_rows() {
    let rows = ui_telemetry_rows_snapshot();
    if rows.is_empty() {
        return;
    }
    charts_cache_clear_active();
    for row in &rows {
        charts_cache_ingest_row(row);
    }
    bump_chart_render_epoch();
}

#[cfg(not(target_arch = "wasm32"))]
pub fn dashboard_has_cached_layout_for_base(base: &str) -> bool {
    let cache_key = layout_cache_key_for_base(base);
    persist::get_string(&cache_key)
        .and_then(|raw| serde_json::from_str::<LayoutConfig>(&raw).ok())
        .is_some_and(|layout| layout.validate().is_ok())
}

// unified storage keys
const WARNING_ACK_STORAGE_KEY: &str = "gs_last_warning_ack_ts";
const ERROR_ACK_STORAGE_KEY: &str = "gs_last_error_ack_ts";
const MAIN_TAB_STORAGE_KEY: &str = "gs_main_tab";
const DATA_TAB_STORAGE_KEY: &str = "gs_data_tab";
const BASE_URL_STORAGE_KEY: &str = "gs_base_url";
const MAP_DISTANCE_UNITS_STORAGE_KEY: &str = "gs_map_distance_units";
const THEME_PRESET_STORAGE_KEY: &str = "gs_theme_preset";
const LANGUAGE_STORAGE_KEY: &str = "gs_language";
const NETWORK_FLOW_ANIMATION_STORAGE_KEY: &str = "gs_network_flow_animation";
const STATE_CHART_LABELS_VERTICAL_STORAGE_KEY: &str = "gs_state_chart_labels_vertical";
const MAP_PREFETCH_ENABLED_STORAGE_KEY: &str = "gs_map_prefetch_enabled";
const CALIBRATION_CAPTURE_SAMPLE_COUNT_STORAGE_KEY: &str = "gs_calibration_capture_sample_count";
const LAYOUT_CACHE_KEY_PREFIX: &str = "gs_layout_cache_v9_";
const NOTIFICATION_DISMISSED_STORAGE_KEY: &str = "gs_notification_dismissed_ids_v1";
const _SKIP_TLS_VERIFY_KEY_PREFIX: &str = "gs_skip_tls_verify_";
const TELEMETRY_CACHE_STORAGE_KEY: &str = "gs_telemetry_rows_cache_v1";
const TELEMETRY_CACHE_MAX_ROWS: usize = 5_000;
const TELEMETRY_CACHE_WRITE_INTERVAL_MS: i64 = 2_500;
const NOTIFICATION_AUTO_DISMISS_MS: u32 = 5_000;
const MAX_ACTIVE_NOTIFICATIONS: usize = 2;
const MAX_NOTIFICATION_HISTORY: usize = 500;

fn clear_cached_layout_configs() {
    persist::remove_prefix(LAYOUT_CACHE_KEY_PREFIX);
}

#[cfg(target_arch = "wasm32")]
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
            }
            if (typeof caches !== "undefined" && typeof caches.keys === "function") {
              const keys = await caches.keys();
              await Promise.all(
                keys
                  .filter((key) => key.startsWith("gs26-tiles-v1:"))
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

fn clear_frontend_caches() {
    charts_cache_clear_active();
    clear_cached_layout_configs();
    #[cfg(not(target_arch = "wasm32"))]
    {
        clear_telemetry_runtime_buffers();
        clear_visible_telemetry_history();
        reset_frontend_network_metrics_state();
        clear_native_tile_cache();
    }
    #[cfg(target_arch = "wasm32")]
    {
        clear_browser_tile_and_data_caches();
    }
}

fn trigger_map_prefetch_now() {
    js_eval(
        r#"
        (function() {
          try {
            if (typeof window.scheduleHighResTilePrefetch === "function") {
              window.scheduleHighResTilePrefetch();
            }
          } catch (e) {
            console.warn("GS26 prefetch trigger failed:", e);
          }
        })();
        "#,
    );
}

fn clear_frontend_caches_and_reseed() {
    clear_frontend_caches();
    set_reseed_status_running();
    charts_cache_request_refit();
    reconnect_and_reload_ui();
    trigger_map_prefetch_now();
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
static WS_EPOCH: GlobalSignal<u64> = Signal::global(|| 0);
static TELEMETRY_RENDER_EPOCH: GlobalSignal<u64> = Signal::global(|| 0);
pub(crate) static CHART_RENDER_EPOCH: GlobalSignal<u64> = Signal::global(|| 0);
static PREFERRED_LANGUAGE: GlobalSignal<String> = Signal::global(|| "en".to_string());
static TRANSLATION_CATALOG: GlobalSignal<HashMap<String, String>> = Signal::global(HashMap::new);
pub(crate) static APP_THEME_CONFIG: GlobalSignal<layout::ThemeConfig> = Signal::global(|| {
    let stored = persist::get_or(THEME_PRESET_STORAGE_KEY, "default");
    let preset = if stored == "layout" {
        "backend"
    } else {
        &stored
    };
    localized_theme(&layout::ThemeConfig::default(), preset)
});
static BUILTIN_THEME_CATALOG: Lazy<layout::ThemePresetCatalog> = Lazy::new(|| {
    serde_json::from_str(include_str!(concat!(
        env!("OUT_DIR"),
        "/theme_presets.json"
    )))
    .expect("compiled theme preset catalog must be valid JSON")
});

#[cfg(target_arch = "wasm32")]
static WS_RAW: GlobalSignal<Option<web_sys::WebSocket>> = Signal::global(|| None);
// Force re-seed of graphs/history from backend.
static SEED_EPOCH: GlobalSignal<u64> = Signal::global(|| 0);
static LAUNCH_TMINUS_DISPLAY_MIN_MS: AtomicI64 = AtomicI64::new(i64::MAX);
static LAUNCH_TMINUS_ZERO_LATCHED: AtomicBool = AtomicBool::new(false);

fn bump_telemetry_render_epoch() {
    let mut render_epoch = TELEMETRY_RENDER_EPOCH.write();
    *render_epoch = render_epoch.wrapping_add(1);
}

fn bump_chart_render_epoch() {
    let mut render_epoch = CHART_RENDER_EPOCH.write();
    *render_epoch = render_epoch.wrapping_add(1);
}

fn bump_render_epoch() {
    bump_telemetry_render_epoch();
    bump_chart_render_epoch();
}

fn set_reseed_status(status: u8, detail: Option<String>) {
    RESEED_STATUS_TOKEN.fetch_add(1, Ordering::Relaxed);
    RESEED_STATUS.store(status, Ordering::Relaxed);
    if let Ok(mut slot) = RESEED_STATUS_DETAIL.lock() {
        *slot = detail;
    }
    bump_render_epoch();
}

fn set_reseed_status_running() {
    set_reseed_status(
        1,
        Some("Getting past data from the Ground Station...".to_string()),
    );
}

fn set_reseed_status_ok(_rows: usize) {
    set_reseed_status(2, Some("Past data loaded.".to_string()));
    let token = RESEED_STATUS_TOKEN.load(Ordering::Relaxed);
    spawn(async move {
        #[cfg(target_arch = "wasm32")]
        gloo_timers::future::TimeoutFuture::new(5_000).await;

        #[cfg(not(target_arch = "wasm32"))]
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        if RESEED_STATUS.load(Ordering::Relaxed) == 2
            && RESEED_STATUS_TOKEN.load(Ordering::Relaxed) == token
        {
            set_reseed_status(0, None);
        }
    });
}

fn set_reseed_status_failed(message: impl Into<String>) {
    set_reseed_status(3, Some(message.into()));
}

fn user_friendly_http_error(err: &str) -> String {
    let lower = err.to_ascii_lowercase();
    if lower.contains("http 502") || lower.contains("502 bad gateway") {
        return "The Ground Station service is temporarily unavailable. Check that the Ground Station is running and try reconnecting.".to_string();
    }
    if lower.contains("http 503") || lower.contains("503 service unavailable") {
        return "The Ground Station service is starting up or overloaded. Wait a moment, then try again.".to_string();
    }
    if lower.contains("http 504") || lower.contains("504 gateway timeout") {
        return "The Ground Station did not respond in time. Check the connection and try again."
            .to_string();
    }
    if lower.contains("http 401") || lower.contains("unauthorized") {
        return "Your session expired. Sign in again to continue.".to_string();
    }
    if lower.contains("http 403") || lower.contains("forbidden") {
        return "You do not have permission to perform this action.".to_string();
    }
    if lower.contains("http 404") || lower.contains("not found") {
        return "The Ground Station is missing a required API endpoint. Check that the backend version matches the frontend.".to_string();
    }
    if lower.contains("http 500") || lower.contains("internal server error") {
        return "The Ground Station backend reported an internal problem. Check the backend logs."
            .to_string();
    }
    if lower.contains("request send failed")
        || lower.contains("failed to fetch")
        || lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("dns")
        || lower.contains("timed out")
        || lower.contains("timeout")
    {
        return "Could not reach the Ground Station backend. Check the network connection and backend address.".to_string();
    }
    if lower.contains("invalid json") || lower.contains("expected value") {
        return "The Ground Station sent data this frontend could not read. Check that the frontend and backend versions match.".to_string();
    }
    "The Ground Station request failed. Check the connection and try again.".to_string()
}

fn layout_load_error_message(err: &str) -> String {
    format!(
        "Could not load the dashboard layout. {}",
        user_friendly_http_error(err)
    )
}

fn reseed_error_message(refresh: bool, err: &str) -> String {
    let detail = user_friendly_http_error(err);
    if refresh {
        format!("Could not refresh past telemetry. Keeping the data already shown. {detail}")
    } else {
        format!("Could not load past telemetry. {detail}")
    }
}

pub(crate) fn reseed_status_note() -> Option<(&'static str, String)> {
    let kind = match RESEED_STATUS.load(Ordering::Relaxed) {
        1 => "info",
        2 => "success",
        3 => "error",
        _ => return None,
    };
    let text = RESEED_STATUS_DETAIL
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .unwrap_or_else(|| match kind {
            "info" => "Getting past data from the Ground Station...".to_string(),
            "success" => "Past data loaded.".to_string(),
            "error" => "Could not get past data from the Ground Station.".to_string(),
            _ => String::new(),
        });
    Some((kind, text))
}

pub(crate) fn reseed_note_banner(
    kind: &'static str,
    note: &str,
    theme: &layout::ThemeConfig,
    margin_bottom: bool,
) -> Element {
    let (background, border, text) = match kind {
        "error" => (
            &theme.error_background,
            &theme.error_border,
            &theme.error_text,
        ),
        "success" => (
            &theme.notification_background,
            &theme.notification_border,
            &theme.notification_text,
        ),
        _ => (&theme.info_background, &theme.info_accent, &theme.info_text),
    };
    let margin = if margin_bottom {
        "margin-bottom:8px;"
    } else {
        ""
    };
    rsx! {
        div { style: "{margin} padding:6px 8px; border-radius:8px; border:1px solid {border}; background:{background}; color:{text}; font-size:11px; line-height:1.35;",
            "{translate_text(note)}"
        }
    }
}

/// Normalizes a stored base URL down to `scheme://host[:port]`.
fn normalize_base_url(mut url: String) -> String {
    if let Some(idx) = url.find('#') {
        url.truncate(idx);
    }
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        if let Some(slash) = rest.find('/') {
            url.truncate(scheme_end + 3 + slash);
        }
    }
    url.trim_end_matches('/').trim().to_ascii_lowercase()
}

fn layout_cache_key_for_base(base: &str) -> String {
    let normalized = normalize_base_url(base.to_string());
    if normalized.is_empty() {
        return format!("{LAYOUT_CACHE_KEY_PREFIX}default");
    }

    let mut key = String::with_capacity(LAYOUT_CACHE_KEY_PREFIX.len() + normalized.len());
    key.push_str(LAYOUT_CACHE_KEY_PREFIX);
    for ch in normalized.chars() {
        if ch.is_ascii_alphanumeric() {
            key.push(ch);
        } else {
            key.push('_');
        }
    }
    key
}

#[cfg(target_arch = "wasm32")]
/// Builds an absolute HTTP path for the web build using the active backend base URL.
pub fn abs_http(path: &str) -> String {
    let base = UrlConfig::base_http();
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    if base.is_empty() {
        path
    } else {
        format!("{base}{path}")
    }
}

/// Returns the tile URL template appropriate for the current platform.
pub fn map_tiles_url() -> String {
    #[cfg(target_os = "windows")]
    {
        // WebView2 cannot always resolve custom subresource schemes directly.
        // WRY maps the custom `gs26://` protocol to this host form on Windows.
        "http://gs26.localhost/tiles/{z}/{x}/{y}.jpg".to_string()
    }

    #[cfg(target_os = "android")]
    {
        // Android WebView raster loads work more reliably through WRY's host-mapped
        // alias while still routing into the same native protocol handler.
        "https://gs26.local/tiles/{z}/{x}/{y}.jpg".to_string()
    }

    #[cfg(target_os = "ios")]
    {
        // iOS uses the same JS-side MapLibre loader path as Android, so route tiles
        // straight through the custom protocol handler here as well.
        "gs26://local/tiles/{z}/{x}/{y}.jpg".to_string()
    }

    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "windows"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    {
        // Native WebViews can block plain-http tile fetches; always proxy through
        // our native protocol handler, which performs the upstream HTTP(S) request.
        "gs26://local/tiles/{z}/{x}/{y}.jpg".to_string()
    }

    #[cfg(target_arch = "wasm32")]
    {
        abs_http("/tiles/{z}/{x}/{y}.jpg")
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// Reads the persisted backend base URL for native blocking I/O paths.
pub(crate) fn persisted_base_http_for_native_io() -> String {
    persist::get_string(BASE_URL_STORAGE_KEY)
        .map(normalize_base_url)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "http://localhost:3000".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
/// Reads the persisted TLS-skip flag for the supplied native base URL.
pub(crate) fn persisted_skip_tls_for_base_for_native_io(base: &str) -> bool {
    persist::get_string(&_tls_skip_key(base))
        .map(|v| v == "true")
        .unwrap_or(false)
}

/// Forces all WebSocket-backed tasks to tear down and reconnect on the next render tick.
fn bump_ws_epoch() {
    *WS_SENDER.write() = None;

    #[cfg(target_arch = "wasm32")]
    {
        if let Some(ws) = WS_RAW.write().take() {
            let _ = ws.close();
        }
    }

    *WS_EPOCH.write() += 1;
}

/// Requests a fresh telemetry reseed from the backend.
fn bump_seed_epoch() {
    let mut epoch = SEED_EPOCH.write();
    *epoch += 1;
    log!("[seed] bump_seed_epoch -> {}", *epoch);
}

fn note_ws_connected_and_restore_data_flow(
    ws_url: String,
    epoch: u64,
    notifications: &mut Signal<Vec<PersistentNotification>>,
    notification_history: &mut Signal<Vec<PersistentNotification>>,
    unread_notification_ids: &mut Signal<Vec<u64>>,
) {
    note_ws_connection_state(true, ws_url, None, epoch);
    clear_ws_connection_notification(notifications, notification_history, unread_notification_ids);
    set_reseed_status_running();
    charts_cache_request_refit();
    bump_render_epoch();
    bump_seed_epoch();
}

pub(crate) fn localized_copy(lang: &str, en: &str, es: &str, fr: &str) -> String {
    match lang {
        "es" => es.to_string(),
        "fr" => fr.to_string(),
        _ => en.to_string(),
    }
}

pub(crate) fn current_language() -> String {
    PREFERRED_LANGUAGE.read().clone()
}

pub(crate) fn set_preferred_language(code: &str) {
    let value = code.to_string();
    *PREFERRED_LANGUAGE.write() = value.clone();
    persist::set_string(LANGUAGE_STORAGE_KEY, &value);
}

pub(crate) fn translate_text(input: &str) -> String {
    let text = input.trim();
    if text.is_empty() {
        return input.to_string();
    }
    if let Some(value) = TRANSLATION_CATALOG.read().get(text) {
        return value.clone();
    }
    if let Some(value) = builtin_translation(&current_language(), text) {
        return value.to_string();
    }
    if let Ok(mut pending) = TRANSLATION_MISS_QUEUE.lock() {
        pending.insert(text.to_string());
    }
    input.to_string()
}

fn builtin_translation(lang: &str, text: &str) -> Option<&'static str> {
    match lang {
        "es" => builtin_translation_es(text),
        "fr" => builtin_translation_fr(text),
        _ => None,
    }
}

fn builtin_translation_es(text: &str) -> Option<&'static str> {
    Some(match text {
        "ABORT" => "ABORTAR",
        "VERSION" => "VERSIÓN",
        "CONNECT" => "CONECTAR",
        "RELOAD" => "RECARGAR",
        "SETTINGS" => "AJUSTES",
        "SIGN IN" => "INICIAR SESIÓN",
        "SIGN OUT" => "CERRAR SESIÓN",
        "Menu" => "Menú",
        "Close menu" => "Cerrar menú",
        "Actions Disabled" => "Acciones Desactivadas",
        "Actions Enabled" => "Acciones Activadas",
        "Close" | "Dismiss" => "Cerrar",
        "State" => "Estado",
        "Current Flight State" => "Estado actual de vuelo",
        "Flight state" => "Estado de vuelo",
        "Fill Test" => "Prueba de llenado",
        "Nitrogen Fill" => "Llenado de nitrógeno",
        "Nitrous Fill" => "Llenado de nitroso",
        "Fill Metrics" => "Métricas de llenado",
        "Tank Pressure" => "Presión del tanque",
        "Mass (kg)" => "Masa (kg)",
        "Fill Percent" => "Porcentaje de llenado",
        "Pressure and Loadcell" => "Presión y celda de carga",
        "Target" => "Objetivo",
        "Target mass (kg)" => "Masa objetivo (kg)",
        "Target pressure (psi)" => "Presión objetivo (psi)",
        "Roll" => "Alabeo",
        "Pitch" => "Cabeceo",
        "Yaw" => "Guiñada",
        "Fullscreen" => "Pantalla completa",
        "Exit Fullscreen" => "Salir de pantalla completa",
        "Collapse" => "Colapsar",
        "Expand" => "Expandir",
        "Center on Me" => "Centrar en mí",
        "Auto Center On" => "Autocentrado activado",
        "Auto Center Off" => "Autocentrado desactivado",
        "User Up" => "Usuario arriba",
        "North Up" => "Norte arriba",
        "Rotate Left" => "Girar a la izquierda",
        "Rotate Right" => "Girar a la derecha",
        "Enable Compass" => "Activar brújula",
        "Distance" => "Distancia",
        "Board Status" => "Estado de placas",
        "Packet Age (ms)" => "Edad del paquete (ms)",
        "Zoom Out" => "Alejar",
        "Zoom In" => "Acercar",
        "Reset" => "Restablecer",
        "Pinch or drag to navigate" => "Pellizca o arrastra para navegar",
        "Actions" => "Acciones",
        "Flight Setup" => "Configuración de vuelo",
        "Fill Targets" => "Objetivos de llenado",
        "Flight profile" => "Perfil de vuelo",
        "Apply To Flight Computer" => "Aplicar a la computadora de vuelo",
        "Save Fill Targets" => "Guardar objetivos de llenado",
        "Enable actions to edit fill targets." => {
            "Activa las acciones para editar los objetivos de llenado."
        }
        "Loading fill targets…" => "Cargando objetivos de llenado…",
        "Loading flight setup…" => "Cargando configuración de vuelo…",
        "Disable Actions is enabled. All action and flight-state buttons except Abort are disabled." => {
            "Desactivar acciones está activado. Todos los botones de acción y estado de vuelo excepto Abortar están desactivados."
        }
        "No actions are available for this user." => {
            "No hay acciones disponibles para este usuario."
        }
        "Nitrogen hold check passed. Pressure and loadcell are stable." => {
            "La verificación de retención de nitrógeno pasó. La presión y la celda de carga están estables."
        }
        "User location unavailable. Native GPS has not provided coordinates yet." => {
            "Ubicación de usuario no disponible. El GPS nativo aún no ha proporcionado coordenadas."
        }
        "Compass unavailable. Orientation permission was denied or has not initialized." => {
            "Brújula no disponible. El permiso de orientación fue denegado o aún no se inicializó."
        }
        "Topology graph is running in testing-mode simulation." => {
            "El grafo de topología está ejecutándose en simulación de modo de prueba."
        }
        "Topology graph is built from Ground Station topology and live node/link status." => {
            "El grafo de topología se construye desde la topología de Ground Station y el estado en vivo de nodos/enlaces."
        }
        "Router graph is running in testing-mode simulation." => {
            "El grafo del router está ejecutándose en simulación de modo de prueba."
        }
        "Router graph is built from the Ground Station SEDSprintf topology and live board/link status." => {
            "El grafo del router se construye desde la topología SEDSprintf de Ground Station y el estado en vivo de placas/enlaces."
        }
        _ => return None,
    })
}

fn builtin_translation_fr(text: &str) -> Option<&'static str> {
    Some(match text {
        "State" => "État",
        "Current Flight State" => "État de vol actuel",
        "Flight state" => "État de vol",
        "Fill Metrics" => "Métriques de remplissage",
        "Tank Pressure" => "Pression du réservoir",
        "Mass (kg)" => "Masse (kg)",
        "Fill Percent" => "Pourcentage de remplissage",
        "Pressure and Loadcell" => "Pression et cellule de charge",
        "Target" => "Cible",
        "Target mass (kg)" => "Masse cible (kg)",
        "Target pressure (psi)" => "Pression cible (psi)",
        "Fullscreen" => "Plein écran",
        "Exit Fullscreen" => "Quitter le plein écran",
        "Center on Me" => "Me centrer",
        "Auto Center On" => "Centrage auto activé",
        "Auto Center Off" => "Centrage auto désactivé",
        "User Up" => "Utilisateur en haut",
        "North Up" => "Nord en haut",
        "Rotate Left" => "Tourner à gauche",
        "Rotate Right" => "Tourner à droite",
        "Close" | "Dismiss" => "Fermer",
        _ => return None,
    })
}

fn drain_translation_misses(limit: usize, catalog: &HashMap<String, String>) -> Vec<String> {
    let Ok(mut pending) = TRANSLATION_MISS_QUEUE.lock() else {
        return Vec::new();
    };
    let mut batch = Vec::new();
    let keys: Vec<String> = pending.iter().cloned().collect();
    for key in keys {
        if batch.len() >= limit {
            break;
        }
        if catalog.contains_key(&key) {
            pending.remove(&key);
            continue;
        }
        pending.remove(&key);
        batch.push(key);
    }
    batch
}

fn merge_translation_map(items: HashMap<String, String>) {
    if items.is_empty() {
        return;
    }
    let mut next = TRANSLATION_CATALOG.read().clone();
    for (key, value) in items {
        if !key.trim().is_empty() && !value.trim().is_empty() {
            next.insert(key, value);
        }
    }
    *TRANSLATION_CATALOG.write() = next;
}

fn localized_theme(base: &layout::ThemeConfig, preset: &str) -> layout::ThemeConfig {
    if preset == "backend" || preset == "layout" {
        return normalize_theme_for_contrast(base);
    }
    builtin_theme_presets()
        .iter()
        .find(|definition| definition.id == preset)
        .map(|definition| normalize_theme_for_contrast(&definition.theme))
        .unwrap_or_else(|| normalize_theme_for_contrast(&layout::ThemeConfig::default()))
}

/// Returns the app-shell theme derived from the persisted preset.
///
/// Outside the live dashboard we do not have a backend-provided theme available, so
/// the "backend" preset falls back to the default theme config for shell styling.
pub fn app_shell_theme() -> layout::ThemeConfig {
    APP_THEME_CONFIG.read().clone()
}

#[cfg(not(target_arch = "wasm32"))]
#[component]
pub fn NativeSettingsPage() -> Element {
    let distance_units_metric = use_signal(|| {
        persist::get_string(MAP_DISTANCE_UNITS_STORAGE_KEY)
            .map(|v| v == "metric")
            .unwrap_or(false)
    });
    let theme_preset = use_signal(|| {
        let stored = persist::get_or(THEME_PRESET_STORAGE_KEY, "default");
        if stored == "layout" {
            "backend".to_string()
        } else {
            stored
        }
    });
    let language_code = use_signal(|| persist::get_or(LANGUAGE_STORAGE_KEY, "en"));
    let network_flow_animation_enabled =
        use_signal(|| persist::get_or(NETWORK_FLOW_ANIMATION_STORAGE_KEY, "on") != "off");
    let state_chart_labels_vertical =
        use_signal(|| persist::get_or(STATE_CHART_LABELS_VERTICAL_STORAGE_KEY, "off") == "on");
    let map_prefetch_enabled =
        use_signal(|| persist::get_or(MAP_PREFETCH_ENABLED_STORAGE_KEY, "on") != "off");
    let calibration_capture_sample_count = use_signal(|| {
        persist::get_or(CALIBRATION_CAPTURE_SAMPLE_COUNT_STORAGE_KEY, "200")
            .parse::<usize>()
            .ok()
            .unwrap_or(200)
            .clamp(1, 5_000)
    });

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
        let theme_preset = theme_preset;
        use_effect(move || {
            let theme = localized_theme(
                &layout::ThemeConfig::default(),
                theme_preset.read().as_str(),
            );
            *APP_THEME_CONFIG.write() = theme.clone();
            apply_window_theme(&theme);
        });
    }

    let theme = app_shell_theme();
    let title = localized_copy(
        &language_code.read().clone(),
        "Settings",
        "Ajustes",
        "Parametres",
    );
    let on_reset_app_data = {
        let mut distance_units_metric = distance_units_metric;
        let mut theme_preset = theme_preset;
        let mut language_code = language_code;
        let mut network_flow_animation_enabled = network_flow_animation_enabled;
        let mut state_chart_labels_vertical = state_chart_labels_vertical;
        let mut map_prefetch_enabled = map_prefetch_enabled;
        let mut calibration_capture_sample_count = calibration_capture_sample_count;
        move |_| {
            reset_local_app_data();
            distance_units_metric.set(false);
            theme_preset.set("default".to_string());
            language_code.set("en".to_string());
            network_flow_animation_enabled.set(true);
            state_chart_labels_vertical.set(false);
            map_prefetch_enabled.set(true);
            calibration_capture_sample_count.set(200);
        }
    };

    rsx! {
        SettingsPage {
            distance_units_metric,
            theme_preset,
            language_code,
            network_flow_animation_enabled,
            state_chart_labels_vertical,
            map_prefetch_enabled,
            calibration_capture_sample_count,
            theme,
            on_clear_cache: move |_| {
                clear_frontend_caches_and_reseed();
            },
            on_reset_app_data,
            title,
        }
    }
}

pub(crate) fn builtin_theme_presets() -> &'static [layout::ThemePresetDefinition] {
    &BUILTIN_THEME_CATALOG.presets
}

pub(crate) fn theme_preset_uses_backend_colors(preset: &str) -> bool {
    matches!(preset, "backend" | "layout")
}

fn parse_hex_color(value: &str) -> Option<(u8, u8, u8)> {
    let raw = value.trim().trim_start_matches('#');
    match raw.len() {
        6 => {
            let r = u8::from_str_radix(&raw[0..2], 16).ok()?;
            let g = u8::from_str_radix(&raw[2..4], 16).ok()?;
            let b = u8::from_str_radix(&raw[4..6], 16).ok()?;
            Some((r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&raw[0..2], 16).ok()?;
            let g = u8::from_str_radix(&raw[2..4], 16).ok()?;
            let b = u8::from_str_radix(&raw[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn color_to_hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn mix_color((r, g, b): (u8, u8, u8), target: (u8, u8, u8), amount: f64) -> (u8, u8, u8) {
    let blend = |from: u8, to: u8| -> u8 {
        let value = from as f64 + (to as f64 - from as f64) * amount;
        value.round().clamp(0.0, 255.0) as u8
    };
    (blend(r, target.0), blend(g, target.1), blend(b, target.2))
}

fn relative_luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let channel = |value: u8| -> f64 {
        let srgb = value as f64 / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

fn contrast_ratio(foreground: (u8, u8, u8), background: (u8, u8, u8)) -> f64 {
    let fg = relative_luminance(foreground);
    let bg = relative_luminance(background);
    let (lighter, darker) = if fg >= bg { (fg, bg) } else { (bg, fg) };
    (lighter + 0.05) / (darker + 0.05)
}

fn ensure_text_contrast(foreground: &str, background: &str, minimum: f64) -> String {
    let Some(fg) = parse_hex_color(foreground) else {
        return foreground.to_string();
    };
    let Some(bg) = parse_hex_color(background) else {
        return foreground.to_string();
    };
    if contrast_ratio(fg, bg) >= minimum {
        return color_to_hex(fg);
    }

    let bg_luminance = relative_luminance(bg);
    let target = if bg_luminance > 0.4 {
        (0_u8, 0_u8, 0_u8)
    } else {
        (255_u8, 255_u8, 255_u8)
    };
    let mut best = fg;
    for step in 1..=20 {
        let amount = step as f64 / 20.0;
        let candidate = mix_color(fg, target, amount);
        if contrast_ratio(candidate, bg) >= minimum {
            best = candidate;
            break;
        }
        best = candidate;
    }
    color_to_hex(best)
}

fn ensure_surface_separation(color: &str, against: &str, minimum: f64) -> String {
    let Some(base) = parse_hex_color(color) else {
        return color.to_string();
    };
    let Some(other) = parse_hex_color(against) else {
        return color.to_string();
    };
    if contrast_ratio(base, other) >= minimum {
        return color_to_hex(base);
    }

    let toward_dark = (0_u8, 0_u8, 0_u8);
    let toward_light = (255_u8, 255_u8, 255_u8);
    let mut best = base;
    for step in 1..=24 {
        let amount = step as f64 / 24.0;
        let dark_candidate = mix_color(base, toward_dark, amount);
        if contrast_ratio(dark_candidate, other) >= minimum {
            return color_to_hex(dark_candidate);
        }
        let light_candidate = mix_color(base, toward_light, amount);
        if contrast_ratio(light_candidate, other) >= minimum {
            return color_to_hex(light_candidate);
        }
        best = if contrast_ratio(light_candidate, other) > contrast_ratio(dark_candidate, other) {
            light_candidate
        } else {
            dark_candidate
        };
    }
    color_to_hex(best)
}

fn normalize_theme_for_contrast(theme: &layout::ThemeConfig) -> layout::ThemeConfig {
    let mut out = theme.clone();
    out.panel_background =
        ensure_surface_separation(&out.panel_background, &out.app_background, 1.08);
    out.panel_background_alt =
        ensure_surface_separation(&out.panel_background_alt, &out.panel_background, 1.12);
    out.tab_shell_background =
        ensure_surface_separation(&out.tab_shell_background, &out.app_background, 1.08);
    out.tab_shell_border =
        ensure_surface_separation(&out.tab_shell_border, &out.tab_shell_background, 1.35);
    out.button_background =
        ensure_surface_separation(&out.button_background, &out.tab_shell_background, 1.18);
    out.button_border = ensure_surface_separation(&out.button_border, &out.button_background, 1.45);
    out.border = ensure_surface_separation(&out.border, &out.panel_background, 1.22);
    out.border_soft = ensure_surface_separation(&out.border_soft, &out.panel_background, 1.12);
    out.border_strong = ensure_surface_separation(&out.border_strong, &out.panel_background, 1.35);
    out.text_primary = ensure_text_contrast(&out.text_primary, &out.app_background, 7.0);
    out.text_secondary = ensure_text_contrast(&out.text_secondary, &out.app_background, 5.0);
    out.text_muted = ensure_text_contrast(&out.text_muted, &out.app_background, 4.5);
    out.text_soft = ensure_text_contrast(&out.text_soft, &out.app_background, 4.5);
    out.button_text = ensure_text_contrast(&out.button_text, &out.button_background, 4.5);
    out.info_text = ensure_text_contrast(&out.info_text, &out.info_background, 4.5);
    out.warning_text = ensure_text_contrast(&out.warning_text, &out.warning_background, 4.5);
    out.error_text = ensure_text_contrast(&out.error_text, &out.error_background, 4.5);
    out.notification_text =
        ensure_text_contrast(&out.notification_text, &out.notification_background, 4.5);
    out
}

pub(crate) fn apply_window_theme(theme: &layout::ThemeConfig) {
    js_eval(&format!(
        r#"
        (function() {{
          try {{
            const vars = {{
              '--gs26-app-background': {app_background:?},
              '--gs26-app-text': {text_primary:?},
              '--gs26-panel-background': {panel_background:?},
              '--gs26-panel-alt-background': {panel_background_alt:?},
              '--gs26-border': {border:?},
              '--gs26-text-muted': {text_muted:?},
              '--gs26-text-secondary': {text_secondary:?},
              '--gs26-button-background': {button_background:?},
              '--gs26-button-text': {button_text:?},
            }};
            const targets = [document.documentElement, document.body, document.getElementById('main')];
            for (const target of targets) {{
              if (!target) continue;
              for (const [key, value] of Object.entries(vars)) {{
                target.style.setProperty(key, value);
              }}
              target.style.backgroundColor = vars['--gs26-app-background'];
              target.style.color = vars['--gs26-app-text'];
            }}
          }} catch (_) {{}}
        }})();
        "#,
        app_background = theme.app_background,
        text_primary = theme.text_primary,
        panel_background = theme.panel_background,
        panel_background_alt = theme.panel_background_alt,
        border = theme.border,
        text_muted = theme.text_muted,
        text_secondary = theme.text_secondary,
        button_background = theme.button_background,
        button_text = theme.button_text,
    ));
}

#[component]
fn NetworkTimeBadge(network_time: Signal<Option<NetworkTimeSync>>, language: String) -> Element {
    let tick = use_signal(|| 0u64);
    {
        let mut tick = tick;
        use_effect(move || {
            spawn(async move {
                loop {
                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(DASHBOARD_CLOCK_REFRESH_MS).await;

                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(
                        DASHBOARD_CLOCK_REFRESH_MS as u64,
                    ))
                    .await;

                    let next_tick = {
                        let current_tick = *tick.read();
                        current_tick.wrapping_add(1)
                    };
                    tick.set(next_tick);
                }
            });
        });
    }
    let _tick_snapshot = *tick.read();
    let Some(ts) = network_time
        .read()
        .as_ref()
        .copied()
        .map(compensated_network_time_ms)
        .map(format_network_time)
    else {
        return rsx! { div {} };
    };

    let label = localized_copy(&language, "Network Time", "Hora de red", "Heure réseau");
    rsx! {
        span { style: "display:inline-flex; align-items:baseline; flex:0 0 auto; min-width:0; line-height:1; vertical-align:baseline;",
            span { style: "color:#cbd5e1; display:inline-flex; align-items:baseline; white-space:nowrap;",
                "({label}:"
                span {
                    style: "display:inline-flex; align-items:baseline; width:16ch; padding-left:0.4ch; white-space:nowrap; font-family: ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace; font-variant-numeric:tabular-nums;",
                    span { "{ts}" }
                    span { ")" }
                }
            }
        }
    }
}

fn format_launch_clock_delta(ms: i64) -> String {
    let total_centis = (ms.max(0) + 5) / 10;
    let minutes = total_centis / 6_000;
    let seconds = (total_centis / 100) % 60;
    let centis = total_centis % 100;
    format!("{minutes:02}:{seconds:02}.{centis:02}")
}

fn launch_clock_tminus_remaining_ms(clock: &LaunchClockMsg, now_ms: i64) -> Option<i64> {
    let remaining = match (clock.anchor_timestamp_ms, clock.duration_ms) {
        // Backend semantics: TMinus anchor is when countdown started, not the T-0 time.
        // Hold duration before the backend provides an anchor, then clamp at zero until TPlus.
        (Some(anchor_ms), Some(duration_ms)) => {
            duration_ms.saturating_sub(now_ms.saturating_sub(anchor_ms))
        }
        // Backward-compatible fallback for older payloads that only sent the target T-0 time.
        (Some(anchor_ms), None) => anchor_ms.saturating_sub(now_ms),
        (None, Some(duration_ms)) => duration_ms,
        (None, None) => return None,
    };
    Some(remaining.clamp(0, i64::MAX))
}

fn launch_clock_tplus_anchor_ms(
    clock: &LaunchClockMsg,
    now_ms: i64,
    fallback_anchor_ms: Option<i64>,
) -> Option<i64> {
    if let Some(anchor_ms) = clock.anchor_timestamp_ms
        && anchor_ms <= now_ms.saturating_add(1_000)
    {
        return Some(anchor_ms);
    }
    fallback_anchor_ms
}

fn monotonic_tminus_display_ms(clock: &LaunchClockMsg, now_ms: Option<i64>) -> Option<i64> {
    let remaining = match now_ms {
        Some(now_ms) => launch_clock_tminus_remaining_ms(clock, now_ms)?,
        None => clock.duration_ms?,
    }
    .max(0);
    let remaining = if remaining <= LAUNCH_TMINUS_ZERO_SNAP_MS {
        0
    } else {
        remaining
    };

    let mut current_min = LAUNCH_TMINUS_DISPLAY_MIN_MS.load(Ordering::Relaxed);
    if current_min != i64::MAX
        && current_min <= LAUNCH_TMINUS_RESET_ZERO_LATCH_MS
        && remaining > current_min
    {
        LAUNCH_TMINUS_DISPLAY_MIN_MS.store(0, Ordering::Relaxed);
        LAUNCH_TMINUS_ZERO_LATCHED.store(true, Ordering::Relaxed);
        return Some(0);
    }
    while remaining < current_min {
        match LAUNCH_TMINUS_DISPLAY_MIN_MS.compare_exchange_weak(
            current_min,
            remaining,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                current_min = remaining;
                break;
            }
            Err(next_min) => current_min = next_min,
        }
    }

    let display = current_min.min(remaining);
    if display == 0 {
        LAUNCH_TMINUS_ZERO_LATCHED.store(true, Ordering::Relaxed);
    }
    Some(display)
}

fn reset_tminus_display_latch() {
    LAUNCH_TMINUS_DISPLAY_MIN_MS.store(i64::MAX, Ordering::Relaxed);
    LAUNCH_TMINUS_ZERO_LATCHED.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod launch_clock_tests {
    use super::{
        launch_clock_tminus_remaining_ms, monotonic_tminus_display_ms, reset_tminus_display_latch,
        LaunchClockKind, LaunchClockMsg,
    };
    use std::sync::Mutex;

    static MONOTONIC_TMINUS_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn tminus_holds_duration_until_backend_anchor_arrives() {
        let clock = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: None,
            duration_ms: Some(10_000),
        };

        assert_eq!(
            launch_clock_tminus_remaining_ms(&clock, 123_000),
            Some(10_000)
        );
    }

    #[test]
    fn tminus_counts_down_from_backend_start_anchor() {
        let clock = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(100_000),
            duration_ms: Some(10_000),
        };

        assert_eq!(
            launch_clock_tminus_remaining_ms(&clock, 100_000),
            Some(10_000)
        );
        assert_eq!(
            launch_clock_tminus_remaining_ms(&clock, 104_250),
            Some(5_750)
        );
    }

    #[test]
    fn tminus_clamps_at_zero_until_tplus_arrives() {
        let clock = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(100_000),
            duration_ms: Some(10_000),
        };

        assert_eq!(launch_clock_tminus_remaining_ms(&clock, 115_000), Some(0));
    }

    #[test]
    fn tminus_display_never_increases_after_backend_reset() {
        let _guard = MONOTONIC_TMINUS_TEST_LOCK.lock().unwrap();
        reset_tminus_display_latch();
        let original = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(100_000),
            duration_ms: Some(10_000),
        };
        let restarted = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(111_000),
            duration_ms: Some(10_000),
        };

        assert_eq!(
            monotonic_tminus_display_ms(&original, Some(110_500)),
            Some(0)
        );
        assert_eq!(
            monotonic_tminus_display_ms(&restarted, Some(111_000)),
            Some(0)
        );
        reset_tminus_display_latch();
    }

    #[test]
    fn tminus_display_snaps_final_milliseconds_to_zero() {
        let _guard = MONOTONIC_TMINUS_TEST_LOCK.lock().unwrap();
        reset_tminus_display_latch();
        let clock = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(100_000),
            duration_ms: Some(10_000),
        };

        assert_eq!(monotonic_tminus_display_ms(&clock, Some(109_989)), Some(0));
        reset_tminus_display_latch();
    }

    #[test]
    fn tminus_display_latches_zero_if_backend_restarts_after_final_tick_window() {
        let _guard = MONOTONIC_TMINUS_TEST_LOCK.lock().unwrap();
        reset_tminus_display_latch();
        let original = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(100_000),
            duration_ms: Some(10_000),
        };
        let restarted = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(110_050),
            duration_ms: Some(10_000),
        };

        assert_eq!(
            monotonic_tminus_display_ms(&original, Some(109_900)),
            Some(100)
        );
        assert_eq!(
            monotonic_tminus_display_ms(&restarted, Some(110_050)),
            Some(0)
        );
        reset_tminus_display_latch();
    }
}

#[component]
fn LaunchClockBadge(
    launch_clock: Signal<Option<LaunchClockMsg>>,
    network_time: Signal<Option<NetworkTimeSync>>,
) -> Element {
    let tick = use_signal(|| 0u64);
    let fallback_tplus_anchor_ms = use_signal(|| None::<i64>);
    {
        let mut tick = tick;
        use_effect(move || {
            spawn(async move {
                loop {
                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(DASHBOARD_CLOCK_REFRESH_MS).await;

                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(
                        DASHBOARD_CLOCK_REFRESH_MS as u64,
                    ))
                    .await;

                    let next = {
                        let current = *tick.read();
                        current.wrapping_add(1)
                    };
                    tick.set(next);
                }
            });
        });
    }
    {
        let launch_clock = launch_clock;
        let network_time = network_time;
        let mut fallback_tplus_anchor_ms = fallback_tplus_anchor_ms;
        use_effect(move || {
            let _tick_snapshot = *tick.read();
            let clock = launch_clock.read().clone();
            let now_ms = network_time
                .read()
                .as_ref()
                .copied()
                .map(compensated_network_time_ms);
            let fallback_anchor = *fallback_tplus_anchor_ms.read();

            match clock {
                Some(clock) => match clock.kind {
                    LaunchClockKind::Idle | LaunchClockKind::TMinus => {
                        if fallback_anchor.is_some() {
                            fallback_tplus_anchor_ms.set(None);
                        }
                        if clock.kind == LaunchClockKind::TMinus {
                            let _ = monotonic_tminus_display_ms(&clock, now_ms);
                        }
                    }
                    LaunchClockKind::TPlus => {
                        reset_tminus_display_latch();
                        let backend_anchor = now_ms
                            .and_then(|now_ms| launch_clock_tplus_anchor_ms(&clock, now_ms, None));
                        if backend_anchor.is_some() {
                            if fallback_anchor.is_some() {
                                fallback_tplus_anchor_ms.set(None);
                            }
                        } else if fallback_anchor.is_none() {
                            fallback_tplus_anchor_ms.set(now_ms);
                        }
                    }
                },
                None => {
                    if fallback_anchor.is_some() {
                        fallback_tplus_anchor_ms.set(None);
                    }
                }
            }
        });
    }
    let _tick_snapshot = *tick.read();
    let clock = launch_clock.read().clone();
    let now_ms = network_time
        .read()
        .as_ref()
        .copied()
        .map(compensated_network_time_ms);

    let label = match clock.as_ref().map(|clock| clock.kind) {
        Some(LaunchClockKind::TPlus) => "T+",
        _ => "T-",
    };
    let tminus_zero_latched = LAUNCH_TMINUS_ZERO_LATCHED.load(Ordering::Relaxed);
    let display = match (clock.as_ref(), now_ms) {
        (Some(clock), Some(now_ms)) => match clock.kind {
            LaunchClockKind::Idle if tminus_zero_latched => format_launch_clock_delta(0),
            LaunchClockKind::Idle => format_launch_clock_delta(
                clock
                    .duration_ms
                    .unwrap_or(DEFAULT_LAUNCH_COUNTDOWN_DURATION_MS),
            ),
            LaunchClockKind::TMinus => format_launch_clock_delta(
                monotonic_tminus_display_ms(clock, Some(now_ms)).unwrap_or(0),
            ),
            LaunchClockKind::TPlus => {
                let anchor_ms =
                    launch_clock_tplus_anchor_ms(clock, now_ms, *fallback_tplus_anchor_ms.read())
                        .unwrap_or(now_ms);
                format_launch_clock_delta(now_ms.saturating_sub(anchor_ms))
            }
        },
        (Some(clock), None) => match clock.kind {
            LaunchClockKind::Idle | LaunchClockKind::TMinus if tminus_zero_latched => {
                format_launch_clock_delta(0)
            }
            LaunchClockKind::TMinus => clock
                .duration_ms
                .and_then(|_| monotonic_tminus_display_ms(clock, None))
                .map(format_launch_clock_delta)
                .unwrap_or_else(|| "--:--.-".to_string()),
            LaunchClockKind::TPlus => "--:--.-".to_string(),
            LaunchClockKind::Idle => format_launch_clock_delta(
                clock
                    .duration_ms
                    .unwrap_or(DEFAULT_LAUNCH_COUNTDOWN_DURATION_MS),
            ),
        },
        (None, _) => format_launch_clock_delta(DEFAULT_LAUNCH_COUNTDOWN_DURATION_MS),
    };

    let value_color = if display.starts_with("--:--") {
        "#94a3b8"
    } else {
        match clock.as_ref().map(|clock| clock.kind) {
            Some(LaunchClockKind::TPlus) => "#38bdf8",
            _ => "#2dd4bf",
        }
    };

    rsx! {
        span { class: "gs26-launch-clock",
            span { "({label}:" }
            span {
                class: "gs26-launch-clock-value",
                style: "color:{value_color};",
                "{display}"
            }
            span { ")" }
        }
    }
}
// tab <-> string
/// Converts a dashboard tab enum into its persisted string id.
fn _main_tab_to_str(tab: MainTab) -> &'static str {
    match tab {
        MainTab::State => "state",
        MainTab::ConnectionStatus => "connection-status",
        MainTab::Detailed => "detailed",
        MainTab::NetworkTopology => "network-topology",
        MainTab::Map => "map",
        MainTab::Actions => "actions",
        MainTab::Calibration => "calibration",
        MainTab::Notifications => "notifications",
        MainTab::Warnings => "warnings",
        MainTab::Errors => "errors",
        MainTab::Data => "data",
    }
}

/// Returns the default label for a dashboard tab when the layout config does not override it.
fn _default_main_tab_label(tab: MainTab) -> String {
    let lang = current_language();
    match tab {
        MainTab::State => localized_copy(&lang, "Flight", "Vuelo", "Vol"),
        MainTab::ConnectionStatus => localized_copy(
            &lang,
            "Connection Status",
            "Estado de Conexion",
            "Etat Connexion",
        ),
        MainTab::Detailed => {
            localized_copy(&lang, "Detailed Info", "Info Detallada", "Infos Detaillees")
        }
        MainTab::NetworkTopology => localized_copy(
            &lang,
            "Network Topology",
            "Topologia Red",
            "Topologie Reseau",
        ),
        MainTab::Map => localized_copy(&lang, "Map", "Mapa", "Carte"),
        MainTab::Actions => localized_copy(&lang, "Actions", "Acciones", "Actions"),
        MainTab::Calibration => localized_copy(&lang, "Calibration", "Calibracion", "Calibration"),
        MainTab::Notifications => {
            localized_copy(&lang, "Notifications", "Notificaciones", "Notifications")
        }
        MainTab::Warnings => localized_copy(&lang, "Warnings", "Avisos", "Alertes"),
        MainTab::Errors => localized_copy(&lang, "Errors", "Errores", "Erreurs"),
        MainTab::Data => localized_copy(&lang, "Data", "Datos", "Donnees"),
    }
}

/// Resolves the visible label for a dashboard tab from the loaded layout config.
fn _main_tab_label(layout: &LayoutConfig, tab: MainTab) -> String {
    layout
        .branding
        .tab_labels
        .get(_main_tab_to_str(tab))
        .map(|label| translate_text(label))
        .unwrap_or_else(|| _default_main_tab_label(tab))
}

/// Resolves the title shown at the top of the dashboard.
fn _dashboard_title(layout: &LayoutConfig) -> String {
    layout
        .branding
        .dashboard_title
        .clone()
        .or_else(|| layout.branding.app_name.clone())
        .map(|title| translate_text(&title))
        .unwrap_or_else(|| {
            let lang = current_language();
            localized_copy(
                &lang,
                "Telemetry Dashboard",
                "Panel de Telemetria",
                "Tableau Telemetrie",
            )
        })
}
/// Converts a persisted tab id back into the corresponding enum.
fn _main_tab_from_str(s: &str) -> MainTab {
    match s {
        "state" => MainTab::State,
        "connection-status" => MainTab::ConnectionStatus,
        "detailed" => MainTab::Detailed,
        "network-topology" => MainTab::NetworkTopology,
        "map" => MainTab::Map,
        "actions" => MainTab::Actions,
        "calibration" => MainTab::Calibration,
        "notifications" => MainTab::Notifications,
        "warnings" => MainTab::Warnings,
        "errors" => MainTab::Errors,
        "data" => MainTab::Data,
        _ => MainTab::State,
    }
}

/// Returns whether a tab is enabled by the loaded layout config.
fn _layout_main_tab_enabled(layout: &LayoutConfig, tab: MainTab) -> bool {
    let listed = layout
        .main_tabs
        .iter()
        .any(|id| _main_tab_from_str(id) == tab);
    listed && (tab != MainTab::NetworkTopology || layout.network_tab.enabled)
}

/// Returns whether the actions tab has at least one command the current session may send.
fn _actions_tab_has_visible_actions(layout: &LayoutConfig, abort_only_mode: bool) -> bool {
    let _ = abort_only_mode;
    auth::can_view_actions() && !layout.actions_tab.actions.is_empty()
}

fn _calibration_tab_visible(calibration_has_sensors: Option<bool>) -> bool {
    auth::can_view_calibration() && calibration_has_sensors.unwrap_or(true)
}

/// Computes the final visible tab list after applying layout and auth filtering.
fn _configured_main_tabs(
    layout: &LayoutConfig,
    abort_only_mode: bool,
    calibration_has_sensors: Option<bool>,
) -> Vec<MainTab> {
    let mut tabs = Vec::new();
    for id in &layout.main_tabs {
        let tab = _main_tab_from_str(id);
        if !_layout_main_tab_enabled(layout, tab) || tabs.contains(&tab) {
            continue;
        }
        if tab == MainTab::Actions && !_actions_tab_has_visible_actions(layout, abort_only_mode) {
            continue;
        }
        if tab == MainTab::Calibration && !_calibration_tab_visible(calibration_has_sensors) {
            continue;
        }
        tabs.push(tab);
    }
    if tabs.is_empty() {
        tabs.push(MainTab::State);
    }
    tabs
}

// ---------- Base URL config ----------
pub struct UrlConfig;

impl UrlConfig {
    /// Normalizes and persists the backend base URL selected by the operator.
    pub fn set_base_url_and_persist(url: String) {
        let clean = normalize_base_url(url);
        *BASE_URL.write() = clean.clone();
        persist::set_string(BASE_URL_STORAGE_KEY, &clean);
    }

    /// Returns the stored backend base URL when one exists.
    pub fn _stored_base_url() -> Option<String> {
        persist::get_string(BASE_URL_STORAGE_KEY)
            .map(normalize_base_url)
            .filter(|s| !s.trim().is_empty())
    }

    /// Returns the current HTTP base URL, including platform-specific defaults.
    pub fn base_http() -> String {
        // load from storage key if present
        let base = persist::get_string(BASE_URL_STORAGE_KEY)
            .map(normalize_base_url)
            .unwrap_or_else(|| BASE_URL.read().clone());

        #[cfg(target_arch = "wasm32")]
        if base.is_empty()
            && let Some(window) = web_sys::window()
            && let Ok(origin) = window.location().origin()
        {
            return normalize_base_url(origin);
        }

        #[cfg(not(target_arch = "wasm32"))]
        if base.is_empty() {
            return "http://localhost:3000".to_string();
        }

        base
    }

    /// Returns ws/wss scheme + host[:port] (no path).
    pub fn base_ws() -> String {
        #[cfg(target_arch = "wasm32")]
        {
            let base_http = BASE_URL.read().clone();
            if base_http.is_empty() {
                if let Some(window) = web_sys::window() {
                    let loc = window.location();
                    let protocol = loc.protocol().unwrap_or_else(|_| "http:".to_string());
                    let host = loc.host().unwrap_or_else(|_| "localhost:3000".to_string());
                    let ws_scheme = if protocol == "https:" { "wss" } else { "ws" };
                    return format!("{ws_scheme}://{host}");
                }
                return "ws://localhost:3000".to_string();
            }
        }

        let base_http = UrlConfig::base_http().trim_end_matches('/').to_string();

        if base_http.starts_with("https://") {
            base_http.replacen("https://", "wss://", 1)
        } else if base_http.starts_with("http://") {
            base_http.replacen("http://", "ws://", 1)
        } else if base_http.starts_with("wss://") || base_http.starts_with("ws://") {
            base_http
        } else {
            format!("ws://{base_http}")
        }
    }

    /// Persists the TLS validation override for a specific backend base URL.
    pub fn _set_skip_tls_verify_for_base(base: &str, value: bool) {
        let clean = normalize_base_url(base.to_string());
        if clean.is_empty() {
            return;
        }
        let key = _tls_skip_key(&clean);
        persist::set_string(&key, if value { "true" } else { "false" });
    }

    /// Returns whether TLS validation is disabled for a specific backend base URL.
    pub fn _skip_tls_verify_for_base(base: &str) -> bool {
        let clean = normalize_base_url(base.to_string());
        if clean.is_empty() {
            return false;
        }
        let key = _tls_skip_key(&clean);
        persist::get_string(&key)
            .map(|v| v == "true")
            .unwrap_or(false)
    }

    /// Persists the TLS validation override for the currently selected backend base URL.
    pub fn _set_skip_tls_verify(value: bool) {
        let base = UrlConfig::base_http();
        UrlConfig::_set_skip_tls_verify_for_base(&base, value);
    }

    /// Returns whether TLS validation is disabled for the currently selected backend base URL.
    pub fn _skip_tls_verify() -> bool {
        let base = UrlConfig::base_http();
        UrlConfig::_skip_tls_verify_for_base(&base)
    }
}

/// Builds the persistence key used for the per-backend TLS validation override.
fn _tls_skip_key(base: &str) -> String {
    let mut cleaned = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_ascii_alphanumeric() {
            cleaned.push(ch.to_ascii_lowercase());
        } else {
            cleaned.push('_');
        }
    }
    format!("{_SKIP_TLS_VERIFY_KEY_PREFIX}{cleaned}")
}

static BASE_URL: GlobalSignal<String> = Signal::global(String::new);

/// Restarts the WebSocket connection and triggers a fresh telemetry reseed.
fn reconnect_and_reload_ui() {
    // Always restart websockets/tasks
    bump_ws_epoch();
    bump_seed_epoch();

    // Native: keep current UI mounted so charts/history remain visible while reseed runs.
}

/// Mirrors the explicit reload button behavior before reconnecting to a backend.
#[cfg(not(target_arch = "wasm32"))]
pub fn clear_and_reconnect_after_connect() {
    hard_reload_dashboard_data();
}

#[cfg(not(target_arch = "wasm32"))]
/// Returns whether the dashboard has ever reached a live backend connection in this process.
pub fn dashboard_has_prior_backend_connection() -> bool {
    DASHBOARD_HAS_CONNECTED.load(Ordering::Relaxed)
}

/// Restarts backend-backed frontend state after login or logout changes.
pub fn reconnect_and_reseed_after_auth_change() {
    reconnect_and_reload_ui();
}

#[cfg(target_arch = "wasm32")]
/// Returns whether the browser should keep dashboard background tasks running on this route.
fn web_dashboard_runtime_allowed() -> bool {
    web_sys::window()
        .and_then(|window| window.location().pathname().ok())
        .map(|path| path != "/login")
        .unwrap_or(true)
}

#[cfg(not(target_arch = "wasm32"))]
/// Native builds always allow the dashboard runtime.
fn web_dashboard_runtime_allowed() -> bool {
    true
}

/// Clears runtime telemetry buffers before a reconnect or reseed.
fn clear_telemetry_runtime_buffers() {
    if let Ok(mut q) = TELEMETRY_QUEUE.lock() {
        q.clear();
    }
}

fn clear_visible_telemetry_history() {
    let snapshot = ui_telemetry_rows_snapshot();
    if let Ok(mut bridge) = RESEED_HISTORY_BRIDGE.lock() {
        if let Some(last) = snapshot.last() {
            let cutoff = last.timestamp_ms - 15_000;
            *bridge = snapshot
                .into_iter()
                .filter(|row| row.timestamp_ms >= cutoff)
                .collect();
        } else {
            bridge.clear();
        }
    }
    if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
        store.replace_from_rows(&[]);
    }
    persist::_remove(TELEMETRY_CACHE_STORAGE_KEY);
    LAST_TELEMETRY_CACHE_PERSIST_MS.store(0, Ordering::Relaxed);
    reset_latest_telemetry(&[]);
    charts_cache_clear_active();
    bump_render_epoch();
}

pub fn hard_reload_dashboard_data() {
    clear_telemetry_runtime_buffers();
    clear_visible_telemetry_history();
    set_reseed_status_running();
    #[cfg(not(target_arch = "wasm32"))]
    charts_cache_request_refit();
    reconnect_and_reload_ui();
}

// ---------- Cross-platform WS handle ----------
#[derive(Clone)]
struct WsSender {
    #[cfg(target_arch = "wasm32")]
    ws: web_sys::WebSocket,

    #[cfg(not(target_arch = "wasm32"))]
    tx: tokio::sync::mpsc::UnboundedSender<String>,
}

impl WsSender {
    /// Sends a command over the current WebSocket transport.
    fn send_cmd(&self, cmd: &str) -> Result<(), String> {
        let msg = format!(r#"{{"cmd":"{}"}}"#, cmd);

        #[cfg(target_arch = "wasm32")]
        {
            self.ws
                .send_with_str(&msg)
                .map_err(|_| "ws send failed".to_string())?;
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.tx
                .send(msg)
                .map_err(|_| "ws channel closed".to_string())?;
        }

        Ok(())
    }
}

static WS_SENDER: GlobalSignal<Option<WsSender>> = Signal::global(|| None::<WsSender>);

// ============================================================================
// OUTER component: owns “real mount” lifetime & publishes it into DASHBOARD_LIFE
// INNER component is keyed for native “reload UI” without tripping outer Drop.
// ============================================================================
#[component]
/// Outer dashboard component that owns the real mount lifetime.
pub fn TelemetryDashboard() -> Element {
    // Create once per real mount
    *DASHBOARD_LIFE.write() = DashboardLife::new_alive();

    log!(
        "[UI] TelemetryDashboard mounted (alive=true, gen={})",
        dashboard_gen()
    );

    rsx! {
        TelemetryDashboardInner {}
    }
}

// ---------- INNER dashboard (this is what we remount on native reload) ----------
#[component]
/// Inner dashboard component that owns the live UI state and background tasks.
fn TelemetryDashboardInner() -> Element {
    // Always valid; becomes “real” once outer publishes it.
    let alive = dashboard_alive();
    let _restored_cached_rows = use_signal(restore_cached_telemetry_rows_if_needed);

    // ----------------------------
    // Persistent values (strings)
    // ----------------------------
    let st_warn_ack = use_signal(|| persist::get_or(WARNING_ACK_STORAGE_KEY, "0"));
    let st_err_ack = use_signal(|| persist::get_or(ERROR_ACK_STORAGE_KEY, "0"));
    let st_main_tab = use_signal(|| persist::get_or(MAIN_TAB_STORAGE_KEY, "state"));
    let st_data_tab = use_signal(|| persist::get_or(DATA_TAB_STORAGE_KEY, "GYRO_DATA"));
    let st_base_url = use_signal(|| persist::get_or(BASE_URL_STORAGE_KEY, ""));
    let distance_units_metric = use_signal(|| {
        persist::get_string(MAP_DISTANCE_UNITS_STORAGE_KEY)
            .map(|v| v == "metric")
            .unwrap_or(false)
    });
    let theme_preset = use_signal(|| {
        let stored = persist::get_or(THEME_PRESET_STORAGE_KEY, "default");
        if stored == "layout" {
            "backend".to_string()
        } else {
            stored
        }
    });
    let language_code = use_signal(|| persist::get_or(LANGUAGE_STORAGE_KEY, "en"));
    let network_flow_animation_enabled =
        use_signal(|| persist::get_or(NETWORK_FLOW_ANIMATION_STORAGE_KEY, "on") != "off");
    let state_chart_labels_vertical =
        use_signal(|| persist::get_or(STATE_CHART_LABELS_VERTICAL_STORAGE_KEY, "off") == "on");
    let map_prefetch_enabled =
        use_signal(|| persist::get_or(MAP_PREFETCH_ENABLED_STORAGE_KEY, "on") != "off");
    let calibration_capture_sample_count = use_signal(|| {
        persist::get_or(CALIBRATION_CAPTURE_SAMPLE_COUNT_STORAGE_KEY, "200")
            .parse::<usize>()
            .ok()
            .unwrap_or(200)
            .clamp(1, 5_000)
    });

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
    let dismissed_notifications = use_signal(load_dismissed_notifications);
    let unread_notification_ids = use_signal(Vec::<u64>::new);
    let action_policy = use_signal(ActionPolicyMsg::default_locked);
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
    #[cfg(not(target_arch = "wasm32"))]
    let show_version_overlay = use_signal(|| false);

    let active_main_tab = use_signal(|| _main_tab_from_str(st_main_tab.read().as_str()));

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
            let epoch = *WS_EPOCH.read();
            spawn(async move {
                while alive.load(Ordering::Relaxed) && *WS_EPOCH.read() == epoch {
                    if *active_main_tab.read() == MainTab::Detailed {
                        frontend_network_metrics.set(frontend_network_metrics_snapshot());
                    }
                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(1_000).await;
                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            });
        });
    }

    {
        let mut active_main_tab = active_main_tab;
        let layout_config = layout_config;
        let abort_only_mode = abort_only_mode;
        let calibration_has_sensors = calibration_has_sensors;
        use_effect(move || {
            let Some(layout) = layout_config.read().clone() else {
                return;
            };
            let current = *active_main_tab.read();
            let configured = _configured_main_tabs(
                &layout,
                *abort_only_mode.read(),
                *calibration_has_sensors.read(),
            );
            if !configured.contains(&current) {
                let next = configured.into_iter().next().unwrap_or(MainTab::State);
                active_main_tab.set(next);
            }
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
            spawn(async move {
                match http_get_json::<CalibrationTabLayout>("/api/calibration_config").await {
                    Ok(layout) => calibration_has_sensors.set(Some(!layout.sensors.is_empty())),
                    Err(_) => calibration_has_sensors.set(Some(false)),
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
    let ack_warning_count = use_signal(|| 0u64);
    let ack_error_count = use_signal(|| 0u64);

    let flash_on = use_signal(|| false);
    let rocket_gps = use_signal(|| {
        ui_telemetry_rows_snapshot()
            .iter()
            .rev()
            .find_map(row_to_gps)
    });
    let user_gps = use_signal(|| None::<(f64, f64)>);

    {
        let rocket_gps = rocket_gps;
        let user_gps = user_gps;
        use_effect(move || {
            let tiles_url = map_tiles_url();
            let tiles_js = serde_json::to_string(&tiles_url).unwrap_or_else(|_| "\"\"".to_string());
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
                if RESTORED_TELEMETRY_CACHE_NEEDS_CHART_REBUILD.swap(false, Ordering::Relaxed) {
                    rebuild_chart_cache_from_visible_rows();
                }
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
                        if RESTORED_TELEMETRY_CACHE_NEEDS_CHART_REBUILD
                            .swap(false, Ordering::Relaxed)
                        {
                            rebuild_chart_cache_from_visible_rows();
                        }
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
            persist::set_string(MAIN_TAB_STORAGE_KEY, &s);
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
                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(300).await;

                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

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

    // ------------------------------------------------------------------------
    // UI flush loop: drain telemetry queue into `rows` at a fixed cadence
    // ------------------------------------------------------------------------
    {
        let alive = alive.clone();
        let active_main_tab = active_main_tab;

        use_effect(move || {
            let alive = alive.clone();
            let active_main_tab = active_main_tab;
            let epoch = *WS_EPOCH.read();

            spawn(async move {
                // Keep telemetry and charts responsive by default; operators can still override
                // cadence through env vars on slower devices.
                let tick_ms: u32 = std::env::var("GS_UI_TICK_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(100)
                    .clamp(16, 500);
                let chart_tick_ms: u32 = std::env::var("GS_CHART_TICK_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(250)
                    .clamp(50, 2_000);
                let chart_every = chart_tick_ms.div_ceil(tick_ms).max(1);
                let mut chart_tick_counter: u32 = 0;

                while alive.load(Ordering::Relaxed) && *WS_EPOCH.read() == epoch {
                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(tick_ms).await;

                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(tick_ms as u64)).await;

                    if !alive.load(Ordering::Relaxed) || *WS_EPOCH.read() != epoch {
                        break;
                    }

                    // Drain queued telemetry in one move to minimize lock hold time and copies.
                    let drained: Vec<TelemetryRow> = if let Ok(mut q) = TELEMETRY_QUEUE.lock() {
                        std::mem::take(&mut *q).into_iter().collect()
                    } else {
                        Vec::new()
                    };

                    if drained.is_empty() {
                        continue;
                    }

                    if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
                        store.apply_rows(drained);
                    }
                    persist_cached_telemetry_snapshot_if_due(false);
                    bump_telemetry_render_epoch();
                    chart_tick_counter = chart_tick_counter.saturating_add(1);
                    if chart_tick_counter >= chart_every {
                        let chart_tab_visible =
                            matches!(*active_main_tab.read(), MainTab::Data | MainTab::State);
                        if chart_tab_visible {
                            bump_chart_render_epoch();
                        }
                        chart_tick_counter = 0;
                    }
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
        let mut user_gps_s = user_gps;
        let mut ack_warning_ts_s = ack_warning_ts;
        let mut ack_error_ts_s = ack_error_ts;
        let mut notifications_s = notifications;
        let mut notification_history_s = notification_history;
        let mut dismissed_notifications_s = dismissed_notifications;
        let mut unread_notification_ids_s = unread_notification_ids;
        let mut action_policy_s = action_policy;
        let mut fill_targets_s = fill_targets;
        let mut network_time_s = network_time;
        let mut launch_clock_s = launch_clock;
        let mut network_topology_s = network_topology;

        let alive = alive.clone();
        let startup_seed_ready = startup_seed_ready;

        use_effect(move || {
            let alive = alive.clone();
            spawn(async move {
                let mut handled_seed_epoch: Option<u64> = None;
                while alive.load(Ordering::Relaxed) {
                    // Initial seed waits until layout has loaded and the startup delay completes.
                    if !*startup_seed_ready.read() {
                        #[cfg(target_arch = "wasm32")]
                        gloo_timers::future::TimeoutFuture::new(150).await;

                        #[cfg(not(target_arch = "wasm32"))]
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                        continue;
                    }

                    let seed_epoch = *SEED_EPOCH.read();
                    if handled_seed_epoch == Some(seed_epoch) {
                        #[cfg(target_arch = "wasm32")]
                        gloo_timers::future::TimeoutFuture::new(150).await;

                        #[cfg(not(target_arch = "wasm32"))]
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                        continue;
                    }
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
                            &mut dismissed_notifications_s,
                            &mut unread_notification_ids_s,
                            &mut action_policy_s,
                            &mut fill_targets_s,
                            &mut network_time_s,
                            &mut launch_clock_s,
                            &mut network_topology_s,
                            &mut board_status_s,
                            &mut rocket_gps_s,
                            &mut user_gps_s,
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
            });
        });
    }

    // Flash loop
    {
        let mut flash_on = flash_on;
        let alive = alive.clone();

        use_effect(move || {
            let alive = alive.clone();
            let epoch = *WS_EPOCH.read();
            spawn(async move {
                while alive.load(Ordering::Relaxed) && *WS_EPOCH.read() == epoch {
                    #[cfg(target_arch = "wasm32")]
                    gloo_timers::future::TimeoutFuture::new(500).await;

                    #[cfg(not(target_arch = "wasm32"))]
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                    if !alive.load(Ordering::Relaxed) || *WS_EPOCH.read() != epoch {
                        break;
                    }

                    let next = {
                        let current = *flash_on.read();
                        !current
                    };
                    flash_on.set(next);
                }
            });
        });
    }

    // Derived state
    let warn_count = warnings.read().len();
    let err_count = errors.read().len();

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
    let has_unread_notifications = !unread_notification_ids.read().is_empty();

    let has_unacked_warnings = latest_warning_ts > 0
        && (latest_warning_ts > *ack_warning_ts.read()
            || *warning_event_counter.read() > *ack_warning_count.read());
    let has_unacked_errors = latest_error_ts > 0
        && (latest_error_ts > *ack_error_ts.read()
            || *error_event_counter.read() > *ack_error_count.read());

    let border_style = "1px solid transparent";
    let shell_alert_effect = if has_unacked_errors && *flash_on.read() {
        "inset 0 0 0 2px #ef4444"
    } else if has_unacked_errors && has_errors {
        "inset 0 0 0 1px #ef4444"
    } else if has_unacked_warnings && *flash_on.read() {
        "inset 0 0 0 2px #facc15"
    } else if has_unacked_warnings && has_warnings {
        "inset 0 0 0 1px #facc15"
    } else {
        "none"
    };
    let warnings_tab_icon_style = if has_unacked_warnings && *flash_on.read() {
        "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#facc15; opacity:1;".to_string()
    } else if has_unacked_warnings {
        "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#facc15; opacity:0.4;".to_string()
    } else if has_warnings {
        "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#94a3b8; opacity:1;".to_string()
    } else {
        "display:none;".to_string()
    };
    let errors_tab_icon_style = if has_unacked_errors && *flash_on.read() {
        "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#fecaca; opacity:1;".to_string()
    } else if has_unacked_errors {
        "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#fecaca; opacity:0.4;".to_string()
    } else if has_errors {
        "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#94a3b8; opacity:1;".to_string()
    } else {
        "display:none;".to_string()
    };
    let notifications_tab_icon_style = if has_unread_notifications {
        "margin-left:6px; width:1.2em; display:inline-flex; justify-content:center; color:#bfdbfe; opacity:1;".to_string()
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
    let show_ack_warnings = *active_main_tab.read() == MainTab::Warnings && has_warnings;
    let show_ack_errors = *active_main_tab.read() == MainTab::Errors && has_errors;
    let ack_button_visible = show_ack_warnings || show_ack_errors;
    let ack_button_label = if show_ack_warnings {
        translate_text("Acknowledge warnings")
    } else if show_ack_errors {
        translate_text("Acknowledge errors")
    } else {
        "Acknowledge".to_string()
    };
    let ack_button_style = format!(
        "
            margin-left:auto;
            padding:0.25rem 0.7rem;
            border-radius:999px;
            border:1px solid #4b5563;
            background:#020617;
            color:#e5e7eb;
            font-size:0.75rem;
            cursor:{};
            visibility:{};
        ",
        if ack_button_visible {
            "pointer"
        } else {
            "default"
        },
        if ack_button_visible {
            "visible"
        } else {
            "hidden"
        },
    );
    let network_time_visible = network_time.read().is_some();

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

    // Checking the Notifications tab dismisses currently active notifications
    // and clears the unread indicator.
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
                    notifications,
                    notification_history,
                    dismissed_notifications,
                    unread_notification_ids,
                    action_policy,
                    fill_targets,
                    network_time,
                    launch_clock,
                    network_topology,
                    warning_event_counter,
                    error_event_counter,
                    flight_state,
                    board_status,
                    rocket_gps,
                    user_gps,
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
        #[cfg(target_arch = "wasm32")]
        {
            rsx! { div {} }
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let show_version_overlay = show_version_overlay;
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
                            show_version_overlay.set(true);
                        }
                    },
                    ontouchend: {
                        let mut show_version_overlay = show_version_overlay;
                        move |_| {
                            show_version_overlay.set(true);
                        }
                    },
                    {translate_text("VERSION")}
                }
            }
        }
    };

    let settings_button: Element = {
        let show_settings_overlay = show_settings_overlay;
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
                        show_settings_overlay.set(true);
                    }
                },
                ontouchend: {
                    let mut show_settings_overlay = show_settings_overlay;
                    move |_| {
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
    let mut _refresh_layout = move || {
        let base = UrlConfig::base_http();
        let cache_key = layout_cache_key_for_base(&base);
        layout_request_base.set(String::new());
        layout_loading.set(true);
        layout_error.set(None);
        layout_error_dismissed.set(None);
        persist::_remove(&cache_key);
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
                    if RESTORED_TELEMETRY_CACHE_NEEDS_CHART_REBUILD.swap(false, Ordering::Relaxed) {
                        rebuild_chart_cache_from_visible_rows();
                    }
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

    let reload_button: Element = rsx! {
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
                _refresh_layout();
                hard_reload_dashboard_data();
            },
            "{reload_button_label}"
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
    #[cfg(not(target_arch = "wasm32"))]
    let version_overlay_open = *show_version_overlay.read();
    let version_overlay: Element = {
        #[cfg(target_arch = "wasm32")]
        {
            rsx! { div {} }
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            if version_overlay_open {
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
                            theme_preset: theme_preset,
                            language_code: language_code,
                            network_flow_animation_enabled: network_flow_animation_enabled,
                            state_chart_labels_vertical: state_chart_labels_vertical,
                            map_prefetch_enabled: map_prefetch_enabled,
                            calibration_capture_sample_count: calibration_capture_sample_count,
                            theme: theme.clone(),
                            on_clear_cache: move |_| {
                                clear_frontend_caches_and_reseed();
                            },
                            on_reset_app_data: {
                                let mut st_warn_ack = st_warn_ack;
                                let mut st_err_ack = st_err_ack;
                                let mut st_main_tab = st_main_tab;
                                let mut st_data_tab = st_data_tab;
                                let mut st_base_url = st_base_url;
                                let mut distance_units_metric = distance_units_metric;
                                let mut theme_preset = theme_preset;
                                let mut language_code = language_code;
                                let mut network_flow_animation_enabled = network_flow_animation_enabled;
                                let mut state_chart_labels_vertical = state_chart_labels_vertical;
                                let mut map_prefetch_enabled = map_prefetch_enabled;
                                let mut calibration_capture_sample_count = calibration_capture_sample_count;
                                move |_| {
                                    reset_local_app_data();
                                    st_warn_ack.set("0".to_string());
                                    st_err_ack.set("0".to_string());
                                    st_main_tab.set("state".to_string());
                                    st_data_tab.set("GYRO_DATA".to_string());
                                    st_base_url.set(String::new());
                                    distance_units_metric.set(false);
                                    theme_preset.set("default".to_string());
                                    language_code.set("en".to_string());
                                    network_flow_animation_enabled.set(true);
                                    state_chart_labels_vertical.set(false);
                                    map_prefetch_enabled.set(true);
                                    calibration_capture_sample_count.set(200);
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
                // Only needed if you want to gate geolocation until the JS is ready on wasm:
                js_ready: Some(start_gps_js()),
            }
                style {
                    "@keyframes gs26-blink-slow-off {{ 0%, 100% {{ opacity: 0.2; }} 18% {{ opacity: 1.0; }} }}
             @keyframes gs26-blink-slow-on  {{ 0%, 100% {{ opacity: 1.0; }} 82% {{ opacity: 0.25; }} }}
             @keyframes gs26-blink-fast-off {{ 0%, 100% {{ opacity: 0.15; }} 45% {{ opacity: 1.0; }} }}
             @keyframes gs26-blink-fast-on  {{ 0%, 100% {{ opacity: 1.0; }} 55% {{ opacity: 0.2; }} }}
             .gs26-tab-shell {{ min-width:260px; }}
             .gs26-tab-toggle {{ display:none; }}
             .gs26-tab-nav {{ display:flex; gap:0.5rem; flex-wrap:wrap; }}
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
             @media (max-width: 720px) {{
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
             .gs26-header-actions-shell {{ margin-left:auto; position:relative; z-index:2000; }}
             .gs26-header-actions-list {{ display:flex; align-items:center; gap:10px; flex-wrap:wrap; }}
             .gs26-header-menu-toggle {{ display:none; }}
             @media (max-width: 900px) {{
               .gs26-header-actions-shell {{
                 display:flex;
                 align-items:center;
                 justify-content:flex-end;
               }}
               .gs26-header-menu-toggle {{
                 display:inline-flex;
                 align-items:center;
                 justify-content:center;
                 padding:0.4rem 0.7rem;
                 border-radius:0.75rem;
                 border:1px solid var(--gs26-header-menu-border);
                 background:var(--gs26-header-menu-background);
                 color:var(--gs26-header-menu-text);
                 font:inherit;
                 font-weight:800;
                 cursor:pointer;
               }}
               .gs26-header-actions-list {{
                 display:none;
                 position:absolute;
                 top:calc(100% + 8px);
                 right:0;
                 z-index:60;
                 min-width:min(320px, calc(100vw - 32px));
                 max-width:calc(100vw - 32px);
                 padding:0.8rem;
                 border-radius:0.9rem;
                 border:1px solid var(--gs26-header-menu-border);
                 background:var(--gs26-header-menu-background);
                 box-shadow:0 18px 40px rgba(0,0,0,0.4);
                 flex-direction:column;
                 align-items:stretch;
                 gap:8px;
               }}
               .gs26-header-actions-shell[data-expanded=\"true\"] .gs26-header-actions-list {{
                 display:flex;
               }}
               .gs26-header-actions-list button {{
                 width:100%;
                 margin-left:0 !important;
               }}
             }}
             @media (max-width: 720px), (max-height: 780px) {{
               .gs26-tab-shell {{
                 flex:1 1 100%;
                 min-width:0;
                 display:grid !important;
                 width:100% !important;
                 justify-content:stretch !important;
                 align-items:center !important;
                 justify-items:center !important;
                 row-gap:0.45rem;
                 padding:0.45rem;
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
                 padding:0.28rem 0.65rem 0.32rem 0.65rem;
                 border-radius:0.75rem;
                 border:1px solid var(--gs26-header-menu-border);
                 background:var(--gs26-header-menu-background);
                 color:var(--gs26-header-menu-text);
                 font-weight:800;
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
                 gap:0.35rem;
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
                 padding:0.28rem 0.65rem 0.32rem 0.65rem !important;
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
             }}"
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
                    box-shadow:{shell_alert_effect};
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
                    box-shadow:{shell_alert_effect};
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
                box-shadow:{shell_alert_effect};
                box-sizing:border-box;
                overflow:hidden;
            ",

                    // Header row 1
                    div {
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
                        h1 { style: "color:{theme.info_accent}; margin:0; font-size:22px; font-weight:800;", "{_dashboard_title(&layout)}" }

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
                                let software_buttons_enabled =
                                    action_policy.read().software_buttons_enabled;
                                let abort_visible = auth::can_send_command("Abort");
                                let abort_allowed = software_buttons_enabled && abort_visible;
                                let abort_style = if abort_allowed {
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
                            "
                                };
                                rsx! {
                                    if abort_visible {
                                        button {
                                            style: "{abort_style} touch-action:manipulation;",
                                            disabled: !abort_allowed,
                                            onmousedown: {
                                                let mut header_actions_expanded = header_actions_expanded;
                                                move |_| {
                                                    header_actions_expanded.set(false);
                                                    if abort_allowed {
                                                        send_cmd_from_press("Abort")
                                                    }
                                                }
                                            },
                                            ontouchstart: {
                                                let mut header_actions_expanded = header_actions_expanded;
                                                move |_| {
                                                    header_actions_expanded.set(false);
                                                    if abort_allowed {
                                                        send_cmd_from_press("Abort")
                                                    }
                                                }
                                            },
                                            onclick: {
                                                let mut header_actions_expanded = header_actions_expanded;
                                                move |_| {
                                                    header_actions_expanded.set(false);
                                                    if abort_allowed {
                                                        send_cmd_from_click("Abort")
                                                    }
                                                }
                                            },
                                            "{translate_text(\"ABORT\")}"
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
                            "Software command buttons are disabled by the hardware GPIO lockout."
                        }
                    }
                    // Header row 2
                    div {
                        style: "
                    display:flex;
                    align-items:center;
                    gap:12px;
                    width:100%;
                    margin-bottom:12px;
                    flex-wrap:wrap;
                ",

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
                            nav { class: "gs26-tab-nav",
                                for tab in _configured_main_tabs(&layout, *abort_only_mode.read(), *calibration_has_sensors.read()).into_iter() {
                                    match tab {
                                        MainTab::State => rsx! {
                                            button {
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
                                        MainTab::Calibration => rsx! {
                                            button {
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
                                        MainTab::Notifications => rsx! {
                                            button {
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
                                    {format!("{}: {err_count}", translate_text("Errors"))}
                                }
                                span {
                                    class: "gs26-status-count",
                                    "data-active": if has_warnings { "true" } else { "false" },
                                    style: "{warnings_status_style}",
                                    {format!("{}: {warn_count}", translate_text("Warnings"))}
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
                                if network_time_visible {
                                    NetworkTimeBadge { network_time: network_time, language: language_snapshot.clone() }
                                }
                            }
                            div { class: "gs26-status-launch",
                                LaunchClockBadge { launch_clock: launch_clock, network_time: network_time }
                            }
                            button {
                                class: "gs26-status-ack",
                                "data-active": if ack_button_visible { "true" } else { "false" },
                                style: "{ack_button_style}",
                                onclick: {
                                    let mut ack_warning_ts = ack_warning_ts;
                                    let mut ack_warning_count = ack_warning_count;
                                    let mut ack_error_ts = ack_error_ts;
                                    let mut ack_error_count = ack_error_count;
                                    move |_| {
                                        if show_ack_warnings {
                                            ack_warning_ts.set(latest_warning_ts);
                                            ack_warning_count.set(*warning_event_counter.read());
                                        } else if show_ack_errors {
                                            ack_error_ts.set(latest_error_ts);
                                            ack_error_count.set(*error_event_counter.read());
                                        }
                                    }
                                },
                                "{ack_button_label}"
                            }
                        }
                    }

                    // Main body
                    if !notifications.read().is_empty() {
                        div {
                            style: "display:flex; flex:0 1 auto; flex-direction:column; gap:8px; margin-bottom:10px; max-height:min(30vh, 180px); overflow-y:auto; overflow-x:hidden; -webkit-overflow-scrolling:auto; min-height:0; padding-right:2px;",
                            for n in notifications.read().iter() {
                                div {
                                    style: "display:flex; align-items:center; gap:10px; padding:10px 12px; border:1px solid {theme.notification_border}; border-radius:10px; background:{theme.notification_background}; color:{theme.notification_text}; min-width:0;",
                                    span { style: "flex:1 1 auto; min-width:0; overflow-wrap:anywhere; word-break:break-word;", {translate_text(&n.message)} }
                                    if let (Some(action_label), Some(action_cmd)) = (n.action_label.as_deref(), n.action_cmd.as_deref())
                                        && auth::can_send_command(action_cmd)
                                    {
                                        button {
                                            style: "padding:0.2rem 0.65rem; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.75rem; cursor:pointer; touch-action:manipulation;",
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
                                        style: "padding:0.2rem 0.55rem; border-radius:999px; border:1px solid {theme.button_border}; background:{theme.button_background}; color:{theme.button_text}; font-size:0.75rem; cursor:pointer;",
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

                    div { style: "flex:1 1 auto; min-height:0; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow:hidden;",
                        match *active_main_tab.read() {
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
                                ConnectionStatusTab {
                                    boards: board_status,
                                    expected_boards: layout.network_tab.expected_boards.clone(),
                                    layout: layout.connection_tab.clone(),
                                    title: _main_tab_label(&layout, MainTab::ConnectionStatus),
                                    theme: theme.clone(),
                                }
                            },
                            MainTab::Detailed => rsx! {
                                DetailedTab {
                                    metrics: frontend_network_metrics,
                                    board_status: board_status,
                                    network_topology: network_topology,
                                    flight_state: flight_state,
                                    warnings: warnings,
                                    errors: errors,
                                    notifications: notifications,
                                    network_time: network_time,
                                    theme: theme.clone(),
                                }
                            },
                            MainTab::NetworkTopology => rsx! {
                                div { style: "height:100%; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow-y:auto; overflow-x:hidden;",
                                    NetworkTopologyTab {
                                        topology: network_topology,
                                        layout: layout.network_tab.clone(),
                                        flow_animation_enabled: *network_flow_animation_enabled.read(),
                                        theme: theme.clone(),
                                    }
                                }
                            },
                            MainTab::Map => rsx! {
                                MapTab {
                                    key: "{*WS_EPOCH.read()}",
                                    rocket_gps: rocket_gps,
                                    user_gps: user_gps,
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
                                    backend_fill_targets: fill_targets,
                                    abort_only_mode: *abort_only_mode.read(),
                                    theme: theme.clone(),
                                }
                                }
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
                                    WarningsTab { warnings: warnings, theme: theme.clone() }
                                }
                            },
                            MainTab::Errors => rsx! {
                                div { style: "height:100%; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow-y:auto; overflow-x:hidden;",
                                    ErrorsTab { errors: errors, theme: theme.clone() }
                                }
                            },
                            MainTab::Data => rsx! {
                                DataTab {
                                    active_tab: active_data_tab,
                                    layout: layout.data_tab.clone(),
                                    theme: theme.clone(),
                                }
                            },
                        }
                    }
                }
                }
                {settings_overlay}
                {version_overlay}
            }
}

fn send_cmd(cmd: &str) {
    if !auth::can_send_command(cmd) {
        return;
    }
    if let Some(sender) = WS_SENDER.read().clone()
        && let Err(e) = sender.send_cmd(cmd)
    {
        log!("[CMD] ws send failed for '{cmd}': {e}");
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
    let Ok(mut pending) = PENDING_COMMAND_PRESS.lock() else {
        return;
    };
    *pending = Some((cmd.to_string(), monotonic_now_ms()));
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
    if should_send_command_release(cmd) {
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

// ---------- Web vs Native logging ----------
fn log(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&msg.into());

    #[cfg(not(target_arch = "wasm32"))]
    println!("{msg}");
}

// ---------- HTTP helpers ----------
#[cfg(target_arch = "wasm32")]
pub(crate) async fn http_get_json<T: for<'de> Deserialize<'de>>(path: &str) -> Result<T, String> {
    use gloo_net::http::Request;

    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    let base = UrlConfig::base_http();

    let url = if base.is_empty() {
        let w = web_sys::window().ok_or("no window".to_string())?;
        let origin = w
            .location()
            .origin()
            .map_err(|_| "failed to read window.location.origin".to_string())?;
        format!("{origin}{path}")
    } else {
        format!("{base}{path}")
    };

    let mut request = Request::get(&url);
    if let Some(token) = auth::current_token() {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let started_mono_ms = monotonic_now_ms();
    let response = request.send().await.map_err(|e| e.to_string())?;
    note_http_rtt_ms(monotonic_now_ms() - started_mono_ms);
    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;
    if status == 401 {
        auth::clear_current_session();
    }
    if !(200..300).contains(&status) {
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("HTTP {status}: {}", snippet.trim()));
    }
    serde_json::from_str::<T>(&body).map_err(|e| e.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn native_http_timeouts(path: &str) -> (std::time::Duration, std::time::Duration) {
    if path == "/api/recent" {
        let secs = std::env::var("GS_RECENT_HTTP_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(300)
            .clamp(15, 600);
        return (
            std::time::Duration::from_secs(10),
            std::time::Duration::from_secs(secs),
        );
    }

    (
        std::time::Duration::from_secs(8),
        std::time::Duration::from_secs(8),
    )
}

#[cfg(target_arch = "wasm32")]
async fn fetch_recent_rows_for_reseed() -> Result<Vec<TelemetryRow>, String> {
    http_get_json::<Vec<TelemetryRow>>("/api/recent").await
}

#[cfg(not(target_arch = "wasm32"))]
async fn fetch_recent_rows_for_reseed() -> Result<Vec<TelemetryRow>, String> {
    use futures_util::StreamExt;

    let path = "/api/recent".to_string();
    let base = UrlConfig::base_http();
    let url = if base.is_empty() {
        format!("http://localhost:3000{path}")
    } else {
        format!("{base}{path}")
    };
    let (connect_timeout, timeout) = native_http_timeouts(&path);
    let client =
        auth::build_native_http_client(UrlConfig::_skip_tls_verify(), connect_timeout, timeout)?;
    let skip_tls = UrlConfig::_skip_tls_verify();

    let mut request = client.get(url);
    if let Some(token) = auth::current_token() {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|e| {
        format!(
            "request send failed: {e:?} (base={} skip_tls={skip_tls} path={path})",
            UrlConfig::base_http()
        )
    })?;

    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();

    if !status.is_success() {
        let body = response.text().await.map_err(|e| {
            format!(
                "response body read failed: {e:?} (base={} skip_tls={skip_tls} path={path})",
                UrlConfig::base_http()
            )
        })?;
        if status == reqwest::StatusCode::UNAUTHORIZED {
            auth::clear_current_session();
        }
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("HTTP {}: {}", status, snippet.trim()));
    }

    let is_ndjson = content_type.contains("ndjson") || content_type.contains("json-seq");
    if !is_ndjson {
        let body = response.text().await.map_err(|e| {
            format!(
                "response body read failed: {e:?} (base={} skip_tls={skip_tls} path={path})",
                UrlConfig::base_http()
            )
        })?;
        return serde_json::from_str::<Vec<TelemetryRow>>(&body).map_err(|e| {
            let snippet: String = body.chars().take(200).collect();
            format!("invalid JSON ({e}): {}", snippet.trim())
        });
    }

    let mut rows = Vec::<TelemetryRow>::new();
    let mut buffered = Vec::<u8>::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("stream read failed: {e}"))?;
        buffered.extend_from_slice(&chunk);
        while let Some(newline_idx) = buffered.iter().position(|b| *b == b'\n') {
            let line = buffered.drain(..=newline_idx).collect::<Vec<_>>();
            let text = String::from_utf8_lossy(&line);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            let row = serde_json::from_str::<TelemetryRow>(trimmed).map_err(|e| {
                format!(
                    "invalid NDJSON row ({e}): {}",
                    trimmed.chars().take(200).collect::<String>()
                )
            })?;
            rows.push(row);
        }
    }
    let tail = String::from_utf8_lossy(&buffered);
    let trimmed = tail.trim();
    if !trimmed.is_empty() {
        let row = serde_json::from_str::<TelemetryRow>(trimmed).map_err(|e| {
            format!(
                "invalid NDJSON tail ({e}): {}",
                trimmed.chars().take(200).collect::<String>()
            )
        })?;
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn http_get_json<T: for<'de> Deserialize<'de>>(path: &str) -> Result<T, String> {
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    let base = UrlConfig::base_http();
    let url = if base.is_empty() {
        format!("http://localhost:3000{path}")
    } else {
        format!("{base}{path}")
    };
    let (connect_timeout, timeout) = native_http_timeouts(&path);

    let client =
        auth::build_native_http_client(UrlConfig::_skip_tls_verify(), connect_timeout, timeout)?;
    let skip_tls = UrlConfig::_skip_tls_verify();
    log!(
        "[http] GET {} skip_tls={} connect_timeout_ms={} timeout_ms={}",
        url,
        skip_tls,
        connect_timeout.as_millis(),
        timeout.as_millis()
    );

    let mut request = client.get(url);
    if let Some(token) = auth::current_token() {
        request = request.bearer_auth(token);
    }
    let started_mono_ms = monotonic_now_ms();
    let response = request.send().await.map_err(|e| {
        let msg = format!(
            "request send failed: {e:?} (base={} skip_tls={skip_tls} path={path})",
            UrlConfig::base_http()
        );
        log!("[http] {msg}");
        msg
    })?;
    note_http_rtt_ms(monotonic_now_ms() - started_mono_ms);

    let status = response.status();
    let body = response.text().await.map_err(|e| {
        let msg = format!(
            "response body read failed: {e:?} (base={} skip_tls={skip_tls} path={path})",
            UrlConfig::base_http()
        );
        log!("[http] {msg}");
        msg
    })?;
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            auth::clear_current_session();
        }
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("HTTP {}: {}", status, snippet.trim()));
    }

    serde_json::from_str::<T>(&body).map_err(|e| {
        let snippet: String = body.chars().take(200).collect();
        format!("invalid JSON ({e}): {}", snippet.trim())
    })
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn http_post_json<B: Serialize, T: for<'de> Deserialize<'de>>(
    path: &str,
    body: &B,
) -> Result<T, String> {
    use gloo_net::http::Request;

    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    let base = UrlConfig::base_http();
    let url = if base.is_empty() {
        let w = web_sys::window().ok_or("no window".to_string())?;
        let origin = w
            .location()
            .origin()
            .map_err(|_| "failed to read window.location.origin".to_string())?;
        format!("{origin}{path}")
    } else {
        format!("{base}{path}")
    };

    let mut request = Request::post(&url);
    if let Some(token) = auth::current_token() {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let response = request
        .json(body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;
    if status == 401 {
        auth::clear_current_session();
    }
    if !(200..300).contains(&status) {
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("HTTP {status}: {}", snippet.trim()));
    }
    serde_json::from_str::<T>(&body).map_err(|e| e.to_string())
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn http_post_json<B: Serialize, T: for<'de> Deserialize<'de>>(
    path: &str,
    body: &B,
) -> Result<T, String> {
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    let base = UrlConfig::base_http();
    let url = if base.is_empty() {
        format!("http://localhost:3000{path}")
    } else {
        format!("{base}{path}")
    };

    let client = auth::build_native_http_client(
        UrlConfig::_skip_tls_verify(),
        std::time::Duration::from_secs(8),
        std::time::Duration::from_secs(8),
    )?;

    let mut request = client.post(url).json(body);
    if let Some(token) = auth::current_token() {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        auth::clear_current_session();
    }
    if !status.is_success() {
        let snippet: String = body.chars().take(200).collect();
        return Err(format!("HTTP {}: {}", status, snippet.trim()));
    }
    serde_json::from_str::<T>(&body).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
async fn http_post_empty(path: &str) -> Result<(), String> {
    use gloo_net::http::Request;

    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    let base = UrlConfig::base_http();
    let url = if base.is_empty() {
        let w = web_sys::window().ok_or("no window".to_string())?;
        let origin = w
            .location()
            .origin()
            .map_err(|_| "failed to read window.location.origin".to_string())?;
        format!("{origin}{path}")
    } else {
        format!("{base}{path}")
    };

    let mut request = Request::post(&url);
    if let Some(token) = auth::current_token() {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if status == 401 {
        auth::clear_current_session();
    }
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {}", body.trim()));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
async fn http_post_empty(path: &str) -> Result<(), String> {
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    let base = UrlConfig::base_http();
    let url = if base.is_empty() {
        format!("http://localhost:3000{path}")
    } else {
        format!("{base}{path}")
    };

    let client = auth::build_native_http_client(
        UrlConfig::_skip_tls_verify(),
        std::time::Duration::from_secs(8),
        std::time::Duration::from_secs(8),
    )?;

    let mut request = client.post(url);
    if let Some(token) = auth::current_token() {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        auth::clear_current_session();
    }
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, body.trim()));
    }
    Ok(())
}

async fn dismiss_notification_remote(id: u64) -> Result<(), String> {
    http_post_empty(&format!("/api/notifications/{id}/dismiss")).await
}

#[cfg(target_arch = "wasm32")]
fn spawn_detached<F>(fut: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(fut);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_detached<F>(fut: F)
where
    F: Future<Output = ()> + 'static,
{
    spawn(fut);
}

fn auth_ws_url(base_ws: &str) -> String {
    let mut url = format!("{}/ws", base_ws.trim_end_matches('/'));
    if let Some(token) = auth::current_token() {
        let sep = if url.contains('?') { '&' } else { '?' };
        url.push(sep);
        url.push_str("token=");
        url.push_str(&token);
    }
    url
}

fn load_dismissed_notifications() -> Vec<DismissedNotification> {
    persist::get_string(NOTIFICATION_DISMISSED_STORAGE_KEY)
        .and_then(|raw| serde_json::from_str::<Vec<DismissedNotification>>(&raw).ok())
        .unwrap_or_default()
}

fn persist_dismissed_notifications(items: &[DismissedNotification]) {
    if let Ok(raw) = serde_json::to_string(items) {
        persist::set_string(NOTIFICATION_DISMISSED_STORAGE_KEY, &raw);
    }
}

async fn cooperative_yield() {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(0).await;

    #[cfg(not(target_arch = "wasm32"))]
    tokio::task::yield_now().await;
}

fn dismiss_all_active_notifications_local_and_remote(
    notifications: Signal<Vec<PersistentNotification>>,
    dismissed_notifications: Signal<Vec<DismissedNotification>>,
    unread_notification_ids: Signal<Vec<u64>>,
) {
    let mut notifications = notifications;
    let mut dismissed_notifications = dismissed_notifications;
    let mut unread_notification_ids = unread_notification_ids;

    let active = { notifications.read().clone() };
    if active.is_empty() {
        unread_notification_ids.set(Vec::new());
        return;
    }

    notifications.set(Vec::new());
    unread_notification_ids.set(Vec::new());

    let mut ids = { dismissed_notifications.read().clone() };
    let mut changed = false;
    for n in &active {
        let item = DismissedNotification {
            id: n.id,
            timestamp_ms: n.timestamp_ms,
        };
        if !ids.contains(&item) {
            ids.push(item);
            changed = true;
        }
    }
    if changed {
        ids.sort_by_key(|x| (x.id, x.timestamp_ms));
        dismissed_notifications.set(ids.clone());
        persist_dismissed_notifications(&ids);
    }

    for n in active {
        let id = n.id;
        spawn_detached(async move {
            let _ = dismiss_notification_remote(id).await;
        });
    }
}

fn clear_all_notifications_local_and_remote(
    notifications: Signal<Vec<PersistentNotification>>,
    notification_history: Signal<Vec<PersistentNotification>>,
    dismissed_notifications: Signal<Vec<DismissedNotification>>,
    unread_notification_ids: Signal<Vec<u64>>,
) {
    dismiss_all_active_notifications_local_and_remote(
        notifications,
        dismissed_notifications,
        unread_notification_ids,
    );
    let mut notification_history = notification_history;
    notification_history.set(Vec::new());
}

fn merge_notification_history(
    history: &mut Vec<PersistentNotification>,
    incoming: &[PersistentNotification],
) {
    let mut seen: HashSet<(u64, i64)> = history.iter().map(|n| (n.id, n.timestamp_ms)).collect();
    for n in incoming {
        if seen.insert((n.id, n.timestamp_ms)) {
            history.push(n.clone());
        }
    }
    history.sort_by_key(|n| -n.timestamp_ms);
    if history.len() > MAX_NOTIFICATION_HISTORY {
        history.truncate(MAX_NOTIFICATION_HISTORY);
    }
}

fn apply_notifications_snapshot(
    incoming: Vec<PersistentNotification>,
    notifications: Signal<Vec<PersistentNotification>>,
    notification_history: Signal<Vec<PersistentNotification>>,
    dismissed_notifications: Signal<Vec<DismissedNotification>>,
    unread_notification_ids: Signal<Vec<u64>>,
) {
    let mut notification_history = notification_history;
    let mut notifications = notifications;
    let mut dismissed_notifications = dismissed_notifications;
    let mut unread_notification_ids = unread_notification_ids;

    // Always keep local history of all notifications (active + dismissed).
    let mut history = { notification_history.read().clone() };
    merge_notification_history(&mut history, &incoming);
    notification_history.set(history);

    // Active notifications come directly from backend snapshot.
    // Backend dismiss endpoint is source of truth; local cache is only for local bookkeeping.
    let mut active: Vec<PersistentNotification> = incoming;
    active.sort_by_key(|n| n.timestamp_ms);
    let mut dismissed_ids = dismissed_notifications.read().clone();
    let mut dismissed_changed = false;

    // Keep only latest N active notifications and auto-dismiss oldest overflow.
    if active.len() > MAX_ACTIVE_NOTIFICATIONS {
        let overflow = active.len() - MAX_ACTIVE_NOTIFICATIONS;
        let overflow_items: Vec<DismissedNotification> = active
            .iter()
            .take(overflow)
            .map(|n| DismissedNotification {
                id: n.id,
                timestamp_ms: n.timestamp_ms,
            })
            .collect();
        for item in overflow_items {
            if !dismissed_ids.contains(&item) {
                dismissed_ids.push(item);
                dismissed_changed = true;
            }
            let id = item.id;
            spawn_detached(async move {
                let _ = dismiss_notification_remote(id).await;
            });
        }
        active = active.split_off(overflow);
    }
    if dismissed_changed {
        dismissed_ids.sort_by_key(|x| (x.id, x.timestamp_ms));
        dismissed_notifications.set(dismissed_ids.clone());
        persist_dismissed_notifications(&dismissed_ids);
    }

    let prev_ids: HashSet<u64> = { notifications.read().iter().map(|n| n.id).collect() };
    notifications.set(active.clone());

    let mut unread: HashSet<u64> = unread_notification_ids.read().iter().copied().collect();
    for n in &active {
        if !prev_ids.contains(&n.id) {
            unread.insert(n.id);
        }
    }
    let mut unread_vec: Vec<u64> = unread.into_iter().collect();
    unread_vec.sort_unstable();
    let unread_snapshot = unread_notification_ids.read().clone();
    if unread_snapshot != unread_vec {
        unread_notification_ids.set(unread_vec);
    }

    // Auto-dismiss new visible notifications after timeout.
    for n in active {
        if prev_ids.contains(&n.id) {
            continue;
        }
        if n.persistent {
            continue;
        }
        let id = n.id;
        let ts = n.timestamp_ms;
        let mut notifications = notifications;
        let mut dismissed_notifications = dismissed_notifications;
        spawn_detached(async move {
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(NOTIFICATION_AUTO_DISMISS_MS).await;

            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(
                NOTIFICATION_AUTO_DISMISS_MS as u64,
            ))
            .await;

            let still_visible = { notifications.read().iter().any(|x| x.id == id) };
            if !still_visible {
                return;
            }

            let mut v = { notifications.read().clone() };
            v.retain(|x| x.id != id);
            notifications.set(v);

            let mut ids = { dismissed_notifications.read().clone() };
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

            let _ = dismiss_notification_remote(id).await;
        });
    }
}

// ------------------------------
// Seed telemetry/alerts/gps
// ------------------------------
#[allow(clippy::too_many_arguments)]
async fn seed_from_db(
    warnings: &mut Signal<Vec<AlertMsg>>,
    errors: &mut Signal<Vec<AlertMsg>>,
    notifications: &mut Signal<Vec<PersistentNotification>>,
    notification_history: &mut Signal<Vec<PersistentNotification>>,
    dismissed_notifications: &mut Signal<Vec<DismissedNotification>>,
    unread_notification_ids: &mut Signal<Vec<u64>>,
    action_policy: &mut Signal<ActionPolicyMsg>,
    fill_targets: &mut Signal<Option<FillTargetsConfig>>,
    network_time: &mut Signal<Option<NetworkTimeSync>>,
    launch_clock: &mut Signal<Option<LaunchClockMsg>>,
    network_topology: &mut Signal<NetworkTopologyMsg>,
    board_status: &mut Signal<Vec<BoardStatusEntry>>,
    rocket_gps: &mut Signal<Option<(f64, f64)>>,
    _user_gps: &mut Signal<Option<(f64, f64)>>,
    ack_warning_ts: &mut Signal<i64>,
    ack_error_ts: &mut Signal<i64>,
    alive: Arc<AtomicBool>,
) -> Result<(), String> {
    log!("[seed] seed_from_db entered");
    struct ReseedGuard;
    impl Drop for ReseedGuard {
        fn drop(&mut self) {
            RESEED_IN_PROGRESS.store(false, Ordering::Relaxed);
            if let Ok(mut v) = RESEED_LIVE_BUFFER.lock() {
                v.clear();
            }
            charts_cache_cancel_reseed_build();
            log!("[seed] seed_from_db exiting");
        }
    }
    RESEED_IN_PROGRESS.store(true, Ordering::Relaxed);
    if let Ok(mut v) = RESEED_LIVE_BUFFER.lock() {
        v.clear();
    }
    charts_cache_begin_reseed_build();
    let _reseed_guard = ReseedGuard;

    fn merge_db_and_live(
        mut db_rows: Vec<TelemetryRow>,
        live_rows: Vec<TelemetryRow>,
    ) -> Vec<TelemetryRow> {
        // Keep full overlap, then compact to the same bucket density the chart can render.
        db_rows.extend(live_rows);
        compact_rows_for_ui(db_rows)
    }

    let queue_snapshot = || -> Vec<TelemetryRow> {
        if let Ok(q) = TELEMETRY_QUEUE.lock() {
            q.iter().cloned().collect()
        } else {
            Vec::new()
        }
    };

    if !alive.load(Ordering::Relaxed) {
        return Ok(());
    }

    // ---- Telemetry history (/api/recent) ----
    let existing_rows_before_seed = ui_telemetry_rows_snapshot();
    let bridge_rows = if let Ok(mut rows) = RESEED_HISTORY_BRIDGE.lock() {
        std::mem::take(&mut *rows)
    } else {
        Vec::new()
    };
    if existing_rows_before_seed.is_empty() {
        set_reseed_status_running();
    }
    log!(
        "[seed] /api/recent begin existing_rows_before_seed={}",
        existing_rows_before_seed.len()
    );
    match fetch_recent_rows_for_reseed().await {
        Ok(mut list) => {
            if !alive.load(Ordering::Relaxed) {
                return Ok(());
            }

            sort_rows(&mut list);
            prune_history(&mut list);
            list = compact_rows_for_ui(list);
            log!("[seed] /api/recent ok compacted_rows={}", list.len());

            if !bridge_rows.is_empty() {
                log!(
                    "[seed] /api/recent merging bridge_rows={}",
                    bridge_rows.len()
                );
                list = merge_db_and_live(list, bridge_rows);
            }

            // Capture rows that arrived while reseed was running and keep them.
            let mut live_rows = ui_telemetry_rows_snapshot();
            live_rows.extend(queue_snapshot());
            if !live_rows.is_empty() {
                sort_rows(&mut live_rows);
                prune_history(&mut live_rows);
                live_rows = compact_rows_for_ui(live_rows);
                log!("[seed] /api/recent merging live_rows={}", live_rows.len());
                list = merge_db_and_live(list, live_rows);
            }

            if let Some(gps) = list.iter().rev().find_map(row_to_gps) {
                rocket_gps.set(Some(gps));
            }

            if list.is_empty() && !existing_rows_before_seed.is_empty() {
                // Treat empty reseed as transient and keep already-visible history.
                log!("[seed] /api/recent empty -> keeping existing rows");
                list = existing_rows_before_seed;
            } else {
                // Build reseed cache in a double buffer while active cache keeps live updates.
                const RESEED_INGEST_CHUNK: usize = 1024;
                for (i, row) in list.iter().enumerate() {
                    charts_cache_reseed_ingest_row(row);
                    if i % RESEED_INGEST_CHUNK == 0 {
                        cooperative_yield().await;
                    }
                }

                // Replay queued rows into reseed cache as a second safety net.
                let post_reset_queued_rows = queue_snapshot();
                for row in &post_reset_queued_rows {
                    charts_cache_reseed_ingest_row(row);
                }
                if !post_reset_queued_rows.is_empty() {
                    list.extend(post_reset_queued_rows);
                    list = compact_rows_for_ui(list);
                }

                // Replay live rows received during reseed build.
                let reseed_live_rows = if let Ok(mut v) = RESEED_LIVE_BUFFER.lock() {
                    std::mem::take(&mut *v)
                } else {
                    Vec::new()
                };
                if !reseed_live_rows.is_empty() {
                    for row in &reseed_live_rows {
                        charts_cache_reseed_ingest_row(row);
                    }
                    list.extend(reseed_live_rows);
                    list = compact_rows_for_ui(list);
                }

                // Atomically swap the prepared reseed cache in.
                charts_cache_finish_reseed_build();
            }
            log!("[seed] applying reseed rows={}", list.len());
            if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
                store.replace_from_rows(&list);
            }
            reset_latest_telemetry(&list);
            persist_cached_telemetry_snapshot_if_due(true);
            if !list.is_empty() {
                DASHBOARD_HAS_CONNECTED.store(true, Ordering::Relaxed);
            }
            set_reseed_status_ok(list.len());
        }
        Err(err) => {
            log!("[seed] /api/recent failed: {err}");
            if existing_rows_before_seed.is_empty() {
                set_reseed_status_failed(reseed_error_message(false, &err));
                return Err(format!("telemetry reseed failed: {err}"));
            }
            set_reseed_status_failed(reseed_error_message(true, &err));
            log!("telemetry reseed failed (keeping existing history): {err}");
        }
    }

    if !alive.load(Ordering::Relaxed) {
        return Ok(());
    }

    // ---- Alerts history (/api/alerts) ----
    if let Ok(mut alerts) = http_get_json::<Vec<AlertDto>>("/api/alerts?minutes=20").await {
        if !alive.load(Ordering::Relaxed) {
            return Ok(());
        }

        let max_ts = alerts.iter().map(|a| a.timestamp_ms).max().unwrap_or(0);
        let current_ack_warning_ts = *ack_warning_ts.read();
        let current_ack_error_ts = *ack_error_ts.read();
        let prev_ack = current_ack_warning_ts.max(current_ack_error_ts);
        if prev_ack > 0 && max_ts > 0 && max_ts < prev_ack - HISTORY_MS {
            ack_warning_ts.set(0);
            ack_error_ts.set(0);
        }

        alerts.sort_by_key(|a| -a.timestamp_ms);

        let mut w = Vec::<AlertMsg>::new();
        let mut e = Vec::<AlertMsg>::new();
        for a in alerts {
            match a.severity.as_str() {
                "warning" => w.push(AlertMsg {
                    timestamp_ms: a.timestamp_ms,
                    message: a.message,
                }),
                "error" => e.push(AlertMsg {
                    timestamp_ms: a.timestamp_ms,
                    message: a.message,
                }),
                _ => {}
            }
        }

        warnings.set(w);
        errors.set(e);
    }

    if let Ok(list) = http_get_json::<Vec<PersistentNotification>>("/api/notifications").await
        && alive.load(Ordering::Relaxed)
    {
        apply_notifications_snapshot(
            list,
            *notifications,
            *notification_history,
            *dismissed_notifications,
            *unread_notification_ids,
        );
    }

    if let Ok(policy) = http_get_json::<ActionPolicyMsg>("/api/action_policy").await
        && alive.load(Ordering::Relaxed)
    {
        action_policy.set(policy);
    }

    if let Ok(targets) = http_get_json::<FillTargetsConfig>("/api/fill_targets").await
        && alive.load(Ordering::Relaxed)
    {
        fill_targets.set(Some(targets));
    }

    if let Ok(nt) = http_get_json::<NetworkTimeMsg>("/api/network_time").await
        && alive.load(Ordering::Relaxed)
    {
        network_time.set(Some(NetworkTimeSync {
            network_ms: nt.timestamp_ms,
            received_mono_ms: monotonic_now_ms(),
        }));
    }

    if let Ok(clock) = http_get_json::<LaunchClockMsg>("/api/launch_clock").await
        && alive.load(Ordering::Relaxed)
    {
        launch_clock.set(Some(clock));
    }

    if let Ok(topology) = http_get_json::<NetworkTopologyMsg>("/api/network_topology").await
        && alive.load(Ordering::Relaxed)
    {
        network_topology.set(topology);
    }

    if !alive.load(Ordering::Relaxed) {
        return Ok(());
    }

    // ---- Board status (/api/boards) ----
    if let Ok(status) = http_get_json::<BoardStatusMsg>("/api/boards").await
        && alive.load(Ordering::Relaxed)
    {
        board_status.set(status.boards);
    }

    if !alive.load(Ordering::Relaxed) {
        return Ok(());
    }

    // ---- Optional GPS seed (/api/gps) ----
    if let Ok(gps) = http_get_json::<GpsResponse>("/api/gps").await
        && alive.load(Ordering::Relaxed)
        && let Some(rocket) = gps.rocket
    {
        rocket_gps.set(Some((rocket.lat, rocket.lon)));
    }

    Ok(())
}

// ---------------------------------------------------------
// WebSocket supervisor (reconnect loop) — both platforms
// ---------------------------------------------------------
#[allow(clippy::too_many_arguments)]
async fn connect_ws_supervisor(
    epoch: u64,
    warnings: Signal<Vec<AlertMsg>>,
    errors: Signal<Vec<AlertMsg>>,
    notifications: Signal<Vec<PersistentNotification>>,
    notification_history: Signal<Vec<PersistentNotification>>,
    dismissed_notifications: Signal<Vec<DismissedNotification>>,
    unread_notification_ids: Signal<Vec<u64>>,
    action_policy: Signal<ActionPolicyMsg>,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    network_time: Signal<Option<NetworkTimeSync>>,
    launch_clock: Signal<Option<LaunchClockMsg>>,
    network_topology: Signal<NetworkTopologyMsg>,
    warning_event_counter: Signal<u64>,
    error_event_counter: Signal<u64>,
    flight_state: Signal<FlightState>,
    board_status: Signal<Vec<BoardStatusEntry>>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    user_gps: Signal<Option<(f64, f64)>>,
    alive: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut notifications = notifications;
    let mut notification_history = notification_history;
    let mut unread_notification_ids = unread_notification_ids;

    if *WS_EPOCH.read() != epoch {
        return Ok(());
    }

    log!("[WS] supervisor starting connection (epoch={epoch})");

    loop {
        if !alive.load(Ordering::Relaxed) {
            break;
        }
        if *WS_EPOCH.read() != epoch {
            break;
        }

        let res = {
            #[cfg(target_arch = "wasm32")]
            {
                connect_ws_once_wasm(
                    epoch,
                    warnings,
                    errors,
                    notifications,
                    notification_history,
                    dismissed_notifications,
                    unread_notification_ids,
                    action_policy,
                    fill_targets,
                    network_time,
                    launch_clock,
                    network_topology,
                    warning_event_counter,
                    error_event_counter,
                    flight_state,
                    board_status,
                    rocket_gps,
                    user_gps,
                    alive.clone(),
                )
                .await
            }

            #[cfg(not(target_arch = "wasm32"))]
            {
                connect_ws_once_native(
                    epoch,
                    warnings,
                    errors,
                    notifications,
                    notification_history,
                    dismissed_notifications,
                    unread_notification_ids,
                    action_policy,
                    fill_targets,
                    network_time,
                    launch_clock,
                    network_topology,
                    warning_event_counter,
                    error_event_counter,
                    flight_state,
                    board_status,
                    rocket_gps,
                    user_gps,
                    alive.clone(),
                )
                .await
            }
        };

        if !alive.load(Ordering::Relaxed) {
            break;
        }
        if *WS_EPOCH.read() != epoch {
            break;
        }

        if let Err(e) = res
            && alive.load(Ordering::Relaxed)
        {
            note_ws_connection_notification(
                &mut notifications,
                &mut notification_history,
                &mut unread_notification_ids,
                &auth_ws_url(&UrlConfig::base_ws()),
                &e,
            );
            log!("[WS] connect error: {e}");
        }

        #[cfg(target_arch = "wasm32")]
        gloo_timers::future::TimeoutFuture::new(800).await;

        #[cfg(not(target_arch = "wasm32"))]
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    }

    Ok(())
}

#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
async fn connect_ws_once_wasm(
    epoch: u64,
    warnings: Signal<Vec<AlertMsg>>,
    errors: Signal<Vec<AlertMsg>>,
    notifications: Signal<Vec<PersistentNotification>>,
    notification_history: Signal<Vec<PersistentNotification>>,
    dismissed_notifications: Signal<Vec<DismissedNotification>>,
    unread_notification_ids: Signal<Vec<u64>>,
    action_policy: Signal<ActionPolicyMsg>,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    network_time: Signal<Option<NetworkTimeSync>>,
    launch_clock: Signal<Option<LaunchClockMsg>>,
    network_topology: Signal<NetworkTopologyMsg>,
    warning_event_counter: Signal<u64>,
    error_event_counter: Signal<u64>,
    flight_state: Signal<FlightState>,
    board_status: Signal<Vec<BoardStatusEntry>>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    user_gps: Signal<Option<(f64, f64)>>,
    alive: Arc<AtomicBool>,
) -> Result<(), String> {
    use futures_channel::oneshot;
    use js_sys::Reflect;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::JsValue;
    use wasm_bindgen::closure::Closure;
    use web_sys::{CloseEvent, ErrorEvent, Event, MessageEvent, WebSocket};

    if !alive.load(Ordering::Relaxed) {
        return Ok(());
    }

    let base_ws = UrlConfig::base_ws();
    let ws_url = auth_ws_url(&base_ws);

    log!("[WS] connecting to {ws_url} (epoch={epoch})");

    let ws = WebSocket::new(&ws_url).map_err(|_| "failed to create websocket".to_string())?;
    note_ws_connection_state(false, ws_url.clone(), None, epoch);

    *WS_RAW.write() = Some(ws.clone());
    *WS_SENDER.write() = Some(WsSender { ws: ws.clone() });

    let (closed_tx, closed_rx) = oneshot::channel::<()>();
    let closed_tx = std::rc::Rc::new(std::cell::RefCell::new(Some(closed_tx)));

    {
        let ws_url_for_open = ws_url.clone();
        let mut notifications_for_open = notifications;
        let mut notification_history_for_open = notification_history;
        let mut unread_notification_ids_for_open = unread_notification_ids;
        let onopen: Closure<dyn FnMut(Event)> = Closure::new(move |_e: Event| {
            log!("[WS] open");
            note_ws_connected_and_restore_data_flow(
                ws_url_for_open.clone(),
                epoch,
                &mut notifications_for_open,
                &mut notification_history_for_open,
                &mut unread_notification_ids_for_open,
            );
        });
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();
    }

    {
        let alive_for_message = alive.clone();
        let onmessage: Closure<dyn FnMut(MessageEvent)> = Closure::new(move |e: MessageEvent| {
            if !alive_for_message.load(Ordering::Relaxed) {
                return;
            }
            if let Some(s) = e.data().as_string() {
                handle_ws_message(
                    &s,
                    warnings,
                    errors,
                    notifications,
                    notification_history,
                    dismissed_notifications,
                    unread_notification_ids,
                    action_policy,
                    fill_targets,
                    network_time,
                    launch_clock,
                    network_topology,
                    warning_event_counter,
                    error_event_counter,
                    flight_state,
                    board_status,
                    rocket_gps,
                    user_gps,
                );
            }
        });
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();
    }

    {
        let closed_tx = closed_tx.clone();
        let alive_for_error = alive.clone();
        let onerror: Closure<dyn FnMut(ErrorEvent)> = Closure::new(move |e: ErrorEvent| {
            if !alive_for_error.load(Ordering::Relaxed) {
                return;
            }
            let message = Reflect::get(e.as_ref(), &JsValue::from_str("message"))
                .ok()
                .and_then(|v| v.as_string())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "websocket error event".to_string());
            log!("[WS] error: {message}");
            if let Some(tx) = closed_tx.borrow_mut().take() {
                let _ = tx.send(());
            }
        });
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();
    }

    {
        let closed_tx = closed_tx.clone();
        let alive_for_close = alive.clone();
        let onclose: Closure<dyn FnMut(CloseEvent)> = Closure::new(move |e: CloseEvent| {
            if !alive_for_close.load(Ordering::Relaxed) {
                return;
            }
            log!("[WS] close code={} reason='{}'", e.code(), e.reason());
            if let Some(tx) = closed_tx.borrow_mut().take() {
                let _ = tx.send(());
            }
        });
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();
    }

    futures_util::pin_mut!(closed_rx);

    loop {
        if !alive.load(Ordering::Relaxed) {
            let _ = ws.close();
            break;
        }
        if *WS_EPOCH.read() != epoch {
            let _ = ws.close();
            break;
        }

        let done = futures_util::future::select(
            &mut closed_rx,
            gloo_timers::future::TimeoutFuture::new(150),
        )
        .await;

        match done {
            futures_util::future::Either::Left((_closed, _timeout)) => break,
            futures_util::future::Either::Right((_timeout, _closed)) => {}
        }
    }

    if *WS_EPOCH.read() == epoch {
        note_ws_connection_state(false, ws_url, Some("websocket closed".to_string()), epoch);
        *WS_SENDER.write() = None;
        *WS_RAW.write() = None;
    }

    Err("websocket closed".to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn insecure_rustls_connector() -> Result<tokio_tungstenite::Connector, String> {
    #[cfg(target_os = "windows")]
    {
        let connector = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()
            .map_err(|e| format!("native-tls connector build failed: {e}"))?;
        return Ok(tokio_tungstenite::Connector::NativeTls(connector));
    }

    #[cfg(not(target_os = "windows"))]
    {
        #[derive(Debug)]
        struct NoCertificateVerification(std::sync::Arc<rustls::crypto::CryptoProvider>);

        impl rustls::client::danger::ServerCertVerifier for NoCertificateVerification {
            fn verify_server_cert(
                &self,
                _end_entity: &rustls::pki_types::CertificateDer<'_>,
                _intermediates: &[rustls::pki_types::CertificateDer<'_>],
                _server_name: &rustls::pki_types::ServerName<'_>,
                _ocsp_response: &[u8],
                _now: rustls::pki_types::UnixTime,
            ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
                Ok(rustls::client::danger::ServerCertVerified::assertion())
            }

            fn verify_tls12_signature(
                &self,
                message: &[u8],
                cert: &rustls::pki_types::CertificateDer<'_>,
                dss: &rustls::DigitallySignedStruct,
            ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error>
            {
                rustls::crypto::verify_tls12_signature(
                    message,
                    cert,
                    dss,
                    &self.0.signature_verification_algorithms,
                )
            }

            fn verify_tls13_signature(
                &self,
                message: &[u8],
                cert: &rustls::pki_types::CertificateDer<'_>,
                dss: &rustls::DigitallySignedStruct,
            ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error>
            {
                rustls::crypto::verify_tls13_signature(
                    message,
                    cert,
                    dss,
                    &self.0.signature_verification_algorithms,
                )
            }

            fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
                self.0.signature_verification_algorithms.supported_schemes()
            }
        }

        let provider = rustls::crypto::CryptoProvider::get_default()
            .cloned()
            .ok_or_else(|| "rustls default crypto provider is not set".to_string())?;

        let config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(NoCertificateVerification(
                provider,
            )))
            .with_no_client_auth();

        Ok(tokio_tungstenite::Connector::Rustls(std::sync::Arc::new(
            config,
        )))
    }
}

#[cfg(any(target_os = "android", target_os = "ios", target_os = "macos"))]
fn platform_rustls_connector() -> Result<tokio_tungstenite::Connector, String> {
    use rustls_platform_verifier::ConfigVerifierExt;
    let tls_config = rustls::ClientConfig::with_platform_verifier()
        .map_err(|e| format!("platform TLS verifier setup failed: {e}"))?;
    Ok(tokio_tungstenite::Connector::Rustls(std::sync::Arc::new(
        tls_config,
    )))
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
async fn connect_ws_once_native(
    epoch: u64,
    warnings: Signal<Vec<AlertMsg>>,
    errors: Signal<Vec<AlertMsg>>,
    mut notifications: Signal<Vec<PersistentNotification>>,
    mut notification_history: Signal<Vec<PersistentNotification>>,
    dismissed_notifications: Signal<Vec<DismissedNotification>>,
    mut unread_notification_ids: Signal<Vec<u64>>,
    action_policy: Signal<ActionPolicyMsg>,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    network_time: Signal<Option<NetworkTimeSync>>,
    launch_clock: Signal<Option<LaunchClockMsg>>,
    network_topology: Signal<NetworkTopologyMsg>,
    warning_event_counter: Signal<u64>,
    error_event_counter: Signal<u64>,
    flight_state: Signal<FlightState>,
    board_status: Signal<Vec<BoardStatusEntry>>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    user_gps: Signal<Option<(f64, f64)>>,
    alive: Arc<AtomicBool>,
) -> Result<(), String> {
    use futures_util::{SinkExt, StreamExt};

    if !alive.load(Ordering::Relaxed) {
        return Ok(());
    }
    if *WS_EPOCH.read() != epoch {
        return Ok(());
    }

    let base_ws = UrlConfig::base_ws();
    let ws_url = auth_ws_url(&base_ws);

    log!("[WS] connecting to {ws_url} (epoch={epoch})");
    note_ws_connection_state(false, ws_url.clone(), None, epoch);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    *WS_SENDER.write() = Some(WsSender { tx });

    let ws_stream = if UrlConfig::_skip_tls_verify() && ws_url.starts_with("wss://") {
        let tls = insecure_rustls_connector()
            .map_err(|e| format!("[WS] rustls connector build failed: {e}"))?;
        tokio_tungstenite::connect_async_tls_with_config(ws_url.as_str(), None, false, Some(tls))
            .await
            .map_err(|e| format!("[WS] connect failed: {e}"))?
            .0
    } else if ws_url.starts_with("wss://") {
        #[cfg(any(target_os = "android", target_os = "ios", target_os = "macos"))]
        {
            let tls = platform_rustls_connector()
                .map_err(|e| format!("[WS] platform rustls connector build failed: {e}"))?;
            tokio_tungstenite::connect_async_tls_with_config(
                ws_url.as_str(),
                None,
                false,
                Some(tls),
            )
            .await
            .map_err(|e| format!("[WS] connect failed: {e}"))?
            .0
        }
        #[cfg(not(any(target_os = "android", target_os = "ios", target_os = "macos")))]
        {
            tokio_tungstenite::connect_async(ws_url.as_str())
                .await
                .map_err(|e| format!("[WS] connect failed: {e}"))?
                .0
        }
    } else {
        tokio_tungstenite::connect_async(ws_url.as_str())
            .await
            .map_err(|e| format!("[WS] connect failed: {e}"))?
            .0
    };

    let (mut write, mut read) = ws_stream.split();
    note_ws_connected_and_restore_data_flow(
        ws_url.clone(),
        epoch,
        &mut notifications,
        &mut notification_history,
        &mut unread_notification_ids,
    );

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let _ = write
                .send(tokio_tungstenite::tungstenite::Message::Text(msg.into()))
                .await;
        }
    });

    while alive.load(Ordering::Relaxed) && *WS_EPOCH.read() == epoch {
        let Some(item) = read.next().await else { break };

        let msg = match item {
            Ok(m) => m,
            Err(e) => {
                log!("[WS] read error: {e}");
                break;
            }
        };

        if let tokio_tungstenite::tungstenite::Message::Text(s) = msg {
            handle_ws_message(
                &s,
                warnings,
                errors,
                notifications,
                notification_history,
                dismissed_notifications,
                unread_notification_ids,
                action_policy,
                fill_targets,
                network_time,
                launch_clock,
                network_topology,
                warning_event_counter,
                error_event_counter,
                flight_state,
                board_status,
                rocket_gps,
                user_gps,
            );
        }
    }

    writer.abort();
    // Only clear sender if this task still owns the active epoch.
    // Prevents old-epoch teardown from clobbering a freshly reconnected sender.
    if *WS_EPOCH.read() == epoch {
        note_ws_connection_state(false, ws_url, Some("websocket closed".to_string()), epoch);
        *WS_SENDER.write() = None;
    }

    Err("websocket closed".to_string())
}

#[allow(clippy::too_many_arguments)]
fn handle_ws_message(
    s: &str,
    warnings: Signal<Vec<AlertMsg>>,
    errors: Signal<Vec<AlertMsg>>,
    notifications: Signal<Vec<PersistentNotification>>,
    notification_history: Signal<Vec<PersistentNotification>>,
    dismissed_notifications: Signal<Vec<DismissedNotification>>,
    unread_notification_ids: Signal<Vec<u64>>,
    action_policy: Signal<ActionPolicyMsg>,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    network_time: Signal<Option<NetworkTimeSync>>,
    launch_clock: Signal<Option<LaunchClockMsg>>,
    network_topology: Signal<NetworkTopologyMsg>,
    warning_event_counter: Signal<u64>,
    error_event_counter: Signal<u64>,
    flight_state: Signal<FlightState>,
    board_status: Signal<Vec<BoardStatusEntry>>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    user_gps: Signal<Option<(f64, f64)>>,
) {
    let mut warnings = warnings;
    let mut errors = errors;
    let mut warning_event_counter = warning_event_counter;
    let mut error_event_counter = error_event_counter;
    let notifications = notifications;
    let notification_history = notification_history;
    let dismissed_notifications = dismissed_notifications;
    let unread_notification_ids = unread_notification_ids;
    let mut action_policy = action_policy;
    let mut fill_targets = fill_targets;
    let mut network_time = network_time;
    let mut launch_clock = launch_clock;
    let mut network_topology = network_topology;
    let mut flight_state = flight_state;
    let mut board_status = board_status;
    let mut rocket_gps = rocket_gps;
    let _user_gps = user_gps;

    let Ok(msg) = serde_json::from_str::<WsInMsg>(s) else {
        return;
    };
    note_incoming_ws_message(s.len());

    match msg {
        WsInMsg::Telemetry(row) => {
            note_incoming_telemetry_rows(1, 0);
            charts_cache_ingest_row(&row);
            update_latest_telemetry(&row);
            if RESEED_IN_PROGRESS.load(Ordering::Relaxed)
                && let Ok(mut v) = RESEED_LIVE_BUFFER.lock()
            {
                v.push(row.clone());
            }

            if let Some((lat, lon)) = row_to_gps(&row) {
                rocket_gps.set(Some((lat, lon)));
            }

            // Queue telemetry for UI batch flush
            if let Ok(mut q) = TELEMETRY_QUEUE.lock() {
                q.push_back(row);

                // Safety cap if UI stalls
                while q.len() > MAX_TELEMETRY_QUEUE {
                    q.pop_front();
                }
            }
        }

        WsInMsg::TelemetryBatch(batch) => {
            if batch.is_empty() {
                return;
            }
            note_incoming_telemetry_rows(batch.len(), 1);
            update_latest_telemetry_batch(&batch);
            let reseed_active = RESEED_IN_PROGRESS.load(Ordering::Relaxed);
            let mut reseed_live = if reseed_active {
                RESEED_LIVE_BUFFER.lock().ok()
            } else {
                None
            };
            let mut latest_gps = None;
            if let Ok(mut q) = TELEMETRY_QUEUE.lock() {
                q.reserve(batch.len());
                for row in batch {
                    charts_cache_ingest_row(&row);
                    if let Some(v) = reseed_live.as_mut() {
                        v.push(row.clone());
                    }
                    if let Some((lat, lon)) = row_to_gps(&row) {
                        latest_gps = Some((lat, lon));
                    }
                    q.push_back(row);
                }

                while q.len() > MAX_TELEMETRY_QUEUE {
                    q.pop_front();
                }
            }
            if latest_gps.is_some() {
                rocket_gps.set(latest_gps);
            }
        }

        WsInMsg::FlightState(st) => {
            flight_state.set(st.state);
        }

        WsInMsg::LaunchClock(clock) => {
            launch_clock.set(Some(clock));
        }

        WsInMsg::Warning(w) => {
            let mut v = warnings.read().clone();
            v.insert(0, w.clone());
            if v.len() > 500 {
                v.truncate(500);
            }
            warnings.set(v);
            let next = {
                let current = *warning_event_counter.read();
                current.saturating_add(1)
            };
            warning_event_counter.set(next);
        }

        WsInMsg::Error(e) => {
            let mut v = errors.read().clone();
            v.insert(0, e.clone());
            if v.len() > 500 {
                v.truncate(500);
            }
            errors.set(v);
            let next = {
                let current = *error_event_counter.read();
                current.saturating_add(1)
            };
            error_event_counter.set(next);
        }

        WsInMsg::BoardStatus(status) => {
            board_status.set(status.boards);
        }

        WsInMsg::NetworkTopology(topology) => {
            network_topology.set(topology);
        }

        WsInMsg::Notifications(list) => {
            apply_notifications_snapshot(
                list,
                notifications,
                notification_history,
                dismissed_notifications,
                unread_notification_ids,
            );
        }

        WsInMsg::ActionPolicy(policy) => {
            action_policy.set(policy);
        }

        WsInMsg::FillTargets(targets) => {
            fill_targets.set(Some(targets));
        }

        WsInMsg::RecordingStatus(_status) => {}

        WsInMsg::NetworkTime(t) => {
            network_time.set(Some(NetworkTimeSync {
                network_ms: t.timestamp_ms,
                received_mono_ms: monotonic_now_ms(),
            }));
        }
    }
}

// --------------------------------------------------------------------------------------------
// JS helpers
// --------------------------------------------------------------------------------------------
#[cfg(any(target_arch = "wasm32", target_os = "ios"))]
fn js_read_window_string(key: &str) -> Option<String> {
    js_eval(&format!(
        r#"
        (function() {{
          try {{
            const v = window[{key:?}];
            window.__gs26_tmp_str = (typeof v === "string" || typeof v === "boolean" || typeof v === "number") ? String(v) : "";
          }} catch (e) {{
            window.__gs26_tmp_str = "";
          }}
        }})();
        "#
    ));

    js_get_tmp_str()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn js_eval(js: &str) {
    let _ = js_sys::eval(js);
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn js_eval(js: &str) {
    dioxus::document::eval(js);
}

#[cfg(target_arch = "wasm32")]
fn js_get_tmp_str() -> Option<String> {
    let win = web_sys::window()?;
    let v = js_sys::Reflect::get(&win, &wasm_bindgen::JsValue::from_str("__gs26_tmp_str")).ok()?;
    v.as_string()
}

#[cfg(all(not(target_arch = "wasm32"), target_os = "ios"))]
fn js_get_tmp_str() -> Option<String> {
    None
}

fn js_is_ground_map_ready() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    return true;

    #[cfg(target_arch = "wasm32")]
    {
        js_eval(
            r#"
        (function() {
          try {
            const ok =
              (window.__gs26_ground_station_loaded === true) &&
              (typeof window.updateGroundMapMarkers === "function") &&
              (typeof window.initGroundMap === "function");

            window.__gs26_tmp_ready = ok ? "true" : "false";
          } catch (e) {
            window.__gs26_tmp_ready = "false";
          }
        })();
        "#,
        );

        js_read_window_string("__gs26_tmp_ready")
            .unwrap_or_else(|| "false".to_string())
            .eq_ignore_ascii_case("true")
    }
}
