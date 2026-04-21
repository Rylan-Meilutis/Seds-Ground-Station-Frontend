use super::*;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FrontendNetworkMetrics {
    pub(crate) ws_connected: bool,
    pub(crate) ws_url: String,
    pub(crate) base_http: String,
    pub(crate) ws_epoch: u64,
    pub(crate) ws_disconnects_total: u64,
    pub(crate) ws_messages_total: u64,
    pub(crate) ws_bytes_total: u64,
    pub(crate) telemetry_rows_total: u64,
    pub(crate) telemetry_batches_total: u64,
    pub(crate) bytes_per_sec: f64,
    pub(crate) msgs_per_sec: f64,
    pub(crate) rows_per_sec: f64,
    pub(crate) http_rtt_ms: Option<f64>,
    pub(crate) http_rtt_ema_ms: Option<f64>,
    pub(crate) last_connect_wall_ms: Option<i64>,
    pub(crate) last_disconnect_reason: Option<String>,
    pub(crate) last_ws_message_wall_ms: Option<i64>,
    pub(crate) last_rate_sample_mono_ms: f64,
    pub(crate) bytes_since_last_sample: u64,
    pub(crate) msgs_since_last_sample: u64,
    pub(crate) rows_since_last_sample: u64,
}

impl Default for FrontendNetworkMetrics {
    fn default() -> Self {
        Self {
            ws_connected: false,
            ws_url: String::new(),
            base_http: String::new(),
            ws_epoch: 0,
            ws_disconnects_total: 0,
            ws_messages_total: 0,
            ws_bytes_total: 0,
            telemetry_rows_total: 0,
            telemetry_batches_total: 0,
            bytes_per_sec: 0.0,
            msgs_per_sec: 0.0,
            rows_per_sec: 0.0,
            http_rtt_ms: None,
            http_rtt_ema_ms: None,
            last_connect_wall_ms: None,
            last_disconnect_reason: None,
            last_ws_message_wall_ms: None,
            last_rate_sample_mono_ms: 0.0,
            bytes_since_last_sample: 0,
            msgs_since_last_sample: 0,
            rows_since_last_sample: 0,
        }
    }
}

/// Resets the frontend-side WebSocket and HTTP metrics to a clean state.
pub(super) fn reset_frontend_network_metrics_state() {
    if let Ok(mut metrics) = FRONTEND_NETWORK_METRICS_STATE.lock() {
        *metrics = FrontendNetworkMetrics {
            base_http: UrlConfig::base_http(),
            ..FrontendNetworkMetrics::default()
        };
    }
}

/// Returns a snapshot of the frontend network metrics without exposing the mutex guard.
pub(super) fn frontend_network_metrics_snapshot() -> FrontendNetworkMetrics {
    FRONTEND_NETWORK_METRICS_STATE
        .lock()
        .map(|metrics| metrics.clone())
        .unwrap_or_default()
}

/// Redacts authentication tokens from a WebSocket URL before it is shown in the UI.
fn redact_ws_url_for_display(ws_url: &str) -> String {
    if let Some((prefix, query)) = ws_url.split_once('?') {
        let redacted_query = query
            .split('&')
            .map(|part| {
                if let Some((key, _)) = part.split_once('=') {
                    if key == "token" {
                        return format!("{key}=<redacted>");
                    }
                } else if part == "token" {
                    return "token=<redacted>".to_string();
                }
                part.to_string()
            })
            .collect::<Vec<_>>()
            .join("&");
        format!("{prefix}?{redacted_query}")
    } else {
        ws_url.to_string()
    }
}

/// Records a WebSocket connection or disconnection transition for the dashboard diagnostics.
pub(super) fn note_ws_connection_state(
    connected: bool,
    ws_url: String,
    reason: Option<String>,
    epoch: u64,
) {
    if connected {
        DASHBOARD_HAS_CONNECTED.store(true, Ordering::Relaxed);
        if let Ok(mut slot) = LAST_WS_CONNECT_WARNING.lock() {
            *slot = None;
        }
    }
    if let Ok(mut next) = FRONTEND_NETWORK_METRICS_STATE.lock() {
        let was_connected = next.ws_connected;
        next.ws_connected = connected;
        next.ws_url = redact_ws_url_for_display(&ws_url);
        next.base_http = UrlConfig::base_http();
        next.ws_epoch = epoch;
        if connected {
            next.last_connect_wall_ms = Some(current_wallclock_ms());
        } else if was_connected {
            next.ws_disconnects_total = next.ws_disconnects_total.saturating_add(1);
        }
        if let Some(reason) = reason {
            next.last_disconnect_reason = Some(reason);
        }
    }
}

pub(super) fn note_ws_connection_notification(
    notifications: &mut Signal<Vec<PersistentNotification>>,
    _notification_history: &mut Signal<Vec<PersistentNotification>>,
    unread_notification_ids: &mut Signal<Vec<u64>>,
    ws_url: &str,
    reason: &str,
) {
    if notifications
        .read()
        .iter()
        .any(|n| n.message.starts_with("WebSocket disconnected.\nURL:"))
    {
        return;
    }

    let now_ms = current_wallclock_ms();
    if let Ok(mut slot) = LAST_WS_CONNECT_WARNING.lock() {
        if slot.is_some() {
            return;
        }
        *slot = Some(redact_ws_url_for_display(ws_url));
    }

    let item = PersistentNotification {
        id: now_ms as u64,
        timestamp_ms: now_ms,
        message: format!(
            "WebSocket disconnected.\nURL: {}\nReason: {}",
            redact_ws_url_for_display(ws_url),
            reason.trim()
        ),
        persistent: false,
        action_label: None,
        action_cmd: None,
    };

    let mut active = notifications.read().clone();
    active.retain(|n| !n.message.starts_with("WebSocket disconnected.\nURL:"));
    active.push(item.clone());
    active.sort_by_key(|n| n.timestamp_ms);
    if active.len() > MAX_ACTIVE_NOTIFICATIONS {
        let keep_from = active.len().saturating_sub(MAX_ACTIVE_NOTIFICATIONS);
        active = active.split_off(keep_from);
    }
    notifications.set(active);

    let mut unread: HashSet<u64> = unread_notification_ids.read().iter().copied().collect();
    unread.insert(item.id);
    let mut unread_vec: Vec<u64> = unread.into_iter().collect();
    unread_vec.sort_unstable();
    unread_notification_ids.set(unread_vec);
}

pub(super) fn clear_ws_connection_notification(
    notifications: &mut Signal<Vec<PersistentNotification>>,
    notification_history: &mut Signal<Vec<PersistentNotification>>,
    unread_notification_ids: &mut Signal<Vec<u64>>,
) {
    if let Ok(mut slot) = LAST_WS_CONNECT_WARNING.lock() {
        *slot = None;
    }

    let removed_ids: HashSet<u64> = notifications
        .read()
        .iter()
        .filter(|n| n.message.starts_with("WebSocket disconnected.\nURL:"))
        .map(|n| n.id)
        .collect();
    if removed_ids.is_empty() {
        let mut history = notification_history.read().clone();
        history.retain(|n| !n.message.starts_with("WebSocket disconnected.\nURL:"));
        notification_history.set(history);
        return;
    }

    let mut active = notifications.read().clone();
    active.retain(|n| !removed_ids.contains(&n.id));
    notifications.set(active);

    let mut unread = unread_notification_ids.read().clone();
    unread.retain(|id| !removed_ids.contains(id));
    unread_notification_ids.set(unread);

    let mut history = notification_history.read().clone();
    history.retain(|n| !n.message.starts_with("WebSocket disconnected.\nURL:"));
    notification_history.set(history);
}

/// Tracks incoming WebSocket message volume and updates rate calculations.
pub(super) fn note_incoming_ws_message(raw_bytes: usize) {
    if let Ok(mut next) = FRONTEND_NETWORK_METRICS_STATE.lock() {
        let now_mono = monotonic_now_ms();
        let now_wall = current_wallclock_ms();
        if next.last_rate_sample_mono_ms <= 0.0 {
            next.last_rate_sample_mono_ms = now_mono;
        }
        next.ws_messages_total = next.ws_messages_total.saturating_add(1);
        next.ws_bytes_total = next.ws_bytes_total.saturating_add(raw_bytes as u64);
        next.bytes_since_last_sample = next
            .bytes_since_last_sample
            .saturating_add(raw_bytes as u64);
        next.msgs_since_last_sample = next.msgs_since_last_sample.saturating_add(1);
        next.last_ws_message_wall_ms = Some(now_wall);

        let dt_ms = (now_mono - next.last_rate_sample_mono_ms).max(0.0);
        if dt_ms >= 800.0 {
            let scale = 1000.0 / dt_ms;
            next.bytes_per_sec = next.bytes_since_last_sample as f64 * scale;
            next.msgs_per_sec = next.msgs_since_last_sample as f64 * scale;
            next.rows_per_sec = next.rows_since_last_sample as f64 * scale;
            next.bytes_since_last_sample = 0;
            next.msgs_since_last_sample = 0;
            next.rows_since_last_sample = 0;
            next.last_rate_sample_mono_ms = now_mono;
        }
    }
}

/// Tracks telemetry row throughput separately from raw WebSocket message volume.
pub(super) fn note_incoming_telemetry_rows(telemetry_rows: usize, telemetry_batch_count: usize) {
    if let Ok(mut next) = FRONTEND_NETWORK_METRICS_STATE.lock() {
        next.telemetry_rows_total = next
            .telemetry_rows_total
            .saturating_add(telemetry_rows as u64);
        next.telemetry_batches_total = next
            .telemetry_batches_total
            .saturating_add(telemetry_batch_count as u64);
        next.rows_since_last_sample = next
            .rows_since_last_sample
            .saturating_add(telemetry_rows as u64);
    }
}

/// Records frontend-observed HTTP request round-trip time for diagnostics.
pub(super) fn note_http_rtt_ms(rtt_ms: f64) {
    if !rtt_ms.is_finite() || rtt_ms < 0.0 {
        return;
    }
    if let Ok(mut next) = FRONTEND_NETWORK_METRICS_STATE.lock() {
        next.base_http = UrlConfig::base_http();
        next.http_rtt_ms = Some(rtt_ms);
        next.http_rtt_ema_ms = Some(match next.http_rtt_ema_ms {
            Some(prev) => prev * 0.82 + rtt_ms * 0.18,
            None => rtt_ms,
        });
    }
}
