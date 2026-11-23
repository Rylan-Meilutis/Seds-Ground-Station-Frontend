//! Motor & flight review (GroundStation26 `backend/src/flight_review.rs`):
//! the computed motor values of a burn (test fire, HITL, integrated ground
//! test) and the flight summary (normal, HITL, integrated ground test,
//! firmware simulation), live from `/api/flight_review/live` and for any
//! recording from `/api/flight_review/recording/{id}`, with the motor and
//! flight export bundles.
//!
//! HITL, firmware-simulation and replayed (integrated ground test) data is
//! always marked SIMULATED. In an integrated ground test the stand's real
//! motor and the FC's replayed flight are shown side by side, each labelled.
use super::burn_report_view::{Flag, Report, encode_segment, severity_class};
use super::layout::ThemeConfig;
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub(crate) const LIVE_PATH: &str = "/api/flight_review/live";

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Predicted {
    pub model: String,
    pub label: String,
    pub total_impulse_ns: Option<f64>,
    pub impulse_sigma_ns: Option<f64>,
    pub burn_time_s: Option<f64>,
    pub peak_thrust_n: Option<f64>,
    pub curve: Vec<[f64; 2]>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Series {
    pub kg1000_kg: Vec<[f64; 2]>,
    pub kg50_kg: Vec<[f64; 2]>,
    pub chamber_psi: Vec<[f64; 2]>,
    pub tank_psi: Vec<[f64; 2]>,
    pub sources: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct MotorView {
    pub report: Option<Report>,
    pub report_origin: String,
    pub reason: Option<String>,
    pub simulated: bool,
    pub ignition_ms: Option<i64>,
    pub window_from_ms: Option<i64>,
    pub window_to_ms: Option<i64>,
    pub series: Series,
    pub predicted: Option<Predicted>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Point {
    pub t_ms: i64,
    pub value: f64,
    pub source: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Deployment {
    pub t_ms: i64,
    pub altitude_m: Option<f64>,
    pub altitude_source: String,
    pub evidence: String,
    pub dry_run: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Event {
    pub name: String,
    pub t_ms: i64,
    pub t_plus_s: Option<f64>,
    pub t_launch_s: Option<f64>,
    pub altitude_m: Option<f64>,
    pub source: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Position {
    pub lat: f64,
    pub lon: f64,
    pub alt_msl_m: Option<f64>,
    pub source: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct FlightSummary {
    pub simulated: bool,
    pub data_label: String,
    pub launch_accepted_ms: Option<i64>,
    pub t0_ms: Option<i64>,
    pub liftoff_ms: Option<i64>,
    pub apogee: Option<Point>,
    pub altitude_maxima: Vec<Point>,
    pub fc_reported_apogee_m: Option<f64>,
    pub fc_reported_max_velocity_mps: Option<f64>,
    pub time_to_apogee_s: Option<f64>,
    pub time_to_apogee_from: String,
    pub drogue: Option<Deployment>,
    pub main: Option<Deployment>,
    pub landed_ms: Option<i64>,
    pub landed_source: String,
    pub flight_time_s: Option<f64>,
    pub max_velocity: Option<Point>,
    pub max_mach: Option<f64>,
    pub max_acceleration: Option<Point>,
    pub max_acceleration_g: Option<f64>,
    pub descent_rate_drogue_mps: Option<f64>,
    pub descent_rate_main_mps: Option<f64>,
    pub pad: Option<Position>,
    pub landing: Option<Position>,
    pub landing_distance_m: Option<f64>,
    pub landing_bearing_deg: Option<f64>,
    pub latest_phase: Option<String>,
    pub events: Vec<Event>,
    pub notes: Vec<String>,
    pub altitude_curve: Vec<[f64; 2]>,
    pub altitude_source: String,
    pub velocity_curve: Vec<[f64; 2]>,
    pub velocity_source: String,
    pub curve_zero_ms: Option<i64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct LiveCapture {
    pub label: String,
    pub kind: String,
    pub open: bool,
    pub window_start_ms: i64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct LiveReview {
    pub mode: String,
    pub igt_active: bool,
    pub firmware_sim: bool,
    pub simulated: bool,
    pub show_motor: bool,
    pub show_flight: bool,
    pub capture: Option<LiveCapture>,
    pub motor: Option<MotorView>,
    pub flight: Option<FlightSummary>,
    pub latest_capture_id: Option<String>,
    pub latest_capture_label: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct RecordingReview {
    pub id: String,
    pub source_kind: String,
    pub label: String,
    pub mode: Option<String>,
    pub provenance: Option<String>,
    pub integrated_ground_test: bool,
    pub simulated: bool,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub data_from_ms: Option<i64>,
    pub data_to_ms: Option<i64>,
    pub motor: Option<MotorView>,
    pub flight: Option<FlightSummary>,
}

pub(crate) fn recording_path(id: &str) -> String {
    format!("/api/flight_review/recording/{}", encode_segment(id))
}

/// The export URL of one bundle (`motor_export` / `flight_export`).
pub(crate) fn export_path(id: &str, kind: &str, range: Option<(i64, i64)>) -> String {
    let base = format!("{}/{kind}", recording_path(id));
    match range {
        Some((from, to)) => format!("{base}?from_ms={from}&to_ms={to}"),
        None => base,
    }
}

fn opt(v: Option<f64>, digits: usize, unit: &str) -> String {
    v.map_or_else(|| "—".to_owned(), |v| format!("{v:.digits$} {unit}"))
}

fn mode_label(mode: &str) -> &'static str {
    match mode {
        "normal" => "Normal (flight)",
        "hitl" => "HITL",
        "test_fire" => "Test fire",
        "testing" => "Testing",
        "integrated_ground_test" => "Integrated ground test",
        _ => "Unknown mode",
    }
}

/// The computed motor values, as label/value rows.
pub(crate) fn motor_rows(m: &MotorView) -> Vec<(String, String)> {
    let Some(r) = &m.report else {
        return vec![(
            "Burn".into(),
            m.reason
                .clone()
                .unwrap_or_else(|| "No burn measured yet".into()),
        )];
    };
    let s = &r.summary;
    let mut rows = vec![
        (
            "Total impulse".to_owned(),
            format!("{:.0} ± {:.0} N·s", s.total_impulse_ns, r.impulse_sigma_ns),
        ),
        (
            "Burn time (5 % of peak)".to_owned(),
            format!("{:.2} s", s.burn_time_s),
        ),
        (
            "Peak / average thrust".to_owned(),
            format!("{:.0} N / {:.0} N", s.peak_thrust_n, r.average_thrust_n),
        ),
        ("Effective Isp".to_owned(), opt(r.isp_s, 0, "s")),
        (
            "Fill at ignition (KG50)".to_owned(),
            opt(r.fill_at_ignition_kg, 2, "kg"),
        ),
        (
            "Propellant consumed".to_owned(),
            opt(r.propellant_consumed_kg, 2, "kg"),
        ),
        (
            "Oxidizer / fuel consumed".to_owned(),
            format!(
                "{} / {}",
                opt(r.oxidizer_kg, 2, "kg"),
                opt(r.fuel_kg, 2, "kg")
            ),
        ),
        (
            "O/F".to_owned(),
            r.of_ratio.map_or_else(
                || "not derivable".to_owned(),
                |v| format!("{v:.2} (all loaded N2O taken as expelled)"),
            ),
        ),
        (
            "Chamber pressure peak / average".to_owned(),
            match &r.chamber {
                Some(c) if c.valid => format!("{:.0} psi / {:.0} psi", c.peak_psi, c.mean_psi),
                Some(_) => "not valid (no pressure rise)".to_owned(),
                None => "no chamber PT data".to_owned(),
            },
        ),
        (
            "Tank pressure start / end".to_owned(),
            format!(
                "{} / {}",
                opt(r.tank_psi_at_ignition, 0, "psi"),
                opt(r.tank_psi_end, 0, "psi")
            ),
        ),
    ];
    if let Some(p) = &m.predicted {
        let delta = p
            .total_impulse_ns
            .filter(|v| *v > 0.0)
            .map(|v| {
                format!(
                    " (measured {:+.1} %)",
                    (s.total_impulse_ns / v - 1.0) * 100.0
                )
            })
            .unwrap_or_default();
        rows.push((
            "Predicted impulse".to_owned(),
            format!(
                "{} ± {}{delta}",
                opt(p.total_impulse_ns, 0, "N·s"),
                opt(p.impulse_sigma_ns, 0, "N·s")
            ),
        ));
    }
    let c = &r.coverage;
    rows.push((
        "Data".to_owned(),
        format!(
            "DAQ SD {} ({} rows), GS telemetry {} ({} rows)",
            if c.daq_sd { "yes" } else { "no" },
            c.daq_rows,
            if c.gs_telemetry { "yes" } else { "no" },
            c.gs_rows
        ),
    ));
    rows
}

fn since(f: &FlightSummary, t: i64) -> String {
    let mut parts = Vec::new();
    if let Some(z) = f.t0_ms {
        parts.push(format!("T{:+.1} s", (t - z) as f64 / 1000.0));
    }
    if let Some(l) = f.launch_accepted_ms {
        parts.push(format!("Launch {:+.1} s", (t - l) as f64 / 1000.0));
    }
    if parts.is_empty() {
        "—".into()
    } else {
        parts.join(" · ")
    }
}

/// The flight summary, as label/value rows.
pub(crate) fn flight_rows(f: &FlightSummary) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    rows.push((
        "Apogee".to_owned(),
        f.apogee.as_ref().map_or_else(
            || "—".into(),
            |p| format!("{:.0} m ({}), {}", p.value, p.source, since(f, p.t_ms)),
        ),
    ));
    for p in &f.altitude_maxima {
        rows.push((format!("Max {}", p.source), format!("{:.0} m", p.value)));
    }
    if let Some(v) = f.fc_reported_apogee_m {
        rows.push(("FC-reported apogee".to_owned(), format!("{v:.0} m")));
    }
    rows.push((
        "Time to apogee".to_owned(),
        f.time_to_apogee_s.map_or_else(
            || "—".into(),
            |t| format!("{t:.1} s from {}", f.time_to_apogee_from),
        ),
    ));
    for (name, d) in [
        ("Drogue deploy", &f.drogue),
        ("Main / reefing deploy", &f.main),
    ] {
        rows.push((
            name.to_owned(),
            d.as_ref().map_or_else(
                || "not seen".into(),
                |d| {
                    format!(
                        "{} at {} ({}){}",
                        opt(d.altitude_m, 0, "m"),
                        since(f, d.t_ms),
                        d.evidence,
                        if d.dry_run {
                            ", DRY-RUN: not fired"
                        } else {
                            ""
                        }
                    )
                },
            ),
        ));
    }
    rows.push((
        "Touchdown / Landed".to_owned(),
        f.landed_ms.map_or_else(
            || "not seen".into(),
            |t| format!("{} ({})", since(f, t), f.landed_source),
        ),
    ));
    rows.push(("Total flight time".to_owned(), opt(f.flight_time_s, 1, "s")));
    rows.push((
        "Max velocity".to_owned(),
        f.max_velocity.as_ref().map_or_else(
            || "—".into(),
            |v| {
                format!(
                    "{:.0} m/s, Mach {}",
                    v.value,
                    f.max_mach.map_or_else(|| "—".into(), |m| format!("{m:.2}"))
                )
            },
        ),
    ));
    rows.push((
        "Max acceleration".to_owned(),
        f.max_acceleration.as_ref().map_or_else(
            || "—".into(),
            |a| {
                format!(
                    "{:.0} m/s² ({:.1} g)",
                    a.value,
                    f.max_acceleration_g.unwrap_or_default()
                )
            },
        ),
    ));
    rows.push((
        "Descent rate drogue / main".to_owned(),
        format!(
            "{} / {}",
            opt(f.descent_rate_drogue_mps, 1, "m/s"),
            opt(f.descent_rate_main_mps, 1, "m/s")
        ),
    ));
    if let Some(l) = &f.landing {
        rows.push((
            "Landing (GPS)".to_owned(),
            format!(
                "{:.5}, {:.5}{}",
                l.lat,
                l.lon,
                f.landing_distance_m.map_or_else(String::new, |d| format!(
                    " · {d:.0} m from the pad, bearing {:.0}°",
                    f.landing_bearing_deg.unwrap_or_default()
                ))
            ),
        ));
    }
    rows
}

// ------------------------------------------------------------------ charts

#[derive(Clone, PartialEq)]
pub(crate) struct ChartLine {
    pub label: String,
    pub points: Vec<[f64; 2]>,
    pub color: String,
    pub dashed: bool,
}

/// The chart's axis maxima and the SVG points of each line.
pub(crate) fn chart_geometry(
    lines: &[ChartLine],
    w: f64,
    h: f64,
) -> ((f64, f64, f64, f64), Vec<String>) {
    let pts = lines
        .iter()
        .flat_map(|l| l.points.iter())
        .filter(|p| p[0].is_finite() && p[1].is_finite());
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in pts {
        x0 = x0.min(p[0]);
        x1 = x1.max(p[0]);
        y0 = y0.min(p[1]);
        y1 = y1.max(p[1]);
    }
    if x0 > x1 {
        (x0, x1, y0, y1) = (0.0, 1.0, 0.0, 1.0);
    }
    y0 = y0.min(0.0);
    if x1 - x0 < 1e-9 {
        x1 = x0 + 1.0;
    }
    if y1 - y0 < 1e-9 {
        y1 = y0 + 1.0;
    }
    let paths = lines
        .iter()
        .map(|l| {
            l.points
                .iter()
                .filter(|p| p[0].is_finite() && p[1].is_finite())
                .map(|p| {
                    format!(
                        "{:.1},{:.1}",
                        (p[0] - x0) / (x1 - x0) * w,
                        h - (p[1] - y0) / (y1 - y0) * h
                    )
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    ((x0, x1, y0, y1), paths)
}

#[component]
pub(crate) fn ReviewChart(
    title: String,
    unit: String,
    x_label: String,
    lines: Vec<ChartLine>,
    markers: Vec<(f64, String)>,
    theme: ThemeConfig,
) -> Element {
    let (w, h) = (600.0, 200.0);
    let ((x0, x1, y0, y1), paths) = chart_geometry(&lines, w, h);
    let has_data = lines.iter().any(|l| !l.points.is_empty());
    let legend: Vec<(String, String)> = lines
        .iter()
        .filter(|l| !l.points.is_empty())
        .map(|l| {
            let dash = if l.dashed { "dashed" } else { "solid" };
            (l.label.clone(), format!("display:inline-block;width:18px;border-top:3px {dash} {};margin-right:4px;vertical-align:middle;", l.color))
        })
        .collect();
    let marks: Vec<(f64, String)> = markers
        .into_iter()
        .filter(|(x, _)| *x >= x0 && *x <= x1)
        .map(|(x, l)| ((x - x0) / (x1 - x0) * w, l))
        .collect();
    rsx! {
        figure { "data-review-chart": "{title}", style: "margin:0;display:grid;gap:4px;min-width:0;",
            figcaption { style: "font-weight:700;font-size:13px;", "{title}" }
            if has_data {
                svg { view_box: "-56 -8 {w + 64.0} {h + 34.0}", width: "100%", role: "img",
                    "aria-label": "{title}: {y0:.0} to {y1:.0} {unit} over {x0:.1} to {x1:.1} s",
                    style: "max-width:720px;background:{theme.panel_background};",
                    line { x1: "0", y1: "{h}", x2: "{w}", y2: "{h}", stroke: "{theme.text_muted}", stroke_width: "1" }
                    line { x1: "0", y1: "0", x2: "0", y2: "{h}", stroke: "{theme.text_muted}", stroke_width: "1" }
                    for (i, (x, label)) in marks.iter().enumerate() {
                        g { key: "m{i}",
                            line { x1: "{x:.1}", y1: "0", x2: "{x:.1}", y2: "{h}", stroke: "{theme.text_muted}", stroke_width: "1", stroke_dasharray: "3 3" }
                            text { x: "{x + 3.0:.1}", y: "{10.0 + (i % 4) as f64 * 11.0}", fill: "{theme.text_muted}", font_size: "10", "{label}" }
                        }
                    }
                    for (i, (line, points)) in lines.iter().zip(paths.iter()).enumerate() {
                        polyline { key: "l{i}", points: "{points}", fill: "none", stroke: "{line.color}", stroke_width: "2",
                            stroke_dasharray: if line.dashed { "6 4" } else { "none" } }
                    }
                    text { x: "0", y: "{h + 16.0}", fill: "{theme.text_muted}", font_size: "11", "{x0:.1} s" }
                    text { x: "{w}", y: "{h + 16.0}", fill: "{theme.text_muted}", font_size: "11", text_anchor: "end", "{x1:.1} s" }
                    text { x: "{w / 2.0}", y: "{h + 28.0}", fill: "{theme.text_muted}", font_size: "11", text_anchor: "middle", "{x_label}" }
                    text { x: "-6", y: "10", fill: "{theme.text_muted}", font_size: "11", text_anchor: "end", "{y1:.0} {unit}" }
                    text { x: "-6", y: "{h}", fill: "{theme.text_muted}", font_size: "11", text_anchor: "end", "{y0:.0}" }
                }
                ul { style: "display:flex;flex-wrap:wrap;gap:4px 12px;margin:0;padding:0;list-style:none;font-size:12px;",
                    for (i, (label, swatch)) in legend.iter().enumerate() {
                        li { key: "k{i}", span { style: "{swatch}" } "{label}" }
                    }
                }
            } else {
                p { style: "margin:0;color:{theme.text_muted};font-size:12px;", "No data in the burn window." }
            }
        }
    }
}

fn rows_grid(rows: Vec<(String, String)>) -> Element {
    rsx! {
        dl { style: "display:grid;grid-template-columns:max-content minmax(0,1fr);gap:4px 12px;margin:0;",
            for (label, value) in rows {
                div { key: "{label}", style: "display:contents;",
                    dt { style: "font-weight:700;", "{label}" }
                    dd { style: "margin:0;overflow-wrap:anywhere;", "{value}" }
                }
            }
        }
    }
}

fn sim_badge(theme: &ThemeConfig) -> Element {
    rsx! { span { "data-review-simulated": "", style: "font-weight:800;padding:1px 6px;border-radius:6px;border:1px dashed {theme.warning_border};color:{theme.warning_text};margin-left:6px;", "SIMULATED" } }
}

// ------------------------------------------------------------------ sections

#[component]
pub(crate) fn MotorSection(
    theme: ThemeConfig,
    motor: Option<MotorView>,
    simulated: bool,
    heading: String,
) -> Element {
    let simulated = simulated || motor.as_ref().is_some_and(|m| m.simulated);
    rsx! {
        section { "data-review-section": "motor", style: "display:grid;gap:8px;border:1px solid {theme.border_soft};border-radius:10px;padding:10px;min-width:0;",
            h3 { style: "margin:0;", "{heading}" if simulated { {sim_badge(&theme)} } }
            if simulated {
                p { role: "status", class: "gs-notice gs-notice--warn", style: "margin:0;", "Simulated data (HITL, firmware simulation or simulator): not a measurement of the motor." }
            }
            match motor {
                None => rsx! { p { role: "status", style: "margin:0;color:{theme.text_muted};", "No load-cell, chamber or tank data yet." } },
                Some(m) => {
                    let rows = motor_rows(&m);
                    let status = m.report.as_ref().map(|r| super::burn_report_view::status_badge(&r.summary.status));
                    let origin = match m.report_origin.as_str() {
                        "live" => "live, from the station's telemetry",
                        "saved" => "saved burn report",
                        "computed" => "computed from this recording and range",
                        _ => "",
                    };
                    let curve = m.report.as_ref().map(|r| r.curve.clone()).unwrap_or_default();
                    let predicted = m.predicted.clone();
                    let flags: Vec<Flag> = m.report.as_ref().map(|r| r.quality_flags.clone()).unwrap_or_default();
                    let burn_end = m.report.as_ref().map(|r| r.burn_end_s).unwrap_or_default();
                    let markers = vec![(0.0, "ignition".to_owned()), (burn_end, "burn end".to_owned())];
                    let x_label = "s from the ignition reference".to_owned();
                    let mut thrust = vec![ChartLine { label: "measured thrust (KG1000 + KG50 unloading)".into(), points: curve, color: theme.info_accent.clone(), dashed: false }];
                    if let Some(p) = predicted.as_ref().filter(|p| !p.curve.is_empty()) {
                        thrust.push(ChartLine { label: format!("predicted: {}", p.label), points: p.curve.clone(), color: theme.warning_text.clone(), dashed: true });
                    }
                    let source = |k: &str| m.series.sources.get(k).map(|s| s.replace('_', " ")).unwrap_or_else(|| "no data".to_owned());
                    rsx! {
                        if let Some(label) = status {
                            p { style: "margin:0;", span { "data-burn-status": "{label}", style: "font-weight:800;padding:1px 6px;border-radius:6px;border:1px solid currentColor;margin-right:6px;", "{label}" } "{origin}" }
                        }
                        if let Some(p) = predicted.as_ref().filter(|p| p.curve.is_empty() && p.total_impulse_ns.is_none()) {
                            p { style: "margin:0;color:{theme.text_muted};", "Prediction: {p.label}" }
                        }
                        {rows_grid(rows)}
                        ReviewChart { title: "Thrust".to_owned(), unit: "N".to_owned(), x_label: x_label.clone(), lines: thrust, markers: markers.clone(), theme: theme.clone() }
                        div { style: "display:grid;grid-template-columns:repeat(auto-fit,minmax(260px,1fr));gap:8px;",
                            ReviewChart { title: format!("Chamber PT ({})", source("chamber")), unit: "psi".to_owned(), x_label: x_label.clone(), lines: vec![ChartLine { label: "chamber pressure".into(), points: m.series.chamber_psi.clone(), color: theme.error_text.clone(), dashed: false }], markers: markers.clone(), theme: theme.clone() }
                            ReviewChart { title: format!("Tank PT ({})", source("tank")), unit: "psi".to_owned(), x_label: x_label.clone(), lines: vec![ChartLine { label: "ox tank pressure".into(), points: m.series.tank_psi.clone(), color: theme.success_text.clone(), dashed: false }], markers: markers.clone(), theme: theme.clone() }
                            ReviewChart { title: format!("Load cells (KG1000 {}, KG50 {})", source("kg1000"), source("kg50")), unit: "kg".to_owned(), x_label: x_label.clone(), lines: vec![
                                ChartLine { label: "KG1000".into(), points: m.series.kg1000_kg.clone(), color: theme.info_accent.clone(), dashed: false },
                                ChartLine { label: "KG50".into(), points: m.series.kg50_kg.clone(), color: theme.warning_text.clone(), dashed: true },
                            ], markers, theme: theme.clone() }
                        }
                        if !flags.is_empty() {
                            ul { "data-review-flags": "", style: "margin:0;display:grid;gap:4px;",
                                for (i, f) in flags.iter().enumerate() {
                                    li { key: "{i}", class: severity_class(&f.severity), "data-flag": "{f.code}", "{f.message}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub(crate) fn FlightSection(
    theme: ThemeConfig,
    flight: Option<FlightSummary>,
    simulated: bool,
    heading: String,
) -> Element {
    let simulated = simulated || flight.as_ref().is_some_and(|f| f.simulated);
    rsx! {
        section { "data-review-section": "flight", style: "display:grid;gap:8px;border:1px solid {theme.border_soft};border-radius:10px;padding:10px;min-width:0;",
            h3 { style: "margin:0;", "{heading}" if simulated { {sim_badge(&theme)} } }
            match flight {
                None => rsx! { p { role: "status", style: "margin:0;color:{theme.text_muted};", "No flight yet: the summary fills in from FC telemetry after Launch." } },
                Some(f) => {
                    let rows = flight_rows(&f);
                    let markers: Vec<(f64, String)> = f.curve_zero_ms.map(|z| f.events.iter().filter(|e| !e.name.starts_with("Max velocity")).map(|e| ((e.t_ms - z) as f64 / 1000.0, e.name.clone())).collect()).unwrap_or_default();
                    let x_label = if f.curve_zero_ms.is_some() && f.curve_zero_ms == f.t0_ms { "s from T-0".to_owned() } else { "s".to_owned() };
                    rsx! {
                        p { class: if simulated { "gs-notice gs-notice--warn" } else { "" }, style: "margin:0;", "{f.data_label}" }
                        if let Some(phase) = &f.latest_phase {
                            p { style: "margin:0;color:{theme.text_muted};", "Latest FC phase: {phase}" }
                        }
                        {rows_grid(rows)}
                        if !f.events.is_empty() {
                            table { "data-review-events": "", style: "border-collapse:collapse;font-size:13px;width:100%;",
                                thead { tr {
                                    th { style: "text-align:left;", "Event" }
                                    th { style: "text-align:right;", "T+ (s)" }
                                    th { style: "text-align:right;", "From Launch (s)" }
                                    th { style: "text-align:right;", "Altitude (m)" }
                                    th { style: "text-align:left;", "Source" }
                                } }
                                tbody {
                                    for (i, e) in f.events.iter().enumerate() {
                                        tr { key: "{i}",
                                            td { "{e.name}" }
                                            td { style: "text-align:right;", {e.t_plus_s.map_or_else(|| "—".to_owned(), |v| format!("{v:.1}"))} }
                                            td { style: "text-align:right;", {e.t_launch_s.map_or_else(|| "—".to_owned(), |v| format!("{v:.1}"))} }
                                            td { style: "text-align:right;", {e.altitude_m.map_or_else(|| "—".to_owned(), |v| format!("{v:.0}"))} }
                                            td { style: "color:{theme.text_muted};", "{e.source}" }
                                        }
                                    }
                                }
                            }
                        }
                        div { style: "display:grid;grid-template-columns:repeat(auto-fit,minmax(260px,1fr));gap:8px;",
                            ReviewChart { title: format!("Altitude ({})", f.altitude_source), unit: "m".to_owned(), x_label: x_label.clone(), lines: vec![ChartLine { label: f.altitude_source.clone(), points: f.altitude_curve.clone(), color: theme.info_accent.clone(), dashed: false }], markers: markers.clone(), theme: theme.clone() }
                            ReviewChart { title: format!("Velocity ({})", f.velocity_source), unit: "m/s".to_owned(), x_label, lines: vec![ChartLine { label: f.velocity_source.clone(), points: f.velocity_curve.clone(), color: theme.success_text.clone(), dashed: false }], markers, theme: theme.clone() }
                        }
                        for (i, note) in f.notes.iter().enumerate() {
                            p { key: "n{i}", style: "margin:0;color:{theme.text_muted};font-size:12px;", "{note}" }
                        }
                    }
                }
            }
        }
    }
}

/// The motor and/or flight sections; in an integrated ground test, side by
/// side as the two bodies they are (the stand's real motor, the FC's
/// replayed flight).
#[component]
pub(crate) fn ReviewSections(
    theme: ThemeConfig,
    show_motor: bool,
    show_flight: bool,
    igt: bool,
    simulated: bool,
    motor: Option<MotorView>,
    flight: Option<FlightSummary>,
) -> Element {
    let (motor_heading, flight_heading) = if igt {
        (
            "Stand: motor burn (real)".to_owned(),
            "Rocket: SIMULATED replayed flight".to_owned(),
        )
    } else {
        ("Motor".to_owned(), "Flight summary".to_owned())
    };
    // In an integrated ground test the motor is real even though the flight is not.
    let motor_simulated = simulated && !igt;
    rsx! {
        div { "data-review-layout": if igt { "two-body" } else { "single" },
            style: if igt { "display:grid;grid-template-columns:repeat(auto-fit,minmax(340px,1fr));gap:10px;align-items:start;" } else { "display:grid;gap:10px;" },
            if show_motor {
                MotorSection { theme: theme.clone(), motor, simulated: motor_simulated, heading: motor_heading }
            }
            if show_flight {
                FlightSection { theme: theme.clone(), flight, simulated: simulated || igt, heading: flight_heading }
            }
        }
    }
}

// ------------------------------------------------------------------ export

/// "YYYY-MM-DDTHH:MM:SS" (UTC, the datetime-local input's format) <-> ms.
pub(crate) fn utc_input(ms: i64) -> String {
    super::recording_store_view::utc_label(ms)
        .trim_end_matches(" UTC")
        .replace(' ', "T")
}

pub(crate) fn parse_utc_input(text: &str) -> Option<i64> {
    let text = text.trim().replace(' ', "T");
    let (date, clock) = text.split_once('T')?;
    let d: Vec<i64> = date
        .split('-')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let mut c: Vec<f64> = clock
        .trim_end_matches('Z')
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    c.resize(3, 0.0);
    if d.len() != 3 {
        return None;
    }
    let month = time::Month::try_from(u8::try_from(d[1]).ok()?).ok()?;
    let date =
        time::Date::from_calendar_date(i32::try_from(d[0]).ok()?, month, u8::try_from(d[2]).ok()?)
            .ok()?;
    let secs =
        date.midnight().assume_utc().unix_timestamp() as f64 + c[0] * 3600.0 + c[1] * 60.0 + c[2];
    Some((secs * 1000.0).round() as i64)
}

#[component]
pub(crate) fn ExportControls(
    theme: ThemeConfig,
    recording: String,
    default_from_ms: Option<i64>,
    default_to_ms: Option<i64>,
    motor: bool,
    flight: bool,
) -> Element {
    let mut from = use_signal(|| default_from_ms.map(utc_input).unwrap_or_default());
    // Whole seconds: the end rounds up so the range still holds the window.
    let mut to = use_signal(|| {
        default_to_ms
            .map(|t| utc_input(t + 999))
            .unwrap_or_default()
    });
    let mut whole = use_signal(|| default_from_ms.is_none());
    let mut message = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let range = move || -> Result<Option<(i64, i64)>, String> {
        if whole() {
            return Ok(None);
        }
        match (parse_utc_input(&from()), parse_utc_input(&to())) {
            (Some(a), Some(b)) if b > a => Ok(Some((a, b))),
            (Some(_), Some(_)) => Err("The range must end after it starts.".into()),
            _ => Err("Enter both times as YYYY-MM-DDTHH:MM:SS (UTC).".into()),
        }
    };
    let rec = use_signal(|| recording.clone());
    let mut start = move |kind: &'static str| {
        let rec = rec.peek().clone();
        let range = match range() {
            Ok(range) => range,
            Err(e) => {
                message.set(e);
                return;
            }
        };
        let owner = match super::recording_download::ExportOwner::capture() {
            Ok(owner) => owner,
            Err(e) => {
                message.set(e);
                return;
            }
        };
        let path = export_path(&rec, kind, range);
        let file = format!(
            "{}-{}.zip",
            if kind == "motor_export" {
                "motor"
            } else {
                "flight"
            },
            rec.trim_end_matches(".db").replace(['.', '/'], "_")
        );
        busy.set(true);
        message.set("Preparing the export…".into());
        spawn(async move {
            let result =
                super::recording_download::download_csv_path_owned(&path, &file, owner).await;
            busy.set(false);
            message.set(result.unwrap_or_else(|e| e));
        });
    };
    rsx! {
        div { "data-review-export": "{recording}", style: "display:grid;gap:6px;border-top:1px solid {theme.border_soft};padding-top:8px;",
            strong { "Export" }
            p { style: "margin:0;color:{theme.text_muted};font-size:12px;",
                "Motor bundle: thrust curve, KG50/KG1000 raw and calibrated, chamber and tank PT, temperatures, valve events, merged DAQ SD data, charts (SVG, PNG), the computed values (JSON + text) and an HTML/PDF report. Flight bundle: the events table, altitude/velocity/GPS data and plots. Times in UTC." }
            label { style: "display:flex;gap:6px;align-items:center;",
                input { r#type: "checkbox", checked: whole(), onchange: move |e| whole.set(e.checked()) }
                if default_from_ms.is_some() { "Default range (motor: the burn window; flight: Launch to touchdown)" } else { "Whole recording" }
            }
            if !whole() {
                div { style: "display:flex;gap:8px;flex-wrap:wrap;",
                    label { "From (UTC) " input { r#type: "datetime-local", step: "1", "aria-label": "Export from (UTC)", value: "{from}", oninput: move |e| from.set(e.value()) } }
                    label { "To (UTC) " input { r#type: "datetime-local", step: "1", "aria-label": "Export to (UTC)", value: "{to}", oninput: move |e| to.set(e.value()) } }
                }
            }
            div { style: "display:flex;gap:8px;flex-wrap:wrap;",
                if motor {
                    button { r#type: "button", class: "gs-btn", "data-export-kind": "motor", disabled: busy(), onclick: move |_| start("motor_export"), "Export motor data (.zip)" }
                }
                if flight {
                    button { r#type: "button", class: "gs-btn", "data-export-kind": "flight", disabled: busy(), onclick: move |_| start("flight_export"), "Export flight summary (.zip)" }
                }
            }
            if !message().is_empty() { p { role: "status", style: "margin:0;", "{message}" } }
        }
    }
}

// ------------------------------------------------------------------ views

#[component]
pub(crate) fn RecordingReviewView(
    theme: ThemeConfig,
    review: Option<Result<RecordingReview, String>>,
    on_close: Option<EventHandler<()>>,
) -> Element {
    rsx! {
        section { "data-recording-review": "", style: "display:grid;gap:8px;border:1px solid {theme.border};border-radius:10px;padding:10px;min-width:0;",
            div { style: "display:flex;gap:8px;align-items:center;flex-wrap:wrap;",
                h3 { style: "margin:0;flex:1;", "Motor & flight summary" }
                if let Some(close) = on_close {
                    button { r#type: "button", class: "gs-btn", onclick: move |_| close.call(()), "Close" }
                }
            }
            match review {
                None => rsx! { p { role: "status", "Computing the summary…" } },
                Some(Err(e)) => rsx! { p { role: "status", class: "gs-notice gs-notice--warn", "Cannot review this recording: {super::igt::refusal(&e)}" } },
                Some(Ok(r)) => {
                    let mode = r.mode.as_deref().map(mode_label).unwrap_or("Unknown mode");
                    let default_from = r.motor.as_ref().and_then(|m| m.window_from_ms);
                    let default_to = r.motor.as_ref().and_then(|m| m.window_to_ms);
                    rsx! {
                        p { style: "margin:0;color:{theme.text_muted};", "{r.label} · {mode} · {r.source_kind}" if r.simulated { {sim_badge(&theme)} } }
                        if r.motor.is_none() && r.flight.is_none() {
                            p { role: "status", "No motor or flight data in this recording." }
                        }
                        ReviewSections { theme: theme.clone(), show_motor: r.motor.is_some(), show_flight: r.flight.is_some(), igt: r.integrated_ground_test, simulated: r.simulated, motor: r.motor.clone(), flight: r.flight.clone() }
                        if r.motor.is_some() || r.flight.is_some() {
                            ExportControls { key: "{r.id}", theme: theme.clone(), recording: r.id.clone(), default_from_ms: default_from, default_to_ms: default_to, motor: r.motor.is_some(), flight: r.flight.is_some() }
                        }
                    }
                }
            }
        }
    }
}

/// The review of one recording (any replay id: a capture, a rolling range,
/// `merge-sd.*`, `fire.*`).
#[component]
pub(crate) fn RecordingReviewPanel(
    theme: ThemeConfig,
    recording: String,
    on_close: Option<EventHandler<()>>,
) -> Element {
    let factory = super::station_settings_request::use_factory();
    let mut review = use_signal(|| None::<Result<RecordingReview, String>>);
    let id = recording.clone();
    use_effect(use_reactive!(|id| {
        review.set(None);
        let request = factory.get::<RecordingReview>(&recording_path(&id));
        spawn(async move { review.set(Some(request.await)) });
    }));
    rsx! { RecordingReviewView { theme, review: review(), on_close } }
}

#[component]
pub(crate) fn LiveReviewView(
    theme: ThemeConfig,
    live: Option<Result<LiveReview, String>>,
) -> Element {
    rsx! {
        section { "data-flight-review": "", style: "display:grid;gap:10px;min-width:0;",
            h2 { style: "margin:0;", "Motor & flight" }
            match live {
                None => rsx! { p { role: "status", "Reading the station…" } },
                Some(Err(e)) => rsx! { p { role: "status", class: "gs-notice gs-notice--warn", "Cannot read the motor and flight summary: {super::igt::refusal(&e)}" } },
                Some(Ok(l)) => {
                    let mode = mode_label(&l.mode);
                    rsx! {
                        p { style: "margin:0;color:{theme.text_muted};",
                            "{mode}" if l.firmware_sim { " · simulated firmware" }
                            if l.simulated { {sim_badge(&theme)} }
                        }
                        if l.igt_active {
                            p { role: "status", class: "gs-notice gs-notice--warn", style: "margin:0;", "Integrated ground test: the motor section is the stand's real burn; the flight summary is the FC's SIMULATED replayed flight." }
                        } else if l.mode == "hitl" {
                            p { role: "status", class: "gs-notice gs-notice--warn", style: "margin:0;", "HITL: every value here is SIMULATED." }
                        }
                        match &l.capture {
                            Some(c) => rsx! { p { style: "margin:0;", strong { "Live: " } "{c.label}" if c.open { " (recording)" } else { " (closing)" } } },
                            None => rsx! { p { style: "margin:0;color:{theme.text_muted};", "No launch capture open. The live summary starts at Launch." } },
                        }
                        if l.capture.is_some() {
                            ReviewSections { theme: theme.clone(), show_motor: l.show_motor, show_flight: l.show_flight, igt: l.igt_active, simulated: l.simulated, motor: l.motor.clone(), flight: l.flight.clone() }
                        }
                        if !l.show_motor && !l.show_flight {
                            p { "This mode shows neither section." }
                        }
                    }
                }
            }
        }
    }
}

/// The dashboard tab: the live sections, then the newest saved launch
/// capture with its exports.
#[component]
pub(crate) fn FlightReviewTab(theme: ThemeConfig) -> Element {
    let factory = super::station_settings_request::use_factory();
    let mut live = use_signal(|| None::<Result<LiveReview, String>>);
    let mut review_latest = use_signal(|| false);
    use_future(move || async move {
        loop {
            live.set(Some(factory.get::<LiveReview>(LIVE_PATH).await));
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(2_000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
    let latest = live().and_then(|l| l.ok()).and_then(|l| {
        l.latest_capture_id
            .clone()
            .zip(l.latest_capture_label.clone())
    });
    rsx! {
        div { style: "height:100%;width:100%;overflow:auto;box-sizing:border-box;padding:10px;display:grid;gap:12px;align-content:start;color:{theme.text_primary};",
            LiveReviewView { theme: theme.clone(), live: live() }
            if let Some((id, label)) = latest {
                div { style: "display:grid;gap:8px;",
                    div { style: "display:flex;gap:8px;align-items:center;flex-wrap:wrap;",
                        span { "Newest saved launch capture: " strong { "{label}" } }
                        button { r#type: "button", class: "gs-btn", "aria-expanded": review_latest(), onclick: move |_| review_latest.set(!review_latest()),
                            if review_latest() { "Hide its summary" } else { "Review and export it" } }
                    }
                    if review_latest() {
                        RecordingReviewPanel { key: "{id}", theme: theme.clone(), recording: id.clone(), on_close: move |_| review_latest.set(false) }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn motor(simulated: bool) -> MotorView {
        serde_json::from_value(json!({
            "report_origin": "saved", "simulated": simulated, "ignition_ms": 1000, "window_from_ms": -4000, "window_to_ms": 18000,
            "report": {
                "id": "launch.x", "capture_label": "Test fire", "status": "final", "simulated": simulated, "mode": "test_fire",
                "total_impulse_ns": 9192.0, "burn_time_s": 5.65, "peak_thrust_n": 3295.0, "average_thrust_n": 1627.0, "impulse_sigma_ns": 900.0,
                "curve": [[0.0, 0.0], [0.8, 3295.0], [5.0, 400.0], [7.5, 0.0]], "isp_s": 140.0, "fill_at_ignition_kg": 5.65,
                "propellant_consumed_kg": 6.65, "oxidizer_kg": 5.65, "fuel_kg": 1.0, "of_ratio": 5.65, "tank_psi_at_ignition": 650.0, "tank_psi_end": 120.0,
                "burn_end_s": 5.9, "chamber": {"valid": true, "peak_psi": 412.0, "mean_psi": 300.0},
                "coverage": {"daq_sd": true, "gs_telemetry": true, "daq_rows": 9000, "gs_rows": 900},
                "quality_flags": [{"code": "drop_back", "severity": "info", "message": "Rocket dropped back at 5.4 s"}]
            },
            "series": {"kg1000_kg": [[-1.0, 0.0], [1.0, 300.0]], "kg50_kg": [[-1.0, 40.0], [1.0, -38.0]], "chamber_psi": [[0.0, 0.0], [1.0, 400.0]], "tank_psi": [[0.0, 650.0], [6.0, 120.0]], "sources": {"kg1000": "daq_sd"}},
            "predicted": {"model": "apogee_planner", "label": "Apogee planner motor for plan p1", "total_impulse_ns": 9000.0, "impulse_sigma_ns": 700.0, "curve": [[0.0, 0.0], [1.0, 3000.0], [6.0, 0.0]]}
        }))
        .unwrap()
    }

    fn flight(simulated: bool) -> FlightSummary {
        serde_json::from_value(json!({
            "simulated": simulated, "data_label": if simulated { "SIMULATED flight: replay" } else { "FC telemetry" },
            "launch_accepted_ms": 7000, "t0_ms": 10000, "liftoff_ms": 10300,
            "apogee": {"t_ms": 34700, "value": 2442.4, "source": "FC Kalman altitude (estimator)"},
            "altitude_maxima": [{"t_ms": 34700, "value": 2442.4, "source": "FC Kalman altitude"}, {"t_ms": 34800, "value": 2450.0, "source": "GPS altitude above the pad"}],
            "time_to_apogee_s": 24.4, "time_to_apogee_from": "liftoff",
            "drogue": {"t_ms": 35700, "altitude_m": 2430.0, "altitude_source": "FC reported", "evidence": "FC phase Descent", "dry_run": simulated},
            "main": {"t_ms": 121200, "altitude_m": 300.0, "altitude_source": "station notice", "evidence": "station notice"},
            "landed_ms": 173200, "landed_source": "FC flight state Landed", "flight_time_s": 162.9,
            "max_velocity": {"t_ms": 14300, "value": 200.0, "source": "FC Kalman velocity"}, "max_mach": 0.59,
            "max_acceleration": {"t_ms": 11000, "value": 60.0, "source": "Raw accelerometer"}, "max_acceleration_g": 6.12,
            "descent_rate_drogue_mps": 25.0, "descent_rate_main_mps": 6.0,
            "landing": {"lat": 43.01, "lon": -78.8, "source": "GPS"}, "landing_distance_m": 1112.0, "landing_bearing_deg": 0.0,
            "events": [{"name": "Apogee (max altitude)", "t_ms": 34700, "t_plus_s": 24.7, "t_launch_s": 27.7, "altitude_m": 2442.4, "source": "FC Kalman"},
                       {"name": "Drogue deploy", "t_ms": 35700, "t_plus_s": 25.7, "t_launch_s": 28.7, "altitude_m": 2430.0, "source": "FC phase Descent"}],
            "altitude_curve": [[0.0, 0.0], [24.7, 2442.4], [163.2, 0.0]], "altitude_source": "FC Kalman altitude", "curve_zero_ms": 10000,
            "velocity_curve": [[0.0, 0.0], [4.3, 200.0]], "velocity_source": "FC Kalman velocity"
        }))
        .unwrap()
    }

    fn live(
        mode: &str,
        igt: bool,
        show_motor: bool,
        show_flight: bool,
        simulated: bool,
    ) -> LiveReview {
        LiveReview {
            mode: mode.into(),
            igt_active: igt,
            simulated,
            show_motor,
            show_flight,
            capture: Some(LiveCapture {
                label: "Launch".into(),
                kind: "launch".into(),
                open: true,
                window_start_ms: 0,
            }),
            motor: show_motor.then(|| motor(simulated && !igt)),
            flight: show_flight.then(|| flight(simulated)),
            ..LiveReview::default()
        }
    }

    fn render_live(l: LiveReview) -> String {
        let mut dom = VirtualDom::new_with_props(
            |l: LiveReview| rsx! { LiveReviewView { theme: ThemeConfig::default(), live: Some(Ok(l)) } },
            l,
        );
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn rows_format_the_motor_and_flight_values() {
        let rows = motor_rows(&motor(false));
        let get = |k: &str| {
            rows.iter()
                .find(|(l, _)| l == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        assert_eq!(get("Total impulse"), "9192 ± 900 N·s");
        assert_eq!(get("O/F"), "5.65 (all loaded N2O taken as expelled)");
        assert_eq!(get("Chamber pressure peak / average"), "412 psi / 300 psi");
        assert_eq!(get("Tank pressure start / end"), "650 psi / 120 psi");
        assert_eq!(
            get("Predicted impulse"),
            "9000 N·s ± 700 N·s (measured +2.1 %)"
        );
        let none = motor_rows(&MotorView {
            reason: Some("no burn found".into()),
            ..MotorView::default()
        });
        assert_eq!(none, vec![("Burn".to_owned(), "no burn found".to_owned())]);
        let rows = flight_rows(&flight(false));
        let get = |k: &str| {
            rows.iter()
                .find(|(l, _)| l == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        assert!(
            get("Apogee")
                .starts_with("2442 m (FC Kalman altitude (estimator)), T+24.7 s · Launch +27.7 s"),
            "{}",
            get("Apogee")
        );
        assert_eq!(get("Time to apogee"), "24.4 s from liftoff");
        assert!(get("Drogue deploy").starts_with("2430 m at T+25.7 s"));
        assert_eq!(get("Max velocity"), "200 m/s, Mach 0.59");
        assert_eq!(get("Descent rate drogue / main"), "25.0 m/s / 6.0 m/s");
        assert!(get("Landing (GPS)").contains("1112 m from the pad"));
        assert_eq!(
            export_path("event.launch.a b.db", "motor_export", Some((1, 2))),
            "/api/flight_review/recording/event.launch.a%20b.db/motor_export?from_ms=1&to_ms=2"
        );
        assert_eq!(
            parse_utc_input("2026-11-16T18:30:05"),
            Some(1_794_853_805_000)
        );
        assert_eq!(
            parse_utc_input(&utc_input(1_794_853_805_000)),
            Some(1_794_853_805_000),
            "{}",
            utc_input(1_794_853_805_000)
        );
        assert!(parse_utc_input("yesterday").is_none());
    }

    #[test]
    fn sections_render_per_mode() {
        // Test fire: the motor alone, real.
        let page = render_live(live("test_fire", false, true, false, false));
        assert!(
            page.contains("data-review-section=\"motor\"")
                && !page.contains("data-review-section=\"flight\""),
            "{page}"
        );
        assert!(
            !page.contains("SIMULATED")
                && page.contains("Total impulse")
                && page.contains("data-thrust") == false
        );
        assert!(
            page.contains("data-review-chart=\"Thrust\"")
                && page.contains("predicted: Apogee planner motor")
        );
        assert!(page.contains("Rocket dropped back at 5.4 s"));
        // Normal: the flight alone.
        let page = render_live(live("normal", false, false, true, false));
        assert!(
            !page.contains("data-review-section=\"motor\"")
                && page.contains("data-review-section=\"flight\"")
        );
        assert!(
            page.contains("data-review-events")
                && page.contains("Drogue deploy")
                && !page.contains("SIMULATED")
        );
        // HITL: both, every value SIMULATED.
        let page = render_live(live("hitl", false, true, true, true));
        assert!(
            page.contains("data-review-section=\"motor\"")
                && page.contains("data-review-section=\"flight\"")
        );
        assert!(
            page.contains("HITL: every value here is SIMULATED.")
                && page.contains("Simulated data (HITL")
        );
        assert!(page.contains("data-review-layout=\"single\""));
        // Integrated ground test: the real stand beside the simulated flight.
        let page = render_live(live("integrated_ground_test", true, true, true, true));
        assert!(page.contains("data-review-layout=\"two-body\""), "{page}");
        assert!(
            page.contains("Stand: motor burn (real)")
                && page.contains("Rocket: SIMULATED replayed flight")
        );
        assert!(
            !page.contains("Simulated data (HITL"),
            "the stand's motor is real"
        );
        assert!(page.contains("DRY-RUN: not fired"));
        // Nothing open yet.
        let page = render_live(LiveReview {
            mode: "normal".into(),
            show_flight: true,
            ..LiveReview::default()
        });
        assert!(page.contains("No launch capture open"));
    }

    #[test]
    fn recording_review_renders_sections_and_exports() {
        let review = RecordingReview {
            id: "event.launch.x.db".into(),
            source_kind: "capture".into(),
            label: "Test fire".into(),
            mode: Some("test_fire".into()),
            motor: Some(motor(false)),
            ..RecordingReview::default()
        };
        let mut dom = VirtualDom::new_with_props(
            |r: RecordingReview| rsx! { RecordingReviewView { theme: ThemeConfig::default(), review: Some(Ok(r)), on_close: None } },
            review,
        );
        dom.rebuild_in_place();
        let page = dioxus::ssr::render(&dom);
        assert!(
            page.contains("data-review-export=\"event.launch.x.db\""),
            "{page}"
        );
        assert!(
            page.contains("Export motor data (.zip)")
                && !page.contains("Export flight summary (.zip)")
        );
        assert!(page.contains("Test fire · Test fire · capture"));
    }

    /// Writes the rendered sections per mode (and any backend sample reviews
    /// in `GS_REVIEW_SAMPLE_DIR`) as HTML pages to `GS_REVIEW_SNAPSHOT_DIR`,
    /// for screenshots: `cargo test --bin groundstation_frontend write_review_snapshots -- --ignored`.
    #[test]
    #[ignore]
    fn write_review_snapshots() {
        let Some(dir) = std::env::var_os("GS_REVIEW_SNAPSHOT_DIR").map(std::path::PathBuf::from)
        else {
            return;
        };
        std::fs::create_dir_all(&dir).unwrap();
        let theme = ThemeConfig::default();
        let page = |body: String| {
            format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><style>{}</style></head><body style=\"margin:0;padding:12px;font-family:sans-serif;background:{};color:{};\">{body}</body></html>",
                include_str!("../design_system.css"),
                theme.app_background,
                theme.text_primary
            )
        };
        for (name, l) in [
            (
                "live-test-fire",
                live("test_fire", false, true, false, false),
            ),
            ("live-normal", live("normal", false, false, true, false)),
            ("live-hitl", live("hitl", false, true, true, true)),
            (
                "live-igt",
                live("integrated_ground_test", true, true, true, true),
            ),
        ] {
            std::fs::write(dir.join(format!("{name}.html")), page(render_live(l))).unwrap();
        }
        let samples = std::env::var_os("GS_REVIEW_SAMPLE_DIR").map(std::path::PathBuf::from);
        for name in ["review-test-fire", "review-flight"] {
            let Some(text) = samples
                .as_ref()
                .and_then(|d| std::fs::read_to_string(d.join(format!("{name}.json"))).ok())
            else {
                continue;
            };
            let review: RecordingReview = serde_json::from_str(&text).unwrap();
            let mut dom = VirtualDom::new_with_props(
                |r: RecordingReview| rsx! { RecordingReviewView { theme: ThemeConfig::default(), review: Some(Ok(r)), on_close: None } },
                review,
            );
            dom.rebuild_in_place();
            std::fs::write(
                dir.join(format!("{name}.html")),
                page(dioxus::ssr::render(&dom)),
            )
            .unwrap();
        }
    }
}
