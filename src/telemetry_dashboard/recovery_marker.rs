use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct Clock {
    authority:String,
    launch_action_utc_ms:Option<i64>,
    session: String,
    t0_utc_ms: Option<i64>,
}
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct Event {
    launch_session: String,
    station: String,
    t_relative_ms: i64,
    reached_utc_ms: i64,
    actor: String,
}
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct Reset {
    pending: bool,
    error: Option<String>,
}
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct Status {
    recovery_marker_supported: bool,
    clock_reset_request_supported: bool,
    reset: Reset,
    clock: Clock,
    synchronized: bool,
    clock_known: Option<bool>,
    persistence_error: Option<String>,
    station: String,
    events: Vec<Event>,
}
#[derive(Serialize)]
struct Mark {
    launch_session: String,
}
fn utc(ms: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000)
        .ok()
        .map(|value| format!("{value} UTC"))
        .unwrap_or_else(|| format!("{ms} UTC milliseconds"))
}
fn elapsed(ms: i64) -> String {
    let sign = if ms < 0 { "−" } else { "+" };
    let seconds = ms.unsigned_abs() / 1000;
    format!(
        "T{sign}{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}
static RECOVERY_STATUS: GlobalSignal<Status> = Signal::global(Status::default);

pub(super) fn dashboard_clock()->(String,Option<i64>) {
    let status=RECOVERY_STATUS.read();
    let identity=format!("{}:{}:{}",super::UrlConfig::base_http(),status.clock.authority,status.clock.session);
    let anchor=if status.clock_known==Some(true) {status.clock.launch_action_utc_ms}else{None};
    (identity,anchor)
}

/// Keep protocol capability/current status live without rendering global controls.
#[component]
pub(super) fn RecoveryRuntime() -> Element {
    use_future(move || async move {
        // A native station switch must not briefly expose the previous
        // station's launch-session metadata or reset capability.
        *RECOVERY_STATUS.write() = Status::default();
        crate::auth::RRLS_CLOCK_RESET_SUPPORTED.store(false, std::sync::atomic::Ordering::Relaxed);
        loop {
            match super::http_get_json::<Status>("/api/recovery").await {
                Ok(value) => {
                    crate::auth::RRLS_CLOCK_RESET_SUPPORTED.store(
                        value.clock_reset_request_supported,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    *RECOVERY_STATUS.write() = value;
                }
                Err(_) => {
                    crate::auth::RRLS_CLOCK_RESET_SUPPORTED
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                    RECOVERY_STATUS.write().recovery_marker_supported = false;
                }
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
pub(super) fn RecoveryMarker() -> Element {
    let requests = super::station_settings_request::use_factory();
    let mut status = RECOVERY_STATUS.signal();
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let current = status.read().clone();
    // An older backend does not advertise this metadata-only capability.
    if !current.recovery_marker_supported {
        return rsx! {};
    }
    let auth = crate::auth::current_status();
    let can_mark = auth.authenticated
        && (auth.roles.iter().any(|role| role == "admin")
            || (auth.permissions.view_data && auth.permissions.send_commands));
    let marked = current.events.iter().find(|event| {
        event.launch_session == current.clock.session && event.station == current.station
    });
    let disabled =
        *busy.read() || !can_mark || current.clock.t0_utc_ms.is_none() || marked.is_some();
    let text = marked.map(|event| {
        format!(
            "Rocket reached {} · {} · {}",
            elapsed(event.t_relative_ms),
            event.actor,
            utc(event.reached_utc_ms)
        )
    });
    let session = current.clock.session.clone();
    // ResetTClock is a local RRLS operation (its backend asks the primary), so
    // it goes over this station's own socket, never the primary command proxy.
    // When this station would refuse it, say so rather than take a press that
    // sends nothing.
    let policy = super::ACTION_POLICY_SIGNAL.read();
    let refusal = if !crate::auth::can_send_command("ResetTClock") {
        Some("not permitted for this account".to_string())
    } else if !super::action_policy_control_enabled(&policy, "ResetTClock") {
        Some(policy.controls.iter().find(|c| c.cmd == "ResetTClock")
            .and_then(|c| c.disabled_reason.clone()).filter(|r| !r.trim().is_empty())
            .unwrap_or_else(|| "disabled by this station".into()))
    } else {
        None
    };
    drop(policy);
    let reset_allowed = !*busy.read() && can_mark && !current.reset.pending && refusal.is_none();
    rsx! {div {class:"gs26-recovery-marker",style:"padding:var(--gs-space-3);border:1px solid var(--gs-line);border-radius:var(--gs-radius);display:flex;gap:var(--gs-space-2);align-items:center;flex-wrap:wrap;",
        strong {style:"flex:1 0 100%;", "Recovery"}
        button {disabled, title:"Record arrival at the rocket; does not send a hardware command",onclick:move |_|{
            busy.set(true);let session=session.clone();spawn(async move {
                let result=requests.post::<Mark,Event>("/api/recovery/reached",&Mark{launch_session:session}).await; if !requests.current(){return;} match result {
                    Ok(event)=>{message.set(format!("Saved rocket arrival {}",elapsed(event.t_relative_ms)));status.write().events.push(event);},
                    Err(error)=>message.set(format!("Recovery mark was not confirmed saved: {error}")),
                }busy.set(false);
            });
        },if marked.is_some(){"Rocket reached"}else{"Mark recovered"}}
        if let Some(text)=text {span{role:"status","{text}"}}
        if auth.station_role.as_deref()==Some("rrls") && current.clock_reset_request_supported {
            button {disabled:!reset_allowed,title:refusal.clone().unwrap_or_default(),
                onpointerdown:move |event|{if reset_allowed {super::command_pointer_down_event("recovery:tclock","ResetTClock",&event,false);}},
                onpointerup:move |event|{super::command_pointer_up_event("recovery:tclock",&event);},
                onpointercancel:move |event|{super::command_pointer_cancel_event("recovery:tclock",&event);},
                ontouchstart:move |event|{if reset_allowed {super::command_touch_start_event("recovery:tclock","ResetTClock",&event,false);}},
                ontouchend:move |event|{super::command_touch_end_event("recovery:tclock",&event);},
                ontouchcancel:move |event|{super::command_touch_cancel_event("recovery:tclock",&event);},
                onmousedown:move |event|{if reset_allowed {super::command_mouse_down_event("recovery:tclock","ResetTClock",&event,false);}},
                onmouseup:move |event|{super::command_mouse_up_event("recovery:tclock",&event);},
                onkeydown:move |event|{if reset_allowed {super::command_key_down_event("recovery:tclock","ResetTClock",&event,false);}},
                onkeyup:move |event|{super::command_key_up_event("recovery:tclock",&event);},
                onclick:move |event|{if reset_allowed {super::command_click("recovery:tclock","ResetTClock",&event,false);}},
                "Request T clock reset"
            }
        }
        if auth.station_role.as_deref()==Some("rrls") && current.clock_reset_request_supported {if let Some(reason)=refusal.as_ref() {span{role:"status","T clock reset unavailable: {reason}"}}}
        if current.reset.pending {span{role:"status","Primary T clock reset pending · clock retained"}}
        if let Some(error)=current.reset.error {span{role:"alert","Primary reset: {error}"}}
        if current.clock_known==Some(false) {span{role:"alert","Saved T0 is unknown · explicit primary T clock reset required"}}
        else if !current.synchronized {span{"Clock retained offline · primary sync pending"}}
        if let Some(error)=current.persistence_error {span{role:"alert","History storage: {error}"}}

        if !message.read().is_empty(){span{role:"status","{message}"}}
    }}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_backend_never_enables_marker() {
        let status: Status = serde_json::from_str("{}").unwrap();
        assert!(!status.recovery_marker_supported);
    }
    #[test]
    fn signed_elapsed() {
        assert_eq!(elapsed(-10000), "T−00:00:10");
        assert_eq!(elapsed(90000), "T+00:01:30");
    }
}
