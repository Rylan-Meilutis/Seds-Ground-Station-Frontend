#![allow(clippy::too_many_arguments)]

// frontend/src/telemetry_dashboard/state_tab.rs

use dioxus::prelude::*;
use dioxus_signals::Signal;

use crate::auth;

use super::blink::{action_opacity, blink_epoch_ms};
use super::layout::{
    ActionSpec, ActionsTabLayout, BooleanLabels, ChartSeriesSpec, DataTabLayout, FillTargetFluid,
    FillTargetValueKind, StateSection, StateSectionStyle, StateSectionValueLayout, StateTabLayout,
    StateWidget, StateWidgetKind, SummaryCardStyle, SummaryItem, ThemeConfig, ValueFormatKind,
    ValueFormatter, ValveColor, ValveColorSet,
};
use super::types::{BoardStatusEntry, FlightState, TelemetryRow};
use super::{
    http_get_json, latest_telemetry_row, latest_telemetry_value, reseed_note_banner, reseed_status_note,
    translate_text, ActionPolicyMsg, BlinkMode, FillTargetsConfig,
    CHART_RENDER_EPOCH, TELEMETRY_RENDER_EPOCH,
};

use crate::telemetry_dashboard::data_chart::{
    charts_cache_get, charts_cache_get_channel_minmax, charts_cache_get_multi_series_per_series, charts_cache_get_subset,
    series_color, ChartCanvas, ChartRenderChunk, SeriesSwatch,
    CHART_GRID_BOTTOM_PAD, CHART_GRID_LEFT, CHART_GRID_RIGHT_PAD, CHART_GRID_TOP, CHART_X_LABEL_BOTTOM,
    CHART_X_LABEL_LEFT_INSET, CHART_Y_LABEL_LEFT, CHART_Y_LABEL_MAX_WIDTH,
};
use crate::telemetry_dashboard::map_tab::MapTab;

const COMBINED_CHART_GRID_LEFT: f32 = CHART_GRID_LEFT as f32;
const VERTICAL_SCALE_LABEL_ROW_GAP: f64 = 17.0;
const VERTICAL_SCALE_LABEL_RAIL_GAP: f64 = 4.0;
const STATE_TAB_RESPONSIVE_CSS: &str = r#"
@media (max-width: 980px) {
  .gs26-state-chart-grid {
    grid-template-columns:minmax(0, 1fr) !important;
  }
}
"#;

#[derive(Clone)]
struct ScaleLabelPlacement {
    series_index: usize,
    target_y: f64,
    label_y: f64,
    rail_column: usize,
    text: String,
}

type CombinedChartPayload = (
    Vec<ChartRenderChunk>,
    f32,
    f32,
    f32,
    Vec<String>,
    bool,
    Vec<Option<(f32, f32)>>,
);

fn min_max_summary_text(min: Option<&str>, max: Option<&str>) -> Option<String> {
    match (min, max) {
        (Some(min), Some(max)) => Some(format!(
            "{} {min} • {} {max}",
            translate_text("min"),
            translate_text("max")
        )),
        _ => None,
    }
}

fn themed_value<'a>(
    enabled: bool,
    override_value: Option<&'a str>,
    default_value: &'a str,
) -> &'a str {
    if enabled {
        override_value.unwrap_or(default_value)
    } else {
        default_value
    }
}

#[component]
pub fn StateTab(
    flight_state: Signal<FlightState>,
    board_status: Signal<Vec<BoardStatusEntry>>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    user_gps: Signal<Option<(f64, f64)>>,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    layout: StateTabLayout,
    data_layout: DataTabLayout,
    actions: ActionsTabLayout,
    action_policy: Signal<ActionPolicyMsg>,
    default_valve_labels: Option<BooleanLabels>,
    abort_only_mode: bool,
    state_chart_labels_vertical: bool,
    theme: ThemeConfig,
    use_layout_theme_overrides: bool,
) -> Element {
    let _ = *TELEMETRY_RENDER_EPOCH.read();

    let state = flight_state.read().clone();
    let boards_snapshot = board_status.read();
    let actions_snapshot = actions.actions.clone();
    let action_policy_snapshot = action_policy.read().clone();
    let visible_state_layout = layout
        .states
        .iter()
        .find(|entry| entry.states.iter().any(|configured| configured == &state))
        .cloned();

    {
        let needs_fill_targets = visible_state_layout
            .as_ref()
            .is_some_and(state_layout_needs_fill_targets);
        let mut fill_targets = fill_targets;
        use_effect(move || {
            if !needs_fill_targets || fill_targets.read().is_some() {
                return;
            }
            spawn(async move {
                if let Ok(targets) = http_get_json::<FillTargetsConfig>("/api/fill_targets").await {
                    fill_targets.set(Some(targets));
                }
            });
        });
    }

    let content = if let Some(state_layout) = visible_state_layout.as_ref() {
        rsx! {
            for (section_idx, section) in state_layout.sections.iter().enumerate() {
                div { key: "state-section-{section_idx}-{section.title.as_deref().unwrap_or_default()}",
                    style: "display:contents;",
                    {render_state_section(
                        section,
                        &boards_snapshot,
                        &data_layout,
                        &actions_snapshot,
                        &action_policy_snapshot,
                        default_valve_labels.as_ref(),
                        rocket_gps,
                        user_gps,
                        fill_targets,
                        abort_only_mode,
                        state_chart_labels_vertical,
                        &theme,
                        use_layout_theme_overrides,
                    )}
                }
            }
        }
    } else {
        rsx! { div { style: "color:{theme.text_muted}; font-size:12px;", "{translate_text(\"No layout for this flight state.\")}" } }
    };

    rsx! {
        div { style: "padding:10px 12px; height:100%; width:100%; max-width:100%; min-width:0; box-sizing:border-box; overflow-y:auto; overflow-x:hidden; -webkit-overflow-scrolling:auto; display:flex; flex-direction:column; gap:10px; padding-bottom:100px;",
            style { "{STATE_TAB_RESPONSIVE_CSS}" }
            {content}
        }
    }
}

fn state_layout_needs_fill_targets(state_layout: &super::layout::StateLayout) -> bool {
    state_layout.sections.iter().any(|section| {
        section.widgets.iter().any(|widget| {
            widget.items.as_ref().is_some_and(|items| {
                items.iter().any(|item| {
                    item.fill_target_fluid.is_some() && item.fill_target_kind.is_some()
                        || summary_item_fill_target_source(widget.data_type.as_deref(), item)
                            .is_some()
                })
            })
        })
    })
}

#[component]
fn Section(
    title: String,
    style: Option<StateSectionStyle>,
    theme: ThemeConfig,
    use_layout_theme_overrides: bool,
    children: Element,
) -> Element {
    let background = themed_value(
        use_layout_theme_overrides,
        style.as_ref().and_then(|style| style.background.as_deref()),
        theme.panel_background.as_str(),
    );
    let border = themed_value(
        use_layout_theme_overrides,
        style.as_ref().and_then(|style| style.border.as_deref()),
        theme.border.as_str(),
    );
    let title_color = themed_value(
        use_layout_theme_overrides,
        style
            .as_ref()
            .and_then(|style| style.title_color.as_deref()),
        theme.text_secondary.as_str(),
    );

    rsx! {
        div { style: "padding:10px; border:1px solid {border}; border-radius:12px; background:{background}; width:100%; box-sizing:border-box; min-width:0;",
            div { style: "font-size:14px; color:{title_color}; font-weight:600; margin-bottom:6px;", "{translate_text(&title)}" }
            {children}
        }
    }
}

fn render_state_section(
    section: &StateSection,
    boards: &[BoardStatusEntry],
    data_layout: &DataTabLayout,
    actions: &[ActionSpec],
    action_policy: &ActionPolicyMsg,
    default_valve_labels: Option<&BooleanLabels>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    user_gps: Signal<Option<(f64, f64)>>,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    abort_only_mode: bool,
    state_chart_labels_vertical: bool,
    theme: &ThemeConfig,
    use_layout_theme_overrides: bool,
) -> Element {
    if !section_has_content(section, actions, abort_only_mode) {
        return rsx! { div {} };
    }
    let title = section
        .title
        .clone()
        .map(|title| translate_text(&title))
        .unwrap_or_else(|| translate_text("Section"));
    let horizontal_values = section_uses_horizontal_values(section);
    let is_compact_horizontal_value_widget = |widget: &StateWidget| {
        let summary_item_count = widget.items.as_ref().map_or(0, Vec::len);
        let valve_item_count = widget.valves.as_ref().map_or(0, Vec::len);
        matches!(
            widget.kind,
            StateWidgetKind::Summary | StateWidgetKind::ValveState
        ) && ((widget.kind == StateWidgetKind::Summary && summary_item_count <= 1)
            || (widget.kind == StateWidgetKind::ValveState && valve_item_count <= 1))
    };
    let compact_horizontal_value_widgets = horizontal_values
        && section
            .widgets
            .iter()
            .all(is_compact_horizontal_value_widget);
    let has_chart_widgets = section
        .widgets
        .iter()
        .any(|widget| matches!(widget.kind, StateWidgetKind::Chart));
    let compact_non_chart_widget_count = section
        .widgets
        .iter()
        .filter(|widget| !matches!(widget.kind, StateWidgetKind::Chart))
        .filter(|widget| is_compact_horizontal_value_widget(widget))
        .count();
    let all_non_chart_widgets_compact = has_chart_widgets
        && section
            .widgets
            .iter()
            .filter(|widget| !matches!(widget.kind, StateWidgetKind::Chart))
            .all(is_compact_horizontal_value_widget);
    let content_style = if compact_horizontal_value_widgets {
        let column_count = section.widgets.len().clamp(1, 4);
        format!(
            "display:grid; grid-template-columns:repeat({column_count}, minmax(0, 1fr)); gap:6px; align-items:start; width:100%; min-width:0;"
        )
    } else if horizontal_values
        && has_chart_widgets
        && all_non_chart_widgets_compact
        && compact_non_chart_widget_count > 0
    {
        let column_count = compact_non_chart_widget_count.clamp(1, 4);
        format!(
            "display:grid; grid-template-columns:repeat({column_count}, minmax(0, 1fr)); gap:6px; align-items:start; width:100%; min-width:0;"
        )
    } else if horizontal_values && has_chart_widgets {
        "display:grid; grid-template-columns:repeat(2, minmax(0, 1fr)); gap:10px; align-items:start; width:100%; min-width:0;"
            .to_string()
    } else if horizontal_values {
        let column_count = section.widgets.len().clamp(1, 4);
        format!(
            "display:grid; grid-template-columns:repeat({column_count}, minmax(0, 1fr)); gap:6px; align-items:start; width:100%; min-width:0;"
        )
    } else {
        "display:flex; flex-direction:column; gap:0; width:100%; min-width:0;".to_string()
    };

    rsx! {
        Section { title: title, style: section.style.clone(), theme: theme.clone(), use_layout_theme_overrides: use_layout_theme_overrides,
            div {
                class: if horizontal_values && has_chart_widgets { "gs26-state-chart-grid" } else { "" },
                style: "{content_style}",
                for (widget_idx, widget) in section.widgets.iter().enumerate() {
                    div {
                        key: "state-widget-{widget_idx}-{widget.data_type.as_deref().unwrap_or_default()}-{widget.chart_title.as_deref().unwrap_or_default()}",
                        style: state_widget_container_style(
                            widget,
                            horizontal_values,
                            has_chart_widgets,
                            compact_horizontal_value_widgets,
                            all_non_chart_widgets_compact,
                        ),
                        {render_state_widget(
                            widget,
                            boards,
                            data_layout,
                            actions,
                            action_policy,
                            default_valve_labels,
                            rocket_gps,
                            user_gps,
                            fill_targets,
                            abort_only_mode,
                            state_chart_labels_vertical,
                            theme,
                            use_layout_theme_overrides,
                            horizontal_values,
                        )}
                    }
                }
            }
        }
    }
}

fn state_widget_container_style(
    widget: &StateWidget,
    horizontal_values: bool,
    has_chart_widgets: bool,
    compact_horizontal_value_widgets: bool,
    all_non_chart_widgets_compact: bool,
) -> &'static str {
    let summary_item_count = widget.items.as_ref().map_or(0, Vec::len);
    let valve_item_count = widget.valves.as_ref().map_or(0, Vec::len);
    let single_horizontal_value = horizontal_values
        && ((widget.kind == StateWidgetKind::Summary && summary_item_count <= 1)
            || (widget.kind == StateWidgetKind::ValveState && valve_item_count <= 1));
    if compact_horizontal_value_widgets {
        return "grid-column:auto / span 1; min-width:0; width:100%;";
    }
    if has_chart_widgets && all_non_chart_widgets_compact {
        if matches!(widget.kind, StateWidgetKind::Chart) {
            return "grid-column:1 / -1; min-width:0; width:100%;";
        }
        return "grid-column:auto / span 1; min-width:0; width:100%;";
    }
    if single_horizontal_value {
        return "grid-column:auto / span 1; min-width:0; width:100%;";
    }
    if has_chart_widgets && matches!(widget.kind, StateWidgetKind::Chart) {
        if widget.full_width
            || widget
                .width_fraction
                .is_some_and(|fraction| fraction >= 0.99)
        {
            return "grid-column:1 / -1; min-width:0; width:100%;";
        }
        return "grid-column:auto / span 1; min-width:0; width:100%;";
    }
    if (widget.full_width && !single_horizontal_value)
        || widget.kind == StateWidgetKind::Actions
        || (horizontal_values && widget.kind == StateWidgetKind::Summary && summary_item_count > 1)
        || (horizontal_values && widget.kind == StateWidgetKind::ValveState && valve_item_count > 1)
    {
        "grid-column:1 / -1; min-width:0; width:100%;"
    } else if horizontal_values
        && widget
            .width_fraction
            .is_some_and(|fraction| fraction >= 0.49)
    {
        "grid-column:span 2; min-width:0; width:100%;"
    } else {
        "grid-column:auto / span 1; min-width:0; width:100%;"
    }
}

fn section_uses_horizontal_values(section: &StateSection) -> bool {
    match section.value_layout {
        StateSectionValueLayout::Horizontal => true,
        StateSectionValueLayout::Vertical => false,
        StateSectionValueLayout::Auto => section.widgets.iter().all(|widget| {
            matches!(
                widget.kind,
                StateWidgetKind::Summary
                    | StateWidgetKind::ValveState
                    | StateWidgetKind::BoardStatus
            )
        }),
    }
}

fn render_state_widget(
    widget: &StateWidget,
    boards: &[BoardStatusEntry],
    data_layout: &DataTabLayout,
    actions: &[ActionSpec],
    action_policy: &ActionPolicyMsg,
    default_valve_labels: Option<&BooleanLabels>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    user_gps: Signal<Option<(f64, f64)>>,
    fill_targets: Signal<Option<FillTargetsConfig>>,
    abort_only_mode: bool,
    state_chart_labels_vertical: bool,
    theme: &ThemeConfig,
    use_layout_theme_overrides: bool,
    horizontal_values: bool,
) -> Element {
    match widget.kind {
        StateWidgetKind::BoardStatus => rsx! { {board_status_table(boards, theme)} },
        StateWidgetKind::Summary => {
            let dt = widget.data_type.as_deref().unwrap_or("");
            let items = widget.items.as_deref().unwrap_or(&[]);
            let has_fill_target_item = items.iter().any(|item| {
                summary_item_fill_target_source(widget.data_type.as_deref(), item).is_some()
            });
            if dt.is_empty() && !has_fill_target_item {
                rsx! { div { style: "color:#94a3b8; font-size:12px;", "{translate_text(\"Missing summary data_type\")}" } }
            } else {
                rsx! { {summary_row(
                    (!dt.is_empty()).then_some(dt),
                    items,
                    fill_targets.read().as_ref(),
                    widget.summary_style.as_ref(),
                    theme,
                    use_layout_theme_overrides,
                    horizontal_values,
                )} }
            }
        }
        StateWidgetKind::Chart => {
            let w = widget.width.unwrap_or(1200.0);
            let h = widget.height.unwrap_or(260.0);
            rsx! {
                StateChartPanel {
                    widget: widget.clone(),
                    data_layout: data_layout.clone(),
                    state_chart_labels_vertical: state_chart_labels_vertical,
                    theme: theme.clone(),
                    view_w: w,
                    view_h: h,
                }
            }
        }
        StateWidgetKind::ValveState => {
            let labels = widget.boolean_labels.as_ref().or(default_valve_labels);
            rsx! { {valve_state_grid(
                widget.data_type.as_deref(),
                widget.valves.as_deref(),
                widget.valve_colors.as_ref(),
                labels,
                widget.valve_labels.as_deref(),
                theme,
                use_layout_theme_overrides,
            )} }
        }
        StateWidgetKind::Map => rsx! {
            MapTab {
                rocket_gps: rocket_gps,
                user_gps: user_gps,
                theme: theme.clone(),
            }
        },
        StateWidgetKind::Actions => {
            rsx! { {action_section(actions, action_policy, widget.actions.as_deref(), abort_only_mode)} }
        }
    }
}

#[component]
fn StateChartPanel(
    widget: StateWidget,
    data_layout: DataTabLayout,
    state_chart_labels_vertical: bool,
    theme: ThemeConfig,
    view_w: f64,
    view_h: f64,
) -> Element {
    let _ = *CHART_RENDER_EPOCH.read();
    let mut is_fullscreen = use_signal(|| false);
    let on_toggle_fullscreen = move |_| {
        let next = !*is_fullscreen.read();
        is_fullscreen.set(next);
    };
    let full_h = fullscreen_view_height().max(view_h).max(320.0);
    let fullscreen_button_label = if *is_fullscreen.read() {
        translate_text("Exit Fullscreen")
    } else {
        translate_text("Fullscreen")
    };

    let chart_body = if let Some(series) = widget.chart_series.as_deref()
        && !series.is_empty()
    {
        let requested_h = if *is_fullscreen.read() {
            full_h
        } else {
            view_h
        };
        let adjusted_h = requested_h.max(min_combined_chart_height_for_labels(
            series,
            state_chart_labels_vertical,
        ));
        combined_state_chart_cached(
            series,
            view_w,
            adjusted_h,
            widget.chart_title.as_deref(),
            &data_layout,
            state_chart_labels_vertical,
            &theme,
        )
    } else {
        let dt = widget.data_type.as_deref().unwrap_or("");
        if dt.is_empty() {
            rsx! { div { style: "color:#94a3b8; font-size:12px;", "{translate_text(\"Missing chart data_type\")}" } }
        } else {
            let labels = labels_from_layout(&data_layout, dt);
            data_style_chart_cached(
                dt,
                view_w,
                if *is_fullscreen.read() {
                    full_h
                } else {
                    view_h
                },
                widget.chart_title.as_deref(),
                &labels,
                &theme,
            )
        }
    };

    rsx! {
        div { style: "display:flex; flex-direction:column; gap:8px;",
            div { style: "display:flex; justify-content:flex-end;",
                button {
                    style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                    onclick: on_toggle_fullscreen,
                    "{fullscreen_button_label}"
                }
            }
            if *is_fullscreen.read() {
                div { style: "position:fixed; inset:0; z-index:9998; padding:16px; background:{theme.app_background}; display:flex; flex-direction:column; gap:12px;",
                    div { style: "display:flex; align-items:center; justify-content:space-between; gap:12px;",
                        h2 { style: "margin:0; color:{theme.text_primary};", "{widget.chart_title.clone().map(|title| translate_text(&title)).unwrap_or_else(|| translate_text(\"Flight Graph\"))}" }
                        button {
                            style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                            onclick: on_toggle_fullscreen,
                            "{translate_text(\"Exit Fullscreen\")}"
                        }
                    }
                    div { style: "flex:1; min-height:0; overflow-y:auto;",
                        {chart_body}
                    }
                }
            } else {
                {chart_body}
            }
        }
    }
}

fn min_combined_chart_height_for_labels(
    specs: &[ChartSeriesSpec],
    state_chart_labels_vertical: bool,
) -> f64 {
    if !state_chart_labels_vertical {
        return 0.0;
    }
    let unique_types = specs
        .iter()
        .map(|spec| spec.data_type.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    if unique_types <= 1 {
        return 0.0;
    }

    let label_rows = specs.len().saturating_mul(3).max(1);
    CHART_GRID_TOP
        + CHART_GRID_BOTTOM_PAD
        + 28.0
        + label_rows.saturating_sub(1) as f64 * VERTICAL_SCALE_LABEL_ROW_GAP
}

fn section_has_content(
    section: &StateSection,
    actions: &[ActionSpec],
    abort_only_mode: bool,
) -> bool {
    if section.widgets.is_empty() {
        return false;
    }
    let has_actions = !actions.is_empty();
    for widget in section.widgets.iter() {
        match widget.kind {
            StateWidgetKind::Actions => {
                if has_actions
                    && has_any_actions(actions, widget.actions.as_deref(), abort_only_mode)
                {
                    return true;
                }
            }
            _ => return true,
        }
    }
    false
}

// ============================================================
// cached chart renderer (uses charts_cache_get)
// ============================================================

fn data_style_chart_cached(
    dt: &str,
    view_w: f64,
    view_h: f64,
    title: Option<&str>,
    labels: &[String],
    theme: &ThemeConfig,
) -> Element {
    let w = view_w as f32;
    let h = view_h as f32;

    let (chunks, y_min, y_max, span_min) = charts_cache_get(dt, w, h);
    let reseed_note = reseed_status_note();

    let left = CHART_GRID_LEFT;
    let right = view_w - CHART_GRID_RIGHT_PAD;
    let top = CHART_GRID_TOP;
    let bottom = view_h - CHART_GRID_BOTTOM_PAD;

    let inner_h = bottom - top;

    let y_mid = (y_min + y_max) * 0.5;
    let x_label_top = bottom + CHART_X_LABEL_BOTTOM;
    let x_pct = |x: f64, total: f64| format!("{:.4}%", (x / total) * 100.0);
    let y_pct = |y: f64, total: f64| format!("{:.4}%", (y / total) * 100.0);

    rsx! {
        div { style: "width:100%; background:{theme.panel_background_alt}; border-radius:14px; border:1px solid {theme.border}; padding:12px; display:flex; flex-direction:column; gap:8px;",
            if let Some(t) = title {
                div { style: "color:{theme.text_primary}; font-weight:700; font-size:14px;", "{translate_text(t)}" }
            }
            if let Some((kind, note)) = reseed_note.as_ref() {
                {reseed_note_banner(kind, note, theme, false)}
            }

            if chunks.is_empty() {
                div { style: "color:{theme.text_muted}; font-size:12px;", "{translate_text(\"No chart data yet.\")}" }
            } else {
                div { style: "position:relative; width:100%; aspect-ratio:{view_w}/{view_h};",
                    ChartCanvas {
                        view_w: view_w,
                        view_h: view_h,
                        chunks: chunks,
                        grid_left: Some(left),
                        grid_right: Some(right),
                        grid_top: Some(top),
                        grid_bottom: Some(bottom),
                        style: "position:absolute; inset:0; width:100%; height:100%; display:block;".to_string(),
                    }
                    div { style: "position:absolute; inset:0; pointer-events:none; font-size:clamp(8px, 1.8vw, 10px); color:{theme.text_secondary}; text-shadow:0 1px 1px rgba(2,6,23,0.75);",
                        span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(top + 6.0, view_h)}; max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", {format!("{:.2}", y_max)} }
                        span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(top + inner_h / 2.0 + 4.0, view_h)}; transform:translateY(-50%); max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", {format!("{:.2}", y_mid)} }
                        span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(bottom + 1.0, view_h)}; transform:translateY(-100%); max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", {format!("{:.2}", y_min)} }
                        span { style: "position:absolute; left:{x_pct(left + CHART_X_LABEL_LEFT_INSET, view_w)}; top:{y_pct(x_label_top, view_h)};", {format!("-{:.1} min", span_min)} }
                        span { style: "position:absolute; left:{x_pct(view_w * 0.5, view_w)}; top:{y_pct(x_label_top, view_h)}; transform:translateX(-50%);", {format!("-{:.1} min", span_min * 0.5)} }
                        span { style: "position:absolute; left:{x_pct(right - 52.0, view_w)}; top:{y_pct(x_label_top, view_h)};", "{translate_text(\"now\")}" }
                    }
                }
            }

            div { style: "display:flex; flex-wrap:wrap; gap:8px; padding:6px 10px; background:rgba(2,6,23,0.75); border:1px solid {theme.border_soft}; border-radius:10px;",
                for (i, label) in labels.iter().enumerate() {
                    if !label.is_empty() {
                        div { style: "display:flex; align-items:center; gap:6px; font-size:12px; color:{theme.text_primary};",
                            SeriesSwatch { index: i }
                            "{translate_text(label)}"
                        }
                    }
                }
            }
        }
    }
}

fn default_series_label(data_layout: &DataTabLayout, spec: &ChartSeriesSpec) -> String {
    if let Some(label) = spec.label.as_ref()
        && !label.trim().is_empty()
    {
        return translate_text(label);
    }
    data_layout
        .tabs
        .iter()
        .find(|tab| tab.id == spec.data_type)
        .and_then(|tab| tab.channels.get(spec.index).cloned())
        .filter(|label| !label.is_empty())
        .map(|label| translate_text(&label))
        .unwrap_or_else(|| format!("{}[{}]", translate_text(&spec.data_type), spec.index))
}

fn combined_chart_payload(
    specs: &[ChartSeriesSpec],
    data_layout: &DataTabLayout,
    view_w: f64,
    view_h: f64,
    _grid_left: f32,
) -> Option<CombinedChartPayload> {
    let normalize_per_series = specs
        .iter()
        .map(|spec| spec.data_type.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        > 1;
    let labels = specs
        .iter()
        .map(|spec| default_series_label(data_layout, spec))
        .collect::<Vec<_>>();

    if !normalize_per_series {
        let data_type = specs.first()?.data_type.as_str();
        let channels = specs.iter().map(|spec| spec.index).collect::<Vec<_>>();
        let (chunks, y_min, y_max, span_min) =
            charts_cache_get_subset(data_type, &channels, view_w as f32, view_h as f32);
        if chunks.is_empty() {
            return None;
        }
        return Some((
            chunks.as_ref().clone(),
            y_min,
            y_max,
            span_min,
            labels,
            false,
            vec![None; specs.len()],
        ));
    }

    let cache_series = specs
        .iter()
        .map(|spec| (spec.data_type.clone(), spec.index))
        .collect::<Vec<_>>();
    let (chunks, series_scales, span_min) =
        charts_cache_get_multi_series_per_series(&cache_series, view_w as f32, view_h as f32);
    if chunks.is_empty() {
        return None;
    }

    Some((
        chunks.as_ref().clone(),
        0.0,
        1.0,
        span_min,
        labels,
        true,
        series_scales.as_ref().clone(),
    ))
}

fn combined_state_chart_cached(
    specs: &[ChartSeriesSpec],
    view_w: f64,
    view_h: f64,
    title: Option<&str>,
    data_layout: &DataTabLayout,
    state_chart_labels_vertical: bool,
    theme: &ThemeConfig,
) -> Element {
    let reseed_note = reseed_status_note();
    let Some((chunks, y_min, y_max, span_min, labels, normalize_per_series, series_scales)) =
        combined_chart_payload(specs, data_layout, view_w, view_h, COMBINED_CHART_GRID_LEFT)
    else {
        return rsx! {
            div { style: "width:100%; background:{theme.panel_background_alt}; border-radius:14px; border:1px solid {theme.border}; padding:12px; display:flex; flex-direction:column; gap:8px;",
                if let Some(t) = title {
                    div { style: "color:{theme.text_primary}; font-weight:700; font-size:14px;", "{translate_text(t)}" }
                }
                if let Some((kind, note)) = reseed_note.as_ref() {
                    {reseed_note_banner(kind, note, theme, false)}
                }
                div { style: "color:{theme.text_muted}; font-size:12px;", "{translate_text(\"No chart data yet.\")}" }
            }
        };
    };

    let left = COMBINED_CHART_GRID_LEFT as f64;
    let right = view_w - CHART_GRID_RIGHT_PAD;
    let top = CHART_GRID_TOP;
    let bottom = view_h - CHART_GRID_BOTTOM_PAD;
    let inner_h = bottom - top;
    let y_mid = (y_min + y_max) * 0.5;
    let visible_scale_count = series_scales.iter().filter(|scale| scale.is_some()).count();
    let scale_label_placements = if normalize_per_series && state_chart_labels_vertical {
        stacked_scale_label_placements(&series_scales, top, inner_h, view_h)
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
        series_scales
            .iter()
            .filter_map(|scale| *scale)
            .flat_map(|(series_min, series_max)| {
                [
                    format!("{:.2}", series_max).len(),
                    format!("{:.2}", (series_min + series_max) * 0.5).len(),
                    format!("{:.2}", series_min).len(),
                ]
            })
            .max()
            .unwrap_or(5)
    };
    let scale_chip_width = (widest_scale_label_chars as f64 * 5.6 + 12.0).clamp(34.0, 68.0);
    let side_rail_width = if normalize_per_series && state_chart_labels_vertical {
        let columns = scale_label_placements
            .iter()
            .map(|entry| entry.rail_column)
            .max()
            .unwrap_or(0)
            + 1;
        (columns as f64 * scale_chip_width)
            + (columns.saturating_sub(1) as f64 * VERTICAL_SCALE_LABEL_RAIL_GAP)
    } else if normalize_per_series {
        let count = visible_scale_count.max(1);
        (count as f64 * scale_chip_width) + (count.saturating_sub(1) as f64 * 4.0)
    } else {
        0.0
    };
    let rendered_chart_height = if normalize_per_series && state_chart_labels_vertical {
        let rows = scale_label_placements.len().max(1);
        CHART_GRID_TOP
            + CHART_GRID_BOTTOM_PAD
            + 28.0
            + rows.saturating_sub(1) as f64 * VERTICAL_SCALE_LABEL_ROW_GAP
    } else {
        0.0
    };
    let chart_shell_size_style = if normalize_per_series && state_chart_labels_vertical {
        format!("height:{rendered_chart_height}px;")
    } else {
        format!("aspect-ratio:{view_w}/{view_h};")
    };
    let x_label_top = bottom + CHART_X_LABEL_BOTTOM;
    let scale_chip_style = |i: usize| {
        format!(
            "box-sizing:border-box; min-width:{scale_chip_width}px; max-width:100%; padding:0 4px; line-height:1.1; font-size:clamp(8px, 1.8vw, 10px); text-align:center; font-variant-numeric:tabular-nums; border-radius:999px; border:1px solid {border}; background:{bg}; color:{fg}; \
             box-shadow: inset 0 0 0 1px rgba(255,255,255,0.04); text-shadow: 0 1px 1px rgba(2,6,23,0.85);",
            border = series_color(i),
            bg = theme.panel_background,
            fg = series_color(i),
            scale_chip_width = scale_chip_width,
        )
    };
    let x_pct = |x: f64, total: f64| format!("{:.4}%", (x / total) * 100.0);
    let y_pct = |y: f64, total: f64| format!("{:.4}%", (y / total) * 100.0);
    rsx! {
        div { style: "width:100%; background:{theme.panel_background_alt}; border-radius:14px; border:1px solid {theme.border}; padding:12px; display:flex; flex-direction:column; gap:8px;",
            if let Some(t) = title {
                div { style: "color:{theme.text_primary}; font-weight:700; font-size:14px;", "{translate_text(t)}" }
            }
            if let Some((kind, note)) = reseed_note.as_ref() {
                {reseed_note_banner(kind, note, theme, false)}
            }
            div { style: "display:flex; align-items:stretch; gap:{VERTICAL_SCALE_LABEL_RAIL_GAP}px; width:100%; {chart_shell_size_style} overflow:hidden;",
                if normalize_per_series {
                    {normalized_scale_labels_side(
                        &labels,
                        &series_scales,
                        side_rail_width,
                        top,
                        inner_h,
                        view_h,
                        state_chart_labels_vertical,
                        &scale_label_placements,
                        scale_chip_width,
                        &scale_chip_style,
                    )}
                }
                div { style: "position:relative; flex:1 1 auto; min-width:0; height:100%; container-type:inline-size;",
                    ChartCanvas {
                        view_w: view_w,
                        view_h: view_h,
                        chunks: chunks.into(),
                        grid_left: Some(left),
                        grid_right: Some(right),
                        grid_top: Some(top),
                        grid_bottom: Some(bottom),
                        style: "position:absolute; inset:0; width:100%; height:100%; display:block;".to_string(),
                    }
                    div { style: "position:absolute; inset:0; pointer-events:none; font-size:clamp(8px, 1.8vw, 10px); color:{theme.text_secondary}; text-shadow:0 1px 1px rgba(2,6,23,0.75);",
                        if !normalize_per_series {
                            span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(top + 6.0, view_h)}; max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", {format!("{:.2}", y_max)} }
                            span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(top + inner_h / 2.0 + 4.0, view_h)}; transform:translateY(-50%); max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", {format!("{:.2}", y_mid)} }
                            span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(bottom + 1.0, view_h)}; transform:translateY(-100%); max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", {format!("{:.2}", y_min)} }
                        }
                        span { style: "position:absolute; left:{x_pct(left + CHART_X_LABEL_LEFT_INSET, view_w)}; top:{y_pct(x_label_top, view_h)};", {format!("-{:.1} min", span_min)} }
                        span { style: "position:absolute; left:{x_pct(view_w * 0.5, view_w)}; top:{y_pct(x_label_top, view_h)}; transform:translateX(-50%);", {format!("-{:.1} min", span_min * 0.5)} }
                        span { style: "position:absolute; left:{x_pct(right - 52.0, view_w)}; top:{y_pct(x_label_top, view_h)};", "{translate_text(\"now\")}" }
                    }
                    if normalize_per_series && state_chart_labels_vertical {
                        {normalized_scale_connector_overlay(
                            &scale_label_placements,
                            left,
                            VERTICAL_SCALE_LABEL_RAIL_GAP,
                            scale_chip_width,
                            view_w,
                            view_h,
                        )}
                    }
                }
            }
            div { style: "display:flex; flex-wrap:wrap; gap:6px; padding:5px 10px; margin-top:-4px; background:{theme.panel_background}; border:1px solid {theme.border_soft}; border-radius:10px;",
                if normalize_per_series {
                    div { style: "font-size:11px; color:{theme.text_secondary}; margin-right:6px;", "{translate_text(\"Scaled per series\")}" }
                }
                for (i, label) in labels.iter().enumerate() {
                    if !label.is_empty() {
                        div { style: "display:flex; align-items:center; gap:5px; font-size:11px; color:{theme.text_primary};",
                            SeriesSwatch { index: i }
                            "{translate_text(label)}"
                        }
                    }
                }
            }
        }
    }
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
                        style: "position:absolute; right:{entry.rail_column as f64 * (scale_chip_width + VERTICAL_SCALE_LABEL_RAIL_GAP)}px; top:{pct(entry.label_y, view_h)}; transform:translateY(-50%); pointer-events:none; max-width:{scale_chip_width}px;",
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
    let row_gap = VERTICAL_SCALE_LABEL_ROW_GAP;
    let rows_per_column = (((max_y - min_y) / row_gap).floor() as usize + 1).max(1);
    let columns = entries.len().div_ceil(rows_per_column).max(1);
    let rows_in_column = entries.len().div_ceil(columns).max(1);
    let effective_gap = if rows_in_column > 1 {
        row_gap.min((max_y - min_y) / (rows_in_column as f64 - 1.0))
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
                    x1: "{-(rail_gap + entry.rail_column as f64 * (scale_chip_width + VERTICAL_SCALE_LABEL_RAIL_GAP))}",
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

fn labels_from_layout(data_layout: &DataTabLayout, dt: &str) -> Vec<String> {
    data_layout
        .tabs
        .iter()
        .find(|tab| tab.id == dt)
        .map(|tab| {
            tab.channels
                .iter()
                .map(|label| translate_text(label))
                .collect()
        })
        .unwrap_or_default()
}

fn fullscreen_view_height() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(win) = web_sys::window()
            && let Ok(height) = win.inner_height()
            && let Some(height) = height.as_f64()
        {
            return (height - 140.0).max(260.0);
        }
    }
    520.0
}

// ============================================================
// Existing StateTab helpers (mostly unchanged)
// ============================================================

fn valve_state_grid(
    data_type: Option<&str>,
    valves: Option<&[SummaryItem]>,
    colors: Option<&ValveColorSet>,
    labels: Option<&BooleanLabels>,
    valve_labels: Option<&[BooleanLabels]>,
    theme: &ThemeConfig,
    use_layout_theme_overrides: bool,
) -> Element {
    let Some(data_type) = data_type.filter(|dt| !dt.trim().is_empty()) else {
        return rsx! { div { style: "color:#94a3b8; font-size:12px;", "{translate_text(\"Missing valve data_type\")}" } };
    };
    let latest = latest_telemetry_row(data_type, None);

    let Some(row) = latest.as_ref() else {
        return rsx! { div { style: "color:#94a3b8; font-size:12px;", "No valve state yet." } };
    };

    let items: Vec<(String, Option<f32>)> = match valves {
        Some(list) if !list.is_empty() => list
            .iter()
            .map(|item| (item.label.clone(), value_at(row, item.index)))
            .collect(),
        _ => Vec::new(),
    };

    if items.is_empty() {
        return rsx! { div { style: "color:#94a3b8; font-size:12px;", "{translate_text(\"No valve items configured.\")}" } };
    }

    let (open, closed, unknown) = valve_colors(colors, theme, use_layout_theme_overrides);

    rsx! {
        div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(150px, 1fr)); gap:10px; margin-bottom:12px; width:100%; max-width:none; box-sizing:border-box; min-width:0;",
            for (idx, (label, value)) in items.iter().enumerate() {
                ValveStateCard {
                    label: translate_text(label),
                    value: *value,
                    open: open.clone(),
                    closed: closed.clone(),
                    unknown: unknown.clone(),
                    labels: widget_valve_labels_at(labels, valve_labels, idx),
                }
            }
        }
    }
}

#[component]
fn ValveStateCard(
    label: String,
    value: Option<f32>,
    open: ValveColor,
    closed: ValveColor,
    unknown: ValveColor,
    labels: Option<BooleanLabels>,
) -> Element {
    let true_label = labels
        .as_ref()
        .map(|l| l.true_label.as_str())
        .unwrap_or("Open");
    let false_label = labels
        .as_ref()
        .map(|l| l.false_label.as_str())
        .unwrap_or("Closed");
    let unknown_label = labels
        .as_ref()
        .and_then(|l| l.unknown_label.as_deref())
        .unwrap_or("Unknown");

    let (bg, border, fg, text) = match value {
        Some(v) if v >= 0.5 => (
            open.bg.as_str(),
            open.border.as_str(),
            open.fg.as_str(),
            true_label,
        ),
        Some(_) => (
            closed.bg.as_str(),
            closed.border.as_str(),
            closed.fg.as_str(),
            false_label,
        ),
        None => (
            unknown.bg.as_str(),
            unknown.border.as_str(),
            unknown.fg.as_str(),
            unknown_label,
        ),
    };

    rsx! {
        div { style: "padding:10px; border-radius:12px; background:{bg}; border:1px solid {border}; min-width:0; width:100%; box-sizing:border-box;",
            div { style: "font-size:12px; color:{fg};", "{translate_text(&label)}" }
            div { style: "font-size:18px; font-weight:700; color:{fg};", "{translate_text(text)}" }
        }
    }
}

fn valve_colors(
    colors: Option<&ValveColorSet>,
    theme: &ThemeConfig,
    use_layout_theme_overrides: bool,
) -> (ValveColor, ValveColor, ValveColor) {
    let default_open = ValveColor {
        bg: "#052e16".to_string(),
        border: "#22c55e".to_string(),
        fg: "#bbf7d0".to_string(),
    };
    let default_closed = ValveColor {
        bg: theme.panel_background_alt.clone(),
        border: theme.border.clone(),
        fg: theme.text_secondary.clone(),
    };
    let default_unknown = ValveColor {
        bg: theme.panel_background.clone(),
        border: theme.border_soft.clone(),
        fg: theme.text_muted.clone(),
    };

    let open = if use_layout_theme_overrides {
        colors.and_then(|c| c.open.clone()).unwrap_or(default_open)
    } else {
        default_open
    };
    let closed = if use_layout_theme_overrides {
        colors
            .and_then(|c| c.closed.clone())
            .unwrap_or(default_closed)
    } else {
        default_closed
    };
    let unknown = if use_layout_theme_overrides {
        colors
            .and_then(|c| c.unknown.clone())
            .unwrap_or(default_unknown)
    } else {
        default_unknown
    };
    (open, closed, unknown)
}

fn widget_valve_labels_at<'a>(
    default_labels: Option<&'a BooleanLabels>,
    valve_labels: Option<&'a [BooleanLabels]>,
    idx: usize,
) -> Option<BooleanLabels> {
    if let Some(list) = valve_labels
        && idx < list.len()
    {
        return Some(list[idx].clone());
    }
    default_labels.cloned()
}

fn action_section(
    actions: &[ActionSpec],
    action_policy: &ActionPolicyMsg,
    selection: Option<&[String]>,
    abort_only_mode: bool,
) -> Element {
    let blink_now_ms = blink_epoch_ms();
    let filtered = filter_actions(actions, selection);
    if filtered.is_empty() {
        return rsx! { div {} };
    }

    rsx! {
        div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 140px), 1fr)); gap:10px; align-items:stretch; width:100%; max-width:none; box-sizing:border-box; min-width:0;",
            for action in filtered.iter() {
                {
                    let control = action_policy.controls.iter().find(|c| c.cmd == action.cmd);
                    let enabled = action_policy.software_buttons_enabled
                        && auth::can_send_command(action.cmd.as_str())
                        && (!abort_only_mode || action.cmd == "Abort")
                        && control.map(|c| c.enabled).unwrap_or(action.cmd == "Abort");
                    let blink = control.map(|c| c.blink).unwrap_or(BlinkMode::None);
                    let actuated = control.and_then(|c| c.actuated);
                    rsx! {
                        button {
                            style: "{action_style(&action.border, &action.bg, &action.fg, blink_now_ms, enabled, blink, actuated)} min-width:0;",
                            disabled: !enabled,
                            onmousedown: {
                                let cmd = action.cmd.clone();
                                move |_| {
                                    if enabled {
                                        crate::telemetry_dashboard::send_cmd_from_press(&cmd)
                                    }
                                }
                            },
                            ontouchstart: {
                                let cmd = action.cmd.clone();
                                move |_| {
                                    if enabled {
                                        crate::telemetry_dashboard::send_cmd_from_press(&cmd)
                                    }
                                }
                            },
                            onclick: {
                                let cmd = action.cmd.clone();
                                move |_| {
                                    if enabled {
                                        crate::telemetry_dashboard::send_cmd_from_click(&cmd)
                                    }
                                }
                            },
                            "{translate_text(&action.label)}"
                        }
                    }
                }
            }
        }
    }
}

fn filter_actions<'a>(
    actions: &'a [ActionSpec],
    selection: Option<&[String]>,
) -> Vec<&'a ActionSpec> {
    let Some(selected) = selection else {
        return actions
            .iter()
            .filter(|action| action_is_visible(action))
            .collect();
    };
    if selected.is_empty() {
        return actions
            .iter()
            .filter(|action| action_is_visible(action))
            .collect();
    }
    let mut filtered = Vec::with_capacity(selected.len());
    for cmd in selected {
        if let Some(action) = actions
            .iter()
            .find(|a| &a.cmd == cmd && action_is_visible(a))
        {
            filtered.push(action);
        }
    }
    filtered
}

fn has_any_actions(
    actions: &[ActionSpec],
    selection: Option<&[String]>,
    abort_only_mode: bool,
) -> bool {
    let _ = abort_only_mode;
    auth::can_view_actions() && !filter_actions(actions, selection).is_empty()
}

fn action_is_visible(action: &ActionSpec) -> bool {
    let _ = action;
    auth::can_view_actions()
}

fn action_style(
    border: &str,
    bg: &str,
    fg: &str,
    blink_now_ms: u64,
    enabled: bool,
    blink: BlinkMode,
    actuated: Option<bool>,
) -> String {
    let cursor = if enabled { "pointer" } else { "not-allowed" };
    let recommended = enabled && blink != BlinkMode::None;
    let opacity = action_opacity(blink_now_ms, enabled, recommended, blink, actuated);
    let filter = if !enabled {
        "grayscale(0.25) brightness(0.9)"
    } else if actuated.unwrap_or(false) || recommended {
        "none"
    } else {
        "saturate(0.58) brightness(0.82)"
    };
    let box_shadow = if recommended || actuated.unwrap_or(false) {
        "0 10px 25px rgba(0,0,0,0.25)"
    } else {
        "0 4px 12px rgba(0,0,0,0.16)"
    };
    format!(
        "padding:0.6rem 0.9rem; border-radius:0.75rem; cursor:{cursor}; opacity:{opacity}; filter:{filter}; width:100%; \
         text-align:left; border:1px solid {border}; background:{bg}; color:{fg}; \
         font-weight:700; box-shadow:{box_shadow}; touch-action:manipulation;"
    )
}

fn summary_row(
    dt: Option<&str>,
    items: &[SummaryItem],
    fill_targets: Option<&FillTargetsConfig>,
    style: Option<&SummaryCardStyle>,
    theme: &ThemeConfig,
    use_layout_theme_overrides: bool,
    fill_parent_single: bool,
) -> Element {
    let want_minmax = dt.is_some();

    let (chan_min, chan_max) = if want_minmax {
        charts_cache_get_channel_minmax(dt.unwrap_or_default(), 1200.0, 300.0)
    } else {
        (Vec::new(), Vec::new())
    };

    let latest = items
        .iter()
        .map(|item| {
            (
                item.label.clone(),
                item.index,
                summary_item_value(dt, item, fill_targets),
                summary_item_fill_target_value_string(dt, item, fill_targets),
                item.formatter.as_ref(),
            )
        })
        .collect::<Vec<_>>();
    let grid_style = if latest.len() == 1 && !fill_parent_single {
        "display:grid; gap:4px; margin-bottom:0; grid-template-columns:minmax(0, min(220px, 100%)); justify-content:start; width:100%; min-width:0; align-items:stretch;".to_string()
    } else {
        let column_count = latest.len().clamp(1, 4);
        format!(
            "display:grid; gap:4px; margin-bottom:0; grid-template-columns:repeat({column_count}, minmax(0, 1fr)); width:100%; min-width:0; align-items:stretch;"
        )
    };

    rsx! {
        div { style: "{grid_style}",
            for (label, idx, value, target, formatter) in latest {
                SummaryCard {
                    label: translate_text(&label),
                    value: format_summary_value(value, formatter),
                    target: target,
                    min: if want_minmax { chan_min.get(idx).copied().flatten().map(|v| format_summary_value(Some(v), formatter)) } else { None },
                    max: if want_minmax { chan_max.get(idx).copied().flatten().map(|v| format_summary_value(Some(v), formatter)) } else { None },
                    style: style.cloned(),
                    theme: theme.clone(),
                    use_layout_theme_overrides: use_layout_theme_overrides,
                }
            }
        }
    }
}

#[component]
fn SummaryCard(
    label: String,
    value: String,
    target: Option<String>,
    min: Option<String>,
    max: Option<String>,
    style: Option<SummaryCardStyle>,
    theme: ThemeConfig,
    use_layout_theme_overrides: bool,
) -> Element {
    let mm = min_max_summary_text(min.as_deref(), max.as_deref());
    let background = themed_value(
        use_layout_theme_overrides,
        style.as_ref().and_then(|style| style.background.as_deref()),
        theme.panel_background_alt.as_str(),
    );
    let border = themed_value(
        use_layout_theme_overrides,
        style.as_ref().and_then(|style| style.border.as_deref()),
        theme.border.as_str(),
    );
    let label_color = themed_value(
        use_layout_theme_overrides,
        style
            .as_ref()
            .and_then(|style| style.label_color.as_deref()),
        theme.info_accent.as_str(),
    );
    let value_color = themed_value(
        use_layout_theme_overrides,
        style
            .as_ref()
            .and_then(|style| style.value_color.as_deref()),
        theme.text_primary.as_str(),
    );

    rsx! {
        div { style: "padding:5px 6px; border-radius:8px; background:{background}; border:1px solid {border}; width:100%; min-width:0; box-sizing:border-box;",
            div { style: "font-size:10px; line-height:1.0; color:{label_color};", "{translate_text(&label)}" }
            div { style: "display:flex; flex-direction:column; align-items:flex-start; gap:0; margin-top:1px; min-width:0; max-width:100%;",
                div { style: "font-size:14px; color:{value_color}; line-height:1.0; min-width:0; max-width:100%; overflow:hidden; text-overflow:clip; font-variant-numeric:tabular-nums; font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace;", "{value}" }
                if let Some(target) = target {
                    div { style: "font-size:9px; color:{theme.info_text}; min-width:0; max-width:100%; text-align:left; overflow:hidden; text-overflow:ellipsis; font-variant-numeric:tabular-nums; font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace;", "{target}" }
                }
            }
            if let Some(t) = mm {
                div { style: "font-size:9px; color:{theme.text_muted}; margin-top:1px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;", "{t}" }
            }
        }
    }
}

fn summary_item_value(
    dt: Option<&str>,
    item: &SummaryItem,
    _fill_targets: Option<&FillTargetsConfig>,
) -> Option<f32> {
    dt.and_then(|dt| latest_telemetry_value(dt, None, item.index))
}

fn summary_item_fill_target_source<'a>(
    dt: Option<&str>,
    item: &'a SummaryItem,
) -> Option<(&'a FillTargetFluid, &'a FillTargetValueKind)> {
    item.fill_target_fluid
        .as_ref()
        .zip(item.fill_target_kind.as_ref())
        .or_else(|| summary_item_fill_target_legacy_source(dt, item))
}

fn summary_item_fill_target_legacy_source(
    dt: Option<&str>,
    item: &SummaryItem,
) -> Option<(&'static FillTargetFluid, &'static FillTargetValueKind)> {
    static NITROUS: FillTargetFluid = FillTargetFluid::Nitrous;
    static PRESSURE: FillTargetValueKind = FillTargetValueKind::PressurePsi;
    static MASS: FillTargetValueKind = FillTargetValueKind::MassKg;

    match (dt, item.index) {
        (Some("FUEL_TANK_PRESSURE"), 0) => Some((&NITROUS, &PRESSURE)),
        (Some("LOADCELL_WEIGHT_KG"), 0) => Some((&NITROUS, &MASS)),
        _ => None,
    }
}

fn summary_item_fill_target_value(
    dt: Option<&str>,
    item: &SummaryItem,
    cfg: &FillTargetsConfig,
) -> Option<f32> {
    let (fluid, kind) = summary_item_fill_target_source(dt, item)?;
    let target = match fluid {
        FillTargetFluid::Nitrogen => &cfg.nitrogen,
        FillTargetFluid::Nitrous => &cfg.nitrous,
    };
    Some(match kind {
        FillTargetValueKind::MassKg => target.target_mass_kg,
        FillTargetValueKind::PressurePsi => target.target_pressure_psi,
    })
}

fn summary_item_fill_target_value_string(
    dt: Option<&str>,
    item: &SummaryItem,
    fill_targets: Option<&FillTargetsConfig>,
) -> Option<String> {
    let (_, kind) = summary_item_fill_target_source(dt, item)?;
    let label = match kind {
        FillTargetValueKind::MassKg => translate_text("Target"),
        FillTargetValueKind::PressurePsi => translate_text("Target"),
    };
    let Some(cfg) = fill_targets else {
        return Some(format!("{label} -"));
    };
    let raw = summary_item_fill_target_value(dt, item, cfg)?;
    let formatted = format_summary_value(Some(raw), item.formatter.as_ref());
    Some(format!("{label} {formatted}"))
}

fn value_at(row: &TelemetryRow, idx: usize) -> Option<f32> {
    row.values.get(idx).copied().flatten()
}

fn format_summary_value(v: Option<f32>, formatter: Option<&ValueFormatter>) -> String {
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
                ValueFormatKind::Number => format!("{x:.prec$}", prec = precision.unwrap_or(3)),
                ValueFormatKind::Integer => format!("{}", x.round() as i64),
            };
            format!("{prefix}{value}{suffix}")
        }
        None => "-".to_string(),
    }
}

fn board_status_table(boards: &[BoardStatusEntry], theme: &ThemeConfig) -> Element {
    if boards.is_empty() {
        return rsx! { div { style: "color:{theme.text_muted};", "No board status yet." } };
    }

    rsx! {
        div { style: "border:1px solid {theme.border_soft}; border-radius:10px; overflow:hidden;",
            div { style: "display:grid; grid-template-columns:1.4fr 0.8fr 0.6fr 0.8fr 0.8fr; background:{theme.app_background};",
                div { style: header_cell_style(theme), "Board" }
                div { style: header_cell_style(theme), "Sender ID" }
                div { style: header_cell_style(theme), "Seen" }
                div { style: header_cell_style(theme), "Last Seen (ms)" }
                div { style: header_cell_style(theme), "Age (ms)" }
            }
            for entry in boards.iter() {
                div { style: "display:grid; grid-template-columns:1.4fr 0.8fr 0.6fr 0.8fr 0.8fr; background:{theme.app_background};",
                    div { style: cell_style(theme), "{entry.display_name()}" }
                    div { style: cell_style(theme), "{entry.sender_id}" }
                    div { style: cell_style(theme), if entry.seen { "yes" } else { "no" } }
                    div { style: cell_style(theme), "{entry.last_seen_ms.map(|v| v.to_string()).unwrap_or_else(|| \"-\".into())}" }
                    div { style: cell_style(theme), "{entry.age_ms.map(|v| v.to_string()).unwrap_or_else(|| \"-\".into())}" }
                }
            }
        }
    }
}

fn header_cell_style(theme: &ThemeConfig) -> String {
    format!(
        "font-weight:600; color:{}; padding:8px; border-bottom:1px solid {}; border-right:1px solid {};",
        theme.text_secondary, theme.border_soft, theme.border_soft
    )
}

fn cell_style(theme: &ThemeConfig) -> String {
    format!(
        "padding:8px; border-bottom:1px solid {}; border-right:1px solid {}; color:{};",
        theme.border_soft, theme.border_soft, theme.text_primary
    )
}
