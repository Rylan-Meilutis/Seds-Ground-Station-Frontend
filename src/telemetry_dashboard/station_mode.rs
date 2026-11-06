//! Primary operating mode is presentation metadata, never a control permission.
use dioxus::prelude::*;
use serde_json::Value;

pub(super) static OPERATING_MODE: GlobalSignal<Value> = Signal::global(|| Value::Null);
type SourceContext = (String, Option<crate::auth::RequestIdentity>);
static OPERATING_MODE_SOURCE: GlobalSignal<Option<SourceContext>> = Signal::global(|| None);
pub(super) fn current_snapshot() -> Value {
    let current = (super::UrlConfig::base_http(), crate::auth::request_identity());
    if OPERATING_MODE_SOURCE.read().as_ref() == Some(&current) {
        OPERATING_MODE.read().clone()
    } else {
        Value::Null
    }
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
    if observer {
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
        *OPERATING_MODE_SOURCE.write() = Some((super::UrlConfig::base_http(), crate::auth::request_identity()));
        *OPERATING_MODE.write() = crate::auth::current_status()
            .operating_mode
            .unwrap_or(Value::Null);
        let mut context = (super::UrlConfig::base_http(), crate::auth::request_identity());
        loop {
            let source = (super::UrlConfig::base_http(), crate::auth::request_identity());
            if context != source {
                *OPERATING_MODE.write() = Value::Null;
                *OPERATING_MODE_SOURCE.write() = None;
                context = source.clone();
            }
            let response = super::http_get_json::<Value>("/api/station_mode").await;
            if source != (super::UrlConfig::base_http(), crate::auth::request_identity()) {
                // The next loop observes the current authority independently.
                // A retired completion must not clear replacement metadata.
                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::TimeoutFuture::new(2000).await;
                #[cfg(not(target_arch = "wasm32"))]
                tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                continue;
            }
            match response {
                Ok(value) => {
                    *OPERATING_MODE_SOURCE.write() = Some(source);
                    *OPERATING_MODE.write() = value;
                },
                Err(_) => {
                    let mut value = OPERATING_MODE.read().clone();
                    if let Some(object) = value.as_object_mut() {
                        object.insert("synchronized".into(), Value::Bool(false));
                        if let Some(configuration) = object.get_mut("retained_configuration").and_then(Value::as_object_mut)
                            && configuration.get("status").and_then(Value::as_str) != Some("unknown")
                        {
                            configuration.insert("status".into(), Value::String("disconnected".into()));
                        }
                    }
                    *OPERATING_MODE.write() = value;
                }
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(2000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
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
