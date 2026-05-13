// Native connection setup page.
//
// This is separated from the rest of the app routes because it owns a large
// diagnostic UI: URL editing, TLS override state, HTTP route checks, and the
// WebSocket handshake report.

#[cfg(not(target_arch = "wasm32"))]
#[component]
pub fn Connect() -> Element {
    let theme = shell_theme();
    let nav = use_navigator();

    let initial = UrlConfig::_stored_base_url()
        .unwrap_or_else(|| "https://your-ground-station-url.com".to_string());
    let (initial_scheme, initial_host) = split_base_url_for_connect(&initial);
    let initial_skip_tls = connect_scheme_supports_skip_tls(initial_scheme)
        && UrlConfig::_skip_tls_verify_for_base(&initial);

    let mut scheme_edit = use_signal(|| initial_scheme.to_string());
    let mut scheme_menu_open = use_signal(|| false);
    let mut host_edit = use_signal(|| initial_host);
    let mut skip_tls = use_signal(|| initial_skip_tls);

    let mut test_status = use_signal(String::new);
    let mut test_report = use_signal(|| None::<ConnectionTestReport>);
    let mut testing = use_signal(|| false);
    let has_test_report = test_report.read().is_some();

    use_effect({
        let scheme_edit = scheme_edit;
        let host_edit = host_edit;
        let mut skip_tls = skip_tls;
        move || {
            let scheme = scheme_edit.read().clone();
            if connect_scheme_supports_skip_tls(&scheme) {
                return;
            }
            if *skip_tls.read() {
                skip_tls.set(false);
            }
            let base = compose_base_url_for_connect(&scheme, &host_edit.read());
            if !base.is_empty() {
                UrlConfig::_set_skip_tls_verify_for_base(&base, false);
            }
        }
    });

    rsx! {
        div {
            style: format!(
                "min-height:var(--gs26-app-height); height:var(--gs26-app-height); overflow:hidden; display:flex; align-items:center; justify-content:center; padding:24px 16px; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont;",
                theme.app_background, theme.text_primary
            ),
            div {
                style: format!(
                    "{} display:flex; flex-direction:column; overflow:hidden; {};",
                    shell_card_style(
                        &theme,
                        if has_test_report { "min(900px, 94vw)" } else { "min(760px, 92vw)" }
                    ),
                    if has_test_report {
                        "height:min(900px, calc(var(--gs26-app-height) - 48px)); max-height:min(900px, calc(var(--gs26-app-height) - 48px));"
                    } else {
                        "height:auto; max-height:min(520px, calc(var(--gs26-app-height) - 48px));"
                    }
                ),

                div {
                    style: "display:flex; align-items:flex-start; justify-content:space-between; gap:12px; margin-bottom:12px;",
                    h1 { style: "margin:0; font-size:20px;", "{APP_DISPLAY_NAME}" }
                    div {
                        style: "display:flex; gap:10px; flex-wrap:wrap;",
                        button {
                            style: shell_button_style(&theme),
                            onclick: move |_| {
                                let _ = nav.push(Route::Settings {});
                            },
                            "Settings"
                        }
                        button {
                            style: shell_button_style(&theme),
                            onclick: move |_| {
                                let _ = nav.push(Route::Version {});
                            },
                            "Version"
                        }
                    }
                }
                div {
                    style: format!(
                        "flex:1 1 auto; min-height:0; {} padding-right:4px;",
                        if has_test_report {
                            "overflow:auto;"
                        } else {
                            "overflow:visible;"
                        }
                    ),
                    p { style: "margin:0 0 16px 0; color:{theme.text_muted};",
                        "Enter the Ground Station host and port. Example: ",
                        code { "your-ground-station-url.com" }
                    }

                    div {
                        style: format!(
                            "position:relative; display:flex; align-items:stretch; min-height:48px; border:1px solid {}; border-radius:12px; background:{}; color:{}; overflow:visible;",
                            theme.border, theme.app_background, theme.text_primary
                        ),
                        div {
                            style: format!(
                                "position:relative; flex:0 0 136px; border-right:1px solid {}; background:{}; padding:4px;",
                                theme.border, theme.panel_background
                            ),
                            button {
                                r#type: "button",
                                style: format!(
                                    "width:100%; height:100%; border-radius:8px; border:1px solid {}; background:{}; color:{}; font-size:13px; font-weight:700; cursor:pointer; display:flex; align-items:center; justify-content:space-between; gap:8px; padding:0 10px;",
                                    theme.button_border, theme.button_background, theme.text_primary
                                ),
                                onclick: move |_| {
                                    let next = !*scheme_menu_open.read();
                                    scheme_menu_open.set(next);
                                },
                                span { if scheme_edit() == "https://" { "https" } else { "http" } }
                                span { style: "font-size:11px; color:{theme.text_muted};", if *scheme_menu_open.read() { "▲" } else { "▼" } }
                            }
                            if *scheme_menu_open.read() {
                                div {
                                    style: "position:absolute; top:calc(100% + 6px); left:4px; right:4px; z-index:50; display:flex; flex-direction:column; gap:4px; padding:4px; border-radius:10px; border:1px solid {theme.border}; background:{theme.panel_background}; box-shadow:0 12px 30px rgba(0,0,0,0.28);",
                                    button {
                                        r#type: "button",
                                        style: format!(
                                            "width:100%; padding:9px 10px; border-radius:8px; border:1px solid {}; background:{}; color:{}; font-size:13px; font-weight:700; text-align:left; cursor:pointer;",
                                            if scheme_edit() == "https://" { theme.info_accent.as_str() } else { theme.button_border.as_str() },
                                            if scheme_edit() == "https://" { theme.info_background.as_str() } else { theme.button_background.as_str() },
                                            theme.text_primary
                                        ),
                                        onclick: move |_| {
                                            scheme_edit.set("https://".to_string());
                                            let base = compose_base_url_for_connect("https://", &host_edit.read());
                                            if !base.is_empty() {
                                                skip_tls.set(UrlConfig::_skip_tls_verify_for_base(&base));
                                            }
                                            scheme_menu_open.set(false);
                                            test_status.set(String::new());
                                            test_report.set(None);
                                        },
                                        "https"
                                    }
                                    button {
                                        r#type: "button",
                                        style: format!(
                                            "width:100%; padding:9px 10px; border-radius:8px; border:1px solid {}; background:{}; color:{}; font-size:13px; font-weight:700; text-align:left; cursor:pointer;",
                                            if scheme_edit() == "http://" { theme.info_accent.as_str() } else { theme.button_border.as_str() },
                                            if scheme_edit() == "http://" { theme.info_background.as_str() } else { theme.button_background.as_str() },
                                            theme.text_primary
                                        ),
                                        onclick: move |_| {
                                            scheme_edit.set("http://".to_string());
                                            skip_tls.set(false);
                                            let base = compose_base_url_for_connect("http://", &host_edit.read());
                                            if !base.is_empty() {
                                                UrlConfig::_set_skip_tls_verify_for_base(&base, false);
                                            }
                                            scheme_menu_open.set(false);
                                            test_status.set(String::new());
                                            test_report.set(None);
                                        },
                                        "http"
                                    }
                                }
                            }
                        },

                        input {
                            style: format!(
                                "flex:1 1 auto; min-width:0; padding:12px 14px; border:none; background:transparent; color:{}; outline:none; font-size:14px;",
                                theme.text_primary
                            ),
                            placeholder: "your-ground-station-url.com",
                            value: "{host_edit()}",
                            autocapitalize: "none",
                            spellcheck: "false",
                            oninput: move |evt| {
                                host_edit.set(evt.value().to_ascii_lowercase());
                                scheme_menu_open.set(false);
                                test_status.set(String::new());
                                test_report.set(None);
                            },
                            onkeydown: move |evt| {
                                if evt.key() != Key::Enter {
                                    return;
                                }
                                evt.prevent_default();
                                let u_norm = compose_base_url_for_connect(&scheme_edit(), &host_edit());
                                if u_norm.is_empty() {
                                    test_status.set("Enter a URL first.".to_string());
                                    return;
                                }

                                objc_poke::poke_url(&u_norm);

                                UrlConfig::set_base_url_and_persist(u_norm.to_string());
                                UrlConfig::_set_skip_tls_verify_for_base(
                                    &u_norm,
                                    connect_scheme_supports_skip_tls(&scheme_edit()) && *skip_tls.read(),
                                );
                                if UrlConfig::_stored_base_url().as_deref() != Some(u_norm.as_str()) {
                                    test_status.set(
                                        "Failed to save the Ground Station URL on this device. The app stayed disconnected."
                                            .to_string(),
                                    );
                                    return;
                                }
                                let tested_ok = test_report
                                    .read()
                                    .as_ref()
                                    .map(|report| all_tests_passed(&report.checks, &report.ws_probe))
                                    .unwrap_or(false);
                                if tested_ok {
                                    crate::telemetry_dashboard::clear_and_reconnect_after_connect();
                                } else {
                                    crate::telemetry_dashboard::reconnect_and_reseed_after_auth_change();
                                }
                                let _ = persist::write_connect_shown(true);
                                let _ = nav.replace(Route::Dashboard {});
                            },
                        }
                    }

                    div { style: "margin-top:12px; display:flex; align-items:center; gap:10px;",
                        input {
                            r#type: "checkbox",
                            disabled: !connect_scheme_supports_skip_tls(&scheme_edit()),
                            checked: *skip_tls.read(),
                            onclick: move |_| {
                                if !connect_scheme_supports_skip_tls(&scheme_edit()) {
                                    skip_tls.set(false);
                                    return;
                                }
                                let next = {
                                    let current = *skip_tls.read();
                                    !current
                                };
                                skip_tls.set(next);
                                let base = compose_base_url_for_connect(&scheme_edit(), &host_edit());
                                if !base.is_empty() {
                                    UrlConfig::_set_skip_tls_verify_for_base(&base, next);
                                }
                            }
                        }
                        div { style: "font-size:13px; color:{theme.text_muted};",
                            if connect_scheme_supports_skip_tls(&scheme_edit()) {
                                "Disable TLS certificate verification for this host (self-signed certs)"
                            } else {
                                "TLS certificate validation only applies to https:// connections"
                            }
                        }
                    }

                    if !test_status().is_empty() {
                        div {
                            style: shell_notice_style(&theme),
                            "{test_status()}"
                        }
                    }

                    if let Some(report) = test_report.read().as_ref() {
                        div {
                            style: "margin-top:14px; display:flex; flex-direction:column; gap:12px;",
                            if all_tests_passed(&report.checks, &report.ws_probe) {
                                div {
                                    style: format!(
                                        "padding:14px 16px; border-radius:14px; border:1px solid {}; background:{}; color:{};",
                                        theme.border, theme.info_background, theme.success_text
                                    ),
                                    div { style: "font-weight:700; margin-bottom:4px;", "All Connection Tests Passed" }
                                    div { style: "font-size:13px;", "Ground Station HTTP routes and WebSocket handshake are reachable." }
                                }
                            } else {
                                div {
                                    style: format!(
                                        "padding:14px 16px; border-radius:14px; border:1px solid {}; background:{}; color:{};",
                                        theme.warning_border, theme.warning_background, theme.warning_text
                                    ),
                                    div { style: "font-weight:700; margin-bottom:4px;", "Connection Test Found Issues" }
                                    div { style: "font-size:13px;", "Review the endpoint list below to see which routes failed or responded unexpectedly." }
                                }
                            }

                            div {
                                style: format!(
                                    "padding:12px; border-radius:12px; border:1px solid {}; background:{}; color:{}; display:grid; grid-template-columns:repeat(auto-fit, minmax(180px, 1fr)); gap:8px 12px;",
                                    theme.border, theme.app_background, theme.text_secondary
                                ),
                                div { style: "font-size:12px;", "Base" }
                                div { style: "font-size:13px; color:{theme.text_primary}; overflow-wrap:anywhere;", "{report.original_base}" }
                                div { style: "font-size:12px;", "Parsed Host" }
                                div { style: "font-size:13px; color:{theme.text_primary};", "{report.parsed_host}" }
                                div { style: "font-size:12px;", "Port" }
                                div { style: "font-size:13px; color:{theme.text_primary};", "{report.parsed_port}" }
                                div { style: "font-size:12px;", "Scheme" }
                                div { style: "font-size:13px; color:{theme.text_primary};", "{report.parsed_scheme}" }
                            }

                            div {
                                style: "display:flex; flex-direction:column; gap:10px;",
                                for check in &report.checks {
                                    div {
                                        key: "{check.method}:{check.path}",
                                        style: format!(
                                            "padding:12px; border-radius:12px; border:1px solid {}; background:{}; color:{};",
                                            theme.border,
                                            if check.ok { &theme.panel_background } else { &theme.panel_background_alt },
                                            theme.text_primary
                                        ),
                                        div {
                                            style: "display:flex; align-items:center; justify-content:space-between; gap:12px; flex-wrap:wrap;",
                                            div { style: "display:flex; align-items:center; gap:10px; flex-wrap:wrap;",
                                                div {
                                                    style: format!(
                                                        "min-width:22px; height:22px; border-radius:999px; display:flex; align-items:center; justify-content:center; font-size:12px; font-weight:700; background:{}; color:{};",
                                                        if check.ok { &theme.info_background } else { &theme.warning_background },
                                                        if check.ok { &theme.success_text } else { &theme.warning_text }
                                                    ),
                                                    if check.ok { "OK" } else { "X" }
                                                }
                                                code { "{check.method}" }
                                                div { style: "font-weight:600;", "{check.path}" }
                                            }
                                            div { style: "font-size:12px; color:{theme.text_muted};",
                                                "Status ",
                                                {check.status.map(|s| s.to_string()).unwrap_or_else(|| "—".to_string())}
                                            }
                                        }
                                        div { style: "margin-top:6px; font-size:13px; color:{theme.text_secondary};", "{check.note}" }
                                        div { style: "margin-top:6px; font-size:12px; color:{theme.text_muted}; overflow-wrap:anywhere;", "{check.url}" }
                                        if let Some(err) = &check.err {
                                            div { style: "margin-top:8px; font-size:12px; color:{theme.warning_text}; overflow-wrap:anywhere;", "{err}" }
                                        }
                                        if !check.body_snip.trim().is_empty() {
                                            div { style: "margin-top:8px; font-size:12px; color:{theme.text_muted}; overflow-wrap:anywhere;", "{check.body_snip.trim()}" }
                                        }
                                    }
                                }

                                div {
                                    style: format!(
                                        "padding:12px; border-radius:12px; border:1px solid {}; background:{}; color:{};",
                                        theme.border,
                                        if report.ws_probe.ok { &theme.panel_background } else { &theme.panel_background_alt },
                                        theme.text_primary
                                    ),
                                    div {
                                        style: "display:flex; align-items:center; justify-content:space-between; gap:12px; flex-wrap:wrap;",
                                        div { style: "display:flex; align-items:center; gap:10px; flex-wrap:wrap;",
                                            div {
                                                style: format!(
                                                    "min-width:22px; height:22px; border-radius:999px; display:flex; align-items:center; justify-content:center; font-size:12px; font-weight:700; background:{}; color:{};",
                                                    if report.ws_probe.ok { &theme.info_background } else { &theme.warning_background },
                                                    if report.ws_probe.ok { &theme.success_text } else { &theme.warning_text }
                                                ),
                                                if report.ws_probe.ok { "OK" } else { "X" }
                                            }
                                            code { "WS" }
                                            div { style: "font-weight:600;", "/ws handshake" }
                                        }
                                        if let Some(status) = report.ws_probe.status {
                                            div { style: "font-size:12px; color:{theme.text_muted};", "HTTP {status}" }
                                        }
                                    }
                                    div { style: "margin-top:6px; font-size:13px; color:{theme.text_secondary};", "{report.ws_probe.note}" }
                                    div { style: "margin-top:6px; font-size:12px; color:{theme.text_muted}; overflow-wrap:anywhere;", "{report.ws_probe.url}" }
                                    if let Some(err) = &report.ws_probe.err {
                                        div { style: "margin-top:8px; font-size:12px; color:{theme.warning_text}; overflow-wrap:anywhere; white-space:pre-wrap;", "{err}" }
                                    }
                                }
                            }
                        }
                    }
                }

                div { style: format!("display:flex; gap:12px; margin-top:16px; padding-top:16px; justify-content:flex-end; flex-wrap:wrap; border-top:1px solid {};", theme.border_soft),
                    button {
                        style: shell_button_alt_style(&theme),
                        disabled: testing(),
                        onclick: move |_| {
                            let u_norm = compose_base_url_for_connect(&scheme_edit(), &host_edit());
                            if u_norm.is_empty() {
                                test_status.set("Enter a URL first.".to_string());
                                test_report.set(None);
                                return;
                            }

                            let parsed = match parse_base_url(&u_norm) {
                                Ok(p) => p,
                                Err(e) => {
                                    test_status.set(e);
                                    test_report.set(None);
                                    return;
                                }
                            };

                            testing.set(true);
                            test_status.set("Testing connection (fast probes)...".to_string());
                            test_report.set(None);

                            objc_poke::poke_url(&u_norm);

                            let skip_tls_verify =
                                connect_scheme_supports_skip_tls(&scheme_edit()) && *skip_tls.read();
                            spawn(async move {
                                let (checks, ws_probe) = futures_util::join!(
                                    test_routes_host_only(&u_norm, skip_tls_verify),
                                    ws_connect_probe(&parsed, skip_tls_verify)
                                );

                                let report =
                                    build_connection_test_report(&u_norm, &parsed, checks, ws_probe);
                                testing.set(false);
                                test_status.set(String::new());
                                test_report.set(Some(report));
                            });
                        },
                        if testing() { "Testing..." } else { "Test Connection" }
                    }

                    button {
                        style: shell_button_style(&theme),
                        onclick: move |_| {
                            let u_norm = compose_base_url_for_connect(&scheme_edit(), &host_edit());
                            if u_norm.is_empty() {
                                test_status.set("Enter a URL first.".to_string());
                                return;
                            }
                            let skip_tls_verify =
                                connect_scheme_supports_skip_tls(&scheme_edit()) && *skip_tls.read();

                            objc_poke::poke_url(&u_norm);

                            UrlConfig::set_base_url_and_persist(u_norm.to_string());
                            UrlConfig::_set_skip_tls_verify_for_base(&u_norm, skip_tls_verify);
                            if UrlConfig::_stored_base_url().as_deref() != Some(u_norm.as_str()) {
                                test_status.set(
                                    "Failed to save the Ground Station URL on this device. The app stayed disconnected."
                                        .to_string(),
                                );
                                return;
                            }
                            let tested_ok = test_report
                                .read()
                                .as_ref()
                                .map(|report| all_tests_passed(&report.checks, &report.ws_probe))
                                .unwrap_or(false);
                            if tested_ok {
                                crate::telemetry_dashboard::clear_and_reconnect_after_connect();
                            } else {
                                crate::telemetry_dashboard::reconnect_and_reseed_after_auth_change();
                            }
                            let _ = persist::write_connect_shown(true);
                            let _ = nav.replace(Route::Dashboard {});
                        },
                        "Connect"
                    }
                }
            }
        }
    }
}
