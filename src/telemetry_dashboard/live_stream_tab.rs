use super::{
    TELEMETRY_RENDER_EPOCH, UrlConfig, http_get_json, http_post_json, latest_telemetry_value,
    layout::ThemeConfig, persist, types::FlightState,
};
use crate::telemetry_dashboard::vehicle_tab::{VehicleTab, VehicleTelemetryBinding};
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
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MissionViewPreferences {
    show_stats: bool,
    show_angle_strip: bool,
    show_feed_labels: bool,
}

impl Default for MissionViewPreferences {
    fn default() -> Self {
        Self {
            show_stats: true,
            show_angle_strip: true,
            show_feed_labels: true,
        }
    }
}

fn preferences_key() -> String {
    let suffix: String = UrlConfig::base_http()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    format!("gs26_mission_view_v1_{suffix}")
}

fn load_preferences() -> MissionViewPreferences {
    persist::get_string(&preferences_key())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_preferences(preferences: &MissionViewPreferences) {
    if let Ok(raw) = serde_json::to_string(preferences) {
        persist::set_string(&preferences_key(), &raw);
    }
}

fn media_url(path: &str) -> String {
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

fn binding_value(binding: &VehicleTelemetryBinding) -> Option<f32> {
    latest_telemetry_value(
        &binding.data_type,
        binding.sender_id.as_deref(),
        binding.index,
    )
    .map(|value| value * binding.scale + binding.offset)
    .filter(|value| value.is_finite())
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
    action_policy: Signal<super::ActionPolicyMsg>,
    abort_only_mode: bool,
    #[props(default = false)] program_only: bool,
    flight_state: Signal<FlightState>,
    launch_clock: Signal<Option<super::LaunchClockMsg>>,
    network_time: Signal<Option<super::NetworkTimeSync>>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    rocket_altitude_m: Signal<Option<f64>>,
) -> Element {
    let config = use_signal(|| None::<LiveStreamConfig>);
    let status = use_signal(|| "Loading camera feeds…".to_string());
    let selected_stream_id = use_signal(String::new);
    let last_broadcast_revision = use_signal(|| None::<u64>);
    let edit_mode = use_signal(|| false);
    let preferences = use_signal(load_preferences);
    let broadcast_label_input = use_signal(String::new);

    {
        let mut config = config;
        let mut status = status;
        let mut selected_stream_id = selected_stream_id;
        let mut last_broadcast_revision = last_broadcast_revision;
        let mut broadcast_label_input = broadcast_label_input;
        use_future(move || async move {
            loop {
                match http_get_json::<LiveStreamConfig>("/api/live_streams").await {
                    Ok(next) => {
                        let online_count = next.streams.iter().filter(|feed| feed.online).count();
                        let preferred = if next.broadcast.featured_stream_id.is_empty() {
                            next.default_stream_id.as_str()
                        } else {
                            next.broadcast.featured_stream_id.as_str()
                        };
                        let initial = next
                            .streams
                            .iter()
                            .find(|feed| feed.online && feed.id == preferred)
                            .or_else(|| next.streams.iter().find(|feed| feed.online))
                            .map(|feed| feed.id.clone())
                            .unwrap_or_default();
                        let broadcast_changed =
                            *last_broadcast_revision.read() != Some(next.broadcast.revision);
                        if !initial.is_empty()
                            && (selected_stream_id.read().is_empty() || broadcast_changed)
                            && *selected_stream_id.read() != initial
                        {
                            selected_stream_id.set(initial);
                        }
                        last_broadcast_revision.set(Some(next.broadcast.revision));
                        if broadcast_label_input.read().is_empty() {
                            broadcast_label_input.set(next.broadcast.label.clone());
                        }
                        let next_status = format!("{online_count} camera feed(s) online");
                        if *status.read() != next_status {
                            status.set(next_status);
                        }
                        if config.read().as_ref() != Some(&next) {
                            config.set(Some(next));
                        }
                    }
                    Err(error) => status.set(format!("Camera service unavailable: {error}")),
                }
                stream_poll_delay().await;
            }
        });
    }

    let _ = *TELEMETRY_RENDER_EPOCH.read();
    let cfg = config.read().clone().unwrap_or_default();
    let can_manage_stream = cfg.can_manage_stream;
    let online_feeds: Vec<LiveStreamSpec> = cfg
        .streams
        .iter()
        .filter(|feed| {
            feed.online
                && !feed.url.trim().is_empty()
                && !cfg.broadcast.hidden_stream_ids.contains(&feed.id)
        })
        .cloned()
        .collect();
    let selected = online_feeds
        .iter()
        .find(|feed| feed.id == *selected_stream_id.read())
        .cloned()
        .or_else(|| online_feeds.first().cloned());
    let prefs = preferences.read().clone();
    if program_only || !cfg.can_preview_live {
        let url = media_url(&cfg.program_url);
        return rsx! {div {style:"height:100%;min-height:70vh;background:#080d15;color:white;",
            if cfg.program_url.is_empty() {p {"Waiting for the delayed broadcast program…"}}
            else {iframe {src:url,title:"Audience program — delayed video and telemetry",allow:"autoplay; fullscreen",style:"width:100%;height:100%;min-height:70vh;border:0;" }}
        }};
    }
    let title = if cfg.title.trim().is_empty() {
        "Mission Live".to_string()
    } else {
        cfg.title.clone()
    };

    rsx! {
        style { {r#"
            @keyframes gs26-mission-fade { from { opacity:0; transform:scale(1.012); } to { opacity:1; transform:scale(1); } }
            .gs26-mission-shell { height:100%; overflow-y:auto; padding:12px; box-sizing:border-box; }
            .gs26-mission-hero { position:relative; width:100%; min-height:clamp(390px,68vh,780px); overflow:hidden; border-radius:18px; }
            .gs26-mission-media { position:absolute; inset:0; width:100%; height:100%; border:0; object-fit:cover; background:#02050a; animation:gs26-mission-fade .55s ease both; }
            .gs26-mission-banner { position:absolute; left:0; right:0; bottom:0; display:grid; grid-template-columns:repeat(auto-fit,minmax(110px,1fr)); gap:1px; padding:1px; backdrop-filter:blur(12px); }
            .gs26-angle-strip { display:grid; grid-template-columns:repeat(auto-fill,minmax(180px,1fr)); gap:8px; margin-top:10px; }
            @media(max-width:700px) { .gs26-mission-hero { min-height:430px; } .gs26-mission-banner { grid-template-columns:repeat(2,1fr); } }
        "#} }
        div { class: "gs26-mission-shell", style: "color:{theme.text_primary}; background:{theme.tab_shell_background};",
            div { class:"gs26-mission-header", style: "display:flex; justify-content:space-between; align-items:flex-start; gap:10px; flex-wrap:wrap; margin-bottom:10px;",
                div {
                    h2 { style: "margin:0; font-size:20px;", "{title}" }
                    div { role: "status", "aria-atomic": "true", style: "margin-top:3px; color:{theme.text_muted}; font-size:12px;", "{status.read()}" }
                }
                a {
                    href: media_url("/radio"), target: "_blank", rel: "noopener",
                    style: "padding:7px 11px; color:{theme.text_primary};",
                    "Crew voice ↗"
                }
                if cfg.can_preview_live {
                    a {
                        href: media_url("/media#recordings-heading"), target: "_blank", rel: "noopener",
                        style: "padding:7px 11px; color:{theme.text_primary};",
                        "Camera recordings ↗"
                    }
                }
                button {
                    style: "padding:7px 11px; border:1px solid {theme.button_border}; border-radius:999px; background:{theme.button_background}; color:{theme.button_text}; cursor:pointer;",
                    onclick: {
                        let mut edit_mode = edit_mode;
                        move |_| {
                            let next = !*edit_mode.read();
                            edit_mode.set(next);
                        }
                    },
                    if *edit_mode.read() { "Finish editing" } else if cfg.can_manage_stream { "Stream controls" } else { "Customize view" }
                }
            }
            if *edit_mode.read() {
                div { class:"gs26-mission-editor", style: "display:flex; gap:14px; flex-wrap:wrap; margin-bottom:10px; padding:11px; border:1px dashed {theme.info_accent}; border-radius:14px; background:{theme.info_background};",
                    span { style: "font-weight:800; font-size:12px;", "Mission view" }
                    label { style: "display:flex; gap:6px; align-items:center; font-size:12px;", input { r#type:"checkbox", checked:prefs.show_stats, onchange:{ let mut preferences=preferences; move |event| { let mut next=preferences.read().clone(); next.show_stats=event.checked(); save_preferences(&next); preferences.set(next); } } } "Stats banner" }
                    label { style: "display:flex; gap:6px; align-items:center; font-size:12px;", input { r#type:"checkbox", checked:prefs.show_angle_strip, onchange:{ let mut preferences=preferences; move |event| { let mut next=preferences.read().clone(); next.show_angle_strip=event.checked(); save_preferences(&next); preferences.set(next); } } } "Camera strip" }
                    label { style: "display:flex; gap:6px; align-items:center; font-size:12px;", input { r#type:"checkbox", checked:prefs.show_feed_labels, onchange:{ let mut preferences=preferences; move |event| { let mut next=preferences.read().clone(); next.show_feed_labels=event.checked(); save_preferences(&next); preferences.set(next); } } } "Feed labels" }
                    button { style:"padding:5px 9px; border:1px solid {theme.button_border}; border-radius:9px; background:{theme.button_background}; color:{theme.button_text}; cursor:pointer;", onclick:{ let mut preferences=preferences; move |_| { let next=MissionViewPreferences::default(); save_preferences(&next); preferences.set(next); } }, "Use ground station default" }
                }
                if cfg.can_manage_stream {
                    super::stream_studio::StreamStudio { broadcast:cfg.broadcast.clone(), program_url:cfg.program_url.clone() }
                    div { class:"gs26-mission-editor", style: "display:flex; gap:8px; align-items:center; flex-wrap:wrap; margin-bottom:10px; padding:11px; border:1px solid {theme.warning_border}; border-radius:14px; background:{theme.warning_background};",
                        strong { style:"font-size:12px;", "STREAM MASTER" }
                        input { style:"flex:1; min-width:180px; padding:7px 9px; border:1px solid {theme.border}; border-radius:9px; background:{theme.panel_background}; color:{theme.text_primary};", placeholder:"Broadcast label (Flight Test 1)", value:"{broadcast_label_input.read()}", oninput:{ let mut broadcast_label_input=broadcast_label_input; move |event| broadcast_label_input.set(event.value()) } }
                        button {
                            style:"padding:6px 10px; border:1px solid {theme.button_border}; border-radius:9px; background:{theme.button_background}; color:{theme.button_text}; cursor:pointer;",
                            onclick:{
                                let cfg=cfg.clone();
                                let broadcast_label_input=broadcast_label_input;
                                let mut config=config;
                                let mut status=status;
                                move |_| {
                                    let mut request=cfg.broadcast.clone();
                                    request.label=broadcast_label_input.read().clone();
                                    spawn(async move {
                                        match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control", &request).await {
                                            Ok(saved) => { let mut next=config.read().clone().unwrap_or_default(); next.broadcast=saved; config.set(Some(next)); status.set("Broadcast label updated".into()); },
                                            Err(error) => status.set(format!("Stream control failed: {error}")),
                                        }
                                    });
                                }
                            },
                            "Apply label"
                        }
                        for layout_mode in ["hero", "grid"] {
                            button {
                                style: if cfg.broadcast.layout == layout_mode { "padding:6px 10px; border:2px solid #f4b860; border-radius:9px; background:#162235; color:#fde7b0; cursor:pointer;" } else { "padding:6px 10px; border:1px solid #42536d; border-radius:9px; background:#162235; color:#f3f7ff; cursor:pointer;" },
                                onclick:{
                                    let mut request=cfg.broadcast.clone();
                                    let layout_mode=layout_mode.to_string();
                                    let mut config=config;
                                    let mut status=status;
                                    move |_| {
                                        request.layout=layout_mode.clone();
                                        let request=request.clone();
                                        spawn(async move {
                                            match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control", &request).await {
                                                Ok(saved) => { let mut next=config.read().clone().unwrap_or_default(); next.broadcast=saved; config.set(Some(next)); status.set("Broadcast layout updated".into()); },
                                                Err(error) => status.set(format!("Stream control failed: {error}")),
                                            }
                                        });
                                    }
                                },
                                "{layout_mode.to_uppercase()}"
                            }
                        }
                        div { style:"flex:1 0 100%; display:flex; gap:8px; flex-wrap:wrap;",
                            for feed in cfg.streams.iter() {
                                label { style:"display:flex; gap:5px; align-items:center; padding:5px 7px; border:1px solid {theme.warning_border}; border-radius:8px; font-size:11px;",
                                    input {
                                        r#type:"checkbox",
                                        checked:!cfg.broadcast.hidden_stream_ids.contains(&feed.id),
                                        onchange:{
                                            let mut request=cfg.broadcast.clone();
                                            let feed_id=feed.id.clone();
                                            let mut config=config;
                                            let mut status=status;
                                            move |event| {
                                                request.hidden_stream_ids.retain(|id| id != &feed_id);
                                                if !event.checked() { request.hidden_stream_ids.push(feed_id.clone()); }
                                                let request=request.clone();
                                                spawn(async move {
                                                    match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control", &request).await {
                                                        Ok(saved) => { let mut next=config.read().clone().unwrap_or_default(); next.broadcast=saved; config.set(Some(next)); status.set("Broadcast camera visibility updated".into()); },
                                                        Err(error) => status.set(format!("Stream control failed: {error}")),
                                                    }
                                                });
                                            }
                                        }
                                    }
                                    "{feed.label}"
                                }
                            }
                        }
                    }
                }
            }
            div { class: "gs26-mission-hero", style: "border:1px solid {theme.tab_shell_border}; background:{theme.panel_background};",
                if cfg.broadcast.layout == "grid" && online_feeds.len() > 1 {
                    div { style:"position:absolute; inset:0; display:grid; grid-template-columns:repeat(auto-fit,minmax(320px,1fr)); gap:2px; background:{theme.border_soft};",
                        for feed in online_feeds.iter() {
                            div { key:"grid-feed-{feed.id}", style:"position:relative; min-height:220px; overflow:hidden; background:#02050a;",
                                {stream_media(feed, true)}
                                if prefs.show_feed_labels { span { style:"position:absolute; left:8px; top:8px; padding:5px 8px; border-radius:8px; background:rgba(0,0,0,.72); font-size:11px; font-weight:800;", "LIVE · {feed.label}" } }
                            }
                        }
                    }
                } else if let Some(feed) = selected.clone() {
                    div { key:"hero-feed-{feed.id}", style:"position:absolute; inset:0;", {stream_media(&feed, true)} }
                    if prefs.show_feed_labels {
                        div { style: "position:absolute; left:12px; top:12px; padding:7px 10px; border-radius:999px; background:{theme.overlay_background}; border:1px solid {theme.border}; font-size:12px; font-weight:800;", "LIVE · {feed.label}" }
                    }
                } else {
                    div {style:"position:absolute;inset:0 0 110px;overflow:auto;",
                        VehicleTab { theme: theme.clone(), flight_state, rocket_gps, rocket_altitude_m }
                    }
                }
                if prefs.show_stats {
                    div { class: "gs26-mission-banner", style: "background:{theme.overlay_background};",
                        div { style: "display:flex;flex-direction:column;gap:4px;",
                            span { style: "font-size:10px;text-transform:uppercase;letter-spacing:.12em;color:{theme.text_muted};", "T clock" }
                            super::LaunchClockBadge { launch_clock, network_time }
                        }
                        {broadcast_card(&theme, "Flight phase", flight_state.read().clone())}
                        if !cfg.broadcast.label.trim().is_empty() { {broadcast_card(&theme, "Mission", cfg.broadcast.label.clone())} }
                        {broadcast_card(&theme, "Altitude", (*rocket_altitude_m.read()).map(|v| format!("{v:.1} m")).unwrap_or_else(|| "--".into()))}
                        for stat in cfg.stats.iter() {
                            {broadcast_card(&theme, &stat.label, binding_value(&stat.binding).map(|value| if stat.unit.is_empty() { format!("{value:.precision$}", precision=stat.precision) } else { format!("{value:.precision$} {}", stat.unit, precision=stat.precision) }).unwrap_or_else(|| "--".into()))}
                        }
                    }
                }
            }
            if crate::auth::can_view_actions() && super::gse_panel::ground_visible(&flight_state.read()) {
                section {style:"margin:16px 0;",h2 {style:"font-size:18px;","Ground setup"}
                    super::gse_panel::GsePanel {action_policy,abort_only_mode,theme:theme.clone(),show_model:false}
                }
            }
            if prefs.show_angle_strip && online_feeds.len() > 1 {
                div { class: "gs26-angle-strip",
                    for feed in online_feeds.iter() {
                        button {
                            style: if selected.as_ref().is_some_and(|current| current.id == feed.id) { "position:relative; min-height:112px; overflow:hidden; padding:0; border:2px solid #38bdf8; border-radius:13px; background:#02050a; cursor:pointer; color:white;" } else { "position:relative; min-height:112px; overflow:hidden; padding:0; border:1px solid #42536d; border-radius:13px; background:#02050a; cursor:pointer; color:white;" },
                            onclick: {
                                let mut selected_stream_id=selected_stream_id;
                                let id=feed.id.clone();
                                let mut request=cfg.broadcast.clone();
                                let mut config=config;
                                let mut status=status;
                                move |_| {
                                    selected_stream_id.set(id.clone());
                                    if can_manage_stream {
                                        request.featured_stream_id=id.clone();
                                        let request=request.clone();
                                        spawn(async move {
                                            match http_post_json::<BroadcastState,BroadcastState>("/api/live_streams/control", &request).await {
                                                Ok(saved) => { let mut next=config.read().clone().unwrap_or_default(); next.broadcast=saved; config.set(Some(next)); status.set("Featured camera updated".into()); },
                                                Err(error) => status.set(format!("Stream control failed: {error}")),
                                            }
                                        });
                                    }
                                }
                            },
                            {stream_media(feed, false)}
                            span { style: "position:absolute; left:7px; bottom:7px; padding:4px 7px; border-radius:7px; background:rgba(0,0,0,.72); font-size:11px; font-weight:800;", "{feed.label}" }
                        }
                    }
                }
            }
        }
    }
}

fn stream_media(feed: &LiveStreamSpec, primary: bool) -> Element {
    let url = media_url(&feed.url);
    let poster = if feed.poster_url.trim().is_empty() {
        String::new()
    } else {
        media_url(&feed.poster_url)
    };
    let class = "gs26-mission-media";
    match feed.kind.as_str() {
        "iframe" | "webrtc" => {
            rsx! { iframe { class:"{class}", src:"{url}", title:"{feed.label}", allow:"autoplay; fullscreen; picture-in-picture" } }
        }
        "mjpeg" | "image" => {
            rsx! { img { class:"{class}", src:"{url}", alt:"Live view from {feed.label}" } }
        }
        _ if primary => {
            rsx! { video { key:"primary-{feed.id}", class:"{class}", src:"{url}", poster:"{poster}", controls:true, autoplay:true, muted:true, playsinline:true, preload:"auto" } }
        }
        _ => {
            rsx! { video { key:"preview-{feed.id}", class:"{class}", src:"{url}", poster:"{poster}", muted:true, playsinline:true, preload:"metadata" } }
        }
    }
}

fn broadcast_card(theme: &ThemeConfig, label: &str, value: String) -> Element {
    rsx! {
        div { style: "min-width:0; padding:10px 12px; background:{theme.overlay_background}; text-align:center;",
            div { style: "color:{theme.text_muted}; font-size:9px; font-weight:800; letter-spacing:.09em; text-transform:uppercase;", "{label}" }
            div { style: "margin-top:3px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:clamp(13px,2vw,20px); font-weight:900; font-variant-numeric:tabular-nums;", "{value}" }
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
