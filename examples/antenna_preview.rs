//! Deterministic preview of the real antenna component; no hardware commands.
//! Build with `dx build --example antenna_preview --platform web`.
use dioxus::prelude::*;
use serde::de::DeserializeOwned;
#[path="../src/telemetry_dashboard/antenna_math.rs"] mod antenna_math;
#[path="../src/telemetry_dashboard/antenna_aiming.rs"] mod antenna_aiming;
fn monotonic_now_ms()->f64 {
    #[cfg(target_arch="wasm32")] {js_sys::Date::now()}
    #[cfg(not(target_arch="wasm32"))] {static START:std::sync::OnceLock<std::time::Instant>=std::sync::OnceLock::new();START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()*1000.0}
}
async fn http_get_json<T:DeserializeOwned>(_:&str)->Result<T,String>{
    #[cfg(target_arch="wasm32")]
    { let response=gloo_net::http::Request::get("/__antenna_fixture").send().await.map_err(|e|e.to_string())?;if !response.ok(){return Err("Fixture disconnected".into())}response.json().await.map_err(|e|e.to_string()) }
    #[cfg(not(target_arch="wasm32"))]
    { Err("Serve the browser preview with a deterministic /__antenna_fixture".into()) }
}
#[component]
fn App()->Element{let mut open=use_signal(||true);rsx!{if *open.read(){antenna_aiming::AntennaAiming{on_exit:move |_|open.set(false)}}else{main {h1 {"Aiming closed"} button {onclick:move |_|open.set(true),"Reopen aiming"}}}}}
fn main(){dioxus::launch(App);}
