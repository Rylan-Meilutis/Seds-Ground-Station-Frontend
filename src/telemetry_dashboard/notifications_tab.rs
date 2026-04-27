use super::{
    format_timestamp_ms_clock, layout::ThemeConfig, translate_text, PersistentNotification,
};
use dioxus::prelude::*;

#[component]
pub fn NotificationsTab(
    history: Signal<Vec<PersistentNotification>>,
    theme: ThemeConfig,
    on_clear: EventHandler<()>,
) -> Element {
    let entries = history
        .read()
        .iter()
        .filter(|item| item.persistent)
        .cloned()
        .collect::<Vec<_>>();
    let has_history = !entries.is_empty();
    let font_stack = "system-ui, -apple-system, BlinkMacSystemFont";
    let clear_button_style = format!(
        "padding:0.25rem 0.65rem; border-radius:999px; border:1px solid {}; background:{}; color:{}; font-size:0.75rem; font-family:{}; cursor:{}; opacity:{};",
        theme.button_border,
        theme.button_background,
        theme.button_text,
        font_stack,
        if has_history { "pointer" } else { "default" },
        if has_history { "1" } else { "0.45" },
    );

    rsx! {
        div { style: "padding:4px 0 2px 0; color:{theme.text_primary}; height:100%; box-sizing:border-box; font-family:{font_stack};",
            div { style: "display:flex; align-items:center; gap:10px; margin:0 0 8px 0;",
                h2 { style: "margin:0; color:{theme.text_primary}; flex:1;", "{translate_text(\"Notifications History\")}" }
                button {
                    disabled: !has_history,
                    style: "{clear_button_style}",
                    onclick: move |_| {
                        on_clear.call(());
                    },
                    "{translate_text(\"Clear\")}"
                }
            }

            div { style: "display:flex; flex-direction:column; gap:6px;",
                for n in entries.iter() {
                    div {
                        style: "border:1px solid {theme.notification_border}; background:{theme.notification_background}; color:{theme.notification_text}; padding:8px 10px 10px 10px; border-radius:10px; font-family:{font_stack};",
                        div { style: "font-size:12px; opacity:0.85; line-height:1.25;", "{format_timestamp_ms_clock(n.timestamp_ms)}" }
                        div { style: "font-size:14px; line-height:1.3; padding-bottom:1px;", "{translate_text(&n.message)}" }
                    }
                }
                if entries.is_empty() {
                    div { style: "color:{theme.text_muted};", "{translate_text(\"No notifications yet.\")}" }
                }
            }
        }
    }
}
