// Dashboard tab selection, backend URL configuration, reconnect, and reset state.

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
        MainTab::FirmwareUpdate => "firmware-update",
        MainTab::Calibration => "calibration",
        MainTab::Mission => "mission",
        MainTab::CrewVoice => "crew-voice",
        MainTab::StreamManager => "stream-manager",
        MainTab::Media => "media",
        MainTab::Vehicle => "vehicle",
        MainTab::Messages => "messages",
        MainTab::Notifications => "notifications",
        MainTab::Warnings => "warnings",
        MainTab::Errors => "errors",
        MainTab::Data => "data",
        MainTab::DataExport => "data-export",
    }
}

/// Returns the default label for a dashboard tab when the layout config does not override it.
fn _default_main_tab_label(tab: MainTab) -> String {
    let lang = current_language();
    match tab {
        MainTab::State => localized_copy(&lang, "Dashboard", "Panel", "Tableau"),
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
        MainTab::FirmwareUpdate => localized_copy(
            &lang,
            "Firmware Update",
            "Actualizacion de Firmware",
            "Mise a jour du micrologiciel",
        ),
        MainTab::Calibration => localized_copy(&lang, "Calibration", "Calibracion", "Calibration"),
        MainTab::Mission => {
            localized_copy(&lang, "Mission Live", "Mision en vivo", "Mission en direct")
        }
        MainTab::StreamManager => localized_copy(&lang, "Stream Manager", "Gestor de transmisión", "Gestion du direct"),
        MainTab::CrewVoice => localized_copy(&lang, "Crew Voice", "Voz de tripulación", "Voix équipage"),
        MainTab::Media => localized_copy(&lang, "Cameras & Recordings", "Cámaras y grabaciones", "Caméras et enregistrements"),
        MainTab::Vehicle => localized_copy(&lang, "Vehicle", "Vehiculo", "Vehicule"),
        MainTab::Messages => localized_copy(&lang, "Messages", "Mensajes", "Messages"),
        MainTab::Notifications => {
            localized_copy(&lang, "Notifications", "Notificaciones", "Notifications")
        }
        MainTab::Warnings => localized_copy(&lang, "Warnings", "Avisos", "Alertes"),
        MainTab::Errors => localized_copy(&lang, "Errors", "Errores", "Erreurs"),
        MainTab::Data => localized_copy(&lang, "Data", "Datos", "Donnees"),
        MainTab::DataExport => localized_copy(&lang, "Data Export", "Exportar datos", "Export de donnees"),
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
        "firmware-update" => MainTab::FirmwareUpdate,
        "calibration" => MainTab::Calibration,
        "mission" | "live-stream" => MainTab::Mission,
        "crew-voice" => MainTab::CrewVoice,
        "stream-manager" => MainTab::StreamManager,
        "my-dashboard" => MainTab::State,
        "media" => MainTab::Media,
        "vehicle" => MainTab::Vehicle,
        "messages" => MainTab::Messages,
        "notifications" => MainTab::Notifications,
        "warnings" => MainTab::Warnings,
        "errors" => MainTab::Errors,
        "data" => MainTab::Data,
        "data-export" => MainTab::DataExport,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
struct DashboardCustomization {
    #[serde(default)]
    order: Vec<String>,
    #[serde(default)]
    hidden: Vec<String>,
}

fn dashboard_customization_key() -> String {
    let suffix: String = UrlConfig::base_http()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    format!("gs26_dashboard_customization_v1_{suffix}")
}

fn ground_station_view_key() -> String {
    format!("{}_ground_station_view", dashboard_customization_key())
}

fn streamer_mode_key() -> String {
    format!("{}_streamer", dashboard_customization_key())
}

fn scoped_main_tab_key() -> String {
    format!("{}_active_tab", dashboard_customization_key())
}

fn personal_dashboard_customization_key() -> String {
    let user = auth::current_status().username.unwrap_or_else(|| "anonymous".into());
    format!("{}_user_{}", dashboard_customization_key(), user)
}

fn load_dashboard_customization() -> DashboardCustomization {
    persist::get_string(&personal_dashboard_customization_key())
        .or_else(|| persist::get_string(&dashboard_customization_key()))
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_dashboard_customization(value: &DashboardCustomization) {
    if let Ok(raw) = serde_json::to_string(value) {
        persist::set_string(&personal_dashboard_customization_key(), &raw);
    }
}

fn _available_main_tabs(
    layout: &LayoutConfig,
    abort_only_mode: bool,
    calibration_has_sensors: Option<bool>,
) -> Vec<MainTab> {
    let mut tabs = Vec::new();
    for id in &layout.main_tabs {
        let tab = _main_tab_from_str(id);
        if tab == MainTab::Vehicle || !_layout_main_tab_enabled(layout, tab) || tabs.contains(&tab)
        {
            continue;
        }
        if tab == MainTab::Actions && !_actions_tab_has_visible_actions(layout, abort_only_mode) {
            continue;
        }
        if tab == MainTab::FirmwareUpdate && !auth::can_send_command("FirmwareUpdate") {
            continue;
        }
        if tab == MainTab::Calibration && !_calibration_tab_visible(calibration_has_sensors) {
            continue;
        }
        tabs.push(tab);
    }
    // These views degrade gracefully when their optional backend endpoints are
    // absent, so older Ground Station layouts gain the new mission tools without
    // requiring an immediate config migration.
    if !tabs.contains(&MainTab::Mission) {
        tabs.insert(0, MainTab::Mission);
    }
    for tab in [MainTab::CrewVoice, MainTab::Media] {
        if !tabs.contains(&tab) { tabs.push(tab); }
    }
    tabs.retain(|tab| *tab != MainTab::State);
    if auth::can_manage_stream() && !tabs.contains(&MainTab::StreamManager) { tabs.push(MainTab::StreamManager); }
    tabs.retain(|tab| *tab != MainTab::StreamManager || auth::can_manage_stream());
    // Older layouts must expose recording export without a server config migration.
    if !tabs.contains(&MainTab::DataExport) {
        tabs.push(MainTab::DataExport);
    }
    tabs.insert(0, MainTab::State);
    tabs
}

fn open_dashboard_tool(tab: MainTab, mut active: Signal<MainTab>, mut customization: Signal<DashboardCustomization>) {
    let mut next = customization.read().clone();
    let id = _main_tab_to_str(tab);
    if next.hidden.iter().any(|hidden| hidden == id) {
        next.hidden.retain(|hidden| hidden != id);
        save_dashboard_customization(&next);
        customization.set(next);
    }
    active.set(tab);
}

fn _ordered_available_main_tabs(layout: &LayoutConfig, abort_only_mode: bool, calibration: Option<bool>, customization: &DashboardCustomization) -> Vec<MainTab> {
    let mut tabs = _available_main_tabs(layout, abort_only_mode, calibration);
    tabs.sort_by_key(|tab| (*tab != MainTab::State, customization.order.iter().position(|id| id == _main_tab_to_str(*tab)).unwrap_or(usize::MAX)));
    tabs
}

/// Computes the final visible tab list after applying layout and auth filtering.
fn _configured_main_tabs(
    layout: &LayoutConfig,
    abort_only_mode: bool,
    calibration_has_sensors: Option<bool>,
    customization: &DashboardCustomization,
) -> Vec<MainTab> {
    let available = _available_main_tabs(layout, abort_only_mode, calibration_has_sensors);
    let mut tabs = Vec::new();
    for id in &customization.order {
        let tab = _main_tab_from_str(id);
        if available.contains(&tab)
            && !tabs.contains(&tab)
            && !customization
                .hidden
                .iter()
                .any(|hidden| hidden == _main_tab_to_str(tab))
        {
            tabs.push(tab);
        }
    }
    for tab in available {
        if !tabs.contains(&tab)
            && !customization
                .hidden
                .iter()
                .any(|hidden| hidden == _main_tab_to_str(tab))
        {
            tabs.push(tab);
        }
    }
    if !customization.hidden.iter().any(|id| id == "messages")
        && !tabs.contains(&MainTab::Messages)
        && let Some(notifications_idx) = tabs.iter().position(|tab| *tab == MainTab::Notifications)
    {
        tabs.insert(notifications_idx, MainTab::Messages);
    }
    // The primary dashboard stays reachable regardless of old tab customization.
    tabs.retain(|tab| *tab != MainTab::State);
    tabs.insert(0, MainTab::State);
    tabs
}

// ---------- Base URL config ----------
pub struct UrlConfig;

impl UrlConfig {
    /// Normalizes and persists the backend base URL selected by the operator.
    pub fn set_base_url_and_persist(url: String) {
        let clean = normalize_base_url(url);
        let previous = normalize_base_url(UrlConfig::base_http());
        *BASE_URL.write() = clean.clone();
        persist::set_string(BASE_URL_STORAGE_KEY, &clean);
        if !previous.trim().is_empty() && previous != clean {
            clear_frontend_caches();
        }
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
        if !clean.starts_with("https://") {
            let key = _tls_skip_key(&clean);
            persist::set_string(&key, "false");
            return;
        }
        let key = _tls_skip_key(&clean);
        persist::set_string(&key, if value { "true" } else { "false" });
    }

    /// Returns whether TLS validation is disabled for a specific backend base URL.
    pub fn _skip_tls_verify_for_base(base: &str) -> bool {
        let clean = normalize_base_url(base.to_string());
        if clean.is_empty() || !clean.starts_with("https://") {
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
static LAST_WS_ACTIVITY_MONO_MS: AtomicI64 = AtomicI64::new(0);
static LAST_TOPOLOGY_ACTIVITY_MONO_MS: AtomicI64 = AtomicI64::new(0);

/// Restarts the WebSocket connection and triggers a fresh telemetry reseed.
fn reconnect_and_reload_ui() {
    note_local_ws_disconnect("frontend requested reconnect");

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

fn clear_visible_telemetry_history_preserving_bridge() {
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
    clear_visible_telemetry_history_without_bridge();
}

fn clear_visible_telemetry_history_without_bridge() {
    if let Ok(mut bridge) = RESEED_HISTORY_BRIDGE.lock() {
        bridge.clear();
    }
    if let Ok(mut live) = RESEED_LIVE_BUFFER.lock() {
        live.clear();
    }
    if let Ok(mut store) = UI_TELEMETRY_STORE.lock() {
        store.replace_from_rows(&[]);
    }
    persist::_remove(TELEMETRY_CACHE_STORAGE_KEY);
    LAST_TELEMETRY_CACHE_PERSIST_MS.store(0, Ordering::Relaxed);
    reset_latest_telemetry(&[]);
    clear_map_rocket_marker();
    charts_cache_clear_active();
    bump_render_epoch();
}

fn clear_map_rocket_marker() {
    js_eval(
        r#"
        (function() {
          try {
            window.__gs26_pending_r_lat = NaN;
            window.__gs26_pending_r_lon = NaN;
            if (typeof window.updateGroundMapMarkers === "function") {
              window.updateGroundMapMarkers(
                NaN,
                NaN,
                window.__gs26_user_lat,
                window.__gs26_user_lon
              );
            }
          } catch (e) {}
        })();
        "#,
    );
}

pub fn hard_reload_dashboard_data() {
    clear_telemetry_runtime_buffers();
    clear_visible_telemetry_history_preserving_bridge();
    set_reseed_status_running();
    #[cfg(not(target_arch = "wasm32"))]
    charts_cache_request_refit();
    bump_seed_epoch();
    if WS_SENDER.read().is_none() {
        bump_ws_epoch();
    }
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

#[cfg(test)]
mod dashboard_customization_tests {
    use super::*;

    #[test]
    fn media_tabs_roundtrip_and_customization_hides_optional_tools() {
        let layout: LayoutConfig = serde_json::from_str(include_str!("../../docs/api-examples/layout.minimal.json")).unwrap();
        for tab in [MainTab::CrewVoice, MainTab::Media, MainTab::StreamManager] {
            assert!(_main_tab_from_str(_main_tab_to_str(tab)) == tab);
        }
        let customization = DashboardCustomization {
            order: vec!["my-dashboard".into(), "mission".into()],
            hidden: vec!["crew-voice".into(), "media".into(), "state".into()],
        };
        let tabs = _configured_main_tabs(&layout, false, None, &customization);
        assert!(tabs[0] == MainTab::State);
        assert!(tabs[1] == MainTab::Mission);
        assert!(!tabs.contains(&MainTab::CrewVoice));
        assert!(!tabs.contains(&MainTab::Media));
        let editor_tabs = _ordered_available_main_tabs(&layout, false, None, &customization);
        assert!(editor_tabs[0] == MainTab::State);
        assert!(editor_tabs[1] == MainTab::Mission);
        assert!(editor_tabs.contains(&MainTab::Media));
    }
}
