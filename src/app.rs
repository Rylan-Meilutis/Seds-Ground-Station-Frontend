// frontend/src/app.rs
//
const _CONNECTION_TIMEOUT_MS: u64 = 8000;
const _BODY_TRANSFER_TIMEOUT_MS: u64 = 10000;
const _WS_TIMEOUT_MS: u64 = 4500;
pub(crate) const APP_DISPLAY_NAME: &str = "UBSEDS GS";

use crate::auth::{self, SessionStatus as AuthSessionStatus};
use crate::telemetry_dashboard::layout::ThemeConfig;
use dioxus::prelude::*;
#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
use dioxus_desktop::use_window;
use dioxus_router::{Routable, Router, use_navigator};

#[allow(unused_imports)]
use crate::telemetry_dashboard::{self, UrlConfig};

const INLINE_MAPLIBRE_CSS: &str = include_str!("../static/vendor/maplibre-gl/maplibre-gl.css");
const INLINE_MAPLIBRE_JS: &str = include_str!("../static/vendor/maplibre-gl/maplibre-gl.js");
const INLINE_GROUND_MAP_JS: &str = include_str!("../static/ground_map.js");

// -------------------------
// Native-only keep-awake shims (mobile)
// -------------------------
#[cfg(not(target_arch = "wasm32"))]
mod keep_awake {
    #[cfg(target_os = "ios")]
    mod ios {
        use std::os::raw::c_int;

        unsafe extern "C" {
            fn gs26_set_idle_timer_disabled(disabled: c_int);
        }

        /// Toggles the iOS idle-timer suppression used while the app is active.
        pub fn set_enabled(enabled: bool) {
            // iOS API is "idle timer disabled", so enabled=true -> disabled=1
            unsafe { gs26_set_idle_timer_disabled(if enabled { 1 } else { 0 }) };
        }
    }

    #[cfg(target_os = "android")]
    mod android {
        /// Forwards the keep-awake request into the Android glue code.
        pub fn set_enabled(enabled: bool) {
            crate::telemetry_dashboard::gps_android::set_keep_screen_on(enabled);
        }
    }

    /// Enables or disables keep-awake on native platforms that support it.
    pub fn set_enabled(enabled: bool) {
        #[cfg(target_os = "ios")]
        ios::set_enabled(enabled);

        #[cfg(target_os = "android")]
        android::set_enabled(enabled);

        // Other native targets: no-op
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            let _ = enabled;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// Returns whether every HTTP probe and the WebSocket handshake succeeded.
fn all_tests_passed(checks: &[RouteCheck], ws_probe: &WsProbeStatus) -> bool {
    let routes_ok = checks.iter().all(|c| c.ok);
    let ws_ok = ws_probe.ok;
    routes_ok && ws_ok
}

// App shell styling, injected web assets, and shared route-card helpers.
include!("app_shell.rs");

#[derive(Clone, Routable, PartialEq)]
pub enum Route {
    #[route("/")]
    Root {},

    #[route("/dashboard")]
    Dashboard {},

    #[route("/login")]
    Login {},

    #[cfg(not(target_arch = "wasm32"))]
    #[route("/connect")]
    Connect {},

    #[route("/version")]
    Version {},

    #[cfg(not(target_arch = "wasm32"))]
    #[route("/settings")]
    Settings {},
}

#[cfg(target_arch = "wasm32")]
/// Redirects the web build's root route to the dashboard entrypoint.
fn connect_route() -> Route {
    Route::Root {}
}

#[cfg(not(target_arch = "wasm32"))]
/// Returns the native-only connection setup route.
fn connect_route() -> Route {
    Route::Connect {}
}

#[cfg(target_arch = "wasm32")]
/// Returns the authenticated landing route for the web build.
fn authenticated_route() -> Route {
    Route::Root {}
}

#[cfg(not(target_arch = "wasm32"))]
/// Returns the authenticated landing route for native builds.
fn authenticated_route() -> Route {
    Route::Dashboard {}
}

// Native connection setup helpers and backend route probes.
include!("app_connection.rs");

// -------------------------
// App
// -------------------------
#[component]
/// Top-level app component that installs global CSS and mounts the router.
pub fn App() -> Element {
    #[cfg(not(target_arch = "wasm32"))]
    {
        keep_awake::set_enabled(true);
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    {
        let window = use_window();
        use_effect(move || {
            window.set_title(APP_DISPLAY_NAME);
        });
    }
    let theme = shell_theme();
    {
        let theme = theme.clone();
        use_effect(move || {
            telemetry_dashboard::apply_window_theme(&theme);
        });
    }

    #[cfg(target_arch = "wasm32")]
    {
        use_effect(move || {
            ensure_document_inline_style("gs26-global-css", GLOBAL_CSS);
            ensure_document_inline_style("gs26-maplibre-css", INLINE_MAPLIBRE_CSS);
            ensure_document_inline_script("gs26-maplibre-js", INLINE_MAPLIBRE_JS);
            ensure_document_inline_script("gs26-ground-map-js", INLINE_GROUND_MAP_JS);
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    let document_assets: Element = rsx! {
        document::Style { "{GLOBAL_CSS}" }
        document::Style { "{INLINE_MAPLIBRE_CSS}" }
        document::Script { "{INLINE_MAPLIBRE_JS}" }
        document::Script { "{INLINE_GROUND_MAP_JS}" }
    };
    #[cfg(target_arch = "wasm32")]
    let document_assets: Element = rsx! { Fragment {} };

    rsx! {
        Meta { name: "viewport", content: "width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no" }
        {document_assets}

        div {
            style: "min-height: var(--gs26-app-height); width: 100%; background: var(--gs26-app-background); color: var(--gs26-app-text);",
            Router::<Route> {}
        }
    }
}

#[component]
/// Root route that redirects native builds to either connect or dashboard.
pub fn Root() -> Element {
    #[cfg(target_arch = "wasm32")]
    {
        return rsx! { Dashboard {} };
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let nav = use_navigator();

        use_effect(move || {
            let _ = nav.replace(route_for_configured_connection());
        });

        rsx! { div {} }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn route_for_configured_connection() -> Route {
    if UrlConfig::_stored_base_url().is_some() {
        Route::Dashboard {}
    } else {
        connect_route()
    }
}

#[cfg(target_arch = "wasm32")]
fn route_for_configured_connection() -> Route {
    Route::Root {}
}

#[component]
/// Shared login card used by both the full login page and the overlay flow.
fn LoginCard(
    title: String,
    subtitle: String,
    back_route: Option<Route>,
    on_success_route: Route,
    #[props(default = false)] overlay_mode: bool,
) -> Element {
    let theme = shell_theme();
    let nav = use_navigator();
    let base = UrlConfig::base_http();
    auth::init_from_storage(&base);
    let effect_base = base.clone();
    let skip_tls = UrlConfig::_skip_tls_verify();
    let sign_in_route = on_success_route.clone();
    let stored_username = auth::current_session()
        .and_then(|session| session.session.username)
        .unwrap_or_default();
    let mut username = use_signal(|| stored_username);
    let mut password = use_signal(String::new);
    let mut status = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut submit_login = move || {
        let base = UrlConfig::base_http();
        if base.trim().is_empty() {
            status.set("Configure the Ground Station URL first.".to_string());
            return;
        }
        let username_value = username();
        let password_value = password();
        if username_value.trim().is_empty() || password_value.is_empty() {
            status.set("Enter both username and password.".to_string());
            return;
        }
        let success_route = sign_in_route.clone();
        busy.set(true);
        status.set("Signing in...".to_string());
        spawn(async move {
            match auth::login(
                &base,
                skip_tls,
                username_value.trim(),
                &password_value,
                true,
            )
            .await
            {
                Ok(_) => {
                    telemetry_dashboard::reconnect_and_reseed_after_auth_change();
                    busy.set(false);
                    status.set(String::new());
                    let _ = nav.replace(success_route);
                }
                Err(err) => {
                    busy.set(false);
                    status.set(err);
                }
            }
        });
    };

    use_effect({
        let effect_base = effect_base.clone();
        move || {
            auth::init_from_storage(&effect_base);
        }
    });

    rsx! {
        div {
            style: if overlay_mode {
                format!(
                    "width:min(560px, 92vw); color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;",
                    theme.text_primary
                )
            } else {
                shell_page_style(&theme)
            },
            div {
                style: shell_card_style(&theme, "min(560px, 92vw)"),
                h1 { style: "margin:0 0 10px 0; font-size:22px;", "{title}" }
                p { style: "margin:0 0 16px 0; color:{theme.text_muted};", "{subtitle}" }
                form {
                    onsubmit: move |evt| {
                        evt.prevent_default();
                        submit_login();
                    },
                    if base.trim().is_empty() {
                        div {
                            style: shell_warning_style(&theme),
                            "Configure the Ground Station URL before logging in."
                        }
                    }

                    label { r#for: "gs26-login-username", style: "display:block; margin-bottom:8px; font-size:13px; color:{theme.text_muted};", "Username" }
                    input {
                        id: "gs26-login-username",
                        name: "username",
                        autocomplete: "username",
                        autocapitalize: "none",
                        spellcheck: "false",
                        style: shell_input_style(&theme, true),
                        placeholder: "Username",
                        value: "{username()}",
                        oninput: move |evt| username.set(evt.value()),
                    }

                    label { r#for: "gs26-login-password", style: "display:block; margin-bottom:8px; font-size:13px; color:{theme.text_muted};", "Password" }
                    input {
                        id: "gs26-login-password",
                        name: "password",
                        autocomplete: "current-password",
                        style: shell_input_style(&theme, false),
                        r#type: "password",
                        placeholder: "Password",
                        value: "{password()}",
                        oninput: move |evt| password.set(evt.value()),
                    }

                    if !status().is_empty() {
                        div {
                            style: shell_notice_style(&theme),
                            "{status()}"
                        }
                    }

                    div { style: "display:flex; gap:12px; margin-top:16px; justify-content:flex-end; flex-wrap:wrap;",
                    if let Some(back_route) = back_route.clone() {
                        button {
                            style: shell_button_alt_style(&theme),
                            r#type: "button",
                            onclick: move |_| {
                                let _ = nav.replace(back_route.clone());
                            },
                            "Back"
                        }
                    }

                    button {
                        r#type: "submit",
                        style: shell_button_style(&theme),
                        disabled: busy() || base.trim().is_empty(),
                        if busy() { "Signing In..." } else { "Sign In" }
                    }
                }
                }
            }
        }
    }
}

#[component]
fn ConnectionFailedCard(message: String, on_retry: EventHandler<()>) -> Element {
    let theme = shell_theme();
    let nav = use_navigator();
    rsx! {
        div {
            style: shell_page_style(&theme),
            div {
                style: shell_card_style(&theme, "min(560px, 92vw)"),
                h1 { style: "margin:0 0 10px 0; font-size:22px;", "Failed to Connect" }
                p { style: "margin:0 0 16px 0; color:{theme.text_muted}; white-space:pre-wrap; overflow-wrap:anywhere; word-break:break-word;", "{message}" }
                div { style: "display:flex; gap:12px; justify-content:flex-end; flex-wrap:wrap;",
                    button {
                        style: shell_button_style(&theme),
                        onclick: move |_| {
                            let _ = nav.replace(connect_route());
                        },
                        "Back to Connect"
                    }
                    button {
                        style: shell_button_alt_style(&theme),
                        onclick: move |_| {
                            on_retry.call(());
                        },
                        "Retry"
                    }
                }
            }
        }
    }
}

#[component]
fn LoginOverlay(
    title: String,
    subtitle: String,
    back_route: Option<Route>,
    on_success_route: Route,
) -> Element {
    let theme = shell_theme();
    rsx! {
        div {
            style: "position:relative; width:100%; min-height:var(--gs26-app-height);",
            crate::telemetry_dashboard::TelemetryDashboard {}
            div {
                style: format!(
                    "position:fixed; inset:0; display:flex; align-items:center; justify-content:center; padding:24px; background:{}; backdrop-filter:blur(8px); z-index:1000;",
                    theme.overlay_background
                ),
                LoginCard {
                    title: title.clone(),
                    subtitle: subtitle.clone(),
                    back_route: back_route.clone(),
                    on_success_route: on_success_route.clone(),
                    overlay_mode: true,
                }
            }
        }
    }
}

#[component]
pub fn Login() -> Element {
    #[cfg(not(target_arch = "wasm32"))]
    if UrlConfig::_stored_base_url().is_none() {
        let nav = use_navigator();
        use_effect(move || {
            let _ = nav.replace(connect_route());
        });
        return rsx! {
            div {
                style: format!(
                    "height:var(--gs26-app-height); display:flex; align-items:center; justify-content:center; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;",
                    shell_theme().app_background,
                    shell_theme().text_primary
                ),
                div {
                    style: shell_card_style(&shell_theme(), "min(520px, 92vw)"),
                    h1 { style: "margin:0 0 10px 0; font-size:20px;", "Connect First" }
                    p { style: "margin:0 0 16px 0; color:{shell_theme().text_muted};", "Configure and connect to a Ground Station before signing in." }
                    button {
                        style: shell_button_style(&shell_theme()),
                        onclick: move |_| {
                            let _ = nav.replace(connect_route());
                        },
                        "Open Connect"
                    }
                }
            }
        };
    }

    #[cfg(target_arch = "wasm32")]
    let show_live_dashboard = false;
    #[cfg(not(target_arch = "wasm32"))]
    let show_live_dashboard = telemetry_dashboard::dashboard_has_prior_backend_connection();
    if show_live_dashboard {
        rsx! {
            LoginOverlay {
                title: "Sign In".to_string(),
                subtitle: "Authenticate with the Ground Station to view protected data or send commands.".to_string(),
                back_route: Some(Route::Dashboard {}),
                on_success_route: authenticated_route(),
            }
        }
    } else {
        rsx! {
            LoginCard {
                title: "Sign In".to_string(),
                subtitle: "Authenticate with the Ground Station to view protected data or send commands.".to_string(),
                back_route: Some(connect_route()),
                on_success_route: authenticated_route(),
            }
        }
    }
}

// Native connection setup page.
include!("app_connect_page.rs");

#[cfg(not(target_arch = "wasm32"))]
#[component]
pub fn Settings() -> Element {
    let theme = shell_theme();
    let nav = use_navigator();

    rsx! {
        div {
            style: format!(
                "min-height:var(--gs26-app-height); height:var(--gs26-app-height); overflow:hidden; display:flex; align-items:center; justify-content:center; padding:24px 16px; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;",
                theme.app_background, theme.text_primary
            ),
            div {
                style: format!(
                    "{} display:flex; flex-direction:column; width:min(980px, 94vw); height:min(900px, calc(var(--gs26-app-height) - 48px)); max-height:min(900px, calc(var(--gs26-app-height) - 48px)); overflow:hidden;",
                    shell_card_style(&theme, "min(980px, 94vw)")
                ),
                div {
                    style: format!(
                        "display:flex; align-items:flex-start; justify-content:space-between; gap:12px; margin-bottom:12px; padding-bottom:12px; flex-wrap:wrap; border-bottom:1px solid {};",
                        theme.border_soft
                    ),
                    h1 { style: "margin:0; font-size:20px;", "Settings" }
                    button {
                        style: shell_button_style(&theme),
                        onclick: move |_| {
                            let _ = nav.push(Route::Connect {});
                        },
                        "Back"
                    }
                }
                div {
                    style: "flex:1 1 auto; min-height:0; overflow:auto; padding-right:4px;",
                    crate::telemetry_dashboard::NativeSettingsPage {}
                }
            }
        }
    }
}

#[component]
pub fn Version() -> Element {
    let theme = shell_theme();
    let nav = use_navigator();
    let can_go_back = nav.can_go_back();
    let back_action = move |_| {
        if can_go_back {
            nav.go_back();
        } else {
            let _ = nav.replace(route_for_configured_connection());
        }
    };

    rsx! {
        div {
            style: format!(
                "position:fixed; inset:0; overflow-y:auto; overflow-x:hidden; display:flex; align-items:flex-start; justify-content:center; padding:24px 16px; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont; overscroll-behavior:contain; -webkit-overflow-scrolling:touch;",
                theme.app_background,
                theme.text_primary
            ),
            div {
                style: shell_card_style(&theme, "min(900px, 100%)"),
                div {
                    style: "display:flex; align-items:flex-start; justify-content:space-between; gap:12px; margin-bottom:12px; flex-wrap:wrap;",
                    h1 { style: "margin:0; font-size:20px;", "{APP_DISPLAY_NAME}" }
                    button {
                        style: shell_button_style(&theme),
                        onclick: back_action,
                        "Back"
                    }
                }
                crate::telemetry_dashboard::version_page::VersionTab { theme: theme.clone() }
            }
        }
    }
}

#[component]
pub fn Dashboard() -> Element {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let nav = use_navigator();
        if UrlConfig::_stored_base_url().is_none() {
            let theme = shell_theme();
            return rsx! {
                div {
                    style: format!(
                        "height:var(--gs26-app-height); display:flex; align-items:center; justify-content:center; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;",
                        theme.app_background,
                        theme.text_primary
                    ),
                    div {
                        style: shell_card_style(&theme, "min(560px, 92vw)"),
                        h1 { style: "margin:0 0 12px 0; font-size:18px;", "Not connected" }
                        p { style: "margin:0 0 16px 0; color:{theme.text_muted};", "Please configure the Ground Station URL on the Connect screen." }
                        button {
                            style: shell_button_style(&theme),
                            onclick: move |_| {
                                let _ = nav.replace(connect_route());
                            },
                            "Back to Connect"
                        }
                    }
                }
            };
        }
    }

    let base = UrlConfig::base_http();
    auth::init_from_storage(&base);
    let nav = use_navigator();
    let mut auth_state = use_signal(|| None::<Result<AuthSessionStatus, String>>);
    let mut auth_state_base = use_signal(String::new);
    #[cfg(not(target_arch = "wasm32"))]
    let has_cached_layout_for_base =
        telemetry_dashboard::dashboard_has_cached_layout_for_base(&base);
    #[cfg(not(target_arch = "wasm32"))]
    let has_completed_connect_flow = persist::read_connect_shown();
    #[cfg(not(target_arch = "wasm32"))]
    let can_render_dashboard_while_auth_check_runs = auth::current_session().is_some()
        || auth::current_status().permissions.view_data
        || telemetry_dashboard::dashboard_has_prior_backend_connection()
        || has_cached_layout_for_base
        || has_completed_connect_flow;
    use_effect(move || {
        let base = UrlConfig::base_http();
        let current_auth_state_base = auth_state_base.read().clone();
        if current_auth_state_base != base {
            auth_state_base.set(base.clone());
            auth_state.set(None);
        }
        let skip_tls = UrlConfig::_skip_tls_verify();
        let auth_state_ready = auth_state.read().is_some();
        if auth_state_ready {
            return;
        }
        spawn(async move {
            auth_state.set(Some(auth::fetch_session_status(&base, skip_tls).await));
        });
    });

    match auth_state.read().as_ref() {
        #[cfg(not(target_arch = "wasm32"))]
        None if can_render_dashboard_while_auth_check_runs => {
            rsx! { crate::telemetry_dashboard::TelemetryDashboard {} }
        }
        None => rsx! {
            div { style: format!("height:var(--gs26-app-height); display:flex; align-items:center; justify-content:center; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;", shell_theme().app_background, shell_theme().text_primary),
                div {
                    style: format!("padding:20px; border:1px solid {}; border-radius:16px; background:{}; min-width:min(560px, 92vw);", shell_theme().border_strong, shell_theme().panel_background),
                    h1 { style: "margin:0 0 10px 0; font-size:22px;", "Checking session..." }
                    p { style: format!("margin:0 0 16px 0; color:{};", shell_theme().text_muted), "Contacting the Ground Station session endpoint." }
                    div { style: "display:flex; gap:12px; justify-content:flex-end; flex-wrap:wrap;",
                        button {
                            style: shell_button_style(&shell_theme()),
                            onclick: move |_| {
                                let _ = nav.replace(connect_route());
                            },
                            "Cancel"
                        }
                    }
                }
            }
        },
        Some(Ok(status)) if status.permissions.view_data => {
            rsx! { crate::telemetry_dashboard::TelemetryDashboard {} }
        }
        Some(Ok(_)) => {
            #[cfg(target_arch = "wasm32")]
            {
                rsx! {
                    LoginCard {
                        title: "Sign In Required".to_string(),
                        subtitle: "This Ground Station does not allow anonymous view access. Sign in to continue.".to_string(),
                        back_route: Some(connect_route()),
                        on_success_route: authenticated_route(),
                    }
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            if telemetry_dashboard::dashboard_has_prior_backend_connection() {
                rsx! {
                    LoginOverlay {
                        title: "Sign In Required".to_string(),
                        subtitle: "This Ground Station does not allow anonymous view access. Sign in to continue.".to_string(),
                        back_route: Some(Route::Dashboard {}),
                        on_success_route: authenticated_route(),
                    }
                }
            } else {
                rsx! {
                    LoginCard {
                        title: "Sign In Required".to_string(),
                        subtitle: "This Ground Station does not allow anonymous view access. Sign in to continue.".to_string(),
                        back_route: Some(connect_route()),
                        on_success_route: authenticated_route(),
                    }
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        Some(Err(_err)) if has_cached_layout_for_base || has_completed_connect_flow => {
            rsx! { crate::telemetry_dashboard::TelemetryDashboard {} }
        }
        Some(Err(err)) => rsx! {
            ConnectionFailedCard {
                message: format_session_load_error(err),
                on_retry: move |_| {
                    let base = UrlConfig::base_http();
                    let skip_tls = UrlConfig::_skip_tls_verify();
                    auth_state.set(None);
                    spawn(async move {
                        auth_state.set(Some(auth::fetch_session_status(&base, skip_tls).await));
                    });
                },
            }
        },
    }
}
