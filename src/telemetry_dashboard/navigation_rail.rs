//! Navigation shared by the operational views; every item comes from the live layout.
use dioxus::prelude::*;
#[derive(Clone, PartialEq)]
pub(super) struct NavigationItem {
    pub id: String,
    pub label: String,
}
fn group(id: &str) -> &'static str {
    match id {
        "state" | "actions" | "mission" | "map" | "vehicle" => "Mission",
        "data" | "connection-status" | "detailed" | "network-topology" => "Telemetry",
        "crew-voice" | "media" | "stream-manager" => "Collaboration",
        "messages" | "notifications" | "warnings" | "errors" => "Activity",
        _ => "Management",
    }
}
fn icon(id: &str) -> &'static str {
    match id {
        "state" => "M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z",
        "map" => "M3 5l6-2 6 2 6-2v16l-6 2-6-2-6 2z M9 3v16 M15 5v16",
        "data" => "M3 3v18h18 M5 16l4-7 4 5 6-9",
        "actions" => "M4 6h16 M4 12h16 M4 18h16 M8 3v6 M16 9v6 M10 15v6",
        "crew-voice" => {
            "M9 5a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0z M6 10v2a6 6 0 0 0 12 0v-2 M12 18v4 M8 22h8"
        }
        "media" | "mission" | "stream-manager" => "M3 5h13v14H3z M16 9l5-3v12l-5-3",
        "connection-status" | "network-topology" => {
            "M3 3h6v6H3z M15 15h6v6h-6z M15 3h6v6h-6z M9 6h6 M6 9v9h9"
        }
        "detailed" => "M12 3l9 5v8l-9 5-9-5V8z M12 11v6 M12 7v1",
        "warnings" | "errors" | "notifications" => "M12 3l10 18H2z M12 9v5 M12 17v1",
        _ => "M4 5h16v14H4z M8 9h8 M8 13h8 M8 17h5",
    }
}
#[component]
pub(super) fn NavigationRail(
    items: Vec<NavigationItem>,
    active: String,
    connected: bool,
    title: String,
    on_navigate: EventHandler<String>,
    on_settings: EventHandler<()>,
) -> Element {
    rsx! {
     style {{include_str!("navigation_rail.css")}}
     aside {class:"gs26-navigation-rail",
      div {class:"gs26-navigation-brand",span {class:"gs26-navigation-mark","S"} div {strong {"{title}"} small {"Mission control"}}}
      nav {"aria-label":"Mission navigation",
       for category in ["Mission","Telemetry","Collaboration","Activity","Management"] {
        if items.iter().any(|item|group(&item.id)==category) {
         div {class:"gs26-navigation-group",p {"{category}"}
          for item in items.iter().filter(|item|group(&item.id)==category) {
           {let id=item.id.clone();rsx! {
            a {href:"#","aria-current":if active==id {"page"}else{"false"},onclick:move |e|{e.prevent_default();on_navigate.call(id.clone());},
             svg {view_box:"0 0 24 24","aria-hidden":"true",path {d:icon(&item.id)}} span {"{item.label}"}
            }
           }}
          }
         }
        }
       }
      }
      footer {span {class:"gs26-navigation-connection","data-connected":connected.to_string(),i {} if connected {"Station connected"}else{"Reconnecting"}}
       a {href:"#",onclick:move|e|{e.prevent_default();on_settings.call(());},"Settings"}
      }
     }
    }
}
