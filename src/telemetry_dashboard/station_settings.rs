//! Shared station configuration; local presentation preferences stay in Client.
use super::{
    ActionPolicyMsg, FillTargetsConfig,
    layout::{ActionsTabLayout, ThemeConfig},
};
use dioxus::prelude::*;

#[component]
pub(super) fn GroundStationSettings(
    layout: ActionsTabLayout,
    action_policy: Signal<ActionPolicyMsg>,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    abort_only_mode: bool,
    theme: ThemeConfig,
    on_open_calibration: EventHandler<()>,
    on_manage_users: EventHandler<()>,
) -> Element {
    rsx! {
        super::actions_tab::MissionSettings {
            layout, action_policy, backend_fill_targets: fill_targets,
            abort_only_mode, theme: theme.clone(),
        }
        section { style: "display:grid;gap:12px;min-width:0;",
            h3 { "DAQ inputs and load cells" }
            super::data_tab::Kg50InputSelector {
                theme: theme.clone(), disabled: !crate::auth::can_edit_calibration() || abort_only_mode,
            }
            p { "Each input retains its own calibration. Calibration captures and fits are managed on the Calibration page." }
            button { r#type: "button", onclick: move |_| on_open_calibration.call(()), "Open calibration" }
        }
        super::gse_panel::GseConfiguration { abort_only_mode, theme: theme.clone() }
        if crate::auth::current_status().roles.iter().any(|role| role == "admin") {
            section {
                h3 { "Administration" }
                button { r#type: "button", onclick: move |_| on_manage_users.call(()), "Manage users & access" }
            }
        }
    }
}
