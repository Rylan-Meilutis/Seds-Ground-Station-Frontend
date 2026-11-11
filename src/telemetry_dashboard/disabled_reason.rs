//! The short reason a disabled control is unavailable, shown as text under
//! or beside the control, not only as a hover title.
use dioxus::prelude::*;

/// The first reason that applies, in priority order.
pub(super) fn first_reason(checks: &[(bool, &str)]) -> Option<String> {
    checks
        .iter()
        .find(|(blocked, _)| *blocked)
        .map(|(_, reason)| (*reason).to_string())
}

/// "<subject> unavailable: <reason>" in the app's small muted style, or
/// nothing when the control is available.
#[component]
pub(super) fn DisabledReason(
    #[props(default)] subject: Option<String>,
    reason: Option<String>,
    #[props(default)] control: Option<String>,
) -> Element {
    let Some(reason) = reason.filter(|reason| !reason.trim().is_empty()) else {
        return rsx! {};
    };
    let text = match subject {
        Some(subject) => format!("{subject} unavailable: {reason}"),
        None => reason,
    };
    rsx! {
        span { class: "gs-disabled-reason", "data-control": control.unwrap_or_default(), "{text}" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_applicable_reason_wins() {
        assert_eq!(first_reason(&[(false, "a"), (true, "b"), (true, "c")]).as_deref(), Some("b"));
        assert_eq!(first_reason(&[(false, "a")]), None);
    }

    fn render(reason: Option<&str>, subject: Option<&str>) -> String {
        let reason = reason.map(str::to_string);
        let subject = subject.map(str::to_string);
        let mut dom = VirtualDom::new_with_props(
            |props: (Option<String>, Option<String>)| rsx! { DisabledReason { subject: props.1, reason: props.0 } },
            (reason, subject),
        );
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn reason_is_visible_text_and_absent_when_enabled() {
        let html = render(Some("Not permitted by your account"), Some("Query"));
        assert!(html.contains("gs-disabled-reason"), "{html}");
        assert!(html.contains("Query unavailable: Not permitted by your account"), "{html}");
        assert_eq!(render(None, Some("Query")), "");
        assert_eq!(render(Some("  "), None), "");
    }
}
