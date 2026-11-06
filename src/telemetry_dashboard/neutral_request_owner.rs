//! Mounted neutral services keep auth/base/lifetime, never ordinary policy epoch.
//! A retired form is not rebound: explicit panel reopen creates a fresh draft.
use super::recording_download::ExportOwner;
use dioxus::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
use std::{future::Future, rc::Rc, sync::atomic::Ordering};

#[derive(Clone, Copy)]
pub(super) struct Factory { original: Signal<Result<Rc<ExportOwner>, String>> }
pub(super) fn use_factory() -> Factory {
    let alive = super::recording_download::use_export_lifetime();
    let original = use_signal(move || ExportOwner::capture_observer_selected(move || alive.load(Ordering::Acquire)));
    Factory { original }
}
impl Factory {
    pub(super) fn current(self) -> bool {
        // Existing shared UI clock observes transport retirement without adding
        // any new polling timer or changing media/source sampling intervals.
        let _ = *super::HEADER_CLOCK_TICK.read();
        self.original.try_peek().is_ok_and(|value| value.as_ref().is_ok_and(|owner| owner.is_current()))
    }
    fn capture(self) -> Result<Rc<ExportOwner>, String> {
        let original = self.original.try_peek().map_err(|_| "Neutral panel retired".to_owned())?;
        let owner = original.clone()?;
        owner.check()?;
        Ok(owner)
    }
    pub(super) fn get<T: DeserializeOwned + 'static>(self, path: &str)
        -> impl Future<Output=Result<T,String>> + 'static + use<T> {
        let request = super::http_get_json_captured(path, self.capture());
        self.resource(request)
    }
    pub(super) fn post<B: Serialize, T: DeserializeOwned + 'static>(self, path: &str, body: &B)
        -> impl Future<Output=Result<T,String>> + 'static + use<B,T> {
        let request = super::station_settings_request::post_captured_independent(path, body, self.capture());
        self.resource(request)
    }
    /// Capture at the original gesture, then guard EVERY poll of its whole
    /// multi-request operation, including error/cleanup/final Signal writes.
    pub(super) fn run<F: Future<Output=()> + 'static>(self, future: F)
        -> std::pin::Pin<Box<dyn Future<Output=()> + 'static>> {
        let original = self.capture();
        Box::pin(async move { if let Ok(owner) = original { let _ = owner.wait(future).await; } })
    }
    pub(super) fn spawn<F: Future<Output=()> + 'static>(self, future: F) -> dioxus::core::Task {
        let task = spawn(self.run(future));
        #[cfg(all(test, not(target_arch="wasm32")))]
        native_tests::observe_actual_spawn(task);
        task
    }
    /// Resources must not publish even a cancellation error into a retired
    /// mounted form. They remain pending until its explicit unmount/reopen.
    pub(super) fn resource<F: Future + 'static>(self, future: F)
        -> std::pin::Pin<Box<dyn Future<Output=F::Output> + 'static>>
    where F::Output: 'static {
        let original = self.capture();
        Box::pin(async move {
            if let Ok(owner) = original && let Ok(value) = owner.wait(future).await { return value; }
            std::future::pending().await
        })
    }
    pub(super) fn effect(self, apply: impl FnOnce()) {
        if self.capture().is_ok() { apply(); }
    }
}
pub(super) fn retired_notice() -> Element {
    rsx! {p {role:"status", "Station, connection or sign-in changed. This panel has retired. Reopen it to load a fresh draft; no old action will be retried."}}
}

#[cfg(all(test, not(target_arch="wasm32")))]
#[path="neutral_request_owner_native_tests.rs"]
pub(super) mod native_tests;
