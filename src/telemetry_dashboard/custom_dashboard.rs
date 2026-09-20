use super::{latest_telemetry_row, layout::ThemeConfig, persist};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Card {
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
    !cards.is_empty()
        && cards.len() <= 32
        && cards.iter().all(|c| {
            ["Fill System", "Avionics"].contains(&c.section.as_str())
                && ["number", "bar", "gauge", "trend", "state"].contains(&c.display.as_str())
                && c.min.is_finite()
                && c.max.is_finite()
                && c.max > c.min
                && c.index < 128
                && !c.data_type.trim().is_empty()
                && c.data_type.len() < 128
                && c.label.len() < 128
                && c.sender.len() < 128
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

#[component]
pub(super) fn CustomDashboard(theme: ThemeConfig) -> Element {
    let mut cards = use_signal(|| {
        persist::get_string(&storage_key())
            .and_then(|raw| serde_json::from_str::<Vec<Card>>(&raw).ok())
            .filter(|cards| valid(cards))
            .unwrap_or_else(defaults)
    });
    let mut editing = use_signal(|| false);
    let snapshot = cards.read().clone();
    rsx! {section {style:"color:{theme.text_primary};",
        div {style:"display:flex;gap:10px;align-items:center;flex-wrap:wrap;",
            h2 {"My dashboard"}
            button {onclick:move |_| {let next=!*editing.read();editing.set(next);}, if *editing.read(){"Done"}else{"Customize dashboard"}}
            if *editing.read() {
                button {onclick:move |_|{let next=defaults();save(&next);cards.set(next);},"Restore defaults"}
                button {disabled:snapshot.len()>=32,onclick:move |_|{let mut next=cards.read().clone();let mut card=defaults()[0].clone();card.label="New telemetry card".into();next.push(card);save(&next);cards.set(next);},"Add card"}
            }
        }
        if *editing.read() {p {"Saved for this user and GroundStation on this device. Choose a source, display, visibility and order. Changes do not affect acquisition or safety controls."}}
        for section in ["Fill System","Avionics"] {
            h3 {"{section}"}
            div {style:"display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:10px;",
                for (index,card) in snapshot.iter().enumerate().filter(|(_,c)|c.section==section && (c.visible || *editing.read())) {
                    div {key:"{index}-{card.data_type}-{card.sender}-{card.index}",style:"min-width:0;border:1px solid #475569;border-radius:8px;padding:12px;background:{theme.panel_background_alt};",
                        if *editing.read() {
                            CardEditor {card:card.clone(),onchange:move |next:Card|{let mut all=cards.read().clone();all[index]=next;if valid(&all){save(&all);cards.set(all);}}}
                            button {disabled:index==0,onclick:move |_|{let mut all=cards.read().clone();all.swap(index,index-1);save(&all);cards.set(all);},"Move up"}
                            button {disabled:index+1==snapshot.len(),onclick:move |_|{let mut all=cards.read().clone();all.swap(index,index+1);save(&all);cards.set(all);},"Move down"}
                            button {disabled:snapshot.len()==1,onclick:move |_|{let mut all=cards.read().clone();all.remove(index);save(&all);cards.set(all);},"Remove"}
                        }
                        if card.visible {TelemetryCard {card:card.clone()}}
                    }
                }
            }
        }
    }}
}

#[component]
fn CardEditor(card: Card, onchange: EventHandler<Card>) -> Element {
    rsx! {div {style:"display:grid;gap:6px;margin-bottom:10px;",
        label {"Show " input {r#type:"checkbox",checked:card.visible,onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.visible=e.checked();onchange.call(c);}}}}
        label {"Title " input {value:card.label.clone(),oninput:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.label=e.value();onchange.call(c);}}}}
        label {"Section " select {value:card.section.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.section=e.value();onchange.call(c);}},option {"Fill System"}option {"Avionics"}}}
        label {"Display " select {value:card.display.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.display=e.value();onchange.call(c);}},
            for kind in ["number","bar","gauge","trend","state"] {option {value:kind,"{kind}"}}
        }}
        label {"Data type " input {value:card.data_type.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.data_type=e.value();onchange.call(c);}}}}
        label {"Board ID (blank = any) " input {value:card.sender.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.sender=e.value();onchange.call(c);}}}}
        label {"Channel index " input {r#type:"number",min:"0",max:"127",value:"{card.index}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(v)=e.value().parse(){let mut c=card.clone();c.index=v;onchange.call(c);}}}}}
        label {"Unit " input {value:card.unit.clone(),onchange:{let card=card.clone();move |e:Event<FormData>|{let mut c=card.clone();c.unit=e.value();onchange.call(c);}}}}
        label {"Minimum " input {r#type:"number",value:"{card.min}",onchange:{let card=card.clone();move |e:Event<FormData>|{if let Ok(v)=e.value().parse(){let mut c=card.clone();c.min=v;onchange.call(c);}}}}}
        label {"Maximum " input {r#type:"number",value:"{card.max}",onchange:move |e:Event<FormData>|{if let Ok(v)=e.value().parse(){let mut c=card.clone();c.max=v;onchange.call(c);}}}}
    }}
}

#[component]
fn TelemetryCard(card: Card) -> Element {
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
                format!("{v:.2} {}", card.unit)
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
    rsx! {div {
        strong {"{card.label}"}
        div {style:"font-size:24px;font-variant-numeric:tabular-nums;","{text}"}
        if value.is_some() && card.display=="bar" {progress {value:fraction as f64,max:1,style:"width:100%;height:20px;"}}
        if value.is_some() && card.display=="gauge" {div {role:"img","aria-label":text.clone(),style:"width:70px;height:70px;border-radius:50%;background:conic-gradient(#38bdf8 {fraction*360.0}deg,#334155 0);",div {style:"position:relative;top:12px;left:12px;width:46px;height:46px;border-radius:50%;background:#0f172a;"}}}
        if card.display=="trend" {svg {view_box:"0 0 260 60",style:"width:100%;height:60px;",polyline {points,fill:"none",stroke:"#38bdf8",stroke_width:"2"}}}
        small {style:if age.is_none_or(|a|a>10000){"color:#fbbf24"}else{"color:#94a3b8"},
            if let Some(ms)=age {if ms>10000 {"Stale · "} "{ms/1000}s ago · {card.sender}"}else{"Waiting for telemetry"}
        }
    }}
}

#[cfg(test)]
mod tests {
    use super::*;
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
