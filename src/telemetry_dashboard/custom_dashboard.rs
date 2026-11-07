use super::{latest_telemetry_row, layout::ThemeConfig, persist};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
#[path = "dashboard_card_values.rs"]
mod card_values;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Card {
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
    cards
}
fn valid(cards: &[Card]) -> bool {
    cards.iter().filter(|c| c.pinned).count() <= 8
        && !cards.is_empty()
        && cards.len() <= 32
        && cards.iter().all(|c| {
            !c.section.trim().is_empty() && c.section.len() < 64 && c.precision <= 6
                && ["number", "bar", "gauge", "trend", "state", "camera"].contains(&c.display.as_str())
                && ["normal", "wide", "full"].contains(&c.width.as_str())
                && c.stream_id.len() <= 64
                && (c.display != "camera" || !c.pinned)
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
            persist::set_string(&format!("{}_defaults_version", storage_key()), "3");
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
    if version.as_deref() != Some("3") {
        // Version2 already migrated the loadcell. Preserve later explicit choices.
        if version.as_deref() != Some("2") {
            migrate_default_loadcell_trend(&mut cards);
        }
        migrate_default_pressure_cards(&mut cards);
        save(&cards);
    }
    cards
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
    let mut sections = Vec::new();
    for card in &snapshot { if (card.visible || *editing.read()) && !sections.contains(&card.section) { sections.push(card.section.clone()); } }
    rsx! {section {class:"gs26-custom-dashboard",style:"color:{theme.text_primary};--card-button-bg:{theme.button_background};--card-button-border:{theme.button_border};--card-button-text:{theme.button_text};",
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
            if *editing.read() {
                button {onclick:move |_|{let next=defaults();save(&next);cards.set(next);},"Restore defaults"}
                button {disabled:snapshot.len()>=32,onclick:move |_|{let mut next=cards.read().clone();let mut card=defaults()[0].clone();card.label="New telemetry card".into();next.push(card);save(&next);cards.set(next);},"Add card"}
                button {disabled:snapshot.len()>=32,onclick:move |_|{let mut next=cards.read().clone();let mut card=defaults()[0].clone();card.label="Live camera".into();card.section="Cameras".into();card.display="camera".into();card.width="wide".into();next.push(card);save(&next);cards.set(next);},"Add camera"}
            }
        }
        if *editing.read() {p {"Saved for this user and GroundStation on this device. Choose a source, display, visibility and order. Changes do not affect acquisition or safety controls."}}
        for section in sections {
            h3 {"{section}"}
            div {class:"gs26-card-grid",
                for (index,card) in snapshot.iter().enumerate().filter(|(_,c)|c.section==section && (c.visible || *editing.read())) {
                    div {key:"card-{index}",class:"gs26-dashboard-card","data-width":card.width.clone(),"data-display":card.display.clone(),style:"min-width:0;border:1px solid #475569;border-radius:8px;padding:12px;background:{theme.panel_background_alt};",
                        if *editing.read() {
                            CardEditor {card:card.clone(),data_layout:data_layout.clone(),section_choices:section_choices.clone(),sender_choices:sender_choices.clone(),unit_choices:unit_choices.clone(),type_choices:type_choices.clone(),cameras:cameras.read().streams.clone(),onchange:move |next:Card|{let mut all=cards.read().clone();all[index]=next;if valid(&all){save(&all);cards.set(all);}}}
                            button {disabled:index==0,onclick:move |_|{let mut all=cards.read().clone();all.swap(index,index-1);save(&all);cards.set(all);},"Move up"}
                            button {disabled:index+1==snapshot.len(),onclick:move |_|{let mut all=cards.read().clone();all.swap(index,index+1);save(&all);cards.set(all);},"Move down"}
                            button {disabled:snapshot.len()==1,onclick:move |_|{let mut all=cards.read().clone();all.remove(index);save(&all);cards.set(all);},"Remove"}
                        }
                        if card.visible {
                            if card.display == "camera" { CameraCard {card:card.clone(),config:cameras.read().clone(),error:camera_error.read().clone()} }
                            else { TelemetryCard {card:display_card(card.clone(),test_fire_mode), aggregate:*aggregate.read(), thrust:test_fire_mode && card.data_type=="LOADCELL_WEIGHT_KG", show_min_max:extrema_policies.read().get(index).copied().unwrap_or(card.display=="trend")} }
                        }
                        if *editing.read() && card.display != "camera" {
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
        label {"Display " select {aria_label:"Display",value:card.display.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.display=e.value();if c.display=="camera" {c.pinned=false;}onchange.call(c);}},
            for kind in ["number","bar","gauge","trend","state","camera"] {option {selected:kind==card.display,value:kind,"{kind}"}}
        }}
        label {"Card width " select {aria_label:"Card width",value:card.width.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.width=e.value();onchange.call(c);}},option {selected:card.width=="normal",value:"normal","Normal"} option {selected:card.width=="wide",value:"wide","Wide"} option {selected:card.width=="full",value:"full","Full width"}}}
        if card.display == "camera" {
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
            for (index, card) in snapshot.iter().enumerate().filter(|(_,card)|card.display != "camera") {
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
    fn test_fire_formats_thrust_without_hiding_dashboard_cards() {
        let thrust=display_card(defaults().into_iter().find(|c|c.data_type=="LOADCELL_WEIGHT_KG").unwrap(),true);
        assert_eq!(thrust.label,"Thrust");assert_eq!(thrust.unit,"N");
    }
}
