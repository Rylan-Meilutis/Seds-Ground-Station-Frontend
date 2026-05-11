use super::{
    MAX_TELEMETRY_HISTORY_MINUTES, MIN_TELEMETRY_HISTORY_MINUTES,
    TELEMETRY_HISTORY_PRESET_MINUTES, builtin_theme_presets, js_eval, layout::ThemeConfig,
    localized_copy, set_preferred_language,
};
use crate::debug_log;
use dioxus::prelude::*;
use dioxus_signals::Signal;

#[component]
pub fn SettingsPage(
    distance_units_metric: Signal<bool>,
    map_header_distance_visible: Signal<bool>,
    map_header_altitude_visible: Signal<bool>,
    user_location_manual: Signal<bool>,
    manual_user_lat: Signal<String>,
    manual_user_lon: Signal<String>,
    user_heading_manual: Signal<bool>,
    manual_user_heading: Signal<String>,
    theme_preset: Signal<String>,
    language_code: Signal<String>,
    clock_24h: Signal<bool>,
    network_flow_animation_enabled: Signal<bool>,
    remote_alert_acks_enabled: Signal<bool>,
    network_topology_vertical: Signal<bool>,
    state_chart_labels_vertical: Signal<bool>,
    chart_interpolated_gap_ms: Signal<u64>,
    telemetry_retention_ms: Signal<u64>,
    telemetry_view_window_ms: Signal<u64>,
    data_cache_enabled: Signal<bool>,
    map_tile_cache_enabled: Signal<bool>,
    cache_budget_mb: Signal<u32>,
    map_prefetch_enabled: Signal<bool>,
    map_prefetch_user_radius_m: Signal<u32>,
    map_prefetch_rocket_radius_m: Signal<u32>,
    calibration_capture_sample_count: Signal<usize>,
    storage_breakdown: Vec<(String, String)>,
    measured_cache_bytes: u64,
    theme: ThemeConfig,
    on_clear_data_cache: EventHandler<()>,
    on_clear_current_data: EventHandler<()>,
    on_clear_data_and_map_cache: EventHandler<()>,
    on_clear_all_caches: EventHandler<()>,
    on_prefetch_map_tiles: EventHandler<()>,
    on_reset_app_data: EventHandler<()>,
    #[props(default)] title: Option<String>,
) -> Element {
    let mut maintenance_status = use_signal(String::new);
    let mut confirm_reset = use_signal(|| false);
    let mut active_settings_tab = use_signal(|| "general".to_string());
    let telemetry_retention_custom_input = use_signal(String::new);
    let telemetry_view_window_custom_input = use_signal(String::new);
    let language = language_code.read().clone();
    let title = title
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| localized_copy(&language, "Settings", "Ajustes", "Parametres"));
    let metric_enabled = *distance_units_metric.read();
    let map_header_distance_visible_value = *map_header_distance_visible.read();
    let map_header_altitude_visible_value = *map_header_altitude_visible.read();
    let user_location_manual_value = *user_location_manual.read();
    let manual_user_lat_value = manual_user_lat.read().clone();
    let manual_user_lon_value = manual_user_lon.read().clone();
    let user_heading_manual_value = *user_heading_manual.read();
    let manual_user_heading_value = manual_user_heading.read().clone();
    let selected_theme = theme_preset.read().clone();
    let selected_language = language_code.read().clone();
    let clock_24h_enabled = *clock_24h.read();
    let flow_animation_enabled = *network_flow_animation_enabled.read();
    let remote_alert_acks_enabled_value = *remote_alert_acks_enabled.read();
    let topology_vertical_enabled = *network_topology_vertical.read();
    let state_chart_labels_vertical_enabled = *state_chart_labels_vertical.read();
    let chart_interpolated_gap_ms_value = (*chart_interpolated_gap_ms.read()).clamp(0, 60_000);
    let telemetry_retention_ms_value = *telemetry_retention_ms.read();
    let telemetry_view_window_ms_value = (*telemetry_view_window_ms.read()).min(telemetry_retention_ms_value);
    let telemetry_retention_minutes = telemetry_retention_ms_value / 60_000;
    let telemetry_view_window_minutes = telemetry_view_window_ms_value / 60_000;
    let telemetry_retention_custom_input_value = telemetry_retention_custom_input.read().clone();
    let telemetry_view_window_custom_input_value = telemetry_view_window_custom_input.read().clone();
    let data_cache_enabled_value = *data_cache_enabled.read();
    let map_tile_cache_enabled_value = *map_tile_cache_enabled.read();
    let cache_budget_mb_value = (*cache_budget_mb.read()).clamp(1, 100_000);
    let cache_budget_bytes = (cache_budget_mb_value as u64).saturating_mul(1024 * 1024);
    let map_prefetch_enabled_value = *map_prefetch_enabled.read();
    let map_prefetch_user_radius_m_value = *map_prefetch_user_radius_m.read();
    let map_prefetch_rocket_radius_m_value = *map_prefetch_rocket_radius_m.read();
    let calibration_capture_sample_count_value = *calibration_capture_sample_count.read();
    let radius_unit_label = if metric_enabled { "m" } else { "ft" };
    let radius_min_display = if metric_enabled { 100 } else { 328 };
    let radius_max_display = if metric_enabled { 20_000 } else { 65_617 };
    let radius_step_display = 100;
    let radius_to_display = |meters: u32| -> u32 {
        if metric_enabled {
            meters
        } else {
            ((meters as f64) * 3.280_839_895)
                .round()
                .clamp(328.0, 65_617.0) as u32
        }
    };
    let map_prefetch_user_radius_value = radius_to_display(map_prefetch_user_radius_m_value);
    let map_prefetch_rocket_radius_value = radius_to_display(map_prefetch_rocket_radius_m_value);
    let cache_budget_percent = if cache_budget_bytes > 0 {
        (measured_cache_bytes as f64 / cache_budget_bytes as f64) * 100.0
    } else {
        0.0
    };
    let cache_budget_percent_label = format!("{cache_budget_percent:.1}%");
    let log_artifacts = debug_log::list_log_artifacts();
    let cache_budget_warning = if measured_cache_bytes >= cache_budget_bytes {
        Some(localized_copy(
            &language,
            "Used cache storage is over the configured limit.",
            "El almacenamiento de cache usado supera el limite configurado.",
            "Le stockage cache utilise depasse la limite configuree.",
        ))
    } else if cache_budget_percent >= 85.0 {
        Some(localized_copy(
            &language,
            "Used cache storage is close to the configured limit.",
            "El almacenamiento de cache usado esta cerca del limite configurado.",
            "Le stockage cache utilise approche la limite configuree.",
        ))
    } else {
        None
    };

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
    let chip_disabled = format!(
        "padding:8px 12px; border-radius:999px; border:1px solid {}; background:{}; color:{}; font-family:system-ui, -apple-system, BlinkMacSystemFont; font-size:0.9rem; font-weight:600; cursor:not-allowed; opacity:0.5; filter:saturate(0.7);",
        theme.button_border, theme.button_background, theme.button_text
    );
    let settings_tab_bar_style = format!(
        "display:flex; gap:8px; flex-wrap:wrap; margin:0 0 14px 0; padding:10px; border:1px solid {}; border-radius:14px; background:{};",
        theme.border, theme.panel_background_alt
    );

    let section_general = localized_copy(&language, "General", "General", "General");
    let section_appearance = localized_copy(&language, "Appearance", "Apariencia", "Apparence");
    let section_map = localized_copy(&language, "Map", "Mapa", "Carte");
    let section_network = localized_copy(&language, "Network", "Red", "Reseau");
    let section_charts = localized_copy(&language, "Charts", "Graficas", "Graphiques");
    let section_calibration =
        localized_copy(&language, "Calibration", "Calibracion", "Calibration");
    let section_storage = localized_copy(&language, "Storage", "Almacenamiento", "Stockage");
    let section_logs = localized_copy(&language, "Logs", "Registros", "Journaux");
    let settings_tab_general = localized_copy(&language, "General", "General", "General");
    let settings_tab_map = localized_copy(&language, "Map", "Mapa", "Carte");
    let settings_tab_telemetry =
        localized_copy(&language, "Telemetry", "Telemetria", "Telemetrie");
    let settings_tab_history = localized_copy(&language, "History", "Historial", "Historique");
    let settings_tab_maintenance =
        localized_copy(&language, "Maintenance", "Mantenimiento", "Maintenance");
    let section_history = localized_copy(&language, "History", "Historial", "Historique");
    let history_retention_title = localized_copy(
        &language,
        "Keep recent data",
        "Conservar datos recientes",
        "Conserver les donnees recentes",
    );
    let history_retention_desc = localized_copy(
        &language,
        "How long the dashboard keeps recent telemetry available locally before older samples are dropped.",
        "Cuanto tiempo el panel conserva la telemetria reciente localmente antes de descartar las muestras antiguas.",
        "Combien de temps le tableau de bord conserve la telemetrie recente localement avant de supprimer les anciens echantillons.",
    );
    let history_view_title = localized_copy(
        &language,
        "Show charts for",
        "Mostrar graficas durante",
        "Afficher les graphiques sur",
    );
    let history_view_desc = localized_copy(
        &language,
        "How much recent telemetry the charts show at once. This cannot be longer than the kept data duration.",
        "Cuanta telemetria reciente muestran las graficas a la vez. No puede ser mayor que la duracion de datos conservados.",
        "Combien de telemetrie recente les graphiques affichent a la fois. Cela ne peut pas depasser la duree de conservation.",
    );
    let history_custom_minutes_title = localized_copy(
        &language,
        "Custom minutes",
        "Minutos personalizados",
        "Minutes personnalisees",
    );
    let history_custom_minutes_hint = localized_copy(
        &language,
        "Enter a value from 5 to 60 minutes.",
        "Ingresa un valor entre 5 y 60 minutos.",
        "Saisissez une valeur entre 5 et 60 minutes.",
    );
    let history_view_custom_minutes_hint = localized_copy(
        &language,
        "Enter a value from 5 minutes up to the kept data duration.",
        "Ingresa un valor desde 5 minutos hasta la duracion conservada.",
        "Saisissez une valeur entre 5 minutes et la duree conservee.",
    );
    let minutes_suffix = localized_copy(&language, "min", "min", "min");
    let section_cache_control = localized_copy(
        &language,
        "Cache Control",
        "Control de cache",
        "Controle cache",
    );
    let storage_breakdown_title = localized_copy(
        &language,
        "Used Storage",
        "Almacenamiento usado",
        "Stockage utilise",
    );
    let prefetch_title = localized_copy(
        &language,
        "Map Tile Prefetch",
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
    let data_cache_title = localized_copy(
        &language,
        "Data Cache",
        "Cache de datos",
        "Cache de donnees",
    );
    let data_cache_desc = localized_copy(
        &language,
        "Stores recent telemetry locally for faster startup and offline recovery.",
        "Guarda telemetria reciente localmente para inicio mas rapido y recuperacion sin conexion.",
        "Stocke la telemetrie recente localement pour un demarrage plus rapide et la recuperation hors ligne.",
    );
    let tile_cache_title = localized_copy(
        &language,
        "Map Tile Cache",
        "Cache de mosaicos",
        "Cache des tuiles",
    );
    let tile_cache_desc = localized_copy(
        &language,
        "Stores fetched map tiles locally for faster reloads and offline map recovery.",
        "Guarda mosaicos del mapa localmente para recargas mas rapidas y recuperacion sin conexion.",
        "Stocke les tuiles localement pour des rechargements plus rapides et la recuperation hors ligne.",
    );
    let cache_budget_title = localized_copy(
        &language,
        "Cache Storage Limit",
        "Limite de almacenamiento de cache",
        "Limite de stockage cache",
    );
    let cache_budget_desc = localized_copy(
        &language,
        "Maximum local storage to use for app data and map tile caches.",
        "Almacenamiento local maximo para datos de la app y cache de mosaicos.",
        "Stockage local maximal pour les donnees de l'application et les tuiles.",
    );
    let cache_budget_used_label = localized_copy(&language, "Used", "Usado", "Utilise");
    let prefetch_estimate_title = localized_copy(
        &language,
        "Next Map Prefetch Estimate",
        "Estimacion de la proxima precarga",
        "Estimation du prochain prechargement",
    );
    let prefetch_estimate_waiting = localized_copy(
        &language,
        "Waiting for map context.",
        "Esperando contexto del mapa.",
        "En attente du contexte carte.",
    );
    let prefetch_estimate_user_label = localized_copy(
        &language,
        "User radius",
        "Radio del usuario",
        "Rayon utilisateur",
    );
    let prefetch_estimate_rocket_label = localized_copy(
        &language,
        "Rocket radius",
        "Radio del cohete",
        "Rayon fusee",
    );
    let prefetch_estimate_combined_label =
        localized_copy(&language, "Combined", "Combinado", "Combine");
    let prefetch_user_radius_title = localized_copy(
        &language,
        "User Prefetch Radius",
        "Radio de precarga del usuario",
        "Rayon de prechargement utilisateur",
    );
    let prefetch_rocket_radius_title = localized_copy(
        &language,
        "Rocket Prefetch Radius",
        "Radio de precarga del cohete",
        "Rayon de prechargement fusee",
    );
    let prefetch_radius_desc = localized_copy(
        &language,
        "Distance of map tiles to grab around each location.",
        "Distancia de mosaicos a capturar alrededor de cada ubicacion.",
        "Distance de tuiles a recuperer autour de chaque position.",
    );
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
    let time_format_title = localized_copy(
        &language,
        "Time Format",
        "Formato de hora",
        "Format d'heure",
    );
    let time_format_desc = localized_copy(
        &language,
        "Shows network epoch time in your device's local timezone using either 12-hour or 24-hour clock formatting.",
        "Muestra la hora de red en la zona horaria local del dispositivo usando formato de 12 o 24 horas.",
        "Affiche l'heure reseau dans le fuseau local de l'appareil avec un format 12 heures ou 24 heures.",
    );
    let time_format_12h = localized_copy(&language, "12 Hour", "12 horas", "12 heures");
    let time_format_24h = localized_copy(&language, "24 Hour", "24 horas", "24 heures");
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
    let map_header_distance_title = localized_copy(
        &language,
        "Map Distance",
        "Distancia del mapa",
        "Distance de la carte",
    );
    let map_header_distance_desc = localized_copy(
        &language,
        "Shows or hides the live distance in the map header.",
        "Muestra u oculta la distancia en vivo en el encabezado del mapa.",
        "Affiche ou masque la distance en direct dans l'en-tete de la carte.",
    );
    let map_header_altitude_title = localized_copy(
        &language,
        "Map Altitude",
        "Elevacion del mapa",
        "Altitude de la carte",
    );
    let map_header_altitude_desc = localized_copy(
        &language,
        "Shows or hides rocket and user elevation in the map header.",
        "Muestra u oculta la elevacion del cohete y del usuario en el encabezado del mapa.",
        "Affiche ou masque l'altitude de la fusee et de l'utilisateur dans l'en-tete de la carte.",
    );
    let map_header_on = localized_copy(&language, "Show", "Mostrar", "Afficher");
    let map_header_off = localized_copy(&language, "Hide", "Ocultar", "Masquer");
    let user_location_title = localized_copy(
        &language,
        "User Location Source",
        "Origen de ubicacion",
        "Source de position",
    );
    let user_location_desc = localized_copy(
        &language,
        "Use live sensor location when available, or disable tracking and enter coordinates manually.",
        "Usa la ubicacion del sensor cuando este disponible, o desactiva el seguimiento e ingresa coordenadas manualmente.",
        "Utilise la position du capteur si disponible, ou desactivez le suivi et saisissez les coordonnees manuellement.",
    );
    let user_location_sensor_label = localized_copy(&language, "Sensor", "Sensor", "Capteur");
    let user_location_manual_label = localized_copy(&language, "Manual", "Manual", "Manuel");
    let manual_lat_title = localized_copy(
        &language,
        "Manual Latitude",
        "Latitud manual",
        "Latitude manuelle",
    );
    let manual_lon_title = localized_copy(
        &language,
        "Manual Longitude",
        "Longitud manual",
        "Longitude manuelle",
    );
    let manual_location_desc = localized_copy(
        &language,
        "Used when manual location mode is selected. Latitude must be between -90 and 90, longitude between -180 and 180.",
        "Se usa cuando el modo manual esta seleccionado. La latitud debe estar entre -90 y 90 y la longitud entre -180 y 180.",
        "Utilise lorsque le mode manuel est selectionne. La latitude doit etre comprise entre -90 et 90 et la longitude entre -180 et 180.",
    );
    let manual_location_invalid = localized_copy(
        &language,
        "Enter a valid latitude and longitude to use manual location.",
        "Ingresa una latitud y longitud validas para usar ubicacion manual.",
        "Saisissez une latitude et une longitude valides pour utiliser la position manuelle.",
    );
    let user_heading_title = localized_copy(
        &language,
        "User Heading Source",
        "Origen de orientacion",
        "Source de cap",
    );
    let user_heading_desc = localized_copy(
        &language,
        "Use live orientation sensors when available, or disable heading tracking and enter a manual heading.",
        "Usa sensores de orientacion cuando esten disponibles, o desactiva el seguimiento e ingresa un rumbo manual.",
        "Utilise les capteurs d'orientation si disponibles, ou desactivez le suivi et saisissez un cap manuel.",
    );
    let user_heading_sensor_label = localized_copy(&language, "Sensor", "Sensor", "Capteur");
    let user_heading_manual_label = localized_copy(&language, "Manual", "Manual", "Manuel");
    let manual_heading_title =
        localized_copy(&language, "Manual Heading", "Rumbo manual", "Cap manuel");
    let manual_heading_desc = localized_copy(
        &language,
        "Used when manual heading mode is selected. Enter degrees from 0 to 360.",
        "Se usa cuando el modo manual de rumbo esta seleccionado. Ingresa grados de 0 a 360.",
        "Utilise lorsque le mode manuel du cap est selectionne. Saisissez des degres de 0 a 360.",
    );
    let manual_heading_invalid = localized_copy(
        &language,
        "Enter a valid heading to use manual orientation.",
        "Ingresa un rumbo valido para usar orientacion manual.",
        "Saisissez un cap valide pour utiliser l'orientation manuelle.",
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
    let remote_alert_ack_title = localized_copy(
        &language,
        "Remote Alert Acknowledgements",
        "Confirmaciones remotas de alertas",
        "Accuses d'alerte distants",
    );
    let remote_alert_ack_desc = localized_copy(
        &language,
        "Applies warning and error acknowledgements broadcast by the backend or hardware panel to this client.",
        "Aplica en este cliente las confirmaciones de avisos y errores difundidas por el backend o el panel fisico.",
        "Applique sur ce client les acquittements d'alertes et d'erreurs diffuses par le backend ou le panneau materiel.",
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
    let chart_gap_title = localized_copy(
        &language,
        "Interpolation Gap Threshold",
        "Umbral de interpolacion",
        "Seuil d'interpolation",
    );
    let chart_gap_desc = localized_copy(
        &language,
        "Milliseconds a sample gap can last before the chart switches to dashed interpolated segments.",
        "Milisegundos que puede durar un hueco antes de mostrar segmentos interpolados discontinuos.",
        "Millisecondes qu'un trou peut durer avant d'afficher des segments interpoles en pointille.",
    );
    let clear_data_cache_title =
        localized_copy(&language, "Clear Cache", "Limpiar cache", "Vider le cache");
    let clear_data_map_cache_title = localized_copy(
        &language,
        "Clear Cache and Map Tiles",
        "Limpiar cache y mosaicos",
        "Vider cache et tuiles",
    );
    let clear_all_caches_title = localized_copy(
        &language,
        "Clear All Caches",
        "Limpiar todos los caches",
        "Vider tous les caches",
    );
    let clear_cache_done_title =
        localized_copy(&language, "Cache Cleared", "Cache limpiada", "Cache vide");
    let clear_current_data_title = localized_copy(
        &language,
        "Clear Current Data",
        "Borrar datos actuales",
        "Effacer les donnees actuelles",
    );
    let clear_data_cache_desc = localized_copy(
        &language,
        "Clears telemetry, chart, and runtime data caches without removing map tiles or layout cache.",
        "Limpia telemetria, graficas y caches de datos sin borrar mosaicos ni layout.",
        "Efface les caches de telemetrie, graphes et donnees sans supprimer les tuiles ni la disposition.",
    );
    let clear_current_data_desc = localized_copy(
        &language,
        "Clears the current live telemetry and removes the saved data cache without reconnecting or reseeding.",
        "Borra la telemetria en vivo actual y elimina la cache de datos guardada sin reconectar ni recargar.",
        "Efface la telemetrie en direct actuelle et supprime le cache de donnees enregistre sans reconnexion ni reseed.",
    );
    let clear_data_map_cache_desc = localized_copy(
        &language,
        "Clears data caches and cached map tiles without removing layout cache.",
        "Limpia caches de datos y mosaicos del mapa sin borrar el layout.",
        "Efface les caches de donnees et les tuiles de carte sans supprimer la disposition.",
    );
    let clear_all_caches_desc = localized_copy(
        &language,
        "Clears data caches, cached map tiles, and cached layout files.",
        "Limpia datos, mosaicos del mapa y layouts en cache.",
        "Efface les donnees, les tuiles de carte et les dispositions en cache.",
    );

    {
        let telemetry_retention_ms = telemetry_retention_ms;
        let mut telemetry_retention_custom_input = telemetry_retention_custom_input;
        use_effect(move || {
            let next_value = ((*telemetry_retention_ms.read()) / 60_000).to_string();
            if telemetry_retention_custom_input.read().as_str() != next_value.as_str() {
                telemetry_retention_custom_input.set(next_value);
            }
        });
    }
    {
        let telemetry_view_window_ms = telemetry_view_window_ms;
        let mut telemetry_view_window_custom_input = telemetry_view_window_custom_input;
        use_effect(move || {
            let next_value = ((*telemetry_view_window_ms.read()) / 60_000).to_string();
            if telemetry_view_window_custom_input.read().as_str() != next_value.as_str() {
                telemetry_view_window_custom_input.set(next_value);
            }
        });
    }
    let prefetch_now_title = localized_copy(
        &language,
        "Prefetch Map Tiles",
        "Precargar mosaicos",
        "Precharger les tuiles",
    );
    let prefetch_started_label = localized_copy(
        &language,
        "Map tile prefetch queued.",
        "Precarga de mosaicos en cola.",
        "Prechargement des tuiles lance.",
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
    let logs_cleared_label = localized_copy(
        &language,
        "Logs cleared.",
        "Registros borrados.",
        "Journaux effaces.",
    );
    let logs_opened_label = localized_copy(
        &language,
        if cfg!(target_arch = "wasm32") {
            "Log download started."
        } else if cfg!(any(target_os = "android", target_os = "ios")) {
            "Share sheet opened."
        } else {
            "Log folder opened."
        },
        if cfg!(target_arch = "wasm32") {
            "Descarga de registros iniciada."
        } else if cfg!(any(target_os = "android", target_os = "ios")) {
            "Menu para compartir abierto."
        } else {
            "Carpeta de registros abierta."
        },
        if cfg!(target_arch = "wasm32") {
            "Telechargement des journaux lance."
        } else if cfg!(any(target_os = "android", target_os = "ios")) {
            "Menu de partage ouvert."
        } else {
            "Dossier des journaux ouvert."
        },
    );
    let logs_export_title = localized_copy(
        &language,
        if cfg!(target_arch = "wasm32") {
            "Download Logs"
        } else if cfg!(any(target_os = "android", target_os = "ios")) {
            "Share Logs"
        } else {
            "View Logs"
        },
        if cfg!(target_arch = "wasm32") {
            "Descargar registros"
        } else if cfg!(any(target_os = "android", target_os = "ios")) {
            "Compartir registros"
        } else {
            "Ver registros"
        },
        if cfg!(target_arch = "wasm32") {
            "Telecharger les journaux"
        } else if cfg!(any(target_os = "android", target_os = "ios")) {
            "Partager les journaux"
        } else {
            "Voir les journaux"
        },
    );
    let logs_clear_title = localized_copy(
        &language,
        "Clear Logs",
        "Borrar registros",
        "Effacer les journaux",
    );
    let logs_clear_desc = localized_copy(
        &language,
        "Deletes the locally stored frontend debug logs without touching saved settings or caches.",
        "Elimina los registros locales de depuracion del frontend sin tocar ajustes guardados ni cache.",
        "Supprime les journaux de debogage locaux du frontend sans toucher aux reglages ni aux caches.",
    );
    let prefetch_disabled_label = localized_copy(
        &language,
        "Enable Map Tile Cache and Map Tile Prefetch to run a manual prefetch.",
        "Activa Cache de mosaicos y Precarga del mapa para ejecutar una precarga manual.",
        "Activez le cache des tuiles et le prechargement carte pour lancer un prechargement manuel.",
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
    let can_manual_prefetch = map_tile_cache_enabled_value && map_prefetch_enabled_value;
    let manual_location_valid =
        super::parse_manual_user_coords_strings(&manual_user_lat_value, &manual_user_lon_value)
            .is_some();
    let manual_heading_valid =
        super::parse_manual_heading_string(&manual_user_heading_value).is_some();

    use_effect(move || {
        js_eval(
            r#"
            (function() {
              if (window.__gs26_cache_budget_settings_timer) return;
              const humanBytes = (bytes) => {
                const units = ["B", "KiB", "MiB", "GiB"];
                let value = Math.max(0, Number(bytes) || 0);
                let unit = 0;
                while (value >= 1024 && unit + 1 < units.length) {
                  value /= 1024;
                  unit += 1;
                }
                return `${unit === 0 ? value.toFixed(0) : value.toFixed(2)} ${units[unit]}`;
              };
              const update = () => {
                const root = document.getElementById("gs26-cache-budget-summary");
                if (!root) {
                  clearInterval(window.__gs26_cache_budget_settings_timer);
                  window.__gs26_cache_budget_settings_timer = null;
                  return;
                }
                const budgetBytes = Number(root.dataset.budgetBytes || 0);
                const measuredBytes = Number(root.dataset.measuredBytes || 0);
                const estimate = window.__gs26_ground_map_prefetch_estimate || {};
                const context = window.__gs26_ground_map_prefetch_context || {};
                const prefetchEnabled = typeof window.__gs26_prefetch_enabled === "boolean"
                  ? window.__gs26_prefetch_enabled
                  : true;
                const summaryStatus = String(estimate.summaryStatus || context.summaryStatus || "");
                const summaryMessage = String(estimate.summaryMessage || context.summaryMessage || "");
                const combinedTiles = Number(estimate.combinedTiles || estimate.tiles || 0);
                const combinedBytes = Number(estimate.combinedEstimatedBytes || estimate.estimatedBytes || 0);
                const tileBytes = Number(estimate.estimatedTileBytes || 0);
                const projected = measuredBytes + combinedBytes;
                const setEstimateText = (id, tiles, bytes, waitingText) => {
                  const el = document.getElementById(id);
                  if (!el) return;
                  const next = Number(tiles) > 0
                    ? `${Number(tiles)} tiles x ${humanBytes(tileBytes)} = ${humanBytes(bytes)}`
                    : waitingText || el.dataset.waitingText || "Waiting for map context.";
                  if (el.textContent !== next) el.textContent = next;
                };
                setEstimateText("gs26-prefetch-user-estimate-text", Number(estimate.userTiles || 0), Number(estimate.userEstimatedBytes || 0), String(estimate.userMessage || context.userMessage || ""));
                setEstimateText("gs26-prefetch-rocket-estimate-text", Number(estimate.rocketTiles || 0), Number(estimate.rocketEstimatedBytes || 0), String(estimate.rocketMessage || context.rocketMessage || ""));
                setEstimateText("gs26-prefetch-combined-estimate-text", combinedTiles, combinedBytes, String(estimate.summaryMessage || context.summaryMessage || ""));
                const warningText = document.getElementById("gs26-prefetch-estimate-warning");
                if (warningText) {
                  let nextWarning = "";
                  const hasRunnablePlan = prefetchEnabled && combinedTiles > 0 && combinedBytes > 0;
                  if (hasRunnablePlan && budgetBytes > 0 && combinedBytes > budgetBytes) {
                    nextWarning = "This prefetch is larger than the configured cache limit.";
                  } else if (hasRunnablePlan && budgetBytes > 0 && projected > budgetBytes) {
                    nextWarning = `This prefetch may exceed the cache limit (${humanBytes(projected)} projected).`;
                  } else if (hasRunnablePlan && (summaryStatus === "tracking" || (estimate.userAvailable && !estimate.rocketAvailable))) {
                    nextWarning = summaryMessage;
                  }
                  if (warningText.textContent !== nextWarning) warningText.textContent = nextWarning;
                  const nextDisplay = nextWarning ? "block" : "none";
                  if (warningText.style.display !== nextDisplay) warningText.style.display = nextDisplay;
                }
              };
              window.__gs26_cache_budget_settings_timer = window.setInterval(update, 2000);
              update();
            })();
            "#,
        );
    });

    rsx! {
        div { style: "padding:16px; overflow:visible; font-family:system-ui, -apple-system, BlinkMacSystemFont; color:{theme.text_primary};",
            h2 { style: "margin:0 0 14px 0; color:{theme.text_primary};", "{title}" }
            div { style: "{settings_tab_bar_style}",
                button {
                    style: if active_settings_tab.read().as_str() == "general" { chip_selected.clone() } else { chip_idle.clone() },
                    onclick: move |_| active_settings_tab.set("general".to_string()),
                    "{settings_tab_general}"
                }
                button {
                    style: if active_settings_tab.read().as_str() == "map" { chip_selected.clone() } else { chip_idle.clone() },
                    onclick: move |_| active_settings_tab.set("map".to_string()),
                    "{settings_tab_map}"
                }
                button {
                    style: if active_settings_tab.read().as_str() == "telemetry" { chip_selected.clone() } else { chip_idle.clone() },
                    onclick: move |_| active_settings_tab.set("telemetry".to_string()),
                    "{settings_tab_telemetry}"
                }
                button {
                    style: if active_settings_tab.read().as_str() == "history" { chip_selected.clone() } else { chip_idle.clone() },
                    onclick: move |_| active_settings_tab.set("history".to_string()),
                    "{settings_tab_history}"
                }
                button {
                    style: if active_settings_tab.read().as_str() == "maintenance" { chip_selected.clone() } else { chip_idle.clone() },
                    onclick: move |_| active_settings_tab.set("maintenance".to_string()),
                    "{settings_tab_maintenance}"
                }
            }

            if active_settings_tab.read().as_str() == "general" {
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
                    div { style: "font-size:13px; color:{theme.text_muted}; margin-top:10px;", "{time_format_title}" }
                    div { style: "font-size:13px; color:{theme.text_soft};", "{time_format_desc}" }
                    div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                        button {
                            style: if !clock_24h_enabled { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| clock_24h.set(false),
                            "{time_format_12h}"
                        }
                        button {
                            style: if clock_24h_enabled { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| clock_24h.set(true),
                            "{time_format_24h}"
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
            }

            if active_settings_tab.read().as_str() == "map" {
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
                    div { style: "font-size:13px; color:{theme.text_muted};", "{map_header_distance_title}" }
                    div { style: "font-size:13px; color:{theme.text_soft};", "{map_header_distance_desc}" }
                    div { style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap;",
                        button {
                            style: if map_header_distance_visible_value { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| map_header_distance_visible.set(true),
                            "{map_header_on}"
                        }
                        button {
                            style: if !map_header_distance_visible_value { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| map_header_distance_visible.set(false),
                            "{map_header_off}"
                        }
                    }
                }
                div { style: "display:flex; flex-direction:column; gap:8px; margin-top:10px;",
                    div { style: "font-size:13px; color:{theme.text_muted};", "{map_header_altitude_title}" }
                    div { style: "font-size:13px; color:{theme.text_soft};", "{map_header_altitude_desc}" }
                    div { style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap;",
                        button {
                            style: if map_header_altitude_visible_value { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| map_header_altitude_visible.set(true),
                            "{map_header_on}"
                        }
                        button {
                            style: if !map_header_altitude_visible_value { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| map_header_altitude_visible.set(false),
                            "{map_header_off}"
                        }
                    }
                }
                div { style: "display:flex; flex-direction:column; gap:8px; margin-top:10px;",
                    div { style: "font-size:13px; color:{theme.text_muted};", "{user_location_title}" }
                    div { style: "font-size:13px; color:{theme.text_soft};", "{user_location_desc}" }
                    div { style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap;",
                        button {
                            style: if !user_location_manual_value { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| user_location_manual.set(false),
                            "{user_location_sensor_label}"
                        }
                        button {
                            style: if user_location_manual_value { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| user_location_manual.set(true),
                            "{user_location_manual_label}"
                        }
                    }
                    div { style: "font-size:12px; color:{theme.text_soft};", "{manual_location_desc}" }
                    div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(220px, 1fr)); gap:10px;" ,
                        div { style: "display:flex; flex-direction:column; gap:6px;",
                            div { style: "font-size:13px; color:{theme.text_muted};", "{manual_lat_title}" }
                            input {
                                style: "padding:8px 10px; border-radius:10px; border:1px solid {theme.border}; background:{theme.panel_background_alt}; color:{theme.text_primary}; width:100%;",
                                value: "{manual_user_lat_value}",
                                placeholder: "31.000000",
                                oninput: {
                                    let mut manual_user_lat = manual_user_lat;
                                    move |e| manual_user_lat.set(e.value())
                                }
                            }
                        }
                        div { style: "display:flex; flex-direction:column; gap:6px;",
                            div { style: "font-size:13px; color:{theme.text_muted};", "{manual_lon_title}" }
                            input {
                                style: "padding:8px 10px; border-radius:10px; border:1px solid {theme.border}; background:{theme.panel_background_alt}; color:{theme.text_primary}; width:100%;",
                                value: "{manual_user_lon_value}",
                                placeholder: "-99.000000",
                                oninput: {
                                    let mut manual_user_lon = manual_user_lon;
                                    move |e| manual_user_lon.set(e.value())
                                }
                            }
                        }
                    }
                    if user_location_manual_value && !manual_location_valid {
                        div { style: "font-size:13px; color:{theme.warning_text};", "{manual_location_invalid}" }
                    }
                }
                div { style: "display:flex; flex-direction:column; gap:8px; margin-top:10px;",
                    div { style: "font-size:13px; color:{theme.text_muted};", "{user_heading_title}" }
                    div { style: "font-size:13px; color:{theme.text_soft};", "{user_heading_desc}" }
                    div { style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap;",
                        button {
                            style: if !user_heading_manual_value { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| user_heading_manual.set(false),
                            "{user_heading_sensor_label}"
                        }
                        button {
                            style: if user_heading_manual_value { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: move |_| user_heading_manual.set(true),
                            "{user_heading_manual_label}"
                        }
                    }
                    div { style: "font-size:12px; color:{theme.text_soft};", "{manual_heading_desc}" }
                    div { style: "display:flex; flex-direction:column; gap:6px; max-width:260px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{manual_heading_title}" }
                        input {
                            style: "padding:8px 10px; border-radius:10px; border:1px solid {theme.border}; background:{theme.panel_background_alt}; color:{theme.text_primary}; width:100%;",
                            value: "{manual_user_heading_value}",
                            placeholder: "0.0",
                            oninput: {
                                let mut manual_user_heading = manual_user_heading;
                                move |e| manual_user_heading.set(e.value())
                            }
                        }
                    }
                    if user_heading_manual_value && !manual_heading_valid {
                        div { style: "font-size:13px; color:{theme.warning_text};", "{manual_heading_invalid}" }
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
                div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(220px, 1fr)); gap:10px; margin-top:10px;",
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{prefetch_user_radius_title}" }
                        div { style: "font-size:12px; color:{theme.text_soft};", "{prefetch_radius_desc} ({radius_unit_label})" }
                        div { style: "display:flex; align-items:center; gap:8px;",
                            input {
                                style: "padding:8px 10px; border-radius:10px; border:1px solid {theme.border}; background:{theme.panel_background_alt}; color:{theme.text_primary}; width:160px;",
                                r#type: "number",
                                min: "{radius_min_display}",
                                max: "{radius_max_display}",
                                step: "{radius_step_display}",
                                value: "{map_prefetch_user_radius_value}",
                                oninput: {
                                    let mut map_prefetch_user_radius_m = map_prefetch_user_radius_m;
                                    let metric_enabled = metric_enabled;
                                    move |e| {
                                        if let Ok(value) = e.value().trim().parse::<u32>() {
                                            let meters = if metric_enabled {
                                                value.clamp(100, 20_000)
                                            } else {
                                                ((value as f64) / 3.280_839_895)
                                                    .round()
                                                    .clamp(100.0, 20_000.0)
                                                    as u32
                                            };
                                            map_prefetch_user_radius_m.set(meters);
                                        }
                                    }
                                }
                            }
                            div { style: "font-size:13px; color:{theme.text_secondary}; min-width:24px;", "{radius_unit_label}" }
                        }
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{prefetch_rocket_radius_title}" }
                        div { style: "font-size:12px; color:{theme.text_soft};", "{prefetch_radius_desc} ({radius_unit_label})" }
                        div { style: "display:flex; align-items:center; gap:8px;",
                            input {
                                style: "padding:8px 10px; border-radius:10px; border:1px solid {theme.border}; background:{theme.panel_background_alt}; color:{theme.text_primary}; width:160px;",
                                r#type: "number",
                                min: "{radius_min_display}",
                                max: "{radius_max_display}",
                                step: "{radius_step_display}",
                                value: "{map_prefetch_rocket_radius_value}",
                                oninput: {
                                    let mut map_prefetch_rocket_radius_m = map_prefetch_rocket_radius_m;
                                    let metric_enabled = metric_enabled;
                                    move |e| {
                                        if let Ok(value) = e.value().trim().parse::<u32>() {
                                            let meters = if metric_enabled {
                                                value.clamp(100, 20_000)
                                            } else {
                                                ((value as f64) / 3.280_839_895)
                                                    .round()
                                                    .clamp(100.0, 20_000.0)
                                                    as u32
                                            };
                                            map_prefetch_rocket_radius_m.set(meters);
                                        }
                                    }
                                }
                            }
                            div { style: "font-size:13px; color:{theme.text_secondary}; min-width:24px;", "{radius_unit_label}" }
                        }
                    }
                }
                div {
                    style: "display:flex; flex-direction:column; gap:6px; margin-top:2px;",
                    div { style: "font-size:13px; color:{theme.text_muted};", "{prefetch_estimate_title}" }
                    div { style: "display:grid; grid-template-columns:minmax(110px, auto) minmax(0, 1fr); gap:4px 12px; align-items:center; max-width:560px;",
                        div { style: "font-size:13px; color:{theme.text_soft};", "{prefetch_estimate_user_label}" }
                        div {
                            id: "gs26-prefetch-user-estimate-text",
                            "data-waiting-text": "{prefetch_estimate_waiting}",
                            style: "font-size:13px; color:{theme.text_primary}; font-family:ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; min-width:0;",
                            "{prefetch_estimate_waiting}"
                        }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{prefetch_estimate_rocket_label}" }
                        div {
                            id: "gs26-prefetch-rocket-estimate-text",
                            "data-waiting-text": "{prefetch_estimate_waiting}",
                            style: "font-size:13px; color:{theme.text_primary}; font-family:ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; min-width:0;",
                            "{prefetch_estimate_waiting}"
                        }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{prefetch_estimate_combined_label}" }
                        div {
                            id: "gs26-prefetch-combined-estimate-text",
                            "data-waiting-text": "{prefetch_estimate_waiting}",
                            style: "font-size:13px; color:{theme.text_primary}; font-family:ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; min-width:0;",
                            "{prefetch_estimate_waiting}"
                        }
                    }
                    div {
                        id: "gs26-prefetch-estimate-warning",
                        style: "display:none; font-size:13px; color:{theme.warning_text};",
                    }
                    if !can_manual_prefetch {
                        div { style: "font-size:13px; color:{theme.text_soft};", "{prefetch_disabled_label}" }
                    }
                    div { style: "display:flex; gap:8px; flex-wrap:wrap; margin-top:8px;",
                        button {
                            disabled: !can_manual_prefetch,
                            style: if can_manual_prefetch { chip_idle.clone() } else { chip_disabled.clone() },
                            onclick: {
                                let prefetch_started_label = prefetch_started_label.clone();
                                move |_| {
                                    on_prefetch_map_tiles.call(());
                                    maintenance_status.set(prefetch_started_label.clone());
                                    confirm_reset.set(false);
                                }
                            },
                            "{prefetch_now_title}"
                        }
                    }
                }
            }
            }

            if active_settings_tab.read().as_str() == "telemetry" {
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
                div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{remote_alert_ack_title}" }
                div { style: "font-size:13px; color:{theme.text_soft};", "{remote_alert_ack_desc}" }
                div { style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap; margin-top:10px;",
                    button {
                        style: if remote_alert_acks_enabled_value { chip_selected.clone() } else { chip_idle.clone() },
                        onclick: move |_| remote_alert_acks_enabled.set(true),
                        "{flow_on_label}"
                    }
                    button {
                        style: if !remote_alert_acks_enabled_value { chip_selected.clone() } else { chip_idle.clone() },
                        onclick: move |_| remote_alert_acks_enabled.set(false),
                        "{flow_off_label}"
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
                div { style: "display:flex; flex-direction:column; gap:8px; margin-top:10px;",
                    div { style: "font-size:13px; color:{theme.text_muted};", "{chart_gap_title}" }
                    div { style: "font-size:13px; color:{theme.text_soft};", "{chart_gap_desc}" }
                    input {
                        style: "padding:8px 10px; border-radius:10px; border:1px solid {theme.border}; background:{theme.panel_background_alt}; color:{theme.text_primary}; width:160px;",
                        r#type: "number",
                        min: "0",
                        max: "60000",
                        step: "100",
                        value: "{chart_interpolated_gap_ms_value}",
                        oninput: {
                            let mut chart_interpolated_gap_ms = chart_interpolated_gap_ms;
                            move |e| {
                                if let Ok(value) = e.value().trim().parse::<u64>() {
                                    chart_interpolated_gap_ms.set(value.clamp(0, 60_000));
                                }
                            }
                        }
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
            }

            if active_settings_tab.read().as_str() == "history" {
            div { style: "margin-top:12px; {card_style}",
                div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{section_history}" }
                div { style: "font-size:13px; color:{theme.text_muted};", "{history_retention_title}" }
                div { style: "font-size:13px; color:{theme.text_soft};", "{history_retention_desc}" }
                div { style: "display:flex; gap:8px; flex-wrap:wrap; margin-top:8px;",
                    for preset_minutes in TELEMETRY_HISTORY_PRESET_MINUTES {
                        button {
                            key: "retention-{preset_minutes}",
                            style: if telemetry_retention_minutes == preset_minutes { chip_selected.clone() } else { chip_idle.clone() },
                            onclick: {
                                let mut telemetry_retention_ms = telemetry_retention_ms;
                                let mut telemetry_view_window_ms = telemetry_view_window_ms;
                                move |_| {
                                    let next_ms = preset_minutes * 60_000;
                                    telemetry_retention_ms.set(next_ms);
                                    if *telemetry_view_window_ms.read() > next_ms {
                                        telemetry_view_window_ms.set(next_ms);
                                    }
                                }
                            },
                            "{preset_minutes} min"
                        }
                    }
                }
                div { style: "display:flex; align-items:center; gap:10px; flex-wrap:wrap; margin-top:10px;",
                    div { style: "font-size:13px; color:{theme.text_muted}; min-width:120px;", "{history_custom_minutes_title}" }
                    input {
                        style: "padding:8px 10px; border-radius:10px; border:1px solid {theme.border}; background:{theme.panel_background_alt}; color:{theme.text_primary}; width:120px;",
                        r#type: "number",
                        min: "{MIN_TELEMETRY_HISTORY_MINUTES}",
                        max: "{MAX_TELEMETRY_HISTORY_MINUTES}",
                        step: "1",
                        value: "{telemetry_retention_custom_input_value}",
                        oninput: {
                            let mut telemetry_retention_custom_input = telemetry_retention_custom_input;
                            let mut telemetry_retention_ms = telemetry_retention_ms;
                            let mut telemetry_view_window_ms = telemetry_view_window_ms;
                            move |e| {
                                let raw = e.value();
                                telemetry_retention_custom_input.set(raw.clone());
                                if let Ok(value) = raw.trim().parse::<u64>() {
                                    let next_minutes = value.clamp(
                                        MIN_TELEMETRY_HISTORY_MINUTES,
                                        MAX_TELEMETRY_HISTORY_MINUTES,
                                    );
                                    let next_ms = next_minutes * 60_000;
                                    telemetry_retention_ms.set(next_ms);
                                    if *telemetry_view_window_ms.read() > next_ms {
                                        telemetry_view_window_ms.set(next_ms);
                                    }
                                }
                            }
                        }
                    }
                    div { style: "font-size:13px; color:{theme.text_soft};", "{minutes_suffix}" }
                }
                div { style: "font-size:12px; color:{theme.text_soft};", "{history_custom_minutes_hint}" }
                div { style: "font-size:13px; color:{theme.text_muted}; margin-top:12px;", "{history_view_title}" }
                div { style: "font-size:13px; color:{theme.text_soft};", "{history_view_desc}" }
                div { style: "display:flex; gap:8px; flex-wrap:wrap; margin-top:8px;",
                    for preset_minutes in TELEMETRY_HISTORY_PRESET_MINUTES {
                        button {
                            key: "view-{preset_minutes}-{telemetry_retention_minutes}",
                            style: if telemetry_view_window_minutes == preset_minutes { chip_selected.clone() } else if preset_minutes > telemetry_retention_minutes { chip_disabled.clone() } else { chip_idle.clone() },
                            disabled: preset_minutes > telemetry_retention_minutes,
                            onclick: {
                                let telemetry_retention_ms = telemetry_retention_ms;
                                let mut telemetry_view_window_ms = telemetry_view_window_ms;
                                move |_| {
                                    let retention_ms = *telemetry_retention_ms.read();
                                    telemetry_view_window_ms
                                        .set((preset_minutes * 60_000).min(retention_ms));
                                }
                            },
                            "{preset_minutes} min"
                        }
                    }
                }
                div { style: "display:flex; align-items:center; gap:10px; flex-wrap:wrap; margin-top:10px;",
                    div { style: "font-size:13px; color:{theme.text_muted}; min-width:120px;", "{history_custom_minutes_title}" }
                    input {
                        style: "padding:8px 10px; border-radius:10px; border:1px solid {theme.border}; background:{theme.panel_background_alt}; color:{theme.text_primary}; width:120px;",
                        r#type: "number",
                        min: "{MIN_TELEMETRY_HISTORY_MINUTES}",
                        max: "{telemetry_retention_minutes.max(MIN_TELEMETRY_HISTORY_MINUTES)}",
                        step: "1",
                        value: "{telemetry_view_window_custom_input_value}",
                        oninput: {
                            let mut telemetry_view_window_custom_input = telemetry_view_window_custom_input;
                            let mut telemetry_view_window_ms = telemetry_view_window_ms;
                            let telemetry_retention_ms = telemetry_retention_ms;
                            move |e| {
                                let raw = e.value();
                                telemetry_view_window_custom_input.set(raw.clone());
                                if let Ok(value) = raw.trim().parse::<u64>() {
                                    let retention_minutes = (*telemetry_retention_ms.read() / 60_000)
                                        .max(MIN_TELEMETRY_HISTORY_MINUTES);
                                    let next_minutes =
                                        value.clamp(MIN_TELEMETRY_HISTORY_MINUTES, retention_minutes);
                                    telemetry_view_window_ms.set(next_minutes * 60_000);
                                }
                            }
                        }
                    }
                    div { style: "font-size:13px; color:{theme.text_soft};", "{minutes_suffix}" }
                }
                div { style: "font-size:12px; color:{theme.text_soft};", "{history_view_custom_minutes_hint}" }
            }
            }

            if active_settings_tab.read().as_str() == "maintenance" {
            div { style: "margin-top:12px; {card_style}",
                div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{section_storage}" }
                div { style: "display:flex; flex-direction:column; gap:12px;",
                    div { style: "display:flex; flex-direction:column; gap:8px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{storage_breakdown_title}" }
                        div { style: "display:grid; grid-template-columns:minmax(0, 1fr) auto; gap:6px 14px; align-items:center; max-width:520px;",
                            for (label, value) in storage_breakdown.iter() {
                                div { style: "font-size:13px; color:{theme.text_soft}; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", "{label}" }
                                div { style: "font-size:13px; color:{theme.text_primary}; font-family:ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; text-align:right;", "{value}" }
                            }
                        }
                    }
                }
            }

            div { style: "margin-top:12px; {card_style}",
                div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{section_logs}" }
                div { style: "display:flex; flex-direction:column; gap:12px;",
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{logs_export_title}" }
                        if log_artifacts.is_empty() {
                            div { style: "font-size:13px; color:{theme.text_soft};", "No logs available." }
                        } else {
                            div { style: "display:flex; flex-direction:column; gap:8px;",
                                for artifact in log_artifacts.iter().cloned() {
                                    div { style: "display:flex; align-items:center; justify-content:space-between; gap:10px; flex-wrap:wrap; padding:10px 12px; border:1px solid {theme.border}; border-radius:12px; background:{theme.panel_background_alt};",
                                        div { style: "font-size:13px; color:{theme.text_primary}; font-family:ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;", "{artifact.label}" }
                                        button {
                                            style: chip_idle.clone(),
                                            onclick: {
                                                let artifact = artifact.clone();
                                                let logs_opened_label = logs_opened_label.clone();
                                                move |_| {
                                                    match debug_log::export_log_artifact_for_user(&artifact.id) {
                                                        Ok(()) => {
                                                            debug_log::append(&format!(
                                                                "[settings] debug log export requested artifact={}",
                                                                artifact.id
                                                            ));
                                                            maintenance_status.set(logs_opened_label.clone());
                                                            confirm_reset.set(false);
                                                        }
                                                        Err(err) => maintenance_status.set(err),
                                                    }
                                                }
                                            },
                                            "{logs_export_title}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{logs_clear_title}" }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{logs_clear_desc}" }
                        div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                            button {
                                style: chip_idle.clone(),
                                onclick: move |_| {
                                    match debug_log::clear_logs() {
                                        Ok(()) => {
                                            maintenance_status.set(logs_cleared_label.clone());
                                            confirm_reset.set(false);
                                        }
                                        Err(err) => maintenance_status.set(err),
                                    }
                                },
                                "{logs_clear_title}"
                            }
                        }
                    }
                }
            }

            div { style: "margin-top:12px; {card_style}",
                div { style: "font-size:15px; color:{theme.text_primary}; font-weight:700;", "{section_cache_control}" }
                div { style: "display:flex; flex-direction:column; gap:12px;",
                    div {
                        id: "gs26-cache-budget-summary",
                        "data-budget-bytes": "{cache_budget_bytes}",
                        "data-measured-bytes": "{measured_cache_bytes}",
                        style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{cache_budget_title}" }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{cache_budget_desc}" }
                        div { style: "display:flex; align-items:center; gap:8px; flex-wrap:wrap;",
                            input {
                                style: "padding:8px 10px; border-radius:10px; border:1px solid {theme.border}; background:{theme.panel_background_alt}; color:{theme.text_primary}; width:160px;",
                                r#type: "number",
                                min: "1",
                                max: "100000",
                                step: "50",
                                value: "{cache_budget_mb_value}",
                                oninput: {
                                    let mut cache_budget_mb = cache_budget_mb;
                                    move |e| {
                                        if let Ok(value) = e.value().trim().parse::<u32>() {
                                            cache_budget_mb.set(value.clamp(1, 100_000));
                                        }
                                    }
                                }
                            }
                            div { style: "font-size:13px; color:{theme.text_secondary};", "MB" }
                            div { style: "font-size:13px; color:{theme.text_soft};",
                                "{cache_budget_used_label}: {cache_budget_percent_label}"
                            }
                        }
                        if let Some(warning) = cache_budget_warning.as_ref() {
                            div { style: "font-size:13px; color:{theme.warning_text};", "{warning}" }
                        }
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{data_cache_title}" }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{data_cache_desc}" }
                        div { style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap;",
                            button {
                                style: if data_cache_enabled_value { chip_selected.clone() } else { chip_idle.clone() },
                                onclick: move |_| data_cache_enabled.set(true),
                                "{prefetch_on}"
                            }
                            button {
                                style: if !data_cache_enabled_value { chip_selected.clone() } else { chip_idle.clone() },
                                onclick: move |_| data_cache_enabled.set(false),
                                "{prefetch_off}"
                            }
                        }
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{tile_cache_title}" }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{tile_cache_desc}" }
                        div { style: "display:flex; align-items:center; gap:12px; flex-wrap:wrap;",
                            button {
                                style: if map_tile_cache_enabled_value { chip_selected.clone() } else { chip_idle.clone() },
                                onclick: move |_| map_tile_cache_enabled.set(true),
                                "{prefetch_on}"
                            }
                            button {
                                style: if !map_tile_cache_enabled_value { chip_selected.clone() } else { chip_idle.clone() },
                                onclick: move |_| map_tile_cache_enabled.set(false),
                                "{prefetch_off}"
                            }
                        }
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{clear_current_data_title}" }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{clear_current_data_desc}" }
                        div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                            button {
                                style: chip_idle.clone(),
                                onclick: {
                                    let cache_cleared_label = cache_cleared_label.clone();
                                    move |_| {
                                        on_clear_current_data.call(());
                                        maintenance_status.set(cache_cleared_label.clone());
                                        confirm_reset.set(false);
                                    }
                                },
                                "{clear_current_data_title}"
                            }
                        }
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{clear_data_cache_title}" }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{clear_data_cache_desc}" }
                        div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                            button {
                                style: chip_idle.clone(),
                                onclick: {
                                    let cache_cleared_label = cache_cleared_label.clone();
                                    move |_| {
                                        on_clear_data_cache.call(());
                                        maintenance_status.set(cache_cleared_label.clone());
                                        confirm_reset.set(false);
                                    }
                                },
                                if maintenance_status.read().as_str() == cache_cleared_label.as_str() {
                                    "{clear_cache_done_title}"
                                } else {
                                    "{clear_data_cache_title}"
                                }
                            }
                        }
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{clear_data_map_cache_title}" }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{clear_data_map_cache_desc}" }
                        div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                            button {
                                style: chip_idle.clone(),
                                onclick: {
                                    let cache_cleared_label = cache_cleared_label.clone();
                                    move |_| {
                                        on_clear_data_and_map_cache.call(());
                                        maintenance_status.set(cache_cleared_label.clone());
                                        confirm_reset.set(false);
                                    }
                                },
                                "{clear_data_map_cache_title}"
                            }
                        }
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{clear_all_caches_title}" }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{clear_all_caches_desc}" }
                        div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                            button {
                                style: chip_idle.clone(),
                                onclick: {
                                    let cache_cleared_label = cache_cleared_label.clone();
                                    move |_| {
                                        on_clear_all_caches.call(());
                                        maintenance_status.set(cache_cleared_label.clone());
                                        confirm_reset.set(false);
                                    }
                                },
                                "{clear_all_caches_title}"
                            }
                        }
                    }
                    div { style: "display:flex; flex-direction:column; gap:6px;",
                        div { style: "font-size:13px; color:{theme.text_muted};", "{reset_app_data_title}" }
                        div { style: "font-size:13px; color:{theme.text_soft};", "{reset_app_data_desc}" }
                        div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                            button {
                                style: danger_idle.clone(),
                                onclick: move |_| {
                                    confirm_reset.set(true);
                                    maintenance_status.set(String::new());
                                },
                                "{reset_app_data_title}"
                            }
                        }
                    }
                    if !maintenance_status.read().is_empty() {
                        div { style: "font-size:13px; color:{theme.info_text};", "{maintenance_status}" }
                    }
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
