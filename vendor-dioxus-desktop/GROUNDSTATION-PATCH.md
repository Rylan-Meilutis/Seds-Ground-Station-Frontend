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
