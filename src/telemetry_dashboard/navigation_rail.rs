//! Navigation shared by the operational views; every item comes from the live layout.
use dioxus::prelude::*;
#[derive(Clone, PartialEq)]
pub(super) struct NavigationItem {
    pub id: String,
    pub label: String,
}
pub(super) static SIDEBAR_NAVIGATION: GlobalSignal<bool> =
    Signal::global(|| super::persist::get_string("navigation-style").as_deref() != Some("tabs"));
fn group(id: &str) -> &'static str {
    match id {
        "state" | "actions" | "mission" | "map" | "vehicle" | "checklists" => "Mission",
        "data" | "connection-status" | "detailed" | "network-topology" => "Telemetry",
        "crew-voice" | "media" | "stream-manager" => "Collaboration",
        "messages" | "notifications" | "warnings" | "errors" => "Activity",
        _ => "Management",
    }
}
fn icon(id: &str) -> &'static str {
    match id {
        "checklists" => "M9 4h11v17H4V4h3 M9 2h6v4H9z M7 11l2 2 3-4 M14 11h3 M7 17h10",
        "state" => "M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z",
        "map" => "M3 5l6-2 6 2 6-2v16l-6 2-6-2-6 2z M9 3v16 M15 5v16",
        "data" => "M3 3v18h18 M5 16l4-7 4 5 6-9",
        "actions" => "M4 6h16 M4 12h16 M4 18h16 M8 3v6 M16 9v6 M10 15v6",
        "crew-voice" => {
            "M9 5a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0z M6 10v2a6 6 0 0 0 12 0v-2 M12 18v4 M8 22h8"
        }
        "media" => "M3 5h13v14H3z M16 9l5-3v12l-5-3",
        "network-topology" => "M3 3h6v6H3z M15 15h6v6h-6z M15 3h6v6h-6z M9 6h6 M6 9v9h9",
        "detailed" => "M12 3l9 5v8l-9 5-9-5V8z M12 11v6 M12 7v1",
        "warnings" => "M12 3l10 18H2z M12 9v5 M12 17v1",
        "mission" => "M12 2l4 7 6 3-6 3-4 7-4-7-6-3 6-3z M12 8v8 M8 12h8",
        "stream-manager" => "M3 4h18v12H3z M8 21h8 M12 16v5 M6 8h4 M14 8h4 M6 12h12",
        "connection-status" => {
            "M3 9a14 14 0 0 1 18 0 M6 13a9 9 0 0 1 12 0 M9 17a4 4 0 0 1 6 0 M12 21h.01"
        }
        "errors" => "M8 2h8l6 6v8l-6 6H8l-6-6V8z M12 7v6 M12 17h.01",
        "notifications" => "M5 17h14l-2-3V9a5 5 0 0 0-10 0v5z M10 21h4",
        "messages" => "M3 3h18v14H8l-5 4z M7 7h10 M7 11h7",
        "calibration" => "M4 4l16 16 M3 8l5-5 M7 12l5-5 M11 16l5-5 M15 20l5-5",
        "firmware-update" => "M8 3h8v5h5v8h-5v5H8v-5H3V8h5z M12 16V8 M9 11l3-3 3 3",
        "data-export" => "M4 3h10l6 6v12H4z M14 3v6h6 M12 11v7 M9 15l3 3 3-3",
        "vehicle" => "M9 16l-4 3 1-6 M15 16l4 3-1-6 M9 16h6V8l-3-6-3 6z M10 20v2 M14 20v2",
        _ => "M4 5h16v14H4z M8 9h8 M8 13h8 M8 17h5",
    }
}
#[component]
pub(super) fn NavigationRail(
    items: Vec<NavigationItem>,
    active: String,
    title: String,
    on_navigate: EventHandler<String>,
    on_settings: EventHandler<()>,
    collapsed: bool,
    on_toggle: EventHandler<()>,
    warning_alert: bool,
    error_alert: bool,
) -> Element {
    rsx! {
     aside {class:"gs26-navigation-rail","aria-label":"{title} pages", "data-collapsed":collapsed.to_string(),
      button {class:"gs26-rail-toggle",title:if collapsed {"Expand sidebar"}else{"Collapse sidebar"},"aria-label":if collapsed {"Expand sidebar"}else{"Collapse sidebar"},"aria-expanded":(!collapsed).to_string(),onclick:move |_|on_toggle.call(()),
       svg {view_box:"0 0 24 24","aria-hidden":"true",path {d:if collapsed {"M9 5l7 7-7 7"}else{"M15 5l-7 7 7 7"}}}
      }
      nav {"aria-label":"Mission navigation",
       for category in ["Mission","Telemetry","Collaboration","Activity","Management"] {
        if items.iter().any(|item|group(&item.id)==category) {
         div {class:"gs26-navigation-group",p {"{category}"}
          for item in items.iter().filter(|item|group(&item.id)==category) {
           {let id=item.id.clone();rsx! {
            a {href:"#",title:item.label.clone(),"aria-label":item.label.clone(),"data-alert":if item.id=="warnings" && warning_alert {"warning"}else if item.id=="errors" && error_alert {"error"}else{"none"},"aria-current":if active==id {"page"}else{"false"},onclick:move |e|{e.prevent_default();on_navigate.call(id.clone());},
             svg {view_box:"0 0 24 24","aria-hidden":"true",path {d:icon(&item.id)}} span {"{item.label}"}
            }
           }}
          }
         }
        }
       }
      }
      footer {
       a {href:"#",onclick:move|e|{e.prevent_default();on_settings.call(());},title:"Settings","aria-label":"Settings",svg {view_box:"0 0 24 24","aria-hidden":"true",path {d:"M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8 M9 3h6l1 3 3 1 2 5-2 5-3 1-1 3H9l-1-3-3-1-2-5 2-5 3-1z"}} span {"Settings"}}
      }
     }
    }
}
