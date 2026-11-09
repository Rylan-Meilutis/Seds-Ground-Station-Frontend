use super::{latest_telemetry_row, layout::ThemeConfig, persist};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
#[path = "dashboard_card_values.rs"]
mod card_values;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Card {
    /// Stable identity used by per-phase profiles. Assigned on load.
    #[serde(default)]
    id: String,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    stream_id: String,
    #[serde(default = "default_width")]
    width: String,
    #[serde(default = "default_precision")]
    precision: usize,
    label: String,
    section: String,
    data_type: String,
    sender: String,
    index: usize,
    unit: String,
    display: String,
    min: f32,
    max: f32,
    visible: bool,
}
fn default_width() -> String { "normal".into() }
fn default_precision() -> usize { 2 }
fn defaults() -> Vec<Card> {
    let mut cards = Vec::new();
    for (section, label, dt, sender, index, unit, max, display) in [
        (
            "Fill System",
            "50 kg loadcell",
            "LOADCELL_50_WEIGHT_KG",
            "DAQ",
            0,
            "kg",
            50.0,
            "trend",
        ),
        (
            "Fill System",
            "1000 kg loadcell",
            "LOADCELL_WEIGHT_KG",
            "DAQ",
            0,
            "kg",
            1000.0,
            "trend",
        ),
        (
            "Fill System",
            "Chamber pressure",
            "COMBUSTION_CHAMBER_PRESSURE",
            "DAQ",
            0,
            "psig",
            1000.0,
            "gauge",
        ),
        (
            "Fill System",
            "Tank pressure",
            "FUEL_TANK_PRESSURE",
            "VB",
            0,
            "psi",
            1000.0,
            "gauge",
        ),
        (
            "Fill System",
            "Fill level",
            "LOADCELL_FILL_PERCENT",
            "DAQ",
            0,
            "%",
            100.0,
            "bar",
        ),
        (
            "Fill System",
            "Fill box battery",
            "BATTERY_VOLTAGE",
            "GB",
            0,
            "V",
            16.8,
            "number",
        ),
        (
            "Fill System",
            "Valve board battery",
            "BATTERY_VOLTAGE",
            "VB",
            0,
            "V",
            16.8,
            "number",
        ),
        (
            "Fill System",
            "Pilot",
            "VALVE_STATE",
            "",
            0,
            "",
            1.0,
            "state",
        ),
        (
            "Fill System",
            "Vent",
            "VALVE_STATE",
            "",
            1,
            "",
            1.0,
            "state",
        ),
        (
            "Fill System",
            "Dump",
            "VALVE_STATE",
            "",
            2,
            "",
            1.0,
            "state",
        ),
        (
            "Fill System",
            "Nitrogen",
            "VALVE_STATE",
            "",
            4,
            "",
            1.0,
            "state",
        ),
        (
            "Fill System",
            "Nitrous",
            "VALVE_STATE",
            "",
            5,
            "",
            1.0,
            "state",
        ),
        (
            "Avionics",
            "AV bay battery",
            "BATTERY_VOLTAGE",
            "PB",
            0,
            "V",
            16.8,
            "gauge",
        ),
        (
            "Avionics",
            "Battery current",
            "BATTERY_CURRENT",
            "PB",
            0,
            "A",
            20.0,
            "number",
        ),
        (
            "Avionics",
            "GPS satellites",
            "GPS_SATELLITE_NUMBER",
            "RF",
            0,
            "",
            30.0,
            "number",
        ),
        (
            "Avionics",
            "Barometer",
            "BAROMETER_DATA",
            "FC",
            0,
            "",
            1200.0,
            "trend",
        ),
    ] {
        cards.push(Card {
            id: default_card_id(section, label, dt, index),
            pinned: false,
            stream_id: String::new(),
            width: default_width(),
            precision: 2,
            section: section.into(),
            label: label.into(),
            data_type: dt.into(),
            sender: sender.into(),
            index,
            unit: unit.into(),
            display: display.into(),
            min: 0.0,
            max,
            visible: true,
        });
    }
    cards.extend(flight_cards());
    cards
}

fn default_card_id(section: &str, label: &str, data_type: &str, index: usize) -> String {
    let slug: String = format!("{section}-{label}-{data_type}-{index}")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    slug.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-")
}

/// Flight cards: hidden in the base dashboard, shown on top by the default
/// In flight and Landed phase profiles.
fn flight_cards() -> Vec<Card> {
    let card = |id: &str, label: &str, data_type: &str, index: usize, unit: &str, min: f32, max: f32, display: &str| Card {
        id: id.into(),
        pinned: false,
        stream_id: String::new(),
        width: default_width(),
        precision: 1,
        label: label.into(),
        section: "Flight".into(),
        data_type: data_type.into(),
        sender: if matches!(display, "map" | "maxima") || data_type == "VEHICLE_SPEED" { String::new() } else { "FC".into() },
        index,
        unit: unit.into(),
        display: display.into(),
        min,
        max,
        visible: false,
    };
    vec![
        card("flight-altitude", "Altitude", "BAROMETER_DATA", 2, "m", 0.0, 5000.0, "trend"),
        card("flight-speed", "Vertical speed", "VEHICLE_SPEED", 0, "m/s", -500.0, 500.0, "trend"),
        card("flight-accel", "Acceleration (Z)", "ACCEL_DATA", 2, "m/s²", -200.0, 200.0, "trend"),
        card("flight-gyro-x", "Gyro roll", "GYRO_DATA", 0, "deg/s", -1000.0, 1000.0, "trend"),
        card("flight-gyro-y", "Gyro pitch", "GYRO_DATA", 1, "deg/s", -1000.0, 1000.0, "trend"),
        card("flight-gyro-z", "Gyro yaw", "GYRO_DATA", 2, "deg/s", -1000.0, 1000.0, "trend"),
        card("flight-max-altitude", "Max altitude", "FLIGHT_MAXIMA", 0, "m", 0.0, 1.0, "maxima"),
        card("flight-max-speed", "Max speed", "FLIGHT_MAXIMA", 1, "m/s", 0.0, 1.0, "maxima"),
        card("flight-max-accel", "Max acceleration", "FLIGHT_MAXIMA", 2, "m/s²", 0.0, 1.0, "maxima"),
        {
            let mut map = card("flight-mini-map", "Rocket position", "MINI_MAP", 0, "", 0.0, 1.0, "map");
            map.width = "wide".into();
            map
        },
    ]
}

/// Telemetry readouts can be pinned to the header; maps, maxima and cameras cannot.
fn pinnable(card: &Card) -> bool {
    !matches!(card.display.as_str(), "camera" | "map" | "maxima")
}

/// Give every card a unique stable id (older saved cards have none).
fn ensure_ids(cards: &mut [Card]) {
    let mut seen = std::collections::HashSet::new();
    for card in cards.iter_mut() {
        if card.id.is_empty() || seen.contains(&card.id) {
            let base = default_card_id(&card.section, &card.label, &card.data_type, card.index);
            let mut id = base.clone();
            let mut n = 2;
            while seen.contains(&id) {
                id = format!("{base}-{n}");
                n += 1;
            }
            card.id = id;
        }
        seen.insert(card.id.clone());
    }
}

fn new_card_id(cards: &[Card], base: &str) -> String {
    let mut n = cards.len() + 1;
    loop {
        let id = format!("{base}-{n}");
        if !cards.iter().any(|card| card.id == id) {
            return id;
        }
        n += 1;
    }
}

/// Saved dashboards get the hidden flight cards appended once (version 4),
/// without touching their order or visibility.
fn migrate_flight_cards(cards: &mut Vec<Card>) {
    for card in flight_cards() {
        if cards.len() >= 32 {
            break;
        }
        if !cards.iter().any(|existing| existing.id == card.id) {
            cards.push(card);
        }
    }
}

fn valid(cards: &[Card]) -> bool {
    cards.iter().filter(|c| c.pinned).count() <= 8
        && !cards.is_empty()
        && cards.len() <= 32
        && cards.iter().all(|c| {
            !c.section.trim().is_empty() && c.section.len() < 64 && c.precision <= 6
                && ["number", "bar", "gauge", "trend", "state", "camera", "map", "maxima"].contains(&c.display.as_str())
                && ["normal", "wide", "full"].contains(&c.width.as_str())
                && c.stream_id.len() <= 64
                && (pinnable(c) || !c.pinned)
                && c.id.len() <= 128
                && c.min.is_finite()
                && c.max.is_finite()
                && c.max > c.min
                && c.index < 128
                && !c.data_type.trim().is_empty()
                && c.data_type.len() < 128
                && c.label.len() < 128
                && c.sender.len() < 128
                && c.unit.len() < 32
        })
}
pub(super) fn storage_key() -> String {
    let user = crate::auth::current_session()
        .and_then(|s| s.session.username)
        .unwrap_or_else(|| "anonymous".into());
    format!("{}_cards_v1_{}", super::dashboard_customization_key(), user)
}
fn save(cards: &[Card]) {
    if valid(cards)
        && let Ok(json) = serde_json::to_string(cards) {
            persist::set_string(&storage_key(), &json);
            persist::set_string(&format!("{}_defaults_version", storage_key()), "4");
        }
}

fn migrate_default_loadcell_trend(cards: &mut [Card]) {
    let mut legacy = defaults()[0].clone();
    legacy.display = "number".into();
    for card in cards {
        let mut candidate = card.clone();
        candidate.pinned = false;
        if candidate == legacy {
            card.display = "trend".into();
        }
    }
}

fn migrate_default_pressure_cards(cards: &mut Vec<Card>) {
    let default_cards = defaults();
    // A custom dashboard gets no new cards. Recognize the original fill dashboard
    // from its existing named loadcells and tank-pressure source.
    let legacy_default_layout = ["LOADCELL_50_WEIGHT_KG", "LOADCELL_WEIGHT_KG", "FUEL_TANK_PRESSURE"]
        .iter().all(|kind| default_cards.iter().find(|card| card.data_type == *kind).is_some_and(|default| {
            cards.iter().any(|card| card.data_type == default.data_type
                && card.label == default.label && card.sender == default.sender
                && card.section == default.section)
        }));
    if legacy_default_layout && cards.len() < 32 && !cards.iter().any(|card| card.data_type == "COMBUSTION_CHAMBER_PRESSURE") {
        let chamber = default_cards.iter().find(|card| card.data_type == "COMBUSTION_CHAMBER_PRESSURE").unwrap().clone();
        let index = cards.iter().position(|card| card.data_type == "FUEL_TANK_PRESSURE").unwrap_or(cards.len());
        cards.insert(index, chamber);
    }

}

pub(super) fn load_cards() -> Vec<Card> {
    let mut cards = persist::get_string(&storage_key())
        .and_then(|raw| serde_json::from_str::<Vec<Card>>(&raw).ok())
        .filter(|cards| valid(cards))
        .unwrap_or_else(defaults);
    let version = persist::get_string(&format!("{}_defaults_version", storage_key()));
    ensure_ids(&mut cards);
    if version.as_deref() != Some("4") {
        if version.as_deref() != Some("3") {
            // Version2 already migrated the loadcell. Preserve later explicit choices.
            if version.as_deref() != Some("2") {
                migrate_default_loadcell_trend(&mut cards);
            }
            migrate_default_pressure_cards(&mut cards);
        }
        migrate_flight_cards(&mut cards);
        save(&cards);
    }
    cards
}

fn profiles_key() -> String {
    format!("{}_phase_profiles_v1", storage_key())
}

/// Per-phase card profiles, saved with the dashboard card layout.
pub(super) fn load_profiles() -> super::mission_phase::PhaseProfiles {
    persist::get_string(&profiles_key())
        .and_then(|raw| serde_json::from_str::<super::mission_phase::PhaseProfiles>(&raw).ok())
        .filter(|profiles| profiles.valid())
        .unwrap_or_default()
}

fn save_profiles(profiles: &super::mission_phase::PhaseProfiles) {
    if profiles.valid()
        && let Ok(json) = serde_json::to_string(profiles)
    {
        persist::set_string(&profiles_key(), &json);
    }
}

/// Base indices with visibility for the dashboard shown in `phase`.
fn phase_view(cards: &[Card], profiles: &super::mission_phase::PhaseProfiles, phase: Option<super::mission_phase::Phase>) -> Vec<(usize, bool)> {
    let ids: Vec<&str> = cards.iter().map(|card| card.id.as_str()).collect();
    let visible: Vec<bool> = cards.iter().map(|card| card.visible).collect();
    super::mission_phase::profile_order(&ids, &visible, phase.and_then(|phase| profiles.profile(phase)))
}

fn data_type_choices(
    layout: &super::layout::DataTabLayout,
    cards: &[Card],
    observed: impl IntoIterator<Item = String>,
) -> Vec<(String, String)> {
    let mut types = std::collections::BTreeMap::<String, String>::new();
    for id in defaults().into_iter().chain(cards.iter().cloned()).map(|card| card.data_type).chain(observed) {
        if !id.is_empty() { types.entry(id.clone()).or_insert(id); }
    }
    let mut insert = |id: &str, label: &str| {
        if !id.is_empty() {
            let entry = types.entry(id.to_string()).or_insert_with(|| label.to_string());
            if entry == id { *entry = label.to_string(); }
        }
    };
    for tab in &layout.tabs {
        if !tab.channels.is_empty() { insert(&tab.id, &tab.label); }
        for group in tab.chart_groups.iter().flatten() {
            if let Some(id) = &group.data_type { insert(id, group.title.as_deref().unwrap_or(id)); }
            for series in group.chart_series.iter().flatten() { insert(&series.data_type, series.label.as_deref().unwrap_or(&series.data_type)); }
        }
        for subtab in tab.subtabs.iter().flatten() {
            if let Some(id) = subtab.data_type.as_deref().or_else(|| (!tab.channels.is_empty()).then_some(tab.id.as_str())) {
                insert(id, &subtab.label);
            }
            for group in subtab.chart_groups.iter().flatten() {
                if let Some(id) = &group.data_type { insert(id, group.title.as_deref().unwrap_or(id)); }
            for series in group.chart_series.iter().flatten() { insert(&series.data_type, series.label.as_deref().unwrap_or(&series.data_type)); }
            }
            for item in subtab.summary_items.iter().flatten() { insert(&item.data_type, &item.label); }
        }
    }
    types.into_iter().map(|(id, label)| {
        let text = if id == label { id.clone() } else { format!("{label} ({id})") };
        (id, text)
    }).collect()
}

fn choice_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    values.into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().collect()
}

fn card_ranges(card: &Card) -> Vec<(f32, f32)> {
    let mut ranges = vec![(0., 1.), (0., 5.), (0., 10.), (0., 20.), (0., 50.),
        (0., 100.), (0., 500.), (0., 1000.), (0., 5000.), (0., 10000.),
        (-1., 1.), (-10., 10.), (-50., 50.), (-100., 100.), (-1000., 1000.)];
    for range in defaults().iter().map(|card| (card.min, card.max)).chain([(card.min, card.max)]) {
        if !ranges.contains(&range) { ranges.push(range); }
    }
    ranges
}

fn card_channels(layout: &super::layout::DataTabLayout, data_type: &str, saved_index: usize,
    observed_count: usize) -> Vec<(usize, String)> {
    use std::collections::BTreeMap;
    fn labels(map: &mut BTreeMap<usize, String>, names: &[String]) {
        for (index, name) in names.iter().take(128).enumerate() { map.entry(index).or_insert_with(|| name.clone()); }
    }
    fn group(map: &mut BTreeMap<usize, String>, group: &super::layout::DataChartGroup, inherited: &str, data_type: &str) {
        if group.data_type.as_deref().unwrap_or(inherited) == data_type {
            for (position, index) in group.channels.iter().copied().filter(|i| *i < 128).enumerate() {
                let label = group.labels.as_ref().and_then(|names| names.get(position)).cloned()
                    .unwrap_or_else(|| format!("Channel {index}"));
                map.entry(index).or_insert(label);
            }
        }
        for series in group.chart_series.iter().flatten().filter(|s| s.data_type == data_type && s.index < 128) {
            map.entry(series.index).or_insert_with(|| series.label.clone().unwrap_or_else(|| format!("Channel {}", series.index)));
        }
    }
    let mut channels = BTreeMap::new();
    for tab in &layout.tabs {
        if tab.id == data_type { labels(&mut channels, &tab.channels); }
        for chart in tab.chart_groups.iter().flatten() { group(&mut channels, chart, &tab.id, data_type); }
        for subtab in tab.subtabs.iter().flatten() {
            let source = subtab.data_type.as_deref().unwrap_or(&tab.id);
            if source == data_type { labels(&mut channels, subtab.channels.as_deref().unwrap_or(&tab.channels)); }
            for chart in subtab.chart_groups.iter().flatten() { group(&mut channels, chart, source, data_type); }
            for item in subtab.summary_items.iter().flatten().filter(|item| item.data_type == data_type && item.index < 128) {
                channels.entry(item.index).or_insert_with(|| item.label.clone());
            }
        }
    }
    for source in defaults().into_iter().filter(|source| source.data_type == data_type) {
        channels.entry(source.index).or_insert(source.label);
    }
    for index in 0..observed_count.min(128) { channels.entry(index).or_insert_with(|| format!("Channel {index}")); }
    channels.entry(saved_index).or_insert_with(|| format!("Saved channel {saved_index}"));
    channels.into_iter().map(|(index, label)| (index, format!("{label} [{index}]"))).collect()
}

fn card_with_source(mut card: Card, data_type: String) -> Card {
    card.data_type = data_type;
    card.index = 0;
    if let Some(source) = defaults().into_iter().find(|source| source.data_type == card.data_type) {
        card.sender = source.sender; card.unit = source.unit; card.min = source.min; card.max = source.max;
    } else {
        card.sender.clear(); card.unit.clear(); card.min = 0.; card.max = 100.;
    }
    card
}

fn display_card(mut card: Card, test_fire: bool) -> Card {
    if test_fire && card.data_type == "LOADCELL_WEIGHT_KG" {
        card.label = "Thrust".into(); card.unit="N".into(); card.max *= 9.80665; card.min *= 9.80665;
    }
    card
}

#[component]
pub(super) fn CustomDashboard(theme: ThemeConfig, data_layout: super::layout::DataTabLayout, #[props(default)] test_fire_mode: bool) -> Element {
    // Launch metadata is installed by the parent's effect into the history
    // store. Refresh availability even when no telemetry or editor is active.
    let _ = *super::HEADER_CLOCK_TICK.read();
    let mut cards = use_context::<Signal<Vec<Card>>>();
    let extrema_policies = use_memo(use_reactive((&data_layout,), move |(layout,)| {
        cards.read().iter().map(|card| card_minmax_policy(&layout, card)).collect::<Vec<_>>()
    }));
    let mut editing = use_signal(|| false);
    let mut aggregate = use_signal(|| false);
    let mut profiles = use_signal(load_profiles);
    let mut preview_phase = use_signal(|| None::<super::mission_phase::Phase>);
    let mut edit_phase = use_signal(|| super::mission_phase::Phase::InFlight);
    let mut cameras = use_signal(super::live_stream_tab::LiveStreamConfig::default);
    let mut camera_error = use_signal(|| "Loading cameras…".to_string());
    use_future(move || async move {
        loop {
            if *editing.peek() || cards.peek().iter().any(|card| card.display == "camera" && card.visible) {
                match super::http_get_json::<super::live_stream_tab::LiveStreamConfig>("/api/live_streams").await {
                    Ok(next) => { if *cameras.peek() != next { cameras.set(next); } camera_error.set(String::new()); }
                    Err(error) => { cameras.set(Default::default()); camera_error.set(format!("Camera service unavailable: {error}")); }
                }
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(2000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
    let snapshot = cards.read().clone();
    let type_choices = if *editing.read() {
        // Refresh discovery at the editor heartbeat, not once per telemetry packet.
        let observed = super::LATEST_TELEMETRY_BY_TYPE.lock().map(|latest| {
            latest.keys().map(|id| super::resolve_telemetry_text(*id).to_string()).collect::<Vec<_>>()
        }).unwrap_or_default();
        data_type_choices(&data_layout, &snapshot, observed)
    } else { Vec::new() };
    let (section_choices, sender_choices, unit_choices) = if *editing.read() {
    let section_choices = choice_strings(defaults().into_iter().chain(snapshot.iter().cloned()).map(|card| card.section).chain(["Cameras".into()]));
    let observed_senders = super::LATEST_TELEMETRY_BY_TYPE.lock().map(|latest| latest.values().map(|row| row.sender_id).collect::<Vec<_>>()).unwrap_or_default();
    let sender_choices = choice_strings(defaults().into_iter().chain(snapshot.iter().cloned()).map(|card| card.sender)
        .chain(observed_senders.into_iter().map(|id| super::resolve_telemetry_text(id).to_string()))
        .chain(["", "GS", "RRLS", "RF", "FC", "PB", "AB", "VB", "GB", "DAQ"].map(str::to_string)));
    let unit_choices = choice_strings(defaults().into_iter().chain(snapshot.iter().cloned()).map(|card| card.unit)
        .chain(["", "%", "kg", "N", "psi", "psig", "bar", "kPa", "MPa", "V", "A", "°C", "m", "m/s", "m/s²", "g", "deg", "rad", "s", "ms"].map(str::to_string)));
    (section_choices, sender_choices, unit_choices)
    } else { (Vec::new(), Vec::new(), Vec::new()) };
    let live_phase = super::mission_phase::MISSION_PHASE.read().as_ref().map(|view| view.phase);
    let profiles_snapshot = profiles.read().clone();
    let shown_phase = (*preview_phase.read()).or(if profiles_snapshot.auto_switch { live_phase } else { None });
    let view: Vec<(usize, bool)> = if *editing.read() {
        (0..snapshot.len()).map(|index| (index, true)).collect()
    } else {
        phase_view(&snapshot, &profiles_snapshot, shown_phase)
    };
    let mut sections = Vec::new();
    for (index, visible) in &view { let card = &snapshot[*index]; if *visible && !sections.contains(&card.section) { sections.push(card.section.clone()); } }
    let phase_key = shown_phase.map(|phase| phase.key()).unwrap_or("base");
    let phase_caption = match (*preview_phase.read(), live_phase) {
        (Some(phase), _) => format!("Previewing the {} dashboard", phase.label()),
        (None, Some(phase)) if profiles_snapshot.auto_switch => format!("{} dashboard · switches with the state", phase.label()),
        (None, Some(phase)) => format!("Base dashboard · state {} (automatic switching off)", phase.label()),
        (None, None) => "Base dashboard · waiting for the station state".to_string(),
    };
    let edit_phase_value = *edit_phase.read();
    let edit_profile = profiles_snapshot.profile(edit_phase_value).cloned().unwrap_or_default();
    rsx! {section {class:"gs26-custom-dashboard","data-phase-profile":phase_key,style:"color:{theme.text_primary};--card-button-bg:{theme.button_background};--card-button-border:{theme.button_border};--card-button-text:{theme.button_text};",
        style { {r#"
            .gs26-custom-dashboard { container-type:inline-size; }
            .gs26-card-grid { display:grid;grid-template-columns:repeat(12,minmax(0,1fr));gap:12px; }
            .gs26-dashboard-card { grid-column:span 4; }
            .gs26-dashboard-card[data-width=wide] { grid-column:span 8; }
            .gs26-dashboard-card[data-width=full] { grid-column:1 / -1; }
            @container(max-width:800px) { .gs26-dashboard-card { grid-column:span 6; } .gs26-dashboard-card[data-width=wide] { grid-column:1 / -1; } }
            @container(max-width:480px) { .gs26-dashboard-card { grid-column:1 / -1; } }
            .gs26-custom-dashboard button, .gs26-custom-dashboard input, .gs26-custom-dashboard select {
                font:inherit; font-size:13px; color:var(--card-button-text); background:var(--card-button-bg);
                border:1px solid var(--card-button-border); border-radius:7px; padding:7px 10px; max-width:100%; box-sizing:border-box;
            }
            .gs26-custom-dashboard button { cursor:pointer; margin:4px 4px 0 0; }
            .gs26-custom-dashboard button:disabled { opacity:.5; cursor:default; }
            .gs26-custom-dashboard input:not([type=checkbox]), .gs26-custom-dashboard select { width:100%; }
            .gs26-custom-dashboard button:focus-visible, .gs26-custom-dashboard input:focus-visible, .gs26-custom-dashboard select:focus-visible { outline:2px solid #38bdf8; outline-offset:2px; }
        "#} }
        div {style:"display:flex;gap:10px;align-items:center;flex-wrap:wrap;",
            button {disabled:super::dashboard_launch_window().is_none(),onclick:move |_| {let next=!*aggregate.read();aggregate.set(next);},
                if *aggregate.read() {"Show live 60s"}else{"Show launch aggregate"}
            }
            small {if super::dashboard_launch_window().is_none() {"Launch aggregate unavailable · admitted action time unknown"}else if *aggregate.read() {
                if super::dashboard_launch_window().is_some_and(|(start,end)|end-start==30_000) {"30s after Launch admission · retained for this launch session"}
                else {"10min after Launch admission · retained for this launch session"}
            }else{"Live · last 60 seconds"}}
            button {onclick:move |_| {let next=!*editing.read();editing.set(next); }, if *editing.read(){"Done"}else{"Customize dashboard"}}
            if !*editing.read() {
                label {style:"display:flex;gap:6px;align-items:center;",
                    "Show "
                    select {aria_label:"Dashboard phase profile",style:"width:auto;",
                        onchange:move |e:Event<FormData>| preview_phase.set(super::mission_phase::Phase::from_key(&e.value())),
                        option {value:"auto",selected:preview_phase.read().is_none(),"Automatic"}
                        for phase in super::mission_phase::Phase::ALL {
                            option {value:phase.key(),selected:*preview_phase.read()==Some(phase),"{phase.label()}"}
                        }
                    }
                }
                small {class:"gs26-phase-profile-caption","{phase_caption}"}
            }
            if *editing.read() {
                button {onclick:move |_|{let next=defaults();save(&next);cards.set(next);},"Restore defaults"}
                button {disabled:snapshot.len()>=32,onclick:move |_|{let mut next=cards.read().clone();let mut card=defaults()[0].clone();card.id=new_card_id(&next,"card");card.label="New telemetry card".into();next.push(card);save(&next);cards.set(next);},"Add card"}
                button {disabled:snapshot.len()>=32,onclick:move |_|{let mut next=cards.read().clone();let mut card=defaults()[0].clone();card.id=new_card_id(&next,"camera");card.label="Live camera".into();card.section="Cameras".into();card.display="camera".into();card.width="wide".into();next.push(card);save(&next);cards.set(next);},"Add camera"}
            }
        }
        if *editing.read() {p {"Saved for this user and GroundStation on this device. Choose a source, display, visibility and order. Changes do not affect acquisition or safety controls."}}
        if *editing.read() {
            fieldset {class:"gs26-phase-profiles",style:"border:1px solid #475569;border-radius:8px;padding:10px;margin:10px 0;",
                legend {"Phase profiles"}
                label {style:"display:block;margin-bottom:6px;",
                    input {r#type:"checkbox",checked:profiles_snapshot.auto_switch,onchange:move |e:Event<FormData>|{let mut next=profiles.read().clone();next.auto_switch=e.checked();save_profiles(&next);profiles.set(next);}}
                    " Switch the dashboard automatically with the state"
                }
                label {"Profile for "
                    select {aria_label:"Profile phase",style:"width:auto;",onchange:move |e:Event<FormData>|{if let Some(phase)=super::mission_phase::Phase::from_key(&e.value()){edit_phase.set(phase);}},
                        for phase in super::mission_phase::Phase::ALL {option {value:phase.key(),selected:phase==edit_phase_value,"{phase.label()}"}}
                    }
                }
                small {style:"display:block;margin:6px 0;","Cards marked On top are shown first in this phase, in this order, even if hidden on the base dashboard. Other cards follow in the base order."}
                button {onclick:move |_|{let mut next=profiles.read().clone();let defaults=super::mission_phase::PhaseProfiles::default();match defaults.profile(edit_phase_value){Some(p)=>{*next.profile_mut(edit_phase_value)=p.clone();}None=>{next.profiles.remove(edit_phase_value.key());}}save_profiles(&next);profiles.set(next);},"Restore this phase's default"}
                for (index,card) in snapshot.iter().enumerate() {
                    {
                        let id=card.id.clone();
                        let top_pos=edit_profile.top.iter().position(|t|*t==id);
                        let mode=if top_pos.is_some(){"top"}else if edit_profile.hidden.contains(&id){"hidden"}else{"base"};
                        let top_len=edit_profile.top.len();
                        rsx!{div {key:"profile-{index}",style:"display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin-top:4px;",
                            span {style:"min-width:180px;","{card.section} · {card.label}"}
                            select {aria_label:"Phase placement for {card.label}",style:"width:auto;",
                                onchange:{let id=id.clone();move |e:Event<FormData>|{let mut next=profiles.read().clone();let p=next.profile_mut(edit_phase_value);p.top.retain(|t|*t!=id);p.hidden.retain(|h|*h!=id);match e.value().as_str(){"top"=>p.top.push(id.clone()),"hidden"=>p.hidden.push(id.clone()),_=>{}}save_profiles(&next);profiles.set(next);}},
                                option {value:"base",selected:mode=="base","Base"}
                                option {value:"top",selected:mode=="top","On top"}
                                option {value:"hidden",selected:mode=="hidden","Hidden"}
                            }
                            if let Some(pos)=top_pos {
                                button {disabled:pos==0,onclick:move |_|{let mut next=profiles.read().clone();next.profile_mut(edit_phase_value).top.swap(pos,pos-1);save_profiles(&next);profiles.set(next);},"Earlier"}
                                button {disabled:pos+1==top_len,onclick:move |_|{let mut next=profiles.read().clone();next.profile_mut(edit_phase_value).top.swap(pos,pos+1);save_profiles(&next);profiles.set(next);},"Later"}
                            }
                        }}
                    }
                }
            }
        }
        for section in sections {
            h3 {"{section}"}
            div {class:"gs26-card-grid",
                for (index,card,shown) in view.iter().map(|(index,shown)|(*index,&snapshot[*index],*shown)).filter(|(_,c,shown)|c.section==section && *shown) {
                    div {key:"card-{index}",class:"gs26-dashboard-card","data-card-id":card.id.clone(),"data-width":card.width.clone(),"data-display":card.display.clone(),style:"min-width:0;border:1px solid #475569;border-radius:8px;padding:12px;background:{theme.panel_background_alt};",
                        if *editing.read() {
                            CardEditor {card:card.clone(),data_layout:data_layout.clone(),section_choices:section_choices.clone(),sender_choices:sender_choices.clone(),unit_choices:unit_choices.clone(),type_choices:type_choices.clone(),cameras:cameras.read().streams.clone(),onchange:move |next:Card|{let mut all=cards.read().clone();all[index]=next;if valid(&all){save(&all);cards.set(all);}}}
                            button {disabled:index==0,onclick:move |_|{let mut all=cards.read().clone();all.swap(index,index-1);save(&all);cards.set(all);},"Move up"}
                            button {disabled:index+1==snapshot.len(),onclick:move |_|{let mut all=cards.read().clone();all.swap(index,index+1);save(&all);cards.set(all);},"Move down"}
                            button {disabled:snapshot.len()==1,onclick:move |_|{let mut all=cards.read().clone();all.remove(index);save(&all);cards.set(all);},"Remove"}
                        }
                        if shown && (card.visible || !*editing.read()) {
                            if card.display == "camera" { CameraCard {card:card.clone(),config:cameras.read().clone(),error:camera_error.read().clone()} }
                            else if card.display == "maxima" { MaximaCard {card:card.clone()} }
                            else if card.display == "map" { MiniMapCard {card:card.clone(),dom_id:format!("gs26-mini-map-{}",card.id)} }
                            else { TelemetryCard {card:display_card(card.clone(),test_fire_mode), aggregate:*aggregate.read(), thrust:test_fire_mode && card.data_type=="LOADCELL_WEIGHT_KG", show_min_max:extrema_policies.read().get(index).copied().unwrap_or(card.display=="trend")} }
                        }
                        if *editing.read() && pinnable(card) {
                        button { disabled: !card.pinned && snapshot.iter().filter(|c| c.pinned).count() >= 8,
                            onclick: move |_| { let mut all=cards.read().clone(); all[index].pinned = !all[index].pinned; save(&all); cards.set(all); },
                            if card.pinned { "Unpin from top bar" } else { "Pin to top bar" }
                        }}
                    }
                }
            }
        }
    }}
}

#[component]
fn MaximaCard(card: Card) -> Element {
    use super::mission_phase::{format_extreme, MaxQuantity, FLIGHT_MAXIMA};
    let quantity = MaxQuantity::from_index(card.index);
    let maxima = FLIGHT_MAXIMA.read().clone();
    let (fc, gs) = maxima.quantity(quantity);
    let primary_style = if fc.is_some() { "font-size:15px;margin-top:4px;" } else { "font-size:20px;font-weight:600;margin-top:6px;" };
    rsx! {section {class:"gs26-maxima-card","data-quantity":"{card.index}",
        strong {class:"gs26-card-label","{card.label}"}
        if let Some(fc) = fc {
            div {class:"gs26-maxima-fc",style:"font-size:20px;font-weight:600;margin-top:6px;","{format_extreme(quantity, fc)}"}
            small {style:"display:block;color:#94a3b8;","FC-reported value (authoritative)"}
        }
        if let Some(first) = gs.first() {
            div {class:"gs26-maxima-value",style:"{primary_style}","{format_extreme(quantity, first)}"}
        }
        for other in gs.iter().skip(1) {
            small {class:"gs26-maxima-other",style:"display:block;color:#94a3b8;","{format_extreme(quantity, other)}"}
        }
        if fc.is_none() && gs.is_empty() {
            small {style:"display:block;color:#94a3b8;margin-top:6px;",
                if maxima.tracking {"Waiting for flight telemetry"} else {"Tracks from T+ · cleared by T-clock reset or Reset all boards"}
            }
        }
    }}
}

/// Small offline-tile map around the rocket. MapLibre is created only while
/// the card is on screen (IntersectionObserver) and positions are pushed at
/// most every 2 s, only when they change and the page is visible.
const MINI_MAP_JS: &str = r#"(function(){if(window.gs26MiniMap)return;
const maps=new Map();const colors={rocket:'#f97316',ground_station:'#38bdf8',rrls:'#facc15'};
const ok=p=>p&&Number.isFinite(p.lat)&&Number.isFinite(p.lon)&&Math.abs(p.lat)<=90&&Math.abs(p.lon)<=180;
const dist=(a,b)=>{const R=6371000,r=Math.PI/180,dl=(b.lat-a.lat)*r,dn=(b.lon-a.lon)*r;const h=Math.sin(dl/2)**2+Math.cos(a.lat*r)*Math.cos(b.lat*r)*Math.sin(dn/2)**2;return 2*R*Math.asin(Math.sqrt(h));};
function create(id){const st=maps.get(id),el=document.getElementById(id);if(!st||!el||st.map||!st.visible)return;
 if(!window.maplibregl){el.dataset.miniMapState='no-library';return;}
 const r=st.last&&ok(st.last.rocket)?st.last.rocket:null;
 st.map=new window.maplibregl.Map({container:el,interactive:false,attributionControl:false,fadeDuration:0,center:r?[r.lon,r.lat]:[0,0],zoom:r?15:1,style:{version:8,sources:{t:{type:'raster',tiles:[st.tiles],tileSize:256,maxzoom:17}},layers:[{id:'t',type:'raster',source:'t'}]}});
 st.map.on('error',()=>{el.dataset.miniMapTiles='error';});st.map.on('load',()=>{el.dataset.miniMapState='ready';apply(id);});}
function apply(id){const st=maps.get(id);if(!st||!st.map||!st.last)return;const s=st.last,r=s.rocket;
 for(const k of ['rocket','ground_station','rrls']){const p=s[k];let m=st.markers[k];const inView=ok(p)&&(k==='rocket'||(ok(r)&&dist(r,p)<=3000));
  if(!inView){if(m){m.remove();delete st.markers[k];}continue;}
  if(!m){const d=document.createElement('div');d.className='gs26-mini-map-marker';d.dataset.marker=k;d.title=k;d.style.cssText='width:12px;height:12px;border-radius:50%;background:'+colors[k]+';border:2px solid #0b1220;box-shadow:0 0 0 2px '+colors[k];m=st.markers[k]=new window.maplibregl.Marker({element:d}).setLngLat([p.lon,p.lat]).addTo(st.map);}
  m.setLngLat([p.lon,p.lat]);m.getElement().style.opacity=p.stale?'.45':'1';}
 if(!ok(r))return;const pts=['ground_station','rrls'].map(k=>s[k]).filter(p=>ok(p)&&dist(r,p)<=3000);
 if(!pts.length){st.map.jumpTo({center:[r.lon,r.lat],zoom:15});return;}
 const b=new window.maplibregl.LngLatBounds([r.lon,r.lat],[r.lon,r.lat]);for(const p of pts)b.extend([p.lon,p.lat]);st.map.fitBounds(b,{padding:28,maxZoom:16,duration:0});}
window.gs26MiniMap={
 mount(id,tiles){if(maps.has(id))return;const st={tiles,markers:{},visible:false,map:null,last:null};maps.set(id,st);
  const observe=n=>{const el=document.getElementById(id);if(!maps.has(id))return;if(!el){if(n<120)setTimeout(()=>observe(n+1),50);return;}
   el.dataset.miniMapState='waiting';st.io=new IntersectionObserver(es=>{for(const e of es){st.visible=e.isIntersecting;el.dataset.miniMapVisible=String(st.visible);if(st.visible){create(id);apply(id);}}});st.io.observe(el);};observe(0);},
 update(id,s){const st=maps.get(id);if(!st)return;st.last=s;const el=document.getElementById(id);if(el){el.dataset.rocketLat=ok(s.rocket)?s.rocket.lat.toFixed(6):'';el.dataset.rocketLon=ok(s.rocket)?s.rocket.lon.toFixed(6):'';el.dataset.updates=String((+el.dataset.updates||0)+1);}if(st.visible){create(id);apply(id);}},
 destroy(id){const st=maps.get(id);if(!st)return;maps.delete(id);st.io&&st.io.disconnect();st.map&&st.map.remove();}};})();"#;

#[derive(Clone, PartialEq, Default, Deserialize)]
struct StationPoint { latitude: f64, longitude: f64, #[serde(default)] stale: bool }
#[derive(Clone, PartialEq, Default, Deserialize)]
struct StationPoints { #[serde(default)] ground_station: Option<StationPoint>, #[serde(default)] rrls: Option<StationPoint> }

fn mini_map_snapshot(rocket: Option<(f64, f64)>, altitude_m: Option<f64>, stations: &StationPoints) -> String {
    let point = |p: &Option<StationPoint>| p.as_ref().map(|p| serde_json::json!({"lat":p.latitude,"lon":p.longitude,"stale":p.stale}));
    serde_json::json!({
        "rocket": rocket.map(|(lat, lon)| serde_json::json!({"lat":lat,"lon":lon,"alt":altitude_m})),
        "ground_station": point(&stations.ground_station),
        "rrls": point(&stations.rrls),
    }).to_string()
}

#[component]
fn MiniMapCard(card: Card, dom_id: String) -> Element {
    let mut readout = use_signal(String::new);
    let mount_id = dom_id.clone();
    use_effect(move || {
        let tiles = super::map_tiles_url();
        super::js_eval(&format!("{MINI_MAP_JS};window.gs26MiniMap.mount({mount_id:?},{tiles:?});"));
    });
    let drop_id = dom_id.clone();
    use_drop(move || super::js_eval(&format!("window.gs26MiniMap&&window.gs26MiniMap.destroy({drop_id:?});")));
    let loop_id = dom_id.clone();
    use_future(move || {
        let id = loop_id.clone();
        async move {
            let mut stations = StationPoints::default();
            let mut last_sent = String::new();
            let mut tick: u32 = 0;
            loop {
                if super::dashboard_page_visible() {
                    if tick % 3 == 0
                        && let Ok(next) = super::http_get_json::<StationPoints>("/api/antenna/status").await
                    {
                        stations = next;
                    }
                    tick = tick.wrapping_add(1);
                    let rocket = super::latest_rocket_gps_from_store();
                    let altitude = super::latest_rocket_gps_altitude_m_from_store();
                    let snapshot = mini_map_snapshot(rocket, altitude, &stations);
                    if snapshot != last_sent {
                        super::js_eval(&format!("window.gs26MiniMap&&window.gs26MiniMap.update({id:?},{snapshot});"));
                        last_sent = snapshot;
                        let text = match rocket {
                            Some((lat, lon)) => format!("Rocket {lat:.5}°, {lon:.5}°{}", altitude.map(|a| format!(" · {a:.0} m")).unwrap_or_default()),
                            None => "Waiting for rocket GPS".to_string(),
                        };
                        if *readout.peek() != text { readout.set(text); }
                    }
                }
                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::TimeoutFuture::new(2000).await;
                #[cfg(not(target_arch = "wasm32"))]
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    });
    rsx! {section {class:"gs26-mini-map-card",
        strong {class:"gs26-card-label","{card.label}"}
        div {id:"{dom_id}",class:"gs26-mini-map",role:"img",aria_label:"Map around the rocket",style:"position:relative;height:220px;margin-top:8px;border-radius:6px;overflow:hidden;background:#102032;"}
        small {class:"gs26-mini-map-readout",style:"display:block;color:#94a3b8;margin-top:4px;", if readout.read().is_empty() {"Waiting for rocket GPS"} else {"{readout}"}}
    }}
}

#[component]
fn CameraCard(card: Card, config: super::live_stream_tab::LiveStreamConfig, error: String) -> Element {
    let feed = config.streams.iter().find(|feed|feed.id == card.stream_id && feed.online);
    rsx! {section {
        strong {class:"gs26-card-label","{card.label}"}
        if !error.is_empty() {p {role:"status","{error}"}}
        else if !config.can_preview_live {p {"Live cameras require operator or stream-management access."}}
        else if card.stream_id.is_empty() {p {"Choose a camera source in Customize dashboard."}}
        else if let Some(feed) = feed {
            iframe {src:super::live_stream_tab::media_url(&feed.url),title:card.label.clone(),allow:"autoplay; fullscreen",style:"display:block;width:100%;aspect-ratio:16 / 9;min-height:180px;border:0;margin-top:8px;background:#080d15;", "sandbox":"allow-scripts allow-same-origin"}
        } else {p {role:"status","Camera offline — waiting for {card.stream_id}"}}
    }}
}

#[component]
fn CardEditor(card: Card, data_layout: super::layout::DataTabLayout,
    section_choices: Vec<String>, sender_choices: Vec<String>, unit_choices: Vec<String>,
    type_choices: Vec<(String, String)>, cameras: Vec<super::live_stream_tab::LiveStreamSpec>,
    onchange: EventHandler<Card>) -> Element {
    let observed_count = latest_telemetry_row(&card.data_type, (!card.sender.is_empty()).then_some(card.sender.as_str()))
        .map(|row| row.values.len()).unwrap_or(0);
    let channels = card_channels(&data_layout, &card.data_type, card.index, observed_count);
    let ranges = card_ranges(&card);
    let selected_range = ranges.iter().position(|range| *range == (card.min, card.max)).unwrap_or(0);
    rsx! {div {class:"gs26-card-editor",style:"display:grid;gap:6px;margin-bottom:10px;",
        label {"Visibility " select {aria_label:"Visibility",value:if card.visible {"show"}else{"hide"},onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.visible=e.value()=="show";onchange.call(c);}},
            option {selected:card.visible,value:"show","Show"} option {selected:!card.visible,value:"hide","Hide"}
        }}
        label {"Title " input {value:card.label.clone(),maxlength:"127",oninput:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.label=e.value();onchange.call(c);}}}}
        label {"Section " select {aria_label:"Section",value:card.section.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.section=e.value();onchange.call(c);}},
            for name in section_choices {option {selected:name==card.section,value:name.clone(),"{name}"}}
        }}
        label {"Display " select {aria_label:"Display",value:card.display.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.display=e.value();if !pinnable(&c) {c.pinned=false;}match c.display.as_str(){"map"=>{c.data_type="MINI_MAP".into();c.min=0.;c.max=1.;}"maxima"=>{c.data_type="FLIGHT_MAXIMA".into();c.index=c.index.min(2);c.min=0.;c.max=1.;}_=>{}}onchange.call(c);}},
            for kind in ["number","bar","gauge","trend","state","camera","map","maxima"] {option {selected:kind==card.display,value:kind,"{kind}"}}
        }}
        label {"Card width " select {aria_label:"Card width",value:card.width.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.width=e.value();onchange.call(c);}},option {selected:card.width=="normal",value:"normal","Normal"} option {selected:card.width=="wide",value:"wide","Wide"} option {selected:card.width=="full",value:"full","Full width"}}}
        if card.display == "maxima" {
            label {"Quantity " select {aria_label:"Maximum quantity",value:"{card.index}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(index)=e.value().parse::<usize>(){let mut c=card.clone();c.index=index.min(2);onchange.call(c);}}},
                for (index,name) in ["Max altitude","Max speed","Max acceleration"].iter().enumerate() {option {selected:index==card.index,value:"{index}","{name}"}}
            }}
            small {"Computed by the GroundStation for the current flight; FC-reported maxima are shown as authoritative when received."}
        } else if card.display == "map" {
            small {"Small offline map around the rocket's latest position, with the GroundStation and RRLS when in view."}
        } else if card.display == "camera" {
            label {"Camera source " select {aria_label:"Camera source",value:card.stream_id.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.stream_id=e.value();onchange.call(c);}},
                option {selected:card.stream_id.is_empty(),value:"","Choose a camera"}
                if !card.stream_id.is_empty() && !cameras.iter().any(|c|c.id==card.stream_id) {option {selected:true,value:card.stream_id.clone(),"{card.stream_id} (unavailable)"}}
                for camera in cameras {option {selected:camera.id==card.stream_id,value:camera.id.clone(),"{camera.label}"}}
            }}
        } else {
            label {"Data type " select {aria_label:"Data type",style:"min-height:44px;",value:card.data_type.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|onchange.call(card_with_source(card.clone(),e.value()))},
                for (id, label) in type_choices {option {key:"{id}",selected:id==card.data_type,value:id.clone(),"{label}"}}
            }}
            small {"Changing source resets board, channel, unit label and range to its defaults."}
            label {"Board " select {aria_label:"Board",value:card.sender.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.sender=e.value();onchange.call(c);}},
                for sender in sender_choices {option {selected:sender==card.sender,value:sender.clone(),if sender.is_empty(){"Any board"}else{"{sender}"}}}
            }}
            label {"Channel " select {aria_label:"Channel",value:"{card.index}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(index)=e.value().parse(){let mut c=card.clone();c.index=index;onchange.call(c);}}},
                for (index,label) in channels {option {selected:index==card.index,value:"{index}","{label}"}}
            }}
            label {"Unit label " select {aria_label:"Unit label",value:card.unit.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.unit=e.value();onchange.call(c);}},
                for unit in unit_choices {option {selected:unit==card.unit,value:unit.clone(),if unit.is_empty(){"None"}else{"{unit}"}}}
            }}
            label {"Decimal places " select {aria_label:"Decimal places",value:"{card.precision}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(precision)=e.value().parse(){let mut c=card.clone();c.precision=precision;onchange.call(c);}}},
                for precision in 0..=6 {option {selected:precision==card.precision,value:"{precision}","{precision}"}}
            }}
            label {"Display range " select {aria_label:"Display range",value:"{selected_range}",onchange:{let card=card.clone();let ranges=ranges.clone();move |e:Event<FormData>|{if let Ok(index)=e.value().parse::<usize>() && let Some(&(min,max))=ranges.get(index){let mut c=card.clone();c.min=min;c.max=max;onchange.call(c);}}},
                for (index,(min,max)) in ranges.iter().enumerate() {option {selected:index==selected_range,value:"{index}","{min} to {max}"}}
            }}
            small {"Unit labels do not convert measurements. Saved custom units and ranges remain selectable."}
        }
    }}
}

/// Draw actual timestamps, breaking across telemetry outages. The launch view
/// uses the full requested window, so absent portions remain visibly empty.
#[cfg(test)]
fn trend_path(samples:&[super::dashboard_history::Sample], window:Option<(i64,i64)>) -> String {
    card_values::trend_geometry(samples, window).path
}

#[cfg(test)]
mod trend_tests {
    use super::*;
    #[test]
    fn gaps_are_broken_and_capture_has_fixed_time_axis() {
        let samples=vec![super::super::dashboard_history::Sample{time:10_000,value:-5.,segment:0},
            super::super::dashboard_history::Sample{time:10_001,value:100.,segment:0},
            super::super::dashboard_history::Sample{time:30_000,value:0.,segment:0}];
        let path=trend_path(&samples,Some((10_000,40_000)));
        assert_eq!(path.matches('M').count(),2);
        assert!(path.contains("M0.00,"));
        assert!(path.contains("M173.33,"));
    }
    #[test]
    fn short_missing_channel_gap_starts_new_subpath() {
        let samples=vec![
            super::super::dashboard_history::Sample {time:1000,value:1.,segment:1},
            super::super::dashboard_history::Sample {time:1001,value:2.,segment:1},
            super::super::dashboard_history::Sample {time:1003,value:3.,segment:2},
            super::super::dashboard_history::Sample {time:1004,value:4.,segment:2},
        ];
        let path=trend_path(&samples,None);
        assert_eq!(path.matches('M').count(),2);
        assert_eq!(path.matches('L').count(),2);
    }

}

fn pinned_details(card: &Card, value: Option<f32>, age_ms: Option<i64>) -> String {
    let value = value.map(|value| format!("{value} {}", card.unit)).unwrap_or_else(|| "No sample".into());
    let age = age_ms.map(|age| format!("received {age} ms ago")).unwrap_or_else(|| "waiting for telemetry".into());
    let sender = if card.sender.is_empty() { "any sender" } else { card.sender.as_str() };
    format!("{}: {value}; {age}; source {} / {sender}", card.label, card.data_type)
}
#[cfg(test)]
mod pinned_size_tests {
    use super::*;
    #[test]
    fn compact_details_preserve_value_source_and_exact_age_without_reserved_digits() {
        let mut card=defaults()[0].clone();card.unit="kg".into();card.sender="DAQ".into();
        let value=-123_456.79_f32;
        let details=pinned_details(&card,Some(value),Some(12345));
        assert!(details.contains(&format!("{value} kg")));
        assert!(details.contains("12345 ms"));
        assert!(details.contains("DAQ"));
        let missing=pinned_details(&card,None,None);
        assert!(missing.contains("No sample"));assert!(missing.contains("waiting for telemetry"));
    }
}
/// Reuse Data's configured view policy without changing saved dashboard cards.
/// A Dashboard has no selected Data view: conflicting matches keep its fallback.
fn card_minmax_policy(layout: &super::layout::DataTabLayout, card: &Card) -> bool {
    if !matches!(card.display.as_str(), "trend" | "number" | "gauge" | "bar") {
        return false;
    }
    let mut policy = None;
    let mut ambiguous = false;
    let matches = |kind: &str, sender: Option<&str>, index: usize| {
        kind == card.data_type && index == card.index
            && sender.is_none_or(|sender| !card.sender.is_empty() && sender == card.sender)
    };
    for tab in &layout.tabs {
        let mut visit = |sub: Option<&super::layout::DataSubtabSpec>| {
            let inherited = sub.and_then(|sub| sub.show_min_max).or(tab.show_min_max).unwrap_or(true);
            let mut admit = |visible: bool| {
                if policy.is_some_and(|previous| previous != visible) {
                    ambiguous = true;
                }
                policy = Some(visible);
            };
            if let Some(items) = sub.and_then(|sub| sub.summary_items.as_ref()).filter(|items| !items.is_empty()) {
                for item in items {
                    if matches(&item.data_type, item.sender_id.as_deref(), item.index) {
                        admit(item.show_min_max.unwrap_or(inherited) && item.boolean_labels.is_none());
                    }
                }
            } else {
                let kind = sub.and_then(|sub| sub.data_type.as_deref()).unwrap_or(&tab.id);
                let sender = sub.and_then(|sub| sub.sender_id.as_deref());
                let channels = sub.and_then(|sub| sub.channels.as_ref()).unwrap_or(&tab.channels);
                if card.index < channels.len() && matches(kind, sender, card.index) {
                    let boolean = sub.and_then(|sub| sub.boolean_labels.as_ref()).or(tab.boolean_labels.as_ref());
                    let channel_boolean = sub.and_then(|sub| sub.channel_boolean_labels.as_ref())
                        .or(tab.channel_boolean_labels.as_ref()).and_then(|labels| labels.get(card.index));
                    admit(inherited && boolean.is_none() && channel_boolean.is_none());
                }
            }
        };
        if let Some(subtabs) = tab.subtabs.as_ref().filter(|subtabs| !subtabs.is_empty()) {
            for sub in subtabs { visit(Some(sub)); }
        } else {
            visit(None);
        }
    }
    if ambiguous { card.display == "trend" } else { policy.unwrap_or(card.display == "trend") }
}

#[component]
fn TelemetryCard(card: Card, #[props(default)] thrust: bool, #[props(default)] aggregate: bool, #[props(default = false)] compact: bool, #[props(default)] show_min_max: bool) -> Element {
    // Share Data's ingress-driven render cadence. A per-card200ms sampler
    // delayed live values and discarded intermediate display updates.
    let _ = *super::TELEMETRY_RENDER_EPOCH.read();
    // Age labels still advance when no sample arrives; this clock never adds
    // trend points or fabricates telemetry.
    let _ = *super::HEADER_CLOCK_TICK.read();
    let row = latest_telemetry_row(
        &card.data_type,
        if card.sender.is_empty() {
            None
        } else {
            Some(&card.sender)
        },
    );
    let age = row.as_ref().map(|r| {
        super::current_wallclock_ms()
            .saturating_sub(r.received_timestamp_ms)
            .max(0)
    });
    let live_value = row
        .as_ref()
        .and_then(|r| r.values.get(card.index).copied().flatten())
        .filter(|v| v.is_finite()).map(|v|if thrust {v*9.80665}else{v});
    let _ = *super::FRONTEND_DATA_CLEAR_EPOCH.read();
    let mut samples = if !compact && (aggregate || card.display == "trend") {
        super::dashboard_trend_samples(&card.data_type,&card.sender,card.index,aggregate)
    }else{Vec::new()};
    if thrust {for sample in &mut samples {sample.value *= 9.80665;}}
    let window=if aggregate {super::dashboard_launch_window()}else{
        samples.last().map(|last|(last.time.saturating_sub(60_000),last.time))
    };
    let capture=if aggregate {card_values::capture_stats(&samples,window)}else{None};
    let value=card_values::display_value(aggregate,live_value,capture);
    let text = value
        .map(|v| {
            if card.display == "state" {
                if v >= 0.5 {
                    "Open / On".into()
                } else {
                    "Closed / Off".into()
                }
            } else {
                format!("{v:.precision$} {}", card.unit, precision=card.precision)
            }
        })
        .unwrap_or_else(|| "—".into());
    // Reserve a stable numeric slot so changing digits do not move adjacent pins.
    // Keep full precision; unusually long values remain horizontally scrollable.
    let compact_number = value.map(|v| if card.display == "state" { text.clone() }
        else { format!("{v:.precision$}", precision=card.precision) }).unwrap_or_else(|| "—".into());
    let compact_details = pinned_details(&card, value, age);
    let compact_stale = age.is_none_or(|age|age>10000);
    let fraction = value
        .map(|v| ((v - card.min) / (card.max - card.min)).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let geometry=if card.display=="trend" {card_values::trend_geometry(&samples,window)}else{card_values::TrendGeometry::default()};
    let has_trend=!geometry.path.is_empty();
    let minmax=if !compact && show_min_max {
        if aggregate {capture.map(|stats|(stats.min,stats.max))}else{
            super::telemetry_channel_minmax(&card.data_type,if card.sender.is_empty(){None}else{Some(&card.sender)},card.index)
                .map(|(min,max)|if thrust {(min*9.80665,max*9.80665)}else{(min,max)})
        }
    }else{None};
    rsx! {div { title:compact.then_some(compact_details.clone()), aria_label:compact.then_some(compact_details.clone()), tabindex:compact.then_some("0"), style: if compact { "display:inline-flex;flex-wrap:nowrap;align-items:baseline;gap:5px;flex:0 1 auto;min-width:0;max-width:100%;" } else { "" },
        strong {class:"gs26-card-label",style:if compact {"min-width:0;max-width:100%;overflow-wrap:anywhere;"}else{""},"{card.label}"}
        div {class:"gs26-card-value",style:if compact {"font-size:13px;display:inline-flex;flex-wrap:nowrap;align-items:baseline;gap:4px;min-width:0;max-width:100%;"} else {"font-size:24px;font-variant-numeric:tabular-nums;"},
            if compact {
                span {class:"gs26-pinned-number",style:if card.display == "state" {"flex:0 1 auto;min-width:0;max-inline-size:18ch;overflow-x:auto;white-space:nowrap;font-variant-numeric:tabular-nums;"} else {"flex:0 0 10ch;inline-size:10ch;min-inline-size:10ch;max-inline-size:10ch;overflow-x:auto;white-space:nowrap;font-variant-numeric:tabular-nums;"},"{compact_number}"}
                if card.display != "state" && value.is_some() {span {class:"gs26-pinned-unit",style:"flex:0 0 auto;white-space:nowrap;","{card.unit}"}}
            } else {"{text}"}
        }
        if !compact && value.is_some() && card.display=="bar" {progress {value:fraction as f64,max:1,style:"width:100%;height:20px;"}}
        if !compact && value.is_some() && card.display=="gauge" {div {role:"img","aria-label":text.clone(),style:"width:70px;height:70px;border-radius:50%;background:conic-gradient(#38bdf8 {fraction*360.0}deg,#334155 0);",div {style:"position:relative;top:12px;left:12px;width:46px;height:46px;border-radius:50%;background:#0f172a;"}}}
        if !compact && has_trend && card.display=="trend" {svg {view_box:"0 0 260 60",style:"width:100%;height:60px;",role:"img","aria-label":format!("{} {} history",card.label,if aggregate {"launch"}else{"live 60 second"}),path {d:geometry.path,fill:"none",stroke:"#38bdf8",stroke_width:"2"}
            for (index,(x,y)) in geometry.isolated.iter().enumerate() {circle {key:"point-{index}",cx:"{x}",cy:"{y}",r:"2",fill:"#38bdf8"}}
        }}
        if let Some((min,max)) = minmax {small {style:"display:block;", {format!("{} · Min {min:.precision$} · Max {max:.precision$} {}",if aggregate {"Launch capture"}else{"Data history"},card.unit,precision=card.precision)}}}
        if aggregate && !compact && !has_trend && card.display=="trend" {small {"No samples with known source time in this launch window"}}
        if aggregate && !compact {
            small {class:"gs26-card-capture-time",style:"color:#94a3b8;",
                if let Some(stats)=capture {if let Some((start,_))=window { {format!("Launch capture · last sample +{:.1}s",stats.last.time.saturating_sub(start) as f64/1000.0)} }}
                else {"Launch capture · no source-timed samples"}
            }
        } else if compact {
            if compact_stale {span {class:"gs26-pinned-stale",style:"color:#fbbf24;flex:0 0 auto;font-size:10px;",aria_label:if age.is_some(){"Stale telemetry"}else{"No telemetry"},"●"}}
        } else {small {class:"gs26-card-freshness",style:format!("color:{};",if age.is_none_or(|a|a>10000){"#fbbf24"}else{"#94a3b8"}),
            if let Some(ms)=age {if ms>10000 {"Stale · "} "{ms/1000}s ago" if !card.sender.is_empty() {" · {card.sender}"}}else{"Waiting for telemetry"}
        }}
    }}
}

#[component]
pub(super) fn PinnedTelemetry(#[props(default)] test_fire_mode: bool) -> Element {
    let cards = use_context::<Signal<Vec<Card>>>();
    rsx! {
        if cards.read().iter().any(|card| card.pinned) {
            div { style:"grid-column:1 / -1;grid-row:5;display:flex;flex-wrap:wrap;justify-content:flex-start;align-items:start;text-align:left;gap:6px 14px;padding:6px 0;font-size:12px;min-width:0;max-width:100%;",
                "aria-label":"Pinned telemetry",
                for (index, card) in cards.read().iter().enumerate().filter(|(_,card)|card.pinned) {
                    TelemetryCard {key:"pinned-{index}", card:display_card(card.clone(),test_fire_mode), compact:true, thrust:test_fire_mode && card.data_type=="LOADCELL_WEIGHT_KG"}
                }
            }
        }
    }
}

#[component]
pub(super) fn HeaderPinSettings() -> Element {
    let mut cards = use_context::<Signal<Vec<Card>>>();
    let snapshot = cards.read().clone();
    let count = snapshot.iter().filter(|card|card.pinned).count();
    rsx! {section {
        h3 {"Pin telemetry to the top bar ({count}/8)"}
        p {"Pinned values remain visible beside the clock and status as you switch tabs. Edit their sources and formatting in Dashboard."}
        div {style:"display:flex;gap:8px 16px;flex-wrap:wrap;",
            for (index, card) in snapshot.iter().enumerate().filter(|(_,card)|pinnable(card)) {
                label {
                    input {r#type:"checkbox",checked:card.pinned,disabled:!card.pinned && count>=8,
                        onchange:move |event:Event<FormData>|{let mut all=cards.read().clone();all[index].pinned=event.checked();if valid(&all){save(&all);cards.set(all);}}
                    }
                    " {card.label}"
                }
            }
        }
    }}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overview_labels_are_not_offered_as_invented_telemetry_types() {
        let layout:super::super::layout::DataTabLayout=serde_json::from_value(serde_json::json!({"tabs":[{"id":"UI_OVERVIEW","label":"Overview","channels":[],"subtabs":[{"id":"all","label":"All","chart_groups":[{"channels":[0],"chart_series":[{"data_type":"REAL_PRESSURE","index":0,"label":"Pressure"}]}],"summary_items":[{"label":"Mass","data_type":"REAL_MASS","index":0}]}]}]})).unwrap();
        let choices=data_type_choices(&layout,&[],[]);
        assert!(!choices.iter().any(|(id,_)|id=="UI_OVERVIEW"));
        for required in ["REAL_PRESSURE","REAL_MASS"] {assert!(choices.iter().any(|(id,_)|id==required));}
    }
    #[test]
    fn dropdown_ranges_keep_saved_custom_bounds_and_source_defaults() {
        let mut card = defaults()[0].clone(); card.min = -12.25; card.max = 97.25;
        let ranges = card_ranges(&card);
        assert_eq!(ranges.iter().filter(|range| **range == (-12.25,97.25)).count(), 1);
        assert!(ranges.contains(&(0.,16.8)));
        assert!(ranges.iter().all(|(min,max)| min.is_finite() && max.is_finite() && max > min));
    }
    #[test]
    fn source_change_resets_dependent_choices_without_losing_user_labels_or_pins() {
        let mut card = defaults()[0].clone();card.label="My pressure".into();card.section="Offline custom".into();card.pinned=true;card.index=7;
        let next=card_with_source(card,"COMBUSTION_CHAMBER_PRESSURE".into());
        assert_eq!((next.sender.as_str(),next.index,next.unit.as_str(),next.min,next.max),("DAQ",0,"psig",0.,1000.));
        assert_eq!((next.label.as_str(),next.section.as_str(),next.pinned),("My pressure","Offline custom",true));
        assert!(valid(&[next]));
    }
    #[test]
    fn channel_choices_use_configured_names_and_preserve_an_offline_saved_channel() {
        let layout:super::super::layout::DataTabLayout=serde_json::from_value(serde_json::json!({"tabs":[{"id":"CUSTOM","label":"Custom","channels":["X","Y"],"chart_groups":[{"data_type":"OTHER","channels":[2],"labels":["Pressure"]}]}]})).unwrap();
        let channels=card_channels(&layout,"CUSTOM",7,3);
        assert_eq!(channels.iter().map(|(index,_)|*index).collect::<Vec<_>>(),vec![0,1,2,7]);
        assert_eq!(channels[0].1,"X [0]");assert_eq!(channels[3].1,"Saved channel 7 [7]");
        assert_eq!(card_channels(&layout,"OTHER",2,0),vec![(2,"Pressure [2]".into())]);
    }
    #[test]
    fn dropdown_choices_preserve_custom_saved_labels_and_remove_duplicates() {
        let choices=choice_strings(["Custom unit".into(),"kg".into(),"kg".into(),String::new()]);
        assert_eq!(choices,vec!["","Custom unit","kg"]);
    }
    #[test]
    fn type_choices_merge_layout_runtime_defaults_and_offline_saved_types() {
        let value: serde_json::Value = serde_json::from_str(include_str!("../../docs/api-examples/layout.full.json")).unwrap();
        let mut layout: super::super::layout::DataTabLayout = serde_json::from_value(value["data_tab"].clone()).unwrap();
        layout.tabs[0].id = "SCHEMA_ONLY".into();
        layout.tabs[0].label = "Schema sensor".into();
        let mut cards = defaults();
        cards[0].data_type = "OFFLINE_CUSTOM".into();
        let choices = data_type_choices(&layout, &cards, ["RUNTIME_ONLY".into(), "RUNTIME_ONLY".into()]);
        for kind in ["SCHEMA_ONLY", "RUNTIME_ONLY", "OFFLINE_CUSTOM", "LOADCELL_WEIGHT_KG"] {
            assert_eq!(choices.iter().filter(|(id, _)| id == kind).count(), 1);
        }
        assert!(choices.iter().any(|(id, label)| id == "SCHEMA_ONLY" && label == "Schema sensor (SCHEMA_ONLY)"));
        assert!(choices.windows(2).all(|pair| pair[0].0 < pair[1].0));
    }

    #[test]
    fn legacy_default_50kg_gets_a_trend_without_changing_custom_cards() {
        let mut cards = defaults();
        assert_eq!(cards[0].display, "trend");
        cards[0].display = "number".into();
        cards[0].pinned = true;
        let mut custom = cards[0].clone();
        custom.label = "Custom numeric readout".into();
        cards.push(custom.clone());
        migrate_default_loadcell_trend(&mut cards);
        assert_eq!(cards[0].display, "trend");
        assert!(cards[0].pinned);
        assert!(cards.last() == Some(&custom));
    }

    #[test]
    fn missing_chamber_migration_is_bounded_and_preserves_pressure_style() {
        let mut cards = defaults();
        cards.retain(|card| card.data_type != "COMBUSTION_CHAMBER_PRESSURE");
        migrate_default_pressure_cards(&mut cards);
        migrate_default_pressure_cards(&mut cards);
        let pressure: Vec<_> = cards.iter().filter(|card| matches!(card.data_type.as_str(), "COMBUSTION_CHAMBER_PRESSURE" | "FUEL_TANK_PRESSURE")).collect();
        assert_eq!(pressure.len(), 2);
        assert!(pressure.iter().all(|card| card.display == "gauge"));
        assert!(pressure.iter().any(|card| card.sender == "DAQ" && card.data_type == "COMBUSTION_CHAMBER_PRESSURE"));
        let mut custom = vec![cards[0].clone()];
        let before = custom.clone();
        migrate_default_pressure_cards(&mut custom);
        assert!(custom == before);
        let mut full = vec![defaults()[0].clone(); 30];
        full.extend(cards.iter().filter(|card| matches!(card.data_type.as_str(), "LOADCELL_WEIGHT_KG" | "FUEL_TANK_PRESSURE")).cloned());
        migrate_default_pressure_cards(&mut full);
        assert_eq!(full.len(), 32);
    }

    #[test]
    fn camera_cards_preserve_source_and_size_without_becoming_telemetry_pins() {
        let mut cards = defaults();
        cards[0].display = "camera".into();
        cards[0].stream_id = "pad-wide".into();
        cards[0].width = "full".into();
        assert!(valid(&cards));
        let restored: Vec<Card> = serde_json::from_str(&serde_json::to_string(&cards).unwrap()).unwrap();
        assert!(restored == cards);
        cards[0].pinned = true;
        assert!(!valid(&cards));
        let mut legacy = serde_json::to_value(&cards[1]).unwrap();
        legacy.as_object_mut().unwrap().remove("width");
        legacy.as_object_mut().unwrap().remove("stream_id");
        let restored: Card = serde_json::from_value(legacy).unwrap();
        assert_eq!(restored.width, "normal");
        assert!(restored.stream_id.is_empty());
    }

    #[test]
    fn pins_are_bounded_and_legacy_cards_keep_their_defaults() {
        let mut cards = defaults();
        for card in cards.iter_mut().take(8) { card.pinned = true; }
        assert!(valid(&cards));
        cards[8].pinned = true;
        assert!(!valid(&cards));
        let mut legacy = serde_json::to_value(&cards[0]).unwrap();
        legacy.as_object_mut().unwrap().remove("pinned");
        legacy.as_object_mut().unwrap().remove("precision");
        let restored: Card = serde_json::from_value(legacy).unwrap();
        assert!(!restored.pinned);
        assert_eq!(restored.precision, 2);
        cards[8].pinned = false;
        cards[0].section = "Launch team".into();
        assert!(valid(&cards));
    }

    #[test]
    fn defaults_cover_fill_and_av_bay_without_mixing_batteries() {
        let cards = defaults();
        assert!(valid(&cards));
        for board in ["GB", "VB", "PB"] {
            assert!(cards
                .iter()
                .any(|c| c.data_type == "BATTERY_VOLTAGE" && c.sender == board));
        }
        for dt in [
            "LOADCELL_50_WEIGHT_KG",
            "LOADCELL_WEIGHT_KG",
            "FUEL_TANK_PRESSURE",
            "LOADCELL_FILL_PERCENT",
            "VALVE_STATE",
        ] {
            assert!(cards
                .iter()
                .any(|c| c.data_type == dt && c.section == "Fill System"));
        }
    }
    #[test]
    fn saved_card_layout_roundtrips_and_invalid_ranges_are_rejected() {
        let mut cards = defaults();
        cards.reverse();
        cards[0].visible = false;
        let encoded = serde_json::to_string(&cards).unwrap();
        assert!(cards == serde_json::from_str::<Vec<Card>>(&encoded).unwrap());
        cards[0].max = cards[0].min;
        assert!(!valid(&cards));
    }
}

#[cfg(test)]
mod phase_tests {
    use super::*;
    #[test]
    fn defaults_carry_hidden_flight_cards_with_stable_unique_ids() {
        let cards = defaults();
        assert!(valid(&cards));
        let mut ids: Vec<_> = cards.iter().map(|c| c.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), cards.len());
        for id in super::super::mission_phase::FLIGHT_CARD_IDS {
            let card = cards.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("{id}"));
            assert!(!card.visible && card.section == "Flight", "{id}");
            assert!(!pinnable(card) || card.display == "trend");
        }
    }

    #[test]
    fn in_flight_profile_shows_flight_cards_first_and_fill_profile_keeps_base() {
        let cards = defaults();
        let profiles = super::super::mission_phase::PhaseProfiles::default();
        let flight = phase_view(&cards, &profiles, Some(super::super::mission_phase::Phase::InFlight));
        let first: Vec<&str> = flight.iter().take(10).map(|(i, _)| cards[*i].id.as_str()).collect();
        assert_eq!(first, super::super::mission_phase::FLIGHT_CARD_IDS);
        assert!(flight.iter().take(10).all(|(_, shown)| *shown));
        // Fill data still follows, visible, below the flight data.
        assert!(flight.iter().skip(10).any(|(i, shown)| *shown && cards[*i].section == "Fill System"));
        let fill = phase_view(&cards, &profiles, Some(super::super::mission_phase::Phase::Fill));
        assert_eq!(cards[fill[0].0].section, "Fill System");
        assert!(fill.iter().all(|(i, shown)| *shown == cards[*i].visible));
        let landed = phase_view(&cards, &profiles, Some(super::super::mission_phase::Phase::Landed));
        assert_eq!(cards[landed[0].0].id, "flight-mini-map");
    }

    #[test]
    fn saved_layouts_gain_flight_cards_once_and_get_ids() {
        let mut saved: Vec<Card> = defaults().into_iter().filter(|c| c.section != "Flight").collect();
        for card in &mut saved { card.id.clear(); }
        saved[1] = saved[0].clone();
        ensure_ids(&mut saved);
        let mut ids: Vec<_> = saved.iter().map(|c| c.id.clone()).collect();
        ids.dedup();
        assert_eq!(ids.len(), saved.len());
        migrate_flight_cards(&mut saved);
        let len = saved.len();
        migrate_flight_cards(&mut saved);
        assert_eq!(saved.len(), len);
        assert!(saved.iter().any(|c| c.id == "flight-mini-map" && !c.visible));
        assert!(valid(&saved));
        let mut full = vec![defaults()[0].clone(); 32];
        ensure_ids(&mut full);
        migrate_flight_cards(&mut full);
        assert_eq!(full.len(), 32);
    }

    #[test]
    fn map_and_maxima_cards_cannot_be_pinned() {
        let mut cards = defaults();
        let map = cards.iter().position(|c| c.display == "map").unwrap();
        cards[map].pinned = true;
        assert!(!valid(&cards));
    }
    #[test]
    fn test_fire_formats_thrust_without_hiding_dashboard_cards() {
        let thrust=display_card(defaults().into_iter().find(|c|c.data_type=="LOADCELL_WEIGHT_KG").unwrap(),true);
        assert_eq!(thrust.label,"Thrust");assert_eq!(thrust.unit,"N");
    }
}
