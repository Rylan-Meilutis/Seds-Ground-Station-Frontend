//! Renders the real Actions tab with sectioned controls (isolated child
//! process: it sets the session and writes the operator's section choices).
use super::*;
use super::super::action_sections as sections;

fn spec(cmd: &str, section: Option<&str>) -> serde_json::Value {
    let mut spec = serde_json::json!({"cmd":cmd,"label":format!("Label {cmd}"),"group":"Flight controls",
        "border":"#53657c","bg":"#142033","fg":"#ffffff"});
    if let Some(section) = section { spec["section"] = serde_json::json!(section); }
    spec
}

fn layout() -> ActionsTabLayout {
    serde_json::from_value(serde_json::json!({"actions":[
        spec("Abort", Some("safety")),
        spec("Nitrogen", Some("fill_system")),
        spec("RetractPlumbing", Some("fill_system")),
        spec("Launch", Some("sequences")),
        spec("DeployParachute", Some("flight_computer")),
        spec("ToggleAvBayUnderglow", Some("av_bay")),
        spec("ToggleButtonInterlock", Some("testing")),
        spec("CustomOperatorCommand", None),
    ]})).unwrap()
}

fn policy() -> ActionPolicyMsg {
    let control = |cmd: &str| serde_json::json!({"cmd":cmd,"enabled":true,"blink":"none","actuated":null,"epoch_fence":"ordinary"});
    // The station lists the GSE sequence commands, so their buttons are added.
    let mut controls: Vec<_> = ["Abort","NitrogenClose","Nitrogen","Launch","DeployParachute","ToggleAvBayUnderglow","CustomOperatorCommand",
        "ValveSelfTest","NitrogenTest","StartFill","PauseFill","CancelFill","Turbocharge"]
        .into_iter().map(control).collect();
    // A running QD retraction and a prompted (blinking) HITL interlock.
    let mut qd = control("RetractPlumbing");
    qd["progress"] = serde_json::json!({"state":"running","confirmed":true,"remaining_ms":12_000});
    controls.push(qd);
    let mut prompted = control("ToggleButtonInterlock");
    prompted["blink"] = serde_json::json!("slow");
    controls.push(prompted);
    serde_json::from_value(serde_json::json!({"key_enabled":true,"software_buttons_enabled":true,"controls":controls})).unwrap()
}

#[component]
fn Harness() -> Element {
    use_context_provider(|| Signal::new(super::super::sequence_progress_host::StatusObservation::default()));
    let action_policy = use_signal(policy);
    let recording_status = use_signal(|| serde_json::from_value::<RecordingStatusMsg>(serde_json::json!({"mode":"idle","db_path":null})).unwrap());
    rsx! { ActionsTab { layout: layout(), action_policy, recording_status, abort_only_mode: false, theme: ThemeConfig::default() } }
}

fn render() -> String {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let _runtime = runtime.enter();
    let mut dom = VirtualDom::new(Harness);
    dom.rebuild_in_place();
    dom.render_immediate_to_vec();
    dioxus::ssr::render(&dom)
}

/// The rendered `<section data-section=…>` blocks in page order.
fn sections_in(html: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let marker = "data-section=\"";
    let mut rest = html;
    while let Some(at) = rest.find(marker) {
        let after = &rest[at + marker.len()..];
        let id = after[..after.find('"').unwrap()].to_string();
        let end = after.find(marker).unwrap_or(after.len());
        out.push((id, after[..end].to_string()));
        rest = &after[end..];
    }
    out
}

fn expanded(block: &str) -> Option<bool> {
    if block.contains("aria-expanded=\"true\"") { Some(true) } else if block.contains("aria-expanded=\"false\"") { Some(false) } else { None }
}

#[test]
fn actions_tab_groups_every_control_into_mode_ordered_collapsible_sections() {
    crate::auth::hold_station_identity_for_test();
    if !super::super::command_owner::isolated_test_child(module_path!(), "actions_tab_groups_every_control_into_mode_ordered_collapsible_sections") { return; }
    auth::set_current_session(auth::StoredAuthSession{token:"section-test-token".into(),remember_me:false,
        session:auth::SessionStatus{authenticated:true,permissions:auth::Permissions{view_data:true,send_commands:true,..Default::default()},..Default::default()}});

    // No operator choices yet: Normal-mode order, defaults, empty sections hidden.
    let html = render();
    let shown = sections_in(&html);
    let order: Vec<&str> = shown.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(order, ["safety","sequences","flight_computer","fill_system","av_bay","testing","other"], "{html}");
    assert!(!html.contains("Recovery / reset"), "a section without controls has no header");
    let block = |id: &str| shown.iter().find(|(s, _)| s == id).unwrap().1.clone();
    // Safety: a plain heading, never a toggle, and Abort stays one click away.
    assert_eq!(expanded(&block("safety")), None);
    assert!(block("safety").contains("data-dispatch-command=\"Abort\""));
    // Every layout control renders exactly once.
    for cmd in ["Abort","NitrogenClose","RetractPlumbing","Launch","DeployParachute","ToggleAvBayUnderglow","ToggleButtonInterlock","CustomOperatorCommand","StartFill","ValveSelfTest"] {
        assert_eq!(html.matches(&format!("data-dispatch-command=\"{cmd}\"")).count(), 1, "{cmd}");
    }
    // Headers are buttons with aria-expanded, a count and a badge.
    let fill = block("fill_system");
    assert_eq!(expanded(&fill), Some(true));
    assert!(fill.contains("aria-controls=\"gs26-actions-section-fill_system\""));
    assert!(fill.contains("2 of 2 enabled"), "{fill}");
    assert!(fill.contains("data-badge=\"In progress\""), "running QD retraction marks the header: {fill}");
    assert!(fill.contains("data-progress=\"running\""), "QD pulse retained: {fill}");
    // Testing defaults closed in Normal, but a prompting control keeps it open.
    assert_eq!(expanded(&block("testing")), Some(true));
    assert!(block("testing").contains("data-badge=\"Action prompted\""));
    assert!(block("other").contains("data-dispatch-command=\"CustomOperatorCommand\""));

    // The operator collapsed Fill system and Testing earlier: their choice
    // stands, the headers still signal the running retraction and the prompt,
    // and Safety/Abort cannot be collapsed by any stored value.
    let mut choices = sections::Choices::new();
    choices.insert("fill_system".into(), false);
    choices.insert("testing".into(), false);
    choices.insert("safety".into(), false);
    sections::save(None, sections::OperatingMode::Normal, &choices);
    let html = render();
    let shown = sections_in(&html);
    let block = |id: &str| shown.iter().find(|(s, _)| s == id).unwrap().1.clone();
    let fill = block("fill_system");
    assert_eq!(expanded(&fill), Some(false));
    assert!(!fill.contains("data-dispatch-command"), "collapsed body is not rendered");
    assert!(fill.contains("data-badge=\"In progress\""));
    assert_eq!(expanded(&block("testing")), Some(false));
    assert!(block("testing").contains("data-badge=\"Action prompted\""));
    assert!(block("safety").contains("data-dispatch-command=\"Abort\""));
    assert_eq!(expanded(&block("flight_computer")), Some(true));
}

fn recover_layout() -> ActionsTabLayout {
    serde_json::from_value(serde_json::json!({"actions":[
        spec("Abort", Some("safety")),
        spec("RecoverStation", Some("recovery")),
    ]})).unwrap()
}

#[component]
fn RecoverHarness(policy: ActionPolicyMsg) -> Element {
    use_context_provider(|| Signal::new(super::super::sequence_progress_host::StatusObservation::default()));
    use_hook(|| *super::super::WS_CONNECTED_SIGNAL.write() = true);
    let action_policy = use_signal(|| policy.clone());
    let recording_status = use_signal(|| serde_json::from_value::<RecordingStatusMsg>(serde_json::json!({"mode":"idle","db_path":null})).unwrap());
    rsx! { ActionsTab { layout: recover_layout(), action_policy, recording_status, abort_only_mode: false, theme: ThemeConfig::default() } }
}

fn render_recover(station_recovery: Option<serde_json::Value>) -> String {
    let mut value = serde_json::json!({"key_enabled":true,"software_buttons_enabled":true,
        "controls":[{"cmd":"Abort","enabled":true,"blink":"none","actuated":null}]});
    if let Some(view) = station_recovery { value["station_recovery"] = view; }
    let policy: ActionPolicyMsg = serde_json::from_value(value).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let _runtime = runtime.enter();
    let mut dom = VirtualDom::new_with_props(RecoverHarness, RecoverHarnessProps { policy });
    dom.rebuild_in_place();
    dom.render_immediate_to_vec();
    dioxus::ssr::render(&dom)
}

#[test]
fn recover_station_is_listed_under_recovery_only_while_the_station_latch_is_set() {
    crate::auth::hold_station_identity_for_test();
    if !super::super::command_owner::isolated_test_child(module_path!(), "recover_station_is_listed_under_recovery_only_while_the_station_latch_is_set") { return; }
    auth::set_current_session(auth::StoredAuthSession{token:"recover-test-token".into(),remember_me:false,
        session:auth::SessionStatus{authenticated:true,permissions:auth::Permissions{view_data:true,send_commands:true,..Default::default()},..Default::default()}});
    // Not latched: no Recovery section, no button.
    let html = render_recover(None);
    assert!(!html.contains("data-dispatch-command=\"RecoverStation\""), "{html}");
    assert!(!sections_in(&html).iter().any(|(id, _)| id == "recovery"), "{html}");
    // Latched but a board still aborted: listed under Recovery, disabled, with the station's reason.
    let html = render_recover(Some(serde_json::json!({"disabled_reason":"VB still reports Abort.","target_state_label":"Startup"})));
    let shown = sections_in(&html);
    let recovery = &shown.iter().find(|(id, _)| id == "recovery").expect("Recovery section").1;
    assert!(recovery.contains("data-dispatch-command=\"RecoverStation\""), "{recovery}");
    assert!(recovery.contains("VB still reports Abort."), "{recovery}");
    assert!(recovery.contains("No board is reset."), "{recovery}");
    assert!(recovery.contains("0 of 1 enabled"), "{recovery}");
    assert!(recovery.contains("disabled"), "{recovery}");
    // Ready: the header counts it enabled.
    let html = render_recover(Some(serde_json::json!({"target_state_label":"Startup"})));
    let shown = sections_in(&html);
    let recovery = &shown.iter().find(|(id, _)| id == "recovery").unwrap().1;
    assert!(recovery.contains("1 of 1 enabled"), "{recovery}");
    assert!(!recovery.contains("Unavailable:"), "{recovery}");
}

#[component]
fn RadioTestHarness(status: Option<serde_json::Value>) -> Element {
    use_context_provider(|| Signal::new(super::super::sequence_progress_host::StatusObservation::default()));
    use_hook(|| {
        *super::super::WS_CONNECTED_SIGNAL.write() = true;
        *super::super::radio_reject_test::RADIO_TEST.write() = status.clone().map(|s| serde_json::from_value(s).unwrap());
    });
    let action_policy = use_signal(|| serde_json::from_value::<ActionPolicyMsg>(serde_json::json!({"key_enabled":true,"software_buttons_enabled":true,
        "controls":[{"cmd":"Abort","enabled":true,"blink":"none","actuated":null}]})).unwrap());
    let recording_status = use_signal(|| serde_json::from_value::<RecordingStatusMsg>(serde_json::json!({"mode":"idle","db_path":null})).unwrap());
    let layout: ActionsTabLayout = serde_json::from_value(serde_json::json!({"actions":[
        spec("Abort", Some("safety")), spec("RadioRejectTest", Some("testing"))]})).unwrap();
    rsx! { ActionsTab { layout, action_policy, recording_status, abort_only_mode: false, theme: ThemeConfig::default() } }
}

fn render_radio_test(status: Option<serde_json::Value>) -> String {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let _runtime = runtime.enter();
    let mut dom = VirtualDom::new_with_props(RadioTestHarness, RadioTestHarnessProps { status });
    dom.rebuild_in_place();
    dom.render_immediate_to_vec();
    dioxus::ssr::render(&dom)
}

#[test]
fn the_radio_test_is_listed_under_testing_with_its_variant_time_left_and_stop() {
    if !super::super::command_owner::isolated_test_child(module_path!(), "the_radio_test_is_listed_under_testing_with_its_variant_time_left_and_stop") { return; }
    auth::set_current_session(auth::StoredAuthSession{token:"radio-test-token".into(),remember_me:false,
        session:auth::SessionStatus{authenticated:true,roles:vec!["admin".into()],permissions:auth::Permissions{view_data:true,send_commands:true,..Default::default()},..Default::default()}});
    let mut choices = sections::Choices::new();
    choices.insert("testing".into(), true);
    sections::save(None, sections::OperatingMode::Normal, &choices);
    // Ready to start: the variant choice and Start, counted enabled.
    let html = render_radio_test(Some(serde_json::json!({"role":"primary","available":true,"may_start":true,"may_stop":true,
        "default_duration_s":600,"min_duration_s":30,"max_duration_s":3600,"abort_escape_ms":4000})));
    let shown = sections_in(&html);
    let testing = &shown.iter().find(|(id, _)| id == "testing").expect("Testing section").1;
    assert!(testing.contains("data-dispatch-command=\"RadioRejectTest\""), "{testing}");
    assert!(testing.contains("Force relay") && testing.contains("Force failover (silent primary)"), "{testing}");
    assert!(testing.contains("Start radio test"), "{testing}");
    assert!(testing.contains("1 of 1 enabled"), "{testing}");
    // In Armed: Start disabled with the station's reason.
    let html = render_radio_test(Some(serde_json::json!({"role":"primary","available":true,"may_start":true,"may_stop":true,
        "start_blocked_reason":"Only on the ground in Startup, Idle or PreFill; the station is in Armed."})));
    let testing = sections_in(&html).into_iter().find(|(id, _)| id == "testing").unwrap().1;
    assert!(testing.contains("Unavailable: Only on the ground in Startup, Idle or PreFill; the station is in Armed."), "{testing}");
    assert!(testing.contains("0 of 1 enabled"), "{testing}");
    // Running: the variant, the time left and Stop.
    let html = render_radio_test(Some(serde_json::json!({"role":"primary","available":true,"may_start":true,"may_stop":true,
        "banner":"RADIO TEST: direct AV-bay link rejected (Force relay)",
        "active":{"variant":"force_relay","variant_label":"Force relay","actor":"admin","remaining_s":125,"duration_s":600}})));
    let testing = sections_in(&html).into_iter().find(|(id, _)| id == "testing").unwrap().1;
    assert!(testing.contains("data-radio-test-active=\"force_relay\""), "{testing}");
    assert!(testing.contains("2:05 left"), "{testing}");
    assert!(testing.contains("Stop radio test"), "{testing}");
    assert!(testing.contains("1 of 1 enabled"), "{testing}");
}
