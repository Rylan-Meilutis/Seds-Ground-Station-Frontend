// Live telemetry runtime.
//
// This include keeps the high-rate data path out of the dashboard component:
// queue trimming, live/latest caches, chart ingestion, persistence snapshots,
// and the focused unit tests for those behaviors live here.

pub const HISTORY_MS: i64 = 60_000 * 20; // default 20 minutes
const DEFAULT_TELEMETRY_RETENTION_MS: u64 = HISTORY_MS as u64;
const DEFAULT_TELEMETRY_VIEW_WINDOW_MS: u64 = HISTORY_MS as u64;
const MIN_TELEMETRY_HISTORY_MS: u64 = 5 * 60_000;
const MAX_TELEMETRY_HISTORY_MS: u64 = 60 * 60_000;
pub(crate) const MIN_TELEMETRY_HISTORY_MINUTES: u64 = MIN_TELEMETRY_HISTORY_MS / 60_000;
pub(crate) const MAX_TELEMETRY_HISTORY_MINUTES: u64 = MAX_TELEMETRY_HISTORY_MS / 60_000;
pub(crate) const TELEMETRY_HISTORY_PRESET_MINUTES: [u64; 5] = [5, 10, 20, 30, 60];
const UI_ROW_BUCKET_MS: i64 = 20; // Match chart bucket width in data_chart.rs.
const STARTUP_SEED_DELAY_MS: u64 = 1_200;
const MAX_TELEMETRY_QUEUE: usize = 120_000;
const MIN_TELEMETRY_QUEUE: usize = 64;
const MAX_TELEMETRY_QUEUE_LATENCY_MS: f64 = 2_000.0;
const TELEMETRY_QUEUE_TARGET_LATENCY_MS: f64 = 500.0;
const MAX_TELEMETRY_DRAIN_ROWS: usize = 4_096;

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
struct UiRowKey {
    bucket: i64,
    data_type: TelemetryTextId,
    sender_id: TelemetryTextId,
}

#[derive(Clone, Eq, PartialEq, Hash)]
struct LatestTelemetryKey {
    data_type: TelemetryTextId,
    sender_id: TelemetryTextId,
}

#[derive(Clone)]
struct LatestTelemetrySample {
    timestamp_ms: i64,
    received_timestamp_ms: i64,
    data_type: TelemetryTextId,
    sender_id: TelemetryTextId,
    values: Arc<[Option<f32>]>,
}

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
struct ChannelMinMaxKey {
    data_type: TelemetryTextId,
    sender_id: TelemetryTextId,
    index: usize,
}

impl LatestTelemetryKey {
    /// Builds the cache key used for latest-row tracking.
    fn new(data_type: TelemetryTextId, sender_id: TelemetryTextId) -> Self {
        Self {
            data_type,
            sender_id,
        }
    }
}

#[derive(Default)]
struct UiTelemetryStore {
    rows: BTreeMap<UiRowKey, TelemetryRow>,
    channel_minmax_cache: HashMap<ChannelMinMaxKey, (f32, f32)>,
    channel_minmax_dirty: bool,
}

impl UiTelemetryStore {
    /// Replaces the compacted UI store with a fresh telemetry snapshot.
    fn replace_from_rows(&mut self, rows: &[TelemetryRow]) {
        self.rows.clear();
        self.channel_minmax_cache.clear();
        self.channel_minmax_dirty = true;
        self.apply_rows(rows.iter().cloned());
    }

    /// Inserts rows into the compacted UI store, keeping only the newest row per bucket.
    fn apply_rows<I>(&mut self, rows: I)
    where
        I: IntoIterator<Item = TelemetryRow>,
    {
        let mut newest_received_ms = None::<i64>;
        for row in rows {
            // The UI only needs one representative row per bucket/sender/type tuple.
            newest_received_ms = Some(
                newest_received_ms.map_or(telemetry_row_received_ms(&row), |current| {
                    current.max(telemetry_row_received_ms(&row))
                }),
            );
            let key = UiRowKey {
                bucket: telemetry_row_received_ms(&row).div_euclid(UI_ROW_BUCKET_MS),
                data_type: row.interned_data_type_id(),
                sender_id: row.interned_sender_id(),
            };
            self.rows.insert(key, row);
        }
        self.channel_minmax_dirty = true;

        if let Some(newest_received_ms) = newest_received_ms {
            self.prune_history_from(newest_received_ms);
        }
    }

    /// Drops buckets that are older than the retained history window.
    #[allow(dead_code)]
    fn prune_history(&mut self) {
        let newest_received_ms = self.rows.values().map(telemetry_row_received_ms).max();
        let Some(newest_received_ms) = newest_received_ms else {
            return;
        };
        self.prune_history_from(newest_received_ms);
    }

    fn prune_history_from(&mut self, newest_received_ms: i64) {
        let min_received_ms = newest_received_ms - telemetry_history_retention_ms();
        let mut pruned_any = false;
        while self
            .rows
            .first_key_value()
            .is_some_and(|(_, row)| telemetry_row_received_ms(row) < min_received_ms)
        {
            self.rows.pop_first();
            pruned_any = true;
        }
        if pruned_any {
            self.channel_minmax_dirty = true;
        }
    }

    /// Returns the compacted UI store as a sorted vector.
    fn snapshot(&self) -> Vec<TelemetryRow> {
        self.rows.values().cloned().collect()
    }

    /// Returns whether the compacted UI store currently has any rows.
    fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Returns up to the newest `limit` rows from the compacted UI store.
    fn snapshot_tail(&self, limit: usize) -> Vec<TelemetryRow> {
        let take = self.rows.len().min(limit);
        self.rows
            .values()
            .rev()
            .take(take)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Returns the newest rocket GPS coordinates currently present in the compacted store.
    fn latest_rocket_gps(&self) -> Option<(f64, f64)> {
        self.rows.values().rev().find_map(row_to_gps)
    }

    /// Returns the newest rocket GPS altitude currently present in the compacted store.
    fn latest_rocket_gps_altitude_m(&self) -> Option<f64> {
        self.rows.values().rev().find_map(row_to_gps_altitude_m)
    }

    fn channel_minmax(
        &mut self,
        data_type: TelemetryTextId,
        sender_id: Option<TelemetryTextId>,
        index: usize,
    ) -> Option<(f32, f32)> {
        self.rebuild_channel_minmax_cache_if_dirty();
        let key = ChannelMinMaxKey {
            data_type,
            sender_id: sender_id.unwrap_or(TelemetryTextId::EMPTY),
            index,
        };
        self.channel_minmax_cache.get(&key).copied()
    }

    fn rebuild_channel_minmax_cache_if_dirty(&mut self) {
        if !self.channel_minmax_dirty {
            return;
        }

        self.channel_minmax_cache.clear();
        for row in self.rows.values() {
            let data_type = row.interned_data_type_id();
            let sender_id = row.interned_sender_id();
            for (index, value) in row.values.iter().enumerate() {
                let Some(value) = *value else {
                    continue;
                };
                for key in [
                    ChannelMinMaxKey {
                        data_type,
                        sender_id,
                        index,
                    },
                    ChannelMinMaxKey {
                        data_type,
                        sender_id: TelemetryTextId::EMPTY,
                        index,
                    },
                ] {
                    self.channel_minmax_cache
                        .entry(key)
                        .and_modify(|(min_value, max_value)| {
                            *min_value = min_value.min(value);
                            *max_value = max_value.max(value);
                        })
                        .or_insert((value, value));
                }
            }
        }
        self.channel_minmax_dirty = false;
    }
}

static UI_TELEMETRY_STORE: Lazy<Mutex<UiTelemetryStore>> =
    Lazy::new(|| Mutex::new(UiTelemetryStore::default()));
static LATEST_TELEMETRY: Lazy<Mutex<HashMap<LatestTelemetryKey, LatestTelemetrySample>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static LATEST_TELEMETRY_BY_TYPE: Lazy<Mutex<HashMap<TelemetryTextId, LatestTelemetrySample>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static TELEMETRY_PACKET_COUNTS_BY_SENDER: Lazy<Mutex<HashMap<TelemetryTextId, u64>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
#[derive(Clone, Copy)]
#[allow(dead_code)]
struct SenderTelemetryActivity {
    last_mono_ms: i64,
    ws_epoch: u64,
}
static TELEMETRY_ACTIVITY_BY_SENDER: Lazy<
    Mutex<HashMap<TelemetryTextId, SenderTelemetryActivity>>,
> = Lazy::new(|| Mutex::new(HashMap::new()));
static LAST_TELEMETRY_CACHE_PERSIST_MS: AtomicI64 = AtomicI64::new(0);
static RESTORED_TELEMETRY_CACHE_NEEDS_CHART_REBUILD: AtomicBool = AtomicBool::new(false);
static TELEMETRY_RETENTION_MS: AtomicU64 = AtomicU64::new(DEFAULT_TELEMETRY_RETENTION_MS);
static TELEMETRY_VIEW_WINDOW_MS: AtomicU64 = AtomicU64::new(DEFAULT_TELEMETRY_VIEW_WINDOW_MS);

pub(crate) fn telemetry_history_retention_ms() -> i64 {
    TELEMETRY_RETENTION_MS.load(Ordering::Relaxed) as i64
}

pub(crate) fn telemetry_view_window_ms() -> i64 {
    TELEMETRY_VIEW_WINDOW_MS.load(Ordering::Relaxed) as i64
}

/// Sorts telemetry rows into a stable UI presentation order.
fn sort_rows(rows: &mut [TelemetryRow]) {
    rows.sort_by(|a, b| {
        telemetry_row_received_ms(a)
            .cmp(&telemetry_row_received_ms(b))
            .then_with(|| a.timestamp_ms.cmp(&b.timestamp_ms))
            .then_with(|| a.sender_id.cmp(&b.sender_id))
            .then_with(|| a.data_type.cmp(&b.data_type))
    });
}

/// Trims a telemetry vector down to the retained history window.
fn prune_history(rows: &mut Vec<TelemetryRow>) {
    if let Some(last_received_ms) = rows.iter().map(telemetry_row_received_ms).max() {
        let cutoff = last_received_ms - telemetry_history_retention_ms();
        rows.retain(|row| telemetry_row_received_ms(row) >= cutoff);
        sort_rows(rows);
    }
}

/// Compacts raw telemetry rows down to the newest row per UI bucket.
fn compact_rows_for_ui(rows: Vec<TelemetryRow>) -> Vec<TelemetryRow> {
    let mut by_key: HashMap<(TelemetryTextId, TelemetryTextId, i64), TelemetryRow> = HashMap::new();
    for row in rows {
        let bucket = telemetry_row_received_ms(&row).div_euclid(UI_ROW_BUCKET_MS);
        let key = (
            row.interned_data_type_id(),
            row.interned_sender_id(),
            bucket,
        );
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
    reset_telemetry_packet_counts(rows);
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
    note_sender_packet_count_batch(rows);
}

fn reset_telemetry_packet_counts(rows: &[TelemetryRow]) {
    if let Ok(mut counts) = TELEMETRY_PACKET_COUNTS_BY_SENDER.lock() {
        counts.clear();
        for row in rows {
            let sender_id = row.interned_sender_id();
            if !sender_id.is_empty() {
                *counts.entry(sender_id).or_insert(0) += 1;
            }
        }
    }
}

fn note_sender_packet_count_batch(rows: &[TelemetryRow]) {
    if let Ok(mut counts) = TELEMETRY_PACKET_COUNTS_BY_SENDER.lock() {
        for row in rows {
            let sender_id = row.interned_sender_id();
            if !sender_id.is_empty() {
                *counts.entry(sender_id).or_insert(0) += 1;
            }
        }
    }
}

fn note_sender_telemetry_activity_batch(rows: &[TelemetryRow]) {
    let now_ms = monotonic_now_ms() as i64;
    let ws_epoch = *WS_EPOCH.read();
    if let Ok(mut activity) = TELEMETRY_ACTIVITY_BY_SENDER.lock() {
        for row in rows {
            let sender_id = row.interned_sender_id();
            if sender_id.is_empty() {
                continue;
            }
            activity.insert(
                sender_id,
                SenderTelemetryActivity {
                    last_mono_ms: now_ms,
                    ws_epoch,
                },
            );
        }
    }
}

fn ingest_telemetry_rows_into_runtime_caches_with_chart_mode(
    rows: Vec<TelemetryRow>,
    charts_live_enabled: bool,
) -> Option<(Option<(f64, f64)>, Option<f64>)> {
    if rows.is_empty() {
        return None;
    }

    note_sender_telemetry_activity_batch(&rows);
    update_latest_telemetry_batch(&rows);
    if charts_live_enabled {
        data_chart::charts_cache_ingest_rows(&rows);
    } else {
        data_chart::charts_cache_store_rows_for_later(&rows);
    }
    if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
        store.apply_rows(rows);
        return Some((
            store.latest_rocket_gps(),
            store.latest_rocket_gps_altitude_m(),
        ));
    }
    None
}

#[cfg(test)]
fn ingest_telemetry_rows_profile_only(
    rows: Vec<TelemetryRow>,
) -> Option<(Option<(f64, f64)>, Option<f64>)> {
    if rows.is_empty() {
        return None;
    }

    update_latest_telemetry_batch(&rows);
    for row in &rows {
        charts_cache_ingest_row(row);
    }
    if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
        store.apply_rows(rows);
        return Some((
            store.latest_rocket_gps(),
            store.latest_rocket_gps_altitude_m(),
        ));
    }
    None
}

#[allow(dead_code)]
fn stale_sender_telemetry_for_epoch(
    activity: &HashMap<TelemetryTextId, SenderTelemetryActivity>,
    ws_epoch: u64,
    now_ms: i64,
    stale_limit_ms: i64,
    active_recent_limit_ms: i64,
) -> Vec<String> {
    if stale_limit_ms <= 0 || active_recent_limit_ms <= 0 {
        return Vec::new();
    }

    let mut current_epoch_senders = 0_usize;
    let mut active_sender_present = false;
    let mut stale_senders = Vec::new();

    for (sender_id, entry) in activity.iter() {
        if entry.ws_epoch != ws_epoch || sender_id.is_empty() {
            continue;
        }
        current_epoch_senders += 1;
        let idle_ms = now_ms.saturating_sub(entry.last_mono_ms);
        if idle_ms <= active_recent_limit_ms {
            active_sender_present = true;
        } else if idle_ms >= stale_limit_ms {
            stale_senders.push(resolve_telemetry_text(*sender_id).to_string());
        }
    }

    if active_sender_present && current_epoch_senders >= 2 {
        stale_senders.sort();
        stale_senders
    } else {
        Vec::new()
    }
}

#[allow(dead_code)]
fn stale_sender_telemetry_for_current_ws_epoch(
    now_ms: i64,
    stale_limit_ms: i64,
    active_recent_limit_ms: i64,
) -> Vec<String> {
    let ws_epoch = *WS_EPOCH.read();
    let Ok(activity) = TELEMETRY_ACTIVITY_BY_SENDER.lock() else {
        return Vec::new();
    };
    stale_sender_telemetry_for_epoch(
        &activity,
        ws_epoch,
        now_ms,
        stale_limit_ms,
        active_recent_limit_ms,
    )
}

pub(crate) fn latest_rocket_gps_from_store() -> Option<(f64, f64)> {
    UI_TELEMETRY_STORE
        .lock()
        .ok()
        .and_then(|store| store.latest_rocket_gps())
}

pub(crate) fn latest_rocket_gps_altitude_m_from_store() -> Option<f64> {
    UI_TELEMETRY_STORE
        .lock()
        .ok()
        .and_then(|store| store.latest_rocket_gps_altitude_m())
}

/// Applies latest-row replacement rules while both latest-row maps are already locked.
fn update_latest_telemetry_locked(
    latest: &mut HashMap<LatestTelemetryKey, LatestTelemetrySample>,
    latest_by_type: &mut HashMap<TelemetryTextId, LatestTelemetrySample>,
    row: &TelemetryRow,
) {
    let row_received_ms = telemetry_row_received_ms(row);
    let data_type_id = row.interned_data_type_id();
    let sender_id = row.interned_sender_id();
    let key = LatestTelemetryKey::new(data_type_id, sender_id);
    let should_replace = latest
        .get(&key)
        .is_none_or(|existing| existing.received_timestamp_ms <= row_received_ms);
    let should_replace_type = latest_by_type
        .get(&data_type_id)
        .is_none_or(|existing| existing.received_timestamp_ms <= row_received_ms);
    if !should_replace && !should_replace_type {
        return;
    }

    let values = Arc::<[Option<f32>]>::from(row.values.clone());
    if should_replace {
        latest.insert(
            key,
            LatestTelemetrySample {
                timestamp_ms: row.timestamp_ms,
                received_timestamp_ms: row_received_ms,
                data_type: data_type_id,
                sender_id,
                values: values.clone(),
            },
        );
    }

    if should_replace_type {
        latest_by_type.insert(
            data_type_id,
            LatestTelemetrySample {
                timestamp_ms: row.timestamp_ms,
                received_timestamp_ms: row_received_ms,
                data_type: data_type_id,
                sender_id,
                values,
            },
        );
    }
}

fn normalize_alert_list(alerts: &mut Vec<AlertMsg>) {
    let mut seen = HashSet::<(i64, String)>::new();
    alerts.retain(|alert| seen.insert((alert.timestamp_ms, alert.message.clone())));
    alerts.sort_by_key(|alert| -alert.timestamp_ms);
    if alerts.len() > 500 {
        alerts.truncate(500);
    }
}

fn push_alert_deduped(alerts: &mut Vec<AlertMsg>, next: AlertMsg) -> bool {
    if alerts.iter().any(|existing| {
        existing.timestamp_ms == next.timestamp_ms && existing.message == next.message
    }) {
        return false;
    }
    alerts.push(next);
    normalize_alert_list(alerts);
    true
}

/// Returns the latest telemetry row for a given data type and optional sender.
pub(crate) fn latest_telemetry_row(
    data_type: &str,
    sender_id: Option<&str>,
) -> Option<TelemetryRow> {
    let data_type_id = intern_telemetry_text(data_type);
    match sender_id {
        Some(sender_id) => {
            let sender_id = intern_telemetry_text(sender_id);
            if let Ok(latest) = LATEST_TELEMETRY.lock() {
                latest
                    .get(&LatestTelemetryKey::new(data_type_id, sender_id))
                    .map(|sample| TelemetryRow {
                        timestamp_ms: sample.timestamp_ms,
                        received_timestamp_ms: sample.received_timestamp_ms,
                        data_type: resolve_telemetry_text(sample.data_type).to_string(),
                        data_type_id: sample.data_type,
                        sender_id: resolve_telemetry_text(sample.sender_id).to_string(),
                        sender_id_id: sample.sender_id,
                        values: sample.values.as_ref().to_vec(),
                    })
            } else {
                None
            }
        }
        None => {
            if let Ok(latest_by_type) = LATEST_TELEMETRY_BY_TYPE.lock() {
                latest_by_type
                    .get(&data_type_id)
                    .map(|sample| TelemetryRow {
                        timestamp_ms: sample.timestamp_ms,
                        received_timestamp_ms: sample.received_timestamp_ms,
                        data_type: resolve_telemetry_text(sample.data_type).to_string(),
                        data_type_id: sample.data_type,
                        sender_id: resolve_telemetry_text(sample.sender_id).to_string(),
                        sender_id_id: sample.sender_id,
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

pub(crate) fn telemetry_channel_minmax(
    data_type: &str,
    sender_id: Option<&str>,
    index: usize,
) -> Option<(f32, f32)> {
    let Ok(mut store) = UI_TELEMETRY_STORE.lock() else {
        return None;
    };
    let data_type_id = intern_telemetry_text(data_type);
    let sender_id = sender_id.map(intern_telemetry_text);
    store.channel_minmax(data_type_id, sender_id, index)
}

fn latest_telemetry_value_direct(
    data_type: &str,
    sender_id: Option<&str>,
    index: usize,
) -> Option<f32> {
    let data_type_id = intern_telemetry_text(data_type);
    match sender_id {
        Some(sender_id) => {
            let sender_id = intern_telemetry_text(sender_id);
            if let Ok(latest) = LATEST_TELEMETRY.lock() {
                latest
                    .get(&LatestTelemetryKey::new(data_type_id, sender_id))
                    .and_then(|row| row.values.get(index).copied().flatten())
            } else {
                None
            }
        }
        None => {
            if let Ok(latest_by_type) = LATEST_TELEMETRY_BY_TYPE.lock() {
                latest_by_type
                    .get(&data_type_id)
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
    let _ = (data_type, sender_id, index);
    None
}

#[cfg(test)]
mod latest_telemetry_tests {
    use super::{
        AlertMsg, LIVE_TELEMETRY_MAX_AGE_MS, LIVE_TELEMETRY_MAX_FUTURE_SKEW_MS, LatestTelemetryKey,
        SenderTelemetryActivity, TelemetryRow, compact_rows_for_ui,
        ingest_telemetry_rows_profile_only, intern_telemetry_text, latest_telemetry_row,
        latest_telemetry_value, live_telemetry_row_is_fresh, normalize_alert_list,
        normalize_telemetry_rows_for_runtime, prune_history, push_alert_deduped,
        reset_latest_telemetry, sort_rows, stale_sender_telemetry_for_epoch,
        telemetry_channel_minmax, telemetry_queue_capacity_for_rows_per_sec,
        telemetry_rows_per_drain_budget_for, update_latest_telemetry_locked,
    };
    use crate::telemetry_dashboard::{
        UI_TELEMETRY_STORE, charts_cache_clear_active, data_chart, data_chart::charts_cache_get,
    };
    use once_cell::sync::Lazy;
    use std::collections::HashMap;
    use std::sync::Mutex;

    static PROFILE_TEST_GUARD: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    #[test]
    fn telemetry_drain_budget_uses_real_elapsed_time() {
        let budget = telemetry_rows_per_drain_budget_for(1_000.0, 10_000, 250);
        assert!(budget >= 250);
    }

    #[test]
    fn telemetry_drain_budget_catches_up_backlog() {
        let steady_state = telemetry_rows_per_drain_budget_for(1_000.0, 500, 16);
        let backlogged = telemetry_rows_per_drain_budget_for(1_000.0, 10_000, 16);
        assert!(backlogged > steady_state);
        assert!(backlogged <= 4_096);
    }

    #[test]
    fn telemetry_queue_capacity_tracks_live_latency_window() {
        assert_eq!(telemetry_queue_capacity_for_rows_per_sec(1.0), 64);
        assert_eq!(telemetry_queue_capacity_for_rows_per_sec(1_000.0), 2_000);
    }

    #[test]
    fn does_not_alias_raw_loadcell_samples_into_calibrated_labels() {
        reset_latest_telemetry(&[TelemetryRow {
            timestamp_ms: 1_700_000_030_000,
            received_timestamp_ms: 1_700_000_030_000,
            data_type: "KG1000".to_string(),
            data_type_id: Default::default(),
            sender_id: "DAQ".to_string(),
            sender_id_id: Default::default(),
            values: vec![Some(9.5754)],
        }]);

        assert_eq!(latest_telemetry_value("KG1000", None, 0), Some(9.5754));
        assert_eq!(latest_telemetry_value("LOADCELL_WEIGHT_KG", None, 0), None);
        reset_latest_telemetry(&[]);
    }

    #[test]
    fn detects_stale_sender_when_another_sender_is_still_active() {
        let mut activity = HashMap::new();
        activity.insert(
            intern_telemetry_text("DAQ"),
            SenderTelemetryActivity {
                last_mono_ms: 1_000,
                ws_epoch: 42,
            },
        );
        activity.insert(
            intern_telemetry_text("PB"),
            SenderTelemetryActivity {
                last_mono_ms: 19_500,
                ws_epoch: 42,
            },
        );

        let stale = stale_sender_telemetry_for_epoch(&activity, 42, 20_000, 15_000, 4_000);
        assert_eq!(stale, vec!["DAQ".to_string()]);
    }

    #[test]
    fn drops_live_rows_that_arrive_too_late() {
        let now_ms = 1_700_000_100_000;
        let row = TelemetryRow {
            timestamp_ms: now_ms - LIVE_TELEMETRY_MAX_AGE_MS - 1,
            received_timestamp_ms: now_ms - LIVE_TELEMETRY_MAX_AGE_MS - 1,
            data_type: "BATTERY_VOLTAGE".to_string(),
            data_type_id: Default::default(),
            sender_id: "PB".to_string(),
            sender_id_id: Default::default(),
            values: vec![Some(12.0)],
        };
        assert!(!live_telemetry_row_is_fresh(&row, now_ms));
    }

    #[test]
    fn keeps_live_rows_with_small_clock_skew() {
        let now_ms = 1_700_000_100_000;
        let row = TelemetryRow {
            timestamp_ms: now_ms + LIVE_TELEMETRY_MAX_FUTURE_SKEW_MS - 1,
            received_timestamp_ms: now_ms + LIVE_TELEMETRY_MAX_FUTURE_SKEW_MS - 1,
            data_type: "BATTERY_VOLTAGE".to_string(),
            data_type_id: Default::default(),
            sender_id: "PB".to_string(),
            sender_id_id: Default::default(),
            values: vec![Some(12.0)],
        };
        assert!(live_telemetry_row_is_fresh(&row, now_ms));
    }

    #[test]
    fn latest_cache_prefers_newer_received_time_over_future_skewed_timestamp() {
        let mut latest = HashMap::new();
        let mut latest_by_type = HashMap::new();
        let data_type = "BATTERY_VOLTAGE".to_string();
        let sender_id = "PB".to_string();

        let first = TelemetryRow {
            timestamp_ms: 1_700_000_000_000 + LIVE_TELEMETRY_MAX_FUTURE_SKEW_MS - 1,
            received_timestamp_ms: 1_700_000_000_000,
            data_type: data_type.clone(),
            data_type_id: Default::default(),
            sender_id: sender_id.clone(),
            sender_id_id: Default::default(),
            values: vec![Some(10.0)],
        };
        update_latest_telemetry_locked(&mut latest, &mut latest_by_type, &first);

        let second = TelemetryRow {
            timestamp_ms: 1_700_000_000_100,
            received_timestamp_ms: 1_700_000_000_100,
            data_type,
            data_type_id: Default::default(),
            sender_id,
            sender_id_id: Default::default(),
            values: vec![Some(11.0)],
        };
        update_latest_telemetry_locked(&mut latest, &mut latest_by_type, &second);

        let key =
            LatestTelemetryKey::new(second.interned_data_type_id(), second.interned_sender_id());
        let sample = latest.get(&key).expect("latest sample should exist");
        assert_eq!(sample.received_timestamp_ms, second.received_timestamp_ms);
        assert_eq!(sample.values.as_ref(), &[Some(11.0)]);
    }

    #[test]
    fn history_pruning_uses_received_time_instead_of_network_timestamp() {
        let newest_received_ms = 1_700_000_500_000;
        let mut rows = vec![
            TelemetryRow {
                timestamp_ms: 1_699_000_000_000,
                received_timestamp_ms: newest_received_ms - 1_000,
                data_type: "GPS".to_string(),
                data_type_id: Default::default(),
                sender_id: "DAQ".to_string(),
                sender_id_id: Default::default(),
                values: vec![Some(1.0)],
            },
            TelemetryRow {
                timestamp_ms: 1_700_000_490_000,
                received_timestamp_ms: newest_received_ms,
                data_type: "GPS".to_string(),
                data_type_id: Default::default(),
                sender_id: "DAQ".to_string(),
                sender_id_id: Default::default(),
                values: vec![Some(2.0)],
            },
        ];

        prune_history(&mut rows);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].values, vec![Some(1.0)]);
    }

    #[test]
    fn duplicate_warning_is_not_added_twice() {
        let warning = AlertMsg {
            timestamp_ms: 1_700_000_100_000,
            message: "low battery".to_string(),
        };
        let mut alerts = vec![warning.clone()];
        assert!(!push_alert_deduped(&mut alerts, warning));
        assert_eq!(alerts.len(), 1);
    }

    #[test]
    fn normalize_alerts_removes_duplicates_and_keeps_newest_first() {
        let mut alerts = vec![
            AlertMsg {
                timestamp_ms: 20,
                message: "b".to_string(),
            },
            AlertMsg {
                timestamp_ms: 10,
                message: "a".to_string(),
            },
            AlertMsg {
                timestamp_ms: 20,
                message: "b".to_string(),
            },
        ];
        normalize_alert_list(&mut alerts);
        assert_eq!(alerts.len(), 2);
        assert_eq!(alerts[0].timestamp_ms, 20);
        assert_eq!(alerts[1].timestamp_ms, 10);
    }

    #[test]
    #[ignore = "profiling smoke test"]
    fn ingress_profile_smoke() {
        use std::time::Instant;

        let _guard = PROFILE_TEST_GUARD.lock().unwrap();
        charts_cache_clear_active();
        if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
            store.rows.clear();
        }
        reset_latest_telemetry(&[]);

        let total_rows = 20_000usize;
        let start_ts = 1_700_000_000_000i64;
        let mut rows: Vec<TelemetryRow> = (0..total_rows)
            .map(|i| TelemetryRow {
                timestamp_ms: start_ts + (i as i64 * 17),
                received_timestamp_ms: start_ts + (i as i64 * 17),
                data_type: if i % 2 == 0 {
                    "BATTERY_VOLTAGE".to_string()
                } else {
                    "GPS".to_string()
                },
                data_type_id: Default::default(),
                sender_id: if i % 3 == 0 {
                    "PB".to_string()
                } else {
                    "DAQ".to_string()
                },
                sender_id_id: Default::default(),
                values: if i % 2 == 0 {
                    vec![Some(12.0 + (i % 10) as f32 * 0.1)]
                } else {
                    vec![
                        Some(42.0 + i as f32 * 0.0001),
                        Some(-78.0 - i as f32 * 0.0001),
                    ]
                },
            })
            .collect();
        normalize_telemetry_rows_for_runtime(&mut rows);

        let started = Instant::now();
        let result = ingest_telemetry_rows_profile_only(rows);
        let elapsed = started.elapsed();

        assert!(result.is_some());
        eprintln!(
            "ingress_profile_smoke: {} rows in {:?} ({:.0} rows/s)",
            total_rows,
            elapsed,
            total_rows as f64 / elapsed.as_secs_f64().max(1e-9)
        );
    }

    #[test]
    #[ignore = "profiling smoke test"]
    fn minmax_lookup_profile_smoke() {
        use std::time::Instant;

        let _guard = PROFILE_TEST_GUARD.lock().unwrap();
        charts_cache_clear_active();
        if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
            store.rows.clear();
            store.channel_minmax_cache.clear();
            store.channel_minmax_dirty = true;
        }
        reset_latest_telemetry(&[]);

        let total_rows = 20_000usize;
        let start_ts = 1_700_100_000_000i64;
        let mut rows: Vec<TelemetryRow> = (0..total_rows)
            .map(|i| TelemetryRow {
                timestamp_ms: start_ts + (i as i64 * 20),
                received_timestamp_ms: start_ts + (i as i64 * 20),
                data_type: if i % 2 == 0 {
                    "BATTERY_VOLTAGE".to_string()
                } else {
                    "LOADCELL_WEIGHT_KG".to_string()
                },
                data_type_id: Default::default(),
                sender_id: if i % 3 == 0 {
                    "PB".to_string()
                } else {
                    "DAQ".to_string()
                },
                sender_id_id: Default::default(),
                values: vec![Some(10.0 + (i % 100) as f32 * 0.1), Some((i % 50) as f32)],
            })
            .collect();
        normalize_telemetry_rows_for_runtime(&mut rows);

        assert!(ingest_telemetry_rows_profile_only(rows).is_some());

        let started = Instant::now();
        let mut observed = 0usize;
        for _ in 0..2_000 {
            observed += telemetry_channel_minmax("BATTERY_VOLTAGE", None, 0).is_some() as usize;
            observed +=
                telemetry_channel_minmax("BATTERY_VOLTAGE", Some("PB"), 0).is_some() as usize;
            observed +=
                telemetry_channel_minmax("LOADCELL_WEIGHT_KG", Some("DAQ"), 1).is_some() as usize;
        }
        let elapsed = started.elapsed();

        assert_eq!(observed, 6_000);
        eprintln!(
            "minmax_lookup_profile_smoke: {} lookups in {:?} ({:.0} lookups/s)",
            observed,
            elapsed,
            observed as f64 / elapsed.as_secs_f64().max(1e-9)
        );
    }

    #[test]
    #[ignore = "profiling smoke test"]
    fn latest_and_chart_query_profile_smoke() {
        use std::time::Instant;

        let _guard = PROFILE_TEST_GUARD.lock().unwrap();
        charts_cache_clear_active();
        if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
            store.rows.clear();
            store.channel_minmax_cache.clear();
            store.channel_minmax_dirty = true;
        }
        reset_latest_telemetry(&[]);

        let total_rows = 12_000usize;
        let start_ts = 1_700_200_000_000i64;
        let mut rows: Vec<TelemetryRow> = (0..total_rows)
            .map(|i| TelemetryRow {
                timestamp_ms: start_ts + (i as i64 * 20),
                received_timestamp_ms: start_ts + (i as i64 * 20),
                data_type: "BATTERY_VOLTAGE".to_string(),
                data_type_id: Default::default(),
                sender_id: if i % 2 == 0 {
                    "PB".to_string()
                } else {
                    "DAQ".to_string()
                },
                sender_id_id: Default::default(),
                values: vec![Some(12.0 + (i % 20) as f32 * 0.05)],
            })
            .collect();
        normalize_telemetry_rows_for_runtime(&mut rows);

        assert!(ingest_telemetry_rows_profile_only(rows).is_some());

        let started = Instant::now();
        let mut lookups = 0usize;
        for _ in 0..500 {
            lookups += latest_telemetry_value("BATTERY_VOLTAGE", None, 0).is_some() as usize;
            lookups += latest_telemetry_row("BATTERY_VOLTAGE", Some("PB")).is_some() as usize;
            let _ = charts_cache_get("BATTERY_VOLTAGE", 1200.0, 280.0);
            lookups += 1;
        }
        let elapsed = started.elapsed();

        assert_eq!(lookups, 1_500);
        eprintln!(
            "latest_and_chart_query_profile_smoke: {} ops in {:?} ({:.0} ops/s)",
            lookups,
            elapsed,
            lookups as f64 / elapsed.as_secs_f64().max(1e-9)
        );
    }

    #[test]
    #[ignore = "profiling smoke test"]
    fn cached_restore_profile_smoke() {
        use std::time::Instant;

        let _guard = PROFILE_TEST_GUARD.lock().unwrap();
        charts_cache_clear_active();
        if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
            store.rows.clear();
            store.channel_minmax_cache.clear();
            store.channel_minmax_dirty = true;
        }
        reset_latest_telemetry(&[]);

        let total_rows = 20_000usize;
        let start_ts = 1_700_300_000_000i64;
        let mut rows: Vec<TelemetryRow> = (0..total_rows)
            .map(|i| TelemetryRow {
                timestamp_ms: start_ts + (i as i64 * 20),
                received_timestamp_ms: start_ts + (i as i64 * 20),
                data_type: if i % 2 == 0 {
                    "BATTERY_VOLTAGE".to_string()
                } else {
                    "LOADCELL_WEIGHT_KG".to_string()
                },
                data_type_id: Default::default(),
                sender_id: if i % 3 == 0 {
                    "PB".to_string()
                } else {
                    "DAQ".to_string()
                },
                sender_id_id: Default::default(),
                values: vec![Some(10.0 + (i % 100) as f32 * 0.1), Some((i % 50) as f32)],
            })
            .collect();

        let started = Instant::now();
        normalize_telemetry_rows_for_runtime(&mut rows);
        sort_rows(&mut rows);
        prune_history(&mut rows);
        let rows = compact_rows_for_ui(rows);
        if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
            store.replace_from_rows(&rows);
        }
        reset_latest_telemetry(&rows);
        charts_cache_clear_active();
        for row in &rows {
            data_chart::charts_cache_ingest_row(row);
        }
        let elapsed = started.elapsed();

        assert!(!rows.is_empty());
        eprintln!(
            "cached_restore_profile_smoke: {} rows in {:?} ({:.0} rows/s)",
            total_rows,
            elapsed,
            total_rows as f64 / elapsed.as_secs_f64().max(1e-9)
        );
    }

    #[test]
    #[ignore = "profiling smoke test"]
    fn offscreen_chart_store_profile_smoke() {
        use std::time::Instant;

        let _guard = PROFILE_TEST_GUARD.lock().unwrap();
        charts_cache_clear_active();

        let total_rows = 20_000usize;
        let start_ts = 1_700_400_000_000i64;
        let mut rows: Vec<TelemetryRow> = (0..total_rows)
            .map(|i| TelemetryRow {
                timestamp_ms: start_ts + (i as i64 * 17),
                received_timestamp_ms: start_ts + (i as i64 * 17),
                data_type: if i % 2 == 0 {
                    "BATTERY_VOLTAGE".to_string()
                } else {
                    "GPS".to_string()
                },
                data_type_id: Default::default(),
                sender_id: if i % 3 == 0 {
                    "PB".to_string()
                } else {
                    "DAQ".to_string()
                },
                sender_id_id: Default::default(),
                values: if i % 2 == 0 {
                    vec![Some(12.0 + (i % 10) as f32 * 0.1)]
                } else {
                    vec![
                        Some(42.0 + i as f32 * 0.0001),
                        Some(-78.0 - i as f32 * 0.0001),
                    ]
                },
            })
            .collect();
        normalize_telemetry_rows_for_runtime(&mut rows);

        let started = Instant::now();
        data_chart::charts_cache_store_rows_for_later(&rows);
        let elapsed = started.elapsed();

        eprintln!(
            "offscreen_chart_store_profile_smoke: {} rows in {:?} ({:.0} rows/s)",
            total_rows,
            elapsed,
            total_rows as f64 / elapsed.as_secs_f64().max(1e-9)
        );
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

#[derive(Serialize, Deserialize)]
struct TelemetryRowsCache {
    base_url: String,
    rows: Vec<TelemetryRow>,
}

fn persist_cached_telemetry_rows(rows: &[TelemetryRow]) {
    if !data_cache_enabled() {
        persist::_remove(TELEMETRY_CACHE_STORAGE_KEY);
        return;
    }
    if cache_storage_measured_bytes() >= stored_cache_budget_bytes() {
        persist::_remove(TELEMETRY_CACHE_STORAGE_KEY);
        return;
    }
    if rows.is_empty() {
        persist::_remove(TELEMETRY_CACHE_STORAGE_KEY);
        return;
    }
    let start = rows.len().saturating_sub(TELEMETRY_CACHE_MAX_ROWS);
    let cache = TelemetryRowsCache {
        base_url: normalize_base_url(UrlConfig::base_http()),
        rows: rows[start..].to_vec(),
    };
    if let Ok(raw) = serde_json::to_string(&cache) {
        persist::set_string(TELEMETRY_CACHE_STORAGE_KEY, &raw);
    }
}

fn persist_cached_telemetry_snapshot_if_due(force: bool) {
    if !data_cache_enabled() {
        persist::_remove(TELEMETRY_CACHE_STORAGE_KEY);
        LAST_TELEMETRY_CACHE_PERSIST_MS.store(0, Ordering::Relaxed);
        return;
    }
    let now_ms = current_wallclock_ms();
    let last_ms = LAST_TELEMETRY_CACHE_PERSIST_MS.load(Ordering::Relaxed);
    if !force && now_ms.saturating_sub(last_ms) < TELEMETRY_CACHE_WRITE_INTERVAL_MS {
        return;
    }

    let rows = if let Ok(store) = UI_TELEMETRY_STORE.lock() {
        if store.is_empty() {
            return;
        }
        store.snapshot_tail(TELEMETRY_CACHE_MAX_ROWS)
    } else {
        return;
    };

    persist_cached_telemetry_rows(&rows);
    LAST_TELEMETRY_CACHE_PERSIST_MS.store(now_ms, Ordering::Relaxed);
}

fn restore_cached_telemetry_rows_if_needed() -> usize {
    if !data_cache_enabled() {
        persist::_remove(TELEMETRY_CACHE_STORAGE_KEY);
        return 0;
    }
    if let Ok(store) = UI_TELEMETRY_STORE.lock()
        && !store.is_empty()
    {
        return 0;
    }

    let Some(raw) = persist::get_string(TELEMETRY_CACHE_STORAGE_KEY) else {
        return 0;
    };
    let Ok(cache) = serde_json::from_str::<TelemetryRowsCache>(&raw) else {
        persist::_remove(TELEMETRY_CACHE_STORAGE_KEY);
        return 0;
    };
    if normalize_base_url(cache.base_url) != normalize_base_url(UrlConfig::base_http()) {
        persist::_remove(TELEMETRY_CACHE_STORAGE_KEY);
        return 0;
    }
    let mut rows = cache.rows;
    if rows.is_empty() {
        return 0;
    }

    normalize_telemetry_rows_for_runtime(&mut rows);
    sort_rows(&mut rows);
    prune_history(&mut rows);
    rows = compact_rows_for_ui(rows);
    if rows.is_empty() {
        return 0;
    }

    if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
        store.replace_from_rows(&rows);
    }
    reset_latest_telemetry(&rows);
    rebuild_chart_cache_from_visible_rows();
    RESTORED_TELEMETRY_CACHE_NEEDS_CHART_REBUILD.store(true, Ordering::Relaxed);
    bump_render_epoch();
    bump_chart_render_epoch();
    rows.len()
}

fn rebuild_chart_cache_from_visible_rows() {
    let Ok(store) = UI_TELEMETRY_STORE.lock() else {
        return;
    };
    if store.is_empty() {
        return;
    }
    charts_cache_clear_active();
    for row in store.rows.values() {
        charts_cache_ingest_row(row);
    }
    bump_chart_render_epoch();
}

fn apply_telemetry_history_settings(retention_ms: u64, view_window_ms: u64) {
    let retention_ms = clamp_telemetry_history_ms(retention_ms);
    let view_window_ms = clamp_telemetry_history_ms(view_window_ms).min(retention_ms);
    TELEMETRY_RETENTION_MS.store(retention_ms, Ordering::Relaxed);
    TELEMETRY_VIEW_WINDOW_MS.store(view_window_ms, Ordering::Relaxed);

    let mut rows = ui_telemetry_rows_snapshot();
    if !rows.is_empty() {
        prune_history(&mut rows);
        if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
            store.replace_from_rows(&rows);
        }
        reset_latest_telemetry(&rows);
        if rows.is_empty() {
            charts_cache_clear_active();
            persist::_remove(TELEMETRY_CACHE_STORAGE_KEY);
        } else {
            rebuild_chart_cache_from_visible_rows();
            persist_cached_telemetry_rows(&rows);
            LAST_TELEMETRY_CACHE_PERSIST_MS.store(current_wallclock_ms(), Ordering::Relaxed);
        }
    }

    charts_cache_request_refit();
    bump_chart_render_epoch();
}
