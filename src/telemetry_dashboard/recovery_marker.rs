use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct Clock {
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
#[component]
pub(super) fn RecoveryMarker() -> Element {
    let mut status = use_signal(Status::default);
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    use_future(move || async move {
        loop {
            match super::http_get_json::<Status>("/api/recovery").await {
                Ok(value) => {
                    crate::auth::RRLS_CLOCK_RESET_SUPPORTED.store(
                        value.clock_reset_request_supported,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                    status.set(value);
                }
                Err(_) => {
                    crate::auth::RRLS_CLOCK_RESET_SUPPORTED
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                    status.write().recovery_marker_supported = false;
                }
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(1000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
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
    rsx! {div {class:"gs26-recovery-marker",style:"padding:6px 12px;border-bottom:1px solid #435165;display:flex;gap:10px;align-items:center;flex-wrap:wrap;",
        button {disabled, title:"Record arrival at the rocket; does not send a hardware command",onclick:move |_|{
            busy.set(true);let session=session.clone();spawn(async move {
                match super::http_post_json::<Mark,Event>("/api/recovery/reached",&Mark{launch_session:session}).await {
                    Ok(event)=>{message.set(format!("Saved rocket arrival {}",elapsed(event.t_relative_ms)));status.write().events.push(event);},
                    Err(error)=>message.set(format!("Recovery mark was not confirmed saved: {error}")),
                }busy.set(false);
            });
        },if marked.is_some(){"Rocket reached"}else{"Mark recovered"}}
        if let Some(text)=text {span{role:"status","{text}"}}
        if auth.station_role.as_deref()==Some("rrls") && current.clock_reset_request_supported {
            button {disabled:*busy.read()||!can_mark||current.reset.pending,onclick:move |_|super::send_cmd_from_click("ResetTClock"),"Request T clock reset"}
        }
        if current.reset.pending {span{role:"status","Primary T clock reset pending · clock retained"}}
        if let Some(error)=current.reset.error {span{role:"alert","Primary reset: {error}"}}
        if current.clock_known==Some(false) {span{role:"alert","Saved T0 is unknown · explicit primary T clock reset required"}}
        else if !current.synchronized {span{"Clock retained offline · primary sync pending"}}
        if let Some(error)=current.persistence_error {span{role:"alert","History storage: {error}"}}
        for kind in ["json","csv"] {button {disabled:*busy.read(),onclick:move |_|{spawn(async move {let path=format!("/api/recovery/export.{kind}");message.set(match super::recording_download::download_csv_path(&path,&format!("launch-recovery-history.{kind}")).await {Ok(text)=>text,Err(error)=>format!("Export failed: {error}")});});},"Export {kind}"}}
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
