//! The FC launch site (GroundStation26 `docs/integrated-ground-test.md` 6.1b,
//! 7.4): the centre of the FC's GPS acceptance window, its altitude window
//! and the replay CSV checks. The station persists the setting and resends
//! `LAUNCH_SITE` until the FC echoes it; with no setting the FC keeps its
//! compiled site. Admins edit it in Settings; everyone else reads it.
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub(crate) const PATH: &str = "/api/fc/launch-site";
pub(crate) const ALT_MIN_M: f64 = -500.0;
pub(crate) const ALT_MAX_M: f64 = 9000.0;

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Site {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_msl_m: f64,
}

impl Site {
    pub(crate) fn label(&self) -> String {
        format!("{:.6}°, {:.6}°, {:.1} m MSL", self.lat_deg, self.lon_deg, self.alt_msl_m)
    }
}

/// `GET /api/fc/launch-site`.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Status {
    pub configured: Option<Site>,
    pub compiled_default: Option<Site>,
    pub fc_reported: Option<Site>,
    pub confirmed: bool,
    pub pending: bool,
}

/// `PUT /api/fc/launch-site`: a site, or `{reset: true}`.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(untagged)]
pub(crate) enum Request {
    Set(Site),
    Reset { reset: bool },
}

pub(crate) fn reset_request() -> Request {
    Request::Reset { reset: true }
}

/// The FC's acceptance rule: |lat| ≤ 90, |lon| ≤ 180, alt −500..9000 m.
pub(crate) fn validate(site: &Site) -> Result<(), String> {
    if !(site.lat_deg.is_finite() && site.lat_deg.abs() <= 90.0) {
        return Err("Latitude must be between -90 and 90 degrees.".into());
    }
    if !(site.lon_deg.is_finite() && site.lon_deg.abs() <= 180.0) {
        return Err("Longitude must be between -180 and 180 degrees.".into());
    }
    if !(site.alt_msl_m.is_finite() && (ALT_MIN_M..=ALT_MAX_M).contains(&site.alt_msl_m)) {
        return Err("Altitude must be between -500 and 9000 m MSL.".into());
    }
    Ok(())
}

/// The typed fields as a valid site.
pub(crate) fn parse(lat: &str, lon: &str, alt: &str) -> Result<Site, String> {
    let number = |text: &str, name: &str| {
        text.trim().parse::<f64>().map_err(|_| format!("{name} must be a number."))
    };
    let site = Site {
        lat_deg: number(lat, "Latitude")?,
        lon_deg: number(lon, "Longitude")?,
        alt_msl_m: number(alt, "Altitude")?,
    };
    validate(&site)?;
    Ok(site)
}

/// The site the FC uses or will use, in words.
pub(crate) fn state_label(status: &Status) -> String {
    match (status.configured.is_some(), status.confirmed, status.pending) {
        (false, _, _) => "No setting: the FC uses its compiled default".into(),
        (true, true, _) => "Confirmed: the FC reports the configured site".into(),
        (true, false, true) => "Pending: resending until the FC reports the configured site".into(),
        (true, false, false) => "Not confirmed: the FC has not reported the configured site".into(),
    }
}

/// Why the site cannot be edited, or None.
pub(crate) fn edit_disabled_reason(admin: bool, rrls: bool) -> Option<&'static str> {
    if rrls {
        Some("The launch site is set at the primary ground station.")
    } else if !admin {
        Some("Administrator required to change the launch site.")
    } else {
        None
    }
}

fn confirm_lines(request: &Request) -> Vec<String> {
    let mut lines = match request {
        Request::Set(site) => vec![format!("Send the FC launch site {}.", site.label())],
        Request::Reset { .. } => vec!["Clear the setting. The station stops sending a site; the FC returns to its compiled default at its next reset.".into()],
    };
    lines.push("The site is the centre of the FC's ±1° GPS acceptance window and its altitude window, and the replayed ground-test GPS must lie within ±1° of it.".into());
    lines.push("The FC applies it only before Launch (Armed or earlier, no replay running). The station resends it every 10 s until the FC echoes it.".into());
    lines
}

#[component]
pub(super) fn LaunchSiteSettings(theme: super::layout::ThemeConfig) -> Element {
    let rrls = crate::auth::current_status().station_role.as_deref() == Some("rrls");
    let reason = edit_disabled_reason(crate::auth::is_admin(), rrls);
    rsx! { LaunchSiteView { theme, edit_disabled_reason: reason.map(str::to_owned) } }
}

#[component]
pub(super) fn LaunchSiteView(theme: super::layout::ThemeConfig, edit_disabled_reason: Option<String>) -> Element {
    let factory = super::station_settings_request::use_factory();
    let mut status = use_signal(|| None::<Result<Status, String>>);
    let mut revision = use_signal(|| 0u64);
    let mut lat = use_signal(String::new);
    let mut lon = use_signal(String::new);
    let mut alt = use_signal(String::new);
    let mut confirming = use_signal(|| None::<Request>);
    let mut sending = use_signal(|| false);
    let mut result = use_signal(|| None::<(bool, String)>);
    use_future(move || async move {
        let mut seen = u64::MAX;
        loop {
            let current = *revision.peek();
            // Read on open, after a change, and every 5 s for the FC's echo.
            let fetched = factory.get::<Status>(PATH).await;
            if seen != current {
                if let Ok(s) = fetched.as_ref() {
                    if let Some(site) = s.configured.or(s.compiled_default) {
                        lat.set(format!("{}", site.lat_deg));
                        lon.set(format!("{}", site.lon_deg));
                        alt.set(format!("{}", site.alt_msl_m));
                    }
                    seen = current;
                }
            }
            status.set(Some(fetched));
            for _ in 0..20 {
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
    let editable = edit_disabled_reason.is_none();
    let parsed = parse(&lat.read(), &lon.read(), &alt.read());
    let row = |label: &str, site: Option<Site>| {
        (label.to_owned(), site.map_or_else(|| "—".to_owned(), |s| s.label()))
    };
    let mut send = move |request: Request| {
        if *sending.peek() {
            return;
        }
        let owner = factory.capture();
        sending.set(true);
        spawn(async move {
            let reply = super::station_settings_request::put_captured::<_, serde_json::Value>(PATH, &request, owner).await;
            sending.set(false);
            confirming.set(None);
            match reply {
                Ok(value) => {
                    if let Ok(s) = serde_json::from_value::<Status>(value) {
                        if s.compiled_default.is_some() {
                            status.set(Some(Ok(s)));
                        }
                    }
                    result.set(Some((true, match request {
                        Request::Set(_) => "Launch site saved. The station sends it to the FC.".into(),
                        Request::Reset { .. } => "Launch site reset to the compiled default.".into(),
                    })));
                    revision += 1;
                }
                Err(e) => result.set(Some((false, format!("Launch site not changed: {}", super::igt::refusal(&e))))),
            }
        });
    };
    let dialog = confirming().map(|request| {
        let title = match request {
            Request::Set(_) => "Change the FC launch site?",
            Request::Reset { .. } => "Reset the launch site to the compiled default?",
        };
        (request, title.to_string(), confirm_lines(&request))
    });
    rsx! {
        section { class: "gs26-launch-site", "data-launch-site": "", style: "display:grid;gap:10px;min-width:0;",
            h3 { style: "margin:0;", "Launch site" }
            p { style: "margin:0;color:{theme.text_muted};", "The FC's launch site: the centre of its GPS acceptance window and altitude window, and the reference for the ground-test replay." }
            match status() {
                None => rsx! { p { role: "status", "Reading the launch site…" } },
                Some(Err(e)) => rsx! { p { role: "status", class: "gs-notice gs-notice--warn", "Cannot read the launch site: {super::igt::refusal(&e)}" } },
                Some(Ok(s)) => {
                    let rows = [row("Compiled default", s.compiled_default), row("Configured", s.configured), row("FC reports", s.fc_reported)];
                    let state = state_label(&s);
                    let warn = s.configured.is_some() && !s.confirmed;
                    rsx! {
                        dl { style: "display:grid;grid-template-columns:max-content minmax(0,1fr);gap:4px 12px;margin:0;",
                            for (label, value) in rows {
                                div { key: "{label}", style: "display:contents;",
                                    dt { style: "font-weight:700;", "{label}" }
                                    dd { style: "margin:0;overflow-wrap:anywhere;", "{value}" }
                                }
                            }
                        }
                        p { role: "status", "data-launch-site-state": if s.confirmed { "confirmed" } else if s.pending { "pending" } else { "unconfirmed" },
                            class: if warn { "gs-notice gs-notice--warn" } else { "gs-notice" }, style: "margin:0;", "{state}" }
                    }
                }
            }
            if let Some(reason) = edit_disabled_reason.clone() {
                p { role: "status", style: "margin:0;color:{theme.text_muted};", "data-disabled-reason": "", "Read only: {reason}" }
            } else {
                div { style: "display:flex;flex-wrap:wrap;gap:8px;align-items:end;",
                    for (id, label, signal) in [("lat", "Latitude (°)", lat), ("lon", "Longitude (°)", lon), ("alt", "Altitude (m MSL)", alt)] {
                        label { key: "{id}", r#for: "gs26-launch-site-{id}", style: "display:grid;gap:2px;font-weight:700;",
                            "{label}"
                            input { id: "gs26-launch-site-{id}", r#type: "text", inputmode: "decimal", value: "{signal}", disabled: sending(),
                                style: "font:inherit;min-height:36px;width:12ch;padding:4px 8px;",
                                oninput: move |event| { let mut signal = signal; signal.set(event.value()); },
                            }
                        }
                    }
                }
                if let Err(problem) = parsed.as_ref() {
                    p { role: "status", class: "gs-notice gs-notice--warn", "data-launch-site-invalid": "", style: "margin:0;", "{problem}" }
                }
                div { style: "display:flex;flex-wrap:wrap;gap:8px;",
                    button { r#type: "button", class: "gs-btn gs-btn--primary", disabled: !editable || parsed.is_err() || sending(),
                        onclick: move |_| {
                            if let Ok(site) = parse(&lat.peek(), &lon.peek(), &alt.peek()) {
                                result.set(None);
                                confirming.set(Some(Request::Set(site)));
                            }
                        },
                        "Set launch site…"
                    }
                    button { r#type: "button", class: "gs-btn", disabled: !editable || sending(),
                        onclick: move |_| { result.set(None); confirming.set(Some(reset_request())); },
                        "Reset to compiled default…"
                    }
                }
            }
            if let Some((ok, text)) = result() {
                p { role: if ok { "status" } else { "alert" }, class: if ok { "gs-notice" } else { "gs-notice gs-notice--danger" }, style: "margin:0;", "{text}" }
            }
        }
        if let Some((request, title, lines)) = dialog {
            super::igt_view::ConfirmDialog { id: "gs26-launch-site-confirm", title, lines,
                confirm_label: match request { Request::Set(_) => "Set launch site", Request::Reset { .. } => "Reset launch site" }.to_string(),
                busy: sending(),
                on_cancel: move |_| confirming.set(None),
                on_confirm: move |_| send(request),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn status_decodes_the_contract_shape_and_defaults() {
        let s: Status = serde_json::from_value(json!({
            "configured": {"lat_deg": 32.99, "lon_deg": -106.97, "alt_msl_m": 1401.0},
            "compiled_default": {"lat_deg": 31.758, "lon_deg": -102.046, "alt_msl_m": 890.016},
            "fc_reported": null, "confirmed": false, "pending": true
        }))
        .unwrap();
        assert_eq!(s.compiled_default, Some(Site { lat_deg: 31.758, lon_deg: -102.046, alt_msl_m: 890.016 }));
        assert_eq!(s.fc_reported, None);
        assert!(state_label(&s).starts_with("Pending"));
        let empty: Status = serde_json::from_value(json!({})).unwrap();
        assert_eq!(empty, Status::default());
        assert!(state_label(&empty).contains("compiled default"));
        let confirmed = Status { configured: s.configured, confirmed: true, ..Status::default() };
        assert!(state_label(&confirmed).starts_with("Confirmed"));
    }

    #[test]
    fn validation_matches_the_fc_rule() {
        assert!(parse("31.758", "-102.046", "890.016").is_ok());
        assert!(parse("90", "180", "9000").is_ok());
        assert!(parse("-90", "-180", "-500").is_ok());
        assert!(parse("90.01", "0", "0").unwrap_err().contains("Latitude"));
        assert!(parse("0", "-180.5", "0").unwrap_err().contains("Longitude"));
        assert!(parse("0", "0", "9000.1").unwrap_err().contains("Altitude"));
        assert!(parse("0", "0", "-501").unwrap_err().contains("Altitude"));
        assert!(parse("abc", "0", "0").unwrap_err().contains("number"));
        assert!(parse("NaN", "0", "0").is_err());
        assert!(parse("0", "inf", "0").is_err());
        assert!(parse("", "0", "0").is_err());
    }

    #[test]
    fn request_bodies() {
        let site = parse(" 32.5 ", "-106.9", "1400").unwrap();
        assert_eq!(serde_json::to_value(Request::Set(site)).unwrap(), json!({"lat_deg": 32.5, "lon_deg": -106.9, "alt_msl_m": 1400.0}));
        assert_eq!(serde_json::to_value(reset_request()).unwrap(), json!({"reset": true}));
    }

    #[test]
    fn only_admins_edit() {
        assert_eq!(edit_disabled_reason(true, false), None);
        assert!(edit_disabled_reason(false, false).unwrap().contains("Administrator"));
        assert!(edit_disabled_reason(true, true).is_some());
    }

    fn render(reason: Option<&str>) -> String {
        let mut dom = VirtualDom::new_with_props(
            |reason: Option<String>| rsx! { LaunchSiteView { theme: super::super::layout::ThemeConfig::default(), edit_disabled_reason: reason } },
            reason.map(str::to_owned),
        );
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn non_admins_see_it_read_only() {
        let page = render(Some("Administrator required to change the launch site."));
        assert!(page.contains("Read only: Administrator required"), "{page}");
        assert!(!page.contains("Set launch site") && !page.contains("Reset to compiled default"));
        let admin = render(None);
        assert!(admin.contains("Set launch site") && admin.contains("Reset to compiled default"), "{admin}");
    }
}
