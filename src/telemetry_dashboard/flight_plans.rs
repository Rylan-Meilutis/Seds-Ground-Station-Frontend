//! Flight plans (GroundStation26 flight-predict contract): manual flight and
//! burn simulations the station runs and stores. The UI only collects the
//! request, shows what the station computed and hands the plan's MapOverlay
//! and replay bundle to the map and the offline replay workspace. Nothing
//! here computes physics or names a vehicle.
use super::apogee_planner::ManualWeather;
use super::map_overlays::MapOverlay;
use serde::{Deserialize, Serialize};

pub(crate) const LIST_PATH: &str = "/api/flight_plans";
pub(crate) const RUN_PATH: &str = "/api/flight_plans/run";

/// `GET /api/flight_plans/{id}` (also `DELETE`).
pub(crate) fn plan_path(id: &str) -> String {
    format!("/api/flight_plans/{}", encode_id(id))
}
pub(crate) fn csv_path(id: &str) -> String {
    format!("/api/flight_plans/{}/csv", encode_id(id))
}
pub(crate) fn json_path(id: &str) -> String {
    format!("/api/flight_plans/{}/json", encode_id(id))
}
pub(crate) fn bundle_path(id: &str) -> String {
    format!("/api/flight_plans/{}/bundle", encode_id(id))
}

/// One URL path segment; plan ids are `fp-<ms>-<hex>` but are never trusted.
pub(crate) fn encode_id(id: &str) -> String {
    id.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// A safe download name for `<id>.<ext>`.
pub(crate) fn file_name(id: &str, ext: &str) -> String {
    let stem: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(80).collect();
    format!("{}.{ext}", if stem.is_empty() { "flight-plan" } else { stem.as_str() })
}

/// A string the station may send as null.
pub(crate) fn nullable<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(<Option<String> as serde::Deserialize>::deserialize(d)?.unwrap_or_default())
}

// ------------------------------------------------------------ request

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct WeatherRequest {
    #[serde(deserialize_with = "nullable")]
    pub source: String,
    pub manual: ManualWeather,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct LaunchRequest {
    pub rail_length_m: Option<f64>,
    pub tilt_deg: Option<f64>,
    pub azimuth_deg: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct SitePoint {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_msl_m: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct DispersionRequest {
    pub samples: u32,
    pub seed: Option<u64>,
}

/// `POST /api/flight_plans/run` body; `defaults` in the list has the same shape.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct PlanRequest {
    #[serde(deserialize_with = "nullable")]
    pub kind: String,
    #[serde(deserialize_with = "nullable")]
    pub label: String,
    pub fill_kg: Option<f64>,
    pub target_apogee_m: Option<f64>,
    pub supercharge_psi: Option<f64>,
    pub tank_temp_c: Option<f64>,
    #[serde(deserialize_with = "nullable")]
    pub motor_model: String,
    pub weather: WeatherRequest,
    pub launch: LaunchRequest,
    pub site: Option<SitePoint>,
    pub dispersion: DispersionRequest,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Limits {
    pub max_samples: u32,
    pub max_plans: u32,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct SummaryLanding {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub drift_m: Option<f64>,
    pub bearing_deg: Option<f64>,
}

/// A list item of `GET /api/flight_plans`.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct PlanSummary {
    #[serde(deserialize_with = "nullable")]
    pub id: String,
    #[serde(deserialize_with = "nullable")]
    pub label: String,
    #[serde(deserialize_with = "nullable")]
    pub kind: String,
    pub created_ms: i64,
    pub created_by: Option<String>,
    pub fill_kg: Option<f64>,
    pub apogee_m: Option<f64>,
    pub landing: Option<SummaryLanding>,
    pub dispersion_samples: u32,
    pub warnings: u32,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct PlanList {
    pub plans: Vec<PlanSummary>,
    pub defaults: PlanRequest,
    pub limits: Limits,
}

// ------------------------------------------------------------ plan document

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Figures {
    pub fill_kg: Option<f64>,
    pub supercharge_psi: Option<f64>,
    pub tank_temp_c: Option<f64>,
    pub impulse_ns: Option<f64>,
    pub impulse_sigma_rel: Option<f64>,
    pub burn_s: Option<f64>,
    pub liftoff_s: Option<f64>,
    pub burnout_s: Option<f64>,
    pub descent_s: Option<f64>,
    pub apogee_m: Option<f64>,
    pub apogee_msl_m: Option<f64>,
    pub time_to_apogee_s: Option<f64>,
    pub max_speed_mps: Option<f64>,
    pub max_mach: Option<f64>,
    pub rail_exit_mps: Option<f64>,
    pub landing_s: Option<f64>,
    pub drift_m: Option<f64>,
    pub drift_bearing_deg: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct PlanEvent {
    #[serde(deserialize_with = "nullable")]
    pub kind: String,
    pub t_s: f64,
    pub lat_deg: Option<f64>,
    pub lon_deg: Option<f64>,
    pub alt_msl_m: Option<f64>,
    pub e_m: Option<f64>,
    pub n_m: Option<f64>,
    pub u_m: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct PlanPoint {
    pub t_s: f64,
    #[serde(deserialize_with = "nullable")]
    pub phase: String,
    pub u_m: Option<f64>,
    pub lat_deg: Option<f64>,
    pub lon_deg: Option<f64>,
    pub alt_msl_m: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct ThrustCurve {
    pub t_s: Vec<f64>,
    pub thrust_n: Vec<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct BurnCurves {
    pub t_s: Vec<f64>,
    pub thrust_n: Vec<f64>,
    pub chamber_psi: Vec<f64>,
    pub tank_psi: Vec<f64>,
    pub mdot_ox_kg_s: Vec<f64>,
    pub mdot_fuel_kg_s: Vec<f64>,
    pub of_ratio: Vec<f64>,
    pub n2o_kg: Vec<f64>,
    pub summary: serde_json::Value,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct PlanLanding {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_msl_m: Option<f64>,
    pub t_s: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Band {
    pub mean: f64,
    pub sd: f64,
    pub p05: f64,
    pub p95: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct LandingEllipse {
    pub center_lat_deg: f64,
    pub center_lon_deg: f64,
    pub semi_major_m: f64,
    pub semi_minor_m: f64,
    pub major_bearing_deg: f64,
    pub confidence: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Dispersion {
    pub samples: u32,
    pub seed: Option<u64>,
    pub apogee_m: Option<Band>,
    pub landing_ellipse: Option<LandingEllipse>,
}

/// `GET /api/flight_plans/{id}` and the `POST .../run` reply.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct PlanDoc {
    #[serde(deserialize_with = "nullable")]
    pub format: String,
    pub version: u32,
    pub predicted: bool,
    #[serde(deserialize_with = "nullable")]
    pub id: String,
    #[serde(deserialize_with = "nullable")]
    pub label: String,
    #[serde(deserialize_with = "nullable")]
    pub kind: String,
    pub created_ms: i64,
    pub created_by: Option<String>,
    pub station_mode: Option<String>,
    pub compute_ms: Option<f64>,
    pub vehicle_source: Option<String>,
    pub motor_model: Option<String>,
    pub site: Option<SitePoint>,
    pub summary: Figures,
    pub events: Vec<PlanEvent>,
    pub points: Vec<PlanPoint>,
    pub thrust_curve: Option<ThrustCurve>,
    pub burn: Option<BurnCurves>,
    pub landing: Option<PlanLanding>,
    pub dispersion: Option<Dispersion>,
    pub warnings: Vec<String>,
    pub overlay: Option<MapOverlay>,
}

impl PlanDoc {
    /// The overlay to draw, when the plan has a trajectory.
    pub(crate) fn map_overlay(&self) -> Option<&MapOverlay> {
        self.overlay.as_ref().filter(|o| !o.is_empty())
    }
    /// Altitude above the pad against time from ignition.
    pub(crate) fn altitude_curve(&self) -> Vec<[f64; 2]> {
        self.points.iter().filter_map(|p| Some([p.t_s, p.u_m?])).filter(|p| p[0].is_finite() && p[1].is_finite()).collect()
    }
    pub(crate) fn thrust(&self) -> Vec<[f64; 2]> {
        match (&self.thrust_curve, &self.burn) {
            (Some(c), _) if !c.t_s.is_empty() => zip(&c.t_s, &c.thrust_n),
            (_, Some(b)) => zip(&b.t_s, &b.thrust_n),
            _ => Vec::new(),
        }
    }
}

/// `(t, value)` pairs of two parallel arrays, finite values only.
pub(crate) fn zip(t: &[f64], v: &[f64]) -> Vec<[f64; 2]> {
    t.iter().zip(v).map(|(t, v)| [*t, *v]).filter(|p| p[0].is_finite() && p[1].is_finite()).collect()
}

// ------------------------------------------------------------ form

/// The editable form: text per field so a half-typed number is kept.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct Form {
    pub kind: String,
    pub label: String,
    /// `fill` or `target`.
    pub fill_mode: String,
    pub fill_kg: String,
    pub target_apogee_m: String,
    pub supercharge_psi: String,
    pub tank_temp_c: String,
    pub motor_model: String,
    pub weather_source: String,
    pub surface_temp_c: String,
    pub surface_pressure_hpa: String,
    pub wind_mps: String,
    pub wind_from_deg: String,
    pub gust_mps: String,
    pub wind_1km_mps: String,
    pub wind_1km_from_deg: String,
    pub wind_3km_mps: String,
    pub wind_3km_from_deg: String,
    pub rail_length_m: String,
    pub tilt_deg: String,
    pub azimuth_deg: String,
    pub site_override: bool,
    pub site_lat: String,
    pub site_lon: String,
    pub site_alt: String,
    pub samples: String,
    pub seed: String,
}

fn num(v: Option<f64>) -> String {
    v.filter(|v| v.is_finite()).map(|v| format!("{v}")).unwrap_or_default()
}

impl Form {
    pub(crate) fn from_request(r: &PlanRequest) -> Self {
        let m = &r.weather.manual;
        Self {
            kind: if r.kind.is_empty() { "flight".into() } else { r.kind.clone() },
            label: r.label.clone(),
            fill_mode: if r.fill_kg.is_none() && r.target_apogee_m.is_some() { "target".into() } else { "fill".into() },
            fill_kg: num(r.fill_kg),
            target_apogee_m: num(r.target_apogee_m),
            supercharge_psi: num(r.supercharge_psi),
            tank_temp_c: num(r.tank_temp_c),
            motor_model: r.motor_model.clone(),
            weather_source: if r.weather.source.is_empty() { "planner".into() } else { r.weather.source.clone() },
            surface_temp_c: num(Some(m.surface_temp_c)),
            surface_pressure_hpa: num(m.surface_pressure_hpa),
            wind_mps: num(Some(m.wind_mps)),
            wind_from_deg: num(Some(m.wind_from_deg)),
            gust_mps: num(Some(m.gust_mps)),
            wind_1km_mps: num(m.wind_1km_mps),
            wind_1km_from_deg: num(m.wind_1km_from_deg),
            wind_3km_mps: num(m.wind_3km_mps),
            wind_3km_from_deg: num(m.wind_3km_from_deg),
            rail_length_m: num(r.launch.rail_length_m),
            tilt_deg: num(r.launch.tilt_deg),
            azimuth_deg: num(r.launch.azimuth_deg),
            site_override: false,
            site_lat: num(r.site.as_ref().map(|s| s.lat_deg)),
            site_lon: num(r.site.as_ref().map(|s| s.lon_deg)),
            site_alt: num(r.site.as_ref().and_then(|s| s.alt_msl_m)),
            samples: r.dispersion.samples.to_string(),
            seed: r.dispersion.seed.map(|s| s.to_string()).unwrap_or_default(),
        }
    }
}

fn opt(text: &str, name: &str) -> Result<Option<f64>, String> {
    super::apogee_planner::parse_optional(text).map_err(|_| format!("{name} must be a number."))
}
fn req(text: &str, name: &str) -> Result<f64, String> {
    opt(text, name)?.ok_or_else(|| format!("{name} is required."))
}

/// The request the form describes. Only the shape is checked here; the
/// station validates ranges and answers with its own reason.
pub(crate) fn build_request(f: &Form, limits: &Limits) -> Result<PlanRequest, String> {
    let (fill_kg, target_apogee_m) = if f.fill_mode == "target" {
        (None, Some(req(&f.target_apogee_m, "Target apogee")?))
    } else {
        (Some(req(&f.fill_kg, "Fill mass")?), None)
    };
    let samples: u32 = match f.samples.trim() {
        "" => 0,
        text => text.parse().map_err(|_| "Monte Carlo samples must be a whole number.".to_string())?,
    };
    if limits.max_samples > 0 && samples > limits.max_samples {
        return Err(format!("Monte Carlo samples must be at most {}.", limits.max_samples));
    }
    let seed = match f.seed.trim() {
        "" => None,
        text => Some(text.parse::<u64>().map_err(|_| "Seed must be a whole number.".to_string())?),
    };
    let site = if f.site_override {
        Some(SitePoint {
            lat_deg: req(&f.site_lat, "Site latitude")?,
            lon_deg: req(&f.site_lon, "Site longitude")?,
            alt_msl_m: opt(&f.site_alt, "Site altitude")?,
        })
    } else {
        None
    };
    if let Some(s) = &site
        && (!(-90.0..=90.0).contains(&s.lat_deg) || !(-180.0..=180.0).contains(&s.lon_deg))
    {
        return Err("The site must be a latitude in −90…90 and a longitude in −180…180.".into());
    }
    let manual = ManualWeather {
        surface_temp_c: opt(&f.surface_temp_c, "Surface temperature")?.unwrap_or(15.0),
        surface_pressure_hpa: opt(&f.surface_pressure_hpa, "Surface pressure")?,
        wind_mps: opt(&f.wind_mps, "Wind speed")?.unwrap_or(0.0),
        wind_from_deg: opt(&f.wind_from_deg, "Wind direction")?.unwrap_or(0.0),
        gust_mps: opt(&f.gust_mps, "Gust")?.unwrap_or(0.0),
        wind_1km_mps: opt(&f.wind_1km_mps, "Wind at 1 km")?,
        wind_1km_from_deg: opt(&f.wind_1km_from_deg, "Wind direction at 1 km")?,
        wind_3km_mps: opt(&f.wind_3km_mps, "Wind at 3 km")?,
        wind_3km_from_deg: opt(&f.wind_3km_from_deg, "Wind direction at 3 km")?,
    };
    Ok(PlanRequest {
        kind: f.kind.clone(),
        label: f.label.trim().chars().take(120).collect(),
        fill_kg,
        target_apogee_m,
        supercharge_psi: opt(&f.supercharge_psi, "Supercharge pressure")?,
        tank_temp_c: opt(&f.tank_temp_c, "Tank temperature")?,
        motor_model: f.motor_model.clone(),
        weather: WeatherRequest { source: f.weather_source.clone(), manual },
        launch: LaunchRequest {
            rail_length_m: opt(&f.rail_length_m, "Rail length")?,
            tilt_deg: opt(&f.tilt_deg, "Rail tilt")?,
            azimuth_deg: opt(&f.azimuth_deg, "Rail azimuth")?,
        },
        site,
        dispersion: DispersionRequest { samples, seed },
    })
}

// ------------------------------------------------------------ formatting

fn fmt(v: Option<f64>, unit: &str, precision: usize) -> String {
    match v.filter(|v| v.is_finite()) {
        Some(v) => format!("{v:.precision$}{unit}"),
        None => "—".into(),
    }
}

pub(crate) fn kind_label(kind: &str) -> &'static str {
    match kind {
        "burn" => "Burn (motor only)",
        _ => "Flight",
    }
}

pub(crate) fn event_label(kind: &str) -> String {
    match kind {
        "ignition" => "Ignition".into(),
        "liftoff" => "Liftoff".into(),
        "rail_exit" => "Rail exit".into(),
        "burnout" => "Burnout".into(),
        "apogee" => "Apogee".into(),
        "drogue" => "Drogue deploy".into(),
        "main" => "Main deploy".into(),
        "landing" => "Landing".into(),
        other => other.replace('_', " "),
    }
}

/// One-line summary of a list item.
pub(crate) fn summary_line(s: &PlanSummary) -> String {
    let mut parts = vec![kind_label(&s.kind).to_string()];
    if let Some(fill) = s.fill_kg {
        parts.push(format!("{fill:.2} kg"));
    }
    if let Some(apogee) = s.apogee_m {
        parts.push(format!("apogee {apogee:.0} m"));
    }
    if let Some(landing) = &s.landing
        && let Some(drift) = landing.drift_m
    {
        parts.push(match landing.bearing_deg {
            Some(b) => format!("lands {drift:.0} m toward {b:.0}°"),
            None => format!("lands {drift:.0} m away"),
        });
    }
    if s.dispersion_samples > 0 {
        parts.push(format!("{} Monte Carlo samples", s.dispersion_samples));
    }
    if s.warnings > 0 {
        parts.push(format!("{} warning{}", s.warnings, if s.warnings == 1 { "" } else { "s" }));
    }
    parts.join(" · ")
}

/// The figures table of a plan document.
pub(crate) fn summary_rows(p: &PlanDoc) -> Vec<(String, String)> {
    let s = &p.summary;
    let mut rows = vec![
        ("Kind".to_owned(), kind_label(&p.kind).to_owned()),
        ("Nitrous fill".to_owned(), fmt(s.fill_kg, " kg", 2)),
        ("Supercharge".to_owned(), fmt(s.supercharge_psi, " psi", 0)),
        ("Tank temperature".to_owned(), fmt(s.tank_temp_c, " °C", 1)),
        (
            "Total impulse".to_owned(),
            match (s.impulse_ns, s.impulse_sigma_rel) {
                (Some(i), Some(r)) => format!("{:.2} kN·s (±{:.0} %)", i / 1000.0, r * 100.0),
                (i, _) => fmt(i.map(|v| v / 1000.0), " kN·s", 2),
            },
        ),
        ("Burn time".to_owned(), fmt(s.burn_s, " s", 1)),
    ];
    if p.kind != "burn" {
        rows.extend([
            ("Apogee (above pad)".to_owned(), fmt(s.apogee_m, " m", 0)),
            ("Apogee (MSL)".to_owned(), fmt(s.apogee_msl_m, " m", 0)),
            ("Time to apogee".to_owned(), fmt(s.time_to_apogee_s, " s", 1)),
            ("Max speed".to_owned(), format!("{} (Mach {})", fmt(s.max_speed_mps, " m/s", 0), fmt(s.max_mach, "", 2))),
            ("Rail exit speed".to_owned(), fmt(s.rail_exit_mps, " m/s", 1)),
            ("Landing".to_owned(), format!("{} after ignition{}", fmt(s.landing_s, " s", 0), s.descent_s.map(|d| format!(" ({d:.0} s descent)")).unwrap_or_default())),
            (
                "Landing drift".to_owned(),
                match (s.drift_m, s.drift_bearing_deg) {
                    (Some(d), Some(b)) => format!("{d:.0} m toward {b:.0}°"),
                    (d, _) => fmt(d, " m", 0),
                },
            ),
        ]);
    }
    rows.push(("Motor model".to_owned(), p.motor_model.clone().unwrap_or_else(|| "—".into())));
    rows.push(("Vehicle".to_owned(), p.vehicle_source.clone().unwrap_or_else(|| "—".into())));
    rows
}

/// Apogee band and landing-ellipse text, when the plan ran Monte Carlo samples.
pub(crate) fn dispersion_rows(p: &PlanDoc) -> Vec<(String, String)> {
    let Some(d) = &p.dispersion else { return Vec::new() };
    let mut rows = vec![("Samples".to_owned(), format!("{}{}", d.samples, d.seed.map(|s| format!(" (seed {s})")).unwrap_or_default()))];
    if let Some(a) = &d.apogee_m {
        rows.push(("Apogee 5–95 %".to_owned(), format!("{:.0}–{:.0} m", a.p05, a.p95)));
        rows.push(("Apogee mean ± σ".to_owned(), format!("{:.0} ± {:.0} m", a.mean, a.sd)));
    }
    if let Some(e) = &d.landing_ellipse {
        rows.push((
            format!("Landing area ({:.0} %)", e.confidence * 100.0),
            format!("{:.0} × {:.0} m, long axis toward {:.0}°", e.semi_major_m * 2.0, e.semi_minor_m * 2.0, e.major_bearing_deg),
        ));
    }
    rows
}

/// `(event, time, altitude above pad)` rows.
pub(crate) fn event_rows(p: &PlanDoc) -> Vec<(String, String, String)> {
    p.events.iter().map(|e| (event_label(&e.kind), format!("{:.1} s", e.t_s), fmt(e.u_m, " m", 0))).collect()
}

/// The prominent label of predicted data.
pub(crate) fn predicted_label(label: &str) -> String {
    let label = label.trim();
    if label.is_empty() {
        "Predicted".into()
    } else if label.starts_with("Predicted") {
        label.to_string()
    } else {
        format!("Predicted: {label}")
    }
}

/// UTC date and time of a Unix ms instant (`2026-01-01 00:00 UTC`).
pub(crate) fn created_text(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let minutes = ms.div_euclid(60_000).rem_euclid(24 * 60);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock<T: serde::de::DeserializeOwned>(name: &str) -> T {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flight-predict-mock").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_the_contract_list_and_plan_document() {
        let list: PlanList = mock("list.json");
        assert_eq!(list.plans.len(), 2);
        assert_eq!(list.limits.max_samples, 500);
        assert_eq!(list.defaults.weather.source, "manual");
        assert_eq!(list.defaults.dispersion.samples, 50);
        assert!(list.plans[1].landing.is_none(), "burn plans have no landing");
        let plan: PlanDoc = mock("plan.json");
        assert!(plan.predicted);
        assert_eq!(plan.format, "gs26.flight_plan");
        assert_eq!(plan.events.first().unwrap().kind, "ignition");
        assert!(plan.burn.is_some());
        assert!(plan.map_overlay().is_some());
        assert!(!plan.altitude_curve().is_empty());
        assert!(!plan.thrust().is_empty());
        assert_eq!(plan.dispersion.as_ref().unwrap().apogee_m.as_ref().unwrap().p95, 3240.0);
        // Nulls the contract allows: no burn, no dispersion, no overlay.
        let sparse: PlanDoc = serde_json::from_value(serde_json::json!({"id": "fp-1", "kind": "burn", "burn": null, "dispersion": null, "overlay": null, "thrust_curve": {"t_s": [0.0, 1.0], "thrust_n": [0.0, 100.0]}})).unwrap();
        assert!(sparse.map_overlay().is_none());
        assert!(dispersion_rows(&sparse).is_empty());
        assert_eq!(sparse.thrust().len(), 2);
    }

    #[test]
    fn form_round_trips_defaults_and_builds_a_request() {
        let list: PlanList = mock("list.json");
        let form = Form::from_request(&list.defaults);
        assert_eq!(form.fill_mode, "fill");
        assert_eq!(form.fill_kg, "8.5");
        assert!(!form.site_override, "the station's site is used unless overridden");
        let request = build_request(&form, &list.limits).unwrap();
        assert_eq!(request.fill_kg, Some(8.5));
        assert_eq!(request.target_apogee_m, None);
        assert_eq!(request.site, None);
        assert_eq!(request.weather.manual.wind_mps, 4.0);
        assert_eq!(request.launch.rail_length_m, Some(5.18));
        assert_eq!(request.dispersion, DispersionRequest { samples: 50, seed: Some(1) });
        let body = serde_json::to_value(&request).unwrap();
        assert_eq!(body["weather"]["manual"]["wind_from_deg"], 300.0);
        assert!(body["target_apogee_m"].is_null());

        let mut target = form.clone();
        target.fill_mode = "target".into();
        target.target_apogee_m = "3048".into();
        target.site_override = true;
        let request = build_request(&target, &list.limits).unwrap();
        assert_eq!((request.fill_kg, request.target_apogee_m), (None, Some(3048.0)));
        assert_eq!(request.site.unwrap().lat_deg, 31.758);
    }

    fn real<T: serde::de::DeserializeOwned>(name: &str) -> T {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/backend-contract/flight-plans").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn decodes_the_backend_fixtures() {
        let list: PlanList = real("list.json");
        assert_eq!(list.limits, Limits { max_samples: 500, max_plans: 50 });
        assert_eq!(list.defaults.label, "", "a null label reads as empty");
        let form = Form::from_request(&list.defaults);
        assert_eq!(form.fill_mode, "target", "defaults with only a target apogee size by target");
        assert_eq!(form.weather_source, "planner");
        let request = build_request(&form, &list.limits).unwrap();
        assert_eq!((request.fill_kg, request.target_apogee_m), (None, Some(3048.0)));
        assert_eq!(summary_line(&list.plans[0]), "Flight · 8.50 kg · apogee 3766 m · lands 1230 m toward 90° · 20 Monte Carlo samples · 1 warning");
        let plan: PlanDoc = real("plan.json");
        assert!(plan.predicted);
        assert!(plan.burn.is_none());
        assert!(!plan.thrust().is_empty(), "the thrust curve stands in for burn curves");
        assert!(!plan.altitude_curve().is_empty());
        assert_eq!(plan.map_overlay().unwrap().tracks.len(), 5);
        let rows = summary_rows(&plan);
        assert!(rows.iter().any(|r| r.0 == "Total impulse" && r.1 == "16.33 kN·s (±15 %)"), "{rows:?}");
        assert!(dispersion_rows(&plan).iter().any(|r| r.1 == "2909–4737 m"));
        let refusal: serde_json::Value = real("run-forbidden.json");
        let text = super::super::igt::refusal(&format!("HTTP 403: {refusal}"));
        assert_eq!(text, "Running a flight simulation needs permission to send commands (operator or administrator).");
        let manifest: serde_json::Value = real("bundle-manifest.json");
        assert_eq!(manifest["predicted"], true);
        assert!(manifest["entries"].as_array().unwrap().iter().any(|e| e["path"] == "derived/flight_plan.json"));
    }

    #[test]
    fn form_errors_name_the_field() {
        let limits = Limits { max_samples: 500, max_plans: 50 };
        let mut form = Form::from_request(&PlanRequest::default());
        form.fill_kg = "abc".into();
        assert_eq!(build_request(&form, &limits).unwrap_err(), "Fill mass must be a number.");
        form.fill_kg = String::new();
        assert_eq!(build_request(&form, &limits).unwrap_err(), "Fill mass is required.");
        form.fill_kg = "8".into();
        form.samples = "501".into();
        assert_eq!(build_request(&form, &limits).unwrap_err(), "Monte Carlo samples must be at most 500.");
        form.samples = "10".into();
        form.seed = "-1".into();
        assert_eq!(build_request(&form, &limits).unwrap_err(), "Seed must be a whole number.");
        form.seed = String::new();
        form.site_override = true;
        form.site_lat = "95".into();
        form.site_lon = "0".into();
        assert!(build_request(&form, &limits).unwrap_err().contains("latitude"));
    }

    #[test]
    fn formatting_and_paths() {
        let plan: PlanDoc = mock("plan.json");
        let rows = summary_rows(&plan);
        assert!(rows.contains(&("Landing drift".to_owned(), "1012 m toward 87°".to_owned())));
        assert!(rows.contains(&("Max speed".to_owned(), "220 m/s (Mach 0.66)".to_owned())));
        let disp = dispersion_rows(&plan);
        assert!(disp.contains(&("Apogee 5–95 %".to_owned(), "2850–3240 m".to_owned())));
        assert!(disp.contains(&("Landing area (95 %)".to_owned(), "800 × 300 m, long axis toward 95°".to_owned())));
        assert_eq!(event_rows(&plan)[3], ("Burnout".to_owned(), "7.1 s".to_owned(), "1250 m".to_owned()));
        let list: PlanList = mock("list.json");
        assert_eq!(summary_line(&list.plans[0]), "Flight · 8.50 kg · apogee 3050 m · lands 1012 m toward 87° · 50 Monte Carlo samples · 1 warning");
        assert_eq!(summary_line(&list.plans[1]), "Burn (motor only) · 6.00 kg");
        assert_eq!(plan_path("fp-1/../x"), "/api/flight_plans/fp-1%2F..%2Fx");
        assert_eq!(bundle_path("fp-1"), "/api/flight_plans/fp-1/bundle");
        assert_eq!(file_name("fp-1:2", "csv"), "fp-1_2.csv");
        assert_eq!(predicted_label("SWEET V6"), "Predicted: SWEET V6");
        assert_eq!(predicted_label("Predicted: no signal"), "Predicted: no signal");
        assert_eq!(predicted_label(""), "Predicted");
        assert_eq!(created_text(1_767_225_600_000), "2026-01-01 00:00 UTC");
    }
}
