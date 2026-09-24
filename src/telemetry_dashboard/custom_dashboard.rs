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
            "number",
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
    if valid(cards) {
        if let Ok(json) = serde_json::to_string(cards) {
            persist::set_string(&storage_key(), &json);
        }
    }
}

pub(super) fn load_cards() -> Vec<Card> {
    persist::get_string(&storage_key())
        .and_then(|raw| serde_json::from_str::<Vec<Card>>(&raw).ok())
        .filter(|cards| valid(cards))
        .unwrap_or_else(defaults)
}

#[component]
pub(super) fn CustomDashboard(theme: ThemeConfig) -> Element {
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
                    div {key:"card-{index}",class:"gs26-dashboard-card","data-width":card.width.clone(),style:"min-width:0;border:1px solid #475569;border-radius:8px;padding:12px;background:{theme.panel_background_alt};",
                        if *editing.read() {
                            CardEditor {card:card.clone(),cameras:cameras.read().streams.clone(),onchange:move |next:Card|{let mut all=cards.read().clone();all[index]=next;if valid(&all){save(&all);cards.set(all);}}}
                            button {disabled:index==0,onclick:move |_|{let mut all=cards.read().clone();all.swap(index,index-1);save(&all);cards.set(all);},"Move up"}
                            button {disabled:index+1==snapshot.len(),onclick:move |_|{let mut all=cards.read().clone();all.swap(index,index+1);save(&all);cards.set(all);},"Move down"}
                            button {disabled:snapshot.len()==1,onclick:move |_|{let mut all=cards.read().clone();all.remove(index);save(&all);cards.set(all);},"Remove"}
                        }
                        if card.visible {
                            if card.display == "camera" { CameraCard {card:card.clone(),config:cameras.read().clone(),error:camera_error.read().clone()} }
                            else { TelemetryCard {card:card.clone()} }
                        }
                        if card.display != "camera" {
                        button { disabled: !card.pinned && snapshot.iter().filter(|c| c.pinned).count() >= 8,
                            onclick: move |_| { let mut all=cards.read().clone(); all[index].pinned=!all[index].pinned; save(&all); cards.set(all); },
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
        strong {"{card.label}"}
        if !error.is_empty() {p {role:"status","{error}"}}
        else if !config.can_preview_live {p {"Live cameras require operator or stream-management access."}}
        else if card.stream_id.is_empty() {p {"Choose a camera source in Customize dashboard."}}
        else if let Some(feed) = feed {
            iframe {src:super::live_stream_tab::media_url(&feed.url),title:card.label.clone(),allow:"autoplay; fullscreen",style:"display:block;width:100%;aspect-ratio:16 / 9;min-height:180px;border:0;margin-top:8px;background:#080d15;", "sandbox":"allow-scripts allow-same-origin"}
        } else {p {role:"status","Camera offline — waiting for {card.stream_id}"}}
    }}
}

#[component]
fn CardEditor(card: Card, cameras: Vec<super::live_stream_tab::LiveStreamSpec>, onchange: EventHandler<Card>) -> Element {
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
        label {"Data type " input {value:card.data_type.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.data_type=e.value();onchange.call(c);}}}}
        label {"Board ID (blank = any) " input {value:card.sender.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.sender=e.value();onchange.call(c);}}}}
        label {"Channel index " input {r#type:"number",min:"0",max:"127",value:"{card.index}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(v)=e.value().parse(){let mut c=card.clone();c.index=v;onchange.call(c);}}}}}
        label {"Unit " input {value:card.unit.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.unit=e.value();onchange.call(c);}}}}
        label {"Decimal places " input {r#type:"number",min:"0",max:"6",value:"{card.precision}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(v)=e.value().parse(){let mut c=card.clone();c.precision=v;onchange.call(c);}}}}}
        label {"Minimum " input {r#type:"number",value:"{card.min}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(v)=e.value().parse(){let mut c=card.clone();c.min=v;onchange.call(c);}}}}}
        label {"Maximum " input {r#type:"number",value:"{card.max}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(v)=e.value().parse(){let mut c=card.clone();c.max=v;onchange.call(c);}}}}}
        }
    }}
}

#[component]
fn TelemetryCard(card: Card, #[props(default = false)] compact: bool) -> Element {
    let mut clock = use_signal(|| 0u64);
    // Sampling the existing ingress cache at 5 Hz avoids a DOM update for every
    // DAQ packet. No polling request, extra telemetry subscription or unbounded history.
    use_future(move || async move {
        loop {
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(200).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let next = *clock.read() + 1;
            clock.set(next);
        }
    });
    let _ = *clock.read();
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
        .filter(|v| v.is_finite());
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
        .unwrap_or_else(|| "No data".into());
    let fraction = value
        .map(|v| ((v - card.min) / (card.max - card.min)).clamp(0.0, 1.0))
        .unwrap_or(0.0);
    let history =
        use_hook(|| std::rc::Rc::new(std::cell::RefCell::new((None::<i64>, Vec::<f32>::new()))));
    let points = {
        let mut h = history.borrow_mut();
        if let (Some(row), Some(v)) = (&row, value) {
            if h.0 != Some(row.received_timestamp_ms) {
                h.0 = Some(row.received_timestamp_ms);
                h.1.push(v);
                if h.1.len() > 150 {
                    h.1.remove(0);
                }
            }
        }
        h.1.iter()
            .enumerate()
            .map(|(i, v)| {
                format!(
                    "{},{}",
                    i as f32 * 260.0 / 149.0,
                    60.0 - ((v - card.min) / (card.max - card.min)).clamp(0.0, 1.0) * 60.0
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    rsx! {div { style: if compact { "display:flex;align-items:baseline;justify-content:center;gap:6px;flex-wrap:wrap;" } else { "" },
        strong {"{card.label}"}
        div {style:if compact {"font-size:13px;font-variant-numeric:tabular-nums;"} else {"font-size:24px;font-variant-numeric:tabular-nums;"},"{text}"}
        if !compact && value.is_some() && card.display=="bar" {progress {value:fraction as f64,max:1,style:"width:100%;height:20px;"}}
        if !compact && value.is_some() && card.display=="gauge" {div {role:"img","aria-label":text.clone(),style:"width:70px;height:70px;border-radius:50%;background:conic-gradient(#38bdf8 {fraction*360.0}deg,#334155 0);",div {style:"position:relative;top:12px;left:12px;width:46px;height:46px;border-radius:50%;background:#0f172a;"}}}
        if !compact && card.display=="trend" {svg {view_box:"0 0 260 60",style:"width:100%;height:60px;",polyline {points,fill:"none",stroke:"#38bdf8",stroke_width:"2"}}}
        if !compact || age.is_none_or(|a| a>10000) { small {style:if age.is_none_or(|a|a>10000){"color:#fbbf24"}else{"color:#94a3b8"},
            if let Some(ms)=age {if ms>10000 {"Stale · "} "{ms/1000}s ago · {card.sender}"}else{"Waiting for telemetry"}
        }}
    }}
}

#[component]
pub(super) fn PinnedTelemetry() -> Element {
    let cards = use_context::<Signal<Vec<Card>>>();
    rsx! {
        if cards.read().iter().any(|card| card.pinned) {
            div { style:"grid-column:1 / -1;grid-row:5;display:flex;flex-wrap:wrap;justify-content:center;align-items:center;text-align:center;gap:8px 16px;padding:6px 0;font-size:12px;",
                "aria-label":"Pinned telemetry",
                for (index, card) in cards.read().iter().enumerate().filter(|(_,card)|card.pinned) {
                    TelemetryCard {key:"pinned-{index}", card:card.clone(), compact:true}
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
