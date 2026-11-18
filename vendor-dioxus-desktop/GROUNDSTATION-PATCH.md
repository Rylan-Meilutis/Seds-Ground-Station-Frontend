# GroundStation desktop navigation patch

Based on crates.io `dioxus-desktop` 0.7.10. `LICENSE-MIT` and
`LICENSE-APACHE` are retained from the upstream `v0.7.10` tag:
https://github.com/DioxusLabs/dioxus/tree/v0.7.10
The original package manifest declares `MIT OR Apache-2.0`.
An optional explicit in-app HTTP(S) allow predicate is checked before the default
external-browser fallback. No predicate means upstream behavior. Existing
navigation_handler behavior is unchanged. The app restricts allowed documents
to its configured station origin and known media routes. This is needed because
upstream opens remote iframe navigation externally before invoking its public
custom navigation handler. No JavaScript dialog/native permissions are bypassed.

Ground Station disables system-browser launches per native window using
`with_external_browser_enabled(false)`. The policy covers HTTP/mailto navigation
fallbacks and browser-open IPC anchors; unknown or closed IPC window IDs cannot
open the browser. Exact trusted station documents retain the in-app navigation
allowlist. Other consumers retain the vendor default of true.

Private menu/tray state is compiled only for the desktop platforms that consume
it. The public tray-click builder remains available as a no-op on platforms
without tray support. Developer hot-reload toast code and its imports share the
same debug/devtools gate as their callers; application notifications are not
changed. The unused mobile menu-icon alias is omitted while mobile menu APIs
remain available.

Web content process recovery. WebKit, WebView2 and the Android WebView may end
the web content process (usually while the app is in the background), leaving
the window showing only its background colour. `recovery::request_webview_reload`
and the macOS/iOS `webViewWebContentProcessDidTerminate:` hook (through wry's
`with_on_web_content_process_terminate_handler`) send `RecoverWebview`; the
window logs the line `[webview] content process terminated, reloading (...)`
(also to `Config::with_webview_recovery_logger`) and reloads the app document.
The "no navigation after the first load" policy lets exactly one armed reload
through. Upstream would rebuild the live virtual dom into the new, empty page
with a stale template cache and replay unacknowledged edits; instead each served
document restarts the edit stream (new connection id, empty template cache, no
pending acknowledgement), rendering pauses until the page initializes, and the
main window then gets a fresh virtual dom from the launch recipe on the route
the old one showed. The edit server also publishes its new location before
waking webviews after iOS reclaims its socket (upstream notified first).
