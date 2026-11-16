//! Primary operating mode is presentation metadata, never a control permission.
use dioxus::prelude::*;
use serde_json::Value;

pub(super) static OPERATING_MODE: GlobalSignal<Value> = Signal::global(|| Value::Null);
/// The station (base URL) the displayed mode was read from. The mode belongs to
/// the station, not the account, so a login/session refresh on the same station
/// keeps showing it while the next poll confirms it.
static OPERATING_MODE_SOURCE: GlobalSignal<Option<String>> = Signal::global(|| None);
/// How long the last successful mode read stays "synchronized" while polls fail.
/// A loaded station can miss or delay an individual poll (and a websocket
/// reconnect cancels the one in flight); one miss must not flip the notice.
const MODE_SYNC_GRACE_MS: f64 = 5_000.0;
const MODE_POLL_MS: u32 = 2_000;
/// Retry delay after a failed poll, so a single miss is retried well inside the grace.
const MODE_RETRY_MS: u32 = 500;
pub(super) fn current_snapshot() -> Value {
    if OPERATING_MODE_SOURCE.read().as_deref() == Some(super::UrlConfig::base_http().as_str()) {
        OPERATING_MODE.read().clone()
    } else {
        Value::Null
    }
}
/// The mode to display after a failed poll: unchanged inside the grace window,
/// otherwise marked last-known (never cleared, never guessed).
fn after_failed_poll(mut value: Value, last_ok_ms: Option<f64>, now_ms: f64) -> Value {
    let age = last_ok_ms.map(|at| now_ms - at);
    if age.is_some_and(|age| age.is_finite() && (0.0..=MODE_SYNC_GRACE_MS).contains(&age)) {
        return value;
    }
    if let Some(object) = value.as_object_mut() {
        object.insert("synchronized".into(), Value::Bool(false));
        if let Some(configuration) = object.get_mut("retained_configuration").and_then(Value::as_object_mut)
            && configuration.get("status").and_then(Value::as_str) != Some("unknown")
        {
            configuration.insert("status".into(), Value::String("disconnected".into()));
        }
    }
    value
}
async fn sleep_ms(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(u64::from(ms))).await;
}

fn is_simulated_primary(mode: &Value) -> bool {
    mode["execution_mode"].as_str() == Some("simulated_primary")
        && mode["output_domain"].as_str() == Some("simulated")
}
fn is_flight_sim(mode: &Value) -> bool {
    mode["execution_mode"].as_str() == Some("flight_sim")
        && mode["output_domain"].as_str() == Some("simulated")
}
pub(super) fn description(mode: &Value, observer: bool) -> String {
    let simulated_flight = is_flight_sim(mode);
    let name = if simulated_flight {
        "FlightSim · simulated outputs"
    } else {
        match mode["effective_mode"].as_str() {
        Some("normal") => "Normal",
        Some("hitl") => "HITL",
        Some("test_fire") => "Test fire",
        Some("testing") => "Testing",
        _ => "unavailable",
        }
    };
    let scope = if observer {
        "Primary operating mode"
    } else {
        "Operating mode"
    };
    let freshness = if mode["effective_mode"].is_string() {
        if mode["synchronized"].as_bool() == Some(true) {
            ""
        } else {
            " (last known; not synchronized)"
        }
    } else {
        ""
    };
    if observer && is_simulated_primary(mode) {
        format!("RRLS observer · SIMULATED paired primary (no radio) · {scope}: {name}{freshness} · Live operations remain read-only")
    } else if observer {
        format!("RRLS observer · {scope}: {name}{freshness} · Live operations remain read-only")
    } else {
        format!("{scope}: {name}{freshness}")
    }
}

pub(super) fn configuration_setting(mode: &Value, index: usize) -> Option<bool> {
    let configuration = &mode["retained_configuration"];
    if mode["effective_mode"].as_str() != Some("hitl")
        || configuration["mode"].as_str() != Some("hitl")
        || !matches!(configuration["status"].as_str(), Some("current" | "last_known" | "disconnected"))
    {
        return None;
    }
    configuration["settings"].as_array()?.get(index)?.as_bool()
}
pub(super) fn configuration_description(mode: &Value) -> String {
    let configuration = if mode["effective_mode"].is_string()
        && mode["effective_mode"] == mode["retained_configuration"]["mode"]
    {
        &mode["retained_configuration"]
    } else {
        &Value::Null
    };
    let label = match configuration["status"].as_str() {
        Some("current") => "Current",
        Some("last_known") => "Last known",
        Some("disconnected") => "Disconnected · last known",
        _ => "Unknown",
    };
    let evidence = match (configuration["source_revision"].as_u64(), configuration["record_generation"].as_u64()) {
        (Some(source), Some(record)) => format!(" · saved source revision {source}, record {record}"),
        _ => String::new(),
    };
    let compile_scope = match configuration["signed_test_fire_scope"].as_bool() {
        Some(true) => " · Test fire scope",
        Some(false) => " · Flight scope",
        None => " · Compile scope unknown",
    };
    let error = if mode["retained_configuration"]["error"].is_string() {" · Configuration could not be saved or restored"} else {""};
    let pending = if mode["retained_configuration"]["persistence_pending"].as_bool() == Some(true) {" · Save pending; displayed saved values remain last known"} else {""};
    format!("Configuration: {label}{compile_scope}{evidence}{error}{pending}. Stored settings do not authorize commands.")
}

#[component]
pub(super) fn OperatingModeNotice(#[props(default = true)] visible: bool) -> Element {
    let observer = crate::auth::current_status().station_role.as_deref() == Some("rrls");
    use_future(move || async move {
        // Auth handover already returns this snapshot; do not briefly guess Normal.
        let mut base = super::UrlConfig::base_http();
        if OPERATING_MODE_SOURCE.peek().as_deref() != Some(base.as_str()) || OPERATING_MODE.peek().is_null() {
            *OPERATING_MODE_SOURCE.write() = Some(base.clone());
            *OPERATING_MODE.write() = crate::auth::current_status()
                .operating_mode
                .unwrap_or(Value::Null);
        }
        // The handover snapshot counts as a fresh read for the grace window.
        let mut last_ok_ms = (!OPERATING_MODE.peek().is_null()).then(super::monotonic_now_ms);
        loop {
            let source = super::UrlConfig::base_http();
            if source != base {
                // Another station: its mode is unknown until read from it.
                *OPERATING_MODE.write() = Value::Null;
                *OPERATING_MODE_SOURCE.write() = Some(source.clone());
                base = source.clone();
                last_ok_ms = None;
            }
            let response = super::http_get_json::<Value>("/api/station_mode").await;
            if source != super::UrlConfig::base_http() {
                // A retired completion must not write another station's notice.
                continue;
            }
            let now = super::monotonic_now_ms();
            let delay = match response {
                Ok(value) if value.is_object() => {
                    last_ok_ms = Some(now);
                    *OPERATING_MODE_SOURCE.write() = Some(source);
                    if *OPERATING_MODE.peek() != value {
                        *OPERATING_MODE.write() = value;
                    }
                    MODE_POLL_MS
                }
                _ => {
                    let value = after_failed_poll(OPERATING_MODE.peek().clone(), last_ok_ms, now);
                    if *OPERATING_MODE.peek() != value {
                        *OPERATING_MODE.write() = value;
                    }
                    MODE_RETRY_MS
                }
            };
            sleep_ms(delay).await;
        }
    });
    let mode = current_snapshot();
    // Preserve compatibility with older primary servers that have no mode API.
    if (!visible && !is_flight_sim(&mode)) || (!observer && mode.is_null()) {
        return rsx! {};
    }
    let text = description(&mode, observer);
    rsx! { p { role:"status",class:"gs26-operating-mode",style:"flex-shrink:0;margin:0 0 8px;font-size:12px;overflow-wrap:anywhere;", "{text}" } }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_failed_poll_keeps_the_synchronized_mode_until_the_grace_expires() {
        let mode = serde_json::json!({"effective_mode":"test_fire","synchronized":true,
            "retained_configuration":{"mode":"test_fire","status":"current"}});
        let held = after_failed_poll(mode.clone(), Some(10_000.0), 10_000.0 + MODE_SYNC_GRACE_MS);
        assert_eq!(held, mode);
        assert_eq!(description(&held, false), "Operating mode: Test fire");
        let expired = after_failed_poll(mode.clone(), Some(10_000.0), 10_001.0 + MODE_SYNC_GRACE_MS);
        assert_eq!(expired["effective_mode"], "test_fire", "never cleared or guessed");
        assert!(description(&expired, false).contains("last known; not synchronized"));
        assert_eq!(expired["retained_configuration"]["status"], "disconnected");
        let never = after_failed_poll(mode.clone(), None, 0.0);
        assert_eq!(never["synchronized"], false, "no successful poll yet is not synchronized");
        assert_eq!(after_failed_poll(Value::Null, None, 0.0), Value::Null);
    }
    #[test]
    fn retained_settings_are_labeled_and_never_inherited_by_another_mode() {
        let mut mode=serde_json::json!({"effective_mode":"hitl","retained_configuration":{"mode":"hitl","status":"current","settings":[false,true,null,false],"source_revision":9,"record_generation":2}});
        assert_eq!(configuration_setting(&mode,0),Some(false));
        assert_eq!(configuration_setting(&mode,2),None);
        assert!(configuration_description(&mode).contains("Current"));
        for (status,label) in [("last_known","Last known"),("disconnected","Disconnected"),("unknown","Unknown")] {
            mode["retained_configuration"]["status"]=serde_json::json!(status);
            assert!(configuration_description(&mode).contains(label));
            assert_eq!(configuration_setting(&mode,0),if status=="unknown"{None}else{Some(false)});
        }
        mode["retained_configuration"]["status"]=serde_json::json!("disconnected");
        mode["effective_mode"]=serde_json::json!("normal");
        assert_eq!(configuration_setting(&mode,0),None);
        assert!(configuration_description(&mode).contains("Unknown"));
        assert!(configuration_description(&mode).contains("do not authorize"));
        mode["retained_configuration"]["mode"]=Value::Null;
        mode["retained_configuration"]["error"]=serde_json::json!("fixture disk failure");
        assert!(configuration_description(&mode).contains("could not be saved or restored"));
    }
    #[test]
    fn observer_mode_never_guesses_normal_or_claims_local_hitl_capabilities() {
        assert!(description(&Value::Null, true).contains("unavailable"));
        let mut mode = serde_json::json!({"effective_mode":"hitl","local_build_mode":"normal","synchronized":true});
        let text = description(&mode, true);
        assert!(text.contains("Primary operating mode: HITL"));
        assert!(!text.contains("Local build"));
        assert!(text.contains("read-only"));
        mode["effective_mode"] = Value::String("testing".into());
        assert!(description(&mode, true).contains("Primary operating mode: Testing"));
        mode["synchronized"] = Value::Bool(false);
        assert!(description(&mode, true).contains("last known; not synchronized"));
        mode["execution_mode"] = Value::String("simulated_primary".into());
        assert!(!description(&mode, true).contains("SIMULATED"), "needs both domain fields");
        mode["output_domain"] = Value::String("simulated".into());
        assert!(description(&mode, true).contains("SIMULATED paired primary"));
    }
    #[test]
    fn flight_sim_notice_requires_both_current_execution_domain_fields() {
        let legacy = serde_json::json!({"effective_mode":"testing","synchronized":true});
        assert!(description(&legacy, false).contains("Operating mode: Testing"));
        let mut mode=legacy.clone();
        mode["execution_mode"]=serde_json::json!("flight_sim");
        assert!(!description(&mode,false).contains("FlightSim"));
        mode["output_domain"]=serde_json::json!("build_defined");
        assert!(!description(&mode,false).contains("FlightSim"));
        mode["output_domain"]=serde_json::json!("simulated");
        assert!(description(&mode,false).contains("Operating mode: FlightSim · simulated outputs"));
        assert!(is_flight_sim(&mode),"isolated execution remains conspicuous even when generic notice is hidden");
        assert_eq!(mode["effective_mode"],"testing","display never changes compiled/proof mode");
        assert!(description(&mode,true).contains("read-only"));
        mode["execution_mode"]=serde_json::json!("compiled_mode");
        assert!(description(&mode,false).contains("Operating mode: Testing"));
        assert!(!is_flight_sim(&mode));
    }

}
