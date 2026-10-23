//! Local browser fixture: real calibration component, synthetic HTTP peer only.
#![allow(dead_code)]
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
#[path = "../src/telemetry_dashboard/layout.rs"]
mod layout;
#[path = "../src/telemetry_dashboard/paired_calibration.rs"]
mod paired_calibration;
mod auth {
    pub fn can_view_calibration() -> bool {
        true
    }
    pub fn can_edit_calibration() -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            !web_sys::window()
                .unwrap()
                .location()
                .search()
                .unwrap_or_default()
                .contains("readonly=1")
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            true
        }
    }
}
async fn http_get_json<T: for<'de> Deserialize<'de>>(path: &str) -> Result<T, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let response = gloo_net::http::Request::get(path)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.ok() {
            return Err(response.status().to_string());
        }
        response.json().await.map_err(|e| e.to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = path;
        Err("browser fixture only".into())
    }
}
async fn http_post_json<B: Serialize, T: for<'de> Deserialize<'de>>(
    path: &str,
    body: &B,
) -> Result<T, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let response = gloo_net::http::Request::post(path)
            .json(body)
            .map_err(|e| e.to_string())?
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.ok() {
            return Err(response.status().to_string());
        }
        response.json().await.map_err(|e| e.to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (path, body);
        Err("browser fixture only".into())
    }
}
#[component]
fn App() -> Element {
    rsx! {main {style:"padding:12px; font-family:system-ui;background:#060b12;min-height:100vh;box-sizing:border-box",
    paired_calibration::PairedCalibrationSettings {theme:layout::ThemeConfig::default()}}}
}
fn main() {
    dioxus::launch(App);
}
