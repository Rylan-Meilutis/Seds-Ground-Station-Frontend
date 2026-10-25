//! Disconnected native onboarding. This component owns only local signals: it
//! never mounts the live dashboard, creates transports, or writes station state.
use super::{
    layout::ThemeConfig,
    navigation_rail::{NavigationItem, NavigationRail},
};
use dioxus::prelude::*;

const PAGES: &[(&str, &str)] = &[
    ("state", "Dashboard"),
    ("mission", "Mission"),
    ("data", "Data"),
    ("network-topology", "Network"),
    ("map", "Map"),
    ("checklists", "Checklists"),
    ("crew-voice", "Voice Chat"),
    ("stream-manager", "Stream Control"),
];
const CHECKS: &[&str] = &[
    "Pressure transducer reading checked",
    "Gas lines and connections inspected",
    "Communications and abort procedure checked",
];
fn known_page(id: &str) -> &str {
    if id == "settings" || PAGES.iter().any(|(key, _)| *key == id) {
        id
    } else {
        "state"
    }
}
fn title(id: &str) -> &str {
    PAGES
        .iter()
        .find_map(|(key, label)| (*key == id).then_some(*label))
        .unwrap_or("Settings")
}
fn readings(test_fire: bool) -> Vec<(&'static str, &'static str, &'static str)> {
    let mut rows = vec![
        ("50 kg loadcell", "12.40 kg", "DAQ"),
        ("Tank pressure", "0.00 psi", "VB"),
        ("Fill box battery", "16.10 V", "GB"),
        ("Valve board battery", "16.20 V", "VB"),
    ];
    if test_fire {
        rows.push(("1000 kg loadcell", "0.00 kg", "DAQ"));
    } else {
        rows.extend([
            ("Rocket battery", "16.35 V", "PB"),
            ("Rocket GPS", "Example lock · 12 satellites", "FC"),
        ]);
    }
    rows
}

#[component]
pub fn OfflinePreview(on_exit: EventHandler<()>, theme: ThemeConfig) -> Element {
    let mut active = use_signal(|| "state".to_string());
    let mut collapsed = use_signal(|| false);
    let mut sidebar = use_signal(|| true);
    let mut test_fire = use_signal(|| false);
    let mut phase = use_signal(|| "Idle".to_string());
    let mut sensor = use_signal(|| "Loadcell".to_string());
    let mut checks = use_signal(|| vec![false; CHECKS.len()]);
    let current = active();
    let items = PAGES
        .iter()
        .map(|(id, label)| NavigationItem {
            id: (*id).into(),
            label: (*label).into(),
        })
        .collect();
    let shell_style = format!(
        "--gs26-app-height:100dvh;--gs26-app-background:{};--gs26-app-text:{};--gs26-panel-background:{};--gs26-panel-alt-background:{};--gs26-border:{};--gs26-text-secondary:{};--gs26-text-muted:{};--gs-control-bg:{};--gs-control-border:{};--gs-control-text:{};",
        theme.app_background,
        theme.text_primary,
        theme.panel_background,
        theme.panel_background_alt,
        theme.border,
        theme.text_secondary,
        theme.text_muted,
        theme.button_background,
        theme.button_border,
        theme.button_text
    );
    rsx! {
        style {{include_str!("navigation_rail.css")}}
        style {{include_str!("controls.css")}}
        style {{include_str!("offline_preview.css")}}
        div {class:"gs26-offline-preview",style:shell_style,
            header {class:"gs26-preview-header",
                div {
                    strong {"Offline preview"}
                    span {"Example data · Controls disabled"}
                }
                button {r#type:"button",onclick:move |_|on_exit.call(()),"Exit preview"}
            }
            div {class:"gs26-preview-workspace","data-sidebar":sidebar().to_string(),
                if sidebar() {
                    NavigationRail {items, active:current.clone(),title:"Example station".to_string(),
                        collapsed:collapsed(),on_toggle:move |_|collapsed.toggle(),
                        on_navigate:move |id:String|active.set(known_page(&id).to_string()),
                        on_settings:move |_|active.set("settings".into()),warning_alert:false,error_alert:false}
                }
                main {class:"gs26-preview-main",
                    div {class:"gs26-preview-page-title",
                        h1 {"{title(&current)}"}
                        if !sidebar() {
                            div {class:"gs26-preview-page-picker",
                                label {r#for:"preview-page","Page"}
                                select {id:"preview-page",value:current.clone(),onchange:move|e|active.set(known_page(&e.value()).to_string()),
                                    for (id,label) in PAGES {option {value:*id,"{label}"}}
                                    option {value:"settings","Settings"}
                                }
                            }
                        }
                    }
                    p {class:"gs26-preview-note","A sample layout for learning the app. Your station’s pages and access depend on its configuration."}
                    if current == "state" {
                        div {class:"gs26-preview-status",span {"Status: Example"}span {"Flight state: {phase}"}span {"T− 00:10.00"}}
                        div {class:"gs26-preview-cards",
                            for (label,value,sender) in readings(test_fire()) {
                                article {class:"gs26-preview-card",span {"{label}"}strong {"{value}"}small {"Example · {sender}"}}
                            }
                        }
                        p {class:"gs26-preview-note","Pin the readings you need on the dashboard; follow procedure progress on Mission."}
                    } else if current == "mission" {
                        section {class:"gs26-preview-panel",
                            h2 {if test_fire() {"Test-fire procedure"}else{"Launch procedure"}}
                            div {class:"gs26-preview-options",label {r#for:"preview-phase","Example phase"}
                                select {id:"preview-phase",value:phase(),onchange:move|e|phase.set(e.value()),
                                    for name in ["Idle","Fill","Armed","Fire"] {option {value:name,"{name}"}}
                                }
                            }
                            ol {class:"gs26-preview-steps",for name in ["Idle","Fill","Armed","Fire"] {
                                li {"data-active":(phase()==name).to_string(),"{name}"}
                            }}
                            div {class:"gs26-preview-actions",for label in ["Valve self-test","Nitrogen test","Start fill","Arm","Fire","Abort"] {
                                button {r#type:"button",disabled:true,"{label}"}
                            }}
                            details {summary {"About procedure controls"}p {"A connected station checks permissions, checklist requirements, sensor freshness, topology and valve interlocks. Preview cannot qualify a procedure or operate outputs."}}
                        }
                    } else if current == "data" {
                        section {class:"gs26-preview-panel",h2 {"Sensor history"}
                            div {class:"gs26-preview-data-tabs",role:"tablist",aria_label:"Example sensor",
                                for name in ["Loadcell", "Tank pressure", "Battery voltage"] {
                                    button {r#type:"button",role:"tab",aria_selected:(sensor()==name).to_string(),
                                        class:if sensor()==name {"selected"}else{""},onclick:move |_|sensor.set(name.into()),"{name}"}
                                }
                            }
                            p {class:"gs26-preview-note","Example · {sensor}"}
                            svg {class:"gs26-preview-chart",view_box:"0 0 600 180",role:"img","aria-label":format!("Synthetic {} history",sensor().to_lowercase()),
                                path {d:"M35 12V150H585",fill:"none",stroke:"#64748b"}
                                path {d:"M35 140L75 136L115 124L155 122L195 107L235 108L275 87L315 81L355 62L395 57L435 40L475 33L515 27L555 27L585 28",fill:"none",stroke:"#67d5c8",stroke_width:"3"}
                                text {x:"40",y:"174",fill:"#94a3b8","Example time →"}
                            }
                            p {class:"gs26-preview-note","On a connected station, select a sensor and subtab to inspect its values and history. Raw telemetry is retained separately from display filters."}
                        }
                    } else if current == "network-topology" {
                        section {class:"gs26-preview-panel",h2 {"Example topology"}
                            div {class:"gs26-preview-links",span {"GroundStation"}span {"Ethernet → Gateway → CAN → DAQ / Valve / Actuator"}}
                            if !test_fire() {div {class:"gs26-preview-links",span {"GroundStation"}span {"Radio → RF → CAN → FC / Power"}}}
                            p {class:"gs26-preview-note","Detailed Info shows observed board health, disconnects and reconnects. These example routes do not indicate a real link."}
                        }
                    } else if current == "map" {
                        section {class:"gs26-preview-panel",h2 {"Recovery map"}
                            svg {class:"gs26-preview-map",view_box:"0 0 600 280",role:"img","aria-label":"Illustrative recovery map without live location",
                                defs {pattern {id:"preview-map-grid",width:"40",height:"40",pattern_units:"userSpaceOnUse",path {d:"M40 0H0V40",fill:"none",stroke:"#24364b"}}}
                                rect {width:"600",height:"280",fill:"url(#preview-map-grid)"}
                                path {d:"M90 225Q200 100 310 150T510 55",fill:"none",stroke:"#67d5c8",stroke_width:"3",stroke_dasharray:"7 5"}
                                circle {cx:"90",cy:"225",r:"8",fill:"#8bc8ff"}circle {cx:"510",cy:"55",r:"8",fill:"#67d5c8"}
                                text {x:"108",y:"230",fill:"#dbeafe","Station"}text {x:"420",y:"40",fill:"#dbeafe","Rocket"}
                            }
                            p {class:"gs26-preview-note","Illustration only. No map tiles, GPS or compass are requested. Find antenna alignment in Settings when connected."}
                        }
                    } else if current == "checklists" {
                        section {class:"gs26-preview-panel",h2 {"Practice checklist"}
                            p {class:"gs26-preview-note","These practice checks stay here and are discarded when you exit. They never complete a real station checklist."}
                            for (index,label) in CHECKS.iter().enumerate() {
                                label {class:"gs26-preview-check",input {r#type:"checkbox",checked:checks()[index],onchange:move|_|{let mut next=checks();next[index]=!next[index];checks.set(next);}}span {"{label}"}}
                            }
                            button {r#type:"button",onclick:move|_|checks.set(vec![false;CHECKS.len()]),"Reset practice checks"}
                        }
                    } else if current == "crew-voice" || current == "stream-manager" {
                        section {class:"gs26-preview-panel",h2 {if current=="crew-voice" {"Crew channels"}else{"Audience program"}}
                            p {class:"gs26-preview-note",if current=="crew-voice" {"Join a channel after connecting and granting microphone access. No microphone or audio devices are opened in preview."}else{"Stream operators can arrange cameras and replay recorded moments. Preview does not connect cameras, publish video or join a voice channel."}}
                            button {r#type:"button",disabled:true,if current=="crew-voice" {"Join channel"}else{"Open stream controls"}}
                        }
                    } else {
                        section {class:"gs26-preview-panel",h2 {"Preview preferences"}
                            label {class:"gs26-preview-check",input {r#type:"checkbox",checked:sidebar(),onchange:move|_|sidebar.toggle()}span {"Use sidebar navigation"}}
                            label {class:"gs26-preview-check",input {r#type:"checkbox",checked:test_fire(),onchange:move|_|test_fire.toggle()}span {"Show a test-fire example"}}
                            p {class:"gs26-preview-note","These choices affect this preview only. Station settings, saved layouts and credentials remain unchanged."}
                            button {r#type:"button",disabled:true,"Aim antenna"}
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_page_allowlist_and_mode_examples_are_bounded() {
        assert_eq!(known_page("https://station/api/command"), "state");
        for (id, label) in PAGES {
            assert_eq!(known_page(id), *id);
            assert_eq!(title(id), *label);
        }
        assert_eq!(known_page("settings"), "settings");
        assert!(readings(true).iter().any(|r| r.0 == "1000 kg loadcell"));
        assert!(!readings(false).iter().any(|r| r.0 == "1000 kg loadcell"));
        assert!(!readings(true).iter().any(|r| r.2 == "FC" || r.2 == "PB"));
    }
}
