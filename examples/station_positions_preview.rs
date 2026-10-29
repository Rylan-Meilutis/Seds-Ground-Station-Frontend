//! Isolated administrator form fixture; serves only a fake positions API.
use dioxus::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
#[path = "../src/telemetry_dashboard/station_positions_admin.rs"]
mod station_positions_admin;
async fn http_get_json<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let response = gloo_net::http::Request::get(path)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.ok() {
            return Err(format!("HTTP {}", response.status()));
        }
        response.json().await.map_err(|e| e.to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = path;
        Err("Browser fixture only".into())
    }
}
async fn http_post_json<B: Serialize, T: DeserializeOwned>(
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
            return Err(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        }
        response.json().await.map_err(|e| e.to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (path, body);
        Err("Browser fixture only".into())
    }
}
fn app() -> Element {
    rsx! {style {{include_str!("../src/telemetry_dashboard/controls.css")}}style {{include_str!("../src/telemetry_dashboard/admin_console.css")}}main{class:"gs26-admin",station_positions_admin::StationPositionsAdmin{}}}
}
fn main() {
    dioxus::launch(app);
}
