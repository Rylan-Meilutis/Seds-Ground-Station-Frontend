// frontend/src/telemetry_dashboard/actions_tab.rs

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::auth;

use super::action_sections::{self as sections, ActionSection, ControlActivity, SectionSummary};
use super::blink::{ACTION_BLINK_CSS, action_animation_style};
use super::layout::{ActionSpec, ActionsTabLayout, ThemeConfig};
use super::{
    ActionPolicyMsg, BlinkMode, FillTargetsConfig, FluidFillTarget, RecordingStatusMsg,
    action_buttons_disabled_message, action_policy_control_enabled, command_feedback_active,
    translate_text,
};

// Older or custom layouts may omit the GSE sequence buttons. Each is added
// only when the station's action policy lists its command, so a vehicle
// without these sequences never shows them.
fn ensure_sequence_actions(actions: &mut Vec<ActionSpec>, theme: &ThemeConfig, policy: &ActionPolicyMsg) {
    for (cmd, label) in [
        ("ValveSelfTest", "Valve self-test"),
        ("NitrogenTest", "Nitrogen test"),
        ("StartFill", "Start fill"),
        ("PauseFill", "Pause fill"),
        ("CancelFill", "Cancel fill"),
        ("Turbocharge", "Turbocharge"),
    ] {
        let offered = policy.controls.iter().any(|control| control.cmd == cmd);
        if offered && !actions.iter().any(|action| action.cmd == cmd) {
            actions.push(serde_json::from_value::<ActionSpec>(serde_json::json!({"cmd":cmd,"label":label,"group":"GSE sequence actions","section":"sequences","border":theme.border,"bg":theme.panel_background_alt,"fg":theme.text_primary})).expect("valid GSE action specification"));
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
mod network_utc_reason_tests {
    #[test]
    fn rrls_network_utc_says_why_it_is_unavailable() {
        let proxy = |available: bool, reason: &str| -> crate::auth::CommandProxyStatus {
            serde_json::from_value(serde_json::json!({"available":available,"reason":reason,"primary_mode":null,"pending":null,"last":null})).unwrap()
        };
        assert_eq!(super::network_utc_reason(false, None), None);
        assert_eq!(super::network_utc_reason(true, Some(&proxy(true, ""))), None);
        assert_eq!(super::network_utc_reason(true, Some(&proxy(false, "waiting for capability"))).as_deref(), Some("Primary GS link not ready: waiting for capability"));
        assert_eq!(super::network_utc_reason(true, None).as_deref(), Some("Primary GS link not ready"));
    }
}
#[cfg(test)]
mod sequence_action_tests {
    use super::*;

    /// The station's real `/api/action_policy`, which lists every sequence command.
    fn station_policy() -> ActionPolicyMsg {
        crate::backend_contract::decode("action-policy/get")
    }

    #[test]
    fn sequence_buttons_are_added_only_for_commands_the_policy_lists() {
        let theme = ThemeConfig::default();
        let mut policy = station_policy();
        policy.controls.retain(|control| control.cmd != "Turbocharge" && control.cmd != "NitrogenTest");
        let mut actions = Vec::new();
        ensure_sequence_actions(&mut actions, &theme, &policy);
        let cmds = actions.iter().map(|a| a.cmd.as_str()).collect::<Vec<_>>();
        assert_eq!(cmds, ["ValveSelfTest", "StartFill", "PauseFill", "CancelFill"]);

        policy.controls.clear();
        let mut none = Vec::new();
        ensure_sequence_actions(&mut none, &theme, &policy);
        assert!(none.is_empty(), "a vehicle without these commands shows no foreign buttons");
    }

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
        ensure_sequence_actions(&mut actions, &ThemeConfig::default(), &station_policy());
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
        ensure_sequence_actions(&mut actions, &theme, &station_policy());
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
        ensure_sequence_actions(&mut actions, &theme, &station_policy());
        assert_eq!(actions, original);
    }
    #[test]
    fn cold_flow_row_follows_self_test_once_even_in_a_custom_layout() {
        let mut actions = Vec::new();
        ensure_sequence_actions(&mut actions, &ThemeConfig::default(), &station_policy());
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


fn cosmetic_output_action(cmd: &str) -> bool {
    matches!(cmd, "ToggleAvBayUnderglow" | "ToggleFlightBuzzer")
}

fn cosmetic_requested(cmd: &str, requested: Option<bool>, legacy_actuated: Option<bool>) -> Option<bool> {
    cosmetic_output_action(cmd).then_some(requested.or(legacy_actuated)).flatten()
}

/// A backend that reports `requested` sends `actuated` only once the network
/// carries that request. Older backends sent their stored request as
/// `actuated`, which is never network feedback.
fn cosmetic_confirmed(cmd: &str, requested: Option<bool>, actuated: Option<bool>) -> Option<bool> {
    if !cosmetic_output_action(cmd) {
        return None;
    }
    requested.zip(actuated).and_then(|(requested, actuated)| (requested == actuated).then_some(actuated))
}

/// Underglow and buzzer are SEDSnet network variables with no owner board:
/// (secondary line, badge). Plain "Unconfirmed" until the network carries the
/// request; the backend note says when no network value was seen at all.
fn cosmetic_network_labels<'a>(confirmed: bool, network_note: Option<&'a str>) -> (&'a str, &'static str) {
    match (confirmed, network_note.map(str::trim).filter(|note| !note.is_empty())) {
        (true, _) => ("Confirmed", "Unconfirmed"),
        (false, Some(note)) => (note, "Unconfirmed"),
        (false, None) => ("Unconfirmed", "Unconfirmed"),
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

fn merged_actuated(cmd: &str, control_actuated: Option<bool>) -> Option<bool> {
    let local_active = command_feedback_active(cmd);
    merge_command_feedback(cmd, control_actuated, local_active)
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
                super::disabled_reason::DisabledReason {
                    subject: "Network UTC".to_string(),
                    reason: network_utc_reason(recovery, super::COMMAND_PROXY_STATUS.read().as_ref()),
                    control: "network-utc".to_string(),
                }
                super::gse_panel::InfoNote {summary:"About network UTC",paragraphs:vec!["Sets the station's network UTC source. A correction over 1 s steps the network clock at once while steps are allowed (Startup/Idle); smaller ones, and any from Armed through Recovery, slew at 20 ppm. The server system clock is unchanged.".into()]}
            }
            p { role: "status", "{message}" }
        }
    }
}

/// Why the RRLS cannot set network UTC: the primary applies it through the
/// command relay, which must be available.
fn network_utc_reason(recovery: bool, proxy: Option<&auth::CommandProxyStatus>) -> Option<String> {
    if !recovery || proxy.is_some_and(|status| status.available) {
        return None;
    }
    Some(proxy.map(|status| status.reason.trim()).filter(|reason| !reason.is_empty())
        .map(|reason| format!("Primary GS link not ready: {reason}"))
        .unwrap_or_else(|| "Primary GS link not ready".into()))
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
        "{scope}: mode={mode} · detailed state={} · prelaunch={} · valve interlock={} · {} · {}",
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

/// How long the last good GSE status stays on screen while reads fail (matches
/// the self-test confirmation's freshness window).
const STATUS_FRESH_MS: u64 = 10_000;
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
        let window=super::super::FILL_DATA_STALE_MS;
        for age in (0..=window).step_by(250) {assert_eq!(sequence_indicators(&status,age,false),initial);}
        assert_ne!(sequence_indicators(&status,window+1,false),initial);
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
        if !rows.is_empty() {button{r#type:"button",class:"gs-btn",onclick:move |_|open.set(true),"Why are controls unavailable?"}}
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

/// Dashboard controls for returning to Nominal after an abort: clear the
/// station's own abort and acknowledge abort alerts, or reset every system.
/// Board aborts persist on the boards until Reset all systems or Review recovery.
#[component]
pub(super) fn AbortClearBar(theme: ThemeConfig, station_aborted: bool, on_ack: EventHandler<()>) -> Element {
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let permitted = auth::can_send_command("ValveSelfTest");
    rsx!{
        section{role:"region","aria-label":"Abort controls",style:"display:flex;flex-wrap:wrap;gap:var(--gs-space-2);align-items:center;padding:var(--gs-space-3);margin:0 0 var(--gs-space-3);border:1px solid {theme.border_soft};border-radius:var(--gs-radius);background:{theme.panel_background};",
            button{r#type:"button",class:"gs-btn",disabled:*busy.read()||!permitted,
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
            super::board_recovery_panel::ResetAllBoards{label:Some("Reset all systems…".to_string())}
            if !permitted {span{style:"font-size:var(--gs-text-xs);color:var(--gs-text-muted);","Requires command permission."}}
            if !message.read().is_empty(){p{role:"status",class:"gs-notice",style:"flex-basis:100%;","{message}"}}
        }
    }
}

/// The isolation checkbox's request, captured synchronously in its change
/// handler: an ordinary owner (station policy lease) bound to this checkbox
/// ticket, and the POST it sends. `post_captured` refuses an owner without
/// the ordinary policy lease before any I/O, so a non-ordinary owner here
/// means the confirmation never reaches the station.
pub(super) fn isolation_confirmation_request(
    desired: bool,
    selected: impl Fn() -> bool + 'static,
) -> Result<
    (
        std::rc::Rc<super::recording_download::ExportOwner>,
        impl std::future::Future<Output = Result<serde_json::Value, String>> + 'static,
    ),
    String,
> {
    let owner = super::recording_download::ExportOwner::capture_ordinary_selected(selected)?;
    let request = super::station_settings_request::post_captured::<_, serde_json::Value>(
        ISOLATION_CONFIRMATION_PATH,
        &serde_json::json!({"confirmed": desired}),
        Ok(owner.clone()),
    );
    Ok((owner, request))
}
pub(super) const ISOLATION_CONFIRMATION_PATH: &str = "/api/gse/self-test-confirmation";

const GSE_SEQUENCE_GROUP: &str = "GSE sequence actions";

/// Whether an action button can be pressed: unchanged gating, shared by the
/// button and its section header count.
/// Retry SD while one is still pending on the FC.
fn sd_retry_block(cmd: &str) -> Option<&'static str> {
    if cmd != super::igt::SD_RETRY {
        return None;
    }
    super::igt::sd_retry_blocked(super::igt::IGT_STATUS.read().as_ref())
}

fn action_enabled(action_cmd: &str, dispatch_cmd: &str, policy: &ActionPolicyMsg, self_test_confirmed: bool, abort_only_mode: bool) -> bool {
    action_policy_control_enabled(policy, dispatch_cmd)
        && auth::can_send_command(dispatch_cmd)
        && (action_cmd != "ValveSelfTest" || self_test_confirmed)
        && (!abort_only_mode || action_cmd == "Abort")
}

fn action_blink(cmd: &str, control: Option<&super::ActionControl>, sequence: Option<super::sequence_indicator::Progress>) -> BlinkMode {
    if cosmetic_output_action(cmd) || sequence == Some(super::sequence_indicator::Progress::Unknown) {
        BlinkMode::None
    } else {
        control.map(|c| c.blink).unwrap_or(BlinkMode::None)
    }
}

/// What one control contributes to its section header. `pending` covers a
/// press awaiting confirmation, a sent press, a request queued at the primary
/// or a held supply gesture; `rejected`, a request the primary refused.
fn control_activity(
    enabled: bool,
    blink: BlinkMode,
    sequence: Option<super::sequence_indicator::Progress>,
    progress: Option<&super::control_progress::ProgressView>,
    pending: bool,
    rejected: bool,
) -> ControlActivity {
    use super::sequence_indicator::Progress;
    ControlActivity {
        enabled,
        busy: pending
            || progress.is_some_and(|p| p.busy)
            || matches!(sequence, Some(Progress::Running | Progress::Paused)),
        prompt: enabled && blink != BlinkMode::None,
        attention: rejected || progress.is_some_and(|p| matches!(p.state, "failed" | "unconfirmed")),
    }
}

/// Alerts shown inside a section that its header must still signal when
/// collapsed (overpressure, tank sensor fault, a GSE abort).
fn section_alert(section: ActionSection, status: &serde_json::Value) -> bool {
    let overpressure = status["overpressure_latched"].as_bool().unwrap_or(false);
    match section {
        ActionSection::FillSystem => overpressure,
        ActionSection::Sequences => overpressure
            || super::gse_panel::tank_sensor_fault(&status["overpressure"]).is_some()
            || status["request_gate"]["flight_state"].as_str() == Some("Aborted")
            || status["automatic_abort_claimed"].as_bool().unwrap_or(false),
        _ => false,
    }
}

fn proxy_request_state(cmd: &str, dispatch_cmd: &str) -> (bool, bool) {
    let proxy = super::COMMAND_PROXY_STATUS.read();
    let Some(status) = proxy.as_ref() else { return (false, false) };
    let ours = |operation: &str| operation == cmd || operation == dispatch_cmd;
    let pending = status.pending.as_ref().is_some_and(|request| ours(&request.operation));
    let rejected = status.pending.is_none() && status.last.as_ref().is_some_and(|request| {
        ours(&request.operation) && matches!(request.phase.as_str(), "rejected_by_primary" | "denied")
    });
    (pending, rejected)
}

fn section_header_style(theme: &ThemeConfig) -> String {
    format!(
        "width:100%; min-height:44px; box-sizing:border-box; display:flex; flex-wrap:wrap; align-items:center; gap:var(--gs-space-2); \
         padding:0.5rem 0.75rem; border:1px solid {}; border-radius:var(--gs-radius); background:{}; color:{}; \
         font:inherit; font-weight:800; text-align:left; cursor:pointer; touch-action:manipulation;",
        theme.border_soft, theme.panel_background, theme.text_primary
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
    // Operator open/closed choices per storage key (operator + mode), read
    // from storage once; `section_revision` re-renders after a toggle.
    let section_choices = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(HashMap::<String, sections::Choices>::new())));
    let mut section_revision = use_signal(|| 0u64);
    // Sections opened because a control was active: kept open for the session.
    let section_latched = use_hook(|| std::rc::Rc::new(std::cell::RefCell::new(BTreeSet::<(String, ActionSection)>::new())));
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
        // A read that keeps failing leaves the last good status in place with only
        // its error set. Past the freshness window that status (qualification,
        // overpressure, physical panel) is no longer current: say so instead.
        let unavailable=serde_json::json!({"unavailable":true});
        let status=if observation.error.is_some()
            && super::board_recovery_guard::status_age_ms(observation.requested_at_ms,now)>STATUS_FRESH_MS {&unavailable} else {&observation.value};
        if *operational_status.peek()!=*status {operational_status.set(status.clone());}
        let recovery=auth::current_status().station_role.as_deref()==Some("rrls");
        let indicators=sequence_indicators(status,super::board_recovery_guard::status_age_ms(observation.requested_at_ms,now),recovery);
        if *displayed_sequence.peek()!=indicators {displayed_sequence.set(indicators);}
        let next=if let Some(error)=observation.error.as_ref() {format!("Cannot read {} GSE gate: {error}",if recovery {"RRLS local backend"} else {"backend"})}
        else {request_gate_diagnostics(status,recovery)};
        if *gse_request_gate.peek()!=next {gse_request_gate.set(next);}
    });
    let mut all_actions = layout.actions.clone();
    ensure_sequence_actions(&mut all_actions, &theme, &action_policy.read());
    all_actions.retain(|action| !redundant_supply_close(&action.cmd) && action.cmd != "AutoColdFlow");
    // Recover station is listed only while the station reports its own Abort latch.
    let station_recovery = action_policy.read().station_recovery.clone();
    all_actions.retain(|action| action.cmd != super::station_recover::COMMAND || station_recovery.is_some());
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
    let mut action_policy_snapshot = action_policy.read().clone();
    super::BOARD_ABORT_SIGNAL.read().constrain_launch_policy(&mut action_policy_snapshot);
    let software_buttons_enabled = action_policy_snapshot.software_buttons_enabled;
    let operating_mode = sections::OperatingMode::from_effective(
        super::station_mode::current_snapshot()["effective_mode"].as_str(), false);
    let operator = auth::current_status().username;
    let choices_key = sections::storage_key(operator.as_deref(), operating_mode);
    let _ = section_revision();
    let choices = section_choices.borrow_mut().entry(choices_key.clone())
        .or_insert_with(|| sections::load(operator.as_deref(), operating_mode)).clone();
    let pending_confirmation = super::command_confirmation::pending_command();
    let held_supply = match supply_gesture.read().as_ref() { Some(SupplyGesture::Armed { base, .. }) => Some(base.clone()), _ => None };
    let mut by_section: BTreeMap<ActionSection, Vec<&ActionSpec>> = BTreeMap::new();
    let mut summaries: BTreeMap<ActionSection, SectionSummary> = BTreeMap::new();
    let igt_active = super::igt::current_active();
    for &action in &visible_actions {
        let section = ActionSection::of(action.section);
        // HITL lists the ground-test commands; they apply (and show) only
        // with the HITL ground-test replay on.
        if section == ActionSection::GroundTest && !sections::ground_test_controls_shown(operating_mode, igt_active) {
            continue;
        }
        by_section.entry(section).or_default().push(action);
        let control = action_policy_snapshot.controls.iter().find(|c| c.cmd == action.cmd);
        let dispatch_cmd = supply_dispatch_command(&action.cmd, control.and_then(|c| c.actuated));
        let enabled = if action.cmd == super::station_recover::COMMAND {
            station_recovery.as_ref().is_some_and(|view| view.disabled_reason.is_none())
        } else if super::igt::ticket_only(&action.cmd) {
            !abort_only_mode && super::igt_view::ticket_disabled_reason(&action.cmd).is_none()
        } else if action.cmd == super::radio_reject_test::COMMAND {
            super::radio_reject_test::enabled(
                super::radio_reject_test::RADIO_TEST.read().as_ref(),
                auth::current_status().station_role.as_deref() == Some("rrls"),
                *super::WS_CONNECTED_SIGNAL.read(),
            )
        } else {
            action_enabled(&action.cmd, dispatch_cmd, &action_policy_snapshot, self_test_confirmation.read().confirmed(), abort_only_mode)
                && sd_retry_block(&action.cmd).is_none()
        };
        let sequence = SEQUENCE_INDICATOR_COMMANDS.iter().position(|command| *command == action.cmd)
            .and_then(|index| displayed_sequence.read()[index]);
        let progress = super::control_progress::view(control.and_then(|c| c.progress.as_ref()));
        let (proxy_pending, proxy_rejected) = proxy_request_state(&action.cmd, dispatch_cmd);
        let pending = proxy_pending
            || super::command_feedback_active(dispatch_cmd)
            || pending_confirmation.as_deref().is_some_and(|cmd| cmd == action.cmd || cmd == dispatch_cmd)
            || held_supply.as_deref() == Some(action.cmd.as_str())
            || (action.cmd == "ValveSelfTest" && self_test_confirmation.read().busy());
        summaries.entry(section).or_default().add(control_activity(
            enabled, action_blink(&action.cmd, control, sequence), sequence, progress.as_ref(), pending, proxy_rejected));
    }
    let present: BTreeSet<ActionSection> = by_section.keys().copied().collect();
    let sd_retry_in_ground_test = by_section.get(&ActionSection::GroundTest)
        .is_some_and(|actions| actions.iter().any(|action| action.cmd == super::igt::SD_RETRY));
    let section_views: Vec<(ActionSection, bool, SectionSummary, Vec<ActionLayoutRow>)> = sections::arrange_for(operating_mode, &present, igt_active)
        .into_iter()
        .map(|(section, default_open)| {
            let mut summary = summaries.get(&section).copied().unwrap_or_default();
            summary.attention |= section_alert(section, &operational_status.read());
            let latch = (choices_key.clone(), section);
            if summary.active() { section_latched.borrow_mut().insert(latch.clone()); }
            let active = summary.active() || section_latched.borrow().contains(&latch);
            let open = sections::is_open(section, default_open, sections::explicit(&choices, section), active);
            let rows = by_section.get(&section).map(|actions| build_action_rows(actions)).unwrap_or_default();
            (section, open, summary, rows)
        })
        .collect();
    rsx! {
        div {
            class: "gs26-actions-body",
            style: "
                padding:var(--gs-space-4);
                display:flex;
                flex-direction:column;
                gap:var(--gs-space-3);
                width:100%;
                max-width:none;
                box-sizing:border-box;
                align-self:stretch;
            ",
            style { "{ACTION_BLINK_CSS}" }
            h2 { style: "margin:0; color:{theme.text_primary}; font-size:var(--gs-text-lg);", "{translate_text(\"Actions\")}" }
            for (section, open, summary, rows) in section_views.iter() {
                section {
                    key: "{section.id()}",
                    class: "gs26-action-section",
                    "data-section": section.id(),
                    "aria-labelledby": "gs26-actions-section-{section.id()}-heading",
                    style: "display:flex; flex-direction:column; gap:var(--gs-space-3); width:100%; min-width:0; box-sizing:border-box;",
                    if section.collapsible() {
                        h3 { style: "margin:0; min-width:0;",
                            button {
                                r#type: "button",
                                id: "gs26-actions-section-{section.id()}-heading",
                                class: "gs26-action-section-toggle",
                                "aria-expanded": if *open { "true" } else { "false" },
                                "aria-controls": "gs26-actions-section-{section.id()}",
                                "data-badge": summary.badge().unwrap_or(""),
                                style: "{section_header_style(&theme)}",
                                onclick: {
                                    let section = *section;
                                    let open = *open;
                                    let key = choices_key.clone();
                                    let store = section_choices.clone();
                                    let operator = operator.clone();
                                    move |_| {
                                        let mut cache = store.borrow_mut();
                                        let choices = cache.entry(key.clone()).or_default();
                                        sections::toggle(choices, section, open);
                                        sections::save(operator.as_deref(), operating_mode, choices);
                                        drop(cache);
                                        section_revision += 1;
                                    }
                                },
                                span { "aria-hidden": "true", style: "flex:0 0 auto; width:1em;", if *open { "▾" } else { "▸" } }
                                span { style: "flex:1 1 auto; min-width:0; overflow-wrap:anywhere;", "{translate_text(section.label())}" }
                                span { class: "gs26-action-section-count", style: "flex:0 0 auto; font-size:var(--gs-text-xs); font-weight:600; color:{theme.text_muted};", "{summary.count_label()}" }
                                if let Some(badge) = summary.badge() {
                                    span {
                                        class: "gs26-action-section-badge",
                                        style: "flex:0 0 auto; padding:0.14rem 0.5rem; border-radius:999px; border:1px solid {theme.warning_border}; background:{theme.warning_background}; color:{theme.warning_text}; font-size:0.68rem; font-weight:800; line-height:1.2; text-transform:uppercase; letter-spacing:0.04em;",
                                        "{translate_text(badge)}"
                                    }
                                }
                            }
                        }
                    } else {
                        h3 { id: "gs26-actions-section-{section.id()}-heading", class: "gs-section-label", style: "margin:0;", "{translate_text(section.label())}" }
                    }
                    if *open {
                        div {
                            id: "gs26-actions-section-{section.id()}",
                            style: "display:flex; flex-direction:column; gap:var(--gs-space-3); width:100%; max-width:none; min-width:0; box-sizing:border-box; align-self:stretch;",
                            if *section == ActionSection::GroundTest {
                                super::igt_view::GroundTestPanel {inhibited:abort_only_mode,retry_in_section:sd_retry_in_ground_test}
                            }
                            if *section == ActionSection::Safety {
                            super::recording_store_view::RecordingIndicator {status: recording_status}
                            super::igt_view::CurrentLaunchLatch {}
                            super::proximity_lock_view::ProximityActionsNotice {}
                            super::board_abort_readiness::BoardAbortReadiness {show_hitl_interlocks:true}
                            section {"aria-label":"Recovery actions",class:"gs26-actions-stack",
                                super::recovery_status::RecoveryStatus {}
                                super::recovery_marker::RecoveryMarker {}
                            }
                            if auth::current_status().station_role.as_deref() == Some("rrls") {
                                p {role:"status",
                                    {super::station_mode::description(&super::station_mode::OPERATING_MODE.read(), true)}
                                }
                            }
                            if auth::current_status().station_role.as_deref() == Some("rrls") {
                                p { role:"status",
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
                            if let Some(error) = super::COMMAND_SEND_ERROR.read().clone() {
                                div {role:"alert",style:"padding:10px 12px;border-radius:8px;color:{theme.error_text};background:{theme.error_background};border:1px solid {theme.error_border};font-size:0.95rem;line-height:1.45;min-width:0;max-width:100%;overflow-wrap:anywhere;box-sizing:border-box;","{error}"}
                            }
                            if abort_only_mode {
                                p {class:"gs-notice gs-notice--danger",
                                    "{translate_text(\"Disable Actions is enabled. All action and flight-state buttons except Abort are disabled.\")}"
                                }
                            }
                            if !software_buttons_enabled {
                                p {class:"gs-notice gs-notice--warn",
                                    "{action_buttons_disabled_message(&action_policy.read())}"
                                }
                            }
                            div {class:"gs26-actions-row",style:"display:flex;flex-wrap:wrap;gap:var(--gs-space-2);align-items:center;",
                                super::board_recovery_panel::ResetAllBoards {}
                                UnavailableControls {policy:action_policy_snapshot.clone(),actions:visible_actions.iter().map(|action|(action.cmd.clone(),action.label.clone())).collect(),theme:theme.clone()}
                            }
                                if visible_actions.is_empty() {
                                    div {
                                        style: "padding:12px; border:1px solid {theme.border}; border-radius:12px; background:{theme.panel_background}; color:{theme.text_muted}; font-size:13px;",
                                        "{translate_text(\"No actions are available for this user.\")}"
                                    }
                                }
                            }
                            for row in rows.iter() {
                                match row {
                                    ActionLayoutRow::Heading(label) => rsx! {
                                        if *label==GSE_SEQUENCE_GROUP {
                                            h4 {class:"gs-section-label",style:"margin:0;","{label}"}
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
                                            label {for confirm_key in [format!("self-test-confirm-{}-{}", self_test_confirmation.read().serial(), self_test_confirmation.read().busy())] {input {key:"{confirm_key}",r#type:"checkbox",checked:self_test_confirmation.read().confirmed(),disabled:auth::current_status().station_role.as_deref()==Some("rrls")||self_test_confirmation.read().busy()||self_test_confirmation.read().exhausted()||abort_only_mode||!auth::can_send_command("ValveSelfTest"),onchange:{let self_test_alive=self_test_alive.clone();move |event| {
                                                if abort_only_mode||auth::current_status().station_role.as_deref()==Some("rrls")||!auth::can_send_command("ValveSelfTest") {return;}
                                                let mut next=self_test_confirmation.peek().clone();
                                                let ticket=next.begin(event.checked(),*super::WS_EPOCH.peek(),super::monotonic_now_ms());
                                                self_test_confirmation.set(next);
                                                let Some(ticket)=ticket else{return;};
                                                let alive=self_test_alive.clone();
                                                let selected=self_test_confirmation;
                                                let captured=isolation_confirmation_request(ticket.desired,move || alive.load(std::sync::atomic::Ordering::Acquire)&&selected.try_peek().is_ok_and(|model|model.owns(ticket)));
                                                let (owner,request)=match captured {Ok(captured)=>captured,Err(error)=>{let mut next=self_test_confirmation.peek().clone();next.finish(ticket,Err(error),super::monotonic_now_ms());self_test_confirmation.set(next);return;}};
                                                self_test_owner.set(Some(owner.clone()));
                                                spawn(async move {
                                                    let result=request.await;
                                                    if !owner.is_current(){return;}
                                                    let mut next=self_test_confirmation.peek().clone();
                                                    if next.finish(ticket,result.map(|_|()),super::monotonic_now_ms()){self_test_confirmation.set(next);}
                                                });
                                            }}}} " Unlock dry valve self-test: I confirm gas supplies are isolated and the area is clear" }
                                            p {role:"status","{self_test_confirmation.read().notice.text()}"}
                                            super::gse_panel::QualificationStatus {status:operational_status,disabled:abort_only_mode}
                                            super::gse_panel::HitlNitrogenOverride {status:operational_status,disabled:abort_only_mode}
                                            details {summary {"Sequence diagnostics"} p {role:"status",style:"font-size:12px;user-select:text;", "{gse_request_gate}"}}
                                        }
                                    },
                                    ActionLayoutRow::ColdFlow => rsx! {
                                        super::cold_flow::AutoColdFlow {status:shared_sequence_status,disabled:abort_only_mode,theme:theme.clone()}
                                    },
                                    ActionLayoutRow::Spacer => rsx! {
                                        div {
                                            style: "height:var(--gs-space-1);"
                                        }
                                    },
                                    ActionLayoutRow::Items(items) => rsx! {
                                        div {
                                            style: "
                                                display:grid;
                                                grid-template-columns: repeat(auto-fit, minmax(min(220px, 100%), 1fr));
                                                gap:var(--gs-space-3);
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
                                                    ActionRowItem::Action(action) if action.cmd == super::station_recover::COMMAND => rsx! {
                                                        super::station_recover::RecoverStationControl { view: station_recovery.clone().unwrap_or_default() }
                                                    },
                                                    // Enter / LIVE PYRO: ticketed and confirmed, never the plain command path.
                                                    ActionRowItem::Action(action) if super::igt::ticket_only(&action.cmd) => rsx! {
                                                        super::igt_view::IgtTicketControl { cmd: action.cmd.clone(), inhibited: abort_only_mode }
                                                    },
                                                    ActionRowItem::Action(action) if action.cmd == super::radio_reject_test::COMMAND => rsx! {
                                                        super::radio_reject_test::RadioTestControl {}
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
                                                        let sd_blocked = sd_retry_block(&action.cmd);
                                                        let enabled = action_enabled(&action.cmd, &dispatch_cmd, &action_policy_snapshot, self_test_confirmation.read().confirmed(), abort_only_mode)
                                                            && sd_blocked.is_none();
                                                        // A Launch latched closed by a ground-test FC image says so in
                                                        // full, never clipped to the card's two lines.
                                                        let latch_reason = matches!(action.cmd.as_str(), "Launch" | "GroundStationLaunch")
                                                            .then(super::igt::current_launch_latch).flatten()
                                                            .and_then(|latch| super::igt::launch_reason(dispatch_control.and_then(backend_disabled_reason), Some(&latch)));
                                                        // Every disabled action shows why (Launch included), not only
                                                        // the GSE sequence group.
                                                        let proxy_reason = auth::primary_proxy_unavailable_reason(&dispatch_cmd);
                                                        let disabled_reason = if enabled {
                                                            None
                                                        } else if let Some(reason) = proxy_reason.as_deref() {
                                                            Some(reason)
                                                        } else if !auth::can_send_command(&dispatch_cmd) {
                                                            Some("Not permitted by your account")
                                                        } else if abort_only_mode {
                                                            Some("Actions Disabled is on (header toggle)")
                                                        } else if let Some(reason) = latch_reason.as_deref() {
                                                            Some(reason)
                                                        } else if let Some(reason) = sd_blocked {
                                                            Some(reason)
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
                                                        let blink = action_blink(&action.cmd, control.as_ref(), sequence);
                                                        let actuated = sequence.map(|state| state.illuminated()).or_else(|| merged_actuated(
                                                            action.cmd.as_str(),
                                                            control.as_ref().and_then(|c| c.actuated),
                                                        )).or_else(|| cosmetic_confirmed(
                                                            action.cmd.as_str(),
                                                            control.as_ref().and_then(|c| c.requested),
                                                            control.as_ref().and_then(|c| c.actuated),
                                                        ));
                                                        // A timed operation the backend reports (the QD retraction):
                                                        // pulse and count down while it runs, then its outcome.
                                                        let progress = super::control_progress::view(control.as_ref().and_then(|c| c.progress.as_ref()));
                                                        let actuated = progress.as_ref().and_then(|p| p.illuminated).or(actuated);
                                                        let unconfirmed = (physical_output_action(&action.cmd) || cosmetic_output_action(&action.cmd)) && actuated.is_none() && progress.is_none();
                                                        let requested = cosmetic_requested(&action.cmd, control.as_ref().and_then(|c| c.requested), control.as_ref().and_then(|c| c.actuated));
                                                        let requested_label = match requested { Some(true) => "Requested: On", Some(false) => "Requested: Off", None => "Requested: unknown" };
                                                        let network_note = control.as_ref().filter(|_| cosmetic_output_action(&action.cmd)).and_then(|c| c.network_note.as_deref());
                                                        let (network_label, unconfirmed_badge) = cosmetic_network_labels(actuated.is_some(), network_note);
                                                        let sequence_label = sequence.map(|state| state.label()).unwrap_or("");
                                                        let sequence_label = progress.as_ref().map(|p| p.badge).filter(|_| sequence_label.is_empty()).unwrap_or(sequence_label);
                                                        let badge_visibility = if !sequence_label.is_empty() || !enabled || unconfirmed { "visible" } else { "hidden" };
                                                        let badge_label = if !sequence_label.is_empty() { sequence_label } else if unconfirmed { unconfirmed_badge } else { "Disabled" };
                                                        let progress_animation = progress.as_ref().map(|p| p.animation).unwrap_or("");
                                                        let progress_state = progress.as_ref().map(|p| p.state).unwrap_or("");
                                                        let progress_detail = progress.as_ref().map(|p| p.detail.clone()).unwrap_or_default();
                                                        let progress_busy = progress.as_ref().is_some_and(|p| p.busy);
                                                        let command_source = format!("action:{}", action.cmd);
                                                        rsx! {
                                                            button {
                                                                class: "gs26-action-control",
                                                                style: "{btn_style(&action.border, &action.bg, &action.fg, enabled, blink, actuated, action.group == \"GSE sequence actions\" && !physical_output_action(&action.cmd))} {progress_animation}",
                                                                disabled: !enabled,
                                                                aria_busy: sequence == Some(super::sequence_indicator::Progress::Running) || progress_busy,
                                                                "data-progress": progress_state,
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
                                                                        span {title:"{requested_label}; {network_label}",style:"display:block;font-size:11px;font-weight:400;line-height:1.3;white-space:normal;overflow-wrap:anywhere;", "{requested_label}; {network_label}"}
                                                                    }
                                                                    if !progress_detail.is_empty() {
                                                                        span {role:"status",title:"{progress_detail}",style:"display:block;margin-top:2px;font-size:11px;line-height:1.3;font-weight:700;white-space:normal;overflow-wrap:anywhere;", "{progress_detail}"}
                                                                    }
                                                                    if action.cmd == super::igt::SD_RETRY {
                                                                        super::igt_view::SdState {view:super::igt::sd_view(super::igt::IGT_STATUS.read().as_ref())}
                                                                    }
                                                                    if action.group == "GSE sequence actions" && latch_reason.is_none() {
                                                                        span {title:disabled_reason.unwrap_or(""),style:"display:-webkit-box;-webkit-box-orient:vertical;-webkit-line-clamp:2;min-height:1.3em;overflow:hidden;margin-top:2px;font-size:11px;line-height:1.3;font-weight:400;white-space:normal;overflow-wrap:anywhere;", "{reason_text}"}
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
                                if abort_only_mode {
                                    super::disabled_reason::DisabledReason {subject:"Apply".to_string(),reason:Some("Actions Disabled is on (header toggle).".to_string()),control:"apply-flight-setup".to_string()}
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
                            for (field, title, target) in fill_target_editors(&layout, &cfg) {
                                FillTargetEditor { key: "{field}-{target.target_mass_kg:.2}-{target.target_pressure_psi:.1}", title, field, target, theme: theme.clone(), fill_targets: fill_targets, fill_targets_status: fill_targets_status, enabled: fill_targets_editable }
                            }
                        }
                        super::apogee_planner::AppliedPlanNote { theme: theme.clone(), nitrous_mass_kg: f64::from(cfg.nitrous.target_mass_kg), nitrous_psi: f64::from(cfg.nitrous.target_pressure_psi) }
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
    if let Some(target) = next_cfg.fluid_mut(field) {
        target.target_mass_kg = normalized;
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
    if let Some(target) = next_cfg.fluid_mut(field) {
        target.target_pressure_psi = normalized;
    }
    fill_targets.set(Some(next_cfg));
    fill_targets_status.set("Unsaved fill target changes.".to_string());
    Ok(format!("{normalized:.1}"))
}

/// `(field, title, targets)` of each fill-target editor: the layout's
/// `fill_target_fluids` (built-in nitrogen and nitrous when it names none)
/// that the station's fill-targets document carries.
fn fill_target_editors(layout: &ActionsTabLayout, cfg: &FillTargetsConfig) -> Vec<(String, String, FluidFillTarget)> {
    super::vehicle_bindings::fluids_of(&layout.fill_target_fluids)
        .into_iter()
        .filter_map(|(id, label)| cfg.fluid(&id).cloned().map(|target| (id, label, target)))
        .collect()
}

#[component]
fn FillTargetEditor(
    title: String,
    field: String,
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
    let (mass_title, mass_field) = (title.clone(), field.clone());
    let (pressure_title, pressure_field) = (title.clone(), field.clone());
    rsx! {
        div { style: "padding:12px; border-radius:12px; border:1px solid {theme.border}; background:{theme.panel_background_alt}; display:flex; flex-direction:column; gap:10px; opacity:{opacity};",
            div { style: "font-size:14px; font-weight:700; color:{theme.text_primary};", "{translate_text(&title)}" }
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
                            &mass_title,
                            &mass_field,
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
                            &pressure_title,
                            &pressure_field,
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
            // Legacy desired-as-actuated is not network feedback.
            assert_eq!(cosmetic_confirmed(cmd, None, Some(true)), None);
            // Only a network value equal to the request is shown.
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
mod network_note_tests {
    use super::*;
    #[test]
    fn underglow_labels_show_no_owner_text() {
        let waiting = "Waiting for network confirmation";
        let legacy_owner = "RF Board not reachable; applies when it reconnects";
        let cases = [
            (cosmetic_network_labels(false, Some(waiting)), (waiting, "Unconfirmed")),
            (cosmetic_network_labels(false, None), ("Unconfirmed", "Unconfirmed")),
            (cosmetic_network_labels(false, Some("  ")), ("Unconfirmed", "Unconfirmed")),
            (cosmetic_network_labels(true, Some(waiting)), ("Confirmed", "Unconfirmed")),
        ];
        for (labels, expected) in cases {
            assert_eq!(labels, expected);
            for text in [labels.0, labels.1] {
                assert!(!text.to_lowercase().contains("owner") && !text.contains("Board"), "{text}");
            }
        }
        // An older backend's owner note is ignored, never shown.
        let control: super::super::ActionControl = serde_json::from_value(serde_json::json!({
            "cmd":"ToggleAvBayUnderglow","enabled":true,"blink":"none","actuated":null,"requested":true,
            "owner_note":legacy_owner
        })).unwrap();
        assert!(control.network_note.is_none());
        assert_eq!(cosmetic_network_labels(false, control.network_note.as_deref()).0, "Unconfirmed");
    }
    #[test]
    fn network_note_deserializes_and_defaults_to_none() {
        let control: super::super::ActionControl = serde_json::from_value(serde_json::json!({
            "cmd":"ToggleAvBayUnderglow","enabled":true,"blink":"none","actuated":null,"requested":true,
            "network_note":"Waiting for network confirmation"
        })).unwrap();
        assert_eq!(control.network_note.as_deref(), Some("Waiting for network confirmation"));
        let legacy: super::super::ActionControl = serde_json::from_value(serde_json::json!({
            "cmd":"ToggleAvBayUnderglow","enabled":true,"blink":"none","actuated":null
        })).unwrap();
        assert!(legacy.network_note.is_none());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "action_sections_render_tests.rs"]
mod section_render_tests;

#[cfg(test)]
mod fill_target_fluid_tests {
    use super::*;

    fn targets() -> FillTargetsConfig {
        serde_json::from_value(serde_json::json!({"fill_source": "kg50", "version": 1,
            "nitrogen": {"target_mass_kg": 0.5, "target_pressure_psi": 500.0},
            "nitrous": {"target_mass_kg": 8.0, "target_pressure_psi": 750.0}}))
        .unwrap()
    }

    fn titles(layout: &ActionsTabLayout) -> Vec<(String, String)> {
        fill_target_editors(layout, &targets()).into_iter().map(|(field, title, _)| (field, title)).collect()
    }

    /// SEDS: the station's layout names nitrogen and nitrous, the editors
    /// read as before; a layout naming none gets the same two.
    #[test]
    fn fill_target_editors_follow_the_layout_fluids() {
        let file: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/backend-contract/replay-export/flight.json")).unwrap();
        let seds: ActionsTabLayout = serde_json::from_value(file["layout"]["actions_tab"].clone()).unwrap();
        let expected = vec![("nitrogen".to_string(), "Nitrogen".to_string()), ("nitrous".to_string(), "Nitrous".to_string())];
        assert_eq!(titles(&seds), expected);
        let bare: ActionsTabLayout = serde_json::from_value(serde_json::json!({"actions": []})).unwrap();
        assert_eq!(titles(&bare), expected);
        // Another station renames and reorders them; a fluid the fill-targets
        // document does not carry gets no editor.
        let other: ActionsTabLayout = serde_json::from_value(serde_json::json!({"actions": [], "fill_target_fluids": [
            {"id": "nitrous", "label": "Oxidizer"}, {"id": "ethanol", "label": "Fuel"}, {"id": "nitrogen", "label": "Pressurant"}]})).unwrap();
        assert_eq!(titles(&other), [("nitrous".to_string(), "Oxidizer".to_string()), ("nitrogen".to_string(), "Pressurant".to_string())]);
        let (_, _, target) = &fill_target_editors(&other, &targets())[0];
        assert_eq!(target.target_mass_kg, 8.0);
    }

    #[test]
    fn a_layout_naming_another_propellant_still_decodes() {
        let item: super::super::layout::SummaryItem = serde_json::from_value(serde_json::json!(
            {"label": "LOX", "index": 0, "formatter": null, "fill_target_fluid": "lox", "fill_target_kind": "mass_kg"})).unwrap();
        assert_eq!(item.fill_target_fluid.as_deref(), Some("lox"));
    }

    #[test]
    fn edits_reach_the_named_fluid_only() {
        let mut cfg = targets();
        cfg.fluid_mut("nitrous").unwrap().target_mass_kg = 9.0;
        assert_eq!(cfg.nitrous.target_mass_kg, 9.0);
        assert_eq!(cfg.nitrogen.target_mass_kg, 0.5);
        assert!(cfg.fluid_mut("ethanol").is_none());
    }
}
