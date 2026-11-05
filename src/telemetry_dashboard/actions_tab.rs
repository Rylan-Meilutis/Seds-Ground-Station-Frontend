// frontend/src/telemetry_dashboard/actions_tab.rs

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth;

use super::blink::{ACTION_BLINK_CSS, action_animation_style};
use super::layout::{ActionSpec, ActionsTabLayout, ThemeConfig};
use super::{
    ActionPolicyMsg, BlinkMode, FillTargetsConfig, FluidFillTarget, RecordingStatusMsg,
    action_buttons_disabled_message, action_policy_control_enabled, command_feedback_active,
    translate_text,
};

// These controls must remain available even in older or custom layouts.
fn ensure_sequence_actions(actions: &mut Vec<ActionSpec>, theme: &ThemeConfig) {
    for (cmd, label) in [
        ("ValveSelfTest", "Valve self-test"),
        ("NitrogenTest", "Nitrogen test"),
        ("StartFill", "Start fill"),
        ("PauseFill", "Pause fill"),
        ("CancelFill", "Cancel fill"),
        ("Turbocharge", "Turbocharge"),
    ] {
        if !actions.iter().any(|action| action.cmd == cmd) {
            actions.push(serde_json::from_value::<ActionSpec>(serde_json::json!({"cmd":cmd,"label":label,"group":"GSE sequence actions","border":theme.border,"bg":theme.panel_background_alt,"fg":theme.text_primary})).expect("valid GSE action specification"));
        }
    }
}

// The main supply control also owns idempotent closing. Unknown/pending feedback
// must never turn a click into an inferred Open request.
fn supply_dispatch_command(cmd: &str, observed: Option<bool>) -> &str {
    match (cmd, observed) {
        ("Nitrogen", Some(false)) | ("Nitrous", Some(false)) => cmd,
        ("Nitrogen", _) => "NitrogenClose",
        ("Nitrous", _) => "NitrousClose",
        _ => cmd,
    }
}

fn redundant_supply_close(cmd: &str) -> bool {
    matches!(cmd, "NitrogenClose" | "NitrousClose")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SupplyGestureOrigin {
    Pointer { id: i32, touch_companion_pending: bool },
    Touch,
    Keyboard,
}
impl SupplyGestureOrigin {
    fn is_companion(self, next: Self) -> bool {
        matches!((self, next), (Self::Pointer { touch_companion_pending: true, .. }, Self::Touch))
    }
    fn owns_pointer(self, id: i32) -> bool {
        matches!(self, Self::Pointer { id: original, .. } if original == id)
    }
    fn finish_pointer(&mut self, id: i32) {
        if let Self::Pointer { id: original, touch_companion_pending } = self && *original == id {
            *touch_companion_pending = false;
        }
    }
}

#[derive(Clone)]
enum SupplyGesture {
    Armed {
        base: String,
        command: String,
        started_ms: f64,
        origin: SupplyGestureOrigin,
        owner: Option<std::rc::Rc<super::recording_download::ExportOwner>>,
    },
    Cancelled,
}
impl SupplyGesture {
    fn current(&self, base: &str, command: &str, now_ms: f64) -> bool {
        match self {
            Self::Armed { base: original, command: chosen, started_ms, owner, .. } => {
                let elapsed = now_ms - started_ms;
                original == base && chosen == command && elapsed.is_finite()
                    && (0. ..=super::COMMAND_MAX_PRESS_RELEASE_MS).contains(&elapsed)
                    && owner.as_ref().is_some_and(|owner| owner.is_current())
            }
            Self::Cancelled => false,
        }
    }
}
fn arm_supply_gesture(gesture: &mut Signal<Option<SupplyGesture>>, base: &str, command: &str, next: SupplyGestureOrigin) -> bool {
    // A new physical gesture retires abandoned intent. Only the touchstart
    // companion of an active touch pointer preserves the first chosen command.
    if matches!(gesture.peek().as_ref(), Some(SupplyGesture::Armed { base: original, origin, owner, .. })
        if original == base && origin.is_companion(next) && owner.as_ref().is_some_and(|owner| owner.is_current())) {
        return false;
    }
    gesture.set(Some(SupplyGesture::Armed {
        base: base.into(), command: command.into(), started_ms: super::monotonic_now_ms(), origin: next,
        owner: super::recording_download::ExportOwner::capture().ok(),
    }));
    true
}

fn finish_supply_pointer(gesture: &mut Signal<Option<SupplyGesture>>, base: &str, id: i32, cancel: bool) {
    let owns = matches!(gesture.peek().as_ref(), Some(SupplyGesture::Armed { base: original, origin, owner, .. })
        if original == base && origin.owns_pointer(id) && owner.as_ref().is_some_and(|owner| owner.is_current()));
    if !owns { return; }
    if cancel {
        gesture.set(Some(SupplyGesture::Cancelled));
    } else if let Some(SupplyGesture::Armed { origin, .. }) = gesture.write().as_mut() {
        // Preserve the chosen click intent through a normal pointerup.
        origin.finish_pointer(id);
    }
}

fn cancel_supply_gesture(gesture: &mut Signal<Option<SupplyGesture>>, base: &str) {
    // Blur from a previously focused valve must not cancel another valve's new press.
    let owned = matches!(gesture.peek().as_ref(), Some(SupplyGesture::Armed { base: original, .. }) if original == base);
    if owned { gesture.set(Some(SupplyGesture::Cancelled)); }
}

#[cfg(test)]
mod sequence_action_tests {
    use super::*;

    #[test]
    fn new_supply_press_retires_abandoned_intent_but_touch_companion_does_not() {
        let mut first = SupplyGestureOrigin::Pointer { id: 7, touch_companion_pending: true };
        assert!(first.is_companion(SupplyGestureOrigin::Touch));
        assert!(!first.is_companion(SupplyGestureOrigin::Pointer { id: 8, touch_companion_pending: true }));
        assert!(!first.is_companion(SupplyGestureOrigin::Keyboard));
        first.finish_pointer(8);
        assert!(first.is_companion(SupplyGestureOrigin::Touch));
        first.finish_pointer(7);
        assert!(!first.is_companion(SupplyGestureOrigin::Touch));
        assert!(!SupplyGestureOrigin::Touch.is_companion(SupplyGestureOrigin::Touch));
        assert!(!SupplyGestureOrigin::Keyboard.is_companion(SupplyGestureOrigin::Keyboard));
        assert!(!SupplyGestureOrigin::Pointer { id: 1, touch_companion_pending: false }.is_companion(SupplyGestureOrigin::Touch));
    }


    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn nitrogen_and_fill_controls_send_the_expected_websocket_command() {
        let mut actions = Vec::new();
        ensure_sequence_actions(&mut actions, &ThemeConfig::default());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = crate::telemetry_dashboard::WsSender { tx };
        for expected in ["NitrogenTest", "StartFill", "Turbocharge"] {
            let button = actions
                .iter()
                .find(|button| button.cmd == expected)
                .unwrap();
            sender.send_cmd(&button.cmd).unwrap();
            let packet: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
            assert_eq!(packet, serde_json::json!({"cmd": expected}));
            assert!(
                rx.try_recv().is_err(),
                "one activation must send one command"
            );
        }
        drop(rx);
        assert!(
            sender.send_cmd("StartFill").is_err(),
            "closed transports must report failure"
        );
    }

    #[test]
    fn all_layouts_retain_fill_nitrogen_and_self_test_without_duplicates() {
        let theme = ThemeConfig::default();
        let mut actions = Vec::new();
        ensure_sequence_actions(&mut actions, &theme);
        assert_eq!(actions.len(), 6);
        assert!(!actions.iter().any(|a| redundant_supply_close(&a.cmd)), "No extra per-supply Close action");
        for cmd in [
            "StartFill",
            "PauseFill",
            "CancelFill",
            "NitrogenTest",
            "ValveSelfTest",
            "Turbocharge",
        ] {
            assert!(actions.iter().any(|action| action.cmd == cmd));
        }
        actions[0].label = "Custom self-test label".into();
        let original = actions.clone();
        ensure_sequence_actions(&mut actions, &theme);
        assert_eq!(actions, original);
    }
    #[test]
    fn cold_flow_row_follows_self_test_once_even_in_a_custom_layout() {
        let mut actions = Vec::new();
        ensure_sequence_actions(&mut actions, &ThemeConfig::default());
        actions[0].label = "Custom dry valve test".into();
        actions.push(actions[0].clone());
        let refs = actions.iter().collect::<Vec<_>>();
        let rows = build_action_rows(&refs);
        let positions = rows.iter().enumerate().filter_map(|(i,row)|matches!(row,ActionLayoutRow::ColdFlow).then_some(i)).collect::<Vec<_>>();
        assert_eq!(positions.len(),1);
        let ActionLayoutRow::Items(items) = &rows[positions[0]-1] else {panic!("Cold Flow must follow valve test controls")};
        assert!(items.iter().any(|item|matches!(item,ActionRowItem::Action(action) if action.cmd == "ValveSelfTest" && action.label == "Custom dry valve test")));
    }
    #[test]
    fn primary_supply_button_closes_unknown_and_never_borrows_close_illumination() {
        for (cmd, close) in [("Nitrogen", "NitrogenClose"), ("Nitrous", "NitrousClose")] {
            assert_eq!(supply_dispatch_command(cmd, Some(false)), cmd);
            assert_eq!(supply_dispatch_command(cmd, Some(true)), close);
            assert_eq!(supply_dispatch_command(cmd, None), close);
            let policy: ActionPolicyMsg = serde_json::from_value(serde_json::json!({"key_enabled":true,"software_buttons_enabled":true,"controls":[{"cmd":cmd,"enabled":false,"blink":"none","actuated":null},{"cmd":close,"enabled":true,"blink":"none","actuated":true}]})).unwrap();
            assert!(action_policy_control_enabled(&policy,supply_dispatch_command(cmd,None)));
            assert_eq!(merge_command_feedback(cmd,None,false),None,"CLOSE policy's inverse position is never physical ON proof");
            let missing: ActionPolicyMsg = serde_json::from_value(serde_json::json!({"key_enabled":true,"software_buttons_enabled":true,"controls":[{"cmd":cmd,"enabled":true,"blink":"none","actuated":null}]})).unwrap();
            assert!(!action_policy_control_enabled(&missing,supply_dispatch_command(cmd,None)));
        }
        assert_eq!(supply_dispatch_command("Abort",None),"Abort");
        assert!(!redundant_supply_close("CancelFill"));
        assert!(!redundant_supply_close("Abort"));
        assert!(!redundant_supply_close("CloseAll"));
    }

}

pub(super) fn btn_style(
    border: &str,
    bg: &str,
    fg: &str,
    enabled: bool,
    blink: BlinkMode,
    actuated: Option<bool>,
    illuminate_enabled: bool,
) -> String {
    let cursor = if enabled { "pointer" } else { "not-allowed" };
    let pointer_events = if enabled { "auto" } else { "none" };
    // A confirmed Off state takes precedence over an available-action glow.
    let actuated_active = actuated.unwrap_or(enabled && illuminate_enabled);
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
    let ready_glow =
        if actuated == Some(true) || (enabled && illuminate_enabled && actuated.is_none()) {
            format!("box-shadow:0 0 14px {border};")
        } else {
            String::new()
        };
    format!(
        "padding:0.35rem 0.65rem; border-radius:0.75rem; cursor:{cursor}; opacity:{opacity}; filter:{filter}; width:100%; min-width:0; height:auto; min-height:2.75rem; flex-shrink:0; box-sizing:border-box; \
         display:flex; align-items:center; justify-content:space-between; gap:0.4rem; flex-wrap:wrap; text-align:left; border:1px solid {border}; background:{bg}; color:{fg}; \
         font-weight:800; box-shadow:{box_shadow}; touch-action:manipulation; pointer-events:{pointer_events}; {animation} {ready_glow}"
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

fn cosmetic_output_action(cmd: &str) -> bool {
    matches!(cmd, "ToggleAvBayUnderglow" | "ToggleFlightBuzzer")
}

fn cosmetic_requested(cmd: &str, requested: Option<bool>, legacy_actuated: Option<bool>) -> Option<bool> {
    cosmetic_output_action(cmd).then_some(requested.or(legacy_actuated)).flatten()
}

fn physical_output_action(cmd: &str) -> bool {
    matches!(
        cmd,
        "Dump"
            | "NormallyOpen"
            | "Nitrogen"
            | "NitrogenClose"
            | "Nitrous"
            | "NitrousClose"
            | "Pilot"
            | "Igniter"
            | "RetractPlumbing"
    )
}

fn merge_command_feedback(cmd: &str, observed: Option<bool>, sent: bool) -> Option<bool> {
    // Neither legacy desired-as-actuated nor local send feedback proves owner state.
    if cosmetic_output_action(cmd) { return None; }
    if physical_output_action(cmd) {
        return observed;
    }
    observed.or(sent.then_some(true))
}

#[cfg(test)]
mod physical_confirmation_tests {
    use super::*;
    #[test]
    fn confirmed_off_is_not_illuminated_by_action_availability() {
        let style = |observed| {
            btn_style(
                "red",
                "black",
                "white",
                true,
                BlinkMode::None,
                observed,
                true,
            )
        };
        assert!(style(Some(true)).contains("opacity:1.0; filter:none;"));
        assert!(style(Some(true)).contains("box-shadow:0 0 14px red;"));
        let off = style(Some(false));
        assert!(off.contains("opacity:0.62;"));
        assert!(!off.contains("box-shadow:0 0 14px"));
        let unknown_output = btn_style("red", "black", "white", true, BlinkMode::None, None, false);
        assert!(unknown_output.contains("opacity:0.62;"));
        assert!(!unknown_output.contains("box-shadow:0 0 14px"));
        let locked_on = btn_style(
            "red",
            "black",
            "white",
            false,
            BlinkMode::None,
            Some(true),
            false,
        );
        assert!(
            locked_on.contains("box-shadow:0 0 14px red;"),
            "permissions do not change confirmed output state"
        );
    }
    #[test]
    fn sending_a_valve_request_does_not_confirm_a_physical_output() {
        for cmd in [
            "Dump",
            "NormallyOpen",
            "Nitrogen",
            "Nitrous",
            "Pilot",
            "Igniter",
            "RetractPlumbing",
        ] {
            assert_eq!(merge_command_feedback(cmd, None, true), None, "{cmd}");
            assert_eq!(
                merge_command_feedback(cmd, Some(false), true),
                Some(false),
                "{cmd}"
            );
            assert_eq!(
                merge_command_feedback(cmd, Some(true), false),
                Some(true),
                "{cmd}"
            );
        }
        assert_eq!(merge_command_feedback("Postinit", None, true), Some(true));
    }
}

fn merged_actuated(
    cmd: &str,
    control_actuated: Option<bool>,
    recording_status: &RecordingStatusMsg,
) -> Option<bool> {
    let local_active = command_feedback_active(cmd);
    let base = control_actuated.or_else(|| recording_command_active(cmd, recording_status));
    merge_command_feedback(cmd, base, local_active)
}

#[derive(Clone, Copy)]
enum ActionRowItem<'a> {
    Action(&'a ActionSpec),
    Spacer,
}

enum ActionLayoutRow<'a> {
    Heading(&'a str),
    Items(Vec<ActionRowItem<'a>>),
    Spacer,
    ColdFlow,
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
    let mut previous_group = "";
    let mut cold_flow_inserted = false;

    for action in actions {
        if !action.group.is_empty() && action.group != previous_group {
            flush_action_row(&mut rows, &mut current_row);
            rows.push(ActionLayoutRow::Heading(&action.group));
            previous_group = &action.group;
        }
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
        if action.cmd == "ValveSelfTest" && !cold_flow_inserted {
            flush_action_row(&mut rows, &mut current_row);
            rows.push(ActionLayoutRow::ColdFlow);
            cold_flow_inserted = true;
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
pub(super) fn NetworkUtcControls() -> Element {
    let mut utc = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let status = auth::current_status();
    if !status.permissions.set_system_time {
        return rsx! {};
    }
    let recovery = status.station_role.as_deref() == Some("rrls");
    rsx! {
        fieldset {
            legend { "Network UTC time" }
            if recovery {
                p { "The primary validates and applies this network UTC request; the RRLS machine clock is unchanged." }
            }
            div {
                label {
                    "UTC date and time "
                    input { r#type: "datetime-local", value: "{utc}", step: "1",
                        oninput: move |event| utc.set(event.value()) }
                }
                button { disabled: busy() || (recovery && !super::COMMAND_PROXY_STATUS.read().as_ref().is_some_and(|s|s.available)), onclick: move |_| {
                    let utc_ms = match super::recording_download::input_utc_ms(&utc()) {
                        Ok(value) => value,
                        Err(error) => { message.set(error); return; }
                    };
                    busy.set(true);
                    spawn(async move {
                        message.set(match super::http_post_json::<_, serde_json::Value>(
                            "/api/network_time", &serde_json::json!({"source":"client","utc_ms":utc_ms})
                        ).await {
                            Ok(value) if recovery => primary_utc_proxy_response(value),
                            Ok(_) => "Network UTC source updated.".into(),
                            Err(error) => format!("Network UTC was not updated: {error}"),
                        });
                        busy.set(false);
                    });
                }, "Set network UTC" }
                button { disabled: busy() || (recovery && !super::COMMAND_PROXY_STATUS.read().as_ref().is_some_and(|s|s.available)), onclick: move |_| {
                    let utc_ms = super::current_wallclock_ms();
                    busy.set(true);
                    spawn(async move {
                        message.set(match super::http_post_json::<_, serde_json::Value>(
                            "/api/network_time", &serde_json::json!({"source":"client","utc_ms":utc_ms})
                        ).await {
                            Ok(value) if recovery => primary_utc_proxy_response(value),
                            Ok(_) => "Network UTC source updated.".into(),
                            Err(error) => format!("Network UTC was not updated: {error}"),
                        });
                        busy.set(false);
                    });
                }, "Use client time" }
                p { "Sets the station's network UTC source. Clock corrections apply gradually; the server system clock is unchanged." }
            }
            p { role: "status", "{message}" }
        }
    }
}

fn request_gate_diagnostics(status: &serde_json::Value, recovery: bool) -> String {
    let scope = if recovery {
        "RRLS local backend"
    } else {
        "Backend gate"
    };
    let Some(gate) = status.get("request_gate") else {
        return format!("{scope}: request-gate diagnostics unavailable.");
    };
    let mode = match gate["hitl_mode"].as_bool() {
        Some(true) => "HITL",
        Some(false) => "non-HITL",
        None => "unknown",
    };
    format!(
        "{scope}: mode={mode} · state={} · prelaunch={} · valve interlock={} · {} · {}",
        gate["flight_state"].as_str().unwrap_or("unknown"),
        gate["prelaunch"]
            .as_bool()
            .map(|value| if value { "yes" } else { "no" })
            .unwrap_or("unknown"),
        gate["button_interlock_satisfied"]
            .as_bool()
            .map(|value| if value { "satisfied" } else { "blocked" })
            .unwrap_or("unknown"),
        status["message"]
            .as_str()
            .unwrap_or("No local sequence status"),
        status["configuration_error"]
            .as_str()
            .unwrap_or("Pressure configuration valid"),
    )
}

#[component]
pub fn ActionsTab(
    layout: ActionsTabLayout,
    action_policy: Signal<ActionPolicyMsg>,
    recording_status: Signal<RecordingStatusMsg>,
    abort_only_mode: bool,
    theme: ThemeConfig,
) -> Element {
    let mut supply_gesture = use_signal(|| None::<SupplyGesture>);
    let mut self_test_confirmation = use_signal(|| super::self_test_confirmation::Model::new(*super::WS_EPOCH.peek()));
    let mut self_test_owner = use_signal(|| None::<std::rc::Rc<super::recording_download::ExportOwner>>);
    let self_test_alive = super::recording_download::use_export_lifetime();
    let mut gse_request_gate = use_signal(String::new);
    let mut operational_status = use_signal(|| serde_json::Value::Null);
    let mut operational_observed_at = use_signal(|| f64::NAN);
    let mut sequence_now = use_signal(super::monotonic_now_ms);
    use_future(move || async move {
        loop {
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(250).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            sequence_now.set(super::monotonic_now_ms());
        }
    });
    let shared_sequence_status=use_context::<Signal<super::sequence_progress_host::StatusObservation>>();
    use_effect(move || {
        let observation=shared_sequence_status.read().clone();
        let current_source=*super::WS_EPOCH.read();
        let _tick=*sequence_now.read();
        let now=super::monotonic_now_ms();
        let mut confirmation=self_test_confirmation.peek().clone();
        let owner_retired=self_test_owner.peek().as_ref().is_some_and(|owner|!owner.is_current());
        if owner_retired {
            confirmation.retire(current_source,now);
            self_test_owner.set(None);
        }
        confirmation.observe(current_source,observation.source_epoch,observation.requested_at_ms,now,
            observation.error.is_none().then(||observation.value["dry_self_test_confirmed"].as_bool()).flatten());
        if *self_test_confirmation.peek()!=confirmation {self_test_confirmation.set(confirmation);}
        if observation.source_epoch!=current_source {
            operational_status.set(serde_json::Value::Null);
            operational_observed_at.set(f64::NAN);
            return;
        }
        let status=observation.value;
        operational_observed_at.set(observation.requested_at_ms);
        operational_status.set(status.clone());
        let next=if let Some(error)=observation.error {format!("Cannot read backend GSE gate: {error}")}
        else {request_gate_diagnostics(&status,auth::current_status().station_role.as_deref()==Some("rrls"))};
        if *gse_request_gate.peek()!=next {gse_request_gate.set(next);}
    });
    let mut all_actions = layout.actions.clone();
    ensure_sequence_actions(&mut all_actions, &theme);
    all_actions.retain(|action| !redundant_supply_close(&action.cmd) && action.cmd != "AutoColdFlow");
    for action in &mut all_actions {
        if matches!(action.cmd.as_str(), "Igniter" | "IgniterSequence") {
            action.group = "Manual GSE valves".into();
        }
    }
    all_actions.sort_by_key(|action| match action.group.as_str() {
        "GSE sequence actions" => 0,
        "Manual GSE valves" => 2,
        _ => 1,
    });
    let visible_actions = if auth::can_view_actions() {
        all_actions.iter().collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let action_rows = build_action_rows(&visible_actions);
    let software_buttons_enabled = action_policy.read().software_buttons_enabled;
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
                "Controls follow backend permissions and interlocks."
            }
            super::board_abort_readiness::BoardAbortReadiness {show_hitl_interlocks:true}
            section {"aria-label":"Recovery actions",
                super::recovery_status::RecoveryStatus {}
                super::recovery_marker::RecoveryMarker {}
            }
            if auth::current_status().station_role.as_deref() == Some("rrls") {
                p {role:"status",style:"margin:0 0 12px;color:{theme.text_muted};font-size:13px;",
                    {super::station_mode::description(&super::station_mode::OPERATING_MODE.read(), true)}
                }
            }
            if auth::current_status().station_role.as_deref() == Some("rrls") {
                p { role:"status", style:"margin:0 0 12px;color:{theme.text_muted};font-size:13px;",
                    {primary_proxy_message(super::COMMAND_PROXY_STATUS.read().as_ref())}
                }
                if let Some(status) = super::COMMAND_PROXY_STATUS.read().as_ref().filter(|status| !status.available) {
                    p {role:"status", "Primary command control unavailable: {status.reason}"}
                }
                if let Some(message) = super::COMMAND_PROXY_MESSAGE.read().clone() {
                    p {role:"status", "{message}"}
                }
            }
            if auth::current_status().station_role.as_deref() == Some("rrls") {
                PrimaryCommandRequest {abort_only_mode}
            }
            if let Some(error) = super::COMMAND_SEND_ERROR.read().clone() {
                div {role:"alert",style:"padding:8px 10px;border-radius:8px;color:{theme.error_text};background:{theme.error_background};border:1px solid {theme.error_border};","{error}"}
            }
            if abort_only_mode {
                div {
                    style: "margin:0; padding:6px 10px; border-radius:8px; border:1px solid {theme.error_border}; background:{theme.error_background}; color:{theme.error_text}; font-size:11px; line-height:1.25;",
                    "{translate_text(\"Disable Actions is enabled. All action and flight-state buttons except Abort are disabled.\")}"
                }
            }
            if let Some(message) = super::BOARD_ABORT_SIGNAL.read().launch_message() {
                p { role: "alert", style: "color:{theme.error_text};", "{message}" }
            }
            if !software_buttons_enabled {
                div {
                    style: "margin:0; padding:8px 10px; border-radius:8px; border:1px solid {theme.warning_border}; background:{theme.warning_background}; color:{theme.warning_text}; font-size:11px; line-height:1.3;",
                    "{action_buttons_disabled_message(&action_policy.read())}"
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
                            ActionLayoutRow::Heading(label) => rsx! {
                                h3 {style:"margin:12px 0 0;color:{theme.text_muted};font-size:12px;letter-spacing:.08em;text-transform:uppercase;","{label}"}
                                if *label=="GSE sequence actions" {
                                    if auth::current_status().station_role.as_deref()==Some("rrls") {
                                        p {"Pressure limits, dry self-test confirmation and qualification changes must be made at the primary ground station."}
                                    }
                                    if operational_status.read()["request_gate"]["flight_state"].as_str() == Some("Aborted") || operational_status.read()["automatic_abort_claimed"].as_bool().unwrap_or(false) {super::gse_panel::ClearGseAbort {}}
                                    p { "Set the empty-tank PT acceptance limit in Settings → Ground Station → GSE limits and plumbing before starting the valve test." }
                                    label {input {r#type:"checkbox",checked:self_test_confirmation.read().confirmed(),disabled:auth::current_status().station_role.as_deref()==Some("rrls")||self_test_confirmation.read().busy()||self_test_confirmation.read().exhausted()||abort_only_mode||!auth::can_send_command("ValveSelfTest"),onchange:{let self_test_alive=self_test_alive.clone();move |event| {
                                        if abort_only_mode||auth::current_status().station_role.as_deref()==Some("rrls")||!auth::can_send_command("ValveSelfTest") {return;}
                                        let mut next=self_test_confirmation.peek().clone();
                                        let ticket=next.begin(event.checked(),*super::WS_EPOCH.peek(),super::monotonic_now_ms());
                                        self_test_confirmation.set(next);
                                        let Some(ticket)=ticket else{return;};
                                        let alive=self_test_alive.clone();
                                        let selected=self_test_confirmation;
                                        let owner=super::recording_download::ExportOwner::capture_selected(move || alive.load(std::sync::atomic::Ordering::Acquire)&&selected.try_peek().is_ok_and(|model|model.owns(ticket)));
                                        let owner=match owner {Ok(owner)=>owner,Err(error)=>{let mut next=self_test_confirmation.peek().clone();next.finish(ticket,Err(error),super::monotonic_now_ms());self_test_confirmation.set(next);return;}};
                                        self_test_owner.set(Some(owner.clone()));
                                        let request=super::station_settings_request::post_captured::<_,serde_json::Value>("/api/gse/self-test-confirmation",&serde_json::json!({"confirmed":ticket.desired}),Ok(owner.clone()));
                                        spawn(async move {
                                            let result=request.await;
                                            if !owner.is_current(){return;}
                                            let mut next=self_test_confirmation.peek().clone();
                                            if next.finish(ticket,result.map(|_|()),super::monotonic_now_ms()){self_test_confirmation.set(next);}
                                        });
                                    }}} " Unlock dry valve self-test: I confirm gas supplies are isolated and the area is clear" }
                                    p {role:"status","{self_test_confirmation.read().notice.text()}"}
                                    super::gse_panel::QualificationStatus {status:operational_status,disabled:abort_only_mode}
                                    details {summary {"Sequence diagnostics"} p {role:"status",style:"font-size:12px;user-select:text;", "{gse_request_gate}"}}
                                }
                            },
                            ActionLayoutRow::ColdFlow => rsx! {
                                super::cold_flow::AutoColdFlow {status:shared_sequence_status,disabled:abort_only_mode,theme:theme.clone()}
                            },
                            ActionLayoutRow::Spacer => rsx! {
                                div {
                                    style: "height:14px;"
                                }
                            },
                            ActionLayoutRow::Items(items) => rsx! {
                                div {
                                    style: "
                                        display:grid;
                                        grid-template-columns: repeat(auto-fit, minmax(min(220px, 100%), 1fr));
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
                                                let mut action_policy_snapshot = action_policy.read().clone();
                                                super::BOARD_ABORT_SIGNAL.read().constrain_launch_policy(&mut action_policy_snapshot);
                                                let control = action_policy_snapshot
                                                    .controls
                                                    .iter()
                                                    .find(|c| c.cmd == action.cmd)
                                                    .cloned();
                                                let is_supply = matches!(action.cmd.as_str(), "Nitrogen" | "Nitrous");
                                                let dispatch_cmd = supply_dispatch_command(&action.cmd, control.as_ref().and_then(|c| c.actuated)).to_owned();
                                                let dispatch_control = action_policy_snapshot.controls.iter().find(|c| c.cmd == dispatch_cmd);
                                                let enabled = action_policy_control_enabled(&action_policy_snapshot, &dispatch_cmd)
                                                    && auth::can_send_command(&dispatch_cmd)
                                                    && (action.cmd!="ValveSelfTest" || self_test_confirmation.read().confirmed())
                                                    && (!abort_only_mode || action.cmd == "Abort")
                                                ;
                                                let disabled_reason = if action.group != "GSE sequence actions" || enabled {
                                                    None
                                                } else if !auth::can_send_command(&dispatch_cmd) {
                                                    Some("Not permitted by your account")
                                                } else if abort_only_mode {
                                                    Some("Abort-only mode is enabled")
                                                } else if !action_policy_snapshot.software_buttons_enabled {
                                                    Some("Software controls disabled by backend")
                                                } else if dispatch_control.is_none() {
                                                    Some("Command missing from backend policy")
                                                } else if !dispatch_control.unwrap().enabled {
                                                    Some("Sequence request disabled by backend")
                                                } else {
                                                    Some("Confirm dry valve self-test first")
                                                };
                                                let reason_text = disabled_reason.unwrap_or("");

                                                let sequence = super::sequence_indicator::progress(
                                                    &action.cmd, &operational_status.read(),
                                                    super::board_recovery_guard::status_age_ms(operational_observed_at(), sequence_now()),
                                                    auth::current_status().station_role.as_deref() == Some("rrls"),
                                                );
                                                let blink = if cosmetic_output_action(&action.cmd) || sequence == Some(super::sequence_indicator::Progress::Unknown) {
                                                    BlinkMode::None
                                                } else { control.as_ref().map(|c| c.blink).unwrap_or(BlinkMode::None) };
                                                let actuated = sequence.map(|state| state.illuminated()).or_else(|| merged_actuated(
                                                    action.cmd.as_str(),
                                                    control.as_ref().and_then(|c| c.actuated),
                                                    &recording_status.read(),
                                                ));
                                                let unconfirmed = (physical_output_action(&action.cmd) || cosmetic_output_action(&action.cmd)) && actuated.is_none();
                                                let requested = cosmetic_requested(&action.cmd, control.as_ref().and_then(|c| c.requested), control.as_ref().and_then(|c| c.actuated));
                                                let requested_label = match requested { Some(true) => "Requested: On", Some(false) => "Requested: Off", None => "Requested: unknown" };
                                                let sequence_label = sequence.map(|state| state.label()).unwrap_or("");
                                                let badge_visibility = if !sequence_label.is_empty() || !enabled || unconfirmed { "visible" } else { "hidden" };
                                                let badge_label = if !sequence_label.is_empty() { sequence_label } else if unconfirmed { "Unconfirmed" } else { "Disabled" };
                                                rsx! {
                                                    button {
                                                        style: "{btn_style(&action.border, &action.bg, &action.fg, enabled, blink, actuated, action.group == \"GSE sequence actions\" && !physical_output_action(&action.cmd))}",
                                                        disabled: !enabled,
                                                        aria_busy: sequence == Some(super::sequence_indicator::Progress::Running),
                                                        "data-sequence-state": sequence_label,
                                                        "data-dispatch-command": "{dispatch_cmd}",
                                                        onpointerdown: {
                                                            let cmd = dispatch_cmd.clone();
                                                            let base = action.cmd.clone();
                                                            move |event| {
                                                                if is_supply && enabled && event.is_primary()
                                                                    && arm_supply_gesture(&mut supply_gesture, &base, &cmd, SupplyGestureOrigin::Pointer { id: event.pointer_id(), touch_companion_pending: event.pointer_type() == "touch" }) {
                                                                    crate::telemetry_dashboard::send_cmd_from_press(&cmd)
                                                                }
                                                            }
                                                        },
                                                        onpointerup: { let base = action.cmd.clone(); move |event| { if is_supply && event.is_primary() { finish_supply_pointer(&mut supply_gesture, &base, event.pointer_id(), false); } } },
                                                        onpointercancel: { let base = action.cmd.clone(); move |event| { if is_supply && event.is_primary() { finish_supply_pointer(&mut supply_gesture, &base, event.pointer_id(), true); } } },
                                                        onmousedown: {
                                                            let cmd = dispatch_cmd.clone();
                                                            move |_| {
                                                                // Supply compatibility mouse events are not new intent.
                                                                // Only primary pointer, touch, or keyboard can arm them.
                                                                if enabled && !is_supply {
                                                                    crate::telemetry_dashboard::send_cmd_from_press(&cmd)
                                                                }
                                                            }
                                                        },
                                                        ontouchstart: {
                                                            let cmd = dispatch_cmd.clone();
                                                            let base = action.cmd.clone();
                                                            move |event| {
                                                                if enabled && (!is_supply || (event.touches().len() == 1 && arm_supply_gesture(&mut supply_gesture, &base, &cmd, SupplyGestureOrigin::Touch))) {
                                                                    crate::telemetry_dashboard::send_cmd_from_press(&cmd)
                                                                }
                                                            }
                                                        },
                                                        ontouchcancel: { let base = action.cmd.clone(); move |_| { if is_supply { cancel_supply_gesture(&mut supply_gesture, &base); } } },
                                                        onmouseleave: { let base = action.cmd.clone(); move |_| { if is_supply { cancel_supply_gesture(&mut supply_gesture, &base); } } },
                                                        onblur: { let base = action.cmd.clone(); move |_| { if is_supply { cancel_supply_gesture(&mut supply_gesture, &base); } } },
                                                        onkeydown: {
                                                            let cmd = dispatch_cmd.clone();
                                                            let base = action.cmd.clone();
                                                            move |event| {
                                                                if !is_supply { return; }
                                                                let key = event.key();
                                                                if matches!(key, Key::Escape | Key::Tab) {
                                                                    cancel_supply_gesture(&mut supply_gesture, &base);
                                                                } else if !event.is_auto_repeating() && (key == Key::Enter || key.to_string() == " ")
                                                                    && enabled && arm_supply_gesture(&mut supply_gesture, &base, &cmd, SupplyGestureOrigin::Keyboard) {
                                                                    crate::telemetry_dashboard::send_cmd_from_press(&cmd);
                                                                }
                                                            }
                                                        },
                                                        onclick: {
                                                            let cmd = dispatch_cmd.clone();
                                                            let base = action.cmd.clone();
                                                            move |_| {
                                                                if is_supply {
                                                                    let original = supply_gesture.write().take();
                                                                    // An unarmed click may be a delayed compatibility
                                                                    // event from a retired component; never infer intent.
                                                                    if original.is_none_or(|gesture| !gesture.current(&base, &cmd, super::monotonic_now_ms())) {
                                                                        super::clear_command_feedback_latches();
                                                                        return;
                                                                    }
                                                                }
                                                                if enabled {
                                                                    crate::telemetry_dashboard::send_cmd_from_click(&cmd)
                                                                }
                                                            }
                                                        },
                                                        span { style: "min-width:0; flex:1 1 auto;",
                                                            span {title:"{action.label}",style:if cosmetic_output_action(&action.cmd) {"display:block;white-space:nowrap;line-height:1.25;"} else {"display:block;overflow-wrap:break-word;white-space:normal;line-height:1.25;"}, "{action.label}"}
                                                            if cosmetic_output_action(&action.cmd) {
                                                                span {style:"display:block;font-size:11px;font-weight:400;line-height:1.3;", "{requested_label}; owner unconfirmed"}
                                                            }
                                                            if action.group == "GSE sequence actions" {
                                                                span {title:disabled_reason.unwrap_or(""),style:"display:block;height:1.3em;overflow:hidden;text-overflow:ellipsis;margin-top:2px;font-size:11px;line-height:1.3;font-weight:400;white-space:nowrap;", "{reason_text}"}
                                                            }
                                                        }
                                                            span {
                                                                "aria-hidden": enabled && !unconfirmed && sequence_label.is_empty(),
                                                                style: "visibility:{badge_visibility};flex:0 0 auto; padding:0.14rem 0.42rem; border-radius:999px; border:1px solid rgba(255,255,255,0.16); background:rgba(0,0,0,0.18); color:rgba(255,255,255,0.82); font-size:0.68rem; font-weight:800; line-height:1; text-transform:uppercase; letter-spacing:0.04em;",
                                                                "{translate_text(badge_label)}"
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
            if operational_status.read()["unavailable"].as_bool()==Some(true) {
                p {role:"alert","Sequence and panel status unavailable"}
            }
            super::gse_panel::PhysicalPanelStatus {status:operational_status}

        }
    }
}

/// Shared mission configuration lives in Ground Station Settings, not Actions.
#[component]
pub(super) fn MissionSettings(
    mut layout: ActionsTabLayout,
    action_policy: Signal<ActionPolicyMsg>,
    backend_fill_targets: Signal<Option<FillTargetsConfig>>,
    abort_only_mode: bool,
    theme: ThemeConfig,
) -> Element {
    let settings_owner = super::station_settings_request::use_factory();
    let abort_only_mode = abort_only_mode || !auth::can_view_actions();
    layout.show_flight_setup = true;
    layout.show_fill_targets = true;
    let mut auto_zero = use_signal(|| None::<bool>);
    let mut auto_zero_busy = use_signal(|| false);
    let mut auto_zero_message = use_signal(String::new);
    use_effect(move || {
        if !settings_owner.current() { return; }
        spawn(async move {
            match settings_owner.get::<serde_json::Value>("/api/auto_zero").await {
                Ok(value) => auto_zero.set(value["enabled"].as_bool()),
                Err(_) if !settings_owner.current() => (),
                Err(error) => {
                    auto_zero_message.set(format!("Automatic zero settings unavailable: {error}"))
                }
            }
        });
    });
    let mut flight_setup = use_signal(|| None::<FlightSetupConfig>);
    let mut flight_setup_status = use_signal(String::new);
    let mut flight_setup_busy = use_signal(|| false);
    let mut fill_targets = use_signal(|| None::<FillTargetsConfig>);
    let mut fill_targets_status = use_signal(String::new);
    let mut fill_targets_busy = use_signal(|| false);
    use_effect(move || {
        if !settings_owner.current() { return; }
        spawn(async move {
            match settings_owner.get::<FlightSetupConfig>(
                "/api/flight_setup",
            )
            .await
            {
                Ok(cfg) => flight_setup.set(Some(cfg)),
                Err(_) if !settings_owner.current() => (),
                Err(err) => flight_setup_status.set(format!("Flight setup load failed: {err}")),
            }
        });
    });
    use_effect(move || {
        if !settings_owner.current() { return; }
        spawn(async move {
            match settings_owner.get::<FillTargetsConfig>(
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
                Err(_) if !settings_owner.current() => (),
                Err(err) => fill_targets_status.set(format!("Fill targets load failed: {err}")),
            }
        });
    });
    use_effect(move || {
        if !settings_owner.current() { return; }
        if fill_targets_status.read().starts_with("Unsaved") {
            return;
        }
        if let Some(cfg) = backend_fill_targets.read().clone() {
            fill_targets.set(Some(cfg));
        }
    });
    let software_buttons_enabled = action_policy.read().software_buttons_enabled;
    let fill_targets_editable = !abort_only_mode
        && (!layout.fill_targets_require_actions_enabled || software_buttons_enabled);
    rsx! {
        section { class: "gs26-station-mission-settings", style: "display:grid;gap:16px;min-width:0;",
            h3 { style: "margin:0;", "Fill and launch" }
            p { "Saved on the connected ground station and shared by its clients." }
            label {
                input { r#type: "checkbox", checked: auto_zero().unwrap_or(false),
                    disabled: auto_zero().is_none() || auto_zero_busy() || abort_only_mode || !auth::can_edit_calibration(),
                    onchange: move |event| {
                        let enabled = event.checked();
                        auto_zero_busy.set(true);
                        spawn(async move {
                            match settings_owner.post::<_, serde_json::Value>("/api/auto_zero", &serde_json::json!({"enabled": enabled})).await {
                                Ok(saved) => { auto_zero.set(saved["enabled"].as_bool()); auto_zero_message.set("Automatic zero setting saved".into()); }
                                Err(_) if !settings_owner.current() => return,
                                Err(error) => auto_zero_message.set(format!("Setting not changed: {error}")),
                            }
                            auto_zero_busy.set(false);
                        });
                    }
                }
                " Auto-zero before fill and launch"
            }
            super::gse_panel::ControlHelp {title:"automatic zero",paragraphs:vec![
                "Fill uses 200 recent samples when stable within ±0.8 kg. Otherwise it captures 200 fresh samples and ignores isolated outliers.".into(),
                "Turning this off preserves the existing tare. Offsets beyond ±0.8 kg require inspection and manual tare.".into()
            ]}
            if !auto_zero_message().is_empty() { p { "{auto_zero_message}" } }
            div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 320px), 1fr)); gap:12px; align-items:start; width:100%; max-width:none; box-sizing:border-box;",
                if layout.show_flight_setup {
                    div { style: "{setup_panel_style(&theme)}",
                    h3 { style: "margin:0; color:{theme.text_primary};", "Flight and weather profile" }
                    if let Some(cfg) = flight_setup.read().clone() {
                        div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 280px), 1fr)); gap:12px; align-items:start; min-width:0;",
                            div { style: "display:flex; flex-direction:column; gap:8px; min-width:0;",
                                label { style: "font-size:12px; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.08em;", "{translate_text(\"Flight profile\")}" }
                                select {
                                    style: "{input_style(&theme)} max-width:100%; min-width:0;",
                                    value: "{cfg.selected_profile_id}",
                                    disabled: abort_only_mode || flight_setup_busy(),
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
                                                match settings_owner.post::<FlightSetupConfig, FlightSetupConfig>(
                                                    "/api/flight_setup",
                                                    &next_cfg,
                                                ).await {
                                                    Ok(saved) => {
                                                        flight_setup.set(Some(saved));
                                                        flight_setup_status.set(format!("Selected profile {next_id}."));
                                                    }
                                                    Err(_) if !settings_owner.current() => (),
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
                                            match settings_owner.post::<EmptyApplyReq, FlightSetupApplyResponse>(
                                                "/api/flight_setup/apply",
                                                &body,
                                            ).await {
                                                Ok(resp) => {
                                                    flight_setup_status.set(format!(
                                                        "Applied wind {} profile ({} bytes queued).",
                                                        resp.wind_level, resp.payload_bytes
                                                    ));
                                                }
                                                Err(_) if !settings_owner.current() => return,
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
                                    match settings_owner.post::<FillTargetsConfig, FillTargetsConfig>(
                                        "/api/fill_targets",
                                        &next_cfg,
                                        ).await {
                                            Ok(saved) => {
                                                backend_fill_targets.set(Some(saved.clone()));
                                                fill_targets.set(Some(saved));
                                                fill_targets_status.set("Fill targets saved.".to_string());
                                            }
                                        Err(_) if !settings_owner.current() => return,
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
        if value.is_sign_negative() {
            -0.01
        } else {
            0.01
        }
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
                    aria_label: "{title} target mass (kg)",
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
                    aria_label: "{title} target pressure (psi)",
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

#[cfg(test)]
mod mode_label_tests {
    use super::request_gate_diagnostics;
    #[test]
    fn local_mode_is_scoped_and_absent_mode_stays_unknown() {
        let mut status =
            serde_json::json!({"request_gate":{"hitl_mode":false,"flight_state":"Idle"}});
        assert!(
            request_gate_diagnostics(&status, true)
                .starts_with("RRLS local backend: mode=non-HITL")
        );
        status["request_gate"]["hitl_mode"] = serde_json::Value::Bool(true);
        assert!(
            request_gate_diagnostics(&status, true).starts_with("RRLS local backend: mode=HITL")
        );
        status["request_gate"]
            .as_object_mut()
            .unwrap()
            .remove("hitl_mode");
        assert!(
            request_gate_diagnostics(&status, true).starts_with("RRLS local backend: mode=unknown")
        );
        assert!(request_gate_diagnostics(&status, false).starts_with("Backend gate: mode=unknown"));
        assert!(
            request_gate_diagnostics(&serde_json::Value::Null, true)
                .contains("diagnostics unavailable")
        );
    }
}

fn primary_proxy_message(status: Option<&auth::CommandProxyStatus>) -> String {
    let Some(status) = status else {
        return "Primary command control is unavailable. This RRLS cannot take standalone hardware authority.".into();
    };
    if let Some(request) = &status.pending {
        return format!(
            "{}: queued at RRLS; waiting for the primary. Device state is unconfirmed.",
            request.operation
        );
    }
    if let Some(request) = &status.last {
        return match request.phase.as_str() {
            "accepted_by_primary" if request.primary_accepted => format!(
                "{}: accepted by the primary. Await fresh board telemetry for device confirmation.",
                request.operation
            ),
            "rejected_by_primary" | "denied" => format!(
                "{}: rejected. {}",
                request.operation,
                request
                    .error
                    .as_deref()
                    .unwrap_or("The primary did not accept this request.")
            ),
            _ => format!(
                "{}: outcome unknown. Check board telemetry before retrying. {}",
                request.operation,
                request.error.as_deref().unwrap_or("")
            ),
        };
    }
    if !status.available {
        return format!("Primary command control unavailable: {}", status.reason);
    }
    "Commands are sent through the primary ground station. Button illumination follows fresh board reports.".into()
}

#[cfg(test)]
mod primary_proxy_tests {
    use super::*;
    #[test]
    fn admission_and_primary_acceptance_never_claim_device_confirmation() {
        let request = auth::CommandProxyRequest {
            request_id: "one".into(),
            operation: "Nitrogen".into(),
            phase: "pending_primary".into(),
            ..Default::default()
        };
        let mut status = auth::CommandProxyStatus {
            pending: Some(request.clone()),
            ..Default::default()
        };
        assert!(primary_proxy_message(Some(&status)).contains("waiting for the primary"));
        status.pending = None;
        status.last = Some(auth::CommandProxyRequest {
            phase: "accepted_by_primary".into(),
            primary_accepted: true,
            device_confirmed: true,
            ..request
        });
        let message = primary_proxy_message(Some(&status));
        assert!(message.contains("Await fresh board telemetry"));
        assert!(!message.contains("device confirmed"));
        status.last.as_mut().unwrap().phase = "denied".into();
        assert!(primary_proxy_message(Some(&status)).contains("rejected"));
        status.last.as_mut().unwrap().phase = "unknown".into();
        assert!(primary_proxy_message(Some(&status)).contains("outcome unknown"));
        assert!(primary_proxy_message(None).contains("cannot take standalone"));
        assert!(
            primary_utc_proxy_response(serde_json::json!({"network_utc_ms":1}))
                .contains("admission is unknown")
        );
    }
}

fn primary_utc_proxy_response(value: serde_json::Value) -> String {
    let Ok(status) = serde_json::from_value::<auth::CommandProxyStatus>(value) else {
        return "Network UTC admission is unknown; the RRLS returned an unsupported response."
            .into();
    };
    let queued = status.supported
        && status.pending.as_ref().is_some_and(|request| {
            request.operation == "NetworkUtc"
                && request.phase == "pending_primary"
                && !request.primary_accepted
        });
    if !queued {
        return "Network UTC admission is unknown; check primary status before retrying.".into();
    }
    // The host-scoped poll publishes live status; this response only describes admission.
    "Network UTC request queued at RRLS; waiting for primary acceptance.".into()
}

#[component]
fn PrimaryCommandRequest(abort_only_mode: bool) -> Element {
    let mut selected = use_signal(String::new);
    let status = super::COMMAND_PROXY_STATUS.read().clone();
    let choices = status
        .as_ref()
        .filter(|status| status.available)
        .map(|status| {
            status
                .supported_commands
                .iter()
                .filter(|command| {
                    auth::can_send_command(command)
                        && (!abort_only_mode || command.as_str() == "Abort")
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let no_choices = choices.is_empty();
    let allowed = choices
        .iter()
        .any(|command| command == selected.read().as_str());
    let pending = status
        .as_ref()
        .is_some_and(|status| status.pending.is_some());
    rsx! {
        fieldset {
            legend {"Request execution at primary"}
            p {"This can actuate hardware if the primary permits it. The primary checks current permissions, mode and interlocks. Stale display state does not grant permission or confirm an output."}
            label {"Command "
                select {"aria-label":"Primary execution command",value:"{selected}",disabled:no_choices||pending,onchange:move |event|selected.set(event.value()),
                    option {value:"","Choose a command"}
                    for command in choices {option {value:"{command}","{command}"}}
                }
            }
            button {disabled:!allowed||pending,onclick:move |_|super::request_primary_command(&selected()),"Request execution if primary permits"}
        }
    }
}

#[cfg(test)]
mod cosmetic_request_truth_tests {
    use super::*;
    #[test]
    fn cosmetic_desire_and_local_send_never_illuminate_as_physical_feedback() {
        for cmd in ["ToggleAvBayUnderglow", "ToggleFlightBuzzer"] {
            for observed in [None, Some(false), Some(true)] {
                for sent in [false, true] {
                    assert_eq!(merge_command_feedback(cmd, observed, sent), None);
                }
            }
            assert_eq!(cosmetic_requested(cmd, Some(false), Some(true)), Some(false));
            assert_eq!(cosmetic_requested(cmd, None, Some(true)), Some(true));
        }
        assert_eq!(merge_command_feedback("NitrogenClose", Some(false), true), Some(false));
        assert_eq!(merge_command_feedback("NitrogenClose", None, true), None);
    }
}
