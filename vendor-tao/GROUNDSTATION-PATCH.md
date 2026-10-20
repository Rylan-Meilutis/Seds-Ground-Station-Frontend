# GroundStation iOS scroll-update patch

Source: crates.io Tao 0.34.8, retaining upstream licenses and source.
The original functional patch changes src/platform_impl/ios/event_loop.rs.
Additional macOS maintenance removes unused private helpers/state, uses typed
NSEvent/NSWindow constants and NSURL pasteboard reads, and removes unnecessary
unsafe blocks. The public windowing API and iOS tracking-mode behavior remain.

The three control-flow observers use common modes, like Tao's existing event
source and wakeup timer, so UITrackingRunLoopMode does not starve user events
and redraws while a scroll is active. Mode entry is a no-op rather than a panic;
normal wakeup/before-waiting transitions remain unchanged.

Reference: https://developer.apple.com/documentation/corefoundation/cfrunloop

Device validation: hold and drag a long dashboard page for at least 30 seconds
while receiving changing telemetry; confirm the clock and readings continue,
release into momentum scrolling, repeat in both directions, and background/resume.
Also test modal presentation during scrolling for nested run-loop regressions.
An iOS compile check alone does not validate this behavior. Re-evaluate this patch
on every Tao/Dioxus update and remove it once the upstream dependency handles
tracking mode.

## Linux build diagnostics

The two legacy GLib channel constructors locally allow `deprecated`, preserving
Tao's existing GLib Sender/Receiver window and X11 event delivery. Other compiler
warnings remain enabled. Remove these allowances when the Linux channel plumbing
is migrated with upstream Tao.
