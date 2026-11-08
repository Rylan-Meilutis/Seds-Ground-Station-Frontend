//! GroundStation and RRLS markers for the map. Positions come from the
//! backend's antenna snapshot: fresh station GPS first, then the administrator's
//! saved fallback, then the last stale fix, each labelled with where it came from.
use super::{http_get_json, js_eval, layout::ThemeConfig};
use dioxus::prelude::*;
use serde::Deserialize;

const POLL_MS: u64 = 3_000;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(super) struct StationFix {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub accuracy_m: Option<f64>,
    #[serde(default)]
    pub age_ms: u64,
    #[serde(default)]
    pub stale: bool,
    #[serde(default)]
    pub source: String,
    /// Administrator-saved fallback rather than a live GPS solution.
    #[serde(default)]
    pub manual: bool,
}
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
struct Snapshot {
    ground_station: Option<StationFix>,
    rrls: Option<StationFix>,
}

impl StationFix {
    fn usable(&self) -> bool {
        self.latitude.is_finite()
            && self.longitude.is_finite()
            && (-90.0..=90.0).contains(&self.latitude)
            && (-180.0..=180.0).contains(&self.longitude)
    }
    fn live(&self) -> bool {
        !self.manual && !self.stale
    }
    /// Short provenance label: saved (manual or device location) vs live GPS,
    /// with accuracy and age where known.
    pub(super) fn provenance(&self) -> String {
        let accuracy = self
            .accuracy_m
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| format!(" ±{v:.0} m"))
            .unwrap_or_default();
        if self.manual {
            let kind = if self.source.contains("device location") {
                "device location"
            } else {
                "manual entry"
            };
            format!("saved, {kind}{accuracy}")
        } else if self.stale {
            format!("last GPS {} old (stale)", format_age(self.age_ms))
        } else {
            format!("live GPS{accuracy}, {} old", format_age(self.age_ms))
        }
    }
}
fn format_age(age_ms: u64) -> String {
    let seconds = age_ms / 1000;
    match seconds {
        0..=119 => format!("{seconds} s"),
        120..=7199 => format!("{} min", seconds / 60),
        _ => format!("{} h", seconds / 3600),
    }
}
fn stations_payload(snapshot: &Snapshot) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = [("gs", &snapshot.ground_station), ("rrls", &snapshot.rrls)]
        .into_iter()
        .filter_map(|(role, fix)| {
            let fix = fix.as_ref().filter(|f| f.usable())?;
            Some(serde_json::json!({"role": role, "lat": fix.latitude, "lon": fix.longitude, "live": fix.live()}))
        })
        .collect();
    serde_json::Value::Array(entries)
}
fn stations_js(snapshot: &Snapshot) -> String {
    let json = stations_payload(snapshot);
    format!(
        "(function(){{try{{window.__gs26_pending_stations={json};if(typeof window.setGroundMapStations===\"function\"){{window.setGroundMapStations(window.__gs26_pending_stations);}}}}catch(e){{console.warn(\"setGroundMapStations threw:\",e);}}}})();"
    )
}
async fn sleep_ms(ms: u64) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

/// Polls station positions while the map is mounted, places the markers and
/// shows a one-line legend per station.
#[component]
pub(super) fn MapStations(theme: ThemeConfig) -> Element {
    let mut snapshot = use_signal(Snapshot::default);
    use_future(move || async move {
        loop {
            if super::dashboard_page_visible()
                && let Ok(next) = http_get_json::<Snapshot>("/api/antenna/status").await
            {
                js_eval(&stations_js(&next));
                if *snapshot.peek() != next {
                    snapshot.set(next);
                }
            }
            sleep_ms(POLL_MS).await;
        }
    });
    let current = snapshot.read().clone();
    let rows: Vec<(&str, StationFix)> = [("GS", current.ground_station), ("RRLS", current.rrls)]
        .into_iter()
        .filter_map(|(name, fix)| fix.filter(|f| f.usable()).map(|f| (name, f)))
        .collect();
    if rows.is_empty() {
        return rsx! {};
    }
    rsx! {
        div {class:"gs26-map-stations",style:"display:flex; flex-wrap:wrap; gap:4px 12px; padding:0 10px; color:{theme.text_muted}; font-size:0.74rem; line-height:1.2; min-width:0;",
            for (name, fix) in rows {
                span {key:"{name}",style:"min-width:0; overflow-wrap:anywhere;","data-station":"{name}",
                    strong {style:"color:{theme.text_secondary};","{name}"}
                    {format!(" {:.5}, {:.5} · {}", fix.latitude, fix.longitude, fix.provenance())}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fix() -> StationFix {
        StationFix {
            latitude: 43.02,
            longitude: -78.82,
            ..Default::default()
        }
    }
    #[test]
    fn labels_distinguish_saved_manual_device_location_live_and_stale() {
        let manual = StationFix {
            manual: true,
            source: "Saved RRLS (manual)".into(),
            ..fix()
        };
        assert_eq!(manual.provenance(), "saved, manual entry");
        let device = StationFix {
            manual: true,
            accuracy_m: Some(12.4),
            source: "Saved GroundStation (device location)".into(),
            ..fix()
        };
        assert_eq!(device.provenance(), "saved, device location ±12 m");
        let live = StationFix {
            accuracy_m: Some(3.0),
            age_ms: 1_500,
            source: "RRLS GPS".into(),
            ..fix()
        };
        assert_eq!(live.provenance(), "live GPS ±3 m, 1 s old");
        let stale = StationFix {
            stale: true,
            age_ms: 600_000,
            ..fix()
        };
        assert_eq!(stale.provenance(), "last GPS 10 min old (stale)");
    }
    #[test]
    fn markers_carry_only_usable_positions_and_whether_they_are_live() {
        let snapshot = Snapshot {
            ground_station: Some(StationFix {
                manual: true,
                ..fix()
            }),
            rrls: Some(StationFix {
                latitude: f64::NAN,
                ..fix()
            }),
        };
        assert_eq!(
            stations_payload(&snapshot),
            serde_json::json!([{"role": "gs", "lat": 43.02, "lon": -78.82, "live": false}])
        );
        let live = Snapshot {
            ground_station: None,
            rrls: Some(fix()),
        };
        assert_eq!(
            stations_payload(&live),
            serde_json::json!([{"role": "rrls", "lat": 43.02, "lon": -78.82, "live": true}])
        );
        assert!(stations_js(&live).contains("window.setGroundMapStations("));
    }
}
