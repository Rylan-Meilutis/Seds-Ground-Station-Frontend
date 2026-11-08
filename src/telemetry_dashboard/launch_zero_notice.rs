//! Live notice while a Launch press is held behind the opt-in loadcell zero.
use super::layout::ThemeConfig;
use super::{ActionPolicyMsg, LaunchZeroStatus};
use dioxus::prelude::*;

pub(super) const CANCEL_PATH: &str = "/api/auto_zero/launch/cancel";

pub(super) fn notice_text(status: &LaunchZeroStatus) -> String {
    if status.sending {
        "Loadcell zero finished; sending Launch.".into()
    } else {
        format!("Zeroing loadcell before Launch, {} s", status.remaining_s)
    }
}

#[component]
pub(super) fn LaunchZeroNotice(action_policy: Signal<ActionPolicyMsg>, theme: ThemeConfig) -> Element {
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let Some(status) = action_policy.read().launch_zero.clone() else {
        return rsx! {};
    };
    let text = notice_text(&status);
    let can_cancel = !status.sending && crate::auth::can_send_command("Launch");
    rsx! {
        aside {
            role: "status",
            class: "gs26-connection-notice gs26-layout-notice",
            style: "border-color:{theme.warning_border};background:{theme.warning_background};color:{theme.warning_text};",
            div { class: "gs26-layout-notice-body",
                strong { "Launch held for loadcell zero" }
                p { "{text}" }
                if !status.sending {
                    p { "Launch, T-10 and the DAQ launch file start after the zero is applied. If the zero fails, Launch is sent with the previous calibration." }
                }
                if let Some(message) = error() { p { role: "alert", "{message}" } }
            }
            if can_cancel {
                div { class: "gs26-layout-notice-actions",
                    button {
                        disabled: busy(),
                        onclick: move |_| {
                            busy.set(true);
                            spawn(async move {
                                match super::http_post_empty(CANCEL_PATH).await {
                                    Ok(()) => error.set(None),
                                    Err(message) => error.set(Some(format!("Cancel failed: {message}"))),
                                }
                                busy.set(false);
                            });
                        },
                        "Cancel Launch"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_launch_policy_decodes_and_reads_as_progress() {
        let policy: ActionPolicyMsg = serde_json::from_value(serde_json::json!({
            "key_enabled": true,
            "controls": [],
            "launch_zero": {"remaining_s": 7, "samples": 200, "sending": false}
        }))
        .unwrap();
        let status = policy.launch_zero.unwrap();
        assert_eq!(notice_text(&status), "Zeroing loadcell before Launch, 7 s");
        assert_eq!(
            notice_text(&LaunchZeroStatus { sending: true, ..status }),
            "Loadcell zero finished; sending Launch."
        );
        // Older stations omit the field: no notice.
        let old: ActionPolicyMsg =
            serde_json::from_value(serde_json::json!({"key_enabled": true, "controls": []})).unwrap();
        assert!(old.launch_zero.is_none());
    }
}
