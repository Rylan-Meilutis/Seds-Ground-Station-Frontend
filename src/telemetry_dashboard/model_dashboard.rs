use super::{http_get_json, layout::ThemeConfig, types::FlightState, vehicle_tab::VehicleTab};
use dioxus::prelude::*;
use serde::Deserialize;
#[derive(Clone, Default, Deserialize)]
struct Snapshot {
    phase: String,
    t_clock: Option<String>,
    stats: Vec<Stat>,
}
#[derive(Clone, Deserialize)]
struct Stat {
    label: String,
    value: Option<f64>,
    unit: String,
    precision: usize,
}
#[component]
pub(super) fn ModelDashboard(
    theme: ThemeConfig,
    flight_state: Signal<FlightState>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    rocket_altitude_m: Signal<Option<f64>>,
) -> Element {
    let mut snapshot = use_signal(Snapshot::default);
    let mut page = use_signal(|| 0usize);
    let mut available = use_signal(|| false);
    use_future(move || async move {
        let mut ticks = 0usize;
        loop {
            match http_get_json::<Snapshot>("/api/dashboard_status").await {
                Ok(next) => {
                    snapshot.set(next);
                    available.set(true);
                }
                Err(_) => {
                    snapshot.set(Snapshot::default());
                    available.set(false);
                }
            }
            ticks = ticks.wrapping_add(1);
            if ticks % 10 == 0 {
                page.set(ticks / 10);
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(500).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });
    let data = snapshot.read().clone();
    let start = (*page.read() % data.stats.len().div_ceil(3).max(1)) * 3;
    rsx! {div {style:"height:100%;display:flex;flex-direction:column;min-height:0;",
        div {style:"flex:1;min-height:0;overflow:auto;",VehicleTab {theme:theme.clone(),flight_state,rocket_gps,rocket_altitude_m}}
        div {style:"flex:0 0 auto;display:flex;gap:24px;flex-wrap:wrap;padding:14px 18px;background:#101923;color:#e5edf4;font-variant-numeric:tabular-nums;",
            {card("Flight state",if *available.read(){data.phase.clone()}else{"Unavailable".into()})}
            {card("T clock",data.t_clock.clone().unwrap_or_else(||"—".into()))}
            for stat in data.stats.iter().skip(start).take(3) {
                {card(&stat.label,stat.value.filter(|v|v.is_finite()).map(|v|format!("{:.*} {}",stat.precision.min(6),v,stat.unit)).unwrap_or_else(||"—".into()))}
            }
        }
    }}
}
fn card(label: &str, value: String) -> Element {
    rsx! {div {div {style:"font-size:10px;text-transform:uppercase;letter-spacing:.12em;color:#9aaebb;","{label}"}div {style:"margin-top:4px;","{value}"}}}
}
