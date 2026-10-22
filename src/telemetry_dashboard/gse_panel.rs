use super::{
    ActionPolicyMsg, UrlConfig, http_get_json, http_post_json, js_eval, layout::ThemeConfig,
};
use crate::auth;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
struct Settings {
    nitrogen_target_psi: f32,
    #[serde(default = "default_pressure_step")]
    pressure_step_psi: f32,
    pressure_ceiling_psi: Option<f32>,
    maximum_zero_offset_psi: Option<f32>,
    dry_self_test_confirmed: bool,
    grouped_panel: bool,
}
fn default_pressure_step() -> f32 {
    50.0
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
                div {role:"dialog","aria-modal":"true","aria-label":"{title}",
                    style:"background:#122231;color:#edf3fa;border:1px solid #526579;border-radius:14px;padding:20px;width:min(100%,480px);max-height:80dvh;overflow:auto;box-sizing:border-box;display:grid;gap:12px;",
                    h3 {style:"margin:0;","{title}"}
                    for paragraph in paragraphs {p {style:"margin:0;line-height:1.5;","{paragraph}"}}
                    button {r#type:"button",autofocus:true,style:"min-height:44px;",onclick:move |_|open.set(false),"Close help"}
                }
            }
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
            "Launch",
            "Ascent",
            "Coast",
            "Apogee",
            "ParachuteDeploy",
            "Descent",
            "Landed",
            "Recovery",
            "Aborted",
        ] {
            assert!(!super::ground_visible(phase));
        }
    }
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
#[derive(Clone, Default, Deserialize)]
struct Status {
    phase: String,
    message: String,
    nitrogen_passed: bool,
    #[serde(default)]
    nitrogen_overridden: bool,
    self_test_locked: bool,
    current_step_psi: f32,
    pressure_psi: Option<f32>,
    baseline: Option<Noise>,
    #[serde(default)]
    valves: [Option<bool>; 5],
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
    let mut settings = use_signal(|| None::<Settings>);
    let mut status = use_signal(Status::default);
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut online = use_signal(|| false);
    use_future(move || async move {
        if let Ok(cfg) = http_get_json::<Settings>("/api/gse/config").await {
            settings.set(Some(cfg));
        }
        loop {
            match http_get_json::<Status>("/api/gse/status").await {
                Ok(next) => {
                    if !["idle", "passed", "cancelled", "fault"].contains(&next.phase.as_str()) {
                        if let Some(cfg) = settings.write().as_mut() {
                            cfg.dry_self_test_confirmed = false;
                        }
                    }
                    status.set(next);
                    online.set(true);
                }
                Err(err) => {
                    online.set(false);
                    message.set(format!("GSE status unavailable: {err}"));
                }
            }
            delay().await;
        }
    });
    let snapshot = status.read().clone();
    use_effect(move || {
        if show_model {
            sync_scene(&status.read());
        }
    });
    let active = !["idle", "passed", "cancelled", "fault"].contains(&snapshot.phase.as_str());
    let can_edit = auth::can_view_actions() && !active && !abort_only_mode && !*busy.read();
    let pressure_label = snapshot
        .pressure_psi
        .map(|v| format!("{v:.1} psi"))
        .unwrap_or_else(|| "—".into());
    rsx! {
        section { style:"border:1px solid {theme.border};border-radius:14px;overflow:hidden;background:{theme.panel_background};",
            div { style:"padding:18px 20px;border-bottom:1px solid {theme.border};display:flex;justify-content:space-between;gap:16px;align-items:center;",
                div { h3 {style:"margin:0;font-weight:600;letter-spacing:.02em;","Reconfigure GSE"}
                    p {style:"margin:6px 0 0;color:{theme.text_muted};font-size:12px;","Nitrogen · Nitrous · Fill manifold · Launch tower"}
                }
                span {style:"font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:{theme.text_muted};",if snapshot.nitrogen_overridden {"Fill unlocked · override"}else if snapshot.nitrogen_passed {"Fill unlocked"}else{"Fill locked"}}
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
                    span {"Last board acknowledgement"}
                    if show_model {span {"Animation indicates sequence activity, not measured flow"}}
                }
                p {style:"margin:0;color:{theme.text_primary};", "{snapshot.message}"}
                div {style:"display:flex;gap:24px;flex-wrap:wrap;color:{theme.text_muted};font-size:12px;font-variant-numeric:tabular-nums;",
                    span {"Tank: {pressure_label}"}
                    span {"Test step: {snapshot.current_step_psi:.0} psi"}
                    if snapshot.self_test_locked {span {"Self-test locked for this fill session"}}
                }
                if let Some(noise)=snapshot.baseline {
                    p {style:"margin:0;font-size:12px;color:{theme.text_muted};","PT baseline {noise.average_psi:.2} psi · range {noise.min_psi:.2}–{noise.max_psi:.2} · noise ±{noise.noise_psi:.2} · {noise.samples} samples"}
                }
                details {
                    summary {style:"cursor:pointer;color:{theme.text_muted};font-size:13px;","Nitrogen test settings"}
                    if let Some(cfg)=settings.read().clone() {
                        div {style:"display:grid;grid-template-columns:repeat(auto-fit,minmax(190px,1fr));gap:12px;padding-top:16px;",
                            label {"Pressure step (psi)" input {r#type:"number",min:"0.1",step:"0.1",value:"{cfg.pressure_step_psi}",disabled:!can_edit,oninput:move |e|{if let Ok(v)=e.value().parse(){if let Some(c)=settings.write().as_mut(){c.pressure_step_psi=v;}}}}}
                            label {"Nitrogen maximum pressure (psi)" input {r#type:"number",value:"{cfg.nitrogen_target_psi}",disabled:!can_edit,oninput:move |e|{if let Ok(v)=e.value().parse(){if let Some(c)=settings.write().as_mut(){c.nitrogen_target_psi=v;}}}}}
                            p {"Automatic failure limit: nitrogen target +100 psi; nitrous fill target +50 psi. A lower hardware ceiling takes precedence."}
                            label {"Maximum acceptable empty-tank PT offset (psi, required)" input {r#type:"number",min:"0",step:"0.1",value:cfg.maximum_zero_offset_psi.map(|v|v.to_string()).unwrap_or_default(),disabled:!can_edit,oninput:move |e|{if let Some(c)=settings.write().as_mut(){c.maximum_zero_offset_psi=e.value().parse().ok();}}}}
                            label {"Hardware pressure ceiling (psi, optional lower limit)" input {r#type:"number",min:"0.1",step:"0.1",value:cfg.pressure_ceiling_psi.map(|v|v.to_string()).unwrap_or_default(),disabled:!can_edit,oninput:move |e|{if let Some(c)=settings.write().as_mut(){c.pressure_ceiling_psi=e.value().parse().ok();}}}}
                        }
                        button {style:"margin-top:12px;padding:10px 14px;",disabled:!can_edit,onclick:move |_|{
                            let Some(cfg)=settings.read().clone() else{return;};
                            busy.set(true);
                            spawn(async move {match http_post_json::<Settings,Settings>("/api/gse/config",&cfg).await{Ok(next)=>{settings.set(Some(next));message.set("GSE configuration saved".into());},Err(err)=>message.set(err)}busy.set(false);});
                        },"Save GSE configuration"}
                    }
                }
                if !message.read().is_empty() {p {role:"status",style:"margin:0;color:{theme.text_muted};font-size:12px;","{message}"}}
            }
        }
    }
}

/// Keep the required baseline limit beside the action that needs it.
#[component]
pub(super) fn SelfTestPtOffset(disabled: bool, theme: ThemeConfig) -> Element {
    let mut value = use_signal(String::new);
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| true);
    use_future(move || async move {
        match http_get_json::<Settings>("/api/gse/config").await {
            Ok(cfg) => {
                value.set(
                    cfg.maximum_zero_offset_psi
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                );
                message.set(if cfg.maximum_zero_offset_psi.is_some() {
                    "Saved empty-tank PT limit loaded.".into()
                } else {
                    "Enter and save this limit before unlocking self-test.".into()
                });
            }
            Err(err) => message.set(format!("Could not load PT limit: {err}. Save will retry.")),
        }
        busy.set(false);
    });
    rsx! {
        div { class:"gs26-pt-limit", style:"display:grid;grid-template-columns:minmax(0,1fr) auto;gap:8px 12px;padding:12px;border:1px solid {theme.border};border-radius:10px;min-width:0;width:100%;max-width:32rem;box-sizing:border-box;",
            label { style:"display:grid;gap:6px;font-size:13px;grid-column:1/-1;",
                "Maximum acceptable empty-tank PT offset (psi)"
                input {
                    r#type:"number", min:"0", step:"any", placeholder:"Required",
                    style:"width:8rem;max-width:100%;min-width:0;box-sizing:border-box;",
                    value:"{value}", disabled:disabled || *busy.read(),
                    oninput:move |event| {value.set(event.value());message.set("Unsaved PT limit.".into());}
                }
            }
            ControlHelp {compact:true,title:"empty-tank pressure limit",paragraphs:vec![
                "Allowed pressure reading above or below zero with an empty, depressurized tank.".into(),
                "This is a test acceptance limit, not a calibration offset. Save it before starting the valve self-test.".into()
            ]}
            button {
                style:"width:auto;min-height:44px;justify-self:end;",
                disabled:disabled || *busy.read(),
                onclick:move |_| {
                    let limit = match value.read().trim().parse::<f32>() {
                        Ok(v) if v.is_finite() && v >= 0.0 => v,
                        _ => {message.set("Enter a finite, nonnegative PT limit in psi.".into());return;}
                    };
                    busy.set(true);
                    spawn(async move {
                        // Read immediately before saving so other GSE settings and
                        // the current dry-test confirmation are preserved.
                        let result = async {
                            let mut cfg = http_get_json::<Settings>("/api/gse/config").await?;
                            cfg.maximum_zero_offset_psi = Some(limit);
                            http_post_json::<Settings,Settings>("/api/gse/config", &cfg).await
                        }.await;
                        match result {
                            Ok(cfg) => {
                                value.set(cfg.maximum_zero_offset_psi.map(|v| v.to_string()).unwrap_or_default());
                                message.set("PT limit saved.".into());
                            }
                            Err(err) => message.set(format!("PT limit was not saved: {err}")),
                        }
                        busy.set(false);
                    });
                },
                if *busy.read() {"Please wait…"} else {"Save PT limit"}
            }
            p { role:"status", style:"margin:0;font-size:12px;grid-column:1/-1;", "{message}" }
        }
    }
}

/// Historical checks and live connectivity are deliberately displayed separately.
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

#[component]
pub(super) fn QualificationStatus(status: Signal<serde_json::Value>, disabled: bool) -> Element {
    let mut confirm = use_signal(|| false);
    let mut override_open = use_signal(|| false);
    let mut override_reason = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let value = status.read().clone();
    let Some(qualification) = value.get("qualification") else {
        return rsx! {};
    };
    let fault = qualification["fault"].as_str().map(str::to_string);
    let stable = value["topology_stable"].as_bool() == Some(true);
    let can_reset = !disabled
        && !*busy.read()
        && auth::can_send_command("ValveSelfTest")
        && value["request_gate"]["flight_state"]
            .as_str()
            .is_some_and(|s| matches!(s, "Startup" | "Idle"));
    let mission_stage = value["mission_stage"]
        .as_str()
        .unwrap_or("idle")
        .to_string();
    let override_actor = qualification["nitrogen_override"]["operator"]
        .as_str()
        .map(str::to_string);
    let can_override = can_reset
        && auth::can_send_command("NitrogenTest")
        && stable
        && fault.is_none()
        && value["storage_error"].is_null()
        && mission_stage == "idle"
        && qualification["self_test"].as_bool() == Some(true)
        && qualification["nitrogen_test"].as_bool() != Some(true)
        && override_actor.is_none();
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
            if can_override {button {style:"min-height:44px;justify-self:start;",onclick:move |_|{override_open.set(true);override_reason.set(String::new());},"Override cold-flow requirement"}}
            if !stable {p {role:"status",style:"margin:0;color:#e6b85c;","Waiting for stable fill-system connections"}}
            if let Some(fault)=fault {p {role:"alert",style:"margin:0;color:#ff8f8f;","Safety fault: {fault}. Recover to Idle and reset checks."}}
            if let Some(error)=value["storage_error"].as_str() {p {role:"alert","Checks cannot be saved: {error}"}}
            if value["launch_queued"].as_bool()==Some(true) {p {role:"status","Launch queued · {qd_label}"}}
            button {style:"min-height:44px;justify-self:start;",disabled:!can_reset,onclick:move |_|confirm.set(true),"Reset operation checks"}
            p {role:"status",style:"margin:0;","{message}"}
            if *override_open.read() {
                div {style:"position:fixed;inset:0;background:#000a;z-index:6000;display:grid;place-items:center;padding:20px;",
                    div {role:"dialog","aria-modal":"true","aria-label":"Override nitrogen cold flow",
                        style:"background:#122231;color:#edf3fa;border:1px solid #526579;border-radius:14px;padding:20px;width:min(100%,420px);box-sizing:border-box;display:grid;gap:16px;",
                        h3 {style:"margin:0;","Override cold-flow requirement?"}
                        p {style:"margin:0;","This records an operator override, not a passed test. Fresh pressure, valve and connection checks still apply."}
                        label {"Reason" textarea {"aria-label":"Cold-flow override reason",maxlength:"500",value:override_reason(),oninput:move |event|override_reason.set(event.value()),style:"width:100%;min-height:88px;box-sizing:border-box;"}}
                        div {style:"display:flex;gap:12px;flex-wrap:wrap;",
                            button {disabled:*busy.read(),onclick:move |_|override_open.set(false),"Cancel override"}
                            button {disabled:*busy.read()||override_reason.read().trim().is_empty(),onclick:move |_|{
                                let reason=override_reason.read().trim().to_string();busy.set(true);
                                spawn(async move {
                                    match http_post_json::<serde_json::Value,serde_json::Value>("/api/gse/cold-flow/override",&serde_json::json!({"reason":reason})).await {
                                        Ok(_)=>{message.set("Cold-flow override saved".into());override_open.set(false);},
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
                    div {role:"dialog","aria-modal":"true","aria-label":"Reset operation checks",
                        style:"background:#122231;color:#edf3fa;border:1px solid #526579;border-radius:14px;padding:20px;max-width:420px;display:grid;gap:16px;",
                        h3 {style:"margin:0;","Reset operation checks?"}
                        p {style:"margin:0;","Valve test, nitrogen test and fill qualification will be cleared. Outputs will not be actuated."}
                        div {style:"display:flex;gap:12px;flex-wrap:wrap;",
                            button {style:"min-height:44px;",disabled:*busy.read(),onclick:move |_|confirm.set(false),"Cancel"}
                            button {style:"min-height:44px;",disabled:*busy.read(),onclick:move |_| {
                                busy.set(true);
                                spawn(async move {
                                    match http_post_json::<serde_json::Value,serde_json::Value>("/api/gse/qualification/reset",&serde_json::json!({})).await {
                                        Ok(_)=>{message.set("Operation checks reset".into());confirm.set(false);},
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
#[component]
pub(super) fn ClearGseAbort() -> Element {
    let mut confirming = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    rsx! {
        div { style: "display:grid;gap:8px;justify-items:start;",
            button {
                r#type:"button",
                disabled: *busy.read() || !auth::can_send_command("ValveSelfTest"),
                onclick: move |_| confirming.set(true),
                "Clear GSE abort"
            }
            if *confirming.read() {
                p { style:"margin:0;max-width:48ch;", "Confirm the system is safe. This returns GroundStation to Idle and resets operation checks. No outputs are commanded. Fully power-cycle locally aborted boards separately." }
                div { style:"display:flex;gap:8px;flex-wrap:wrap;",
                    button { r#type:"button", disabled:*busy.read(), onclick:move |_| {
                        busy.set(true);
                        spawn(async move {
                            match http_post_json::<serde_json::Value,serde_json::Value>("/api/gse/abort/clear", &serde_json::json!({})).await {
                                Ok(_) => {confirming.set(false);message.set("GSE abort cleared. Repeat operation checks before filling.".into());},
                                Err(error) => message.set(error),
                            }
                            busy.set(false);
                        });
                    }, "Confirm clear abort" }
                    button {r#type:"button",disabled:*busy.read(),onclick:move |_|confirming.set(false),"Cancel"}
                }
            }
            if !message.read().is_empty() {p {role:"status",style:"margin:0;","{message}"}}
        }
    }
}
