//! The Flight Computer's flight estimator (Kalman filter) status.
//!
//! Launch makes the FC start its Kalman filter; the station decodes the FC's
//! report (WebSocket `FcEstimator`, `/api/fc_estimator`) and warns when it
//! does not start, stops, diverges or stalls. From Launch on this shows
//! "FC estimator: running / not running / faulted" with the time since the
//! filter started on every view. A test fire has no FC and shows nothing.
use super::layout::ThemeConfig;
use dioxus::prelude::*;
use serde::Deserialize;

/// The station's `fc_estimator` status.
#[derive(Deserialize, Clone, Debug, PartialEq, Default)]
pub(crate) struct FcEstimatorView {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub cycles: Option<u32>,
    #[serde(default)]
    pub rate_hz: Option<f64>,
    #[serde(default)]
    pub running_for_ms: Option<u64>,
    #[serde(default)]
    pub started_t_plus_ms: Option<i64>,
    #[serde(default)]
    pub started_network_ms: Option<u64>,
    #[serde(default)]
    pub report_age_ms: Option<u64>,
    #[serde(default)]
    pub launch_seen: bool,
    #[serde(default)]
    pub expected: bool,
    #[serde(default)]
    pub stalled: bool,
    #[serde(default)]
    pub warning: Option<String>,
}

pub(crate) static FC_ESTIMATOR: GlobalSignal<Option<FcEstimatorView>> = Signal::global(|| None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tone {
    Running,
    Waiting,
    Fault,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Notice {
    pub title: String,
    pub body: String,
    pub tone: Tone,
}

/// "0:07", "12:34", "1:02:03".
pub(super) fn elapsed(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn started(t_plus_ms: i64) -> String {
    if t_plus_ms >= 0 {
        format!("Kalman filter started at T+{:.1} s", t_plus_ms as f64 / 1000.0)
    } else {
        format!("Kalman filter started {:.1} s before Launch", -t_plus_ms as f64 / 1000.0)
    }
}

fn fallback_title(state: &str) -> &'static str {
    match state {
        "running" => "FC estimator: running",
        "faulted" => "FC estimator: faulted",
        "not_running" => "FC estimator: not running",
        _ => "FC estimator: no report",
    }
}

/// The notice, from Launch on (or once the filter has run or warned);
/// nothing on the pad or without an FC.
pub(super) fn notice(view: &FcEstimatorView) -> Option<Notice> {
    let has_run = view.phase.as_deref().is_some_and(|p| !matches!(p, "idle" | "initializing"));
    if !view.expected || view.state == "not_expected" || !(view.launch_seen || has_run || view.warning.is_some()) {
        return None;
    }
    let tone = match view.state.as_str() {
        "running" => Tone::Running,
        "faulted" => Tone::Fault,
        _ if view.warning.is_some() => Tone::Fault,
        _ => Tone::Waiting,
    };
    let mut parts = Vec::new();
    if let Some(warning) = &view.warning {
        parts.push(warning.clone());
    }
    if let Some(t_plus) = view.started_t_plus_ms.filter(|_| has_run) {
        parts.push(started(t_plus));
    }
    if let Some(ms) = view.running_for_ms {
        parts.push(format!("running for {}", elapsed(ms)));
    }
    if let Some(rate) = view.rate_hz.filter(|r| *r > 0.0 && tone == Tone::Running) {
        parts.push(format!("{rate:.0} Hz"));
    }
    if parts.is_empty() {
        parts.push(match view.phase.as_deref() {
            Some("paused") => "Paused by the FC's recovery hold.".into(),
            Some("initializing") => "Launch accepted; the FC is initializing its Kalman filter.".into(),
            _ => "Launch accepted; waiting for the FC to report its Kalman filter running.".into(),
        });
    }
    if let Some(age) = view.report_age_ms.filter(|age| *age > 7_000) {
        parts.push(format!("last FC report {} s ago", age / 1000));
    }
    let title = if view.label.is_empty() { fallback_title(&view.state).to_string() } else { view.label.clone() };
    Some(Notice { title, body: parts.join(" · "), tone })
}

#[component]
pub(super) fn FcEstimatorNotice(theme: ThemeConfig) -> Element {
    let Some(n) = FC_ESTIMATOR.read().as_ref().and_then(notice) else {
        return rsx! {};
    };
    let (border, background, text) = match n.tone {
        Tone::Running => (theme.info_accent.clone(), theme.info_background.clone(), theme.info_text.clone()),
        Tone::Waiting => (theme.warning_border.clone(), theme.warning_background.clone(), theme.warning_text.clone()),
        Tone::Fault => (theme.error_border.clone(), theme.error_background.clone(), theme.error_text.clone()),
    };
    rsx! {
        aside {
            role: if n.tone == Tone::Fault { "alert" } else { "status" },
            class: "gs26-connection-notice gs26-layout-notice",
            "data-fc-estimator": match n.tone { Tone::Running => "running", Tone::Waiting => "waiting", Tone::Fault => "faulted" },
            style: "border-color:{border};background:{background};color:{text};",
            div { class: "gs26-layout-notice-body",
                strong { "{n.title}" }
                p { "{n.body}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn view(value: serde_json::Value) -> FcEstimatorView {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn running_after_launch_shows_the_start_and_time_since() {
        let n = notice(&view(json!({
            "state": "running", "label": "FC estimator: running (converged)", "phase": "converged",
            "cycles": 112, "rate_hz": 50.0, "running_for_ms": 72_250, "started_t_plus_ms": 300,
            "report_age_ms": 1_000, "launch_seen": true, "expected": true
        })))
        .unwrap();
        assert_eq!(n.title, "FC estimator: running (converged)");
        assert_eq!(n.body, "Kalman filter started at T+0.3 s · running for 1:12 · 50 Hz");
        assert_eq!(n.tone, Tone::Running);
    }

    #[test]
    fn waiting_then_not_running_then_faulted() {
        let waiting = notice(&view(json!({
            "state": "not_running", "label": "FC estimator: not running (initializing)", "phase": "initializing",
            "launch_seen": true, "expected": true
        })))
        .unwrap();
        assert_eq!(waiting.tone, Tone::Waiting);
        assert_eq!(waiting.body, "Launch accepted; the FC is initializing its Kalman filter.");
        let late = notice(&view(json!({
            "state": "not_running", "label": "FC estimator: not running", "launch_seen": true, "expected": true,
            "warning": "FC flight estimator not running 3.2 s after Launch."
        })))
        .unwrap();
        assert_eq!((late.tone, late.body.as_str()), (Tone::Fault, "FC flight estimator not running 3.2 s after Launch."));
        let diverged = notice(&view(json!({
            "state": "faulted", "label": "FC estimator: faulted (diverged)", "phase": "diverged",
            "started_t_plus_ms": 300, "report_age_ms": 9_400, "launch_seen": true, "expected": true,
            "warning": "FC flight estimator diverged: its Kalman filter state is not finite."
        })))
        .unwrap();
        assert_eq!(diverged.tone, Tone::Fault);
        assert_eq!(
            diverged.body,
            "FC flight estimator diverged: its Kalman filter state is not finite. · Kalman filter started at T+0.3 s · last FC report 9 s ago"
        );
    }

    #[test]
    fn nothing_on_the_pad_or_without_an_fc() {
        assert_eq!(notice(&view(json!({"state": "not_running", "phase": "idle", "expected": true}))), None);
        assert_eq!(notice(&view(json!({"state": "unknown", "expected": true}))), None);
        assert_eq!(
            notice(&view(json!({"state": "not_expected", "label": "FC estimator: no FC in this configuration",
                "launch_seen": true, "expected": false}))),
            None,
            "a test fire has no FC"
        );
        assert_eq!(notice(&FcEstimatorView::default()), None);
    }

    #[test]
    fn started_before_launch_and_clock_format() {
        assert_eq!(started(-1_500), "Kalman filter started 1.5 s before Launch");
        assert_eq!(elapsed(0), "0:00");
        assert_eq!(elapsed(59_999), "0:59");
        assert_eq!(elapsed(3_723_000), "1:02:03");
        let n = notice(&view(json!({"state": "running", "phase": "running", "expected": true, "running_for_ms": 4_000})))
            .unwrap();
        assert_eq!((n.title.as_str(), n.body.as_str()), ("FC estimator: running", "running for 0:04"));
    }
}
