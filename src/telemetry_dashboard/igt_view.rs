//! Integrated ground test views: the SIMULATED banner shown on every view,
//! the `/api/igt` poller, the ticketed Enter / LIVE PYRO controls and the
//! replayed GPS track on the map. The rules live in `igt`.
use super::igt::{self, IGT_STATUS, IgtStatus, Pyro};
use dioxus::prelude::*;

const POLL_MS: u32 = 2_000;

async fn sleep_ms(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
}

/// Reads `/api/igt`: every 2 s while ground-test presentation applies, and
/// every 6 s otherwise, since the image notice, the Launch latch and the FC
/// SD state matter in every mode (the websocket `IgtStatus` frame updates it
/// between reads). Mount once.
#[component]
pub(super) fn IgtStatusPoller() -> Element {
    use_future(move || async move {
        let mut idle_ticks = 0u32;
        loop {
            let snapshot = super::station_mode::current_snapshot();
            let active = igt::active(&snapshot, IGT_STATUS.peek().as_ref());
            if super::dashboard_page_visible() && (active || idle_ticks == 0) {
                if let Ok(status) = super::http_get_json::<IgtStatus>(igt::STATUS_PATH).await {
                    igt::apply_status(status);
                }
            }
            idle_ticks = if active { 0 } else { (idle_ticks + 1) % 3 };
            if !igt::active(&snapshot, IGT_STATUS.peek().as_ref()) && snapshot["effective_mode"].is_string() {
                // Not in a ground test: no replayed track.
                igt::sim_track::clear();
                igt::sim_track::set_gps_simulated(None);
            }
            sleep_ms(POLL_MS).await;
        }
    });
    rsx! {}
}

const BANNER_CSS: &str = "
.gs26-igt-banner{flex-shrink:0;margin:0 0 8px;padding:10px 12px;border:3px solid #fecaca;border-radius:10px;background:#991b1b;color:#fff;display:flex;flex-direction:column;gap:6px;box-sizing:border-box;min-width:0;overflow-wrap:anywhere;font-family:inherit;}
.gs26-igt-banner-title{font-size:clamp(14px,2.4vw,18px);font-weight:900;letter-spacing:.04em;}
.gs26-igt-banner-row{display:flex;flex-wrap:wrap;gap:6px 8px;align-items:center;min-width:0;}
.gs26-igt-chip{display:inline-flex;align-items:center;min-height:24px;padding:2px 8px;border-radius:999px;border:1px solid #fecaca;background:#7f1d1d;color:#fff;font-size:12px;font-weight:800;}
.gs26-igt-chip[data-pyro=\"dry_run\"]{background:#14532d;border-color:#bbf7d0;}
.gs26-igt-chip[data-pyro=\"live\"]{background:repeating-linear-gradient(135deg,#facc15 0 10px,#111 10px 20px);color:#fff;border:2px solid #facc15;text-shadow:0 0 3px #000,0 0 3px #000;font-size:13px;font-weight:900;animation:gs26-igt-live 1s steps(2,start) infinite;}
.gs26-igt-banner[data-pyro=\"live\"]{border-color:#facc15;box-shadow:0 0 0 3px #111,0 0 0 6px #facc15;}
.gs26-igt-banner p{margin:0;font-size:12px;}
.gs26-igt-fault{padding:4px 8px;border-radius:6px;background:#fff;color:#991b1b;font-weight:900;}
.gs26-igt-image-notice{flex-shrink:0;margin:0 0 8px;padding:8px 12px;border-radius:8px;box-sizing:border-box;min-width:0;overflow-wrap:anywhere;font-size:13px;line-height:1.4;}
.gs26-igt-image-notice[data-level=\"warning\"]{border:2px solid #f59e0b;background:#451a03;color:#fde68a;font-weight:800;}
.gs26-igt-image-notice[data-level=\"info\"]{border:1px solid var(--gs-line,#526579);background:transparent;color:inherit;opacity:.85;font-size:12px;padding:4px 10px;}
.gs26-igt-latch{margin:0;padding:8px 10px;border:2px solid #f59e0b;border-radius:8px;background:#451a03;color:#fde68a;font-weight:800;overflow-wrap:anywhere;white-space:normal;}
@keyframes gs26-igt-live{50%{filter:brightness(1.6);}}
@media (prefers-reduced-motion: reduce){.gs26-igt-chip[data-pyro=\"live\"]{animation:none;}}
";

/// The FC image warning or note, in any station mode.
#[component]
pub(super) fn ImageNoticeBar(notice: Option<igt::NoticeView>) -> Element {
    let Some(n) = notice else {
        return rsx! {};
    };
    let level = if n.warning { "warning" } else { "info" };
    rsx! {
        p { class: "gs26-igt-image-notice", "data-igt-image-notice": level, "data-level": level,
            role: if n.warning { "alert" } else { "status" },
            if n.warning { strong { "FC image warning: " } }
            "{n.text}"
        }
    }
}

/// The Launch latch reason, in full.
#[component]
pub(super) fn LaunchLatchNotice(reason: Option<String>) -> Element {
    let Some(reason) = reason else {
        return rsx! {};
    };
    rsx! {
        style { {BANNER_CSS} }
        p { class: "gs26-igt-latch", role: "alert", "data-launch-latch": "true", "Launch latched closed: {reason}" }
    }
}

/// [`LaunchLatchNotice`] from the current status.
#[component]
pub(super) fn CurrentLaunchLatch() -> Element {
    rsx! { LaunchLatchNotice { reason: igt::current_launch_latch() } }
}

/// The FC image notice (any mode) and the red INTEGRATED GROUND TEST bar
/// (only while ground-test presentation applies), so it can be placed on
/// every view.
#[component]
pub(super) fn IgtBanner() -> Element {
    let status = IGT_STATUS.read().clone();
    let active = igt::active(&super::station_mode::current_snapshot(), status.as_ref());
    let notice = igt::image_notice(status.as_ref());
    let Some(b) = igt::banner(active, status.as_ref()) else {
        if notice.is_none() {
            return rsx! {};
        }
        return rsx! { style { {BANNER_CSS} } ImageNoticeBar { notice } };
    };
    let pyro = match b.pyro {
        Pyro::Live => "live",
        Pyro::DryRun => "dry_run",
        Pyro::Unknown => "unknown",
    };
    rsx! {
        style { {BANNER_CSS} }
        ImageNoticeBar { notice }
        aside {
            class: "gs26-igt-banner",
            role: "alert",
            "data-igt-banner": "true",
            "data-pyro": pyro,
            "aria-label": "Integrated ground test",
            div { class: "gs26-igt-banner-title", "{b.title}" }
            div { class: "gs26-igt-banner-row",
                span { class: "gs26-igt-chip", "data-igt-fc": "true", "{b.fc}" }
                span { class: "gs26-igt-chip", "data-pyro": pyro, "{b.pyro_text}" }
                span { class: "gs26-igt-chip", "data-gps-simulated": if b.gps == "GPS: simulated" { "true" } else { "false" }, "{b.gps}" }
            }
            if let Some(fault) = b.fault.clone() {
                p { class: "gs26-igt-fault", role: "alert", "{fault}" }
            }
            for note in b.notes.iter() {
                p { key: "{note}", "{note}" }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Enter ground test / LIVE PYRO (prepare + confirm)
// ---------------------------------------------------------------------------

/// What the operator reads after a ticketed request.
pub(super) fn outcome_message(cmd: &str, result: &Result<(), String>) -> String {
    match (cmd, result) {
        (igt::ENTER, Ok(())) => "Ground test requested. The FC validates its SD-card CSVs; wait for FC IGT Ready.".into(),
        (igt::LIVE_PYRO, Ok(())) => "LIVE PYRO requested. The banner shows LIVE PYRO once the FC confirms it.".into(),
        (_, Ok(())) => "Requested.".into(),
        (igt::LIVE_PYRO, Err(e)) => format!("LIVE PYRO not armed: {}", igt::refusal(e)),
        (_, Err(e)) => format!("Ground test not entered: {}", igt::refusal(e)),
    }
}

async fn prepare_and_confirm(
    cmd: &'static str,
    typed: Option<String>,
    owner: std::rc::Rc<super::recording_download::ExportOwner>,
) -> Result<(), String> {
    let action = igt::prepare_action(cmd).ok_or("Not a ground-test ticket action.")?;
    let prepared: igt::Prepared = super::station_settings_request::post_captured_no_redirect(
        igt::PREPARE_PATH,
        &igt::PrepareRequest { action },
        Ok(owner.clone()),
    )
    .await?;
    if !owner.is_current() {
        return Err("Connection changed. Review the current station and confirm again.".into());
    }
    let body = igt::confirm_request(&prepared, typed.as_deref())?;
    let _: serde_json::Value =
        super::station_settings_request::post_captured_no_redirect(igt::CONFIRM_PATH, &body, Ok(owner)).await?;
    Ok(())
}

/// Why a ticketed control is disabled now, or None.
pub(super) fn ticket_disabled_reason(cmd: &str) -> Option<String> {
    let status = IGT_STATUS.read().clone();
    let rrls = crate::auth::current_status().station_role.as_deref() == Some("rrls");
    let connected = *super::WS_CONNECTED_SIGNAL.read();
    // Integrated ground test, or HITL with the HITL ground-test replay on.
    let station_igt = igt::current_active();
    if cmd == igt::LIVE_PYRO {
        igt::live_pyro_disabled_reason(status.as_ref(), crate::auth::is_admin(), connected, rrls)
    } else {
        igt::enter_disabled_reason(status.as_ref(), crate::auth::can_send_command(cmd), connected, rrls, station_igt)
    }
}

/// The Ground test section's ticketed control for `IgtEnter` or `IgtLivePyro`.
#[component]
pub(super) fn IgtTicketControl(cmd: String, #[props(default)] inhibited: bool) -> Element {
    let mut confirming = use_signal(|| false);
    let mut sending = use_signal(|| false);
    let mut typed = use_signal(String::new);
    let mut result = use_signal(String::new);
    let live = cmd == igt::LIVE_PYRO;
    let cmd: &'static str = if live { igt::LIVE_PYRO } else { igt::ENTER };
    let reason = if inhibited {
        Some("Actions Disabled is on (header toggle).".to_string())
    } else {
        ticket_disabled_reason(cmd)
    };
    let disabled = reason.is_some() || sending();
    let phrase_ok = igt::live_pyro_phrase_ok(&typed.read());
    let (title, lines): (&str, &[&str]) = if live {
        (igt::LIVE_PYRO_TITLE, &igt::LIVE_PYRO_LINES)
    } else {
        (igt::ENTER_TITLE, &igt::ENTER_LINES)
    };
    let dialog_id = if live { "gs26-igt-live-pyro-confirm" } else { "gs26-igt-enter-confirm" };
    let button_class = if live { "gs-btn gs26-modal-danger" } else { "gs-btn" };
    rsx! {
        div { class: "gs26-igt-control", "data-igt-action": cmd, style: "display:grid;gap:var(--gs-space-2);justify-items:start;min-width:0;",
            button { r#type: "button", class: button_class, "data-dispatch-command": cmd, disabled, title: reason.clone().unwrap_or_default(),
                style: if live { "font-weight:900;" } else { "" },
                onclick: move |_| { result.set(String::new()); typed.set(String::new()); confirming.set(true); },
                if live { "LIVE PYRO…" } else { "Enter ground test…" }
            }
            p { style: "margin:0;font-size:var(--gs-text-sm);",
                if live { "Arm the real drogue and main charges for this test (Admin, typed confirmation). Default is DRY-RUN." }
                else { "Put the FC into ground-test mode: it validates its SD-card flight and replays it after Launch. Pyros stay DRY-RUN." }
            }
            if let Some(reason) = reason.clone() { p { role: "status", class: "gs-notice gs-notice--warn", "data-disabled-reason": "", "Unavailable: {reason}" } }
            if !result.read().is_empty() { p { role: "status", class: "gs-notice", "{result}" } }
        }
        if confirming() {
            div { style: "position:fixed;inset:0;z-index:200000;background:#000c;display:flex;align-items:center;justify-content:center;padding:20px;box-sizing:border-box;",
                onkeydown: move |event| {
                    event.stop_propagation();
                    match event.key() {
                        Key::Escape if !*sending.peek() => { event.prevent_default(); confirming.set(false); }
                        Key::Tab => { event.prevent_default(); super::board_recovery_panel::cycle_modal_focus(dialog_id, event.modifiers().contains(Modifiers::SHIFT)); }
                        _ => {}
                    }
                },
                div { id: dialog_id, role: "alertdialog", "aria-modal": "true", "aria-labelledby": "{dialog_id}-title",
                    style: if live {
                        "background:#450a0a;color:#fff;border:4px solid #facc15;font:inherit;padding:var(--gs-space-5);border-radius:var(--gs-radius-lg);width:min(100%,640px);max-height:calc(100dvh - 40px);display:flex;flex-direction:column;gap:var(--gs-space-3);overflow-y:auto;box-sizing:border-box;overflow-wrap:anywhere;"
                    } else {
                        "background:var(--gs26-panel-background,#142534);color:var(--gs26-app-text,#e5e7eb);border:2px solid #b91c1c;font:inherit;padding:var(--gs-space-5);border-radius:var(--gs-radius-lg);width:min(100%,640px);max-height:calc(100dvh - 40px);display:flex;flex-direction:column;gap:var(--gs-space-3);overflow-y:auto;box-sizing:border-box;overflow-wrap:anywhere;"
                    },
                    h2 { id: "{dialog_id}-title", style: "margin:0;font-size:var(--gs-text-lg);", "{title}" }
                    for (index, line) in lines.iter().enumerate() {
                        p { key: "{index}", style: if index == 0 { "margin:0;font-weight:800;" } else { "margin:0;" }, "{line}" }
                    }
                    if live {
                        label { r#for: "gs26-igt-live-pyro-phrase", style: "display:grid;gap:4px;font-weight:700;",
                            "Type {igt::LIVE_PYRO_PHRASE} to confirm"
                            input { id: "gs26-igt-live-pyro-phrase", r#type: "text", autocomplete: "off", spellcheck: "false",
                                autocapitalize: "characters", value: "{typed}", disabled: sending(),
                                style: "font:inherit;min-height:44px;padding:6px 10px;border-radius:8px;",
                                oninput: move |event| typed.set(event.value()),
                            }
                        }
                    }
                    div { style: "display:flex;gap:var(--gs-space-2);flex-wrap:wrap;justify-content:flex-end;",
                        button { r#type: "button", class: "gs-btn", autofocus: true, disabled: sending(), onclick: move |_| confirming.set(false), "Cancel" }
                        button { r#type: "button", class: if live { "gs-btn gs26-modal-danger" } else { "gs-btn gs-btn--primary" },
                            disabled: sending() || (live && !phrase_ok),
                            onclick: move |_| {
                                if *sending.peek() { return; }
                                let phrase = typed.peek().clone();
                                if live && !igt::live_pyro_phrase_ok(&phrase) { return; }
                                let owner = match super::recording_download::ExportOwner::capture_ordinary_selected(|| true) {
                                    Ok(owner) => owner,
                                    Err(_) => { result.set("No current station or sign-in. Reconnect and try again.".into()); confirming.set(false); return; }
                                };
                                sending.set(true);
                                spawn(async move {
                                    let outcome = prepare_and_confirm(cmd, live.then_some(phrase), owner.clone()).await;
                                    if !owner.is_current() { return; }
                                    result.set(outcome_message(cmd, &outcome));
                                    sending.set(false);
                                    confirming.set(false);
                                    typed.set(String::new());
                                });
                            },
                            if sending() { "Sending…" } else if live { "Arm LIVE PYRO" } else { "Enter ground test" }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Ground test section: settings toggles, FC SD state
// ---------------------------------------------------------------------------

const DIALOG_STYLE: &str = "background:var(--gs26-panel-background,#142534);color:var(--gs26-app-text,#e5e7eb);border:2px solid #b45309;font:inherit;padding:var(--gs-space-5);border-radius:var(--gs-radius-lg);width:min(100%,600px);max-height:calc(100dvh - 40px);display:flex;flex-direction:column;gap:var(--gs-space-3);overflow-y:auto;box-sizing:border-box;overflow-wrap:anywhere;";

/// A modal confirm with the effect spelled out.
#[component]
pub(super) fn ConfirmDialog(
    id: String,
    title: String,
    lines: Vec<String>,
    confirm_label: String,
    busy: bool,
    on_cancel: EventHandler<()>,
    on_confirm: EventHandler<()>,
) -> Element {
    let focus_id = id.clone();
    rsx! {
        div { style: "position:fixed;inset:0;z-index:200000;background:#000c;display:flex;align-items:center;justify-content:center;padding:20px;box-sizing:border-box;",
            onkeydown: move |event| {
                event.stop_propagation();
                match event.key() {
                    Key::Escape if !busy => { event.prevent_default(); on_cancel.call(()); }
                    Key::Tab => { event.prevent_default(); super::board_recovery_panel::cycle_modal_focus(&focus_id, event.modifiers().contains(Modifiers::SHIFT)); }
                    _ => {}
                }
            },
            div { id: "{id}", role: "alertdialog", "aria-modal": "true", "aria-labelledby": "{id}-title", style: DIALOG_STYLE,
                h2 { id: "{id}-title", style: "margin:0;font-size:var(--gs-text-lg);", "{title}" }
                for (index, line) in lines.iter().enumerate() {
                    p { key: "{index}", style: "margin:0;", "{line}" }
                }
                div { style: "display:flex;gap:var(--gs-space-2);flex-wrap:wrap;justify-content:flex-end;",
                    button { r#type: "button", class: "gs-btn", autofocus: true, disabled: busy, onclick: move |_| on_cancel.call(()), "Cancel" }
                    button { r#type: "button", class: "gs-btn gs-btn--primary", "data-confirm": "true", disabled: busy,
                        onclick: move |_| on_confirm.call(()),
                        if busy { "Saving…" } else { "{confirm_label}" }
                    }
                }
            }
        }
    }
}

/// What the operator reads after a settings PUT.
pub(super) fn settings_outcome(toggle: igt::Toggle, on: bool, result: &Result<(), String>) -> String {
    match result {
        Ok(()) => format!("{} turned {}.", toggle.label(), if on { "on" } else { "off" }),
        Err(e) => format!("{} not changed: {}", toggle.label(), igt::refusal(e)),
    }
}

async fn put_settings(update: igt::SettingsUpdate, owner: std::rc::Rc<super::recording_download::ExportOwner>) -> Result<(), String> {
    let reply: serde_json::Value = super::station_settings_request::put_captured(igt::SETTINGS_PATH, &update, Ok(owner)).await?;
    if let Ok(settings) = serde_json::from_value::<igt::IgtSettings>(reply) {
        igt::apply_settings(settings);
    }
    Ok(())
}

/// One admin-only ground-test setting, changed through a confirm dialog.
#[component]
pub(super) fn SettingsToggle(toggle: igt::Toggle, value: Option<bool>, disabled_reason: Option<String>) -> Element {
    let mut confirming = use_signal(|| None::<bool>);
    let mut sending = use_signal(|| false);
    let mut result = use_signal(|| None::<(bool, String)>);
    let reason = disabled_reason.or_else(|| value.is_none().then(|| "No ground-test settings from the station yet.".to_string()));
    let checked = value.unwrap_or(false);
    let disabled = reason.is_some() || sending();
    let input_id = format!("gs26-igt-setting-{}", toggle.id());
    let dialog = confirming().map(|on| {
        let (title, lines, button) = toggle.confirm_copy(on);
        (on, title.to_string(), lines.iter().map(|l| l.to_string()).collect::<Vec<_>>(), button.to_string())
    });
    rsx! {
        div { class: "gs26-igt-setting", "data-igt-setting": toggle.id(), style: "display:grid;gap:4px;min-width:0;",
            label { r#for: "{input_id}", style: "display:flex;gap:8px;align-items:center;font-weight:700;",
                input { id: "{input_id}", r#type: "checkbox", checked, disabled,
                    onchange: move |event| {
                        if *sending.peek() { return; }
                        result.set(None);
                        confirming.set(Some(event.checked()));
                    },
                }
                "{toggle.label()}"
                span { style: "font-weight:400;", if checked { " (on)" } else { " (off)" } }
            }
            p { style: "margin:0;font-size:var(--gs-text-sm);", "{toggle.help()}" }
            if let Some(reason) = reason.clone() {
                p { role: "status", class: "gs-notice gs-notice--warn", "data-disabled-reason": "", "Unavailable: {reason}" }
            }
            if let Some((ok, text)) = result() {
                p { role: if ok { "status" } else { "alert" }, class: if ok { "gs-notice" } else { "gs-notice gs-notice--danger" }, "data-igt-setting-result": if ok { "ok" } else { "error" }, "{text}" }
            }
        }
        if let Some((on, title, lines, button)) = dialog {
            ConfirmDialog { id: "gs26-igt-setting-confirm-{toggle.id()}", title, lines, confirm_label: button, busy: sending(),
                on_cancel: move |_| confirming.set(None),
                on_confirm: move |_| {
                    if *sending.peek() { return; }
                    let owner = match super::recording_download::ExportOwner::capture_ordinary_selected(|| true) {
                        Ok(owner) => owner,
                        Err(_) => { result.set(Some((false, "No current station or sign-in. Reconnect and try again.".into()))); confirming.set(None); return; }
                    };
                    sending.set(true);
                    spawn(async move {
                        let outcome = put_settings(toggle.update(on), owner.clone()).await;
                        if !owner.is_current() { return; }
                        result.set(Some((outcome.is_ok(), settings_outcome(toggle, on, &outcome))));
                        sending.set(false);
                        confirming.set(None);
                    });
                },
            }
        }
    }
}

/// The FC SD card state shown with Retry SD.
#[component]
pub(super) fn SdState(view: igt::SdView) -> Element {
    rsx! {
        span { role: "status", "data-fc-sd-state": if view.warning { "not_ready" } else { "ok" },
            style: if view.warning { "display:block;margin-top:2px;font-size:11px;line-height:1.3;font-weight:800;color:#fbbf24;white-space:normal;overflow-wrap:anywhere;" } else { "display:block;margin-top:2px;font-size:11px;line-height:1.3;font-weight:400;white-space:normal;overflow-wrap:anywhere;" },
            "{view.text}"
        }
    }
}

/// The settings and state above the Ground test section's controls.
#[component]
pub(super) fn GroundTestPanelView(
    effective_mode: Option<String>,
    status: Option<IgtStatus>,
    toggle_reason: Option<String>,
    retry_in_section: bool,
) -> Element {
    let latch = igt::launch_latch(status.as_ref());
    let notice = igt::image_notice(status.as_ref());
    let sd = igt::sd_view(status.as_ref());
    let toggles: Vec<igt::Toggle> = [igt::Toggle::MockWetDress, igt::Toggle::HitlReplay]
        .into_iter()
        .filter(|t| t.offered(effective_mode.as_deref()))
        .collect();
    rsx! {
        div { class: "gs26-igt-panel", "data-igt-panel": "", style: "display:grid;gap:var(--gs-space-3);min-width:0;",
            LaunchLatchNotice { reason: latch }
            ImageNoticeBar { notice }
            if effective_mode.as_deref() == Some(igt::HITL_MODE) && !status.as_ref().is_some_and(|s| s.igt_active) {
                p { class: "gs-notice", style: "margin:0;", "HITL: turn on the HITL ground-test replay (Admin) to use the ground-test controls below." }
            }
            for toggle in toggles {
                SettingsToggle { key: "{toggle.id()}", toggle, value: igt::Toggle::value(toggle, status.as_ref()), disabled_reason: toggle_reason.clone() }
            }
            div { "data-igt-sd": "", style: "min-width:0;",
                SdState { view: sd }
                if !retry_in_section {
                    span { style: "display:block;font-size:11px;", "Retry FC SD is in the Flight computer section." }
                }
            }
        }
    }
}

/// [`GroundTestPanelView`] from the current status, mode and account.
#[component]
pub(super) fn GroundTestPanel(inhibited: bool, retry_in_section: bool) -> Element {
    let status = IGT_STATUS.read().clone();
    let effective_mode = super::station_mode::current_snapshot()["effective_mode"].as_str().map(str::to_owned);
    let rrls = crate::auth::current_status().station_role.as_deref() == Some("rrls");
    let connected = *super::WS_CONNECTED_SIGNAL.read();
    let toggle_reason = igt::toggle_disabled_reason(crate::auth::is_admin(), connected, rrls, inhibited);
    rsx! { GroundTestPanelView { effective_mode, status, toggle_reason, retry_in_section } }
}

// ---------------------------------------------------------------------------
// Map: the replayed GPS track
// ---------------------------------------------------------------------------

fn track_js(points: &[(f64, f64)]) -> String {
    let json: Vec<[f64; 2]> = points.iter().map(|(lat, lon)| [*lat, *lon]).collect();
    let json = serde_json::to_string(&json).unwrap_or_else(|_| "[]".into());
    format!(
        "(function(){{try{{window.__gs26_pending_sim_track={json};if(typeof window.setGroundMapSimTrack===\"function\"){{window.setGroundMapSimTrack(window.__gs26_pending_sim_track);}}}}catch(e){{console.warn(\"setGroundMapSimTrack threw:\",e);}}}})();"
    )
}

/// Draws the replayed (SIMULATED) GPS track on the map, separate from the
/// real rocket marker, and labels it. The track stops growing when the FC
/// switches GPS back to real at landing.
#[component]
pub(super) fn IgtMapTrack(theme: super::layout::ThemeConfig) -> Element {
    let mut drawn = use_signal(|| u64::MAX);
    let mut points = use_signal(|| 0usize);
    use_future(move || async move {
        loop {
            let revision = igt::sim_track::revision();
            if revision != *drawn.peek() {
                let track = igt::sim_track::points();
                super::js_eval(&track_js(&track));
                drawn.set(revision);
                if *points.peek() != track.len() {
                    points.set(track.len());
                }
            }
            sleep_ms(500).await;
        }
    });
    let status = IGT_STATUS.read().clone();
    let active = igt::active(&super::station_mode::current_snapshot(), status.as_ref());
    let count = points();
    if !active && count == 0 {
        return rsx! {};
    }
    let gps = match status.as_ref() {
        Some(s) if s.gps_simulated => "GPS: simulated (rocket marker shows real GPS)",
        Some(_) => "GPS: real",
        None => "GPS: unknown",
    };
    rsx! {
        div { class: "gs26-map-sim-track", "data-sim-track-points": "{count}",
            style: "display:flex;flex-wrap:wrap;gap:4px 12px;padding:0 10px;font-size:0.74rem;line-height:1.2;min-width:0;color:{theme.text_muted};",
            if count > 0 {
                span { style: "color:#e879f9;font-weight:800;", "╌ SIMULATED GPS track (replayed flight, {count} fixes)" }
            }
            span { "data-gps-simulated": if status.as_ref().is_some_and(|s| s.gps_simulated) { "true" } else { "false" }, style: "font-weight:700;", "{gps}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn banner_html(status: Option<serde_json::Value>) -> String {
        fn app() -> Element {
            rsx! { IgtBanner {} }
        }
        let mut dom = VirtualDom::new(app);
        dom.in_scope(ScopeId::ROOT, || {
            *IGT_STATUS.write() = status.map(|v| serde_json::from_value(v).unwrap());
        });
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn banner_renders_only_in_igt_with_dry_run_live_and_fault_text() {
        assert!(!banner_html(None).contains("data-igt-banner"), "no IGT: nothing");
        let other_mode = banner_html(Some(serde_json::json!({"supported": true, "fc_mode": "off", "station_mode_igt": false})));
        assert!(!other_mode.contains("data-igt-banner"));

        let page = banner_html(Some(serde_json::json!({
            "supported": true, "fc_mode": "sim", "live_pyro": true, "gps_simulated": true,
            "station_mode_igt": true, "updated_ms": 5
        })));
        assert!(page.contains("INTEGRATED GROUND TEST — SIMULATED FLIGHT DATA"), "{page}");
        assert!(page.contains("data-pyro=\"live\"") && page.contains("LIVE PYRO ARMED"));
        assert!(page.contains("GPS: simulated") && page.contains("FC IGT: SIM"));

        let page = banner_html(Some(serde_json::json!({
            "supported": true, "fc_mode": "fault", "reason": 4, "reason_text": "IGT IMU0.CSV:12 parse error",
            "station_mode_igt": true, "updated_ms": 5
        })));
        assert!(page.contains("data-pyro=\"dry_run\"") && page.contains("DRY-RUN"));
        assert!(page.contains("FC IGT fault: IGT IMU0.CSV:12 parse error"));
    }

    #[test]
    fn image_notice_shows_in_every_mode_by_level() {
        // Not in a ground test (station_mode_igt false, no mode read): no red
        // banner, but the image warning still shows.
        let page = banner_html(Some(serde_json::json!({"supported": true, "fc_image": "ground_test",
            "station_mode_igt": false, "igt_active": false, "updated_ms": 1,
            "image_notice": {"level": "warning", "text": "FC is running a ground-test image (dry-run capable)"}})));
        assert!(!page.contains("data-igt-banner"), "{page}");
        assert!(page.contains("data-igt-image-notice=\"warning\"") && page.contains("role=\"alert\""), "{page}");
        assert!(page.contains("FC image warning: ") && page.contains("ground-test image (dry-run capable)"));
        let info = banner_html(Some(serde_json::json!({"image_notice": {"level": "info", "text": "Ground-test image on the FC"}})));
        assert!(info.contains("data-igt-image-notice=\"info\"") && info.contains("role=\"status\""), "{info}");
        assert!(!info.contains("FC image warning"));
        // In IGT the mismatch warning shows with the banner.
        let both = banner_html(Some(serde_json::json!({"supported": false, "fc_image": "flight", "station_mode_igt": true,
            "igt_active": true, "updated_ms": 2,
            "image_notice": {"level": "warning", "text": "FC is running a flight image: integrated ground test unavailable, flash the ground-test image"}})));
        assert!(both.contains("data-igt-banner") && both.contains("flash the ground-test image"), "{both}");
        // HITL replay (igt_active without station_mode_igt) shows the banner.
        let hitl = banner_html(Some(serde_json::json!({"supported": true, "fc_mode": "ready", "station_mode_igt": false,
            "igt_active": true, "hitl_replay": true, "updated_ms": 3})));
        assert!(hitl.contains("data-igt-banner"), "{hitl}");
        assert!(!banner_html(Some(serde_json::json!({"supported": true}))).contains("data-igt-image-notice"));
    }

    fn render<P: Clone + 'static>(props: P, app: fn(P) -> Element) -> String {
        let mut dom = VirtualDom::new_with_props(app, props);
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn latch_reason_renders_in_full() {
        let long = format!("{}. This sentence is here so a two-line clamp would cut it off.", igt::LATCH_REASON);
        let page = render(Some(long.clone()), |reason| rsx! { LaunchLatchNotice { reason } });
        assert!(page.contains("data-launch-latch") && page.contains(&long), "{page}");
        assert!(!page.contains("line-clamp"));
        assert!(!render(None::<String>, |reason| rsx! { LaunchLatchNotice { reason } }).contains("data-launch-latch"));
    }

    fn panel(mode: &str, status: serde_json::Value, admin: bool) -> String {
        let status: IgtStatus = serde_json::from_value(status).unwrap();
        let reason = igt::toggle_disabled_reason(admin, true, false, false);
        render((mode.to_owned(), status, reason), |(mode, status, reason)| rsx! {
            GroundTestPanelView { effective_mode: Some(mode), status: Some(status), toggle_reason: reason, retry_in_section: true }
        })
    }

    #[test]
    fn ground_test_panel_gates_the_settings_to_admins() {
        let igt_status = serde_json::json!({"supported": true, "fc_mode": "off", "igt_active": true, "mock_wet_dress": true,
            "sd_ready": false, "sd_retry_pending": true, "updated_ms": 1});
        let admin = panel("integrated_ground_test", igt_status.clone(), true);
        assert!(admin.contains("data-igt-setting=\"mock_wet_dress\"") && !admin.contains("data-igt-setting=\"hitl_replay\""), "{admin}");
        assert!(admin.contains("Mock wet dress (flight fill steps)") && admin.contains("checked"), "{admin}");
        assert!(!admin.contains("Unavailable: Administrator required"));
        assert!(admin.contains("data-fc-sd-state=\"not_ready\"") && admin.contains("FC SD card not mounted / not writable · Retry pending"));
        let operator = panel("integrated_ground_test", igt_status, false);
        assert!(operator.contains("Unavailable: Administrator required."), "{operator}");
        assert!(operator.contains("disabled"));
        let hitl = panel("hitl", serde_json::json!({"igt_active": false, "hitl_replay": false}), true);
        assert!(hitl.contains("data-igt-setting=\"hitl_replay\"") && hitl.contains("HITL ground-test replay"), "{hitl}");
        assert!(!hitl.contains("data-igt-setting=\"mock_wet_dress\""));
        assert!(hitl.contains("turn on the HITL ground-test replay"));
        let latched = panel("integrated_ground_test", serde_json::json!({"launch_latched": true}), true);
        assert!(latched.contains("Launch latched closed: FC is running a ground-test image"), "{latched}");
    }

    #[test]
    fn settings_outcome_shows_the_station_refusal() {
        assert_eq!(settings_outcome(igt::Toggle::MockWetDress, true, &Ok(())), "Mock wet dress (flight fill steps) turned on.");
        assert_eq!(
            settings_outcome(igt::Toggle::HitlReplay, true, &Err(r#"HTTP 409: {"error":"refused","message":"FC is replaying (SIM)"}"#.into())),
            "HITL ground-test replay not changed: FC is replaying (SIM)"
        );
        assert_eq!(
            settings_outcome(igt::Toggle::MockWetDress, false, &Err(r#"HTTP 403: {"error":"Administrator required"}"#.into())),
            "Mock wet dress (flight fill steps) not changed: Administrator required"
        );
    }

    #[test]
    fn sd_state_renders_for_retry_sd() {
        let page = render(igt::sd_view(None), |view| rsx! { SdState { view } });
        assert!(page.contains("data-fc-sd-state=\"ok\"") && page.contains("no report"));
    }

    #[test]
    fn outcomes_name_the_next_step_or_the_refusal() {
        assert!(outcome_message(igt::ENTER, &Ok(())).contains("wait for FC IGT Ready"));
        assert_eq!(
            outcome_message(igt::LIVE_PYRO, &Err(r#"HTTP 403: {"error":"Administrator required"}"#.into())),
            "LIVE PYRO not armed: Administrator required"
        );
    }

    #[test]
    fn map_track_script_hands_lat_lon_pairs_to_the_map() {
        let js = track_js(&[(31.5, -102.25)]);
        assert!(js.contains("__gs26_pending_sim_track=[[31.5,-102.25]]"));
        assert!(js.contains("setGroundMapSimTrack"));
        assert!(track_js(&[]).contains("=[]"));
    }
}
