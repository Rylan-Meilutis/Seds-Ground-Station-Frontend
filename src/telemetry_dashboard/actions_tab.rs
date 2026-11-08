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
    if matches!(gesture.peek().as_ref(), Some(SupplyGesture::Armed { base: original, origin, .. })
        if original == base && origin.is_companion(next)) {
        return false;
    }
    gesture.set(Some(SupplyGesture::Armed {
        base: base.into(), command: command.into(), started_ms: super::monotonic_now_ms(), origin: next,
        owner: super::recording_download::ExportOwner::capture_ordinary_selected(|| true).ok(),
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
        if !super::super::command_owner::isolated_test_child(module_path!(), "nitrogen_and_fill_controls_send_the_expected_websocket_command") { return; }
        let dom=super::super::command_owner::legacy_test_dom("http://legacy-sequence-fixture.invalid");
        dom.in_scope(ScopeId::ROOT, || {
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
            let packet: serde_json::Value = serde_json::from_str(&rx.try_recv().unwrap().wire).unwrap();
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
        });
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
        let mut dom = VirtualDom::new(|| rsx! { div {} });
        dom.rebuild_in_place();
        dom.in_scope(ScopeId::ROOT, || {
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
        });
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

/// A backend that reports `requested` sends `actuated` only once the owner
/// board has acknowledged that request. Older backends sent their stored
/// request as `actuated`, which is never owner feedback.
fn cosmetic_confirmed(cmd: &str, requested: Option<bool>, actuated: Option<bool>) -> Option<bool> {
    if !cosmetic_output_action(cmd) {
        return None;
    }
    requested.zip(actuated).and_then(|(requested, actuated)| (requested == actuated).then_some(actuated))
}

/// Owner wording for underglow/buzzer: (secondary line, badge). A backend
/// note (owner board unreachable) replaces the bare "unconfirmed" text.
fn cosmetic_owner_labels<'a>(confirmed: bool, owner_note: Option<&'a str>) -> (&'a str, &'static str) {
    match (confirmed, owner_note.map(str::trim).filter(|note| !note.is_empty())) {
        (true, _) => ("owner confirmed", "Unconfirmed"),
        (false, Some(note)) => (note, "Owner offline"),
        (false, None) => ("owner unconfirmed", "Unconfirmed"),
    }
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
    let requests = super::station_settings_request::use_factory();
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
                super::gse_panel::InfoNote {summary:"Applied by the primary",paragraphs:vec!["The primary validates and applies this network UTC request; the RRLS machine clock is unchanged.".into()]}
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
                    let request = requests.post::<_, serde_json::Value>(
                        "/api/network_time", &serde_json::json!({"source":"client","utc_ms":utc_ms})
                    );
                    busy.set(true);
                    spawn(async move {
                        let result = request.await;
                        if !requests.current() { return; }
                        message.set(match result {
                            Ok(value) if recovery => primary_utc_proxy_response(value),
                            Ok(_) => "Network UTC source updated.".into(),
                            Err(error) => format!("Network UTC was not updated: {error}"),
                        });
                        busy.set(false);
                    });
                }, "Set network UTC" }
                button { disabled: busy() || (recovery && !super::COMMAND_PROXY_STATUS.read().as_ref().is_some_and(|s|s.available)), onclick: move |_| {
                    let utc_ms = super::current_wallclock_ms();
                    let request = requests.post::<_, serde_json::Value>(
                        "/api/network_time", &serde_json::json!({"source":"client","utc_ms":utc_ms})
                    );
                    busy.set(true);
                    spawn(async move {
                        let result = request.await;
                        if !requests.current() { return; }
                        message.set(match result {
                            Ok(value) if recovery => primary_utc_proxy_response(value),
                            Ok(_) => "Network UTC source updated.".into(),
                            Err(error) => format!("Network UTC was not updated: {error}"),
                        });
                        busy.set(false);
                    });
                }, "Use client time" }
                super::gse_panel::InfoNote {summary:"About network UTC",paragraphs:vec!["Sets the station's network UTC source. Clock corrections apply gradually; the server system clock is unchanged.".into()]}
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

const SEQUENCE_INDICATOR_COMMANDS: [&str; 6] = ["ValveSelfTest", "NitrogenTest", "StartFill", "PauseFill", "CancelFill", "Turbocharge"];
type SequenceIndicators = [Option<super::sequence_indicator::Progress>; 6];
fn sequence_indicators(status: &serde_json::Value, age_ms: u64, rrls: bool) -> SequenceIndicators {
    core::array::from_fn(|index| super::sequence_indicator::progress(SEQUENCE_INDICATOR_COMMANDS[index], status, age_ms, rrls))
}
#[cfg(test)]
mod actions_refresh_tests {
    use super::*;
    #[test]
    fn unchanged_clock_ticks_do_not_change_render_projection_but_expiry_does() {
        let status=serde_json::json!({"phase":"filling","primary_observation":{"fresh":true,"age_ms":400}});
        let initial=sequence_indicators(&status,0,false);
        for age in [0,250,500,750,1000,1250,1500,1750,2000] {assert_eq!(sequence_indicators(&status,age,false),initial);}
        assert_ne!(sequence_indicators(&status,2001,false),initial);
        let remote=sequence_indicators(&status,1600,true);
        assert_eq!(remote,initial);
        assert_ne!(sequence_indicators(&status,1601,true),remote);
        assert_ne!(sequence_indicators(&serde_json::Value::Null,0,false),initial);
        let paused=serde_json::json!({"phase":"paused"});
        assert_ne!(sequence_indicators(&paused,0,false),initial);
    }
}

fn backend_disabled_reason(control: &super::ActionControl) -> Option<&str> {
    (!control.enabled).then_some(control.disabled_reason.as_deref()).flatten()
        .filter(|reason| !reason.trim().is_empty())
}

#[component]
fn UnavailableControls(policy: ActionPolicyMsg, actions: Vec<(String, String)>, theme: ThemeConfig) -> Element {
    let mut open = use_signal(|| false);
    let rows: Vec<_> = actions.iter().filter_map(|(command,label)| {
        let base=policy.controls.iter().find(|control|control.cmd==*command)?;
        let dispatch=supply_dispatch_command(command,base.actuated);
        let control=policy.controls.iter().find(|control|control.cmd==dispatch)?;
        backend_disabled_reason(control).map(|reason|(label.as_str(),reason))
    }).collect();
    if rows.is_empty() && !open() { return rsx!{}; }
    rsx!{
        if !rows.is_empty() {button{r#type:"button",style:"min-height:44px;padding:8px 12px;align-self:flex-start;",onclick:move |_|open.set(true),"Why are controls unavailable?"}}
        if open(){
            div{style:"position:fixed;inset:0;z-index:199999;background:#000a;padding:max(16px,env(safe-area-inset-top)) max(16px,env(safe-area-inset-right)) max(16px,env(safe-area-inset-bottom)) max(16px,env(safe-area-inset-left));display:flex;align-items:center;justify-content:center;box-sizing:border-box;",onkeydown:move|event|{event.stop_propagation();match event.key(){Key::Escape=>{event.prevent_default();open.set(false);},Key::Tab=>{event.prevent_default();super::board_recovery_panel::cycle_modal_focus("gs26-unavailable-controls",event.modifiers().contains(Modifiers::SHIFT));},_=>{}}},
                section{id:"gs26-unavailable-controls",role:"dialog","aria-modal":"true","aria-label":"Unavailable controls",style:"width:min(100%,760px);max-height:88dvh;min-width:0;padding:18px;border-radius:14px;border:1px solid {theme.border};background:{theme.panel_background};color:{theme.text_primary};display:flex;flex-direction:column;gap:12px;box-sizing:border-box;",
                    header{style:"display:flex;justify-content:space-between;align-items:center;gap:12px;",h3{style:"margin:0;","Unavailable controls"}button{r#type:"button",onmounted:move|event|async move{let _=event.data().set_focus(true).await;},onclick:move |_|open.set(false),"Close"}}
                    p{style:"margin:0;font-size:0.9rem;line-height:1.4;","Current reasons reported by GroundStation. Other available controls remain usable."}
                    div{class:"gs26-modal-scroll-body",style:"overflow-y:auto;min-height:0;display:grid;gap:10px;overflow-wrap:anywhere;",
                        if rows.is_empty() {p{role:"status","All displayed controls are now available."}}
                        for (label,reason) in rows {article{style:"padding:12px;border:1px solid {theme.border_soft};border-radius:8px;min-width:0;",strong{"{label}"}p{style:"margin:6px 0 0;line-height:1.45;","{reason}"}}}
                    }
                }
            }
        }
    }
}

/// Keeps the reset next to Launch. The dialog is the existing recovery tool, so
/// it carries the same server-issued ticket and typed confirmation. It owns its
/// status polling only while open.
/// Dashboard controls for returning to Nominal after an abort: clear the
/// station's own abort and acknowledge abort alerts, or reset every system.
/// Board aborts persist on the boards until Reset all systems or Review recovery.
#[component]
pub(super) fn AbortClearBar(theme: ThemeConfig, station_aborted: bool, on_ack: EventHandler<()>) -> Element {
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let permitted = auth::can_send_command("ValveSelfTest");
    rsx!{
        section{role:"region","aria-label":"Abort controls",style:"display:flex;flex-wrap:wrap;gap:8px;align-items:center;padding:10px 12px;margin:0 0 12px;border:1px solid {theme.border_soft};border-radius:10px;background:{theme.panel_background};",
            button{r#type:"button",style:"min-height:44px;padding:8px 12px;",disabled:*busy.read()||!permitted,
                onclick:move |_|{
                    if *busy.peek(){return;}
                    on_ack.call(());
                    if !station_aborted {message.set("Abort alerts acknowledged. Board aborts clear with Reset all systems.".into());return;}
                    let owner=match super::recording_download::ExportOwner::capture_ordinary_selected(||true){Ok(owner)=>owner,Err(error)=>{message.set(error);return;}};
                    let request=super::station_settings_request::post_captured_no_redirect::<_,serde_json::Value>("/api/gse/abort/clear",&serde_json::json!({}),Ok(owner.clone()));
                    busy.set(true);
                    spawn(async move{
                        let result=request.await;
                        if !owner.is_current(){return;}
                        message.set(match result{Ok(_)=>"Station abort cleared and alerts acknowledged. Board aborts clear with Reset all systems.".into(),Err(error)=>error});
                        busy.set(false);
                    });
                },
                "Clear aborts"
            }
            ResetBoardsEntry{theme:theme.clone(),label:Some("Reset all systems…".to_string())}
            if !permitted {span{style:"font-size:12px;","Requires command permission."}}
            if !message.read().is_empty(){p{role:"status",style:"margin:0;flex-basis:100%;font-size:12px;","{message}"}}
        }
    }
}

#[component]
fn ResetBoardsEntry(theme: ThemeConfig, #[props(default)] label: Option<String>) -> Element {
    let mut open = use_signal(|| false);
    let label = label.unwrap_or_else(|| "Reset all boards to Idle…".into());
    rsx!{
        button{r#type:"button",style:"min-height:44px;padding:8px 12px;align-self:flex-start;",onclick:move |_|open.set(true),"{label}"}
        if open(){ResetBoardsDialog{theme:theme.clone(),on_close:move |_|open.set(false)}}
    }
}

#[component]
fn ResetBoardsDialog(theme: ThemeConfig, on_close: EventHandler<()>) -> Element {
    super::board_recovery_panel::use_recovery_provider();
    let ws_connected = *super::WS_CONNECTED_SIGNAL.read();
    rsx!{
        div{style:"position:fixed;inset:0;z-index:9000;background:#000a;padding:max(16px,env(safe-area-inset-top)) max(16px,env(safe-area-inset-right)) max(16px,env(safe-area-inset-bottom)) max(16px,env(safe-area-inset-left));display:flex;align-items:center;justify-content:center;box-sizing:border-box;",
            section{role:"dialog","aria-modal":"true","aria-label":"Reset all boards",style:"width:min(100%,760px);max-height:88dvh;min-width:0;padding:16px;display:flex;flex-direction:column;gap:12px;background:{theme.panel_background};border:1px solid {theme.border_soft};border-radius:12px;box-sizing:border-box;overflow-y:auto;",
                header{style:"display:flex;justify-content:space-between;align-items:center;gap:12px;",
                    h3{style:"margin:0;","Reset all boards"}
                    button{r#type:"button",style:"min-height:44px;padding:8px 12px;",onclick:move |_|on_close.call(()),"Close"}
                }
                super::board_recovery_panel::RecoveryTools{ws_connected,theme:theme.clone()}
            }
        }
        super::board_recovery_panel::RecoveryDialogs{ws_connected}
    }
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
    let mut displayed_sequence = use_signal(|| [None; 6]);
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
        let observation=shared_sequence_status.read();
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
            if !operational_status.peek().is_null() {operational_status.set(serde_json::Value::Null);}
            let next=sequence_indicators(&serde_json::Value::Null,u64::MAX,false);
            if *displayed_sequence.peek()!=next {displayed_sequence.set(next);}
            return;
        }
        let status=&observation.value;
        if *operational_status.peek()!=*status {operational_status.set(status.clone());}
        let recovery=auth::current_status().station_role.as_deref()==Some("rrls");
        let indicators=sequence_indicators(status,super::board_recovery_guard::status_age_ms(observation.requested_at_ms,now),recovery);
        if *displayed_sequence.peek()!=indicators {displayed_sequence.set(indicators);}
        let next=if let Some(error)=observation.error.as_ref() {format!("Cannot read backend GSE gate: {error}")}
        else {request_gate_diagnostics(status,recovery)};
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
    let mut action_policy_snapshot = action_policy.read().clone();
    super::BOARD_ABORT_SIGNAL.read().constrain_launch_policy(&mut action_policy_snapshot);
    let software_buttons_enabled = action_policy_snapshot.software_buttons_enabled;
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
                    if super::settings_navigation::proxy_needs_pairing(status.paired, &status.reason) {
                        super::settings_navigation::StationPairingShortcut {}
                    }
                }
                if let Some(message) = super::COMMAND_PROXY_MESSAGE.read().clone() {
                    p {role:"status", "{message}"}
                }
            }
            if auth::current_status().station_role.as_deref() == Some("rrls") {
                PrimaryCommandRequest {abort_only_mode}
            }
            if let Some(error) = super::COMMAND_SEND_ERROR.read().clone() {
                div {role:"alert",style:"padding:10px 12px;border-radius:8px;color:{theme.error_text};background:{theme.error_background};border:1px solid {theme.error_border};font-size:0.95rem;line-height:1.45;min-width:0;max-width:100%;overflow-wrap:anywhere;box-sizing:border-box;","{error}"}
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
            ResetBoardsEntry {theme:theme.clone()}
            UnavailableControls {policy:action_policy_snapshot.clone(),actions:visible_actions.iter().map(|action|(action.cmd.clone(),action.label.clone())).collect(),theme:theme.clone()}
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
                                    if operational_status.read()["overpressure_latched"].as_bool().unwrap_or(false) {
                                        super::gse_panel::OverpressureAcknowledge {
                                            latched: true,
                                            peak_psi: operational_status.read()["overpressure"]["peak_psi"].as_f64(),
                                            detail: operational_status.read()["overpressure"]["message"].as_str().unwrap_or_default().to_string(),
                                        }
                                    }
                                    super::gse_panel::TankSensorFault { detail: super::gse_panel::tank_sensor_fault(&operational_status.read()["overpressure"]).unwrap_or_default() }
                                    if operational_status.read()["request_gate"]["flight_state"].as_str() == Some("Aborted") || operational_status.read()["automatic_abort_claimed"].as_bool().unwrap_or(false) {super::gse_panel::ClearGseAbort {}}
                                    super::gse_panel::InfoNote {summary:"Before the valve test",paragraphs:vec!["Set the empty-tank PT acceptance limit in Settings → Ground Station → GSE limits and plumbing before starting the valve test.".into()]}
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
                                                // Every disabled action shows why (Launch included), not only
                                                // the GSE sequence group.
                                                let disabled_reason = if enabled {
                                                    None
                                                } else if !auth::can_send_command(&dispatch_cmd) {
                                                    Some("Not permitted by your account")
                                                } else if abort_only_mode {
                                                    Some("Abort-only mode is enabled")
                                                } else if let Some(reason) = dispatch_control.and_then(backend_disabled_reason) {
                                                    Some(reason)
                                                } else if !action_policy_snapshot.software_buttons_enabled {
                                                    Some("Software controls disabled by backend")
                                                } else if dispatch_control.is_none() {
                                                    Some("Command missing from backend policy")
                                                } else if !dispatch_control.unwrap().enabled {
                                                    Some("Disabled by GroundStation")
                                                } else if action.cmd=="ValveSelfTest" {
                                                    Some("Confirm dry valve self-test first")
                                                } else {
                                                    None
                                                };
                                                let reason_text = disabled_reason.unwrap_or("");

                                                let sequence = SEQUENCE_INDICATOR_COMMANDS.iter().position(|command| *command==action.cmd)
                                                    .and_then(|index| displayed_sequence.read()[index]);
                                                let blink = if cosmetic_output_action(&action.cmd) || sequence == Some(super::sequence_indicator::Progress::Unknown) {
                                                    BlinkMode::None
                                                } else { control.as_ref().map(|c| c.blink).unwrap_or(BlinkMode::None) };
                                                let actuated = sequence.map(|state| state.illuminated()).or_else(|| merged_actuated(
                                                    action.cmd.as_str(),
                                                    control.as_ref().and_then(|c| c.actuated),
                                                    &recording_status.read(),
                                                )).or_else(|| cosmetic_confirmed(
                                                    action.cmd.as_str(),
                                                    control.as_ref().and_then(|c| c.requested),
                                                    control.as_ref().and_then(|c| c.actuated),
                                                ));
                                                let unconfirmed = (physical_output_action(&action.cmd) || cosmetic_output_action(&action.cmd)) && actuated.is_none();
                                                let requested = cosmetic_requested(&action.cmd, control.as_ref().and_then(|c| c.requested), control.as_ref().and_then(|c| c.actuated));
                                                let requested_label = match requested { Some(true) => "Requested: On", Some(false) => "Requested: Off", None => "Requested: unknown" };
                                                let owner_note = control.as_ref().filter(|_| cosmetic_output_action(&action.cmd)).and_then(|c| c.owner_note.as_deref());
                                                let (owner_label, unconfirmed_badge) = cosmetic_owner_labels(actuated.is_some(), owner_note);
                                                let sequence_label = sequence.map(|state| state.label()).unwrap_or("");
                                                let badge_visibility = if !sequence_label.is_empty() || !enabled || unconfirmed { "visible" } else { "hidden" };
                                                let badge_label = if !sequence_label.is_empty() { sequence_label } else if unconfirmed { unconfirmed_badge } else { "Disabled" };
                                                let command_source = format!("action:{}", action.cmd);
                                                rsx! {
                                                    button {
                                                        class: "gs26-action-control",
                                                        style: "{btn_style(&action.border, &action.bg, &action.fg, enabled, blink, actuated, action.group == \"GSE sequence actions\" && !physical_output_action(&action.cmd))}",
                                                        disabled: !enabled,
                                                        aria_busy: sequence == Some(super::sequence_indicator::Progress::Running),
                                                        "data-sequence-state": sequence_label,
                                                        "data-dispatch-command": "{dispatch_cmd}",
                                                        onpointerdown: {
                                                            let cmd = dispatch_cmd.clone();
                                                            let base = action.cmd.clone();
                                                            let source = command_source.clone();
                                                            move |event| {
                                                                if enabled {
                                                                    if is_supply && event.is_primary() && (event.pointer_type()=="touch" || event.trigger_button()==Some(dioxus::html::input_data::MouseButton::Primary)) {
                                                                        arm_supply_gesture(&mut supply_gesture, &base, &cmd, SupplyGestureOrigin::Pointer { id:event.pointer_id(), touch_companion_pending:event.pointer_type()=="touch" });
                                                                    }
                                                                    super::command_pointer_down_event(&source, &cmd, &event, false);
                                                                }
                                                            }
                                                        },
                                                        onpointerup: { let base = action.cmd.clone(); let source=command_source.clone(); move |event| { super::command_pointer_up_event(&source,&event); if is_supply && event.is_primary() { finish_supply_pointer(&mut supply_gesture, &base, event.pointer_id(), false); } } },
                                                        onpointercancel: { let base = action.cmd.clone(); let source=command_source.clone(); move |event| { super::command_pointer_cancel_event(&source,&event); if is_supply && event.is_primary() { finish_supply_pointer(&mut supply_gesture, &base, event.pointer_id(), true); } } },
                                                        onmousedown: {
                                                            let cmd = dispatch_cmd.clone();
                                                            let source = command_source.clone();
                                                            move |event| { if enabled { super::command_mouse_down_event(&source, &cmd, &event, false); } }
                                                        },
                                                        onmouseup: {let source=command_source.clone();move |event|{super::command_mouse_up_event(&source,&event);}},
                                                        ontouchstart: {
                                                            let cmd = dispatch_cmd.clone();
                                                            let base = action.cmd.clone();
                                                            let source = command_source.clone();
                                                            move |event| {
                                                                if enabled {
                                                                    let single = event.touches().len()==1;
                                                                    if is_supply && single { arm_supply_gesture(&mut supply_gesture, &base, &cmd, SupplyGestureOrigin::Touch); }
                                                                    super::command_touch_start_event(&source, &cmd, &event, false);
                                                                }
                                                            }
                                                        },
                                                        ontouchend: {let source=command_source.clone();move |event|{super::command_touch_end_event(&source,&event);}},
                                                        ontouchcancel: { let base = action.cmd.clone(); let source=command_source.clone(); move |event| { super::command_touch_cancel_event(&source,&event); if is_supply { cancel_supply_gesture(&mut supply_gesture, &base); } } },
                                                        onmouseleave: { let base = action.cmd.clone(); move |_| { if is_supply { cancel_supply_gesture(&mut supply_gesture, &base); } } },
                                                        onblur: { let base = action.cmd.clone(); move |_| { if is_supply { cancel_supply_gesture(&mut supply_gesture, &base); } } },
                                                        onkeydown: {
                                                            let cmd = dispatch_cmd.clone();
                                                            let base = action.cmd.clone();
                                                            let source = command_source.clone();
                                                            move |event| {
                                                                let key = event.key();
                                                                if matches!(key, Key::Escape | Key::Tab) {
                                                                    if is_supply { cancel_supply_gesture(&mut supply_gesture, &base); }
                                                                } else if !event.is_auto_repeating() && (key==Key::Enter || key.to_string()==" ") && enabled {
                                                                    if is_supply { arm_supply_gesture(&mut supply_gesture, &base, &cmd, SupplyGestureOrigin::Keyboard); }
                                                                    super::command_key_down_event(&source, &cmd, &event, false);
                                                                }
                                                            }
                                                        },
                                                        onkeyup: {let source=command_source.clone();move |event|{super::command_key_up_event(&source,&event);}},
                                                        onclick: {
                                                            let cmd = dispatch_cmd.clone();
                                                            let base = action.cmd.clone();
                                                            let source = command_source.clone();
                                                            move |event| {
                                                                let chosen = if is_supply {
                                                                    let original = supply_gesture.write().take();
                                                                    let Some(gesture) = original else { super::clear_command_feedback_latches(); return; };
                                                                    let chosen = match &gesture { SupplyGesture::Armed {command,..}=>command.clone(), SupplyGesture::Cancelled=>{super::clear_command_feedback_latches();return;} };
                                                                    // Companions cannot choose another command. Changed
                                                                    // feedback cancels this supply press; neither the old
                                                                    // Close nor a newly inferred Open is dispatched.
                                                                    if chosen != cmd || !gesture.current(&base, &chosen, super::monotonic_now_ms()) { super::clear_command_feedback_latches(); return; }
                                                                    chosen
                                                                } else { cmd.clone() };
                                                                if enabled { super::command_click(&source, &chosen, &event, false); }
                                                            }
                                                        },
                                                        span { class:"gs26-action-copy",style: "min-width:0; flex:1 1 auto;",
                                                            span {title:"{action.label}",style:if cosmetic_output_action(&action.cmd) {"display:block;white-space:nowrap;line-height:1.25;"} else {"display:block;overflow-wrap:break-word;white-space:normal;line-height:1.25;"}, "{action.label}"}
                                                            if cosmetic_output_action(&action.cmd) {
                                                                span {title:"{requested_label}; {owner_label}",style:"display:block;font-size:11px;font-weight:400;line-height:1.3;white-space:normal;overflow-wrap:anywhere;", "{requested_label}; {owner_label}"}
                                                            }
                                                            if action.group == "GSE sequence actions" {
                                                                span {title:disabled_reason.unwrap_or(""),style:"display:block;height:1.3em;overflow:hidden;text-overflow:ellipsis;margin-top:2px;font-size:11px;line-height:1.3;font-weight:400;white-space:nowrap;", "{reason_text}"}
                                                            } else if disabled_reason.is_some() {
                                                                span {title:disabled_reason.unwrap_or(""),style:"display:block;margin-top:2px;font-size:11px;line-height:1.3;font-weight:400;white-space:normal;overflow-wrap:anywhere;", "{reason_text}"}
                                                            }
                                                        }
                                                            span {
                                                                class:"gs26-action-badge",
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
    let mut zero_before_launch = use_signal(|| None::<bool>);
    let mut auto_zero_busy = use_signal(|| false);
    let mut auto_zero_message = use_signal(String::new);
    use_effect(move || {
        if !settings_owner.current() { return; }
        spawn(async move {
            match settings_owner.get::<serde_json::Value>("/api/auto_zero").await {
                Ok(value) => {
                    auto_zero.set(value["enabled"].as_bool());
                    // Older stations omit the opt-in field; it is off there.
                    zero_before_launch.set(value["enabled"].as_bool().map(|_| value["before_launch"].as_bool().unwrap_or(false)));
                }
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
                                Ok(saved) => {
                                    auto_zero.set(saved["enabled"].as_bool());
                                    zero_before_launch.set(Some(saved["before_launch"].as_bool().unwrap_or(false)));
                                    auto_zero_message.set("Automatic zero setting saved".into());
                                }
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
            label {
                input { r#type: "checkbox", checked: zero_before_launch().unwrap_or(false),
                    disabled: zero_before_launch().is_none() || auto_zero_busy() || abort_only_mode || !auth::can_edit_calibration(),
                    onchange: move |event| {
                        let before_launch = event.checked();
                        auto_zero_busy.set(true);
                        spawn(async move {
                            match settings_owner.post::<_, serde_json::Value>("/api/auto_zero", &serde_json::json!({"before_launch": before_launch})).await {
                                Ok(saved) => {
                                    auto_zero.set(saved["enabled"].as_bool());
                                    zero_before_launch.set(Some(saved["before_launch"].as_bool().unwrap_or(false)));
                                    auto_zero_message.set("Zero before Launch setting saved".into());
                                }
                                Err(_) if !settings_owner.current() => return,
                                Err(error) => auto_zero_message.set(format!("Setting not changed: {error}")),
                            }
                            auto_zero_busy.set(false);
                        });
                    }
                }
                " Zero loadcell before Launch"
            }
            super::gse_panel::ControlHelp {title:"zero before Launch",paragraphs:vec![
                "Off by default. When on, pressing Launch first zeroes the 1000 kg loadcell (at least 100 samples, up to 10 seconds, same ±0.8 kg check) and sends the tare to the DAQ. Launch, T-10 and the DAQ launch file start only after that, so the launch file uses the new zero.".into(),
                "Cancel, Stop or Abort during the zero cancels the Launch. If the zero fails, Launch is still sent and warns that the previous calibration is in use. When on, it replaces the zero that otherwise runs during the countdown.".into()
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
            div { style: "font-size:14px; color:{theme.text_primary}; font-family:var(--gs-font-mono);", "{value}" }
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
            p {class:"gs-notice gs-notice--warn","Can actuate hardware if the primary permits it."}
            super::gse_panel::InfoNote {summary:"How the primary decides",paragraphs:vec!["The primary checks current permissions, mode and interlocks. Stale display state does not grant permission or confirm an output.".into()]}
            label {"Command "
                select {"aria-label":"Primary execution command",value:"{selected}",disabled:no_choices||pending,onchange:move |event|selected.set(event.value()),
                    option {value:"","Choose a command"}
                    for command in choices {option {value:"{command}","{command}"}}
                }
            }
            button {disabled:!allowed||pending,
                onpointerdown:move |event|{if allowed&&!pending {super::command_pointer_down_event("primary:execution",&selected(),&event,true);}},
                onpointerup:move |event|{super::command_pointer_up_event("primary:execution",&event);},
                onpointercancel:move |event|{super::command_pointer_cancel_event("primary:execution",&event);},
                ontouchstart:move |event|{if allowed&&!pending {super::command_touch_start_event("primary:execution",&selected(),&event,true);}},
                ontouchend:move |event|{super::command_touch_end_event("primary:execution",&event);},
                ontouchcancel:move |event|{super::command_touch_cancel_event("primary:execution",&event);},
                onmousedown:move |event|{if allowed&&!pending {super::command_mouse_down_event("primary:execution",&selected(),&event,true);}},
                onmouseup:move |event|{super::command_mouse_up_event("primary:execution",&event);},
                onkeydown:move |event|{if allowed&&!pending {super::command_key_down_event("primary:execution",&selected(),&event,true);}},
                onkeyup:move |event|{super::command_key_up_event("primary:execution",&event);},
                onclick:move |event|{if allowed&&!pending {super::command_click("primary:execution",&selected(),&event,true);}},
                "Request execution if primary permits"
            }
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
            // Legacy desired-as-actuated is not owner feedback.
            assert_eq!(cosmetic_confirmed(cmd, None, Some(true)), None);
            // Only an owner-acknowledged value equal to the request is shown.
            assert_eq!(cosmetic_confirmed(cmd, Some(false), None), None);
            assert_eq!(cosmetic_confirmed(cmd, Some(false), Some(true)), None);
            assert_eq!(cosmetic_confirmed(cmd, Some(false), Some(false)), Some(false));
        }
        assert_eq!(cosmetic_confirmed("NitrogenClose", Some(true), Some(true)), None);
        assert_eq!(merge_command_feedback("NitrogenClose", Some(false), true), Some(false));
        assert_eq!(merge_command_feedback("NitrogenClose", None, true), None);
    }
}

#[cfg(test)]
mod disabled_reason_tests {
    use super::*;
    #[test]
    fn unavailable_controls_show_only_the_backend_reason_without_new_gating() {
        let mut control: super::super::ActionControl = serde_json::from_value(serde_json::json!({
            "cmd":"Nitrogen","enabled":false,"blink":"none","actuated":null,
            "disabled_reason":"Fill-system manual control is unavailable while the launch sequence is active."
        })).unwrap();
        assert_eq!(backend_disabled_reason(&control),control.disabled_reason.as_deref());
        control.enabled=true;
        assert!(backend_disabled_reason(&control).is_none());
        control.enabled=false;
        control.disabled_reason=None;
        assert!(backend_disabled_reason(&control).is_none());
    }
}

#[cfg(test)]
mod owner_note_tests {
    use super::*;
    #[test]
    fn unreachable_owner_note_replaces_bare_unconfirmed() {
        let note = "RF Board not reachable; applies when it reconnects";
        assert_eq!(cosmetic_owner_labels(false, Some(note)), (note, "Owner offline"));
        assert_eq!(cosmetic_owner_labels(false, None), ("owner unconfirmed", "Unconfirmed"));
        assert_eq!(cosmetic_owner_labels(false, Some("  ")), ("owner unconfirmed", "Unconfirmed"));
        // A confirmed owner never shows a stale note.
        assert_eq!(cosmetic_owner_labels(true, Some(note)).0, "owner confirmed");
    }
    #[test]
    fn owner_note_deserializes_and_defaults_to_none() {
        let control: super::super::ActionControl = serde_json::from_value(serde_json::json!({
            "cmd":"ToggleAvBayUnderglow","enabled":true,"blink":"none","actuated":null,"requested":true,
            "owner_note":"RF Board not reachable; applies when it reconnects"
        })).unwrap();
        assert_eq!(control.owner_note.as_deref(), Some("RF Board not reachable; applies when it reconnects"));
        let legacy: super::super::ActionControl = serde_json::from_value(serde_json::json!({
            "cmd":"ToggleAvBayUnderglow","enabled":true,"blink":"none","actuated":null
        })).unwrap();
        assert!(legacy.owner_note.is_none());
    }
}
