//! Valve Board post-Launch auto-vent (GSE status `auto_vent`).
//!
//! After Launch the Valve Board vents the tank on its own 150 s after its
//! station link is lost or the launch continuity wire breaks. The station
//! keeps the board's last report and counts it down; this shows it.
use super::layout::ThemeConfig;
use super::sequence_progress_host::StatusObservation;
use dioxus::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Notice {
    pub title: String,
    pub body: String,
    pub vented: bool,
}

fn cause(link: bool, continuity: bool) -> &'static str {
    match (link, continuity) {
        (true, true) => "Link lost, continuity broken",
        (false, true) => "Continuity broken",
        _ => "Link lost",
    }
}

pub(super) fn clock(ms: u64) -> String {
    let s = ms.div_ceil(1000);
    format!("{}:{:02}", s / 60, s % 60)
}

pub(super) fn notice(status: &serde_json::Value) -> Option<Notice> {
    let v = &status["auto_vent"];
    let link = v["link_lost"].as_bool().unwrap_or(false);
    let continuity = v["continuity_broken"].as_bool().unwrap_or(false);
    match v["state"].as_str()? {
        "counting" => {
            let remaining = v["remaining_ms"].as_u64().unwrap_or(0);
            let title = if remaining == 0 {
                format!("{}: auto-vent due", cause(link, continuity))
            } else {
                format!("{}: auto-vent in {}", cause(link, continuity), clock(remaining))
            };
            Some(Notice {
                title,
                body: "After Launch the Valve Board opens Vent and Dump and closes Pilot on its own when this reaches zero. \
                       A GS heartbeat with continuity intact, or any valve command, cancels it."
                    .into(),
                vented: false,
            })
        }
        "vented" => Some(Notice {
            title: "Auto-vented".into(),
            body: if link || continuity {
                format!("The Valve Board vented the tank on its own ({}): Vent and Dump open, Pilot closed.", cause(link, continuity).to_lowercase())
            } else {
                "The Valve Board vented the tank on its own: Vent and Dump open, Pilot closed.".into()
            },
            vented: true,
        }),
        _ => None,
    }
}

#[component]
pub(super) fn AutoVentNotice(status: Signal<StatusObservation>, theme: ThemeConfig) -> Element {
    let Some(n) = notice(&status.read().value) else {
        return rsx! {};
    };
    let (border, background, text) = if n.vented {
        (theme.warning_border.clone(), theme.warning_background.clone(), theme.warning_text.clone())
    } else {
        (theme.error_border.clone(), theme.error_background.clone(), theme.error_text.clone())
    };
    rsx! {
        aside {
            role: if n.vented { "status" } else { "alert" },
            class: "gs26-connection-notice gs26-layout-notice",
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

    #[test]
    fn counting_shows_the_trigger_and_minutes_seconds() {
        let n = notice(&json!({"auto_vent":{"state":"counting","link_lost":true,"continuity_broken":false,"remaining_ms":145_000}})).unwrap();
        assert_eq!(n.title, "Link lost: auto-vent in 2:25");
        assert!(!n.vented);
        let n = notice(&json!({"auto_vent":{"state":"counting","link_lost":false,"continuity_broken":true,"remaining_ms":59_001}})).unwrap();
        assert_eq!(n.title, "Continuity broken: auto-vent in 1:00");
        let n = notice(&json!({"auto_vent":{"state":"counting","link_lost":true,"continuity_broken":true,"remaining_ms":9_000}})).unwrap();
        assert_eq!(n.title, "Link lost, continuity broken: auto-vent in 0:09");
        let n = notice(&json!({"auto_vent":{"state":"counting","link_lost":true,"remaining_ms":0}})).unwrap();
        assert_eq!(n.title, "Link lost: auto-vent due");
    }

    #[test]
    fn vented_and_idle() {
        let n = notice(&json!({"auto_vent":{"state":"vented","link_lost":true,"continuity_broken":false}})).unwrap();
        assert_eq!(n.title, "Auto-vented");
        assert!(n.vented && n.body.contains("(link lost)"));
        assert!(notice(&json!({"auto_vent":{"state":"vented"}})).unwrap().body.starts_with("The Valve Board vented the tank on its own:"));
        assert_eq!(notice(&json!({"auto_vent":{"state":"idle"}})), None);
        assert_eq!(notice(&json!({})), None);
        assert_eq!(notice(&serde_json::Value::Null), None);
    }

    #[test]
    fn clock_rounds_up() {
        assert_eq!(clock(150_000), "2:30");
        assert_eq!(clock(1), "0:01");
        assert_eq!(clock(0), "0:00");
    }
}
