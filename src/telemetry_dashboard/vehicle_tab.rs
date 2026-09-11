use super::{
    TELEMETRY_RENDER_EPOCH, UrlConfig, http_get_json, js_eval, latest_telemetry_value,
    layout::ThemeConfig, types::FlightState,
};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MODEL_VIEWER_HTML: &str = r#"<model-viewer id="gs26-vehicle-model" camera-controls touch-action="pan-y" loading="eager" reveal="auto" shadow-intensity="1" exposure="1" style="width:100%;height:100%;background:transparent" aria-label="Live three-dimensional rocket model"></model-viewer>"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct VehicleVisualizationConfig {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub model_url: String,
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

fn value(binding: &VehicleTelemetryBinding) -> Option<f32> {
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
    config: &VehicleVisualizationConfig,
    phase: &str,
    roll: Option<f32>,
    pitch: Option<f32>,
    yaw: Option<f32>,
) {
    if config.model_url.trim().is_empty() {
        return;
    }
    let src = serde_json::to_string(&backend_url(&config.model_url)).unwrap_or_default();
    let alt = serde_json::to_string(if config.model_alt.trim().is_empty() {
        "Live three-dimensional rocket model"
    } else {
        config.model_alt.as_str()
    })
    .unwrap_or_default();
    let orbit = serde_json::to_string(if config.camera_orbit.trim().is_empty() {
        "35deg 70deg auto"
    } else {
        config.camera_orbit.as_str()
    })
    .unwrap_or_default();
    let animation = serde_json::to_string(&model_animation(config, phase)).unwrap_or_default();
    let orientation = serde_json::to_string(&format!(
        "{}deg {}deg {}deg",
        pitch.unwrap_or(0.0),
        yaw.unwrap_or(0.0),
        roll.unwrap_or(0.0)
    ))
    .unwrap_or_default();
    let renderer_url = if config.renderer_url.trim().is_empty() {
        "https://ajax.googleapis.com/ajax/libs/model-viewer/4.3.1/model-viewer.min.js".to_string()
    } else {
        backend_url(&config.renderer_url)
    };
    let renderer = serde_json::to_string(&renderer_url).unwrap_or_default();
    js_eval(&format!(
        r#"(() => {{
          if (!document.getElementById('gs26-model-viewer-loader')) {{
            const script = document.createElement('script');
            script.id = 'gs26-model-viewer-loader';
            script.type = 'module';
            script.src = {renderer};
            document.head.appendChild(script);
          }}
          const model = document.getElementById('gs26-vehicle-model');
          if (!model) return;
          model.setAttribute('src', {src});
          model.setAttribute('alt', {alt});
          model.setAttribute('camera-orbit', {orbit});
          model.setAttribute('orientation', {orientation});
          const animation = {animation};
          if (animation) {{
            model.setAttribute('animation-name', animation);
            model.setAttribute('autoplay', '');
          }} else {{
            model.removeAttribute('animation-name');
            model.removeAttribute('autoplay');
          }}
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
        let cfg = cfg.clone();
        let phase = phase.clone();
        use_effect(move || sync_model_viewer(&cfg, &phase, roll, pitch, yaw));
    }

    rsx! {
        style { {r#"
            @keyframes gs26-chute-breathe { 0%,100% { transform:scale(.96) translateY(2px); } 50% { transform:scale(1.04) translateY(-2px); } }
            @keyframes gs26-thrust { 0%,100% { transform:scaleY(.72); opacity:.68; } 50% { transform:scaleY(1.08); opacity:1; } }
            .gs26-vehicle-grid { display:grid; grid-template-columns:minmax(300px,1.35fr) minmax(280px,1fr); gap:12px; }
            .gs26-vehicle-stage { transition:transform .65s ease, opacity .4s ease; }
            @media(max-width:900px) { .gs26-vehicle-grid { grid-template-columns:1fr; } }
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
                        div { style: "position:absolute; inset:0;", dangerous_inner_html: "{MODEL_VIEWER_HTML}" }
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
                div { style: "display:flex; flex-direction:column; gap:10px;",
                    for stage in cfg.stages.iter() {
                        div { style: "padding:12px; border:1px solid {theme.tab_shell_border}; border-radius:16px; background:{theme.panel_background};",
                            div { style: "display:flex; justify-content:space-between; gap:8px; margin-bottom:9px;",
                                strong { "{stage.label}" }
                                span { style: "color:{theme.text_muted}; font-size:11px;", if stage.separation.as_ref().and_then(value).is_some_and(|v| v >= 0.5) { "SEPARATED" } else { "ATTACHED" } }
                            }
                            div { style: "display:grid; gap:7px;",
                                for component in stage.components.iter() {
                                    {component_row(&theme, component)}
                                }
                                if stage.components.is_empty() {
                                    div { style: "color:{theme.text_muted}; font-size:12px;", "No component bindings configured." }
                                }
                            }
                        }
                    }
                    if !cfg.ground_systems.is_empty() {
                        div { style: "padding:12px; border:1px solid {theme.tab_shell_border}; border-radius:16px; background:{theme.panel_background};",
                            strong { "Ground operations" }
                            div { style: "display:grid; gap:7px; margin-top:9px;",
                                for component in cfg.ground_systems.iter() { {component_row(&theme, component)} }
                            }
                        }
                    }
                    if cfg.stages.is_empty() && cfg.ground_systems.is_empty() {
                        div { style: "padding:14px; border:1px dashed {theme.border}; border-radius:16px; color:{theme.text_muted}; font-size:13px; line-height:1.5;", "Add stages and component telemetry bindings to /api/vehicle_visualization to display motors, fin deflection, gimbal position, air brakes, tank fullness, separation, parachutes, fill systems, and link state." }
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

        assert_eq!(config.stages.len(), 2);
        assert_eq!(config.ground_systems.len(), 2);
        assert_eq!(
            model_animation(&config, "Drogue Descent").as_deref(),
            Some("drogue-deploy")
        );
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
