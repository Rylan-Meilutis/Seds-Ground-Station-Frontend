//! Proximity lock presentation and the device reporter
//! (docs/proximity-lock.md). See `proximity_lock` for the model.
use super::igt_view::ConfirmDialog;
use super::layout::ThemeConfig;
use super::proximity_lock::{self as model, *};
use super::proximity_native::{self as native, Fence};
use dioxus::prelude::*;
use std::sync::Mutex;

/// What this device last decided, for the Settings line and the banner.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OwnView {
    pub participation: Participation,
    pub permission: Permission,
    pub result: Option<Result<bool, Unknown>>,
    pub reference: ReferencePoint,
    pub background: bool,
    pub last_error: Option<String>,
}

impl Default for OwnView {
    fn default() -> Self {
        Self {
            participation: Participation::StationOff,
            permission: Permission::NotDetermined,
            result: None,
            reference: ReferencePoint::None,
            background: false,
            last_error: None,
        }
    }
}

pub(crate) static OWN: GlobalSignal<OwnView> = Signal::global(OwnView::default);

/// The fix and permission from the browser or desktop location (non-mobile).
static FOREGROUND_FIX: Mutex<Option<OwnFix>> = Mutex::new(None);
static FOREGROUND_PERMISSION: Mutex<Permission> = Mutex::new(Permission::NotDetermined);

#[cfg(not(target_os = "android"))]
fn set_foreground(fix: Option<OwnFix>, permission: Permission) {
    if let Ok(mut slot) = FOREGROUND_PERMISSION.lock() {
        *slot = permission;
    }
    if let Some(fix) = fix
        && let Ok(mut slot) = FOREGROUND_FIX.lock()
    {
        *slot = Some(fix);
    }
}

async fn sleep_ms(ms: u64) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

// ---------------------------------------------------------------------------
// Local preferences

fn account_key() -> Option<(String, String)> {
    let session = crate::auth::current_session()?;
    if session.session.anonymous {
        return None;
    }
    let user = session.session.username.clone().filter(|u| !u.is_empty())?;
    Some((format!("{}|{}", super::UrlConfig::base_http(), user), user))
}

fn load_prefs() -> Prefs {
    Prefs::parse(&super::persist::get_string(PREFS_KEY).unwrap_or_default())
}

fn edit_prefs(key: &str, f: impl FnOnce(&mut AccountPrefs)) {
    let mut prefs = load_prefs();
    let mut account = prefs.account(key);
    f(&mut account);
    prefs.accounts.insert(key.to_owned(), account);
    if let Ok(raw) = serde_json::to_string(&prefs) {
        super::persist::set_string(PREFS_KEY, &raw);
    }
    PREFS_REVISION.with_mut(|r| *r += 1);
}

/// Bumped on every local preference change so views re-read them.
static PREFS_REVISION: GlobalSignal<u64> = Signal::global(|| 0);

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

fn device_name(prefs: &AccountPrefs, user: &str) -> String {
    prefs
        .device_name
        .clone()
        .filter(|n| valid_device_name(n))
        .unwrap_or_else(|| default_device_name(user, platform()))
}

fn current_permission() -> Permission {
    if native::NATIVE_MOBILE {
        native::permission()
    } else {
        FOREGROUND_PERMISSION.lock().map(|p| *p).unwrap_or(Permission::NotDetermined)
    }
}

fn current_fix() -> Option<OwnFix> {
    if native::NATIVE_MOBILE {
        native::own_fix()
    } else {
        FOREGROUND_FIX.lock().ok().and_then(|f| *f)
    }
}

// ---------------------------------------------------------------------------
// Runtime: status poll, location source and reporter

/// Mounted once in the dashboard. Reads the station's status, keeps this
/// device's location source running while it participates, and reports.
#[component]
pub(super) fn ProximityRuntime() -> Element {
    // The status also arrives on the websocket; this covers a station that
    // was reached before the socket and keeps it fresh.
    use_future(move || async move {
        loop {
            if let Ok(status) = super::http_get_json::<Status>(STATUS_PATH).await {
                model::apply_status(status);
            }
            sleep_ms(30_000).await;
        }
    });
    use_future(move || async move { reporter().await });
    let own = OWN.read().clone();
    // The browser or desktop prompts as soon as location is first read, so
    // that waits until the user has read the explanation and agreed.
    let foreground_source = !native::NATIVE_MOBILE && own.participation.participating() && explained();
    rsx! {
        if foreground_source {
            ForegroundLocation {}
        }
    }
}

/// The desktop and browser location source, only while participating.
#[component]
fn ForegroundLocation() -> Element {
    #[cfg(target_arch = "wasm32")]
    {
        use_future(move || async move {
            let mut script = document::eval(
                r#"
                if (!('geolocation' in navigator)) { dioxus.send({ok:false, code:0}); }
                else {
                    const id = navigator.geolocation.watchPosition(
                        p => dioxus.send({ok:true, lat:p.coords.latitude, lon:p.coords.longitude, acc:p.coords.accuracy, t:p.timestamp}),
                        e => dioxus.send({ok:false, code:e.code}),
                        {enableHighAccuracy:true, maximumAge:10000, timeout:30000});
                    window.__gs26ProximityStop = () => navigator.geolocation.clearWatch(id);
                }
                "#,
            );
            while let Ok(message) = script.recv::<serde_json::Value>().await {
                if message["ok"].as_bool() == Some(true) {
                    let fix = OwnFix {
                        lat: message["lat"].as_f64().unwrap_or(f64::NAN),
                        lon: message["lon"].as_f64().unwrap_or(f64::NAN),
                        accuracy_m: message["acc"].as_f64(),
                        at_ms: message["t"].as_f64().map_or_else(super::current_wallclock_ms, |t| t as i64),
                    };
                    set_foreground(Some(fix), Permission::Foreground);
                } else {
                    // 1 PERMISSION_DENIED, 2 POSITION_UNAVAILABLE, 3 TIMEOUT, 0 no API.
                    let permission = match message["code"].as_i64() {
                        Some(1) => Permission::Denied,
                        Some(0) => Permission::Unsupported,
                        _ => *FOREGROUND_PERMISSION.lock().unwrap(),
                    };
                    set_foreground(None, permission);
                }
            }
        });
        use_drop(|| {
            document::eval("window.__gs26ProximityStop?.(); delete window.__gs26ProximityStop;");
        });
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    use_future(move || async move {
        loop {
            match super::gps::one_shot_location(std::time::Duration::from_secs(10)).await {
                Ok(fix) => set_foreground(
                    Some(OwnFix {
                        lat: fix.latitude,
                        lon: fix.longitude,
                        accuracy_m: fix.accuracy_m,
                        at_ms: super::current_wallclock_ms(),
                    }),
                    Permission::Foreground,
                ),
                Err(error) => {
                    let lower = error.to_lowercase();
                    let permission = if lower.contains("denied") || lower.contains("restricted") {
                        Permission::Denied
                    } else if lower.contains("cannot share") || lower.contains("unsupported") {
                        Permission::Unsupported
                    } else {
                        *FOREGROUND_PERMISSION.lock().unwrap()
                    };
                    set_foreground(None, permission);
                }
            }
            sleep_ms(15_000).await;
        }
    });
    rsx! {}
}

struct Reporter {
    previous: Option<bool>,
    last_sent: Option<(Report, i64)>,
    seq: u64,
    fence: Option<Fence>,
    retry_after_ms: i64,
}

/// The reporting loop: evaluates every second (and at once on a geofence
/// wake), sends on change and every heartbeat.
async fn reporter() {
    let mut r = Reporter { previous: None, last_sent: None, seq: 0, fence: None, retry_after_ms: 0 };
    loop {
        tick(&mut r).await;
        for _ in 0..10 {
            sleep_ms(100).await;
            if native::take_wake() {
                break;
            }
        }
    }
}

async fn tick(r: &mut Reporter) {
    let now = super::current_wallclock_ms();
    let status = PROXIMITY_STATUS.peek().clone();
    let settings = status.as_ref().map(|s| s.settings).unwrap_or_default();
    let account = account_key();
    let prefs = account.as_ref().map(|(key, _)| load_prefs().account(key)).unwrap_or_default();
    let permission = current_permission();
    let participation = Participation::of(settings.enabled, account.is_some(), prefs.opted_out, permission);
    let rocket = super::latest_rocket_gps_fix_from_store();
    let reference = choose_reference(rocket, status.as_ref().and_then(|s| s.pad), now);
    let fix = participation.participating().then(current_fix).flatten();
    let geofence = Geofence::of(&settings);
    let result = participation
        .participating()
        .then(|| model::evaluate(&geofence, r.previous, fix, reference, now));
    if let Some(Ok(inside)) = result {
        r.previous = Some(inside);
    } else if !participation.participating() {
        r.previous = None;
    }
    // Background operation follows participation and the reference.
    let want_fence = (native::NATIVE_MOBILE
        && participation == Participation::Participating
        && matches!(permission, Permission::Foreground | Permission::Background))
        .then(|| reference.point())
        .flatten()
        .map(|p| Fence { lat: p.lat, lon: p.lon, radius_m: (geofence.radius_m + geofence.exit_margin_m).max(100.0) });
    let fence_changed = match (&r.fence, &want_fence) {
        (Some(a), Some(b)) => !a.close_to(b, 25.0),
        (None, None) => false,
        _ => true,
    };
    if fence_changed {
        native::set_background(want_fence);
        r.fence = want_fence;
    }
    let background = background_capable(platform(), permission);
    let mut error = OWN.peek().last_error.clone();
    if let Some((_, user)) = account.as_ref().filter(|_| participation.reports())
        && now >= r.retry_after_ms
    {
        let next = build_report(
            &device_id(),
            &device_name(&prefs, user),
            platform(),
            participation,
            result,
            fix,
            reference,
            background,
            r.seq + 1,
            now,
        );
        let interval = if next.participating { HEARTBEAT_MS } else { 300_000 };
        if report_due_every(r.last_sent.as_ref().map(|(rep, at)| (rep, *at)), &next, now, interval) {
            r.seq += 1;
            match super::http_post_json::<_, Status>(REPORT_PATH, &next).await {
                Ok(status) => {
                    model::apply_status(status);
                    r.last_sent = Some((next, now));
                    error = None;
                }
                Err(e) => {
                    r.retry_after_ms = now + 5_000;
                    error = Some(super::igt::refusal(&e));
                }
            }
        }
    } else if !participation.reports() {
        r.last_sent = None;
        error = None;
    }
    let view = OwnView { participation, permission, result, reference, background, last_error: error };
    if *OWN.peek() != view {
        *OWN.write() = view;
    }
}

/// [`report_due`] with a heartbeat interval: opted-out devices need none
/// (they are never unknown), so they repeat only every few minutes.
fn report_due_every(last: Option<(&Report, i64)>, next: &Report, now_ms: i64, interval_ms: i64) -> bool {
    match last {
        Some((_, at)) if now_ms.saturating_sub(at) < interval_ms => model::report_due(last.map(|(r, _)| (r, now_ms)), next, now_ms),
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Banners

const CSS: &str = r#"
.gs26-prox{margin:6px 8px;padding:8px 12px;border-radius:10px;border:1px solid;display:flex;flex-wrap:wrap;gap:6px 12px;align-items:center;font-size:14px;line-height:1.35}
.gs26-prox strong{letter-spacing:.02em}
.gs26-prox[data-level=active]{border-color:var(--gs-danger-line,#b91c1c);background:var(--gs-danger-bg,#450a0a);color:var(--gs-danger-text,#fecaca)}
.gs26-prox[data-level=override]{border-color:#a855f7;background:#2e1065;color:#f3e8ff}
.gs26-prox[data-level=warning]{border-color:var(--gs-warn-line,#b45309);background:var(--gs-warn-bg,#451a03);color:var(--gs-warn-text,#fde68a)}
.gs26-prox[data-level=indicator]{border-color:transparent;background:transparent;padding:2px 12px;font-size:12px;opacity:.8}
.gs26-prox-ask{margin:6px 8px;padding:10px 12px;border-radius:10px;border:1px solid var(--gs-info-line,#2563eb);display:grid;gap:8px}
"#;

/// The status banner on every view: red while it refuses, purple while
/// overridden, amber for unknown devices, otherwise a quiet indicator; and
/// this device's permission request while it is pending.
#[component]
pub(super) fn ProximityBanner() -> Element {
    let status = PROXIMITY_STATUS.read().clone();
    let own = OWN.read().clone();
    let banner = model::banner(status.as_ref());
    rsx! {
        style { {CSS} }
        match banner {
            None => rsx! {},
            Some(Banner::Active(text)) => rsx! {
                aside { class: "gs26-prox", "data-level": "active", "data-proximity-banner": "active", role: "alert",
                    strong { "PROXIMITY LOCK" }
                    span { "{text}. Arming and pressurizing commands are refused; safing is not." }
                }
            },
            Some(Banner::Overridden { text, by, reason, expires_ms }) => rsx! {
                aside { class: "gs26-prox", "data-level": "override", "data-proximity-banner": "override", role: "alert",
                    strong { "PROXIMITY LOCK OVERRIDDEN" }
                    span { "by {by} ({remaining_text(expires_ms, super::current_wallclock_ms())} left): {reason}. {text}." }
                }
            },
            Some(Banner::Warning(text)) => rsx! {
                aside { class: "gs26-prox", "data-level": "warning", "data-proximity-banner": "warning", role: "status",
                    strong { "Proximity lock" }
                    span { "{text}." }
                }
            },
            Some(Banner::Indicator(text)) => rsx! {
                p { class: "gs26-prox", "data-level": "indicator", "data-proximity-banner": "indicator", role: "status", "{text}" }
            },
        }
        if show_ask(&own, explained()) {
            PermissionAsk { own }
        }
    }
}

/// Ask (with the explanation) while the first permission answer is pending,
/// and once more on a phone for background ("Always") location.
fn show_ask(own: &OwnView, explained: bool) -> bool {
    match own.participation {
        Participation::AwaitingPermission => native::NATIVE_MOBILE || !explained,
        Participation::Participating => {
            native::NATIVE_MOBILE && own.permission == Permission::Foreground && !explained
        }
        _ => false,
    }
}

fn explained() -> bool {
    let _ = PREFS_REVISION.read();
    account_key().is_some_and(|(key, _)| load_prefs().account(&key).explained)
}

/// The station's request for this device's location, with what is sent.
#[component]
fn PermissionAsk(own: OwnView) -> Element {
    let radius = PROXIMITY_STATUS.read().as_ref().map_or(150, |s| s.settings.radius_m);
    let upgrade = own.permission == Permission::Foreground;
    let ask = move |_| {
        // The background explanation is shown once; the first request leads
        // to it on a phone once "while in use" is granted.
        if (upgrade || !native::NATIVE_MOBILE) && let Some((key, _)) = account_key() {
            edit_prefs(&key, |p| p.explained = true);
        }
        // Phones: the OS prompt. Elsewhere the browser or desktop prompts
        // when the location source starts, which `explained` now allows.
        native::request_permission(native::NATIVE_MOBILE);
    };
    let decline = move |_| {
        if let Some((key, _)) = account_key() {
            edit_prefs(&key, |p| {
                p.explained = true;
                if !upgrade {
                    p.opted_out = true;
                }
            });
        }
    };
    rsx! {
        section { class: "gs26-prox-ask", "data-proximity-ask": "", role: "dialog", "aria-label": "Proximity lock location request",
            strong { "This station uses a personnel proximity lock" }
            p { style: "margin:0;",
                "While someone is within {radius} m of the rocket, the station refuses arming and filling. "
                "This device works out on the device whether you are within that distance and sends the station only "
                "\"in range\" or \"out of range\". Your location never leaves this device."
            }
            p { style: "margin:0;",
                if upgrade {
                    "Allow location \"Always\" / \"All the time\" so it keeps reporting while the app is in the background; otherwise it reports only while open."
                } else {
                    "{platform_note(platform())} You can opt out at any time in Settings > Proximity; denying location counts as opting out."
                }
            }
            div { style: "display:flex;gap:8px;flex-wrap:wrap;",
                button { r#type: "button", class: "gs-btn gs-btn--primary", "data-proximity-allow": "", onclick: ask,
                    if upgrade { "Allow in the background" } else { "Allow location" }
                }
                button { r#type: "button", class: "gs-btn", onclick: decline,
                    if upgrade { "Only while open" } else { "Opt out on this device" }
                }
            }
        }
    }
}

/// The Actions tab's Safety section: who is in range or unknown, and the
/// override (administrators start it, anyone with command access ends it).
#[component]
pub(super) fn ProximityActionsNotice() -> Element {
    let status = PROXIMITY_STATUS.read().clone();
    let Some(status) = status.filter(|s| s.settings.enabled) else {
        return rsx! {};
    };
    let in_range = status.names(DeviceState::InRange);
    let unknown = status.names(DeviceState::Unknown);
    let admin = crate::auth::is_admin();
    let can_end = crate::auth::current_status().permissions.send_commands;
    rsx! {
        section { "aria-label": "Proximity lock", "data-proximity-actions": "", style: "display:grid;gap:6px;",
            ProximityBanner {}
            if !in_range.is_empty() {
                p { style: "margin:0;", "data-proximity-in-range": "", strong { "In range: " } "{in_range.join(\", \")}" }
            }
            if !unknown.is_empty() {
                p { style: "margin:0;", "data-proximity-unknown": "", strong { "Unknown (not reporting): " } "{unknown.join(\", \")}" }
            }
            if !status.blocked_commands.is_empty() && (status.active || status.override_view.is_some()) {
                p { style: "margin:0;font-size:13px;opacity:.85;", "Refused while active: {status.blocked_commands.join(\", \")}. Never refused: Abort, Stop, vent, dump, supply close, pause/cancel, reset and recover." }
            }
            if status.override_view.is_some() && can_end {
                EndOverride {}
            } else if admin && status.condition {
                StartOverride {}
            }
        }
    }
}

#[component]
fn StartOverride() -> Element {
    let mut open = use_signal(|| false);
    let mut minutes = use_signal(|| "10".to_string());
    let mut reason = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(|| None::<String>);
    let parsed = minutes.read().trim().parse::<u32>().map_err(|_| "Minutes must be a number.".to_string())
        .and_then(|m| validate_override(m, &reason.read()));
    rsx! {
        div { style: "display:flex;flex-wrap:wrap;gap:8px;align-items:end;",
            label { style: "display:grid;gap:2px;", "Override minutes (1–30)"
                input { r#type: "number", min: "1", max: "30", value: "{minutes}", style: "width:6ch;", oninput: move |e| minutes.set(e.value()) }
            }
            label { style: "display:grid;gap:2px;flex:1;min-width:16ch;", "Reason"
                input { r#type: "text", value: "{reason}", placeholder: "pad crew filling on purpose", oninput: move |e| reason.set(e.value()) }
            }
            button { r#type: "button", class: "gs-btn", "data-proximity-override": "", disabled: parsed.is_err() || busy(),
                onclick: move |_| open.set(true), "Override proximity lock…" }
        }
        if let Err(problem) = parsed.as_ref() && !reason.read().is_empty() {
            p { role: "status", class: "gs-notice gs-notice--warn", style: "margin:0;", "{problem}" }
        }
        if let Some(text) = message() { p { role: "status", style: "margin:0;", "{text}" } }
        if open() && let Ok(request) = parsed.clone() {
            ConfirmDialog { id: "gs26-proximity-override", title: "Override the proximity lock?".to_string(),
                lines: vec![
                    format!("For {} minutes, arming and pressurizing are allowed with people within the lockout radius.", request.minutes),
                    "Use it only when the people near the rocket are meant to be there (the pad crew filling). It is logged with your name and reason, shown on every view, and ends by itself.".to_string(),
                ],
                confirm_label: "Override".to_string(), busy: busy(),
                on_cancel: move |_| open.set(false),
                on_confirm: move |_| {
                    let request = request.clone();
                    busy.set(true);
                    spawn(async move {
                        let reply = super::http_post_json::<_, Status>(OVERRIDE_PATH, &request).await;
                        busy.set(false);
                        open.set(false);
                        match reply {
                            Ok(status) => { model::apply_status(status); message.set(Some("Override active.".into())); }
                            Err(e) => message.set(Some(format!("Override refused: {}", super::igt::refusal(&e)))),
                        }
                    });
                },
            }
        }
    }
}

#[component]
fn EndOverride() -> Element {
    let mut busy = use_signal(|| false);
    let mut message = use_signal(|| None::<String>);
    rsx! {
        button { r#type: "button", class: "gs-btn gs-btn--primary", disabled: busy(), "data-proximity-end-override": "",
            onclick: move |_| {
                busy.set(true);
                spawn(async move {
                    let reply = super::http_post_json::<_, Status>(OVERRIDE_END_PATH, &serde_json::json!({})).await;
                    busy.set(false);
                    match reply {
                        Ok(status) => model::apply_status(status),
                        Err(e) => message.set(Some(format!("Not ended: {}", super::igt::refusal(&e)))),
                    }
                });
            },
            "End override now"
        }
        if let Some(text) = message() { p { role: "status", style: "margin:0;", "{text}" } }
    }
}

// ---------------------------------------------------------------------------
// Settings

fn permission_text(permission: Permission) -> &'static str {
    match permission {
        Permission::NotDetermined => "Not asked yet",
        Permission::Denied => "Denied (counts as opting out)",
        Permission::Foreground => "While the app is open",
        Permission::Background => "Always (background)",
        Permission::Unsupported => "No location on this device",
    }
}

/// Settings > Proximity (this device): participation, permission, and
/// what this device reports. Stored on this device only.
#[component]
pub(super) fn ProximityUserSettings(theme: ThemeConfig) -> Element {
    let _ = PREFS_REVISION.read();
    let status = PROXIMITY_STATUS.read().clone();
    let own = OWN.read().clone();
    let Some((key, user)) = account_key() else {
        return rsx! { section { p { "Sign in to take part in the proximity lock." } } };
    };
    let prefs = load_prefs().account(&key);
    let name = device_name(&prefs, &user);
    let mut draft = use_signal(|| name.clone());
    let enabled = status.as_ref().is_some_and(|s| s.settings.enabled);
    let state = own_state_text(own.participation, own.result, own.reference);
    let key_toggle = key.clone();
    let key_name = key.clone();
    rsx! {
        section { "data-proximity-user": "", style: "display:grid;gap:10px;min-width:0;",
            h3 { style: "margin:0;", "Proximity lock (this device)" }
            p { style: "margin:0;color:{theme.text_muted};",
                "When the station turns the lock on, this device takes part unless you opt out here. It works out on the device whether you are within the lockout radius of the rocket and sends the station only in range / out of range. Your location is never sent."
            }
            if !enabled {
                p { role: "status", class: "gs-notice", style: "margin:0;", "The station has the proximity lock off. Nothing is sent." }
            }
            label { style: "display:flex;gap:8px;align-items:center;font-weight:700;",
                input { r#type: "checkbox", "data-proximity-participate": "", checked: !prefs.opted_out,
                    onchange: move |e| {
                        let on = e.checked();
                        edit_prefs(&key_toggle, |p| p.opted_out = !on);
                    },
                }
                "Take part on this device"
            }
            dl { style: "display:grid;grid-template-columns:max-content minmax(0,1fr);gap:4px 12px;margin:0;",
                dt { style: "font-weight:700;", "Location permission" }
                dd { style: "margin:0;", "data-proximity-permission": "", "{permission_text(own.permission)}" }
                dt { style: "font-weight:700;", "Your device reports" }
                dd { style: "margin:0;", "data-proximity-own": "", "{state}" }
                dt { style: "font-weight:700;", "Background" }
                dd { style: "margin:0;", "{platform_note(platform())}" }
            }
            if own.permission == Permission::Denied {
                p { role: "status", class: "gs-notice gs-notice--warn", style: "margin:0;",
                    "Location permission is denied for this app, so this device does not take part (the same as opting out). Allow location in the system settings to take part."
                }
            }
            if native::NATIVE_MOBILE && !prefs.opted_out {
                div { style: "display:flex;gap:8px;flex-wrap:wrap;",
                    if matches!(own.permission, Permission::NotDetermined | Permission::Foreground) {
                        button { r#type: "button", class: "gs-btn", onclick: move |_| native::request_permission(true),
                            if own.permission == Permission::Foreground { "Allow in the background" } else { "Allow location" }
                        }
                    }
                    button { r#type: "button", class: "gs-btn", onclick: move |_| native::open_settings(), "Open system settings" }
                }
            }
            label { style: "display:grid;gap:2px;font-weight:700;", "Device name shown to the station"
                div { style: "display:flex;gap:8px;flex-wrap:wrap;",
                    input { r#type: "text", maxlength: "48", value: "{draft}", oninput: move |e| draft.set(e.value()) }
                    button { r#type: "button", class: "gs-btn", disabled: !valid_device_name(&draft.read()) || *draft.read() == name,
                        onclick: move |_| {
                            let next = draft.peek().trim().to_owned();
                            edit_prefs(&key_name, |p| p.device_name = Some(next));
                        },
                        "Save name"
                    }
                }
            }
            if let Some(error) = own.last_error.clone() {
                p { role: "alert", class: "gs-notice gs-notice--danger", style: "margin:0;", "Last report failed: {error}" }
            }
        }
    }
}

/// Settings > Safety (station): the lock's settings for administrators,
/// read-only for everyone else, with the device list.
#[component]
pub(super) fn ProximityAdminSettings(theme: ThemeConfig) -> Element {
    // A write owner bound to this view's station and sign-in, as the other
    // station settings use; an observer owner cannot carry a write.
    let factory = super::station_settings_request::use_factory();
    let status = PROXIMITY_STATUS.read().clone();
    let admin = crate::auth::is_admin();
    let current = status.as_ref().map(|s| s.settings).unwrap_or_default();
    let mut draft = use_signal(|| current);
    let mut seen = use_signal(|| current);
    if *seen.peek() != current {
        // The station changed (another admin, or our save): follow it.
        seen.set(current);
        draft.set(current);
    }
    let mut confirming = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut result = use_signal(|| None::<(bool, String)>);
    let next = draft();
    let update = SettingsUpdate::between(&current, &next);
    let valid = validate_settings(&next);
    let number = |text: String, fallback: u32| text.trim().parse::<u32>().unwrap_or(fallback);
    let devices = status.as_ref().map(|s| s.devices.clone()).unwrap_or_default();
    rsx! {
        section { "data-proximity-admin": "", style: "display:grid;gap:10px;min-width:0;",
            h3 { style: "margin:0;", "Personnel proximity lock" }
            p { style: "margin:0;color:{theme.text_muted};",
                "Optional. When on, devices that take part tell the station only whether they are within the radius of the rocket; while one is, the station refuses new arming and pressurizing commands. Safing is never refused. Users may opt out on their own devices."
            }
            if let Some(s) = status.as_ref().filter(|s| s.settings.enabled) {
                p { role: "status", "data-proximity-summary": "", style: "margin:0;", "{s.summary}" }
            }
            fieldset { disabled: !admin || busy(), style: "display:grid;gap:8px;border:1px solid {theme.border};border-radius:10px;padding:10px;",
                legend { if admin { "Settings" } else { "Settings (administrator only)" } }
                label { style: "display:flex;gap:8px;align-items:center;font-weight:700;",
                    input { r#type: "checkbox", "data-proximity-enabled": "", checked: next.enabled, onchange: move |e| draft.with_mut(|d| d.enabled = e.checked()) }
                    "Proximity lock on"
                }
                div { style: "display:flex;flex-wrap:wrap;gap:12px;",
                    label { style: "display:grid;gap:2px;", "Radius (25–2000 m)"
                        input { r#type: "number", min: "25", max: "2000", "data-proximity-radius": "", value: "{next.radius_m}", style: "width:9ch;",
                            oninput: move |e| draft.with_mut(|d| d.radius_m = number(e.value(), d.radius_m)) }
                    }
                    label { style: "display:grid;gap:2px;", "Exit margin (5–500 m)"
                        input { r#type: "number", min: "5", max: "500", value: "{next.exit_margin_m}", style: "width:8ch;",
                            oninput: move |e| draft.with_mut(|d| d.exit_margin_m = number(e.value(), d.exit_margin_m)) }
                    }
                    label { style: "display:grid;gap:2px;", "Heartbeat timeout (30–600 s)"
                        input { r#type: "number", min: "30", max: "600", value: "{next.heartbeat_timeout_s}", style: "width:8ch;",
                            oninput: move |e| draft.with_mut(|d| d.heartbeat_timeout_s = number(e.value(), d.heartbeat_timeout_s)) }
                    }
                }
                div { role: "group", "aria-label": "Blocked actions", style: "display:flex;flex-wrap:wrap;gap:12px;",
                    span { style: "font-weight:700;", "Block:" }
                    label { input { r#type: "checkbox", checked: next.blocks.arming, onchange: move |e| draft.with_mut(|d| d.blocks.arming = e.checked()) } " Arming (Postinit, Advance, automatic Armed, LIVE PYRO)" }
                    label { input { r#type: "checkbox", checked: next.blocks.fill, onchange: move |e| draft.with_mut(|d| d.blocks.fill = e.checked()) } " Fill sequences (fill, nitrogen test, turbocharge)" }
                    label { input { r#type: "checkbox", checked: next.blocks.launch, onchange: move |e| draft.with_mut(|d| d.blocks.launch = e.checked()) } " Launch sequences and ignition" }
                    label { input { r#type: "checkbox", checked: next.blocks.manual_valves, onchange: move |e| draft.with_mut(|d| d.blocks.manual_valves = e.checked()) } " Single manual valve opens (Nitrogen, Nitrous, PILOT)" }
                    small { style: "flex-basis:100%;opacity:0.8;", "The valve self-test and safing (Abort, vent, dump, closes) are never blocked." }
                }
                div { role: "radiogroup", "aria-label": "Devices that stop reporting", style: "display:grid;gap:4px;",
                    span { style: "font-weight:700;", "A participating device that stops reporting:" }
                    label { input { r#type: "radio", name: "gs26-proximity-stale", checked: next.stale_policy == StalePolicy::Warn,
                        onchange: move |_| draft.with_mut(|d| d.stale_policy = StalePolicy::Warn) }
                        " Warns (recommended): listed prominently, does not block. A phone that dies or loses signal does not stop the operation." }
                    label { input { r#type: "radio", name: "gs26-proximity-stale", checked: next.stale_policy == StalePolicy::Block,
                        onchange: move |_| draft.with_mut(|d| d.stale_policy = StalePolicy::Block) }
                        " Blocks (safest): treated as in range until it reports again or an administrator removes it." }
                }
                if let Err(problem) = valid.as_ref() {
                    p { role: "status", class: "gs-notice gs-notice--warn", style: "margin:0;", "{problem}" }
                }
                if admin {
                    div { style: "display:flex;gap:8px;",
                        button { r#type: "button", class: "gs-btn gs-btn--primary", disabled: update.is_empty() || valid.is_err() || busy(),
                            onclick: move |_| { result.set(None); confirming.set(true); }, "Save…" }
                        button { r#type: "button", class: "gs-btn", disabled: update.is_empty() || busy(), onclick: move |_| draft.set(current), "Discard" }
                    }
                }
            }
            if !devices.is_empty() {
                DeviceTable { devices, admin }
            }
            if let Some((ok, text)) = result() {
                p { role: if ok { "status" } else { "alert" }, class: if ok { "gs-notice" } else { "gs-notice gs-notice--danger" }, style: "margin:0;", "{text}" }
            }
        }
        if confirming() {
            ConfirmDialog { id: "gs26-proximity-settings", title: "Change the proximity lock?".to_string(),
                lines: settings_confirm_lines(&current, &next), confirm_label: "Save".to_string(), busy: busy(),
                on_cancel: move |_| confirming.set(false),
                on_confirm: move |_| {
                    let body = SettingsUpdate::between(&current, &draft.peek());
                    let owner = factory.capture();
                    busy.set(true);
                    spawn(async move {
                        let reply = super::station_settings_request::put_captured::<_, Status>(SETTINGS_PATH, &body, owner).await;
                        busy.set(false);
                        confirming.set(false);
                        match reply {
                            Ok(status) => { model::apply_status(status); result.set(Some((true, "Proximity lock settings saved.".into()))); }
                            Err(e) => result.set(Some((false, format!("Not saved: {}", super::igt::refusal(&e))))),
                        }
                    });
                },
            }
        }
    }
}

pub(crate) fn settings_confirm_lines(current: &Settings, next: &Settings) -> Vec<String> {
    let mut lines = Vec::new();
    if current.enabled != next.enabled {
        lines.push(if next.enabled {
            format!("Turn the lock ON: devices within {} m of the rocket refuse new arming and pressurizing commands. Every signed-in device takes part unless its user opts out.", next.radius_m)
        } else {
            "Turn the lock OFF: nothing is refused for proximity, device states are forgotten and any override ends.".into()
        });
    }
    if current.radius_m != next.radius_m {
        lines.push(format!("Radius {} m -> {} m.", current.radius_m, next.radius_m));
    }
    if current.exit_margin_m != next.exit_margin_m {
        lines.push(format!("Exit margin {} m -> {} m.", current.exit_margin_m, next.exit_margin_m));
    }
    if current.blocks != next.blocks {
        let on = |b: bool| if b { "blocked" } else { "allowed" };
        lines.push(format!(
            "Arming {}, fill sequences {}, launch/ignition {}, manual valve opens {}.",
            on(next.blocks.arming),
            on(next.blocks.fill),
            on(next.blocks.launch),
            on(next.blocks.manual_valves)
        ));
    }
    if current.stale_policy != next.stale_policy {
        lines.push(match next.stale_policy {
            StalePolicy::Warn => "Devices that stop reporting only warn.".into(),
            StalePolicy::Block => "Devices that stop reporting BLOCK until they report again or are removed.".into(),
        });
    }
    if current.heartbeat_timeout_s != next.heartbeat_timeout_s {
        lines.push(format!("Heartbeat timeout {} s -> {} s.", current.heartbeat_timeout_s, next.heartbeat_timeout_s));
    }
    lines.push("The change is logged with your name.".into());
    lines
}

fn state_label(state: DeviceState) -> &'static str {
    match state {
        DeviceState::InRange => "in range",
        DeviceState::OutOfRange => "out of range",
        DeviceState::Unknown => "unknown",
        DeviceState::OptedOut => "not participating",
    }
}

#[component]
fn DeviceTable(devices: Vec<DeviceView>, admin: bool) -> Element {
    let mut message = use_signal(|| None::<String>);
    rsx! {
        table { "data-proximity-devices": "", style: "border-collapse:collapse;width:100%;font-size:14px;",
            thead { tr { th { style: "text-align:left;", "Device" } th { style: "text-align:left;", "Account" } th { style: "text-align:left;", "State" } th {} } }
            tbody {
                for device in devices {
                    tr { key: "{device.id}", "data-state": state_label(device.state),
                        td { "{device.name}" }
                        td { "{device.user}" }
                        td {
                            "{state_label(device.state)}"
                            if device.state == DeviceState::OptedOut && device.opt_out == Some(OptOut::PermissionDenied) { " (location denied)" }
                            if device.participating_in_background() { " · background" }
                        }
                        td {
                            if admin && device.state == DeviceState::Unknown {
                                button { r#type: "button", class: "gs-btn", onclick: {
                                    let id = device.id.clone();
                                    move |_| {
                                        let id = id.clone();
                                        spawn(async move {
                                            match super::http_post_json::<_, Status>(DISMISS_PATH, &serde_json::json!({"id": id})).await {
                                                Ok(status) => model::apply_status(status),
                                                Err(e) => message.set(Some(super::igt::refusal(&e))),
                                            }
                                        });
                                    }
                                }, "Remove" }
                            }
                        }
                    }
                }
            }
        }
        if let Some(text) = message() { p { role: "alert", style: "margin:0;", "{text}" } }
    }
}

impl DeviceView {
    fn participating_in_background(&self) -> bool {
        self.background && self.state != DeviceState::OptedOut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(status: Option<Status>, own: OwnView) -> String {
        let mut dom = VirtualDom::new_with_props(
            |(status, own): (Option<Status>, OwnView)| {
                *PROXIMITY_STATUS.write() = status;
                *OWN.write() = own;
                rsx! { ProximityBanner {} }
            },
            (status, own),
        );
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    fn enabled() -> Status {
        Status {
            settings: Settings { enabled: true, ..Settings::default() },
            summary: "Proximity lock: 4 devices reporting, 1 in range (Alex's iPhone), 2 opted out".into(),
            ..Status::default()
        }
    }

    #[test]
    fn the_banner_shows_the_lock_the_override_and_the_indicator() {
        assert!(!render(None, OwnView::default()).contains("data-proximity-banner"));
        let quiet = render(Some(enabled()), OwnView::default());
        assert!(quiet.contains("data-proximity-banner=\"indicator\"") && quiet.contains("2 opted out"), "{quiet}");
        let active = Status {
            active: true,
            condition: true,
            reason: Some("Proximity lock: 1 person within 150 m of the rocket (Alex's iPhone)".into()),
            ..enabled()
        };
        let page = render(Some(active.clone()), OwnView::default());
        assert!(page.contains("PROXIMITY LOCK") && page.contains("1 person within 150 m of the rocket (Alex&#39;s iPhone)"), "{page}");
        assert!(page.contains("safing is not"));
        let overridden = Status {
            active: false,
            override_view: Some(OverrideView { by: "admin".into(), reason: "pad crew".into(), expires_ms: i64::MAX / 2, remaining_s: 0 }),
            ..active
        };
        let page = render(Some(overridden), OwnView::default());
        assert!(page.contains("PROXIMITY LOCK OVERRIDDEN") && page.contains("pad crew"), "{page}");
    }

    #[test]
    fn the_permission_request_explains_what_is_sent() {
        let own = OwnView { participation: Participation::AwaitingPermission, ..OwnView::default() };
        let page = render(Some(enabled()), own);
        assert!(page.contains("data-proximity-ask") && page.contains("never leaves this device"), "{page}");
        assert!(page.contains("Opt out on this device"));
        assert!(!render(Some(enabled()), OwnView { participation: Participation::OptedOut, ..OwnView::default() })
            .contains("data-proximity-ask"));
    }

    fn render_settings(status: Option<Status>, admin_view: bool) -> String {
        let mut dom = VirtualDom::new_with_props(
            |(status, user): (Option<Status>, bool)| {
                *PROXIMITY_STATUS.write() = status;
                rsx! {
                    if user {
                        ProximityUserSettings { theme: ThemeConfig::default() }
                    } else {
                        ProximityAdminSettings { theme: ThemeConfig::default() }
                    }
                }
            },
            (status, !admin_view),
        );
        dom.rebuild_in_place();
        dioxus::ssr::render(&dom)
    }

    #[test]
    fn settings_flows_render_for_users_and_non_admins() {
        // Not signed in: the user section says so. The session is process
        // wide and other tests leave theirs behind, so clear it under the lock.
        crate::auth::hold_station_identity_for_test();
        crate::auth::clear_current_session();
        let page = render_settings(Some(enabled()), false);
        assert!(page.contains("Sign in to take part"), "{page}");
        // Non-administrators see the settings read-only, with the summary.
        let mut status = enabled();
        status.devices = vec![DeviceView {
            id: "a1".into(), name: "Bo's Pixel".into(), user: "bo".into(), platform: Platform::Android,
            state: DeviceState::Unknown, accuracy: None, reference: None, background: true, opt_out: None, last_report_ms: 0,
        }];
        let page = render_settings(Some(status), true);
        assert!(page.contains("Settings (administrator only)"), "{page}");
        assert!(page.contains("<fieldset disabled"), "{page}");
        assert!(page.contains("2 opted out") && page.contains("Bo&#39;s Pixel") && page.contains("unknown"), "{page}");
        assert!(!page.contains(">Remove<"), "only administrators remove devices");
        assert!(page.contains("Warns (recommended)") && page.contains("Blocks (safest)"));
    }

    #[test]
    fn the_explanation_comes_before_any_prompt() {
        let own = |participation, permission| OwnView { participation, permission, ..OwnView::default() };
        let waiting = own(Participation::AwaitingPermission, Permission::NotDetermined);
        assert!(show_ask(&waiting, false));
        // On desktop/web the explanation is answered once; a phone keeps
        // asking until the OS answers.
        assert_eq!(show_ask(&waiting, true), native::NATIVE_MOBILE);
        let foreground = own(Participation::Participating, Permission::Foreground);
        assert_eq!(show_ask(&foreground, false), native::NATIVE_MOBILE, "background upgrade on phones only");
        assert!(!show_ask(&foreground, true));
        assert!(!show_ask(&own(Participation::OptedOut, Permission::Background), false));
        assert!(!show_ask(&own(Participation::PermissionDenied, Permission::Denied), false));
    }

    #[test]
    fn confirm_lines_name_each_change() {
        let current = Settings::default();
        let next = Settings { enabled: true, radius_m: 300, stale_policy: StalePolicy::Block, ..current };
        let lines = settings_confirm_lines(&current, &next).join("\n");
        assert!(lines.contains("Turn the lock ON") && lines.contains("300 m"), "{lines}");
        assert!(lines.contains("Radius 150 m -> 300 m"));
        assert!(lines.contains("BLOCK until"));
        assert!(lines.contains("logged with your name"));
    }

    #[test]
    fn heartbeats_follow_participation() {
        let base = Report {
            v: 1, device_id: "d".into(), device_name: "n".into(), platform: Platform::Web, participating: false,
            in_range: None, accuracy: None, reference: None, background: false, opt_out: Some(OptOut::User), seq: 1, sent_ms: 0,
        };
        assert!(report_due_every(None, &base, 0, 300_000));
        assert!(!report_due_every(Some((&base, 0)), &base, 20_000, 300_000), "opted out: no 15 s heartbeat");
        assert!(report_due_every(Some((&base, 0)), &base, 300_000, 300_000));
        let participating = Report { participating: true, opt_out: None, in_range: Some(false), ..base.clone() };
        assert!(report_due_every(Some((&base, 0)), &participating, 1_000, HEARTBEAT_MS), "a change goes at once");
        assert!(!report_due_every(Some((&participating, 0)), &participating, 14_000, HEARTBEAT_MS));
        assert!(report_due_every(Some((&participating, 0)), &participating, 15_000, HEARTBEAT_MS));
    }
}
