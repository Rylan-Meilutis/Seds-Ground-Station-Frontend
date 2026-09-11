// Dashboard localization, settings UI, theme normalization, and header badges.

pub(crate) fn localized_copy(lang: &str, en: &str, es: &str, fr: &str) -> String {
    match lang {
        "es" => es.to_string(),
        "fr" => fr.to_string(),
        _ => en.to_string(),
    }
}

pub(crate) fn current_language() -> String {
    PREFERRED_LANGUAGE.read().clone()
}

pub(crate) fn set_preferred_language(code: &str) {
    let value = code.to_string();
    *PREFERRED_LANGUAGE.write() = value.clone();
    persist::set_string(LANGUAGE_STORAGE_KEY, &value);
}

pub(crate) fn translate_text(input: &str) -> String {
    let text = input.trim();
    if text.is_empty() {
        return input.to_string();
    }
    if let Some(value) = TRANSLATION_CATALOG.read().get(text) {
        return value.clone();
    }
    if let Some(value) = builtin_translation(&current_language(), text) {
        return value.to_string();
    }
    if let Ok(mut pending) = TRANSLATION_MISS_QUEUE.lock() {
        pending.insert(text.to_string());
    }
    input.to_string()
}

fn builtin_translation(lang: &str, text: &str) -> Option<&'static str> {
    match lang {
        "es" => builtin_translation_es(text),
        "fr" => builtin_translation_fr(text),
        _ => None,
    }
}

fn builtin_translation_es(text: &str) -> Option<&'static str> {
    Some(match text {
        "ABORT" => "ABORTAR",
        "VERSION" => "VERSIÓN",
        "CONNECT" => "CONECTAR",
        "RELOAD" => "RECARGAR",
        "SETTINGS" => "AJUSTES",
        "SIGN IN" => "INICIAR SESIÓN",
        "SIGN OUT" => "CERRAR SESIÓN",
        "Menu" => "Menú",
        "Close menu" => "Cerrar menú",
        "Actions Disabled" => "Acciones Desactivadas",
        "Actions Enabled" => "Acciones Activadas",
        "Close" | "Dismiss" => "Cerrar",
        "State" => "Estado",
        "Current Flight State" => "Estado actual de vuelo",
        "Flight state" => "Estado de vuelo",
        "Fill Test" => "Prueba de llenado",
        "Nitrogen Fill" => "Llenado de nitrógeno",
        "Nitrous Fill" => "Llenado de nitroso",
        "Fill Metrics" => "Métricas de llenado",
        "Tank Pressure" => "Presión del tanque",
        "Mass (kg)" => "Masa (kg)",
        "Fill Percent" => "Porcentaje de llenado",
        "Pressure and Loadcell" => "Presión y celda de carga",
        "Target" => "Objetivo",
        "Target mass (kg)" => "Masa objetivo (kg)",
        "Target pressure (psi)" => "Presión objetivo (psi)",
        "Roll" => "Alabeo",
        "Pitch" => "Cabeceo",
        "Yaw" => "Guiñada",
        "Fullscreen" => "Pantalla completa",
        "Exit Fullscreen" => "Salir de pantalla completa",
        "Collapse" => "Colapsar",
        "Expand" => "Expandir",
        "Center on Me" => "Centrar en mí",
        "Auto Center On" => "Autocentrado activado",
        "Auto Center Off" => "Autocentrado desactivado",
        "User Up" => "Usuario arriba",
        "North Up" => "Norte arriba",
        "Rotate Left" => "Girar a la izquierda",
        "Rotate Right" => "Girar a la derecha",
        "Enable Compass" => "Activar brújula",
        "Distance" => "Distancia",
        "Board Status" => "Estado de placas",
        "Packet Age (ms)" => "Edad del paquete (ms)",
        "Zoom Out" => "Alejar",
        "Zoom In" => "Acercar",
        "Reset" => "Restablecer",
        "Pinch or drag to navigate" => "Pellizca o arrastra para navegar",
        "Actions" => "Acciones",
        "Flight Setup" => "Configuración de vuelo",
        "Fill Targets" => "Objetivos de llenado",
        "Flight profile" => "Perfil de vuelo",
        "Apply To Flight Computer" => "Aplicar a la computadora de vuelo",
        "Save Fill Targets" => "Guardar objetivos de llenado",
        "Enable actions to edit fill targets." => {
            "Activa las acciones para editar los objetivos de llenado."
        }
        "Loading fill targets…" => "Cargando objetivos de llenado…",
        "Loading flight setup…" => "Cargando configuración de vuelo…",
        "Disable Actions is enabled. All action and flight-state buttons except Abort are disabled." => {
            "Desactivar acciones está activado. Todos los botones de acción y estado de vuelo excepto Abortar están desactivados."
        }
        "No actions are available for this user." => {
            "No hay acciones disponibles para este usuario."
        }
        "Nitrogen hold check passed. Pressure and loadcell are stable." => {
            "La verificación de retención de nitrógeno pasó. La presión y la celda de carga están estables."
        }
        "User location unavailable. Native GPS has not provided coordinates yet." => {
            "Ubicación de usuario no disponible. El GPS nativo aún no ha proporcionado coordenadas."
        }
        "Compass unavailable. Orientation permission was denied or has not initialized." => {
            "Brújula no disponible. El permiso de orientación fue denegado o aún no se inicializó."
        }
        "Topology graph is running in testing-mode simulation." => {
            "El grafo de topología está ejecutándose en simulación de modo de prueba."
        }
        "Topology graph is built from Ground Station topology and live node/link status." => {
            "El grafo de topología se construye desde la topología de Ground Station y el estado en vivo de nodos/enlaces."
        }
        "Router graph is running in testing-mode simulation." => {
            "El grafo del router está ejecutándose en simulación de modo de prueba."
        }
        "Router graph is built from the Ground Station SEDSprintf topology and live board/link status." => {
            "El grafo del router se construye desde la topología SEDSprintf de Ground Station y el estado en vivo de placas/enlaces."
        }
        _ => return None,
    })
}

fn builtin_translation_fr(text: &str) -> Option<&'static str> {
    Some(match text {
        "State" => "État",
        "Current Flight State" => "État de vol actuel",
        "Flight state" => "État de vol",
        "Fill Metrics" => "Métriques de remplissage",
        "Tank Pressure" => "Pression du réservoir",
        "Mass (kg)" => "Masse (kg)",
        "Fill Percent" => "Pourcentage de remplissage",
        "Pressure and Loadcell" => "Pression et cellule de charge",
        "Target" => "Cible",
        "Target mass (kg)" => "Masse cible (kg)",
        "Target pressure (psi)" => "Pression cible (psi)",
        "Fullscreen" => "Plein écran",
        "Exit Fullscreen" => "Quitter le plein écran",
        "Center on Me" => "Me centrer",
        "Auto Center On" => "Centrage auto activé",
        "Auto Center Off" => "Centrage auto désactivé",
        "User Up" => "Utilisateur en haut",
        "North Up" => "Nord en haut",
        "Rotate Left" => "Tourner à gauche",
        "Rotate Right" => "Tourner à droite",
        "Close" | "Dismiss" => "Fermer",
        _ => return None,
    })
}

fn drain_translation_misses(limit: usize, catalog: &HashMap<String, String>) -> Vec<String> {
    let Ok(mut pending) = TRANSLATION_MISS_QUEUE.lock() else {
        return Vec::new();
    };
    let mut batch = Vec::new();
    let keys: Vec<String> = pending.iter().cloned().collect();
    for key in keys {
        if batch.len() >= limit {
            break;
        }
        if catalog.contains_key(&key) {
            pending.remove(&key);
            continue;
        }
        pending.remove(&key);
        batch.push(key);
    }
    batch
}

fn merge_translation_map(items: HashMap<String, String>) {
    if items.is_empty() {
        return;
    }
    let mut next = TRANSLATION_CATALOG.read().clone();
    for (key, value) in items {
        if !key.trim().is_empty() && !value.trim().is_empty() {
            next.insert(key, value);
        }
    }
    *TRANSLATION_CATALOG.write() = next;
}

fn localized_theme(base: &layout::ThemeConfig, preset: &str) -> layout::ThemeConfig {
    if preset == "backend" || preset == "layout" {
        return normalize_theme_for_contrast(base);
    }
    builtin_theme_presets()
        .iter()
        .find(|definition| definition.id == preset)
        .map(|definition| normalize_theme_for_contrast(&definition.theme))
        .unwrap_or_else(|| normalize_theme_for_contrast(&layout::ThemeConfig::default()))
}

fn data_display_filter_kind_name(filter: Option<&layout::DataDisplayFilter>) -> String {
    match filter.map(|filter| (&filter.kind, filter.enabled)) {
        Some((layout::DataDisplayFilterKind::TimeAverage, true)) => "time_average",
        Some((layout::DataDisplayFilterKind::LowPass, true)) => "low_pass",
        Some((layout::DataDisplayFilterKind::HighPass, true)) => "high_pass",
        Some((layout::DataDisplayFilterKind::ExponentialAverage, true)) => "exponential_average",
        Some((layout::DataDisplayFilterKind::Median, true)) => "median",
        Some((layout::DataDisplayFilterKind::MinMax, true)) => "min_max",
        Some((layout::DataDisplayFilterKind::Deadband, true)) => "deadband",
        Some((layout::DataDisplayFilterKind::RateLimit, true)) => "rate_limit",
        _ => "raw",
    }
    .to_string()
}

fn data_filter_settings_rows(layout: Option<&LayoutConfig>) -> Vec<DataFilterSettingsRow> {
    let Some(layout) = layout else {
        return Vec::new();
    };
    let default_filter = layout.data_tab.default_display_filter.as_ref();
    layout
        .data_tab
        .tabs
        .iter()
        .map(|tab| {
            let filter = tab.display_filter.as_ref().or(default_filter);
            DataFilterSettingsRow {
                id: tab.id.clone(),
                label: tab.label.clone(),
                backend_default: data_display_filter_kind_name(filter),
                backend_window_ms: filter.and_then(|filter| filter.window_ms),
                backend_cutoff_hz: filter.and_then(|filter| filter.cutoff_hz),
                backend_alpha: filter.and_then(|filter| filter.alpha),
                backend_deadband: filter.and_then(|filter| filter.deadband),
                backend_max_rate_per_sec: filter.and_then(|filter| filter.max_rate_per_sec),
            }
        })
        .collect()
}

/// Returns the app-shell theme derived from the persisted preset.
///
/// Outside the live dashboard we do not have a backend-provided theme available, so
/// the "backend" preset falls back to the default theme config for shell styling.
pub fn app_shell_theme() -> layout::ThemeConfig {
    APP_THEME_CONFIG.read().clone()
}

#[cfg(not(target_arch = "wasm32"))]
#[component]
pub fn NativeSettingsPage() -> Element {
    let distance_units_metric = use_signal(|| {
        persist::get_string(MAP_DISTANCE_UNITS_STORAGE_KEY)
            .map(|v| v == "metric")
            .unwrap_or(false)
    });
    let map_header_distance_visible =
        use_signal(|| persist::get_or(MAP_HEADER_DISTANCE_VISIBLE_STORAGE_KEY, "on") != "off");
    let map_header_altitude_visible =
        use_signal(|| persist::get_or(MAP_HEADER_ALTITUDE_VISIBLE_STORAGE_KEY, "off") != "off");
    let user_location_manual =
        use_signal(|| persist::get_or(USER_LOCATION_SOURCE_STORAGE_KEY, "sensor") == "manual");
    let manual_user_lat = use_signal(|| persist::get_or(USER_MANUAL_LAT_STORAGE_KEY, ""));
    let manual_user_lon = use_signal(|| persist::get_or(USER_MANUAL_LON_STORAGE_KEY, ""));
    let user_heading_manual =
        use_signal(|| persist::get_or(USER_HEADING_SOURCE_STORAGE_KEY, "sensor") == "manual");
    let manual_user_heading = use_signal(|| persist::get_or(USER_MANUAL_HEADING_STORAGE_KEY, ""));
    let theme_preset = use_signal(|| {
        let stored = persist::get_or(THEME_PRESET_STORAGE_KEY, "default");
        if stored == "layout" {
            "backend".to_string()
        } else {
            stored
        }
    });
    let language_code = use_signal(|| persist::get_or(LANGUAGE_STORAGE_KEY, "en"));
    let clock_24h = use_signal(|| persist::get_or(CLOCK_24H_STORAGE_KEY, "off") == "on");
    let network_flow_animation_enabled =
        use_signal(|| persist::get_or(NETWORK_FLOW_ANIMATION_STORAGE_KEY, "on") != "off");
    let remote_alert_acks_enabled =
        use_signal(|| persist::get_or(REMOTE_ALERT_ACKS_ENABLED_STORAGE_KEY, "on") != "off");
    let network_topology_vertical =
        use_signal(|| persist::get_or(NETWORK_TOPOLOGY_VERTICAL_STORAGE_KEY, "off") == "on");
    let state_chart_labels_vertical =
        use_signal(|| persist::get_or(STATE_CHART_LABELS_VERTICAL_STORAGE_KEY, "off") == "on");
    let chart_interpolated_gap_ms = use_signal(|| {
        let stored = persist::get_or(CHART_INTERPOLATED_GAP_MS_STORAGE_KEY, "1200000");
        let parsed = stored.parse::<u64>().ok().unwrap_or(HISTORY_MS as u64);
        if matches!(stored.trim(), "5000" | "15000") {
            HISTORY_MS as u64
        } else {
            parsed.clamp(0, HISTORY_MS as u64)
        }
    });
    let data_filter_overrides = use_signal(HashMap::<String, String>::new);
    let telemetry_retention_ms = use_signal(stored_telemetry_retention_ms);
    let telemetry_view_window_ms = use_signal(stored_telemetry_view_window_ms);
    let data_cache_enabled =
        use_signal(|| persist::get_or(DATA_CACHE_ENABLED_STORAGE_KEY, "on") != "off");
    let map_tile_cache_enabled =
        use_signal(|| persist::get_or(MAP_TILE_CACHE_ENABLED_STORAGE_KEY, "on") != "off");
    let cache_budget_mb = use_signal(stored_cache_budget_mb);
    let map_prefetch_enabled =
        use_signal(|| persist::get_or(MAP_PREFETCH_ENABLED_STORAGE_KEY, "on") != "off");
    let map_prefetch_user_radius_m =
        use_signal(|| stored_prefetch_radius_m(MAP_PREFETCH_USER_RADIUS_STORAGE_KEY));
    let map_prefetch_rocket_radius_m =
        use_signal(|| stored_prefetch_radius_m(MAP_PREFETCH_ROCKET_RADIUS_STORAGE_KEY));
    let calibration_capture_sample_count = use_signal(|| {
        persist::get_or(CALIBRATION_CAPTURE_SAMPLE_COUNT_STORAGE_KEY, "200")
            .parse::<usize>()
            .ok()
            .unwrap_or(200)
            .clamp(1, 5_000)
    });
    let streamer_mode = use_signal(|| persist::get_or(&streamer_mode_key(), "off") == "on");

    {
        let streamer_mode = streamer_mode;
        use_effect(move || {
            persist::set_string(
                &streamer_mode_key(),
                if *streamer_mode.read() { "on" } else { "off" },
            );
        });
    }

    {
        let distance_units_metric = distance_units_metric;
        use_effect(move || {
            let value = if *distance_units_metric.read() {
                "metric"
            } else {
                "imperial"
            };
            persist::set_string(MAP_DISTANCE_UNITS_STORAGE_KEY, value);
        });
    }
    {
        let map_header_distance_visible = map_header_distance_visible;
        use_effect(move || {
            persist::set_string(
                MAP_HEADER_DISTANCE_VISIBLE_STORAGE_KEY,
                if *map_header_distance_visible.read() {
                    "on"
                } else {
                    "off"
                },
            );
        });
    }
    {
        let map_header_altitude_visible = map_header_altitude_visible;
        use_effect(move || {
            persist::set_string(
                MAP_HEADER_ALTITUDE_VISIBLE_STORAGE_KEY,
                if *map_header_altitude_visible.read() {
                    "on"
                } else {
                    "off"
                },
            );
        });
    }
    {
        let user_location_manual = user_location_manual;
        use_effect(move || {
            persist::set_string(
                USER_LOCATION_SOURCE_STORAGE_KEY,
                if *user_location_manual.read() {
                    "manual"
                } else {
                    "sensor"
                },
            );
        });
    }
    {
        let manual_user_lat = manual_user_lat;
        use_effect(move || {
            persist::set_string(USER_MANUAL_LAT_STORAGE_KEY, manual_user_lat.read().as_str());
        });
    }
    {
        let manual_user_lon = manual_user_lon;
        use_effect(move || {
            persist::set_string(USER_MANUAL_LON_STORAGE_KEY, manual_user_lon.read().as_str());
        });
    }
    {
        let user_heading_manual = user_heading_manual;
        use_effect(move || {
            persist::set_string(
                USER_HEADING_SOURCE_STORAGE_KEY,
                if *user_heading_manual.read() {
                    "manual"
                } else {
                    "sensor"
                },
            );
        });
    }
    {
        let manual_user_heading = manual_user_heading;
        use_effect(move || {
            persist::set_string(
                USER_MANUAL_HEADING_STORAGE_KEY,
                manual_user_heading.read().as_str(),
            );
        });
    }
    {
        let theme_preset = theme_preset;
        use_effect(move || {
            let value = theme_preset.read().clone();
            persist::set_string(THEME_PRESET_STORAGE_KEY, &value);
        });
    }
    {
        let language_code = language_code;
        use_effect(move || {
            let value = language_code.read().clone();
            *PREFERRED_LANGUAGE.write() = value.clone();
            persist::set_string(LANGUAGE_STORAGE_KEY, &value);
        });
    }
    {
        let clock_24h = clock_24h;
        use_effect(move || {
            let enabled = *clock_24h.read();
            *PREFERRED_CLOCK_24H.write() = enabled;
            persist::set_string(CLOCK_24H_STORAGE_KEY, if enabled { "on" } else { "off" });
        });
    }
    {
        let network_flow_animation_enabled = network_flow_animation_enabled;
        use_effect(move || {
            let value = if *network_flow_animation_enabled.read() {
                "on"
            } else {
                "off"
            };
            persist::set_string(NETWORK_FLOW_ANIMATION_STORAGE_KEY, value);
        });
    }
    {
        let remote_alert_acks_enabled = remote_alert_acks_enabled;
        use_effect(move || {
            let value = if *remote_alert_acks_enabled.read() {
                "on"
            } else {
                "off"
            };
            persist::set_string(REMOTE_ALERT_ACKS_ENABLED_STORAGE_KEY, value);
        });
    }
    {
        let network_topology_vertical = network_topology_vertical;
        use_effect(move || {
            let value = if *network_topology_vertical.read() {
                "on"
            } else {
                "off"
            };
            persist::set_string(NETWORK_TOPOLOGY_VERTICAL_STORAGE_KEY, value);
        });
    }
    {
        let state_chart_labels_vertical = state_chart_labels_vertical;
        use_effect(move || {
            let value = if *state_chart_labels_vertical.read() {
                "on"
            } else {
                "off"
            };
            persist::set_string(STATE_CHART_LABELS_VERTICAL_STORAGE_KEY, value);
        });
    }
    {
        let chart_interpolated_gap_ms = chart_interpolated_gap_ms;
        use_effect(move || {
            let value = (*chart_interpolated_gap_ms.read()).clamp(0, 60_000);
            persist::set_string(CHART_INTERPOLATED_GAP_MS_STORAGE_KEY, &value.to_string());
            data_chart::set_interpolated_gap_threshold_ms(value);
        });
    }
    {
        let telemetry_retention_ms = telemetry_retention_ms;
        let mut telemetry_view_window_ms = telemetry_view_window_ms;
        use_effect(move || {
            let retention_ms = clamp_telemetry_history_ms(*telemetry_retention_ms.read());
            let view_window_ms =
                clamp_telemetry_history_ms(*telemetry_view_window_ms.read()).min(retention_ms);
            if *telemetry_view_window_ms.read() != view_window_ms {
                telemetry_view_window_ms.set(view_window_ms);
            }
            persist::set_string(
                TELEMETRY_RETENTION_MS_STORAGE_KEY,
                &retention_ms.to_string(),
            );
            persist::set_string(
                TELEMETRY_VIEW_WINDOW_MS_STORAGE_KEY,
                &view_window_ms.to_string(),
            );
            apply_telemetry_history_settings(retention_ms, view_window_ms);
        });
    }
    {
        let data_cache_enabled = data_cache_enabled;
        use_effect(move || {
            let enabled = *data_cache_enabled.read();
            persist::set_string(
                DATA_CACHE_ENABLED_STORAGE_KEY,
                if enabled { "on" } else { "off" },
            );
            if !enabled {
                persist::_remove(TELEMETRY_CACHE_STORAGE_KEY);
                LAST_TELEMETRY_CACHE_PERSIST_MS.store(0, Ordering::Relaxed);
            }
        });
    }
    {
        let map_tile_cache_enabled = map_tile_cache_enabled;
        use_effect(move || {
            let enabled = *map_tile_cache_enabled.read();
            persist::set_string(
                MAP_TILE_CACHE_ENABLED_STORAGE_KEY,
                if enabled { "on" } else { "off" },
            );
            js_eval(&format!(
                r#"
                (function() {{
                  try {{
                    window.__gs26_tile_cache_enabled = {enabled};
                    window.__gs26_tile_cache_disabled = !{enabled};
                    if (window.localStorage) {{
                      window.localStorage.setItem("gs26_tile_cache_enabled", {enabled} ? "on" : "off");
                    }}
                    const api = window.GS26 || window;
                    if (typeof api.setTileCacheEnabled === "function") {{
                      api.setTileCacheEnabled({enabled});
                    }}
                  }} catch (e) {{
                    console.warn("GS26 tile cache toggle sync failed:", e);
                  }}
                }})();
                "#
            ));
        });
    }
    {
        let cache_budget_mb = cache_budget_mb;
        use_effect(move || {
            let budget_mb = (*cache_budget_mb.read()).clamp(1, 100_000);
            let budget_bytes = cache_budget_bytes_from_mb(budget_mb);
            persist::set_string(CACHE_BUDGET_MB_STORAGE_KEY, &budget_mb.to_string());
            js_eval(&format!(
                r#"
                (function() {{
                  try {{
                    window.__gs26_cache_budget_bytes = {budget_bytes};
                    if (window.localStorage) {{
                      window.localStorage.setItem("gs_cache_budget_mb", "{budget_mb}");
                    }}
                    const api = window.GS26 || window;
                    if (typeof api.setCacheBudgetBytes === "function") {{
                      api.setCacheBudgetBytes({budget_bytes});
                    }}
                  }} catch (e) {{
                    console.warn("GS26 cache budget sync failed:", e);
                  }}
                }})();
                "#
            ));
        });
    }
    {
        let map_prefetch_enabled = map_prefetch_enabled;
        use_effect(move || {
            let enabled = *map_prefetch_enabled.read();
            persist::set_string(
                MAP_PREFETCH_ENABLED_STORAGE_KEY,
                if enabled { "on" } else { "off" },
            );
            js_eval(&format!(
                r#"
                (function() {{
                  try {{
                    window.__gs26_prefetch_enabled = {enabled};
                    if ({enabled}) {{
                      if (typeof window.scheduleHighResTilePrefetch === "function") {{
                        window.scheduleHighResTilePrefetch();
                      }}
                    }} else {{
                      window.__gs26_ground_map_cache_state = {{ key: "", state: "idle", pending: 0, completed: 0, failed: 0, lastStartedAt: 0, lastCompletedAt: 0 }};
                      window.__gs26_ground_map_cache_ready = false;
                    }}
                  }} catch (e) {{
                    console.warn("GS26 prefetch toggle sync failed:", e);
                  }}
                }})();
                "#
            ));
        });
    }
    {
        let map_prefetch_user_radius_m = map_prefetch_user_radius_m;
        let map_prefetch_rocket_radius_m = map_prefetch_rocket_radius_m;
        use_effect(move || {
            let user_radius = clamp_prefetch_radius_m(*map_prefetch_user_radius_m.read());
            let rocket_radius = clamp_prefetch_radius_m(*map_prefetch_rocket_radius_m.read());
            persist::set_string(
                MAP_PREFETCH_USER_RADIUS_STORAGE_KEY,
                &user_radius.to_string(),
            );
            persist::set_string(
                MAP_PREFETCH_ROCKET_RADIUS_STORAGE_KEY,
                &rocket_radius.to_string(),
            );
            js_eval(&format!(
                r#"
                (function() {{
                  try {{
                    window.__gs26_prefetch_user_radius_m = {user_radius};
                    window.__gs26_prefetch_rocket_radius_m = {rocket_radius};
                    if (typeof window.scheduleHighResTilePrefetch === "function") {{
                      window.scheduleHighResTilePrefetch({{ force: true }});
                    }}
                  }} catch (e) {{
                    console.warn("GS26 prefetch radius sync failed:", e);
                  }}
                }})();
                "#
            ));
        });
    }
    {
        let calibration_capture_sample_count = calibration_capture_sample_count;
        use_effect(move || {
            let count = (*calibration_capture_sample_count.read()).clamp(1, 5_000);
            persist::set_string(
                CALIBRATION_CAPTURE_SAMPLE_COUNT_STORAGE_KEY,
                &count.to_string(),
            );
        });
    }
    {
        let theme_preset = theme_preset;
        use_effect(move || {
            let theme = localized_theme(
                &layout::ThemeConfig::default(),
                theme_preset.read().as_str(),
            );
            *APP_THEME_CONFIG.write() = theme.clone();
            apply_window_theme(&theme);
        });
    }

    let theme = app_shell_theme();
    let title = localized_copy(
        &language_code.read().clone(),
        "Settings",
        "Ajustes",
        "Parametres",
    );
    let on_reset_app_data = {
        let mut distance_units_metric = distance_units_metric;
        let mut map_header_distance_visible = map_header_distance_visible;
        let mut map_header_altitude_visible = map_header_altitude_visible;
        let mut user_location_manual = user_location_manual;
        let mut manual_user_lat = manual_user_lat;
        let mut manual_user_lon = manual_user_lon;
        let mut user_heading_manual = user_heading_manual;
        let mut manual_user_heading = manual_user_heading;
        let mut theme_preset = theme_preset;
        let mut language_code = language_code;
        let mut clock_24h = clock_24h;
        let mut network_flow_animation_enabled = network_flow_animation_enabled;
        let mut remote_alert_acks_enabled = remote_alert_acks_enabled;
        let mut network_topology_vertical = network_topology_vertical;
        let mut state_chart_labels_vertical = state_chart_labels_vertical;
        let mut chart_interpolated_gap_ms = chart_interpolated_gap_ms;
        let mut telemetry_retention_ms = telemetry_retention_ms;
        let mut telemetry_view_window_ms = telemetry_view_window_ms;
        let mut data_cache_enabled = data_cache_enabled;
        let mut map_tile_cache_enabled = map_tile_cache_enabled;
        let mut cache_budget_mb = cache_budget_mb;
        let mut map_prefetch_enabled = map_prefetch_enabled;
        let mut map_prefetch_user_radius_m = map_prefetch_user_radius_m;
        let mut map_prefetch_rocket_radius_m = map_prefetch_rocket_radius_m;
        let mut calibration_capture_sample_count = calibration_capture_sample_count;
        let mut streamer_mode = streamer_mode;
        move |_| {
            reset_local_app_data();
            distance_units_metric.set(false);
            map_header_distance_visible.set(true);
            map_header_altitude_visible.set(false);
            user_location_manual.set(false);
            manual_user_lat.set(String::new());
            manual_user_lon.set(String::new());
            user_heading_manual.set(false);
            manual_user_heading.set(String::new());
            theme_preset.set("default".to_string());
            language_code.set("en".to_string());
            clock_24h.set(false);
            network_flow_animation_enabled.set(true);
            remote_alert_acks_enabled.set(true);
            network_topology_vertical.set(false);
            state_chart_labels_vertical.set(false);
            chart_interpolated_gap_ms.set(HISTORY_MS as u64);
            telemetry_retention_ms.set(DEFAULT_TELEMETRY_RETENTION_MS);
            telemetry_view_window_ms.set(DEFAULT_TELEMETRY_VIEW_WINDOW_MS);
            data_cache_enabled.set(true);
            map_tile_cache_enabled.set(true);
            cache_budget_mb.set(DEFAULT_CACHE_BUDGET_MB);
            map_prefetch_enabled.set(true);
            map_prefetch_user_radius_m.set(DEFAULT_PREFETCH_RADIUS_M);
            map_prefetch_rocket_radius_m.set(DEFAULT_PREFETCH_RADIUS_M);
            calibration_capture_sample_count.set(200);
            streamer_mode.set(false);
        }
    };

    rsx! {
        SettingsPage {
            distance_units_metric,
            map_header_distance_visible,
            map_header_altitude_visible,
            user_location_manual,
            manual_user_lat,
            manual_user_lon,
            user_heading_manual,
            manual_user_heading,
            theme_preset,
            language_code,
            clock_24h,
            network_flow_animation_enabled,
            remote_alert_acks_enabled,
            network_topology_vertical,
            state_chart_labels_vertical,
            chart_interpolated_gap_ms,
            data_filter_rows: Vec::new(),
            data_filter_overrides,
            telemetry_retention_ms,
            telemetry_view_window_ms,
            data_cache_enabled,
            map_tile_cache_enabled,
            cache_budget_mb,
            map_prefetch_enabled,
            map_prefetch_user_radius_m,
            map_prefetch_rocket_radius_m,
            calibration_capture_sample_count,
            streamer_mode,
            storage_breakdown: cache_storage_stats_rows(),
            measured_cache_bytes: cache_storage_measured_bytes(),
            theme,
            on_clear_data_cache: move |_| {
                clear_data_caches_and_reseed();
            },
            on_clear_current_data: move |_| {
                clear_current_dashboard_data_without_reseed();
            },
            on_clear_data_and_map_cache: move |_| {
                clear_data_and_map_tile_caches_and_reseed();
            },
            on_clear_all_caches: move |_| {
                clear_all_frontend_caches_and_reseed();
            },
            on_prefetch_map_tiles: move |_| {
                trigger_map_prefetch_now();
            },
            on_reset_app_data,
            title,
        }
    }
}

pub(crate) fn builtin_theme_presets() -> &'static [layout::ThemePresetDefinition] {
    &BUILTIN_THEME_CATALOG.presets
}

pub(crate) fn theme_preset_uses_backend_colors(preset: &str) -> bool {
    matches!(preset, "backend" | "layout")
}

fn parse_hex_color(value: &str) -> Option<(u8, u8, u8)> {
    let raw = value.trim().trim_start_matches('#');
    match raw.len() {
        6 => {
            let r = u8::from_str_radix(&raw[0..2], 16).ok()?;
            let g = u8::from_str_radix(&raw[2..4], 16).ok()?;
            let b = u8::from_str_radix(&raw[4..6], 16).ok()?;
            Some((r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&raw[0..2], 16).ok()?;
            let g = u8::from_str_radix(&raw[2..4], 16).ok()?;
            let b = u8::from_str_radix(&raw[4..6], 16).ok()?;
            Some((r, g, b))
        }
        _ => None,
    }
}

fn color_to_hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn mix_color((r, g, b): (u8, u8, u8), target: (u8, u8, u8), amount: f64) -> (u8, u8, u8) {
    let blend = |from: u8, to: u8| -> u8 {
        let value = from as f64 + (to as f64 - from as f64) * amount;
        value.round().clamp(0.0, 255.0) as u8
    };
    (blend(r, target.0), blend(g, target.1), blend(b, target.2))
}

fn relative_luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let channel = |value: u8| -> f64 {
        let srgb = value as f64 / 255.0;
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

fn contrast_ratio(foreground: (u8, u8, u8), background: (u8, u8, u8)) -> f64 {
    let fg = relative_luminance(foreground);
    let bg = relative_luminance(background);
    let (lighter, darker) = if fg >= bg { (fg, bg) } else { (bg, fg) };
    (lighter + 0.05) / (darker + 0.05)
}

fn ensure_text_contrast(foreground: &str, background: &str, minimum: f64) -> String {
    let Some(fg) = parse_hex_color(foreground) else {
        return foreground.to_string();
    };
    let Some(bg) = parse_hex_color(background) else {
        return foreground.to_string();
    };
    if contrast_ratio(fg, bg) >= minimum {
        return color_to_hex(fg);
    }

    let bg_luminance = relative_luminance(bg);
    let target = if bg_luminance > 0.4 {
        (0_u8, 0_u8, 0_u8)
    } else {
        (255_u8, 255_u8, 255_u8)
    };
    let mut best = fg;
    for step in 1..=20 {
        let amount = step as f64 / 20.0;
        let candidate = mix_color(fg, target, amount);
        if contrast_ratio(candidate, bg) >= minimum {
            best = candidate;
            break;
        }
        best = candidate;
    }
    color_to_hex(best)
}

fn ensure_surface_separation(color: &str, against: &str, minimum: f64) -> String {
    let Some(base) = parse_hex_color(color) else {
        return color.to_string();
    };
    let Some(other) = parse_hex_color(against) else {
        return color.to_string();
    };
    if contrast_ratio(base, other) >= minimum {
        return color_to_hex(base);
    }

    let toward_dark = (0_u8, 0_u8, 0_u8);
    let toward_light = (255_u8, 255_u8, 255_u8);
    let mut best = base;
    for step in 1..=24 {
        let amount = step as f64 / 24.0;
        let dark_candidate = mix_color(base, toward_dark, amount);
        if contrast_ratio(dark_candidate, other) >= minimum {
            return color_to_hex(dark_candidate);
        }
        let light_candidate = mix_color(base, toward_light, amount);
        if contrast_ratio(light_candidate, other) >= minimum {
            return color_to_hex(light_candidate);
        }
        best = if contrast_ratio(light_candidate, other) > contrast_ratio(dark_candidate, other) {
            light_candidate
        } else {
            dark_candidate
        };
    }
    color_to_hex(best)
}

fn normalize_theme_for_contrast(theme: &layout::ThemeConfig) -> layout::ThemeConfig {
    let mut out = theme.clone();
    out.panel_background =
        ensure_surface_separation(&out.panel_background, &out.app_background, 1.08);
    out.panel_background_alt =
        ensure_surface_separation(&out.panel_background_alt, &out.panel_background, 1.12);
    out.tab_shell_background =
        ensure_surface_separation(&out.tab_shell_background, &out.app_background, 1.08);
    out.tab_shell_border =
        ensure_surface_separation(&out.tab_shell_border, &out.tab_shell_background, 1.35);
    out.button_background =
        ensure_surface_separation(&out.button_background, &out.tab_shell_background, 1.18);
    out.button_border = ensure_surface_separation(&out.button_border, &out.button_background, 1.45);
    out.border = ensure_surface_separation(&out.border, &out.panel_background, 1.22);
    out.border_soft = ensure_surface_separation(&out.border_soft, &out.panel_background, 1.12);
    out.border_strong = ensure_surface_separation(&out.border_strong, &out.panel_background, 1.35);
    out.text_primary = ensure_text_contrast(&out.text_primary, &out.app_background, 7.0);
    out.text_secondary = ensure_text_contrast(&out.text_secondary, &out.app_background, 5.0);
    out.text_muted = ensure_text_contrast(&out.text_muted, &out.app_background, 4.5);
    out.text_soft = ensure_text_contrast(&out.text_soft, &out.app_background, 4.5);
    out.button_text = ensure_text_contrast(&out.button_text, &out.button_background, 4.5);
    out.info_text = ensure_text_contrast(&out.info_text, &out.info_background, 4.5);
    out.warning_text = ensure_text_contrast(&out.warning_text, &out.warning_background, 4.5);
    out.error_text = ensure_text_contrast(&out.error_text, &out.error_background, 4.5);
    out.notification_text =
        ensure_text_contrast(&out.notification_text, &out.notification_background, 4.5);
    out
}

pub(crate) fn apply_window_theme(theme: &layout::ThemeConfig) {
    js_eval(&format!(
        r#"
        (function() {{
          try {{
            const vars = {{
              '--gs26-app-background': {app_background:?},
              '--gs26-app-text': {text_primary:?},
              '--gs26-panel-background': {panel_background:?},
              '--gs26-panel-alt-background': {panel_background_alt:?},
              '--gs26-border': {border:?},
              '--gs26-text-muted': {text_muted:?},
              '--gs26-text-secondary': {text_secondary:?},
              '--gs26-button-background': {button_background:?},
              '--gs26-button-text': {button_text:?},
            }};
            const targets = [document.documentElement, document.body, document.getElementById('main')];
            for (const target of targets) {{
              if (!target) continue;
              for (const [key, value] of Object.entries(vars)) {{
                target.style.setProperty(key, value);
              }}
              target.style.backgroundColor = vars['--gs26-app-background'];
              target.style.color = vars['--gs26-app-text'];
            }}
          }} catch (_) {{}}
        }})();
        "#,
        app_background = theme.app_background,
        text_primary = theme.text_primary,
        panel_background = theme.panel_background,
        panel_background_alt = theme.panel_background_alt,
        border = theme.border,
        text_muted = theme.text_muted,
        text_secondary = theme.text_secondary,
        button_background = theme.button_background,
        button_text = theme.button_text,
    ));
}

#[component]
fn NetworkTimeBadge(network_time: Signal<Option<NetworkTimeSync>>, language: String) -> Element {
    let _tick_snapshot = *HEADER_CLOCK_TICK.read();
    let (label, ts) = if let Some(sync) = network_time.read().as_ref().copied() {
        (
            localized_copy(&language, "Network Time", "Hora de red", "Heure reseau"),
            format_network_time(compensated_network_time_ms(sync)),
        )
    } else {
        (
            localized_copy(
                &language,
                "System Time",
                "Hora del sistema",
                "Heure systeme",
            ),
            format_network_time(current_wallclock_ms()),
        )
    };
    rsx! {
        span { style: "display:inline-flex; align-items:baseline; flex:0 0 auto; min-width:0; line-height:1; vertical-align:baseline;",
            span { style: "color:#cbd5e1; display:inline-flex; align-items:baseline; white-space:nowrap;",
                "({label}:"
                span {
                    style: "display:inline-flex; align-items:baseline; width:16ch; padding-left:0.4ch; white-space:nowrap; font-family: ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace; font-variant-numeric:tabular-nums;",
                    span { "{ts}" }
                    span { ")" }
                }
            }
        }
    }
}

fn format_launch_clock_delta(ms: i64) -> String {
    let total_centis = (ms.max(0) + 5) / 10;
    let minutes = total_centis / 6_000;
    let seconds = (total_centis / 100) % 60;
    let centis = total_centis % 100;
    format!("{minutes:02}:{seconds:02}.{centis:02}")
}

fn launch_clock_tminus_remaining_ms(clock: &LaunchClockMsg, now_ms: i64) -> Option<i64> {
    let remaining = match (clock.anchor_timestamp_ms, clock.duration_ms) {
        // Backend semantics: TMinus anchor is when countdown started, not the T-0 time.
        // Hold duration before the backend provides an anchor, then clamp at zero until TPlus.
        (Some(anchor_ms), Some(duration_ms)) => {
            duration_ms.saturating_sub(now_ms.saturating_sub(anchor_ms))
        }
        // Backward-compatible fallback for older payloads that only sent the target T-0 time.
        (Some(anchor_ms), None) => anchor_ms.saturating_sub(now_ms),
        (None, Some(duration_ms)) => duration_ms,
        (None, None) => return None,
    };
    Some(remaining.clamp(0, i64::MAX))
}

fn launch_clock_tplus_anchor_ms(
    clock: &LaunchClockMsg,
    now_ms: i64,
    fallback_anchor_ms: Option<i64>,
) -> Option<i64> {
    if let Some(anchor_ms) = clock.anchor_timestamp_ms
        && anchor_ms <= now_ms.saturating_add(1_000)
    {
        return Some(anchor_ms);
    }
    fallback_anchor_ms
}

fn monotonic_tminus_display_ms(clock: &LaunchClockMsg, now_ms: Option<i64>) -> Option<i64> {
    let remaining = match now_ms {
        Some(now_ms) => launch_clock_tminus_remaining_ms(clock, now_ms)?,
        None => clock.duration_ms?,
    }
    .max(0);
    let remaining = if remaining <= LAUNCH_TMINUS_ZERO_SNAP_MS {
        0
    } else {
        remaining
    };

    let mut current_min = LAUNCH_TMINUS_DISPLAY_MIN_MS.load(Ordering::Relaxed);
    if current_min != i64::MAX
        && current_min <= LAUNCH_TMINUS_RESET_ZERO_LATCH_MS
        && remaining > current_min
    {
        LAUNCH_TMINUS_DISPLAY_MIN_MS.store(0, Ordering::Relaxed);
        LAUNCH_TMINUS_ZERO_LATCHED.store(true, Ordering::Relaxed);
        return Some(0);
    }
    while remaining < current_min {
        match LAUNCH_TMINUS_DISPLAY_MIN_MS.compare_exchange_weak(
            current_min,
            remaining,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                current_min = remaining;
                break;
            }
            Err(next_min) => current_min = next_min,
        }
    }

    let display = current_min.min(remaining);
    if display == 0 {
        LAUNCH_TMINUS_ZERO_LATCHED.store(true, Ordering::Relaxed);
    }
    Some(display)
}

fn reset_tminus_display_latch() {
    LAUNCH_TMINUS_DISPLAY_MIN_MS.store(i64::MAX, Ordering::Relaxed);
    LAUNCH_TMINUS_ZERO_LATCHED.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod launch_clock_tests {
    use super::{
        LaunchClockKind, LaunchClockMsg, launch_clock_tminus_remaining_ms,
        monotonic_tminus_display_ms, reset_tminus_display_latch,
    };
    use std::sync::Mutex;

    static MONOTONIC_TMINUS_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn tminus_holds_duration_until_backend_anchor_arrives() {
        let clock = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: None,
            duration_ms: Some(10_000),
        };

        assert_eq!(
            launch_clock_tminus_remaining_ms(&clock, 123_000),
            Some(10_000)
        );
    }

    #[test]
    fn tminus_counts_down_from_backend_start_anchor() {
        let clock = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(100_000),
            duration_ms: Some(10_000),
        };

        assert_eq!(
            launch_clock_tminus_remaining_ms(&clock, 100_000),
            Some(10_000)
        );
        assert_eq!(
            launch_clock_tminus_remaining_ms(&clock, 104_250),
            Some(5_750)
        );
    }

    #[test]
    fn tminus_clamps_at_zero_until_tplus_arrives() {
        let clock = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(100_000),
            duration_ms: Some(10_000),
        };

        assert_eq!(launch_clock_tminus_remaining_ms(&clock, 115_000), Some(0));
    }

    #[test]
    fn tminus_display_never_increases_after_backend_reset() {
        let _guard = MONOTONIC_TMINUS_TEST_LOCK.lock().unwrap();
        reset_tminus_display_latch();
        let original = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(100_000),
            duration_ms: Some(10_000),
        };
        let restarted = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(111_000),
            duration_ms: Some(10_000),
        };

        assert_eq!(
            monotonic_tminus_display_ms(&original, Some(110_500)),
            Some(0)
        );
        assert_eq!(
            monotonic_tminus_display_ms(&restarted, Some(111_000)),
            Some(0)
        );
        reset_tminus_display_latch();
    }

    #[test]
    fn tminus_display_snaps_final_milliseconds_to_zero() {
        let _guard = MONOTONIC_TMINUS_TEST_LOCK.lock().unwrap();
        reset_tminus_display_latch();
        let clock = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(100_000),
            duration_ms: Some(10_000),
        };

        assert_eq!(monotonic_tminus_display_ms(&clock, Some(109_989)), Some(0));
        reset_tminus_display_latch();
    }

    #[test]
    fn tminus_display_latches_zero_if_backend_restarts_after_final_tick_window() {
        let _guard = MONOTONIC_TMINUS_TEST_LOCK.lock().unwrap();
        reset_tminus_display_latch();
        let original = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(100_000),
            duration_ms: Some(10_000),
        };
        let restarted = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(110_050),
            duration_ms: Some(10_000),
        };

        assert_eq!(
            monotonic_tminus_display_ms(&original, Some(109_900)),
            Some(100)
        );
        assert_eq!(
            monotonic_tminus_display_ms(&restarted, Some(110_050)),
            Some(0)
        );
        reset_tminus_display_latch();
    }

    #[test]
    fn tminus_display_can_reset_after_reconnect_or_idle_snapshot() {
        let _guard = MONOTONIC_TMINUS_TEST_LOCK.lock().unwrap();
        reset_tminus_display_latch();
        let expired = LaunchClockMsg {
            kind: LaunchClockKind::TMinus,
            anchor_timestamp_ms: Some(100_000),
            duration_ms: Some(10_000),
        };
        let reset = LaunchClockMsg {
            kind: LaunchClockKind::Idle,
            anchor_timestamp_ms: None,
            duration_ms: Some(10_000),
        };

        assert_eq!(
            monotonic_tminus_display_ms(&expired, Some(110_500)),
            Some(0)
        );
        reset_tminus_display_latch();
        assert_eq!(
            monotonic_tminus_display_ms(&reset, Some(111_000)),
            Some(10_000)
        );
        reset_tminus_display_latch();
    }
}

#[component]
fn LaunchClockBadge(
    launch_clock: Signal<Option<LaunchClockMsg>>,
    network_time: Signal<Option<NetworkTimeSync>>,
) -> Element {
    let fallback_tplus_anchor_ms = use_signal(|| None::<i64>);
    {
        let launch_clock = launch_clock;
        let network_time = network_time;
        let mut fallback_tplus_anchor_ms = fallback_tplus_anchor_ms;
        use_effect(move || {
            let _tick_snapshot = *HEADER_CLOCK_TICK.read();
            let clock = launch_clock.read().clone();
            let now_ms = network_time
                .read()
                .as_ref()
                .copied()
                .map(compensated_network_time_ms);
            let fallback_anchor = *fallback_tplus_anchor_ms.read();

            match clock {
                Some(clock) => match clock.kind {
                    LaunchClockKind::Idle => {
                        reset_tminus_display_latch();
                        if fallback_anchor.is_some() {
                            fallback_tplus_anchor_ms.set(None);
                        }
                    }
                    LaunchClockKind::TMinus => {
                        if fallback_anchor.is_some() {
                            fallback_tplus_anchor_ms.set(None);
                        }
                        let _ = monotonic_tminus_display_ms(&clock, now_ms);
                    }
                    LaunchClockKind::TPlus => {
                        reset_tminus_display_latch();
                        let backend_anchor = now_ms
                            .and_then(|now_ms| launch_clock_tplus_anchor_ms(&clock, now_ms, None));
                        if backend_anchor.is_some() {
                            if fallback_anchor.is_some() {
                                fallback_tplus_anchor_ms.set(None);
                            }
                        } else if fallback_anchor.is_none() {
                            fallback_tplus_anchor_ms.set(now_ms);
                        }
                    }
                },
                None => {
                    if fallback_anchor.is_some() {
                        fallback_tplus_anchor_ms.set(None);
                    }
                }
            }
        });
    }
    let _tick_snapshot = *HEADER_CLOCK_TICK.read();
    let clock = launch_clock.read().clone();
    let now_ms = network_time
        .read()
        .as_ref()
        .copied()
        .map(compensated_network_time_ms);

    let label = match clock.as_ref().map(|clock| clock.kind) {
        Some(LaunchClockKind::TPlus) => "T+",
        _ => "T-",
    };
    let tminus_zero_latched = LAUNCH_TMINUS_ZERO_LATCHED.load(Ordering::Relaxed);
    let display = match (clock.as_ref(), now_ms) {
        (Some(clock), Some(now_ms)) => match clock.kind {
            LaunchClockKind::Idle if tminus_zero_latched => format_launch_clock_delta(0),
            LaunchClockKind::Idle => format_launch_clock_delta(
                clock
                    .duration_ms
                    .unwrap_or(DEFAULT_LAUNCH_COUNTDOWN_DURATION_MS),
            ),
            LaunchClockKind::TMinus => format_launch_clock_delta(
                monotonic_tminus_display_ms(clock, Some(now_ms)).unwrap_or(0),
            ),
            LaunchClockKind::TPlus => {
                let anchor_ms =
                    launch_clock_tplus_anchor_ms(clock, now_ms, *fallback_tplus_anchor_ms.read())
                        .unwrap_or(now_ms);
                format_launch_clock_delta(now_ms.saturating_sub(anchor_ms))
            }
        },
        (Some(clock), None) => match clock.kind {
            LaunchClockKind::Idle | LaunchClockKind::TMinus if tminus_zero_latched => {
                format_launch_clock_delta(0)
            }
            LaunchClockKind::TMinus => clock
                .duration_ms
                .and_then(|_| monotonic_tminus_display_ms(clock, None))
                .map(format_launch_clock_delta)
                .unwrap_or_else(|| "--:--.-".to_string()),
            LaunchClockKind::TPlus => "--:--.-".to_string(),
            LaunchClockKind::Idle => format_launch_clock_delta(
                clock
                    .duration_ms
                    .unwrap_or(DEFAULT_LAUNCH_COUNTDOWN_DURATION_MS),
            ),
        },
        (None, _) => format_launch_clock_delta(DEFAULT_LAUNCH_COUNTDOWN_DURATION_MS),
    };

    let value_color = if display.starts_with("--:--") {
        "#94a3b8"
    } else {
        match clock.as_ref().map(|clock| clock.kind) {
            Some(LaunchClockKind::TPlus) => "#38bdf8",
            _ => "#2dd4bf",
        }
    };

    rsx! {
        span { class: "gs26-launch-clock",
            span { "({label}:" }
            span {
                class: "gs26-launch-clock-value",
                style: "color:{value_color};",
                "{display}"
            }
            span { ")" }
        }
    }
}
