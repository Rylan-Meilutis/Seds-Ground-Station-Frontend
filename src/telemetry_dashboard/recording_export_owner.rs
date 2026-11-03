//! A read-only export retains its original station, credential and UI lifetime.
use super::super::{UrlConfig, dashboard_alive, dashboard_gen, frontend_data_clear_epoch};
use crate::auth;
use dioxus::prelude::*;
use std::{
    future::Future,
    rc::Rc,
    sync::{Arc, atomic::Ordering},
};

// Reuse the published transport ownership algorithm on both targets; no epoch
// lives in this module. All identities come from their existing actual owners.
use super::super::native_session as session;

pub(crate) struct ExportOwner {
    session: session::Owner,
    identity: auth::RequestIdentity,
    base: String,
    ws_epoch: u64,
    data_epoch: u64,
    mount: u64,
    disconnects: u64,
    connected: bool,
    alive: Arc<std::sync::atomic::AtomicBool>,
    selection: Box<dyn Fn() -> bool>,
    skip_tls: bool,
}
impl ExportOwner {
    pub(crate) fn capture() -> Result<Rc<Self>, String> {
        Self::capture_selected(|| true)
    }
    pub(crate) fn capture_selected(
        selection: impl Fn() -> bool + 'static,
    ) -> Result<Rc<Self>, String> {
        let identity = auth::request_identity().ok_or("Authentication identity is unavailable")?;
        let base = UrlConfig::base_http();
        let ws_epoch = *super::super::WS_EPOCH.read();
        let (disconnects, connected) = {
            let metrics = super::super::FRONTEND_NETWORK_METRICS_STATE
                .lock()
                .map_err(|_| "Connection identity is unavailable")?;
            (metrics.ws_disconnects_total, metrics.ws_connected)
        };
        let owner = Rc::new(Self {
            session: session::Owner::new(
                ws_epoch,
                base.clone(),
                identity.token().map(str::to_owned),
            ),
            identity,
            base,
            ws_epoch,
            disconnects,
            connected,
            data_epoch: frontend_data_clear_epoch(),
            mount: dashboard_gen(),
            alive: dashboard_alive(),
            selection: Box::new(selection),
            skip_tls: UrlConfig::_skip_tls_verify(),
        });
        owner.check()?;
        Ok(owner)
    }
    pub(crate) fn is_current(&self) -> bool {
        if !self.alive.load(Ordering::Acquire) || !(self.selection)() {
            return false;
        }
        let Ok(metrics) = super::super::FRONTEND_NETWORK_METRICS_STATE.lock() else {
            return false;
        };
        let connection = self.disconnects == metrics.ws_disconnects_total
            && self.connected == metrics.ws_connected;
        drop(metrics);
        connection
            && Arc::ptr_eq(&self.alive, &dashboard_alive())
            && self.mount == dashboard_gen()
            && self.data_epoch == frontend_data_clear_epoch()
            && self.ws_epoch == *super::super::WS_EPOCH.read()
            && self.skip_tls == UrlConfig::_skip_tls_verify()
            && self.session.is_current(
                true,
                self.ws_epoch,
                &UrlConfig::base_http(),
                auth::current_token().as_deref(),
            )
            && auth::request_identity_is_current(&self.identity)
    }
    pub(crate) fn check(&self) -> Result<(), String> {
        if self.is_current() {
            Ok(())
        } else {
            Err("Export canceled: recording, station, login or connection changed. Select the recording again.".into())
        }
    }
    pub(crate) async fn wait<F: Future>(&self, future: F) -> Result<F::Output, String> {
        let mut future = std::pin::pin!(future);
        loop {
            #[cfg(not(target_arch = "wasm32"))]
            let tick = tokio::time::sleep(std::time::Duration::from_millis(100));
            #[cfg(target_arch = "wasm32")]
            let tick = gloo_timers::future::TimeoutFuture::new(100);
            let selected = futures_util::future::select(future.as_mut(), Box::pin(tick));
            let event = session::await_owned(selected, || self.is_current())
                .await
                .ok_or_else(|| "Export canceled: its original owner changed.".to_owned())?;
            match event {
                futures_util::future::Either::Left((result, _)) => return Ok(result),
                futures_util::future::Either::Right(_) => self.check()?,
            }
        }
    }
    pub(crate) fn base(&self) -> &str {
        &self.base
    }
    pub(crate) fn token(&self) -> Option<&str> {
        self.identity.token()
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn skip_tls(&self) -> bool {
        self.skip_tls
    }
}
