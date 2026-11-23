//! Original mounted settings view, using the existing transport/auth owners.
//! Retirement never rebinds an old draft to the new station or account.
use super::recording_download::{ExportOwner, saved_json};
use dioxus::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
use std::{rc::Rc, sync::atomic::Ordering};

/// HTTP method of a settings write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verb {
    Post,
    Put,
    Patch,
    Delete,
}
impl Verb {
    /// PUT, PATCH and DELETE may answer with an empty body (read as `null`).
    fn empty_ok(self) -> bool {
        !matches!(self, Verb::Post)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Factory {
    original: Signal<Result<Rc<ExportOwner>, String>>,
}
pub(super) fn use_factory() -> Factory {
    use_factory_selected(|| true)
}
pub(super) fn use_factory_selected(selected: impl Fn() -> bool + 'static) -> Factory {
    let alive = super::recording_download::use_export_lifetime();
    let original = use_signal(move || {
        ExportOwner::capture_ordinary_selected(move || alive.load(Ordering::Acquire) && selected())
    });
    Factory { original }
}
impl Factory {
    pub(super) fn current(self) -> bool {
        self.original.try_peek().is_ok_and(|original| {
            original.as_ref().is_ok_and(|owner| owner.is_current())
        })
    }
    pub(super) fn capture(self) -> Result<Rc<ExportOwner>, String> {
        let original = self.original.try_peek().map_err(|_| "Original settings view retired".to_owned())?;
        let owner = original.clone()?;
        owner.check()?;
        Ok(owner)
    }
    pub(super) fn get<T: DeserializeOwned + 'static>(
        self,
        path: &str,
    ) -> impl std::future::Future<Output = Result<T, String>> + 'static + use<T> {
        let owner = self.capture();
        let path = path.to_owned();
        async move { bounded(saved_json(&path, owner?)).await }
    }
    pub(super) fn post<B: Serialize, T: DeserializeOwned + 'static>(
        self,
        path: &str,
        body: &B,
    ) -> impl std::future::Future<Output = Result<T, String>> + 'static + use<B,T> {
        // Capture/serialize synchronously before the returned future can yield.
        let owner = self.capture();
        post_captured(path, body, owner)
    }
}
/// Original request owner captured by the caller before its first yield.
/// Reuses the same bounded transport as mounted settings without rebinding it.
pub(super) fn post_captured<B: Serialize, T: DeserializeOwned + 'static>(
    path: &str,
    body: &B,
    owner: Result<Rc<ExportOwner>, String>,
) -> impl std::future::Future<Output = Result<T, String>> + 'static + use<B,T> {
    let path = path.to_owned();
    let body = serde_json::to_string(body).map_err(|e| e.to_string());
    async move { bounded(post_owned(&path, body?, owner?)).await }
}
/// Independent auth/admin/history/media requests never acquire ordinary authority.
/// The backend route classifier still decides whether an epoch is mandatory.
pub(super) fn post_captured_independent<B: Serialize, T: DeserializeOwned + 'static>(
    path: &str, body: &B, owner: Result<Rc<ExportOwner>, String>,
) -> impl std::future::Future<Output = Result<T, String>> + 'static + use<B,T> {
    let path = path.to_owned();
    let body = serde_json::to_string(body).map_err(|e| e.to_string());
    async move { bounded(post_owned_class(&path, body?, owner?, false)).await }
}
/// A JSON `PUT` (station settings) with the caller's captured owner. An
/// empty success reply reads as `null`.
pub(super) fn put_captured<B: Serialize, T: DeserializeOwned + 'static>(
    path: &str,
    body: &B,
    owner: Result<Rc<ExportOwner>, String>,
) -> impl std::future::Future<Output = Result<T, String>> + 'static + use<B,T> {
    let path = path.to_owned();
    let body = serde_json::to_string(body).map_err(|e| e.to_string());
    async move { bounded(send_verb(Verb::Put, &path, JSON, body?.into_bytes(), owner?)).await }
}
const JSON: &str = "application/json";
/// A JSON `PATCH` with the caller's captured owner.
pub(super) fn patch_captured<B: Serialize, T: DeserializeOwned + 'static>(
    path: &str,
    body: &B,
    owner: Result<Rc<ExportOwner>, String>,
) -> impl std::future::Future<Output = Result<T, String>> + 'static + use<B,T> {
    let path = path.to_owned();
    let body = serde_json::to_string(body).map_err(|e| e.to_string());
    async move { bounded(send_verb(Verb::Patch, &path, JSON, body?.into_bytes(), owner?)).await }
}
/// A `DELETE` (no body) with the caller's captured owner.
pub(super) fn delete_captured<T: DeserializeOwned + 'static>(
    path: &str,
    owner: Result<Rc<ExportOwner>, String>,
) -> impl std::future::Future<Output = Result<T, String>> + 'static + use<T> {
    let path = path.to_owned();
    async move { bounded(send_verb(Verb::Delete, &path, JSON, Vec::new(), owner?)).await }
}
/// A raw-bytes `POST` (`application/octet-stream`), e.g. an uploaded file.
pub(super) fn post_bytes_captured<T: DeserializeOwned + 'static>(
    path: &str,
    bytes: Vec<u8>,
    owner: Result<Rc<ExportOwner>, String>,
) -> impl std::future::Future<Output = Result<T, String>> + 'static + use<T> {
    let path = path.to_owned();
    async move { bounded(send_verb(Verb::Post, &path, "application/octet-stream", bytes, owner?)).await }
}
async fn send_verb<T: DeserializeOwned>(
    verb: Verb,
    path: &str,
    content_type: &'static str,
    body: Vec<u8>,
    owner: Rc<ExportOwner>,
) -> Result<T, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        send_owned_class(verb, path, content_type, body, owner, true).await
    }
    #[cfg(target_arch = "wasm32")]
    {
        send_owned_policy(verb, path, content_type, body, owner, true, true).await
    }
}
/// Critical commands reject redirects as well as retaining their gesture owner.
pub(super) fn post_captured_no_redirect<B: Serialize, T: DeserializeOwned + 'static>(
    path: &str,
    body: &B,
    owner: Result<Rc<ExportOwner>, String>,
) -> impl std::future::Future<Output = Result<T, String>> + 'static + use<B,T> {
    let path = path.to_owned();
    let body = serde_json::to_string(body).map_err(|e| e.to_string());
    async move {
        #[cfg(not(target_arch = "wasm32"))]
        {
            bounded(post_owned(&path, body?, owner?)).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            bounded(post_owned_policy(&path, body?, owner?, true, true)).await
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
async fn post_owned<T: DeserializeOwned>(
    path: &str,
    body: String,
    owner: Rc<ExportOwner>,
) -> Result<T, String> {
    post_owned_class(path, body, owner, true).await
}
#[cfg(not(target_arch = "wasm32"))]
async fn post_owned_class<T: DeserializeOwned>(
    path: &str, body: String, owner: Rc<ExportOwner>, require_ordinary: bool,
) -> Result<T, String> {
    send_owned_class(Verb::Post, path, JSON, body.into_bytes(), owner, require_ordinary).await
}
#[cfg(not(target_arch = "wasm32"))]
async fn send_owned_class<T: DeserializeOwned>(
    verb: Verb, path: &str, content_type: &str, body: Vec<u8>, owner: Rc<ExportOwner>, require_ordinary: bool,
) -> Result<T, String> {
    if require_ordinary { owner.check_ordinary()?; } else { owner.check()?; }
    let base = if owner.base().is_empty() {
        "http://localhost:3000"
    } else {
        owner.base().trim_end_matches('/')
    };
    let (connect, timeout) = super::native_http_timeouts(path);
    let client = crate::auth::build_native_http_client(owner.skip_tls(), connect, timeout)?;
    let method = match verb {
        Verb::Post => reqwest::Method::POST,
        Verb::Put => reqwest::Method::PUT,
        Verb::Patch => reqwest::Method::PATCH,
        Verb::Delete => reqwest::Method::DELETE,
    };
    let mut request = client
        .request(method, format!("{base}{path}"))
        .header("Content-Type", content_type)
        .body(body);
    if let Some(epoch) = owner.policy_epoch() {
        request = request.header("X-Station-Policy-Epoch", epoch);
    }
    if require_ordinary && let Some(origin) = owner.control_origin() {
        request = request.header("X-Station-Control-Origin", origin);
    }
    if let Some(token) = owner.token() {
        request = request.bearer_auth(token);
    }
    owner.check()?;
    let response = owner
        .wait(request.send())
        .await?
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let text = owner
        .wait(response.text())
        .await?
        .map_err(|e| e.to_string())?;
    owner.check()?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "HTTP {status}: {}",
            text.chars().take(200).collect::<String>()
        ));
    }
    let text = if verb.empty_ok() && text.trim().is_empty() { "null" } else { text.as_str() };
    serde_json::from_str(text).map_err(|e| e.to_string())
}
#[cfg(target_arch = "wasm32")]
async fn post_owned<T: DeserializeOwned>(
    path: &str,
    body: String,
    owner: Rc<ExportOwner>,
) -> Result<T, String> {
    post_owned_class(path, body, owner, true).await
}
#[cfg(target_arch = "wasm32")]
async fn post_owned_class<T: DeserializeOwned>(
    path: &str, body: String, owner: Rc<ExportOwner>, require_ordinary: bool,
) -> Result<T, String> {
    post_owned_policy(path, body, owner, true, require_ordinary).await
}
#[cfg(target_arch = "wasm32")]
async fn post_owned_policy<T: DeserializeOwned>(
    path: &str,
    body: String,
    owner: Rc<ExportOwner>,
    reject_redirects: bool,
    require_ordinary: bool,
) -> Result<T, String> {
    send_owned_policy(Verb::Post, path, JSON, body.into_bytes(), owner, reject_redirects, require_ordinary).await
}
#[cfg(target_arch = "wasm32")]
async fn send_owned_policy<T: DeserializeOwned>(
    verb: Verb,
    path: &str,
    content_type: &str,
    body: Vec<u8>,
    owner: Rc<ExportOwner>,
    reject_redirects: bool,
    require_ordinary: bool,
) -> Result<T, String> {
    if require_ordinary { owner.check_ordinary()?; } else { owner.check()?; }
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
        web_sys::AbortController::new().map_err(|_| "Settings cancellation unavailable")?;
    let _cancel = super::HttpAbortOnDrop(controller.clone());
    let signal = controller.signal();
    let url = format!("{base}{path}");
    let request = match verb {
        Verb::Post => gloo_net::http::Request::post(&url),
        Verb::Put => gloo_net::http::Request::put(&url),
        Verb::Patch => gloo_net::http::Request::patch(&url),
        Verb::Delete => gloo_net::http::Request::delete(&url),
    };
    let mut request = request
        .abort_signal(Some(&signal))
        .header("Content-Type", content_type);
    if reject_redirects {
        request = request.redirect(web_sys::RequestRedirect::Error);
    }
    if let Some(epoch) = owner.policy_epoch() {
        request = request.header("X-Station-Policy-Epoch", epoch);
    }
    if require_ordinary && let Some(origin) = owner.control_origin() {
        request = request.header("X-Station-Control-Origin", origin);
    }
    if let Some(token) = owner.token() {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let request = if verb == Verb::Delete && body.is_empty() {
        request.build().map_err(|e| e.to_string())?
    } else {
        request.body(js_sys::Uint8Array::from(body.as_slice())).map_err(|e| e.to_string())?
    };
    owner.check()?;
    let response = owner
        .wait(request.send())
        .await?
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let text = owner
        .wait(response.text())
        .await?
        .map_err(|e| e.to_string())?;
    owner.check()?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "HTTP {status}: {}",
            text.chars().take(200).collect::<String>()
        ));
    }
    let text = if verb.empty_ok() && text.trim().is_empty() { "null" } else { text.as_str() };
    serde_json::from_str(text).map_err(|e| e.to_string())
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
        match futures_util::future::select(
            future,
            Box::pin(gloo_timers::future::TimeoutFuture::new(15_000)),
        )
        .await
        {
            futures_util::future::Either::Left((result, _)) => result,
            futures_util::future::Either::Right(_) => Err("Settings request timed out".into()),
        }
    }
}

#[cfg(test)]
#[path = "station_settings_request_tests.rs"]
mod tests;
