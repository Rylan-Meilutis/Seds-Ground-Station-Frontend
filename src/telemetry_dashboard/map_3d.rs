//! The map tab's 2D/3D switch and its data feeds.
//!
//! - The flight track with the station's phase segmentation (`GET /api/flight_phases`) feeds
//!   both views: the 2D map draws it coloured by phase (`setGroundMapFlightTrack` in
//!   ground_map.js) and the 3D view (station_ui/rendering/three/map-3d.js) listens for it.
//! - In 3D the station's offline terrain (`GET /api/terrain/heightmap`) and coverage
//!   (`GET /api/terrain/status`) are fetched here, with the app's credentials, and handed to
//!   the view; imagery comes through the 2D map's own tile source. 2D stays the default and
//!   the fallback (no WebGL: the view says so and the 2D map stays visible).
//!
//! Every value is the station's; nothing here derives phases or terrain.
use super::{UrlConfig, http_get_json, js_eval, layout::ThemeConfig};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

const TRACK_POLL_MS: u64 = 2_000;
const VIEW_STORAGE_KEY: &str = "gs26_map_view_mode_v1";
/// The 3D DEM grid: 12 km square, 129 samples (~94 m spacing; 32k triangles).
const TERRAIN_EXTENT_M: u32 = 12_000;
const TERRAIN_GRID_SIZE: u32 = 129;

static MAP_VIEW_3D: GlobalSignal<bool> = Signal::global(load_view_mode);

/// One flight-phase segment, `[start_ms, end_ms)`; `end_ms` None while it lasts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhaseSegment {
    pub phase: String,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhaseEvent {
    pub kind: String,
    pub t_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude_m: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlightTrackPoint {
    pub timestamp_ms: i64,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub break_before: bool,
}

/// `GET /api/flight_phases` and the phase fields of `GET /api/recordings/{id}/replay/trajectory`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FlightTrack {
    #[serde(default)]
    pub points: Vec<FlightTrackPoint>,
    #[serde(default)]
    pub phases: Vec<PhaseSegment>,
    #[serde(default)]
    pub events: Vec<PhaseEvent>,
    #[serde(default)]
    pub source_sender_id: Option<String>,
    #[serde(default)]
    pub current_phase: Option<String>,
    #[serde(default)]
    pub sampled: bool,
}

impl FlightTrack {
    /// The phase at `t_ms` (a boundary belongs to the phase starting there); the views do
    /// the same lookup in JS, this checks the station's per-point phases in tests.
    #[cfg(test)]
    pub fn phase_at(&self, t_ms: i64) -> Option<&str> {
        let i = self.phases.partition_point(|s| s.start_ms <= t_ms);
        let segment = self.phases.get(i.checked_sub(1)?)?;
        let last = i == self.phases.len();
        match segment.end_ms {
            Some(end) if t_ms > end || (t_ms == end && !last) => None,
            _ => Some(segment.phase.as_str()),
        }
    }
    /// The scene origin for the 3D view: the first fix (the pad).
    pub fn origin(&self) -> Option<(f64, f64, f64)> {
        self.points.first().map(|p| (p.latitude, p.longitude, p.altitude_m))
    }
}

/// `GET /api/terrain/heightmap` (only the fields the app checks; the view decodes the rest).
#[cfg(test)]
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct HeightmapSummary {
    pub available: bool,
    #[serde(default)]
    pub size: Option<u32>,
    #[serde(default)]
    pub extent_m: Option<f64>,
    #[serde(default)]
    pub zoom: Option<u32>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub attribution: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct CoverageZoom {
    pub z: u32,
    pub present: u64,
    pub checked: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct CoverageCheck {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub radius_m: f64,
    #[serde(default)]
    pub dem: Vec<CoverageZoom>,
    #[serde(default)]
    pub imagery: Vec<CoverageZoom>,
    #[serde(default)]
    pub dem_complete: bool,
    #[serde(default)]
    pub imagery_complete: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct LayerStatus {
    pub available: bool,
    #[serde(default)]
    pub max_zoom: Option<u32>,
    #[serde(default)]
    pub attribution: Option<String>,
}

/// `GET /api/terrain/status`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct TerrainStatus {
    pub dem: LayerStatus,
    pub imagery: LayerStatus,
    pub check: CoverageCheck,
}

impl TerrainStatus {
    /// Offline coverage around the checked point, in percent (DEM, imagery).
    pub fn coverage_percent(&self) -> (u32, u32) {
        let pct = |zooms: &[CoverageZoom]| {
            let checked: u64 = zooms.iter().map(|z| z.checked).sum();
            if checked == 0 { 0 } else { (100 * zooms.iter().map(|z| z.present).sum::<u64>() / checked) as u32 }
        };
        (pct(&self.check.dem), pct(&self.check.imagery))
    }
}

fn load_view_mode() -> bool {
    super::persist::get_string(VIEW_STORAGE_KEY).as_deref() == Some("3d")
}

fn module_url() -> String {
    let path = "/assets/three/map-3d.js";
    if let Some(local) = crate::native_assets::station_asset_url(path) {
        return local;
    }
    format!("{}{path}", UrlConfig::base_http())
}

async fn sleep_ms(ms: u64) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

/// The script that hands the flight track to the 2D map (and, through its event, the 3D view).
pub(crate) fn track_js(track: &FlightTrack) -> String {
    let json = serde_json::to_string(track).unwrap_or_else(|_| "null".into());
    format!("(function(){{try{{window.__gs26_flight_track={json};if(typeof window.setGroundMapFlightTrack===\"function\"){{window.setGroundMapFlightTrack(window.__gs26_flight_track);}}}}catch(e){{console.warn(\"setGroundMapFlightTrack threw:\",e);}}}})();")
}

/// The script that switches the view (mounting the 3D module on first use).
pub(crate) fn view_js(three_d: bool, low_power: bool) -> String {
    let url = serde_json::to_string(&module_url()).unwrap_or_default();
    let mode = if three_d { "3d" } else { "2d" };
    format!(
        r#"(function(){{const el=document.getElementById('gs26-map-3d');if(!el)return;const map2d=document.getElementById('ground-map');
if('{mode}'==='2d'&&!window.__gs26Map3dModule){{el.style.display='none';el.dataset.mapView='2d';return;}}
window.__gs26Map3dModule ||= import({url});
window.__gs26Map3dModule.then(m=>{{const view=m.setMapViewMode(el,'{mode}',{{map2d,lowPower:{low_power}}});window.__gs26Map3dView=view;
 if(view&&window.__gs26_map3d_terrain)view.setTerrain(window.__gs26_map3d_terrain.heightmap,window.__gs26_map3d_terrain.status);
 if(view&&window.__gs26_map3d_origin)view.setOrigin(window.__gs26_map3d_origin);}}).catch(e=>{{el.dataset.mapView='3d-unavailable';el.textContent='The 3D map could not load from the station. The 2D map stays available.';if(map2d)map2d.style.visibility='';console.warn('map-3d',e);}});}})();"#
    )
}

/// Frees 3D views whose map left the page (after the DOM update, so a remounted map keeps its view).
const DISPOSE_JS: &str = "(function(){setTimeout(function(){if(window.__gs26Map3dModule){window.__gs26Map3dModule.then(function(m){m.disposeDetached();if(window.__gs26Map3dView&&window.__gs26Map3dView.disposed)window.__gs26Map3dView=null;});}},0);})();";

/// The script that hands the terrain (and the fallback origin) to the view.
pub(crate) fn terrain_js(heightmap: &serde_json::Value, status: &serde_json::Value, origin: (f64, f64)) -> String {
    format!(
        "(function(){{window.__gs26_map3d_terrain={{heightmap:{heightmap},status:{status}}};window.__gs26_map3d_origin=[{},{},null];const v=window.__gs26Map3dView;if(v){{v.setOrigin(window.__gs26_map3d_origin);v.setTerrain(window.__gs26_map3d_terrain.heightmap,window.__gs26_map3d_terrain.status);}}}})();",
        origin.0, origin.1
    )
}

fn low_power_device() -> bool {
    cfg!(any(target_os = "ios", target_os = "android"))
}

/// Polls the live flight track while the map is mounted and hands it to both views; shows the
/// 2D/3D switch and, in 3D, the terrain coverage line.
#[component]
pub(super) fn Map3dPanel(theme: ThemeConfig) -> Element {
    let mut track = use_signal(FlightTrack::default);
    let mut status = use_signal(|| None::<TerrainStatus>);
    use_future(move || async move {
        loop {
            if super::dashboard_page_visible()
                && let Ok(next) = http_get_json::<FlightTrack>("/api/flight_phases").await
                && *track.peek() != next
            {
                js_eval(&track_js(&next));
                track.set(next);
            }
            sleep_ms(TRACK_POLL_MS).await;
        }
    });
    // Terrain for the 3D view: fetched when 3D opens and when the pad moves.
    let origin_key = use_memo(move || {
        let three_d = *MAP_VIEW_3D.read();
        let origin = track.read().origin().map(|(lat, lon, _)| ((lat * 100.0).round() as i64, (lon * 100.0).round() as i64));
        (three_d, origin)
    });
    use_effect(move || {
        let (three_d, _) = *origin_key.read();
        let origin = track.peek().origin();
        spawn(async move {
            if !three_d {
                return;
            }
            let Ok(raw_status) = http_get_json::<serde_json::Value>("/api/terrain/status").await else {
                js_eval(&terrain_js(&serde_json::json!({"available": false}), &serde_json::Value::Null, origin.map(|o| (o.0, o.1)).unwrap_or((0.0, 0.0))));
                return;
            };
            let parsed: Option<TerrainStatus> = serde_json::from_value(raw_status.clone()).ok();
            let (lat, lon) = origin.map(|o| (o.0, o.1)).or_else(|| parsed.as_ref().map(|s| (s.check.lat_deg, s.check.lon_deg))).unwrap_or((0.0, 0.0));
            status.set(parsed);
            let path = format!("/api/terrain/heightmap?lat={lat:.6}&lon={lon:.6}&extent_m={TERRAIN_EXTENT_M}&size={TERRAIN_GRID_SIZE}");
            let heightmap = http_get_json::<serde_json::Value>(&path).await.unwrap_or_else(|_| serde_json::json!({"available": false}));
            js_eval(&terrain_js(&heightmap, &raw_status, (lat, lon)));
        });
    });
    use_effect(move || {
        let three_d = *MAP_VIEW_3D.read();
        js_eval(&view_js(three_d, low_power_device()));
    });
    // Leaving the map (or switching to fullscreen, which remounts it) frees the 3D view's GPU context.
    use_drop(|| js_eval(DISPOSE_JS));
    let three_d = *MAP_VIEW_3D.read();
    let button = |active: bool| {
        format!(
            "padding:3px 10px; border:1px solid {}; background:{}; color:{}; font-size:0.78rem; cursor:pointer; min-height:28px;",
            theme.button_border,
            if active { theme.button_text.clone() } else { theme.button_background.clone() },
            if active { theme.button_background.clone() } else { theme.button_text.clone() }
        )
    };
    let coverage = status.read().as_ref().map(|s| {
        let (dem, imagery) = s.coverage_percent();
        format!("Offline within {:.0} km: DEM {dem} %, imagery {imagery} %", s.check.radius_m / 1000.0)
    });
    rsx! {
        div { class: "gs26-map-view-switch", style: "display:flex; align-items:center; flex-wrap:wrap; gap:6px 10px; padding:0 10px; color:{theme.text_muted}; font-size:0.74rem; min-width:0;",
            div { role: "group", "aria-label": "Map view", style: "display:inline-flex; border-radius:999px; overflow:hidden;",
                button { style: "{button(!three_d)} border-radius:999px 0 0 999px;", "aria-pressed": "{!three_d}", "data-map-view": "2d",
                    onclick: move |_| set_view_mode(false), "2D" }
                button { style: "{button(three_d)} border-radius:0 999px 999px 0;", "aria-pressed": "{three_d}", "data-map-view": "3d",
                    onclick: move |_| set_view_mode(true), "3D" }
            }
            if three_d && let Some(text) = coverage { span { "{text}" } }
        }
    }
}

fn set_view_mode(three_d: bool) {
    *MAP_VIEW_3D.write() = three_d;
    super::persist::set_string(VIEW_STORAGE_KEY, if three_d { "3d" } else { "2d" });
}

/// The 3D view's container, layered over the 2D map inside the map canvas.
pub(super) fn map3d_layer() -> Element {
    rsx! {
        div { id: "gs26-map-3d", style: "display:none; position:absolute; inset:0; border-radius:12px; overflow:hidden; z-index:2;" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend_contract::decode;

    #[test]
    fn live_and_recorded_tracks_decode_with_their_phases() {
        let live: FlightTrack = decode("flight-phases/live");
        assert_eq!(live.phases.iter().map(|p| p.phase.as_str()).collect::<Vec<_>>(), ["pad", "boost", "coast", "drogue", "landed"]);
        assert_eq!(live.phases.last().unwrap().end_ms, None);
        assert_eq!(live.current_phase.as_deref(), Some("landed"));
        for point in &live.points {
            assert_eq!(point.phase.as_deref(), live.phase_at(point.timestamp_ms), "t={}", point.timestamp_ms);
        }
        let recorded: FlightTrack = decode("replay-trajectory/flight");
        assert!(recorded.points.len() > 100);
        for point in &recorded.points {
            assert_eq!(point.phase.as_deref(), recorded.phase_at(point.timestamp_ms));
        }
        let apogee = recorded.events.iter().find(|e| e.kind == "apogee").unwrap();
        assert!(apogee.altitude_m.unwrap() > 3_000.0);
        // Boundaries belong to the phase that starts there.
        let boost = &recorded.phases[1];
        assert_eq!(recorded.phase_at(boost.start_ms), Some("boost"));
        assert_eq!(recorded.phase_at(boost.start_ms - 1), Some("pad"));
        assert_eq!(recorded.origin().map(|o| o.0.round()), Some(32.0));
    }

    #[test]
    fn terrain_routes_decode_with_and_without_a_dem() {
        let map: HeightmapSummary = decode("terrain/heightmap");
        assert!(map.available && map.size == Some(17) && map.attribution.unwrap().contains("Terrain"));
        let none: HeightmapSummary = decode("terrain/heightmap-unavailable");
        assert!(!none.available && none.reason.is_some());
        let status: TerrainStatus = decode("terrain/status-empty");
        assert!(!status.dem.available && !status.imagery.available);
        assert_eq!(status.coverage_percent(), (0, 0));
        assert!(!status.check.dem_complete);
    }

    #[test]
    fn coverage_counts_present_tiles() {
        let status = TerrainStatus {
            check: CoverageCheck {
                dem: vec![CoverageZoom { z: 10, present: 4, checked: 4 }, CoverageZoom { z: 11, present: 2, checked: 4 }],
                imagery: vec![CoverageZoom { z: 16, present: 9, checked: 9 }],
                ..CoverageCheck::default()
            },
            ..TerrainStatus::default()
        };
        assert_eq!(status.coverage_percent(), (75, 100));
    }

    #[test]
    fn scripts_hand_data_to_the_views() {
        let track = FlightTrack { phases: vec![PhaseSegment { phase: "boost".into(), start_ms: 0, end_ms: None }], ..FlightTrack::default() };
        let js = track_js(&track);
        assert!(js.contains("window.setGroundMapFlightTrack(") && js.contains("\"phase\":\"boost\""));
        let js = view_js(true, false);
        assert!(js.contains("setMapViewMode(el,'3d'") && js.contains("map-3d.js"));
        assert!(view_js(false, true).contains("'2d'"));
        assert!(view_js(false, true).contains("'2d'==='2d'&&!window.__gs26Map3dModule"), "2D never loads the 3D module");
        assert!(DISPOSE_JS.contains("disposeDetached()"));
        let js = terrain_js(&serde_json::json!({"available": false}), &serde_json::Value::Null, (31.758, -102.046));
        assert!(js.contains("[31.758,-102.046,null]") && js.contains("setTerrain"));
    }
}
