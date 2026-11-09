#[path = "gse_config_compatibility.rs"]
mod config_compatibility;
use super::{
    ActionPolicyMsg, UrlConfig, js_eval, layout::ThemeConfig,
};
use crate::auth;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::{rc::Rc, sync::{Arc, atomic::{AtomicBool, Ordering}}};
use super::recording_download::ExportOwner;

// Capture at the actual gesture, never inside an async task's first poll.
fn capture_fill_owner(alive: Arc<AtomicBool>) -> Result<Rc<ExportOwner>, String> {
    ExportOwner::capture_ordinary_selected(move || alive.load(Ordering::Acquire))
}


#[derive(Clone, Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    cold_flow_target_psi: Option<f32>,
    nitrogen_target_psi: f32,
    #[serde(default = "default_pressure_step")]
    pressure_step_psi: f32,
    #[serde(default)]
    nitrogen_steps: Vec<NitrogenStep>,
    #[serde(default = "default_fill_overfill")]
    fill_overfill_kg: f32,
    #[serde(default = "default_fill_tolerance")]
    fill_tolerance_kg: f32,
    #[serde(default = "default_fill_settle")]
    fill_settle_ms: u64,
    #[serde(default)]
    turbocharge_max_added_mass_kg: Option<f32>,
    #[serde(default = "default_turbocharge_deadband")]
    turbocharge_pressure_deadband_psi: f32,
    pressure_ceiling_psi: Option<f32>,
    maximum_zero_offset_psi: Option<f32>,
    dry_self_test_confirmed: bool,
    grouped_panel: bool,
    #[serde(flatten)]
    extra: std::collections::BTreeMap<String, serde_json::Value>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            cold_flow_target_psi: None, nitrogen_target_psi: 0.0, pressure_step_psi: 0.0,
            nitrogen_steps: Vec::new(), fill_overfill_kg: 0.0, fill_tolerance_kg: 0.0,
            fill_settle_ms: 0, turbocharge_max_added_mass_kg: None,
            turbocharge_pressure_deadband_psi: default_turbocharge_deadband(),
            pressure_ceiling_psi: None, maximum_zero_offset_psi: None,
            dry_self_test_confirmed: false, grouped_panel: false,
            extra: std::collections::BTreeMap::new(),
        }
    }
}
fn default_turbocharge_deadband() -> f32 { 12.0 }
fn default_pressure_step() -> f32 {
    50.0
}
fn default_fill_overfill() -> f32 {
    0.05
}
fn default_fill_tolerance() -> f32 {
    0.07
}
fn default_fill_settle() -> u64 {
    3000
}
#[derive(Clone)]
struct FillDraft {
    overfill: String,
    tolerance: String,
    settle: String,
}
impl From<&Settings> for FillDraft {
    fn from(config: &Settings) -> Self {
        Self {
            overfill: config.fill_overfill_kg.to_string(),
            tolerance: config.fill_tolerance_kg.to_string(),
            settle: (config.fill_settle_ms as f64 / 1000.0).to_string(),
        }
    }
}
impl Default for FillDraft {
    fn default() -> Self {
        Self {
            overfill: "0.05".into(),
            tolerance: "0.07".into(),
            settle: "3".into(),
        }
    }
}
fn validated_fill(draft: &FillDraft) -> Result<(f32, f32, u64), String> {
    let overfill = draft.overfill.parse::<f32>().unwrap_or(f32::NAN);
    if !overfill.is_finite() || !(0.0..=0.1).contains(&overfill) {
        return Err("Overfill allowance must be 0–0.1 kg.".into());
    }
    let tolerance = draft.tolerance.parse::<f32>().unwrap_or(f32::NAN);
    if !tolerance.is_finite() || tolerance <= 0.0 || tolerance > 0.07 {
        return Err("Mass tolerance must be greater than 0 and at most 0.07 kg.".into());
    }
    let settle = draft.settle.parse::<f64>().unwrap_or(f64::NAN);
    if !settle.is_finite() || !(1.0..=600.0).contains(&settle) {
        return Err("Fill settling wait must be 1–600 seconds.".into());
    }
    Ok((overfill, tolerance, (settle * 1000.0).round() as u64))
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct NitrogenStep {
    target_psi: f32,
    settle_ms: u64,
    hold_ms: u64,
}
#[derive(Clone, PartialEq)]
struct NitrogenStepDraft {
    pressure: String,
    settle: String,
    hold: String,
}
impl From<&NitrogenStep> for NitrogenStepDraft {
    fn from(step: &NitrogenStep) -> Self {
        Self {
            pressure: step.target_psi.to_string(),
            settle: (step.settle_ms as f64 / 1000.0).to_string(),
            hold: (step.hold_ms as f64 / 1000.0).to_string(),
        }
    }
}
fn nitrogen_drafts(config: &Settings) -> Vec<NitrogenStepDraft> {
    if !config.nitrogen_steps.is_empty() {
        return config
            .nitrogen_steps
            .iter()
            .map(NitrogenStepDraft::from)
            .collect();
    }
    if !config.nitrogen_target_psi.is_finite()
        || config.nitrogen_target_psi <= 0.0
        || !config.pressure_step_psi.is_finite()
        || config.pressure_step_psi <= 0.0
    {
        return Vec::new();
    }
    let count = (config.nitrogen_target_psi / config.pressure_step_psi).ceil();
    // Preserve legacy configuration rather than silently dropping/truncating steps.
    if count > 32.0 {
        return Vec::new();
    }
    (1..=count as u32)
        .map(|index| {
            NitrogenStepDraft::from(&NitrogenStep {
                target_psi: (index as f32 * config.pressure_step_psi)
                    .min(config.nitrogen_target_psi),
                settle_ms: 2000,
                hold_ms: 5000,
            })
        })
        .collect()
}
// A Cold Flow-only edit must preserve the legacy empty-plan representation.
fn install_nitrogen_drafts(
    config: &mut Settings,
    rows: &[NitrogenStepDraft],
) -> Result<(), String> {
    if rows != nitrogen_drafts(config) {
        let steps = validated_nitrogen_steps(rows)?;
        config.nitrogen_target_psi = steps.last().unwrap().target_psi;
        config.nitrogen_steps = steps;
    }
    Ok(())
}

fn validated_nitrogen_steps(rows: &[NitrogenStepDraft]) -> Result<Vec<NitrogenStep>, String> {
    if rows.is_empty() || rows.len() > 32 {
        return Err("Add 1–32 pressure steps.".into());
    }
    let mut previous = 0.0;
    let mut total_ms = 0u64;
    let mut steps = Vec::with_capacity(rows.len());
    for (index, row) in rows.iter().enumerate() {
        let pressure = row.pressure.parse::<f32>().unwrap_or(f32::NAN);
        if !pressure.is_finite() || pressure <= previous {
            return Err(format!(
                "Step {}: pressure must be positive and higher than the previous step.",
                index + 1
            ));
        }
        let wait = |text: &str| -> Option<u64> {
            let seconds = text.parse::<f64>().ok()?;
            if !seconds.is_finite() || !(1.0..=600.0).contains(&seconds) {
                return None;
            }
            Some((seconds * 1000.0).round() as u64)
        };
        let (Some(settle_ms), Some(hold_ms)) = (wait(&row.settle), wait(&row.hold)) else {
            return Err(format!(
                "Step {}: each wait must be 1–600 seconds.",
                index + 1
            ));
        };
        total_ms += settle_ms + hold_ms;
        if total_ms > 3_600_000 {
            return Err("Total settling and holding time must not exceed 1 hour.".into());
        }
        steps.push(NitrogenStep {
            target_psi: pressure,
            settle_ms,
            hold_ms,
        });
        previous = pressure;
    }
    Ok(steps)
}
#[component]
pub(super) fn ControlHelp(
    title: String,
    paragraphs: Vec<String>,
    #[props(default = false)] compact: bool,
) -> Element {
    let mut open = use_signal(|| false);
    rsx! {
        button { r#type:"button",style:if compact {"min-height:44px;width:auto;justify-self:start;padding:6px 10px;background:transparent;border-color:transparent;font-size:13px;text-decoration:underline;text-underline-offset:3px;"} else {"min-height:44px;align-self:start;"},onclick:move |_|open.set(true),"About {title}" }
        if open() {
            div {style:"position:fixed;inset:0;z-index:6100;background:#000b;display:grid;place-items:center;padding:16px;",
                onkeydown:move |event| {if event.key()==Key::Escape {open.set(false);}},
                div {class:"gs26-modal",role:"dialog","aria-modal":"true","aria-label":"{title}",
                    style:"background:var(--gs26-panel-background,#122231);color:var(--gs26-app-text,#edf3fa);border:1px solid var(--gs26-border,#526579);border-radius:14px;padding:20px;width:min(100%,480px);max-height:80dvh;overflow:auto;box-sizing:border-box;display:grid;gap:12px;",
                    h3 {style:"margin:0;","{title}"}
                    for paragraph in paragraphs {p {style:"margin:0;line-height:1.5;","{paragraph}"}}
                    button {r#type:"button",autofocus:true,style:"min-height:44px;",onclick:move |_|open.set(false),"Close help"}
                }
            }
        }
    }
}
/// Inline disclosure for explanations that should stay available without
/// filling the page: a one-line summary that expands in place.
#[component]
pub(super) fn InfoNote(summary: String, paragraphs: Vec<String>) -> Element {
    rsx! {
        details { class: "gs-info",
            summary { "{summary}" }
            div { for paragraph in paragraphs { p { style: "margin:0 0 6px;", "{paragraph}" } } }
        }
    }
}
#[cfg(test)]
mod visibility_tests {
    #[test]
    fn ground_equipment_is_hidden_from_launch_onward() {
        for phase in [
            "Idle",
            "PreFill",
            "FillTest",
            "NitrogenFill",
            "NitrousFill",
            "Armed",
        ] {
            assert!(super::ground_visible(phase));
        }
        for phase in [
            "Launch", "Ascent", "Coast", "Apogee", "Descent", "Reefing", "Landed", "Recovery",
            "Aborted",
        ] {
            assert!(!super::ground_visible(phase));
        }
    }
}
/// The station's own reason Launch is disabled, if it is.
pub(super) fn launch_block_reason(policy: &ActionPolicyMsg) -> Option<String> {
    let control = policy.controls.iter().find(|control| control.cmd == "Launch")?;
    if control.enabled { return None; }
    Some(control.disabled_reason.clone().filter(|reason| !reason.trim().is_empty())
        .unwrap_or_else(|| "disabled by GroundStation".into()))
}
pub(super) fn ground_visible(phase: &str) -> bool {
    matches!(
        phase,
        "Startup" | "Idle" | "PreFill" | "FillTest" | "NitrogenFill" | "NitrousFill" | "Armed"
    )
}

#[derive(Clone, Default, Deserialize)]
struct Noise {
    average_psi: f32,
    min_psi: f32,
    max_psi: f32,
    noise_psi: f32,
    samples: usize,
}
#[derive(Clone, Deserialize)]
struct PrimaryObservation {
    fresh: bool,
    age_ms: u64,
}
#[derive(Clone, Default, Deserialize)]
struct Status {
    #[serde(default)]
    primary_observation: Option<PrimaryObservation>,
    phase: String,
    message: String,
    nitrogen_passed: bool,
    #[serde(default)]
    nitrogen_overridden: bool,
    self_test_locked: bool,
    current_step_psi: f32,
    pressure_psi: Option<f32>,
    #[serde(default)]
    valve_pressure: Option<serde_json::Value>,
    #[serde(default)]
    turbocharge: Option<serde_json::Value>,
    baseline: Option<Noise>,
    #[serde(default)]
    valves: [Option<bool>; 5],
    #[serde(default)]
    overpressure_latched: bool,
    #[serde(default)]
    overpressure: Option<serde_json::Value>,
}
pub(super) async fn delay() {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(500).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
}
fn sync_scene(status: &Status) {
    let url = serde_json::to_string(&format!(
        "{}/assets/models/gse-site.glb",
        UrlConfig::base_http()
    ))
    .unwrap();
    let renderer = serde_json::to_string(&format!(
        "{}/assets/model-viewer.min.js",
        UrlConfig::base_http()
    ))
    .unwrap();
    let animation = match status.phase.as_str() {
        "raising" | "settling" | "holding" => "nitrogen-test",
        "filling" => "nitrous-fill",
        "dumping" | "cancel_setup" => "dumping",
        _ => "",
    };
    let clip = serde_json::to_string(animation).unwrap();
    let valves = serde_json::to_string(&status.valves).unwrap();
    js_eval(&format!(
        r#"(() => {{
        if(!document.getElementById('gs26-model-viewer-loader')) {{
            const script=document.createElement('script');script.id='gs26-model-viewer-loader';script.type='module';script.src={renderer};document.head.append(script);
        }}
        const model=document.getElementById('gs26-gse-model');if(!model)return;
        if(model.getAttribute('src')!=={url})model.setAttribute('src',{url});
        if(model.getAttribute('animation-name')!=={clip}){{model.setAttribute('animation-name',{clip});if({clip}){{model.setAttribute('autoplay','');model.play?.();}}else{{model.removeAttribute('autoplay');model.pause?.();}}}}
        const states={valves},names=['pilot','vent','dump','nitrogen','nitrous'];
        for(const [i,name] of names.entries()){{const material=model.model?.materials.find(m=>m.name==='valve-'+name);material?.pbrMetallicRoughness.setBaseColorFactor(states[i]===true?[.2,.8,.62,1]:states[i]===false?[.22,.27,.31,1]:[.7,.5,.16,1]);}}
    }})();"#
    ));
}
#[component]
pub(super) fn GsePanel(
    action_policy: Signal<ActionPolicyMsg>,
    abort_only_mode: bool,
    theme: ThemeConfig,
    #[props(default = true)] show_model: bool,
) -> Element {
    let mut status = use_signal(Status::default);
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut online = use_signal(|| false);
    let mut command_owner = use_signal(|| None::<Rc<ExportOwner>>);
    let mut observation_owner = use_signal(|| None::<Rc<ExportOwner>>);
    let alive = super::recording_download::use_export_lifetime();
    let polling_alive = alive.clone();
    use_future(move || { let alive = polling_alive.clone(); async move {
        loop {
            if !alive.load(Ordering::Acquire) { return; }
            if command_owner.peek().as_ref().is_some_and(|owner| !owner.is_current()) {
                command_owner.set(None);
                busy.set(false);
                message.set("Station, login or connection changed; the previous request was retired.".into());
            }
            // Invalidate only the previously accepted observation's actual
            // context, independently of any pending response or its contents.
            if observation_owner.peek().as_ref().is_some_and(|owner| !owner.is_current()) {
                observation_owner.set(None);
                online.set(false);
                status.set(Status {phase: "unknown".into(), ..Status::default()});
            }
            let owner = ExportOwner::capture_observer_selected({let alive=alive.clone();move || alive.load(Ordering::Acquire)});
            let response = match &owner {
                Ok(owner) => super::recording_download::saved_json::<Status>("/api/gse/status", owner.clone()).await,
                Err(error) => Err(error.clone()),
            };
            if !alive.load(Ordering::Acquire) { return; }
            if owner.as_ref().is_ok_and(|owner| !owner.is_current()) {
                // No response from a retired owner may publish or clear UI.
                delay().await; continue;
            }
            match response {
                Ok(next)
                    if auth::current_status().station_role.as_deref() == Some("rrls")
                        && !next
                            .primary_observation
                            .as_ref()
                            .is_some_and(|source| source.fresh && source.age_ms <= 1500) =>
                {
                    online.set(false);
                    status.set(Status {
                        phase: "unknown".into(),
                        ..Status::default()
                    });
                    message.set("Fresh primary GSE observations are unavailable; local RRLS settings are not primary evidence.".into());
                }
                Ok(next) => {
                    observation_owner.set(owner.as_ref().ok().cloned());
                    status.set(next);
                    online.set(true);
                }
                Err(err) => {
                    online.set(false);
                    // Never keep stale confirmed valves or an active model animation
                    // after the primary observation expires or becomes unavailable.
                    status.set(Status {
                        phase: "unknown".into(),
                        ..Status::default()
                    });
                    message.set(format!("GSE status unavailable: {err}"));
                }
            }
            delay().await;
        }
    }});
    let snapshot = status.read().clone();
    use_effect(move || {
        if show_model {
            sync_scene(&status.read());
        }
    });
    let pressure_label = snapshot
        .pressure_psi
        .map(|v| format!("{v:.1} psi"))
        .unwrap_or_else(|| "—".into());
    let valve_pressure_label = snapshot
        .valve_pressure
        .as_ref()
        .and_then(|p| (p["age_ms"].as_u64()? <= 2000).then_some(p["psi"].as_f64()?))
        .map(|p| format!("{p:.1} psi"))
        .unwrap_or_else(|| "Unknown / stale".into());
    let turbo_enabled = *online.read()
        && !abort_only_mode
        && !*busy.read()
        && auth::can_send_command("Turbocharge")
        && action_policy
            .read()
            .controls
            .iter()
            .any(|c| c.cmd == "Turbocharge" && c.enabled);
    let turbo_reason = snapshot
        .turbocharge
        .as_ref()
        .and_then(|v| v["disabled_reason"].as_str())
        .unwrap_or(
            "Requires paused qualified fill, fresh valve-board pressure, routes and interlocks",
        )
        .to_string();
    rsx! {
        section { style:"border:1px solid {theme.border};border-radius:14px;overflow:hidden;background:{theme.panel_background};",
            div { style:"padding:18px 20px;border-bottom:1px solid {theme.border};display:flex;justify-content:space-between;gap:16px;align-items:center;",
                div { h3 {style:"margin:0;font-weight:600;letter-spacing:.02em;","Reconfigure GSE"}
                    p {style:"margin:6px 0 0;color:{theme.text_muted};font-size:12px;","Nitrogen · Nitrous · Fill manifold · Launch tower"}
                }
                span {style:"font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:{theme.text_muted};",if !*online.read() {"Fill status unknown"}else if snapshot.nitrogen_overridden {"Fill unlocked · override"}else if snapshot.nitrogen_passed {"Fill unlocked"}else{"Fill locked"}}
            }
            if show_model {
            div {style:"height:360px;background:radial-gradient(ellipse at center,#202b34,#0b1016);",
                div {style:"width:100%;height:100%;",dangerous_inner_html:r#"<model-viewer id="gs26-gse-model" alt="Nitrogen and nitrous tanks, fill manifold, plumbing and launch tower" camera-controls camera-orbit="35deg 68deg auto" shadow-intensity="0.6" exposure="1" style="width:100%;height:100%;background:transparent"></model-viewer>"#}
            }
            }
            div {style:"padding:18px 20px;display:grid;gap:14px;",
                div {style:"display:flex;gap:16px;flex-wrap:wrap;font-size:11px;color:{theme.text_muted};",
                    for (i, label) in ["Pilot", "Vent", "Dump", "Nitrogen", "Nitrous"].iter().enumerate() {
                        { let value=match snapshot.valves[i] {Some(true)=>"OPEN",Some(false)=>"CLOSED",None=>"UNKNOWN"}; rsx! {span {"{label}: {value}"}} }
                    }
                    span {"Latest board observation"}
                    if show_model {span {"Animation indicates sequence activity, not measured flow"}}
                }
                if let Some(reason) = launch_block_reason(&action_policy.read()) {
                    p {role:"status",style:"margin:0;padding:8px 10px;border:1px solid #b77b18;border-radius:8px;background:#3b2c11;color:#ffdc8a;", "Launch blocked: {reason}"}
                }
                OverpressureAcknowledge {
                    latched: snapshot.overpressure_latched,
                    peak_psi: snapshot.overpressure.as_ref().and_then(|o| o["peak_psi"].as_f64()),
                    detail: snapshot.overpressure.as_ref().and_then(|o| o["message"].as_str()).unwrap_or_default().to_string(),
                }
                TankSensorFault { detail: snapshot.overpressure.as_ref().and_then(tank_sensor_fault).unwrap_or_default() }
                p {style:"margin:0;color:{theme.text_primary};", "{snapshot.message}"}
                p {style:"margin:0;", "Valve-board pressure (psi): {valve_pressure_label}"}
                button {r#type:"button",disabled:!turbo_enabled,onclick:move |_| {
                    if !alive.load(Ordering::Acquire) || *busy.peek() || !*online.peek() || abort_only_mode || !auth::can_send_command("Turbocharge") || !action_policy.read().controls.iter().any(|c|c.cmd=="Turbocharge"&&c.enabled) {return;}
                    let owner = match capture_fill_owner(alive.clone()) {
                        Ok(owner) => owner,
                        Err(error) => {message.set(error); return;}
                    };
                    let request = super::station_settings_request::post_captured_no_redirect::<_,serde_json::Value>("/api/command", &"Turbocharge", Ok(owner.clone()));
                    let alive = alive.clone();
                    command_owner.set(Some(owner.clone()));
                    busy.set(true);
                    spawn(async move {
                        let result = request.await;
                        if !alive.load(Ordering::Acquire) {return;}
                        if !owner.is_current() {return;}
                        command_owner.set(None);
                        match result {
                            Ok(_)=>message.set("Turbocharge requested; valve confirmations remain required".into()),
                            Err(error)=>message.set(error),
                        }
                        busy.set(false);
                    });
                },"Turbocharge"}
                InfoNote {summary:"About Turbocharge",paragraphs:vec!["Turbocharge uses the configured nitrous pressure target. It confirms every valve closed, admits nitrogen only, then closes nitrogen and verifies settled mass. It never starts launch.".into()]}
                if !turbo_enabled {p {style:"margin:0;font-size:12px;","{turbo_reason}"}}
                div {style:"display:flex;gap:24px;flex-wrap:wrap;color:{theme.text_muted};font-size:12px;font-variant-numeric:tabular-nums;",
                    span {"GS-calibrated PT: {pressure_label}"}
                    if *online.read() {span {"Test step: {snapshot.current_step_psi:.0} psi"}}
                    else {span {"Test step: unknown"}}
                    if snapshot.self_test_locked {span {"Self-test locked for this fill session"}}
                }
                if let Some(noise)=snapshot.baseline {
                    p {style:"margin:0;font-size:12px;color:{theme.text_muted};","PT baseline {noise.average_psi:.2} psi · range {noise.min_psi:.2}–{noise.max_psi:.2} · noise ±{noise.noise_psi:.2} · {noise.samples} samples"}
                }
                if !message.read().is_empty() {p {role:"status",style:"margin:0;color:{theme.text_muted};font-size:12px;","{message}"}}
            }
        }
    }
}

#[component]
pub(super) fn PhysicalPanelStatus(status: Signal<serde_json::Value>) -> Element {
    let value = status.read();
    let Some(panel) = value.get("physical_panel") else {
        return rsx! {};
    };
    let controls = panel["controls"].as_array().cloned().unwrap_or_default();
    let key = if panel["key_enabled"].as_bool() == Some(true) {
        "Enabled"
    } else {
        "Locked"
    };
    rsx! {
        details {class:"gs26-physical-panel",
            summary {style:"cursor:pointer;min-height:44px;display:flex;align-items:center;","Physical panel · {key}"}
            div {style:"display:grid;grid-template-columns:repeat(auto-fit,minmax(min(180px,100%),1fr));gap:8px;",
                for control in controls {
                    {let enabled=control["enabled"].as_bool()==Some(true);
                    let active=control["actuated"].as_bool();
                    let blink=serde_json::from_value::<super::BlinkMode>(control["blink"].clone()).unwrap_or(super::BlinkMode::None);
                    let animation=super::blink::action_animation_style(enabled,blink,active);
                    let blinking=enabled && control["blink"].as_str().is_some_and(|s|s!="none");
                    let state=if !enabled {"Locked"}else if blinking {"Blinking"}else if active==Some(true) {"Active"}else {"Ready"};
                    let color=if !enabled {"#8390a0"}else if active==Some(true) {"#62d99c"}else {"#77bcff"};
                    rsx! {
                        div {style:"border:1px solid #526579;border-radius:8px;padding:10px;display:grid;gap:5px;min-width:0;",
                            strong {"{control[\"physical\"].as_str().unwrap_or(\"Control\")}"}
                            span {style:"overflow-wrap:anywhere;","{control[\"action\"].as_str().unwrap_or(\"Unknown\")}"}
                            span {style:"color:{color};font-size:12px;{animation}","● {state}"}
                        }
                    }}
                }
            }
            ControlHelp {title:"physical panel feedback",paragraphs:vec![
                "Each tile shows the physical button label, its current mapped action and the backend's requested indicator state.".into(),
                "This mirrors the panel mapping; it does not measure physical lamps or prove valve movement. Use fresh board confirmations for valve state.".into()
            ]}
        }
    }
}

// Reuse the existing policy at the actual opening/confirmation gesture, so a
// stale rendered enabled button cannot authorize a changed operation state.
fn qualification_access(value: &serde_json::Value, disabled: bool, busy: bool) -> (bool, bool) {
    let Some(qualification) = value.get("qualification") else {return (false, false);};
    let can_reset = !disabled && !busy && auth::can_send_command("ValveSelfTest")
        && value["request_gate"]["flight_state"].as_str().is_some_and(|s| matches!(s, "Startup" | "Idle"));
    let can_override = can_reset && auth::can_send_command("NitrogenTest")
        && value["topology_stable"].as_bool() == Some(true)
        && qualification["fault"].as_str().is_none()
        && value["storage_error"].is_null()
        && value["mission_stage"].as_str().unwrap_or("idle") == "idle"
        && qualification["self_test"].as_bool() == Some(true)
        && qualification["nitrogen_test"].as_bool() != Some(true)
        && qualification["nitrogen_override"]["operator"].as_str().is_none();
    (can_reset, can_override)
}

#[component]
pub(super) fn QualificationStatus(status: Signal<serde_json::Value>, disabled: bool) -> Element {
    let mut confirm = use_signal(|| false);
    let mut override_open = use_signal(|| false);
    let mut override_reason = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let mut reset_owner = use_signal(|| None::<Rc<ExportOwner>>);
    let mut override_owner = use_signal(|| None::<Rc<ExportOwner>>);
    let mut pending_owner = use_signal(|| None::<Rc<ExportOwner>>);
    let alive = super::recording_download::use_export_lifetime();
    let reset_open_alive = alive.clone();
    let override_open_alive = alive.clone();
    let reset_confirm_alive = alive.clone();
    let override_confirm_alive = alive.clone();
    use_future(move || {let alive = alive.clone(); async move {
        loop {
            if !alive.load(Ordering::Acquire) {return;}
            let mut retired = false;
            if reset_owner.peek().as_ref().is_some_and(|owner| !owner.is_current()) {
                reset_owner.set(None);confirm.set(false);retired = true;
            }
            if override_owner.peek().as_ref().is_some_and(|owner| !owner.is_current()) {
                override_owner.set(None);override_open.set(false);retired = true;
            }
            // An inactive dialog must not clear another, newer pending request.
            if pending_owner.peek().as_ref().is_some_and(|owner| !owner.is_current()) {
                pending_owner.set(None);busy.set(false);retired = true;
            }
            if retired {message.set("Station, login or connection changed. Review a new qualification confirmation.".into());}
            delay().await;
        }
    }});
    let value = status.read().clone();
    let Some(qualification) = value.get("qualification") else {
        return rsx! {};
    };
    let fault = qualification["fault"].as_str().map(str::to_string);
    let stable = value["topology_stable"].as_bool() == Some(true);
    let mission_stage = value["mission_stage"]
        .as_str()
        .unwrap_or("idle")
        .to_string();
    let override_actor = qualification["nitrogen_override"]["operator"]
        .as_str()
        .map(str::to_string);
    let (can_reset, can_override) = qualification_access(&value, disabled, *busy.read());
    let qd_label = value["qd"].as_str().unwrap_or("Waiting for QD").to_string();
    rsx! {
        section {"aria-label":"Operational qualification",class:"gs26-qualification",
            style:"padding:12px;border:1px solid #526579;border-radius:10px;display:grid;gap:12px;",
            div {"aria-label":"Mission workflow",style:"display:flex;gap:8px;flex-wrap:wrap;",
                for (key,label) in [("idle","Idle"),("fill","Fill"),("armed","Armed"),("fire","Fire")] {
                    span {"aria-current":if mission_stage==key {"step"}else{"false"},style:if mission_stage==key {"padding:6px 10px;border-radius:8px;background:#69b4ce;color:#071018;font-weight:700;"}else{"padding:6px 10px;border-radius:8px;background:#172b3a;color:#a6b5c8;"},"{label}"}
                }
            }
            strong {"Operation checks"}
            div {style:"display:flex;gap:10px;flex-wrap:wrap;",
                for (key,label) in [("self_test","Valve test"),("nitrogen_test","Nitrogen test"),("fill_target","Fill target")] {
                    span {style:"padding:6px 10px;border-radius:8px;background:#172b3a;color:#edf3fa;",
                        if key=="nitrogen_test" && override_actor.is_some() {"⚠ Cold flow overridden"}else if qualification[key].as_bool()==Some(true) {"✓ {label}"}else{"○ {label}"}
                    }
                }
            }
            if let Some(actor)=override_actor {p {style:"margin:0;color:#e6b85c;","Cold-flow override by {actor}"}}
            if can_override {button {style:"min-height:44px;justify-self:start;",onclick:move |_|{
                if !override_open_alive.load(Ordering::Acquire) || !qualification_access(&status.peek(), disabled, *busy.peek()).1 {return;}
                match capture_fill_owner(override_open_alive.clone()) {
                    Ok(owner) => {override_owner.set(Some(owner));override_open.set(true);override_reason.set(String::new());message.set(String::new());},
                    Err(error) => message.set(error),
                }
            },"Override cold-flow requirement"}}
            if !stable {p {role:"status",style:"margin:0;color:#e6b85c;","Waiting for stable fill-system connections"}}
            if let Some(fault)=fault {p {role:"alert",style:"margin:0;color:#ff8f8f;","Safety fault: {fault}. Recover to Idle and reset checks."}}
            if let Some(error)=value["storage_error"].as_str() {p {role:"alert","Checks cannot be saved: {error}"}}
            if value["launch_queued"].as_bool()==Some(true) {p {role:"status","Launch queued · {qd_label}"}}
            button {style:"min-height:44px;justify-self:start;",disabled:!can_reset,onclick:move |_|{
                if !reset_open_alive.load(Ordering::Acquire) || !qualification_access(&status.peek(), disabled, *busy.peek()).0 {return;}
                match capture_fill_owner(reset_open_alive.clone()) {
                    Ok(owner) => {reset_owner.set(Some(owner));confirm.set(true);message.set(String::new());},
                    Err(error) => message.set(error),
                }
            },"Reset operation checks"}
            p {role:"status",style:"margin:0;","{message}"}
            if *override_open.read() {
                div {style:"position:fixed;inset:0;background:#000a;z-index:6000;display:grid;place-items:center;padding:20px;",
                    div {class:"gs26-modal",role:"dialog","aria-modal":"true","aria-label":"Override nitrogen cold flow",
                        style:"background:var(--gs26-panel-background,#122231);color:var(--gs26-app-text,#edf3fa);border:1px solid var(--gs26-border,#526579);border-radius:14px;padding:20px;width:min(100%,420px);box-sizing:border-box;display:grid;gap:16px;",
                        h3 {style:"margin:0;","Override cold-flow requirement?"}
                        p {style:"margin:0;","This records an operator override, not a passed test. Fresh pressure, valve and connection checks still apply."}
                        label {"Reason" textarea {"aria-label":"Cold-flow override reason",maxlength:"500",value:override_reason(),oninput:move |event|override_reason.set(event.value()),style:"width:100%;min-height:88px;box-sizing:border-box;"}}
                        div {style:"display:flex;gap:12px;flex-wrap:wrap;",
                            button {disabled:*busy.read(),onclick:move |_|{if *busy.peek() {return;} override_open.set(false);override_owner.set(None);},"Cancel override"}
                            button {class:"gs26-modal-danger",disabled:!can_override||override_reason.read().trim().is_empty(),onclick:move |_|{
                                if !override_confirm_alive.load(Ordering::Acquire) || !qualification_access(&status.peek(), disabled, *busy.peek()).1 {return;}
                                let reason=override_reason.peek().trim().to_string();if reason.is_empty() {return;}
                                let Some(owner)=override_owner.peek().clone().filter(|owner| owner.is_current()) else {
                                    override_open.set(false);override_owner.set(None);
                                    message.set("Station, login or connection changed. Review a new qualification confirmation.".into());return;
                                };
                                let request=super::station_settings_request::post_captured_no_redirect::<_,serde_json::Value>("/api/gse/cold-flow/override",&serde_json::json!({"reason":reason}),Ok(owner.clone()));
                                let alive=override_confirm_alive.clone();pending_owner.set(Some(owner.clone()));busy.set(true);
                                spawn(async move {
                                    let result=request.await;
                                    if !alive.load(Ordering::Acquire) || !owner.is_current() {return;}
                                    pending_owner.set(None);
                                    match result {
                                        Ok(_)=>{message.set("Cold-flow override saved".into());override_open.set(false);override_owner.set(None);},
                                        Err(error)=>message.set(error),
                                    }
                                    busy.set(false);
                                });
                            },"Confirm override"}
                        }
                    }
                }
            }
            if *confirm.read() {
                div {style:"position:fixed;inset:0;background:#000a;z-index:6000;display:grid;place-items:center;padding:20px;",
                    div {class:"gs26-modal",role:"dialog","aria-modal":"true","aria-label":"Reset operation checks",
                        style:"background:var(--gs26-panel-background,#122231);color:var(--gs26-app-text,#edf3fa);border:1px solid var(--gs26-border,#526579);border-radius:14px;padding:20px;max-width:420px;display:grid;gap:16px;",
                        h3 {style:"margin:0;","Reset operation checks?"}
                        p {style:"margin:0;","Valve test, nitrogen test and fill qualification will be cleared. Outputs will not be actuated."}
                        div {style:"display:flex;gap:12px;flex-wrap:wrap;",
                            button {style:"min-height:44px;",disabled:*busy.read(),onclick:move |_|{if *busy.peek() {return;} confirm.set(false);reset_owner.set(None);},"Cancel"}
                            button {class:"gs26-modal-danger",style:"min-height:44px;",disabled:!can_reset,onclick:move |_| {
                                if !reset_confirm_alive.load(Ordering::Acquire) || !qualification_access(&status.peek(), disabled, *busy.peek()).0 {return;}
                                let Some(owner)=reset_owner.peek().clone().filter(|owner| owner.is_current()) else {
                                    confirm.set(false);reset_owner.set(None);
                                    message.set("Station, login or connection changed. Review a new qualification confirmation.".into());return;
                                };
                                let request=super::station_settings_request::post_captured_no_redirect::<_,serde_json::Value>("/api/gse/qualification/reset",&serde_json::json!({}),Ok(owner.clone()));
                                let alive=reset_confirm_alive.clone();pending_owner.set(Some(owner.clone()));busy.set(true);
                                spawn(async move {
                                    let result=request.await;
                                    if !alive.load(Ordering::Acquire) || !owner.is_current() {return;}
                                    pending_owner.set(None);
                                    match result {
                                        Ok(_)=>{message.set("Operation checks reset".into());confirm.set(false);reset_owner.set(None);},
                                        Err(error)=>message.set(error),
                                    }
                                    busy.set(false);
                                });
                            },"Confirm reset"}
                        }
                    }
                }
            }
        }
    }
}

/// Remains available while aborted, including when normal sequence controls lock.
/// Shown while the station latches a tank overpressure. Acknowledging commands
/// nothing; the station refuses it unless a fresh Valve Board pressure is below
/// the relief limit, and it also acknowledges the Valve Board's relief hold.
/// HITL: an operator who knows the tank is good skips the nitrogen test and
/// goes straight to fill. The station decides availability and says why not.
#[component]
pub(super) fn HitlNitrogenOverride(status: Signal<serde_json::Value>, disabled: bool) -> Element {
    let mut open = use_signal(|| false);
    let mut reason = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let mut owner = use_signal(|| None::<Rc<ExportOwner>>);
    let alive = super::recording_download::use_export_lifetime();
    let open_alive = alive.clone();
    let confirm_alive = alive.clone();
    let value = status.read().clone();
    let Some(state) = value.get("nitrogen_override").cloned() else {
        return rsx! {};
    };
    let permitted = !disabled && auth::can_send_command("NitrogenTest");
    let available = state["available"].as_bool() == Some(true) && permitted;
    let blocked = state["blocked_reason"].as_str().map(str::to_string);
    rsx! {
        section {"aria-label":"Nitrogen test override",style:"padding:12px;border:1px solid #526579;border-radius:10px;display:grid;gap:8px;",
            if state["active"].as_bool() == Some(true) {
                p {style:"margin:0;color:#e6b85c;","⚠ Nitrogen test overridden; fill is available."}
            } else {
                button {style:"min-height:44px;justify-self:start;",disabled:!available||*busy.read(),onclick:move |_|{
                    if !open_alive.load(Ordering::Acquire) {return;}
                    match capture_fill_owner(open_alive.clone()) {
                        Ok(captured) => {owner.set(Some(captured));open.set(true);reason.set(String::new());message.set(String::new());},
                        Err(error) => message.set(error),
                    }
                },"Override nitrogen test…"}
                if let Some(blocked)=blocked {p {role:"status",style:"margin:0;","Override unavailable: {blocked}."}}
                else if !permitted {p {role:"status",style:"margin:0;","Sign in with an account allowed to run the nitrogen test."}}
            }
            if !message.read().is_empty() {p {role:"status",style:"margin:0;","{message}"}}
            if *open.read() {
                div {style:"position:fixed;inset:0;background:#000a;z-index:6000;display:grid;place-items:center;padding:20px;",
                    div {class:"gs26-modal",role:"dialog","aria-modal":"true","aria-label":"Override nitrogen test",
                        style:"background:var(--gs26-panel-background,#122231);color:var(--gs26-app-text,#edf3fa);border:1px solid var(--gs26-border,#526579);border-radius:14px;padding:20px;width:min(100%,420px);box-sizing:border-box;display:grid;gap:12px;",
                        h3 {style:"margin:0;","Skip the nitrogen test?"}
                        p {style:"margin:0;","For a tank you know is good. Fill's pressure, valve and interlock checks still apply."}
                        label {"Reason" textarea {"aria-label":"Nitrogen test override reason",maxlength:"500",value:reason(),oninput:move |event|reason.set(event.value()),style:"width:100%;min-height:72px;box-sizing:border-box;"}}
                        div {style:"display:flex;gap:12px;flex-wrap:wrap;",
                            button {disabled:*busy.read(),onclick:move |_|{if *busy.peek() {return;} open.set(false);owner.set(None);},"Cancel"}
                            button {class:"gs26-modal-danger",disabled:*busy.read()||reason.read().trim().is_empty(),onclick:move |_|{
                                if !confirm_alive.load(Ordering::Acquire) {return;}
                                let text=reason.peek().trim().to_string();if text.is_empty() {return;}
                                let Some(captured)=owner.peek().clone().filter(|o| o.is_current()) else {
                                    open.set(false);owner.set(None);
                                    message.set("Station, login or connection changed. Open the override again.".into());return;
                                };
                                let request=super::station_settings_request::post_captured_no_redirect::<_,serde_json::Value>("/api/gse/cold-flow/override",&serde_json::json!({"reason":text}),Ok(captured.clone()));
                                let alive=confirm_alive.clone();busy.set(true);
                                spawn(async move {
                                    let result=request.await;
                                    if !alive.load(Ordering::Acquire) || !captured.is_current() {return;}
                                    match result {
                                        Ok(_)=>{message.set("Nitrogen test overridden. Fill is available.".into());open.set(false);owner.set(None);},
                                        Err(error)=>message.set(error),
                                    }
                                    busy.set(false);
                                });
                            },"Skip nitrogen test"}
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub(super) fn OverpressureAcknowledge(latched: bool, peak_psi: Option<f64>, detail: String) -> Element {
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let alive = super::recording_download::use_export_lifetime();
    if !latched && message.read().is_empty() { return rsx! {}; }
    let peak = peak_psi.map(|p| format!("{p:.0} psi")).unwrap_or_else(|| "unknown".into());
    let permitted = auth::can_send_command("CancelFill");
    rsx! {
        div { role:"alert", style:"display:grid;gap:8px;justify-items:start;padding:12px 14px;border:1px solid #c0392b;border-radius:10px;background:#3a1512;color:#ffd6d0;",
            if latched {
                strong { "Tank overpressure latched · peak {peak}" }
                if !detail.is_empty() { p { style:"margin:0;max-width:60ch;", "{detail}" } }
                p { style:"margin:0;max-width:60ch;font-size:12px;", "Launch and fill stay blocked until this is acknowledged. The station accepts it only with a fresh Valve Board reading below the relief pressure, and it also releases the Valve Board relief hold. No valve is moved. Then use Reset all boards to clear board aborts." }
                button { r#type:"button", disabled: *busy.read() || !permitted,
                    onclick: move |_| {
                        if !alive.load(Ordering::Acquire) || *busy.peek() || !auth::can_send_command("CancelFill") {return;}
                        let owner = match capture_fill_owner(alive.clone()) { Ok(owner) => owner, Err(error) => {message.set(error); return;} };
                        let request = super::station_settings_request::post_captured_no_redirect::<_,serde_json::Value>("/api/gse/overpressure/acknowledge", &serde_json::json!({}), Ok(owner.clone()));
                        let alive = alive.clone();
                        busy.set(true);
                        spawn(async move {
                            let result = request.await;
                            if !alive.load(Ordering::Acquire) || !owner.is_current() {return;}
                            match result {
                                Ok(_) => message.set("Overpressure acknowledged. Valves unchanged. Use Reset all boards to clear board aborts.".into()),
                                Err(error) => message.set(error),
                            }
                            busy.set(false);
                        });
                    },
                    "Acknowledge overpressure"
                }
                if !permitted { p { style:"margin:0;font-size:12px;", "Requires permission to send fill commands." } }
            }
            if !message.read().is_empty() { p { role:"status", style:"margin:0;", "{message}" } }
        }
    }
}

/// Operator text for a tank pressure sensor fault from `/api/gse/status`
/// (`overpressure.sensor`): an implausible step (typically an unconnected,
/// floating transducer) or a Valve Board sensor report. Never an overpressure.
pub(super) fn tank_sensor_fault(overpressure: &serde_json::Value) -> Option<String> {
    let sensor = &overpressure["sensor"];
    if !sensor["fault"].as_bool().unwrap_or(false) {
        return None;
    }
    let detail = sensor["message"].as_str().unwrap_or("Tank pressure sensor fault.").to_string();
    Some(if sensor["settling"].as_bool().unwrap_or(false) {
        format!("{detail} Readings are settling; overpressure protection ignores them until they do.")
    } else {
        detail
    })
}

/// Shown while the tank pressure sensor is implausible, invalid or stale. A
/// warning, not an overpressure: nothing was commanded.
#[component]
pub(super) fn TankSensorFault(detail: String) -> Element {
    if detail.is_empty() { return rsx! {}; }
    rsx! {
        div { role:"status", style:"display:grid;gap:6px;justify-items:start;padding:10px 14px;border:1px solid #d39e00;border-radius:10px;background:#3a2e08;color:#ffe9a8;",
            strong { "Tank pressure sensor implausible / not connected" }
            p { style:"margin:0;max-width:60ch;", "{detail}" }
            p { style:"margin:0;max-width:60ch;font-size:12px;", "Check the tank pressure transducer connection. This is not an overpressure and no valve was moved." }
        }
    }
}

#[cfg(test)]
mod tank_sensor_fault_tests {
    #[test]
    fn sensor_fault_is_shown_only_when_reported() {
        assert_eq!(super::tank_sensor_fault(&serde_json::json!({"latched":false})), None);
        assert_eq!(super::tank_sensor_fault(&serde_json::json!({"sensor":{"fault":false,"message":null}})), None);
        let shown = super::tank_sensor_fault(&serde_json::json!({"sensor":{"fault":true,"settling":true,
            "message":"Tank pressure sensor implausible or not connected: valve-board reading jumped 0 -> 1038 psi in 20 ms."}})).unwrap();
        assert!(shown.starts_with("Tank pressure sensor implausible or not connected") && shown.contains("settling"), "{shown}");
        let board = super::tank_sensor_fault(&serde_json::json!({"sensor":{"fault":true,"settling":false,
            "message":"Valve Board: tank pressure sensor stale; overpressure protection has no reading."}})).unwrap();
        assert!(!board.contains("settling"));
    }
}

#[component]
pub(super) fn ClearGseAbort() -> Element {
    let mut confirming = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let mut confirmation_owner = use_signal(|| None::<Rc<ExportOwner>>);
    let alive = super::recording_download::use_export_lifetime();
    let opening_alive = alive.clone();
    let watching_alive = alive.clone();
    use_future(move || {let alive = watching_alive.clone(); async move {
        loop {
            if !alive.load(Ordering::Acquire) {return;}
            if confirmation_owner.peek().as_ref().is_some_and(|owner| !owner.is_current()) {
                confirmation_owner.set(None);
                confirming.set(false);
                busy.set(false);
                message.set("Station, login or connection changed. Review a new clear-abort confirmation.".into());
            }
            delay().await;
        }
    }});
    rsx! {
        div { style: "display:grid;gap:8px;justify-items:start;",
            button {
                r#type:"button",
                disabled: *busy.read() || !auth::can_send_command("ValveSelfTest"),
                onclick: move |_| {
                    if !opening_alive.load(Ordering::Acquire) || *busy.peek() || !auth::can_send_command("ValveSelfTest") {return;}
                    match capture_fill_owner(opening_alive.clone()) {
                        Ok(owner) => {confirmation_owner.set(Some(owner)); confirming.set(true); message.set(String::new());},
                        Err(error) => message.set(error),
                    }
                },
                "Clear GSE abort"
            }
            if *confirming.read() {
                p { style:"margin:0;max-width:48ch;", "Confirm the system is safe. This clears the ground station operation abort, returns it to Idle and resets operation checks. It does not clear individual board aborts or command outputs. In Connectivity → Board state and recovery, request fresh status and use Recover Abort for boards that support it. Unsupported or unavailable board recovery requires local servicing." }
                div { style:"display:flex;gap:8px;flex-wrap:wrap;",
                    button { r#type:"button", disabled:*busy.read(), onclick:move |_| {
                        if !alive.load(Ordering::Acquire) || *busy.peek() || !auth::can_send_command("ValveSelfTest") {return;}
                        let Some(owner) = confirmation_owner.peek().clone().filter(|owner| owner.is_current()) else {
                            confirming.set(false);
                            confirmation_owner.set(None);
                            message.set("Station, login or connection changed. Review a new clear-abort confirmation.".into());
                            return;
                        };
                        let request = super::station_settings_request::post_captured_no_redirect::<_,serde_json::Value>("/api/gse/abort/clear", &serde_json::json!({}), Ok(owner.clone()));
                        let alive = alive.clone();
                        busy.set(true);
                        spawn(async move {
                            let result = request.await;
                            if !alive.load(Ordering::Acquire) {return;}
                            if !owner.is_current() {return;}
                            match result {
                                Ok(value) => {confirming.set(false);confirmation_owner.set(None);message.set(clear_abort_result_message(&value).into());},
                                Err(error) => message.set(error),
                            }
                            busy.set(false);
                        });
                    }, "Confirm clear abort" }
                    button {r#type:"button",disabled:*busy.read(),onclick:move |_|{confirming.set(false);confirmation_owner.set(None);},"Cancel"}
                }
            }
            if !message.read().is_empty() {p {role:"status",style:"margin:0;","{message}"}}
        }
    }
}

/// The primary answers `{"cleared":true}`; an RRLS answers with the status of
/// its request to the primary, which is not yet a cleared abort.
fn clear_abort_result_message(value: &serde_json::Value) -> &'static str {
    if value["cleared"].as_bool() == Some(true) {
        "GSE abort cleared. Repeat operation checks before filling."
    } else {
        "Clear-abort request sent to the primary. Check the primary's GSE state; it is not confirmed here."
    }
}

#[cfg(test)]
mod clear_abort_message_tests {
    #[test]
    fn launch_block_reason_uses_the_station_reason() {
        use super::{ActionPolicyMsg, launch_block_reason};
        let policy: ActionPolicyMsg = serde_json::from_value(serde_json::json!({"key_enabled":true,"software_buttons_enabled":true,"controls":[
            {"cmd":"Launch","enabled":false,"blink":"none","actuated":null,"disabled_reason":"Launch is locked after the last attempt. Use Reset all boards to unlock it."}]})).unwrap();
        assert_eq!(launch_block_reason(&policy).as_deref(), Some("Launch is locked after the last attempt. Use Reset all boards to unlock it."));
        let open: ActionPolicyMsg = serde_json::from_value(serde_json::json!({"key_enabled":true,"software_buttons_enabled":true,"controls":[
            {"cmd":"Launch","enabled":true,"blink":"none","actuated":null}]})).unwrap();
        assert!(launch_block_reason(&open).is_none());
    }
    #[test]
    fn rrls_request_is_not_reported_as_cleared() {
        assert!(super::clear_abort_result_message(&serde_json::json!({"cleared":true})).starts_with("GSE abort cleared"));
        let proxied = serde_json::json!({"pending":{"operation":"ClearGseAbort","phase":"pending_primary"}});
        assert!(super::clear_abort_result_message(&proxied).contains("not confirmed"));
    }
}

#[cfg(test)]
mod nitrogen_settings_tests {
    use super::*;
    #[test]
    fn cold_target_save_preserves_legacy_nitrogen_plan_and_explicit_edit_still_installs() {
        let mut config: Settings =
            serde_json::from_value(serde_json::json!({"nitrogen_target_psi":100.,"dry_self_test_confirmed":false,"grouped_panel":true})).unwrap();
        let before = serde_json::to_value(&config).unwrap();
        let rows = nitrogen_drafts(&config);
        config.cold_flow_target_psi = Some(90.);
        install_nitrogen_drafts(&mut config, &rows).unwrap();
        let after = serde_json::to_value(&config).unwrap();
        assert_eq!(before["nitrogen_steps"], after["nitrogen_steps"]);
        assert_eq!(before["nitrogen_target_psi"], after["nitrogen_target_psi"]);
        let mut changed = rows;
        changed[0].pressure = "45".into();
        install_nitrogen_drafts(&mut config, &changed).unwrap();
        assert!(!config.nitrogen_steps.is_empty());
        assert_eq!(config.nitrogen_steps[0].target_psi, 45.);
    }
    fn row(pressure: &str, settle: &str, hold: &str) -> NitrogenStepDraft {
        NitrogenStepDraft {
            pressure: pressure.into(),
            settle: settle.into(),
            hold: hold.into(),
        }
    }
    #[test]
    fn custom_targets_and_per_step_waits_round_trip_without_losing_other_settings() {
        let mut config: Settings = serde_json::from_value(serde_json::json!({
            "nitrogen_target_psi":500.0,"pressure_step_psi":50.0,"pressure_ceiling_psi":650.0,
            "maximum_zero_offset_psi":20.0,"dry_self_test_confirmed":false,"grouped_panel":true,
            "future_fill_setting":{"tolerance":0.1}
        }))
        .unwrap();
        config.nitrogen_steps = validated_nitrogen_steps(&[
            row("100", "2", "5"),
            row("300", "3.5", "12"),
            row("500", "10", "30"),
        ])
        .unwrap();
        config.nitrogen_target_psi = config.nitrogen_steps.last().unwrap().target_psi;
        let encoded = serde_json::to_value(&config).unwrap();
        assert_eq!(encoded["nitrogen_steps"][1]["settle_ms"], 3500);
        assert_eq!(encoded["future_fill_setting"]["tolerance"], 0.1);
        assert_eq!(encoded["pressure_ceiling_psi"], 650.0);
        let restored: Settings = serde_json::from_value(encoded).unwrap();
        assert_eq!(
            validated_nitrogen_steps(&nitrogen_drafts(&restored)).unwrap(),
            config.nitrogen_steps
        );
    }
    #[test]
    fn fill_defaults_units_and_limits_are_preserved() {
        let config:Settings=serde_json::from_value(serde_json::json!({"nitrogen_target_psi":100.0,"pressure_step_psi":50.0,"pressure_ceiling_psi":null,"maximum_zero_offset_psi":20.0,"dry_self_test_confirmed":false,"grouped_panel":false})).unwrap();
        assert_eq!(
            validated_fill(&FillDraft::from(&config)).unwrap(),
            (0.05, 0.07, 3000)
        );
        assert_eq!(
            validated_fill(&FillDraft {
                overfill: "0.1".into(),
                tolerance: "0.03".into(),
                settle: "4.25".into()
            })
            .unwrap(),
            (0.1, 0.03, 4250)
        );
        for (overfill, tolerance, settle) in [
            ("0.101", "0.07", "3"),
            ("NaN", "0.07", "3"),
            ("0.05", "0", "3"),
            ("0.05", "0.071", "3"),
            ("0.05", "0.07", "0"),
            ("0.05", "0.07", "601"),
        ] {
            assert!(
                validated_fill(&FillDraft {
                    overfill: overfill.into(),
                    tolerance: tolerance.into(),
                    settle: settle.into()
                })
                .is_err()
            );
        }
    }
    #[test]
    fn viewing_legacy_profile_does_not_modify_it() {
        let config = Settings {
            cold_flow_target_psi: None,
            nitrogen_target_psi: 125.0,
            pressure_step_psi: 50.0,
            ..Settings::default()
        };
        let steps = validated_nitrogen_steps(&nitrogen_drafts(&config)).unwrap();
        assert_eq!(
            steps.iter().map(|s| s.target_psi).collect::<Vec<_>>(),
            vec![50.0, 100.0, 125.0]
        );
        assert!(
            steps
                .iter()
                .all(|s| s.settle_ms == 2000 && s.hold_ms == 5000)
        );
        assert!(config.nitrogen_steps.is_empty());
        assert!(
            nitrogen_drafts(&Settings {
                pressure_step_psi: 0.0,
                ..config
            })
            .is_empty()
        );
    }
    #[test]
    fn rejects_invalid_pressure_waits_and_unbounded_plans() {
        for rows in [
            vec![],
            vec![row("NaN", "2", "5")],
            vec![row("0", "2", "5")],
            vec![row("300", "2", "5"), row("100", "2", "5")],
            vec![row("100", "", "5")],
            vec![row("100", "0", "5")],
            vec![row("100", "2", "601")],
            vec![row("100", "inf", "5")],
        ] {
            assert!(validated_nitrogen_steps(&rows).is_err());
        }
        assert!(
            validated_nitrogen_steps(
                &(1..=33)
                    .map(|i| row(&i.to_string(), "1", "1"))
                    .collect::<Vec<_>>()
            )
            .is_err()
        );
        assert!(
            validated_nitrogen_steps(
                &(1..=4)
                    .map(|i| row(&i.to_string(), "600", "600"))
                    .collect::<Vec<_>>()
            )
            .is_err()
        );
        assert!(validated_nitrogen_steps(&[row("100", "1", "600")]).is_ok());
    }
    #[test]
    fn cold_flow_optional_target_roundtrips_without_changing_other_settings() {
        let mut settings = Settings {
            nitrogen_target_psi: 120.,
            turbocharge_max_added_mass_kg: Some(0.3),
            fill_tolerance_kg: 0.07,
            ..Default::default()
        };
        assert!(settings.cold_flow_target_psi.is_none());
        settings.cold_flow_target_psi = Some(95.);
        let json = serde_json::to_value(&settings).unwrap();
        let restored: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(restored.cold_flow_target_psi, Some(95.));
        assert_eq!(restored.nitrogen_target_psi, 120.);
        assert_eq!(restored.turbocharge_max_added_mass_kg, Some(0.3));
        assert_eq!(restored.fill_tolerance_kg, 0.07);
    }
    #[test]
    fn turbocharge_allowance_is_explicit_and_roundtrips_without_liquid_tolerance_reuse() {
        let mut settings = Settings::default();
        assert!(settings.turbocharge_max_added_mass_kg.is_none());
        settings.fill_tolerance_kg = 0.07;
        settings.turbocharge_max_added_mass_kg = Some(0.3);
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["turbocharge_max_added_mass_kg"], 0.3_f32);
        let restored: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(restored.turbocharge_max_added_mass_kg, Some(0.3));
        assert_eq!(restored.fill_tolerance_kg, 0.07);
    }
}

/// Configuration only: mounting this component never starts a sequence or valve.
#[component]
pub(super) fn GseConfiguration(abort_only_mode: bool, theme: ThemeConfig) -> Element {
    let mut settings = use_signal(|| None::<Settings>);
    let mut config_grant = use_signal(|| None::<config_compatibility::Grant>);
    let mut nitrogen_rows = use_signal(Vec::<NitrogenStepDraft>::new);
    let mut fill_draft = use_signal(FillDraft::default);
    let mut status = use_signal(Status::default);
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut online = use_signal(|| false);
    use_future(move || async move {
        loop {
            if config_grant.peek().as_ref().is_none_or(|grant| !grant.current()) {
                // Retire the old schema capability BEFORE obtaining another
                // station/session's actual configuration. Never reuse a draft.
                config_grant.set(None); settings.set(None); online.set(false); message.set(String::new());
                match config_compatibility::Grant::load().await {
                    Ok((cfg, grant)) => {
                        nitrogen_rows.set(nitrogen_drafts(&cfg));
                        fill_draft.set(FillDraft::from(&cfg));
                        settings.set(Some(cfg)); config_grant.set(Some(grant));
                    }
                    Err(error) => { message.set(format!("GSE configuration unavailable: {error}")); delay().await; continue; }
                }
            }
            let Some(grant) = config_grant.peek().clone() else { delay().await; continue; };
            match grant.status::<Status>().await {
                Ok(next) if grant.current() => {
                    if !["idle", "passed", "cancelled", "fault"].contains(&next.phase.as_str())
                        && let Some(cfg) = settings.write().as_mut() { cfg.dry_self_test_confirmed = false; }
                    status.set(next); online.set(true);
                }
                _ => online.set(false),
            }
            delay().await;
        }
    });
    let active = !["idle", "passed", "cancelled", "fault"].contains(&status.read().phase.as_str());
    let can_edit = online()
        && config_grant.read().as_ref().is_some_and(|grant| grant.current())
        && auth::current_status().station_role.as_deref() != Some("rrls")
        && auth::can_view_actions()
        && !active
        && !abort_only_mode
        && !busy();
    let cold_flow_supported = config_grant.read().as_ref().is_some_and(|grant| grant.cold_flow_supported());
    let turbo_deadband_supported = config_grant.read().as_ref().is_some_and(|grant| grant.turbo_deadband_supported());
    let rows = nitrogen_rows.read().clone();
    let step_error = validated_nitrogen_steps(&rows).err();
    let fill = fill_draft.read().clone();
    let fill_error = validated_fill(&fill).err();
    rsx! {
        section { class: "gs26-station-gse-settings", style: "display:grid;gap:12px;min-width:0;",
            h3 { "GSE limits and plumbing" }
            p { class: "gs-section-note", style: "margin:0;color:var(--gs-text-muted);font-size:var(--gs-text-sm);", "Changes apply to the next operation; permissions and interlocks still apply." }
            super::plumbing_drive::PlumbingDriveSelector { theme: theme.clone() }
                    details {
                    summary {style:"cursor:pointer;color:{theme.text_muted};font-size:13px;","Nitrogen test settings"}
                    if let Some(cfg)=settings.read().clone() {
                        div {style:"display:grid;gap:12px;padding-top:12px;max-width:880px;min-width:0;",
                            if cfg.nitrogen_steps.is_empty() {
                                p {style:"margin:0;font-size:12px;color:{theme.text_muted};", "Legacy profile · saving stores the steps below."}
                            }
                            for (index, row) in rows.iter().enumerate() {
                                fieldset {key:"n2-step-{index}",style:"margin:0;padding:10px;border:1px solid {theme.border};border-radius:10px;min-width:0;",
                                    legend {style:"padding:0 5px;font-size:13px;", "Step {index + 1}"}
                                    div {style:"display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,130px),1fr));gap:10px;align-items:end;",
                                        label {style:"display:grid;gap:5px;min-width:0;", "Pressure (psi)"
                                            input {r#type:"number",min:"0.1",step:"0.1",value:row.pressure.clone(),disabled:!can_edit,aria_label:"Step {index + 1} pressure in psi",style:"box-sizing:border-box;width:100%;min-width:0;min-height:44px;",oninput:move |event|{nitrogen_rows.write()[index].pressure=event.value();}}
                                        }
                                        label {style:"display:grid;gap:5px;min-width:0;", "Settle wait (s)"
                                            input {r#type:"number",min:"1",max:"600",step:"0.1",value:row.settle.clone(),disabled:!can_edit,aria_label:"Step {index + 1} settling seconds",style:"box-sizing:border-box;width:100%;min-width:0;min-height:44px;",oninput:move |event|{nitrogen_rows.write()[index].settle=event.value();}}
                                        }
                                        label {style:"display:grid;gap:5px;min-width:0;", "Leak-test hold (s)"
                                            input {r#type:"number",min:"1",max:"600",step:"0.1",value:row.hold.clone(),disabled:!can_edit,aria_label:"Step {index + 1} holding seconds",style:"box-sizing:border-box;width:100%;min-width:0;min-height:44px;",oninput:move |event|{nitrogen_rows.write()[index].hold=event.value();}}
                                        }
                                    }
                                    div {style:"display:flex;gap:8px;flex-wrap:wrap;margin-top:10px;",
                                        button {r#type:"button",aria_label:"Move step {index + 1} up",disabled:!can_edit||index==0,style:"min-height:44px;min-width:44px;width:auto;padding:6px 12px;",onclick:move |_|nitrogen_rows.write().swap(index,index-1),"↑"}
                                        button {r#type:"button",aria_label:"Move step {index + 1} down",disabled:!can_edit||index+1==rows.len(),style:"min-height:44px;min-width:44px;width:auto;padding:6px 12px;",onclick:move |_|nitrogen_rows.write().swap(index,index+1),"↓"}
                                        button {r#type:"button",disabled:!can_edit,style:"min-height:44px;width:auto;padding:6px 12px;",onclick:move |_|{nitrogen_rows.write().remove(index);},"Remove"}
                                    }
                                }
                            }
                            div {style:"display:flex;gap:10px;flex-wrap:wrap;align-items:center;",
                                button {r#type:"button",disabled:!can_edit||rows.len()>=32,style:"min-height:44px;width:auto;padding:8px 12px;",onclick:move |_|{
                                    let pressure=nitrogen_rows.read().last().and_then(|row|row.pressure.parse::<f32>().ok()).filter(|v|v.is_finite()).unwrap_or(0.0)+50.0;
                                    nitrogen_rows.write().push(NitrogenStepDraft {pressure:pressure.to_string(),settle:"2".into(),hold:"5".into()});
                                },"Add pressure step"}
                                ControlHelp {title:"pressure steps",compact:true,paragraphs:vec!["Targets run in ascending order. Settling starts after nitrogen closes at the target; the hold checks for pressure loss. Each wait is 1–600 seconds; total waits are limited to one hour.".into(),"Saving changes the next test only. It does not run valves. Existing pressure ceilings and interlocks still apply.".into()]}
                            }
                            if let Some(error)=step_error.as_ref() {p {role:"alert",style:"margin:0;font-size:13px;color:{theme.text_primary};", "{error}"}}
                        }
                        div {style:"display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,190px),1fr));gap:12px;padding-top:16px;max-width:880px;",
                            label {style:"display:grid;gap:5px;min-width:0;", "Maximum acceptable empty-tank PT offset (psi, required)" input {style:"box-sizing:border-box;width:100%;min-width:0;min-height:44px;",r#type:"number",min:"0",step:"0.1",value:cfg.maximum_zero_offset_psi.map(|v|v.to_string()).unwrap_or_default(),disabled:!can_edit,oninput:move |e|{if let Some(c)=settings.write().as_mut(){c.maximum_zero_offset_psi=e.value().parse().ok();}}}}
                            label {style:"display:grid;gap:5px;min-width:0;", "Hardware pressure ceiling (psi; required for Turbocharge)" input {style:"box-sizing:border-box;width:100%;min-width:0;min-height:44px;",r#type:"number",min:"0.1",step:"0.1",value:cfg.pressure_ceiling_psi.map(|v|v.to_string()).unwrap_or_default(),disabled:!can_edit,oninput:move |e|{if let Some(c)=settings.write().as_mut(){c.pressure_ceiling_psi=e.value().parse().ok();}}}}
                        }
                        label {style:"display:grid;gap:5px;max-width:440px;", "Maximum added nitrogen mass (kg, required for Turbocharge)"
                            input {r#type:"number",min:"0.001",step:"0.01",disabled:!can_edit,value:cfg.turbocharge_max_added_mass_kg.map(|v|v.to_string()).unwrap_or_default(),oninput:move|event|{if let Some(c)=settings.write().as_mut(){c.turbocharge_max_added_mass_kg=event.value().parse().ok();}}}
                            small {"Blank disables Turbocharge. This cumulative allowance is separate from liquid-fill tolerance."}
                        }
                        label {style:"display:grid;gap:6px;max-width:440px;min-width:0;", "Turbocharge pressure deadband (psi)"
                            select {aria_label:"Turbocharge pressure deadband (psi)",style:"box-sizing:border-box;width:100%;min-width:0;min-height:44px;",disabled:!can_edit||!turbo_deadband_supported,value:cfg.turbocharge_pressure_deadband_psi.to_string(),onchange:move |event|{
                                if !can_edit||!turbo_deadband_supported{return;}
                                if let Ok(value)=event.value().parse::<f32>() && [8.0,12.0,16.0].contains(&value)
                                    && let Some(config)=settings.write().as_mut(){config.turbocharge_pressure_deadband_psi=value;}
                            },
                                if ![8.0,12.0,16.0].contains(&cfg.turbocharge_pressure_deadband_psi) {option {value:cfg.turbocharge_pressure_deadband_psi.to_string(),disabled:true,"Current {cfg.turbocharge_pressure_deadband_psi} psi"}}
                                for psi in [8,12,16] {option {value:psi.to_string(),"{psi} psi"}}
                            }
                            small {"Nitrogen stops at the pressure target. A fall within this band does not trigger a top-up. The hardware pressure ceiling is unchanged."}
                            if !turbo_deadband_supported {small {role:"status","Turbocharge deadband configuration is unavailable on this ground station; other settings can still be saved."}}
                        }
                        label {style:"display:grid;gap:6px;margin-top:16px;max-width:400px;min-width:0;", "Cold Flow nitrogen target (psi)"
                            input {r#type:"number",min:"0.001",step:"0.1",disabled:!can_edit||!cold_flow_supported,value:cfg.cold_flow_target_psi.map(|v|v.to_string()).unwrap_or_default(),style:"width:100%;min-height:44px;box-sizing:border-box;",oninput:move|event|{if let Some(c)=settings.write().as_mut(){c.cold_flow_target_psi=event.value().parse().ok();}}}
                            small {"Blank disables Auto Cold Flow. This target is separate from nitrogen tests, fill and Turbocharge."}
                            if !cold_flow_supported {small {role:"status","Cold Flow configuration is unavailable on this ground station; other settings can still be saved."}}
                        }
                        fieldset {style:"margin:16px 0 0;padding:12px;border:1px solid {theme.border};border-radius:10px;max-width:880px;min-width:0;box-sizing:border-box;",
                            legend {style:"padding:0 5px;font-size:13px;", "Nitrous fill precision"}
                            div {style:"display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,150px),1fr));gap:10px;",
                                label {style:"display:grid;gap:5px;min-width:0;", "Overfill allowance (kg)"
                                    input {r#type:"number",min:"0",max:"0.1",step:"0.01",value:fill.overfill.clone(),disabled:!can_edit,style:"width:100%;min-width:0;min-height:44px;box-sizing:border-box;",oninput:move |event|fill_draft.write().overfill=event.value()}
                                }
                                label {style:"display:grid;gap:5px;min-width:0;", "Mass tolerance (kg)"
                                    input {r#type:"number",min:"0.001",max:"0.07",step:"0.01",value:fill.tolerance.clone(),disabled:!can_edit,style:"width:100%;min-width:0;min-height:44px;box-sizing:border-box;",oninput:move |event|fill_draft.write().tolerance=event.value()}
                                }
                                label {style:"display:grid;gap:5px;min-width:0;", "Vent settling wait (s)"
                                    input {r#type:"number",min:"1",max:"600",step:"0.1",value:fill.settle.clone(),disabled:!can_edit,style:"width:100%;min-width:0;min-height:44px;box-sizing:border-box;",oninput:move |event|fill_draft.write().settle=event.value()}
                                }
                            }
                            ControlHelp {title:"fill precision",compact:true,paragraphs:vec!["The overfill allowance is limited to 0.1 kg. Mass tolerance accounts for load-cell noise and is limited to ±0.07 kg. The settling wait lets the mass stabilize between vent adjustments.".into(),"These limits control the automated sequence; they are not a guarantee of sensor accuracy. Existing pressure and connection interlocks remain active.".into()]}
                            if let Some(error)=fill_error.as_ref() {p {role:"alert",style:"margin:0;font-size:13px;color:{theme.text_primary};", "{error}"}}
                        }
                        button {style:"margin-top:12px;padding:10px 14px;min-height:44px;width:auto;max-width:100%;",disabled:!can_edit||step_error.is_some()||fill_error.is_some(),onclick:move |_|{
                            let Some(mut cfg)=settings.read().clone() else{return;};
                            if let Err(error)=install_nitrogen_drafts(&mut cfg,&nitrogen_rows.read()) {message.set(error);return;}
                            let (overfill,tolerance,settle)=match validated_fill(&fill_draft.read()) {Ok(next)=>next,Err(error)=>{message.set(error);return;}};
                            cfg.fill_overfill_kg=overfill;cfg.fill_tolerance_kg=tolerance;cfg.fill_settle_ms=settle;
                            let Some(grant)=config_grant.peek().clone().filter(|grant|grant.current()) else {message.set("Ground station configuration changed; reload it before saving.".into());return;};
                            let request=grant.save(&cfg);
                            busy.set(true);
                            spawn(async move {
                                let result=request.await;
                                if !grant.current(){return;}
                                match result {Ok((next,next_grant))=>{nitrogen_rows.set(nitrogen_drafts(&next));fill_draft.set(FillDraft::from(&next));settings.set(Some(next));config_grant.set(Some(next_grant));message.set("GSE configuration saved".into());},Err(err)=>message.set(err)}
                                busy.set(false);
                            });
                        },"Save GSE configuration"}
                    }
                }
            if !message.read().is_empty() { p { role: "status", "{message}" } }
        }
    }
}
