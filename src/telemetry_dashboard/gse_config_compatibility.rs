//! Current GET-config schema presence is the capability, never a guessed version.
use super::super::recording_download::{ExportOwner, saved_json};
use super::Settings;
use serde_json::Value;
use std::rc::Rc;

#[derive(Clone)]
pub(super) struct Grant {
    owner: Rc<ExportOwner>,
    cold_flow: bool,
    turbo_deadband: bool,
}
fn supports_cold_flow(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.contains_key("cold_flow_target_psi"))
}
fn supports_turbo_deadband(value: &Value) -> bool {
    value.as_object().is_some_and(|object| object.contains_key("turbocharge_pressure_deadband_psi"))
}
fn payload(config: &Settings, supported: bool, turbo_deadband: bool) -> Result<Value, String> {
    if !config.turbocharge_pressure_deadband_psi.is_finite()
        || !(8.0..=16.0).contains(&config.turbocharge_pressure_deadband_psi) {
        return Err("Turbocharge pressure deadband must be 8–16 psi.".into());
    }
    if !turbo_deadband && config.turbocharge_pressure_deadband_psi != super::default_turbocharge_deadband() {
        return Err("This ground station does not support the Turbocharge pressure deadband; the value was not saved.".into());
    }
    if !supported && config.cold_flow_target_psi.is_some() {
        return Err("This ground station does not support the Cold Flow target. Reload its configuration; the value was not saved.".into());
    }
    if config
        .cold_flow_target_psi
        .is_some_and(|value| !value.is_finite() || value <= 0.)
    {
        return Err(
            "Cold Flow target must be a finite positive pressure, or blank to disable it.".into(),
        );
    }
    // Preserve the original Settings serializer's f32 JSON representation;
    // converting f32 directly to Value would widen unrelated field decimals.
    let serialized = serde_json::to_string(config).map_err(|error| error.to_string())?;
    let mut value: Value = serde_json::from_str(&serialized).map_err(|error| error.to_string())?;
    if !supported {
        value
            .as_object_mut()
            .ok_or("Invalid GSE configuration")?
            .remove("cold_flow_target_psi");
    }
    if !turbo_deadband {
        value.as_object_mut().ok_or("Invalid GSE configuration")?.remove("turbocharge_pressure_deadband_psi");
    }
    Ok(value)
}
impl Grant {
    pub(super) async fn load() -> Result<(Settings, Self), String> {
        let owner = ExportOwner::capture()?;
        let value = saved_json::<Value>("/api/gse/config", owner.clone()).await?;
        Self::from_response(value, owner)
    }
    fn from_response(value: Value, owner: Rc<ExportOwner>) -> Result<(Settings, Self), String> {
        owner.check()?;
        let cold_flow = supports_cold_flow(&value);
        let turbo_deadband = supports_turbo_deadband(&value);
        let config = serde_json::from_value(value)
            .map_err(|error| format!("Invalid GSE configuration: {error}"))?;
        Ok((config, Self { owner, cold_flow, turbo_deadband }))
    }
    pub(super) fn current(&self) -> bool {
        self.owner.is_current()
    }
    pub(super) fn cold_flow_supported(&self) -> bool {
        self.current() && self.cold_flow
    }
    pub(super) fn turbo_deadband_supported(&self) -> bool {
        self.current() && self.turbo_deadband
    }
    pub(super) async fn status<T: for<'de> serde::Deserialize<'de>>(&self) -> Result<T, String> {
        saved_json("/api/gse/status", self.owner.clone()).await
    }
    pub(super) async fn save(&self, config: &Settings) -> Result<(Settings, Self), String> {
        self.owner.check()?;
        let body = payload(config, self.cold_flow, self.turbo_deadband)?;
        let value = post_owned(body, self.owner.clone()).await?;
        let (next, grant) = Self::from_response(value, self.owner.clone())?;
        if self.cold_flow
            && (!grant.cold_flow || next.cold_flow_target_psi != config.cold_flow_target_psi)
        {
            return Err("The ground station did not confirm the requested Cold Flow target; it is not reported as saved.".into());
        }
        if self.turbo_deadband && (!grant.turbo_deadband
            || next.turbocharge_pressure_deadband_psi != config.turbocharge_pressure_deadband_psi) {
            return Err("The ground station did not confirm the requested Turbocharge pressure deadband; it is not reported as saved.".into());
        }
        Ok((next, grant))
    }
}
#[cfg(not(target_arch = "wasm32"))]
async fn post_owned(body: Value, owner: Rc<ExportOwner>) -> Result<Value, String> {
    owner.check()?;
    let base = if owner.base().is_empty() {
        "http://localhost:3000"
    } else {
        owner.base().trim_end_matches('/')
    };
    let client = crate::auth::build_native_http_client(
        owner.skip_tls(),
        std::time::Duration::from_secs(8),
        std::time::Duration::from_secs(8),
    )?;
    let mut request = client.post(format!("{base}/api/gse/config")).json(&body);
    if let Some(token) = owner.token() {
        request = request.bearer_auth(token);
    }
    owner.check()?;
    let response = owner
        .wait(request.send())
        .await?
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let text = owner
        .wait(response.text())
        .await?
        .map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "HTTP {status}: {}",
            text.chars().take(200).collect::<String>()
        ));
    }
    owner.check()?;
    serde_json::from_str(&text).map_err(|error| error.to_string())
}
#[cfg(target_arch = "wasm32")]
async fn post_owned(body: Value, owner: Rc<ExportOwner>) -> Result<Value, String> {
    owner.check()?;
    let base = if owner.base().is_empty() {
        web_sys::window()
            .ok_or("Window unavailable")?
            .location()
            .origin()
            .map_err(|_| "Origin unavailable")?
    } else {
        owner.base().trim_end_matches('/').to_owned()
    };
    let controller =
        web_sys::AbortController::new().map_err(|_| "Configuration cancellation unavailable")?;
    let _cancel = super::super::HttpAbortOnDrop(controller.clone());
    let mut request = gloo_net::http::Request::post(&format!("{base}/api/gse/config"));
    if let Some(token) = owner.token() {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let signal = controller.signal();
    let request = request
        .abort_signal(Some(&signal))
        .json(&body)
        .map_err(|error| error.to_string())?;
    owner.check()?;
    let response = owner
        .wait(request.send())
        .await?
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let text = owner
        .wait(response.text())
        .await?
        .map_err(|error| error.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "HTTP {status}: {}",
            text.chars().take(200).collect::<String>()
        ));
    }
    owner.check()?;
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "gse_config_compatibility_tests.rs"]
mod tests;
