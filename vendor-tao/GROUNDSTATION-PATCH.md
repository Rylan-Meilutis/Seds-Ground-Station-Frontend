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

## Linux main-context channels

Window requests and X11 device events use `async_channel::unbounded`, with
nonblocking `try_send` producers and `MainContext::spawn_local` consumers. This
replaces the deprecated GLib Sender/Receiver API rather than suppressing warnings.
GTK callbacks remain on their owning main-context thread, and FIFO delivery and
closed-channel handling are preserved. A consumer yields at idle priority after
every 64 events so a sustained producer cannot monopolize the GTK main loop.

Headless main-context tests exercise cross-thread wakeup, FIFO bursts, channel
closure, callback cancellation and fairness. Compile/test both default X11 and
`--no-default-features --features rwh_06` Linux configurations when updating this patch.

Validated on ARM64 Linux with Rust 1.99.1: all five tests pass under Xvfb,
including actual cross-thread GTK window updates. Without X11, four channel
tests pass (the display test is intentionally ignored). Neither build emits
compiler warnings. macOS and WebAssembly frontend checks also pass.

```sh
xvfb-run -a cargo test --manifest-path vendor-tao/Cargo.toml --lib platform_impl::platform::channel::tests -- --include-ignored --test-threads=1
cargo test --manifest-path vendor-tao/Cargo.toml --no-default-features --features rwh_06 --lib platform_impl::platform::channel::tests -- --test-threads=1
```
