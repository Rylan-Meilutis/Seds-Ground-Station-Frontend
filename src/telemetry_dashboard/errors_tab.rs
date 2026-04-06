use super::{format_timestamp_ms_clock, layout::ThemeConfig, translate_text, AlertMsg};
use dioxus::prelude::*;
use dioxus_signals::Signal;

#[component]
pub fn ErrorsTab(errors: Signal<Vec<AlertMsg>>, theme: ThemeConfig) -> Element {
    rsx! {
        div { style: "padding:16px; color:{theme.text_primary};",
            h2 { style: "margin:0 0 12px 0; color:{theme.text_primary};", "{translate_text(\"Errors\")}" }

            div { style: "display:flex; flex-direction:column; gap:10px;",
                for e in errors.read().iter() {
                    div {
                        style: "border:1px solid {theme.error_border}; background:{theme.error_background}; color:{theme.error_text}; padding:12px; border-radius:12px;",
                        div { style: "font-size:12px; opacity:0.85;", "{format_timestamp_ms_clock(e.timestamp_ms)}" }
                        div { style: "font-size:14px;", "{translate_text(&e.message)}" }
                    }
                }
                if errors.read().is_empty() {
                    div { style: "color:{theme.text_muted};", "{translate_text(\"No errors.\")}" }
                }
            }
        }
    }
}
