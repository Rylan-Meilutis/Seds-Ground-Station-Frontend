//! Original mounted settings view, using the existing transport/auth owners.
//! Retirement never rebinds an old draft to the new station or account.
use super::recording_download::{ExportOwner, saved_json};
use dioxus::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
use std::{rc::Rc, sync::atomic::Ordering};

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
        ExportOwner::capture_selected(move || alive.load(Ordering::Acquire) && selected())
    });
    Factory { original }
}
impl Factory {
    pub(super) fn current(self) -> bool {
        self.original
            .peek()
            .as_ref()
            .is_ok_and(|owner| owner.is_current())
    }
    fn capture(self) -> Result<Rc<ExportOwner>, String> {
        let owner = self.original.peek().clone()?;
        owner.check()?;
        Ok(owner)
    }
    pub(super) fn get<T: DeserializeOwned + 'static>(
        self,
        path: &str,
    ) -> impl std::future::Future<Output = Result<T, String>> + 'static {
        let owner = self.capture();
        let path = path.to_owned();
        async move { bounded(saved_json(&path, owner?)).await }
    }
    pub(super) fn post<B: Serialize, T: DeserializeOwned + 'static>(
        self,
        path: &str,
        body: &B,
    ) -> impl std::future::Future<Output = Result<T, String>> + 'static {
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
) -> impl std::future::Future<Output = Result<T, String>> + 'static {
    let path = path.to_owned();
    let body = serde_json::to_string(body).map_err(|e| e.to_string());
    async move { bounded(post_owned(&path, body?, owner?)).await }
}
#[cfg(not(target_arch = "wasm32"))]
async fn post_owned<T: DeserializeOwned>(
    path: &str,
    body: String,
    owner: Rc<ExportOwner>,
) -> Result<T, String> {
    owner.check()?;
    let base = if owner.base().is_empty() {
        "http://localhost:3000"
    } else {
        owner.base().trim_end_matches('/')
    };
    let (connect, timeout) = super::native_http_timeouts(path);
    let client = crate::auth::build_native_http_client(owner.skip_tls(), connect, timeout)?;
    let mut request = client
        .post(format!("{base}{path}"))
        .header("Content-Type", "application/json")
        .body(body);
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
    serde_json::from_str(&text).map_err(|e| e.to_string())
}
#[cfg(target_arch = "wasm32")]
async fn post_owned<T: DeserializeOwned>(
    path: &str,
    body: String,
    owner: Rc<ExportOwner>,
) -> Result<T, String> {
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
        web_sys::AbortController::new().map_err(|_| "Settings cancellation unavailable")?;
    let _cancel = super::HttpAbortOnDrop(controller.clone());
    let signal = controller.signal();
    let mut request = gloo_net::http::Request::post(&format!("{base}{path}"))
        .abort_signal(Some(&signal))
        .header("Content-Type", "application/json");
    if let Some(token) = owner.token() {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let request = request.body(body).map_err(|e| e.to_string())?;
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
    serde_json::from_str(&text).map_err(|e| e.to_string())
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
