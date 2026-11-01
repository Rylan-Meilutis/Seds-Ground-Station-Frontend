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
