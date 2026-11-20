//! Radio test mode (GroundStation `radio_reject_test`): the primary's radio
//! driver rejects the direct AV-bay link, so the RRLS relay ("Force relay")
//! or the RRLS takeover ("Force failover", the primary goes silent) can be
//! tested with the rocket in range. The station decides everything; this
//! shows its state in a banner on every view (primary and RRLS), and lists
//! the control under Actions > Testing on the primary: variant, timeout, a
//! confirmation, the time left and Stop.
use super::layout::ThemeConfig;
use dioxus::prelude::*;
use serde::Deserialize;

/// The Actions-tab entry the station's layout lists under Testing.
pub(super) const COMMAND: &str = "RadioRejectTest";
const PATH: &str = "/api/radio/reject-test";
const START_PATH: &str = "/api/radio/reject-test/start";
const STOP_PATH: &str = "/api/radio/reject-test/stop";
pub(super) const VARIANTS: [(&str, &str); 2] = [("force_relay", "Force relay"), ("silent_primary", "Force failover (silent primary)")];

#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct ActiveTest {
    pub variant: String,
    pub variant_label: String,
    pub actor: String,
    pub remaining_s: u64,
    pub duration_s: u64,
    pub filtering: bool,
    pub abort_pending_ms: Option<u64>,
    pub relay_engaged: bool,
    pub rx_rejected: u64,
    pub tx_rejected: u64,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct PrimaryTest {
    pub variant: String,
    pub variant_label: String,
    pub remaining_s_at_most: u64,
    pub banner: String,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct HistoryEntry {
    pub event: String,
    pub utc_ms: u64,
    pub variant: String,
    pub actor: String,
    pub reason: Option<String>,
}

/// GET /api/radio/reject-test.
#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct RadioTestStatus {
    pub role: String,
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub start_blocked_reason: Option<String>,
    pub may_start: bool,
    pub may_stop: bool,
    pub active: Option<ActiveTest>,
    pub banner: Option<String>,
    pub primary: Option<PrimaryTest>,
    pub default_duration_s: u64,
    pub min_duration_s: u64,
    pub max_duration_s: u64,
    pub abort_escape_ms: u64,
    pub history: Vec<HistoryEntry>,
}

pub(crate) static RADIO_TEST: GlobalSignal<Option<RadioTestStatus>> = Signal::global(|| None);

/// "9:59", "1:00:00".
pub(super) fn remaining(s: u64) -> String {
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// The banner on every view, or None. The primary's own test, or on an RRLS
/// the test the primary announced.
pub(super) fn banner(status: &RadioTestStatus) -> Option<(String, String)> {
    if let Some(active) = &status.active {
        let mut detail = format!("{} by {}; {} left.", active.variant_label, active.actor, remaining(active.remaining_s));
        if active.variant == "silent_primary" {
            detail.push_str(if active.filtering {
                " This station is radio-silent: the RRLS takes over."
            } else {
                " Announcing the test to the RRLS before going silent."
            });
        } else {
            detail.push_str(if active.relay_engaged {
                " The RRLS relay carries the AV bay."
            } else {
                " Waiting for the RRLS relay."
            });
        }
        if active.abort_pending_ms.is_some() {
            detail.push_str(" An Abort is going through the relay; the station sends it direct if the AV bay does not confirm it.");
        }
        let title = status.banner.clone().unwrap_or_else(|| "RADIO TEST: direct AV-bay link rejected".into());
        return Some((title, detail));
    }
    status.primary.as_ref().map(|p| {
        (
            p.banner.clone(),
            format!("The primary ground station is running a radio test ({}); at most {} left.", p.variant_label, remaining(p.remaining_s_at_most)),
        )
    })
}

/// Why Start is unavailable here, or None. Local reasons first.
pub(super) fn start_disabled_reason(status: Option<&RadioTestStatus>, rrls: bool, connected: bool) -> Option<String> {
    if rrls {
        return Some("The radio test runs on the primary ground station.".into());
    }
    if !connected {
        return Some("Not connected to the ground station.".into());
    }
    let Some(status) = status else {
        return Some("Reading the station's radio test state…".into());
    };
    if !status.available {
        return Some(status.unavailable_reason.clone().unwrap_or_else(|| "Not available on this station.".into()));
    }
    if !status.may_start {
        return Some("Only an administrator may start the radio test.".into());
    }
    status.start_blocked_reason.clone()
}

/// Enabled in the section header: startable now, or running (Stop).
pub(super) fn enabled(status: Option<&RadioTestStatus>, rrls: bool, connected: bool) -> bool {
    status.is_some_and(|s| s.active.is_some() && s.may_stop) || start_disabled_reason(status, rrls, connected).is_none()
}

pub(super) fn confirm_title(variant: &str) -> &'static str {
    if variant == "silent_primary" {
        "Make this station radio-silent to test the RRLS takeover?"
    } else {
        "Reject the direct AV-bay link to test the RRLS relay?"
    }
}

pub(super) fn confirm_lines(variant: &str, minutes: u64, escape_ms: u64) -> [String; 3] {
    let what = if variant == "silent_primary" {
        "This station stops receiving and transmitting on the radio, toward the AV bay and the RRLS. The RRLS should take over the AV bay after about 6 s; stopping the test tests the hand-back.".to_string()
    } else {
        "This station ignores everything it hears from the AV bay and sends it nothing directly; its link to the RRLS stays up, so the RRLS relay should engage within a few seconds.".to_string()
    };
    let abort = if variant == "silent_primary" {
        "Abort still works: an Abort ends the silence at once and goes out on the direct link.".to_string()
    } else {
        format!("Abort still works: it goes through the RRLS relay, and if the AV bay has not confirmed it within {:.0} s the station lifts the test and sends it direct.", escape_ms as f64 / 1000.0)
    };
    [
        what,
        abort,
        format!("Only in Startup, Idle or PreFill. It ends after {minutes} min, when the flight state advances, or on a restart; the umbilical is unaffected. Recorded under your name."),
    ]
}

use super::board_recovery_panel::server_refusal;

pub(super) fn outcome_message(started: bool, result: Result<serde_json::Value, String>) -> String {
    match result {
        Ok(_) if started => "Radio test started.".into(),
        Ok(_) => "Radio test stopped; the direct AV-bay link is accepted again.".into(),
        Err(error) => format!("Radio test {}: {}", if started { "not started" } else { "not stopped" }, server_refusal(&error)),
    }
}

fn stop_reason_label(reason: &str) -> &str {
    match reason {
        "operator" => "stopped by an operator",
        "expired" => "timed out",
        "flight_state" => "flight state advanced",
        "abort_lift" => "lifted for an Abort",
        "abort_relayed" => "Abort confirmed through the relay",
        "abort_escape" => "Abort sent direct (relay did not confirm)",
        "restart" => "station restarted",
        other => other,
    }
}

/// Polls the station's radio test state for the banner and the control.
#[component]
pub(super) fn RadioTestRuntime() -> Element {
    use_future(move || async move {
        *RADIO_TEST.write() = None;
        loop {
            let next = super::http_get_json::<RadioTestStatus>(PATH).await.ok();
            if *RADIO_TEST.peek() != next {
                *RADIO_TEST.write() = next;
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(1000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    rsx! {}
}

#[component]
pub(super) fn RadioTestBanner(theme: ThemeConfig) -> Element {
    let Some((title, detail)) = RADIO_TEST.read().as_ref().and_then(banner) else {
        return rsx! {};
    };
    rsx! {
        aside {
            role: "alert",
            class: "gs26-connection-notice gs26-layout-notice",
            "data-radio-test": "",
            style: "border-color:{theme.error_border};background:{theme.error_background};color:{theme.error_text};border-width:3px;",
            div { class: "gs26-layout-notice-body",
                strong { style: "letter-spacing:0.04em;", "{title}" }
                p { "{detail}" }
            }
        }
    }
}

/// The Actions-tab control: variant, timeout, start (confirmed), and while
/// running the variant, time left and Stop.
#[component]
pub(super) fn RadioTestControl() -> Element {
    let mut variant = use_signal(|| VARIANTS[0].0.to_string());
    let mut minutes = use_signal(|| None::<u64>);
    let mut confirming = use_signal(|| false);
    let mut sending = use_signal(|| false);
    let mut result = use_signal(String::new);
    let status = RADIO_TEST.read().clone();
    let rrls = crate::auth::current_status().station_role.as_deref() == Some("rrls");
    let connected = *super::WS_CONNECTED_SIGNAL.read();
    let reason = start_disabled_reason(status.as_ref(), rrls, connected);
    let defaults = status.clone().unwrap_or_default();
    let min_m = defaults.min_duration_s.div_ceil(60).max(1);
    let max_m = (defaults.max_duration_s / 60).max(min_m);
    let chosen_m = minutes().unwrap_or(if defaults.default_duration_s > 0 { defaults.default_duration_s / 60 } else { 10 }).clamp(min_m, max_m);
    let active = defaults.active.clone();
    let lines = confirm_lines(&variant(), chosen_m, defaults.abort_escape_ms.max(1));
    let last = defaults.history.iter().find(|h| h.event == "stop").cloned();
    let mut send = move |path: &'static str, body: serde_json::Value, started: bool| {
        if *sending.peek() {
            return;
        }
        let owner = match super::recording_download::ExportOwner::capture_ordinary_selected(|| true) {
            Ok(owner) => owner,
            Err(_) => {
                result.set("No current station or sign-in. Reconnect and try again.".into());
                confirming.set(false);
                return;
            }
        };
        sending.set(true);
        spawn(async move {
            let outcome = super::station_settings_request::post_captured::<_, serde_json::Value>(path, &body, Ok(owner.clone())).await;
            if !owner.is_current() {
                return;
            }
            if let Ok(value) = outcome.as_ref()
                && let Ok(next) = serde_json::from_value::<RadioTestStatus>(value.clone())
            {
                *RADIO_TEST.write() = Some(next);
            }
            result.set(outcome_message(started, outcome));
            sending.set(false);
            confirming.set(false);
        });
    };
    rsx! {
        div { class: "gs26-radio-test", "data-radio-test-control": "", style: "display:grid;gap:var(--gs-space-2);justify-items:start;min-width:0;",
            if let Some(active) = active.clone() {
                p { role: "status", class: "gs-notice gs-notice--warn", "data-radio-test-active": "{active.variant}",
                    strong { "{active.variant_label}" }
                    " · {remaining(active.remaining_s)} left · started by {active.actor} · rejected {active.rx_rejected} received / {active.tx_rejected} sent frames"
                }
                button { r#type: "button", class: "gs-btn gs-btn--primary", "data-dispatch-command": COMMAND,
                    disabled: sending() || !defaults.may_stop,
                    onclick: move |_| { result.set(String::new()); send(STOP_PATH, serde_json::json!({}), false); },
                    if sending() { "Stopping…" } else { "Stop radio test" }
                }
            } else {
                fieldset { style: "border:0;padding:0;margin:0;display:flex;gap:var(--gs-space-3);flex-wrap:wrap;",
                    legend { style: "font-weight:700;margin-bottom:var(--gs-space-1);", "Radio test: reject the direct AV-bay link" }
                    for (id, label) in VARIANTS {
                        label { style: "display:flex;gap:6px;align-items:center;",
                            input { r#type: "radio", name: "gs26-radio-test-variant", value: id, checked: variant() == id,
                                onchange: move |_| variant.set(id.to_string()) }
                            "{label}"
                        }
                    }
                    label { style: "display:flex;gap:6px;align-items:center;",
                        "Timeout (min)"
                        input { r#type: "number", min: "{min_m}", max: "{max_m}", value: "{chosen_m}", style: "width:5em;",
                            oninput: move |event| minutes.set(event.value().parse::<u64>().ok()) }
                    }
                }
                button { r#type: "button", class: "gs-btn", "data-dispatch-command": COMMAND,
                    disabled: reason.is_some() || sending(), title: reason.clone().unwrap_or_default(),
                    onclick: move |_| { result.set(String::new()); confirming.set(true); },
                    "Start radio test…"
                }
                if let Some(reason) = reason.clone() {
                    p { role: "status", class: "gs-notice gs-notice--warn", "data-disabled-reason": "", "Unavailable: {reason}" }
                }
            }
            p { style: "margin:0;font-size:var(--gs-text-sm);",
                "Tests the RRLS relay or takeover with the rocket in range. Ground only (Startup, Idle, PreFill)."
            }
            if let Some(last) = last {
                p { style: "margin:0;font-size:var(--gs-text-sm);opacity:0.8;",
                    "Last test: {last.variant}, {stop_reason_label(last.reason.as_deref().unwrap_or(\"\"))}."
                }
            }
            if !result.read().is_empty() { p { role: "status", class: "gs-notice", "{result}" } }
        }
        if confirming() {
            div { style: "position:fixed;inset:0;z-index:200000;background:#000a;display:flex;align-items:center;justify-content:center;padding:20px;box-sizing:border-box;",
                onkeydown: move |event| { event.stop_propagation(); match event.key() {
                    Key::Escape if !*sending.peek() => { event.prevent_default(); confirming.set(false); },
                    Key::Tab => { event.prevent_default(); super::board_recovery_panel::cycle_modal_focus("gs26-radio-test-confirm", event.modifiers().contains(Modifiers::SHIFT)); },
                    _ => {}
                }},
                div { id: "gs26-radio-test-confirm", role: "alertdialog", "aria-modal": "true", "aria-labelledby": "radio-test-title",
                    style: "background:var(--gs26-panel-background,#142534);color:var(--gs26-app-text,#e5e7eb);font:inherit;padding:var(--gs-space-5);border-radius:var(--gs-radius-lg);width:min(100%,640px);max-height:calc(100dvh - 40px);display:flex;flex-direction:column;gap:var(--gs-space-3);overflow-y:auto;box-sizing:border-box;overflow-wrap:anywhere;",
                    h2 { id: "radio-test-title", style: "margin:0;font-size:var(--gs-text-lg);", "{confirm_title(&variant())}" }
                    p { style: "margin:0;font-weight:700;", "{lines[0]}" }
                    p { style: "margin:0;", "{lines[1]}" }
                    p { style: "margin:0;", "{lines[2]}" }
                    div { style: "display:flex;gap:var(--gs-space-2);flex-wrap:wrap;justify-content:flex-end;",
                        button { r#type: "button", class: "gs-btn", autofocus: true, disabled: sending(), onclick: move |_| confirming.set(false), "Cancel" }
                        button { r#type: "button", class: "gs-btn gs-btn--primary", disabled: sending(),
                            onclick: move |_| send(START_PATH, serde_json::json!({"variant": variant(), "duration_s": chosen_m * 60, "confirm": true}), true),
                            if sending() { "Starting…" } else { "Start radio test" }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status(value: serde_json::Value) -> RadioTestStatus {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn the_banner_shows_the_primarys_own_test_and_on_an_rrls_the_announced_one() {
        assert_eq!(banner(&RadioTestStatus::default()), None);
        let relay = status(json!({"role":"primary","banner":"RADIO TEST: direct AV-bay link rejected (Force relay)",
            "active":{"variant":"force_relay","variant_label":"Force relay","actor":"rylan","remaining_s":599,"relay_engaged":true}}));
        let (title, detail) = banner(&relay).unwrap();
        assert_eq!(title, "RADIO TEST: direct AV-bay link rejected (Force relay)");
        assert_eq!(detail, "Force relay by rylan; 9:59 left. The RRLS relay carries the AV bay.");
        let silent = status(json!({"active":{"variant":"silent_primary","variant_label":"Force failover (silent primary)","actor":"a","remaining_s":3600,"filtering":true,"abort_pending_ms":null}}));
        let (title, detail) = banner(&silent).unwrap();
        assert!(title.starts_with("RADIO TEST: direct AV-bay link rejected"), "{title}");
        assert!(detail.contains("1:00:00 left") && detail.contains("radio-silent"), "{detail}");
        let rrls = status(json!({"role":"rrls","primary":{"variant":"force_relay","variant_label":"Force relay","remaining_s_at_most":120,
            "banner":"RADIO TEST: direct AV-bay link rejected by the primary (Force relay)"}}));
        let (title, detail) = banner(&rrls).unwrap();
        assert!(title.contains("by the primary"), "{title}");
        assert!(detail.contains("at most 2:00 left"), "{detail}");
    }

    #[test]
    fn start_is_disabled_with_the_local_reason_first_then_the_stations() {
        let ready = status(json!({"available":true,"may_start":true,"may_stop":true}));
        assert_eq!(start_disabled_reason(Some(&ready), false, true), None);
        assert!(enabled(Some(&ready), false, true));
        assert!(start_disabled_reason(Some(&ready), true, true).unwrap().contains("primary ground station"));
        assert!(start_disabled_reason(Some(&ready), false, false).unwrap().contains("Not connected"));
        assert!(start_disabled_reason(None, false, true).is_some());
        let operator = status(json!({"available":true,"may_start":false,"may_stop":true}));
        assert!(start_disabled_reason(Some(&operator), false, true).unwrap().contains("administrator"));
        let armed = status(json!({"available":true,"may_start":true,"start_blocked_reason":"Only on the ground in Startup, Idle or PreFill; the station is in Armed."}));
        assert!(start_disabled_reason(Some(&armed), false, true).unwrap().contains("Armed"));
        let no_mesh = status(json!({"available":false,"unavailable_reason":"Radio recovery is not configured."}));
        assert_eq!(start_disabled_reason(Some(&no_mesh), false, true).as_deref(), Some("Radio recovery is not configured."));
        // Running: an operator who may stop it sees it enabled (Stop).
        let running = status(json!({"available":true,"may_start":false,"may_stop":true,"active":{"variant":"force_relay"}}));
        assert!(enabled(Some(&running), false, true));
    }

    #[test]
    fn the_confirmation_names_the_variant_and_how_abort_still_works() {
        assert!(confirm_title("force_relay").contains("RRLS relay"));
        assert!(confirm_title("silent_primary").contains("takeover"));
        let relay = confirm_lines("force_relay", 10, 4_000);
        assert!(relay[0].contains("link to the RRLS stays up"), "{}", relay[0]);
        assert!(relay[1].contains("through the RRLS relay") && relay[1].contains("within 4 s"), "{}", relay[1]);
        assert!(relay[2].contains("after 10 min") && relay[2].contains("Startup, Idle or PreFill"), "{}", relay[2]);
        let silent = confirm_lines("silent_primary", 5, 4_000);
        assert!(silent[0].contains("stops receiving and transmitting"), "{}", silent[0]);
        assert!(silent[1].contains("ends the silence at once"), "{}", silent[1]);
    }

    #[test]
    fn outcomes_show_the_stations_refusal() {
        assert_eq!(outcome_message(true, Ok(json!({}))), "Radio test started.");
        assert!(outcome_message(false, Ok(json!({}))).starts_with("Radio test stopped"));
        let refused = outcome_message(true, Err(r#"HTTP 409: {"error":"radio_test_refused","message":"Only on the ground in Startup, Idle or PreFill; the station is in Armed."}"#.into()));
        assert_eq!(refused, "Radio test not started: Only on the ground in Startup, Idle or PreFill; the station is in Armed.");
        assert_eq!(remaining(59), "0:59");
        assert_eq!(stop_reason_label("abort_escape"), "Abort sent direct (relay did not confirm)");
    }
}
