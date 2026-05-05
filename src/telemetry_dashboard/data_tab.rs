use super::layout::{
    BooleanLabels, ChartSeriesSpec, DataChartGroup, DataChartScaleMode, DataSubtabSpec,
    DataSummaryItem, DataTabLayout, DataTabSpec, ThemeConfig, ValueFormatKind, ValueFormatter,
};
// frontend/src/telemetry_dashboard/data_tab.rs
use dioxus::prelude::*;
use dioxus_signals::{Signal, WritableExt};
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use super::data_chart::{
    CHART_GRID_BOTTOM_PAD, CHART_GRID_LEFT, CHART_GRID_RIGHT_PAD, CHART_GRID_TOP,
    CHART_X_LABEL_BOTTOM, CHART_X_LABEL_LEFT_INSET, CHART_Y_LABEL_LEFT, CHART_Y_LABEL_MAX_WIDTH,
    ChartCanvas, SeriesSwatch, charts_cache_get, charts_cache_get_channel_minmax,
    charts_cache_get_multi_series_per_series_with_grid, charts_cache_get_subset,
    charts_cache_get_subset_per_series_with_grid, sender_scoped_chart_key, series_color,
    use_chart_panel_visibility,
};
use super::{
    CHART_RENDER_EPOCH, TELEMETRY_RENDER_EPOCH, latest_telemetry_row, latest_telemetry_value,
    persist, reseed_note_banner, reseed_status_note, translate_text,
};

const _ACTIVE_SUBTAB_STORAGE_KEY_PREFIX: &str = "gs26_active_data_subtab::";
const DATA_TAB_RESPONSIVE_CSS: &str = r#"
.gs26-data-tab-shell, .gs26-data-subtab-shell { min-width: 0; width:100%; align-self:stretch; box-sizing:border-box; }
.gs26-data-tab-toggle, .gs26-data-subtab-toggle { display:none; }
.gs26-data-tab-nav, .gs26-data-subtab-nav { display:flex; gap:6px; flex-wrap:wrap; align-items:center; min-width:0; width:100%; box-sizing:border-box; }
.gs26-data-tab-nav button, .gs26-data-subtab-nav button,
.gs26-data-tab-toggle, .gs26-data-subtab-toggle {
  box-sizing:border-box;
  align-items:center;
  justify-content:center;
  text-align:center;
}
@media (max-width: 720px), (max-height: 780px) {
  .gs26-data-tab-shell, .gs26-data-subtab-shell {
    display:grid;
    grid-template-columns:1fr;
    gap:0.45rem;
    width:100%;
  }
  .gs26-data-tab-toggle, .gs26-data-subtab-toggle {
    display:inline-flex;
    align-items:center;
    justify-content:center;
    justify-self:stretch;
    width:100%;
    max-width:100%;
    padding:0.25rem 0.65rem 0.3rem 0.65rem;
    border-radius:0.65rem;
    border:1px solid var(--gs26-data-toggle-border);
    background:var(--gs26-data-toggle-background);
    color:var(--gs26-data-toggle-text);
    font:inherit;
    font-size:0.82rem;
    font-weight:800;
    line-height:1.12;
    cursor:pointer;
    white-space:normal;
    overflow-wrap:anywhere;
    word-break:break-word;
  }
  .gs26-data-tab-nav, .gs26-data-subtab-nav { display:none; width:100%; }
  .gs26-data-tab-shell[data-expanded="true"] .gs26-data-tab-nav,
  .gs26-data-subtab-shell[data-expanded="true"] .gs26-data-subtab-nav {
    display:grid;
    grid-template-columns:repeat(2, minmax(0, 1fr));
    align-items:stretch;
    justify-items:stretch;
    justify-content:stretch;
    align-self:stretch;
    width:100%;
    min-width:100%;
    box-sizing:border-box;
  }
  .gs26-data-tab-nav button, .gs26-data-subtab-nav button {
    width:100%;
    min-height:2.15rem;
  }
}
@media (max-width: 980px) {
  .gs26-data-graph-groups,
  .gs26-data-graph-groups-fullscreen {
    grid-template-columns:minmax(0, 1fr) !important;
  }
}
@media (max-width: 360px) {
  .gs26-data-tab-shell[data-expanded="true"] .gs26-data-tab-nav,
  .gs26-data-subtab-shell[data-expanded="true"] .gs26-data-subtab-nav {
    grid-template-columns:1fr;
  }
}
"#;
const DATA_CHART_NORMALIZED_GRID_LEFT: f32 = 18.0;
const DATA_CHART_NORMALIZED_GRID_RIGHT_PAD: f32 = 12.0;
const DATA_CHART_PER_SERIES_LABEL_ROW_GAP: f64 = 18.0;
const DATA_CHART_VERTICAL_SCALE_LABEL_RAIL_GAP: f64 = 6.0;

#[derive(Clone)]
struct ScaleLabelPlacement {
    series_index: usize,
    target_y: f64,
    label_y: f64,
    rail_column: usize,
    text: String,
}

#[derive(Clone)]
struct DataChartRenderPayload {
    filtered_chunks: Rc<Vec<super::data_chart::ChartRenderChunk>>,
    y_min: f32,
    y_max: f32,
    span_min: f32,
    legend_labels: Vec<String>,
    per_series_scales: Rc<Vec<Option<(f32, f32)>>>,
    scale_label_placements: Vec<ScaleLabelPlacement>,
}

thread_local! {
    static DATA_CHART_RENDER_CACHE: RefCell<HashMap<u64, DataChartRenderPayload>> = RefCell::new(HashMap::new());
}

fn subtab_storage_key(tab_id: &str) -> String {
    format!("{_ACTIVE_SUBTAB_STORAGE_KEY_PREFIX}{tab_id}")
}

fn retain_recent_chart_cache_entries<T>(cache: &mut HashMap<u64, T>) {
    if cache.len() > 256 {
        cache.clear();
    }
}

fn hash_chart_series_specs(
    hasher: &mut std::collections::hash_map::DefaultHasher,
    specs: &[ChartSeriesSpec],
) {
    for spec in specs {
        spec.data_type.hash(hasher);
        spec.index.hash(hasher);
        spec.sender_id.hash(hasher);
        spec.label.hash(hasher);
    }
}

fn chart_canvas_identity_key(
    chart_key: &str,
    group: &DataChartGroup,
    fallback_labels: &[String],
    legend_labels: &[String],
    multi_series: Option<&[ChartSeriesSpec]>,
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    chart_key.hash(&mut hasher);
    group.title.hash(&mut hasher);
    group.data_type.hash(&mut hasher);
    group.sender_id.hash(&mut hasher);
    group.labels.hash(&mut hasher);
    group.channels.hash(&mut hasher);
    group.scale_mode.hash(&mut hasher);
    fallback_labels.hash(&mut hasher);
    legend_labels.hash(&mut hasher);
    if let Some(series) = multi_series {
        hash_chart_series_specs(&mut hasher, series);
    }
    format!("data-chart::{:016x}", hasher.finish())
}

#[component]
pub fn DataTab(
    active_tab: Signal<String>,
    layout: DataTabLayout,
    #[props(default = false)] state_chart_labels_vertical: bool,
    theme: ThemeConfig,
) -> Element {
    let is_fullscreen = use_signal(|| false);
    let show_chart = use_signal(|| true);
    let active_subtabs = use_signal(HashMap::<String, String>::new);
    let tabs_expanded = use_signal(|| false);
    let subtabs_expanded = use_signal(|| false);
    let did_restore_subtabs = use_signal(|| false);
    let last_saved_subtab = use_signal(String::new);

    use_effect({
        let layout_tabs = layout.tabs.clone();
        let mut active_subtabs = active_subtabs;
        let mut did_restore_subtabs = did_restore_subtabs;
        move || {
            if *did_restore_subtabs.read() {
                return;
            }
            did_restore_subtabs.set(true);

            let mut restored = active_subtabs.read().clone();
            for tab in &layout_tabs {
                let Some(subtabs) = tab.subtabs.as_ref() else {
                    continue;
                };
                if subtabs.is_empty() {
                    continue;
                }
                if let Some(saved) = persist::get_string(&subtab_storage_key(&tab.id))
                    .filter(|saved| subtabs.iter().any(|subtab| subtab.id == *saved))
                {
                    restored.insert(tab.id.clone(), saved);
                }
            }
            active_subtabs.set(restored);
        }
    });

    // Layout-defined data types (for buttons)
    let types = layout.tabs.clone();
    let current = active_tab.read().clone();
    let current_tab = types.iter().find(|t| t.id == current);
    let current_subtabs = current_tab
        .and_then(|tab| tab.subtabs.as_ref())
        .cloned()
        .unwrap_or_default();
    let selected_subtab_id = active_subtabs
        .read()
        .get(&current)
        .cloned()
        .unwrap_or_default();
    let selected_subtab = if current_subtabs.is_empty() {
        None
    } else {
        current_subtabs
            .iter()
            .find(|subtab| subtab.id == selected_subtab_id)
            .cloned()
            .or_else(|| current_subtabs.first().cloned())
    };

    use_effect({
        let current_tab_id = current.clone();
        let current_subtabs = current_subtabs.clone();
        let mut active_subtabs = active_subtabs;
        move || {
            if current_tab_id.is_empty() || current_subtabs.is_empty() {
                return;
            }
            let current_selection = active_subtabs.read().get(&current_tab_id).cloned();
            if current_selection
                .as_ref()
                .is_some_and(|selected| current_subtabs.iter().any(|subtab| subtab.id == *selected))
            {
                return;
            }
            let persisted = persist::get_string(&subtab_storage_key(&current_tab_id));
            if let Some(saved) =
                persisted.filter(|saved| current_subtabs.iter().any(|subtab| subtab.id == *saved))
            {
                active_subtabs.write().insert(current_tab_id.clone(), saved);
                return;
            }

            if let Some(first) = current_subtabs.first() {
                active_subtabs
                    .write()
                    .insert(current_tab_id.clone(), first.id.clone());
            }
        }
    });

    use_effect({
        let current_tab_id = current.clone();
        let current_subtabs = current_subtabs.clone();
        let active_subtabs = active_subtabs;
        let mut last_saved_subtab = last_saved_subtab;
        move || {
            if current_tab_id.is_empty() || current_subtabs.is_empty() {
                return;
            }
            let subtab = active_subtabs
                .read()
                .get(&current_tab_id)
                .cloned()
                .unwrap_or_default();
            if subtab.is_empty() {
                return;
            }
            if !current_subtabs
                .iter()
                .any(|candidate| candidate.id == subtab)
            {
                return;
            }
            let cache_marker = format!("{current_tab_id}:{subtab}");
            if *last_saved_subtab.read() == cache_marker {
                return;
            }
            last_saved_subtab.set(cache_marker);
            persist::set_string(&subtab_storage_key(&current_tab_id), &subtab);
        }
    });

    let data_tabs_toggle_label = if *tabs_expanded.read() {
        "Hide data tabs".to_string()
    } else {
        let label = current_tab
            .map(|tab| translate_text(&tab.label))
            .unwrap_or_else(|| translate_text("Data tabs"));
        format!("Show data tabs ({label})")
    };
    let data_subtabs_toggle_label = if *subtabs_expanded.read() {
        "Hide subtabs".to_string()
    } else {
        let label = selected_subtab
            .as_ref()
            .map(|subtab| translate_text(&subtab.label))
            .unwrap_or_else(|| translate_text("Subtabs"));
        format!("Show subtabs ({label})")
    };
    let current_tab_snapshot = current_tab.cloned();
    let selected_subtab_snapshot = selected_subtab.clone();

    rsx! {
        style {
            "{DATA_TAB_RESPONSIVE_CSS}"
        }
        div {
            style: "padding:8px 0 8px 0; height:100%; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow-y:auto; overflow-x:hidden; -webkit-overflow-scrolling:auto; display:flex; flex-direction:column; gap:8px; --gs26-data-toggle-background:{theme.tab_shell_background}; --gs26-data-toggle-border:{theme.tab_shell_border}; --gs26-data-toggle-text:{theme.button_text};",

            div { style: "display:flex; flex-direction:column; gap:6px; width:100%; min-width:0; align-self:stretch;",

                div {
                    class: "gs26-data-tab-shell",
                    "data-expanded": if *tabs_expanded.read() { "true" } else { "false" },
                    button {
                        class: "gs26-data-tab-toggle",
                        onclick: {
                            let mut tabs_expanded = tabs_expanded;
                            move |_| {
                                let next = {
                                    let current = *tabs_expanded.read();
                                    !current
                                };
                                tabs_expanded.set(next);
                            }
                        },
                        "{data_tabs_toggle_label}"
                    }
                div { class: "gs26-data-tab-nav",
                    for t in types.iter().take(32) {
                        button {
                            style: if t.id == current {
                                {
                                    let accent = theme
                                        .main_tab_accents
                                        .get("data")
                                        .map(String::as_str)
                                        .unwrap_or("#f97316");
                                    format!(
                                        "padding:4px 8px; border-radius:999px; border:1px solid {accent}; background:{}; color:{accent}; cursor:pointer;\
                                         display:inline-flex; align-items:center; justify-content:center;\
                                         font:inherit; font-size:12px;\
                                         min-width:0; max-width:100%; text-align:center; line-height:1.2;\
                                         white-space:normal; overflow-wrap:anywhere; word-break:break-word;",
                                        theme.button_background
                                    )
                                }
                            } else {
                                format!(
                                    "padding:4px 8px; border-radius:999px; border:1px solid {}; background:{}; color:{}; cursor:pointer;\
                                     display:inline-flex; align-items:center; justify-content:center;\
                                     font:inherit; font-size:12px;\
                                     min-width:0; max-width:100%; text-align:center; line-height:1.2;\
                                     white-space:normal; overflow-wrap:anywhere; word-break:break-word;",
                                    theme.border, theme.panel_background, theme.text_primary
                                )
                            },
                            onclick: {
                                let t = t.id.clone();
                                let mut active_tab2 = active_tab;
                                let mut tabs_expanded = tabs_expanded;
                                let mut subtabs_expanded = subtabs_expanded;
                                move |_| {
                                    active_tab2.set(t.clone());
                                    tabs_expanded.set(false);
                                    subtabs_expanded.set(false);
                                }
                            },
                            "{translate_text(&t.label)}"
                        }
                    }
                }
                }

                if !current_subtabs.is_empty() {
                    div {
                        class: "gs26-data-subtab-shell",
                        "data-expanded": if *subtabs_expanded.read() { "true" } else { "false" },
                        button {
                            class: "gs26-data-subtab-toggle",
                            onclick: {
                                let mut subtabs_expanded = subtabs_expanded;
                                move |_| {
                                    let next = {
                                        let current = *subtabs_expanded.read();
                                        !current
                                    };
                                    subtabs_expanded.set(next);
                                }
                            },
                            "{data_subtabs_toggle_label}"
                        }
                    div { class: "gs26-data-subtab-nav",
                        for subtab in current_subtabs.iter() {
                            button {
                                style: if selected_subtab.as_ref().is_some_and(|active| active.id == subtab.id) {
                                    {
                                        let accent = theme
                                            .main_tab_accents
                                            .get("data")
                                            .map(String::as_str)
                                            .unwrap_or("#f97316");
                                        format!(
                                            "padding:4px 8px; border-radius:999px; border:1px solid {accent}; background:{}; color:{accent}; cursor:pointer; font-size:11px;\
                                             display:inline-flex; align-items:center; justify-content:center;\
                                             font-family:inherit;\
                                             min-width:0; max-width:100%; text-align:center; line-height:1.2;\
                                             white-space:normal; overflow-wrap:anywhere; word-break:break-word;",
                                            theme.button_background
                                        )
                                    }
                                } else {
                                    format!(
                                        "padding:4px 8px; border-radius:999px; border:1px solid {}; background:{}; color:{}; cursor:pointer; font-size:11px;\
                                         display:inline-flex; align-items:center; justify-content:center;\
                                         font-family:inherit;\
                                         min-width:0; max-width:100%; text-align:center; line-height:1.2;\
                                         white-space:normal; overflow-wrap:anywhere; word-break:break-word;",
                                        theme.border_soft, theme.panel_background, theme.text_secondary
                                    )
                                },
                                onclick: {
                                    let id = subtab.id.clone();
                                    let current_tab_id = current.clone();
                                    let mut active_subtabs = active_subtabs;
                                    let mut subtabs_expanded = subtabs_expanded;
                                    move |_| {
                                        active_subtabs.write().insert(current_tab_id.clone(), id.clone());
                                        subtabs_expanded.set(false);
                                    }
                                },
                                "{translate_text(&subtab.label)}"
                            }
                        }
                    }
                    }
                }

                div {
                    key: format!(
                        "data-live-{current}-{}",
                        selected_subtab_snapshot
                            .as_ref()
                            .map(|subtab| subtab.id.as_str())
                            .unwrap_or("__none__")
                    ),
                    DataLivePanel {
                        current_tab: current_tab_snapshot,
                        selected_subtab: selected_subtab_snapshot,
                        current_tab_id: current.clone(),
                        state_chart_labels_vertical: state_chart_labels_vertical,
                        theme: theme.clone(),
                        is_fullscreen: is_fullscreen,
                        show_chart: show_chart,
                    }
                }
            }
        }
    }
}

fn summary_color(i: usize) -> &'static str {
    series_color(i)
}

fn effective_source(
    tab: Option<&DataTabSpec>,
    subtab: Option<&DataSubtabSpec>,
) -> Option<DataSource> {
    let data_type = subtab
        .and_then(|subtab| subtab.data_type.clone())
        .or_else(|| tab.map(|tab| tab.id.clone()))?;
    let sender_id = subtab.and_then(|subtab| subtab.sender_id.clone());
    Some(DataSource {
        data_type,
        sender_id,
    })
}

fn effective_labels(tab: Option<&DataTabSpec>, subtab: Option<&DataSubtabSpec>) -> Vec<String> {
    if let Some(channels) = subtab.and_then(|subtab| subtab.channels.as_ref()) {
        return channels.iter().map(|label| translate_text(label)).collect();
    }
    tab.map(|tab| {
        tab.channels
            .iter()
            .map(|label| translate_text(label))
            .collect()
    })
    .unwrap_or_default()
}

fn effective_channel_formatters<'a>(
    tab: Option<&'a DataTabSpec>,
    subtab: Option<&'a DataSubtabSpec>,
) -> Option<&'a Vec<ValueFormatter>> {
    subtab
        .and_then(|subtab| subtab.channel_formatters.as_ref())
        .or_else(|| tab.and_then(|tab| tab.channel_formatters.as_ref()))
}

fn effective_boolean_labels<'a>(
    tab: Option<&'a DataTabSpec>,
    subtab: Option<&'a DataSubtabSpec>,
) -> Option<&'a BooleanLabels> {
    subtab
        .and_then(|subtab| subtab.boolean_labels.as_ref())
        .or_else(|| tab.and_then(|tab| tab.boolean_labels.as_ref()))
}

fn effective_channel_boolean_labels<'a>(
    tab: Option<&'a DataTabSpec>,
    subtab: Option<&'a DataSubtabSpec>,
) -> Option<&'a Vec<BooleanLabels>> {
    subtab
        .and_then(|subtab| subtab.channel_boolean_labels.as_ref())
        .or_else(|| tab.and_then(|tab| tab.channel_boolean_labels.as_ref()))
}

fn effective_chart_groups(
    tab: Option<&DataTabSpec>,
    subtab: Option<&DataSubtabSpec>,
    channel_count: usize,
    summary_item_count: usize,
) -> Vec<DataChartGroup> {
    let inferred_channel_count = channel_count.max(summary_item_count);
    subtab
        .and_then(|subtab| subtab.chart_groups.as_ref())
        .or_else(|| tab.and_then(|tab| tab.chart_groups.as_ref()))
        .cloned()
        .unwrap_or_else(|| {
            vec![DataChartGroup {
                title: None,
                data_type: None,
                sender_id: None,
                labels: None,
                channels: (0..inferred_channel_count).collect(),
                chart_series: None,
                scale_mode: None,
            }]
        })
}

fn chart_groups_have_graph_source(
    chart_groups: &[DataChartGroup],
    summary_items: &[DataSummaryItem],
    fallback_labels: &[String],
) -> bool {
    chart_groups.iter().any(|group| {
        group.data_type.is_some()
            || group
                .chart_series
                .as_ref()
                .is_some_and(|series| !series.is_empty())
            || chart_series_for_group(group, summary_items, fallback_labels)
                .is_some_and(|series| !series.is_empty())
            || (!group.channels.is_empty() && !fallback_labels.is_empty())
    })
}

fn chart_key_for_group(group: &DataChartGroup, fallback: &str) -> String {
    if let Some(data_type) = group.data_type.as_deref() {
        if let Some(sender_id) = group.sender_id.as_deref() {
            return sender_scoped_chart_key(data_type, sender_id);
        }
        return data_type.to_string();
    }
    fallback.to_string()
}

fn chart_key_for_source(source: &DataSource) -> String {
    source
        .sender_id
        .as_deref()
        .map(|sender_id| sender_scoped_chart_key(&source.data_type, sender_id))
        .unwrap_or_else(|| source.data_type.clone())
}

fn chart_series_for_group(
    group: &DataChartGroup,
    summary_items: &[DataSummaryItem],
    fallback_labels: &[String],
) -> Option<Vec<ChartSeriesSpec>> {
    if let Some(series) = group.chart_series.as_ref()
        && !series.is_empty()
    {
        return Some(series.clone());
    }

    if summary_items.is_empty() {
        return None;
    }

    let legend_source = group.labels.as_deref().unwrap_or(fallback_labels);
    let mut series = Vec::new();
    for (group_idx, channel_idx) in group.channels.iter().enumerate() {
        let label = legend_source
            .get(*channel_idx)
            .or_else(|| legend_source.get(group_idx));
        let item = label
            .and_then(|label| {
                summary_items
                    .iter()
                    .find(|item| item.label.eq_ignore_ascii_case(label))
            })
            .or_else(|| summary_items.get(*channel_idx))
            .or_else(|| summary_items.get(group_idx));
        let Some(item) = item else {
            continue;
        };
        series.push(ChartSeriesSpec {
            data_type: item.data_type.clone(),
            index: item.index,
            sender_id: item.sender_id.clone(),
            label: Some(item.label.clone()),
        });
    }

    if series.is_empty() {
        None
    } else {
        Some(series)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DataChartGroup, DataSource, DataSummaryItem, chart_groups_have_graph_source,
        chart_series_for_group, data_live_panel_has_telemetry, effective_chart_groups,
    };
    use crate::telemetry_dashboard::{TelemetryRow, reset_latest_telemetry};

    #[test]
    fn inferred_chart_series_preserves_summary_sender_id() {
        let group = DataChartGroup {
            title: Some("Battery".to_string()),
            data_type: None,
            sender_id: None,
            labels: Some(vec!["Battery".to_string()]),
            channels: vec![0],
            chart_series: None,
            scale_mode: None,
        };
        let summary_items = vec![DataSummaryItem {
            label: "Battery".to_string(),
            data_type: "BATTERY_VOLTAGE".to_string(),
            index: 0,
            sender_id: Some("PB".to_string()),
            formatter: None,
            boolean_labels: None,
        }];

        let series = chart_series_for_group(&group, &summary_items, &[])
            .expect("summary items should infer chart series");

        assert_eq!(series.len(), 1);
        assert_eq!(series[0].data_type, "BATTERY_VOLTAGE");
        assert_eq!(series[0].sender_id.as_deref(), Some("PB"));
    }

    #[test]
    fn summary_only_subtab_builds_default_chart_group_channels() {
        let groups = effective_chart_groups(None, None, 0, 2);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].channels, vec![0, 1]);
    }

    #[test]
    fn summary_only_subtab_has_graph_source_from_inferred_series() {
        let group = DataChartGroup {
            title: None,
            data_type: None,
            sender_id: None,
            labels: None,
            channels: vec![0],
            chart_series: None,
            scale_mode: None,
        };
        let summary_items = vec![DataSummaryItem {
            label: "AV Bay".to_string(),
            data_type: "BATTERY_VOLTAGE".to_string(),
            index: 0,
            sender_id: Some("PB".to_string()),
            formatter: None,
            boolean_labels: None,
        }];

        assert!(chart_groups_have_graph_source(
            &[group],
            &summary_items,
            &[]
        ));
    }

    #[test]
    fn sender_scoped_chart_group_counts_as_live_telemetry() {
        reset_latest_telemetry(&[TelemetryRow {
            timestamp_ms: 1_700_000_050_000,
            data_type: "BATTERY_VOLTAGE".to_string(),
            sender_id: "PB".to_string(),
            values: vec![Some(12.4)],
        }]);

        let group = DataChartGroup {
            title: Some("Battery".to_string()),
            data_type: None,
            sender_id: None,
            labels: Some(vec!["Battery".to_string()]),
            channels: vec![0],
            chart_series: None,
            scale_mode: None,
        };
        let summary_items = vec![DataSummaryItem {
            label: "Battery".to_string(),
            data_type: "BATTERY_VOLTAGE".to_string(),
            index: 0,
            sender_id: Some("PB".to_string()),
            formatter: None,
            boolean_labels: None,
        }];

        assert!(data_live_panel_has_telemetry(
            Some(&DataSource {
                data_type: "BATTERY_VOLTAGE".to_string(),
                sender_id: None,
            }),
            &summary_items,
            &[group],
            &["Battery".to_string()],
        ));

        reset_latest_telemetry(&[]);
    }
}

fn summary_item_has_value(item: &DataSummaryItem) -> bool {
    latest_telemetry_value(&item.data_type, item.sender_id.as_deref(), item.index).is_some()
}

fn summary_item_value(item: &DataSummaryItem) -> String {
    let value = latest_telemetry_value(&item.data_type, item.sender_id.as_deref(), item.index);
    if item.boolean_labels.is_some() {
        boolean_value_text(value, item.boolean_labels.as_ref())
    } else {
        format_value(value, item.formatter.as_ref())
    }
}

#[derive(Clone, PartialEq, Eq)]
struct DataSource {
    data_type: String,
    sender_id: Option<String>,
}

#[component]
#[allow(clippy::too_many_arguments)]
fn DataLivePanel(
    current_tab: Option<DataTabSpec>,
    selected_subtab: Option<DataSubtabSpec>,
    current_tab_id: String,
    state_chart_labels_vertical: bool,
    theme: ThemeConfig,
    is_fullscreen: Signal<bool>,
    show_chart: Signal<bool>,
) -> Element {
    let current_tab_ref = current_tab.as_ref();
    let selected_subtab_ref = selected_subtab.as_ref();
    let effective_source = effective_source(current_tab_ref, selected_subtab_ref);
    let labels = effective_labels(current_tab_ref, selected_subtab_ref);
    let channel_formatters = effective_channel_formatters(current_tab_ref, selected_subtab_ref);
    let boolean_labels = effective_boolean_labels(current_tab_ref, selected_subtab_ref);
    let channel_boolean_labels =
        effective_channel_boolean_labels(current_tab_ref, selected_subtab_ref);

    let chart_enabled = selected_subtab_ref
        .and_then(|subtab| subtab.chart.as_ref().map(|c| c.enabled))
        .or_else(|| current_tab_ref.and_then(|tab| tab.chart.as_ref().map(|c| c.enabled)))
        .unwrap_or(true);
    let summary_items = selected_subtab_ref
        .and_then(|subtab| subtab.summary_items.as_ref())
        .cloned()
        .unwrap_or_default();

    let chart_groups = effective_chart_groups(
        current_tab_ref,
        selected_subtab_ref,
        labels.len(),
        summary_items.len(),
    );
    let has_chart_source = effective_source.is_some()
        || !summary_items.is_empty()
        || chart_groups_have_graph_source(&chart_groups, &summary_items, &labels);
    let _has_telemetry = data_live_panel_has_telemetry(
        effective_source.as_ref(),
        &summary_items,
        &chart_groups,
        &labels,
    );
    let is_graph_allowed = chart_enabled && has_chart_source;

    let view_w = 1200.0_f64;
    let view_h = 360.0_f64;
    let view_h_full = fullscreen_view_height().max(260.0);
    let left = CHART_GRID_LEFT;
    let right = view_w - CHART_GRID_RIGHT_PAD;
    let pad_top = CHART_GRID_TOP;
    let pad_bottom = CHART_GRID_BOTTOM_PAD;
    let inner_h = view_h - pad_top - pad_bottom;
    let inner_h_full = view_h_full - pad_top - pad_bottom;

    let chart_key = effective_source
        .as_ref()
        .map(chart_key_for_source)
        .unwrap_or_else(|| current_tab_id.clone());
    rsx! {
        DataSummarySection {
            theme: theme.clone(),
            summary_items: summary_items.clone(),
            effective_source: effective_source.clone(),
            labels: labels.clone(),
            channel_formatters: channel_formatters.cloned(),
            boolean_labels: boolean_labels.cloned(),
            channel_boolean_labels: channel_boolean_labels.cloned(),
            is_graph_allowed: is_graph_allowed,
            chart_key: chart_key.clone(),
            view_w: view_w,
            view_h: view_h,
        }
        if is_graph_allowed {
            DataGraphPanel {
                theme: theme.clone(),
                chart_groups: chart_groups.clone(),
                chart_key: chart_key.clone(),
                labels: labels.clone(),
                summary_items: summary_items.clone(),
                state_chart_labels_vertical: state_chart_labels_vertical,
                view_w: view_w,
                view_h: view_h,
                view_h_full: view_h_full,
                left: left,
                right: right,
                pad_top: pad_top,
                pad_bottom: pad_bottom,
                inner_h: inner_h,
                inner_h_full: inner_h_full,
                is_fullscreen: is_fullscreen,
                show_chart: show_chart,
            }
        }
    }
}

fn data_live_panel_has_telemetry(
    effective_source: Option<&DataSource>,
    summary_items: &[DataSummaryItem],
    chart_groups: &[DataChartGroup],
    fallback_labels: &[String],
) -> bool {
    if !summary_items.is_empty() {
        if summary_items.iter().any(summary_item_has_value) {
            return true;
        }
    }

    if effective_source
        .and_then(|source| latest_telemetry_row(&source.data_type, source.sender_id.as_deref()))
        .is_some()
    {
        return true;
    }

    chart_groups.iter().any(|group| {
        if let Some(data_type) = group.data_type.as_deref() {
            return latest_telemetry_row(data_type, group.sender_id.as_deref()).is_some();
        }

        chart_series_for_group(group, summary_items, fallback_labels).is_some_and(|series| {
            series.iter().any(|spec| {
                latest_telemetry_value(&spec.data_type, spec.sender_id.as_deref(), spec.index)
                    .is_some()
            })
        }) || (!group.channels.is_empty()
            && effective_source.is_some_and(|source| {
                latest_telemetry_row(&source.data_type, source.sender_id.as_deref()).is_some()
            }))
    })
}

#[component]
#[allow(clippy::too_many_arguments)]
fn DataSummarySection(
    theme: ThemeConfig,
    summary_items: Vec<DataSummaryItem>,
    effective_source: Option<DataSource>,
    labels: Vec<String>,
    channel_formatters: Option<Vec<ValueFormatter>>,
    boolean_labels: Option<BooleanLabels>,
    channel_boolean_labels: Option<Vec<BooleanLabels>>,
    is_graph_allowed: bool,
    chart_key: String,
    view_w: f64,
    view_h: f64,
) -> Element {
    let _ = *TELEMETRY_RENDER_EPOCH.read();
    if is_graph_allowed {
        let _ = *CHART_RENDER_EPOCH.read();
    }

    let latest_row = effective_source
        .as_ref()
        .and_then(|source| latest_telemetry_row(&source.data_type, source.sender_id.as_deref()));
    let (chan_min, chan_max) = if is_graph_allowed {
        charts_cache_get_channel_minmax(&chart_key, view_w as f32, view_h as f32)
    } else {
        (Vec::new(), Vec::new())
    };

    if !summary_items.is_empty() {
        let grid_style = summary_grid_style(summary_items.len());
        return rsx! {
            div {
                style: "{grid_style}",
                for (i, item) in summary_items.iter().enumerate() {
                    SummaryCard {
                        label: translate_text(&item.label),
                        min: None,
                        max: None,
                        value: summary_item_value(item),
                        color: summary_color(i),
                        theme: theme.clone(),
                    }
                }
            }
        };
    }

    let Some(row) = latest_row else {
        return rsx! {
            div { style: "color:{theme.text_muted}; padding:2px 2px;", "{translate_text(\"Waiting for telemetry…\")}" }
        };
    };

    let vals = &row.values;
    let visible_label_count = labels.iter().filter(|label| !label.is_empty()).count();
    let grid_style = summary_grid_style(visible_label_count);
    rsx! {
        div {
            style: "{grid_style}",
            for (i, label) in labels.iter().enumerate() {
                if !label.is_empty() {
                    SummaryCard {
                        label: label.clone(),
                        min: if is_graph_allowed { chan_min.get(i).copied().flatten().map(|v| format_value(Some(v), channel_formatters.as_ref().and_then(|list| list.get(i)))) } else { None },
                        max: if is_graph_allowed { chan_max.get(i).copied().flatten().map(|v| format_value(Some(v), channel_formatters.as_ref().and_then(|list| list.get(i)))) } else { None },
                        value: if let Some(lbls) = channel_boolean_labels
                            .as_ref()
                            .and_then(|list| list.get(i))
                        {
                            boolean_value_text(vals.get(i).copied().flatten(), Some(lbls))
                        } else if boolean_labels.is_some() {
                            boolean_value_text(vals.get(i).copied().flatten(), boolean_labels.as_ref())
                        } else {
                            format_value(vals.get(i).copied().flatten(), channel_formatters.as_ref().and_then(|list| list.get(i)))
                        },
                        color: summary_color(i),
                        theme: theme.clone(),
                    }
                }
            }
        }
    }
}

#[component]
#[allow(clippy::too_many_arguments)]
fn DataGraphPanel(
    theme: ThemeConfig,
    chart_groups: Vec<DataChartGroup>,
    chart_key: String,
    labels: Vec<String>,
    summary_items: Vec<DataSummaryItem>,
    state_chart_labels_vertical: bool,
    view_w: f64,
    view_h: f64,
    view_h_full: f64,
    left: f64,
    right: f64,
    pad_top: f64,
    pad_bottom: f64,
    inner_h: f64,
    inner_h_full: f64,
    is_fullscreen: Signal<bool>,
    show_chart: Signal<bool>,
) -> Element {
    let charts_enabled = *show_chart.read();
    let fullscreen = *is_fullscreen.read();
    let (panel_id, _) = use_chart_panel_visibility(charts_enabled && !fullscreen);
    if charts_enabled || fullscreen {
        let _ = *CHART_RENDER_EPOCH.read();
    }
    let chart_groups_grid_style = if chart_groups.len() >= 2 {
        "display:grid; grid-template-columns:repeat(2, minmax(0, 1fr)); gap:12px; width:100%; align-items:start;"
    } else {
        "display:grid; grid-template-columns:minmax(0, 1fr); gap:12px; width:100%; align-items:start;"
    };
    let x_pct = |x: f64, total: f64| format!("{:.4}%", (x / total) * 100.0);
    let y_pct = |y: f64, total: f64| format!("{:.4}%", (y / total) * 100.0);
    let on_toggle_fullscreen = move |_: Event<MouseData>| {
        let next = {
            let current = *is_fullscreen.read();
            !current
        };
        is_fullscreen.set(next);
    };
    let on_toggle_chart = move |_: Event<MouseData>| {
        let next = {
            let current = *show_chart.read();
            !current
        };
        show_chart.set(next);
    };

    rsx! {
        div { id: "{panel_id}", style: "flex:0; width:100%; margin-top:6px;",
            div { style: "width:100%;",
                div { style: "display:flex; justify-content:flex-end; gap:8px; margin-bottom:6px;",
                    button {
                        style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                        onclick: on_toggle_chart,
                        if *show_chart.read() {
                            "{translate_text(\"Collapse\")}"
                        } else {
                            "{translate_text(\"Expand\")}"
                        }
                    }
                    button {
                        style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                        onclick: on_toggle_fullscreen,
                        "{translate_text(\"Fullscreen\")}"
                    }
                }

                if *show_chart.read() {
                    div { style: "width:100%;",
                        div { class: "gs26-data-graph-groups", style: "{chart_groups_grid_style}",
                        for group in chart_groups.iter() {
                            {render_chart_group(
                                group,
                                &chart_key,
                                &labels,
                                &summary_items,
                                state_chart_labels_vertical,
                                view_w,
                                view_h,
                                left,
                                right,
                                pad_top,
                                pad_bottom,
                                inner_h,
                                &x_pct,
                                &y_pct,
                                &theme,
                            )}
                        }
                        }
                    }
                }
            }
        }

        if *is_fullscreen.read() {
            {
                let (_chunks_full, _y_min2, _y_max2, _span_min2) =
                    charts_cache_get(&chart_key, view_w as f32, view_h_full as f32);

                rsx! {
                    div { style: "position:fixed; inset:0; z-index:9998; padding:16px; background:{theme.app_background}; display:flex; flex-direction:column; gap:12px;",
                        div { style: "display:flex; align-items:center; justify-content:space-between; gap:12px;",
                            h2 { style: "margin:0; color:{theme.main_tab_accents.get(\"data\").map(String::as_str).unwrap_or(\"#f97316\")};", "{translate_text(\"Data Graph\")}" }
                            button {
                                style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                                onclick: on_toggle_fullscreen,
                                "{translate_text(\"Exit Fullscreen\")}"
                            }
                        }

                        div {
                            style: "flex:1; min-height:0; width:100%; overflow-y:auto;",
                            div { class: "gs26-data-graph-groups-fullscreen", style: "{chart_groups_grid_style}",
                            for group in chart_groups.iter() {
                                {render_chart_group(
                                    group,
                                    &chart_key,
                                    &labels,
                                    &summary_items,
                                    state_chart_labels_vertical,
                                    view_w,
                                    view_h_full,
                                    left,
                                    right,
                                    pad_top,
                                    pad_bottom,
                                    inner_h_full,
                                    &x_pct,
                                    &y_pct,
                                    &theme,
                                )}
                            }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_chart_group(
    group: &DataChartGroup,
    fallback_chart_key: &str,
    fallback_labels: &[String],
    summary_items: &[DataSummaryItem],
    state_chart_labels_vertical: bool,
    view_w: f64,
    view_h: f64,
    left: f64,
    right: f64,
    pad_top: f64,
    pad_bottom: f64,
    inner_h: f64,
    x_pct: &dyn Fn(f64, f64) -> String,
    y_pct: &dyn Fn(f64, f64) -> String,
    theme: &ThemeConfig,
) -> Element {
    let chart_key = chart_key_for_group(group, fallback_chart_key);
    let per_series_scale = matches!(group.scale_mode, Some(DataChartScaleMode::PerSeries));
    let multi_series = chart_series_for_group(group, summary_items, fallback_labels);
    let normalize_multi_series = multi_series.as_ref().is_some_and(|series| series.len() > 1);
    let use_per_series_scale = per_series_scale || normalize_multi_series;
    let chart_left = if use_per_series_scale {
        DATA_CHART_NORMALIZED_GRID_LEFT as f64
    } else {
        left
    };
    let chart_right = if use_per_series_scale {
        view_w - DATA_CHART_NORMALIZED_GRID_RIGHT_PAD as f64
    } else {
        right
    };
    let DataChartRenderPayload {
        filtered_chunks,
        y_min,
        y_max,
        span_min,
        legend_labels,
        per_series_scales,
        scale_label_placements,
    } = chart_group_render_payload_cached(
        group,
        &chart_key,
        fallback_labels,
        multi_series.as_deref(),
        view_w,
        view_h,
        chart_left,
        chart_right,
        pad_top,
        pad_bottom,
        inner_h,
        state_chart_labels_vertical,
    );
    let canvas_identity_key = chart_canvas_identity_key(
        &chart_key,
        group,
        fallback_labels,
        &legend_labels,
        multi_series.as_deref(),
    );
    let reseed_note = reseed_status_note();
    if filtered_chunks.is_empty() {
        return rsx! {
            div { style: "width:100%; background:{theme.app_background}; border-radius:14px; border:1px solid {theme.border}; padding:12px; display:flex; flex-direction:column; gap:8px;",
                if let Some(title) = group.title.as_ref() {
                    div { style: "font-size:13px; font-weight:600; color:{theme.text_primary};", "{translate_text(title)}" }
                }
                if let Some((kind, note)) = reseed_note.as_ref() {
                    {reseed_note_banner(kind, note, theme, false)}
                }
                div { style: "color:{theme.text_muted}; font-size:12px;", "{translate_text(\"No chart data yet.\")}" }
            }
        };
    }
    let x_left_s = fmt_span(span_min);
    let x_mid_s = fmt_span(span_min * 0.5);
    let y_mid = (y_min + y_max) * 0.5;
    let y_max_s = format!("{:.2}", y_max);
    let y_mid_s = format!("{:.2}", y_mid);
    let y_min_s = format!("{:.2}", y_min);
    let x_label_top = view_h - pad_bottom + CHART_X_LABEL_BOTTOM;
    let legend_rows: Vec<(usize, &str)> = legend_labels
        .iter()
        .enumerate()
        .filter(|(_, label)| !label.is_empty())
        .map(|(idx, label)| (idx, label.as_str()))
        .collect();
    let scale_entries = if use_per_series_scale {
        per_series_scales
            .iter()
            .enumerate()
            .filter_map(|(i, scale)| {
                scale.map(|(series_min, series_max)| {
                    [
                        (i, format!("{:.2}", series_max)),
                        (i, format!("{:.2}", (series_min + series_max) * 0.5)),
                        (i, format!("{:.2}", series_min)),
                    ]
                })
            })
            .flatten()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let widest_scale_label_chars = if state_chart_labels_vertical {
        scale_label_placements
            .iter()
            .map(|entry| entry.text.len())
            .max()
            .unwrap_or(5)
    } else {
        scale_entries
            .iter()
            .map(|(_, text)| text.len())
            .max()
            .unwrap_or(5)
    };
    let scale_chip_width = (widest_scale_label_chars as f64 * 5.8 + 13.0).clamp(38.0, 74.0);
    let per_series_label_width = if use_per_series_scale && state_chart_labels_vertical {
        let columns = scale_label_placements
            .iter()
            .map(|entry| entry.rail_column)
            .max()
            .unwrap_or(0)
            + 1;
        (columns as f64 * scale_chip_width)
            + (columns.saturating_sub(1) as f64 * DATA_CHART_VERTICAL_SCALE_LABEL_RAIL_GAP)
    } else if use_per_series_scale {
        scale_chip_width + 8.0
    } else {
        0.0
    };
    let rendered_chart_height = if use_per_series_scale && state_chart_labels_vertical {
        view_h
    } else if use_per_series_scale {
        let rows = scale_entries.len().max(1);
        (CHART_GRID_TOP
            + CHART_GRID_BOTTOM_PAD
            + 28.0
            + rows.saturating_sub(1) as f64 * DATA_CHART_PER_SERIES_LABEL_ROW_GAP)
            .max(view_h)
    } else {
        view_h
    };
    let chart_shell_size_style = if use_per_series_scale {
        format!("height:{rendered_chart_height}px;")
    } else {
        format!("aspect-ratio:{view_w}/{view_h};")
    };
    let per_series_chip_style = |i: usize| {
        format!(
            "box-sizing:border-box; min-width:{scale_chip_width}px; max-width:{scale_chip_width}px; padding:0 4px; line-height:1.1; font-size:clamp(8px, 1.8vw, 10px); text-align:center; font-variant-numeric:tabular-nums; border-radius:999px; border:1px solid {border}; background:{bg}; color:{fg}; \
             box-shadow: inset 0 0 0 1px rgba(255,255,255,0.04); text-shadow:0 1px 1px rgba(2,6,23,0.85);",
            border = series_color(i),
            bg = theme.panel_background,
            fg = series_color(i),
            scale_chip_width = scale_chip_width,
        )
    };
    rsx! {
        div { style: "width:100%; background:{theme.app_background}; border-radius:14px; border:1px solid {theme.border}; padding:12px; display:flex; flex-direction:column; gap:8px;",
            if let Some(title) = group.title.as_ref() {
                div { style: "font-size:13px; font-weight:600; color:{theme.text_primary};", "{translate_text(title)}" }
            }
            if let Some((kind, note)) = reseed_note.as_ref() {
                {reseed_note_banner(kind, note, theme, false)}
            }
            div { style: "display:flex; gap:6px; align-items:stretch; width:100%; {chart_shell_size_style}",
                if use_per_series_scale {
                    {normalized_scale_labels_side(
                        &legend_labels,
                        &per_series_scales,
                        per_series_label_width,
                        pad_top,
                        inner_h,
                        rendered_chart_height,
                        state_chart_labels_vertical,
                        &scale_label_placements,
                        scale_chip_width,
                        &per_series_chip_style,
                    )}
                }
                div { style: "position:relative; flex:1 1 auto; min-width:0; height:100%;",
                    ChartCanvas {
                        key: canvas_identity_key.clone(),
                        identity_key: canvas_identity_key.clone(),
                        view_w: view_w,
                        view_h: view_h,
                        chunks: filtered_chunks,
                        grid_left: Some(chart_left),
                        grid_right: Some(chart_right),
                        grid_top: Some(pad_top),
                        grid_bottom: Some(view_h - pad_bottom),
                        style: "position:absolute; inset:0; width:100%; height:100%; display:block;".to_string(),
                    }
                    div { style: "position:absolute; inset:0; pointer-events:none; font-size:clamp(8px, 1.8vw, 10px); color:{theme.text_muted};",
                        if !use_per_series_scale {
                            span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(pad_top + 6.0, view_h)}; max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", "{y_max_s}" }
                            span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(pad_top + inner_h / 2.0 + 4.0, view_h)}; transform:translateY(-50%); max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", "{y_mid_s}" }
                            span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(view_h - pad_bottom + 1.0, view_h)}; transform:translateY(-100%); max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", "{y_min_s}" }
                        }
                        span { style: "position:absolute; left:{x_pct(chart_left + CHART_X_LABEL_LEFT_INSET, view_w)}; top:{y_pct(x_label_top, view_h)};", "{x_left_s}" }
                        span { style: "position:absolute; left:{x_pct(view_w * 0.5, view_w)}; top:{y_pct(x_label_top, view_h)}; transform:translateX(-50%);", "{x_mid_s}" }
                        span { style: "position:absolute; left:{x_pct(chart_right - 52.0, view_w)}; top:{y_pct(x_label_top, view_h)};", "{translate_text(\"now\")}" }
                    }
                    if use_per_series_scale && state_chart_labels_vertical {
                        {normalized_scale_connector_overlay(
                            &scale_label_placements,
                            chart_left,
                            DATA_CHART_VERTICAL_SCALE_LABEL_RAIL_GAP,
                            scale_chip_width,
                            view_w,
                            rendered_chart_height,
                        )}
                    }
                }
            }
            if !legend_rows.is_empty() {
                div { style: "display:flex; flex-wrap:wrap; gap:8px; padding:6px 10px; background:rgba(2,6,23,0.75); border:1px solid {theme.border_soft}; border-radius:10px;",
                    for (i, label) in legend_rows.iter() {
                        div { style: "display:flex; align-items:center; gap:6px; font-size:12px; color:{theme.text_secondary};",
                            SeriesSwatch { index: *i }
                            "{translate_text(label)}"
                        }
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn chart_group_render_payload_cached(
    group: &DataChartGroup,
    chart_key: &str,
    fallback_labels: &[String],
    multi_series: Option<&[ChartSeriesSpec]>,
    view_w: f64,
    view_h: f64,
    chart_left: f64,
    chart_right: f64,
    pad_top: f64,
    pad_bottom: f64,
    inner_h: f64,
    state_chart_labels_vertical: bool,
) -> DataChartRenderPayload {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (*CHART_RENDER_EPOCH.read()).hash(&mut hasher);
    chart_key.hash(&mut hasher);
    group.title.hash(&mut hasher);
    group.data_type.hash(&mut hasher);
    group.sender_id.hash(&mut hasher);
    group.labels.hash(&mut hasher);
    group.channels.hash(&mut hasher);
    group.scale_mode.hash(&mut hasher);
    fallback_labels.hash(&mut hasher);
    view_w.to_bits().hash(&mut hasher);
    view_h.to_bits().hash(&mut hasher);
    chart_left.to_bits().hash(&mut hasher);
    chart_right.to_bits().hash(&mut hasher);
    pad_top.to_bits().hash(&mut hasher);
    pad_bottom.to_bits().hash(&mut hasher);
    inner_h.to_bits().hash(&mut hasher);
    state_chart_labels_vertical.hash(&mut hasher);
    if let Some(series) = multi_series {
        hash_chart_series_specs(&mut hasher, series);
    }
    let key = hasher.finish();

    if let Some(payload) = DATA_CHART_RENDER_CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return payload;
    }

    let legend_labels = if let Some(series) = multi_series {
        series
            .iter()
            .map(|spec| {
                spec.label
                    .clone()
                    .unwrap_or_else(|| format!("{}[{}]", spec.data_type, spec.index))
            })
            .collect::<Vec<_>>()
    } else {
        let legend_source = group.labels.as_deref().unwrap_or(fallback_labels);
        group
            .channels
            .iter()
            .enumerate()
            .filter_map(|(group_idx, idx)| {
                legend_source
                    .get(*idx)
                    .or_else(|| legend_source.get(group_idx))
                    .cloned()
            })
            .collect::<Vec<_>>()
    };

    let use_per_series_scale = matches!(group.scale_mode, Some(DataChartScaleMode::PerSeries))
        || multi_series.is_some_and(|series| series.len() > 1);
    let (filtered_chunks, y_min, y_max, span_min, per_series_scales) =
        if let Some(series) = multi_series {
            if series.len() == 1 {
                let spec = &series[0];
                let single_chart_key = spec
                    .sender_id
                    .as_deref()
                    .map(|sender_id| sender_scoped_chart_key(&spec.data_type, sender_id))
                    .unwrap_or_else(|| spec.data_type.clone());
                let single_channel = [spec.index];
                if use_per_series_scale {
                    let (chunks, scales, span_min) = charts_cache_get_subset_per_series_with_grid(
                        &single_chart_key,
                        &single_channel,
                        view_w as f32,
                        view_h as f32,
                        chart_left as f32,
                        (view_w - chart_right) as f32,
                        pad_top as f32,
                        pad_bottom as f32,
                    );
                    let overall_min = scales
                        .iter()
                        .flatten()
                        .map(|(min, _)| *min)
                        .fold(f32::INFINITY, f32::min);
                    let overall_max = scales
                        .iter()
                        .flatten()
                        .map(|(_, max)| *max)
                        .fold(f32::NEG_INFINITY, f32::max);
                    (
                        chunks,
                        if overall_min.is_finite() {
                            overall_min
                        } else {
                            0.0
                        },
                        if overall_max.is_finite() {
                            overall_max
                        } else {
                            1.0
                        },
                        span_min,
                        scales,
                    )
                } else {
                    let (chunks, y_min, y_max, span_min) = charts_cache_get_subset(
                        &single_chart_key,
                        &single_channel,
                        view_w as f32,
                        view_h as f32,
                    );
                    (chunks, y_min, y_max, span_min, Rc::new(Vec::new()))
                }
            } else {
                let cache_series = series
                    .iter()
                    .map(|spec| {
                        let chart_key = spec
                            .sender_id
                            .as_deref()
                            .map(|sender_id| sender_scoped_chart_key(&spec.data_type, sender_id))
                            .unwrap_or_else(|| spec.data_type.clone());
                        (chart_key, spec.index)
                    })
                    .collect::<Vec<_>>();
                let (chunks, scales, span_min) = charts_cache_get_multi_series_per_series_with_grid(
                    &cache_series,
                    view_w as f32,
                    view_h as f32,
                    chart_left as f32,
                    (view_w - chart_right) as f32,
                    pad_top as f32,
                    pad_bottom as f32,
                );
                let overall_min = scales
                    .iter()
                    .flatten()
                    .map(|(min, _)| *min)
                    .fold(f32::INFINITY, f32::min);
                let overall_max = scales
                    .iter()
                    .flatten()
                    .map(|(_, max)| *max)
                    .fold(f32::NEG_INFINITY, f32::max);
                (
                    chunks,
                    if overall_min.is_finite() {
                        overall_min
                    } else {
                        0.0
                    },
                    if overall_max.is_finite() {
                        overall_max
                    } else {
                        1.0
                    },
                    span_min,
                    scales,
                )
            }
        } else if use_per_series_scale {
            let (chunks, scales, span_min) = charts_cache_get_subset_per_series_with_grid(
                chart_key,
                &group.channels,
                view_w as f32,
                view_h as f32,
                chart_left as f32,
                (view_w - chart_right) as f32,
                pad_top as f32,
                pad_bottom as f32,
            );
            let overall_min = scales
                .iter()
                .flatten()
                .map(|(min, _)| *min)
                .fold(f32::INFINITY, f32::min);
            let overall_max = scales
                .iter()
                .flatten()
                .map(|(_, max)| *max)
                .fold(f32::NEG_INFINITY, f32::max);
            (
                chunks,
                if overall_min.is_finite() {
                    overall_min
                } else {
                    0.0
                },
                if overall_max.is_finite() {
                    overall_max
                } else {
                    1.0
                },
                span_min,
                scales,
            )
        } else {
            let (chunks, y_min, y_max, span_min) =
                charts_cache_get_subset(chart_key, &group.channels, view_w as f32, view_h as f32);
            (chunks, y_min, y_max, span_min, Rc::new(Vec::new()))
        };
    let scale_label_placements = if use_per_series_scale && state_chart_labels_vertical {
        stacked_scale_label_placements(&per_series_scales, pad_top, inner_h, view_h)
    } else {
        Vec::new()
    };

    let payload = DataChartRenderPayload {
        filtered_chunks,
        y_min,
        y_max,
        span_min,
        legend_labels,
        per_series_scales,
        scale_label_placements,
    };
    DATA_CHART_RENDER_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        retain_recent_chart_cache_entries(&mut cache);
        cache.insert(key, payload.clone());
    });
    payload
}

fn stacked_scale_label_placements(
    series_scales: &[Option<(f32, f32)>],
    top: f64,
    inner_h: f64,
    view_h: f64,
) -> Vec<ScaleLabelPlacement> {
    let mut entries = Vec::new();
    let top_y = top + 8.0;
    let mid_y = top + inner_h * 0.5;
    let bottom_y = top + inner_h - 8.0;

    for (series_index, scale) in series_scales.iter().enumerate() {
        let Some((series_min, series_max)) = scale else {
            continue;
        };
        entries.push(ScaleLabelPlacement {
            series_index,
            target_y: top_y,
            label_y: top_y,
            rail_column: 0,
            text: format!("{:.2}", series_max),
        });
        entries.push(ScaleLabelPlacement {
            series_index,
            target_y: mid_y,
            label_y: mid_y,
            rail_column: 0,
            text: format!("{:.2}", (series_min + series_max) * 0.5),
        });
        entries.push(ScaleLabelPlacement {
            series_index,
            target_y: bottom_y,
            label_y: bottom_y,
            rail_column: 0,
            text: format!("{:.2}", series_min),
        });
    }

    entries.sort_by(|a, b| {
        a.target_y
            .partial_cmp(&b.target_y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.series_index.cmp(&b.series_index))
    });

    let min_y = top_y;
    let max_y = (view_h - CHART_GRID_BOTTOM_PAD - 8.0).max(min_y);
    let row_gap = DATA_CHART_PER_SERIES_LABEL_ROW_GAP;
    let rows_per_column = (((max_y - min_y) / row_gap).floor() as usize + 1).max(1);
    let columns = entries.len().div_ceil(rows_per_column).max(1);
    let rows_in_column = entries.len().div_ceil(columns).max(1);
    let effective_gap = if rows_in_column > 1 {
        row_gap.max((max_y - min_y) / (rows_in_column as f64 - 1.0))
    } else {
        row_gap
    };

    for (idx, entry) in entries.iter_mut().enumerate() {
        let column = idx / rows_in_column;
        let row = idx % rows_in_column;
        entry.rail_column = column;
        entry.label_y = (min_y + row as f64 * effective_gap).clamp(min_y, max_y);
    }

    entries
}

fn normalized_scale_labels_side<F>(
    labels: &[String],
    series_scales: &[Option<(f32, f32)>],
    label_width: f64,
    top: f64,
    inner_h: f64,
    view_h: f64,
    vertical_mode: bool,
    vertical_placements: &[ScaleLabelPlacement],
    scale_chip_width: f64,
    scale_chip_style: &F,
) -> Element
where
    F: Fn(usize) -> String,
{
    let cols = labels
        .iter()
        .enumerate()
        .filter_map(|(i, _)| {
            series_scales
                .get(i)
                .and_then(|scale| scale.map(|pair| (i, pair)))
        })
        .collect::<Vec<_>>();
    let pct = |value: f64, total: f64| format!("{:.4}%", (value / total) * 100.0);
    let row_style = |y: f64, transform: &str| {
        format!(
            "position:absolute; right:0; top:{}; display:flex; align-items:center; gap:clamp(2px, 0.35vw, 4px); \
             transform:{transform}; pointer-events:none; white-space:nowrap;",
            pct(y, view_h)
        )
    };
    let top_row_style = row_style(top + 6.0, "");
    let mid_row_style = row_style(top + inner_h * 0.5, "translateY(-50%)");
    let bottom_row_style = row_style(top + inner_h - 6.0, "translateY(-100%)");

    rsx! {
        div { style: "position:relative; flex:0 0 {label_width}px; width:{label_width}px; min-width:{label_width}px; height:100%; overflow:hidden; container-type:inline-size;",
            if vertical_mode {
                for entry in vertical_placements.iter() {
                    div {
                        style: "position:absolute; right:{entry.rail_column as f64 * (scale_chip_width + DATA_CHART_VERTICAL_SCALE_LABEL_RAIL_GAP)}px; top:{pct(entry.label_y, view_h)}; transform:translateY(-50%); pointer-events:none; max-width:{scale_chip_width}px;",
                        div { style: "{scale_chip_style(entry.series_index)}", "{entry.text}" }
                    }
                }
            } else {
                div { style: "{top_row_style}",
                    for (i, (_, series_max)) in cols.iter().copied() {
                        div { style: "{scale_chip_style(i)}", {format!("{:.2}", series_max)} }
                    }
                }
                div { style: "{mid_row_style}",
                    for (i, (series_min, series_max)) in cols.iter().copied() {
                        div { style: "{scale_chip_style(i)}", {format!("{:.2}", (series_min + series_max) * 0.5)} }
                    }
                }
                div { style: "{bottom_row_style}",
                    for (i, (series_min, _)) in cols.iter().copied() {
                        div { style: "{scale_chip_style(i)}", {format!("{:.2}", series_min)} }
                    }
                }
            }
        }
    }
}

fn normalized_scale_connector_overlay(
    placements: &[ScaleLabelPlacement],
    y_axis_x: f64,
    rail_gap: f64,
    scale_chip_width: f64,
    view_w: f64,
    view_h: f64,
) -> Element {
    rsx! {
        svg {
            style: "position:absolute; inset:0; width:100%; height:100%; pointer-events:none; overflow:visible;",
            view_box: "0 0 {view_w} {view_h}",
            preserve_aspect_ratio: "none",
            for entry in placements.iter() {
                line {
                    x1: "{-(rail_gap + entry.rail_column as f64 * (scale_chip_width + DATA_CHART_VERTICAL_SCALE_LABEL_RAIL_GAP))}",
                    y1: "{entry.label_y}",
                    x2: "{y_axis_x}",
                    y2: "{entry.target_y}",
                    stroke: "#fbbf24",
                    stroke_width: "1.2",
                    stroke_opacity: "0.72",
                    vector_effect: "non-scaling-stroke",
                }
                circle {
                    cx: "{y_axis_x}",
                    cy: "{entry.target_y}",
                    r: "1.4",
                    fill: "#fbbf24",
                    opacity: "0.86",
                }
            }
        }
    }
}

#[component]
fn SummaryCard(
    label: String,
    value: String,
    min: Option<String>,
    max: Option<String>,
    color: &'static str,
    theme: ThemeConfig,
) -> Element {
    let mm = match (min.as_deref(), max.as_deref()) {
        (Some(mi), Some(ma)) => Some(format!(
            "{} {mi} • {} {ma}",
            translate_text("min"),
            translate_text("max")
        )),
        _ => None,
    };

    rsx! {
        div { style: "padding:10px; border-radius:12px; background:{theme.panel_background_alt}; border:1px solid {theme.border}; width:100%; min-width:0; box-sizing:border-box;",
            div { style: "font-size:12px; color:{color}; line-height:1.15; min-width:0; overflow-wrap:anywhere; word-break:break-word;", "{label}" }
            div { style: "font-size:18px; color:{theme.text_primary}; line-height:1.1; min-width:0; overflow-wrap:anywhere; word-break:break-word;", "{value}" }
            if let Some(t) = mm {
                div { style: "font-size:11px; color:{theme.text_muted}; margin-top:4px; line-height:1.2; min-width:0; overflow-wrap:anywhere; word-break:break-word;", "{t}" }
            }
        }
    }
}

fn summary_grid_style(item_count: usize) -> String {
    if item_count <= 1 {
        "display:grid; gap:10px; align-items:stretch; grid-template-columns:minmax(0, min(220px, 100%)); justify-content:start; width:100%; min-width:0;".to_string()
    } else {
        "display:grid; gap:10px; align-items:stretch; grid-template-columns:repeat(auto-fit, minmax(min(100%, 128px), 1fr)); width:100%; min-width:0;".to_string()
    }
}

fn format_value(v: Option<f32>, formatter: Option<&ValueFormatter>) -> String {
    match v {
        Some(x) => {
            let kind = formatter
                .and_then(|formatter| formatter.kind.clone())
                .unwrap_or(ValueFormatKind::Number);
            let precision = formatter.and_then(|formatter| formatter.precision);
            let prefix = formatter
                .and_then(|formatter| formatter.prefix.as_deref())
                .unwrap_or("");
            let suffix = formatter
                .and_then(|formatter| formatter.suffix.as_deref())
                .unwrap_or("");
            let value = match kind {
                ValueFormatKind::Number => format!("{x:.prec$}", prec = precision.unwrap_or(4)),
                ValueFormatKind::Integer => format!("{}", x.round() as i64),
            };
            format!("{prefix}{value}{suffix}")
        }
        None => "-".to_string(),
    }
}

fn boolean_value_text(v: Option<f32>, labels: Option<&BooleanLabels>) -> String {
    let true_label = labels.map(|l| l.true_label.as_str()).unwrap_or("Open");
    let false_label = labels.map(|l| l.false_label.as_str()).unwrap_or("Closed");
    let unknown_label = labels
        .and_then(|l| l.unknown_label.as_deref())
        .unwrap_or("Unknown");
    match v {
        Some(val) if val >= 0.5 => translate_text(true_label),
        Some(_) => translate_text(false_label),
        None => translate_text(unknown_label),
    }
}

fn fullscreen_view_height() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        let h = web_sys::window()
            .and_then(|w| w.inner_height().ok())
            .and_then(|v| v.as_f64())
            .unwrap_or(700.0);
        (h - 140.0).max(360.0)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        600.0
    }
}

fn fmt_span(span_min: f32) -> String {
    if !span_min.is_finite() || span_min <= 0.0 {
        "-0 s".to_string()
    } else if span_min < 1.0 {
        format!("-{:.0} s", span_min * 60.0)
    } else {
        format!("-{:.1} min", span_min)
    }
}
