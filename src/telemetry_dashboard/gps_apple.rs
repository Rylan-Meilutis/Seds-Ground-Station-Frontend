// frontend/src/telemetry_dashboard/gps_apple.rs

use dioxus_signals::{Signal, WritableExt};
use std::sync::atomic::{AtomicU64, Ordering};

unsafe extern "C" {
    fn gs26_location_start(cb: extern "C" fn(f64, f64, f64));
    fn gs26_heading_start(cb: extern "C" fn(f64));
    // Optional if you have it:
    // fn gs26_location_stop();
}

// NOTE:
// We intentionally keep this as `static mut` and ONLY use it to *best-effort* write.
// We never unwrap in the callback, and we detach it in `stop()`.
static mut GPS_SIGNAL: Option<Signal<Option<(f64, f64)>>> = None;
static LAT_BITS: AtomicU64 = AtomicU64::new(f64::NAN.to_bits());
static LON_BITS: AtomicU64 = AtomicU64::new(f64::NAN.to_bits());
static GPS_ALT_BITS: AtomicU64 = AtomicU64::new(f64::NAN.to_bits());
static HEADING_BITS: AtomicU64 = AtomicU64::new(f64::NAN.to_bits());

extern "C" fn on_loc(lat: f64, lon: f64, alt_m: f64) {
    if lat.is_finite() && lon.is_finite() {
        LAT_BITS.store(lat.to_bits(), Ordering::Relaxed);
        LON_BITS.store(lon.to_bits(), Ordering::Relaxed);
    }
    if alt_m.is_finite() {
        GPS_ALT_BITS.store(alt_m.to_bits(), Ordering::Relaxed);
    } else {
        GPS_ALT_BITS.store(f64::NAN.to_bits(), Ordering::Relaxed);
    }

    unsafe {
        let Some(mut sig) = GPS_SIGNAL else { return };

        // Never `sig.set(...)` here; it can panic if dropped.
        if let Ok(mut w) = sig.try_write() {
            *w = Some((lat, lon));
        } else {
            // Dropped/unmounted; ignore late callback
        }
    }
}

extern "C" fn on_heading(deg: f64) {
    if deg.is_finite() {
        HEADING_BITS.store(deg.to_bits(), Ordering::Relaxed);
    }
}

/// Start feeding CoreLocation updates into `user_gps`.
/// Safe to call multiple times; it replaces the stored signal.
pub fn start(user_gps: Signal<Option<(f64, f64)>>) {
    unsafe {
        GPS_SIGNAL = Some(user_gps);
        gs26_location_start(on_loc);
        gs26_heading_start(on_heading);
    }
}

/// Detach from UI so late callbacks can't panic.
pub fn stop() {
    unsafe {
        GPS_SIGNAL = None;
        // Optional:
        // gs26_location_stop();
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[allow(dead_code)]
pub fn latest_location() -> Option<(f64, f64)> {
    let lat = f64::from_bits(LAT_BITS.load(Ordering::Relaxed));
    let lon = f64::from_bits(LON_BITS.load(Ordering::Relaxed));
    if lat.is_finite() && lon.is_finite() {
        Some((lat, lon))
    } else {
        None
    }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
pub fn latest_altitude_m() -> Option<f64> {
    let gps = f64::from_bits(GPS_ALT_BITS.load(Ordering::Relaxed));
    if gps.is_finite() { Some(gps) } else { None }
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
#[allow(dead_code)]
pub fn latest_heading_deg() -> Option<f64> {
    let v = f64::from_bits(HEADING_BITS.load(Ordering::Relaxed));
    if v.is_finite() { Some(v) } else { None }
}
