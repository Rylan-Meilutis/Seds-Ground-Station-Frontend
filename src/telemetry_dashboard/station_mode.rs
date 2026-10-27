//! Primary operating mode is presentation metadata, never a control permission.
use dioxus::prelude::*;
use serde_json::Value;

pub(super) static OPERATING_MODE: GlobalSignal<Value> = Signal::global(|| Value::Null);

pub(super) fn description(mode: &Value, observer: bool) -> String {
    let name = match mode["effective_mode"].as_str() {
        Some("normal") => "Normal",
        Some("hitl") => "HITL",
        Some("test_fire") => "Test fire",
        _ => "unavailable",
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
    let local = match mode["local_build_mode"].as_str() {
        Some("hitl") => "HITL",
        Some("normal") => "Normal",
        Some("test_fire") => "Test fire",
        _ => "unknown",
    };
    if observer {
        format!(
            "RRLS observer · {scope}: {name}{freshness} · Local build: {local} · Live operations remain read-only"
        )
    } else {
        format!("{scope}: {name}{freshness}")
    }
}

#[component]
pub(super) fn OperatingModeNotice() -> Element {
    let observer = crate::auth::current_status().station_role.as_deref() == Some("rrls");
    use_future(move || async move {
        // Auth handover already returns this snapshot; do not briefly guess Normal.
        *OPERATING_MODE.write() = crate::auth::current_status()
            .operating_mode
            .unwrap_or(Value::Null);
        loop {
            match super::http_get_json::<Value>("/api/station_mode").await {
                Ok(value) => *OPERATING_MODE.write() = value,
                Err(_) => {
                    let mut value = OPERATING_MODE.read().clone();
                    if let Some(object) = value.as_object_mut() {
                        object.insert("synchronized".into(), Value::Bool(false));
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
    let mode = OPERATING_MODE.read();
    // Preserve compatibility with older primary servers that have no mode API.
    if !observer && mode.is_null() {
        return rsx! {};
    }
    let text = description(&mode, observer);
    rsx! { p { role:"status",class:"gs26-operating-mode",style:"flex-shrink:0;margin:0 0 8px;font-size:12px;overflow-wrap:anywhere;", "{text}" } }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observer_mode_never_guesses_normal_or_claims_local_hitl_capabilities() {
        assert!(description(&Value::Null, true).contains("unavailable"));
        let mut mode = serde_json::json!({"effective_mode":"hitl","local_build_mode":"normal","synchronized":true});
        let text = description(&mode, true);
        assert!(text.contains("Primary operating mode: HITL"));
        assert!(text.contains("Local build: Normal"));
        assert!(text.contains("read-only"));
        mode["synchronized"] = Value::Bool(false);
        assert!(description(&mode, true).contains("last known; not synchronized"));
    }
}
