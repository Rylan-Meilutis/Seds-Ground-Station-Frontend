// frontend/src/telemetry_dashboard/gps.rs
#![allow(dead_code)]

use dioxus::prelude::*;
use dioxus_signals::Signal;

const NATIVE_ALTITUDE_SYNC_MS: u64 = 250;

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
