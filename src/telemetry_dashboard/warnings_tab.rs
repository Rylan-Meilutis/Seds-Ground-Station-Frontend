use super::{format_timestamp_ms_clock, layout::ThemeConfig, translate_text, AlertMsg};
use dioxus::prelude::*;

#[component]
pub fn WarningsTab(
    warnings: Signal<Vec<AlertMsg>>,
    ack_timestamp_ms: i64,
    theme: ThemeConfig,
    on_ack: EventHandler<()>,
) -> Element {
    let has_warnings = !warnings.read().is_empty();
    let ack_button_style = format!(
        "padding:0.25rem 0.65rem; border-radius:999px; border:1px solid {}; background:{}; color:{}; font-size:0.75rem; cursor:{}; opacity:{};",
        theme.button_border,
        theme.button_background,
        theme.button_text,
        if has_warnings { "pointer" } else { "default" },
        if has_warnings { "1" } else { "0.45" },
    );

    rsx! {
        div { style: "padding:4px 0 0 0; color:{theme.text_primary}; height:100%; box-sizing:border-box;",
            div { style: "display:flex; align-items:center; gap:10px; margin:0 0 8px 0;",
                h2 { style: "margin:0; color:{theme.text_primary}; flex:1;", "{translate_text(\"Warnings\")}" }
                button {
                    disabled: !has_warnings,
                    style: "{ack_button_style}",
                    onclick: move |_| {
                        on_ack.call(());
                    },
                    "{translate_text(\"Acknowledge\")}"
                }
            }

            div { style: "display:flex; flex-direction:column; gap:6px;",
                for w in warnings.read().iter() {
                    {
                        let acknowledged = w.timestamp_ms <= ack_timestamp_ms;
                        let row_style = if acknowledged {
                            format!(
                                "border:1px solid {}; background:{}; color:{}; padding:8px 10px 10px 10px; border-radius:10px;",
                                theme.border, theme.panel_background, theme.text_secondary
                            )
                        } else {
                            format!(
                                "border:1px solid {}; background:{}; color:{}; padding:8px 10px 10px 10px; border-radius:10px;",
                                theme.warning_border, theme.warning_background, theme.warning_text
                            )
                        };
                        rsx! {
                            div {
                                style: "{row_style}",
                                div { style: "font-size:12px; opacity:0.85; line-height:1.25;", "{format_timestamp_ms_clock(w.timestamp_ms)}" }
                                div { style: "font-size:14px; line-height:1.3; padding-bottom:1px;", "{translate_text(&w.message)}" }
                            }
                        }
                    }
                }
                if warnings.read().is_empty() {
                    div { style: "color:{theme.text_muted};", "{translate_text(\"No warnings.\")}" }
                }
            }
        }
    }
}
