//! Burn reports (GroundStation26 `docs/apogee-planner.md` section 8): after
//! every burn the station measures the thrust from the stand load cells, first
//! provisionally from its own telemetry and then finally once the DAQ SD
//! launch file is merged. HITL/simulated reports are marked and stay out of
//! the motor fit.
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub(crate) const LIST_PATH: &str = "/api/burn_reports";

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Summary {
    pub id: String,
    /// None for a live provisional report (no saved capture yet).
    pub capture_id: Option<String>,
    pub capture_label: String,
    pub status: String,
    pub simulated: bool,
    pub mode: String,
    pub created_ms: i64,
    pub total_impulse_ns: f64,
    pub burn_time_s: f64,
    pub peak_thrust_n: f64,
    pub flags: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct List {
    pub reports: Vec<Summary>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Chamber {
    pub valid: bool,
    pub peak_psi: f64,
    pub mean_psi: f64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Coverage {
    pub daq_sd: bool,
    pub gs_telemetry: bool,
    pub daq_rows: u64,
    pub gs_rows: u64,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Flag {
    pub code: String,
    pub severity: String,
    pub message: String,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Report {
    #[serde(flatten)]
    pub summary: Summary,
    pub curve: Vec<[f64; 2]>,
    pub average_thrust_n: f64,
    pub impulse_sigma_ns: f64,
    pub propellant_consumed_kg: Option<f64>,
    pub isp_s: Option<f64>,
    pub fill_at_ignition_kg: Option<f64>,
    pub fill_after_kg: Option<f64>,
    pub ignition_ms: i64,
    pub ignition_source: String,
    pub tank_psi_at_ignition: Option<f64>,
    pub tank_psi_end: Option<f64>,
    pub burn_end_s: f64,
    pub oxidizer_kg: Option<f64>,
    pub fuel_kg: Option<f64>,
    pub of_ratio: Option<f64>,
    pub kg1000_impulse_ns: f64,
    pub kg50_term_ns: f64,
    pub chamber: Option<Chamber>,
    pub coverage: Coverage,
    pub quality_flags: Vec<Flag>,
    pub archive_id: Option<String>,
}

pub(crate) fn detail_path(id: &str) -> String {
    format!("{LIST_PATH}/{}", encode_segment(id))
}

/// Percent-encode a path segment (ids are station-made but may hold spaces).
pub(crate) fn encode_segment(id: &str) -> String {
    id.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub(crate) fn status_badge(status: &str) -> &'static str {
    if status == "final" { "FINAL" } else { "PROVISIONAL" }
}

pub(crate) fn severity_class(severity: &str) -> &'static str {
    match severity {
        "bad" => "gs-notice gs-notice--danger",
        "warn" => "gs-notice gs-notice--warn",
        _ => "gs-notice",
    }
}

/// SVG polyline points for a thrust curve in a `w` x `h` box (y up), plus the axis maxima.
pub(crate) fn polyline(curve: &[[f64; 2]], w: f64, h: f64) -> (String, f64, f64) {
    let t_max = curve.iter().map(|p| p[0]).fold(0.0_f64, f64::max).max(1e-6);
    let f_max = curve.iter().map(|p| p[1]).fold(0.0_f64, f64::max).max(1e-6);
    let points = curve
        .iter()
        .filter(|p| p[0].is_finite() && p[1].is_finite())
        .map(|p| format!("{:.1},{:.1}", p[0] / t_max * w, h - p[1].max(0.0) / f_max * h))
        .collect::<Vec<_>>()
        .join(" ");
    (points, t_max, f_max)
}

pub(crate) fn report_rows(r: &Report) -> Vec<(String, String)> {
    let s = &r.summary;
    let mut rows = vec![
        ("Total impulse".to_owned(), format!("{:.0} ± {:.0} N·s", s.total_impulse_ns, r.impulse_sigma_ns)),
        ("Burn time (5% of peak)".to_owned(), format!("{:.2} s", s.burn_time_s)),
        ("Peak / average thrust".to_owned(), format!("{:.0} N / {:.0} N", s.peak_thrust_n, r.average_thrust_n)),
    ];
    if let Some(m) = r.fill_at_ignition_kg {
        rows.push(("Fill at ignition".to_owned(), format!("{m:.2} kg")));
    }
    rows.push((
        "Propellant consumed".to_owned(),
        r.propellant_consumed_kg.map_or_else(|| "not measured".to_owned(), |m| format!("{m:.2} kg")),
    ));
    rows.push(("Effective Isp".to_owned(), r.isp_s.map_or_else(|| "—".to_owned(), |v| format!("{v:.0} s"))));
    rows.push((
        "Chamber pressure".to_owned(),
        match &r.chamber {
            Some(c) if c.valid => format!("peak {:.0} psi, mean {:.0} psi", c.peak_psi, c.mean_psi),
            _ => "not valid".to_owned(),
        },
    ));
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

#[component]
pub(super) fn BurnReportsPanel(theme: super::layout::ThemeConfig) -> Element {
    let factory = super::station_settings_request::use_factory();
    let mut list = use_signal(|| None::<Result<List, String>>);
    let mut selected = use_signal(|| None::<String>);
    let mut detail = use_signal(|| None::<Result<Report, String>>);
    use_future(move || async move {
        loop {
            list.set(Some(factory.get::<List>(LIST_PATH).await));
            #[cfg(target_arch = "wasm32")]
            gloo_timers::future::TimeoutFuture::new(10_000).await;
            #[cfg(not(target_arch = "wasm32"))]
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
    });
    let mut open = move |id: String| {
        selected.set(Some(id.clone()));
        detail.set(None);
        let request = factory.get::<Report>(&detail_path(&id));
        spawn(async move {
            let reply = request.await;
            if selected.peek().as_deref() == Some(id.as_str()) {
                detail.set(Some(reply));
            }
        });
    };
    rsx! {
        BurnReportsView { theme, list: list(), detail: detail(), selected: selected(), on_open: move |id: String| open(id) }
    }
}

#[component]
pub(super) fn BurnReportsView(
    theme: super::layout::ThemeConfig,
    list: Option<Result<List, String>>,
    detail: Option<Result<Report, String>>,
    selected: Option<String>,
    on_open: EventHandler<String>,
) -> Element {
    rsx! {
        section { class: "gs26-burn-reports", "data-burn-reports": "", style: "display:grid;gap:8px;min-width:0;",
            h3 { style: "margin:0;", "Burn reports" }
            p { style: "margin:0;color:{theme.text_muted};font-size:13px;",
                "Thrust = KG1000 + KG50 unloading, measured after every burn. PROVISIONAL uses the station's telemetry; FINAL adds the DAQ SD launch file." }
            match list {
                None => rsx! { p { role: "status", "Reading burn reports…" } },
                Some(Err(e)) => rsx! { p { role: "status", class: "gs-notice gs-notice--warn", "Cannot read burn reports: {super::igt::refusal(&e)}" } },
                Some(Ok(l)) if l.reports.is_empty() => rsx! { p { role: "status", "No burns measured yet." } },
                Some(Ok(l)) => rsx! {
                    ul { style: "display:grid;gap:4px;padding:0;margin:0;list-style:none;",
                        for r in l.reports.iter().cloned() {
                            li { key: "{r.id}",
                                button { r#type: "button", class: "gs-btn", "aria-pressed": selected.as_deref() == Some(r.id.as_str()),
                                    style: "width:100%;text-align:left;",
                                    onclick: { let id = r.id.clone(); move |_| on_open.call(id.clone()) },
                                    Badges { status: r.status.clone(), simulated: r.simulated }
                                    " {r.capture_label} — {r.total_impulse_ns:.0} N·s, {r.burn_time_s:.2} s, peak {r.peak_thrust_n:.0} N"
                                    if !r.flags.is_empty() { " ⚠ {r.flags.len()} flag(s)" }
                                }
                            }
                        }
                    }
                },
            }
            if selected.is_some() {
                match detail {
                    None => rsx! { p { role: "status", "Reading the report…" } },
                    Some(Err(e)) => rsx! { p { role: "status", class: "gs-notice gs-notice--warn", "Cannot read the report: {super::igt::refusal(&e)}" } },
                    Some(Ok(report)) => rsx! { BurnReportDetail { theme: theme.clone(), report } },
                }
            }
        }
    }
}

#[component]
fn Badges(status: String, simulated: bool) -> Element {
    let label = status_badge(&status);
    rsx! {
        span { "data-burn-status": "{label}", style: "font-weight:800;padding:1px 6px;border-radius:6px;border:1px solid currentColor;margin-right:4px;", "{label}" }
        if simulated {
            span { "data-burn-simulated": "", style: "font-weight:800;padding:1px 6px;border-radius:6px;border:1px dashed currentColor;margin-right:4px;", "SIMULATED" }
        }
    }
}

#[component]
pub(super) fn ThrustPlot(curve: Vec<[f64; 2]>, theme: super::layout::ThemeConfig) -> Element {
    let (w, h) = (600.0, 220.0);
    let (points, t_max, f_max) = polyline(&curve, w, h);
    rsx! {
        svg { "data-thrust-plot": "", view_box: "-50 -10 {w + 60.0} {h + 40.0}", width: "100%", role: "img",
            "aria-label": "Thrust curve, peak {f_max:.0} N over {t_max:.1} s",
            style: "max-width:720px;background:{theme.panel_background};",
            line { x1: "0", y1: "{h}", x2: "{w}", y2: "{h}", stroke: "{theme.text_muted}", stroke_width: "1" }
            line { x1: "0", y1: "0", x2: "0", y2: "{h}", stroke: "{theme.text_muted}", stroke_width: "1" }
            polyline { points: "{points}", fill: "none", stroke: "{theme.info_accent}", stroke_width: "2" }
            text { x: "0", y: "{h + 18.0}", fill: "{theme.text_muted}", font_size: "12", "0 s" }
            text { x: "{w}", y: "{h + 18.0}", fill: "{theme.text_muted}", font_size: "12", text_anchor: "end", "{t_max:.1} s" }
            text { x: "-6", y: "10", fill: "{theme.text_muted}", font_size: "12", text_anchor: "end", "{f_max:.0} N" }
        }
    }
}

#[component]
fn BurnReportDetail(theme: super::layout::ThemeConfig, report: Report) -> Element {
    let rows = report_rows(&report);
    let s = report.summary.clone();
    rsx! {
        article { "data-burn-report": "{s.id}", style: "display:grid;gap:6px;border:1px solid {theme.border_soft};border-radius:10px;padding:10px;min-width:0;",
            h4 { style: "margin:0;", Badges { status: s.status.clone(), simulated: s.simulated } " {s.capture_label} ({s.mode})" }
            if s.status != "final" {
                p { role: "status", class: "gs-notice gs-notice--warn", style: "margin:0;", "Provisional: computed from the station's telemetry only. It is recomputed when the DAQ SD launch file arrives." }
            }
            if s.simulated {
                p { role: "status", class: "gs-notice", style: "margin:0;", "Simulated data: excluded from the motor fit by default." }
            }
            ThrustPlot { curve: report.curve.clone(), theme: theme.clone() }
            dl { style: "display:grid;grid-template-columns:max-content minmax(0,1fr);gap:4px 12px;margin:0;",
                for (label, value) in rows {
                    div { key: "{label}", style: "display:contents;",
                        dt { style: "font-weight:700;", "{label}" }
                        dd { style: "margin:0;overflow-wrap:anywhere;", "{value}" }
                    }
                }
            }
            if !report.quality_flags.is_empty() {
                ul { "data-burn-flags": "", style: "margin:0;display:grid;gap:4px;",
                    for (i, f) in report.quality_flags.iter().enumerate() {
                        li { key: "{i}", class: severity_class(&f.severity), "data-flag": "{f.code}", "{f.message}" }
                    }
                }
            }
            if let Some(id) = report.archive_id.clone() {
                p { style: "margin:0;color:{theme.text_muted};", "Fire archive record: {id} (review it in Settings → Mission → Fire archive)." }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report(status: &str, simulated: bool) -> Report {
        serde_json::from_value(json!({
            "id": "burn-1", "capture_id": "launch/2026-11-16", "capture_label": "Test fire 2026-11-16",
            "status": status, "simulated": simulated, "mode": "test_fire", "created_ms": 1,
            "total_impulse_ns": 9192.0, "burn_time_s": 5.65, "peak_thrust_n": 3295.0, "flags": ["kg1000_open_lead"],
            "curve": [[0.0, 0.0], [0.8, 3295.0], [5.0, 400.0], [7.5, 0.0]],
            "average_thrust_n": 1627.0, "impulse_sigma_ns": 900.0, "propellant_consumed_kg": null, "isp_s": 140.0,
            "fill_at_ignition_kg": 5.65, "chamber": {"valid": false},
            "coverage": {"daq_sd": status == "final", "gs_telemetry": true, "daq_rows": 1000, "gs_rows": 400},
            "quality_flags": [{"code": "kg1000_open_lead", "severity": "bad", "message": "KG1000 open lead 0-0.65 s"},
                              {"code": "gap_closing", "severity": "info", "message": "Gap-closing transient bridged"}],
            "archive_id": "2026-11-16_static"
        }))
        .unwrap()
    }

    #[test]
    fn decodes_flattened_summary_and_formats() {
        let r = report("final", false);
        assert_eq!(r.summary.total_impulse_ns, 9192.0);
        assert_eq!(r.curve.len(), 4);
        let rows = report_rows(&r);
        assert_eq!(rows[0].1, "9192 ± 900 N·s");
        assert!(rows.iter().any(|(k, v)| k == "Propellant consumed" && v == "not measured"));
        assert!(rows.iter().any(|(k, v)| k == "Chamber pressure" && v == "not valid"));
        let empty: Report = serde_json::from_value(json!({})).unwrap();
        // A live provisional report has no capture yet: the list still decodes.
        let live: List = serde_json::from_value(json!({"reports": [{"id": "live-3", "capture_id": null, "status": "provisional"}]})).unwrap();
        assert_eq!(live.reports[0].capture_id, None);
        assert!(empty.curve.is_empty());
        let (points, t, f) = polyline(&r.curve, 100.0, 50.0);
        assert_eq!((t, f), (7.5, 3295.0));
        assert!(points.starts_with("0.0,50.0 ") && points.ends_with("100.0,50.0"), "{points}");
        assert_eq!(detail_path("a b/c"), "/api/burn_reports/a%20b%2Fc");
        assert_eq!(status_badge("final"), "FINAL");
        assert_eq!(status_badge("provisional"), "PROVISIONAL");
    }

    fn render(list: List, detail: Option<Report>) -> String {
        let selected = detail.as_ref().map(|d| d.summary.id.clone());
        let mut dom = VirtualDom::new_with_props(
            |(list, detail, selected): (List, Option<Report>, Option<String>)| rsx! {
                BurnReportsView { theme: super::super::layout::ThemeConfig::default(), list: Some(Ok(list)), detail: detail.map(Ok), selected, on_open: |_| {} }
            },
            (list, detail, selected),
        );
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn provisional_and_final_badges_and_simulated() {
        let prov = report("provisional", true);
        let page = render(List { reports: vec![prov.summary.clone()] }, Some(prov));
        assert!(page.contains("PROVISIONAL") && page.contains("SIMULATED"), "{page}");
        assert!(page.contains("Provisional: computed from the station"));
        assert!(page.contains("data-thrust-plot"));
        assert!(page.contains("KG1000 open lead 0-0.65 s"));
        let fin = report("final", false);
        let page = render(List { reports: vec![fin.summary.clone()] }, Some(fin));
        assert!(page.contains("FINAL") && !page.contains("SIMULATED") && !page.contains("Provisional:"), "{page}");
        assert!(render(List::default(), None).contains("No burns measured yet."));
    }
}
