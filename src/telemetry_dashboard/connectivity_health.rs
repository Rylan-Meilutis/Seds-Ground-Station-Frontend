use super::{
    layout::ThemeConfig, network_health::NetworkHealthSection, types::NetworkBoardHealthEntry,
};
use dioxus::prelude::*;

#[derive(serde::Deserialize)]
struct SdRetryReply {
    accepted: bool,
    token: u32,
}

/// Connectivity owns per-board diagnostics and manual network/storage actions.
#[component]
pub fn ConnectivityHealthSection(
    rows: Vec<NetworkBoardHealthEntry>,
    ws_connected: bool,
    generated_ms: u64,
    simulated: bool,
    theme: ThemeConfig,
) -> Element {
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let mut token = use_signal(|| None::<u32>);
    rsx! {
        NetworkHealthSection {
            rows, ws_connected, generated_ms, simulated, theme,
            recovery_renderer: |(slot, board_id, ws_connected, theme): (super::network_health::RecoverySlot, String, bool, ThemeConfig)| {
                use super::network_health::RecoverySlot;
                match slot {
                    RecoverySlot::Tools => rsx! { super::board_recovery_panel::RecoveryTools { ws_connected, theme } },
                    RecoverySlot::State => rsx! { super::board_recovery_panel::RecoveryBoardState { board_id } },
                    RecoverySlot::Actions => rsx! { super::board_recovery_panel::RecoveryBoardActions { board_id, ws_connected } },
                    RecoverySlot::Details => rsx! { super::board_recovery_panel::RecoveryBoardDetails { board_id, ws_connected, theme } },
                }
            },
            on_recovery_focus: |(id, backwards): (String, bool)| super::board_recovery_panel::cycle_modal_focus(&id, backwards),
            sd_retry_allowed: crate::auth::can_send_command("DaqSdRetry")
                && crate::auth::current_status().station_role.as_deref() != Some("rrls"),
            sd_retry_busy: busy(), sd_retry_message: message(), sd_retry_token: token(),
            on_sd_retry: move |_| {
                if busy() { return; }
                let owner = match super::recording_download::ExportOwner::capture_ordinary_selected(|| true) {
                    Ok(owner) => owner, Err(error) => { message.set(error); return; }
                };
                let request = super::station_settings_request::post_captured::<_, SdRetryReply>(
                    "/api/network-health/daq/sd/retry", &serde_json::json!({}), Ok(owner.clone()));
                busy.set(true); token.set(None); message.set("Requesting SD retry…".into());
                spawn(async move {
                    let result = request.await;
                    if !owner.is_current() { return; }
                    match result {
                        Ok(reply) if reply.accepted && reply.token != 0 => {
                            token.set(Some(reply.token));
                            message.set("Retry command queued; waiting for DAQ diagnostics.".into());
                        }
                        Ok(_) => message.set("SD retry was not accepted.".into()),
                        Err(error) => message.set(error),
                    }
                    busy.set(false);
                });
            }
        }
    }
}
