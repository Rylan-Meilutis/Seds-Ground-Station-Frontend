#![allow(clippy::redundant_locals)]

// frontend/src/telemetry_dashboard/mod.rs

mod actions_tab;
mod calibration_tab;
mod connection_status_tab;
pub mod data_chart;
pub mod data_tab;
mod detailed_tab;
pub mod errors_tab;
mod firmware_update_tab;
mod gps;
pub(crate) mod gps_android;
#[cfg(target_os = "linux")]
mod gps_linux;
mod gps_webview;
#[cfg(target_os = "windows")]
mod gps_windows;
pub mod layout;
mod layout_settings_tab;
mod messages_tab;
mod network_topology_tab;
mod notifications_tab;
pub(crate) mod prelude;
pub mod types;
pub mod version_page;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod gps_apple;

pub mod map_tab;
pub mod state_tab;
pub mod warnings_tab;

use crate::app::Route;
use crate::auth;
use crate::debug_log;
use data_chart::charts_cache_request_refit;
use data_chart::{
    charts_cache_begin_reseed_build, charts_cache_cancel_reseed_build, charts_cache_clear_active,
    charts_cache_finish_reseed_build, charts_cache_ingest_row, configure_sender_split_data_types,
};

use crate::telemetry_dashboard::actions_tab::ActionsTab;
use calibration_tab::{CalibrationTab, CalibrationTabLayout};
use connection_status_tab::ConnectionStatusTab;
use data_tab::DataTab;
use detailed_tab::DetailedTab;
use dioxus::prelude::*;
use dioxus_signals::Signal;
use errors_tab::ErrorsTab;
use firmware_update_tab::FirmwareUpdateTab;
use layout::LayoutConfig;
use layout_settings_tab::{DataFilterSettingsRow, SettingsPage};
use map_tab::MapTab;
use messages_tab::MessagesTab;
use network_topology_tab::NetworkTopologyTab;
use notifications_tab::NotificationsTab;
use serde::{Deserialize, Serialize};
use state_tab::StateTab;
use types::{
    BoardStatusEntry, BoardStatusMsg, FlightState, NetworkTopologyMsg, TelemetryRow,
    TelemetryTextId, display_flight_state, intern_telemetry_text, resolve_telemetry_text,
};
use version_page::VersionTab;
use warnings_tab::WarningsTab;

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU64, Ordering},
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
static LAST_TELEMETRY_QUEUE_DRAIN_MS: AtomicI64 = AtomicI64::new(0);
static DASHBOARD_HAS_CONNECTED: AtomicBool = AtomicBool::new(false);
static LAST_WS_CONNECT_WARNING: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));
static FRONTEND_NETWORK_METRICS_STATE: Lazy<Mutex<FrontendNetworkMetrics>> =
    Lazy::new(|| Mutex::new(FrontendNetworkMetrics::default()));
static TRANSLATION_MISS_QUEUE: Lazy<Mutex<HashSet<String>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));
static TRANSLATION_REQUEST_ACTIVE: AtomicBool = AtomicBool::new(false);
static LAST_COMMAND_ACTIVATION: Lazy<Mutex<Option<(String, f64)>>> = Lazy::new(|| Mutex::new(None));
static PENDING_COMMAND_PRESS: Lazy<Mutex<Option<(String, f64)>>> = Lazy::new(|| Mutex::new(None));
#[cfg(any(target_arch = "wasm32", target_os = "ios"))]
static PAGE_VISIBILITY_BRIDGE_INSTALLED: AtomicBool = AtomicBool::new(false);
#[cfg(any(target_arch = "wasm32", target_os = "ios"))]
static PAGE_VISIBILITY_LAST_POLL_MS: AtomicI64 = AtomicI64::new(0);
#[cfg(any(target_arch = "wasm32", target_os = "ios"))]
static PAGE_VISIBILITY_CACHED: AtomicBool = AtomicBool::new(true);

const COMMAND_ACTIVATION_DEDUP_MS: f64 = 450.0;
const COMMAND_MAX_PRESS_RELEASE_MS: f64 = 650.0;
const COMMAND_VISUAL_FEEDBACK_MS: f64 = 700.0;
#[cfg(any(target_arch = "wasm32", target_os = "ios"))]
const PAGE_VISIBILITY_POLL_INTERVAL_MS: i64 = 750;

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
const NETWORK_TIME_BADGE_REFRESH_MS: u32 = 1_000;
const ACTIVE_LAUNCH_CLOCK_REFRESH_MS: u32 = 100;
const TELEMETRY_RENDER_MIN_INTERVAL_MS: i64 = 16;
const CHART_RENDER_MIN_INTERVAL_MS: i64 = 16;
const WS_STALE_RECONNECT_MS: i64 = 8_000;
const LIVE_TELEMETRY_MAX_AGE_MS: i64 = 20 * 60 * 1000;
const LIVE_TELEMETRY_MAX_FUTURE_SKEW_MS: i64 = 60_000;

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

fn live_telemetry_row_is_fresh(row: &TelemetryRow, now_ms: i64) -> bool {
    let age_ms = now_ms.saturating_sub(row.timestamp_ms);
    (-LIVE_TELEMETRY_MAX_FUTURE_SKEW_MS..=LIVE_TELEMETRY_MAX_AGE_MS).contains(&age_ms)
}

pub(crate) fn telemetry_row_received_ms(row: &TelemetryRow) -> i64 {
    if row.received_timestamp_ms > 0 {
        row.received_timestamp_ms
    } else {
        row.timestamp_ms
    }
}

fn normalize_live_telemetry_row_for_client_clock(
    mut row: TelemetryRow,
    now_ms: i64,
) -> Option<TelemetryRow> {
    row.received_timestamp_ms = now_ms;
    row.refresh_interned_ids();
    if live_telemetry_row_is_fresh(&row, now_ms) {
        return Some(row);
    }

    // Remote backends can have wall-clock skew relative to the client.
    // Preserve live telemetry instead of dropping it entirely when the row
    // is otherwise current but timestamped against a skewed host clock.
    row.timestamp_ms = now_ms;
    row.refresh_interned_ids();
    Some(row)
}

fn normalize_telemetry_rows_for_runtime(rows: &mut [TelemetryRow]) {
    for row in rows {
        if row.received_timestamp_ms == 0 {
            row.received_timestamp_ms = row.timestamp_ms;
        }
        row.refresh_interned_ids();
    }
}

fn action_control<'a>(policy: &'a ActionPolicyMsg, cmd: &str) -> Option<&'a ActionControl> {
    policy.controls.iter().find(|control| control.cmd == cmd)
}

pub(crate) fn action_policy_control_enabled(policy: &ActionPolicyMsg, cmd: &str) -> bool {
    if !policy.software_buttons_enabled {
        return false;
    }

    let Some(control) = action_control(policy, cmd) else {
        return cmd == "Abort";
    };
    control.enabled
}

pub(crate) fn action_buttons_disabled_message(policy: &ActionPolicyMsg) -> &'static str {
    if !policy.hitl_button_interlock_satisfied || !policy.hitl_launch_interlock_satisfied {
        "Actions disabled until the hardware interlock is pressed."
    } else {
        "Actions are currently disabled by the ground station."
    }
}

#[cfg(any(target_arch = "wasm32", target_os = "ios"))]
pub(crate) fn dashboard_page_visible() -> bool {
    let now_ms = current_wallclock_ms();
    let last_poll_ms = PAGE_VISIBILITY_LAST_POLL_MS.load(Ordering::Relaxed);
    if last_poll_ms > 0 && now_ms.saturating_sub(last_poll_ms) < PAGE_VISIBILITY_POLL_INTERVAL_MS {
        return PAGE_VISIBILITY_CACHED.load(Ordering::Relaxed);
    }

    if PAGE_VISIBILITY_BRIDGE_INSTALLED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        js_eval(
            r#"
            (function() {
              try {
                if (window.__gs26_page_visibility_hook_installed) return;
                const update = () => {
                  try {
                    const visible = !(document && document.visibilityState === "hidden");
                    window.__gs26_page_visible_state = visible ? "true" : "false";
                  } catch (e) {
                    window.__gs26_page_visible_state = "true";
                  }
                };
                window.__gs26_page_visibility_hook_installed = true;
                window.__gs26_page_visible_state = "true";
                if (typeof document !== "undefined" && document && typeof document.addEventListener === "function") {
                  document.addEventListener("visibilitychange", update, { passive: true });
                }
                if (typeof window !== "undefined" && window && typeof window.addEventListener === "function") {
                  window.addEventListener("pageshow", update, { passive: true });
                  window.addEventListener("pagehide", update, { passive: true });
                  window.addEventListener("focus", update, { passive: true });
                  window.addEventListener("blur", update, { passive: true });
                }
                update();
              } catch (e) {
                window.__gs26_page_visible_state = "true";
              }
            })();
            "#,
        );
    }

    js_eval(
        r#"
        (function() {
          try {
            window.__gs26_tmp_page_visible = String(window.__gs26_page_visible_state || "true");
          } catch (e) {
            window.__gs26_tmp_page_visible = "true";
          }
        })();
        "#,
    );

    let visible = js_read_window_string("__gs26_tmp_page_visible")
        .unwrap_or_else(|| "true".to_string())
        .eq_ignore_ascii_case("true");
    PAGE_VISIBILITY_CACHED.store(visible, Ordering::Relaxed);
    PAGE_VISIBILITY_LAST_POLL_MS.store(now_ms, Ordering::Relaxed);
    visible
}

#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
pub(crate) fn dashboard_page_visible() -> bool {
    true
}

macro_rules! log {
    ($($t:tt)*) => {{
        let s = format!($($t)*);
        crate::telemetry_dashboard::log(&s);
    }}
}

// Live telemetry stores, queue budgeting, cache persistence, and tests.
include!("telemetry_runtime.rs");

#[cfg(not(target_arch = "wasm32"))]
pub fn dashboard_has_cached_layout_for_base(base: &str) -> bool {
    let cache_key = layout_cache_key_for_base(base);
    persist::get_string(&cache_key)
        .and_then(|raw| serde_json::from_str::<LayoutConfig>(&raw).ok())
        .is_some_and(|layout| layout.validate().is_ok())
}

// Persisted dashboard settings, cache maintenance, and local clear/reseed actions.
include!("dashboard_storage.rs");

static WS_EPOCH: GlobalSignal<u64> = Signal::global(|| 0);
pub(crate) static WS_CONNECTED_SIGNAL: GlobalSignal<bool> = Signal::global(|| false);
static SOFTWARE_BUTTONS_ENABLED_SIGNAL: GlobalSignal<bool> = Signal::global(|| false);
static ACTION_POLICY_SIGNAL: GlobalSignal<ActionPolicyMsg> =
    Signal::global(ActionPolicyMsg::default_locked);
static ACTION_POLICY_RESYNC_EPOCH: GlobalSignal<u64> = Signal::global(|| 0);
static TELEMETRY_RENDER_EPOCH: GlobalSignal<u64> = Signal::global(|| 0);
pub(crate) static CHART_RENDER_EPOCH: GlobalSignal<u64> = Signal::global(|| 0);
static HEADER_CLOCK_TICK: GlobalSignal<u64> = Signal::global(|| 0);
static TELEMETRY_RENDER_DIRTY: AtomicBool = AtomicBool::new(false);
static CHART_RENDER_DIRTY: AtomicBool = AtomicBool::new(false);
static DASHBOARD_RUNTIME_PUMP_SCHEDULED: AtomicBool = AtomicBool::new(false);
static DASHBOARD_RUNTIME_DELAYED_PUMP_SCHEDULED: AtomicBool = AtomicBool::new(false);
static DASHBOARD_RUNTIME_TX: Lazy<
    Mutex<Option<futures_channel::mpsc::UnboundedSender<DashboardRuntimeEvent>>>,
> = Lazy::new(|| Mutex::new(None));
static PENDING_WS_OPEN_EVENTS: Lazy<Mutex<VecDeque<(u64, String)>>> =
    Lazy::new(|| Mutex::new(VecDeque::new()));
static PENDING_WS_MESSAGE_EVENTS: Lazy<Mutex<VecDeque<(u64, String)>>> =
    Lazy::new(|| Mutex::new(VecDeque::new()));
static SEED_WATCHER_TX: Lazy<Mutex<Option<futures_channel::mpsc::UnboundedSender<()>>>> =
    Lazy::new(|| Mutex::new(None));
static SEED_WATCHER_PENDING: AtomicBool = AtomicBool::new(false);
static PREFERRED_LANGUAGE: GlobalSignal<String> = Signal::global(|| "en".to_string());
static PREFERRED_CLOCK_24H: GlobalSignal<bool> = Signal::global(|| false);
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
static FRONTEND_DATA_CLEAR_EPOCH: GlobalSignal<u64> = Signal::global(|| 0);
static LAUNCH_TMINUS_DISPLAY_MIN_MS: AtomicI64 = AtomicI64::new(i64::MAX);
static LAUNCH_TMINUS_ZERO_LATCHED: AtomicBool = AtomicBool::new(false);
static LAST_TELEMETRY_RENDER_FLUSH_MS: AtomicI64 = AtomicI64::new(0);
static LAST_CHART_RENDER_FLUSH_MS: AtomicI64 = AtomicI64::new(0);
static HIDDEN_PENDING_WS_STATE: Lazy<Mutex<HiddenPendingWsState>> =
    Lazy::new(|| Mutex::new(HiddenPendingWsState::default()));

#[derive(Default, Clone)]
struct HiddenPendingWsState {
    flight_state: Option<FlightState>,
    launch_clock: Option<LaunchClockMsg>,
    board_status: Option<Vec<BoardStatusEntry>>,
    network_topology: Option<NetworkTopologyMsg>,
    notifications: Option<Vec<PersistentNotification>>,
    messages: Option<Vec<PersistentNotification>>,
    action_policy: Option<ActionPolicyMsg>,
    fill_targets: Option<FillTargetsConfig>,
    recording_status: Option<RecordingStatusMsg>,
    network_time: Option<NetworkTimeMsg>,
}

#[derive(Clone, Copy)]
enum DashboardRuntimeEvent {
    Pump,
}

fn bump_telemetry_render_epoch() {
    let mut render_epoch = TELEMETRY_RENDER_EPOCH.write();
    *render_epoch = render_epoch.wrapping_add(1);
}

fn bump_frontend_data_clear_epoch() {
    let mut epoch = FRONTEND_DATA_CLEAR_EPOCH.write();
    *epoch = epoch.wrapping_add(1);
}

pub(crate) fn frontend_data_clear_epoch() -> u64 {
    *FRONTEND_DATA_CLEAR_EPOCH.read()
}

fn mark_live_dashboard_data_dirty() {
    TELEMETRY_RENDER_DIRTY.store(true, Ordering::Release);
    CHART_RENDER_DIRTY.store(true, Ordering::Release);
    schedule_dashboard_runtime_pump();
}

fn telemetry_queue_has_rows() -> bool {
    TELEMETRY_QUEUE
        .lock()
        .map(|q| !q.is_empty())
        .unwrap_or(false)
}

fn telemetry_queue_capacity_for_rows_per_sec(rows_per_sec: f64) -> usize {
    let rows_per_sec = rows_per_sec.max(1.0);
    ((rows_per_sec * MAX_TELEMETRY_QUEUE_LATENCY_MS / 1000.0).ceil() as usize)
        .clamp(MIN_TELEMETRY_QUEUE, MAX_TELEMETRY_QUEUE)
}

fn telemetry_queue_capacity() -> usize {
    let metrics = frontend_network_metrics_snapshot();
    telemetry_queue_capacity_for_rows_per_sec(metrics.rows_per_sec)
}

fn trim_telemetry_queue_to_live_window(q: &mut VecDeque<TelemetryRow>) {
    let overflow = q.len().saturating_sub(telemetry_queue_capacity());
    if overflow > 0 {
        q.drain(0..overflow);
    }
}

fn telemetry_rows_per_drain_budget_for(
    rows_per_sec: f64,
    queue_len: usize,
    elapsed_ms: i64,
) -> usize {
    if queue_len == 0 {
        return 0;
    }

    let rows_per_sec = rows_per_sec.max(1.0);
    let elapsed_ms = elapsed_ms.max(TELEMETRY_RENDER_MIN_INTERVAL_MS).min(1_000) as f64;
    let baseline = (rows_per_sec * elapsed_ms / 1000.0).ceil() as usize;
    let target_backlog =
        (rows_per_sec * TELEMETRY_QUEUE_TARGET_LATENCY_MS / 1000.0).ceil() as usize;
    let catch_up = queue_len.saturating_sub(target_backlog).saturating_add(3) / 4;

    baseline
        .saturating_add(catch_up)
        .clamp(1, MAX_TELEMETRY_DRAIN_ROWS)
        .min(queue_len)
}

fn telemetry_rows_per_drain_budget(queue_len: usize, now_ms: i64) -> usize {
    let last = LAST_TELEMETRY_QUEUE_DRAIN_MS.swap(now_ms, Ordering::Relaxed);
    let elapsed_ms = if last > 0 {
        now_ms.saturating_sub(last)
    } else {
        TELEMETRY_RENDER_MIN_INTERVAL_MS
    };
    let metrics = frontend_network_metrics_snapshot();
    telemetry_rows_per_drain_budget_for(metrics.rows_per_sec, queue_len, elapsed_ms)
}

fn drain_telemetry_queue_for_frame() -> Vec<TelemetryRow> {
    if let Ok(mut q) = TELEMETRY_QUEUE.lock() {
        if q.is_empty() {
            return Vec::new();
        }
        trim_telemetry_queue_to_live_window(&mut q);
        let take = telemetry_rows_per_drain_budget(q.len(), current_wallclock_ms());
        q.drain(0..take).collect()
    } else {
        Vec::new()
    }
}

fn hidden_pending_ws_state_exists() -> bool {
    HIDDEN_PENDING_WS_STATE
        .lock()
        .map(|pending| {
            pending.flight_state.is_some()
                || pending.launch_clock.is_some()
                || pending.board_status.is_some()
                || pending.network_topology.is_some()
                || pending.notifications.is_some()
                || pending.messages.is_some()
                || pending.action_policy.is_some()
                || pending.fill_targets.is_some()
                || pending.recording_status.is_some()
                || pending.network_time.is_some()
        })
        .unwrap_or(false)
}

fn telemetry_render_flush_due(now_ms: i64) -> bool {
    now_ms.saturating_sub(LAST_TELEMETRY_RENDER_FLUSH_MS.load(Ordering::Relaxed))
        >= TELEMETRY_RENDER_MIN_INTERVAL_MS
}

fn chart_render_flush_due(now_ms: i64) -> bool {
    now_ms.saturating_sub(LAST_CHART_RENDER_FLUSH_MS.load(Ordering::Relaxed))
        >= CHART_RENDER_MIN_INTERVAL_MS
}

#[allow(clippy::too_many_arguments)]
fn flush_hidden_pending_ws_state(
    flight_state: &mut Signal<FlightState>,
    launch_clock: &mut Signal<Option<LaunchClockMsg>>,
    board_status: &mut Signal<Vec<BoardStatusEntry>>,
    network_topology: &mut Signal<NetworkTopologyMsg>,
    notifications: Signal<Vec<PersistentNotification>>,
    notification_history: Signal<Vec<PersistentNotification>>,
    message_history: Signal<Vec<PersistentNotification>>,
    dismissed_notifications: Signal<Vec<DismissedNotification>>,
    unread_notification_ids: Signal<Vec<u64>>,
    action_policy: &mut Signal<ActionPolicyMsg>,
    fill_targets: &mut Signal<Option<FillTargetsConfig>>,
    recording_status: &mut Signal<RecordingStatusMsg>,
    network_time: &mut Signal<Option<NetworkTimeSync>>,
) {
    let pending = if let Ok(mut slot) = HIDDEN_PENDING_WS_STATE.lock() {
        std::mem::take(&mut *slot)
    } else {
        HiddenPendingWsState::default()
    };

    if let Some(next) = pending.flight_state {
        set_signal_if_changed(flight_state, next);
    }
    if let Some(next) = pending.launch_clock {
        set_signal_if_changed(launch_clock, Some(next));
    }
    if let Some(next) = pending.board_status {
        set_signal_if_changed(board_status, next);
    }
    if let Some(next) = pending.network_topology {
        note_network_topology_received();
        set_signal_if_changed(network_topology, next);
    }
    if let Some(next) = pending.notifications {
        apply_notifications_snapshot(
            next,
            notifications,
            notification_history,
            dismissed_notifications,
            unread_notification_ids,
        );
    }
    if let Some(next) = pending.messages {
        apply_messages_snapshot(next, message_history);
    }
    if let Some(next) = pending.action_policy {
        clear_command_feedback_latches();
        set_signal_if_changed(action_policy, next);
    }
    if let Some(next) = pending.fill_targets {
        set_signal_if_changed(fill_targets, Some(next));
    }
    if let Some(next) = pending.recording_status {
        set_signal_if_changed(recording_status, next);
    }
    if let Some(next) = pending.network_time {
        let next_sync = NetworkTimeSync {
            network_ms: next.timestamp_ms,
            received_mono_ms: monotonic_now_ms(),
        };
        let changed = {
            let current = network_time.read();
            current
                .as_ref()
                .is_none_or(|value| value.network_ms != next_sync.network_ms)
        };
        if changed {
            network_time.set(Some(next_sync));
        }
    }
}

fn set_signal_if_changed<T>(signal: &mut Signal<T>, next: T)
where
    T: PartialEq + 'static,
{
    let changed = {
        let current = signal.read();
        *current != next
    };
    if changed {
        signal.set(next);
    }
}

fn bump_chart_render_epoch() {
    let mut render_epoch = CHART_RENDER_EPOCH.write();
    *render_epoch = render_epoch.wrapping_add(1);
}

fn bump_render_epoch() {
    bump_telemetry_render_epoch();
    bump_chart_render_epoch();
}

fn schedule_dashboard_runtime_pump() {
    if DASHBOARD_RUNTIME_PUMP_SCHEDULED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let sent = DASHBOARD_RUNTIME_TX
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().cloned())
        .is_some_and(|sender| sender.unbounded_send(DashboardRuntimeEvent::Pump).is_ok());

    if !sent {
        DASHBOARD_RUNTIME_PUMP_SCHEDULED.store(false, Ordering::Release);
    }
}

fn schedule_dashboard_runtime_pump_after(delay_ms: u32) {
    if delay_ms == 0 {
        schedule_dashboard_runtime_pump();
        return;
    }
    if DASHBOARD_RUNTIME_DELAYED_PUMP_SCHEDULED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    spawn(async move {
        #[cfg(target_arch = "wasm32")]
        gloo_timers::future::TimeoutFuture::new(delay_ms).await;

        #[cfg(not(target_arch = "wasm32"))]
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms as u64)).await;

        DASHBOARD_RUNTIME_DELAYED_PUMP_SCHEDULED.store(false, Ordering::Release);
        schedule_dashboard_runtime_pump();
    });
}

fn pending_ws_open_events_exist() -> bool {
    PENDING_WS_OPEN_EVENTS
        .lock()
        .map(|q| !q.is_empty())
        .unwrap_or(false)
}

fn pending_ws_message_events_exist() -> bool {
    PENDING_WS_MESSAGE_EVENTS
        .lock()
        .map(|q| !q.is_empty())
        .unwrap_or(false)
}

#[cfg(target_arch = "wasm32")]
fn queue_ws_open_event(epoch: u64, ws_url: String) {
    if let Ok(mut q) = PENDING_WS_OPEN_EVENTS.lock() {
        q.push_back((epoch, ws_url));
        while q.len() > 8 {
            q.pop_front();
        }
    }
    schedule_dashboard_runtime_pump();
}

#[cfg(target_arch = "wasm32")]
fn queue_ws_message_event(epoch: u64, payload: String) {
    if let Ok(mut q) = PENDING_WS_MESSAGE_EVENTS.lock() {
        q.push_back((epoch, payload));
        while q.len() > 512 {
            q.pop_front();
        }
    }
    schedule_dashboard_runtime_pump();
}

#[cfg(target_arch = "wasm32")]
fn queue_live_telemetry_from_ws_message(payload: &str) -> bool {
    let Ok(msg) = serde_json::from_str::<WsTelemetryIngressMsg>(payload) else {
        return false;
    };

    LAST_WS_ACTIVITY_MONO_MS.store(monotonic_now_ms() as i64, Ordering::Relaxed);
    note_incoming_ws_message(payload.len());

    let page_visible = dashboard_page_visible();
    let now_ms = current_wallclock_ms();

    match msg {
        WsTelemetryIngressMsg::Telemetry(row) => {
            let Some(row) = normalize_live_telemetry_row_for_client_clock(row, now_ms) else {
                return true;
            };
            note_incoming_telemetry_rows(1, 0);
            if RESEED_IN_PROGRESS.load(Ordering::Relaxed)
                && let Ok(mut v) = RESEED_LIVE_BUFFER.lock()
            {
                v.push(row.clone());
            }
            if let Ok(mut q) = TELEMETRY_QUEUE.lock() {
                q.push_back(row);
                trim_telemetry_queue_to_live_window(&mut q);
            }
        }
        WsTelemetryIngressMsg::TelemetryBatch(batch) => {
            if batch.is_empty() {
                return true;
            }
            let batch: Vec<TelemetryRow> = batch
                .into_iter()
                .filter_map(|row| normalize_live_telemetry_row_for_client_clock(row, now_ms))
                .collect();
            if batch.is_empty() {
                return true;
            }
            note_incoming_telemetry_rows(batch.len(), 1);
            let reseed_active = RESEED_IN_PROGRESS.load(Ordering::Relaxed);
            let mut reseed_live = if reseed_active {
                RESEED_LIVE_BUFFER.lock().ok()
            } else {
                None
            };
            if let Ok(mut q) = TELEMETRY_QUEUE.lock() {
                if let Some(v) = reseed_live.as_mut() {
                    q.reserve(batch.len());
                    for row in batch {
                        v.push(row.clone());
                        q.push_back(row);
                    }
                } else {
                    q.extend(batch);
                }
                trim_telemetry_queue_to_live_window(&mut q);
            }
        }
    }

    if page_visible {
        mark_live_dashboard_data_dirty();
    }
    true
}

fn schedule_seed_watcher() {
    SEED_WATCHER_PENDING.store(true, Ordering::Release);
    let _ = SEED_WATCHER_TX
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().cloned())
        .map(|sender| sender.unbounded_send(()));
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

fn calibration_visibility_cache_key_for_base(base: &str) -> String {
    let normalized = normalize_base_url(base.to_string());
    if normalized.is_empty() {
        return format!("{CALIBRATION_VISIBILITY_CACHE_KEY_PREFIX}default");
    }

    let mut key =
        String::with_capacity(CALIBRATION_VISIBILITY_CACHE_KEY_PREFIX.len() + normalized.len());
    key.push_str(CALIBRATION_VISIBILITY_CACHE_KEY_PREFIX);
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
    reset_tminus_display_latch();
    if let Ok(mut activity) = TELEMETRY_ACTIVITY_BY_SENDER.lock() {
        activity.clear();
    }

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
    schedule_seed_watcher();
}

fn note_ws_connected_and_restore_data_flow(
    ws_url: String,
    epoch: u64,
    notifications: &mut Signal<Vec<PersistentNotification>>,
    notification_history: &mut Signal<Vec<PersistentNotification>>,
    unread_notification_ids: &mut Signal<Vec<u64>>,
) {
    let was_connected = frontend_network_metrics_snapshot().ws_connected;
    LAST_WS_ACTIVITY_MONO_MS.store(monotonic_now_ms() as i64, Ordering::Relaxed);
    note_ws_connection_state(true, ws_url, None, epoch);
    clear_ws_connection_notification(notifications, notification_history, unread_notification_ids);
    bump_render_epoch();
    set_reseed_status_running();
    bump_seed_epoch();
    if !was_connected {
        charts_cache_request_refit();
    }
}

fn refresh_flight_state_after_ws_reconnect(mut flight_state: Signal<FlightState>, epoch: u64) {
    spawn(async move {
        match http_get_json::<FlightState>("/flightstate").await {
            Ok(state) => {
                if *WS_EPOCH.read() == epoch {
                    set_signal_if_changed(&mut flight_state, state);
                }
            }
            Err(err) => {
                log!("[flightstate] reconnect refresh failed: {err}");
            }
        }
    });
}

fn note_ws_connected_from_live_message(
    epoch: u64,
    notifications: Signal<Vec<PersistentNotification>>,
    notification_history: Signal<Vec<PersistentNotification>>,
    unread_notification_ids: Signal<Vec<u64>>,
) {
    if frontend_network_metrics_snapshot().ws_connected {
        return;
    }
    LAST_WS_ACTIVITY_MONO_MS.store(monotonic_now_ms() as i64, Ordering::Relaxed);
    note_ws_connection_state(true, auth_ws_url(&UrlConfig::base_ws()), None, epoch);
    let mut notifications = notifications;
    let mut notification_history = notification_history;
    let mut unread_notification_ids = unread_notification_ids;
    clear_ws_connection_notification(
        &mut notifications,
        &mut notification_history,
        &mut unread_notification_ids,
    );
    bump_render_epoch();
}

fn note_local_ws_disconnect(reason: impl Into<String>) {
    if !frontend_network_metrics_snapshot().ws_connected {
        return;
    }
    let reason = reason.into();
    LAST_WS_ACTIVITY_MONO_MS.store(0, Ordering::Relaxed);
    LAST_TOPOLOGY_ACTIVITY_MONO_MS.store(0, Ordering::Relaxed);
    note_ws_connection_state(
        false,
        auth_ws_url(&UrlConfig::base_ws()),
        Some(reason),
        *WS_EPOCH.read(),
    );
}

#[cfg(target_arch = "wasm32")]
fn platform_network_available() -> bool {
    web_sys::window()
        .map(|window| window.navigator().on_line())
        .unwrap_or(true)
}

#[cfg(not(target_arch = "wasm32"))]
fn platform_network_available() -> bool {
    true
}

pub(crate) fn note_network_topology_received() {
    LAST_TOPOLOGY_ACTIVITY_MONO_MS.store(monotonic_now_ms() as i64, Ordering::Relaxed);
}

pub(crate) fn frontend_topology_message_age_ms() -> Option<i64> {
    let last_mono_ms = LAST_TOPOLOGY_ACTIVITY_MONO_MS.load(Ordering::Relaxed);
    if last_mono_ms <= 0 {
        return None;
    }
    Some((monotonic_now_ms() as i64).saturating_sub(last_mono_ms))
}

#[allow(clippy::too_many_arguments)]
fn refresh_layout_after_ws_reconnect(
    layout_config: Signal<Option<LayoutConfig>>,
    layout_loading: Signal<bool>,
    layout_error: Signal<Option<String>>,
    layout_error_dismissed: Signal<Option<String>>,
    layout_request_base: Signal<String>,
    calibration_has_sensors: Signal<Option<bool>>,
    calibration_request_base: Signal<String>,
    action_policy: Signal<ActionPolicyMsg>,
) {
    let base = UrlConfig::base_http();
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
        match http_get_json::<LayoutConfig>("/api/layout").await {
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

                if let Ok(policy) = http_get_json::<ActionPolicyMsg>("/api/action_policy").await {
                    set_signal_if_changed(&mut action_policy, policy);
                }

                if !changed {
                    return;
                }

                configure_sender_split_data_types(&layout.data_tab.sender_split_data_types);
                rebuild_chart_cache_from_visible_rows();
                RESTORED_TELEMETRY_CACHE_NEEDS_CHART_REBUILD.store(false, Ordering::Relaxed);
                layout_config.set(Some(layout.clone()));
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

include!("dashboard_settings.rs");
include!("dashboard_connection.rs");
include!("dashboard_component.rs");
include!("dashboard_transport.rs");
