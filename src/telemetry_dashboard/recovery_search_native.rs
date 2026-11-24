//! Platform glue for recording a recovery search track with the app in the
//! background (docs/proximity-lock.md, "Recovery search in the background").
//! The decision is `recovery_search::background_recording`: only while this
//! device shares (a running search and the user's own opt-in) and the OS
//! location permission is granted. `recovery_search_view` starts and stops
//! the native recorder from that decision and takes its fixes every second.
//!
//! * iOS: `assets/SearchTrackShim.m`: its own CLLocationManager with
//!   background location updates (`UIBackgroundModes` `location`, the blue
//!   location indicator shown) while sharing; fixes wait in a bounded buffer
//!   in the shim.
//! * Android: `SearchTrackBridge` / `SearchTrackService` (Kotlin): a
//!   foreground service of type `location` with an ongoing "Recovery search"
//!   notification (and a "Stop sharing" action); fused location fixes come
//!   back through JNI into a bounded queue here.
//! * Desktop and web: no background recording; the view records while open.
//!
//! The native code never sends anything. Fixes reach the station only
//! through the same thinning, persisted outbox and upload as foreground
//! fixes, so nothing is shared that the opt-in does not cover.
#![allow(dead_code)]

use super::recovery_search::{MAX_PENDING, RawFix};
use std::collections::VecDeque;

/// Fixes the native layer recorded while the app may have been in the
/// background, waiting for the runtime loop (bounded, oldest dropped).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FixQueue {
    fixes: VecDeque<RawFix>,
    pub dropped: u64,
}

impl FixQueue {
    pub(crate) const fn new() -> Self {
        Self { fixes: VecDeque::new(), dropped: 0 }
    }
    pub(crate) fn push(&mut self, fix: RawFix) {
        self.fixes.push_back(fix);
        while self.fixes.len() > MAX_PENDING {
            self.fixes.pop_front();
            self.dropped += 1;
        }
    }
    /// Everything queued, oldest first.
    pub(crate) fn take(&mut self) -> Vec<RawFix> {
        let mut out: Vec<RawFix> = self.fixes.drain(..).collect();
        out.sort_by_key(|f| f.3);
        out
    }
    pub(crate) fn clear(&mut self) {
        self.fixes.clear();
    }
    pub(crate) fn len(&self) -> usize {
        self.fixes.len()
    }
}

#[cfg(target_os = "ios")]
mod imp {
    use super::*;
    use std::ffi::c_int;
    unsafe extern "C" {
        fn gs26_search_bg_start();
        fn gs26_search_bg_stop();
        fn gs26_search_bg_take(out: *mut f64, max_fixes: c_int) -> c_int;
    }

    const CHUNK: usize = 256;

    pub fn set_running(on: bool) {
        if on {
            unsafe { gs26_search_bg_start() }
        } else {
            unsafe { gs26_search_bg_stop() }
        }
    }

    pub fn take() -> Vec<RawFix> {
        let mut out = Vec::new();
        let mut buf = vec![f64::NAN; CHUNK * 4];
        loop {
            let n = unsafe { gs26_search_bg_take(buf.as_mut_ptr(), CHUNK as c_int) }.clamp(0, CHUNK as c_int) as usize;
            for f in buf[..n * 4].chunks_exact(4) {
                let acc = f[2].is_finite().then_some(f[2]).filter(|a| *a >= 0.0);
                if f[3].is_finite() {
                    out.push((f[0], f[1], acc, (f[3] * 1000.0) as i64));
                }
            }
            if n < CHUNK {
                break;
            }
        }
        out.sort_by_key(|f| f.3);
        out
    }

    /// iOS has no notification action: the app's own "Stop sharing" is used.
    pub fn take_stop_request() -> bool {
        false
    }
}

#[cfg(target_os = "android")]
mod imp {
    use super::*;
    use ::jni::EnvUnowned;
    use ::jni::objects::JClass;
    use ::jni::sys::{jdouble, jlong};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    const CLASS: &str = "com.ubseds.gs26.SearchTrackBridge";
    static QUEUE: Mutex<FixQueue> = Mutex::new(FixQueue::new());
    static RUNNING: AtomicBool = AtomicBool::new(false);
    static STOP: AtomicBool = AtomicBool::new(false);

    pub fn set_running(on: bool) {
        RUNNING.store(on, Ordering::Release);
        let method = if on { "start" } else { "stop" };
        let _ = crate::telemetry_dashboard::proximity_native::call_class(
            CLASS,
            method,
            "(Landroid/content/Context;)V",
            &[],
            |_, _| Some(()),
        );
        if !on && let Ok(mut queue) = QUEUE.lock() {
            queue.clear();
        }
    }

    pub fn take() -> Vec<RawFix> {
        QUEUE.lock().map(|mut q| q.take()).unwrap_or_default()
    }

    pub fn take_stop_request() -> bool {
        STOP.swap(false, Ordering::AcqRel)
    }

    /// A fused location fix from `SearchTrackService` (any thread). Kept only
    /// while Rust asked for background recording.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_ubseds_gs26_SearchTrackBridge_nativeOnSearchFix(
        _env: EnvUnowned<'_>,
        _class: JClass<'_>,
        lat: jdouble,
        lon: jdouble,
        accuracy_m: jdouble,
        fix_time_ms: jlong,
    ) {
        if !RUNNING.load(Ordering::Acquire) {
            return;
        }
        let acc = accuracy_m.is_finite().then_some(accuracy_m).filter(|a| *a >= 0.0);
        if let Ok(mut queue) = QUEUE.lock() {
            queue.push((lat, lon, acc, fix_time_ms));
        }
    }

    /// "Stop sharing" in the service's notification: the runtime opts out.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_ubseds_gs26_SearchTrackBridge_nativeOnStopRequested(
        _env: EnvUnowned<'_>,
        _class: JClass<'_>,
    ) {
        STOP.store(true, Ordering::Release);
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod imp {
    use super::*;
    pub fn set_running(_on: bool) {}
    pub fn take() -> Vec<RawFix> {
        Vec::new()
    }
    pub fn take_stop_request() -> bool {
        false
    }
}

/// Whether this platform can record in the background at all.
pub(crate) const SUPPORTED: bool = cfg!(any(target_os = "ios", target_os = "android"));

/// Starts or stops the native background recorder.
pub(crate) fn set_running(on: bool) {
    imp::set_running(on)
}

/// The fixes the native recorder took since the last call, oldest first.
pub(crate) fn take() -> Vec<RawFix> {
    imp::take()
}

/// The user pressed "Stop sharing" outside the app (Android notification).
pub(crate) fn take_stop_request() -> bool {
    imp::take_stop_request()
}

/// The OS location permission (the same one the proximity lock reads).
pub(crate) fn permission() -> super::proximity_lock::Permission {
    super::proximity_native::permission()
}

/// Asks for "while using" location when it was never asked (from the user's
/// own opt-in). Never asks for background ("Always") location.
pub(crate) fn request_permission_if_needed() {
    if SUPPORTED && permission() == super::proximity_lock::Permission::NotDetermined {
        super::proximity_native::request_permission(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_builds_never_record_in_the_background() {
        assert_eq!(SUPPORTED, cfg!(any(target_os = "ios", target_os = "android")));
        if !SUPPORTED {
            set_running(true);
            assert!(take().is_empty());
            assert!(!take_stop_request());
            set_running(false);
        }
    }

    #[test]
    fn the_native_queue_is_bounded_and_drained_in_time_order() {
        let mut queue = FixQueue::new();
        queue.push((31.0, -102.0, None, 3));
        queue.push((31.0, -102.0, None, 1));
        assert_eq!(queue.take().iter().map(|f| f.3).collect::<Vec<_>>(), vec![1, 3]);
        assert_eq!(queue.len(), 0);
        for i in 0..MAX_PENDING as i64 + 3 {
            queue.push((31.0, -102.0, None, i));
        }
        assert_eq!((queue.len(), queue.dropped), (MAX_PENDING, 3));
        assert_eq!(queue.take()[0].3, 3, "the oldest went first");
        queue.push((31.0, -102.0, None, 1));
        queue.clear();
        assert!(queue.take().is_empty());
    }
}
