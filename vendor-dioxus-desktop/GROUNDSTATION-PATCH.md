# GroundStation desktop navigation patch

Based on crates.io dioxus-desktop0.7.10; original licenses retained.
An optional explicit in-app HTTP(S) allow predicate is checked before the default
external-browser fallback. No predicate means upstream behavior. Existing
navigation_handler behavior is unchanged. The app restricts allowed documents
to its configured station origin and known media routes. This is needed because
upstream opens remote iframe navigation externally before invoking its public
custom navigation handler. No JavaScript dialog/native permissions are bypassed.
