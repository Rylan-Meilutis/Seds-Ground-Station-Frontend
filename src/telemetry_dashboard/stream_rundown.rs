//! Editor for the backend-owned audience rundown; no operational state changes.
use super::{
    http_get_json, http_post_json,
    live_stream_tab::{BroadcastState, LiveStreamConfig},
    vehicle_tab::VehicleTelemetryBinding,
};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct Rundown {
    pub profiles: BTreeMap<String, Profile>,
    pub phase_profiles: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Profile {
    pub title: String,
    pub cycle_seconds: u32,
    pub stats_per_page: usize,
    pub scene: String,
    pub stats: Vec<Stat>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Stat {
    pub label: String,
    pub binding: VehicleTelemetryBinding,
    pub unit: String,
    pub precision: usize,
    pub source: String,
    pub format: String,
}
fn edit(
    mut draft: Signal<Option<Rundown>>,
    selected: Signal<String>,
    change: impl FnOnce(&mut Profile),
) {
    if let Some(profile) = draft
        .write()
        .as_mut()
        .and_then(|r| r.profiles.get_mut(&*selected.peek()))
    {
        change(profile);
    }
}
fn new_stat() -> Stat {
    Stat {
        label: "Metric".into(),
        binding: VehicleTelemetryBinding {
            data_type: "BATTERY_VOLTAGE".into(),
            sender_id: None,
            index: 0,
            scale: 1.,
            offset: 0.,
        },
        unit: String::new(),
        precision: 1,
        source: "telemetry".into(),
        format: "number".into(),
    }
}
#[component]
pub(super) fn RundownEditor(broadcast: BroadcastState) -> Element {
    let mut base = use_signal(|| broadcast.clone());
    let mut draft = use_signal(|| broadcast.rundown.clone());
    let mut selected = use_signal(|| "ground".to_string());
    let mut phase = use_signal(|| "Launch".to_string());
    let mut busy = use_signal(|| false);
    let mut status = use_signal(String::new);
    let snapshot = draft();
    let profile = snapshot
        .as_ref()
        .and_then(|r| r.profiles.get(&selected()))
        .cloned();
    rsx! {details {class:"gs26-rundown-editor",
        summary {"Audience rundown"}
        p {class:"gs26-studio-help","Choose data, cycle order and scene for each stage. These choices affect the audience programme only."}
        if let Some(rundown)=snapshot {
            div {class:"gs26-studio-fields",
                label {"Stage profile" select {value:selected(),onchange:move|e|selected.set(e.value()),
                    for (id,p) in &rundown.profiles {option {value:id.clone(),"{p.title}"}}
                }}
                if let Some(page)=profile.clone() {
                    label {"Title" input {maxlength:"96",value:page.title.clone(),oninput:move|e|edit(draft,selected,|p|p.title=e.value())}}
                    label {"Seconds per page" input {r#type:"number",min:"1",max:"30",value:page.cycle_seconds.to_string(),oninput:move|e|{if let Ok(v)=e.value().parse(){edit(draft,selected,|p|p.cycle_seconds=v);}}}}
                    label {"Metrics per page" input {r#type:"number",min:"1",max:"6",value:page.stats_per_page.to_string(),oninput:move|e|{if let Ok(v)=e.value().parse(){edit(draft,selected,|p|p.stats_per_page=v);}}}}
                    label {"Scene" select {value:page.scene.clone(),onchange:move|e|edit(draft,selected,|p|p.scene=e.value()),
                        for (id,label) in [("inherit","Operator’s current scene"),("hero","Featured camera"),("grid","Camera grid"),("model","Rocket artwork"),("grid-model","Cameras + artwork"),("interview","Interview + picture-in-picture"),("map","Recovery map"),("recorded-replay","Recorded vehicle replay")] {option {value:id,"{label}"}}
                    }}
                }
            }
            if let Some(page)=profile {
                div {class:"gs26-program-metrics",aria_label:"Ordered audience metrics",
                    for (index,stat) in page.stats.iter().enumerate() {MetricEditor {key:"{selected}-{index}",stat:stat.clone(),index,draft,selected}}
                }
                button {disabled:busy()||page.stats.len()>=24,onclick:move|_|edit(draft,selected,|p|p.stats.push(new_stat())),"Add metric"}
            }
            details {summary {"Flight phase assignments"}
                p {class:"gs26-studio-help","Defaults keep fill data on the ground and flight data after Launch. Test-fire mode always uses its own profile."}
                for (name,target) in &rundown.phase_profiles {
                    {let remove_name=name.clone();rsx!{div {class:"gs26-studio-actions",span {"{name} → {target}"}button {onclick:move|_|{if let Some(r)=draft.write().as_mut(){r.phase_profiles.remove(&remove_name);}},"Remove assignment"}}}}
                }
                div {class:"gs26-studio-fields",label {"Flight phase" select {value:phase(),onchange:move|e|phase.set(e.value()),
                    for name in ["Startup","Idle","PreFill","FillTest","NitrogenFill","NitrousFill","Armed","Launch","Ascent","Coast","Apogee","ParachuteDeploy","Descent","Landed","Recovery","Aborted"] {option {value:name,"{name}"}}
                }}button {onclick:move|_|{if let Some(r)=draft.write().as_mut(){r.phase_profiles.insert(phase(),selected());}},"Use selected profile for phase"}}
            }
        } else {p {"Reload broadcast settings to edit this station’s audience rundown."}}
        div {class:"gs26-studio-actions",
            button {disabled:busy()||draft.read().is_none(),onclick:move|_|{
                let mut request=base();request.rundown=draft();busy.set(true);
                spawn(async move{match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control",&request).await {
                    Ok(saved)=>{base.set(saved.clone());draft.set(saved.rundown);status.set("Audience rundown saved".into());},Err(e)=>status.set(e)
                }busy.set(false);});
            },"Save audience rundown"}
            button {disabled:busy(),onclick:move|_|{busy.set(true);spawn(async move {
                match http_get_json::<LiveStreamConfig>("/api/live_streams").await {Ok(cfg)=>{base.set(cfg.broadcast.clone());draft.set(cfg.broadcast.rundown);status.set("Broadcast settings reloaded".into());},Err(e)=>status.set(e)}busy.set(false);
            });},"Reload broadcast settings"}
        }
        if !status().is_empty(){p {role:"status","{status}"}}
    }}
}
#[component]
fn MetricEditor(
    stat: Stat,
    index: usize,
    draft: Signal<Option<Rundown>>,
    selected: Signal<String>,
) -> Element {
    rsx! {details {class:"gs26-program-metric",
        summary {"{index+1}. {stat.label}"}
        div {class:"gs26-studio-fields",
            label {"Label" input {maxlength:"96",value:stat.label.clone(),oninput:move|e|edit(draft,selected,|p|p.stats[index].label=e.value())}}
            label {"Source" select {value:stat.source.clone(),onchange:move|e|edit(draft,selected,|p|p.stats[index].source=e.value()),option {value:"telemetry","Telemetry channel"}option {value:"selected_fill_mass","Selected fill mass"}}}
            if stat.source=="telemetry" {
                label {"Data type" input {maxlength:"96",value:stat.binding.data_type.clone(),oninput:move|e|edit(draft,selected,|p|p.stats[index].binding.data_type=e.value())}}
                label {"Sender (blank = any)" input {maxlength:"96",value:stat.binding.sender_id.clone().unwrap_or_default(),oninput:move|e|edit(draft,selected,|p|p.stats[index].binding.sender_id=if e.value().is_empty(){None}else{Some(e.value())})}}
                label {"Channel index" input {r#type:"number",min:"0",max:"63",value:stat.binding.index.to_string(),oninput:move|e|{if let Ok(v)=e.value().parse(){edit(draft,selected,|p|p.stats[index].binding.index=v);}}}}
                label {"Scale" input {r#type:"number",step:"any",value:stat.binding.scale.to_string(),oninput:move|e|{if let Ok(v)=e.value().parse(){edit(draft,selected,|p|p.stats[index].binding.scale=v);}}}}
                label {"Offset" input {r#type:"number",step:"any",value:stat.binding.offset.to_string(),oninput:move|e|{if let Ok(v)=e.value().parse(){edit(draft,selected,|p|p.stats[index].binding.offset=v);}}}}
            }
            label {"Unit" input {maxlength:"24",value:stat.unit.clone(),oninput:move|e|edit(draft,selected,|p|p.stats[index].unit=e.value())}}
            label {"Decimal places" input {r#type:"number",min:"0",max:"6",value:stat.precision.to_string(),oninput:move|e|{if let Ok(v)=e.value().parse(){edit(draft,selected,|p|p.stats[index].precision=v);}}}}
            label {"Display" select {value:stat.format.clone(),onchange:move|e|edit(draft,selected,|p|p.stats[index].format=e.value()),option {value:"number","Number"}option {value:"boolean","Yes / No"}option {value:"valve","Open / Closed"}}}
        }
        div {class:"gs26-studio-actions",
            button {aria_label:format!("Move {} up",stat.label),disabled:index==0,onclick:move|_|edit(draft,selected,|p|{if index>0 {p.stats.swap(index,index-1);}}),"Move up"}
            button {aria_label:format!("Move {} down",stat.label),disabled:draft.read().as_ref().and_then(|r|r.profiles.get(&selected())).is_none_or(|p|index+1>=p.stats.len()),onclick:move|_|edit(draft,selected,|p|{if index+1<p.stats.len(){p.stats.swap(index,index+1);}}),"Move down"}
            button {aria_label:format!("Remove {}",stat.label),onclick:move|_|edit(draft,selected,|p|{p.stats.remove(index);}),"Remove metric"}
        }
    }}
}
