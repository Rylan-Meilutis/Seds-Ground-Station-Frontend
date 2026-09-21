// HTTP seeding, notification synchronization, WebSocket transport, and JS bridges.

fn ws_activity_expired(elapsed_ms: f64) -> bool {
    elapsed_ms > 15_000.0
}

#[cfg(target_arch = "wasm32")]
struct WebSocketCleanup(web_sys::WebSocket);
#[cfg(target_arch = "wasm32")]
impl Drop for WebSocketCleanup {
    fn drop(&mut self) {
        self.0.set_onopen(None);
        self.0.set_onmessage(None);
        self.0.set_onerror(None);
        self.0.set_onclose(None);
        let _ = self.0.close();
    }
}

#[cfg(target_arch = "wasm32")]
struct HttpAbortOnDrop(web_sys::AbortController);
#[cfg(target_arch = "wasm32")]
impl Drop for HttpAbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[test]
fn websocket_idle_deadline_allows_jitter_but_recovers_stalls() {
    assert!(!ws_activity_expired(1000.0));
    assert!(!ws_activity_expired(15_000.0));
    assert!(ws_activity_expired(15_001.0));
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
    let controller =
        web_sys::AbortController::new().map_err(|_| "HTTP abort controller unavailable")?;
    let _abort_on_cancel = HttpAbortOnDrop(controller.clone());
    let signal = controller.signal();
    request = request.abort_signal(Some(&signal));
    let started_mono_ms = monotonic_now_ms();
    let fetch = async {
        let response = request.send().await.map_err(|e| e.to_string())?;
        let status = response.status();
        let body = response.text().await.map_err(|e| e.to_string())?;
        Ok::<_, String>((status, body))
    };
    futures_util::pin_mut!(fetch);
    let timeout_ms = if path == "/api/recent" || path.starts_with("/api/recordings/report?") {
        300_000
    } else {
        15_000
    };
    let (status, body) = match futures_util::future::select(
        fetch,
        gloo_timers::future::TimeoutFuture::new(timeout_ms),
    )
    .await
    {
        futures_util::future::Either::Left((result, _)) => result?,
        futures_util::future::Either::Right(_) => {
            controller.abort();
            return Err(format!("HTTP GET {path} timed out"));
        }
    };
    note_http_rtt_ms(monotonic_now_ms() - started_mono_ms);
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
    if path == "/api/recent" || path.starts_with("/api/recordings/report?") {
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

#[cfg(not(target_arch = "wasm32"))]
fn native_ws_connect_timeout() -> std::time::Duration {
    let secs = std::env::var("GS_WS_CONNECT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(8)
        .clamp(3, 30);
    std::time::Duration::from_secs(secs)
}

// Live rows use the client's receipt clock, while /api/recent uses the
// backend's wall clock. Align historical receipt times before merging or a
// Pi with a skewed clock loses its entire history at the first live packet.
async fn fetch_recent_rows_for_reseed() -> Result<Vec<TelemetryRow>, String> {
    #[derive(Deserialize)]
    struct HistoryClock { utc_ms: i64 }
    let before = current_wallclock_ms();
    let clock = http_get_json::<HistoryClock>("/api/system/time").await;
    let after = current_wallclock_ms();
    let mut rows = fetch_recent_rows_raw().await?;
    match clock {
        Ok(clock) => align_history_receipt_clock(&mut rows, clock.utc_ms,
            before.saturating_add(after.saturating_sub(before) / 2)),
        Err(error) => log!("[seed] backend clock unavailable; retaining original history timestamps: {error}"),
    }
    Ok(rows)
}

fn align_history_receipt_clock(rows: &mut [TelemetryRow], server_ms: i64, client_ms: i64) {
    let offset = client_ms.saturating_sub(server_ms);
    for row in rows {
        row.received_timestamp_ms = telemetry_row_received_ms(row).saturating_add(offset);
    }
}

#[cfg(test)]
mod history_clock_tests {
    use super::*;

    #[test]
    fn reload_keeps_ten_minutes_when_backend_clock_is_a_day_behind() {
        let server_now = 1_700_000_000_000i64;
        let client_now = server_now + 86_400_000;
        let mut rows: Vec<TelemetryRow> = (0..=600).map(|second| TelemetryRow {
            timestamp_ms: server_now - 600_000 + second * 1000,
            received_timestamp_ms: 0,
            data_type: "KG1000".into(), sender_id: "DAQ".into(),
            data_type_id: Default::default(), sender_id_id: Default::default(),
            values: vec![Some(second as f32)],
        }).collect();
        normalize_telemetry_rows_for_runtime(&mut rows);
        align_history_receipt_clock(&mut rows, server_now, client_now);
        // The same merge/prune used on reseed must not discard the prefix.
        let mut live = rows.last().unwrap().clone();
        live.received_timestamp_ms = client_now + 100;
        rows.push(live);
        let merged = compact_rows_for_ui(rows);
        assert_eq!(merged.len(), 602);
        assert_eq!(merged[0].received_timestamp_ms, client_now - 600_000);
        assert_eq!(merged[0].timestamp_ms, server_now - 600_000);
    }
}

#[cfg(target_arch = "wasm32")]
async fn fetch_recent_rows_raw() -> Result<Vec<TelemetryRow>, String> {
    let mut rows = http_get_json::<Vec<TelemetryRow>>("/api/recent").await?;
    normalize_telemetry_rows_for_runtime(&mut rows);
    Ok(rows)
}

#[cfg(not(target_arch = "wasm32"))]
async fn fetch_recent_rows_raw() -> Result<Vec<TelemetryRow>, String> {
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
        let mut rows = serde_json::from_str::<Vec<TelemetryRow>>(&body).map_err(|e| {
            let snippet: String = body.chars().take(200).collect();
            format!("invalid JSON ({e}): {}", snippet.trim())
        })?;
        normalize_telemetry_rows_for_runtime(&mut rows);
        return Ok(rows);
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
            let mut row = serde_json::from_str::<TelemetryRow>(trimmed).map_err(|e| {
                format!(
                    "invalid NDJSON row ({e}): {}",
                    trimmed.chars().take(200).collect::<String>()
                )
            })?;
            row.refresh_interned_ids();
            if row.received_timestamp_ms == 0 {
                row.received_timestamp_ms = row.timestamp_ms;
            }
            rows.push(row);
        }
    }
    let tail = String::from_utf8_lossy(&buffered);
    let trimmed = tail.trim();
    if !trimmed.is_empty() {
        let mut row = serde_json::from_str::<TelemetryRow>(trimmed).map_err(|e| {
            format!(
                "invalid NDJSON tail ({e}): {}",
                trimmed.chars().take(200).collect::<String>()
            )
        })?;
        row.refresh_interned_ids();
        if row.received_timestamp_ms == 0 {
            row.received_timestamp_ms = row.timestamp_ms;
        }
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

#[derive(Serialize)]
struct AlertAckRequest {
    warning_timestamp_ms: i64,
    error_timestamp_ms: i64,
}

async fn post_remote_alert_ack(
    warning_timestamp_ms: i64,
    error_timestamp_ms: i64,
) -> Result<(), String> {
    let _: AlertAckStateMsg = http_post_json(
        "/api/alerts/ack",
        &AlertAckRequest {
            warning_timestamp_ms,
            error_timestamp_ms,
        },
    )
    .await?;
    Ok(())
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

fn notification_identity(n: &PersistentNotification) -> DismissedNotification {
    DismissedNotification {
        id: n.id,
        timestamp_ms: n.timestamp_ms,
    }
}

#[cfg(test)]
mod notification_replay_tests {
    use super::*;

    #[test]
    fn snapshots_and_reload_do_not_replay_transient_notifications() {
        let mut notice = PersistentNotification {
            id: 42,
            timestamp_ms: 1000,
            message: "GSE state changed".into(),
            persistent: false,
            action_label: None,
            action_cmd: None,
        };
        let empty = HashSet::new();
        let seen = HashSet::from([notification_identity(&notice)]);
        assert!(notification_visible_on_snapshot(
            &notice, &empty, &empty, &empty
        ));
        // A repeat snapshot keeps an existing toast, but remount/reload does not replay it.
        assert!(notification_visible_on_snapshot(
            &notice, &seen, &empty, &seen
        ));
        assert!(!notification_visible_on_snapshot(
            &notice, &seen, &empty, &empty
        ));
        // A genuinely new event is not hidden just because the server reused its ID.
        notice.timestamp_ms += 1;
        assert!(notification_visible_on_snapshot(
            &notice, &seen, &empty, &empty
        ));
        notice.timestamp_ms -= 1;
        notice.persistent = true;
        assert!(notification_visible_on_snapshot(
            &notice, &seen, &empty, &empty
        ));
        assert!(!notification_visible_on_snapshot(
            &notice, &seen, &seen, &empty
        ));
    }

    #[test]
    fn repeated_history_snapshots_do_not_duplicate_entries() {
        let notice = PersistentNotification {
            id: 42,
            timestamp_ms: 1000,
            message: "GSE state changed".into(),
            persistent: false,
            action_label: None,
            action_cmd: None,
        };
        let mut history = Vec::new();
        merge_notification_history(&mut history, &[notice.clone(), notice.clone()]);
        merge_notification_history(&mut history, &[notice]);
        assert_eq!(history.len(), 1);
    }
}

fn notification_visible_on_snapshot(
    n: &PersistentNotification,
    seen: &HashSet<DismissedNotification>,
    dismissed: &HashSet<DismissedNotification>,
    visible: &HashSet<DismissedNotification>,
) -> bool {
    let identity = notification_identity(n);
    !dismissed.contains(&identity)
        && (n.persistent || !seen.contains(&identity) || visible.contains(&identity))
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

    // Snapshots restore state; they are not new events. Scope replay tracking to
    // the backend and include the timestamp because IDs restart with the server.
    let seen_key = format!("gs_notification_seen_v1:{}", UrlConfig::base_http());
    let seen: HashSet<DismissedNotification> = persist::get_string(&seen_key)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    let visible: HashSet<_> = notifications
        .read()
        .iter()
        .map(notification_identity)
        .collect();
    let dismissed: HashSet<_> = dismissed_notifications.read().iter().copied().collect();
    let mut unique = HashSet::new();
    let mut active: Vec<PersistentNotification> = incoming
        .iter()
        .filter(|n| unique.insert(notification_identity(n)))
        .filter(|n| notification_visible_on_snapshot(n, &seen, &dismissed, &visible))
        .cloned()
        .collect();
    let mut remembered = seen.clone();
    remembered.extend(incoming.iter().map(notification_identity));
    if remembered != seen {
        let mut remembered: Vec<_> = remembered.into_iter().collect();
        remembered.sort_by_key(|n| std::cmp::Reverse(n.timestamp_ms));
        remembered.truncate(MAX_NOTIFICATION_HISTORY);
        if let Ok(raw) = serde_json::to_string(&remembered) {
            persist::set_string(&seen_key, &raw);
        }
    }
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
    let active_ids: HashSet<u64> = active.iter().map(|n| n.id).collect();
    unread.retain(|id| active_ids.contains(id));
    for n in &active {
        if !prev_ids.contains(&n.id) && !seen.contains(&notification_identity(n)) {
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

fn apply_messages_snapshot(
    incoming: Vec<PersistentNotification>,
    message_history: Signal<Vec<PersistentNotification>>,
) {
    let mut message_history = message_history;
    let mut history = incoming;
    history.sort_by_key(|n| -n.timestamp_ms);
    if history.len() > MAX_MESSAGE_HISTORY {
        history.truncate(MAX_MESSAGE_HISTORY);
    }
    message_history.set(history);
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
    message_history: &mut Signal<Vec<PersistentNotification>>,
    dismissed_notifications: &mut Signal<Vec<DismissedNotification>>,
    unread_notification_ids: &mut Signal<Vec<u64>>,
    action_policy: &mut Signal<ActionPolicyMsg>,
    recording_status: &mut Signal<RecordingStatusMsg>,
    fill_targets: &mut Signal<Option<FillTargetsConfig>>,
    network_time: &mut Signal<Option<NetworkTimeSync>>,
    launch_clock: &mut Signal<Option<LaunchClockMsg>>,
    network_topology: &mut Signal<NetworkTopologyMsg>,
    board_status: &mut Signal<Vec<BoardStatusEntry>>,
    rocket_gps: &mut Signal<Option<(f64, f64)>>,
    rocket_gps_altitude_m: &mut Signal<Option<f64>>,
    _user_gps: &mut Signal<Option<(f64, f64)>>,
    _user_gps_altitude_m: &mut Signal<Option<f64>>,
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
    if let Ok(mut rows) = RESEED_HISTORY_BRIDGE.lock() {
        rows.clear();
    }
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

            // Capture rows that arrived while reseed was running and keep them.
            // Existing visible rows may be restored offline cache, so successful
            // /api/recent responses replace them instead of merging stale data.
            let mut live_rows = queue_snapshot();
            if !live_rows.is_empty() {
                sort_rows(&mut live_rows);
                prune_history(&mut live_rows);
                live_rows = compact_rows_for_ui(live_rows);
                log!("[seed] /api/recent merging queued_rows={}", live_rows.len());
                list = merge_db_and_live(list, live_rows);
            }

            rocket_gps.set(list.iter().rev().find_map(row_to_gps));
            rocket_gps_altitude_m.set(list.iter().rev().find_map(row_to_gps_altitude_m));

            // Build reseed cache in a double buffer while active cache keeps live updates.
            const RESEED_INGEST_CHUNK: usize = 1024;
            for chunk in list.chunks(RESEED_INGEST_CHUNK) {
                data_chart::charts_cache_reseed_ingest_rows(chunk);
                cooperative_yield().await;
            }

            // Replay queued rows into reseed cache as a second safety net.
            let post_reset_queued_rows = queue_snapshot();
            for chunk in post_reset_queued_rows.chunks(RESEED_INGEST_CHUNK) {
                data_chart::charts_cache_reseed_ingest_rows(chunk);
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
                for chunk in reseed_live_rows.chunks(RESEED_INGEST_CHUNK) {
                    data_chart::charts_cache_reseed_ingest_rows(chunk);
                }
                list.extend(reseed_live_rows);
                list = compact_rows_for_ui(list);
            }

            // Atomically swap the prepared reseed cache in. Empty /api/recent
            // means empty history and should clear stale offline data.
            charts_cache_finish_reseed_build();
            log!("[seed] applying reseed rows={}", list.len());
            if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
                store.replace_from_rows(&list);
            }
            reset_latest_telemetry(&list);
            rebuild_chart_cache_from_visible_rows();
            bump_render_epoch();
            persist_cached_telemetry_rows(&list);
            LAST_TELEMETRY_CACHE_PERSIST_MS.store(current_wallclock_ms(), Ordering::Relaxed);
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
        normalize_alert_list(&mut w);
        normalize_alert_list(&mut e);
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

    if let Ok(list) = http_get_json::<Vec<PersistentNotification>>("/api/messages").await
        && alive.load(Ordering::Relaxed)
    {
        apply_messages_snapshot(list, *message_history);
    }

    if let Ok(policy) = http_get_json::<ActionPolicyMsg>("/api/action_policy").await
        && alive.load(Ordering::Relaxed)
    {
        clear_command_feedback_latches();
        action_policy.set(policy);
    }

    if let Ok(targets) = http_get_json::<FillTargetsConfig>("/api/fill_targets").await
        && alive.load(Ordering::Relaxed)
    {
        fill_targets.set(Some(targets));
    }

    if let Ok(clock) = http_get_json::<LaunchClockMsg>("/api/launch_clock").await
        && alive.load(Ordering::Relaxed)
    {
        launch_clock.set(Some(clock));
    }

    if let Ok(status) = http_get_json::<BoardStatusMsg>("/api/boards").await
        && alive.load(Ordering::Relaxed)
    {
        board_status.set(status.boards);
    }

    if let Ok(topology) = http_get_json::<NetworkTopologyMsg>("/api/network_topology").await
        && alive.load(Ordering::Relaxed)
    {
        note_network_topology_received();
        network_topology.set(topology);
    }

    if let Ok(status) = http_get_json::<RecordingStatusMsg>("/api/recording_status").await
        && alive.load(Ordering::Relaxed)
    {
        recording_status.set(status);
    }

    if let Ok(nt) = http_get_json::<NetworkTimeMsg>("/api/network_time").await
        && alive.load(Ordering::Relaxed)
    {
        network_time.set(Some(NetworkTimeSync {
            network_ms: nt.timestamp_ms,
            received_mono_ms: monotonic_now_ms(),
        }));
    }

    if !alive.load(Ordering::Relaxed) {
        return Ok(());
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
    ack_warning_ts: Signal<i64>,
    ack_error_ts: Signal<i64>,
    remote_alert_acks_enabled: Signal<bool>,
    notifications: Signal<Vec<PersistentNotification>>,
    notification_history: Signal<Vec<PersistentNotification>>,
    message_history: Signal<Vec<PersistentNotification>>,
    dismissed_notifications: Signal<Vec<DismissedNotification>>,
    unread_notification_ids: Signal<Vec<u64>>,
    action_policy: Signal<ActionPolicyMsg>,
    recording_status: Signal<RecordingStatusMsg>,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    network_time: Signal<Option<NetworkTimeSync>>,
    launch_clock: Signal<Option<LaunchClockMsg>>,
    network_topology: Signal<NetworkTopologyMsg>,
    warning_event_counter: Signal<u64>,
    error_event_counter: Signal<u64>,
    flight_state: Signal<FlightState>,
    board_status: Signal<Vec<BoardStatusEntry>>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    rocket_gps_altitude_m: Signal<Option<f64>>,
    user_gps: Signal<Option<(f64, f64)>>,
    user_gps_altitude_m: Signal<Option<f64>>,
    layout_config: Signal<Option<LayoutConfig>>,
    layout_loading: Signal<bool>,
    layout_error: Signal<Option<String>>,
    layout_error_dismissed: Signal<Option<String>>,
    layout_request_base: Signal<String>,
    calibration_has_sensors: Signal<Option<bool>>,
    calibration_request_base: Signal<String>,
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
            }

            #[cfg(not(target_arch = "wasm32"))]
            {
                connect_ws_once_native(
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

        let reconnect_delay_ms = if dashboard_page_visible() { 250 } else { 1_500 };

        #[cfg(target_arch = "wasm32")]
        gloo_timers::future::TimeoutFuture::new(reconnect_delay_ms).await;

        #[cfg(not(target_arch = "wasm32"))]
        tokio::time::sleep(std::time::Duration::from_millis(reconnect_delay_ms as u64)).await;
    }

    Ok(())
}

#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
async fn connect_ws_once_wasm(
    epoch: u64,
    _warnings: Signal<Vec<AlertMsg>>,
    _errors: Signal<Vec<AlertMsg>>,
    _ack_warning_ts: Signal<i64>,
    _ack_error_ts: Signal<i64>,
    _remote_alert_acks_enabled: Signal<bool>,
    _notifications: Signal<Vec<PersistentNotification>>,
    _notification_history: Signal<Vec<PersistentNotification>>,
    _message_history: Signal<Vec<PersistentNotification>>,
    _dismissed_notifications: Signal<Vec<DismissedNotification>>,
    _unread_notification_ids: Signal<Vec<u64>>,
    _action_policy: Signal<ActionPolicyMsg>,
    _recording_status: Signal<RecordingStatusMsg>,
    _fill_targets: Signal<Option<FillTargetsConfig>>,
    _network_time: Signal<Option<NetworkTimeSync>>,
    _launch_clock: Signal<Option<LaunchClockMsg>>,
    _network_topology: Signal<NetworkTopologyMsg>,
    _warning_event_counter: Signal<u64>,
    _error_event_counter: Signal<u64>,
    _flight_state: Signal<FlightState>,
    _board_status: Signal<Vec<BoardStatusEntry>>,
    _rocket_gps: Signal<Option<(f64, f64)>>,
    _rocket_gps_altitude_m: Signal<Option<f64>>,
    _user_gps: Signal<Option<(f64, f64)>>,
    _user_gps_altitude_m: Signal<Option<f64>>,
    _layout_config: Signal<Option<LayoutConfig>>,
    _layout_loading: Signal<bool>,
    _layout_error: Signal<Option<String>>,
    _layout_error_dismissed: Signal<Option<String>>,
    _layout_request_base: Signal<String>,
    _calibration_has_sensors: Signal<Option<bool>>,
    _calibration_request_base: Signal<String>,
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
    let last_activity_ms = std::rc::Rc::new(std::cell::Cell::new(monotonic_now_ms()));

    *WS_RAW.write() = Some(ws.clone());
    *WS_SENDER.write() = Some(WsSender { ws: ws.clone() });

    let (closed_tx, closed_rx) = oneshot::channel::<()>();
    let closed_tx = std::rc::Rc::new(std::cell::RefCell::new(Some(closed_tx)));
    // Own callbacks for this connection, then detach and drop them on teardown.
    let onopen;
    let onmessage;
    let onerror;
    let onclose;

    {
        let last_activity_ms = last_activity_ms.clone();
        let ws_url_for_open = ws_url.clone();
        onopen = Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
            last_activity_ms.set(monotonic_now_ms());
            log!("[WS] open");
            queue_ws_open_event(epoch, ws_url_for_open.clone());
        });
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    }

    {
        let alive_for_message = alive.clone();
        let last_activity_ms = last_activity_ms.clone();
        onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if !alive_for_message.load(Ordering::Relaxed) || *WS_EPOCH.read() != epoch {
                return;
            }
            last_activity_ms.set(monotonic_now_ms());
            if let Some(s) = e.data().as_string() {
                if !queue_live_telemetry_from_ws_message(&s) {
                    queue_ws_message_event(epoch, s);
                }
            }
        });
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    }

    {
        let closed_tx = closed_tx.clone();
        let alive_for_error = alive.clone();
        onerror = Closure::<dyn FnMut(ErrorEvent)>::new(move |e: ErrorEvent| {
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
    }

    {
        let closed_tx = closed_tx.clone();
        let alive_for_close = alive.clone();
        onclose = Closure::<dyn FnMut(CloseEvent)>::new(move |e: CloseEvent| {
            if !alive_for_close.load(Ordering::Relaxed) {
                return;
            }
            log!("[WS] close code={} reason='{}'", e.code(), e.reason());
            if let Some(tx) = closed_tx.borrow_mut().take() {
                let _ = tx.send(());
            }
        });
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
    }

    // Declared after callbacks: cancellation detaches JS handlers before Rust
    // closures are dropped, including component unmount and epoch replacement.
    let _socket_cleanup = WebSocketCleanup(ws.clone());
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
            gloo_timers::future::TimeoutFuture::new(2_000),
        )
        .await;

        match done {
            futures_util::future::Either::Left((_closed, _timeout)) => break,
            futures_util::future::Either::Right((_timeout, _closed)) => {
                let ready_state = ws.ready_state();
                // The backend emits network time every second even with no
                // telemetry. OPEN alone does not mean the connection is alive.
                if ws_activity_expired(monotonic_now_ms() - last_activity_ms.get()) {
                    log!("[WS] activity timeout; reconnecting and reseeding");
                    break;
                }
                if ready_state == WebSocket::CLOSING || ready_state == WebSocket::CLOSED {
                    log!("[WS] websocket ready_state transitioned to {}", ready_state);
                    let _ = ws.close();
                    break;
                }
            }
        }
    }

    ws.set_onopen(None);
    ws.set_onmessage(None);
    ws.set_onerror(None);
    ws.set_onclose(None);
    let _ = ws.close();
    drop((onopen, onmessage, onerror, onclose));
    if *WS_EPOCH.read() == epoch {
        note_ws_connection_state(
            false,
            ws_url,
            Some("websocket closed or stalled".to_string()),
            epoch,
        );
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
    ack_warning_ts: Signal<i64>,
    ack_error_ts: Signal<i64>,
    remote_alert_acks_enabled: Signal<bool>,
    mut notifications: Signal<Vec<PersistentNotification>>,
    mut notification_history: Signal<Vec<PersistentNotification>>,
    message_history: Signal<Vec<PersistentNotification>>,
    dismissed_notifications: Signal<Vec<DismissedNotification>>,
    mut unread_notification_ids: Signal<Vec<u64>>,
    action_policy: Signal<ActionPolicyMsg>,
    recording_status: Signal<RecordingStatusMsg>,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    network_time: Signal<Option<NetworkTimeSync>>,
    launch_clock: Signal<Option<LaunchClockMsg>>,
    network_topology: Signal<NetworkTopologyMsg>,
    warning_event_counter: Signal<u64>,
    error_event_counter: Signal<u64>,
    flight_state: Signal<FlightState>,
    board_status: Signal<Vec<BoardStatusEntry>>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    rocket_gps_altitude_m: Signal<Option<f64>>,
    user_gps: Signal<Option<(f64, f64)>>,
    user_gps_altitude_m: Signal<Option<f64>>,
    layout_config: Signal<Option<LayoutConfig>>,
    layout_loading: Signal<bool>,
    layout_error: Signal<Option<String>>,
    layout_error_dismissed: Signal<Option<String>>,
    layout_request_base: Signal<String>,
    calibration_has_sensors: Signal<Option<bool>>,
    calibration_request_base: Signal<String>,
    alive: Arc<AtomicBool>,
) -> Result<(), String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio::time::{Duration, timeout};
    use tokio_tungstenite::tungstenite::Message;

    if !alive.load(Ordering::Relaxed) {
        return Ok(());
    }
    if *WS_EPOCH.read() != epoch {
        return Ok(());
    }

    let base_ws = UrlConfig::base_ws();
    let ws_url = auth_ws_url(&base_ws);

    log!("[WS] connecting to {ws_url} (epoch={epoch})");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    *WS_SENDER.write() = Some(WsSender { tx });
    let connect_timeout = native_ws_connect_timeout();
    let connect_result = timeout(connect_timeout, async {
        if UrlConfig::_skip_tls_verify() && ws_url.starts_with("wss://") {
            let tls = insecure_rustls_connector()
                .map_err(|e| format!("[WS] rustls connector build failed: {e}"))?;
            tokio_tungstenite::connect_async_tls_with_config(
                ws_url.as_str(),
                None,
                false,
                Some(tls),
            )
            .await
            .map_err(|e| format!("[WS] connect failed: {e}"))
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
                .map_err(|e| format!("[WS] connect failed: {e}"))
            }
            #[cfg(not(any(target_os = "android", target_os = "ios", target_os = "macos")))]
            {
                tokio_tungstenite::connect_async(ws_url.as_str())
                    .await
                    .map_err(|e| format!("[WS] connect failed: {e}"))
            }
        } else {
            tokio_tungstenite::connect_async(ws_url.as_str())
                .await
                .map_err(|e| format!("[WS] connect failed: {e}"))
        }
    })
    .await;

    let mut ws_stream = match connect_result {
        Ok(Ok((stream, _response))) => stream,
        Ok(Err(err)) => {
            if *WS_EPOCH.read() == epoch {
                *WS_SENDER.write() = None;
            }
            return Err(err);
        }
        Err(_) => {
            if *WS_EPOCH.read() == epoch {
                *WS_SENDER.write() = None;
            }
            return Err(format!(
                "[WS] connect timed out after {}s",
                connect_timeout.as_secs()
            ));
        }
    };

    note_ws_connected_and_restore_data_flow(
        ws_url.clone(),
        epoch,
        &mut notifications,
        &mut notification_history,
        &mut unread_notification_ids,
    );
    refresh_flight_state_after_ws_reconnect(flight_state, epoch);
    refresh_layout_after_ws_reconnect(
        layout_config,
        layout_loading,
        layout_error,
        layout_error_dismissed,
        layout_request_base,
        calibration_has_sensors,
        calibration_request_base,
        action_policy,
    );

    enum NativeWsEvent {
        Incoming(
            Option<
                Result<
                    tokio_tungstenite::tungstenite::Message,
                    tokio_tungstenite::tungstenite::Error,
                >,
            >,
        ),
        Outgoing(Option<String>),
    }

    let mut last_activity = std::time::Instant::now();
    while alive.load(Ordering::Relaxed) && *WS_EPOCH.read() == epoch {
        if ws_activity_expired(last_activity.elapsed().as_secs_f64() * 1000.0) {
            log!("[WS] activity timeout; reconnecting and reseeding");
            break;
        }
        let event = timeout(Duration::from_millis(250), async {
            tokio::select! {
                outgoing = rx.recv() => NativeWsEvent::Outgoing(outgoing),
                incoming = ws_stream.next() => NativeWsEvent::Incoming(incoming),
            }
        })
        .await;

        let Ok(next_event) = event else {
            continue;
        };
        if matches!(&next_event, NativeWsEvent::Incoming(Some(Ok(_)))) {
            last_activity = std::time::Instant::now();
        }

        match next_event {
            NativeWsEvent::Outgoing(Some(msg)) => {
                if !matches!(
                    timeout(
                        Duration::from_secs(5),
                        ws_stream.send(Message::Text(msg.into()))
                    )
                    .await,
                    Ok(Ok(()))
                ) {
                    log!("[WS] write failed or timed out; command will not be replayed");
                    break;
                }
            }
            NativeWsEvent::Outgoing(None) => break,
            NativeWsEvent::Incoming(Some(Ok(Message::Text(s)))) => {
                handle_ws_message(
                    &s,
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
                );
            }
            NativeWsEvent::Incoming(Some(Ok(Message::Ping(payload)))) => {
                if !matches!(
                    timeout(
                        Duration::from_secs(5),
                        ws_stream.send(Message::Pong(payload))
                    )
                    .await,
                    Ok(Ok(()))
                ) {
                    log!("[WS] pong failed or timed out");
                    break;
                }
            }
            NativeWsEvent::Incoming(Some(Ok(Message::Close(_)))) => break,
            NativeWsEvent::Incoming(Some(Ok(_))) => {}
            NativeWsEvent::Incoming(Some(Err(e))) => {
                log!("[WS] read error: {e}");
                break;
            }
            NativeWsEvent::Incoming(None) => break,
        }
    }

    // Only clear sender if this task still owns the active epoch.
    // Prevents old-epoch teardown from clobbering a freshly reconnected sender.
    if *WS_EPOCH.read() == epoch {
        note_ws_connection_state(false, ws_url, Some("websocket closed".to_string()), epoch);
        *WS_SENDER.write() = None;
    }

    Err("websocket closed".to_string())
}

#[allow(clippy::too_many_arguments)]
fn try_handle_ws_telemetry_message_fast(
    s: &str,
    notifications: Signal<Vec<PersistentNotification>>,
    notification_history: Signal<Vec<PersistentNotification>>,
    unread_notification_ids: Signal<Vec<u64>>,
) -> bool {
    let Ok(msg) = serde_json::from_str::<WsTelemetryIngressMsg>(s) else {
        return false;
    };

    note_ws_connected_from_live_message(
        *WS_EPOCH.read(),
        notifications,
        notification_history,
        unread_notification_ids,
    );
    LAST_WS_ACTIVITY_MONO_MS.store(monotonic_now_ms() as i64, Ordering::Relaxed);
    note_incoming_ws_message(s.len());

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

            if page_visible {
                mark_live_dashboard_data_dirty();
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

            if page_visible {
                mark_live_dashboard_data_dirty();
            }
        }
    }

    true
}

#[allow(clippy::too_many_arguments)]
fn handle_ws_message(
    s: &str,
    warnings: Signal<Vec<AlertMsg>>,
    errors: Signal<Vec<AlertMsg>>,
    ack_warning_ts: Signal<i64>,
    ack_error_ts: Signal<i64>,
    remote_alert_acks_enabled: Signal<bool>,
    notifications: Signal<Vec<PersistentNotification>>,
    notification_history: Signal<Vec<PersistentNotification>>,
    message_history: Signal<Vec<PersistentNotification>>,
    dismissed_notifications: Signal<Vec<DismissedNotification>>,
    unread_notification_ids: Signal<Vec<u64>>,
    action_policy: Signal<ActionPolicyMsg>,
    recording_status: Signal<RecordingStatusMsg>,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    network_time: Signal<Option<NetworkTimeSync>>,
    launch_clock: Signal<Option<LaunchClockMsg>>,
    network_topology: Signal<NetworkTopologyMsg>,
    warning_event_counter: Signal<u64>,
    error_event_counter: Signal<u64>,
    flight_state: Signal<FlightState>,
    board_status: Signal<Vec<BoardStatusEntry>>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    rocket_gps_altitude_m: Signal<Option<f64>>,
    user_gps: Signal<Option<(f64, f64)>>,
    _user_gps_altitude_m: Signal<Option<f64>>,
) {
    if try_handle_ws_telemetry_message_fast(
        s,
        notifications,
        notification_history,
        unread_notification_ids,
    ) {
        return;
    }

    note_ws_connected_from_live_message(
        *WS_EPOCH.read(),
        notifications,
        notification_history,
        unread_notification_ids,
    );
    LAST_WS_ACTIVITY_MONO_MS.store(monotonic_now_ms() as i64, Ordering::Relaxed);
    let mut warnings = warnings;
    let mut errors = errors;
    let mut ack_warning_ts = ack_warning_ts;
    let mut ack_error_ts = ack_error_ts;
    let mut warning_event_counter = warning_event_counter;
    let mut error_event_counter = error_event_counter;
    let remote_alert_acks_enabled = remote_alert_acks_enabled;
    let notifications = notifications;
    let notification_history = notification_history;
    let message_history = message_history;
    let dismissed_notifications = dismissed_notifications;
    let unread_notification_ids = unread_notification_ids;
    let mut action_policy = action_policy;
    let mut recording_status = recording_status;
    let mut fill_targets = fill_targets;
    let mut network_time = network_time;
    let mut launch_clock = launch_clock;
    let mut network_topology = network_topology;
    let mut flight_state = flight_state;
    let mut board_status = board_status;
    let _rocket_gps = rocket_gps;
    let _rocket_gps_altitude_m = rocket_gps_altitude_m;
    let _user_gps = user_gps;

    let Ok(msg) = serde_json::from_str::<WsInMsg>(s) else {
        return;
    };
    note_incoming_ws_message(s.len());
    let page_visible = dashboard_page_visible();
    let now_ms = current_wallclock_ms();

    match msg {
        WsInMsg::Telemetry(row) => {
            let Some(row) = normalize_live_telemetry_row_for_client_clock(row, now_ms) else {
                return;
            };
            note_incoming_telemetry_rows(1, 0);
            if RESEED_IN_PROGRESS.load(Ordering::Relaxed)
                && let Ok(mut v) = RESEED_LIVE_BUFFER.lock()
            {
                v.push(row.clone());
            }

            // Queue telemetry for UI batch flush
            if let Ok(mut q) = TELEMETRY_QUEUE.lock() {
                q.push_back(row);
                trim_telemetry_queue_to_live_window(&mut q);
            }

            if page_visible {
                mark_live_dashboard_data_dirty();
            }
        }

        WsInMsg::TelemetryBatch(batch) => {
            if batch.is_empty() {
                return;
            }
            let batch: Vec<TelemetryRow> = batch
                .into_iter()
                .filter_map(|row| normalize_live_telemetry_row_for_client_clock(row, now_ms))
                .collect();
            if batch.is_empty() {
                return;
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

            if page_visible {
                mark_live_dashboard_data_dirty();
            }
        }

        WsInMsg::FlightState(st) => {
            set_signal_if_changed(&mut flight_state, st.state);
        }

        WsInMsg::LaunchClock(clock) => {
            set_signal_if_changed(&mut launch_clock, Some(clock));
        }

        WsInMsg::Warning(w) => {
            let mut v = { warnings.read().clone() };
            if push_alert_deduped(&mut v, w) {
                warnings.set(v);
                let next = {
                    let current = *warning_event_counter.read();
                    current.saturating_add(1)
                };
                warning_event_counter.set(next);
            }
        }

        WsInMsg::Error(e) => {
            let mut v = { errors.read().clone() };
            if push_alert_deduped(&mut v, e) {
                errors.set(v);
                let next = {
                    let current = *error_event_counter.read();
                    current.saturating_add(1)
                };
                error_event_counter.set(next);
            }
        }

        WsInMsg::AlertAckState(ack_state) => {
            if *remote_alert_acks_enabled.read() {
                let next_warning_ack =
                    (*ack_warning_ts.read()).max(ack_state.warning_ack_timestamp_ms);
                if next_warning_ack != *ack_warning_ts.read() {
                    ack_warning_ts.set(next_warning_ack);
                }
                let next_error_ack = (*ack_error_ts.read()).max(ack_state.error_ack_timestamp_ms);
                if next_error_ack != *ack_error_ts.read() {
                    ack_error_ts.set(next_error_ack);
                }
            }
        }

        WsInMsg::BoardStatus(status) => {
            set_signal_if_changed(&mut board_status, status.boards);
        }

        WsInMsg::NetworkTopology(topology) => {
            note_network_topology_received();
            set_signal_if_changed(&mut network_topology, topology);
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

        WsInMsg::Messages(list) => {
            apply_messages_snapshot(list, message_history);
        }

        WsInMsg::ActionPolicy(policy) => {
            clear_command_feedback_latches();
            set_signal_if_changed(&mut action_policy, policy);
        }

        WsInMsg::FillTargets(targets) => {
            set_signal_if_changed(&mut fill_targets, Some(targets));
        }

        WsInMsg::RecordingStatus(status) => {
            set_signal_if_changed(&mut recording_status, status);
        }

        WsInMsg::NetworkTime(t) => {
            let next = NetworkTimeSync {
                network_ms: t.timestamp_ms,
                received_mono_ms: monotonic_now_ms(),
            };
            let changed = {
                let current = network_time.read();
                current
                    .as_ref()
                    .is_none_or(|value| value.network_ms != next.network_ms)
            };
            if changed {
                network_time.set(Some(next));
            }
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

#[cfg(target_arch = "wasm32")]
fn js_is_ground_map_ready() -> bool {
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
