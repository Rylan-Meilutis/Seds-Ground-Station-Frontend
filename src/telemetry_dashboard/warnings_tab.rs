use super::{format_timestamp_ms_clock, layout::ThemeConfig, translate_text, AlertMsg};
use dioxus::prelude::*;

#[component]
pub fn WarningsTab(warnings: Signal<Vec<AlertMsg>>, theme: ThemeConfig) -> Element {
    rsx! {
        div { style: "padding:16px; color:{theme.text_primary};",
            h2 { style: "margin:0 0 12px 0; color:{theme.text_primary};", "{translate_text(\"Warnings\")}" }

            div { style: "display:flex; flex-direction:column; gap:10px;",
                for w in warnings.read().iter() {
                    div {
                        style: "border:1px solid {theme.warning_border}; background:{theme.warning_background}; color:{theme.warning_text}; padding:12px; border-radius:12px;",
                        div { style: "font-size:12px; opacity:0.85;", "{format_timestamp_ms_clock(w.timestamp_ms)}" }
                        div { style: "font-size:14px;", "{translate_text(&w.message)}" }
                    }
                }
                if warnings.read().is_empty() {
                    div { style: "color:{theme.text_muted};", "{translate_text(\"No warnings.\")}" }
                }
            }
        }
    }
}
