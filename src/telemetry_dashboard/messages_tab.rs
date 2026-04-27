use super::{
    format_timestamp_ms_clock, layout::ThemeConfig, translate_text, PersistentNotification,
};
use dioxus::prelude::*;

#[component]
pub fn MessagesTab(history: Signal<Vec<PersistentNotification>>, theme: ThemeConfig) -> Element {
    let font_stack = "system-ui, -apple-system, BlinkMacSystemFont";
    let entries = history.read().clone();

    rsx! {
        div { style: "padding:4px 0 2px 0; color:{theme.text_primary}; height:100%; box-sizing:border-box; font-family:{font_stack};",
            div { style: "display:flex; align-items:center; gap:10px; margin:0 0 8px 0;",
                h2 { style: "margin:0; color:{theme.text_primary}; flex:1;", "{translate_text(\"Messages\")}" }
            }

            div { style: "display:flex; flex-direction:column; gap:6px;",
                for n in entries.iter() {
                    div {
                        style: "border:1px solid {theme.border_soft}; background:{theme.panel_background_alt}; color:{theme.text_primary}; padding:8px 10px 10px 10px; border-radius:10px; font-family:{font_stack};",
                        div { style: "font-size:12px; opacity:0.85; line-height:1.25; color:{theme.text_muted};", "{format_timestamp_ms_clock(n.timestamp_ms)}" }
                        div { style: "font-size:14px; line-height:1.3; padding-bottom:1px;", "{translate_text(&n.message)}" }
                    }
                }
                if entries.is_empty() {
                    div { style: "color:{theme.text_muted};", "{translate_text(\"No messages yet.\")}" }
                }
            }
        }
    }
}
