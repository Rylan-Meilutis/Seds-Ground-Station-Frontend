#![allow(dead_code)]
#![cfg(target_os = "linux")]

use dioxus_signals::{ReadableExt, Signal, WritableExt};
use std::sync::atomic::{AtomicU64, Ordering};
use zbus::zvariant::OwnedObjectPath;
use zbus::Proxy;

static GPS_RUN_TOKEN: AtomicU64 = AtomicU64::new(0);

const GEOCLUE_SERVICE: &str = "org.freedesktop.GeoClue2";
const GEOCLUE_MANAGER_PATH: &str = "/org/freedesktop/GeoClue2/Manager";
const GEOCLUE_MANAGER_IFACE: &str = "org.freedesktop.GeoClue2.Manager";
const GEOCLUE_CLIENT_IFACE: &str = "org.freedesktop.GeoClue2.Client";
const GEOCLUE_LOCATION_IFACE: &str = "org.freedesktop.GeoClue2.Location";
const GS26_LINUX_DESKTOP_ID: &str = "ubseds-groundstation";
const GEOCLUE_ACCURACY_EXACT: u32 = 8;

fn current_run_token() -> u64 {
    GPS_RUN_TOKEN.load(Ordering::SeqCst)
}

fn begin_new_run() -> u64 {
    GPS_RUN_TOKEN.fetch_add(1, Ordering::SeqCst) + 1
}

pub fn stop() {
    GPS_RUN_TOKEN.fetch_add(1, Ordering::SeqCst);
}

async fn open_geoclue_client() -> Result<(zbus::Connection, OwnedObjectPath), String> {
    let connection = zbus::Connection::system()
        .await
        .map_err(|e| format!("Linux GeoClue system bus connect failed: {e}"))?;

    let manager = Proxy::new(
        &connection,
        GEOCLUE_SERVICE,
        GEOCLUE_MANAGER_PATH,
        GEOCLUE_MANAGER_IFACE,
    )
    .await
    .map_err(|e| format!("Linux GeoClue manager proxy failed: {e}"))?;

    let client_path: OwnedObjectPath = manager
        .call("GetClient", &())
        .await
        .map_err(|e| format!("Linux GeoClue GetClient failed: {e}"))?;

    let client = Proxy::new(
        &connection,
        GEOCLUE_SERVICE,
        client_path.as_str(),
        GEOCLUE_CLIENT_IFACE,
    )
    .await
    .map_err(|e| format!("Linux GeoClue client proxy failed: {e}"))?;

    client
        .set_property("DesktopId", &GS26_LINUX_DESKTOP_ID)
        .await
        .map_err(|e| format!("Linux GeoClue DesktopId set failed: {e}"))?;
    client
        .set_property("RequestedAccuracyLevel", &GEOCLUE_ACCURACY_EXACT)
        .await
        .map_err(|e| format!("Linux GeoClue accuracy set failed: {e}"))?;
    client
        .call::<_, _, ()>("Start", &())
        .await
        .map_err(|e| format!("Linux GeoClue Start failed: {e}"))?;

    Ok((connection, client_path))
}

async fn read_location_once(
    connection: &zbus::Connection,
    client_path: &OwnedObjectPath,
) -> Result<Option<(f64, f64, Option<f64>)>, String> {
    let client = Proxy::new(
        connection,
        GEOCLUE_SERVICE,
        client_path.as_str(),
        GEOCLUE_CLIENT_IFACE,
    )
    .await
    .map_err(|e| format!("Linux GeoClue client proxy failed: {e}"))?;

    let location_path: OwnedObjectPath = client
        .get_property("Location")
        .await
        .map_err(|e| format!("Linux GeoClue location property failed: {e}"))?;

    let path = location_path.as_str();
    if path.is_empty() || path == "/" {
        return Ok(None);
    }

    let location = Proxy::new(connection, GEOCLUE_SERVICE, path, GEOCLUE_LOCATION_IFACE)
        .await
        .map_err(|e| format!("Linux GeoClue location proxy failed: {e}"))?;

    let lat: f64 = location
        .get_property("Latitude")
        .await
        .map_err(|e| format!("Linux GeoClue latitude read failed: {e}"))?;
    let lon: f64 = location
        .get_property("Longitude")
        .await
        .map_err(|e| format!("Linux GeoClue longitude read failed: {e}"))?;
    let altitude: Result<f64, _> = location.get_property("Altitude").await;
    let altitude = altitude.ok().filter(|value| value.is_finite());

    if lat.is_finite() && lon.is_finite() && !(lat.abs() < 0.000_001 && lon.abs() < 0.000_001) {
        Ok(Some((lat, lon, altitude)))
    } else {
        Ok(None)
    }
}

pub async fn run(
    mut user_gps: Signal<Option<(f64, f64)>>,
    mut user_altitude_m: Signal<Option<f64>>,
) {
    let token = begin_new_run();
    let mut last_error = String::new();

    while current_run_token() == token {
        match open_geoclue_client().await {
            Ok((connection, client_path)) => {
                last_error.clear();
                loop {
                    if current_run_token() != token {
                        if let Ok(client) = Proxy::new(
                            &connection,
                            GEOCLUE_SERVICE,
                            client_path.as_str(),
                            GEOCLUE_CLIENT_IFACE,
                        )
                        .await
                        {
                            let _ = client.call::<_, _, ()>("Stop", &()).await;
                        }
                        break;
                    }

                    match read_location_once(&connection, &client_path).await {
                        Ok(Some((lat, lon, altitude_m))) => {
                            if *user_gps.read() != Some((lat, lon)) {
                                user_gps.set(Some((lat, lon)));
                            }
                            if *user_altitude_m.read() != altitude_m {
                                user_altitude_m.set(altitude_m);
                            }
                        }
                        Ok(None) => {}
                        Err(message) => {
                            if last_error != message {
                                eprintln!("{message}");
                                last_error = message;
                            }
                            break;
                        }
                    }

                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
            Err(message) => {
                if last_error != message {
                    eprintln!("{message}");
                    last_error = message;
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}
