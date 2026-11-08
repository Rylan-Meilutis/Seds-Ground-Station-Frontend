// frontend/src/telemetry_dashboard/gps.rs
#![allow(dead_code)]

use dioxus::prelude::*;
use dioxus_signals::Signal;

const NATIVE_ALTITUDE_SYNC_MS: u64 = 250;

/// One operating-system location reading for an explicit operator request.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceFix {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: Option<f64>,
    pub accuracy_m: Option<f64>,
}

impl DeviceFix {
    /// Rejects non-finite, out-of-range and null-island (0, 0) readings.
    pub fn checked(self) -> Option<Self> {
        let usable = self.latitude.is_finite()
            && self.longitude.is_finite()
            && (-90.0..=90.0).contains(&self.latitude)
            && (-180.0..=180.0).contains(&self.longitude)
            && !(self.latitude.abs() < 0.000_001 && self.longitude.abs() < 0.000_001);
        usable.then_some(Self {
            // GeoClue reports an unknown altitude as -DBL_MAX.
            altitude_m: self
                .altitude_m
                .filter(|v| v.is_finite() && (-15_000.0..=100_000.0).contains(v)),
            accuracy_m: self.accuracy_m.filter(|v| v.is_finite() && *v >= 0.0),
            ..self
        })
    }
}

/// Asks the operating system for one fresh location, used where the desktop or
/// iOS WebView does not grant page geolocation. Web and Android builds use the
/// WebView's `getCurrentPosition` instead. No watch is left running afterwards
/// beyond whatever the dashboard's own GPS driver already runs.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub async fn one_shot_location(timeout: std::time::Duration) -> Result<DeviceFix, String> {
    #[cfg(target_os = "linux")]
    {
        crate::telemetry_dashboard::gps_linux::one_shot(timeout).await
    }
    #[cfg(target_os = "windows")]
    {
        crate::telemetry_dashboard::gps_windows::one_shot(timeout).await
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        crate::telemetry_dashboard::gps_apple::one_shot(timeout).await
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "windows",
        target_os = "macos",
        target_os = "ios"
    )))]
    {
        let _ = timeout;
        Err("This device has no location service. Enter coordinates manually.".into())
    }
}

/// Imperative start (only meaningful on platforms that need it).
/// Safe to call multiple times.
pub fn start_gps_updates(_user_gps: Signal<Option<(f64, f64)>>) {
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
    imp::start(_user_gps);
}

/// Imperative stop (only meaningful on platforms that need it).
pub fn stop_gps_updates() {
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
    imp::stop();
}

/// ONE common interface:
/// Mount this once in the dashboard and it will:
/// - connect `user_gps`
/// - start/stop native backends if needed
/// - use dioxus_sdk_geolocation on wasm
///
/// This component renders nothing visible.
#[component]
pub fn GpsDriver(
    user_gps: Signal<Option<(f64, f64)>>,
    #[props(optional)] user_altitude_m: Option<Signal<Option<f64>>>,
    #[props(optional)] js_ready: Option<bool>,
) -> Element {
    // wasm: hook-based SDK (no globals, no stop needed)
    #[cfg(target_arch = "wasm32")]
    {
        use dioxus_sdk_geolocation::use_geolocation;

        if let Some(false) = js_ready {
            return rsx!(div {});
        }

        let geo = use_geolocation();

        use_effect(move || {
            if let Ok(pos) = geo() {
                let lat = pos.latitude;
                let lon = pos.longitude;
                if lat.is_finite() && lon.is_finite() {
                    user_gps.set(Some((lat, lon)));
                }
                let _ = user_altitude_m;
            } else {
                // not supported / permission denied / unavailable / etc.
                // ignore (or log if you want)
            }
        });

        return rsx!(div {});
    }

    #[cfg(target_os = "windows")]
    {
        use_effect(move || {
            spawn(async move {
                if let Some(user_altitude_m) = user_altitude_m {
                    crate::telemetry_dashboard::gps_windows::run(user_gps, user_altitude_m).await;
                }
            });
        });
        use_drop(|| {
            crate::telemetry_dashboard::gps_windows::stop();
        });

        return rsx!(div {});
    }

    #[cfg(target_os = "linux")]
    {
        use_effect(move || {
            spawn(async move {
                if let Some(user_altitude_m) = user_altitude_m {
                    crate::telemetry_dashboard::gps_linux::run(user_gps, user_altitude_m).await;
                }
            });
        });
        use_drop(|| {
            crate::telemetry_dashboard::gps_linux::stop();
        });

        return rsx!(div {});
    }

    // native imperative backends: start on mount, stop on unmount
    #[cfg(not(any(target_arch = "wasm32", target_os = "windows", target_os = "linux")))]
    {
        use_effect({
            let user_gps = user_gps;
            move || {
                start_gps_updates(user_gps);
                #[cfg(target_os = "ios")]
                if let Some(mut alt_signal) = user_altitude_m {
                    alt_signal.set(crate::telemetry_dashboard::gps_apple::latest_altitude_m());
                }
                #[cfg(target_os = "android")]
                if let Some(mut alt_signal) = user_altitude_m {
                    alt_signal.set(crate::telemetry_dashboard::gps_android::latest_altitude_m());
                }
            }
        });

        #[cfg(any(target_os = "android", target_os = "ios", target_os = "macos"))]
        use_effect(move || {
            spawn(async move {
                loop {
                    #[cfg(target_os = "android")]
                    if let Some((lat, lon)) =
                        crate::telemetry_dashboard::gps_android::latest_location()
                    {
                        user_gps.set(Some((lat, lon)));
                    }

                    #[cfg(any(target_os = "ios", target_os = "macos"))]
                    if let Some(mut alt_signal) = user_altitude_m {
                        alt_signal.set(crate::telemetry_dashboard::gps_apple::latest_altitude_m());
                    }

                    #[cfg(target_os = "android")]
                    if let Some(mut alt_signal) = user_altitude_m {
                        alt_signal
                            .set(crate::telemetry_dashboard::gps_android::latest_altitude_m());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(NATIVE_ALTITUDE_SYNC_MS))
                        .await;
                }
            });
        });

        // Stop when this component is dropped (unmounted)
        use_drop(|| {
            stop_gps_updates();
        });

        rsx!(div {})
    }
}

//
// Platform-specific imperative backends
//

// Apple platforms
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod imp {
    use super::*;

    pub fn start(user_gps: Signal<Option<(f64, f64)>>) {
        crate::telemetry_dashboard::gps_apple::start(user_gps);
    }

    pub fn stop() {
        crate::telemetry_dashboard::gps_apple::stop();
    }
}

// Android
#[cfg(target_os = "android")]
mod imp {
    use super::*;

    pub fn start(_user_gps: Signal<Option<(f64, f64)>>) {
        crate::telemetry_dashboard::gps_android::start();
    }

    pub fn stop() {
        crate::telemetry_dashboard::gps_android::stop();
    }
}

// Everything else native: no-op
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_arch = "wasm32",
    target_os = "windows",
    target_os = "linux"
)))]
mod imp {
    use dioxus_signals::Signal;

    pub fn start(_user_gps: Signal<Option<(f64, f64)>>) {}
    pub fn stop() {}
}

#[cfg(test)]
mod tests {
    use super::DeviceFix;

    #[test]
    fn one_shot_fix_rejects_impossible_readings_and_drops_unknown_fields() {
        let fix = DeviceFix {
            latitude: 43.0,
            longitude: -78.8,
            altitude_m: Some(-f64::MAX),
            accuracy_m: Some(-1.0),
        };
        let checked = fix.checked().unwrap();
        assert_eq!((checked.altitude_m, checked.accuracy_m), (None, None));
        let good = DeviceFix {
            altitude_m: Some(190.0),
            accuracy_m: Some(8.0),
            ..fix
        };
        assert_eq!(good.checked(), Some(good));
        for (lat, lon) in [(0.0, 0.0), (91.0, 0.0), (0.5, 181.0), (f64::NAN, 1.0)] {
            assert!(
                DeviceFix {
                    latitude: lat,
                    longitude: lon,
                    ..fix
                }
                .checked()
                .is_none()
            );
        }
    }
}
