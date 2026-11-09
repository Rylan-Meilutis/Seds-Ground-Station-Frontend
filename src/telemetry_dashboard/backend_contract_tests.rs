//! Decodes the backend's contract fixtures (see `crate::backend_contract`)
//! with the dashboard's real wire types and checks the fields the UI acts on.
use super::station_mode_contract::{Accepted, Challenge, Confirmation, Snapshot, applied_by};
use super::*;
use crate::backend_contract::{decode, fixture};
use serde_json::Value;

fn snapshot(name: &str) -> Snapshot {
    Snapshot::decode(fixture(name))
        .unwrap_or_else(|e| panic!("the UI rejects backend fixture {name}: {e}"))
}

/// The prepare/confirm round trip the settings page runs, on real backend
/// replies. A challenge without `"version": 2` fails here, as it failed in
/// the app with "Unsupported mode challenge".
#[test]
fn station_mode_review_and_switch_decode_with_the_ui_contract() {
    let before = snapshot("station-modes/snapshot");
    assert!(before.change_supported && before.permissions.can_change, "operator may change mode");
    assert!(before.options.iter().all(|o| o.restarts()));
    let current = before.current.clone().expect("current mode");
    assert_eq!(current.mode_id, "normal");
    assert!(before.reason("hitl", 0.0, 1.0).is_none(), "HITL is offered: {:?}", before.reason("hitl", 0.0, 1.0));

    for (name, target, in_process) in [
        ("station-modes/prepare-in-process", "hitl", true),
        ("station-modes/prepare-restart", "testing-simulation", false),
    ] {
        let challenge: Challenge = serde_json::from_value(fixture(name))
            .unwrap_or_else(|e| panic!("Unsupported mode challenge ({name}): {e}"));
        let binding = before.binding(target).expect("binding for an offered target");
        assert!(challenge.matches(&binding), "{name} does not match the snapshot it reviews");
        assert_eq!(challenge.current, current);
        assert_eq!(challenge.target, *before.option(target).unwrap());
        assert_eq!(challenge.target.switches_in_process(), in_process);
        assert!(challenge.fresh(0.0, 1.0));
        let phrase = challenge.required_confirmation.clone().unwrap();
        assert!(challenge.confirms(&phrase));
    }

    // The receipt answers the third review (the one confirmed).
    let accepted: Accepted = decode("station-modes/confirm-accepted");
    let mut confirmed = fixture("station-modes/prepare-in-process");
    for (key, value) in [("ticket", "fixture-ticket-3"), ("transition_id", "switch-fixture-3")] {
        confirmed[key] = Value::from(value);
    }
    let challenge: Challenge = serde_json::from_value(confirmed).unwrap();
    let request = Confirmation {
        binding: challenge.binding.clone(),
        ticket: challenge.ticket.clone(),
        transition_id: challenge.transition_id.clone(),
        confirmation: challenge.required_confirmation.clone().unwrap(),
    };
    assert!(accepted.matches(&request), "confirmation receipt does not match its request");

    let after = snapshot("station-modes/snapshot-after-switch");
    assert!(applied_by(&request, &challenge.target, &after, 0.0, 1.0), "the switch reads back as applied");
}

#[test]
fn station_mode_changed_frame_decodes_as_a_websocket_message() {
    let message: WsInMsg = decode("websocket/station-mode-changed");
    let WsInMsg::StationModeChanged(change) = message else {
        panic!("not StationModeChanged: {message:?}");
    };
    assert_eq!(change.mode.as_deref(), Some("hitl"));
    assert_eq!(change.label.as_deref(), Some("HITL"));
    let epoch = change.policy_epoch.expect("policy_epoch");
    assert_eq!(Some(epoch), snapshot("station-modes/snapshot-after-switch").policy_epoch);
    assert!(change.layout_revision.is_some());
}

#[test]
fn operating_mode_status_names_the_mode() {
    let mode = fixture("station-mode/status");
    assert_eq!(super::station_mode::description(&mode, false), "Operating mode: Normal");
    assert_eq!(
        super::station_mode::description(&mode, true),
        "RRLS observer · Primary operating mode: Normal · Live operations remain read-only"
    );
}

#[test]
fn board_recovery_status_decodes_and_validates() {
    for (name, user) in [
        ("board-recovery/status-viewer", "viewer"),
        ("board-recovery/status-operator", "operator"),
    ] {
        let status: board_recovery_model::Status = decode(name);
        assert!(status.valid(), "{name} fails the UI's identity checks");
        assert_eq!(status.principal.username.as_deref(), Some(user));
        assert_eq!(status.mode_capability.current, "normal");
        assert!(status.mode_capability.supported_modes.iter().any(|m| m == "hitl"));
        let ids: Vec<_> = status.boards.iter().map(|b| b.id.as_str()).collect();
        for id in ["FC", "AB", "VB"] {
            assert!(ids.contains(&id), "{name} lists {id}: {ids:?}");
        }
        // Raw keys the lenient (all-default) type would otherwise hide.
        let raw = fixture(name);
        for key in ["version", "backend_instance", "principal", "reset_epoch", "boards", "permissions", "mode_capability"] {
            assert!(raw.get(key).is_some(), "{name} lacks {key}");
        }
        for key in ["id", "label", "network", "capabilities", "output_readback", "state_label", "recovery_required"] {
            assert!(raw["boards"][0].get(key).is_some(), "{name} boards lack {key}");
        }
    }
}

#[test]
fn action_policy_carries_network_notes_for_network_outputs() {
    let policy: ActionPolicyMsg = decode("action-policy/get");
    let WsInMsg::ActionPolicy(frame) = decode::<WsInMsg>("websocket/action-policy") else {
        panic!("websocket/action-policy is not an ActionPolicy frame");
    };
    for (name, policy) in [("action-policy/get", policy), ("websocket/action-policy", frame)] {
        assert!(!policy.controls.is_empty(), "{name}");
        let underglow = policy
            .controls
            .iter()
            .find(|c| c.cmd == "ToggleAvBayUnderglow")
            .unwrap_or_else(|| panic!("{name} lists the underglow output"));
        assert_eq!(underglow.network_note.as_deref(), Some("Waiting for network confirmation"), "{name}");
        assert_eq!(underglow.requested, Some(true), "{name}");
        assert_eq!(underglow.actuated, None, "{name}");
        assert!(policy.controls.iter().any(|c| c.cmd == "Abort"), "{name}");
        assert!(policy.controls.iter().filter(|c| c.cmd != "ToggleAvBayUnderglow" && c.cmd != "ToggleFlightBuzzer").all(|c| c.network_note.is_none()), "{name}");
    }
}

#[test]
fn alert_history_and_acknowledgement_decode() {
    let alerts: Vec<AlertDto> = decode("alerts/list");
    assert_eq!(alerts.len(), 2, "cleared events are not listed");
    assert_eq!(alerts[0].severity, "error");
    assert_eq!(alerts[1].severity, "warning");
    assert!(alerts[0].timestamp_ms > alerts[1].timestamp_ms, "newest first");
    assert!(alerts.iter().all(|a| !a.message.is_empty() && a.condition.is_none()));

    let ack: AlertAckStateMsg = decode("alerts/ack");
    assert_eq!(ack.warning_ack_timestamp_ms, alerts[1].timestamp_ms);
    assert_eq!(ack.error_ack_timestamp_ms, alerts[0].timestamp_ms);
    assert!(ack.history_cleared_through_ms > 0, "the clear point is sent, not defaulted");
    assert!(fixture("alerts/ack").get("history_cleared_through_ms").is_some());
    assert!(alerts.iter().all(|a| a.timestamp_ms > ack.history_cleared_through_ms));

    let WsInMsg::AlertAckState(frame) = decode::<WsInMsg>("websocket/alert-ack-state") else {
        panic!("websocket/alert-ack-state is not an AlertAckState frame");
    };
    assert_eq!(frame, ack);
}

#[test]
fn radio_modem_status_uses_the_ui_field_names() {
    let status: radio_modem_health::ModemStatus = decode("radio/modem");
    // Every field is optional in the UI type, so check the names it reads.
    let raw = fixture("radio/modem");
    for key in [
        "available", "profile_name", "opt_in_profile", "rolled_out_all_nodes", "configured_mode", "running",
        "reconfiguration_allowed", "blocked_reason", "needs_attention", "report", "rf_board", "receive_quality",
    ] {
        assert!(raw.get(key).is_some(), "radio/modem lacks {key}");
    }
    for key in ["outcome", "summary", "trigger", "identity", "registers", "warnings", "eeprom_saved", "data_mode"] {
        assert!(raw["report"].get(key).is_some(), "radio/modem report lacks {key}");
    }
    for key in ["number", "name", "observed", "expected", "policy", "status"] {
        assert!(raw["report"]["registers"][0].get(key).is_some(), "modem register rows lack {key}");
    }
    for key in ["good_frames_last_minute", "crc_failures_last_minute"] {
        assert!(raw["receive_quality"].get(key).is_some(), "receive_quality lacks {key}");
    }
    assert!(status.available && status.configured_mode == "fix");
    assert_eq!(radio_modem_health::outcome_label(&status), "Radio modem settings match the profile");
    let report = status.report.as_ref().unwrap();
    assert_eq!(report.registers[0].name.as_deref(), Some("GLOBAL_RETRIES"));
    assert_eq!(report.registers[0].policy, "fix");
    let rf = status.rf_board.as_ref().expect("rf_board");
    for key in ["outcome", "summary", "pending", "needs_attention", "eeprom_saved", "forwarding_entries", "max_data"] {
        assert!(raw["rf_board"].get(key).is_some(), "rf_board lacks {key}");
    }
    assert_eq!(rf.outcome, "matched");
    assert_eq!(status.receive_quality.as_ref().unwrap().good_frames_last_minute, 120);
}

/// No UI code reads `/api/network_diagnostics` yet; this pins the shape of
/// its `radio_tx_budget` for the first consumer.
#[test]
fn radio_tx_budget_shape_is_known() {
    let budget = fixture("network-diagnostics/radio-tx-budget");
    for key in [
        "pending_bytes", "pending_control_bytes", "pending_ordinary_bytes", "parked_frames", "parked_bytes",
        "control_limit_bytes", "ordinary_limit_bytes", "refusals",
    ] {
        assert!(budget[key].is_u64() || budget[key].is_object(), "radio_tx_budget lacks {key}: {budget}");
    }
    assert!(budget["refusals"].as_object().unwrap().values().all(Value::is_u64));
}
