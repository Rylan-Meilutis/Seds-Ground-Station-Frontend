#![allow(dead_code)]
#![cfg(target_os = "windows")]

use dioxus_signals::{ReadableExt, Signal, WritableExt};
use std::sync::atomic::{AtomicU64, Ordering};
use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator, PositionAccuracy};

static GPS_RUN_TOKEN: AtomicU64 = AtomicU64::new(0);

fn current_run_token() -> u64 {
    GPS_RUN_TOKEN.load(Ordering::SeqCst)
}

fn begin_new_run() -> u64 {
    GPS_RUN_TOKEN.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn stop() {
    GPS_RUN_TOKEN.fetch_add(1, Ordering::SeqCst);
}

fn read_location_once_blocking() -> Result<Option<(f64, f64)>, String> {
    let access = Geolocator::RequestAccessAsync()
        .map_err(|e| format!("Windows GPS access request failed: {e}"))?
        .get()
        .map_err(|e| format!("Windows GPS access status failed: {e}"))?;

    if access != GeolocationAccessStatus::Allowed {
        return Err(format!("Windows GPS access not allowed: {access:?}"));
    }

    let locator = Geolocator::new().map_err(|e| format!("Windows Geolocator init failed: {e}"))?;
    let _ = locator.SetDesiredAccuracy(PositionAccuracy::High);

    let position = locator
        .GetGeopositionAsync()
        .map_err(|e| format!("Windows GPS position request failed: {e}"))?
        .get()
        .map_err(|e| format!("Windows GPS position read failed: {e}"))?;

    let coordinate = position
        .Coordinate()
        .map_err(|e| format!("Windows GPS coordinate read failed: {e}"))?;
    let point = coordinate
        .Point()
        .map_err(|e| format!("Windows GPS point read failed: {e}"))?;
    let basic = point
        .Position()
        .map_err(|e| format!("Windows GPS basic position read failed: {e}"))?;
    let lat = basic.Latitude;
    let lon = basic.Longitude;

    if lat.is_finite() && lon.is_finite() && !(lat.abs() < 0.000_001 && lon.abs() < 0.000_001) {
        Ok(Some((lat, lon)))
    } else {
        Ok(None)
    }
}

pub async fn run(mut user_gps: Signal<Option<(f64, f64)>>) {
    let token = begin_new_run();
    let mut last_error = String::new();

    loop {
        if current_run_token() != token {
            break;
        }

        match tokio::task::spawn_blocking(read_location_once_blocking).await {
            Ok(Ok(Some((lat, lon)))) => {
                if *user_gps.read() != Some((lat, lon)) {
                    user_gps.set(Some((lat, lon)));
                }
                last_error.clear();
            }
            Ok(Ok(None)) => {}
            Ok(Err(message)) => {
                if last_error != message {
                    eprintln!("{message}");
                    last_error = message;
                }
            }
            Err(err) => {
                let message = format!("Windows GPS worker join failed: {err}");
                if last_error != message {
                    eprintln!("{message}");
                    last_error = message;
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}
