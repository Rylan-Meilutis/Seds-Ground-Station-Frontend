//! Exclusive antenna guidance; leaving the page tears down sensor listeners.
use super::antenna_math::{self as geometry, AltitudeReference, Position};
use super::{http_get_json, monotonic_now_ms};
use dioxus::prelude::*;
use serde::Deserialize;
#[derive(Clone, Default, Deserialize)]
struct Fix {
    latitude: f64,
    longitude: f64,
    altitude_m: Option<f64>,
    altitude_reference: String,
    age_ms: u64,
    stale: bool,
    source: String,
}
impl Fix {
    fn position(&self, geoid: Option<f64>) -> Position {
        let mut altitude = self.altitude_m;
        let mut datum = match self.altitude_reference.as_str() {
            "mean_sea_level" => AltitudeReference::MeanSeaLevel,
            "ellipsoid" => AltitudeReference::Ellipsoid,
            _ => AltitudeReference::Unknown,
        };
        if datum == AltitudeReference::Ellipsoid {
            if let Some(offset) = geoid {
                altitude = altitude.map(|v| v - offset);
                datum = AltitudeReference::MeanSeaLevel;
            }
        }
        Position {
            latitude: self.latitude,
            longitude: self.longitude,
            altitude_m: altitude,
            altitude_reference: datum,
        }
    }
    fn fresh(&self, elapsed_ms: f64) -> bool {
        !self.stale
            && self.age_ms as f64 + elapsed_ms.max(0.0) <= 15_000.0
            && self.position(None).valid()
    }
}
#[derive(Clone, Default, Deserialize)]
struct Snapshot {
    rocket: Option<Fix>,
    ground_station: Option<Fix>,
    rrls: Option<Fix>,
    rssi: Option<Rssi>,
}
#[derive(Clone, Deserialize)]
struct Rssi {
    value: f64,
    unit: String,
    source: String,
}
#[derive(Clone, Default, Deserialize)]
struct Sensors {
    orientation: Option<Orientation>,
    position: Option<Fix>,
    #[serde(default)]
    error: String,
}
#[derive(Clone, Deserialize)]
struct Orientation {
    alpha: f64,
    beta: f64,
    gamma: f64,
    age_ms: f64,
}
async fn sleep(ms: u64) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}
fn number(value: &str) -> Option<f64> {
    value.parse::<f64>().ok().filter(|v| v.is_finite())
}
fn degrees(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.1}°"))
        .unwrap_or_else(|| "—".into())
}
#[component]
pub fn AntennaAiming(on_exit: EventHandler<()>) -> Element {
    let mut snapshot = use_signal(Snapshot::default);
    let mut sensors = use_signal(Sensors::default);
    let mut origin = use_signal(|| "device".to_string());
    let mut target = use_signal(|| "rocket".to_string());
    let mut heading_trim = use_signal(|| "0".to_string());
    let mut pitch_trim = use_signal(|| "0".to_string());
    let mut geoid = use_signal(String::new);
    let mut manual = use_signal(|| false);
    let mut heading = use_signal(|| "0".to_string());
    let mut pitch = use_signal(|| "0".to_string());
    let mut best_rssi = use_signal(|| None::<(String, f64)>);
    let mut received = use_signal(monotonic_now_ms);
    let mut clock = use_signal(monotonic_now_ms);
    use_future(move || async move {
        loop {
            match http_get_json::<Snapshot>("/api/antenna/status").await {
                Ok(value) => {
                    if let Some(r) = &value.rssi {
                        let key = format!("{} {}", r.source, r.unit);
                        if best_rssi
                            .read()
                            .as_ref()
                            .is_none_or(|(old, v)| old != &key || r.value > *v)
                        {
                            best_rssi.set(Some((key, r.value)));
                        }
                    }
                    snapshot.set(value);
                    received.set(monotonic_now_ms());
                }
                Err(_) => snapshot.set(Snapshot::default()),
            }
            sleep(1000).await;
        }
    });
    use_future(move || async move {
        loop {
            let result = document::eval(include_str!("antenna_sensor_snapshot.js"))
                .recv::<Sensors>()
                .await;
            sensors.set(result.unwrap_or_default());
            clock.set(monotonic_now_ms());
            sleep(100).await;
        }
    });
    use_drop(|| {
        document::eval("window.__gs26aim?.stop?.();delete window.__gs26aim;");
    });
    let current = snapshot.read().clone();
    let sensor = sensors.read().clone();
    let backend_age_ms = (*clock.read() - *received.read()).max(0.0);
    let backend_fresh = backend_age_ms < 3_000.0;
    let source = match origin.read().as_str() {
        "gs" => current.ground_station.clone(),
        "rrls" => current.rrls.clone(),
        _ => sensor.position.clone(),
    };
    let destination = match target.read().as_str() {
        "gs" => current.ground_station.clone(),
        "rrls" => current.rrls.clone(),
        _ => current.rocket.clone(),
    };
    let origin_fresh = origin.read().as_str() == "device" || backend_fresh;
    let aim = source
        .as_ref()
        .filter(|p| {
            p.fresh(if origin.read().as_str() == "device" {
                0.0
            } else {
                backend_age_ms
            }) && origin_fresh
        })
        .zip(
            destination
                .as_ref()
                .filter(|p| p.fresh(backend_age_ms) && backend_fresh),
        )
        .and_then(|(a, b)| {
            geometry::direction(
                a.position(number(&geoid.read())),
                b.position(number(&geoid.read())),
            )
        });
    let measured = if *manual.read() {
        Some(geometry::Aim {
            bearing_deg: number(&heading.read()),
            elevation_deg: number(&pitch.read()).filter(|v| (-90.0..=90.0).contains(v)),
            distance_m: 0.0,
        })
    } else {
        sensor
            .orientation
            .as_ref()
            .filter(|o| o.age_ms <= 3_000.0)
            .and_then(|o| geometry::back_camera_direction(o.alpha, o.beta, o.gamma))
    };
    let horizontal = aim
        .and_then(|a| a.bearing_deg)
        .zip(measured.and_then(|m| m.bearing_deg))
        .and_then(|(a, m)| {
            number(&heading_trim.read()).and_then(|trim| geometry::signed_error(a, m + trim))
        });
    let vertical = aim
        .and_then(|a| a.elevation_deg)
        .zip(measured.and_then(|m| m.elevation_deg))
        .and_then(|(a, m)| number(&pitch_trim.read()).map(|trim| a - m - trim));
    let hpos = 50.0 + horizontal.unwrap_or(0.0).clamp(-45.0, 45.0) / 45.0 * 42.0;
    let vpos = 50.0 - vertical.unwrap_or(0.0).clamp(-45.0, 45.0) / 45.0 * 42.0;
    let bearing = degrees(aim.and_then(|a| a.bearing_deg));
    let elevation = degrees(aim.and_then(|a| a.elevation_deg));
    let distance = aim
        .map(|a| {
            if a.distance_m > 1000.0 {
                format!("{:.2} km", a.distance_m / 1000.0)
            } else {
                format!("{:.0} m", a.distance_m)
            }
        })
        .unwrap_or_else(|| "—".into());
    let hlabel = horizontal
        .map(|v| {
            if v.abs() < 2.0 {
                "Centered".into()
            } else {
                format!("{} {:.1}°", if v > 0.0 { "Right" } else { "Left" }, v.abs())
            }
        })
        .unwrap_or_else(|| "Waiting for position / compass".into());
    let vlabel = vertical
        .map(|v| {
            if v.abs() < 2.0 {
                "Centered".into()
            } else {
                format!("{} {:.1}°", if v > 0.0 { "Up" } else { "Down" }, v.abs())
            }
        })
        .unwrap_or_else(|| "Elevation unavailable".into());
    let source_label = source
        .as_ref()
        .map(|f| f.source.clone())
        .unwrap_or_else(|| "No fix".into());
    rsx! {
        style { {include_str!("antenna_aiming.css")} }
        main {class:"antenna-aim", "aria-label":"Antenna alignment",
            header { h1 {"Antenna alignment"} button {onclick:move |_|on_exit.call(()),"Exit aiming"} }
            div {class:"aim-options",
                label {"GPS origin" select {value:origin.read().clone(),onchange:move |e|origin.set(e.value()),option {value:"device","This device"} option {value:"gs","GroundStation"} option {value:"rrls","RRLS"}}}
                label {"Aim at" select {value:target.read().clone(),onchange:move |e|target.set(e.value()),option {value:"rocket","Rocket"} option {value:"rrls","RRLS"} option {value:"gs","GroundStation"}}}
                button {onclick:move |_|{document::eval(include_str!("antenna_sensor.js"));},"Enable compass & device GPS"}
            }
            div {class:"aim-summary",span {"Bearing {bearing}"} span {"Elevation {elevation}"} span {"Distance {distance}"} span {"Origin: {source_label}"}}
            div {class:"aim-axes",
                section {class:"aim-axis", "aria-label":"Horizontal alignment",h2 {"Horizontal"} p {"{hlabel}"}
                    div {class:"aim-track horizontal",span {class:"aim-center"} if horizontal.is_some(){span {class:"aim-rocket",style:"left:{hpos}%;top:50%;","🚀"}}else{span {class:"aim-missing","—"}}}
                }
                section {class:"aim-axis", "aria-label":"Vertical alignment",h2 {"Vertical"} p {"{vlabel}"}
                    div {class:"aim-track vertical",span {class:"aim-center"} if vertical.is_some(){span {class:"aim-rocket",style:"left:50%;top:{vpos}%;","🚀"}}else{span {class:"aim-missing","—"}}}
                }
            }
            if let Some(r)=current.rssi.filter(|_|backend_fresh){div {class:"aim-summary",span {{format!("Signal {:.1} {} · {}",r.value,r.unit,r.source)}} if let Some((_,value))=best_rssi.read().as_ref(){span {"Best {value:.1}"}} button {onclick:move |_|best_rssi.set(None),"Reset signal peak"}}}
            if !sensor.error.is_empty(){p {role:"status","{sensor.error}"}}
            details {summary {"Alignment & sensor settings"}
                p {"Point the back of the device toward the target. Only a north-referenced compass is used. Trim against a known direction or the received signal peak; radio strength is shown only when provided."}
                div {class:"aim-options",
                    label {"Heading trim (°)" input {r#type:"number",step:"0.5",value:heading_trim.read().clone(),oninput:move|e|heading_trim.set(e.value())}}
                    label {"Elevation trim (°)" input {r#type:"number",step:"0.5",value:pitch_trim.read().clone(),oninput:move|e|pitch_trim.set(e.value())}}
                    label {"Geoid separation (m)" input {r#type:"number",value:geoid.read().clone(),oninput:move|e|geoid.set(e.value())}}
                }
                p {"Vertical guidance needs fresh 3D fixes with the same altitude reference. Device GPS uses ellipsoid height; rocket GPS uses sea-level height. Enter a known local geoid separation to convert device height. Leave it blank if unknown."}
                label {input {r#type:"checkbox",checked:*manual.read(),onchange:move|e|manual.set(e.checked())} " Manual compass / inclinometer"}
                if *manual.read(){div {class:"aim-options",label {"True bearing (°)" input {r#type:"number",value:heading.read().clone(),oninput:move|e|heading.set(e.value())}} label {"Elevation (°)" input {r#type:"number",min:"-90",max:"90",value:pitch.read().clone(),oninput:move|e|pitch.set(e.value())}}}}
            }
        }
    }
}
