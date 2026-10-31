use super::{latest_telemetry_row, layout::ThemeConfig, persist};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

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
        if !id.is_empty() { types.insert(id.to_string(), label.to_string()); }
    };
    for tab in &layout.tabs {
        if !tab.channels.is_empty() || tab.subtabs.as_ref().is_none_or(Vec::is_empty) {
            insert(&tab.id, &tab.label);
        }
        for group in tab.chart_groups.iter().flatten() {
            if let Some(id) = &group.data_type { insert(id, group.title.as_deref().unwrap_or(id)); }
        }
        for subtab in tab.subtabs.iter().flatten() {
            let id = subtab.data_type.as_deref().unwrap_or(&tab.id);
            insert(id, &subtab.label);
            for group in subtab.chart_groups.iter().flatten() {
                if let Some(id) = &group.data_type { insert(id, group.title.as_deref().unwrap_or(id)); }
            }
            for item in subtab.summary_items.iter().flatten() { insert(&item.data_type, &item.label); }
        }
    }
    types.into_iter().map(|(id, label)| {
        let text = if id == label { id.clone() } else { format!("{label} ({id})") };
        (id, text)
    }).collect()
}

fn display_card(mut card: Card, test_fire: bool) -> Card {
    if test_fire && card.data_type == "LOADCELL_WEIGHT_KG" {
        card.label = "Thrust".into(); card.unit="N".into(); card.max *= 9.80665; card.min *= 9.80665;
    }
    card
}

#[component]
pub(super) fn CustomDashboard(theme: ThemeConfig, data_layout: super::layout::DataTabLayout, #[props(default)] test_fire_mode: bool) -> Element {
    let mut cards = use_context::<Signal<Vec<Card>>>();
    let mut editing = use_signal(|| false);
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
        let _ = *super::HEADER_CLOCK_TICK.read();
        let observed = super::LATEST_TELEMETRY_BY_TYPE.lock().map(|latest| {
            latest.keys().map(|id| super::resolve_telemetry_text(*id).to_string()).collect::<Vec<_>>()
        }).unwrap_or_default();
        data_type_choices(&data_layout, &snapshot, observed)
    } else { Vec::new() };
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
            button {onclick:move |_| {let next=!*editing.read();editing.set(next);}, if *editing.read(){"Done"}else{"Customize dashboard"}}
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
                            CardEditor {card:card.clone(),type_choices:type_choices.clone(),cameras:cameras.read().streams.clone(),onchange:move |next:Card|{let mut all=cards.read().clone();all[index]=next;if valid(&all){save(&all);cards.set(all);}}}
                            button {disabled:index==0,onclick:move |_|{let mut all=cards.read().clone();all.swap(index,index-1);save(&all);cards.set(all);},"Move up"}
                            button {disabled:index+1==snapshot.len(),onclick:move |_|{let mut all=cards.read().clone();all.swap(index,index+1);save(&all);cards.set(all);},"Move down"}
                            button {disabled:snapshot.len()==1,onclick:move |_|{let mut all=cards.read().clone();all.remove(index);save(&all);cards.set(all);},"Remove"}
                        }
                        if card.visible {
                            if card.display == "camera" { CameraCard {card:card.clone(),config:cameras.read().clone(),error:camera_error.read().clone()} }
                            else { TelemetryCard {card:display_card(card.clone(),test_fire_mode), thrust:test_fire_mode && card.data_type=="LOADCELL_WEIGHT_KG"} }
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
fn CardEditor(card: Card, type_choices: Vec<(String, String)>, cameras: Vec<super::live_stream_tab::LiveStreamSpec>, onchange: EventHandler<Card>) -> Element {
    rsx! {div {style:"display:grid;gap:6px;margin-bottom:10px;",
        label {"Show " input {r#type:"checkbox",checked:card.visible,onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.visible=e.checked();onchange.call(c);}}}}
        label {"Title " input {value:card.label.clone(),oninput:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.label=e.value();onchange.call(c);}}}}
        label {"Section " input {value:card.section.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.section=e.value();onchange.call(c);}}}}
        label {"Display " select {value:card.display.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.display=e.value();if c.display=="camera" {c.pinned=false;}onchange.call(c);}},
            for kind in ["number","bar","gauge","trend","state","camera"] {option {value:kind,"{kind}"}}
        }}
        label {"Card width " select {value:card.width.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.width=e.value();onchange.call(c);}},option {value:"normal","Normal"} option {value:"wide","Wide"} option {value:"full","Full width"}}}
        if card.display == "camera" {
            label {"Camera source " select {value:card.stream_id.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.stream_id=e.value();onchange.call(c);}},
                option {value:"","Choose a camera"}
                if !card.stream_id.is_empty() && !cameras.iter().any(|c|c.id==card.stream_id) {option {value:card.stream_id.clone(),"{card.stream_id} (unavailable)"}}
                for camera in cameras {option {value:camera.id.clone(),"{camera.label}"}}
            }}
        } else {
        label {"Data type " select {aria_label:"Data type",style:"min-height:44px;",value:card.data_type.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.data_type=e.value();onchange.call(c);}},
            for (id, label) in type_choices {option {key:"{id}",value:id,"{label}"}}
        }}
        label {"Board ID (blank = any) " input {value:card.sender.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.sender=e.value();onchange.call(c);}}}}
        label {"Channel index " input {r#type:"number",min:"0",max:"127",value:"{card.index}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(v)=e.value().parse(){let mut c=card.clone();c.index=v;onchange.call(c);}}}}}
        label {"Unit " input {value:card.unit.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.unit=e.value();onchange.call(c);}}}}
        label {"Decimal places " input {r#type:"number",min:"0",max:"6",value:"{card.precision}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(v)=e.value().parse(){let mut c=card.clone();c.precision=v;onchange.call(c);}}}}}
        label {"Minimum " input {r#type:"number",value:"{card.min}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(v)=e.value().parse(){let mut c=card.clone();c.min=v;onchange.call(c);}}}}}
        label {"Maximum " input {r#type:"number",value:"{card.max}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(v)=e.value().parse(){let mut c=card.clone();c.max=v;onchange.call(c);}}}}}
        }
    }}
}

#[derive(Default)]
struct TrendHistory {
    source: Option<(String, String, usize, bool, u64)>,
    samples: Vec<(i64, f32)>,
}
impl TrendHistory {
    fn select(&mut self, (kind, sender, index, thrust, clear_epoch): (&str, &str, usize, bool, u64)) {
        let next = (kind.to_owned(), sender.to_owned(), index, thrust, clear_epoch);
        if self.source.as_ref() != Some(&next) {
            self.source = Some(next);
            self.samples.clear();
        }
    }
    fn record(&mut self, timestamp: i64, value: f32) {
        if !value.is_finite() { return; }
        match self.samples.binary_search_by_key(&timestamp, |sample| sample.0) {
            Ok(index) => self.samples[index].1 = value,
            Err(index) => self.samples.insert(index, (timestamp, value)),
        }
        if self.samples.len() > 150 { self.samples.remove(0); }
    }
    fn measured_range(&self) -> Option<(f32, f32)> {
        let low = self
            .samples
            .iter()
            .map(|(_, value)| *value)
            .reduce(f32::min)?;
        let high = self
            .samples
            .iter()
            .map(|(_, value)| *value)
            .reduce(f32::max)?;
        // Preserve small raw changes and negative tare offsets. A fixed
        // 0..1000 kg plot otherwise turns every negative value into one line.
        let padding = ((high - low) * 0.05)
            .max(high.abs().max(low.abs()) * 0.001)
            .max(0.000001);
        Some((low - padding, high + padding))
    }
    fn points(&self, min: f32, max: f32) -> String {
        let first = self.samples.first().map_or(0, |sample| sample.0);
        let span = self.samples.last().map_or(1, |sample| sample.0.saturating_sub(first).max(1));
        self.samples.iter().map(|&(timestamp, value)| format!("{},{}",
            timestamp.saturating_sub(first) as f64 * 260.0 / span as f64,
            60.0 - ((value-min)/(max-min)).clamp(0.0,1.0)*60.0,
        )).collect::<Vec<_>>().join(" ")
    }
}
#[cfg(test)]
mod trend_tests {
    use super::TrendHistory;
    #[test]
    fn measured_loadcell_range_shows_negative_and_small_raw_changes() {
        let mut history = TrendHistory::default();
        assert!(history.measured_range().is_none());
        for (timestamp, value) in [(1, -5.0), (2, -4.0), (3, -3.0)] {
            history.record(timestamp, value);
        }
        let (low, high) = history.measured_range().unwrap();
        assert!(low < -5.0 && high > -3.0);
        let points = history.points(low, high);
        let ys: Vec<f32> = points
            .split_whitespace()
            .map(|point| point.split_once(',').unwrap().1.parse().unwrap())
            .collect();
        assert!(ys[0] > ys[1] && ys[1] > ys[2]);
        assert!(ys.iter().all(|value| (0.0..=60.0).contains(value)));
        history.samples.clear();
        history.record(1, 0.0117);
        history.record(2, 0.0118);
        let (low, high) = history.measured_range().unwrap();
        assert!(high - low < 0.001);
        history.samples.clear();
        history.record(1, 0.0);
        let (low, high) = history.measured_range().unwrap();
        assert!(low.is_finite() && high.is_finite() && high > low);
    }
    #[test]
    fn source_time_order_and_bounded_history_are_independent_of_refresh_ticks() {
        let mut history=TrendHistory::default();history.select(("X","FC",0,false,0));
        history.record(100,1.0);history.record(300,3.0);history.record(200,2.0);history.record(300,4.0);history.record(400,f32::NAN);
        assert_eq!(history.samples,vec![(100,1.0),(200,2.0),(300,4.0)]);
        assert!(history.points(0.0,10.0).contains("130,"));
        for n in 0..200 { history.record(1000+n,1.0); }
        assert_eq!(history.samples.len(),150);
        history.select(("X","PB",0,false,0));assert!(history.samples.is_empty());
    }
}

#[component]
fn TelemetryCard(card: Card, #[props(default)] thrust: bool, #[props(default = false)] compact: bool) -> Element {
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
    let value = row
        .as_ref()
        .and_then(|r| r.values.get(card.index).copied().flatten())
        .filter(|v| v.is_finite()).map(|v|if thrust {v*9.80665}else{v});
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
    let fraction = value
        .map(|v| ((v - card.min) / (card.max - card.min)).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let clear_epoch = *super::FRONTEND_DATA_CLEAR_EPOCH.read();
    let history = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(TrendHistory::default())));
    let points = {
        let mut history = history.borrow_mut();
        history.select((&card.data_type, &card.sender, card.index, thrust, clear_epoch));
        if card.display == "trend" && !compact && let (Some(row), Some(value)) = (&row, value) {
            history.record(row.timestamp_ms, value);
        }
        let range = if matches!(
            card.data_type.as_str(),
            "KG1000" | "KG50" | "LOADCELL_WEIGHT_KG" | "LOADCELL_50_WEIGHT_KG"
        ) {
            history.measured_range()
        } else {
            None
        };
        let (min, max) = range.unwrap_or((card.min, card.max));
        history.points(min, max)
    };
    let has_trend = history.borrow().samples.len() >= 2;
    rsx! {div { style: if compact { "display:flex;align-items:baseline;justify-content:center;gap:6px;flex-wrap:wrap;" } else { "" },
        strong {class:"gs26-card-label","{card.label}"}
        div {class:"gs26-card-value",style:if compact {"font-size:13px;font-variant-numeric:tabular-nums;"} else {"font-size:24px;font-variant-numeric:tabular-nums;"},"{text}"}
        if !compact && value.is_some() && card.display=="bar" {progress {value:fraction as f64,max:1,style:"width:100%;height:20px;"}}
        if !compact && value.is_some() && card.display=="gauge" {div {role:"img","aria-label":text.clone(),style:"width:70px;height:70px;border-radius:50%;background:conic-gradient(#38bdf8 {fraction*360.0}deg,#334155 0);",div {style:"position:relative;top:12px;left:12px;width:46px;height:46px;border-radius:50%;background:#0f172a;"}}}
        if !compact && value.is_some() && has_trend && card.display=="trend" {svg {view_box:"0 0 260 60",style:"width:100%;height:60px;",polyline {points,fill:"none",stroke:"#38bdf8",stroke_width:"2"}}}
        if !compact || age.is_none_or(|a| a>10000) { small {class:"gs26-card-freshness",style:if age.is_none_or(|a|a>10000){"color:#fbbf24"}else{"color:#94a3b8"},
            if let Some(ms)=age {if ms>10000 {"Stale · "} "{ms/1000}s ago" if !card.sender.is_empty() {" · {card.sender}"}}else{"Waiting for telemetry"}
        }}
    }}
}

#[component]
pub(super) fn PinnedTelemetry(#[props(default)] test_fire_mode: bool) -> Element {
    let cards = use_context::<Signal<Vec<Card>>>();
    rsx! {
        if cards.read().iter().any(|card| card.pinned) {
            div { style:"grid-column:1 / -1;grid-row:5;display:flex;flex-wrap:wrap;justify-content:center;align-items:center;text-align:center;gap:8px 16px;padding:6px 0;font-size:12px;",
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
