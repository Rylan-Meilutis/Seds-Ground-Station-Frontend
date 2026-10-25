//! Browser acceptance fixture for the exact disconnected native preview component.
#![allow(dead_code)]
use dioxus::prelude::*;
#[path = "../src/telemetry_dashboard/layout.rs"]
mod layout;
#[path = "../src/telemetry_dashboard/navigation_rail.rs"]
mod navigation_rail;
#[path = "../src/telemetry_dashboard/offline_preview.rs"]
mod offline_preview;
// The shared rail declares a saved preference signal; preview does not use it.
mod persist {
    pub fn get_string(_: &str) -> Option<String> {
        None
    }
}
#[component]
fn App() -> Element {
    let mut exited = use_signal(|| false);
    rsx! {
        style {"html,body,#main{{margin:0;width:100%;height:100%;box-sizing:border-box}}*{{box-sizing:border-box}}"}
        if exited() {h1 {"Connection setup"}button {onclick:move|_|exited.set(false),"Explore offline"}}
        else {offline_preview::OfflinePreview {theme:layout::ThemeConfig::default(),on_exit:move |_|exited.set(true)}}
    }
}
fn main() {
    dioxus::launch(App);
}
