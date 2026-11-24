//! Map overlays (flight-predict contract `MapOverlay`): predicted tracks,
//! markers and areas the backend sends for a flight plan, a loss-of-signal
//! prediction or search coverage. The UI never computes them; it hands the
//! backend's JSON to `window.setGroundMapOverlays(provider, overlays)`
//! (static/ground_map.js), which draws them on the 2D map and publishes
//! `window.__gs26_map_overlays` for the 3D views.
use serde::{Deserialize, Serialize};

/// Overlay providers this UI owns.
pub(crate) const PROVIDER_FLIGHT_PLAN: &str = "flight_plan";
pub(crate) const PROVIDER_LOS: &str = "los";

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct OverlayTrack {
    pub id: String,
    pub phase: String,
    pub label: String,
    pub style: String,
    /// `[lat_deg, lon_deg, alt_msl_m]`; altitude may be null.
    pub points: Vec<Vec<Option<f64>>>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct OverlayMarker {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_msl_m: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct OverlayArea {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub polygon: Vec<[f64; 2]>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct MapOverlay {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub predicted: bool,
    pub tracks: Vec<OverlayTrack>,
    pub markers: Vec<OverlayMarker>,
    pub areas: Vec<OverlayArea>,
}

impl MapOverlay {
    pub(crate) fn is_empty(&self) -> bool {
        self.tracks.is_empty() && self.markers.is_empty() && self.areas.is_empty()
    }
}

/// JS that replaces one provider's overlays. It is kept as pending when the
/// map script has not loaded yet; ground_map.js applies pending overlays when
/// it starts.
pub(crate) fn overlays_js(provider: &str, overlays: &[MapOverlay]) -> String {
    let provider = serde_json::to_string(provider).unwrap_or_else(|_| "\"\"".into());
    let json = serde_json::to_string(overlays)
        .unwrap_or_else(|_| "[]".into())
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    format!(
        "(function(){{try{{var p=window.__gs26_pending_map_overlays=window.__gs26_pending_map_overlays||{{}};var o={json};if(o.length){{p[{provider}]=o;}}else{{delete p[{provider}];}}if(typeof window.setGroundMapOverlays===\"function\"){{window.setGroundMapOverlays({provider},o);}}}}catch(e){{console.warn(\"setGroundMapOverlays threw:\",e);}}}})();"
    )
}

/// Replace one provider's overlays on the map (`[]` clears them).
pub(crate) fn set(provider: &str, overlays: &[MapOverlay]) {
    super::js_eval(&overlays_js(provider, overlays));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_json_round_trips_the_contract_shape() {
        let plan: serde_json::Value = serde_json::from_str(include_str!("../../tests/fixtures/flight-predict-mock/plan.json")).unwrap();
        let overlay: MapOverlay = serde_json::from_value(plan["overlay"].clone()).unwrap();
        assert_eq!(overlay.kind, "flight_plan");
        assert!(overlay.predicted);
        assert_eq!(overlay.tracks[0].phase, "boost");
        assert_eq!(overlay.tracks[0].points[0].len(), 3);
        assert_eq!(overlay.markers[0].kind, "landing");
        assert_eq!(overlay.areas[0].kind, "landing_ellipse");
        let back = serde_json::to_value(&overlay).unwrap();
        assert_eq!(back["tracks"][0]["points"][0], plan["overlay"]["tracks"][0]["points"][0]);
        // A null altitude survives.
        let sparse: MapOverlay = serde_json::from_value(serde_json::json!({"id": "x", "tracks": [{"points": [[1.0, 2.0, null]]}]})).unwrap();
        assert_eq!(sparse.tracks[0].points[0], vec![Some(1.0), Some(2.0), None]);
        assert!(!sparse.is_empty());
        assert!(MapOverlay::default().is_empty());
    }

    #[test]
    fn js_stores_pending_overlays_and_escapes_labels() {
        let overlay = MapOverlay { id: "a".into(), label: "</script>\u{2028}".into(), ..Default::default() };
        let js = overlays_js(PROVIDER_LOS, &[overlay]);
        assert!(js.contains("window.__gs26_pending_map_overlays"));
        assert!(js.contains("window.setGroundMapOverlays(\"los\",o)"));
        assert!(!js.contains("</script>"));
        assert!(!js.contains('\u{2028}'));
        let clear = overlays_js(PROVIDER_FLIGHT_PLAN, &[]);
        assert!(clear.contains("var o=[]"));
        assert!(clear.contains("delete p[\"flight_plan\"]"));
    }
}
