//! The stream manager's scene/source model (`GET /api/studio`,
//! docs/backend-contract.md 5.12) and the editing rules the visual editor
//! applies before saving: slot geometry on a 16:9 canvas in percent, grid
//! snapping, one source per slot, tally and hotkeys. Everything here is
//! generic: layouts, scenes and sources come from the station.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Smallest slot, in percent of the canvas (the station rejects smaller).
pub(crate) const MIN_SLOT: f64 = 5.0;
/// Editor grid, in percent.
pub(crate) const GRID: f64 = 2.5;
pub(crate) const MAX_TRANSITION_MS: u32 = 5000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Slot {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub z: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Layout {
    pub id: String,
    pub name: String,
    pub slots: Vec<Slot>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Scene {
    pub id: String,
    pub name: String,
    pub layout_id: String,
    /// Slot id -> source id.
    #[serde(default)]
    pub sources: BTreeMap<String, String>,
    #[serde(default = "yes")]
    pub program_bar: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TransitionKind {
    Cut,
    #[default]
    Fade,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Transition {
    pub kind: TransitionKind,
    pub duration_ms: u32,
}
impl Default for Transition {
    fn default() -> Self {
        Self { kind: TransitionKind::Fade, duration_ms: 600 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub(crate) struct Source {
    pub id: String,
    /// `camera`, `telemetry`, `overlay` or `graphic`.
    pub kind: String,
    /// What draws it: `camera`, `diagram`, `stats`, `plot`, `tclock`, `map`,
    /// `model`, `recorded_replay`, `lower_third`, `tclock_bug`, `stats_bar`,
    /// `title`, `slate`; an unknown view is shown by its label.
    #[serde(default)]
    pub view: String,
    pub label: String,
    /// `live`, `no_signal`, `stale` or `ready`.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub transparent: bool,
    #[serde(default)]
    pub stream_id: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub preview_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub(crate) struct Preset {
    pub id: String,
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub(crate) struct StudioView {
    pub revision: u64,
    #[serde(default)]
    pub program_url: String,
    #[serde(default)]
    pub program_scene_id: String,
    #[serde(default)]
    pub on_air: bool,
    #[serde(default)]
    pub transition: Transition,
    #[serde(default)]
    pub take_seq: u64,
    #[serde(default)]
    pub layouts: Vec<Layout>,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    #[serde(default)]
    pub sources: Vec<Source>,
    #[serde(default)]
    pub presets: Vec<Preset>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct LayoutsRequest {
    pub revision: u64,
    pub layouts: Vec<Layout>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct ScenesRequest {
    pub revision: u64,
    pub scenes: Vec<Scene>,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct TakeRequest {
    pub scene_id: String,
    pub transition: Transition,
}

pub(crate) fn snap(value: f64) -> f64 {
    (value / GRID).round() * GRID
}

/// Move a slot by a delta (percent), snapped, kept inside the canvas.
pub(crate) fn moved(slot: &Slot, dx: f64, dy: f64) -> Slot {
    Slot {
        x: snap(slot.x + dx).clamp(0.0, 100.0 - slot.width),
        y: snap(slot.y + dy).clamp(0.0, 100.0 - slot.height),
        ..slot.clone()
    }
}

/// Resize a slot from its bottom-right corner, snapped, at least `MIN_SLOT`.
pub(crate) fn resized(slot: &Slot, dw: f64, dh: f64) -> Slot {
    Slot {
        width: snap(slot.width + dw).clamp(MIN_SLOT, 100.0 - slot.x),
        height: snap(slot.height + dh).clamp(MIN_SLOT, 100.0 - slot.y),
        ..slot.clone()
    }
}

/// Typed-in geometry, clamped the same way.
pub(crate) fn with_geometry(slot: &Slot, x: f64, y: f64, width: f64, height: f64) -> Slot {
    let finite = |v: f64, d: f64| if v.is_finite() { v } else { d };
    let width = finite(width, slot.width).clamp(MIN_SLOT, 100.0);
    let height = finite(height, slot.height).clamp(MIN_SLOT, 100.0);
    Slot {
        x: finite(x, slot.x).clamp(0.0, 100.0 - width),
        y: finite(y, slot.y).clamp(0.0, 100.0 - height),
        width,
        height,
        ..slot.clone()
    }
}

pub(crate) fn inside(slot: &Slot) -> bool {
    [slot.x, slot.y, slot.width, slot.height].iter().all(|v| v.is_finite())
        && slot.x >= 0.0
        && slot.y >= 0.0
        && slot.width >= MIN_SLOT
        && slot.height >= MIN_SLOT
        && slot.x + slot.width <= 100.001
        && slot.y + slot.height <= 100.001
}

fn unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let base: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if base.is_empty() { "item".to_owned() } else { base.chars().take(40).collect() };
    if !taken(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|id| !taken(id)).unwrap_or(base)
}

pub(crate) fn new_slot(layout: &Layout) -> Slot {
    let id = unique(&format!("slot-{}", layout.slots.len() + 1), |id| layout.slots.iter().any(|s| s.id == id));
    let z = layout.slots.iter().map(|s| s.z).max().map_or(0, |z| (z + 1).min(9));
    Slot { id, x: 25.0, y: 25.0, width: 50.0, height: 50.0, z }
}

pub(crate) fn new_layout(layouts: &[Layout], name: &str, slots: Vec<Slot>) -> Layout {
    Layout { id: unique(name, |id| layouts.iter().any(|l| l.id == id)), name: name.to_owned(), slots }
}

pub(crate) fn new_scene(scenes: &[Scene], name: &str, layout_id: &str) -> Scene {
    Scene {
        id: unique(name, |id| scenes.iter().any(|s| s.id == id)),
        name: name.to_owned(),
        layout_id: layout_id.to_owned(),
        sources: BTreeMap::new(),
        program_bar: true,
    }
}

/// Put a source in a slot: a source fills one slot of a scene, and a scene
/// shows one live diagram, so either moves out of its old slot.
pub(crate) fn assign(scene: &mut Scene, slot: &str, source: &str) {
    let diagram = source.starts_with("diagram:");
    scene.sources.retain(|s, v| s != slot && v != source && !(diagram && v.starts_with("diagram:")));
    scene.sources.insert(slot.to_owned(), source.to_owned());
}

/// A changed layout keeps only the sources whose slot it still has.
pub(crate) fn fit_to_layout(scene: &mut Scene, layout: &Layout) {
    scene.sources.retain(|slot, _| layout.slots.iter().any(|s| &s.id == slot));
}

/// Which source ids are on a scene, with automatic cameras also counted as
/// the camera they show now (so the camera's own tile lights too).
pub(crate) fn scene_sources(view: &StudioView, scene_id: Option<&str>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(scene) = scene_id.and_then(|id| view.scenes.iter().find(|s| s.id == id)) else {
        return out;
    };
    for id in scene.sources.values() {
        out.insert(id.clone());
        if let Some(stream) = view.sources.iter().find(|s| &s.id == id).and_then(|s| s.stream_id.as_ref()) {
            out.insert(format!("camera:{stream}"));
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tally {
    Program,
    Preview,
    Both,
    Off,
}

pub(crate) fn tally(program: &BTreeSet<String>, preview: &BTreeSet<String>, id: &str) -> Tally {
    match (program.contains(id), preview.contains(id)) {
        (true, true) => Tally::Both,
        (true, false) => Tally::Program,
        (false, true) => Tally::Preview,
        _ => Tally::Off,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hotkey {
    /// Load the Nth scene (0-based) into Preview.
    Preview(usize),
    Cut,
    Transition,
}

/// 1–9 load a scene into Preview; Enter or C cuts; Space or T transitions.
pub(crate) fn hotkey(key: &str) -> Option<Hotkey> {
    match key {
        "Enter" | "c" | "C" => Some(Hotkey::Cut),
        " " | "t" | "T" => Some(Hotkey::Transition),
        k if k.len() == 1 && ('1'..='9').contains(&k.chars().next()?) => {
            Some(Hotkey::Preview(k.parse::<usize>().ok()? - 1))
        }
        _ => None,
    }
}

fn with_query(url: &str, extra: &str) -> String {
    if url.is_empty() {
        return String::new();
    }
    format!("{url}{}{extra}", if url.contains('?') { '&' } else { '?' })
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The programme page drawing a scene (the Preview monitor).
pub(crate) fn preview_url(program_url: &str, scene_id: &str) -> String {
    with_query(program_url, &format!("scene={}", encode(scene_id)))
}
/// The programme as viewers see it, muted and without controls (unless `listen`).
pub(crate) fn program_monitor_url(program_url: &str, listen: bool) -> String {
    with_query(program_url, if listen { "clean=1" } else { "monitor=1" })
}
/// One source full frame, e.g. an OBS browser source; transparent for an overlay.
pub(crate) fn source_url(program_url: &str, source: &Source) -> String {
    with_query(
        program_url,
        &format!("source={}{}", encode(&source.id), if source.transparent { "&transparent=1" } else { "" }),
    )
}

pub(crate) fn state_label(state: &str) -> &'static str {
    match state {
        "live" => "Live",
        "ready" => "Ready",
        "stale" => "Stale",
        "no_signal" => "No signal",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> serde_json::Value {
        crate::backend_contract::fixture(&format!("studio/{name}"))
    }

    #[test]
    fn station_studio_view_decodes_and_round_trips_its_editable_parts() {
        let wire = fixture("view");
        let view: StudioView = serde_json::from_value(wire.clone()).unwrap();
        assert!(!view.scenes.is_empty() && !view.layouts.is_empty() && !view.sources.is_empty());
        assert!(view.presets.iter().any(|p| p.id == "lower_third"));
        assert!(view.sources.iter().any(|s| s.view == "diagram"), "diagrams are sources");
        assert!(view.sources.iter().any(|s| s.transparent), "an overlay source");
        // What the editor saves is byte-for-byte what the station sent.
        for (key, value) in [("layouts", serde_json::to_value(&view.layouts).unwrap()), ("scenes", serde_json::to_value(&view.scenes).unwrap())] {
            assert_eq!(value, wire[key], "{key}");
        }
        for scene in &view.scenes {
            let layout = view.layouts.iter().find(|l| l.id == scene.layout_id).unwrap();
            assert!(scene.sources.keys().all(|slot| layout.slots.iter().any(|s| &s.id == slot)));
        }
        let taken: StudioView = serde_json::from_value(fixture("take")).unwrap();
        assert!(taken.on_air);
        assert_eq!(taken.take_seq, 1);
        let request: serde_json::Value = serde_json::to_value(TakeRequest { scene_id: taken.program_scene_id.clone(), transition: taken.transition.clone() }).unwrap();
        assert_eq!(request, fixture("take.request"), "the take this UI sends is the one the station accepted");
        // The programme page's resolved scene: every view is one the page draws
        // (station_ui/scripts/program-studio.js and program.html).
        let scene = fixture("program-scene");
        assert_eq!(scene["scene_id"], serde_json::json!(taken.program_scene_id));
        const DRAWN: &[&str] = &["camera", "diagram", "stats", "plot", "tclock", "map", "model", "recorded_replay", "lower_third", "tclock_bug", "stats_bar", "title", "slate"];
        for slot in scene["slots"].as_array().unwrap() {
            let geometry: Slot = serde_json::from_value(serde_json::json!({"id":slot["id"],"x":slot["x"],"y":slot["y"],"width":slot["width"],"height":slot["height"],"z":slot["z"]})).unwrap();
            assert!(inside(&geometry));
            if let Some(view) = slot["source"]["view"].as_str() {
                assert!(DRAWN.contains(&view), "{view}");
            }
        }
        for source in &view.sources {
            assert!(DRAWN.contains(&source.view.as_str()), "{}", source.view);
        }
    }

    #[test]
    fn requests_serialize_as_the_station_expects() {
        let layout = Layout { id: "l".into(), name: "L".into(), slots: vec![Slot { id: "a".into(), x: 0.0, y: 0.0, width: 100.0, height: 100.0, z: 0 }] };
        assert_eq!(
            serde_json::to_value(LayoutsRequest { revision: 3, layouts: vec![layout] }).unwrap(),
            serde_json::json!({"revision":3,"layouts":[{"id":"l","name":"L","slots":[{"id":"a","x":0.0,"y":0.0,"width":100.0,"height":100.0,"z":0}]}]})
        );
        let scene = new_scene(&[], "Pad cam", "l");
        assert_eq!(
            serde_json::to_value(ScenesRequest { revision: 4, scenes: vec![scene] }).unwrap(),
            serde_json::json!({"revision":4,"scenes":[{"id":"pad-cam","name":"Pad cam","layout_id":"l","sources":{},"program_bar":true}]})
        );
        assert_eq!(serde_json::to_value(Transition { kind: TransitionKind::Cut, duration_ms: 0 }).unwrap(), serde_json::json!({"kind":"cut","duration_ms":0}));
    }

    #[test]
    fn layout_editing_snaps_and_stays_on_the_canvas() {
        let slot = Slot { id: "a".into(), x: 10.0, y: 10.0, width: 40.0, height: 30.0, z: 0 };
        assert_eq!((moved(&slot, 3.1, -1.0).x, moved(&slot, 3.1, -1.0).y), (12.5, 10.0));
        let far = moved(&slot, 500.0, 500.0);
        assert_eq!((far.x, far.y), (60.0, 70.0));
        assert!(inside(&far));
        assert_eq!(moved(&slot, -500.0, -500.0).x, 0.0);
        let big = resized(&slot, 500.0, 500.0);
        assert_eq!((big.width, big.height), (90.0, 90.0));
        let small = resized(&slot, -500.0, -500.0);
        assert_eq!((small.width, small.height), (MIN_SLOT, MIN_SLOT));
        let typed = with_geometry(&slot, 95.0, f64::NAN, 30.0, 1.0);
        assert!(inside(&typed));
        assert_eq!((typed.x, typed.y, typed.width, typed.height), (70.0, 10.0, 30.0, MIN_SLOT));
        for dx in [-37.0, -2.4, 0.0, 1.3, 41.0] {
            for dy in [-90.0, 7.7, 66.0] {
                assert!(inside(&moved(&slot, dx, dy)) && inside(&resized(&slot, dx, dy)));
            }
        }
    }

    #[test]
    fn new_items_get_unique_station_ids() {
        let mut layout = new_layout(&[], "Two-up!", vec![]);
        assert_eq!(layout.id, "two-up");
        let existing = vec![layout.clone()];
        assert_eq!(new_layout(&existing, "Two-up!", vec![]).id, "two-up-2");
        layout.slots.push(new_slot(&layout));
        layout.slots.push(new_slot(&layout));
        assert_eq!(layout.slots[0].id, "slot-1");
        assert_ne!(layout.slots[0].id, layout.slots[1].id);
        assert_eq!(layout.slots[1].z, 1, "a new slot comes to the front");
        assert_eq!(new_scene(&[], "  ", "l").id, "item");
    }

    #[test]
    fn a_source_fills_one_slot_and_a_scene_one_diagram() {
        let mut scene = new_scene(&[], "S", "grid");
        assign(&mut scene, "a", "camera:pad");
        assign(&mut scene, "b", "camera:pad");
        assert_eq!(scene.sources, BTreeMap::from([("b".into(), "camera:pad".into())]));
        assign(&mut scene, "c", "diagram:flight");
        assign(&mut scene, "d", "diagram:network");
        assert_eq!(scene.sources.get("c"), None);
        assert_eq!(scene.sources.get("d").map(String::as_str), Some("diagram:network"));
        assign(&mut scene, "b", "telemetry:stats");
        assert_eq!(scene.sources.get("b").map(String::as_str), Some("telemetry:stats"));
        let layout = Layout { id: "grid".into(), name: "G".into(), slots: vec![Slot { id: "b".into(), x: 0.0, y: 0.0, width: 50.0, height: 50.0, z: 0 }] };
        fit_to_layout(&mut scene, &layout);
        assert_eq!(scene.sources.keys().collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn tally_lights_scene_sources_and_the_camera_an_auto_source_shows() {
        let mut view = StudioView::default();
        view.sources.push(Source { id: "camera-auto:1".into(), kind: "camera".into(), stream_id: Some("pad".into()), ..Default::default() });
        let mut program = new_scene(&[], "P", "full");
        program.sources.insert("main".into(), "camera-auto:1".into());
        let mut preview = new_scene(&[program.clone()], "V", "full");
        preview.sources.insert("main".into(), "telemetry:stats".into());
        view.scenes = vec![program, preview];
        let on_program = scene_sources(&view, Some("p"));
        let on_preview = scene_sources(&view, Some("v"));
        assert_eq!(tally(&on_program, &on_preview, "camera:pad"), Tally::Program);
        assert_eq!(tally(&on_program, &on_preview, "camera-auto:1"), Tally::Program);
        assert_eq!(tally(&on_program, &on_preview, "telemetry:stats"), Tally::Preview);
        assert_eq!(tally(&on_program, &on_program, "camera:pad"), Tally::Both);
        assert_eq!(tally(&on_program, &on_preview, "graphic:slate"), Tally::Off);
        assert!(scene_sources(&view, None).is_empty());
    }

    #[test]
    fn hotkeys_and_monitor_urls() {
        assert_eq!(hotkey("1"), Some(Hotkey::Preview(0)));
        assert_eq!(hotkey("9"), Some(Hotkey::Preview(8)));
        assert_eq!(hotkey("0"), None);
        assert_eq!(hotkey("Enter"), Some(Hotkey::Cut));
        assert_eq!(hotkey("c"), Some(Hotkey::Cut));
        assert_eq!(hotkey(" "), Some(Hotkey::Transition));
        assert_eq!(hotkey("T"), Some(Hotkey::Transition));
        assert_eq!(hotkey("x"), None);
        let program = "/api/media-assets/program?ticket=T";
        assert_eq!(preview_url(program, "pad camera"), "/api/media-assets/program?ticket=T&scene=pad%20camera");
        assert_eq!(program_monitor_url(program, false), "/api/media-assets/program?ticket=T&monitor=1");
        assert_eq!(program_monitor_url(program, true), "/api/media-assets/program?ticket=T&clean=1");
        let overlay = Source { id: "overlay:lower-third".into(), transparent: true, ..Default::default() };
        assert_eq!(source_url(program, &overlay), "/api/media-assets/program?ticket=T&source=overlay%3Alower-third&transparent=1");
        assert_eq!(preview_url("", "x"), "");
        assert_eq!(state_label("no_signal"), "No signal");
    }
}
