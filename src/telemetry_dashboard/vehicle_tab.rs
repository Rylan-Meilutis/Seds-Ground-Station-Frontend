use super::{UrlConfig, http_get_json, js_eval, layout::ThemeConfig};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

static VIEWER_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct VehicleVisualizationConfig {
    #[serde(default)]
    pub motions: Vec<ModelMotion>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub model_url: String,
    #[serde(default)]
    pub ground_model_url: String,
    /// Optional self-hosted model-viewer module. Ground stations can serve this
    /// alongside the GLB for fully offline operation.
    #[serde(default)]
    pub renderer_url: String,
    #[serde(default)]
    pub model_alt: String,
    #[serde(default)]
    pub camera_orbit: String,
    #[serde(default)]
    pub phase_animations: BTreeMap<String, String>,
    #[serde(default)]
    pub attitude: VehicleAttitudeBindings,
    #[serde(default)]
    pub stages: Vec<VehicleStageConfig>,
    #[serde(default)]
    pub ground_systems: Vec<VehicleComponentConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct ModelMotion {
    pub node: String,
    pub transform: String,
    pub axis: [f32; 3],
    pub from: f32,
    pub to: f32,
    #[serde(default)]
    pub binding: Option<VehicleTelemetryBinding>,
    #[serde(default)]
    pub phase_values: BTreeMap<String, f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct VehicleAttitudeBindings {
    pub roll: Option<VehicleTelemetryBinding>,
    pub pitch: Option<VehicleTelemetryBinding>,
    pub yaw: Option<VehicleTelemetryBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct VehicleStageConfig {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub separation: Option<VehicleTelemetryBinding>,
    #[serde(default)]
    pub components: Vec<VehicleComponentConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct VehicleComponentConfig {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub binding: Option<VehicleTelemetryBinding>,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub min: Option<f32>,
    #[serde(default)]
    pub max: Option<f32>,
    #[serde(default)]
    pub active_threshold: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct VehicleTelemetryBinding {
    pub data_type: String,
    #[serde(default)]
    pub sender_id: Option<String>,
    #[serde(default)]
    pub index: usize,
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default)]
    pub offset: f32,
}

fn default_scale() -> f32 {
    1.0
}

fn value(binding: &VehicleTelemetryBinding, frame: &super::vehicle_replay::Frame) -> Option<f32> {
    frame.value(binding, false)
}

fn backend_url(path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    let base = UrlConfig::base_http();
    if base.is_empty() {
        path.to_string()
    } else if path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

fn normalized_phase(phase: &str) -> String {
    phase
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn model_animation(config: &VehicleVisualizationConfig, phase: &str) -> Option<String> {
    let normalized = normalized_phase(phase);
    config
        .phase_animations
        .iter()
        .find(|(key, _)| normalized_phase(key) == normalized)
        .map(|(_, animation)| animation.clone())
}

fn sync_model_viewer(
    viewer_id: usize,
    config: &VehicleVisualizationConfig,
    frame: &super::vehicle_replay::Frame,
    replay_seconds: f64,
    gps_sender: &str,
) {
    let phase = frame.phase().unwrap_or_else(|| "Unknown".into());
    let roll = config.attitude.roll.as_ref().and_then(|b| value(b, frame));
    let pitch = config.attitude.pitch.as_ref().and_then(|b| value(b, frame));
    let yaw = config.attitude.yaw.as_ref().and_then(|b| value(b, frame));
    if config.model_url.trim().is_empty() {
        return;
    }
    let motions: Vec<_> = config
        .motions
        .iter()
        .map(|m| {
            let v = if let Some(binding) = &m.binding {
                value(binding, frame)
            } else {
                m.phase_values
                    .iter()
                    .find(|(name, _)| normalized_phase(name) == normalized_phase(&phase))
                    .map(|(_, v)| *v)
                    .or_else(|| m.phase_values.get("*").copied())
            };
            let mut result = serde_json::to_value(m).unwrap_or_default();
            result["value"] = serde_json::json!(v);
            result
        })
        .collect();
    let payload=serde_json::json!({"motions":motions,"orbit":config.camera_orbit,"attitude":[pitch,yaw,roll],"gps":frame.gps_for(gps_sender).map(|(lat,lon,alt)|[lat,lon,alt]),"clip":model_animation(config,&phase),"replay_time_seconds":replay_seconds}).to_string();
    let payload = serde_json::to_string(&payload).unwrap();
    let src = serde_json::to_string(&backend_url(&config.model_url)).unwrap();
    let renderer =
        serde_json::to_string(&backend_url("/assets/three/vehicle-renderer.js")).unwrap();
    js_eval(&format!(
        r#"(() => {{
        const m=document.getElementById('gs26-vehicle-model-{viewer_id}');if(!m)return;
        window.__gs26RendererModules ||= new Map();
        if(!window.__gs26RendererModules.has({renderer}))window.__gs26RendererModules.set({renderer},import({renderer}));
        window.__gs26RendererModules.get({renderer}).catch(()=>{{m.textContent='3D renderer could not load. Check the GroundStation connection and update the backend.';}});
        if(m.getAttribute('src')!=={src})m.setAttribute('src',{src});
        if(m.getAttribute('data-state')!=={payload})m.setAttribute('data-state',{payload});
    }})();"#
    ));
}

fn component_kind_label(kind: &str) -> &'static str {
    match kind {
        "motor" => "MOTOR",
        "fin" => "FIN",
        "gimbal" => "GIMBAL",
        "air_brake" | "airbrake" => "AIR BRAKE",
        "tank" | "propellant" => "TANK",
        "parachute" => "PARACHUTE",
        "link" => "LINK",
        _ => "SYSTEM",
    }
}

fn component_readout(
    component: &VehicleComponentConfig,
    frame: &super::vehicle_replay::Frame,
) -> (Option<f32>, String, f32, bool) {
    let raw = component.binding.as_ref().and_then(|binding| {
        frame.value(
            binding,
            matches!(component.kind.as_str(), "parachute" | "link"),
        )
    });
    let min = component.min.unwrap_or(0.0);
    let max = component.max.unwrap_or(100.0);
    let fraction = raw
        .map(|value| ((value - min) / (max - min).max(f32::EPSILON)).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let active = raw.is_some_and(|value| value >= component.active_threshold.unwrap_or(0.5));
    let display = raw
        .map(|value| {
            if component.unit.trim().is_empty() {
                format!("{value:.2}")
            } else {
                format!("{value:.2} {}", component.unit)
            }
        })
        .unwrap_or_else(|| "No data".to_string());
    (raw, display, fraction, active)
}

fn percent_component(value: &str) -> String {
    const HEX: &[u8] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[(byte >> 4) as usize] as char);
            out.push(HEX[(byte & 15) as usize] as char);
        }
    }
    out
}
fn replay_types(cfg: &VehicleVisualizationConfig) -> Vec<String> {
    let mut types = std::collections::BTreeSet::from([
        "FLIGHT_STATE".to_owned(),
        "GPS_DATA".to_owned(),
        "GPS".to_owned(),
        "ROCKET_GPS".to_owned(),
    ]);
    let mut add = |binding: &Option<VehicleTelemetryBinding>| {
        if let Some(binding) = binding {
            types.insert(binding.data_type.clone());
        }
    };
    for b in [&cfg.attitude.roll, &cfg.attitude.pitch, &cfg.attitude.yaw] {
        add(b);
    }
    for motion in &cfg.motions {
        add(&motion.binding);
    }
    for stage in &cfg.stages {
        add(&stage.separation);
        for component in &stage.components {
            add(&component.binding);
        }
    }
    for component in &cfg.ground_systems {
        add(&component.binding);
    }
    types.into_iter().collect()
}
fn playback_clock_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let global = js_sys::global();
        if let Ok(performance) = js_sys::Reflect::get(&global, &"performance".into())
            && let Ok(now) = js_sys::Reflect::get(&performance, &"now".into())
            && let Some(now) = now.dyn_ref::<js_sys::Function>()
            && let Ok(value) = now.call0(&performance)
            && let Some(value) = value.as_f64()
        {
            return value;
        }
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_secs_f64()
            * 1000.
    }
}
// A native application may switch stations while a paginated request is in flight.
// Never combine records, configuration or pages fetched from different servers.
async fn saved_get<T: serde::de::DeserializeOwned>(origin: &str, path: &str) -> Result<T, String> {
    if UrlConfig::base_http() != origin {
        return Err("Station changed; select a saved recording again".into());
    }
    let result = http_get_json::<T>(path).await;
    if UrlConfig::base_http() != origin {
        return Err("Station changed; select a saved recording again".into());
    }
    result
}
#[component]
pub(crate) fn VehicleTab(theme: ThemeConfig) -> Element {
    use super::vehicle_replay::{
        Buffer, Frame, Index, Page, Recording, Snapshot, Trajectory, advance,
    };
    let mut station = use_signal(UrlConfig::base_http);
    let mut config = use_resource(move || {
        let origin = station.read().clone();
        async move {
            saved_get::<VehicleVisualizationConfig>(&origin, "/api/vehicle_visualization").await
        }
    });
    let mut recordings = use_resource(move || {
        let origin = station.read().clone();
        async move { saved_get::<Vec<Recording>>(&origin, "/api/recordings").await }
    });
    let mut selected = use_signal(String::new);
    let mut position = use_signal(|| 0f64);
    let mut seek_preview = use_signal(|| None::<f64>);
    let mut playing = use_signal(|| false);
    let mut speed = use_signal(|| 1f64);
    let mut loaded = use_signal(|| (String::new(), -1i64));
    let mut snapshot = use_resource(move || {
        let id = selected.read().clone();
        let origin = station.read().clone();
        async move {
            if id.is_empty() {
                return Ok(None);
            }
            let value = saved_get::<Snapshot>(
                &origin,
                &format!("/api/recordings/{}/replay", percent_component(&id)),
            )
            .await?;
            Ok::<_, String>(Some((origin, id, value)))
        }
    });
    let mut trajectory = use_resource(move || {
        let saved = snapshot
            .read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(|r| r.clone());
        let id = selected.read().clone();
        let origin = station.read().clone();
        async move {
            let Some((saved_origin, recording, saved)) = saved else {
                return Ok(None);
            };
            if recording != id || saved_origin != origin {
                return Ok(None);
            }
            let track = saved_get::<Trajectory>(
                &origin,
                &format!(
                    "/api/recordings/{}/replay/trajectory?upper_id={}",
                    percent_component(&id),
                    saved.upper_id
                ),
            )
            .await?;
            if track.upper_id != saved.upper_id || track.points.len() > 8192 {
                return Err("Invalid saved trajectory snapshot".into());
            }
            Ok::<_, String>(Some((origin, id, saved.upper_id, track)))
        }
    });
    let chunk = use_memo(move || (*position.read() / 10000.).floor() as i64);
    let mut buffer = use_resource(move || {
        let saved = snapshot
            .read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(|r| r.clone());
        let cfg = config
            .read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .cloned();
        let id = selected.read().clone();
        let chunk = *chunk.read();
        let origin = station.read().clone();
        async move {
            let (Some((saved_origin, recording, saved)), Some(cfg)) = (saved, cfg) else {
                return Ok(None);
            };
            if recording != id || saved_origin != origin {
                return Ok(None);
            }
            let (Some(first), Some(last)) = (saved.start_ms, saved.end_ms) else {
                return Ok(None);
            };
            let types = replay_types(&cfg);
            if types.len() > 32 {
                return Err("Replay configuration uses too many data types".into());
            }
            let start = first
                .saturating_add(chunk.saturating_mul(10000))
                .saturating_sub(5000)
                .max(first);
            let end = start.saturating_add(25000).min(last);
            let encoded = serde_json::to_string(&types).map_err(|e| e.to_string())?;
            let mut after = 0;
            let mut samples = Vec::new();
            for _ in 0..32 {
                let url = format!(
                    "/api/recordings/{}/replay/window?upper_id={}&after_id={after}&start_ms={start}&end_ms={end}&types={}",
                    percent_component(&id),
                    saved.upper_id,
                    percent_component(&encoded)
                );
                let page = saved_get::<Page>(&origin, &url).await?;
                if page.upper_id != saved.upper_id || (page.has_more && page.next_id <= after) {
                    return Err("Invalid saved replay cursor".into());
                }
                samples.extend(page.seeds);
                samples.extend(page.rows);
                if samples.len() > 16384 {
                    return Err("Saved replay window is too large".into());
                }
                if !page.has_more {
                    return Ok(Some(Buffer {
                        origin,
                        recording: id,
                        upper_id: saved.upper_id,
                        start_ms: start,
                        end_ms: end,
                        data: std::rc::Rc::new(Index::new(samples)),
                    }));
                }
                after = page.next_id;
            }
            Err::<Option<Buffer>, String>("Saved replay window is too large".into())
        }
    });
    use_effect(move || {
        let saved = snapshot
            .read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(|r| r.clone());
        if let Some((origin, id, saved)) = saved {
            let key = (id.clone(), saved.upper_id);
            if origin == UrlConfig::base_http() && id == *selected.read() && key != *loaded.read() {
                loaded.set(key);
                playing.set(false);
                position.set(0.);
            }
        }
    });
    use_future(move || async move {
        let mut previous = playback_clock_ms();
        loop {
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(50).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let now = playback_clock_ms();
            let elapsed = now - previous;
            previous = now;
            let origin = UrlConfig::base_http();
            if origin != *station.read() {
                playing.set(false);
                selected.set(String::new());
                position.set(0.);
                seek_preview.set(None);
                station.set(origin);
                continue;
            }
            if !*playing.read() {
                continue;
            }
            let saved = snapshot
                .read()
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .and_then(|r| r.clone());
            if let Some((origin, id, saved)) = saved {
                if origin != UrlConfig::base_http() || id != *selected.read() {
                    continue;
                }
                if let (Some(first), Some(last)) = (saved.start_ms, saved.end_ms) {
                    let duration = last.saturating_sub(first).max(0) as f64;
                    let next = advance(*position.read(), elapsed, *speed.read(), duration);
                    let ready = buffer
                        .read()
                        .as_ref()
                        .and_then(|r| r.as_ref().ok())
                        .and_then(|r| r.as_ref())
                        .is_some_and(|b| {
                            b.contains(
                                &origin,
                                &id,
                                saved.upper_id,
                                first.saturating_add(next as i64),
                            )
                        });
                    if ready {
                        position.set(next);
                        if next >= duration {
                            playing.set(false);
                        }
                    }
                }
            }
        }
    });
    let viewer_id = use_hook(|| VIEWER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    use_effect(move || {
        let id = selected.read().clone();
        let origin = station.read().clone();
        let upper = snapshot
            .read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(|r| r.as_ref())
            .map(|(_, _, s)| s.upper_id);
        let saved_track = trajectory
            .read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(|r| r.as_ref())
            .filter(|(o, i, u, _)| o == &origin && i == &id && Some(*u) == upper)
            .map(|(_, _, _, track)| track.clone());
        let payload = serde_json::to_string(&serde_json::to_string(&saved_track).unwrap()).unwrap();
        js_eval(&format!(
            "(()=>{{const m=document.getElementById('gs26-vehicle-model-{viewer_id}');if(m)m.setAttribute('data-track',{payload});}})();"
        ));
    });
    let viewer_html = format!(
        r#"<gs-vehicle-viewer id="gs26-vehicle-model-{viewer_id}" style="display:block;width:100%;height:100%;position:relative" aria-label="Saved rocket telemetry replay"></gs-vehicle-viewer>"#
    );
    let cfg = config
        .read()
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .cloned()
        .unwrap_or_default();
    let saved = snapshot
        .read()
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .and_then(|r| r.clone())
        .filter(|(origin, id, _)| origin == &UrlConfig::base_http() && id == &*selected.read());
    let first = saved.as_ref().and_then(|(_, _, s)| s.start_ms).unwrap_or(0);
    let duration = saved
        .as_ref()
        .and_then(|(_, _, s)| s.end_ms)
        .map(|last| last.saturating_sub(first).max(0) as f64)
        .unwrap_or(0.);
    let seek_position = seek_preview.read().unwrap_or(*position.read());
    let absolute = first.saturating_add(*position.read() as i64);
    let data = buffer
        .read()
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .and_then(|r| r.as_ref())
        .filter(|b| {
            saved
                .as_ref()
                .is_some_and(|(origin, id, s)| b.contains(origin, id, s.upper_id, absolute))
        })
        .map(|b| b.data.clone());
    let frame = Frame {
        data,
        at_ms: absolute,
    };
    let phase = frame.phase().unwrap_or_else(|| "Unknown".into());
    let roll = cfg.attitude.roll.as_ref().and_then(|b| value(b, &frame));
    let pitch = cfg.attitude.pitch.as_ref().and_then(|b| value(b, &frame));
    let yaw = cfg.attitude.yaw.as_ref().and_then(|b| value(b, &frame));
    let gps_sender = trajectory
        .read()
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .and_then(|r| r.as_ref())
        .filter(|(o, i, u, _)| {
            o == &UrlConfig::base_http()
                && i == &*selected.read()
                && saved.as_ref().is_some_and(|(_, _, s)| s.upper_id == *u)
        })
        .and_then(|(_, _, _, t)| t.source_sender_id.clone())
        .unwrap_or_else(|| "FC".into());
    let gps = frame.gps_for(&gps_sender);
    let error = recordings
        .read()
        .as_ref()
        .and_then(|r| r.as_ref().err())
        .cloned()
        .or_else(|| {
            snapshot
                .read()
                .as_ref()
                .and_then(|r| r.as_ref().err())
                .cloned()
        })
        .or_else(|| {
            buffer
                .read()
                .as_ref()
                .and_then(|r| r.as_ref().err())
                .cloned()
        })
        .or_else(|| {
            config
                .read()
                .as_ref()
                .and_then(|r| r.as_ref().err())
                .cloned()
        })
        .or_else(|| {
            trajectory
                .read()
                .as_ref()
                .and_then(|r| r.as_ref().err())
                .map(|e| format!("Saved trajectory: {e}"))
        });
    let mut show_error = use_signal(|| false);
    use_effect(move || {
        let saved = snapshot
            .read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(|r| r.clone());
        let cfg = config
            .read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .cloned();
        let at = *position.read();
        let id = selected.read().clone();
        let data = buffer
            .read()
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(|r| r.as_ref())
            .filter(|b| {
                saved.as_ref().is_some_and(|(origin, recording, s)| {
                    origin == &UrlConfig::base_http()
                        && recording == &id
                        && b.contains(
                            origin,
                            &id,
                            s.upper_id,
                            s.start_ms.unwrap_or(0).saturating_add(at as i64),
                        )
                })
            })
            .map(|b| b.data.clone());
        if let (Some(cfg), Some((_, _, saved))) = (cfg, saved) {
            let frame = Frame {
                data,
                at_ms: saved.start_ms.unwrap_or(0).saturating_add(at as i64),
            };
            let gps_sender = trajectory
                .read()
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .and_then(|r| r.as_ref())
                .filter(|(o, i, u, _)| {
                    o == &UrlConfig::base_http() && i == &id && *u == saved.upper_id
                })
                .and_then(|(_, _, _, t)| t.source_sender_id.clone())
                .unwrap_or_else(|| "FC".into());
            sync_model_viewer(viewer_id, &cfg, &frame, at / 1000., &gps_sender);
        }
    });
    let button = format!(
        "min-height:44px;padding:8px 14px;border-radius:10px;border:1px solid {};background:{};color:{};font:inherit;cursor:pointer;",
        theme.border_soft, theme.panel_background_alt, theme.text_primary
    );
    let available = frame.data.is_some();
    rsx! {
        div {class:"gs26-vehicle-shell",style:"height:100%;overflow-y:auto;padding:12px;box-sizing:border-box;color:{theme.text_primary};background:{theme.tab_shell_background};",
            h2 {"3D replay"}
            div {style:"display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin-bottom:12px;",
                label {r#for:"replay-recording","Recording"}
                select {id:"replay-recording",style:"{button}width:min(100%,480px);max-width:100%;",value:"{selected}",onchange:move|event|{playing.set(false);seek_preview.set(None);position.set(0.);selected.set(event.value());},
                    option {value:"","Select a saved recording"}
                    if let Some(Ok(items))=recordings.read().as_ref(){for item in items{option {value:"{item.id}","{item.id}" if item.active{" (snapshot)"}}}}
                }
                button {style:"{button}",onclick:move |_|{playing.set(false);config.restart();recordings.restart();snapshot.restart();buffer.restart();trajectory.restart();},"Reload"}
                if error.is_some(){button {style:"{button}",onclick:move |_|show_error.set(true),"Details"}}
            }
            div {style:"display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin-bottom:8px;",
                button {style:"{button}",disabled:!available || duration<=0.,onclick:move |_|{if *position.read()>=duration{position.set(0.);}let next=!*playing.read();playing.set(next);},if *playing.read(){"Pause"}else{"Play"}}
                button {style:"{button}",disabled:saved.is_none(),onclick:move |_|{playing.set(false);position.set(0.);},"Rewind"}
                label {r#for:"replay-speed","Speed"}
                select {id:"replay-speed",style:"{button}",value:"{speed}",onchange:move |event|{if let Ok(value)=event.value().parse::<f64>(){speed.set(value.clamp(0.25,4.));}},
                    option {value:"0.25","0.25×"}option {value:"0.5","0.5×"}option {value:"1","1×"}option {value:"2","2×"}option {value:"4","4×"}
                }
                span {style:"font-variant-numeric:tabular-nums;color:{theme.text_muted};",{format!("{:.1} / {:.1} s",*position.read()/1000.,duration/1000.)}}
            }
            input {r#type:"range",aria_label:"Saved replay position",min:"0",max:"{duration}",step:"1",value:"{seek_position}",disabled:saved.is_none(),style:"width:100%;min-height:44px;",oninput:move |event|{playing.set(false);if let Ok(value)=event.value().parse::<f64>(){seek_preview.set(Some(value.clamp(0.,duration)));}},onchange:move |event|{if let Ok(value)=event.value().parse::<f64>(){position.set(value.clamp(0.,duration));}seek_preview.set(None);}}
            div {style:"display:flex;gap:10px;flex-wrap:wrap;margin:8px 0;color:{theme.text_muted};",
                span {"Saved data · {phase}"}
                if roll.is_none() || pitch.is_none() || yaw.is_none(){span {"Attitude unavailable"}}
                if error.is_some(){span {"Replay unavailable"}}else if !available && !selected.read().is_empty(){span {"Loading saved samples…"}}
            }
            div {role:"group",aria_label:"Replay camera",style:"display:flex;gap:6px;flex-wrap:wrap;margin:8px 0;",
                for (action,label) in [("follow","Follow rocket"),("overview","Flight path"),("reset","Reset view"),("zoom-in","Zoom in"),("zoom-out","Zoom out")] {
                    button {style:"{button}",onclick:move |_|js_eval(&format!("document.getElementById('gs26-vehicle-model-{viewer_id}')?.cameraAction?.('{action}');")),"{label}"}
                }
                details {style:"flex-basis:100%;",summary {style:"cursor:pointer;min-height:44px;display:flex;align-items:center;","Camera controls"}
                    div {style:"display:flex;gap:6px;flex-wrap:wrap;",
                        for (action,label) in [("pan-left","Pan left"),("pan-right","Pan right"),("pan-up","Pan up"),("pan-down","Pan down"),("rise","Move higher"),("descend","Move lower")] {
                            button {style:"{button}",onclick:move |_|js_eval(&format!("document.getElementById('gs26-vehicle-model-{viewer_id}')?.cameraAction?.('{action}');")),"{label}"}
                        }
                    }
                    p {style:"max-width:40rem;font-size:13px;","Drag to orbit. Shift-drag or two fingers pan; pinch or scroll zooms. Focus the scene: arrow keys orbit, W/A/S/D pan, Q/E move vertically, F follows and R resets. Ground is a curved Earth reference, not recorded terrain."}
                }
            }
            div {style:"position:relative;min-height:360px;height:55vh;border:1px solid {theme.border_soft};border-radius:14px;background:{theme.panel_background};overflow:hidden;",
                div {style:if available{"height:100%;visibility:visible"}else{"height:100%;visibility:hidden"},dangerous_inner_html:viewer_html}
                div {style:"position:absolute;left:12px;top:12px;right:12px;pointer-events:none;font:12px ui-monospace,monospace;",{format!("Roll {} · Pitch {} · Yaw {}",roll.map(|v|format!("{v:.1}°")).unwrap_or_else(||"—".into()),pitch.map(|v|format!("{v:.1}°")).unwrap_or_else(||"—".into()),yaw.map(|v|format!("{v:.1}°")).unwrap_or_else(||"—".into()))}}
                div {style:"position:absolute;left:12px;right:12px;bottom:12px;display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:6px;",
                    {position_card(&theme,"Latitude",gps.map(|v|format!("{:.6}°",v.0)).unwrap_or_else(||"—".into()))}
                    {position_card(&theme,"Longitude",gps.map(|v|format!("{:.6}°",v.1)).unwrap_or_else(||"—".into()))}
                    {position_card(&theme,"Altitude",gps.map(|v|format!("{:.1} m",v.2)).unwrap_or_else(||"—".into()))}
                }
            }
            div {style:"display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,260px),1fr));gap:10px;margin-top:12px;",
                for stage in &cfg.stages {if !stage.components.is_empty() || stage.separation.is_some(){
                    div {style:"padding:12px;border:1px solid {theme.border_soft};border-radius:12px;",
                        strong {"{stage.label}"}
                        span {style:"margin-left:8px;color:{theme.text_muted};",{stage.separation.as_ref().and_then(|b|frame.value(b,true)).map(|v|if v>=0.5{"Separated"}else{"Attached"}).unwrap_or("No saved state")}}
                        for component in &stage.components{{component_row(&theme,component,&frame)}}
                    }
                }}
            }
            if *show_error.read(){div {role:"dialog",aria_modal:"true",aria_label:"Replay details",style:"position:fixed;inset:0;background:rgba(0,0,0,.7);display:grid;place-items:center;z-index:10000;padding:16px;",
                div {style:"max-width:560px;padding:20px;border-radius:14px;background:{theme.panel_background};color:{theme.text_primary};",
                    h3 {"Saved replay"}p {style:"white-space:pre-wrap;overflow-wrap:anywhere;","{error.clone().unwrap_or_default()}"}
                    button {style:"{button}",onclick:move |_|show_error.set(false),"Close"}
                }
            }}
        }
    }
}

fn position_card(theme: &ThemeConfig, label: &str, value: String) -> Element {
    rsx! {
        div { style: "min-width:0; padding:8px; border-radius:10px; background:{theme.overlay_background}; border:1px solid {theme.border_soft}; text-align:center;",
            div { style: "color:{theme.text_muted}; font-size:9px; letter-spacing:.06em;", "{label.to_uppercase()}" }
            div { style: "overflow:hidden; text-overflow:ellipsis; font-size:11px; font-weight:800; font-variant-numeric:tabular-nums;", "{value}" }
        }
    }
}

fn component_row(
    theme: &ThemeConfig,
    component: &VehicleComponentConfig,
    frame: &super::vehicle_replay::Frame,
) -> Element {
    let (raw, display, fraction, active) = component_readout(component, frame);
    let width = format!("{:.1}%", fraction * 100.0);
    let status = if raw.is_none() {
        "NO DATA"
    } else if matches!(component.kind.as_str(), "motor" | "parachute" | "link") {
        if active { "ACTIVE" } else { "STANDBY" }
    } else {
        "RECORDED"
    };
    let status_color = if raw.is_none() {
        theme.text_muted.as_str()
    } else if active {
        theme.success_text.as_str()
    } else {
        theme.info_accent.as_str()
    };
    rsx! {
        div { style: "padding:9px; border:1px solid {theme.border_soft}; border-radius:11px; background:{theme.panel_background_alt};",
            div { style: "display:grid; grid-template-columns:auto minmax(0,1fr) auto; align-items:center; gap:8px;",
                span { style: "font-size:9px; font-weight:800; color:{theme.text_muted};", "{component_kind_label(&component.kind)}" }
                span { style: "min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:12px;", "{component.label}" }
                span { style: "color:{status_color}; font-size:9px; font-weight:900;", "{status}" }
            }
            div { style: "display:flex; align-items:center; gap:8px; margin-top:6px;",
                div { style: "height:5px; flex:1; overflow:hidden; border-radius:999px; background:{theme.border_soft};",
                    div { class: "gs26-vehicle-stage", style: "height:100%; width:{width}; border-radius:inherit; background:{status_color};" }
                }
                span { style: "min-width:76px; text-align:right; color:{theme.text_secondary}; font:11px ui-monospace,SFMono-Regular,Menlo,monospace;", "{display}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_paths_and_filter_json_are_encoded_as_one_url_component() {
        assert_eq!(percent_component("a/b ?#é"), "a%2Fb%20%3F%23%C3%A9");
        assert_eq!(
            percent_component(r#"["ROLL","GPS_DATA"]"#),
            "%5B%22ROLL%22%2C%22GPS_DATA%22%5D"
        );
    }

    #[test]
    fn parses_documented_vehicle_configuration() {
        let config: VehicleVisualizationConfig = serde_json::from_str(include_str!(
            "../../docs/api-examples/vehicle-visualization.json"
        ))
        .expect("vehicle example should remain valid");

        assert_eq!(config.stages.len(), 1);
        assert_eq!(config.stages[0].id, "stage-1");
        assert!(config.ground_systems.is_empty());
        assert_eq!(config.motions.len(), 3);
        assert_eq!(model_animation(&config, "ParachuteDeploy"), None);
    }

    #[test]
    fn component_fraction_is_clamped_for_deflection_ranges() {
        let component = VehicleComponentConfig {
            min: Some(-20.0),
            max: Some(20.0),
            ..Default::default()
        };
        let (_, _, fraction, active) =
            component_readout(&component, &super::super::vehicle_replay::Frame::default());
        assert_eq!(fraction, 0.0);
        assert!(!active);
    }
}
