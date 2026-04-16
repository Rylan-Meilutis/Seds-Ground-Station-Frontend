use super::{
    format_timestamp_ms_clock, layout::ThemeConfig, translate_text, PersistentNotification,
};
use dioxus::prelude::*;

#[component]
pub fn NotificationsTab(
    history: Signal<Vec<PersistentNotification>>,
    theme: ThemeConfig,
) -> Element {
    rsx! {
        div { style: "padding:4px 0 2px 0; color:{theme.text_primary}; height:100%; box-sizing:border-box;",
            h2 { style: "margin:0 0 8px 0; color:{theme.text_primary};", "{translate_text(\"Notifications History\")}" }

            div { style: "display:flex; flex-direction:column; gap:6px;",
                for n in history.read().iter() {
                    div {
                        style: "border:1px solid {theme.notification_border}; background:{theme.notification_background}; color:{theme.notification_text}; padding:8px 10px 10px 10px; border-radius:10px;",
                        div { style: "font-size:12px; opacity:0.85; line-height:1.25;", "{format_timestamp_ms_clock(n.timestamp_ms)}" }
                        div { style: "font-size:14px; line-height:1.3; padding-bottom:1px;", "{translate_text(&n.message)}" }
                    }
                }
                if history.read().is_empty() {
                    div { style: "color:{theme.text_muted};", "{translate_text(\"No notifications yet.\")}" }
                }
            }
        }
    }
}
