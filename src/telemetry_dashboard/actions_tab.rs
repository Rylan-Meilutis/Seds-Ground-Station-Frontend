// frontend/src/telemetry_dashboard/actions_tab.rs

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth;

use super::blink::{ACTION_BLINK_CSS, action_animation_style};
use super::layout::{ActionSpec, ActionsTabLayout, ThemeConfig};
use super::{
    ActionPolicyMsg, BlinkMode, FillTargetsConfig, FluidFillTarget, RecordingStatusMsg,
    action_policy_control_enabled, command_feedback_active, translate_text,
};

fn btn_style(
    border: &str,
    bg: &str,
    fg: &str,
    enabled: bool,
    blink: BlinkMode,
    actuated: Option<bool>,
) -> String {
    let cursor = if enabled { "pointer" } else { "not-allowed" };
    let pointer_events = if enabled { "auto" } else { "none" };
    let actuated_active = actuated.unwrap_or(false);
    let recommended = enabled && blink != BlinkMode::None;
    let opacity = if recommended || actuated_active {
        "1.0"
    } else if !enabled {
        "0.72"
    } else {
        "0.62"
    };
    let filter = if actuated_active || recommended {
        "none"
    } else if !enabled {
        "grayscale(0.72) brightness(0.78)"
    } else {
        "saturate(0.58) brightness(0.82)"
    };
    let box_shadow = if recommended || actuated_active {
        "0 10px 25px rgba(0,0,0,0.25)"
    } else if !enabled {
        "none"
    } else {
        "0 4px 12px rgba(0,0,0,0.16)"
    };
    let animation = action_animation_style(enabled, blink, actuated);
    format!(
        "padding:0.65rem 1rem; border-radius:0.75rem; cursor:{cursor}; opacity:{opacity}; filter:{filter}; width:100%; \
         display:flex; align-items:center; justify-content:space-between; gap:0.75rem; text-align:left; border:1px solid {border}; background:{bg}; color:{fg}; \
         font-weight:800; box-shadow:{box_shadow}; touch-action:manipulation; pointer-events:{pointer_events}; {animation}"
    )
}

fn recording_command_active(cmd: &str, status: &RecordingStatusMsg) -> Option<bool> {
    match cmd {
        "StartWritingNow" | "StartWritingLastTwoMinutes" => Some(status.mode == "recording"),
        "PauseWritingDb" => Some(status.mode == "paused"),
        "StopWritingDb" => Some(status.mode == "idle"),
        _ => None,
    }
}

fn merged_actuated(
    cmd: &str,
    control_actuated: Option<bool>,
    recording_status: &RecordingStatusMsg,
) -> Option<bool> {
    let local_active = command_feedback_active(cmd);
    let base = control_actuated.or_else(|| recording_command_active(cmd, recording_status));
    match (base, local_active) {
        (Some(active), _) => Some(active),
        (None, true) => Some(true),
        (None, false) => None,
    }
}

#[derive(Clone, Copy)]
enum ActionRowItem<'a> {
    Action(&'a ActionSpec),
    Spacer,
}

enum ActionLayoutRow<'a> {
    Items(Vec<ActionRowItem<'a>>),
    Spacer,
}

fn flush_action_row<'a>(
    rows: &mut Vec<ActionLayoutRow<'a>>,
    current_row: &mut Vec<ActionRowItem<'a>>,
) {
    if !current_row.is_empty() {
        rows.push(ActionLayoutRow::Items(std::mem::take(current_row)));
    }
}

fn build_action_rows<'a>(actions: &'a [&'a ActionSpec]) -> Vec<ActionLayoutRow<'a>> {
    let mut rows = Vec::new();
    let mut current_row = Vec::new();

    for action in actions {
        if action.new_row_before || action.spacer_row_before {
            flush_action_row(&mut rows, &mut current_row);
        }
        if action.spacer_row_before {
            rows.push(ActionLayoutRow::Spacer);
        }
        if action.spacer_before && !current_row.is_empty() {
            current_row.push(ActionRowItem::Spacer);
        }

        current_row.push(ActionRowItem::Action(action));

        if action.spacer_after {
            current_row.push(ActionRowItem::Spacer);
        }
        if action.new_row_after || action.spacer_row_after {
            flush_action_row(&mut rows, &mut current_row);
        }
        if action.spacer_row_after {
            rows.push(ActionLayoutRow::Spacer);
        }
    }

    flush_action_row(&mut rows, &mut current_row);
    rows
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct KalmanFilterConstants {
    process_position_variance: f32,
    process_velocity_variance: f32,
    accel_variance: f32,
    baro_altitude_variance: f32,
    gps_altitude_variance: f32,
    gps_velocity_variance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct FlightProfileConfig {
    id: String,
    label: String,
    wind_level: u8,
    kalman: KalmanFilterConstants,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct FlightSetupConfig {
    version: u32,
    selected_profile_id: String,
    profiles: Vec<FlightProfileConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FlightSetupApplyResponse {
    selected_profile_id: String,
    wind_level: u8,
    payload_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
struct EmptyApplyReq {}

fn selected_profile(cfg: &FlightSetupConfig) -> Option<&FlightProfileConfig> {
    cfg.profiles
        .iter()
        .find(|profile| profile.id == cfg.selected_profile_id)
}

fn setup_panel_style(theme: &ThemeConfig) -> String {
    format!(
        "padding:14px; border-radius:14px; border:1px solid {}; background:{}; display:flex; flex-direction:column; gap:12px;",
        theme.border, theme.panel_background
    )
}

fn input_style(theme: &ThemeConfig) -> String {
    format!(
        "width:100%; padding:10px 12px; border-radius:10px; border:1px solid {}; background:{}; color:{};",
        theme.border, theme.panel_background_alt, theme.text_primary
    )
}

fn apply_button_style(theme: &ThemeConfig, enabled: bool) -> String {
    let opacity = if enabled { "1.0" } else { "0.55" };
    let cursor = if enabled { "pointer" } else { "not-allowed" };
    format!(
        "padding:10px 14px; border-radius:10px; border:1px solid {}; background:{}; color:{}; font-weight:700; cursor:{}; opacity:{};",
        theme.button_border, theme.button_background, theme.button_text, cursor, opacity
    )
}

#[component]
pub fn ActionsTab(
    layout: ActionsTabLayout,
    action_policy: Signal<ActionPolicyMsg>,
    recording_status: Signal<RecordingStatusMsg>,
    backend_fill_targets: Signal<Option<FillTargetsConfig>>,
    abort_only_mode: bool,
    theme: ThemeConfig,
) -> Element {
    let mut flight_setup = use_signal(|| None::<FlightSetupConfig>);
    let mut flight_setup_status = use_signal(String::new);
    let mut flight_setup_busy = use_signal(|| false);
    let mut fill_targets = use_signal(|| None::<FillTargetsConfig>);
    let mut fill_targets_status = use_signal(String::new);
    let mut fill_targets_busy = use_signal(|| false);
    use_effect(move || {
        spawn(async move {
            match crate::telemetry_dashboard::http_get_json::<FlightSetupConfig>(
                "/api/flight_setup",
            )
            .await
            {
                Ok(cfg) => flight_setup.set(Some(cfg)),
                Err(err) => flight_setup_status.set(format!("Flight setup load failed: {err}")),
            }
        });
    });
    use_effect(move || {
        spawn(async move {
            match crate::telemetry_dashboard::http_get_json::<FillTargetsConfig>(
                "/api/fill_targets",
            )
            .await
            {
                Ok(cfg) => {
                    let mut backend_fill_targets = backend_fill_targets;
                    backend_fill_targets.set(Some(cfg.clone()));
                    let mut fill_targets = fill_targets;
                    fill_targets.set(Some(cfg));
                }
                Err(err) => fill_targets_status.set(format!("Fill targets load failed: {err}")),
            }
        });
    });
    use_effect(move || {
        if fill_targets_status.read().starts_with("Unsaved") {
            return;
        }
        if let Some(cfg) = backend_fill_targets.read().clone() {
            fill_targets.set(Some(cfg));
        }
    });
    let visible_actions = if auth::can_view_actions() {
        layout.actions.iter().collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let action_rows = build_action_rows(&visible_actions);
    let software_buttons_enabled = action_policy.read().software_buttons_enabled;
    let fill_targets_editable = if layout.fill_targets_require_actions_enabled {
        software_buttons_enabled && !abort_only_mode
    } else {
        true
    };
    rsx! {
        div {
            style: "
                padding:16px;
                display:flex;
                flex-direction:column;
                gap:12px;
                width:100%;
                max-width:none;
                box-sizing:border-box;
                align-self:stretch;
            ",
            style { "{ACTION_BLINK_CSS}" }
            h2 { style: "margin:0 0 8px 0; color:{theme.text_primary};", "{translate_text(\"Actions\")}" }
            p  { style: "margin:0 0 12px 0; color:{theme.text_soft}; font-size:0.9rem;",
                "All available actions are available all the time, use with caution as improper use \
                can and will damage the system."
            }
            if abort_only_mode {
                div {
                    style: "margin:0; padding:6px 10px; border-radius:8px; border:1px solid {theme.error_border}; background:{theme.error_background}; color:{theme.error_text}; font-size:11px; line-height:1.25;",
                    "{translate_text(\"Disable Actions is enabled. All action and flight-state buttons except Abort are disabled.\")}"
                }
            }
            if visible_actions.is_empty() {
                div {
                    style: "padding:12px; border:1px solid {theme.border}; border-radius:12px; background:{theme.panel_background}; color:{theme.text_muted}; font-size:13px;",
                    "{translate_text(\"No actions are available for this user.\")}"
                }
            } else {
                div {
                    style: "
                        display:flex;
                        flex-direction:column;
                        gap:12px;
                        width:100%;
                        max-width:none;
                        box-sizing:border-box;
                        align-self:stretch;
                    ",
                    for row in action_rows.iter() {
                        match row {
                            ActionLayoutRow::Spacer => rsx! {
                                div {
                                    style: "height:14px;"
                                }
                            },
                            ActionLayoutRow::Items(items) => rsx! {
                                div {
                                    style: "
                                        display:grid;
                                        grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
                                        gap:12px;
                                        align-items:stretch;
                                        width:100%;
                                        max-width:none;
                                        box-sizing:border-box;
                                    ",
                                    for item in items.iter() {
                                        match item {
                                            ActionRowItem::Spacer => rsx! {
                                                div {
                                                    style: "min-width:32px;"
                                                }
                                            },
                                            ActionRowItem::Action(action) => {
                                                let action_policy_snapshot = action_policy.read().clone();
                                                let control = action_policy
                                                    .read()
                                                    .controls
                                                    .iter()
                                                    .find(|c| c.cmd == action.cmd)
                                                    .cloned();
                                                let enabled = action_policy_control_enabled(&action_policy_snapshot, action.cmd.as_str())
                                                    && auth::can_send_command(action.cmd.as_str())
                                                    && (!abort_only_mode || action.cmd == "Abort")
                                                ;
                                                let blink = control.as_ref().map(|c| c.blink).unwrap_or(BlinkMode::None);
                                                let actuated = merged_actuated(
                                                    action.cmd.as_str(),
                                                    control.as_ref().and_then(|c| c.actuated),
                                                    &recording_status.read(),
                                                );
                                                rsx! {
                                                    button {
                                                        style: "{btn_style(&action.border, &action.bg, &action.fg, enabled, blink, actuated)}",
                                                        disabled: !enabled,
                                                        onmousedown: {
                                                            let cmd = action.cmd.clone();
                                                            move |_| {
                                                                if enabled {
                                                                    crate::telemetry_dashboard::send_cmd_from_press(&cmd)
                                                                }
                                                            }
                                                        },
                                                        ontouchstart: {
                                                            let cmd = action.cmd.clone();
                                                            move |_| {
                                                                if enabled {
                                                                    crate::telemetry_dashboard::send_cmd_from_press(&cmd)
                                                                }
                                                            }
                                                        },
                                                        onclick: {
                                                            let cmd = action.cmd.clone();
                                                            move |_| {
                                                                if enabled {
                                                                    crate::telemetry_dashboard::send_cmd_from_click(&cmd)
                                                                }
                                                            }
                                                        },
                                                        span { style: "min-width:0; flex:1 1 auto;", "{action.label}" }
                                                        if !enabled {
                                                            span {
                                                                style: "flex:0 0 auto; padding:0.14rem 0.42rem; border-radius:999px; border:1px solid rgba(255,255,255,0.16); background:rgba(0,0,0,0.18); color:rgba(255,255,255,0.82); font-size:0.68rem; font-weight:800; line-height:1; text-transform:uppercase; letter-spacing:0.04em;",
                                                                "{translate_text(\"Disabled\")}"
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 320px), 1fr)); gap:12px; align-items:start; width:100%; max-width:none; box-sizing:border-box;",
                if layout.show_flight_setup {
                    div { style: "{setup_panel_style(&theme)}",
                    h3 { style: "margin:0; color:{theme.text_primary};", "{translate_text(\"Flight Setup\")}" }
                    if let Some(cfg) = flight_setup.read().clone() {
                        div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 280px), 1fr)); gap:12px; align-items:start; min-width:0;",
                            div { style: "display:flex; flex-direction:column; gap:8px; min-width:0;",
                                label { style: "font-size:12px; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.08em;", "{translate_text(\"Flight profile\")}" }
                                select {
                                    style: "{input_style(&theme)} max-width:100%; min-width:0;",
                                    value: "{cfg.selected_profile_id}",
                                    onchange: {
                                        move |evt| {
                                            let next_id = evt.value();
                                            let Some(mut next_cfg) = flight_setup.read().clone() else {
                                                return;
                                            };
                                            next_cfg.selected_profile_id = next_id.clone();
                                            flight_setup.set(Some(next_cfg.clone()));
                                            flight_setup_status.set("Saving flight setup…".to_string());
                                            spawn(async move {
                                                match crate::telemetry_dashboard::http_post_json::<FlightSetupConfig, FlightSetupConfig>(
                                                    "/api/flight_setup",
                                                    &next_cfg,
                                                ).await {
                                                    Ok(saved) => {
                                                        flight_setup.set(Some(saved));
                                                        flight_setup_status.set(format!("Selected profile {next_id}."));
                                                    }
                                                    Err(err) => {
                                                        flight_setup_status.set(format!("Flight setup save failed: {err}"));
                                                    }
                                                }
                                            });
                                        }
                                    },
                                    for profile in cfg.profiles.iter() {
                                        option {
                                            value: "{profile.id}",
                                            "{profile.label} (wind {profile.wind_level})"
                                        }
                                    }
                                }
                                button {
                                    style: "{apply_button_style(&theme, !*flight_setup_busy.read() && !abort_only_mode)}",
                                    disabled: *flight_setup_busy.read() || abort_only_mode,
                                    onclick: move |_| {
                                        let is_busy = *flight_setup_busy.read();
                                        if is_busy || abort_only_mode {
                                            return;
                                        }
                                        flight_setup_busy.set(true);
                                        flight_setup_status.set("Applying flight setup…".to_string());
                                        spawn(async move {
                                            let body = EmptyApplyReq {};
                                            match crate::telemetry_dashboard::http_post_json::<EmptyApplyReq, FlightSetupApplyResponse>(
                                                "/api/flight_setup/apply",
                                                &body,
                                            ).await {
                                                Ok(resp) => {
                                                    flight_setup_status.set(format!(
                                                        "Applied wind {} profile ({} bytes queued).",
                                                        resp.wind_level, resp.payload_bytes
                                                    ));
                                                }
                                                Err(err) => {
                                                    flight_setup_status.set(format!("Flight setup apply failed: {err}"));
                                                }
                                            }
                                            flight_setup_busy.set(false);
                                        });
                                    },
                                    "{translate_text(\"Apply To Flight Computer\")}"
                                }
                                if !flight_setup_status.read().is_empty() {
                                    div { style: "font-size:12px; color:{theme.text_muted};", "{flight_setup_status.read().clone()}" }
                                }
                            }
                            div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 180px), 1fr)); gap:10px; min-width:0;",
                                if let Some(profile) = selected_profile(&cfg) {
                                    {flight_setup_metric("Profile", profile.label.clone(), &theme)}
                                    {flight_setup_metric("Wind", format!("{}", profile.wind_level), &theme)}
                                    {flight_setup_metric("Q Position", format!("{:.3}", profile.kalman.process_position_variance), &theme)}
                                    {flight_setup_metric("Q Velocity", format!("{:.3}", profile.kalman.process_velocity_variance), &theme)}
                                    {flight_setup_metric("Accel R", format!("{:.3}", profile.kalman.accel_variance), &theme)}
                                    {flight_setup_metric("Baro R", format!("{:.3}", profile.kalman.baro_altitude_variance), &theme)}
                                    {flight_setup_metric("GPS Alt R", format!("{:.3}", profile.kalman.gps_altitude_variance), &theme)}
                                    {flight_setup_metric("GPS Vel R", format!("{:.3}", profile.kalman.gps_velocity_variance), &theme)}
                                }
                            }
                        }
                    } else {
                        div { style: "font-size:13px; color:{theme.text_muted};", "{translate_text(\"Loading flight setup…\")}" }
                    }
                }
                }
                if layout.show_fill_targets {
                    div { style: "{setup_panel_style(&theme)}",
                    h3 { style: "margin:0; color:{theme.text_primary};", "{translate_text(\"Fill Targets\")}" }
                    if let Some(cfg) = fill_targets.read().clone() {
                        div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 220px), 1fr)); gap:10px;",
                            FillTargetEditor { key: "nitrogen-{cfg.nitrogen.target_mass_kg:.2}-{cfg.nitrogen.target_pressure_psi:.1}", title: "Nitrogen", field: "nitrogen", target: cfg.nitrogen.clone(), theme: theme.clone(), fill_targets: fill_targets, fill_targets_status: fill_targets_status, enabled: fill_targets_editable }
                            FillTargetEditor { key: "nitrous-{cfg.nitrous.target_mass_kg:.2}-{cfg.nitrous.target_pressure_psi:.1}", title: "Nitrous", field: "nitrous", target: cfg.nitrous.clone(), theme: theme.clone(), fill_targets: fill_targets, fill_targets_status: fill_targets_status, enabled: fill_targets_editable }
                        }
                        button {
                            style: "{apply_button_style(&theme, !*fill_targets_busy.read() && fill_targets_editable)}",
                            disabled: *fill_targets_busy.read() || !fill_targets_editable,
                            onclick: move |_| {
                                if *fill_targets_busy.read() || !fill_targets_editable {
                                    return;
                                }
                                let Some(next_cfg) = fill_targets.read().clone() else {
                                    return;
                                };
                                fill_targets_busy.set(true);
                                fill_targets_status.set("Saving fill targets…".to_string());
                                spawn(async move {
                                    match crate::telemetry_dashboard::http_post_json::<FillTargetsConfig, FillTargetsConfig>(
                                        "/api/fill_targets",
                                        &next_cfg,
                                        ).await {
                                            Ok(saved) => {
                                                backend_fill_targets.set(Some(saved.clone()));
                                                fill_targets.set(Some(saved));
                                                fill_targets_status.set("Fill targets saved.".to_string());
                                            }
                                        Err(err) => {
                                            fill_targets_status.set(format!("Fill targets save failed: {err}"));
                                        }
                                    }
                                    fill_targets_busy.set(false);
                                });
                            },
                            "{translate_text(\"Save Fill Targets\")}"
                        }
                        if !fill_targets_editable {
                            div { style: "font-size:12px; color:{theme.text_muted};", "{translate_text(\"Enable actions to edit fill targets.\")}" }
                        }
                        if !fill_targets_status.read().is_empty() {
                            div { style: "font-size:12px; color:{theme.text_muted};", "{fill_targets_status.read().clone()}" }
                        }
                    } else {
                        div { style: "font-size:13px; color:{theme.text_muted};", "{translate_text(\"Loading fill targets…\")}" }
                    }
                }
                }
            }
        }
    }
}

fn flight_setup_metric(label: &str, value: String, theme: &ThemeConfig) -> Element {
    rsx! {
        div { style: "padding:10px 12px; border-radius:10px; border:1px solid {theme.border}; background:{theme.panel_background_alt};",
            div { style: "font-size:11px; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.08em; margin-bottom:4px;", "{translate_text(label)}" }
            div { style: "font-size:14px; color:{theme.text_primary}; font-family: ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace;", "{value}" }
        }
    }
}

fn sanitize_fill_target_input(raw: &str) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '.'))
        .collect()
}

fn commit_fill_target_mass(
    title: &str,
    field: &str,
    draft: &str,
    fill_targets: &mut Signal<Option<FillTargetsConfig>>,
    fill_targets_status: &mut Signal<String>,
) -> Result<String, String> {
    let value = draft
        .trim()
        .parse::<f32>()
        .map_err(|_| format!("Enter a valid target mass for {title}."))?;
    let normalized = if value.abs() < 0.01 {
        if value.is_sign_negative() { -0.01 } else { 0.01 }
    } else {
        value
    };
    let Some(mut next_cfg) = fill_targets.read().clone() else {
        return Err("Fill targets are not loaded yet.".to_string());
    };
    match field {
        "nitrogen" => next_cfg.nitrogen.target_mass_kg = normalized,
        "nitrous" => next_cfg.nitrous.target_mass_kg = normalized,
        _ => {}
    }
    fill_targets.set(Some(next_cfg));
    fill_targets_status.set("Unsaved fill target changes.".to_string());
    Ok(format!("{normalized:.2}"))
}

fn commit_fill_target_pressure(
    title: &str,
    field: &str,
    draft: &str,
    fill_targets: &mut Signal<Option<FillTargetsConfig>>,
    fill_targets_status: &mut Signal<String>,
) -> Result<String, String> {
    let value = draft
        .trim()
        .parse::<f32>()
        .map_err(|_| format!("Enter a valid target pressure for {title}."))?;
    let normalized = value.max(0.0);
    let Some(mut next_cfg) = fill_targets.read().clone() else {
        return Err("Fill targets are not loaded yet.".to_string());
    };
    match field {
        "nitrogen" => next_cfg.nitrogen.target_pressure_psi = normalized,
        "nitrous" => next_cfg.nitrous.target_pressure_psi = normalized,
        _ => {}
    }
    fill_targets.set(Some(next_cfg));
    fill_targets_status.set("Unsaved fill target changes.".to_string());
    Ok(format!("{normalized:.1}"))
}

#[component]
fn FillTargetEditor(
    title: &'static str,
    field: &'static str,
    target: FluidFillTarget,
    theme: ThemeConfig,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    fill_targets_status: Signal<String>,
    enabled: bool,
) -> Element {
    let mut fill_targets = fill_targets;
    let mut fill_targets_status = fill_targets_status;
    let mut mass_draft = use_signal(|| format!("{:.2}", target.target_mass_kg));
    let mut pressure_draft = use_signal(|| format!("{:.1}", target.target_pressure_psi));
    let committed_mass_value = format!("{:.2}", target.target_mass_kg);
    let committed_pressure_value = format!("{:.1}", target.target_pressure_psi);
    let cursor = if enabled { "text" } else { "not-allowed" };
    let opacity = if enabled { "1.0" } else { "0.6" };
    rsx! {
        div { style: "padding:12px; border-radius:12px; border:1px solid {theme.border}; background:{theme.panel_background_alt}; display:flex; flex-direction:column; gap:10px; opacity:{opacity};",
            div { style: "font-size:14px; font-weight:700; color:{theme.text_primary};", "{translate_text(title)}" }
            div { style: "display:flex; flex-direction:column; gap:6px;",
                label { style: "font-size:12px; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.08em;", "{translate_text(\"Target mass (kg)\")}" }
                input {
                    r#type: "text",
                    inputmode: "decimal",
                    disabled: !enabled,
                    style: "{input_style(&theme)} cursor:{cursor};",
                    value: "{mass_draft.read().clone()}",
                    oninput: move |evt| {
                        if !enabled {
                            return;
                        }
                        mass_draft.set(sanitize_fill_target_input(&evt.value()));
                    },
                    onblur: move |_| {
                        if !enabled {
                            mass_draft.set(committed_mass_value.clone());
                            return;
                        }
                        let current_draft = mass_draft.read().clone();
                        match commit_fill_target_mass(
                            title,
                            field,
                            current_draft.as_str(),
                            &mut fill_targets,
                            &mut fill_targets_status,
                        ) {
                            Ok(next) => mass_draft.set(next),
                            Err(err) => {
                                fill_targets_status.set(err);
                                mass_draft.set(committed_mass_value.clone());
                            }
                        }
                    }
                }
            }
            div { style: "display:flex; flex-direction:column; gap:6px;",
                label { style: "font-size:12px; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.08em;", "{translate_text(\"Target pressure (psi)\")}" }
                input {
                    r#type: "text",
                    inputmode: "decimal",
                    disabled: !enabled,
                    style: "{input_style(&theme)} cursor:{cursor};",
                    value: "{pressure_draft.read().clone()}",
                    oninput: move |evt| {
                        if !enabled {
                            return;
                        }
                        pressure_draft.set(sanitize_fill_target_input(&evt.value()));
                    },
                    onblur: move |_| {
                        if !enabled {
                            pressure_draft.set(committed_pressure_value.clone());
                            return;
                        }
                        let current_draft = pressure_draft.read().clone();
                        match commit_fill_target_pressure(
                            title,
                            field,
                            current_draft.as_str(),
                            &mut fill_targets,
                            &mut fill_targets_status,
                        ) {
                            Ok(next) => pressure_draft.set(next),
                            Err(err) => {
                                fill_targets_status.set(err);
                                pressure_draft.set(committed_pressure_value.clone());
                            }
                        }
                    }
                }
            }
        }
    }
}
