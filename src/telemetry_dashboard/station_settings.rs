//! Shared station configuration; local presentation preferences stay in Client.
use super::settings_navigation::{self, StationSection};
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
    calibration_context: settings_navigation::CalibrationContext,
    abort_only_mode: bool,
    theme: ThemeConfig,
    on_manage_users: EventHandler<()>,
) -> Element {
    let mut revision = use_signal(|| 0_u64);
    // A key replaces a scope through Dioxus's keyed-iterator diff. A single
    // component key alone memoizes the same body and retains its old Factory.
    // Ordinary policy retirement refreshes only ordinary drafts. The separately
    // owned mode control survives closure so explicit backend repair stays reachable.
    rsx! {super::station_mode_settings::ModeRequestNotice {theme:theme.clone()}
        // A mode change is station configuration with its own server checks
        // and typed confirmation, not an output action: Actions Disabled does
        // not lock it.
        super::station_mode_settings::StationModeSettings {theme:theme.clone(),inhibited:false}
        for (current_revision, policy_view_key) in [(revision(), super::station_mode_observer::view_key())] {
        StationSettingsBody {key:"station-settings-{current_revision}-{policy_view_key}",layout:layout.clone(),action_policy,fill_targets,calibration_context,abort_only_mode,theme:theme.clone(),on_manage_users,
            on_reload:move |_|revision.set(revision().saturating_add(1)),
        }
    }}
}

#[component]
fn StationSettingsBody(
    layout: ActionsTabLayout,
    action_policy: Signal<ActionPolicyMsg>,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    calibration_context: settings_navigation::CalibrationContext,
    abort_only_mode: bool,
    theme: ThemeConfig,
    on_manage_users: EventHandler<()>,
    on_reload: EventHandler<()>,
) -> Element {
    let context = super::station_settings_request::use_factory();
    let mut retired = use_signal(|| !context.current());
    use_future(move || async move {
        loop {
            if !context.current() {
                retired.set(true);
                return;
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(250).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    });
    let input_epoch = calibration_context.input_epoch;
    let calibration_locked = calibration_context.locked;
    let input_status = calibration_context.selection.read().clone();
    if retired() {
        return rsx! { section {role:"status",style:"display:grid;gap:12px;",
            p {"The connected station or signed-in account changed. Reload settings before editing."}
            button {r#type:"button",onclick:move |_|on_reload.call(()),"Reload settings"}
        }};
    }
    let admin = crate::auth::current_status()
        .roles
        .iter()
        .any(|role| role == "admin");
    let requested = *settings_navigation::SECTION.read();
    let section = if requested == StationSection::Administration && !admin {
        StationSection::Mission
    } else {
        requested
    };
    let selected_style = format!(
        "padding:10px 14px;border-radius:9px;border:1px solid {};background:{};color:{};font:inherit;",
        theme.info_accent, theme.button_background, theme.text_primary
    );
    let idle_style = format!(
        "padding:10px 14px;border-radius:9px;border:1px solid {};background:transparent;color:{};font:inherit;",
        theme.border, theme.text_primary
    );
    rsx! {
        nav {aria_label:"Ground Station settings sections",style:"display:flex;gap:8px;flex-wrap:wrap;",
            for item in [StationSection::Mission,StationSection::Loadcells,StationSection::Sequences,StationSection::Network,StationSection::Safety,StationSection::Administration] {
                if item!=StationSection::Administration||admin {
                    button {r#type:"button",aria_pressed:section==item,style:if section==item{selected_style.clone()}else{idle_style.clone()},onclick:move |_|settings_navigation::select_section(item),"{item.label()}"}
                }
            }
        }
        match section {
            StationSection::Mission=>rsx! {
                super::actions_tab::MissionSettings {layout,action_policy,backend_fill_targets:fill_targets,abort_only_mode,theme:theme.clone()}
            },
            StationSection::Loadcells=>rsx! {
                section {style:"display:grid;gap:14px;min-width:0;",
                    h3 {style:"margin:0;","Loadcells"}
                    p {style:"margin:0;color:{theme.text_muted};","Choose the DAQ input and the load cell used for fill cutoff here. Each input retains its own calibration."}
                    super::data_tab::Kg50InputSelector {theme:theme.clone(),selection:calibration_context,disabled:!crate::auth::can_edit_calibration()||(abort_only_mode&&crate::auth::can_view_actions())||calibration_locked()||input_epoch()==u64::MAX,disabled_reason:input_disabled_reason(abort_only_mode,calibration_locked())}
                    FillSourceSetting {theme:theme.clone(),fill_targets,disabled:abort_only_mode||!crate::auth::can_view_actions()}
                    p {style:"color:{theme.text_muted};", "Temperature correction follows the sensor’s saved drift calibration. This backend has no independent on/off preference; fitted coefficients are preserved."}
                    if crate::auth::can_view_calibration() {
                        button {r#type:"button",style:idle_style.clone(),disabled:input_status.pending(),onclick:move |_|settings_navigation::open_calibration(),"Open Calibration page"}
                        p {style:"color:{theme.text_muted};","Capture samples, adjust fits and save calibration on the Calibration page."}
                    } else {p {"Calibration access is required to view sensor calibration."}}
                }
            },
            StationSection::Sequences=>rsx! {super::gse_panel::GseConfiguration {abort_only_mode,theme:theme.clone()}},
            StationSection::Network=>rsx! {
                section {style:"display:grid;gap:8px;min-width:0;",h3 {"Station pairing"}
                    p {style:"margin:0;color:{theme.text_muted};","Pair the RRLS with the primary for authenticated primary commands and relay control."}
                    settings_navigation::StationPairingShortcut {}
                }
                if crate::auth::can_view_data() {super::paired_settings::PairedSettings {theme:theme.clone()}}
                section {style:"display:grid;gap:12px;min-width:0;",h3 {"Station clocks"}
                    super::actions_tab::NetworkUtcControls {}
                    style {{include_str!("system_clock_settings.css")}}
                    super::recording_download::SystemClockSettings {}
                }
            },
            StationSection::Safety=>rsx! {super::about_safety::AboutSafety {}},
            StationSection::Administration=>rsx! {section {h3 {"Administration"}button {r#type:"button",onclick:move |_|on_manage_users.call(()),"Manage users & access"}
                p {style:"color:{theme.text_muted};","Station pairing (RRLS and primary) is in the administration console."}
                settings_navigation::StationPairingShortcut {}}},
        }
    }
}

fn input_disabled_reason(abort_only_mode: bool, calibration_locked: bool) -> Option<String> {
    if !crate::auth::can_edit_calibration() {
        Some("This account cannot change the DAQ input.".into())
    } else if abort_only_mode && crate::auth::can_view_actions() {
        Some("Actions are disabled, so the DAQ input cannot be changed.".into())
    } else if calibration_locked {
        Some("Finish or discard the edits on the Calibration page before changing the DAQ input.".into())
    } else {
        None
    }
}

#[component]
fn FillSourceSetting(
    theme: ThemeConfig,
    mut fill_targets: Signal<Option<FillTargetsConfig>>,
    disabled: bool,
) -> Element {
    let settings_owner = super::station_settings_request::use_factory();
    let mut value = use_signal(|| None::<FillTargetsConfig>);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let mut supported = use_signal(|| false);
    use_effect(move || {
        spawn(async move {
            match settings_owner
                .get::<serde_json::Value>("/api/fill_targets")
                .await
            {
                Ok(raw) => {
                    let has_source = raw
                        .get("fill_source")
                        .and_then(|v| v.as_str())
                        .is_some_and(|source| matches!(source, "kg50" | "kg1000_absolute"));
                    match serde_json::from_value::<FillTargetsConfig>(raw) {
                        Ok(cfg) => {
                            supported.set(has_source);
                            fill_targets.set(Some(cfg.clone()));
                            value.set(Some(cfg));
                            if !has_source {
                                message.set("Fill cutoff selection is unavailable on this station; its existing source is unchanged.".into());
                            }
                        }
                        Err(error) => message.set(format!("Fill settings unavailable: {error}")),
                    }
                }
                Err(_) if !settings_owner.current() => (),
                Err(error) => message.set(format!("Fill settings unavailable: {error}")),
            }
        });
    });
    rsx! {section {style:"display:grid;gap:8px;padding:12px;border:1px solid {theme.border_soft};border-radius:10px;",
        label {r#for:"fill-cutoff-loadcell","Fill cutoff loadcell"}
        select {id:"fill-cutoff-loadcell",value:value.read().as_ref().map(|cfg|cfg.fill_source.clone()).unwrap_or_default(),disabled:disabled||busy()||!supported()||value.read().is_none(),
            onchange:move|event|{
                if disabled||busy()||!supported(){return;}let Some(mut cfg)=value.read().clone()else{return;};
                let requested=event.value();if !matches!(requested.as_str(),"kg50"|"kg1000_absolute"){return;}
                cfg.fill_source=requested.clone();busy.set(true);
                spawn(async move {match settings_owner.post::<_,FillTargetsConfig>("/api/fill_targets",&cfg).await {
                    Ok(saved)=>{let confirmed=saved.fill_source==requested;fill_targets.set(Some(saved.clone()));value.set(Some(saved));message.set(if confirmed {"Fill source saved"}else{"The station did not confirm this fill source. Review the current selection."}.into());},
                    Err(_) if !settings_owner.current() => return,
                    Err(error) =>message.set(format!("Fill source not changed: {error}")),
                }busy.set(false);});
            },
            option {value:"kg50","50 kg loadcell (signed calibrated value)"}
            option {value:"kg1000_absolute","1000 kg loadcell (absolute calibrated value)"}
        }
        if !message().is_empty(){p {role:"status","{message}"}}
    }}
}
