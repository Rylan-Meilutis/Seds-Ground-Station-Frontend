//! Apogee planner (GroundStation26 `docs/apogee-planner.md`): a target apogee
//! and the day's weather become the nitrous fill mass target and the
//! supercharge (Turbocharge) pressure target. Administrators edit the planner
//! settings; anyone who may send commands can compute a plan and, after an
//! explicit confirmation, apply it to the fill targets. The station refuses an
//! apply in Test fire and Testing modes and once the fill has started; the
//! reason it gives is shown as is. Applying only sets targets, it never
//! actuates anything.
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub(crate) const PATH: &str = "/api/apogee_planner";
pub(crate) const SETTINGS_PATH: &str = "/api/apogee_planner/settings";
pub(crate) const COMPUTE_PATH: &str = "/api/apogee_planner/compute";
pub(crate) const APPLY_PATH: &str = "/api/apogee_planner/apply";
pub(crate) const FETCH_PATH: &str = "/api/apogee_planner/weather/fetch";
/// Writes the plan's weather into the firmware simulation's plant conditions (admin; simulated-firmware stations).
pub(crate) const REHEARSE_PATH: &str = "/api/apogee_planner/sim_environment/apply";
pub(crate) const ORK_PATH: &str = "/api/apogee_planner/ork";
pub(crate) const CALIBRATE_PATH: &str = "/api/apogee_planner/calibrate_flight";
/// IREC/SA Cup minimum rail exit speed.
pub(crate) const MIN_RAIL_EXIT_MPS: f64 = 30.5;
pub(crate) const M_PER_FT: f64 = 0.3048;

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub(crate) struct ManualWeather {
    pub surface_temp_c: f64,
    pub surface_pressure_hpa: Option<f64>,
    pub wind_mps: f64,
    pub wind_from_deg: f64,
    pub gust_mps: f64,
    pub wind_1km_mps: Option<f64>,
    pub wind_1km_from_deg: Option<f64>,
    pub wind_3km_mps: Option<f64>,
    pub wind_3km_from_deg: Option<f64>,
}
impl Default for ManualWeather {
    fn default() -> Self {
        Self {
            surface_temp_c: 15.0,
            surface_pressure_hpa: None,
            wind_mps: 0.0,
            wind_from_deg: 0.0,
            gust_mps: 0.0,
            wind_1km_mps: None,
            wind_1km_from_deg: None,
            wind_3km_mps: None,
            wind_3km_from_deg: None,
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub(crate) struct Settings {
    pub target_apogee_m: f64,
    pub display_unit: String,
    pub weather_source: String,
    pub online_weather_enabled: bool,
    pub launch_time_ms: Option<i64>,
    pub manual_weather: ManualWeather,
    pub tank_temp_c: Option<f64>,
    // Tank and limit values come from the station. One it omits is shown as
    // "not set" (never an invented value) and is left out when saving, so
    // the station keeps its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tank_volume_l: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_ullage_frac: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_fill_kg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meop_psi: Option<f64>,
    pub nominal_supercharge_psi: Option<f64>,
    /// Every field this editor does not model (`motor_model`, `version`, and
    /// whatever a newer station adds), sent back exactly as the station
    /// served it so a save never resets them.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            target_apogee_m: 3048.0,
            display_unit: "ft".into(),
            weather_source: "manual".into(),
            online_weather_enabled: false,
            launch_time_ms: None,
            manual_weather: ManualWeather::default(),
            tank_temp_c: None,
            tank_volume_l: None,
            min_ullage_frac: None,
            min_fill_kg: None,
            meop_psi: None,
            nominal_supercharge_psi: None,
            extra: serde_json::Map::new(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Site {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_msl_m: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Level {
    pub h_agl_m: f64,
    pub speed_mps: f64,
    pub from_deg: f64,
    pub temp_c: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Weather {
    pub provider: String,
    pub fetched_ms: i64,
    pub valid_time_ms: i64,
    pub request_url: Option<String>,
    pub surface_temp_c: f64,
    pub surface_pressure_pa: f64,
    pub wind_mps: f64,
    pub wind_from_deg: f64,
    pub gust_mps: f64,
    pub levels: Vec<Level>,
    pub site: Site,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Limits {
    pub max_fill_kg: f64,
    pub min_fill_kg: f64,
    pub max_supercharge_psi: f64,
    pub vapour_pressure_psi: f64,
    pub limit_sources: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct PlanFit {
    pub c_ref_ns_per_kg: f64,
    pub n: f64,
    pub sigma_c_rel: f64,
    pub fires: u32,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Targets {
    pub nitrous_target_mass_kg: f64,
    pub nitrous_target_pressure_psi: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Plan {
    pub id: String,
    pub computed_ms: i64,
    pub computed_by: Option<String>,
    pub status: String,
    pub target_apogee_m: f64,
    pub fill_mass_kg: f64,
    pub supercharge_psi: f64,
    pub apogee_m: f64,
    pub apogee_band_m: [f64; 2],
    pub apogee_sigma_m: f64,
    pub max_achievable_apogee_m: f64,
    pub min_achievable_apogee_m: f64,
    pub rail_exit_mps: f64,
    pub max_mach: f64,
    pub time_to_apogee_s: f64,
    pub burnout_s: f64,
    pub drift_m: f64,
    pub drift_bearing_deg: f64,
    pub impulse_ns: f64,
    pub impulse_sigma_ns: f64,
    pub fill_band_kg: [f64; 2],
    pub limits: Limits,
    pub warnings: Vec<String>,
    pub weather: Weather,
    pub weather_age_s: f64,
    pub vehicle_source: String,
    pub motor_fit: PlanFit,
    pub current_targets: Targets,
}
impl Plan {
    pub(crate) fn reachable(&self) -> bool {
        self.status == "ok"
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Applied {
    pub plan: Plan,
    pub applied_ms: i64,
    pub applied_by: Option<String>,
    pub previous_targets: Targets,
    pub new_targets: Targets,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Vehicle {
    pub source: String,
    pub name: String,
    pub dry_mass_kg: f64,
    pub length_m: f64,
    pub diameter_m: f64,
    pub cn_alpha_per_rad: f64,
    pub static_margin_cal: f64,
    pub imported_ms: Option<i64>,
    pub warnings: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct FitFire {
    pub id: String,
    pub label: String,
    pub fill_kg: f64,
    pub impulse_ns: f64,
    pub weight: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct FitHistory {
    pub at_ms: i64,
    pub c_ref_ns_per_kg: f64,
    pub n: f64,
    pub fires: u32,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct MotorFit {
    pub c_ref_ns_per_kg: f64,
    pub n: f64,
    pub sigma_c_rel: f64,
    pub p_ref_psi: f64,
    pub fires: Vec<FitFire>,
    pub history: Vec<FitHistory>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct ApplyGate {
    pub allowed: bool,
    pub reason: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct PostFlight {
    pub applied_plan_id: String,
    pub predicted_apogee_m: f64,
    pub band_m: [f64; 2],
    pub actual_apogee_m: Option<f64>,
    pub source: Option<String>,
    pub error_m: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct CalibrationPoint {
    pub at_ms: i64,
    pub cd_scale: f64,
    pub predicted_m: f64,
    pub actual_m: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub(crate) struct FlightCalibration {
    pub cd_scale: f64,
    pub points: Vec<CalibrationPoint>,
}
impl Default for FlightCalibration {
    fn default() -> Self {
        Self { cd_scale: 1.0, points: Vec::new() }
    }
}

/// `GET /api/apogee_planner`.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Status {
    pub settings: Settings,
    pub last_plan: Option<Plan>,
    pub applied: Option<Applied>,
    pub weather_cache: Option<Weather>,
    pub vehicle: Vehicle,
    pub motor_fit: MotorFit,
    pub apply_gate: ApplyGate,
    pub post_flight: Option<PostFlight>,
    pub flight_calibration: FlightCalibration,
    pub online_request_preview: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub(crate) struct ApplyRequest {
    pub plan_id: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub(crate) struct CalibrateRequest {
    pub actual_apogee_m: Option<f64>,
}

// ------------------------------------------------------------ pure helpers

pub(crate) fn is_feet(unit: &str) -> bool {
    unit != "m"
}

/// Metres in the display unit.
pub(crate) fn to_unit(m: f64, unit: &str) -> f64 {
    if is_feet(unit) { m / M_PER_FT } else { m }
}

/// Display-unit text as metres; positive and finite only.
pub(crate) fn parse_altitude(text: &str, unit: &str) -> Result<f64, String> {
    let value: f64 = text.trim().parse().map_err(|_| "Target apogee must be a number.".to_string())?;
    if !value.is_finite() || value <= 0.0 {
        return Err("Target apogee must be above zero.".into());
    }
    let m = if is_feet(unit) { value * M_PER_FT } else { value };
    if m > 30_000.0 {
        return Err("Target apogee must be below 30 km.".into());
    }
    Ok(m)
}

pub(crate) fn altitude_label(m: f64, unit: &str) -> String {
    format!("{:.0} {}", to_unit(m, unit), if is_feet(unit) { "ft" } else { "m" })
}

pub(crate) fn band_label(band: [f64; 2], unit: &str) -> String {
    format!("{:.0}–{:.0} {}", to_unit(band[0], unit), to_unit(band[1], unit), if is_feet(unit) { "ft" } else { "m" })
}

pub(crate) fn age_label(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "unknown age".into();
    }
    let s = seconds.round() as i64;
    if s < 90 {
        format!("{s} s old")
    } else if s < 90 * 60 {
        format!("{} min old", (s + 30) / 60)
    } else if s < 48 * 3600 {
        format!("{:.1} h old", s as f64 / 3600.0)
    } else {
        format!("{} days old", s / 86_400)
    }
}

pub(crate) fn provider_label(provider: &str) -> &'static str {
    match provider {
        "open_meteo" => "Open-Meteo forecast",
        "manual" => "Manual entry",
        "cached" => "Cached forecast",
        _ => "Unknown source",
    }
}

/// HH:MM UTC of a Unix ms time.
pub(crate) fn hhmm_utc(ms: i64) -> String {
    let minutes = ms.div_euclid(60_000).rem_euclid(24 * 60);
    format!("{:02}:{:02} UTC", minutes / 60, minutes % 60)
}

pub(crate) fn status_label(plan: &Plan, unit: &str) -> Option<String> {
    match plan.status.as_str() {
        "ok" => None,
        "unreachable_high" => Some(format!(
            "Target not reachable: the most this vehicle can reach in these conditions is {} (max fill at the highest allowed supercharge).",
            altitude_label(plan.max_achievable_apogee_m, unit)
        )),
        "unreachable_low" => Some(format!(
            "Target below the minimum: the minimum fill already reaches {}.",
            altitude_label(plan.min_achievable_apogee_m, unit)
        )),
        other => Some(format!("Plan status: {other}")),
    }
}

pub(crate) fn rail_exit_low(plan: &Plan) -> bool {
    plan.rail_exit_mps > 0.0 && plan.rail_exit_mps < MIN_RAIL_EXIT_MPS
}

/// Why Apply is unavailable, or None. The station's own reason comes first.
pub(crate) fn apply_disabled_reason(status: &Status, plan: Option<&Plan>, can_send: bool) -> Option<String> {
    if !can_send {
        return Some("Command permission is required to apply a plan.".into());
    }
    if !status.apply_gate.allowed {
        return Some(status.apply_gate.reason.clone().unwrap_or_else(|| "The station does not allow applying a plan now.".into()));
    }
    let Some(plan) = plan else {
        return Some("Compute a plan first.".into());
    };
    if !plan.reachable() {
        return Some("This plan does not reach the target; it cannot be applied.".into());
    }
    None
}

pub(crate) fn confirm_lines(plan: &Plan, unit: &str) -> Vec<String> {
    let old = &plan.current_targets;
    vec![
        format!("Nitrous fill mass target: {:.2} kg → {:.2} kg.", old.nitrous_target_mass_kg, plan.fill_mass_kg),
        format!("Supercharge pressure target: {:.0} psi → {:.0} psi.", old.nitrous_target_pressure_psi, plan.supercharge_psi),
        format!(
            "Target {}; predicted {} (±1σ {}).",
            altitude_label(plan.target_apogee_m, unit),
            altitude_label(plan.apogee_m, unit),
            band_label(plan.apogee_band_m, unit)
        ),
        format!(
            "Weather: {}, {}, {:.1} °C, wind {:.1} m/s from {:.0}°.",
            provider_label(&plan.weather.provider),
            age_label(plan.weather_age_s),
            plan.weather.surface_temp_c,
            plan.weather.wind_mps,
            plan.weather.wind_from_deg
        ),
        format!(
            "Vehicle: {}; motor fit from {} fire(s), ±{:.0}% impulse.",
            if plan.vehicle_source.is_empty() { "default" } else { plan.vehicle_source.as_str() },
            plan.motor_fit.fires,
            plan.motor_fit.sigma_c_rel * 100.0
        ),
        "This only sets the fill targets (the same as editing them by hand). Nothing is actuated. The station logs who applied it and with which inputs.".into(),
    ]
}

/// The note for the Actions fill targets when they still match the applied plan.
pub(crate) fn applied_note(applied: &Applied, nitrous_mass_kg: f64, nitrous_psi: f64, unit: &str) -> Option<String> {
    let t = &applied.new_targets;
    let same = (t.nitrous_target_mass_kg - nitrous_mass_kg).abs() < 0.005
        && (t.nitrous_target_pressure_psi - nitrous_psi).abs() < 0.05;
    same.then(|| {
        format!(
            "Set by apogee plan for {} at {} by {}",
            altitude_label(applied.plan.target_apogee_m, unit),
            hhmm_utc(applied.applied_ms),
            applied.applied_by.as_deref().filter(|u| !u.is_empty()).unwrap_or("an operator")
        )
    })
}

pub(crate) fn plan_rows(plan: &Plan, unit: &str) -> Vec<(String, String)> {
    let mut rows = vec![
        ("Nitrous fill mass".to_owned(), format!("{:.2} kg (±1σ impulse: {:.2}–{:.2} kg)", plan.fill_mass_kg, plan.fill_band_kg[0], plan.fill_band_kg[1])),
        ("Supercharge pressure".to_owned(), format!("{:.0} psi", plan.supercharge_psi)),
        ("Predicted apogee".to_owned(), format!("{} (±1σ {})", altitude_label(plan.apogee_m, unit), band_label(plan.apogee_band_m, unit))),
        ("Total impulse".to_owned(), format!("{:.2} ± {:.2} kN·s", plan.impulse_ns / 1000.0, plan.impulse_sigma_ns / 1000.0)),
        ("Rail exit speed".to_owned(), format!("{:.1} m/s", plan.rail_exit_mps)),
        ("Max Mach".to_owned(), format!("{:.2}", plan.max_mach)),
        ("Time to apogee".to_owned(), format!("{:.1} s (burnout {:.1} s)", plan.time_to_apogee_s, plan.burnout_s)),
        ("Landing drift".to_owned(), format!("{:.0} m toward {:.0}°", plan.drift_m, plan.drift_bearing_deg)),
        ("Max fill / min fill".to_owned(), format!("{:.2} kg / {:.2} kg", plan.limits.max_fill_kg, plan.limits.min_fill_kg)),
        ("Supercharge limit".to_owned(), format!("{:.0} psi (N2O vapour pressure {:.0} psi)", plan.limits.max_supercharge_psi, plan.limits.vapour_pressure_psi)),
    ];
    rows.push((
        "Weather used".to_owned(),
        format!(
            "{}, {} — {:.1} °C, {:.0} hPa, wind {:.1} m/s from {:.0}° (gust {:.1})",
            provider_label(&plan.weather.provider),
            age_label(plan.weather_age_s),
            plan.weather.surface_temp_c,
            plan.weather.surface_pressure_pa / 100.0,
            plan.weather.wind_mps,
            plan.weather.wind_from_deg,
            plan.weather.gust_mps
        ),
    ));
    rows
}

fn opt_text(value: Option<f64>) -> String {
    value.map(|v| format!("{v}")).unwrap_or_default()
}

/// A required tank or limit value the station did not send reads "not set".
fn limit_placeholder(value: &str, optional: bool) -> &'static str {
    if value.is_empty() && !optional { "not set" } else { "" }
}

/// Empty text is None; anything else must be a finite number.
pub(crate) fn parse_optional(text: &str) -> Result<Option<f64>, ()> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<f64>().ok().filter(|v| v.is_finite()).map(Some).ok_or(())
}

// ------------------------------------------------------------ views

#[component]
pub(super) fn ApogeePlannerSettings(theme: super::layout::ThemeConfig) -> Element {
    rsx! {
        ApogeePlannerView { theme, admin: crate::auth::is_admin(), can_send: crate::auth::can_view_actions(), initial: None }
    }
}

type NumberSetter = fn(&mut Settings, Option<f64>);

#[component]
pub(super) fn ApogeePlannerView(theme: super::layout::ThemeConfig, admin: bool, can_send: bool, initial: Option<Status>) -> Element {
    let factory = super::station_settings_request::use_factory();
    let seeded = initial.is_some();
    let mut status = use_signal(move || initial.clone().map(Ok::<Status, String>));
    let mut draft = use_signal(|| None::<Settings>);
    let mut draft_rev = use_signal(|| 0u64);
    let mut invalid = use_signal(Vec::<String>::new);
    let mut target_text = use_signal(String::new);
    let mut revision = use_signal(|| 0u64);
    let mut busy = use_signal(|| false);
    let mut result = use_signal(|| None::<(bool, String)>);
    let mut confirming = use_signal(|| None::<Plan>);
    let mut actual_text = use_signal(String::new);
    use_future(move || async move {
        if seeded {
            return;
        }
        loop {
            let current = *revision.peek();
            let fetched = factory.get::<Status>(PATH).await;
            status.set(Some(fetched));
            // Read on open, after every action and every 10 s (the apply gate follows the flight state).
            for _ in 0..40 {
                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::TimeoutFuture::new(250).await;
                #[cfg(not(target_arch = "wasm32"))]
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                if *revision.peek() != current {
                    break;
                }
            }
        }
    });
    // Seed the editable copy from the station until the operator changes something.
    let loaded = status.read().as_ref().and_then(|s| s.as_ref().ok()).cloned();
    if let Some(s) = loaded.as_ref() {
        if draft.peek().is_none() {
            draft.set(Some(s.settings.clone()));
            target_text.set(format!("{:.0}", to_unit(s.settings.target_apogee_m, &s.settings.display_unit)));
        }
    }
    let Some(s) = loaded else {
        return rsx! {
            section { class: "gs26-apogee-planner", style: "display:grid;gap:10px;min-width:0;",
                h3 { style: "margin:0;", "Apogee planner" }
                match status() {
                    Some(Err(e)) => rsx! { p { role: "status", class: "gs-notice gs-notice--warn", "Cannot read the apogee planner: {super::igt::refusal(&e)}" } },
                    _ => rsx! { p { role: "status", "Reading the apogee planner…" } },
                }
            }
        };
    };
    let d = draft().unwrap_or_else(|| s.settings.clone());
    let unit = d.display_unit.clone();
    let ushort = if is_feet(&unit) { "ft" } else { "m" };
    let vehicle_kind = if s.vehicle.source == "ork" { "OpenRocket import" } else { "default assumptions" };
    let dirty = d != s.settings;
    let editable = admin && !busy();
    let plan = s.last_plan.clone();
    let apply_reason = apply_disabled_reason(&s, plan.as_ref(), can_send);
    let parsed_target = parse_altitude(&target_text.read(), &unit);

    // One action at a time; every reply refreshes the view.
    let mut run = move |label: &'static str, work: std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<Status>, String>>>>| {
        if *busy.peek() {
            return;
        }
        busy.set(true);
        result.set(None);
        spawn(async move {
            let reply = work.await;
            busy.set(false);
            confirming.set(None);
            match reply {
                Ok(next) => {
                    if let Some(next) = next {
                        status.set(Some(Ok(next)));
                    }
                    result.set(Some((true, label.to_string())));
                }
                Err(e) => result.set(Some((false, format!("{label} failed: {}", super::igt::refusal(&e))))),
            }
            revision += 1;
        });
    };

    let save = move |_| {
        let Some(mut next) = draft.peek().clone() else { return };
        match parse_altitude(&target_text.peek(), &next.display_unit) {
            Ok(m) => next.target_apogee_m = m,
            Err(e) => {
                result.set(Some((false, e)));
                return;
            }
        }
        if !invalid.peek().is_empty() {
            result.set(Some((false, format!("Fix the fields first: {}.", invalid.peek().join(", ")))));
            return;
        }
        let owner = factory.capture();
        let work = super::station_settings_request::put_captured::<_, Status>(SETTINGS_PATH, &next, owner);
        draft.set(None);
        run("Planner settings saved", Box::pin(async move { work.await.map(Some) }));
    };
    let compute = move |_| {
        let owner = factory.capture();
        let work = super::station_settings_request::post_captured::<_, Plan>(COMPUTE_PATH, &serde_json::json!({}), owner);
        run("Plan computed", Box::pin(async move {
            let plan = work.await?;
            let mut next = status.peek().as_ref().and_then(|s| s.as_ref().ok()).cloned().unwrap_or_default();
            next.last_plan = Some(plan);
            Ok(Some(next))
        }));
    };
    let fetch = move |_| {
        let owner = factory.capture();
        let work = super::station_settings_request::post_captured::<_, Weather>(FETCH_PATH, &serde_json::json!({}), owner);
        run("Forecast fetched", Box::pin(async move {
            let weather = work.await?;
            let mut next = status.peek().as_ref().and_then(|s| s.as_ref().ok()).cloned().unwrap_or_default();
            next.weather_cache = Some(weather);
            Ok(Some(next))
        }));
    };
    let rehearse = move |_| {
        let owner = factory.capture();
        let work = super::station_settings_request::post_captured::<_, serde_json::Value>(REHEARSE_PATH, &serde_json::json!({}), owner);
        run("Plan weather sent to the firmware simulation", Box::pin(async move { work.await.map(|_| None) }));
    };
    let mut apply = move |plan: Plan| {
        let owner = factory.capture();
        let work = super::station_settings_request::post_captured_no_redirect::<_, Applied>(APPLY_PATH, &ApplyRequest { plan_id: plan.id.clone() }, owner);
        run("Plan applied to the fill targets", Box::pin(async move {
            let applied = work.await?;
            let mut next = status.peek().as_ref().and_then(|s| s.as_ref().ok()).cloned().unwrap_or_default();
            next.applied = Some(applied);
            Ok(Some(next))
        }));
    };
    let calibrate = move |_| {
        let actual = match parse_optional(&actual_text.peek()) {
            Ok(v) => v.map(|v| if is_feet(&unit_for_calibration(&status)) { v * M_PER_FT } else { v }),
            Err(()) => {
                result.set(Some((false, "Actual apogee must be a number or empty.".into())));
                return;
            }
        };
        let owner = factory.capture();
        let work = super::station_settings_request::post_captured::<_, Status>(CALIBRATE_PATH, &CalibrateRequest { actual_apogee_m: actual }, owner);
        run("Calibration point added", Box::pin(async move { work.await.map(Some) }));
    };
    let clear_ork = move |_| {
        let owner = factory.capture();
        let work = super::station_settings_request::delete_captured::<Status>(ORK_PATH, owner);
        run("Default vehicle restored", Box::pin(async move { work.await.map(Some) }));
    };
    let upload_ork = move |event: Event<FormData>| {
        let Some(file) = event.files().first().cloned() else { return };
        let owner = factory.capture();
        run("OpenRocket file imported", Box::pin(async move {
            if file.size() > 16 * 1024 * 1024 {
                return Err("The .ork file is larger than 16 MiB.".into());
            }
            let bytes = file.read_bytes().await.map_err(|e| format!("Cannot read the file: {e}"))?;
            super::station_settings_request::post_bytes_captured::<Status>(ORK_PATH, bytes.to_vec(), owner).await.map(Some)
        }));
    };

    let mut edit = move |f: &dyn Fn(&mut Settings)| {
        let mut next = draft.peek().clone().unwrap_or_default();
        f(&mut next);
        draft.set(Some(next));
    };
    let mut number_field = move |key: &'static str, text: String, set: NumberSetter, optional: bool| {
        match parse_optional(&text) {
            Ok(v) if v.is_some() || optional => {
                invalid.with_mut(|list| list.retain(|k| k != key));
                let mut next = draft.peek().clone().unwrap_or_default();
                set(&mut next, v);
                draft.set(Some(next));
            }
            _ => invalid.with_mut(|list| {
                if !list.iter().any(|k| k == key) {
                    list.push(key.to_owned());
                }
            }),
        }
    };
    let fields: Vec<(&'static str, &'static str, String, NumberSetter, bool)> = vec![
        ("temp", "Surface temperature (°C)", format!("{}", d.manual_weather.surface_temp_c), |s, v| s.manual_weather.surface_temp_c = v.unwrap_or_default(), false),
        ("pressure", "Surface pressure (hPa, empty = standard)", opt_text(d.manual_weather.surface_pressure_hpa), |s, v| s.manual_weather.surface_pressure_hpa = v, true),
        ("wind", "Wind (m/s)", format!("{}", d.manual_weather.wind_mps), |s, v| s.manual_weather.wind_mps = v.unwrap_or_default(), false),
        ("wind_from", "Wind from (°)", format!("{}", d.manual_weather.wind_from_deg), |s, v| s.manual_weather.wind_from_deg = v.unwrap_or_default(), false),
        ("gust", "Gusts (m/s)", format!("{}", d.manual_weather.gust_mps), |s, v| s.manual_weather.gust_mps = v.unwrap_or_default(), false),
        ("w1", "Wind at 1 km (m/s, optional)", opt_text(d.manual_weather.wind_1km_mps), |s, v| s.manual_weather.wind_1km_mps = v, true),
        ("w1d", "from (°)", opt_text(d.manual_weather.wind_1km_from_deg), |s, v| s.manual_weather.wind_1km_from_deg = v, true),
        ("w3", "Wind at 3 km (m/s, optional)", opt_text(d.manual_weather.wind_3km_mps), |s, v| s.manual_weather.wind_3km_mps = v, true),
        ("w3d", "from (°)", opt_text(d.manual_weather.wind_3km_from_deg), |s, v| s.manual_weather.wind_3km_from_deg = v, true),
    ];
    let limit_fields: Vec<(&'static str, &'static str, String, NumberSetter, bool)> = vec![
        ("tank_temp", "Tank temperature override (°C, empty = ambient)", opt_text(d.tank_temp_c), |s, v| s.tank_temp_c = v, true),
        ("tank_volume", "Run tank volume (L)", opt_text(d.tank_volume_l), |s, v| s.tank_volume_l = v, false),
        ("ullage", "Minimum ullage (fraction)", opt_text(d.min_ullage_frac), |s, v| s.min_ullage_frac = v, false),
        ("min_fill", "Minimum fill (kg)", opt_text(d.min_fill_kg), |s, v| s.min_fill_kg = v, false),
        ("meop", "Max supercharge / MEOP (psi)", opt_text(d.meop_psi), |s, v| s.meop_psi = v, false),
        ("nominal_sc", "Nominal supercharge (psi, empty = current target)", opt_text(d.nominal_supercharge_psi), |s, v| s.nominal_supercharge_psi = v, true),
    ];
    let input_style = "font:inherit;min-height:36px;width:12ch;padding:4px 8px;";
    let label_style = "display:grid;gap:2px;font-weight:700;";
    let rev = draft_rev();
    let show_fetch = s.settings.online_weather_enabled && s.settings.weather_source == "open_meteo";
    let dialog_plan = confirming();
    let dialog_lines = dialog_plan.as_ref().map(|p| confirm_lines(p, &unit));

    rsx! {
        section { class: "gs26-apogee-planner", "data-apogee-planner": "", style: "display:grid;gap:10px;min-width:0;",
            h3 { style: "margin:0;", "Apogee planner" }
            p { style: "margin:0;color:{theme.text_muted};",
                "Computes the nitrous fill mass and supercharge pressure that reach the target apogee in the given weather. Applying a plan only sets the fill targets; it is allowed before the fill starts, and not in Test fire or Testing mode."
            }
            if !admin {
                p { role: "status", "data-planner-read-only": "", style: "margin:0;color:{theme.text_muted};", "Read only: an administrator changes the planner settings." }
            }
            fieldset { disabled: !editable, style: "display:grid;gap:8px;border:1px solid {theme.border_soft};border-radius:10px;padding:10px;min-width:0;",
                legend { "Target and weather" }
                // Dioxus honours `key` only on a template root, so each keyed block is
                // its own rsx!: Discard (draft_rev) then remounts its uncontrolled inputs.
                {rsx! { div { key: "target-{rev}", style: "display:flex;flex-wrap:wrap;gap:8px;align-items:end;",
                    label { r#for: "gs26-apogee-target", style: label_style, "Target apogee (AGL)"
                        input { id: "gs26-apogee-target", r#type: "text", inputmode: "decimal", value: "{target_text}", style: input_style,
                            oninput: move |e| target_text.set(e.value()) }
                    }
                    label { style: label_style, "Unit"
                        select { value: "{unit}", style: "font:inherit;min-height:36px;",
                            onchange: move |e| {
                                let next_unit = e.value();
                                let current_unit = draft.peek().as_ref().map(|d| d.display_unit.clone()).unwrap_or_default();
                                let typed = target_text.peek().clone();
                                if let Ok(m) = parse_altitude(&typed, &current_unit) {
                                    target_text.set(format!("{:.0}", to_unit(m, &next_unit)));
                                }
                                edit(&|s: &mut Settings| s.display_unit = next_unit.clone());
                            },
                            option { value: "ft", "ft" }
                            option { value: "m", "m" }
                        }
                    }
                    label { style: label_style, "Weather source"
                        select { value: "{d.weather_source}", style: "font:inherit;min-height:36px;",
                            onchange: move |e| { let v = e.value(); edit(&|s: &mut Settings| s.weather_source = v.clone()); },
                            option { value: "manual", "Manual entry" }
                            option { value: "open_meteo", "Open-Meteo forecast (online)" }
                            option { value: "cached", "Last cached forecast" }
                        }
                    }
                }}}
                if let Err(problem) = parsed_target.as_ref() {
                    p { role: "status", class: "gs-notice gs-notice--warn", style: "margin:0;", "{problem}" }
                }
                label { style: "display:flex;gap:8px;align-items:center;",
                    input { r#type: "checkbox", checked: d.online_weather_enabled, "data-online-weather": "",
                        onchange: move |e| { let on = e.checked(); edit(&|s: &mut Settings| s.online_weather_enabled = on); } }
                    "Allow online weather (sends the launch site to Open-Meteo)"
                }
                details { class: "gs-info", "data-online-preview": "",
                    summary { "What is sent" }
                    p { style: "margin:0;", "Exactly this request, with the launch site rounded to 0.01° (about 1 km). No key, station name or user is sent." }
                    code { style: "display:block;overflow-wrap:anywhere;font-size:12px;", "{s.online_request_preview}" }
                }
                {rsx! { div { key: "weather-{rev}", style: "display:flex;flex-wrap:wrap;gap:8px;align-items:end;",
                    for (key, label, value, set, optional) in fields {
                        label { key: "{key}", style: label_style, "{label}"
                            input { r#type: "text", inputmode: "decimal", initial_value: "{value}", style: input_style,
                                oninput: move |e| number_field(key, e.value(), set, optional) }
                        }
                    }
                }}}
                details {
                    summary { "Tank and limits" }
                    {rsx! { div { key: "limits-{rev}", style: "display:flex;flex-wrap:wrap;gap:8px;align-items:end;",
                        for (key, label, value, set, optional) in limit_fields {
                            label { key: "{key}", style: label_style, "{label}"
                                input { r#type: "text", inputmode: "decimal", initial_value: "{value}", style: input_style,
                                    placeholder: limit_placeholder(&value, optional),
                                    oninput: move |e| number_field(key, e.value(), set, optional) }
                            }
                        }
                    }}}
                }
                if admin {
                    div { style: "display:flex;flex-wrap:wrap;gap:8px;",
                        button { r#type: "button", class: "gs-btn gs-btn--primary", disabled: !dirty && parsed_target.as_ref().is_ok_and(|m| (*m - s.settings.target_apogee_m).abs() < 0.01) || busy(),
                            onclick: save, "Save planner settings" }
                        button { r#type: "button", class: "gs-btn", disabled: busy(),
                            onclick: move |_| {
                                draft.set(None);
                                invalid.set(Vec::new());
                                draft_rev += 1;
                            },
                            "Discard changes" }
                    }
                }
            }
            div { style: "display:flex;flex-wrap:wrap;gap:8px;align-items:center;",
                if show_fetch {
                    button { r#type: "button", class: "gs-btn", disabled: !can_send || busy(), onclick: fetch, "Fetch forecast" }
                }
                button { r#type: "button", class: "gs-btn gs-btn--primary", "data-compute": "", disabled: !can_send || busy() || dirty, onclick: compute, "Compute" }
                if admin {
                    button { r#type: "button", class: "gs-btn", "data-rehearse": "", disabled: busy(),
                        title: "Stations running simulated firmware only: sets the simulation's ambient temperature and wind to the plan's weather.",
                        onclick: rehearse, "Rehearse in firmware sim" }
                }
                if dirty {
                    span { style: "color:{theme.text_muted};", "Save or discard the settings changes before computing." }
                }
                if let Some(w) = s.weather_cache.as_ref() {
                    span { style: "color:{theme.text_muted};", "Cached forecast: {provider_label(&w.provider)}, fetched {hhmm_utc(w.fetched_ms)}" }
                }
            }
            if let Some(p) = plan.clone() {
                PlanPanel { theme: theme.clone(), plan: p, unit: unit.clone() }
            }
            div { style: "display:grid;gap:4px;",
                button { r#type: "button", class: "gs-btn gs-btn--primary", "data-apply": "", disabled: apply_reason.is_some() || busy(),
                    onclick: move |_| {
                        if let Some(p) = status.peek().as_ref().and_then(|s| s.as_ref().ok()).and_then(|s| s.last_plan.clone()) {
                            confirming.set(Some(p));
                        }
                    },
                    "Apply to fill targets…"
                }
                if let Some(reason) = apply_reason.clone() {
                    p { role: "status", "data-apply-disabled-reason": "", style: "margin:0;color:{theme.text_muted};", "{reason}" }
                }
            }
            if let Some(a) = s.applied.as_ref() {
                p { role: "status", "data-applied-plan": "", style: "margin:0;",
                    "Last applied: {a.new_targets.nitrous_target_mass_kg:.2} kg at {a.new_targets.nitrous_target_pressure_psi:.0} psi for {altitude_label(a.plan.target_apogee_m, &unit)}, {hhmm_utc(a.applied_ms)} by {a.applied_by.clone().unwrap_or_default()}."
                }
            }
            if let Some(pf) = s.post_flight.clone() {
                section { "data-post-flight": "", style: "display:grid;gap:6px;border:1px solid {theme.border_soft};border-radius:10px;padding:10px;",
                    h4 { style: "margin:0;", "Predicted vs actual apogee" }
                    p { style: "margin:0;",
                        "Predicted {altitude_label(pf.predicted_apogee_m, &unit)} (±1σ {band_label(pf.band_m, &unit)}); actual "
                        match pf.actual_apogee_m { Some(a) => altitude_label(a, &unit), None => "not reported yet".to_owned() }
                        if let Some(src) = pf.source.clone() { " ({src})" }
                        if let Some(err) = pf.error_m { ", error {to_unit(err, &unit):+.0} {ushort}" }
                        "."
                    }
                    div { style: "display:flex;flex-wrap:wrap;gap:8px;align-items:end;",
                        label { style: label_style, "Actual apogee override ({ushort}, optional)"
                            input { r#type: "text", inputmode: "decimal", value: "{actual_text}", style: input_style, oninput: move |e| actual_text.set(e.value()) }
                        }
                        button { r#type: "button", class: "gs-btn", disabled: !can_send || busy(), onclick: calibrate, "Add calibration point" }
                    }
                }
            }
            details { "data-motor-fit": "",
                summary { "Motor model: {s.motor_fit.c_ref_ns_per_kg:.0} N·s per kg fill, n = {s.motor_fit.n:.2}, ±{s.motor_fit.sigma_c_rel * 100.0:.0}% from {s.motor_fit.fires.len()} fire(s)" }
                p { style: "margin:0;color:{theme.text_muted};", "Impulse = c(P) × fill reading, fitted from the accepted fires in the fire archive (reference pressure {s.motor_fit.p_ref_psi:.0} psi). Drag scale from flights: {s.flight_calibration.cd_scale:.3} ({s.flight_calibration.points.len()} point(s))." }
                ul {
                    for f in s.motor_fit.fires.iter() {
                        li { key: "{f.id}", "{f.label}: {f.fill_kg:.2} kg, {f.impulse_ns / 1000.0:.2} kN·s, weight {f.weight:.2}" }
                    }
                }
                if !s.motor_fit.history.is_empty() {
                    table { style: "border-collapse:collapse;",
                        thead { tr { th { "Fit at" } th { "c (N·s/kg)" } th { "n" } th { "Fires" } } }
                        tbody {
                            for h in s.motor_fit.history.iter() {
                                tr { key: "{h.at_ms}", td { "{hhmm_utc(h.at_ms)}" } td { "{h.c_ref_ns_per_kg:.0}" } td { "{h.n:.2}" } td { "{h.fires}" } }
                            }
                        }
                    }
                }
            }
            details { "data-vehicle": "",
                summary { "Vehicle: {s.vehicle.name} ({vehicle_kind})" }
                p { style: "margin:0;", "Dry mass {s.vehicle.dry_mass_kg:.1} kg, length {s.vehicle.length_m:.2} m, diameter {s.vehicle.diameter_m * 1000.0:.0} mm, CNα {s.vehicle.cn_alpha_per_rad:.1}/rad, static margin {s.vehicle.static_margin_cal:.2} cal." }
                for w in s.vehicle.warnings.iter() {
                    p { class: "gs-notice gs-notice--warn", style: "margin:0;", "{w}" }
                }
                if admin {
                    div { style: "display:flex;flex-wrap:wrap;gap:8px;align-items:end;",
                        label { style: label_style, "Import OpenRocket .ork"
                            input { r#type: "file", accept: ".ork", disabled: busy(), onchange: upload_ork }
                        }
                        if s.vehicle.source == "ork" {
                            button { r#type: "button", class: "gs-btn", disabled: busy(), onclick: clear_ork, "Use default vehicle" }
                        }
                    }
                }
            }
            if let Some((ok, text)) = result() {
                p { role: if ok { "status" } else { "alert" }, class: if ok { "gs-notice" } else { "gs-notice gs-notice--danger" }, style: "margin:0;", "{text}" }
            }
        }
        if let (Some(p), Some(lines)) = (dialog_plan, dialog_lines) {
            super::igt_view::ConfirmDialog { id: "gs26-apogee-apply-confirm", title: "Apply the apogee plan to the fill targets?".to_string(), lines,
                confirm_label: "Apply to fill targets".to_string(),
                busy: busy(),
                on_cancel: move |_| confirming.set(None),
                on_confirm: move |_| apply(p.clone()),
            }
        }
    }
}

fn unit_for_calibration(status: &Signal<Option<Result<Status, String>>>) -> String {
    status.peek().as_ref().and_then(|s| s.as_ref().ok()).map(|s| s.settings.display_unit.clone()).unwrap_or_else(|| "ft".into())
}

#[component]
fn PlanPanel(theme: super::layout::ThemeConfig, plan: Plan, unit: String) -> Element {
    let rows = plan_rows(&plan, &unit);
    let problem = status_label(&plan, &unit);
    let rail_low = rail_exit_low(&plan);
    let limits = plan.limits.limit_sources.join("; ");
    rsx! {
        section { "data-plan": "{plan.status}", style: "display:grid;gap:6px;border:1px solid {theme.border_soft};border-radius:10px;padding:10px;min-width:0;",
            h4 { style: "margin:0;", "Plan for {altitude_label(plan.target_apogee_m, &unit)} (computed {hhmm_utc(plan.computed_ms)})" }
            if let Some(problem) = problem {
                p { role: "alert", "data-plan-unreachable": "", class: "gs-notice gs-notice--danger", style: "margin:0;", "{problem}" }
            }
            dl { style: "display:grid;grid-template-columns:max-content minmax(0,1fr);gap:4px 12px;margin:0;",
                for (label, value) in rows {
                    div { key: "{label}", style: "display:contents;",
                        dt { style: "font-weight:700;", "{label}" }
                        dd { style: "margin:0;overflow-wrap:anywhere;", "{value}" }
                    }
                }
            }
            if rail_low {
                p { role: "alert", "data-rail-exit-low": "", class: "gs-notice gs-notice--warn", style: "margin:0;",
                    "Rail exit speed {plan.rail_exit_mps:.1} m/s is below the {MIN_RAIL_EXIT_MPS} m/s minimum." }
            }
            if !plan.warnings.is_empty() {
                ul { "data-plan-warnings": "", style: "margin:0;",
                    for (i, w) in plan.warnings.iter().enumerate() {
                        li { key: "{i}", class: "gs-notice gs-notice--warn", "{w}" }
                    }
                }
            }
            if !plan.limits.limit_sources.is_empty() {
                p { style: "margin:0;color:{theme.text_muted};font-size:13px;", "Limits: {limits}" }
            }
        }
    }
}

/// One line beside the Actions nitrous target when it came from an applied plan.
#[component]
pub(super) fn AppliedPlanNote(theme: super::layout::ThemeConfig, nitrous_mass_kg: f64, nitrous_psi: f64) -> Element {
    let factory = super::station_settings_request::use_factory();
    let mut status = use_signal(|| None::<Status>);
    use_future(move || async move {
        loop {
            if let Ok(s) = factory.get::<Status>(PATH).await {
                status.set(Some(s));
            }
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(15_000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        }
    });
    let note = status().and_then(|s| {
        let unit = s.settings.display_unit.clone();
        s.applied.as_ref().and_then(|a| applied_note(a, nitrous_mass_kg, nitrous_psi, &unit))
    });
    rsx! {
        if let Some(note) = note {
            div { "data-applied-plan-note": "", style: "font-size:12px;color:{theme.text_muted};", "{note}" }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_save_round_trips_the_fields_this_editor_does_not_model() {
        let served = serde_json::json!({"target_apogee_m": 3048.0, "display_unit": "ft", "weather_source": "manual",
            "online_weather_enabled": false, "launch_time_ms": null, "manual_weather": {"surface_temp_c": 15.0, "wind_mps": 0.0, "wind_from_deg": 0.0, "gust_mps": 0.0},
            "tank_temp_c": null, "tank_volume_l": 15.8537, "min_ullage_frac": 0.15, "min_fill_kg": 3.0, "meop_psi": 750.0,
            "nominal_supercharge_psi": null, "motor_model": "scaling", "version": 1, "added_by_a_newer_station": {"x": 1}});
        let mut settings: Settings = serde_json::from_value(served.clone()).unwrap();
        settings.target_apogee_m = 2500.0;
        let saved = serde_json::to_value(&settings).unwrap();
        assert_eq!(saved["motor_model"], "scaling", "not reset by a save");
        assert_eq!(saved["version"], 1, "never written as 0");
        assert_eq!(saved["added_by_a_newer_station"], served["added_by_a_newer_station"]);
        assert_eq!(saved["tank_volume_l"], 15.8537);
        assert_eq!(saved["target_apogee_m"], 2500.0);
    }

    use super::*;
    use serde_json::json;

    pub(crate) fn plan_fixture(status: &str) -> Plan {
        serde_json::from_value(json!({
            "id": "plan-1", "computed_ms": 1_795_000_000_000i64, "status": status,
            "target_apogee_m": 3048.0, "fill_mass_kg": 6.4, "supercharge_psi": 700.0,
            "apogee_m": 3050.0, "apogee_band_m": [2800.0, 3300.0], "apogee_sigma_m": 250.0,
            "max_achievable_apogee_m": 3300.0, "min_achievable_apogee_m": 1200.0,
            "rail_exit_mps": 28.0, "max_mach": 0.71, "time_to_apogee_s": 25.0, "burnout_s": 6.0,
            "drift_m": 900.0, "drift_bearing_deg": 120.0, "impulse_ns": 10400.0, "impulse_sigma_ns": 1000.0,
            "fill_band_kg": [5.9, 7.1],
            "limits": {"max_fill_kg": 8.0, "min_fill_kg": 3.0, "max_supercharge_psi": 750.0, "vapour_pressure_psi": 730.0, "limit_sources": ["MEOP 750 psi"]},
            "warnings": ["Forecast is 7 h old."],
            "weather": {"provider": "open_meteo", "surface_temp_c": 21.0, "surface_pressure_pa": 91000.0, "wind_mps": 4.0, "wind_from_deg": 300.0, "gust_mps": 6.0},
            "weather_age_s": 1800.0, "vehicle_source": "default", "motor_fit": {"c_ref_ns_per_kg": 1627.0, "n": 0.3, "sigma_c_rel": 0.1, "fires": 1},
            "current_targets": {"nitrous_target_mass_kg": 8.0, "nitrous_target_pressure_psi": 750.0}
        }))
        .unwrap()
    }

    fn status_fixture(plan: Option<Plan>, gate: (bool, Option<&str>)) -> Status {
        let mut s: Status = serde_json::from_value(json!({
            "settings": {"target_apogee_m": 3048.0, "display_unit": "ft", "weather_source": "open_meteo", "online_weather_enabled": true},
            "vehicle": {"source": "default", "name": "6 in x 15 ft (assumed)", "dry_mass_kg": 31.6},
            "motor_fit": {"c_ref_ns_per_kg": 1627.0, "n": 0.3, "sigma_c_rel": 0.1, "p_ref_psi": 350.0,
                "fires": [{"id": "2026-11-16_static", "label": "2026-11-16 static fire", "fill_kg": 5.65, "impulse_ns": 9192.0, "weight": 1.0}]},
            "online_request_preview": "https://api.open-meteo.com/v1/forecast?latitude=31.76&longitude=-102.05"
        }))
        .unwrap();
        s.last_plan = plan;
        s.apply_gate = ApplyGate { allowed: gate.0, reason: gate.1.map(str::to_owned) };
        s
    }

    #[test]
    fn omitted_tank_and_limit_settings_are_not_set_rather_than_invented() {
        let empty: Settings = serde_json::from_value(json!({})).unwrap();
        assert_eq!((empty.tank_volume_l, empty.min_ullage_frac, empty.min_fill_kg, empty.meop_psi), (None, None, None, None));
        assert_eq!(opt_text(empty.tank_volume_l), "");
        assert_eq!(limit_placeholder(&opt_text(empty.tank_volume_l), false), "not set");
        assert_eq!(limit_placeholder("", true), "", "optional overrides explain themselves");
        // Saving leaves them out, so the station keeps its own values.
        let saved = serde_json::to_value(&empty).unwrap();
        for field in ["tank_volume_l", "min_ullage_frac", "min_fill_kg", "meop_psi"] {
            assert!(saved.get(field).is_none(), "{field} must not be sent when not set");
        }
        // Values the station sends are shown and saved unchanged.
        let station: Settings = serde_json::from_value(json!({"tank_volume_l": 15.8537, "min_ullage_frac": 0.15, "min_fill_kg": 3.0, "meop_psi": 750.0})).unwrap();
        assert_eq!(station.tank_volume_l, Some(15.8537));
        assert_eq!(opt_text(station.tank_volume_l), "15.8537");
        assert_eq!(serde_json::to_value(&station).unwrap()["meop_psi"], 750.0);
    }

    #[test]
    fn status_decodes_leniently() {
        let empty: Status = serde_json::from_value(json!({})).unwrap();
        assert_eq!(empty.settings, Settings::default());
        assert!(empty.last_plan.is_none());
        assert_eq!(empty.flight_calibration.cd_scale, 1.0);
        let s = status_fixture(Some(plan_fixture("ok")), (true, None));
        assert_eq!(s.motor_fit.fires.len(), 1);
        assert_eq!(s.last_plan.unwrap().apogee_band_m, [2800.0, 3300.0]);
    }

    #[test]
    fn units_convert_and_validate() {
        assert!((parse_altitude("10000", "ft").unwrap() - 3048.0).abs() < 1e-9);
        assert_eq!(parse_altitude(" 3000 ", "m").unwrap(), 3000.0);
        assert!(parse_altitude("0", "ft").is_err());
        assert!(parse_altitude("-5", "m").is_err());
        assert!(parse_altitude("abc", "m").is_err());
        assert!(parse_altitude("NaN", "m").is_err());
        assert!(parse_altitude("200000", "ft").is_err());
        assert_eq!(altitude_label(3048.0, "ft"), "10000 ft");
        assert_eq!(altitude_label(3048.0, "m"), "3048 m");
        assert_eq!(band_label([2800.0, 3300.0], "m"), "2800–3300 m");
        assert_eq!(parse_optional(""), Ok(None));
        assert_eq!(parse_optional("2.5"), Ok(Some(2.5)));
        assert!(parse_optional("x").is_err());
        assert!(parse_optional("inf").is_err());
    }

    #[test]
    fn labels() {
        assert_eq!(age_label(30.0), "30 s old");
        assert_eq!(age_label(600.0), "10 min old");
        assert_eq!(age_label(7.0 * 3600.0), "7.0 h old");
        assert_eq!(age_label(-1.0), "unknown age");
        assert_eq!(hhmm_utc(0), "00:00 UTC");
        assert_eq!(hhmm_utc(13 * 3_600_000 + 5 * 60_000), "13:05 UTC");
        assert_eq!(provider_label("open_meteo"), "Open-Meteo forecast");
        let high = plan_fixture("unreachable_high");
        assert!(status_label(&high, "ft").unwrap().contains("10827 ft"));
        assert!(status_label(&plan_fixture("ok"), "ft").is_none());
        assert!(status_label(&plan_fixture("unreachable_low"), "m").unwrap().contains("1200 m"));
        assert!(rail_exit_low(&plan_fixture("ok")));
    }

    #[test]
    fn apply_gate_reason_comes_from_the_station() {
        let plan = plan_fixture("ok");
        let refused = status_fixture(Some(plan.clone()), (false, Some("Test fire mode: plans are not applied to fill targets.")));
        assert_eq!(apply_disabled_reason(&refused, refused.last_plan.as_ref(), true).unwrap(), "Test fire mode: plans are not applied to fill targets.");
        let ok = status_fixture(Some(plan.clone()), (true, None));
        assert_eq!(apply_disabled_reason(&ok, ok.last_plan.as_ref(), true), None);
        assert!(apply_disabled_reason(&ok, ok.last_plan.as_ref(), false).unwrap().contains("permission"));
        assert!(apply_disabled_reason(&ok, None, true).unwrap().contains("Compute"));
        let high = status_fixture(Some(plan_fixture("unreachable_high")), (true, None));
        assert!(apply_disabled_reason(&high, high.last_plan.as_ref(), true).unwrap().contains("does not reach"));
    }

    #[test]
    fn confirm_lists_old_and_new_targets_and_inputs() {
        let lines = confirm_lines(&plan_fixture("ok"), "ft");
        assert_eq!(lines[0], "Nitrous fill mass target: 8.00 kg → 6.40 kg.");
        assert_eq!(lines[1], "Supercharge pressure target: 750 psi → 700 psi.");
        assert!(lines[2].contains("10000 ft") && lines[2].contains("9186–10827 ft"), "{}", lines[2]);
        assert!(lines[3].contains("Open-Meteo") && lines[3].contains("30 min old"));
        assert!(lines.last().unwrap().contains("Nothing is actuated"));
        assert_eq!(serde_json::to_value(ApplyRequest { plan_id: "plan-1".into() }).unwrap(), json!({"plan_id": "plan-1"}));
        assert_eq!(serde_json::to_value(CalibrateRequest { actual_apogee_m: None }).unwrap(), json!({"actual_apogee_m": null}));
    }

    #[test]
    fn applied_note_only_while_targets_match() {
        let applied = Applied {
            plan: plan_fixture("ok"),
            applied_ms: 14 * 3_600_000 + 30 * 60_000,
            applied_by: Some("rylan".into()),
            new_targets: Targets { nitrous_target_mass_kg: 6.4, nitrous_target_pressure_psi: 700.0 },
            ..Default::default()
        };
        assert_eq!(applied_note(&applied, 6.4, 700.0, "ft").unwrap(), "Set by apogee plan for 10000 ft at 14:30 UTC by rylan");
        assert!(applied_note(&applied, 6.5, 700.0, "ft").is_none());
        assert!(applied_note(&applied, 6.4, 650.0, "ft").is_none());
    }

    fn render(admin: bool, can_send: bool, status: Status) -> String {
        let mut dom = VirtualDom::new_with_props(
            |(admin, can_send, status): (bool, bool, Status)| rsx! {
                ApogeePlannerView { theme: super::super::layout::ThemeConfig::default(), admin, can_send, initial: Some(status) }
            },
            (admin, can_send, status),
        );
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn admin_edits_operators_read_only_but_compute() {
        let s = status_fixture(Some(plan_fixture("ok")), (true, None));
        let admin = render(true, true, s.clone());
        assert!(admin.contains("Save planner settings") && admin.contains("Import OpenRocket"), "{admin}");
        assert!(admin.contains("Fetch forecast"));
        assert!(admin.contains("api.open-meteo.com/v1/forecast?latitude=31.76"));
        let operator = render(false, true, s.clone());
        assert!(operator.contains("Read only: an administrator"), "{operator}");
        assert!(!operator.contains("Save planner settings") && !operator.contains("Import OpenRocket"));
        assert!(operator.contains("data-compute"));
        assert!(admin.contains("data-rehearse") && !operator.contains("data-rehearse"), "the firmware-sim rehearsal is admin only");
        let viewer = render(false, false, s);
        assert!(viewer.contains("Command permission is required"));
    }

    #[test]
    fn apply_disabled_with_station_reason_and_plan_shown() {
        let reason = "Integrated ground test fill already started (flight state NitrousFill).";
        let page = render(true, true, status_fixture(Some(plan_fixture("ok")), (false, Some(reason))));
        assert!(page.contains(reason), "{page}");
        assert!(page.contains("6.40 kg") && page.contains("700 psi"));
        assert!(page.contains("data-rail-exit-low"));
        assert!(page.contains("Forecast is 7 h old."));
        let ok = render(true, true, status_fixture(Some(plan_fixture("ok")), (true, None)));
        assert!(!ok.contains("data-apply-disabled-reason"), "{ok}");
    }

    #[test]
    fn unreachable_plan_shows_max_and_blocks_apply() {
        let page = render(true, true, status_fixture(Some(plan_fixture("unreachable_high")), (true, None)));
        assert!(page.contains("data-plan-unreachable") && page.contains("10827 ft"), "{page}");
        assert!(page.contains("does not reach the target"));
    }
}
