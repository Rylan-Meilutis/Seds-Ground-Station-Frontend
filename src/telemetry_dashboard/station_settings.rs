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
        super::firmware_sim_panel::FirmwareSimPanel {theme:theme.clone()}
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
                super::launch_site::LaunchSiteSettings {theme:theme.clone()}
                super::apogee_planner::ApogeePlannerSettings {theme:theme.clone()}
                super::flight_plans_view::FlightSimSettings {theme:theme.clone()}
                super::los_prediction::LosSettingsPanel {theme:theme.clone()}
                super::fire_archive_view::FireArchiveSettings {theme:theme.clone()}
            },
            StationSection::Loadcells=>rsx! {
                section {style:"display:grid;gap:14px;min-width:0;",
                    h3 {style:"margin:0;","Loadcells"}
                    p {style:"margin:0;color:{theme.text_muted};","Choose the DAQ input and the load cell used for fill cutoff here. Each input retains its own calibration."}
                    super::data_tab::Kg50InputSelector {theme:theme.clone(),selection:calibration_context,disabled:!crate::auth::can_edit_calibration()||(abort_only_mode&&crate::auth::can_view_actions())||calibration_locked()||input_epoch()==u64::MAX,disabled_reason:input_disabled_reason(abort_only_mode,calibration_locked())}
                    McpVrefSetting {theme:theme.clone(),disabled:!crate::auth::can_edit_calibration()||(abort_only_mode&&crate::auth::can_view_actions())||calibration_locked()}
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
            StationSection::Safety=>rsx! {
                super::proximity_lock_view::ProximityAdminSettings {theme:theme.clone()}
                super::about_safety::AboutSafety {}
            },
            StationSection::Administration=>rsx! {section {h3 {"Administration"}button {r#type:"button",onclick:move |_|on_manage_users.call(()),"Open administration console"}
                p {style:"color:{theme.text_muted};","The administration console holds station locations (GS and RRLS positions for the map and antenna aiming), station pairing (RRLS and primary), users and access, and sequence and panel permissions."}
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

/// The station's MCP3564R reference setting (volts), as last loaded or saved
/// here. The DAQ health panel compares it with the reference the DAQ reports.
pub(super) static STATION_MCP_VREF_V: GlobalSignal<Option<f32>> = Signal::global(|| None);

/// Saves the station's MCP3564R reference. The backend's loadcell settings
/// field is `mcp_vref_v` (GET /api/calibration).
pub(super) const MCP_VREF_PATH: &str = "/api/calibration/mcp_vref";
pub(super) const MCP_VREF_MIN_V: f32 = 1.9;
pub(super) const MCP_VREF_MAX_V: f32 = 2.6;

/// An operator-entered reference: 1.9–2.6 V, kept to 3 decimals.
pub(super) fn parse_mcp_vref(text: &str) -> Result<f32, String> {
    let value: f32 = text.trim().parse().map_err(|_| "Enter the reference in volts, e.g. 2.200".to_string())?;
    if !value.is_finite() || !(MCP_VREF_MIN_V..=MCP_VREF_MAX_V).contains(&value) {
        return Err(format!("The reference must be {MCP_VREF_MIN_V:.1}–{MCP_VREF_MAX_V:.1} V"));
    }
    Ok((value * 1000.0).round() / 1000.0)
}

/// The station's saved reference from a GET /api/calibration body.
pub(super) fn mcp_vref_from_calibration(raw: &serde_json::Value) -> Option<f32> {
    raw.get("mcp_vref_v")
        .and_then(serde_json::Value::as_f64)
        .map(|v| v as f32)
        .filter(|v| v.is_finite())
}

#[component]
fn McpVrefSetting(theme: ThemeConfig, disabled: bool) -> Element {
    let settings_owner = super::station_settings_request::use_factory();
    let mut saved = use_signal(|| None::<f32>);
    let mut text = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    use_effect(move || {
        spawn(async move {
            match settings_owner.get::<serde_json::Value>("/api/calibration").await {
                Ok(raw) => match mcp_vref_from_calibration(&raw) {
                    Some(value) => {
                        saved.set(Some(value));
                        text.set(format!("{value:.3}"));
                        *STATION_MCP_VREF_V.write() = Some(value);
                    }
                    None => message.set("This station does not report an MCP3564R reference setting.".into()),
                },
                Err(_) if !settings_owner.current() => (),
                Err(error) => message.set(format!("MCP3564R reference unavailable: {error}")),
            }
        });
    });
    let unavailable = saved.read().is_none();
    let save = move |_| {
        if disabled || busy() || saved.peek().is_none() {
            return;
        }
        let value = match parse_mcp_vref(&text.peek()) {
            Ok(value) => value,
            Err(error) => {
                message.set(error);
                return;
            }
        };
        busy.set(true);
        spawn(async move {
            let body = serde_json::json!({ "mcp_vref_v": value });
            match settings_owner.post::<_, serde_json::Value>(MCP_VREF_PATH, &body).await {
                Ok(raw) => {
                    let confirmed = mcp_vref_from_calibration(&raw).filter(|v| (v - value).abs() < 0.0005);
                    if let Some(v) = confirmed {
                        saved.set(Some(v));
                        text.set(format!("{v:.3}"));
                        *STATION_MCP_VREF_V.write() = Some(v);
                        message.set(format!("Reference saved: {v:.3} V. The load-cell calibrations were converted by the station."));
                    } else {
                        message.set("The station did not confirm this reference. Review the current value.".into());
                    }
                }
                Err(_) if !settings_owner.current() => return,
                Err(error) => message.set(format!("Reference not changed: {error}")),
            }
            busy.set(false);
        });
    };
    rsx! {section {"data-mcp-vref":"",style:"display:grid;gap:8px;padding:12px;border:1px solid {theme.border_soft};border-radius:10px;",
        label {r#for:"mcp-vref","MCP3564R reference (V)"}
        div {style:"display:flex;flex-wrap:wrap;gap:8px;align-items:center;",
            input {id:"mcp-vref",r#type:"number",min:"1.900",max:"2.600",step:"0.001",inputmode:"decimal",value:"{text}",
                disabled:disabled||busy()||unavailable,
                style:"padding:8px;border-radius:6px;background:{theme.button_background};color:{theme.text_primary};border:1px solid {theme.border};font:inherit;width:9em;",
                oninput:move |event| text.set(event.value()),
            }
            button {r#type:"button",class:"gs-btn",disabled:disabled||busy()||unavailable,onclick:save,"Save reference"}
        }
        p {style:"margin:0;font-size:12px;color:{theme.text_muted};",
            "Measure with a DMM between P7/P8 pin 6 (+VREF) and GND. Changing it rescales every MCP3564R volt reading, and the stored load-cell calibrations are converted automatically. Nominal 2.400 V."
        }
        if !message().is_empty() {p {role:"status",style:"margin:0;","{message}"}}
    }}
}

#[cfg(test)]
mod mcp_vref_tests {
    use super::*;

    #[test]
    fn reference_entry_is_bounded_and_kept_to_three_decimals() {
        assert_eq!(parse_mcp_vref("2.2"), Ok(2.2));
        assert_eq!(parse_mcp_vref(" 2.40049 "), Ok(2.4));
        assert_eq!(parse_mcp_vref("1.9"), Ok(1.9));
        assert_eq!(parse_mcp_vref("2.6"), Ok(2.6));
        assert!(parse_mcp_vref("1.899").is_err());
        assert!(parse_mcp_vref("2.601").is_err());
        assert!(parse_mcp_vref("NaN").is_err());
        assert!(parse_mcp_vref("").is_err());
    }

    #[test]
    fn station_reference_is_read_from_the_calibration_file() {
        assert_eq!(mcp_vref_from_calibration(&serde_json::json!({"kg50_input":"amp2","mcp_vref_v":2.2})), Some(2.2));
        assert_eq!(mcp_vref_from_calibration(&serde_json::json!({"kg50_input":"amp2"})), None, "older stations omit it");
    }

    #[test]
    fn reference_field_renders_bounds_and_measurement_help() {
        let mut dom = VirtualDom::new(|| rsx! { McpVrefSetting { theme: ThemeConfig::default(), disabled: false } });
        dom.rebuild_in_place();
        let html = dioxus::ssr::render(&dom);
        assert!(html.contains("MCP3564R reference (V)"), "{html}");
        assert!(html.contains("min=\"1.900\"") && html.contains("max=\"2.600\"") && html.contains("step=\"0.001\""));
        assert!(html.contains("P7/P8 pin 6 (+VREF) and GND"));
        assert!(html.contains("converted automatically"));
        // Nothing to edit until the station reports its setting.
        assert!(html.contains("disabled"));
    }
}
