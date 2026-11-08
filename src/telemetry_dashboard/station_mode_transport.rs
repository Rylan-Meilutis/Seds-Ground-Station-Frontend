//! Fixed-origin, non-redirecting mode requests using the original Settings owner.
use super::recording_download::ExportOwner;
use std::rc::Rc;

#[derive(Clone, Copy)]
enum Route {
    Capabilities,
    Prepare,
    Confirm,
}
impl Route {
    fn path(self) -> &'static str {
        match self {
            Self::Capabilities => "/api/station-modes",
            Self::Prepare => "/api/station-modes/prepare",
            Self::Confirm => "/api/station-modes/confirm",
        }
    }
}
pub(super) fn capabilities(
    owner: Result<Rc<ExportOwner>, String>,
) -> impl std::future::Future<Output = Result<serde_json::Value, String>> + 'static + use<> {
    captured(owner, Route::Capabilities, Ok(None))
}
pub(super) fn prepare(
    owner: Result<Rc<ExportOwner>, String>,
    binding: &super::station_mode_contract::Binding,
) -> impl std::future::Future<Output = Result<serde_json::Value, String>> + 'static + use<> {
    captured(
        owner,
        Route::Prepare,
        serde_json::to_string(binding)
            .map(Some)
            .map_err(|error| error.to_string()),
    )
}
pub(super) fn confirm(
    owner: Result<Rc<ExportOwner>, String>,
    confirmation: &super::station_mode_contract::Confirmation,
) -> impl std::future::Future<Output = Result<serde_json::Value, String>> + 'static + use<> {
    captured(
        owner,
        Route::Confirm,
        serde_json::to_string(confirmation)
            .map(Some)
            .map_err(|error| error.to_string()),
    )
}
fn captured(
    owner: Result<Rc<ExportOwner>, String>,
    route: Route,
    body: Result<Option<String>, String>,
) -> impl std::future::Future<Output = Result<serde_json::Value, String>> + 'static + use<> {
    // Capture and serialize before the caller's first yield. Never bind a draft
    // to whichever backend or account happens to exist after an await.
    let owner = owner.map_err(retired);
    async move { bounded(request(owner?, route, body?)).await }
}
fn retired(_: String) -> String {
    "Mode request cancelled: the station, account or connection changed. Reload Settings.".into()
}
fn url(owner: &ExportOwner, route: Route) -> Result<String, String> {
    owner.check().map_err(retired)?;
    if owner.base().is_empty() {
        #[cfg(not(target_arch = "wasm32"))]
        return Ok(format!("http://localhost:3000{}", route.path()));
        #[cfg(target_arch = "wasm32")]
        return Ok(route.path().to_owned());
    }
    Ok(format!(
        "{}{}",
        owner.base().trim_end_matches('/'),
        route.path()
    ))
}
#[cfg(not(target_arch = "wasm32"))]
async fn request(
    owner: Rc<ExportOwner>,
    route: Route,
    body: Option<String>,
) -> Result<serde_json::Value, String> {
    let url = url(&owner, route)?;
    let (connect, timeout) = super::native_http_timeouts(route.path());
    // The shared native client rejects redirects rather than forwarding an
    // authorization header or confirmation ticket to a redirect destination.
    let client = crate::auth::build_native_http_client(owner.skip_tls(), connect, timeout)?;
    let mut request = if matches!(route, Route::Capabilities) {
        client.get(url)
    } else {
        client.post(url)
    };
    if let Some(body) = body {
        request = request
            .header("Content-Type", "application/json")
            .body(body);
    }
    if let Some(token) = owner.token() {
        request = request.bearer_auth(token);
    }
    owner.check().map_err(retired)?;
    let response = owner
        .wait(request.send())
        .await
        .map_err(retired)?
        .map_err(|error| error.to_string())?;
    let status = response.status().as_u16();
    let text = owner
        .wait(response.text())
        .await
        .map_err(retired)?
        .map_err(|error| error.to_string())?;
    owner.check().map_err(retired)?;
    decode(status, &text)
}
#[cfg(target_arch = "wasm32")]
async fn request(
    owner: Rc<ExportOwner>,
    route: Route,
    body: Option<String>,
) -> Result<serde_json::Value, String> {
    let url = url(&owner, route)?;
    let controller =
        web_sys::AbortController::new().map_err(|_| "Mode request cancellation unavailable")?;
    let _cancel = super::HttpAbortOnDrop(controller.clone());
    let signal = controller.signal();
    let mut request = if matches!(route, Route::Capabilities) {
        gloo_net::http::Request::get(&url)
    } else {
        gloo_net::http::Request::post(&url)
    }
    .redirect(web_sys::RequestRedirect::Error)
    .abort_signal(Some(&signal));
    if let Some(token) = owner.token() {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => request
            .header("Content-Type", "application/json")
            .body(body),
        None => request.build(),
    }
    .map_err(|error| error.to_string())?;
    owner.check().map_err(retired)?;
    let response = owner
        .wait(request.send())
        .await
        .map_err(retired)?
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let text = owner
        .wait(response.text())
        .await
        .map_err(retired)?
        .map_err(|error| error.to_string())?;
    owner.check().map_err(retired)?;
    decode(status, &text)
}
fn decode(status: u16, text: &str) -> Result<serde_json::Value, String> {
    if !(200..300).contains(&status) {
        return Err(match server_reason(text) {
            Some(reason) => format!("{reason} (HTTP {status}; no mode change is confirmed.)"),
            None => format!("Station mode request returned HTTP {status}; no mode change is confirmed."),
        });
    }
    serde_json::from_str(text).map_err(|_| "Invalid station mode response.".into())
}
/// The server's refusal text, from `{"error": ...}` or a short plain body.
/// Bounded and stripped of control characters; it is display text only.
fn server_reason(text: &str) -> Option<String> {
    let raw = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| value["error"].as_str().map(str::to_owned))
        .or_else(|| (!text.trim_start().starts_with(['{', '[', '<'])).then(|| text.to_owned()))?;
    let clean: String = raw
        .chars()
        .filter(|c| !c.is_control())
        .take(400)
        .collect();
    let clean = clean.trim();
    (!clean.is_empty()).then(|| clean.to_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refusals_show_the_server_reason_and_never_claim_a_change() {
        let error = decode(409, r#"{"error":"Station is not idle: the T clock is running"}"#).unwrap_err();
        assert!(error.starts_with("Station is not idle: the T clock is running"));
        assert!(error.contains("HTTP 409") && error.contains("no mode change is confirmed"));
        assert!(decode(403, "Application administrator required").unwrap_err().starts_with("Application administrator required"));
        let plain = decode(502, "<html>bad gateway</html>").unwrap_err();
        assert!(plain.starts_with("Station mode request returned HTTP 502"));
        let hostile = decode(409, "{\"error\":\"a\\nb\\u0007c\"}").unwrap_err();
        assert!(hostile.starts_with("abc"));
        assert_eq!(decode(200, "{\"ok\":true}").unwrap()["ok"], true);
    }
}
async fn bounded<T>(
    future: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        future.await
    }
    #[cfg(target_arch = "wasm32")]
    {
        let future = std::pin::pin!(future);
        match futures_util::future::select(future,Box::pin(gloo_timers::future::TimeoutFuture::new(15_000))).await {
            futures_util::future::Either::Left((result,_))=>result,
            futures_util::future::Either::Right(_)=>Err("Mode request timed out. Its outcome is unconfirmed; refresh status, do not retry automatically.".into()),
        }
    }
}
