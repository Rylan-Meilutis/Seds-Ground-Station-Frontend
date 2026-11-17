//! Station positions as `/api/antenna/status` reports them: fresh station GPS
//! first, then the administrator's saved fallback, then the last stale fix.
//! Shared by the map markers and the station location editor so both always
//! show the same fix.
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(crate) struct StationFix {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub altitude_m: Option<f64>,
    /// `mean_sea_level`, `ellipsoid` or `unknown`.
    #[serde(default)]
    pub altitude_reference: Option<String>,
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
/// The station half of `/api/antenna/status`. The backend already maps its
/// local and peer GPS to these roles, on a GroundStation and on an RRLS alike.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(crate) struct Snapshot {
    pub ground_station: Option<StationFix>,
    pub rrls: Option<StationFix>,
}
pub(crate) const STATUS_PATH: &str = "/api/antenna/status";

impl StationFix {
    pub(crate) fn usable(&self) -> bool {
        self.latitude.is_finite()
            && self.longitude.is_finite()
            && (-90.0..=90.0).contains(&self.latitude)
            && (-180.0..=180.0).contains(&self.longitude)
    }
    /// A fresh GPS solution, not a saved fallback or an old fix.
    pub(crate) fn live(&self) -> bool {
        !self.manual && !self.stale
    }
    /// Short provenance label: saved (manual or device location) vs live GPS,
    /// with accuracy and age where known.
    pub(crate) fn provenance(&self) -> String {
        let accuracy = self
            .accuracy_m
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(|v| format!(" ±{v:.0} m"))
            .unwrap_or_default();
        if self.manual {
            let kind = if self.source.contains("device location") {
                "device location"
            } else if self.source.contains("station GPS") {
                "station GPS"
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
pub(crate) fn format_age(age_ms: u64) -> String {
    let seconds = age_ms / 1000;
    match seconds {
        0..=119 => format!("{seconds} s"),
        120..=7199 => format!("{} min", seconds / 60),
        _ => format!("{} h", seconds / 3600),
    }
}
