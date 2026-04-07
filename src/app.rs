// frontend/src/app.rs
//
const _CONNECTION_TIMEOUT_MS: u64 = 8000;
const _BODY_TRANSFER_TIMEOUT_MS: u64 = 10000;
const _WS_TIMEOUT_MS: u64 = 4500;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const APP_DISPLAY_NAME: &str = "UBSEDS GS";

use crate::auth::{self, SessionStatus as AuthSessionStatus};
use crate::telemetry_dashboard::layout::ThemeConfig;
use dioxus::prelude::*;
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
use dioxus_desktop::use_window;
use dioxus_router::{Routable, Router, use_navigator};

#[allow(unused_imports)]
use crate::telemetry_dashboard::{self, UrlConfig};

const INLINE_LEAFLET_CSS: &str = include_str!("../static/vendor/leaflet/leaflet.css");
const INLINE_LEAFLET_JS: &str = include_str!("../static/vendor/leaflet/leaflet.js");
const INLINE_GROUND_MAP_JS: &str = include_str!("../static/ground_map.js");

// -------------------------
// Native-only keep-awake shims (mobile)
// -------------------------
#[cfg(not(target_arch = "wasm32"))]
mod keep_awake {
    #[cfg(target_os = "ios")]
    mod ios {
        use std::os::raw::c_int;

        unsafe extern "C" {
            fn gs26_set_idle_timer_disabled(disabled: c_int);
        }

        /// Toggles the iOS idle-timer suppression used while the app is active.
        pub fn set_enabled(enabled: bool) {
            // iOS API is "idle timer disabled", so enabled=true -> disabled=1
            unsafe { gs26_set_idle_timer_disabled(if enabled { 1 } else { 0 }) };
        }
    }

    #[cfg(target_os = "android")]
    mod android {
        /// Forwards the keep-awake request into the Android glue code.
        pub fn set_enabled(enabled: bool) {
            crate::telemetry_dashboard::gps_android::set_keep_screen_on(enabled);
        }
    }

    /// Enables or disables keep-awake on native platforms that support it.
    pub fn set_enabled(enabled: bool) {
        #[cfg(target_os = "ios")]
        ios::set_enabled(enabled);

        #[cfg(target_os = "android")]
        android::set_enabled(enabled);

        // Other native targets: no-op
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            let _ = enabled;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// Returns whether every HTTP probe and the WebSocket handshake succeeded.
fn all_tests_passed(checks: &[RouteCheck], ws_probe: &WsProbeStatus) -> bool {
    let routes_ok = checks.iter().all(|c| c.ok);
    let ws_ok = ws_probe.ok;
    routes_ok && ws_ok
}

// --- global css ---
const GLOBAL_CSS: &str = r#"
:root {
    --gs26-app-height: 100dvh;
    --gs26-app-background: #020617;
    --gs26-app-text: #e5e7eb;
    --gs26-panel-background: #0b1220;
    --gs26-panel-alt-background: #0f172a;
    --gs26-border: #334155;
    --gs26-text-muted: #94a3b8;
    --gs26-text-secondary: #cbd5e1;
    --gs26-button-background: #111827;
    --gs26-button-text: #e5e7eb;
}

@supports not (height: 100dvh) {
    :root {
        --gs26-app-height: 100vh;
    }
}

html, body {
    margin: 0;
    padding: 0;
    width: 100%;
    min-height: var(--gs26-app-height);
    height: var(--gs26-app-height);
    background: var(--gs26-app-background);
    color: var(--gs26-app-text);
    overflow: hidden;
}

:root, html {
    color-scheme: dark;
}

#main {
    width: 100%;
    min-height: var(--gs26-app-height);
    height: var(--gs26-app-height);
    background: var(--gs26-app-background);
    color: var(--gs26-app-text);
}

* { box-sizing: border-box; }
"#;

const _CONNECT_SHOWN_KEY: &str = "gs_connect_shown";

fn shell_theme() -> ThemeConfig {
    telemetry_dashboard::app_shell_theme()
}

fn shell_page_style(theme: &ThemeConfig) -> String {
    format!(
        "min-height:var(--gs26-app-height); height:var(--gs26-app-height); overflow-y:auto; overflow-x:hidden; display:flex; align-items:center; justify-content:center; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;",
        theme.app_background, theme.text_primary
    )
}

fn shell_card_style(theme: &ThemeConfig, width: &str) -> String {
    format!(
        "width:{width}; padding:24px; border:1px solid {}; border-radius:16px; background:{}; color:{}; box-shadow:0 12px 30px rgba(0,0,0,0.34);",
        theme.border_strong, theme.panel_background, theme.text_primary
    )
}

fn shell_button_style(theme: &ThemeConfig) -> String {
    format!(
        "padding:10px 14px; border-radius:12px; border:1px solid {}; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont; cursor:pointer;",
        theme.button_border, theme.button_background, theme.button_text
    )
}

fn shell_button_alt_style(theme: &ThemeConfig) -> String {
    format!(
        "padding:10px 14px; border-radius:12px; border:1px solid {}; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont; cursor:pointer;",
        theme.tab_shell_border, theme.panel_background_alt, theme.text_primary
    )
}

fn shell_input_style(theme: &ThemeConfig, margin_bottom: bool) -> String {
    format!(
        "width:100%; padding:12px; border-radius:12px; border:1px solid {}; background:{}; color:{}; outline:none;{}",
        theme.border,
        theme.app_background,
        theme.text_primary,
        if margin_bottom {
            " margin-bottom:12px;"
        } else {
            ""
        }
    )
}

fn shell_notice_style(theme: &ThemeConfig) -> String {
    format!(
        "margin-top:14px; padding:12px; border-radius:12px; border:1px solid {}; background:{}; color:{}; white-space:pre-wrap; overflow-wrap:anywhere; word-break:break-word; line-height:1.4; max-width:72ch; align-self:flex-start;",
        theme.border, theme.app_background, theme.text_secondary
    )
}

fn shell_warning_style(theme: &ThemeConfig) -> String {
    format!(
        "margin-bottom:14px; padding:12px; border-radius:12px; border:1px solid {}; background:{}; color:{}; white-space:pre-wrap; overflow-wrap:anywhere; word-break:break-word;",
        theme.warning_border, theme.warning_background, theme.warning_text
    )
}

fn format_session_load_error(err: &str) -> String {
    let lower = err.to_ascii_lowercase();
    let tls_like = lower.contains("ssl")
        || lower.contains("tls")
        || lower.contains("certificate")
        || lower.contains("unknown issuer")
        || lower.contains("self signed")
        || lower.contains("invalid peer certificate");

    if tls_like {
        err.to_string()
    } else {
        format!(
            "{}\n\nThe app could not load the Ground Station session endpoint. Check that the Ground Station URL is correct and that the proxy or server is healthy.",
            err
        )
    }
}

#[derive(Clone, Routable, PartialEq)]
pub enum Route {
    #[route("/")]
    Root {},

    #[route("/dashboard")]
    Dashboard {},

    #[route("/login")]
    Login {},

    #[cfg(not(target_arch = "wasm32"))]
    #[route("/connect")]
    Connect {},

    #[cfg(not(target_arch = "wasm32"))]
    #[route("/version")]
    Version {},

    #[cfg(not(target_arch = "wasm32"))]
    #[route("/settings")]
    Settings {},
}

#[cfg(target_arch = "wasm32")]
/// Redirects the web build's root route to the dashboard entrypoint.
fn connect_route() -> Route {
    Route::Root {}
}

#[cfg(not(target_arch = "wasm32"))]
/// Returns the native-only connection setup route.
fn connect_route() -> Route {
    Route::Connect {}
}

#[cfg(target_arch = "wasm32")]
/// Returns the authenticated landing route for the web build.
fn authenticated_route() -> Route {
    Route::Root {}
}

#[cfg(not(target_arch = "wasm32"))]
/// Returns the authenticated landing route for native builds.
fn authenticated_route() -> Route {
    Route::Dashboard {}
}

// -------------------------
// Native-only Objective-C poke shims
// -------------------------
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod objc_poke {
    use std::ffi::CString;
    use std::os::raw::c_char;

    unsafe extern "C" {
        fn gs26_localnet_poke_url(url: *const c_char);
    }

    /// Touches a LAN URL through native code so Apple platforms can surface local-network prompts.
    pub fn poke_url(url: &str) {
        if let Ok(c) = CString::new(url) {
            unsafe { gs26_localnet_poke_url(c.as_ptr()) };
        }
    }
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(any(target_os = "macos", target_os = "ios"))
))]
mod objc_poke {
    /// No-op placeholder for platforms that do not need Objective-C local-network pokes.
    pub fn poke_url(_url: &str) {}
}

// -------------------------
// Persistence helpers
// -------------------------
#[cfg(not(target_arch = "wasm32"))]
mod persist {
    use super::_CONNECT_SHOWN_KEY;
    use std::io;

    /// Resolves the app-specific fallback directory for persisted native state.
    fn fallback_storage_dir() -> std::path::PathBuf {
        dirs::data_local_dir()
            .or_else(dirs::data_dir)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()))
            .join("gs26")
    }

    #[cfg(target_os = "android")]
    /// Resolves the Android app-private files directory when the JNI context is available.
    fn android_storage_dir() -> Option<std::path::PathBuf> {
        use ::jni::objects::{JObject, JString};
        use ::jni::{JavaVM, jni_sig, jni_str};
        use ndk_context::android_context;

        let ctx = android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) };
        vm.attach_current_thread(|env| -> ::jni::errors::Result<std::path::PathBuf> {
            let context = unsafe { JObject::from_raw(env, ctx.context().cast()) };

            let files_dir = env
                .call_method(
                    &context,
                    jni_str!("getFilesDir"),
                    jni_sig!("()Ljava/io/File;"),
                    &[],
                )?
                .l()?;
            let path_obj = env
                .call_method(
                    &files_dir,
                    jni_str!("getAbsolutePath"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let path = env.as_cast::<JString>(&path_obj)?.try_to_string(env)?;

            let _ = context.into_raw();
            Ok(std::path::PathBuf::from(path).join("gs26"))
        })
        .ok()
    }

    /// Chooses the best writable native storage directory for simple key-value persistence.
    fn storage_dir() -> std::path::PathBuf {
        #[cfg(target_os = "android")]
        {
            if let Some(path) = android_storage_dir() {
                return path;
            }
        }

        fallback_storage_dir()
    }

    /// Builds the on-disk path for a persisted key.
    fn path_for(key: &str) -> std::path::PathBuf {
        storage_dir().join(format!("{key}.txt"))
    }

    /// Reads and trims a persisted string value.
    fn _read_key(key: &str) -> Option<String> {
        let path = path_for(key);
        std::fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_string())
    }

    /// Writes a string key to disk, creating the storage directory on demand.
    fn write_key(key: &str, v: &str) -> Result<(), io::Error> {
        let dir = storage_dir();
        std::fs::create_dir_all(&dir)?;
        std::fs::write(path_for(key), v.as_bytes())
    }

    /// Persists that the native connect screen has already been shown.
    pub fn write_connect_shown(v: bool) -> Result<(), io::Error> {
        write_key(_CONNECT_SHOWN_KEY, if v { "true" } else { "false" })
    }
}

// -------------------------
// URL parsing / normalization
// -------------------------
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
struct ParsedBaseUrl {
    scheme: String, // "http" or "https"
    host: String,
    port: u16,
}

#[cfg(not(target_arch = "wasm32"))]
/// Normalizes a base URL down to `scheme://host[:port]`.
fn normalize_base_url(mut base: String) -> String {
    // strip fragment
    if let Some(i) = base.find('#') {
        base.truncate(i);
    }

    // strip path but keep scheme://host[:port]
    if let Some(scheme_end) = base.find("://") {
        let rest = &base[scheme_end + 3..];
        if let Some(slash) = rest.find('/') {
            base.truncate(scheme_end + 3 + slash);
        }
    }

    base.trim_end_matches('/').trim().to_ascii_lowercase()
}

#[cfg(not(target_arch = "wasm32"))]
/// Joins a normalized base URL with an absolute path segment.
fn join_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = if path.starts_with('/') { path } else { "/" };
    format!("{base}{path}")
}

#[cfg(not(target_arch = "wasm32"))]
/// Parses a backend base URL into the pieces needed for HTTP and WebSocket probes.
fn parse_base_url(url: &str) -> Result<ParsedBaseUrl, String> {
    let u = url.trim().to_ascii_lowercase();
    let (scheme, rest) = if let Some(x) = u.strip_prefix("http://") {
        ("http".to_string(), x)
    } else if let Some(x) = u.strip_prefix("https://") {
        ("https".to_string(), x)
    } else {
        return Err("URL must start with http:// or https://".to_string());
    };

    let hostport = rest.split('/').next().unwrap_or(rest);
    let mut parts = hostport.split(':');
    let host = parts.next().unwrap_or("").trim().to_string();

    if host.is_empty() {
        return Err("Missing host in URL".to_string());
    }

    let port = parts
        .next()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or_else(|| if scheme == "https" { 443 } else { 80 });

    Ok(ParsedBaseUrl { scheme, host, port })
}

#[cfg(not(target_arch = "wasm32"))]
fn split_base_url_for_connect(url: &str) -> (&'static str, String) {
    let normalized = normalize_base_url(url.to_string());
    if let Some(rest) = normalized.strip_prefix("https://") {
        ("https://", rest.to_string())
    } else if let Some(rest) = normalized.strip_prefix("http://") {
        ("http://", rest.to_string())
    } else {
        ("https://", normalized)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn compose_base_url_for_connect(scheme: &str, host_input: &str) -> String {
    let host = host_input.trim().to_ascii_lowercase();
    if host.is_empty() {
        String::new()
    } else {
        normalize_base_url(format!("{scheme}{host}"))
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// Converts an HTTP base URL into a WebSocket origin.
fn ws_origin_for_base(parsed: &ParsedBaseUrl) -> String {
    let ws_scheme = if parsed.scheme == "https" {
        "wss"
    } else {
        "ws"
    };
    format!("{ws_scheme}://{}:{}", parsed.host, parsed.port)
}

#[cfg(not(target_arch = "wasm32"))]
/// Truncates a string for UI display while keeping line endings predictable.
fn snip(mut s: String, max: usize) -> String {
    s = s.replace('\r', "");
    if s.len() > max {
        s.truncate(max);
        s.push('…');
    }
    s
}

// -------------------------
// Route probing (actual backend routes)
// -------------------------
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
struct RouteProbeSpec {
    path: &'static str,
    method: &'static str,
}

#[cfg(not(target_arch = "wasm32"))]
const ROUTE_PROBE_SPECS: &[RouteProbeSpec] = &[
    RouteProbeSpec {
        path: "/api/auth/session",
        method: "GET",
    },
    RouteProbeSpec {
        path: "/api/auth/login",
        method: "POST",
    },
    RouteProbeSpec {
        path: "/api/auth/logout",
        method: "POST",
    },
    RouteProbeSpec {
        path: "/api/recent",
        method: "GET",
    },
    RouteProbeSpec {
        path: "/api/alerts",
        method: "GET",
    },
    RouteProbeSpec {
        path: "/api/layout",
        method: "GET",
    },
    RouteProbeSpec {
        path: "/api/map_config",
        method: "GET",
    },
    RouteProbeSpec {
        path: "/flightstate",
        method: "GET",
    },
    RouteProbeSpec {
        path: "/api/gps",
        method: "GET",
    },
    RouteProbeSpec {
        path: "/tiles",
        method: "GET",
    },
    RouteProbeSpec {
        path: "/ws",
        method: "GET",
    },
];

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
struct RouteCheck {
    method: &'static str,
    path: &'static str,
    url: String,
    ok: bool,
    status: Option<u16>,
    body_snip: String,
    note: String,
    err: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
struct WsProbeStatus {
    ok: bool,
    url: String,
    status: Option<u16>,
    note: String,
    err: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
struct ConnectionTestReport {
    original_base: String,
    parsed_host: String,
    parsed_port: u16,
    parsed_scheme: String,
    checks: Vec<RouteCheck>,
    ws_probe: WsProbeStatus,
}

#[cfg(not(target_arch = "wasm32"))]
/// Evaluates whether a route returned the status code expected by the connection tester.
fn status_ok_for_path(method: &str, path: &str, status: u16) -> (bool, &'static str) {
    match (method, path) {
        ("GET", "/api/auth/session") => (status == 200, "expected 200"),
        ("POST", "/api/auth/login") => match status {
            200 | 400 | 401 | 403 | 415 => (true, "reachable (auth login endpoint responded)"),
            _ => (false, "unexpected status for auth login"),
        },
        ("POST", "/api/auth/logout") => match status {
            200 | 204 | 401 | 403 => (true, "reachable (auth logout endpoint responded)"),
            _ => (false, "unexpected status for auth logout"),
        },
        ("GET", "/api/recent")
        | ("GET", "/api/alerts")
        | ("GET", "/api/layout")
        | ("GET", "/api/map_config")
        | ("GET", "/flightstate")
        | ("GET", "/api/gps") => (status == 200, "expected 200"),
        (_, "/ws") => match status {
            101 | 400 | 426 => (true, "reachable (ws upgrade required)"),
            _ => (false, "unexpected status for ws route"),
        },
        (_, "/tiles") => match status {
            200 | 403 | 404 => (true, "reachable (tile may not exist)"),
            _ => (false, "unexpected status for tiles"),
        },
        _ => ((200..400).contains(&status), "ok"),
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// Rewrites a `reqwest` error into a short UI-friendly category string.
fn classify_reqwest_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        return "timeout".into();
    }
    if e.is_connect() {
        return "connect failed (refused/unreachable/DNS/TLS)".into();
    }
    if e.is_request() {
        return "request build/dispatch error".into();
    }
    if e.is_body() {
        return "body read error".into();
    }
    if e.is_decode() {
        return "decode error".into();
    }

    let mut chain = String::new();
    let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = cur {
        chain.push_str(&format!(" -> {err}"));
        cur = err.source();
    }
    format!("unknown ({chain})")
}

#[cfg(not(target_arch = "wasm32"))]
/// Builds the short-timeout client used by the native connection test screen.
fn build_probe_client(skip_tls_verify: bool) -> Result<reqwest::Client, String> {
    // Fast but still reliable:
    // - connect_timeout: how long we wait for TCP/TLS connect
    // - timeout: total request time budget (includes body)
    auth::build_native_http_client(
        skip_tls_verify,
        std::time::Duration::from_millis(_CONNECTION_TIMEOUT_MS),
        std::time::Duration::from_millis(_BODY_TRANSFER_TIMEOUT_MS),
    )
    .map_err(|e| format!("build client failed: {e}"))
}

#[cfg(not(target_arch = "wasm32"))]
/// Executes a single HTTP probe and captures a small body snippet for diagnostics.
async fn http_probe_with_client(
    client: &reqwest::Client,
    method: &'static str,
    path: &'static str,
    url: String,
) -> Result<(u16, String), String> {
    use tokio::time::{Duration, timeout};

    const MAX_BODY_BYTES: usize = 4096;
    const BODY_SNIP_TIMEOUT_MS: u64 = 400;

    let mut request = match method {
        "POST" => client.post(&url),
        _ => client.get(&url),
    };
    if method == "GET" && path == "/api/recent" {
        request = request.header(reqwest::header::ACCEPT, "application/x-ndjson");
    }
    let mut resp = request
        .send()
        .await
        .map_err(|e| format!("send failed: {} | kind={}", e, classify_reqwest_error(&e)))?;

    let status = resp.status().as_u16();
    if resp.status().is_success() {
        return Ok((status, String::new()));
    }

    // Only sample a small error body snippet, and do not let it hold up the probe UI.
    let body = match timeout(Duration::from_millis(BODY_SNIP_TIMEOUT_MS), async {
        let mut out = Vec::new();
        while out.len() < MAX_BODY_BYTES {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    let remaining = MAX_BODY_BYTES - out.len();
                    out.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                    if out.len() >= MAX_BODY_BYTES {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => return Err(format!("read body failed: {e}")),
            }
        }
        Ok::<String, String>(String::from_utf8_lossy(&out).to_string())
    })
    .await
    {
        Ok(Ok(text)) => text,
        Ok(Err(err)) => return Err(err),
        Err(_) => "<body omitted: timed out reading error snippet>".to_string(),
    };

    let body = if body.is_empty() {
        String::new()
    } else {
        snip(body, 300)
    }
    ;
    Ok((status, body))
}

#[cfg(not(target_arch = "wasm32"))]
/// Probes the main backend HTTP routes concurrently against the configured host.
async fn test_routes_host_only(base: &str, skip_tls_verify: bool) -> Vec<RouteCheck> {
    use futures_util::future::join_all;

    let client = match build_probe_client(skip_tls_verify) {
        Ok(c) => c,
        Err(e) => {
            // If client build failed, mark everything failed quickly.
            return ROUTE_PROBE_SPECS
                .iter()
                .map(|probe| RouteCheck {
                    method: probe.method,
                    path: probe.path,
                    url: join_url(base, probe.path),
                    ok: false,
                    status: None,
                    body_snip: "".to_string(),
                    note: "client build failed".to_string(),
                    err: Some(e.clone()),
                })
                .collect();
        }
    };

    // Run all probes concurrently.
    let futs = ROUTE_PROBE_SPECS.iter().map(|probe| {
        let url = join_url(base, probe.path);
        let path = probe.path;
        let method = probe.method;
        let client = &client;

        async move {
            match http_probe_with_client(client, method, path, url.clone()).await {
                Ok((status, body_snip)) => {
                    let (ok, note) = status_ok_for_path(method, path, status);
                    RouteCheck {
                        method,
                        path,
                        url,
                        ok,
                        status: Some(status),
                        body_snip,
                        note: note.to_string(),
                        err: None,
                    }
                }
                Err(e) => RouteCheck {
                    method,
                    path,
                    url,
                    ok: false,
                    status: None,
                    body_snip: "".to_string(),
                    note: "request failed".to_string(),
                    err: Some(e),
                },
            }
        }
    });

    join_all(futs).await
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
/// Performs a real WebSocket handshake with a hard timeout for the connection test UI.
async fn ws_connect_probe(parsed: &ParsedBaseUrl, skip_tls_verify: bool) -> Result<String, String> {
    use tokio::time::timeout;

    let ws_origin = ws_origin_for_base(parsed);
    let ws_url = format!("{ws_origin}/ws");

    // Real websocket handshake, but time-bounded so it can't hang forever.
    let res = timeout(std::time::Duration::from_millis(_WS_TIMEOUT_MS), async {
        if skip_tls_verify && ws_url.starts_with("wss://") {
            let tls = insecure_rustls_connector()
                .map_err(|e| format!("rustls connector build failed: {e}"))?;
            tokio_tungstenite::connect_async_tls_with_config(ws_url.clone(), None, false, Some(tls))
                .await
                .map_err(|e| format!("{e}"))
        } else if ws_url.starts_with("wss://") {
            #[cfg(any(target_os = "android", target_os = "ios", target_os = "macos"))]
            {
                let tls = platform_rustls_connector()
                    .map_err(|e| format!("platform rustls connector build failed: {e}"))?;
                tokio_tungstenite::connect_async_tls_with_config(
                    ws_url.clone(),
                    None,
                    false,
                    Some(tls),
                )
                .await
                .map_err(|e| format!("{e}"))
            }
            #[cfg(not(any(target_os = "android", target_os = "ios", target_os = "macos")))]
            {
                tokio_tungstenite::connect_async(ws_url.clone())
                    .await
                    .map_err(|e| format!("{e}"))
            }
        } else {
            tokio_tungstenite::connect_async(ws_url.clone())
                .await
                .map_err(|e| format!("{e}"))
        }
    })
    .await;

    match res {
        Err(_) => Err(format!(
            "❌ WebSocket connect FAILED (timeout)\n    URL: {}",
            ws_url
        )),
        Ok(Ok((_stream, resp))) => Ok(format!(
            "✅ WebSocket handshake OK\n    URL: {}\n    HTTP: {}",
            ws_url,
            resp.status()
        )),
        Ok(Err(e)) => Err(format!(
            "❌ WebSocket connect FAILED\n    URL: {}\n    ERROR: {}",
            ws_url, e
        )),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn ws_probe_status(parsed: &ParsedBaseUrl, ws_probe: Result<String, String>) -> WsProbeStatus {
    let url = format!("{}/ws", ws_origin_for_base(parsed));
    match ws_probe {
        Ok(message) => {
            let status = message
                .lines()
                .find_map(|line| line.strip_prefix("    HTTP: "))
                .and_then(|value| value.trim().parse::<u16>().ok());
            WsProbeStatus {
                ok: true,
                url,
                status,
                note: "websocket handshake ok".to_string(),
                err: None,
            }
        }
        Err(err) => WsProbeStatus {
            ok: false,
            url,
            status: None,
            note: "websocket handshake failed".to_string(),
            err: Some(err),
        },
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn build_connection_test_report(
    original_base: &str,
    parsed: &ParsedBaseUrl,
    checks: Vec<RouteCheck>,
    ws_probe: Result<String, String>,
) -> ConnectionTestReport {
    ConnectionTestReport {
        original_base: original_base.to_string(),
        parsed_host: parsed.host.clone(),
        parsed_port: parsed.port,
        parsed_scheme: parsed.scheme.clone(),
        checks,
        ws_probe: ws_probe_status(parsed, ws_probe),
    }
}

// -------------------------
// App
// -------------------------
#[component]
/// Top-level app component that installs global CSS and mounts the router.
pub fn App() -> Element {
    #[cfg(not(target_arch = "wasm32"))]
    {
        keep_awake::set_enabled(true);
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    {
        let window = use_window();
        use_effect(move || {
            window.set_title(APP_DISPLAY_NAME);
        });
    }
    let theme = shell_theme();
    {
        let theme = theme.clone();
        use_effect(move || {
            telemetry_dashboard::apply_window_theme(&theme);
        });
    }
    let map_assets: Element = {
        rsx! {
            document::Style { "{INLINE_LEAFLET_CSS}" }
            document::Script { "{INLINE_LEAFLET_JS}" }
            document::Script { "{INLINE_GROUND_MAP_JS}" }
        }
    };
    rsx! {
        document::Style { "{GLOBAL_CSS}" }
        Meta { name: "viewport", content: "width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no" }
        {map_assets}

        div {
            style: "min-height: var(--gs26-app-height); width: 100%; background: var(--gs26-app-background); color: var(--gs26-app-text);",
            Router::<Route> {}
        }
    }
}

#[component]
/// Root route that redirects native builds to either connect or dashboard.
pub fn Root() -> Element {
    #[cfg(target_arch = "wasm32")]
    {
        return rsx! { Dashboard {} };
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let nav = use_navigator();

        use_effect(move || {
            if UrlConfig::_stored_base_url().is_some() {
                let _ = nav.replace(Route::Dashboard {});
            } else {
                let _ = nav.replace(connect_route());
            }
        });

        rsx! { div {} }
    }
}

#[component]
/// Shared login card used by both the full login page and the overlay flow.
fn LoginCard(
    title: String,
    subtitle: String,
    allow_back_to_connect: bool,
    on_success_route: Route,
    #[props(default = false)] overlay_mode: bool,
) -> Element {
    let theme = shell_theme();
    let nav = use_navigator();
    let base = UrlConfig::base_http();
    auth::init_from_storage(&base);
    let effect_base = base.clone();
    let continue_logged_out_base = base.clone();
    let skip_tls = UrlConfig::_skip_tls_verify();
    let mut logged_out_status = use_signal(|| None::<Result<AuthSessionStatus, String>>);
    let mut logged_out_probe_base = use_signal(String::new);
    let continue_logged_out_route = on_success_route.clone();
    let sign_in_route = on_success_route.clone();
    let stored_username = auth::current_session()
        .and_then(|session| session.session.username)
        .unwrap_or_default();
    let mut username = use_signal(|| stored_username);
    let mut password = use_signal(String::new);
    let mut remember_me = use_signal(|| true);
    let mut status = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut submit_login = move || {
        let base = UrlConfig::base_http();
        if base.trim().is_empty() {
            status.set("Configure the Ground Station URL first.".to_string());
            return;
        }
        let username_value = username();
        let password_value = password();
        if username_value.trim().is_empty() || password_value.is_empty() {
            status.set("Enter both username and password.".to_string());
            return;
        }
        let remember = *remember_me.read();
        let success_route = sign_in_route.clone();
        busy.set(true);
        status.set("Signing in...".to_string());
        spawn(async move {
            match auth::login(
                &base,
                skip_tls,
                username_value.trim(),
                &password_value,
                remember,
            )
            .await
            {
                Ok(_) => {
                    telemetry_dashboard::reconnect_and_reseed_after_auth_change();
                    busy.set(false);
                    status.set(String::new());
                    let _ = nav.replace(success_route);
                }
                Err(err) => {
                    busy.set(false);
                    status.set(err);
                }
            }
        });
    };

    use_effect({
        let effect_base = effect_base.clone();
        move || {
            auth::init_from_storage(&effect_base);
        }
    });

    use_effect(move || {
        if effect_base.trim().is_empty() {
            return;
        }
        if *logged_out_probe_base.read() == effect_base {
            return;
        }
        logged_out_probe_base.set(effect_base.clone());
        logged_out_status.set(None);
        let base = effect_base.clone();
        spawn(async move {
            logged_out_status.set(Some(
                auth::fetch_logged_out_session_status(&base, skip_tls).await,
            ));
        });
    });

    rsx! {
        div {
            style: if overlay_mode {
                format!(
                    "width:min(560px, 92vw); color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;",
                    theme.text_primary
                )
            } else {
                shell_page_style(&theme)
            },
            div {
                style: shell_card_style(&theme, "min(560px, 92vw)"),
                h1 { style: "margin:0 0 10px 0; font-size:22px;", "{title}" }
                p { style: "margin:0 0 16px 0; color:{theme.text_muted};", "{subtitle}" }
                form {
                    onsubmit: move |evt| {
                        evt.prevent_default();
                        submit_login();
                    },
                    if base.trim().is_empty() {
                        div {
                            style: shell_warning_style(&theme),
                            "Configure the Ground Station URL before logging in."
                        }
                    }

                    label { r#for: "gs26-login-username", style: "display:block; margin-bottom:8px; font-size:13px; color:{theme.text_muted};", "Username" }
                    input {
                        id: "gs26-login-username",
                        name: "username",
                        autocomplete: "username",
                        autocapitalize: "none",
                        spellcheck: "false",
                        style: shell_input_style(&theme, true),
                        placeholder: "Username",
                        value: "{username()}",
                        oninput: move |evt| username.set(evt.value()),
                    }

                    label { r#for: "gs26-login-password", style: "display:block; margin-bottom:8px; font-size:13px; color:{theme.text_muted};", "Password" }
                    input {
                        id: "gs26-login-password",
                        name: "password",
                        autocomplete: "current-password",
                        style: shell_input_style(&theme, false),
                        r#type: "password",
                        placeholder: "Password",
                        value: "{password()}",
                        oninput: move |evt| password.set(evt.value()),
                    }

                    div { style: "margin-top:12px; display:flex; align-items:center; gap:10px;",
                        input {
                            r#type: "checkbox",
                            checked: *remember_me.read(),
                            onclick: move |_| {
                                let next = !*remember_me.read();
                                remember_me.set(next);
                            },
                        }
                        div { style: "font-size:13px; color:{theme.text_muted};", "Remember this device until the Ground Station session expires" }
                    }

                    if !status().is_empty() {
                        div {
                            style: shell_notice_style(&theme),
                            "{status()}"
                        }
                    }

                    div { style: "display:flex; gap:12px; margin-top:16px; justify-content:flex-end; flex-wrap:wrap;",
                    if matches!(logged_out_status.read().as_ref(), Some(Ok(status)) if status.permissions.view_data) {
                        button {
                            style: shell_button_alt_style(&theme),
                            r#type: "button",
                            disabled: busy() || base.trim().is_empty(),
                            onclick: move |_| {
                                let success_route = continue_logged_out_route.clone();
                                let base = continue_logged_out_base.clone();
                                busy.set(true);
                                status.set("Continuing logged out...".to_string());
                                spawn(async move {
                                    match auth::fetch_logged_out_session_status(&base, skip_tls).await {
                                        Ok(session) => {
                                            auth::set_logged_out_status(session);
                                            busy.set(false);
                                            status.set(String::new());
                                            let _ = nav.replace(success_route);
                                        }
                                        Err(err) => {
                                            busy.set(false);
                                            status.set(format!("Logged-out access failed: {err}"));
                                        }
                                    }
                                });
                            },
                            "Use Logged Out"
                        }
                    }

                    if allow_back_to_connect {
                        button {
                            style: shell_button_style(&theme),
                            r#type: "button",
                            onclick: move |_| {
                                let _ = nav.replace(connect_route());
                            },
                            "Back"
                        }
                    }

                    button {
                        r#type: "submit",
                        style: shell_button_style(&theme),
                        disabled: busy() || base.trim().is_empty(),
                        if busy() { "Signing In..." } else { "Sign In" }
                    }
                }
                }
            }
        }
    }
}

#[component]
fn ConnectionFailedCard(message: String, on_retry: EventHandler<()>) -> Element {
    let theme = shell_theme();
    let nav = use_navigator();
    rsx! {
        div {
            style: shell_page_style(&theme),
            div {
                style: shell_card_style(&theme, "min(560px, 92vw)"),
                h1 { style: "margin:0 0 10px 0; font-size:22px;", "Failed to Connect" }
                p { style: "margin:0 0 16px 0; color:{theme.text_muted}; white-space:pre-wrap; overflow-wrap:anywhere; word-break:break-word;", "{message}" }
                div { style: "display:flex; gap:12px; justify-content:flex-end; flex-wrap:wrap;",
                    button {
                        style: shell_button_style(&theme),
                        onclick: move |_| {
                            let _ = nav.replace(connect_route());
                        },
                        "Back to Connect"
                    }
                    button {
                        style: shell_button_alt_style(&theme),
                        onclick: move |_| {
                            on_retry.call(());
                        },
                        "Retry"
                    }
                }
            }
        }
    }
}

#[component]
fn LoginOverlay(
    title: String,
    subtitle: String,
    allow_back_to_connect: bool,
    on_success_route: Route,
) -> Element {
    let theme = shell_theme();
    rsx! {
        div {
            style: "position:relative; width:100%; min-height:var(--gs26-app-height);",
            crate::telemetry_dashboard::TelemetryDashboard {}
            div {
                style: format!(
                    "position:fixed; inset:0; display:flex; align-items:center; justify-content:center; padding:24px; background:{}; backdrop-filter:blur(8px); z-index:1000;",
                    theme.overlay_background
                ),
                LoginCard {
                    title: title.clone(),
                    subtitle: subtitle.clone(),
                    allow_back_to_connect,
                    on_success_route: on_success_route.clone(),
                    overlay_mode: true,
                }
            }
        }
    }
}

#[component]
pub fn Login() -> Element {
    #[cfg(target_arch = "wasm32")]
    let show_live_dashboard = false;
    #[cfg(not(target_arch = "wasm32"))]
    let show_live_dashboard = telemetry_dashboard::dashboard_has_prior_backend_connection();
    if show_live_dashboard {
        rsx! {
            LoginOverlay {
                title: "Sign In".to_string(),
                subtitle: "Authenticate with the Ground Station to view protected data or send commands.".to_string(),
                allow_back_to_connect: true,
                on_success_route: authenticated_route(),
            }
        }
    } else {
        rsx! {
            LoginCard {
                title: "Sign In".to_string(),
                subtitle: "Authenticate with the Ground Station to view protected data or send commands.".to_string(),
                allow_back_to_connect: true,
                on_success_route: authenticated_route(),
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[component]
pub fn Connect() -> Element {
    let theme = shell_theme();
    let nav = use_navigator();

    let initial =
        UrlConfig::_stored_base_url()
            .unwrap_or_else(|| "https://your-ground-station-url.com".to_string());
    let (initial_scheme, initial_host) = split_base_url_for_connect(&initial);
    let initial_skip_tls = UrlConfig::_skip_tls_verify_for_base(&initial);

    let mut scheme_edit = use_signal(|| initial_scheme.to_string());
    let mut host_edit = use_signal(|| initial_host);
    let mut skip_tls = use_signal(|| initial_skip_tls);

    let mut test_status = use_signal(String::new);
    let mut test_report = use_signal(|| None::<ConnectionTestReport>);
    let mut testing = use_signal(|| false);
    let has_test_report = test_report.read().is_some();

    rsx! {
        div {
            style: format!(
                "min-height:var(--gs26-app-height); height:var(--gs26-app-height); overflow:hidden; display:flex; align-items:center; justify-content:center; padding:24px 16px; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;",
                theme.app_background, theme.text_primary
            ),
            div {
                style: format!(
                    "{} display:flex; flex-direction:column; overflow:hidden; {};",
                    shell_card_style(
                        &theme,
                        if has_test_report { "min(900px, 94vw)" } else { "min(760px, 92vw)" }
                    ),
                    if has_test_report {
                        "height:min(900px, calc(var(--gs26-app-height) - 48px)); max-height:min(900px, calc(var(--gs26-app-height) - 48px));"
                    } else {
                        "height:auto; max-height:min(520px, calc(var(--gs26-app-height) - 48px));"
                    }
                ),

                div {
                    style: "display:flex; align-items:flex-start; justify-content:space-between; gap:12px; margin-bottom:12px;",
                    h1 { style: "margin:0; font-size:20px;", "{APP_DISPLAY_NAME}" }
                    div {
                        style: "display:flex; gap:10px; flex-wrap:wrap;",
                        button {
                            style: shell_button_style(&theme),
                            onclick: move |_| {
                                let _ = nav.push(Route::Settings {});
                            },
                            "Settings"
                        }
                        button {
                            style: shell_button_style(&theme),
                            onclick: move |_| {
                                let _ = nav.push(Route::Version {});
                            },
                            "Version"
                        }
                    }
                }
                div {
                    style: format!(
                        "flex:1 1 auto; min-height:0; {} padding-right:4px;",
                        if has_test_report {
                            "overflow:auto;"
                        } else {
                            "overflow:visible;"
                        }
                    ),
                    p { style: "margin:0 0 16px 0; color:{theme.text_muted};",
                        "Enter the Ground Station host and port. Example: ",
                        code { "your-ground-station-url.com" }
                    }

                    div {
                        style: format!(
                            "display:flex; align-items:stretch; min-height:48px; border:1px solid {}; border-radius:12px; background:{}; color:{}; overflow:hidden;",
                            theme.border, theme.app_background, theme.text_primary
                        ),
                        div {
                            style: format!(
                                "position:relative; flex:0 0 118px; border-right:1px solid {}; background:{};",
                                theme.border, theme.panel_background
                            ),
                            select {
                                style: format!(
                                    "width:100%; height:100%; padding:0 30px 0 14px; border:none; border-radius:0; background:transparent; color:{}; appearance:none; -webkit-appearance:none; font-size:14px; outline:none; cursor:pointer;",
                                    theme.text_primary
                                ),
                                value: "{scheme_edit()}",
                                onchange: move |evt| {
                                    scheme_edit.set(evt.value());
                                    test_status.set(String::new());
                                    test_report.set(None);
                                },
                                option { value: "https://", "https" },
                                option { value: "http://", "http" },
                            },
                            div {
                                style: format!(
                                    "position:absolute; right:12px; top:50%; transform:translateY(-50%); color:{}; font-size:11px; pointer-events:none;",
                                    theme.text_muted
                                ),
                                "▼"
                            },
                        },

                        input {
                            style: format!(
                                "flex:1 1 auto; min-width:0; padding:12px 14px; border:none; background:transparent; color:{}; outline:none; font-size:14px;",
                                theme.text_primary
                            ),
                            placeholder: "your-ground-station-url.com",
                            value: "{host_edit()}",
                            autocapitalize: "none",
                            spellcheck: "false",
                            oninput: move |evt| {
                                host_edit.set(evt.value().to_ascii_lowercase());
                                test_status.set(String::new());
                                test_report.set(None);
                            },
                        }
                    }

                    div { style: "margin-top:12px; display:flex; align-items:center; gap:10px;",
                        input {
                            r#type: "checkbox",
                            checked: *skip_tls.read(),
                            onclick: move |_| {
                                let next = !*skip_tls.read();
                                skip_tls.set(next);
                                let base = compose_base_url_for_connect(&scheme_edit(), &host_edit());
                                if !base.is_empty() {
                                    UrlConfig::_set_skip_tls_verify_for_base(&base, next);
                                }
                            }
                        }
                        div { style: "font-size:13px; color:{theme.text_muted};",
                            "Disable TLS certificate verification for this host (self-signed certs)"
                        }
                    }

                    if !test_status().is_empty() {
                        div {
                            style: shell_notice_style(&theme),
                            "{test_status()}"
                        }
                    }

                    if let Some(report) = test_report.read().as_ref() {
                        div {
                            style: "margin-top:14px; display:flex; flex-direction:column; gap:12px;",
                            if all_tests_passed(&report.checks, &report.ws_probe) {
                                div {
                                    style: format!(
                                        "padding:14px 16px; border-radius:14px; border:1px solid {}; background:{}; color:{};",
                                        theme.border, theme.info_background, theme.success_text
                                    ),
                                    div { style: "font-weight:700; margin-bottom:4px;", "All Connection Tests Passed" }
                                    div { style: "font-size:13px;", "Ground Station HTTP routes and WebSocket handshake are reachable." }
                                }
                            } else {
                                div {
                                    style: format!(
                                        "padding:14px 16px; border-radius:14px; border:1px solid {}; background:{}; color:{};",
                                        theme.warning_border, theme.warning_background, theme.warning_text
                                    ),
                                    div { style: "font-weight:700; margin-bottom:4px;", "Connection Test Found Issues" }
                                    div { style: "font-size:13px;", "Review the endpoint list below to see which routes failed or responded unexpectedly." }
                                }
                            }

                            div {
                                style: format!(
                                    "padding:12px; border-radius:12px; border:1px solid {}; background:{}; color:{}; display:grid; grid-template-columns:repeat(auto-fit, minmax(180px, 1fr)); gap:8px 12px;",
                                    theme.border, theme.app_background, theme.text_secondary
                                ),
                                div { style: "font-size:12px;", "Base" }
                                div { style: "font-size:13px; color:{theme.text_primary}; overflow-wrap:anywhere;", "{report.original_base}" }
                                div { style: "font-size:12px;", "Parsed Host" }
                                div { style: "font-size:13px; color:{theme.text_primary};", "{report.parsed_host}" }
                                div { style: "font-size:12px;", "Port" }
                                div { style: "font-size:13px; color:{theme.text_primary};", "{report.parsed_port}" }
                                div { style: "font-size:12px;", "Scheme" }
                                div { style: "font-size:13px; color:{theme.text_primary};", "{report.parsed_scheme}" }
                            }

                            div {
                                style: "display:flex; flex-direction:column; gap:10px;",
                                for check in &report.checks {
                                    div {
                                        key: "{check.method}:{check.path}",
                                        style: format!(
                                            "padding:12px; border-radius:12px; border:1px solid {}; background:{}; color:{};",
                                            theme.border,
                                            if check.ok { &theme.panel_background } else { &theme.panel_background_alt },
                                            theme.text_primary
                                        ),
                                        div {
                                            style: "display:flex; align-items:center; justify-content:space-between; gap:12px; flex-wrap:wrap;",
                                            div { style: "display:flex; align-items:center; gap:10px; flex-wrap:wrap;",
                                                div {
                                                    style: format!(
                                                        "min-width:22px; height:22px; border-radius:999px; display:flex; align-items:center; justify-content:center; font-size:12px; font-weight:700; background:{}; color:{};",
                                                        if check.ok { &theme.info_background } else { &theme.warning_background },
                                                        if check.ok { &theme.success_text } else { &theme.warning_text }
                                                    ),
                                                    if check.ok { "OK" } else { "X" }
                                                }
                                                code { "{check.method}" }
                                                div { style: "font-weight:600;", "{check.path}" }
                                            }
                                            div { style: "font-size:12px; color:{theme.text_muted};",
                                                "Status ",
                                                {check.status.map(|s| s.to_string()).unwrap_or_else(|| "—".to_string())}
                                            }
                                        }
                                        div { style: "margin-top:6px; font-size:13px; color:{theme.text_secondary};", "{check.note}" }
                                        div { style: "margin-top:6px; font-size:12px; color:{theme.text_muted}; overflow-wrap:anywhere;", "{check.url}" }
                                        if let Some(err) = &check.err {
                                            div { style: "margin-top:8px; font-size:12px; color:{theme.warning_text}; overflow-wrap:anywhere;", "{err}" }
                                        }
                                        if !check.body_snip.trim().is_empty() {
                                            div { style: "margin-top:8px; font-size:12px; color:{theme.text_muted}; overflow-wrap:anywhere;", "{check.body_snip.trim()}" }
                                        }
                                    }
                                }

                                div {
                                    style: format!(
                                        "padding:12px; border-radius:12px; border:1px solid {}; background:{}; color:{};",
                                        theme.border,
                                        if report.ws_probe.ok { &theme.panel_background } else { &theme.panel_background_alt },
                                        theme.text_primary
                                    ),
                                    div {
                                        style: "display:flex; align-items:center; justify-content:space-between; gap:12px; flex-wrap:wrap;",
                                        div { style: "display:flex; align-items:center; gap:10px; flex-wrap:wrap;",
                                            div {
                                                style: format!(
                                                    "min-width:22px; height:22px; border-radius:999px; display:flex; align-items:center; justify-content:center; font-size:12px; font-weight:700; background:{}; color:{};",
                                                    if report.ws_probe.ok { &theme.info_background } else { &theme.warning_background },
                                                    if report.ws_probe.ok { &theme.success_text } else { &theme.warning_text }
                                                ),
                                                if report.ws_probe.ok { "OK" } else { "X" }
                                            }
                                            code { "WS" }
                                            div { style: "font-weight:600;", "/ws handshake" }
                                        }
                                        if let Some(status) = report.ws_probe.status {
                                            div { style: "font-size:12px; color:{theme.text_muted};", "HTTP {status}" }
                                        }
                                    }
                                    div { style: "margin-top:6px; font-size:13px; color:{theme.text_secondary};", "{report.ws_probe.note}" }
                                    div { style: "margin-top:6px; font-size:12px; color:{theme.text_muted}; overflow-wrap:anywhere;", "{report.ws_probe.url}" }
                                    if let Some(err) = &report.ws_probe.err {
                                        div { style: "margin-top:8px; font-size:12px; color:{theme.warning_text}; overflow-wrap:anywhere; white-space:pre-wrap;", "{err}" }
                                    }
                                }
                            }
                        }
                    }
                }

                div { style: format!("display:flex; gap:12px; margin-top:16px; padding-top:16px; justify-content:flex-end; flex-wrap:wrap; border-top:1px solid {};", theme.border_soft),
                    button {
                        style: shell_button_style(&theme),
                        onclick: move |_| {
                            let u_norm = compose_base_url_for_connect(&scheme_edit(), &host_edit());
                            if u_norm.is_empty() {
                                test_status.set("Enter a URL first.".to_string());
                                test_report.set(None);
                                return;
                            }

                            objc_poke::poke_url(&u_norm);
                            UrlConfig::set_base_url_and_persist(u_norm.to_string());
                            UrlConfig::_set_skip_tls_verify_for_base(&u_norm, *skip_tls.read());
                            let _ = persist::write_connect_shown(true);
                            let _ = nav.replace(Route::Login {});
                        },
                        "Sign In"
                    }

                    button {
                        style: shell_button_alt_style(&theme),
                        disabled: testing(),
                        onclick: move |_| {
                            let u_norm = compose_base_url_for_connect(&scheme_edit(), &host_edit());
                            if u_norm.is_empty() {
                                test_status.set("Enter a URL first.".to_string());
                                test_report.set(None);
                                return;
                            }

                            let parsed = match parse_base_url(&u_norm) {
                                Ok(p) => p,
                                Err(e) => {
                                    test_status.set(e);
                                    test_report.set(None);
                                    return;
                                }
                            };

                            testing.set(true);
                            test_status.set("Testing connection (fast probes)...".to_string());
                            test_report.set(None);

                            objc_poke::poke_url(&u_norm);

                            let skip_tls_verify = *skip_tls.read();
                            spawn(async move {
                                let (checks, ws_probe) = futures_util::join!(
                                    test_routes_host_only(&u_norm, skip_tls_verify),
                                    ws_connect_probe(&parsed, skip_tls_verify)
                                );

                                let report =
                                    build_connection_test_report(&u_norm, &parsed, checks, ws_probe);
                                testing.set(false);
                                test_status.set(String::new());
                                test_report.set(Some(report));
                            });
                        },
                        if testing() { "Testing..." } else { "Test Connection" }
                    }

                    button {
                        style: shell_button_style(&theme),
                        onclick: move |_| {
                            let u_norm = compose_base_url_for_connect(&scheme_edit(), &host_edit());
                            if u_norm.is_empty() {
                                test_status.set("Enter a URL first.".to_string());
                                return;
                            }

                            objc_poke::poke_url(&u_norm);

                            UrlConfig::set_base_url_and_persist(u_norm.to_string());
                            UrlConfig::_set_skip_tls_verify_for_base(&u_norm, *skip_tls.read());
                            if UrlConfig::_stored_base_url().as_deref() != Some(u_norm.as_str()) {
                                test_status.set(
                                    "Failed to save the Ground Station URL on this device. The app stayed disconnected."
                                        .to_string(),
                                );
                                return;
                            }
                            crate::telemetry_dashboard::clear_and_reconnect_after_connect();
                            let _ = persist::write_connect_shown(true);
                            let _ = nav.replace(Route::Dashboard {});
                        },
                        "Connect"
                    }
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[component]
pub fn Settings() -> Element {
    let theme = shell_theme();
    let nav = use_navigator();

    rsx! {
        div {
            style: format!(
                "min-height:var(--gs26-app-height); height:var(--gs26-app-height); overflow:hidden; display:flex; align-items:center; justify-content:center; padding:24px 16px; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;",
                theme.app_background, theme.text_primary
            ),
            div {
                style: format!(
                    "{} display:flex; flex-direction:column; width:min(980px, 94vw); height:min(900px, calc(var(--gs26-app-height) - 48px)); max-height:min(900px, calc(var(--gs26-app-height) - 48px)); overflow:hidden;",
                    shell_card_style(&theme, "min(980px, 94vw)")
                ),
                div {
                    style: format!(
                        "display:flex; align-items:flex-start; justify-content:space-between; gap:12px; margin-bottom:12px; padding-bottom:12px; flex-wrap:wrap; border-bottom:1px solid {};",
                        theme.border_soft
                    ),
                    h1 { style: "margin:0; font-size:20px;", "Settings" }
                    button {
                        style: shell_button_style(&theme),
                        onclick: move |_| {
                            let _ = nav.push(Route::Connect {});
                        },
                        "Back"
                    }
                }
                div {
                    style: "flex:1 1 auto; min-height:0; overflow:auto; padding-right:4px;",
                    crate::telemetry_dashboard::NativeSettingsPage {}
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[component]
pub fn Version() -> Element {
    let theme = shell_theme();
    let nav = use_navigator();
    let can_go_back = nav.can_go_back();
    let back_action = move |_| {
        if can_go_back {
            nav.go_back();
        } else if UrlConfig::_stored_base_url().is_some() {
            let _ = nav.replace(Route::Dashboard {});
        } else {
            let _ = nav.replace(connect_route());
        }
    };

    rsx! {
        div {
            style: format!(
                "position:fixed; inset:0; overflow-y:auto; overflow-x:hidden; display:flex; align-items:flex-start; justify-content:center; padding:24px 16px; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont; overscroll-behavior:contain; -webkit-overflow-scrolling:touch;",
                theme.app_background,
                theme.text_primary
            ),
            div {
                style: shell_card_style(&theme, "min(900px, 100%)"),
                div {
                    style: "display:flex; align-items:flex-start; justify-content:space-between; gap:12px; margin-bottom:12px; flex-wrap:wrap;",
                    h1 { style: "margin:0; font-size:20px;", "{APP_DISPLAY_NAME}" }
                    button {
                        style: shell_button_style(&theme),
                        onclick: back_action,
                        "Back"
                    }
                }
                crate::telemetry_dashboard::version_page::VersionTab { theme: theme.clone() }
            }
        }
    }
}

#[component]
pub fn Dashboard() -> Element {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let nav = use_navigator();
        if UrlConfig::_stored_base_url().is_none() {
            let theme = shell_theme();
            return rsx! {
                div {
                    style: format!(
                        "height:var(--gs26-app-height); display:flex; align-items:center; justify-content:center; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;",
                        theme.app_background,
                        theme.text_primary
                    ),
                    div {
                        style: shell_card_style(&theme, "min(560px, 92vw)"),
                        h1 { style: "margin:0 0 12px 0; font-size:18px;", "Not connected" }
                        p { style: "margin:0 0 16px 0; color:{theme.text_muted};", "Please configure the Ground Station URL on the Connect screen." }
                        button {
                            style: shell_button_style(&theme),
                            onclick: move |_| {
                                let _ = nav.replace(connect_route());
                            },
                            "Back to Connect"
                        }
                    }
                }
            };
        }
    }

    let base = UrlConfig::base_http();
    auth::init_from_storage(&base);
    let nav = use_navigator();
    let mut auth_state = use_signal(|| None::<Result<AuthSessionStatus, String>>);
    let mut auth_state_base = use_signal(String::new);
    use_effect(move || {
        let base = UrlConfig::base_http();
        if *auth_state_base.read() != base {
            auth_state_base.set(base.clone());
            auth_state.set(None);
        }
        let skip_tls = UrlConfig::_skip_tls_verify();
        if auth_state.read().is_some() {
            return;
        }
        spawn(async move {
            auth_state.set(Some(auth::fetch_session_status(&base, skip_tls).await));
        });
    });

    match auth_state.read().as_ref() {
        None => rsx! {
            div { style: format!("height:var(--gs26-app-height); display:flex; align-items:center; justify-content:center; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;", shell_theme().app_background, shell_theme().text_primary),
                div {
                    style: format!("padding:20px; border:1px solid {}; border-radius:16px; background:{}; min-width:min(560px, 92vw);", shell_theme().border_strong, shell_theme().panel_background),
                    h1 { style: "margin:0 0 10px 0; font-size:22px;", "Checking session..." }
                    p { style: format!("margin:0 0 16px 0; color:{};", shell_theme().text_muted), "Contacting the Ground Station session endpoint." }
                    div { style: "display:flex; gap:12px; justify-content:flex-end; flex-wrap:wrap;",
                        button {
                            style: shell_button_style(&shell_theme()),
                            onclick: move |_| {
                                let _ = nav.replace(connect_route());
                            },
                            "Cancel"
                        }
                    }
                }
            }
        },
        Some(Ok(status)) if status.permissions.view_data => {
            rsx! { crate::telemetry_dashboard::TelemetryDashboard {} }
        }
        Some(Ok(_)) => {
            #[cfg(target_arch = "wasm32")]
            {
                rsx! {
                    LoginCard {
                        title: "Sign In Required".to_string(),
                        subtitle: "This Ground Station does not allow anonymous view access. Sign in to continue.".to_string(),
                        allow_back_to_connect: true,
                        on_success_route: authenticated_route(),
                    }
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            if telemetry_dashboard::dashboard_has_prior_backend_connection() {
                rsx! {
                    LoginOverlay {
                        title: "Sign In Required".to_string(),
                        subtitle: "This Ground Station does not allow anonymous view access. Sign in to continue.".to_string(),
                        allow_back_to_connect: true,
                        on_success_route: authenticated_route(),
                    }
                }
            } else {
                rsx! {
                    LoginCard {
                        title: "Sign In Required".to_string(),
                        subtitle: "This Ground Station does not allow anonymous view access. Sign in to continue.".to_string(),
                        allow_back_to_connect: true,
                        on_success_route: authenticated_route(),
                    }
                }
            }
        }
        Some(Err(err)) => rsx! {
            ConnectionFailedCard {
                message: format_session_load_error(err),
                on_retry: move |_| {
                    let base = UrlConfig::base_http();
                    let skip_tls = UrlConfig::_skip_tls_verify();
                    auth_state.set(None);
                    spawn(async move {
                        auth_state.set(Some(auth::fetch_session_status(&base, skip_tls).await));
                    });
                },
            }
        },
    }
}
