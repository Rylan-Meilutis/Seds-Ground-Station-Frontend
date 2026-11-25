//! Native WebView set-up for the station's voice, camera and recording pages.
//!
//! The web build loads these pages from the station's own origin. Native
//! builds show them in iframes inside the app's document, so each WebView
//! needs what a browser tab gets for free:
//!
//! - the iframe navigation must be allowed for the station the dashboard is
//!   actually showing (saved or session-only);
//! - WebRTC, getUserMedia and Media Source must be enabled, and capture
//!   permission requests answered (WebKitGTK disables or denies all of them by
//!   default);
//! - a station whose HTTPS certificate the operator chose not to verify must
//!   be trusted by the WebView too, not only by the Rust HTTP client;
//! - the app document itself must be a secure context so the iframe can be one
//!   (WebView2 serves it from `http://dioxus.index.html`).
//!
//! Android's WebView settings and certificate handling live in the Kotlin
//! overlay (`platform/android`), and Apple's certificate handling in
//! `assets/MediaWebViewShim.m`.

use std::time::Duration;

/// Origin of the document WebView2 serves the app from.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) const WINDOWS_APP_ORIGIN: &str = "http://dioxus.index.html";

/// WebView2 browser arguments. They replace wry's defaults, so those are kept.
///
/// `--unsafely-treat-insecure-origin-as-secure` makes the app document a
/// secure context (a nested frame is secure only when its ancestors are), and
/// lets a plain-HTTP station's media pages use the microphone and camera like
/// a localhost page. When the operator skips TLS verification for an HTTPS
/// station, the WebView skips it too.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn windows_browser_args(base: &str, skip_tls: bool) -> String {
    let mut args = String::from(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required",
    );
    let mut secure = vec![WINDOWS_APP_ORIGIN.to_string()];
    let station = url::Url::parse(base).ok();
    if let Some(url) = station.as_ref().filter(|url| url.scheme() == "http" && url.host_str().is_some()) {
        let origin = url.origin().ascii_serialization();
        if !secure.contains(&origin) {
            secure.push(origin);
        }
    }
    args.push_str(" --unsafely-treat-insecure-origin-as-secure=");
    args.push_str(&secure.join(","));
    if skip_tls && station.as_ref().is_some_and(|url| url.scheme() == "https") {
        args.push_str(" --ignore-certificate-errors");
    }
    args
}

/// Host of an HTTPS station whose certificate the WebView should be taught.
pub(crate) fn pinned_tls_host(base: &str, skip_tls: bool) -> Option<String> {
    if !skip_tls {
        return None;
    }
    let url = url::Url::parse(base).ok()?;
    (url.scheme() == "https").then(|| url.host_str().map(str::to_ascii_lowercase)).flatten()
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn der_to_pem(der: &[u8]) -> String {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(der);
    let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
    for line in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(line).unwrap_or_default());
        pem.push('\n');
    }
    pem.push_str("-----END CERTIFICATE-----\n");
    pem
}

/// Why a station media page cannot load in this app, worded for the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreflightIssue {
    UntrustedCertificate,
    Unreachable,
    Status(u16),
}

pub(crate) fn preflight_message(issue: &PreflightIssue, title: &str) -> String {
    match issue {
        PreflightIssue::UntrustedCertificate => format!(
            "{title} cannot load: this device does not trust the station's HTTPS certificate. \
             In Connection settings, enable skipping TLS verification for this station, \
             or install the station's certificate on this device."
        ),
        PreflightIssue::Unreachable => format!(
            "{title} cannot load: the station did not answer. Check the station address and network."
        ),
        PreflightIssue::Status(status) => {
            format!("{title} cannot load: the station answered HTTP {status}.")
        }
    }
}

fn error_mentions_certificate(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(err) = current {
        let text = err.to_string().to_ascii_lowercase();
        if text.contains("certificate") || text.contains("unknownissuer") || text.contains("self-signed") || text.contains("self signed") {
            return true;
        }
        current = err.source();
    }
    false
}

/// Loads a station page the way the WebView will, to explain a blank frame.
pub(crate) fn preflight_station_document(base: &str, path: &str, skip_tls: bool) -> Result<(), PreflightIssue> {
    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(skip_tls)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| PreflightIssue::Unreachable)?;
    let url = format!("{}{path}", base.trim_end_matches('/'));
    match client.get(url).send() {
        Ok(response) if response.status().is_success() => Ok(()),
        Ok(response) => Err(PreflightIssue::Status(response.status().as_u16())),
        Err(error) if error_mentions_certificate(&error) => Err(PreflightIssue::UntrustedCertificate),
        Err(_) => Err(PreflightIssue::Unreachable),
    }
}

/// Reads the certificate an HTTPS station presents, without verifying it.
/// Used only for a station the operator chose not to verify.
pub(crate) fn fetch_station_certificate(base: &str) -> Option<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .tls_info(true)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?;
    let response = client.get(format!("{}/radio", base.trim_end_matches('/'))).send().ok()?;
    response
        .extensions()
        .get::<reqwest::tls::TlsInfo>()
        .and_then(|info| info.peer_certificate())
        .map(<[u8]>::to_vec)
}

#[cfg(target_os = "linux")]
mod platform {
    use dioxus_desktop::wry::{WebView, WebViewExtUnix as _};
    use webkit2gtk::glib::prelude::*;
    use webkit2gtk::{
        DeviceInfoPermissionRequest, PermissionRequestExt as _,
        UserMediaPermissionRequest, WebContextExt as _, WebViewExt as _,
    };

    /// Capture permission is granted only for microphone/camera and device
    /// listing; the page asks only after the operator chooses Join voice or
    /// Start sharing, and navigation is restricted to station documents.
    pub(super) const PINS_CERTIFICATES: bool = true;

    pub(super) fn grants_permission(request: &webkit2gtk::PermissionRequest) -> bool {
        request.is::<UserMediaPermissionRequest>() || request.is::<DeviceInfoPermissionRequest>()
    }

    pub(super) fn configure(webview: &WebView) {
        let view = webview.webview();
        if let Some(settings) = view.settings() {
            super::apply_webkitgtk_media_settings(&settings);
        }
        view.connect_permission_request(|_, request| {
            if grants_permission(request) {
                request.allow();
                true
            } else {
                false
            }
        });
    }

    pub(super) fn trust_certificate(webview: &WebView, host: &str, der: &[u8]) -> bool {
        let pem = super::der_to_pem(der);
        let Ok(certificate) = webkit2gtk::gio::TlsCertificate::from_pem(&pem) else {
            return false;
        };
        let Some(context) = webview.webview().context() else {
            return false;
        };
        context.allow_tls_certificate_for_host(&certificate, host);
        true
    }
}

/// WebKitGTK ships WebRTC and getUserMedia disabled; Media Source (hls.js)
/// and inline, gesture-free playback match the browser build. These are
/// WebKitSettings property names.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const WEBKITGTK_MEDIA_SETTINGS: [(&str, bool); 8] = [
    ("enable-media", true),
    ("enable-mediasource", true),
    ("enable-media-capabilities", true),
    ("enable-media-stream", true),
    ("enable-webrtc", true),
    ("enable-webaudio", true),
    ("media-playback-requires-user-gesture", false),
    ("media-playback-allows-inline", true),
];

#[cfg(target_os = "linux")]
pub(crate) fn apply_webkitgtk_media_settings(settings: &webkit2gtk::Settings) {
    use webkit2gtk::glib::prelude::*;
    for (name, enabled) in WEBKITGTK_MEDIA_SETTINGS {
        settings.set_property(name, enabled);
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod platform {
    use dioxus_desktop::wry::WebView;
    #[cfg(target_os = "ios")]
    use dioxus_desktop::wry::WebViewExtIOS as _;
    #[cfg(target_os = "macos")]
    use dioxus_desktop::wry::WebViewExtMacOS as _;
    use std::ffi::{CString, c_char, c_void};

    unsafe extern "C" {
        fn gs26_webview_install_media_policy(webview: *mut c_void);
        fn gs26_webview_trust_certificate(host: *const c_char, der: *const u8, len: usize);
    }

    pub(super) const PINS_CERTIFICATES: bool = true;

    pub(super) fn configure(webview: &WebView) {
        // wry already enables inline playback, gesture-free autoplay and grants
        // WKUIDelegate media-capture requests (Info.plist carries the usage
        // strings). Add the pinned-certificate challenge handler.
        let view = webview.webview();
        let pointer: *const _ = &*view;
        unsafe { gs26_webview_install_media_policy(pointer as *mut c_void) };
    }

    pub(super) fn trust_certificate(_webview: &WebView, host: &str, der: &[u8]) -> bool {
        let Ok(host) = CString::new(host) else {
            return false;
        };
        unsafe { gs26_webview_trust_certificate(host.as_ptr(), der.as_ptr(), der.len()) };
        true
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios")))]
mod platform {
    // Android: RustWebView settings and onReceivedSslError are in the Kotlin
    // overlay. Windows: browser arguments are chosen at launch.
    use dioxus_desktop::wry::WebView;
    pub(super) const PINS_CERTIFICATES: bool = false;
    pub(super) fn configure(_webview: &WebView) {}
    pub(super) fn trust_certificate(_webview: &WebView, _host: &str, _der: &[u8]) -> bool {
        false
    }
}

static CONFIGURED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Prepares this window's WebView for the station media pages and records the
/// live station for iframe navigation. Call before mounting the iframes.
pub(crate) fn use_station_media_webview(base: String) {
    use dioxus::prelude::*;
    crate::native_navigation::remember_live_station(&base);
    // iOS: report the audio session and keep crew voice off the earpiece.
    crate::native_voice_audio::install();
    // Server-side renders and previews have no desktop window to configure.
    let window = use_hook(try_consume_context::<dioxus_desktop::DesktopContext>);
    use_hook(|| {
        if let Some(window) = window.as_ref()
            && !CONFIGURED.swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            platform::configure(&window.webview);
        }
    });
    use_effect(use_reactive((&base,), move |(base,)| {
        crate::native_navigation::remember_live_station(&base);
        let Some(window) = window.clone() else {
            return;
        };
        let skip_tls = crate::telemetry_dashboard::persisted_skip_tls_for_base_for_native_io(&base);
        let Some(host) = pinned_tls_host(&base, skip_tls).filter(|_| platform::PINS_CERTIFICATES) else {
            return;
        };
        spawn(async move {
            let (tx, rx) = futures_channel::oneshot::channel();
            let station = base.clone();
            let _ = std::thread::Builder::new()
                .name("gs26-media-cert".into())
                .spawn(move || {
                    let _ = tx.send(fetch_station_certificate(&station));
                });
            let Ok(Some(der)) = rx.await else {
                crate::debug_log::append("[media] could not read the station certificate");
                return;
            };
            if platform::trust_certificate(&window.webview, &host, &der) {
                crate::debug_log::append("[media] station certificate trusted for media pages");
                // Frames that already failed the TLS handshake load again.
                document::eval(
                    "for (const frame of document.querySelectorAll('#gs26-media,#gs26-crew-voice,.gs26-program-player')) { const src = frame.src; frame.src = 'about:blank'; frame.src = src; }",
                );
            }
        });
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_app_document_and_http_station_are_secure_contexts() {
        let args = windows_browser_args("http://192.168.1.20:3000", false);
        assert!(args.contains("--autoplay-policy=no-user-gesture-required"));
        assert!(args.contains("msSmartScreenProtection"), "wry's defaults are kept");
        assert!(args.contains(
            "--unsafely-treat-insecure-origin-as-secure=http://dioxus.index.html,http://192.168.1.20:3000"
        ));
        assert!(!args.contains("--ignore-certificate-errors"));
    }

    #[test]
    fn windows_skips_tls_only_for_an_unverified_https_station() {
        let verified = windows_browser_args("https://gs.local:3443", false);
        assert!(verified.ends_with("--unsafely-treat-insecure-origin-as-secure=http://dioxus.index.html"));
        assert!(!verified.contains("--ignore-certificate-errors"));
        assert!(windows_browser_args("https://gs.local:3443", true).contains(" --ignore-certificate-errors"));
        assert!(!windows_browser_args("http://gs.local:3000", true).contains("--ignore-certificate-errors"));
        // A malformed address adds nothing that could split the argument list.
        let malformed = windows_browser_args("not a url --remote-debugging-port=1", true);
        assert!(!malformed.contains("remote-debugging"));
    }

    #[test]
    fn only_unverified_https_stations_get_a_pinned_certificate() {
        assert_eq!(pinned_tls_host("https://GS.Local:3443", true).as_deref(), Some("gs.local"));
        assert_eq!(pinned_tls_host("https://gs.local:3443", false), None);
        assert_eq!(pinned_tls_host("http://gs.local:3000", true), None);
        assert_eq!(pinned_tls_host("garbage", true), None);
    }

    #[test]
    fn certificate_pem_is_wrapped_base64() {
        let pem = der_to_pem(&[0u8; 100]);
        assert!(pem.starts_with("-----BEGIN CERTIFICATE-----\n"));
        assert!(pem.ends_with("-----END CERTIFICATE-----\n"));
        assert!(pem.lines().all(|line| line.len() <= 64));
    }

    #[test]
    fn preflight_messages_explain_a_blank_media_frame() {
        let tls = preflight_message(&PreflightIssue::UntrustedCertificate, "Voice chat");
        assert!(tls.starts_with("Voice chat cannot load"));
        assert!(tls.contains("skipping TLS verification"));
        assert!(preflight_message(&PreflightIssue::Status(404), "Cameras").contains("HTTP 404"));
        assert!(preflight_message(&PreflightIssue::Unreachable, "Cameras").contains("did not answer"));
    }

    #[test]
    fn preflight_reports_an_unreachable_station() {
        // Port 9 on the loopback interface is closed on test hosts.
        assert_eq!(
            preflight_station_document("http://127.0.0.1:9", "/radio", false),
            Err(PreflightIssue::Unreachable)
        );
    }

    #[test]
    fn webkitgtk_enables_webrtc_capture_and_mse() {
        let settings: std::collections::HashMap<_, _> = WEBKITGTK_MEDIA_SETTINGS.into_iter().collect();
        for name in ["enable-media-stream", "enable-webrtc", "enable-mediasource", "enable-media", "media-playback-allows-inline"] {
            assert_eq!(settings.get(name), Some(&true), "{name}");
        }
        assert_eq!(settings.get("media-playback-requires-user-gesture"), Some(&false));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn webkitgtk_media_settings_are_real_boolean_properties() {
        // Class lookup needs no display; a misspelt name would panic in set_property.
        use webkit2gtk::glib::{self, prelude::*};
        let class = glib::object::ObjectClass::from_type(webkit2gtk::Settings::static_type()).unwrap();
        for (name, _) in WEBKITGTK_MEDIA_SETTINGS {
            let property = class.find_property(name).unwrap_or_else(|| panic!("WebKitSettings has no {name}"));
            assert_eq!(property.value_type(), bool::static_type(), "{name}");
            assert!(property.flags().contains(glib::ParamFlags::WRITABLE), "{name}");
        }
    }
}
