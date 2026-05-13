// Native connection support.
//
// The connect screen needs real route probes, WebSocket handshakes, platform
// TLS behavior, and a tiny persistence layer. Keeping that code out of
// `app.rs` leaves the route components easier to scan.

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
        crate::native_storage::android_files_dir().map(|path| path.join("gs26"))
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

    /// Reads whether the user has already completed the native connect flow before.
    pub fn read_connect_shown() -> bool {
        matches!(_read_key(_CONNECT_SHOWN_KEY).as_deref(), Some("true"))
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
fn connect_scheme_supports_skip_tls(scheme: &str) -> bool {
    scheme == "https://"
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
        path: "/api/auth/challenge",
        method: "POST",
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
        ("POST", "/api/auth/challenge") => match status {
            200 | 400 | 401 | 403 | 415 => (true, "reachable (auth challenge endpoint responded)"),
            _ => (false, "unexpected status for auth challenge"),
        },
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
    };
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
