//! Loss-of-signal (LOS) prediction (flight-predict contract): when the
//! rocket goes quiet in flight the station predicts where it is now and where
//! it lands. The Map tab polls `GET /api/los_prediction`, draws the station's
//! overlay (predicted path, position, landing point and uncertainty areas)
//! beside the real track and last fix, and shows a "Predicted: no signal"
//! card with bearing and distance from the viewer. The prediction is the
//! station's; the UI only measures from the viewer's own position.
use super::layout::ThemeConfig;
use super::map_overlays::{self, MapOverlay};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub(crate) const PATH: &str = "/api/los_prediction";
pub(crate) const SETTINGS_PATH: &str = "/api/los_prediction/settings";
pub(crate) const SIMULATE_PATH: &str = "/api/los_prediction/simulate";
const POLL_MS: u64 = 2_000;
/// An older station without the route: ask rarely.
const ABSENT_POLL_MS: u64 = 30_000;
pub(crate) const MIN_TIMEOUT_S: f64 = 2.0;
pub(crate) const MAX_TIMEOUT_S: f64 = 60.0;
/// Overlay provider of a simulated cut in the 3D replay (kept apart from the
/// live prediction so the live poller never clears it).
pub(crate) const PROVIDER_REPLAY_LOS: &str = "los_replay";

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub(crate) struct Settings {
    pub enabled: bool,
    pub timeout_s: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self { enabled: true, timeout_s: 5.0 }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Fix {
    pub t_ms: i64,
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_msl_m: Option<f64>,
    pub alt_agl_m: Option<f64>,
    pub ve_mps: Option<f64>,
    pub vn_mps: Option<f64>,
    pub vu_mps: Option<f64>,
    pub source: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Predicted {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_msl_m: Option<f64>,
    pub alt_agl_m: Option<f64>,
    pub sigma_m: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Landing {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_msl_m: Option<f64>,
    pub t_ms: Option<i64>,
    pub in_s: Option<f64>,
    pub semi_major_m: Option<f64>,
    pub semi_minor_m: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct LatLon {
    pub lat_deg: f64,
    pub lon_deg: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct FromStation {
    pub bearing_deg: f64,
    pub distance_m: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Comparison {
    pub resolved_ms: i64,
    pub resolved_by: String,
    pub predicted: Option<Predicted>,
    pub actual: Option<Predicted>,
    pub error_m: Option<f64>,
    pub since_fix_s: Option<f64>,
}

/// `GET /api/los_prediction` (and the simulate reply).
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct LosState {
    pub state: String,
    pub label: String,
    pub advisory: bool,
    pub settings: Settings,
    pub last_fix: Option<Fix>,
    pub signal_lost_ms: Option<i64>,
    pub since_fix_s: Option<f64>,
    pub phase: Option<String>,
    pub predicted_now: Option<Predicted>,
    pub landing: Option<Landing>,
    pub station: Option<LatLon>,
    pub from_station: Option<FromStation>,
    pub wind_source: Option<String>,
    pub overlay: Option<MapOverlay>,
    pub comparison: Option<Comparison>,
    pub history: Vec<Comparison>,
}

impl LosState {
    /// Predicting or resolved: the card and the overlay are shown.
    pub(crate) fn shows_prediction(&self) -> bool {
        matches!(self.state.as_str(), "predicting" | "resolved")
    }
    pub(crate) fn label(&self) -> String {
        if self.label.trim().is_empty() { "Predicted: no signal".into() } else { self.label.clone() }
    }
    pub(crate) fn map_overlay(&self) -> Option<&MapOverlay> {
        self.overlay.as_ref().filter(|o| self.shows_prediction() && !o.is_empty())
    }
}

// ------------------------------------------------------------ text

fn valid(lat: f64, lon: f64) -> bool {
    lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0
}

/// Initial great-circle bearing, degrees from true north in [0, 360).
pub(crate) fn bearing_deg(from: (f64, f64), to: (f64, f64)) -> Option<f64> {
    if !valid(from.0, from.1) || !valid(to.0, to.1) || super::map_tab::haversine_meters(from.0, from.1, to.0, to.1) < 1.0 {
        return None;
    }
    let (p1, p2) = (from.0.to_radians(), to.0.to_radians());
    let dl = (to.1 - from.1).to_radians();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    Some(y.atan2(x).to_degrees().rem_euclid(360.0))
}

pub(crate) fn cardinal(deg: f64) -> &'static str {
    const NAMES: [&str; 16] = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];
    NAMES[((deg.rem_euclid(360.0) + 11.25) / 22.5) as usize % 16]
}

pub(crate) fn distance_text(m: f64, metric: bool) -> String {
    if !m.is_finite() || m < 0.0 {
        return "—".into();
    }
    if metric {
        if m < 999.5 { format!("{m:.0} m") } else { format!("{:.2} km", m / 1000.0) }
    } else {
        let ft = m * 3.280_839_895;
        if ft < 999.5 { format!("{ft:.0} ft") } else { format!("{:.2} mi", ft / 5280.0) }
    }
}

pub(crate) fn altitude_text(m: Option<f64>, metric: bool) -> String {
    match m.filter(|m| m.is_finite()) {
        Some(m) if metric => format!("{m:.0} m"),
        Some(m) => format!("{:.0} ft", m * 3.280_839_895),
        None => "—".into(),
    }
}

/// "1.21 km at 70° ENE from you", or from the station when the viewer's
/// position is unknown (the station's own figure), or None.
pub(crate) fn direction_text(viewer: Option<(f64, f64)>, target: (f64, f64), station: Option<&FromStation>, metric: bool) -> Option<String> {
    if let Some(v) = viewer.filter(|v| valid(v.0, v.1)) {
        let d = super::map_tab::haversine_meters(v.0, v.1, target.0, target.1);
        return Some(match bearing_deg(v, target) {
            Some(b) => format!("{} at {:.0}° {} from you", distance_text(d, metric), b, cardinal(b)),
            None => format!("{} from you", distance_text(d, metric)),
        });
    }
    station.filter(|s| s.distance_m.is_finite()).map(|s| format!("{} at {:.0}° {} from the station", distance_text(s.distance_m, metric), s.bearing_deg, cardinal(s.bearing_deg)))
}

pub(crate) fn seconds_text(s: f64) -> String {
    if !s.is_finite() || s < 0.0 {
        return "—".into();
    }
    let s = s.round() as i64;
    if s < 60 { format!("{s} s") } else { format!("{} min {:02} s", s / 60, s % 60) }
}

pub(crate) fn phase_label(phase: &str) -> &'static str {
    match phase {
        "pad" => "on the pad",
        "boost" => "powered ascent",
        "coast" => "coasting",
        "drogue" => "drogue descent",
        "main" => "main descent",
        "landed" => "landed",
        _ => "unknown phase",
    }
}

/// The card rows: (label, value).
pub(crate) fn card_rows(s: &LosState, viewer: Option<(f64, f64)>, metric: bool) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if let Some(t) = s.since_fix_s {
        let source = s.last_fix.as_ref().and_then(|f| f.source.clone()).map(|src| format!(" ({src})")).unwrap_or_default();
        rows.push(("Since last fix".to_owned(), format!("{}{source}", seconds_text(t))));
    }
    if let Some(p) = &s.predicted_now {
        let phase = s.phase.as_deref().map(phase_label).unwrap_or("unknown phase");
        rows.push(("Predicted now".to_owned(), format!("{} above the pad, {phase}", altitude_text(p.alt_agl_m, metric))));
        // `from_station` is the station's bearing to the predicted position.
        if let Some(text) = direction_text(viewer, (p.lat_deg, p.lon_deg), s.from_station.as_ref(), metric) {
            let sigma = p.sigma_m.map(|m| format!(" (±{})", distance_text(m, metric))).unwrap_or_default();
            rows.push(("Predicted position".to_owned(), format!("{text}{sigma}")));
        }
    }
    if let Some(l) = &s.landing {
        if let Some(t) = l.in_s {
            rows.push(("Time to landing".to_owned(), seconds_text(t)));
        }
        let area = match (l.semi_major_m, l.semi_minor_m) {
            (Some(a), Some(b)) => format!(" (area {} × {})", distance_text(a * 2.0, metric), distance_text(b * 2.0, metric)),
            _ => String::new(),
        };
        // The station's own bearing is to the predicted position; for the
        // landing point it is only a fallback when nothing else is known.
        if let Some(text) = direction_text(viewer, (l.lat_deg, l.lon_deg), None, metric) {
            rows.push(("Predicted landing".to_owned(), format!("{text}{area}")));
        } else {
            rows.push(("Predicted landing".to_owned(), format!("{:.5}°, {:.5}°{area}", l.lat_deg, l.lon_deg)));
        }
    }
    if viewer.is_none() && s.from_station.is_some() {
        rows.push(("Your position".to_owned(), "unknown; bearings are from the station".to_owned()));
    }
    if let Some(c) = &s.comparison {
        let by = if c.resolved_by == "gps_fix" { "a GPS fix" } else { "the signal" };
        rows.push(("Resolved".to_owned(), format!("{by} came back; the prediction was off by {}", c.error_m.map(|m| distance_text(m, metric)).unwrap_or_else(|| "—".into()))));
    }
    rows
}

// ------------------------------------------------------------ replay simulation

/// A saved fix of the replayed recording: `(t_ms, lat, lon, alt_msl)`.
pub(crate) type TrackPoint = (i64, f64, f64, f64);

/// The contract phase of a station FlightState for a simulated cut, or an
/// explanation when the rocket is not in flight there.
pub(crate) fn simulate_phase(flight_state: &str) -> Result<Option<&'static str>, &'static str> {
    match flight_state {
        "Launch" | "Ascent" | "InFlight" => Ok(Some("boost")),
        "Coast" | "Apogee" => Ok(Some("coast")),
        "Descent" => Ok(Some("drogue")),
        "Reefing" => Ok(Some("main")),
        "Landed" | "Recovery" => Err("The rocket has landed at this point of the replay."),
        "Aborted" | "Unknown" | "" => Ok(None),
        _ => Err("The rocket is not in flight at this point of the replay."),
    }
}

/// `POST /api/los_prediction/simulate` body for a cut at `at_ms`: the last
/// fix at or before it, its velocity from the previous fix at least 0.5 s
/// earlier (none if there is none), the phase and the recording's first fix
/// as the site.
pub(crate) fn simulate_body(track: &[TrackPoint], at_ms: i64, flight_state: &str, elapsed_s: f64) -> Result<serde_json::Value, String> {
    let phase = simulate_phase(flight_state).map_err(str::to_string)?;
    let n = track.partition_point(|p| p.0 <= at_ms);
    let last = *track.get(n.wrapping_sub(1)).filter(|_| n > 0).ok_or("No GPS fix before this point of the replay.")?;
    let previous = track[..n - 1].iter().rev().find(|p| last.0 - p.0 >= 500).copied();
    let mut fix = serde_json::json!({"t_ms": last.0, "lat_deg": last.1, "lon_deg": last.2, "alt_msl_m": last.3});
    if let Some(p) = previous {
        let dt = (last.0 - p.0) as f64 / 1000.0;
        let m_per_deg = 111_320.0;
        fix["ve_mps"] = ((last.2 - p.2) * m_per_deg * last.1.to_radians().cos() / dt).into();
        fix["vn_mps"] = ((last.1 - p.1) * m_per_deg / dt).into();
        fix["vu_mps"] = ((last.3 - p.3) / dt).into();
    }
    let site = track.first().map(|p| serde_json::json!({"lat_deg": p.1, "lon_deg": p.2, "alt_msl_m": p.3}));
    Ok(serde_json::json!({"last_fix": fix, "phase": phase, "elapsed_s": elapsed_s, "site": site}))
}

/// JS that sets the 3D replay's `data-overlays` (contract: a JSON array of
/// MapOverlay drawn on the ground of the replay scene).
pub(crate) fn viewer_overlays_js(element_id: &str, overlays: &[MapOverlay]) -> String {
    let id = serde_json::to_string(element_id).unwrap_or_default();
    let attr = serde_json::to_string(&serde_json::to_string(overlays).unwrap_or_else(|_| "[]".into())).unwrap_or_default();
    format!("(()=>{{const m=document.getElementById({id});if(m)m.setAttribute('data-overlays',{attr});}})();")
}

async fn sleep_ms(ms: u64) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

// ------------------------------------------------------------ views

fn card(theme: &ThemeConfig, s: &LosState, viewer: Option<(f64, f64)>, metric: bool) -> Element {
    let rows = card_rows(s, viewer, metric);
    let resolved = s.state == "resolved";
    let border = if resolved { theme.border.clone() } else { "#f472b6".to_string() };
    let title = if resolved { format!("{} — resolved", s.label()) } else { s.label() };
    rsx! {
        div { class: "gs26-los-card", role: "status", "aria-live": "polite", "data-los-state": "{s.state}",
            style: "margin:0 8px;display:grid;gap:4px;padding:8px 10px;border-radius:10px;border:2px dashed {border};background:rgba(244,114,182,0.10);color:{theme.text_primary};font-size:0.84rem;line-height:1.25;min-width:0;",
            div { style: "display:flex;flex-wrap:wrap;gap:6px 10px;align-items:center;",
                strong { style: "color:#f472b6;font-size:0.95rem;", "{title}" }
                if s.advisory { span { style: "color:{theme.text_muted};font-size:0.78rem;", "advisory: a prediction, not a position fix" } }
            }
            dl { style: "display:grid;grid-template-columns:minmax(0,max-content) minmax(0,1fr);gap:2px 10px;margin:0;",
                for (k, v) in rows {
                    dt { style: "color:{theme.text_muted};", "{k}" }
                    dd { style: "margin:0;overflow-wrap:anywhere;font-variant-numeric:tabular-nums;", "{v}" }
                }
            }
        }
    }
}

/// Polls the live prediction while the map is mounted, draws its overlay on
/// the map and shows the card. `viewer` is the viewer's own position.
#[component]
pub(super) fn LosMapCard(
    theme: ThemeConfig,
    viewer: Option<(f64, f64)>,
    #[props(default = false)] metric: bool,
    /// A seeded state (tests and previews); None polls the station.
    #[props(default)]
    initial: Option<LosState>,
) -> Element {
    let seeded = initial.is_some();
    let mut state = use_signal(move || initial.clone());
    use_drop(|| map_overlays::set(map_overlays::PROVIDER_LOS, &[]));
    use_future(move || async move {
        if seeded {
            return;
        }
        let mut drawn: Option<MapOverlay> = None;
        loop {
            let mut wait = POLL_MS;
            if super::dashboard_page_visible() {
                match super::http_get_json::<LosState>(PATH).await {
                    Ok(next) => {
                        let overlay = next.map_overlay().cloned();
                        if overlay != drawn {
                            map_overlays::set(map_overlays::PROVIDER_LOS, overlay.as_slice());
                            drawn = overlay;
                        }
                        if state.peek().as_ref() != Some(&next) {
                            state.set(Some(next));
                        }
                    }
                    Err(e) => {
                        if e.contains("404") {
                            wait = ABSENT_POLL_MS;
                        }
                    }
                }
            }
            sleep_ms(wait).await;
        }
    });
    // Seeded states draw once.
    use_effect(move || {
        if seeded && let Some(overlay) = state.read().as_ref().and_then(|s| s.map_overlay().cloned()) {
            map_overlays::set(map_overlays::PROVIDER_LOS, &[overlay]);
        }
    });
    let current = state.read().clone();
    match current {
        Some(s) if s.shows_prediction() => card(&theme, &s, viewer, metric),
        _ => rsx! {},
    }
}

/// Administrator control for the prediction (`PUT /api/los_prediction/settings`).
#[component]
pub(super) fn LosSettingsPanel(theme: ThemeConfig) -> Element {
    let admin = crate::auth::is_admin();
    let factory = super::station_settings_request::use_factory();
    let mut current = use_signal(|| None::<Result<LosState, String>>);
    let mut enabled = use_signal(|| None::<bool>);
    let mut timeout = use_signal(String::new);
    let mut result = use_signal(|| None::<(bool, String)>);
    let mut busy = use_signal(|| false);
    use_future(move || async move {
        let fetched = factory.get::<LosState>(PATH).await;
        if let Ok(s) = &fetched {
            enabled.set(Some(s.settings.enabled));
            timeout.set(format!("{}", s.settings.timeout_s));
        }
        current.set(Some(fetched));
    });
    let Some(Ok(s)) = current() else {
        return match current() {
            Some(Err(e)) if e.contains("404") => rsx! {},
            Some(Err(e)) => rsx! { section { h3 { style: "margin:0;", "Loss-of-signal prediction" } p { role: "status", class: "gs-notice gs-notice--warn", "Cannot read the prediction settings: {super::igt::refusal(&e)}" } } },
            _ => rsx! {},
        };
    };
    let on = enabled().unwrap_or(s.settings.enabled);
    let parsed = timeout.read().trim().parse::<f64>().ok().filter(|t| t.is_finite() && (MIN_TIMEOUT_S..=MAX_TIMEOUT_S).contains(t));
    let save = move |_| {
        let Some(timeout_s) = timeout.peek().trim().parse::<f64>().ok().filter(|t| (MIN_TIMEOUT_S..=MAX_TIMEOUT_S).contains(t)) else {
            result.set(Some((false, format!("The timeout must be {MIN_TIMEOUT_S:.0}–{MAX_TIMEOUT_S:.0} s."))));
            return;
        };
        let body = Settings { enabled: enabled.peek().unwrap_or(true), timeout_s };
        let owner = factory.capture();
        let work = super::station_settings_request::put_captured::<_, serde_json::Value>(SETTINGS_PATH, &body, owner);
        busy.set(true);
        spawn(async move {
            let reply = work.await;
            busy.set(false);
            result.set(Some(match reply {
                Ok(_) => (true, "Prediction settings saved.".into()),
                Err(e) => (false, format!("The station did not save them: {}", super::igt::refusal(&e))),
            }));
        });
    };
    rsx! {
        section { "data-los-settings": "", style: "display:grid;gap:8px;min-width:0;",
            h3 { style: "margin:0;", "Loss-of-signal prediction" }
            p { style: "margin:0;color:{theme.text_muted};",
                "When no rocket packet arrives for the timeout during a flight, the station predicts the rocket's position and landing point from the last fix and the wind, and the Map tab and watches show it as “Predicted: no signal”. It is advisory and never replaces a real fix."
            }
            if !admin {
                p { style: "margin:0;color:{theme.text_muted};", "data-los-settings-read-only": "",
                    {format!("Prediction {}, after {} s without signal. An administrator changes this.", if s.settings.enabled { "on" } else { "off" }, s.settings.timeout_s)}
                }
            } else {
                div { style: "display:flex;flex-wrap:wrap;gap:12px;align-items:end;",
                    label { style: "display:flex;gap:8px;align-items:center;",
                        input { r#type: "checkbox", checked: on, disabled: busy(), onchange: move |e| enabled.set(Some(e.checked())) }
                        "Predict when the signal is lost"
                    }
                    label { style: "display:grid;gap:2px;font-weight:700;", "Timeout without signal (s, {MIN_TIMEOUT_S:.0}–{MAX_TIMEOUT_S:.0})"
                        input { r#type: "text", inputmode: "decimal", value: "{timeout}", disabled: busy(), style: "font:inherit;min-height:36px;width:8ch;padding:4px 8px;",
                            oninput: move |e| timeout.set(e.value()) }
                    }
                    button { r#type: "button", class: "gs-btn gs-btn--primary", disabled: busy() || parsed.is_none(), onclick: save, "Save" }
                }
            }
            if let Some((ok, message)) = result() {
                p { role: "status", class: if ok { "gs-notice" } else { "gs-notice gs-notice--warn" }, "{message}" }
            }
        }
    }
}

/// 3D replay tools on a connected station: compare the recording with a
/// stored flight plan and simulate a signal loss at the replay position.
#[component]
pub(super) fn ReplayPredictionTools(
    theme: ThemeConfig,
    /// The `<gs-vehicle-viewer>` element id that gets `data-overlays`.
    viewer_element: String,
    track: Vec<TrackPoint>,
    at_ms: i64,
    flight_state: String,
) -> Element {
    let factory = super::station_settings_request::use_factory();
    let mut plans = use_signal(|| None::<Result<super::flight_plans::PlanList, String>>);
    let mut plan = use_signal(|| None::<super::flight_plans::PlanDoc>);
    let mut simulated = use_signal(|| None::<LosState>);
    let mut message = use_signal(|| None::<(bool, String)>);
    let mut busy = use_signal(|| false);
    let mut elapsed = use_signal(|| "30".to_string());
    use_future(move || async move {
        plans.set(Some(factory.get::<super::flight_plans::PlanList>(super::flight_plans::LIST_PATH).await));
    });
    use_drop(|| map_overlays::set(PROVIDER_REPLAY_LOS, &[]));
    {
        let viewer_element = viewer_element.clone();
        use_effect(move || {
            let mut overlays: Vec<MapOverlay> = Vec::new();
            overlays.extend(plan.read().as_ref().and_then(|p| p.map_overlay().cloned()));
            overlays.extend(simulated.read().as_ref().and_then(|s| s.overlay.clone()));
            super::js_eval(&viewer_overlays_js(&viewer_element, &overlays));
        });
    }
    let reason = match simulate_body(&track, at_ms, &flight_state, 30.0) {
        Ok(_) => None,
        Err(e) => Some(e),
    };
    let mut choose = move |id: String| {
        if id.is_empty() {
            plan.set(None);
            return;
        }
        let work = factory.get::<super::flight_plans::PlanDoc>(&super::flight_plans::plan_path(&id));
        spawn(async move {
            match work.await {
                Ok(p) => {
                    let _ = super::flight_plans_view::show_on_map(&p);
                    plan.set(Some(p));
                }
                Err(e) => message.set(Some((false, format!("Cannot read the plan: {}", super::igt::refusal(&e))))),
            }
        });
    };
    let simulate = {
        let track = track.clone();
        let flight_state = flight_state.clone();
        move |_| {
            let secs = elapsed.peek().trim().parse::<f64>().ok().filter(|s| s.is_finite() && *s >= 0.0).unwrap_or(30.0);
            let body = match simulate_body(&track, at_ms, &flight_state, secs) {
                Ok(b) => b,
                Err(e) => {
                    message.set(Some((false, e)));
                    return;
                }
            };
            let owner = factory.capture();
            let work = super::station_settings_request::post_captured::<_, LosState>(SIMULATE_PATH, &body, owner);
            busy.set(true);
            message.set(None);
            spawn(async move {
                let reply = work.await;
                busy.set(false);
                match reply {
                    Ok(s) => {
                        map_overlays::set(PROVIDER_REPLAY_LOS, s.overlay.as_slice());
                        message.set(Some((true, "Simulated signal loss: the prediction is drawn on the 3D replay and the Map tab.".into())));
                        simulated.set(Some(s));
                    }
                    Err(e) => message.set(Some((false, format!("The station did not simulate it: {}", super::igt::refusal(&e))))),
                }
            });
        }
    };
    let choices: Vec<(String, String)> = match plans() {
        Some(Ok(l)) => l.plans.iter().filter(|p| p.kind != "burn").map(|p| (p.id.clone(), super::flight_plans::predicted_label(&p.label))).collect(),
        _ => Vec::new(),
    };
    let chosen = plan.read().as_ref().map(|p| p.id.clone()).unwrap_or_default();
    rsx! {
        div { class: "gs26-replay-prediction", "data-replay-prediction": "", style: "display:grid;gap:8px;margin:8px 0;padding:10px;border:1px solid {theme.border_soft};border-radius:12px;",
            div { style: "display:flex;flex-wrap:wrap;gap:8px;align-items:end;",
                if !choices.is_empty() {
                    label { style: "display:grid;gap:2px;", "Compare with a flight plan"
                        select { value: "{chosen}", style: "font:inherit;min-height:44px;", onchange: move |e| choose(e.value()),
                            option { value: "", "None" }
                            for (id, label) in choices { option { value: "{id}", "{label}" } }
                        }
                    }
                }
                label { style: "display:grid;gap:2px;", "Predict ahead (s)"
                    input { r#type: "text", inputmode: "decimal", value: "{elapsed}", style: "font:inherit;min-height:44px;width:7ch;padding:4px 8px;", oninput: move |e| elapsed.set(e.value()) }
                }
                button { r#type: "button", class: "gs-btn", "data-simulate-los": "", disabled: busy() || reason.is_some(), title: reason.clone().unwrap_or_default(), onclick: simulate,
                    if busy() { "Simulating…" } else { "Simulate signal loss here" } }
                if simulated().is_some() {
                    button { r#type: "button", class: "gs-btn", onclick: move |_| { simulated.set(None); map_overlays::set(PROVIDER_REPLAY_LOS, &[]); }, "Clear simulation" }
                }
            }
            if let Some(r) = reason.as_ref() { small { style: "color:{theme.text_muted};", "Signal loss simulation: {r}" } }
            if plan.read().is_some() {
                small { style: "color:#a78bfa;", "╌ The plan's predicted track and landing area are drawn dashed on the 3D replay and the Map tab." }
            }
            if let Some(s) = simulated() { {card(&theme, &s, None, true)} }
            if let Some((ok, text)) = message() {
                p { role: "status", class: if ok { "gs-notice" } else { "gs-notice gs-notice--warn" }, style: "margin:0;", "{text}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock(name: &str) -> LosState {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flight-predict-mock").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_states_and_nulls() {
        let p = mock("los-predicting.json");
        assert!(p.shows_prediction());
        assert_eq!(p.label(), "Predicted: no signal");
        assert_eq!(p.map_overlay().unwrap().kind, "los_prediction");
        assert_eq!(p.landing.as_ref().unwrap().in_s, Some(140.0));
        let idle = mock("los-idle.json");
        assert!(!idle.shows_prediction());
        assert!(idle.map_overlay().is_none());
        assert_eq!(idle.settings, Settings { enabled: true, timeout_s: 5.0 });
        let resolved = mock("los-resolved.json");
        assert_eq!(resolved.comparison.as_ref().unwrap().error_m, Some(38.4));
        assert_eq!(resolved.history.len(), 1);
    }

    #[test]
    fn bearing_and_distance_text() {
        assert_eq!(bearing_deg((0.0, 0.0), (1.0, 0.0)).map(|b| b.round()), Some(0.0));
        assert_eq!(bearing_deg((0.0, 0.0), (0.0, 1.0)).map(|b| b.round()), Some(90.0));
        assert_eq!(bearing_deg((0.0, 0.0), (0.0, 0.0)), None);
        assert_eq!(cardinal(70.0), "ENE");
        assert_eq!(cardinal(359.0), "N");
        assert_eq!(distance_text(1500.0, true), "1.50 km");
        assert_eq!(distance_text(250.4, true), "250 m");
        assert_eq!(distance_text(1500.0, false), "0.93 mi");
        assert_eq!(seconds_text(140.0), "2 min 20 s");
        assert_eq!(seconds_text(12.3), "12 s");
        let station = FromStation { bearing_deg: 70.1, distance_m: 1500.0 };
        assert_eq!(direction_text(None, (31.76, -102.03), Some(&station), true).unwrap(), "1.50 km at 70° ENE from the station");
        let from_you = direction_text(Some((31.75, -102.05)), (31.75, -102.04), Some(&station), true).unwrap();
        assert!(from_you.ends_with("at 90° E from you"), "{from_you}");
        assert!(from_you.starts_with("946 m"), "{from_you}");
        assert_eq!(direction_text(None, (31.0, -102.0), None, true), None);
    }

    #[test]
    fn card_rows_measure_from_the_viewer_and_fall_back_to_the_station() {
        let p = mock("los-predicting.json");
        let viewer = Some((31.75, -102.05));
        let rows = card_rows(&p, viewer, true);
        let get = |rows: &[(String, String)], k: &str| rows.iter().find(|r| r.0 == k).map(|r| r.1.clone());
        assert_eq!(get(&rows, "Since last fix").unwrap(), "12 s (FC GPS)");
        assert_eq!(get(&rows, "Predicted now").unwrap(), "1300 m above the pad, drogue descent");
        assert!(get(&rows, "Predicted position").unwrap().contains("from you (±25 m)"));
        assert_eq!(get(&rows, "Time to landing").unwrap(), "2 min 20 s");
        assert!(get(&rows, "Predicted landing").unwrap().contains("from you (area 440 m × 180 m)"));
        assert!(get(&rows, "Your position").is_none());
        let rows = card_rows(&p, None, true);
        assert!(get(&rows, "Predicted position").unwrap().ends_with("from the station (±25 m)"));
        assert!(get(&rows, "Your position").is_some());
        let resolved = card_rows(&mock("los-resolved.json"), viewer, true);
        assert_eq!(get(&resolved, "Resolved").unwrap(), "a GPS fix came back; the prediction was off by 38 m");
    }

    #[test]
    fn simulate_body_uses_the_last_fix_previous_velocity_and_phase() {
        let track: Vec<TrackPoint> = vec![(0, 31.758, -102.046, 890.0), (10_000, 31.759, -102.046, 2890.0), (10_200, 31.759, -102.046, 2895.0), (11_000, 31.759, -102.045, 2900.0), (20_000, 31.76, -102.04, 3000.0)];
        let body = simulate_body(&track, 11_500, "Coast", 30.0).unwrap();
        assert_eq!(body["last_fix"]["t_ms"], 11_000);
        assert_eq!(body["phase"], "coast");
        assert_eq!(body["elapsed_s"], 30.0);
        assert_eq!(body["site"]["alt_msl_m"], 890.0);
        // Previous fix at least 0.5 s earlier: 10 200 ms (0.8 s).
        let vu = body["last_fix"]["vu_mps"].as_f64().unwrap();
        assert!((vu - 6.25).abs() < 1e-9, "{vu}");
        assert!(body["last_fix"]["ve_mps"].as_f64().unwrap() > 0.0);
        assert!(body["last_fix"]["vn_mps"].as_f64().unwrap().abs() < 1e-9);
        let first = simulate_body(&track, 5_000, "Ascent", 30.0).unwrap();
        assert!(first["last_fix"].get("vu_mps").is_none(), "no earlier fix, no velocity");
        assert_eq!(simulate_body(&track, 11_000, "Landed", 30.0).unwrap_err(), "The rocket has landed at this point of the replay.");
        assert!(simulate_body(&track, 11_000, "Armed", 30.0).is_err());
        assert_eq!(simulate_body(&[], 11_000, "Coast", 30.0).unwrap_err(), "No GPS fix before this point of the replay.");
        assert_eq!(simulate_body(&track, -1, "Coast", 30.0).unwrap_err(), "No GPS fix before this point of the replay.");
        assert_eq!(simulate_body(&track, 11_000, "Unknown", 30.0).unwrap()["phase"], serde_json::Value::Null);
    }

    #[test]
    fn card_renders_predicted_label_and_viewer_overlays_js() {
        fn app() -> Element {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flight-predict-mock/los-predicting.json");
            let state: LosState = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            rsx! { LosMapCard { theme: ThemeConfig::default(), viewer: Some((31.75, -102.05)), metric: true, initial: Some(state) } }
        }
        let mut dom = VirtualDom::new(app);
        dom.rebuild_in_place();
        let html = dioxus::ssr::render(&dom);
        assert!(html.contains("Predicted: no signal"));
        assert!(html.contains("data-los-state=\"predicting\""));
        assert!(html.contains("from you"));
        let js = viewer_overlays_js("gs26-vehicle-model-1", &[MapOverlay { id: "x".into(), ..Default::default() }]);
        assert!(js.contains("setAttribute('data-overlays',"));
        assert!(js.contains("\\\"id\\\":\\\"x\\\""));
    }
}
