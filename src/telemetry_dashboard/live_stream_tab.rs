use crate::telemetry_dashboard::vehicle_tab::VehicleTelemetryBinding;
use super::{UrlConfig, http_get_json, http_post_json, layout::ThemeConfig};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct LiveStreamConfig {
    #[serde(default)]
    pub program_url: String,
    #[serde(default)]
    pub can_manage_stream: bool,
    #[serde(default)]
    pub can_preview_live: bool,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub default_stream_id: String,
    #[serde(default)]
    pub streams: Vec<LiveStreamSpec>,
    #[serde(default)]
    pub stats: Vec<BroadcastStatSpec>,
    #[serde(default)]
    pub broadcast: BroadcastState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct BroadcastState {
    #[serde(default)]
    pub comms_audio_enabled: bool,
    #[serde(default = "default_delay")]
    pub delay_seconds: u32,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub featured_stream_id: String,
    #[serde(default)]
    pub hidden_stream_ids: Vec<String>,
    #[serde(default)]
    pub layout: String,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub replay: Option<ReplayPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ReplayCamera {pub id:String,pub x:f32,pub y:f32,pub zoom:f32}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ReplayPlan {pub id:String,pub duration_seconds:u32,pub speed:f32,pub cameras:Vec<ReplayCamera>,pub started_at_ms:u64,pub source_end_ms:u64}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct LiveStreamSpec {
    pub id: String,
    pub label: String,
    pub url: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub poster_url: String,
    #[serde(default = "default_true")]
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub(crate) struct BroadcastStatSpec {
    pub label: String,
    pub binding: VehicleTelemetryBinding,
    #[serde(default)]
    pub unit: String,
    #[serde(default = "default_precision")]
    pub precision: usize,
}

fn default_delay() -> u32 {
    10
}
fn default_true() -> bool {
    true
}

fn default_precision() -> usize {
    1
}

pub(super) fn media_url(path: &str) -> String {
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

async fn stream_poll_delay() {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(2_000).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
}

#[component]
pub(crate) fn LiveStreamTab(
    theme: ThemeConfig,
    on_open_voice: EventHandler,
    on_open_media: EventHandler,
    #[props(default = false)] manager_mode: bool,
    #[props(default = false)] program_only: bool,
    #[props(default = false)] dedicated: bool,
) -> Element {
    let mut config = use_signal(|| None::<LiveStreamConfig>);
    let mut status = use_signal(|| "Loading broadcast…".to_string());
    let mut edit_mode = use_signal(|| manager_mode);
    use_future(move || async move {
        loop {
            match http_get_json::<LiveStreamConfig>("/api/live_streams").await {
                Ok(next) => {
                    status.set(String::new());
                    if config.read().as_ref() != Some(&next) {
                        config.set(Some(next));
                    }
                }
                Err(error) => status.set(format!("Camera service unavailable: {error}")),
            }
            stream_poll_delay().await;
        }
    });
    let cfg = config.read().clone().unwrap_or_default();
    let url = media_url(&cfg.program_url);
    rsx! {
        style { {r#"
            .gs26-program-shell { height:100%; min-height:0; min-width:0; display:flex; flex-direction:column; overflow:auto; padding:8px; box-sizing:border-box; }
            .gs26-program-header { display:flex; align-items:center; gap:10px; flex-wrap:wrap; padding:4px 0 10px; }
            .gs26-program-header a { color:inherit; }
            .gs26-program-shell button,.gs26-program-shell input,.gs26-program-shell select {font:inherit;min-height:44px;max-width:100%;box-sizing:border-box;border:1px solid #526579;border-radius:10px;padding:8px 12px;background:#172b3a;color:inherit;}
            .gs26-program-shell label {display:flex;flex-direction:column;gap:6px;min-width:0;font-size:14px;}
            .gs26-program-shell input:not([type=checkbox]),.gs26-program-shell select {width:min(100%,16rem);}
            .gs26-program-shell input[type=range] {width:140px;}
            .gs26-program-shell button:disabled {opacity:.5;}
            .gs26-program-shell input[type=checkbox] {min-height:18px;width:18px;height:18px;padding:0;flex:0 0 auto;}
            .gs26-program-shell label:has(> input[type=checkbox]) {flex-direction:row;align-items:center;min-height:44px;}
            .gs26-program-shell fieldset {border:1px solid #344454;border-radius:10px;margin:0;padding:12px;min-width:0;}
            .gs26-program-workspace > p {align-self:center;justify-self:center;padding:20px;color:#a9b7c4;font-size:14px;}

            .gs26-program-shell[data-program-only=true] {padding:0;overflow:hidden;}
            .gs26-program-shell[data-program-only=true] .gs26-program-player {min-height:0;flex:1;}

            .gs26-program-player { width:100%; flex:1 0 420px; min-height:420px; border:0; background:#080d15; }
            .gs26-program-editor { padding:10px 0; }
            .gs26-program-workspace {display:flex;flex-direction:column;flex:1;min-height:0;min-width:0;}
            .gs26-program-workspace[data-manager=true] {display:grid;grid-template-columns:minmax(280px,380px) minmax(0,1fr);gap:12px;overflow:hidden;}
            .gs26-program-workspace[data-manager=true] > .gs26-program-editor {overflow:auto;min-height:0;padding:0;}
            .gs26-program-workspace[data-manager=true] > .gs26-program-player {height:100%;min-height:0;}
            @media(max-width:800px) {
                .gs26-program-workspace[data-manager=true] {display:flex;overflow:auto;}
                .gs26-program-workspace[data-manager=true] > .gs26-program-editor {overflow:visible;}
                .gs26-program-workspace[data-manager=true] > .gs26-program-player {order:-1;flex:0 0 36vh;min-height:180px;position:sticky;top:0;z-index:2;}
            }

            @media(max-width:500px) { .gs26-program-player { flex-basis:560px; min-height:560px; } }
        "#} }
        div { class:"gs26-program-shell", "data-program-only":program_only.to_string(), style:"color:{theme.text_primary};background:{theme.tab_shell_background};",
            if manager_mode && !program_only {
                div { class:"gs26-program-header",
                    strong { if manager_mode { "Stream Manager" } else { "Mission broadcast" } }
                    if !dedicated {button { onclick:move |_| on_open_voice.call(()), "Voice Chat" }}
                    if cfg.can_preview_live {
                        button { onclick:move |_| on_open_media.call(()), "Camera recordings" }
                    }
                    if cfg.can_manage_stream {
                        button { onclick:move |_| { let next=!*edit_mode.read();edit_mode.set(next); },
                            if *edit_mode.read() { "Close stream controls" } else { "Stream controls" }
                        }
                    }
                }
            }
            if manager_mode && config.read().is_some() && !cfg.can_manage_stream { p { "Stream-management permission is required to use these controls." } }
            if !status.read().is_empty() { p { role:"status", "{status}" } }
            div {class:"gs26-program-workspace","data-manager":(manager_mode && cfg.can_manage_stream && *edit_mode.read() && !program_only).to_string(),
            if manager_mode && cfg.can_manage_stream && *edit_mode.read() && !program_only {
                div {class:"gs26-program-editor",
                    super::stream_studio::StreamStudio { broadcast:cfg.broadcast.clone(), program_url:cfg.program_url.clone() }
                    super::stream_studio::ReplayControls {broadcast:cfg.broadcast.clone(),streams:cfg.streams.clone()}
                    div {style:"display:flex;gap:10px;flex-wrap:wrap;padding:10px 0;",
                        label { "Camera layout "
                            select { value:cfg.broadcast.layout.clone(), onchange:{
                                let broadcast=cfg.broadcast.clone();
                                move |event: Event<FormData>| {
                                    let mut request=broadcast.clone();request.layout=event.value();
                                    spawn(async move {
                                        match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control",&request).await {
                                            Ok(saved)=>{let mut next=config.read().clone().unwrap_or_default();next.broadcast=saved;config.set(Some(next));},
                                            Err(error)=>status.set(error),
                                        }
                                    });
                                }
                            }, option {value:"hero","Featured camera"} option {value:"grid","Camera grid"} option {value:"model","Rocket graphic"} option {value:"grid-model","Cameras + graphic"} }
                        }
                        label { "Featured camera "
                            select { value:cfg.broadcast.featured_stream_id.clone(), onchange:{
                                let broadcast=cfg.broadcast.clone();
                                move |event: Event<FormData>| {
                                    let mut request=broadcast.clone();request.featured_stream_id=event.value();
                                    spawn(async move {
                                        match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control",&request).await {
                                            Ok(saved)=>{let mut next=config.read().clone().unwrap_or_default();next.broadcast=saved;config.set(Some(next));},
                                            Err(error)=>status.set(error),
                                        }
                                    });
                                }
                            }, option {value:"","Automatic"}
                                for feed in &cfg.streams { option {value:feed.id.clone(),"{feed.label}"} }
                            }
                        }
                        for feed in &cfg.streams {
                            label {
                                input {r#type:"checkbox",checked:!cfg.broadcast.hidden_stream_ids.contains(&feed.id),onchange:{
                                    let broadcast=cfg.broadcast.clone();let id=feed.id.clone();
                                    move |event: Event<FormData>| {
                                        let mut request=broadcast.clone();request.hidden_stream_ids.retain(|s|s!=&id);
                                        if !event.checked(){request.hidden_stream_ids.push(id.clone());}
                                        spawn(async move {
                                            match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control",&request).await {
                                                Ok(saved)=>{let mut next=config.read().clone().unwrap_or_default();next.broadcast=saved;config.set(Some(next));},
                                                Err(error)=>status.set(error),
                                            }
                                        });
                                    }
                                }} " {feed.label}"
                            }
                        }
                    }
                }
            }
            if cfg.program_url.is_empty() {p {"Waiting for the delayed broadcast program…"}}
            else {iframe {class:"gs26-program-player",src:url,title:"Mission broadcast — delayed video, audio and telemetry",allow:"autoplay; fullscreen"}}

            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_multi_camera_configuration() {
        let config: LiveStreamConfig =
            serde_json::from_str(include_str!("../../docs/api-examples/live-streams.json"))
                .expect("live-stream example should remain valid");

        assert_eq!(config.streams.len(), 3);
        assert_eq!(config.broadcast.featured_stream_id, "pad-wide");
        assert_eq!(config.stats.len(), 2);
    }
}
