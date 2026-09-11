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
fn GroundChecklist() -> Element {
    let key = format!("{}_ground_checklist", super::dashboard_customization_key());
    let mut checked = use_signal(|| {
        super::persist::get_string(&key)
            .and_then(|s| serde_json::from_str::<[bool; 4]>(&s).ok())
            .unwrap_or([false; 4])
    });
    rsx! {details {summary {"Ground checklist"}
        p {style:"font-size:12px;","Operator reminders only; checkmarks do not bypass interlocks or unlock self-test. Reset before each operation."}
        for (i,label) in ["Pressure transducer reading checked","Gas lines and connections inspected","Personnel clear of valves and vent/dump paths","Communications and abort procedure checked"].iter().enumerate() {
            label {style:"display:block;padding:5px;",input {r#type:"checkbox",checked:checked.read()[i],onchange:{let key=key.clone();move |event|{let mut next=*checked.read();next[i]=event.checked();checked.set(next);super::persist::set_string(&key,&serde_json::to_string(&next).unwrap());}}} "{label}"}
        }
        button {onclick:{let key=key.clone();move |_|{checked.set([false;4]);super::persist::set_string(&key,"[false,false,false,false]");}},"Reset checklist"}
    }}
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
    self_test_locked: bool,
    current_step_psi: f32,
    pressure_psi: Option<f32>,
    baseline: Option<Noise>,
    #[serde(default)]
    valves: [Option<bool>; 5],
}
async fn delay() {
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
    use_effect(move || sync_scene(&status.read()));
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
                span {style:"font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:{theme.text_muted};",if snapshot.nitrogen_passed {"Fill unlocked"}else{"Fill locked"}}
            }
            div {style:"height:360px;background:radial-gradient(ellipse at center,#202b34,#0b1016);",
                div {style:"width:100%;height:100%;",dangerous_inner_html:r#"<model-viewer id="gs26-gse-model" alt="Nitrogen and nitrous tanks, fill manifold, plumbing and launch tower" camera-controls camera-orbit="35deg 68deg auto" shadow-intensity="0.6" exposure="1" style="width:100%;height:100%;background:transparent"></model-viewer>"#}
            }
            div {style:"padding:18px 20px;display:grid;gap:14px;",
                div {style:"display:flex;gap:16px;flex-wrap:wrap;font-size:11px;color:{theme.text_muted};",
                    for (i, label) in ["Pilot", "Vent", "Dump", "Nitrogen", "Nitrous"].iter().enumerate() {
                        { let value=match snapshot.valves[i] {Some(true)=>"OPEN",Some(false)=>"CLOSED",None=>"UNKNOWN"}; rsx! {span {"{label}: {value}"}} }
                    }
                    span {"Last board acknowledgement · animation indicates sequence activity, not measured flow"}
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
                GroundChecklist {}
                details {
                    summary {style:"cursor:pointer;color:{theme.text_muted};font-size:13px;","Nitrogen test settings"}
                    if let Some(cfg)=settings.read().clone() {
                        div {style:"display:grid;grid-template-columns:repeat(auto-fit,minmax(190px,1fr));gap:12px;padding-top:16px;",
                            label {"Pressure step (psi)" input {r#type:"number",min:"0.1",step:"0.1",value:"{cfg.pressure_step_psi}",disabled:!can_edit,oninput:move |e|{if let Ok(v)=e.value().parse(){if let Some(c)=settings.write().as_mut(){c.pressure_step_psi=v;}}}}}
                            label {"Nitrogen maximum pressure (psi)" input {r#type:"number",value:"{cfg.nitrogen_target_psi}",disabled:!can_edit,oninput:move |e|{if let Ok(v)=e.value().parse(){if let Some(c)=settings.write().as_mut(){c.nitrogen_target_psi=v;}}}}}
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
