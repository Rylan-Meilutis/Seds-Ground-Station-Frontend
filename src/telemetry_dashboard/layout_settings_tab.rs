use super::{builtin_theme_presets, layout::ThemeConfig, localized_copy, set_preferred_language};
use dioxus::prelude::*;
use dioxus_signals::Signal;

#[component]
pub fn SettingsPage(
    distance_units_metric: Signal<bool>,
    theme_preset: Signal<String>,
    language_code: Signal<String>,
    network_flow_animation_enabled: Signal<bool>,
    network_topology_vertical: Signal<bool>,
    state_chart_labels_vertical: Signal<bool>,
    map_prefetch_enabled: Signal<bool>,
    calibration_capture_sample_count: Signal<usize>,
    theme: ThemeConfig,
    on_clear_cache: EventHandler<()>,
    on_reset_app_data: EventHandler<()>,
    #[props(default)] title: Option<String>,
) -> Element {
    let mut maintenance_status = use_signal(String::new);
    let mut confirm_reset = use_signal(|| false);
    let language = language_code.read().clone();
    let title = title
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| localized_copy(&language, "Settings", "Ajustes", "Parametres"));
    let metric_enabled = *distance_units_metric.read();
    let selected_theme = theme_preset.read().clone();
    let selected_language = language_code.read().clone();
    let flow_animation_enabled = *network_flow_animation_enabled.read();
    let topology_vertical_enabled = *network_topology_vertical.read();
    let state_chart_labels_vertical_enabled = *state_chart_labels_vertical.read();
    let map_prefetch_enabled_value = *map_prefetch_enabled.read();
    let calibration_capture_sample_count_value = *calibration_capture_sample_count.read();

    let card_style = format!(
        "padding:16px; border-radius:14px; border:1px solid {}; background:{}; display:flex; flex-direction:column; gap:12px;",
        theme.border, theme.panel_background
    );
    let chip_selected = format!(
        "padding:8px 12px; border-radius:999px; border:1px solid {}; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont; font-size:0.9rem; font-weight:700; cursor:pointer;",
        theme.info_accent, theme.info_background, theme.text_primary
    );
    let chip_idle = format!(
        "padding:8px 12px; border-radius:999px; border:1px solid {}; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont; font-size:0.9rem; font-weight:600; cursor:pointer;",
        theme.button_border, theme.button_background, theme.button_text
    );

    let section_general = localized_copy(&language, "General", "General", "General");
    let section_appearance = localized_copy(&language, "Appearance", "Apariencia", "Apparence");
    let section_map = localized_copy(&language, "Map", "Mapa", "Carte");
    let section_network = localized_copy(&language, "Network", "Red", "Reseau");
    let section_charts = localized_copy(&language, "Charts", "Graficas", "Graphiques");
    let section_calibration =
        localized_copy(&language, "Calibration", "Calibracion", "Calibration");
    let section_storage = localized_copy(&language, "Storage", "Almacenamiento", "Stockage");
    let prefetch_title = localized_copy(
        &language,
        "Map Prefetch",
        "Precarga del mapa",
        "Prechargement carte",
    );
    let prefetch_desc = localized_copy(
        &language,
        "Warms map tiles around the user, rocket, and viewport for faster fullscreen transitions and offline recovery.",
        "Precarga mosaicos alrededor del usuario, del cohete y del viewport para transiciones rapidas y recuperacion sin conexion.",
        "Precharge les tuiles autour de l'utilisateur, de la fusee et du viewport pour des transitions rapides et la recuperation hors ligne.",
    );
    let prefetch_on = localized_copy(&language, "Enabled", "Activado", "Active");
    let prefetch_off = localized_copy(&language, "Disabled", "Desactivado", "Desactive");
    let calibration_samples_title = localized_copy(
        &language,
        "Capture Sample Count",
        "Cantidad de muestras",
        "Nombre d'echantillons",
    );
    let calibration_samples_desc = localized_copy(
        &language,
        "Used when capturing and averaging live raw samples for calibration points and sequences.",
        "Se usa al capturar y promediar muestras crudas en vivo para puntos y secuencias de calibracion.",
        "Utilise lors de la capture et de la moyenne des echantillons bruts en direct pour les points et sequences d'etalonnage.",
    );
    let language_title = localized_copy(&language, "Language", "Idioma", "Langue");
    let language_desc = localized_copy(
        &language,
        "Localizes dashboard tab labels, settings copy, and core chrome.",
        "Localiza las pestanas, los textos de ajustes y partes clave de la interfaz.",
        "Localise les onglets, les textes de configuration et les elements principaux.",
    );
    let theme_title = localized_copy(&language, "Theme Preset", "Tema", "Theme");
    let theme_desc = localized_copy(
        &language,
        "Choose between the built-in default theme, the Ground Station theme, or local overrides.",
        "Elige entre el tema predeterminado integrado, el tema de la Estacion terrestre o variantes locales.",
        "Choisissez entre le theme integre par defaut, le theme de la Station au sol ou des variantes locales.",
    );
    let units_title = localized_copy(
        &language,
        "Distance Units",
        "Unidades de distancia",
        "Unites de distance",
    );
    let units_desc = localized_copy(
        &language,
        "Controls the rocket distance label and the live guide line readout on the map.",
        "Controla la distancia al cohete y la lectura de la linea guia en el mapa.",
        "Controle la distance vers la fusee et la lecture de la ligne guide sur la carte.",
    );
    let metric_label = localized_copy(&language, "Metric", "Metrico", "Metrique");
    let imperial_label = localized_copy(&language, "Imperial", "Imperial", "Imperial");
    let metric_hint = localized_copy(
        &language,
        "Meters below 1 km, then kilometers.",
        "Metros hasta 1 km y luego kilometros.",
        "Metres jusqu'a 1 km puis kilometres.",
    );
    let imperial_hint = localized_copy(
        &language,
        "Feet below 1000 ft, then miles.",
        "Pies hasta 1000 ft y luego millas.",
        "Pieds jusqu'a 1000 ft puis miles.",
    );
    let network_anim_title = localized_copy(
        &language,
        "Flow Animations",
        "Animaciones de flujo",
        "Animations de flux",
    );
    let network_anim_desc = localized_copy(
        &language,
        "Controls animated directional lanes on the network graph.",
        "Controla los carriles animados direccionales en el grafo de red.",
        "Controle les voies directionnelles animees sur le graphe reseau.",
    );
    let flow_on_label = localized_copy(&language, "On", "Activado", "Active");
    let flow_off_label = localized_copy(&language, "Off", "Desactivado", "Desactive");
    let topology_layout_title = localized_copy(
        &language,
        "Topology Layout",
        "Diseno de topologia",
        "Disposition topologie",
    );
    let topology_layout_desc = localized_copy(
        &language,
        "Choose whether the network graph expands across columns or down rows.",
        "Elige si el grafo de red se expande en columnas o en filas.",
        "Choisissez si le graphe reseau s'etend en colonnes ou en lignes.",
    );
    let topology_columns_label = localized_copy(&language, "Columns", "Columnas", "Colonnes");
    let topology_rows_label = localized_copy(&language, "Rows", "Filas", "Lignes");
    let chart_labels_title = localized_copy(
        &language,
        "State Chart Scale Labels",
        "Etiquetas de escala del grafico de estado",
        "Etiquettes d'echelle du graphe d'etat",
    );
    let chart_labels_desc = localized_copy(
        &language,
        "Reserve a side rail for normalized labels or stack them vertically with guide lines into the Y axis.",
        "Reserva un riel lateral para las etiquetas normalizadas o apilalas verticalmente con guias hacia el eje Y.",
        "Reserve un rail lateral pour les etiquettes normalisees ou empilez-les verticalement avec des guides vers l'axe Y.",
    );
    let chart_labels_side = localized_copy(&language, "Side Rail", "Riel lateral", "Rail lateral");
    let chart_labels_vertical = localized_copy(&language, "Vertical", "Vertical", "Vertical");
    let clear_cache_title =
        localized_copy(&language, "Clear Cache", "Limpiar cache", "Vider le cache");
    let clear_cache_done_title =
        localized_copy(&language, "Cache Cleared", "Cache limpiada", "Cache vide");
    let clear_cache_desc = localized_copy(
        &language,
        "Clears frontend data caches and cached map tiles without removing login or saved preferences.",
        "Limpia los caches de datos y los mosaicos del mapa sin borrar el inicio de sesion ni las preferencias guardadas.",
        "Efface les caches de donnees et les tuiles de carte sans supprimer la session ni les preferences enregistrees.",
    );
    let reset_app_data_title = localized_copy(
        &language,
        "Reset App Data",
        "Restablecer datos",
        "Reinitialiser les donnees",
    );
    let reset_app_data_desc = localized_copy(
        &language,
        "Purges local tokens, saved settings, cached map tiles, and cached frontend data.",
        "Elimina tokens locales, ajustes guardados, mosaicos de mapa en cache y caches de datos del frontend.",
        "Supprime les jetons locaux, les reglages enregistres, les tuiles en cache et les caches de donnees du frontend.",
    );
    let danger_idle = format!(
        "padding:8px 12px; border-radius:999px; border:1px solid {}; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont; font-size:0.9rem; font-weight:700; cursor:pointer;",
        theme.warning_border, theme.warning_background, theme.warning_text
    );
    let confirm_idle = format!(
        "padding:8px 12px; border-radius:999px; border:1px solid {}; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont; font-size:0.9rem; font-weight:700; cursor:pointer;",
        theme.button_border, theme.button_background, theme.button_text
    );
    let reset_confirm_title = localized_copy(
        &language,
        "Confirm Reset",
        "Confirmar restablecimiento",
        "Confirmer la reinitialisation",
    );
    let reset_confirm_desc = localized_copy(
        &language,
        "This will clear login data, saved settings, cached map tiles, and cached frontend data.",
        "Esto borrara el inicio de sesion, los ajustes guardados, los mosaicos en cache y los caches de datos del frontend.",
        "Cela effacera la session, les reglages enregistres, les tuiles en cache et les caches de donnees du frontend.",
    );
    let cancel_label = localized_copy(&language, "Cancel", "Cancelar", "Annuler");
    let cache_cleared_label = localized_copy(
        &language,
        "Caches cleared and reload started.",
        "Cache limpiada y recarga iniciada.",
        "Caches vides et rechargement lance.",
    );
    let reset_done_label = localized_copy(
        &language,
        "App data cleared.",
        "Datos borrados.",
        "Donnees effacees.",
    );
    let confirm_action_label =
        localized_copy(&language, "Clear Everything", "Borrar todo", "Tout effacer");
    let english_label = "English".to_string();
    let spanish_label = "Español".to_string();
    let french_label = "Français".to_string();
    let backend_theme_label = localized_copy(
        &language,
        "Ground Station Theme",
        "Tema de la Estacion terrestre",
        "Theme de la Station au sol",
    );
    let theme_presets = builtin_theme_presets();

    rsx! {
        div { style: "padding:16px; overflow:visible; font-family:system-ui, -apple-system, BlinkMacSystemFont; color:{theme.text_primary};",
            h2 { style: "margin:0 0 14px 0; color:{theme.text_primary};", "{title}" }

            div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(280px, 1fr)); gap:12px;",
                div { style: "{card_style}",
                    div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{section_general}" }
                    div { style: "font-size:13px; color:{theme.text_muted};", "{language_title}" }
                    div { style: "font-size:13px; color:{theme.text_soft};", "{language_desc}" }
                    div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                        button {
                            style: if selected_language == "en" { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| {
                                let code = "en".to_string();
                                language_code.set(code.clone());
                                set_preferred_language(&code);
                            },
                            "{english_label}"
                        }
                        button {
                            style: if selected_language == "es" { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| {
                                let code = "es".to_string();
                                language_code.set(code.clone());
                                set_preferred_language(&code);
                            },
                            "{spanish_label}"
                        }
                        button {
                            style: if selected_language == "fr" { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| {
                                let code = "fr".to_string();
                                language_code.set(code.clone());
                                set_preferred_language(&code);
                            },
                            "{french_label}"
                        }
                    }
                }

                div { style: "{card_style}",
                    div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{section_appearance}" }
                    div { style: "font-size:13px; color:{theme.text_muted};", "{theme_title}" }
                    div { style: "font-size:13px; color:{theme.text_soft};", "{theme_desc}" }
                    div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                        button {
                            style: if selected_theme == "backend" { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| theme_preset.set("backend".to_string()),
                            "{backend_theme_label}"
                        }
                        for preset in theme_presets.iter() {
                            button {
                                key: "{preset.id}",
                                style: if selected_theme == preset.id.as_str() { chip_selected.clone() } else { chip_idle.clone() },
                                onclick: {
                                    let id = preset.id.clone();
                                    move |_| theme_preset.set(id.clone())
                                },
                                "{preset.label.localized(&language, &preset.id)}"
                            }
                        }
                    }
                }
            }

            div { style: "margin-top:12px; {card_style}",
                div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{section_map}" }
                div { style: "font-size:13px; color:{theme.text_muted};", "{units_title}" }
                div { style: "font-size:13px; color:{theme.text_soft};", "{units_desc}" }
                div { style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap;",
                    button {
                        style: if metric_enabled { chip_selected.clone() } else { chip_idle.clone() },
                        onclick: move |_| distance_units_metric.set(true),
                        "{metric_label}"
                    }
                    button {
                        style: if !metric_enabled { chip_selected.clone() } else { chip_idle.clone() },
                        onclick: move |_| distance_units_metric.set(false),
                        "{imperial_label}"
                    }
                    div { style: "font-size:13px; color:{theme.text_secondary};",
                        if metric_enabled { "{metric_hint}" } else { "{imperial_hint}" }
                    }
                }
                div { style: "display:flex; flex-direction:column; gap:8px; margin-top:10px;",
                    div { style: "font-size:13px; color:{theme.text_muted};", "{prefetch_title}" }
                    div { style: "font-size:13px; color:{theme.text_soft};", "{prefetch_desc}" }
                    div { style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap;",
                        button {
                            style: if map_prefetch_enabled_value { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| map_prefetch_enabled.set(true),
                            "{prefetch_on}"
                        }
                        button {
                            style: if !map_prefetch_enabled_value { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| map_prefetch_enabled.set(false),
                            "{prefetch_off}"
                        }
                    }
                }
            }

            div { style: "margin-top:12px; {card_style}",
                div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{section_network}" }
                div { style: "font-size:13px; color:{theme.text_muted};", "{network_anim_title}" }
                div { style: "font-size:13px; color:{theme.text_soft};", "{network_anim_desc}" }
                div { style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap;",
                    button {
                        style: if flow_animation_enabled { chip_selected.clone() } else { chip_idle.clone() },
                        onclick: move |_| network_flow_animation_enabled.set(true),
                        "{flow_on_label}"
                    }
                    button {
                        style: if !flow_animation_enabled { chip_selected.clone() } else { chip_idle.clone() },
                        onclick: move |_| network_flow_animation_enabled.set(false),
                        "{flow_off_label}"
                    }
                }
                div { style: "display:flex; flex-direction:column; gap:8px; margin-top:10px;",
                    div { style: "font-size:13px; color:{theme.text_muted};", "{topology_layout_title}" }
                    div { style: "font-size:13px; color:{theme.text_soft};", "{topology_layout_desc}" }
                    div { style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap;",
                        button {
                            style: if !topology_vertical_enabled { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| network_topology_vertical.set(false),
                            "{topology_columns_label}"
                        }
                        button {
                            style: if topology_vertical_enabled { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| network_topology_vertical.set(true),
                            "{topology_rows_label}"
                        }
                    }
                }
            }

            div { style: "margin-top:12px; {card_style}",
                div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{section_charts}" }
                div { style: "font-size:13px; color:{theme.text_muted};", "{chart_labels_title}" }
                div { style: "font-size:13px; color:{theme.text_soft};", "{chart_labels_desc}" }
                div { style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap;",
                    button {
                        style: if !state_chart_labels_vertical_enabled { chip_selected.clone() } else { chip_idle.clone() },
                        onclick: move |_| state_chart_labels_vertical.set(false),
                        "{chart_labels_side}"
                    }
                    button {
                        style: if state_chart_labels_vertical_enabled { chip_selected.clone() } else { chip_idle.clone() },
                        onclick: move |_| state_chart_labels_vertical.set(true),
                        "{chart_labels_vertical}"
                    }
                }
            }

            div { style: "margin-top:12px; {card_style}",
                div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{section_calibration}" }
                div { style: "font-size:13px; color:{theme.text_muted};", "{calibration_samples_title}" }
                div { style: "font-size:13px; color:{theme.text_soft};", "{calibration_samples_desc}" }
                input {
                    style: "padding:8px 10px; border-radius:10px; border:1px solid {theme.border}; background:{theme.panel_background_alt}; color:{theme.text_primary}; width:140px;",
                    r#type: "number",
                    min: "1",
                    max: "5000",
                    step: "1",
                    value: "{calibration_capture_sample_count_value}",
                    oninput: {
                        let mut calibration_capture_sample_count = calibration_capture_sample_count;
                        move |e| {
                            if let Ok(value) = e.value().trim().parse::<usize>() {
                                calibration_capture_sample_count.set(value.clamp(1, 5_000));
                            }
                        }
                    }
                }
            }

            div { style: "margin-top:12px; {card_style}",
                div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{section_storage}" }
                div { style: "display:flex; flex-direction:column; gap:12px;",
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{clear_cache_title}" }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{clear_cache_desc}" }
                        button {
                            style: chip_idle.clone(),
                            onclick: move |_| {
                                on_clear_cache.call(());
                                maintenance_status.set(cache_cleared_label.clone());
                                confirm_reset.set(false);
                            },
                            if maintenance_status.read().as_str() == cache_cleared_label.as_str() {
                                "{clear_cache_done_title}"
                            } else {
                                "{clear_cache_title}"
                            }
                        }
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{reset_app_data_title}" }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{reset_app_data_desc}" }
                        button {
                            style: danger_idle,
                            onclick: move |_| {
                                confirm_reset.set(true);
                                maintenance_status.set(String::new());
                            },
                            "{reset_app_data_title}"
                        }
                    }
                    if !maintenance_status.read().is_empty() {
                        div { style: "font-size:13px; color:{theme.info_text};", "{maintenance_status}" }
                    }
                }
            }

            if *confirm_reset.read() {
                div {
                    style: "position:fixed; inset:0; z-index:4100; display:flex; align-items:center; justify-content:center; padding:20px; background:rgba(0,0,0,0.45);",
                    onclick: move |_| confirm_reset.set(false),
                    div {
                        style: "width:min(420px, 100%); display:flex; flex-direction:column; gap:10px; padding:16px; border-radius:16px; border:1px solid {theme.warning_border}; background:{theme.panel_background}; box-shadow:0 16px 40px rgba(0,0,0,0.35);",
                        onclick: move |evt| evt.stop_propagation(),
                        div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{reset_confirm_title}" }
                        div { style: "font-size:13px; color:{theme.text_secondary};", "{reset_confirm_desc}" }
                        div { style: "display:flex; justify-content:flex-end; gap:8px; flex-wrap:wrap; margin-top:4px;",
                            button {
                                style: confirm_idle.clone(),
                                onclick: move |_| confirm_reset.set(false),
                                "{cancel_label}"
                            }
                            button {
                                style: danger_idle.clone(),
                                onclick: move |_| {
                                    on_reset_app_data.call(());
                                    maintenance_status.set(reset_done_label.clone());
                                    confirm_reset.set(false);
                                },
                                "{confirm_action_label}"
                            }
                        }
                    }
                }
            }
        }
    }
}
