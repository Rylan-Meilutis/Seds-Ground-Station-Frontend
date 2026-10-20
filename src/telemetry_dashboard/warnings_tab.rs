use super::{AlertMsg, format_timestamp_ms_clock, prelude::*};

#[component]
pub fn WarningsTab(
    warnings: Signal<Vec<AlertMsg>>,
    ack_warning_ts: Signal<i64>,
    theme: ThemeConfig,
) -> Element {
    let latest_warning_ts = warnings.read().iter().map(|w| w.timestamp_ms).max().unwrap_or(0);
    let ack_timestamp_ms = *ack_warning_ts.read();

    rsx! {
        div { style: "padding:4px 0 0 0; color:{theme.text_primary}; height:100%; box-sizing:border-box;",
            div { style: "display:flex; align-items:center; gap:10px; margin:0 0 8px 0;",
                h2 { style: "margin:0; color:{theme.text_primary}; flex:1;", "{translate_text(\"Warnings\")}" }
                ClearWarningsButton { latest_warning_ts, ack_warning_ts }

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

/// Acknowledge a fixed warning cutoff on the station. New warnings arriving
/// during the request remain active; errors and flight state are untouched.
#[component]
pub(super) fn ClearWarningsButton(latest_warning_ts: i64, mut ack_warning_ts: Signal<i64>) -> Element {
    let mut pending = use_signal(|| false);
    let mut error = use_signal(String::new);
    rsx! { div {style:"display:flex;align-items:center;gap:8px;flex-wrap:wrap;",
        button {
            disabled:*pending.read() || latest_warning_ts <= *ack_warning_ts.read(),
            onclick:move |_| {
                if *pending.read() || latest_warning_ts <= *ack_warning_ts.read() { return; }
                pending.set(true); error.set(String::new());
                spawn(async move {
                    match super::post_remote_alert_ack(latest_warning_ts, 0).await {
                        Ok(()) => { let cutoff=(*ack_warning_ts.read()).max(latest_warning_ts); ack_warning_ts.set(cutoff); }
                        Err(reason) => error.set(reason),
                    }
                    pending.set(false);
                });
            },
            if *pending.read() {"Clearing…"} else {"Clear warnings"}
        }
        if !error.read().is_empty() {small {role:"alert",title:error.read().clone(),"Could not clear warnings. Please retry."}}
    }}
}
