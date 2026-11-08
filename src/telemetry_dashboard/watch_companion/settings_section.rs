//! Settings → Watch: choose and order watch pages and cycle items, set the
//! cycle interval, compass and power options. Edits sync to the watch and,
//! when signed in, to the account.
use super::registry;
use super::settings::{MAX_CYCLE_ITEMS, North, Power, Units, WatchSettings};
use super::{WATCH_RECORD, WATCH_SYNC, bridge, edit_settings, sleep_ms};
use crate::telemetry_dashboard::layout::ThemeConfig;
use dioxus::prelude::*;

const INTERVALS_S: &[u32] = &[2, 3, 5, 8, 10, 15, 30, 60];
const STALE_S: &[u32] = &[10, 15, 30, 60, 120, 300, 600];

fn item_label(id: &str) -> &'static str {
    registry::item(id)
        .map(|i| i.label)
        .unwrap_or("Unknown item")
}

#[component]
pub(in crate::telemetry_dashboard) fn WatchSettingsSection(theme: ThemeConfig) -> Element {
    let mut link = use_signal(bridge::link_state);
    use_future(move || async move {
        loop {
            sleep_ms(2_000).await;
            let next = bridge::link_state();
            if *link.peek() != next {
                link.set(next);
            }
        }
    });
    let settings: WatchSettings = WATCH_RECORD.read().settings.clone();
    let sync = *WATCH_SYNC.read();
    let link_text = link.read().summary();

    let card = format!(
        "padding:16px; border-radius:14px; border:1px solid {}; background:{}; display:flex; flex-direction:column; gap:10px;",
        theme.border, theme.panel_background
    );
    let row = "display:flex; align-items:center; gap:8px; flex-wrap:wrap; min-height:36px;";
    let muted = format!("color:{}; font-size:0.9rem;", theme.text_secondary);
    let button = format!(
        "padding:6px 10px; border-radius:8px; border:1px solid {}; background:{}; color:{}; cursor:pointer; min-width:36px;",
        theme.button_border, theme.button_background, theme.button_text
    );
    let select = button.clone();
    let heading = format!("margin:0; font-size:1.05rem; color:{};", theme.text_primary);

    let unselected: Vec<&registry::ItemDef> = registry::ITEMS
        .iter()
        .filter(|i| !settings.cycle.items.iter().any(|s| s == i.id))
        .collect();
    let cycle_full = settings.cycle.items.len() >= MAX_CYCLE_ITEMS;

    rsx! {
        div { style: "display:flex; flex-direction:column; gap:14px;", "data-testid": "watch-settings",
            div { style: "{card}",
                h3 { style: "{heading}", "Watch companion" }
                p { style: "{muted} margin:0;",
                    "Read-only Apple Watch and Wear OS pages, an auto-cycle page and a recovery compass. "
                    "The watch shows whatever station this phone is connected to and is marked DEMO in the offline preview. "
                    "Settings changed here or on the watch stay in sync."
                }
                div { style: "{row}", strong { "Watch: " } span { "{link_text}" } }
                div { style: "{row}", strong { "Settings: " } span { "{sync.label()}" } }
            }

            div { style: "{card}",
                h3 { style: "{heading}", "Pages" }
                p { style: "{muted} margin:0;", "Swipe order on the watch. Turn off pages you do not need." }
                for (index, page) in settings.pages.iter().enumerate() {
                    {
                        let id = page.id.clone();
                        let (id_toggle, id_up, id_down) = (id.clone(), id.clone(), id.clone());
                        let def = registry::page(&id);
                        let title = def.map(|d| d.title).unwrap_or("Unknown");
                        let detail = def
                            .map(|d| if d.items.is_empty() { d.kind.wire().to_string() } else { d.items.iter().map(|i| item_label(i)).collect::<Vec<_>>().join(", ") })
                            .unwrap_or_default();
                        let last = index + 1 == settings.pages.len();
                        rsx! {
                            div { key: "{id}", style: "{row}",
                                label { style: "display:flex; align-items:center; gap:8px; flex:1; min-width:180px;",
                                    input { r#type: "checkbox", checked: page.enabled,
                                        onchange: move |_| edit_settings(|s| s.toggle_page(&id_toggle)) }
                                    span { strong { "{title}" } span { style: "{muted}", " · {detail}" } }
                                }
                                button { style: "{button}", disabled: index == 0, "aria-label": "Move {title} earlier",
                                    onclick: move |_| edit_settings(|s| s.move_page(&id_up, true)), "↑" }
                                button { style: "{button}", disabled: last, "aria-label": "Move {title} later",
                                    onclick: move |_| edit_settings(|s| s.move_page(&id_down, false)), "↓" }
                            }
                        }
                    }
                }
            }

            div { style: "{card}",
                h3 { style: "{heading}", "Auto-cycle" }
                p { style: "{muted} margin:0;", "The cycle page shows one item at a time, in this order." }
                div { style: "{row}",
                    span { "Interval" }
                    select { style: "{select}", value: "{settings.cycle.interval_s}",
                        onchange: move |e| if let Ok(v) = e.value().parse::<u32>() { edit_settings(|s| s.cycle.interval_s = v) },
                        for v in INTERVALS_S.iter().copied() {
                            option { value: "{v}", selected: v == settings.cycle.interval_s, "{v} s" }
                        }
                    }
                    label { style: "display:flex; align-items:center; gap:6px;",
                        input { r#type: "checkbox", checked: settings.cycle.autostart,
                            onchange: move |_| edit_settings(|s| s.cycle.autostart = !s.cycle.autostart) }
                        "Start cycling when the page opens"
                    }
                }
                for (index, id) in settings.cycle.items.iter().enumerate() {
                    {
                        let (id_rm, id_up, id_down) = (id.clone(), id.clone(), id.clone());
                        let label = item_label(id);
                        let last = index + 1 == settings.cycle.items.len();
                        let only = settings.cycle.items.len() == 1;
                        rsx! {
                            div { key: "{id}", style: "{row}",
                                span { style: "flex:1; min-width:160px;", "{index + 1}. {label}" }
                                button { style: "{button}", disabled: index == 0, "aria-label": "Move {label} earlier",
                                    onclick: move |_| edit_settings(|s| s.move_cycle_item(&id_up, true)), "↑" }
                                button { style: "{button}", disabled: last, "aria-label": "Move {label} later",
                                    onclick: move |_| edit_settings(|s| s.move_cycle_item(&id_down, false)), "↓" }
                                button { style: "{button}", disabled: only, "aria-label": "Remove {label}",
                                    onclick: move |_| edit_settings(|s| s.toggle_cycle_item(&id_rm)), "Remove" }
                            }
                        }
                    }
                }
                if !unselected.is_empty() {
                    div { style: "{row}",
                        span { "Add" }
                        select { style: "{select}", disabled: cycle_full, value: "",
                            onchange: move |e| { let id = e.value(); if !id.is_empty() { edit_settings(|s| s.toggle_cycle_item(&id)) } },
                            option { value: "", if cycle_full { "Limit of {MAX_CYCLE_ITEMS} items" } else { "Choose an item…" } }
                            for item in unselected.iter() {
                                option { value: "{item.id}", "{item.group}: {item.label}" }
                            }
                        }
                    }
                }
            }

            div { style: "{card}",
                h3 { style: "{heading}", "Recovery compass" }
                p { style: "{muted} margin:0;",
                    "Bearing and distance from the watch to the rocket's latest GPS fix and to the stations. "
                    "Station targets use live station GPS, or the saved station position (labelled) when there is no fix."
                }
                div { style: "{row}",
                    span { "Bearings" }
                    select { style: "{select}",
                        onchange: move |e| { let north = if e.value() == "magnetic" { North::Magnetic } else { North::True }; edit_settings(|s| s.compass.north = north) },
                        option { value: "true", selected: settings.compass.north == North::True, "True north" }
                        option { value: "magnetic", selected: settings.compass.north == North::Magnetic, "Magnetic north" }
                    }
                    span { "Stale after" }
                    select { style: "{select}",
                        onchange: move |e| if let Ok(v) = e.value().parse::<u32>() { edit_settings(|s| s.compass.stale_after_s = v) },
                        for v in STALE_S.iter().copied() {
                            option { value: "{v}", selected: v == settings.compass.stale_after_s, "{v} s" }
                        }
                    }
                }
                div { style: "{row}",
                    for (id, label) in registry::TARGETS.iter().copied() {
                        label { key: "{id}", style: "display:flex; align-items:center; gap:6px;",
                            input { r#type: "checkbox", checked: settings.compass.targets.iter().any(|t| t == id),
                                onchange: move |_| edit_settings(|s| s.toggle_target(id)) }
                            "{label}"
                        }
                    }
                }
            }

            div { style: "{card}",
                h3 { style: "{heading}", "Display and battery" }
                div { style: "{row}",
                    span { "Units" }
                    select { style: "{select}",
                        onchange: move |e| { let units = if e.value() == "imperial" { Units::Imperial } else { Units::Metric }; edit_settings(|s| s.units = units) },
                        option { value: "metric", selected: settings.units == Units::Metric, "Metric (m, km)" }
                        option { value: "imperial", selected: settings.units == Units::Imperial, "Imperial (ft, mi)" }
                    }
                    span { "Updates" }
                    select { style: "{select}",
                        onchange: move |e| {
                            let power = match e.value().as_str() { "saver" => Power::Saver, "recovery" => Power::Recovery, _ => Power::Balanced };
                            edit_settings(|s| s.power = power)
                        },
                        option { value: "saver", selected: settings.power == Power::Saver, "Battery saver (5 s)" }
                        option { value: "balanced", selected: settings.power == Power::Balanced, "Balanced (2 s)" }
                        option { value: "recovery", selected: settings.power == Power::Recovery, "Recovery (1 s)" }
                    }
                }
                p { style: "{muted} margin:0;",
                    "Rates apply while the watch app is open; in the background the watch gets the latest state every 10–60 s."
                }
            }
        }
    }
}
