//! Local browser fixture: real paired settings component, synthetic HTTP peer only.
#![allow(dead_code)]
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
#[path = "../src/telemetry_dashboard/layout.rs"]
mod layout;
#[path = "../src/telemetry_dashboard/paired_settings.rs"]
mod paired_settings;
mod auth {
    fn flag(name: &str) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::window()
                .unwrap()
                .location()
                .search()
                .unwrap_or_default()
                .contains(name)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = name;
            false
        }
    }
    pub fn can_view_data() -> bool {
        true
    }
    pub fn is_admin() -> bool {
        flag("admin=1") && !flag("readonly=1")
    }
    pub fn can_view_actions() -> bool {
        !flag("readonly=1") && !flag("calibration_only=1")
    }

    pub fn can_view_calibration() -> bool {
        !flag("mission_only=1")
    }
    pub fn can_edit_calibration() -> bool {
        !flag("readonly=1") && !flag("mission_only=1")
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
    paired_settings::PairedSettings {theme:layout::ThemeConfig::default()}}}
}
fn main() {
    dioxus::launch(App);
}
