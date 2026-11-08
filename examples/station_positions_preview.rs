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
// Standalone preview only: this has one document and a synthetic local API,
// never production authentication, mode capabilities, epochs or board authority.
// Actual mode/transport ownership is qualified using the main application.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
mod gps {
    pub struct DeviceFix {
        pub latitude: f64,
        pub longitude: f64,
        pub altitude_m: Option<f64>,
        pub accuracy_m: Option<f64>,
    }
    pub async fn one_shot_location(_: std::time::Duration) -> Result<DeviceFix, String> {
        Err("Browser fixture only".into())
    }
}
mod station_mode_observer {
    pub fn view_key() -> &'static str { "isolated-positions-preview" }
}
mod station_settings_request {
    use dioxus::prelude::*;
    use serde::{Serialize, de::DeserializeOwned};
    #[derive(Clone, Copy)]
    pub struct Factory { alive: Signal<bool> }
    pub fn use_factory() -> Factory {
        let mut alive=use_signal(||true);
        use_drop(move || { if let Ok(mut value)=alive.try_write() { *value=false; } });
        Factory {alive}
    }
    impl Factory {
        pub fn current(self)->bool { self.alive.try_peek().is_ok_and(|value|*value) }
        pub fn post<B:Serialize,T:DeserializeOwned+'static>(self,path:&str,body:&B)
            ->impl std::future::Future<Output=Result<T,String>>+'static+use<B,T> {
            let path=path.to_owned();
            let body=serde_json::to_value(body).map_err(|e|e.to_string());
            let original_current=self.current();
            async move {
                if !original_current||!self.current(){return Err("Preview view retired".into());}
                let result=super::http_post_json(&path,&body?).await;
                if !self.current(){return Err("Preview view retired".into());}
                result
            }
        }
    }
}
fn app() -> Element {
    rsx! {style {{include_str!("../src/telemetry_dashboard/controls.css")}}style {{include_str!("../src/telemetry_dashboard/admin_console.css")}}main{class:"gs26-admin",station_positions_admin::StationPositionsAdmin{}}}
}
fn main() {
    dioxus::launch(app);
}
