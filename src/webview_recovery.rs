//! Bring the app back when the platform kills or blanks the webview's content.
//!
//! Every native webview runs page content in a separate process that the OS can
//! end, usually while the app sits in the background. Left alone the window shows
//! only its background colour until the app is relaunched. The handlers here ask
//! dioxus-desktop to reload the app document (`request_webview_reload`); it then
//! starts a fresh virtual dom on the same route, and persisted state (session,
//! dashboard tab, settings) is read again on mount.
//!
//! - iOS/macOS: dioxus-desktop handles `webViewWebContentProcessDidTerminate:`
//!   through wry. `MediaWebViewShim.m` adds a foreground check: on becoming
//!   active, a page that does not answer or has an empty root is reloaded.
//! - Linux (WebKitGTK): the `web-process-terminated` signal.
//! - Windows (WebView2): the `ProcessFailed` event.
//! - Android: `RustWebViewClient.kt` handles `onRenderProcessGone` and the
//!   foreground check itself.

/// Why the WebKitGTK web process ended.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WebProcessEnd {
    Crashed,
    ExceededMemoryLimit,
    /// The app itself asked WebKit to end the process.
    TerminatedByApi,
    Unknown,
}

/// The reason to log and reload with, or `None` when the page must stay as it is.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn webkitgtk_reload_reason(end: WebProcessEnd) -> Option<&'static str> {
    match end {
        WebProcessEnd::Crashed => Some("WebKitGTK web process crashed"),
        WebProcessEnd::ExceededMemoryLimit => Some("WebKitGTK web process exceeded its memory limit"),
        WebProcessEnd::Unknown => Some("WebKitGTK web process ended"),
        WebProcessEnd::TerminatedByApi => None,
    }
}

/// WebView2 `COREWEBVIEW2_PROCESS_FAILED_KIND` values that blank the app page.
/// A reload restarts the renderer. A browser-process exit cannot be fixed by a
/// reload (the WebView2 instance itself is gone); iframe, GPU and helper process
/// exits are recovered by WebView2 or do not blank the page.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn webview2_reload_reason(kind: i32) -> Option<&'static str> {
    match kind {
        1 => Some("WebView2 render process exited"),
        2 => Some("WebView2 render process unresponsive"),
        _ => None,
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn webview2_browser_process_exited(kind: i32) -> bool {
    kind == 0
}

/// The app document was rebuilt in a new page: JavaScript helpers that Rust
/// installed into the old page, and latches whose tasks died with the old
/// virtual dom, start over. Runs when `App` mounts (once per document).
pub(crate) fn reset_document_scoped_state() {
    crate::telemetry_dashboard::reset_document_scoped_state();
}

#[cfg(not(target_arch = "wasm32"))]
static INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Install this window's content-process recovery. The webview outlives each
/// virtual dom, so it is installed once per process.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn use_webview_recovery() {
    use dioxus::prelude::*;
    let window = use_hook(try_consume_context::<dioxus_desktop::DesktopContext>);
    use_hook(move || {
        let Some(window) = window else {
            return;
        };
        if INSTALLED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        platform::install(&window.webview);
    });
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{WebProcessEnd, webkitgtk_reload_reason};
    use dioxus_desktop::wry::{WebView, WebViewExtUnix as _};
    use webkit2gtk::{WebProcessTerminationReason, WebViewExt as _};

    pub(super) fn install(webview: &WebView) {
        webview.webview().connect_web_process_terminated(|_, reason| {
            let end = match reason {
                WebProcessTerminationReason::Crashed => WebProcessEnd::Crashed,
                WebProcessTerminationReason::ExceededMemoryLimit => WebProcessEnd::ExceededMemoryLimit,
                WebProcessTerminationReason::TerminatedByApi => WebProcessEnd::TerminatedByApi,
                _ => WebProcessEnd::Unknown,
            };
            if let Some(reason) = webkitgtk_reload_reason(end) {
                dioxus_desktop::request_webview_reload(reason);
            }
        });
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{webview2_browser_process_exited, webview2_reload_reason};
    use dioxus_desktop::wry::{WebView, WebViewExtWindows as _};
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_KIND;
    use webview2_com::ProcessFailedEventHandler;

    pub(super) fn install(webview: &WebView) {
        let core = webview.webview();
        let handler = ProcessFailedEventHandler::create(Box::new(|_sender, args| {
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            if let Some(args) = args {
                unsafe { args.ProcessFailedKind(&mut kind)? };
            }
            if let Some(reason) = webview2_reload_reason(kind.0) {
                dioxus_desktop::request_webview_reload(reason);
            } else if webview2_browser_process_exited(kind.0) {
                crate::debug_log::append(
                    "[webview] WebView2 browser process exited; restart the app to recover",
                );
            }
            Ok(())
        }));
        let mut token = 0i64;
        if let Err(err) = unsafe { core.add_ProcessFailed(&handler, &mut token) } {
            crate::debug_log::append(&format!("[webview] could not watch WebView2 process failures: {err}"));
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod platform {
    use dioxus_desktop::wry::WebView;
    #[cfg(target_os = "ios")]
    use dioxus_desktop::wry::WebViewExtIOS as _;
    #[cfg(target_os = "macos")]
    use dioxus_desktop::wry::WebViewExtMacOS as _;
    use std::ffi::{CStr, c_char, c_void};

    unsafe extern "C" {
        fn gs26_webview_install_recovery(webview: *mut c_void);
    }

    pub(super) fn install(webview: &WebView) {
        let view = webview.webview();
        let pointer: *const _ = &*view;
        unsafe { gs26_webview_install_recovery(pointer as *mut c_void) };
    }

    /// Called by `MediaWebViewShim.m` on the main thread when the page came back
    /// blank or unresponsive after the app became active.
    #[unsafe(no_mangle)]
    pub extern "C" fn gs26_webview_request_reload(reason: *const c_char) {
        let reason = if reason.is_null() {
            "page blank after returning to the foreground".to_string()
        } else {
            unsafe { CStr::from_ptr(reason) }.to_string_lossy().into_owned()
        };
        dioxus_desktop::request_webview_reload(&reason);
    }
}

#[cfg(target_os = "android")]
mod platform {
    // RustWebViewClient.kt handles onRenderProcessGone and the resume check.
    pub(super) fn install(_webview: &dioxus_desktop::wry::WebView) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webkitgtk_reloads_after_crashes_but_not_deliberate_termination() {
        assert!(webkitgtk_reload_reason(WebProcessEnd::Crashed).is_some());
        assert!(webkitgtk_reload_reason(WebProcessEnd::ExceededMemoryLimit).is_some());
        assert!(webkitgtk_reload_reason(WebProcessEnd::Unknown).is_some());
        assert_eq!(webkitgtk_reload_reason(WebProcessEnd::TerminatedByApi), None);
    }

    #[test]
    fn webview2_reloads_only_when_the_page_renderer_failed() {
        assert!(webview2_reload_reason(1).is_some(), "render process exited");
        assert!(webview2_reload_reason(2).is_some(), "render process unresponsive");
        for kind in [0, 3, 4, 5, 6, 7, 8, 9] {
            assert_eq!(webview2_reload_reason(kind), None, "kind {kind}");
        }
        assert!(webview2_browser_process_exited(0));
        assert!(!webview2_browser_process_exited(1));
    }
}
