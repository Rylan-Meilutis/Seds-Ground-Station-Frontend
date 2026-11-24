//! Recovery search: the runtime (status, recorder, outbox, map overlay) and
//! the Map tab panel. See `recovery_search` for the model and the privacy
//! rules it enforces.
use super::igt_view::ConfirmDialog;
use super::layout::ThemeConfig;
use super::recovery_search::{self as model, *};
use dioxus::prelude::*;
use serde_json::{Value, json};
use std::sync::Mutex;

pub(crate) static STATUS: GlobalSignal<Option<Status>> = Signal::global(|| None);
/// "Draw my own track on this map only": recorded here, never sent.
pub(crate) static LOCAL_ON: GlobalSignal<bool> = Signal::global(|| false);

/// What the panel shows about this device.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DeviceView {
    pub sharing: bool,
    pub pending: usize,
    pub dropped: u64,
    pub uploaded: u64,
    pub last_error: Option<String>,
    pub local_points: usize,
    pub local_distance_m: f64,
    pub has_fix: bool,
    pub watch_pending: usize,
}

pub(crate) static DEVICE: GlobalSignal<DeviceView> = Signal::global(DeviceView::default);

struct Runtime {
    outbox: Option<Outbox>,
    dirty: bool,
    thinner: Thinner,
    local: LocalTrack,
    last_overlay: String,
    last_overlay_ms: i64,
}

static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime {
    outbox: None,
    dirty: false,
    thinner: Thinner::new(),
    local: LocalTrack { points: std::collections::VecDeque::new() },
    last_overlay: String::new(),
    last_overlay_ms: 0,
});

/// The browser's own fix (web builds), with its accuracy and time.
static WEB_FIX: Mutex<Option<(f64, f64, Option<f64>, i64)>> = Mutex::new(None);

async fn sleep_ms(ms: u64) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

fn now_ms() -> i64 {
    super::current_wallclock_ms()
}

/// Station and account the buffer belongs to (None: anonymous).
fn owner() -> Option<(String, String)> {
    let session = crate::auth::current_session()?;
    if session.session.anonymous {
        return None;
    }
    let user = session.session.username.clone().filter(|u| !u.is_empty())?;
    Some((format!("{}|{}", super::UrlConfig::base_http(), user), user))
}

fn device_id() -> String {
    if let Some(id) = super::persist::get_string(DEVICE_ID_KEY).filter(|id| !id.is_empty()) {
        return id;
    }
    let mut random = [0u8; 16];
    let _ = getrandom::fill(&mut random);
    let id = new_device_id(u128::from_le_bytes(random));
    super::persist::set_string(DEVICE_ID_KEY, &id);
    id
}

fn platform_name() -> &'static str {
    match super::proximity_lock::platform() {
        super::proximity_lock::Platform::Ios => "ios",
        super::proximity_lock::Platform::Android => "android",
        super::proximity_lock::Platform::Web => "web",
        super::proximity_lock::Platform::Desktop => "desktop",
        super::proximity_lock::Platform::Watch => "watch",
    }
}

fn device_name(user: &str) -> String {
    super::proximity_lock::default_device_name(user, super::proximity_lock::platform())
}

/// The outbox of the current station and account, loaded from storage.
fn with_outbox<R>(f: impl FnOnce(&mut Outbox, &mut Runtime) -> R) -> Option<R> {
    let (key, _) = owner()?;
    let mut rt = RUNTIME.lock().ok()?;
    if rt.outbox.as_ref().is_none_or(|o| o.owner != key) {
        let stored = super::persist::get_string(OUTBOX_KEY).and_then(|raw| Outbox::parse(&raw)).filter(|o| o.owner == key);
        rt.outbox = Some(stored.unwrap_or_else(|| Outbox::new(&key, &device_id())));
        rt.dirty = true;
    }
    let mut outbox = rt.outbox.take()?;
    let result = f(&mut outbox, &mut rt);
    rt.outbox = Some(outbox);
    Some(result)
}

fn save_outbox(force: bool) {
    let raw = {
        let Ok(mut rt) = RUNTIME.lock() else { return };
        if !(rt.dirty || force) {
            return;
        }
        rt.dirty = false;
        rt.outbox.as_ref().and_then(|o| serde_json::to_string(o).ok())
    };
    if let Some(raw) = raw {
        super::persist::set_string(OUTBOX_KEY, &raw);
    }
}

fn publish_view(has_fix: bool) {
    let view = with_outbox(|o, rt| DeviceView {
        sharing: o.armed(),
        pending: o.pending.len(),
        dropped: o.dropped,
        uploaded: o.uploaded,
        last_error: o.last_error.clone(),
        local_points: rt.local.points.len(),
        local_distance_m: rt.local.distance_m(),
        has_fix,
        watch_pending: super::watch_companion::search_relay::pending(),
    })
    .unwrap_or_default();
    if *DEVICE.peek() != view {
        *DEVICE.write() = view;
    }
}

/// The overlay call, re-sent on change and every 30 s (the map may load later).
fn publish_overlays() {
    let status = STATUS.peek().clone();
    let Ok(mut rt) = RUNTIME.lock() else { return };
    let local = if *LOCAL_ON.peek() { local_overlay(&rt.local) } else { None };
    let js = overlays_js(&overlays(status.as_ref(), local));
    let now = now_ms();
    if js != rt.last_overlay || now - rt.last_overlay_ms >= 30_000 {
        rt.last_overlay = js.clone();
        rt.last_overlay_ms = now;
        drop(rt);
        super::js_eval(&js);
    }
}

fn status_path() -> String {
    let id = with_outbox(|o, _| o.device_id.clone()).unwrap_or_default();
    if id.is_empty() { STATUS_PATH.to_owned() } else { format!("{STATUS_PATH}?device_id={id}") }
}

pub(crate) fn apply_status(status: Status) {
    let resend_opt_out = with_outbox(|o, rt| {
        let before = o.armed();
        o.reconcile(&status);
        if before != o.armed() {
            rt.dirty = true;
            rt.thinner.reset();
        }
        o.opt_out_due(&status)
    })
    .unwrap_or(false);
    if resend_opt_out {
        spawn(async move {
            let _ = send_opt_in(false, false).await;
        });
    }
    *STATUS.write() = Some(status);
}

async fn refresh() {
    if let Ok(status) = super::http_get_json::<Status>(&status_path()).await {
        apply_status(status);
    }
}

async fn send_opt_in(sharing: bool, delete_my_track: bool) -> Result<OptInReply, String> {
    let (_, user) = owner().ok_or("Sign in to share a search track.")?;
    let request = OptInRequest {
        device_id: with_outbox(|o, _| o.device_id.clone()).unwrap_or_else(device_id),
        device_name: device_name(&user),
        platform: platform_name().into(),
        sharing,
        delete_my_track,
    };
    super::http_post_json::<_, OptInReply>(OPT_IN_PATH, &request).await
}

/// The user's explicit opt-in: arms only after the station accepted it.
async fn start_sharing() -> Result<(), String> {
    let reply = send_opt_in(true, false).await?;
    match (reply.sharing, reply.session_id.as_deref(), reply.opt_in_id.as_deref()) {
        (true, Some(session), Some(id)) => {
            with_outbox(|o, rt| {
                o.arm(session, id, reply.max_seq);
                rt.dirty = true;
                rt.thinner.reset();
            });
            save_outbox(true);
            Ok(())
        }
        _ => Err("The station did not accept the opt-in.".into()),
    }
}

/// Stops at once on this device (nothing more is recorded or sent), then
/// tells the station; an offline opt-out is resent when the station is back.
async fn stop_sharing(delete_my_track: bool) -> Result<(), String> {
    with_outbox(|o, rt| {
        o.opted_out_session = o.session_id.clone();
        o.disarm();
        rt.dirty = true;
    });
    save_outbox(true);
    let reply = send_opt_in(false, delete_my_track).await;
    if reply.is_ok() {
        with_outbox(|o, rt| {
            o.opted_out_session = None;
            rt.dirty = true;
        });
        save_outbox(true);
    }
    refresh().await;
    reply.map(|_| ())
}

/// This device's current fix: the dashboard's location (not a manually
/// entered one), or the browser's own on the web.
fn current_fix(user_gps: Option<(f64, f64)>, manual: bool) -> Option<(f64, f64, Option<f64>, i64)> {
    if let Some(fix) = WEB_FIX.lock().ok().and_then(|f| *f)
        && now_ms() - fix.3 < 60_000
    {
        return Some(fix);
    }
    if manual {
        return None;
    }
    user_gps.map(|(lat, lon)| (lat, lon, None, now_ms()))
}

async fn upload_once() {
    let Some(batch) = with_outbox(|o, _| o.batch(now_ms())).flatten() else { return };
    let reply = super::http_post_json::<_, UploadReply>(TRACK_PATH, &batch).await;
    with_outbox(|o, rt| {
        match &reply {
            Ok(_) => o.sent(&batch),
            Err(error) => {
                o.failed(&batch, error, now_ms());
            }
        }
        rt.dirty = true;
    });
    save_outbox(false);
}

/// Mounted once in the dashboard: polls the station's search status, records
/// this device's track while it shares (or draws it locally), uploads the
/// buffer, relays a watch's track, and publishes the map overlay.
#[component]
pub(super) fn RecoverySearchRuntime(user_gps: Signal<Option<(f64, f64)>>, user_location_manual: Signal<bool>) -> Element {
    use_future(move || async move {
        let mut last_poll = 0i64;
        let mut last_save = 0i64;
        loop {
            let now = now_ms();
            let busy = STATUS.peek().as_ref().is_some_and(|s| s.active || s.accepting_uploads)
                || *LOCAL_ON.peek()
                || with_outbox(|o, _| !o.pending.is_empty()).unwrap_or(false);
            if now - last_poll >= if busy { 5_000 } else { 20_000 } {
                last_poll = now;
                refresh().await;
            }
            let fix = current_fix(*user_gps.peek(), *user_location_manual.peek());
            let mode = with_outbox(|o, _| model::mode(STATUS.peek().as_ref(), o, *LOCAL_ON.peek())).unwrap_or(if *LOCAL_ON.peek() {
                Mode::LocalOnly
            } else {
                Mode::Off
            });
            if mode != Mode::Off
                && let Some((lat, lon, acc, t)) = fix
            {
                with_outbox(|o, rt| {
                    if rt.thinner.keep(lat, lon, t, acc) {
                        if mode == Mode::Sharing && o.push(t, lat, lon, acc).is_some() {
                            rt.dirty = true;
                        }
                        if *LOCAL_ON.peek() {
                            rt.local.push(lat, lon, t);
                        }
                    }
                });
            }
            super::watch_companion::search_relay::relay_once().await;
            upload_once().await;
            if now - last_save >= 5_000 {
                last_save = now;
                save_outbox(false);
            }
            publish_view(fix.is_some());
            publish_overlays();
            sleep_ms(1_000).await;
        }
    });
    let recording = DEVICE.read().sharing || *LOCAL_ON.read();
    rsx! {
        if recording {
            WebLocation {}
        }
    }
}

/// The browser's location while this device records (web builds only: the
/// prompt follows the user's own choice to share or draw a track).
#[component]
fn WebLocation() -> Element {
    #[cfg(target_arch = "wasm32")]
    {
        use_future(move || async move {
            let mut script = document::eval(
                r#"
                if ('geolocation' in navigator) {
                    const id = navigator.geolocation.watchPosition(
                        p => dioxus.send({lat:p.coords.latitude, lon:p.coords.longitude, acc:p.coords.accuracy, t:p.timestamp}),
                        () => {}, {enableHighAccuracy:true, maximumAge:5000, timeout:30000});
                    window.__gs26SearchStop = () => navigator.geolocation.clearWatch(id);
                }
                "#,
            );
            while let Ok(m) = script.recv::<Value>().await {
                if let (Some(lat), Some(lon)) = (m["lat"].as_f64(), m["lon"].as_f64())
                    && let Ok(mut slot) = WEB_FIX.lock()
                {
                    *slot = Some((lat, lon, m["acc"].as_f64(), m["t"].as_f64().map_or_else(now_ms, |t| t as i64)));
                }
            }
        });
        use_drop(|| {
            document::eval("window.__gs26SearchStop?.(); delete window.__gs26SearchStop;");
            if let Ok(mut slot) = WEB_FIX.lock() {
                *slot = None;
            }
        });
    }
    rsx! {}
}

// ---------------------------------------------------------------------------
// Panel

fn pct_text(status: &Status) -> String {
    match status.stats.percent_covered {
        Some(p) => format!("{p:.1}%"),
        None => "—".into(),
    }
}

/// The predicted landing area: the loss-of-signal prediction's ellipse, else
/// the newest flight plan's dispersion ellipse.
async fn predicted_area() -> Result<(Vec<[f64; 2]>, &'static str, String), String> {
    if let Ok(los) = super::http_get_json::<Value>(LOS_PATH).await
        && let Some(polygon) = los_landing_polygon(&los)
    {
        return Ok((polygon, "los_prediction", "Predicted landing area (no signal)".into()));
    }
    let plans = super::http_get_json::<Value>(FLIGHT_PLANS_PATH).await.map_err(|_| "No prediction is available.".to_string())?;
    let id = plans["plans"].as_array().and_then(|p| p.first()).and_then(|p| p["id"].as_str()).ok_or("No flight plan has a landing area.")?;
    let plan = super::http_get_json::<Value>(&format!("{FLIGHT_PLANS_PATH}/{id}")).await?;
    let polygon = plan_landing_polygon(&plan).ok_or("The newest flight plan has no landing area.")?;
    let label = plan["label"].as_str().map_or_else(|| "Flight plan landing area".to_string(), |l| format!("Flight plan: {l}"));
    Ok((polygon, "flight_plan", label))
}

async fn operator_put(body: Value) -> Result<Status, String> {
    let owner = super::recording_download::ExportOwner::capture_observer_selected(|| true);
    super::station_settings_request::put_captured::<_, Status>(SETTINGS_PATH, &body, owner).await
}

#[component]
pub(super) fn RecoverySearchPanel(theme: ThemeConfig) -> Element {
    let status = STATUS.read().clone();
    let device = DEVICE.read().clone();
    let local_on = *LOCAL_ON.read();
    let mut open = use_signal(|| false);
    let mut confirm_share = use_signal(|| false);
    let mut confirm_clear = use_signal(|| false);
    let mut delete_mine = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut note = use_signal(|| None::<(bool, String)>);
    let mut swath = use_signal(String::new);
    let session = crate::auth::current_status();
    let can_operate = session.authenticated && !session.anonymous && (session.permissions.send_commands || crate::auth::is_admin());
    let admin = crate::auth::is_admin();
    let signed_in = owner().is_some();
    let active = status.as_ref().is_some_and(|s| s.active);
    let has_session = status.as_ref().is_some_and(|s| s.session_id.is_some());
    // Nothing to show until a search exists, unless the user wants to start
    // one or draw their own track.
    let expanded = open() || active || device.sharing;
    let mut run = move |work: std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>>>>| {
        busy.set(true);
        spawn(async move {
            let result = work.await;
            busy.set(false);
            note.set(Some(match result {
                Ok(text) => (true, text),
                Err(e) => (false, super::igt::refusal(&e)),
            }));
        });
    };
    let border = if device.sharing { "#ef4444" } else { theme.border.as_str() };
    rsx! {
        section { "data-recovery-search": if active { "active" } else { "off" },
            style: "margin:0 8px;padding:6px 10px;border-radius:10px;border:1px solid {border};background:{theme.panel_background};color:{theme.text_primary};font-size:0.8rem;display:grid;gap:6px;min-width:0;",
            div { style: "display:flex;flex-wrap:wrap;align-items:center;gap:6px 12px;min-width:0;",
                button { r#type: "button", style: "background:none;border:none;color:inherit;font:inherit;font-weight:800;cursor:pointer;padding:0;",
                    "aria-expanded": if expanded { "true" } else { "false" },
                    onclick: move |_| open.set(!open()),
                    "Recovery search"
                }
                if active {
                    span { "data-search-state": "on", style: "padding:1px 8px;border-radius:999px;background:#14532d;color:#bbf7d0;font-weight:800;", "ON" }
                } else if has_session {
                    span { "data-search-state": "stopped", style: "color:{theme.text_muted};", "Stopped" }
                } else {
                    span { "data-search-state": "off", style: "color:{theme.text_muted};", "Off" }
                }
                if let Some(s) = status.as_ref().filter(|s| s.session_id.is_some()) {
                    span { "data-search-percent": "{pct_text(s)}", style: "font-weight:800;", "Covered {pct_text(s)}" }
                    span { style: "color:{theme.text_muted};", "Swath {s.settings.swath_width_m:.0} m · {s.stats.sharing} sharing · {s.stats.searchers} searchers" }
                }
                if device.sharing {
                    span { "data-search-sharing": "true", role: "status",
                        style: "padding:2px 8px;border-radius:999px;background:#7f1d1d;color:#fecaca;font-weight:800;",
                        "● Sharing your location track"
                    }
                }
            }
            if expanded {
                // This device: sharing (opt-in), or a local-only track.
                div { style: "display:flex;flex-wrap:wrap;gap:6px 10px;align-items:center;",
                    if device.sharing {
                        span { "Your position goes to the station about every 5 m while the search is on. " }
                        if device.pending > 0 {
                            span { "data-search-pending": "{device.pending}", style: "color:#fbbf24;font-weight:700;", "{device.pending} fixes waiting for signal" }
                        }
                        label { style: "display:inline-flex;gap:4px;align-items:center;",
                            input { r#type: "checkbox", checked: delete_mine(), onchange: move |e| delete_mine.set(e.checked()) }
                            "also delete my uploaded track"
                        }
                        button { r#type: "button", "data-search-stop-sharing": "", disabled: busy(),
                            style: "padding:4px 10px;border-radius:999px;border:1px solid #ef4444;background:#7f1d1d;color:#fff;font-weight:800;cursor:pointer;",
                            onclick: move |_| {
                                let delete = delete_mine();
                                run(Box::pin(async move { stop_sharing(delete).await.map(|_| "You stopped sharing. Nothing more is sent.".to_string()) }));
                            },
                            "Stop sharing"
                        }
                    } else if active && signed_in {
                        button { r#type: "button", "data-search-share": "", disabled: busy(),
                            style: "padding:4px 10px;border-radius:999px;border:1px solid {theme.button_border};background:{theme.button_background};color:{theme.button_text};font-weight:700;cursor:pointer;",
                            onclick: move |_| confirm_share.set(true),
                            "Share my track for this search…"
                        }
                    } else if active {
                        span { style: "color:{theme.text_muted};", "Sign in to share your track." }
                    }
                    label { style: "display:inline-flex;gap:4px;align-items:center;",
                        input { r#type: "checkbox", "data-search-local": "", checked: local_on,
                            onchange: move |e| { *LOCAL_ON.write() = e.checked(); }
                        }
                        "Draw my track on this map only (never sent)"
                    }
                    if local_on {
                        span { style: "color:{theme.text_muted};", "{device.local_points} fixes · {device.local_distance_m:.0} m" }
                        button { r#type: "button", style: "padding:2px 8px;border-radius:999px;border:1px solid {theme.button_border};background:none;color:inherit;cursor:pointer;",
                            onclick: move |_| { if let Ok(mut rt) = RUNTIME.lock() { rt.local = LocalTrack::default(); rt.thinner.reset(); } },
                            "Clear"
                        }
                    }
                    if (device.sharing || local_on) && !device.has_fix {
                        span { style: "color:#fbbf24;", "Waiting for this device's location…" }
                    }
                    if device.watch_pending > 0 {
                        span { style: "color:{theme.text_muted};", "Watch: {device.watch_pending} fixes to relay" }
                    }
                }
                if let Some(s) = status.as_ref().filter(|s| s.session_id.is_some()) {
                    div { style: "display:flex;flex-wrap:wrap;gap:4px 14px;color:{theme.text_muted};",
                        match s.target.as_ref() {
                            Some(t) => rsx! { span { "Area: {t.label.clone().unwrap_or_else(|| \"predicted landing area\".into())} ({source_label(&t.source)})" } },
                            None => rsx! { span { "No predicted landing area set: only the searched area is shown." } },
                        }
                        if let Some(t) = s.stats.target_m2 {
                            span { "{area_text(s.stats.covered_in_target_m2.unwrap_or(0.0))} of {area_text(t)} searched" }
                        }
                        span { "Swept {area_text(s.stats.swath_m2)} · walked {s.stats.distance_m / 1000.0:.2} km" }
                        if !s.active && s.accepting_uploads {
                            span { "Late uploads from devices that were out of signal are still merged." }
                        }
                    }
                    if !s.searchers.is_empty() {
                        ul { "data-search-searchers": "{s.searchers.len()}", style: "margin:0;padding:0;list-style:none;display:flex;flex-wrap:wrap;gap:4px 12px;",
                            for searcher in s.searchers.iter().cloned() {
                                li { key: "{searcher.id}", style: "display:inline-flex;gap:4px;align-items:center;",
                                    span { style: "width:10px;height:10px;border-radius:50%;background:{searcher.color};display:inline-block;" }
                                    "{searcher.name} · {searcher.distance_m:.0} m"
                                    if searcher.sharing { span { style: "color:#4ade80;", " · sharing" } }
                                }
                            }
                        }
                    }
                }
                if can_operate {
                    div { style: "display:flex;flex-wrap:wrap;gap:6px;align-items:center;",
                        if active {
                            button { r#type: "button", "data-search-stop": "", disabled: busy(),
                                style: "padding:4px 10px;border-radius:999px;border:1px solid {theme.warning_border};background:{theme.warning_background};color:{theme.warning_text};cursor:pointer;",
                                onclick: move |_| run(Box::pin(async move {
                                    let s = super::http_post_json::<_, Status>(STOP_PATH, &json!({})).await?;
                                    apply_status(s);
                                    Ok("Search stopped. Devices stop sharing.".to_string())
                                })),
                                "Stop search"
                            }
                        } else {
                            button { r#type: "button", "data-search-start": "", disabled: busy(),
                                style: "padding:4px 10px;border-radius:999px;border:1px solid {theme.button_border};background:{theme.button_background};color:{theme.button_text};cursor:pointer;",
                                onclick: move |_| run(Box::pin(async move {
                                    let s = super::http_post_json::<_, Status>(START_PATH, &json!({})).await?;
                                    apply_status(s);
                                    Ok("Search started. Searchers can opt in to share their track.".to_string())
                                })),
                                "Start search"
                            }
                        }
                        input { r#type: "number", min: "2", max: "500", step: "1", placeholder: "swath m", "aria-label": "Swath width (m)",
                            style: "width:6em;", value: "{swath}", oninput: move |e| swath.set(e.value()) }
                        button { r#type: "button", disabled: busy() || swath().trim().is_empty(),
                            style: "padding:4px 10px;border-radius:999px;border:1px solid {theme.button_border};background:none;color:inherit;cursor:pointer;",
                            onclick: move |_| {
                                let text = swath();
                                run(Box::pin(async move {
                                    let width = parse_swath(&text)?;
                                    apply_status(operator_put(json!({"swath_width_m": width})).await?);
                                    Ok(format!("Swath width set to {width:.0} m."))
                                }));
                            },
                            "Set swath"
                        }
                        button { r#type: "button", "data-search-use-prediction": "", disabled: busy(),
                            style: "padding:4px 10px;border-radius:999px;border:1px solid {theme.button_border};background:none;color:inherit;cursor:pointer;",
                            onclick: move |_| run(Box::pin(async move {
                                let (polygon, source, label) = predicted_area().await?;
                                apply_status(operator_put(json!({"target": {"polygon": polygon, "source": source, "label": label}})).await?);
                                Ok(format!("Search area set from the {}.", source_label(source)))
                            })),
                            "Use predicted landing area"
                        }
                        if status.as_ref().is_some_and(|s| s.target.is_some()) {
                            button { r#type: "button", disabled: busy(),
                                style: "padding:4px 10px;border-radius:999px;border:1px solid {theme.button_border};background:none;color:inherit;cursor:pointer;",
                                onclick: move |_| run(Box::pin(async move {
                                    apply_status(operator_put(json!({"target": null})).await?);
                                    Ok("Search area removed.".to_string())
                                })),
                                "Remove area"
                            }
                        }
                        if admin && has_session {
                            button { r#type: "button", "data-search-clear": "", disabled: busy(),
                                style: "padding:4px 10px;border-radius:999px;border:1px solid {theme.error_border};background:none;color:{theme.error_text};cursor:pointer;",
                                onclick: move |_| confirm_clear.set(true),
                                "Clear all tracks"
                            }
                        }
                    }
                }
                if let Some((ok, text)) = note() {
                    div { role: "status", style: if ok { "color:#4ade80;" } else { "color:#f87171;" }, "{text}" }
                }
            }
        }
        if confirm_share() {
            ConfirmDialog { id: "gs26-search-share".to_string(), title: "Share your track for this search?".to_string(),
                lines: vec![
                    "While this recovery search is on, this device sends its GPS position to the station about every 5 m (and every 30 s when still).".to_string(),
                    "Everyone signed in to the station sees your track and name on the search map.".to_string(),
                    "If you lose signal, positions wait on this device and are sent when you are back in range.".to_string(),
                    "Sharing stops when you press Stop sharing or the search ends. The station keeps the track only for this search; an administrator can delete it, and you can delete yours.".to_string(),
                ],
                confirm_label: "Share my track".to_string(), busy: busy(),
                on_cancel: move |_| confirm_share.set(false),
                on_confirm: move |_| {
                    confirm_share.set(false);
                    run(Box::pin(async move { start_sharing().await.map(|_| "Sharing your track for this search.".to_string()) }));
                },
            }
        }
        if confirm_clear() {
            ConfirmDialog { id: "gs26-search-clear".to_string(), title: "Clear every search track?".to_string(),
                lines: vec!["Deletes every searcher's uploaded track from the station. Devices that are sharing keep sharing.".to_string()],
                confirm_label: "Clear tracks".to_string(), busy: busy(),
                on_cancel: move |_| confirm_clear.set(false),
                on_confirm: move |_| {
                    confirm_clear.set(false);
                    run(Box::pin(async move {
                        let owner = super::recording_download::ExportOwner::capture_observer_selected(|| true);
                        let s = super::station_settings_request::delete_captured::<Status>(TRACKS_PATH, owner).await?;
                        apply_status(s);
                        Ok("Search tracks cleared.".to_string())
                    }));
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(status: Option<Status>, device: DeviceView) -> String {
        fn app() -> Element {
            rsx! { RecoverySearchPanel { theme: ThemeConfig::default() } }
        }
        let mut dom = VirtualDom::new(app);
        dom.in_scope(ScopeId::ROOT, || {
            *STATUS.write() = status;
            *DEVICE.write() = device;
        });
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    fn active() -> Status {
        serde_json::from_value(json!({
            "active": true, "session_id": "rs-1", "accepting_uploads": true,
            "settings": {"swath_width_m": 30.0},
            "target": {"polygon": [[1, 1], [1, 2], [2, 2]], "source": "los_prediction", "label": "95% landing area"},
            "stats": {"percent_covered": 42.5, "target_m2": 250000.0, "covered_in_target_m2": 106250.0, "swath_m2": 180000.0, "searchers": 2, "sharing": 1, "points": 10, "distance_m": 1200.0},
            "searchers": [{"id": "a", "name": "Ada's iPhone", "color": "#0ea5e9", "sharing": true, "points": 5, "distance_m": 600.0}],
            "overlay": {"kind": "search_coverage"}
        }))
        .unwrap()
    }

    #[test]
    fn the_panel_shows_mode_coverage_and_the_sharing_indicator() {
        let off = render(None, DeviceView::default());
        assert!(off.contains("data-search-state=\"off\"") && !off.contains("data-search-sharing"));
        let on = render(Some(active()), DeviceView::default());
        assert!(on.contains("data-search-state=\"on\""));
        assert!(on.contains("Covered 42.5%"), "{on}");
        assert!(on.contains("Ada&#39;s iPhone") || on.contains("Ada's iPhone"));
        assert!(on.contains("loss-of-signal prediction"));
        assert!(!on.contains("data-search-sharing"), "not sharing until the user opts in");
        let sharing = render(Some(active()), DeviceView { sharing: true, pending: 3, ..DeviceView::default() });
        assert!(sharing.contains("data-search-sharing=\"true\""));
        assert!(sharing.contains("Stop sharing"));
        assert!(sharing.contains("3 fixes waiting for signal"));
        assert!(sharing.contains("Draw my track on this map only (never sent)"));
    }
}
