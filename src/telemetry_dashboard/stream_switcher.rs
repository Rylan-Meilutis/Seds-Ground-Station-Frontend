//! The visual stream manager (docs/broadcast-studio.md, "Stream manager"):
//! Preview and Program monitors, a grid of scene thumbnails, source tiles
//! dragged into a scene's slots, a canvas layout editor, Cut / Fade with a
//! duration, hotkeys and tally. Layouts, scenes and sources are the
//! station's (`/api/studio`); the station checks stream-manager access on
//! every request. Preview is this operator's own choice; Program is shared.
use super::live_stream_tab::{media_url, BroadcastState};
use super::neutral_request_owner;
use super::studio_model::{self as model, Layout, Scene, Slot, Source, StudioView, Tally, Transition, TransitionKind};
use dioxus::prelude::*;
use std::rc::Rc;

async fn sleep_ms(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
}

/// Keys reach Rust only outside text fields and without modifiers; Enter and
/// Space on a focused button keep their normal meaning.
const HOTKEY_JS: &str = r#"
window.__gs26SwitcherKeys?.();
const onKey=e=>{
 if(e.defaultPrevented||e.ctrlKey||e.metaKey||e.altKey||e.repeat)return;
 const t=e.target;if(t&&(t.isContentEditable||/^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName)))return;
 const k=e.key;if((k==='Enter'||k===' ')&&t&&/^(BUTTON|A|SUMMARY)$/.test(t.tagName))return;
 if(/^[1-9]$/.test(k)||k==='Enter'||k===' '||/^[cCtT]$/.test(k)){e.preventDefault();dioxus.send(k);}
};
document.addEventListener('keydown',onKey);
window.__gs26SwitcherKeys=()=>{document.removeEventListener('keydown',onKey);delete window.__gs26SwitcherKeys;};
"#;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Scene,
    Layouts,
}

#[derive(Clone, Copy, PartialEq)]
enum DragMode {
    Move,
    Resize,
}

#[derive(Clone, PartialEq)]
struct Drag {
    slot: String,
    mode: DragMode,
    start: (f64, f64),
    original: Slot,
}

fn tally_attr(t: Tally) -> &'static str {
    match t {
        Tally::Program => "program",
        Tally::Preview => "preview",
        Tally::Both => "both",
        Tally::Off => "off",
    }
}

fn tally_text(t: Tally) -> &'static str {
    match t {
        Tally::Program => ", on Program",
        Tally::Preview => ", in Preview",
        Tally::Both => ", on Program and in Preview",
        Tally::Off => "",
    }
}

fn badge(view: &str) -> &'static str {
    match view {
        "camera" => "CAM",
        "diagram" => "DIAG",
        "stats" => "DATA",
        "plot" => "PLOT",
        "tclock" | "tclock_bug" => "T−",
        "map" => "MAP",
        "model" => "3D",
        "recorded_replay" => "REPLAY",
        "lower_third" => "L3",
        "stats_bar" => "STRIP",
        "title" => "TITLE",
        "slate" => "SLATE",
        _ => "SRC",
    }
}

/// A live value for a telemetry tile (the station's dashboard snapshot).
fn live_value(source: &Source, telemetry: &Option<serde_json::Value>) -> Option<String> {
    let t = telemetry.as_ref()?;
    match source.view.as_str() {
        "tclock" | "tclock_bug" | "slate" => t["t_clock"].as_str().map(str::to_owned),
        "stats" | "stats_bar" | "plot" | "lower_third" => {
            let stats = t["stats"].as_array()?;
            let stat = if source.view == "lower_third" {
                stats.iter().find(|s| s["label"].as_str().is_some_and(|l| l.to_lowercase().contains("altitude"))).or(stats.first())?
            } else {
                stats.first()?
            };
            let value = stat["value"].as_f64()?;
            let precision = stat["precision"].as_u64().unwrap_or(1).min(6) as usize;
            Some(format!("{} {value:.precision$} {}", stat["label"].as_str().unwrap_or(""), stat["unit"].as_str().unwrap_or("")).trim().to_owned())
        }
        _ => None,
    }
}

fn source_by<'a>(sources: &'a [Source], id: &str) -> Option<&'a Source> {
    sources.iter().find(|s| s.id == id)
}

fn slot_style(slot: &Slot) -> String {
    format!(
        "left:{}%;top:{}%;width:{}%;height:{}%;z-index:{};",
        slot.x,
        slot.y,
        slot.width,
        slot.height,
        1 + slot.z as u32
    )
}

/// A source drawn small: kind badge, label, state and a live value.
fn source_chip(source: Option<&Source>, fallback: &str, telemetry: &Option<serde_json::Value>) -> Element {
    match source {
        None => rsx! { span { class: "sw-chip sw-chip-missing", span { class: "sw-badge", "?" } span { class: "sw-chip-label", "{fallback}" } } },
        Some(source) => {
            let value = live_value(source, telemetry);
            rsx! {
                span { class: "sw-chip", "data-kind": "{source.kind}", "data-state": "{source.state}", "data-transparent": source.transparent.to_string(),
                    span { class: "sw-badge", {badge(&source.view)} }
                    span { class: "sw-chip-label", "{source.label}" }
                    if let Some(value) = value { span { class: "sw-chip-value", "{value}" } }
                    if source.state != "live" && source.state != "ready" { span { class: "sw-chip-state", {model::state_label(&source.state)} } }
                }
            }
        }
    }
}

/// A scene's thumbnail: its layout's slots with each slot's source.
fn scene_thumb(layout: Option<&Layout>, scene: &Scene, sources: &[Source], telemetry: &Option<serde_json::Value>) -> Element {
    let Some(layout) = layout else {
        return rsx! { div { class: "sw-thumb", p { class: "sw-thumb-missing", "Layout missing" } } };
    };
    rsx! {
        div { class: "sw-thumb", "aria-hidden": "true",
            for slot in layout.slots.iter() {
                {
                    let assigned = scene.sources.get(&slot.id);
                    let source = assigned.and_then(|id| source_by(sources, id));
                    rsx! {
                        div { key: "{slot.id}", class: "sw-thumb-slot", "data-empty": assigned.is_none().to_string(), style: slot_style(slot),
                            if let Some(id) = assigned { {source_chip(source, id, telemetry)} }
                        }
                    }
                }
            }
        }
    }
}

fn layout_thumb(slots: &[Slot]) -> Element {
    rsx! {
        div { class: "sw-thumb sw-thumb-layout", "aria-hidden": "true",
            for slot in slots.iter() {
                div { key: "{slot.id}", class: "sw-thumb-slot", style: slot_style(slot) }
            }
        }
    }
}

fn preset_name(id: &str) -> &str {
    match id {
        "full" => "Full",
        "pip" => "Picture in picture",
        "side_by_side" => "Side by side",
        "grid_2x2" => "2 × 2",
        "lower_third" => "Lower third",
        other => other,
    }
}

#[component]
pub(super) fn StreamSwitcher(broadcast: BroadcastState) -> Element {
    let authority = neutral_request_owner::use_factory();
    let mut view = use_signal(|| None::<StudioView>);
    let mut status = use_signal(String::new);
    let mut preview = use_signal(|| None::<String>);
    let mut transition = use_signal(Transition::default);
    let mut transition_loaded = use_signal(|| false);
    let mut scenes = use_signal(Vec::<Scene>::new);
    let mut scenes_dirty = use_signal(|| false);
    let mut layouts = use_signal(Vec::<Layout>::new);
    let mut layouts_dirty = use_signal(|| false);
    let mut tab = use_signal(|| Tab::Scene);
    let mut dragging = use_signal(|| None::<String>);
    let mut selected_source = use_signal(|| None::<String>);
    let mut live_thumbs = use_signal(|| false);
    let mut listen = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut telemetry = use_signal(|| None::<serde_json::Value>);
    let mut editing_layout = use_signal(|| None::<String>);
    let mut selected_slot = use_signal(|| None::<String>);
    let mut drag = use_signal(|| None::<Drag>);
    let mut canvas = use_signal(|| (0.0f64, 0.0f64));
    let mut canvas_node = use_signal(|| None::<Rc<MountedData>>);

    // A new station view replaces the drafts unless this operator is editing.
    let mut apply = move |next: StudioView| {
        if !*scenes_dirty.peek() {
            scenes.set(next.scenes.clone());
        }
        if !*layouts_dirty.peek() {
            layouts.set(next.layouts.clone());
        }
        if !*transition_loaded.peek() {
            transition.set(next.transition.clone());
            transition_loaded.set(true);
        }
        let current = preview.peek().clone();
        if current.as_ref().is_none_or(|id| !next.scenes.iter().any(|s| &s.id == id)) {
            preview.set(next.scenes.iter().find(|s| s.id != next.program_scene_id).map(|s| s.id.clone()));
        }
        if editing_layout.peek().as_ref().is_none_or(|id| !next.layouts.iter().any(|l| &l.id == id)) && !*layouts_dirty.peek() {
            editing_layout.set(next.layouts.first().map(|l| l.id.clone()));
        }
        if view.peek().as_ref() != Some(&next) {
            view.set(Some(next));
        }
    };

    use_future(move || authority.run(async move {
        loop {
            match authority.get::<StudioView>("/api/studio").await {
                Ok(next) => {
                    apply(next);
                    if status.peek().starts_with("Studio unavailable") {
                        status.set(String::new());
                    }
                }
                Err(error) => status.set(format!("Studio unavailable: {error}")),
            }
            if let Ok(snapshot) = authority.get::<serde_json::Value>("/api/dashboard_status").await {
                telemetry.set(Some(snapshot));
            }
            sleep_ms(2000).await;
        }
    }));

    let mut take = move |kind: TransitionKind| {
        let Some(scene_id) = preview.peek().clone() else {
            status.set("Click a scene to load it into Preview first".into());
            return;
        };
        if *scenes_dirty.peek() {
            status.set("Save the scene changes before taking them to Program".into());
            return;
        }
        if !authority.current() || *busy.peek() {
            return;
        }
        let mut chosen = transition.peek().clone();
        chosen.kind = kind;
        if kind == TransitionKind::Cut {
            chosen.duration_ms = 0;
        }
        let previous = view.peek().as_ref().map(|v| v.program_scene_id.clone()).unwrap_or_default();
        busy.set(true);
        authority.spawn(async move {
            let request = model::TakeRequest { scene_id: scene_id.clone(), transition: chosen };
            match authority.post::<_, StudioView>("/api/studio/take", &request).await {
                Ok(next) => {
                    let name = next.scenes.iter().find(|s| s.id == scene_id).map(|s| s.name.clone()).unwrap_or_default();
                    apply(next);
                    // Program and Preview swap, as on a vision mixer.
                    if !previous.is_empty() && previous != scene_id {
                        preview.set(Some(previous));
                    }
                    status.set(format!("{name} on Program ({})", if kind == TransitionKind::Cut { "cut" } else { "fade" }));
                }
                Err(error) => status.set(error),
            }
            busy.set(false);
        });
    };

    use_future(move || async move {
        let mut keys = document::eval(HOTKEY_JS);
        while let Ok(key) = keys.recv::<String>().await {
            match model::hotkey(&key) {
                Some(model::Hotkey::Preview(n)) => {
                    if let Some(scene) = scenes.peek().get(n) {
                        preview.set(Some(scene.id.clone()));
                        tab.set(Tab::Scene);
                    }
                }
                Some(model::Hotkey::Cut) => take(TransitionKind::Cut),
                Some(model::Hotkey::Transition) => take(TransitionKind::Fade),
                None => {}
            }
        }
    });
    use_drop(|| {
        document::eval("window.__gs26SwitcherKeys?.();");
    });

    let save_scenes = move |_| {
        let Some(revision) = view.peek().as_ref().map(|v| v.revision) else { return };
        if !authority.current() {
            return;
        }
        busy.set(true);
        let request = model::ScenesRequest { revision, scenes: scenes.peek().clone() };
        authority.spawn(async move {
            match authority.post::<_, StudioView>("/api/studio/scenes", &request).await {
                Ok(next) => {
                    scenes_dirty.set(false);
                    apply(next);
                    status.set("Scenes saved".into());
                }
                Err(error) => status.set(error),
            }
            busy.set(false);
        });
    };
    let save_layouts = move |_| {
        let Some(revision) = view.peek().as_ref().map(|v| v.revision) else { return };
        if !authority.current() {
            return;
        }
        busy.set(true);
        let request = model::LayoutsRequest { revision, layouts: layouts.peek().clone() };
        authority.spawn(async move {
            match authority.post::<_, StudioView>("/api/studio/layouts", &request).await {
                Ok(next) => {
                    layouts_dirty.set(false);
                    apply(next);
                    status.set("Layouts saved".into());
                }
                Err(error) => status.set(error),
            }
            busy.set(false);
        });
    };

    if !authority.current() {
        return neutral_request_owner::retired_notice();
    }
    let Some(current) = view.read().clone() else {
        return rsx! {
            section { class: "gs26-switcher", "aria-label": "Stream manager",
                style { {include_str!("stream_switcher.css")} }
                p { role: "status", if status.read().is_empty() { "Loading stream manager…" } else { "{status}" } }
            }
        };
    };
    let program_id = current.program_scene_id.clone();
    let preview_id = preview.read().clone();
    let on_program = model::scene_sources(&current, current.on_air.then_some(program_id.as_str()));
    // Tally follows the scenes as the station saved them; the editor shows the draft.
    let on_preview = model::scene_sources(&current, preview_id.as_deref());
    let draft_scenes = scenes.read().clone();
    let draft_layouts = layouts.read().clone();
    let layout_of = |scene: &Scene| draft_layouts.iter().find(|l| l.id == scene.layout_id).cloned();
    let telemetry_now = telemetry.read().clone();
    let program_url = current.program_url.clone();
    let preview_scene = preview_id.as_ref().and_then(|id| draft_scenes.iter().find(|s| &s.id == id)).cloned();
    let program_name = current.scenes.iter().find(|s| s.id == program_id).map(|s| s.name.clone());
    let chosen = transition.read().clone();
    let selected = selected_source.read().clone();
    let selected_label = selected.as_ref().and_then(|id| source_by(&current.sources, id)).map(|s| s.label.clone());
    let groups = [("camera", "Cameras"), ("telemetry", "Telemetry views"), ("overlay", "Overlays"), ("graphic", "Graphics")];
    let busy_now = *busy.read();
    let dirty_scenes = *scenes_dirty.read();
    let dirty_layouts = *layouts_dirty.read();

    rsx! {
        section { class: "gs26-switcher", "aria-label": "Stream manager",
            style { {include_str!("stream_switcher.css")} }
            div { class: "sw-monitors",
                figure { class: "sw-monitor", "data-tally": "preview", "aria-label": "Preview monitor",
                    figcaption {
                        span { class: "sw-tag sw-tag-preview", "Preview" }
                        span { class: "sw-monitor-name", {preview_scene.as_ref().map(|s| s.name.clone()).unwrap_or_else(|| "No scene".into())} }
                    }
                    div { class: "sw-screen",
                        match preview_id.as_ref() {
                            Some(id) if !program_url.is_empty() => {
                                let url = media_url(&model::preview_url(&program_url, id));
                                rsx! { iframe { key: "{url}", src: "{url}", title: "Preview monitor", allow: "autoplay" } }
                            }
                            _ => rsx! { p { "Click a scene to load it into Preview." } },
                        }
                    }
                }
                div { class: "sw-take", role: "group", "aria-label": "Take to Program",
                    button { class: "sw-cut", r#type: "button", disabled: busy_now || preview_id.is_none(), title: "Cut to Program (Enter or C)", onclick: move |_| take(TransitionKind::Cut), "Cut" }
                    button { class: "sw-fade", r#type: "button", disabled: busy_now || preview_id.is_none(), title: "Fade to Program (Space or T)", onclick: move |_| take(TransitionKind::Fade), "Fade" }
                    label { class: "sw-duration",
                        span { "Fade" }
                        input { r#type: "range", min: "100", max: "{model::MAX_TRANSITION_MS}", step: "100", "aria-label": "Fade duration (ms)", value: "{chosen.duration_ms.max(100)}",
                            oninput: move |e| if let Ok(ms) = e.value().parse::<u32>() { transition.write().duration_ms = ms.min(model::MAX_TRANSITION_MS); } }
                        output { "{chosen.duration_ms.max(100)} ms" }
                    }
                    p { class: "sw-keys", kbd { "1" } "–" kbd { "9" } " preview · " kbd { "Enter" } " cut · " kbd { "Space" } " fade" }
                }
                figure { class: "sw-monitor", "data-tally": "program", "aria-label": "Program monitor",
                    figcaption {
                        span { class: "sw-tag sw-tag-program", "Program" }
                        span { class: "sw-monitor-name", {if current.on_air { program_name.clone().unwrap_or_default() } else { "Classic layout (not a studio scene)".into() }} }
                        button { class: "sw-listen", r#type: "button", "aria-pressed": listen().to_string(), onclick: move |_| listen.toggle(), if listen() { "Mute" } else { "Listen" } }
                    }
                    div { class: "sw-screen",
                        if !program_url.is_empty() {
                            {
                                let url = media_url(&model::program_monitor_url(&program_url, listen()));
                                rsx! { iframe { key: "{url}", src: "{url}", title: "Program monitor — what viewers see", allow: "autoplay; fullscreen" } }
                            }
                        }
                    }
                }
            }
            if !status.read().is_empty() { p { class: "sw-status", role: "status", "{status}" } }
            div { class: "sw-scenes", role: "group", "aria-label": "Scenes",
                for (index, scene) in draft_scenes.iter().enumerate() {
                    {
                        let id = scene.id.clone();
                        let take_id = scene.id.clone();
                        let program = current.on_air && scene.id == program_id;
                        let in_preview = preview_id.as_deref() == Some(scene.id.as_str());
                        let t = match (program, in_preview) { (true, true) => Tally::Both, (true, false) => Tally::Program, (false, true) => Tally::Preview, _ => Tally::Off };
                        let hot = if index < 9 { format!("{}", index + 1) } else { String::new() };
                        let saved = current.scenes.iter().any(|s| s == scene);
                        rsx! {
                            button { key: "{scene.id}", class: "sw-scene", r#type: "button", "data-tally": tally_attr(t), "aria-pressed": in_preview.to_string(),
                                "aria-label": format!("Scene {}: {}{}", index + 1, scene.name, tally_text(t)),
                                onclick: move |_| { preview.set(Some(id.clone())); tab.set(Tab::Scene); },
                                ondoubleclick: move |_| { preview.set(Some(take_id.clone())); take(TransitionKind::Fade); },
                                {scene_thumb(layout_of(scene).as_ref(), scene, &current.sources, &telemetry_now)}
                                span { class: "sw-scene-name",
                                    if !hot.is_empty() { kbd { "{hot}" } }
                                    span { "{scene.name}" }
                                    if !saved { span { class: "sw-unsaved", title: "Unsaved changes", "•" } }
                                }
                                if program { span { class: "sw-flag sw-flag-program", "PGM" } }
                                if in_preview { span { class: "sw-flag sw-flag-preview", "PVW" } }
                            }
                        }
                    }
                }
                button { class: "sw-scene sw-new", r#type: "button", disabled: draft_scenes.len() >= 24 || draft_layouts.is_empty(),
                    onclick: move |_| {
                        let layout = layouts.peek().first().map(|l| l.id.clone()).unwrap_or_default();
                        let scene = model::new_scene(&scenes.peek(), &format!("Scene {}", scenes.peek().len() + 1), &layout);
                        let id = scene.id.clone();
                        scenes.write().push(scene);
                        scenes_dirty.set(true);
                        preview.set(Some(id));
                        tab.set(Tab::Scene);
                    },
                    span { class: "sw-plus", "+" } span { "New scene" }
                }
            }
            div { class: "sw-tabs", role: "tablist", "aria-label": "Editor",
                button { r#type: "button", role: "tab", "aria-selected": (tab() == Tab::Scene).to_string(), onclick: move |_| tab.set(Tab::Scene), "Scene & sources" }
                button { r#type: "button", role: "tab", "aria-selected": (tab() == Tab::Layouts).to_string(), onclick: move |_| tab.set(Tab::Layouts), "Layout editor" }
            }
            if tab() == Tab::Scene {
                div { class: "sw-workspace",
                    aside { class: "sw-sources", "aria-label": "Sources",
                        header {
                            h4 { "Sources" }
                            label { input { r#type: "checkbox", checked: live_thumbs(), onchange: move |e| live_thumbs.set(e.checked()) } " Live camera thumbnails" }
                        }
                        for (kind, title) in groups {
                            if current.sources.iter().any(|s| s.kind == kind) {
                                h5 { "{title}" }
                                div { class: "sw-source-grid",
                                    for source in current.sources.iter().filter(|s| s.kind == kind) {
                                        {
                                            let t = model::tally(&on_program, &on_preview, &source.id);
                                            let id = source.id.clone();
                                            let drag_id = source.id.clone();
                                            let key_id = source.id.clone();
                                            let is_selected = selected.as_deref() == Some(source.id.as_str());
                                            let thumb_url = source.preview_url.clone().filter(|_| live_thumbs() && source.state == "live");
                                            let value = live_value(source, &telemetry_now);
                                            let visibility = source.stream_id.clone().filter(|_| source.id.starts_with("camera:"));
                                            let hidden = source.hidden;
                                            rsx! {
                                                div { key: "{source.id}", class: "sw-source", role: "button", tabindex: "0", draggable: "true",
                                                    "data-tally": tally_attr(t), "data-state": "{source.state}", "data-kind": "{source.kind}", "data-source": "{source.id}",
                                                    "aria-pressed": is_selected.to_string(),
                                                    "aria-label": format!("{}, {}{}", source.label, model::state_label(&source.state), tally_text(t)),
                                                    ondragstart: move |_| dragging.set(Some(drag_id.clone())),
                                                    ondragend: move |_| dragging.set(None),
                                                    onclick: move |_| { let next = if selected_source.peek().as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) }; selected_source.set(next); },
                                                    onkeydown: move |e: Event<KeyboardData>| if e.key() == Key::Enter { e.stop_propagation(); selected_source.set(Some(key_id.clone())); },
                                                    div { class: "sw-source-art", "data-view": "{source.view}",
                                                        if let Some(url) = thumb_url {
                                                            iframe { src: media_url(&url), title: "{source.label} live thumbnail", allow: "autoplay", tabindex: "-1" }
                                                        } else {
                                                            span { class: "sw-badge", {badge(&source.view)} }
                                                            if let Some(value) = value { span { class: "sw-art-value", "{value}" } }
                                                        }
                                                        if t != Tally::Off { span { class: "sw-tally-light", "data-tally": tally_attr(t), {match t { Tally::Program | Tally::Both => "PGM", _ => "PVW" }} } }
                                                    }
                                                    div { class: "sw-source-meta",
                                                        span { class: "sw-dot", "data-state": "{source.state}" }
                                                        strong { "{source.label}" }
                                                        small { {model::state_label(&source.state)} if source.transparent { " · transparent" } if hidden { " · hidden from auto" } }
                                                    }
                                                    if let Some(stream) = visibility {
                                                        button { class: "sw-mini", r#type: "button", disabled: busy_now,
                                                            "aria-label": format!("{} {} from automatic cameras", if hidden { "Show" } else { "Hide" }, source.label),
                                                            onclick: {
                                                                let broadcast = broadcast.clone();
                                                                move |e: Event<MouseData>| {
                                                                    e.stop_propagation();
                                                                    let mut request = broadcast.clone();
                                                                    request.hidden_stream_ids.retain(|s| s != &stream);
                                                                    if !hidden { request.hidden_stream_ids.push(stream.clone()); }
                                                                    if !authority.current() { return; }
                                                                    authority.spawn(async move {
                                                                        match authority.post::<BroadcastState, BroadcastState>("/api/live_streams/control", &request).await {
                                                                            Ok(_) => status.set("Camera visibility saved".into()),
                                                                            Err(error) => status.set(error),
                                                                        }
                                                                    });
                                                                }
                                                            },
                                                            if hidden { "Show" } else { "Hide" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "sw-editor",
                        if let Some(scene) = preview_scene.clone() {
                            {
                                let layout = layout_of(&scene);
                                let scene_id = scene.id.clone();
                                let rename_id = scene.id.clone();
                                let bar_id = scene.id.clone();
                                let delete_id = scene.id.clone();
                                let copy = scene.clone();
                                let is_program = current.on_air && scene.id == program_id;
                                rsx! {
                                    header { class: "sw-editor-head",
                                        input { "aria-label": "Scene name", value: "{scene.name}", maxlength: "60",
                                            oninput: move |e| { if let Some(s) = scenes.write().iter_mut().find(|s| s.id == rename_id) { s.name = e.value(); } scenes_dirty.set(true); } }
                                        label { input { r#type: "checkbox", checked: scene.program_bar,
                                            onchange: move |e| { if let Some(s) = scenes.write().iter_mut().find(|s| s.id == bar_id) { s.program_bar = e.checked(); } scenes_dirty.set(true); } } " Programme telemetry bar" }
                                        button { r#type: "button", class: "sw-primary", disabled: busy_now || !dirty_scenes, onclick: save_scenes, "Save scenes" }
                                        button { r#type: "button", disabled: busy_now || !dirty_scenes,
                                            onclick: move |_| { scenes_dirty.set(false); if let Some(v) = view.peek().clone() { scenes.set(v.scenes.clone()); } status.set("Scene changes discarded".into()); }, "Revert" }
                                        button { r#type: "button", disabled: draft_scenes.len() >= 24,
                                            onclick: move |_| { let mut c = copy.clone(); let fresh = model::new_scene(&scenes.peek(), &format!("{} copy", c.name), &c.layout_id); c.id = fresh.id.clone(); c.name = fresh.name; let id = c.id.clone(); scenes.write().push(c); scenes_dirty.set(true); preview.set(Some(id)); }, "Duplicate" }
                                        button { r#type: "button", disabled: is_program, title: if is_program { "Take another scene before deleting this one" } else { "" },
                                            onclick: move |_| { scenes.write().retain(|s| s.id != delete_id); scenes_dirty.set(true); preview.set(scenes.peek().iter().find(|s| s.id != program_id).map(|s| s.id.clone())); }, "Delete" }
                                    }
                                    div { class: "sw-layout-picker", role: "group", "aria-label": "Scene layout",
                                        for candidate in draft_layouts.iter() {
                                            {
                                                let chosen_layout = candidate.clone();
                                                let target = scene_id.clone();
                                                let pressed = candidate.id == scene.layout_id;
                                                rsx! {
                                                    button { key: "{candidate.id}", r#type: "button", class: "sw-layout-choice", "aria-pressed": pressed.to_string(), "aria-label": format!("Use layout {}", candidate.name),
                                                        onclick: move |_| { if let Some(s) = scenes.write().iter_mut().find(|s| s.id == target) { s.layout_id = chosen_layout.id.clone(); model::fit_to_layout(s, &chosen_layout); } scenes_dirty.set(true); },
                                                        {layout_thumb(&candidate.slots)}
                                                        span { "{candidate.name}" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    if let Some(layout) = layout {
                                        div { class: "sw-canvas", "aria-label": "Scene canvas: drop sources into slots",
                                            for slot in layout.slots.iter() {
                                                {
                                                    let assigned = scene.sources.get(&slot.id).cloned();
                                                    let source = assigned.as_ref().and_then(|id| source_by(&current.sources, id)).cloned();
                                                    let drop_scene = scene_id.clone();
                                                    let drop_slot = slot.id.clone();
                                                    let click_scene = scene_id.clone();
                                                    let click_slot = slot.id.clone();
                                                    let clear_scene = scene_id.clone();
                                                    let clear_slot = slot.id.clone();
                                                    let label = source.as_ref().map(|s| s.label.clone()).or(assigned.clone()).unwrap_or_else(|| "empty".into());
                                                    rsx! {
                                                        div { key: "{slot.id}", class: "sw-slot", role: "button", tabindex: "0", "data-slot": "{slot.id}", "data-empty": assigned.is_none().to_string(),
                                                            "aria-label": format!("Slot {}: {}", slot.id, label), style: slot_style(slot),
                                                            ondragover: move |e: Event<DragData>| e.prevent_default(),
                                                            ondrop: move |e: Event<DragData>| {
                                                                e.prevent_default();
                                                                let Some(source) = dragging.peek().clone() else { return };
                                                                dragging.set(None);
                                                                if let Some(s) = scenes.write().iter_mut().find(|s| s.id == drop_scene) { model::assign(s, &drop_slot, &source); }
                                                                scenes_dirty.set(true);
                                                            },
                                                            onclick: move |_| {
                                                                let Some(source) = selected_source.peek().clone() else { return };
                                                                if let Some(s) = scenes.write().iter_mut().find(|s| s.id == click_scene) { model::assign(s, &click_slot, &source); }
                                                                scenes_dirty.set(true);
                                                            },
                                                            if let Some(id) = assigned.as_ref() {
                                                                {source_chip(source.as_ref(), id, &telemetry_now)}
                                                                button { class: "sw-clear", r#type: "button", "aria-label": format!("Clear slot {}", slot.id),
                                                                    onclick: move |e: Event<MouseData>| { e.stop_propagation(); if let Some(s) = scenes.write().iter_mut().find(|s| s.id == clear_scene) { s.sources.remove(&clear_slot); } scenes_dirty.set(true); }, "×" }
                                                            } else {
                                                                span { class: "sw-drop-hint", "Drop a source" }
                                                            }
                                                            span { class: "sw-slot-id", "{slot.id}" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        p { "This scene's layout no longer exists; choose a layout." }
                                    }
                                    if let Some(label) = selected_label.clone() {
                                        p { class: "sw-hint", "Tap a slot to place " strong { "{label}" } ", or drag a source tile onto a slot." }
                                        if let Some(source) = selected.as_ref().and_then(|id| source_by(&current.sources, id)) {
                                            if source.kind != "camera" && !program_url.is_empty() {
                                                label { class: "sw-url", "Browser source (OBS) for {source.label}"
                                                    input { r#type: "url", readonly: true, value: format!("{}{}", super::UrlConfig::base_http().trim_end_matches('/'), model::source_url(&program_url, source)) } }
                                            }
                                        }
                                    } else {
                                        p { class: "sw-hint", "Drag a source onto a slot, or select a source and then tap a slot." }
                                    }
                                }
                            }
                        } else {
                            p { "Select or create a scene." }
                        }
                    }
                }
            } else {
                div { class: "sw-layouts",
                    div { class: "sw-layout-list", role: "group", "aria-label": "Layouts",
                        for candidate in draft_layouts.iter() {
                            {
                                let id = candidate.id.clone();
                                let pressed = editing_layout.read().as_deref() == Some(candidate.id.as_str());
                                rsx! {
                                    button { key: "{candidate.id}", r#type: "button", class: "sw-layout-choice", "aria-pressed": pressed.to_string(), "aria-label": format!("Edit layout {}", candidate.name),
                                        onclick: move |_| { editing_layout.set(Some(id.clone())); selected_slot.set(None); },
                                        {layout_thumb(&candidate.slots)}
                                        span { "{candidate.name}" }
                                    }
                                }
                            }
                        }
                        button { r#type: "button", class: "sw-layout-choice sw-new", disabled: draft_layouts.len() >= 32,
                            onclick: move |_| {
                                let slots = view.peek().as_ref().and_then(|v| v.presets.first().map(|p| p.slots.clone())).unwrap_or_default();
                                let layout = model::new_layout(&layouts.peek(), &format!("Layout {}", layouts.peek().len() + 1), slots);
                                let id = layout.id.clone();
                                layouts.write().push(layout);
                                layouts_dirty.set(true);
                                editing_layout.set(Some(id));
                            },
                            span { class: "sw-plus", "+" } span { "New layout" }
                        }
                    }
                    if let Some(layout) = editing_layout.read().as_ref().and_then(|id| draft_layouts.iter().find(|l| &l.id == id)).cloned() {
                        {
                            let layout_id = layout.id.clone();
                            let rename_id = layout.id.clone();
                            let delete_id = layout.id.clone();
                            let add_id = layout.id.clone();
                            let used = draft_scenes.iter().any(|s| s.layout_id == layout.id);
                            let selected_geometry = selected_slot.read().as_ref().and_then(|id| layout.slots.iter().find(|s| &s.id == id)).cloned();
                            rsx! {
                                header { class: "sw-editor-head",
                                    input { "aria-label": "Layout name", value: "{layout.name}", maxlength: "60",
                                        oninput: move |e| { if let Some(l) = layouts.write().iter_mut().find(|l| l.id == rename_id) { l.name = e.value(); } layouts_dirty.set(true); } }
                                    button { r#type: "button", class: "sw-primary", disabled: busy_now || !dirty_layouts || !draft_layouts.iter().all(|l| l.slots.iter().all(model::inside)), onclick: save_layouts, "Save layouts" }
                                    button { r#type: "button", disabled: busy_now || !dirty_layouts,
                                        onclick: move |_| { layouts_dirty.set(false); if let Some(v) = view.peek().clone() { layouts.set(v.layouts.clone()); } status.set("Layout changes discarded".into()); }, "Revert" }
                                    button { r#type: "button", disabled: used, title: if used { "A scene uses this layout" } else { "" },
                                        onclick: move |_| { layouts.write().retain(|l| l.id != delete_id); layouts_dirty.set(true); editing_layout.set(layouts.peek().first().map(|l| l.id.clone())); }, "Delete layout" }
                                }
                                div { class: "sw-presets", role: "group", "aria-label": "Aspect presets",
                                    for preset in current.presets.iter() {
                                        {
                                            let slots = preset.slots.clone();
                                            let target = layout_id.clone();
                                            rsx! {
                                                button { key: "{preset.id}", r#type: "button", class: "sw-layout-choice", "aria-label": format!("Preset {}", preset_name(&preset.id)),
                                                    onclick: move |_| { if let Some(l) = layouts.write().iter_mut().find(|l| l.id == target) { l.slots = slots.clone(); } layouts_dirty.set(true); selected_slot.set(None); },
                                                    {layout_thumb(&preset.slots)}
                                                    span { {preset_name(&preset.id)} }
                                                }
                                            }
                                        }
                                    }
                                    button { r#type: "button", class: "sw-layout-choice", disabled: layout.slots.len() >= 12,
                                        onclick: move |_| { let mut id = None; if let Some(l) = layouts.write().iter_mut().find(|l| l.id == add_id) { let s = model::new_slot(l); id = Some(s.id.clone()); l.slots.push(s); } layouts_dirty.set(true); selected_slot.set(id); },
                                        span { class: "sw-plus", "+" } span { "Add slot" }
                                    }
                                }
                                div { class: "sw-canvas sw-grid", "aria-label": "Layout canvas: drag a slot to move it, its corner to resize; arrows move, Shift+arrows resize",
                                    onmounted: move |e: Event<MountedData>| async move {
                                        let node = e.data();
                                        if let Ok(rect) = node.get_client_rect().await { canvas.set((rect.width(), rect.height())); }
                                        canvas_node.set(Some(node));
                                    },
                                    onpointermove: move |e: Event<PointerData>| {
                                        let Some(d) = drag.peek().clone() else { return };
                                        // Released outside the canvas: the drag is over.
                                        if e.held_buttons().is_empty() { drag.set(None); return; }
                                        let (w, h) = *canvas.peek();
                                        if w <= 0.0 || h <= 0.0 { return; }
                                        let p = e.client_coordinates();
                                        let (dx, dy) = ((p.x - d.start.0) / w * 100.0, (p.y - d.start.1) / h * 100.0);
                                        let next = match d.mode { DragMode::Move => model::moved(&d.original, dx, dy), DragMode::Resize => model::resized(&d.original, dx, dy) };
                                        let target = editing_layout.peek().clone();
                                        if let Some(l) = layouts.write().iter_mut().find(|l| Some(&l.id) == target.as_ref()) {
                                            if let Some(s) = l.slots.iter_mut().find(|s| s.id == d.slot) { if *s != next { *s = next; layouts_dirty.set(true); } }
                                        }
                                    },
                                    // No pointerleave: delegated, it also fires on leaving a slot.
                                    onpointerup: move |_| drag.set(None),
                                    onpointercancel: move |_| drag.set(None),
                                    for slot in layout.slots.iter() {
                                        {
                                            let move_slot = slot.clone();
                                            let resize_slot = slot.clone();
                                            let key_slot = slot.clone();
                                            let key_layout = layout_id.clone();
                                            let is_selected = selected_slot.read().as_deref() == Some(slot.id.as_str());
                                            rsx! {
                                                div { key: "{slot.id}", class: "sw-slot sw-edit", role: "button", tabindex: "0", "data-slot": "{slot.id}", "aria-pressed": is_selected.to_string(),
                                                    "aria-label": format!("Slot {} at {}%, {}%, {} by {}", slot.id, slot.x, slot.y, slot.width, slot.height), style: slot_style(slot),
                                                    onpointerdown: move |e: Event<PointerData>| {
                                                        let p = e.client_coordinates();
                                                        selected_slot.set(Some(move_slot.id.clone()));
                                                        drag.set(Some(Drag { slot: move_slot.id.clone(), mode: DragMode::Move, start: (p.x, p.y), original: move_slot.clone() }));
                                                        if let Some(node) = canvas_node.peek().clone() { spawn(async move { if let Ok(rect) = node.get_client_rect().await { canvas.set((rect.width(), rect.height())); } }); }
                                                    },
                                                    onkeydown: move |e: Event<KeyboardData>| {
                                                        let shift = e.modifiers().contains(Modifiers::SHIFT);
                                                        let (dx, dy) = match e.key() { Key::ArrowLeft => (-model::GRID, 0.0), Key::ArrowRight => (model::GRID, 0.0), Key::ArrowUp => (0.0, -model::GRID), Key::ArrowDown => (0.0, model::GRID), _ => return };
                                                        e.prevent_default();
                                                        let next = if shift { model::resized(&key_slot, dx, dy) } else { model::moved(&key_slot, dx, dy) };
                                                        if let Some(l) = layouts.write().iter_mut().find(|l| l.id == key_layout) { if let Some(s) = l.slots.iter_mut().find(|s| s.id == key_slot.id) { *s = next; } }
                                                        layouts_dirty.set(true);
                                                        selected_slot.set(Some(key_slot.id.clone()));
                                                    },
                                                    span { class: "sw-slot-id", "{slot.id}" }
                                                    span { class: "sw-slot-geom", "{slot.width}×{slot.height}" }
                                                    div { class: "sw-handle", "aria-hidden": "true",
                                                        onpointerdown: move |e: Event<PointerData>| {
                                                            e.stop_propagation();
                                                            let p = e.client_coordinates();
                                                            selected_slot.set(Some(resize_slot.id.clone()));
                                                            drag.set(Some(Drag { slot: resize_slot.id.clone(), mode: DragMode::Resize, start: (p.x, p.y), original: resize_slot.clone() }));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                if let Some(slot) = selected_geometry {
                                    {
                                        let field_layout = layout_id.clone();
                                        let field_slot = slot.clone();
                                        let field = move |label: &'static str, value: f64, which: usize| {
                                            let target_layout = field_layout.clone();
                                            let target = field_slot.clone();
                                            rsx! {
                                                label { "{label}"
                                                    input { r#type: "number", min: "0", max: "100", step: "{model::GRID}", value: "{value}",
                                                        onchange: move |e| {
                                                            let Ok(v) = e.value().parse::<f64>() else { return };
                                                            let g = [target.x, target.y, target.width, target.height];
                                                            let mut n = g; n[which] = v;
                                                            let next = model::with_geometry(&target, n[0], n[1], n[2], n[3]);
                                                            if let Some(l) = layouts.write().iter_mut().find(|l| l.id == target_layout) { if let Some(s) = l.slots.iter_mut().find(|s| s.id == target.id) { *s = next; } }
                                                            layouts_dirty.set(true);
                                                        } }
                                                }
                                            }
                                        };
                                        let z_layout = layout_id.clone();
                                        let z_slot = slot.id.clone();
                                        let remove_layout = layout_id.clone();
                                        let remove_slot = slot.id.clone();
                                        let in_use = draft_scenes.iter().any(|s| s.layout_id == layout_id && s.sources.contains_key(&slot.id));
                                        rsx! {
                                            fieldset { class: "sw-slot-fields",
                                                legend { "Slot {slot.id} (percent of the frame)" }
                                                {field("X", slot.x, 0)}
                                                {field("Y", slot.y, 1)}
                                                {field("Width", slot.width, 2)}
                                                {field("Height", slot.height, 3)}
                                                label { "Layer"
                                                    input { r#type: "number", min: "0", max: "9", step: "1", value: "{slot.z}",
                                                        onchange: move |e| { if let Ok(z) = e.value().parse::<u8>() { if let Some(l) = layouts.write().iter_mut().find(|l| l.id == z_layout) { if let Some(s) = l.slots.iter_mut().find(|s| s.id == z_slot) { s.z = z.min(9); } } layouts_dirty.set(true); } } }
                                                }
                                                button { r#type: "button", disabled: in_use || layout.slots.len() <= 1, title: if in_use { "A scene fills this slot" } else { "" },
                                                    onclick: move |_| { if let Some(l) = layouts.write().iter_mut().find(|l| l.id == remove_layout) { l.slots.retain(|s| s.id != remove_slot); } layouts_dirty.set(true); selected_slot.set(None); }, "Remove slot" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

