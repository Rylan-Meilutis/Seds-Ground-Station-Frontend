use super::{
    TELEMETRY_RENDER_EPOCH, UrlConfig, http_get_json, js_eval, latest_telemetry_value,
    layout::ThemeConfig, types::FlightState,
};
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

fn visual_now_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }
}
pub(super) fn value(binding: &VehicleTelemetryBinding) -> Option<f32> {
    let row = super::latest_telemetry_row(&binding.data_type, binding.sender_id.as_deref())?;
    let age = visual_now_ms().saturating_sub(row.received_timestamp_ms);
    if !(0..=5000).contains(&age) {
        return None;
    }
    latest_telemetry_value(
        &binding.data_type,
        binding.sender_id.as_deref(),
        binding.index,
    )
    .map(|value| value * binding.scale + binding.offset)
    .filter(|value| value.is_finite())
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
    phase: &str,
    roll: Option<f32>,
    pitch: Option<f32>,
    yaw: Option<f32>,
) {
    if config.model_url.trim().is_empty() {
        return;
    }
    let motions: Vec<_> = config
        .motions
        .iter()
        .map(|m| {
            let v = if let Some(binding) = &m.binding {
                value(binding)
            } else {
                m.phase_values
                    .iter()
                    .find(|(name, _)| normalized_phase(name) == normalized_phase(phase))
                    .map(|(_, v)| *v)
                    .or_else(|| m.phase_values.get("*").copied())
            };
            let mut result = serde_json::to_value(m).unwrap_or_default();
            result["value"] = serde_json::json!(v);
            result
        })
        .collect();
    let payload=serde_json::json!({"motions":motions,"orbit":config.camera_orbit,"attitude":[pitch.unwrap_or(0.0),yaw.unwrap_or(0.0),roll.unwrap_or(0.0)],"clip":model_animation(config,phase)}).to_string();
    let payload = serde_json::to_string(&payload).unwrap();
    let ground = super::gse_panel::ground_visible(phase) && !config.ground_model_url.is_empty();
    let src = serde_json::to_string(&backend_url(if ground {
        &config.ground_model_url
    } else {
        &config.model_url
    }))
    .unwrap();
    let payload = if ground {
        let clip = match phase {
            "NitrogenFill" => "nitrogen-test",
            "NitrousFill" => "nitrous-fill",
            _ => "",
        };
        serde_json::to_string(
            &serde_json::json!({"motions":[],"attitude":[0,0,0],"clip":clip}).to_string(),
        )
        .unwrap()
    } else {
        payload
    };
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

fn component_readout(component: &VehicleComponentConfig) -> (Option<f32>, String, f32, bool) {
    let raw = component.binding.as_ref().and_then(value);
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

#[component]
pub(crate) fn VehicleTab(
    theme: ThemeConfig,
    flight_state: Signal<FlightState>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    rocket_altitude_m: Signal<Option<f64>>,
) -> Element {
    let config = use_signal(|| None::<VehicleVisualizationConfig>);
    let viewer_id = use_hook(|| VIEWER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    let viewer_html = format!(
        r#"<gs-vehicle-viewer id="gs26-vehicle-model-{viewer_id}" style="display:block;width:100%;height:100%;position:relative" aria-label="Rocket and prelaunch ground equipment"></gs-vehicle-viewer>"#
    );
    let mut model_tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(500).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let next = *model_tick.read() + 1;
            model_tick.set(next);
        }
    });
    let load_status = use_signal(|| "Loading vehicle configuration…".to_string());

    {
        let mut config = config;
        let mut load_status = load_status;
        use_effect(move || {
            spawn(async move {
                match http_get_json::<VehicleVisualizationConfig>("/api/vehicle_visualization").await {
                    Ok(next) => {
                        load_status.set("Live model connected".to_string());
                        config.set(Some(next));
                    }
                    Err(error) => load_status.set(format!(
                        "Vehicle model configuration unavailable: {error}. Showing live fallback telemetry."
                    )),
                }
            });
        });
    }

    let _ = *TELEMETRY_RENDER_EPOCH.read();
    let phase = flight_state.read().clone();
    let cfg = config.read().clone().unwrap_or_default();
    let title = if cfg.title.trim().is_empty() {
        "Vehicle".to_string()
    } else {
        cfg.title.clone()
    };
    let position = *rocket_gps.read();
    let altitude = *rocket_altitude_m.read();
    let roll = cfg.attitude.roll.as_ref().and_then(value);
    let pitch = cfg.attitude.pitch.as_ref().and_then(value);
    let yaw = cfg.attitude.yaw.as_ref().and_then(value);
    let chute_visible = matches!(
        normalized_phase(&phase).as_str(),
        value if value.contains("drogue") || value.contains("parachute") || value.contains("descent") || value.contains("recovery")
    );

    {
        use_effect(move || {
            let _ = *TELEMETRY_RENDER_EPOCH.read();
            let _ = *model_tick.read();
            let phase = flight_state.read();
            if let Some(cfg) = config.read().as_ref() {
                sync_model_viewer(
                    viewer_id,
                    cfg,
                    &phase,
                    cfg.attitude.roll.as_ref().and_then(value),
                    cfg.attitude.pitch.as_ref().and_then(value),
                    cfg.attitude.yaw.as_ref().and_then(value),
                );
            }
        });
    }

    rsx! {
        style { {r#"
            @keyframes gs26-chute-breathe { 0%,100% { transform:scale(.96) translateY(2px); } 50% { transform:scale(1.04) translateY(-2px); } }
            @keyframes gs26-thrust { 0%,100% { transform:scaleY(.72); opacity:.68; } 50% { transform:scaleY(1.08); opacity:1; } }
            .gs26-vehicle-grid { display:grid; grid-template-columns:minmax(0,1fr); gap:12px; }
            .gs26-vehicle-grid:has(.gs26-vehicle-details > *) { grid-template-columns:minmax(300px,1.8fr) minmax(220px,1fr); }
            .gs26-vehicle-stage { transition:transform .65s ease, opacity .4s ease; }
            @media(max-width:700px) { .gs26-vehicle-grid:has(.gs26-vehicle-details > *) { grid-template-columns:1fr; } }
            @media(prefers-reduced-motion:reduce) { .gs26-vehicle-stage, .gs26-vehicle-motion { animation:none!important; transition:none!important; } }
        "#} }
        div { class:"gs26-vehicle-shell", style: "height:100%; overflow-y:auto; padding:12px; box-sizing:border-box; color:{theme.text_primary}; background:{theme.tab_shell_background};",
            div { class:"gs26-vehicle-header", style: "display:flex; justify-content:space-between; align-items:flex-start; gap:12px; flex-wrap:wrap; margin-bottom:12px;",
                div {
                    h2 { style: "margin:0; font-size:20px;", "{title}" }
                    div { style: "margin-top:4px; color:{theme.text_muted}; font-size:13px;", "Live position, configuration, propulsion, control surfaces, recovery, and ground operations." }
                }
                div { role: "status", "aria-atomic": "true", style: "padding:7px 10px; border:1px solid {theme.border}; border-radius:999px; color:{theme.text_secondary}; font-size:12px;", "{load_status.read()}" }
            }
            div { class: "gs26-vehicle-grid",
                div { style: "min-height:460px; position:relative; overflow:hidden; border:1px solid {theme.tab_shell_border}; border-radius:18px; background:radial-gradient(circle at 50% 42%, {theme.panel_background_alt}, {theme.panel_background} 68%);",
                    if !cfg.model_url.trim().is_empty() {
                        div { style: "position:absolute; inset:0;", dangerous_inner_html: "{viewer_html}" }
                    } else {
                        div { style: "height:100%; min-height:460px; display:flex; flex-direction:column; align-items:center; justify-content:center; gap:0; padding:36px; box-sizing:border-box;",
                            if chute_visible {
                                div { class: "gs26-vehicle-motion", style: "width:150px; height:58px; margin-bottom:18px; border-radius:90px 90px 12px 12px; border:3px solid {theme.info_accent}; background:linear-gradient(135deg,{theme.info_background},{theme.panel_background_alt}); animation:gs26-chute-breathe 1.4s ease-in-out infinite;" }
                            }
                            for (index, stage) in cfg.stages.iter().enumerate() {
                                {
                                    let separated = stage.separation.as_ref().and_then(value).is_some_and(|v| v >= 0.5);
                                    let offset = if separated { (index as i32 + 1) * 18 } else { 0 };
                                    rsx! {
                                        div { class: "gs26-vehicle-stage", style: "width:108px; min-height:92px; display:flex; align-items:center; justify-content:center; border:2px solid {theme.info_accent}; border-radius:14px 14px 5px 5px; background:linear-gradient(90deg,{theme.panel_background_alt},{theme.info_background},{theme.panel_background_alt}); transform:translateY({offset}px); box-shadow:0 8px 30px rgba(0,0,0,.28);",
                                            span { style: "font-size:12px; font-weight:800; text-align:center;", "{stage.label}" }
                                        }
                                    }
                                }
                            }
                            if cfg.stages.is_empty() {
                                div { style: "width:96px; height:250px; clip-path:polygon(50% 0,88% 18%,88% 82%,100% 100%,65% 91%,35% 91%,0 100%,12% 82%,12% 18%); background:linear-gradient(90deg,{theme.border_strong},{theme.text_secondary},{theme.border_strong});" }
                            }
                        }
                    }
                    div { style: "position:absolute; left:12px; top:12px; display:flex; flex-direction:column; gap:6px;",
                        span { style: "padding:6px 9px; border-radius:999px; background:{theme.overlay_background}; border:1px solid {theme.border}; font-size:12px; font-weight:800;", "{phase}" }
                        span { style: "padding:6px 9px; border-radius:999px; background:{theme.overlay_background}; border:1px solid {theme.border}; font-size:11px;", {format!("ROLL {}  PITCH {}  YAW {}", roll.map(|v| format!("{v:.1}°")).unwrap_or_else(|| "--".into()), pitch.map(|v| format!("{v:.1}°")).unwrap_or_else(|| "--".into()), yaw.map(|v| format!("{v:.1}°")).unwrap_or_else(|| "--".into()))} }
                    }
                    div { style: "position:absolute; left:12px; right:12px; bottom:12px; display:grid; grid-template-columns:repeat(3,1fr); gap:6px;",
                        {position_card(&theme, "Latitude", position.map(|v| format!("{:.6}°", v.0)).unwrap_or_else(|| "--".into()))}
                        {position_card(&theme, "Longitude", position.map(|v| format!("{:.6}°", v.1)).unwrap_or_else(|| "--".into()))}
                        {position_card(&theme, "Altitude", altitude.map(|v| format!("{v:.1} m")).unwrap_or_else(|| "--".into()))}
                    }
                }
                div { class:"gs26-vehicle-details", style: "display:flex; flex-direction:column; gap:10px;",
                    for stage in cfg.stages.iter().filter(|stage| !stage.components.is_empty() || stage.separation.is_some()) {
                        div { style: "padding:12px; border:1px solid {theme.tab_shell_border}; border-radius:16px; background:{theme.panel_background};",
                            div { style: "display:flex; justify-content:space-between; gap:8px; margin-bottom:9px;",
                                strong { "{stage.label}" }
                                span { style: "color:{theme.text_muted}; font-size:11px;", if stage.separation.as_ref().and_then(value).is_some_and(|v| v >= 0.5) { "SEPARATED" } else { "ATTACHED" } }
                            }
                            div { style: "display:grid; gap:7px;",
                                for component in stage.components.iter() {
                                    {component_row(&theme, component)}
                                }
                            }
                        }
                    }
                    if !cfg.ground_systems.is_empty() && super::gse_panel::ground_visible(&phase) {
                        div { style: "padding:12px; border:1px solid {theme.tab_shell_border}; border-radius:16px; background:{theme.panel_background};",
                            strong { "Ground operations" }
                            div { style: "display:grid; gap:7px; margin-top:9px;",
                                for component in cfg.ground_systems.iter() { {component_row(&theme, component)} }
                            }
                        }
                    }
                }
            }
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

fn component_row(theme: &ThemeConfig, component: &VehicleComponentConfig) -> Element {
    let (raw, display, fraction, active) = component_readout(component);
    let width = format!("{:.1}%", fraction * 100.0);
    let status = if raw.is_none() {
        "NO DATA"
    } else if matches!(component.kind.as_str(), "motor" | "parachute" | "link") {
        if active { "ACTIVE" } else { "STANDBY" }
    } else {
        "LIVE"
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
        let (_, _, fraction, active) = component_readout(&component);
        assert_eq!(fraction, 0.0);
        assert!(!active);
    }
}
