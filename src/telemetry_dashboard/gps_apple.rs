// frontend/src/telemetry_dashboard/gps_apple.rs

use dioxus_signals::{Signal, WritableExt};
use std::sync::atomic::{AtomicU64, Ordering};

unsafe extern "C" {
    fn gs26_location_start(cb: extern "C" fn(f64, f64, f64));
    fn gs26_heading_start(cb: extern "C" fn(f64));
    fn gs26_location_auth_status() -> i32;
    fn gs26_last_horizontal_accuracy() -> f64;
    fn gs26_last_location_unix_s() -> f64;
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
/// Counts delivered locations so a one-shot request can wait for a new one.
static FIX_SEQ: AtomicU64 = AtomicU64::new(0);
/// A delivered location older than this is a cached reading, not a fresh fix.
const ONE_SHOT_MAX_AGE_S: f64 = 10.0;

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
    FIX_SEQ.fetch_add(1, Ordering::Release);

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

/// One explicit reading from CoreLocation. Starts (or re-kicks) the shared
/// location manager, which also triggers the permission prompt, then waits for
/// a newly delivered, recent location. CoreLocation keeps feeding the map
/// afterwards exactly as the dashboard's GPS driver already does.
#[cfg(any(target_os = "ios", target_os = "macos"))]
pub async fn one_shot(
    timeout: std::time::Duration,
) -> Result<crate::telemetry_dashboard::gps::DeviceFix, String> {
    const RESTRICTED: i32 = 1;
    const DENIED: i32 = 2;
    let start = FIX_SEQ.load(Ordering::Acquire);
    unsafe { gs26_location_start(on_loc) };
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match unsafe { gs26_location_auth_status() } {
            DENIED => {
                return Err("Location permission is denied for this app. Allow it in Settings > Privacy > Location Services, or enter coordinates.".into());
            }
            RESTRICTED => {
                return Err(
                    "Location access is restricted on this device. Enter coordinates manually."
                        .into(),
                );
            }
            _ => {}
        }
        if FIX_SEQ.load(Ordering::Acquire) != start {
            let fixed_at = unsafe { gs26_last_location_unix_s() };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(f64::NAN);
            let recent = (now - fixed_at).abs() <= ONE_SHOT_MAX_AGE_S;
            if let Some((latitude, longitude)) = latest_location().filter(|_| recent) {
                let accuracy = unsafe { gs26_last_horizontal_accuracy() };
                if let Some(fix) = (crate::telemetry_dashboard::gps::DeviceFix {
                    latitude,
                    longitude,
                    altitude_m: latest_altitude_m(),
                    accuracy_m: Some(accuracy),
                })
                .checked()
                {
                    return Ok(fix);
                }
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(
                "The device did not find its location in time. Try again or enter coordinates."
                    .into(),
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}
