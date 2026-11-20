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

/// Reads `/api/igt` while the station is in the integrated ground test (the
/// websocket `IgtStatus` frame updates it between reads). Mount once.
#[component]
pub(super) fn IgtStatusPoller() -> Element {
    use_future(move || async move {
        loop {
            let station_igt = igt::is_mode(super::station_mode::current_snapshot()["effective_mode"].as_str());
            if station_igt && super::dashboard_page_visible() {
                if let Ok(status) = super::http_get_json::<IgtStatus>(igt::STATUS_PATH).await {
                    igt::apply_status(status);
                }
            } else if !station_igt && IGT_STATUS.peek().is_some()
                && super::station_mode::current_snapshot()["effective_mode"].is_string()
            {
                // Left IGT: drop the status and the replayed track.
                *IGT_STATUS.write() = None;
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
@keyframes gs26-igt-live{50%{filter:brightness(1.6);}}
@media (prefers-reduced-motion: reduce){.gs26-igt-chip[data-pyro=\"live\"]{animation:none;}}
";

/// The red INTEGRATED GROUND TEST bar. Renders nothing outside IGT, so it can
/// be placed on every view.
#[component]
pub(super) fn IgtBanner() -> Element {
    let status = IGT_STATUS.read().clone();
    let active = igt::active(&super::station_mode::current_snapshot(), status.as_ref());
    let Some(b) = igt::banner(active, status.as_ref()) else {
        return rsx! {};
    };
    let pyro = match b.pyro {
        Pyro::Live => "live",
        Pyro::DryRun => "dry_run",
        Pyro::Unknown => "unknown",
    };
    rsx! {
        style { {BANNER_CSS} }
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
    let station_igt = igt::is_mode(super::station_mode::current_snapshot()["effective_mode"].as_str());
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
