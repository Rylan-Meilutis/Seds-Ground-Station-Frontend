//! Fire archive review (GroundStation26 `docs/apogee-planner.md` section 9):
//! every measured burn becomes a pending record; an operator accepts or
//! rejects it, excludes it from the motor fit, and corrects its metadata
//! before the apogee planner's motor model is fitted from it.
use super::burn_report_view::{Flag, ThrustPlot, encode_segment, severity_class};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub(crate) const PATH: &str = "/api/fire_archive";

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Review {
    pub status: String,
    pub exclude_from_fit: bool,
    pub notes: String,
    pub reviewed_by: Option<String>,
    pub reviewed_ms: Option<i64>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Fire {
    pub id: String,
    pub label: String,
    pub date: String,
    pub source: String,
    pub burn_report_id: Option<String>,
    pub fill_kg: Option<f64>,
    pub fill_before_kg: Option<f64>,
    pub fill_after_kg: Option<f64>,
    pub tank_psi_at_ignition: Option<f64>,
    pub tank_temp_c: Option<f64>,
    pub supercharge_psi: Option<f64>,
    pub ambient_c: Option<f64>,
    pub weather: Option<serde_json::Value>,
    pub impulse_ns: Option<f64>,
    pub impulse_sigma_ns: Option<f64>,
    pub burn_time_s: Option<f64>,
    pub peak_thrust_n: Option<f64>,
    pub chamber_valid: bool,
    pub result: String,
    pub applied_plan: Option<serde_json::Value>,
    pub quality_flags: Vec<Flag>,
    pub simulated: bool,
    pub review: Review,
    pub in_fit: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Archive {
    pub fires: Vec<Fire>,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub(crate) struct Curve {
    pub curve: Vec<[f64; 2]>,
}

/// `PATCH /api/fire_archive/{id}`: only the fields that change.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub(crate) struct Patch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_from_fit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_kg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambient_c: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tank_psi_at_ignition: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supercharge_psi: Option<f64>,
}
impl Patch {
    pub(crate) fn is_empty(&self) -> bool {
        *self == Patch::default()
    }
}

pub(crate) fn record_path(id: &str) -> String {
    format!("{PATH}/{}", encode_segment(id))
}

pub(crate) fn thrust_path(id: &str) -> String {
    format!("{}/thrust", record_path(id))
}

fn num(v: Option<f64>, unit: &str, digits: usize) -> String {
    v.map_or_else(|| "—".to_owned(), |v| format!("{v:.digits$} {unit}"))
}

pub(crate) fn review_label(fire: &Fire) -> String {
    let base = match fire.review.status.as_str() {
        "accepted" => "Accepted",
        "rejected" => "Rejected",
        _ => "Pending review",
    };
    let mut label = base.to_owned();
    if fire.simulated {
        label.push_str(", simulated");
    }
    if fire.review.exclude_from_fit {
        label.push_str(", excluded from fit");
    }
    label.push_str(if fire.in_fit { " — used by the motor fit" } else { " — not in the fit" });
    label
}

/// The metadata edits typed in the form (text fields), as a patch against `fire`.
pub(crate) fn metadata_patch(fire: &Fire, label: &str, fill: &str, ambient: &str, tank: &str, supercharge: &str, notes: &str) -> Result<Patch, String> {
    let parse = |text: &str, name: &str, old: Option<f64>| -> Result<Option<f64>, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let v: f64 = text.parse().ok().filter(|v: &f64| v.is_finite()).ok_or_else(|| format!("{name} must be a number."))?;
        Ok((old.is_none_or(|o| (o - v).abs() > 1e-9)).then_some(v))
    };
    Ok(Patch {
        label: (label.trim() != fire.label && !label.trim().is_empty()).then(|| label.trim().to_owned()),
        fill_kg: parse(fill, "Fill", fire.fill_kg)?,
        ambient_c: parse(ambient, "Ambient temperature", fire.ambient_c)?,
        tank_psi_at_ignition: parse(tank, "Tank pressure", fire.tank_psi_at_ignition)?,
        supercharge_psi: parse(supercharge, "Supercharge pressure", fire.supercharge_psi)?,
        notes: (notes != fire.review.notes).then(|| notes.to_owned()),
        ..Patch::default()
    })
}

#[component]
pub(super) fn FireArchiveSettings(theme: super::layout::ThemeConfig) -> Element {
    let factory = super::station_settings_request::use_factory();
    let mut archive = use_signal(|| None::<Result<Archive, String>>);
    let mut revision = use_signal(|| 0u64);
    let mut result = use_signal(|| None::<(bool, String)>);
    let mut busy = use_signal(|| false);
    use_effect(move || {
        let _ = revision();
        let request = factory.get::<Archive>(PATH);
        spawn(async move { archive.set(Some(request.await)) });
    });
    let mut send = move |id: String, patch: Patch| {
        if *busy.peek() || patch.is_empty() {
            return;
        }
        busy.set(true);
        let owner = factory.capture();
        let work = super::station_settings_request::patch_captured::<_, Fire>(&record_path(&id), &patch, owner);
        spawn(async move {
            match work.await {
                Ok(_) => result.set(Some((true, "Fire record updated.".into()))),
                Err(e) => result.set(Some((false, format!("Fire record not changed: {}", super::igt::refusal(&e))))),
            }
            busy.set(false);
            revision += 1;
        });
    };
    rsx! {
        FireArchiveView { theme, archive: archive(), can_review: crate::auth::can_view_actions(), busy: busy(), result: result(),
            on_patch: move |(id, patch): (String, Patch)| send(id, patch) }
    }
}

#[component]
pub(super) fn FireArchiveView(
    theme: super::layout::ThemeConfig,
    archive: Option<Result<Archive, String>>,
    can_review: bool,
    busy: bool,
    result: Option<(bool, String)>,
    on_patch: EventHandler<(String, Patch)>,
) -> Element {
    rsx! {
        details { class: "gs26-fire-archive", "data-fire-archive": "", style: "min-width:0;",
            summary { style: "font-weight:700;", "Fire archive" }
            p { style: "margin:4px 0;color:{theme.text_muted};",
                "Each measured burn is kept here. Accepted, non-simulated fires that are not excluded feed the apogee planner's motor fit." }
            match archive {
                None => rsx! { p { role: "status", "Reading the fire archive…" } },
                Some(Err(e)) => rsx! { p { role: "status", class: "gs-notice gs-notice--warn", "Cannot read the fire archive: {super::igt::refusal(&e)}" } },
                Some(Ok(a)) if a.fires.is_empty() => rsx! { p { role: "status", "No fires archived yet." } },
                Some(Ok(a)) => rsx! {
                    for fire in a.fires.iter().cloned() {
                        FireRow { key: "{fire.id}", theme: theme.clone(), fire, can_review, busy, on_patch }
                    }
                },
            }
            if let Some((ok, text)) = result {
                p { role: if ok { "status" } else { "alert" }, class: if ok { "gs-notice" } else { "gs-notice gs-notice--danger" }, style: "margin:0;", "{text}" }
            }
        }
    }
}

#[component]
fn FireRow(theme: super::layout::ThemeConfig, fire: Fire, can_review: bool, busy: bool, on_patch: EventHandler<(String, Patch)>) -> Element {
    let factory = super::station_settings_request::use_factory();
    let mut curve = use_signal(|| None::<Result<Curve, String>>);
    let label = use_signal(|| fire.label.clone());
    let fill = use_signal(|| fire.fill_kg.map(|v| format!("{v}")).unwrap_or_default());
    let ambient = use_signal(|| fire.ambient_c.map(|v| format!("{v}")).unwrap_or_default());
    let tank = use_signal(|| fire.tank_psi_at_ignition.map(|v| format!("{v}")).unwrap_or_default());
    let supercharge = use_signal(|| fire.supercharge_psi.map(|v| format!("{v}")).unwrap_or_default());
    let mut notes = use_signal(|| fire.review.notes.clone());
    let mut problem = use_signal(|| None::<String>);
    let id = fire.id.clone();
    let summary_line = format!(
        "{} {} — {} , fill {}",
        fire.date,
        fire.label,
        num(fire.impulse_ns, "N·s", 0),
        num(fire.fill_kg, "kg", 2)
    );
    let state = review_label(&fire);
    let disabled = !can_review || busy;
    let load_curve = {
        let id = id.clone();
        move |_| {
            if curve.peek().is_some() {
                return;
            }
            let request = factory.get::<Curve>(&thrust_path(&id));
            spawn(async move { curve.set(Some(request.await)) });
        }
    };
    let set_status = |status: &'static str| {
        let id = id.clone();
        move |_| on_patch.call((id.clone(), Patch { status: Some(status.into()), ..Patch::default() }))
    };
    let toggle_exclude = {
        let id = id.clone();
        let exclude = !fire.review.exclude_from_fit;
        move |_| on_patch.call((id.clone(), Patch { exclude_from_fit: Some(exclude), ..Patch::default() }))
    };
    let save_meta = {
        let id = id.clone();
        let fire = fire.clone();
        move |_| match metadata_patch(&fire, &label.peek(), &fill.peek(), &ambient.peek(), &tank.peek(), &supercharge.peek(), &notes.peek()) {
            Ok(patch) if patch.is_empty() => problem.set(Some("Nothing changed.".into())),
            Ok(patch) => {
                problem.set(None);
                on_patch.call((id.clone(), patch));
            }
            Err(e) => problem.set(Some(e)),
        }
    };
    let input_style = "font:inherit;min-height:32px;width:11ch;padding:2px 6px;";
    rsx! {
        details { "data-fire": "{fire.id}", style: "border:1px solid {theme.border_soft};border-radius:8px;padding:6px 10px;margin:4px 0;", ontoggle: load_curve,
            summary { "{summary_line}" span { "data-review-state": "{fire.review.status}", style: "color:{theme.text_muted};", " ({state})" } }
            dl { style: "display:grid;grid-template-columns:max-content minmax(0,1fr);gap:2px 12px;margin:6px 0;",
                dt { "Source" } dd { style: "margin:0;", "{fire.source}" }
                dt { "Fill before / at ignition / after" } dd { style: "margin:0;", "{num(fire.fill_before_kg, \"kg\", 2)} / {num(fire.fill_kg, \"kg\", 2)} / {num(fire.fill_after_kg, \"kg\", 2)}" }
                dt { "Tank at ignition" } dd { style: "margin:0;", "{num(fire.tank_psi_at_ignition, \"psi\", 0)}, {num(fire.tank_temp_c, \"°C\", 1)}" }
                dt { "Supercharge / ambient" } dd { style: "margin:0;", "{num(fire.supercharge_psi, \"psi\", 0)} / {num(fire.ambient_c, \"°C\", 1)}" }
                dt { "Impulse" } dd { style: "margin:0;", "{num(fire.impulse_ns, \"N·s\", 0)} ± {num(fire.impulse_sigma_ns, \"N·s\", 0)}, burn {num(fire.burn_time_s, \"s\", 2)}, peak {num(fire.peak_thrust_n, \"N\", 0)}" }
                dt { "Chamber PT" } dd { style: "margin:0;", if fire.chamber_valid { "valid" } else { "not valid" } }
                dt { "Result" } dd { style: "margin:0;", "{fire.result}" }
                if fire.applied_plan.is_some() { dt { "Applied plan" } dd { style: "margin:0;", "recorded" } }
            }
            for (i, f) in fire.quality_flags.iter().enumerate() {
                p { key: "{i}", class: severity_class(&f.severity), style: "margin:2px 0;", "{f.message}" }
            }
            match curve() {
                Some(Ok(c)) if !c.curve.is_empty() => rsx! { ThrustPlot { curve: c.curve, theme: theme.clone() } },
                Some(Err(e)) => rsx! { p { class: "gs-notice gs-notice--warn", "Cannot read the thrust curve: {super::igt::refusal(&e)}" } },
                _ => rsx! {},
            }
            if can_review {
                div { "data-review-controls": "", style: "display:flex;flex-wrap:wrap;gap:6px;margin:6px 0;",
                    button { r#type: "button", class: "gs-btn", disabled: disabled || fire.review.status == "accepted", onclick: set_status("accepted"), "Accept" }
                    button { r#type: "button", class: "gs-btn", disabled: disabled || fire.review.status == "rejected", onclick: set_status("rejected"), "Reject" }
                    button { r#type: "button", class: "gs-btn", disabled, onclick: toggle_exclude,
                        if fire.review.exclude_from_fit { "Include in fit" } else { "Exclude from fit" } }
                }
                div { style: "display:flex;flex-wrap:wrap;gap:6px;align-items:end;",
                    for (name, signal) in [("Label", label), ("Fill at ignition (kg)", fill), ("Ambient (°C)", ambient), ("Tank (psi)", tank), ("Supercharge (psi)", supercharge)] {
                        label { key: "{name}", style: "display:grid;gap:2px;font-weight:700;", "{name}"
                            input { r#type: "text", value: "{signal}", style: input_style, disabled, oninput: move |e| { let mut s = signal; s.set(e.value()); } }
                        }
                    }
                    label { style: "display:grid;gap:2px;font-weight:700;flex:1 1 220px;", "Notes"
                        textarea { value: "{notes}", disabled, rows: "2", oninput: move |e| notes.set(e.value()) }
                    }
                    button { r#type: "button", class: "gs-btn gs-btn--primary", disabled, onclick: save_meta, "Save metadata" }
                }
                if let Some(p) = problem() {
                    p { role: "alert", class: "gs-notice gs-notice--warn", style: "margin:0;", "{p}" }
                }
            } else {
                p { style: "margin:0;color:{theme.text_muted};", "Command permission is required to review fires." }
            }
            if let Some(by) = fire.review.reviewed_by.clone() {
                p { style: "margin:0;color:{theme.text_muted};font-size:12px;", "Reviewed by {by}." }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fire(status: &str) -> Fire {
        serde_json::from_value(json!({
            "id": "2026-11-16_static", "label": "2026-11-16 static fire", "date": "2026-11-16", "source": "seed",
            "fill_kg": 5.65, "tank_psi_at_ignition": 404.0, "ambient_c": null, "impulse_ns": 9192.0, "impulse_sigma_ns": 900.0,
            "burn_time_s": 5.65, "peak_thrust_n": 3295.0, "result": "static fire",
            "quality_flags": [{"code": "kg50_slope_unverified", "severity": "warn", "message": "KG50 slope unverified"}],
            "review": {"status": status, "exclude_from_fit": false, "notes": ""}, "in_fit": status == "accepted"
        }))
        .unwrap()
    }

    #[test]
    fn decode_label_and_patch() {
        let f = fire("accepted");
        assert_eq!(review_label(&f), "Accepted — used by the motor fit");
        let mut p = fire("pending");
        p.simulated = true;
        p.review.exclude_from_fit = true;
        assert_eq!(review_label(&p), "Pending review, simulated, excluded from fit — not in the fit");
        let empty: Archive = serde_json::from_value(json!({"fires": [{}]})).unwrap();
        assert_eq!(empty.fires[0].review.status, "");
        let patch = metadata_patch(&f, "2026-11-16 static fire", "5.65", "4", "404", "", "").unwrap();
        assert_eq!(serde_json::to_value(&patch).unwrap(), json!({"ambient_c": 4.0}));
        assert!(metadata_patch(&f, "x", "abc", "", "", "", "").unwrap_err().contains("Fill"));
        let notes = metadata_patch(&f, "", "", "", "", "", "bad tare").unwrap();
        assert_eq!(serde_json::to_value(&notes).unwrap(), json!({"notes": "bad tare"}));
        assert!(metadata_patch(&f, "", "5.65", "", "404", "", "").unwrap().is_empty());
        assert_eq!(serde_json::to_value(Patch { status: Some("rejected".into()), ..Patch::default() }).unwrap(), json!({"status": "rejected"}));
        assert_eq!(thrust_path("a b"), "/api/fire_archive/a%20b/thrust");
    }

    fn render(can_review: bool) -> String {
        let mut dom = VirtualDom::new_with_props(
            |can_review: bool| rsx! {
                FireArchiveView { theme: super::super::layout::ThemeConfig::default(),
                    archive: Some(Ok(Archive { fires: vec![fire("pending"), fire("accepted")] })),
                    can_review, busy: false, result: None, on_patch: |_| {} }
            },
            can_review,
        );
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn review_controls_only_with_command_permission() {
        let page = render(true);
        assert!(page.contains("data-review-controls") && page.contains("Exclude from fit") && page.contains("Save metadata"), "{page}");
        assert!(page.contains("Pending review") && page.contains("KG50 slope unverified"));
        let viewer = render(false);
        assert!(!viewer.contains("data-review-controls"));
        assert!(viewer.contains("Command permission is required to review fires."));
    }
}
