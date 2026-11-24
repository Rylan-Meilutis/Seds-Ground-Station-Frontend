//! The flight simulation panel: run a flight or burn simulation on the
//! station, list stored plans, view one, export it, show it on the map or
//! play it in the offline replay workspace. Running is read-only computation
//! (any station mode); the station decides who may run or delete and its
//! refusal text is shown as is.
use super::flight_plans::{self, Form, PlanDoc, PlanList, PlanSummary};
use super::layout::ThemeConfig;
use super::map_overlays;
use dioxus::prelude::*;

/// The plan drawn on the map (shared by the map tab legend and this panel).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ShownPlan {
    pub id: String,
    pub label: String,
}
pub(crate) static SHOWN_PLAN: GlobalSignal<Option<ShownPlan>> = Signal::global(|| None);

/// Bundles larger than this are refused before opening.
const MAX_BUNDLE_BYTES: usize = 192 << 20;

pub(crate) fn show_on_map(plan: &PlanDoc) -> Result<(), String> {
    let overlay = plan.map_overlay().ok_or("This plan has no trajectory to draw (burn plans are motor-only).")?;
    map_overlays::set(map_overlays::PROVIDER_FLIGHT_PLAN, std::slice::from_ref(overlay));
    *SHOWN_PLAN.write() = Some(ShownPlan { id: plan.id.clone(), label: flight_plans::predicted_label(&overlay.label) });
    Ok(())
}

pub(crate) fn hide_from_map() {
    map_overlays::set(map_overlays::PROVIDER_FLIGHT_PLAN, &[]);
    *SHOWN_PLAN.write() = None;
}

async fn sleep_ms(ms: u64) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

/// Why Run/Delete are unavailable, or None.
pub(crate) fn command_reason(can_send: bool) -> Option<&'static str> {
    (!can_send).then_some("Running or deleting a plan needs command permission (operator or administrator).")
}

#[component]
pub(super) fn FlightSimSettings(theme: ThemeConfig) -> Element {
    rsx! { FlightSimView { theme, can_send: crate::auth::can_view_actions(), initial: None } }
}

type FieldSetter = fn(&mut Form, String);

#[component]
pub(super) fn FlightSimView(
    theme: ThemeConfig,
    can_send: bool,
    /// Seeded list and selected plan (tests and previews); None reads the station.
    initial: Option<(PlanList, Option<PlanDoc>)>,
) -> Element {
    let factory = super::station_settings_request::use_factory();
    let seeded = initial.is_some();
    let mut list = use_signal({
        let initial = initial.clone();
        move || initial.map(|(l, _)| Ok::<PlanList, String>(l))
    });
    let mut form = use_signal(|| None::<Form>);
    let mut selected = use_signal({
        let initial = initial.clone();
        move || initial.and_then(|(_, p)| p)
    });
    let mut busy = use_signal(|| None::<&'static str>);
    let mut elapsed_s = use_signal(|| 0u32);
    let mut result = use_signal(|| None::<(bool, String)>);
    let mut revision = use_signal(|| 0u64);
    use_future(move || async move {
        if seeded {
            return;
        }
        loop {
            let current = *revision.peek();
            let fetched = factory.get::<PlanList>(flight_plans::LIST_PATH).await;
            list.set(Some(fetched));
            for _ in 0..120 {
                sleep_ms(250).await;
                if *revision.peek() != current {
                    break;
                }
            }
        }
    });
    let loaded = list.read().as_ref().and_then(|l| l.as_ref().ok()).cloned();
    if let Some(l) = loaded.as_ref()
        && form.peek().is_none()
    {
        form.set(Some(Form::from_request(&l.defaults)));
    }
    let Some(l) = loaded else {
        return rsx! {
            section { class: "gs26-flight-sim", style: "display:grid;gap:10px;min-width:0;",
                h3 { style: "margin:0;", "Flight simulation" }
                match list() {
                    Some(Err(e)) => rsx! { p { role: "status", class: "gs-notice gs-notice--warn", "Cannot read flight plans: {super::igt::refusal(&e)}" } },
                    _ => rsx! { p { role: "status", "Reading flight plans…" } },
                }
            }
        };
    };
    let f = form().unwrap_or_else(|| Form::from_request(&l.defaults));
    let reason = command_reason(can_send);
    let running = busy().is_some();
    let mut set_field = move |set: FieldSetter, value: String| {
        let mut next = form.peek().clone().unwrap_or_default();
        set(&mut next, value);
        form.set(Some(next));
    };

    // One station action at a time; the reply refreshes the list.
    let run = move |_| {
        if busy.peek().is_some() {
            return;
        }
        let limits = list.peek().as_ref().and_then(|l| l.as_ref().ok()).map(|l| l.limits.clone()).unwrap_or_default();
        let request = match flight_plans::build_request(&form.peek().clone().unwrap_or_default(), &limits) {
            Ok(r) => r,
            Err(e) => {
                result.set(Some((false, e)));
                return;
            }
        };
        let owner = factory.capture();
        let work = super::station_settings_request::post_captured::<_, PlanDoc>(flight_plans::RUN_PATH, &request, owner);
        busy.set(Some("Running the simulation"));
        elapsed_s.set(0);
        result.set(None);
        let started = super::current_wallclock_ms();
        let ticker = spawn(async move {
            loop {
                sleep_ms(500).await;
                elapsed_s.set(((super::current_wallclock_ms() - started) / 1000).max(0) as u32);
            }
        });
        spawn(async move {
            let reply = work.await;
            ticker.cancel();
            busy.set(None);
            match reply {
                Ok(plan) => {
                    result.set(Some((true, format!("{} computed.", flight_plans::predicted_label(&plan.label)))));
                    selected.set(Some(plan));
                }
                Err(e) => result.set(Some((false, format!("The station did not run the plan: {}", super::igt::refusal(&e))))),
            }
            revision += 1;
        });
    };
    let mut view = move |id: String| {
        if busy.peek().is_some() {
            return;
        }
        let work = factory.get::<PlanDoc>(&flight_plans::plan_path(&id));
        busy.set(Some("Reading the plan"));
        result.set(None);
        spawn(async move {
            let reply = work.await;
            busy.set(None);
            match reply {
                Ok(plan) => selected.set(Some(plan)),
                Err(e) => result.set(Some((false, format!("Cannot read the plan: {}", super::igt::refusal(&e))))),
            }
        });
    };
    let mut delete = move |id: String| {
        if busy.peek().is_some() {
            return;
        }
        let owner = factory.capture();
        let work = super::station_settings_request::delete_captured::<serde_json::Value>(&flight_plans::plan_path(&id), owner);
        busy.set(Some("Deleting the plan"));
        result.set(None);
        spawn(async move {
            let reply = work.await;
            busy.set(None);
            match reply {
                Ok(_) => {
                    if selected.peek().as_ref().is_some_and(|p| p.id == id) {
                        selected.set(None);
                    }
                    if SHOWN_PLAN.peek().as_ref().is_some_and(|p| p.id == id) {
                        hide_from_map();
                    }
                    result.set(Some((true, "Plan deleted.".into())));
                }
                Err(e) => result.set(Some((false, format!("The station did not delete the plan: {}", super::igt::refusal(&e))))),
            }
            revision += 1;
        });
    };
    let mut export = move |id: String, csv: bool| {
        let owner = match factory.capture() {
            Ok(owner) => owner,
            Err(e) => {
                result.set(Some((false, e)));
                return;
            }
        };
        let (path, name) = if csv {
            (flight_plans::csv_path(&id), flight_plans::file_name(&id, "csv"))
        } else {
            (flight_plans::json_path(&id), flight_plans::file_name(&id, "json"))
        };
        spawn(async move {
            let reply = super::recording_download::download_csv_path_owned(&path, &name, owner).await;
            result.set(Some(match reply {
                Ok(message) => (true, message),
                Err(e) => (false, format!("Export failed: {}", super::igt::refusal(&e))),
            }));
        });
    };
    // Absent only in isolated renders (tests): Play then reports the error.
    let router = use_hook(dioxus_router::try_router);
    let mut play = move |id: String| {
        if busy.peek().is_some() {
            return;
        }
        let owner = match factory.capture() {
            Ok(owner) => owner,
            Err(e) => {
                result.set(Some((false, e)));
                return;
            }
        };
        busy.set(Some("Downloading the replay bundle"));
        result.set(None);
        spawn(async move {
            let reply = super::recording_download::saved_bytes(&flight_plans::bundle_path(&id), owner, MAX_BUNDLE_BYTES).await;
            busy.set(None);
            let opened = reply.map_err(|e| super::igt::refusal(&e)).and_then(|bytes| {
                // Check it now so a bad bundle is reported here, not as a silent demo.
                let files = super::local_station::bundle::unzip(&bytes)?;
                super::local_station::bundle::verify(&files)?;
                Ok(bytes)
            });
            match opened {
                Ok(bytes) => {
                    let Some(router) = router.clone() else {
                        result.set(Some((false, "The replay workspace is unavailable here.".into())));
                        return;
                    };
                    super::offline_preview::queue_bundle(flight_plans::file_name(&id, "gsreplay"), bytes);
                    let _ = router.push(crate::app::Route::Preview {});
                }
                Err(e) => result.set(Some((false, format!("Cannot play the plan: {e}")))),
            }
        });
    };
    let mut map = move |plan: PlanDoc| match show_on_map(&plan) {
        Ok(()) => result.set(Some((true, "Shown on the Map tab with the live track.".into()))),
        Err(e) => result.set(Some((false, e))),
    };

    let label_style = "display:grid;gap:2px;font-weight:700;";
    let input_style = "font:inherit;min-height:36px;width:11ch;padding:4px 8px;";
    let row_style = "display:flex;flex-wrap:wrap;gap:8px;align-items:end;";
    let fieldset_style = format!("display:grid;gap:8px;border:1px solid {};border-radius:10px;padding:10px;min-width:0;", theme.border_soft);
    let numbers = |items: Vec<(&'static str, &'static str, String, FieldSetter)>| -> Element {
        rsx! {
            div { style: row_style,
                for (key, label, value, set) in items {
                    label { key: "{key}", style: label_style, "{label}"
                        input { r#type: "text", inputmode: "decimal", value: "{value}", style: input_style, "data-field": key,
                            oninput: move |e| set_field(set, e.value()) }
                    }
                }
            }
        }
    };
    let flight = f.kind != "burn";
    let motor_models: Vec<String> = {
        let mut v = vec!["physics".to_string(), "scaling".to_string()];
        if !f.motor_model.is_empty() && !v.contains(&f.motor_model) {
            v.insert(0, f.motor_model.clone());
        }
        v
    };
    let selected_plan = selected();
    let shown = SHOWN_PLAN.read().clone();

    rsx! {
        section { class: "gs26-flight-sim", "data-flight-sim": "", style: "display:grid;gap:10px;min-width:0;",
            h3 { style: "margin:0;", "Flight simulation" }
            p { style: "margin:0;color:{theme.text_muted};",
                "Runs a flight or burn simulation on the station from the inputs below and stores it as a plan. It only computes; nothing is actuated, and it is allowed in any station mode."
            }
            if let Some(reason) = reason {
                p { role: "status", "data-flight-sim-read-only": "", style: "margin:0;color:{theme.text_muted};", "{reason}" }
            }
            fieldset { disabled: running || reason.is_some(), style: "{fieldset_style}",
                legend { "Simulation" }
                div { style: row_style,
                    label { style: label_style, "Kind"
                        select { value: "{f.kind}", style: "font:inherit;min-height:36px;", "data-field": "kind",
                            onchange: move |e| set_field(|f, v| f.kind = v, e.value()),
                            option { value: "flight", "Flight (trajectory and motor)" }
                            option { value: "burn", "Burn (motor curves only)" }
                        }
                    }
                    label { style: label_style, "Label"
                        input { r#type: "text", value: "{f.label}", style: "font:inherit;min-height:36px;width:22ch;padding:4px 8px;", "data-field": "label",
                            oninput: move |e| set_field(|f, v| f.label = v, e.value()) }
                    }
                    label { style: label_style, "Motor model"
                        select { value: "{f.motor_model}", style: "font:inherit;min-height:36px;", "data-field": "motor_model",
                            onchange: move |e| set_field(|f, v| f.motor_model = v, e.value()),
                            for m in motor_models { option { value: "{m}", "{m}" } }
                        }
                    }
                }
                div { style: row_style,
                    label { style: label_style, "Size the fill by"
                        select { value: "{f.fill_mode}", style: "font:inherit;min-height:36px;", "data-field": "fill_mode",
                            onchange: move |e| set_field(|f, v| f.fill_mode = v, e.value()),
                            option { value: "fill", "Fill mass" }
                            option { value: "target", "Target apogee (the planner picks the fill)" }
                        }
                    }
                    if f.fill_mode == "target" {
                        label { style: label_style, "Target apogee (m above pad)"
                            input { r#type: "text", inputmode: "decimal", value: "{f.target_apogee_m}", style: input_style, "data-field": "target_apogee_m",
                                oninput: move |e| set_field(|f, v| f.target_apogee_m = v, e.value()) }
                        }
                    } else {
                        label { style: label_style, "Nitrous fill (kg)"
                            input { r#type: "text", inputmode: "decimal", value: "{f.fill_kg}", style: input_style, "data-field": "fill_kg",
                                oninput: move |e| set_field(|f, v| f.fill_kg = v, e.value()) }
                        }
                    }
                    label { style: label_style, "Supercharge (psi)"
                        input { r#type: "text", inputmode: "decimal", value: "{f.supercharge_psi}", style: input_style, "data-field": "supercharge_psi",
                            oninput: move |e| set_field(|f, v| f.supercharge_psi = v, e.value()) }
                    }
                    label { style: label_style, "Tank temperature (°C, empty = ambient)"
                        input { r#type: "text", inputmode: "decimal", value: "{f.tank_temp_c}", style: input_style, "data-field": "tank_temp_c",
                            oninput: move |e| set_field(|f, v| f.tank_temp_c = v, e.value()) }
                    }
                }
                if flight {
                    div { style: row_style,
                        label { style: label_style, "Weather"
                            select { value: "{f.weather_source}", style: "font:inherit;min-height:36px;", "data-field": "weather_source",
                                onchange: move |e| set_field(|f, v| f.weather_source = v, e.value()),
                                option { value: "planner", "The apogee planner's current source" }
                                option { value: "manual", "Manual entry" }
                                option { value: "cached", "Last cached forecast" }
                            }
                        }
                    }
                    if f.weather_source == "manual" {
                        {numbers(vec![
                            ("surface_temp_c", "Surface temperature (°C)", f.surface_temp_c.clone(), |f, v| f.surface_temp_c = v),
                            ("surface_pressure_hpa", "Surface pressure (hPa, empty = standard)", f.surface_pressure_hpa.clone(), |f, v| f.surface_pressure_hpa = v),
                            ("wind_mps", "Wind (m/s)", f.wind_mps.clone(), |f, v| f.wind_mps = v),
                            ("wind_from_deg", "Wind from (°)", f.wind_from_deg.clone(), |f, v| f.wind_from_deg = v),
                            ("gust_mps", "Gust (m/s)", f.gust_mps.clone(), |f, v| f.gust_mps = v),
                            ("wind_1km_mps", "Wind at 1 km (m/s)", f.wind_1km_mps.clone(), |f, v| f.wind_1km_mps = v),
                            ("wind_1km_from_deg", "From at 1 km (°)", f.wind_1km_from_deg.clone(), |f, v| f.wind_1km_from_deg = v),
                            ("wind_3km_mps", "Wind at 3 km (m/s)", f.wind_3km_mps.clone(), |f, v| f.wind_3km_mps = v),
                            ("wind_3km_from_deg", "From at 3 km (°)", f.wind_3km_from_deg.clone(), |f, v| f.wind_3km_from_deg = v),
                        ])}
                    }
                    details {
                        summary { "Launch rail, site and Monte Carlo" }
                        div { style: "display:grid;gap:8px;margin-top:8px;",
                            {numbers(vec![
                                ("rail_length_m", "Rail length (m)", f.rail_length_m.clone(), |f, v| f.rail_length_m = v),
                                ("tilt_deg", "Rail tilt (°)", f.tilt_deg.clone(), |f, v| f.tilt_deg = v),
                                ("azimuth_deg", "Rail azimuth (°)", f.azimuth_deg.clone(), |f, v| f.azimuth_deg = v),
                            ])}
                            label { style: "display:flex;gap:8px;align-items:center;",
                                input { r#type: "checkbox", checked: f.site_override, "data-field": "site_override",
                                    onchange: move |e| { let on = e.checked(); let mut next = form.peek().clone().unwrap_or_default(); next.site_override = on; form.set(Some(next)); } }
                                "Override the launch site"
                            }
                            if f.site_override {
                                {numbers(vec![
                                    ("site_lat", "Latitude (°)", f.site_lat.clone(), |f, v| f.site_lat = v),
                                    ("site_lon", "Longitude (°)", f.site_lon.clone(), |f, v| f.site_lon = v),
                                    ("site_alt", "Altitude MSL (m)", f.site_alt.clone(), |f, v| f.site_alt = v),
                                ])}
                            }
                            {numbers(vec![
                                ("samples", "Monte Carlo samples (0 = none)", f.samples.clone(), |f, v| f.samples = v),
                                ("seed", "Seed", f.seed.clone(), |f, v| f.seed = v),
                            ])}
                            if l.limits.max_samples > 0 {
                                p { style: "margin:0;color:{theme.text_muted};font-size:0.85rem;", "At most {l.limits.max_samples} samples; the station keeps the latest {l.limits.max_plans} plans." }
                            }
                        }
                    }
                }
                div { style: "display:flex;flex-wrap:wrap;gap:8px;align-items:center;",
                    button { r#type: "button", class: "gs-btn gs-btn--primary", "data-flight-sim-run": "", disabled: running || reason.is_some(),
                        title: reason.unwrap_or(""), onclick: run,
                        if busy() == Some("Running the simulation") { "Running…" } else { "Run simulation" }
                    }
                    button { r#type: "button", class: "gs-btn", disabled: running,
                        onclick: move |_| form.set(list.peek().as_ref().and_then(|l| l.as_ref().ok()).map(|l| Form::from_request(&l.defaults))),
                        "Reset to station defaults" }
                }
            }
            if let Some(what) = busy() {
                p { role: "status", "aria-live": "polite", "data-flight-sim-busy": "", class: "gs-notice",
                    if what == "Running the simulation" { "{what}… {elapsed_s} s (a Monte Carlo run can take several seconds)" } else { "{what}…" }
                }
            }
            if let Some((ok, message)) = result() {
                p { role: "status", class: if ok { "gs-notice" } else { "gs-notice gs-notice--warn" }, "data-flight-sim-result": if ok { "ok" } else { "error" }, "{message}" }
            }
            if let Some(plan) = selected_plan {
                PlanResult { theme: theme.clone(), plan: plan.clone(),
                    on_map: shown.as_ref().is_some_and(|s| s.id == plan.id),
                    on_show: move |p: PlanDoc| map(p),
                    on_hide: move |_| hide_from_map(),
                    on_close: move |_| selected.set(None) }
            }
            h4 { style: "margin:4px 0 0;", "Stored plans" }
            if l.plans.is_empty() {
                p { style: "margin:0;color:{theme.text_muted};", "No plans yet." }
            }
            ul { class: "gs26-flight-plan-list", style: "list-style:none;margin:0;padding:0;display:grid;gap:6px;",
                for p in l.plans.clone() {
                    PlanRow { key: "{p.id}", theme: theme.clone(), plan: p.clone(), busy: running, reason: reason.map(str::to_string),
                        open: selected().as_ref().is_some_and(|s| s.id == p.id),
                        on_map: shown.as_ref().is_some_and(|s| s.id == p.id),
                        on_view: move |id: String| view(id),
                        on_delete: move |id: String| delete(id),
                        on_export: move |(id, csv): (String, bool)| export(id, csv),
                        on_play: move |id: String| play(id),
                        on_map_toggle: move |id: String| {
                            if SHOWN_PLAN.peek().as_ref().is_some_and(|s| s.id == id) {
                                hide_from_map();
                            } else if let Some(plan) = selected.peek().clone().filter(|p| p.id == id) {
                                map(plan);
                            } else {
                                let work = factory.get::<PlanDoc>(&flight_plans::plan_path(&id));
                                spawn(async move {
                                    match work.await {
                                        Ok(plan) => map(plan),
                                        Err(e) => result.set(Some((false, format!("Cannot read the plan: {}", super::igt::refusal(&e))))),
                                    }
                                });
                            }
                        } }
                }
            }
        }
    }
}

#[component]
fn PlanRow(
    theme: ThemeConfig,
    plan: PlanSummary,
    busy: bool,
    reason: Option<String>,
    open: bool,
    on_map: bool,
    on_view: EventHandler<String>,
    on_delete: EventHandler<String>,
    on_export: EventHandler<(String, bool)>,
    on_play: EventHandler<String>,
    on_map_toggle: EventHandler<String>,
) -> Element {
    let mut confirm = use_signal(|| false);
    let id = plan.id.clone();
    let flight = plan.kind != "burn";
    let who = plan.created_by.clone().filter(|w| !w.is_empty()).map(|w| format!(" by {w}")).unwrap_or_default();
    rsx! {
        li { "data-flight-plan": "{plan.id}", style: "border:1px solid {theme.border_soft};border-radius:10px;padding:8px 10px;display:grid;gap:6px;min-width:0;",
            div { style: "display:flex;flex-wrap:wrap;gap:4px 10px;align-items:baseline;min-width:0;",
                strong { style: "overflow-wrap:anywhere;", "{flight_plans::predicted_label(&plan.label)}" }
                small { style: "color:{theme.text_muted};", "{flight_plans::created_text(plan.created_ms)}{who}" }
            }
            span { style: "color:{theme.text_secondary};font-size:0.9rem;overflow-wrap:anywhere;", "{flight_plans::summary_line(&plan)}" }
            div { style: "display:flex;flex-wrap:wrap;gap:6px;",
                button { r#type: "button", class: "gs-btn", disabled: busy, "aria-pressed": open.to_string(), onclick: { let id = id.clone(); move |_| on_view.call(id.clone()) }, "View" }
                button { r#type: "button", class: "gs-btn", onclick: { let id = id.clone(); move |_| on_export.call((id.clone(), true)) }, "Export CSV" }
                button { r#type: "button", class: "gs-btn", onclick: { let id = id.clone(); move |_| on_export.call((id.clone(), false)) }, "Export JSON" }
                if flight {
                    button { r#type: "button", class: "gs-btn", disabled: busy, "data-flight-plan-play": "", onclick: { let id = id.clone(); move |_| on_play.call(id.clone()) }, "Play in replay" }
                    button { r#type: "button", class: "gs-btn", "aria-pressed": on_map.to_string(), "data-flight-plan-map": "", onclick: { let id = id.clone(); move |_| on_map_toggle.call(id.clone()) },
                        if on_map { "Hide from map" } else { "Show on map" } }
                }
                if confirm() {
                    button { r#type: "button", class: "gs-btn gs-btn--danger", disabled: busy || reason.is_some(), onclick: { let id = id.clone(); move |_| { confirm.set(false); on_delete.call(id.clone()); } }, "Confirm delete" }
                    button { r#type: "button", class: "gs-btn", onclick: move |_| confirm.set(false), "Keep" }
                } else {
                    button { r#type: "button", class: "gs-btn", disabled: busy || reason.is_some(), title: reason.clone().unwrap_or_default(), onclick: move |_| confirm.set(true), "Delete" }
                }
            }
        }
    }
}

/// A plotted series: label, colour, dashed, `(t_s, value)` points.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Series {
    pub label: String,
    pub color: String,
    pub dashed: bool,
    pub points: Vec<[f64; 2]>,
}

/// SVG polyline points of series on one shared scale, plus the axis ranges.
pub(crate) fn scaled_paths(series: &[Series], w: f64, h: f64) -> (Vec<String>, (f64, f64), (f64, f64)) {
    let all = series.iter().flat_map(|s| s.points.iter());
    let (mut t0, mut t1, mut v0, mut v1) = (f64::INFINITY, f64::NEG_INFINITY, 0.0_f64, f64::NEG_INFINITY);
    for p in all {
        t0 = t0.min(p[0]);
        t1 = t1.max(p[0]);
        v0 = v0.min(p[1]);
        v1 = v1.max(p[1]);
    }
    if !t0.is_finite() {
        return (vec![String::new(); series.len()], (0.0, 1.0), (0.0, 1.0));
    }
    let (ts, vs) = ((t1 - t0).max(1e-6), (v1 - v0).max(1e-6));
    let paths = series
        .iter()
        .map(|s| s.points.iter().map(|p| format!("{:.1},{:.1}", (p[0] - t0) / ts * w, h - (p[1] - v0) / vs * h)).collect::<Vec<_>>().join(" "))
        .collect();
    (paths, (t0, t1), (v0, v1))
}

#[component]
pub(crate) fn CurvePlot(title: String, unit: String, series: Vec<Series>, theme: ThemeConfig) -> Element {
    let (w, h) = (600.0, 200.0);
    let (paths, (t0, t1), (v0, v1)) = scaled_paths(&series, w, h);
    let lines: Vec<(Series, String)> = series.into_iter().zip(paths).collect();
    rsx! {
        figure { style: "margin:0;display:grid;gap:4px;min-width:0;", "data-curve-plot": "{title}",
            figcaption { style: "font-weight:700;display:flex;flex-wrap:wrap;gap:4px 12px;",
                "{title}"
                for (s, _) in lines.iter().filter(|(s, _)| !s.label.is_empty()) {
                    span { style: "font-weight:400;font-size:0.85rem;color:{s.color};", if s.dashed { "╌ {s.label}" } else { "— {s.label}" } }
                }
            }
            svg { view_box: "-56 -10 {w + 66.0} {h + 34.0}", width: "100%", role: "img", style: "max-width:720px;background:{theme.panel_background};",
                "aria-label": "{title}: {v0:.0} to {v1:.0} {unit} over {t0:.0}–{t1:.0} s",
                line { x1: "0", y1: "{h}", x2: "{w}", y2: "{h}", stroke: "{theme.text_muted}", stroke_width: "1" }
                line { x1: "0", y1: "0", x2: "0", y2: "{h}", stroke: "{theme.text_muted}", stroke_width: "1" }
                for (s, points) in lines {
                    polyline { points: "{points}", fill: "none", stroke: "{s.color}", stroke_width: "2", stroke_dasharray: if s.dashed { "8 5" } else { "none" } }
                }
                text { x: "0", y: "{h + 18.0}", fill: "{theme.text_muted}", font_size: "12", "{t0:.0} s" }
                text { x: "{w}", y: "{h + 18.0}", fill: "{theme.text_muted}", font_size: "12", text_anchor: "end", "{t1:.0} s" }
                text { x: "-6", y: "10", fill: "{theme.text_muted}", font_size: "12", text_anchor: "end", "{v1:.0} {unit}" }
                text { x: "-6", y: "{h}", fill: "{theme.text_muted}", font_size: "12", text_anchor: "end", "{v0:.0}" }
            }
        }
    }
}

fn table(rows: Vec<(String, String)>, theme: &ThemeConfig) -> Element {
    rsx! {
        dl { style: "display:grid;grid-template-columns:minmax(0,max-content) minmax(0,1fr);gap:4px 12px;margin:0;",
            for (k, v) in rows {
                dt { style: "color:{theme.text_muted};", "{k}" }
                dd { style: "margin:0;overflow-wrap:anywhere;", "{v}" }
            }
        }
    }
}

#[component]
fn PlanResult(theme: ThemeConfig, plan: PlanDoc, on_map: bool, on_show: EventHandler<PlanDoc>, on_hide: EventHandler<()>, on_close: EventHandler<()>) -> Element {
    let altitude = plan.altitude_curve();
    let thrust = plan.thrust();
    let burn = plan.burn.clone();
    let events = flight_plans::event_rows(&plan);
    let dispersion = flight_plans::dispersion_rows(&plan);
    let has_overlay = plan.map_overlay().is_some();
    let label = flight_plans::predicted_label(&plan.label);
    let curve = |title: &str, unit: &str, items: Vec<(&str, &str, Vec<[f64; 2]>)>| -> Element {
        let series: Vec<Series> = items.into_iter().filter(|(_, _, p)| !p.is_empty()).map(|(l, c, p)| Series { label: l.into(), color: c.into(), dashed: false, points: p }).collect();
        if series.is_empty() {
            return rsx! {};
        }
        rsx! { CurvePlot { title: title.to_string(), unit: unit.to_string(), series, theme: theme.clone() } }
    };
    rsx! {
        article { "data-flight-plan-result": "{plan.id}", style: "display:grid;gap:10px;border:1px solid {theme.border};border-radius:12px;padding:12px;min-width:0;background:{theme.panel_background};",
            div { style: "display:flex;flex-wrap:wrap;gap:8px;align-items:center;justify-content:space-between;",
                div { style: "display:flex;flex-wrap:wrap;gap:8px;align-items:center;min-width:0;",
                    span { class: "gs-badge gs-badge--warn", "data-predicted-label": "", "Predicted" }
                    strong { style: "overflow-wrap:anywhere;", "{label}" }
                }
                div { style: "display:flex;gap:6px;flex-wrap:wrap;",
                    if has_overlay {
                        if on_map {
                            button { r#type: "button", class: "gs-btn", onclick: move |_| on_hide.call(()), "Hide from map" }
                        } else {
                            button { r#type: "button", class: "gs-btn", onclick: { let plan = plan.clone(); move |_| on_show.call(plan.clone()) }, "Show on map" }
                        }
                    }
                    button { r#type: "button", class: "gs-btn", onclick: move |_| on_close.call(()), "Close" }
                }
            }
            p { style: "margin:0;color:{theme.text_muted};font-size:0.85rem;",
                "Computed {flight_plans::created_text(plan.created_ms)}"
                if let Some(by) = plan.created_by.as_ref().filter(|b| !b.is_empty()) { " by {by}" }
                if let Some(mode) = plan.station_mode.as_ref() { " · station mode {mode}" }
                ". A prediction, not a measurement."
            }
            if !plan.warnings.is_empty() {
                div { class: "gs-notice gs-notice--warn", role: "status", "data-flight-plan-warnings": "",
                    strong { "Warnings" }
                    ul { style: "margin:4px 0 0;padding-left:18px;", for w in plan.warnings.iter() { li { "{w}" } } }
                }
            }
            div { style: "display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,300px),1fr));gap:12px;",
                section { h4 { style: "margin:0 0 6px;", "Summary" } {table(flight_plans::summary_rows(&plan), &theme)} }
                if !dispersion.is_empty() {
                    section { "data-flight-plan-dispersion": "", h4 { style: "margin:0 0 6px;", "Dispersion" } {table(dispersion, &theme)} }
                }
            }
            if !events.is_empty() {
                section {
                    h4 { style: "margin:0 0 6px;", "Events" }
                    table { style: "border-collapse:collapse;font-variant-numeric:tabular-nums;", "data-flight-plan-events": "",
                        thead { tr { th { style: "text-align:left;padding:2px 12px 2px 0;", "Event" } th { style: "text-align:right;padding:2px 12px;", "Time" } th { style: "text-align:right;padding:2px 0;", "Altitude (pad)" } } }
                        tbody {
                            for (name, t, alt) in events {
                                tr { td { style: "padding:2px 12px 2px 0;", "{name}" } td { style: "text-align:right;padding:2px 12px;", "{t}" } td { style: "text-align:right;padding:2px 0;", "{alt}" } }
                            }
                        }
                    }
                }
            }
            div { style: "display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,320px),1fr));gap:12px;",
                if !altitude.is_empty() {
                    {curve("Predicted altitude above pad", "m", vec![("Predicted", "#38bdf8", altitude)])}
                }
                if !thrust.is_empty() {
                    figure { style: "margin:0;display:grid;gap:4px;", figcaption { style: "font-weight:700;", "Predicted thrust" }
                        super::burn_report_view::ThrustPlot { curve: thrust, theme: theme.clone() }
                    }
                }
                if let Some(b) = burn {
                    {curve("Chamber and tank pressure", "psi", vec![("Chamber", "#f97316", flight_plans::zip(&b.t_s, &b.chamber_psi)), ("Tank", "#38bdf8", flight_plans::zip(&b.t_s, &b.tank_psi))])}
                    {curve("Mass flow", "kg/s", vec![("Oxidiser", "#38bdf8", flight_plans::zip(&b.t_s, &b.mdot_ox_kg_s)), ("Fuel", "#facc15", flight_plans::zip(&b.t_s, &b.mdot_fuel_kg_s))])}
                    {curve("Nitrous remaining", "kg", vec![("N₂O", "#a78bfa", flight_plans::zip(&b.t_s, &b.n2o_kg))])}
                }
            }
        }
    }
}

/// One line under the map while a plan is drawn on it.
#[component]
pub(super) fn PlanMapLegend(theme: ThemeConfig) -> Element {
    let Some(shown) = SHOWN_PLAN.read().clone() else { return rsx! {} };
    rsx! {
        div { class: "gs26-map-plan-legend", "data-map-plan": "{shown.id}",
            style: "display:flex;flex-wrap:wrap;gap:4px 12px;align-items:center;padding:0 10px;font-size:0.78rem;line-height:1.2;min-width:0;color:{theme.text_muted};",
            span { style: "color:#a78bfa;font-weight:800;", "╌ {shown.label}" }
            span { "predicted track and landing area, dashed; the real track and last fix stay solid" }
            button { r#type: "button", class: "gs-btn", style: "min-height:28px;padding:2px 10px;font-size:0.78rem;", onclick: move |_| hide_from_map(), "Hide" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock<T: serde::de::DeserializeOwned>(name: &str) -> T {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flight-predict-mock").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn render(can_send: bool, plan: Option<PlanDoc>) -> String {
        #[derive(Props, Clone, PartialEq)]
        struct P {
            can_send: bool,
            plan: Option<PlanDoc>,
        }
        fn app(p: P) -> Element {
            let list: PlanList = mock("list.json");
            rsx! { FlightSimView { theme: ThemeConfig::default(), can_send: p.can_send, initial: Some((list, p.plan)) } }
        }
        let mut dom = VirtualDom::new_with_props(app, P { can_send, plan });
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn panel_prefills_defaults_lists_plans_and_follows_permission() {
        let html = render(true, None);
        assert!(html.contains("data-flight-sim"));
        assert!(html.contains("value=\"8.5\""), "fill prefilled from defaults");
        assert!(html.contains("Predicted: 8.5 kg nominal"));
        assert!(html.contains("Burn (motor only) · 6.00 kg"));
        assert!(html.contains("Play in replay"));
        assert!(!html.contains("data-flight-sim-read-only"));
        let viewer = render(false, None);
        assert!(viewer.contains("data-flight-sim-read-only"));
        assert!(viewer.contains("command permission"));
    }

    #[test]
    fn result_shows_summary_events_curves_dispersion_and_warnings() {
        let plan: PlanDoc = mock("plan.json");
        let html = render(true, Some(plan));
        assert!(html.contains("data-flight-plan-result"));
        assert!(html.contains("data-predicted-label"));
        assert!(html.contains("1012 m toward 87°"));
        assert!(html.contains("data-flight-plan-events"));
        assert!(html.contains("Rail exit"));
        assert!(html.contains("data-thrust-plot"));
        assert!(html.contains("Chamber and tank pressure"));
        assert!(html.contains("Predicted altitude above pad"));
        assert!(html.contains("2850–3240 m"));
        assert!(html.contains("Rail exit speed 26 m/s"));
    }

    #[test]
    fn shared_scale_paths_and_reasons() {
        let s = vec![
            Series { label: "a".into(), color: "#fff".into(), dashed: false, points: vec![[0.0, 0.0], [10.0, 100.0]] },
            Series { label: "b".into(), color: "#fff".into(), dashed: true, points: vec![[5.0, 50.0]] },
        ];
        let (paths, t, v) = scaled_paths(&s, 100.0, 100.0);
        assert_eq!(paths[0], "0.0,100.0 100.0,0.0");
        assert_eq!(paths[1], "50.0,50.0");
        assert_eq!((t, v), ((0.0, 10.0), (0.0, 100.0)));
        assert!(command_reason(false).is_some());
        assert!(command_reason(true).is_none());
    }
}
