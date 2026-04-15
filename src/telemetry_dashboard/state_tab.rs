#![allow(clippy::too_many_arguments)]

// frontend/src/telemetry_dashboard/state_tab.rs

use dioxus::prelude::*;
use dioxus_signals::Signal;

use crate::auth;

use super::layout::{
    ActionSpec, ActionsTabLayout, BooleanLabels, ChartSeriesSpec, DataTabLayout, StateSection,
    StateSectionStyle, StateTabLayout, StateWidget, StateWidgetKind, SummaryCardStyle, SummaryItem,
    ThemeConfig, ValueFormatKind, ValueFormatter, ValveColor, ValveColorSet,
};
use super::types::{BoardStatusEntry, FlightState, TelemetryRow};
use super::{
    latest_telemetry_row, latest_telemetry_value, reseed_status_note, translate_text, ui_telemetry_rows_snapshot,
    ActionPolicyMsg, BlinkMode, CHART_RENDER_EPOCH, HISTORY_MS,
    TELEMETRY_RENDER_EPOCH,
};

use crate::telemetry_dashboard::data_chart::{
    charts_cache_get, charts_cache_get_channel_minmax, series_color, ChartCanvas,
    ChartRenderChunk, SeriesSwatch, CHART_GRID_BOTTOM_PAD, CHART_GRID_LEFT,
    CHART_GRID_RIGHT_PAD, CHART_GRID_TOP, CHART_X_LABEL_BOTTOM, CHART_X_LABEL_LEFT_INSET, CHART_Y_LABEL_LEFT,
    CHART_Y_LABEL_MAX_WIDTH,
};
use crate::telemetry_dashboard::map_tab::MapTab;
use std::hash::{Hash, Hasher};

const COMBINED_CURVE_MIN_DELTA_PX: f32 = 0.35;
const COMBINED_SMOOTHING_MAX_POINTS: usize = 240;
const COMBINED_CHART_GRID_LEFT: f32 = 24.0;

#[cfg(target_arch = "wasm32")]
fn blink_epoch_ms() -> u64 {
    js_sys::Date::now().max(0.0) as u64
}

#[cfg(not(target_arch = "wasm32"))]
fn blink_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn blink_opacity(blink_now_ms: u64, blink: BlinkMode, actuated: Option<bool>) -> Option<f32> {
    let (period_ms, dim, bright, invert) = match (blink, actuated.unwrap_or(false)) {
        (BlinkMode::None, _) => return None,
        (BlinkMode::Slow, false) => (1_800, 0.2, 1.0, false),
        (BlinkMode::Slow, true) => (1_800, 0.25, 1.0, true),
        (BlinkMode::Fast, false) => (600, 0.15, 1.0, false),
        (BlinkMode::Fast, true) => (600, 0.2, 1.0, true),
    };
    let phase = (blink_now_ms % period_ms) as f32 / period_ms as f32;
    let wave = 0.5 - 0.5 * f32::cos(std::f32::consts::TAU * phase);
    let pulse = if invert { 1.0 - wave } else { wave };
    Some(dim + (bright - dim) * pulse)
}

fn action_opacity(
    blink_now_ms: u64,
    enabled: bool,
    recommended: bool,
    blink: BlinkMode,
    actuated: Option<bool>,
) -> f32 {
    if !enabled {
        0.45
    } else if recommended {
        blink_opacity(blink_now_ms, blink, actuated).unwrap_or(1.0)
    } else if actuated.unwrap_or(false) {
        1.0
    } else {
        0.62
    }
}

#[component]
pub fn StateTab(
    flight_state: Signal<FlightState>,
    board_status: Signal<Vec<BoardStatusEntry>>,
    rocket_gps: Signal<Option<(f64, f64)>>,
    user_gps: Signal<Option<(f64, f64)>>,
    layout: StateTabLayout,
    data_layout: DataTabLayout,
    actions: ActionsTabLayout,
    action_policy: Signal<ActionPolicyMsg>,
    default_valve_labels: Option<BooleanLabels>,
    abort_only_mode: bool,
    theme: ThemeConfig,
    use_layout_theme_overrides: bool,
) -> Element {
    let _ = *TELEMETRY_RENDER_EPOCH.read();

    let state = flight_state.read().clone();
    let boards_snapshot = board_status.read();
    let actions_snapshot = actions.actions.clone();
    let action_policy_snapshot = action_policy.read().clone();

    let content = if let Some(state_layout) = layout
        .states
        .iter()
        .find(|entry| entry.states.iter().any(|configured| configured == &state))
    {
        rsx! {
            for section in state_layout.sections.iter() {
                {render_state_section(
                    section,
                    &boards_snapshot,
                    &data_layout,
                    &actions_snapshot,
                    &action_policy_snapshot,
                    default_valve_labels.as_ref(),
                    rocket_gps,
                    user_gps,
                    abort_only_mode,
                    &theme,
                    use_layout_theme_overrides,
                )}
            }
        }
    } else {
        rsx! { div { style: "color:{theme.text_muted}; font-size:12px;", "{translate_text(\"No layout for this flight state.\")}" } }
    };

    rsx! {
        div { style: "padding:16px; height:100%; overflow-y:auto; overflow-x:hidden; -webkit-overflow-scrolling:auto; display:flex; flex-direction:column; gap:16px; padding-bottom:100px;",
            h2 { style: "margin:0; color:{theme.text_primary};", "{translate_text(\"State\")}" }
            div { style: "padding:14px; border:1px solid {theme.border}; border-radius:14px; background:{theme.panel_background};",
                div { style: "font-size:14px; color:{theme.text_muted};", "{translate_text(\"Current Flight State\")}" }
                div { style: "font-size:22px; font-weight:700; margin-top:6px; color:{theme.text_primary};",
                    "{translate_text(&state.to_string())}"
                }
            }
            {content}
        }
    }
}

#[component]
fn Section(
    title: String,
    style: Option<StateSectionStyle>,
    theme: ThemeConfig,
    use_layout_theme_overrides: bool,
    children: Element,
) -> Element {
    let background = if use_layout_theme_overrides {
        style
            .as_ref()
            .and_then(|style| style.background.as_deref())
            .unwrap_or(theme.panel_background.as_str())
    } else {
        theme.panel_background.as_str()
    };
    let border = if use_layout_theme_overrides {
        style
            .as_ref()
            .and_then(|style| style.border.as_deref())
            .unwrap_or(theme.border.as_str())
    } else {
        theme.border.as_str()
    };
    let title_color = if use_layout_theme_overrides {
        style
            .as_ref()
            .and_then(|style| style.title_color.as_deref())
            .unwrap_or(theme.text_secondary.as_str())
    } else {
        theme.text_secondary.as_str()
    };

    rsx! {
        div { style: "padding:14px; border:1px solid {border}; border-radius:14px; background:{background};",
            div { style: "font-size:15px; color:{title_color}; font-weight:600; margin-bottom:10px;", "{translate_text(&title)}" }
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
    abort_only_mode: bool,
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

    rsx! {
        Section { title: title, style: section.style.clone(), theme: theme.clone(), use_layout_theme_overrides: use_layout_theme_overrides,
            for widget in section.widgets.iter() {
                {render_state_widget(
                    widget,
                    boards,
                    data_layout,
                    actions,
                    action_policy,
                    default_valve_labels,
                    rocket_gps,
                    user_gps,
                    abort_only_mode,
                    theme,
                    use_layout_theme_overrides,
                )}
            }
        }
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
    abort_only_mode: bool,
    theme: &ThemeConfig,
    use_layout_theme_overrides: bool,
) -> Element {
    match widget.kind {
        StateWidgetKind::BoardStatus => rsx! { {board_status_table(boards, theme)} },
        StateWidgetKind::Summary => {
            let dt = widget.data_type.as_deref().unwrap_or("");
            let items = widget.items.as_deref().unwrap_or(&[]);
            if dt.is_empty() {
                rsx! { div { style: "color:#94a3b8; font-size:12px;", "{translate_text(\"Missing summary data_type\")}" } }
            } else {
                rsx! { {summary_row(dt, items, widget.summary_style.as_ref(), theme, use_layout_theme_overrides)} }
            }
        }
        StateWidgetKind::Chart => {
            let w = widget.width.unwrap_or(1200.0);
            let h = widget.height.unwrap_or(260.0);
            rsx! {
                StateChartPanel {
                    widget: widget.clone(),
                    data_layout: data_layout.clone(),
                    theme: theme.clone(),
                    view_w: w,
                    view_h: h,
                }
            }
        }
        StateWidgetKind::ValveState => {
            let labels = widget.boolean_labels.as_ref().or(default_valve_labels);
            rsx! { {valve_state_grid(
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
        combined_state_chart_cached(
            series,
            view_w,
            if *is_fullscreen.read() {
                full_h
            } else {
                view_h
            },
            widget.chart_title.as_deref(),
            &data_layout,
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
    let x_label_bottom = CHART_X_LABEL_BOTTOM + 4.0;
    let x_pct = |x: f64, total: f64| format!("{:.4}%", (x / total) * 100.0);
    let y_pct = |y: f64, total: f64| format!("{:.4}%", (y / total) * 100.0);

    rsx! {
        div { style: "width:100%; background:{theme.panel_background_alt}; border-radius:14px; border:1px solid {theme.border}; padding:12px; display:flex; flex-direction:column; gap:8px;",
            if let Some(t) = title {
                div { style: "color:{theme.text_primary}; font-weight:700; font-size:14px;", "{translate_text(t)}" }
            }
            if let Some((kind, note)) = reseed_note.as_ref() {
                {
                    let (background, border, text) = match *kind {
                        "error" => (&theme.error_background, &theme.error_border, &theme.error_text),
                        "success" => (
                            &theme.notification_background,
                            &theme.notification_border,
                            &theme.notification_text,
                        ),
                        _ => (&theme.info_background, &theme.info_accent, &theme.info_text),
                    };
                    rsx! {
                        div { style: "padding:6px 8px; border-radius:8px; border:1px solid {border}; background:{background}; color:{text}; font-size:11px; line-height:1.35;",
                            "{translate_text(note)}"
                        }
                    }
                }
            }

            if chunks.is_empty() {
                div { style: "color:{theme.text_muted}; font-size:12px;", "{translate_text(\"No chart data yet.\")}" }
            } else {
                div { style: "position:relative; width:100%; aspect-ratio:{view_w}/{view_h};",
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
                    div { style: "position:absolute; inset:0; pointer-events:none; font-size:clamp(8px, 1.8vw, 10px); color:{theme.text_muted};",
                        span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(top + 6.0, view_h)}; max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", {format!("{:.2}", y_max)} }
                        span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(top + inner_h / 2.0 + 4.0, view_h)}; transform:translateY(-50%); max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", {format!("{:.2}", y_mid)} }
                        span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(bottom + 1.0, view_h)}; transform:translateY(-100%); max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", {format!("{:.2}", y_min)} }
                        span { style: "position:absolute; left:{x_pct(left + CHART_X_LABEL_LEFT_INSET, view_w)}; bottom:{x_label_bottom}px;", {format!("-{:.1} min", span_min)} }
                        span { style: "position:absolute; left:{x_pct(view_w * 0.5, view_w)}; bottom:{x_label_bottom}px; transform:translateX(-50%);", {format!("-{:.1} min", span_min * 0.5)} }
                        span { style: "position:absolute; left:{x_pct(right - 52.0, view_w)}; bottom:{x_label_bottom}px;", "{translate_text(\"now\")}" }
                    }
                }
            }

            div { style: "display:flex; flex-wrap:wrap; gap:8px; padding:6px 10px; background:{theme.app_background}; border:1px solid {theme.border_soft}; border-radius:10px;",
                for (i, label) in labels.iter().enumerate() {
                    if !label.is_empty() {
                        div { style: "display:flex; align-items:center; gap:6px; font-size:12px; color:{theme.text_secondary};",
                            SeriesSwatch { index: i }
                            "{translate_text(label)}"
                        }
                    }
                }
            }
        }
    }
}

fn push_curve_point(points: &mut Vec<(f32, f32)>, x: f32, y: f32) {
    if let Some((px, py)) = points.last().copied()
        && (px - x).abs() < COMBINED_CURVE_MIN_DELTA_PX
        && (py - y).abs() < COMBINED_CURVE_MIN_DELTA_PX
    {
        return;
    }
    points.push((x, y));
}

fn flush_curve_segment(path: &mut String, points: &[(f32, f32)], smooth: bool) {
    if points.is_empty() {
        return;
    }
    let (x0, y0) = points[0];
    path.push_str(&format!("M {:.2} {:.2} ", x0, y0));
    if points.len() == 1 {
        return;
    }
    if points.len() == 2 || !smooth || points.len() > COMBINED_SMOOTHING_MAX_POINTS {
        for &(x, y) in &points[1..] {
            path.push_str(&format!("L {:.2} {:.2} ", x, y));
        }
        return;
    }
    for i in 1..(points.len() - 1) {
        let (cx, cy) = points[i];
        let (nx, ny) = points[i + 1];
        let mx = (cx + nx) * 0.5;
        let my = (cy + ny) * 0.5;
        path.push_str(&format!("Q {:.2} {:.2} {:.2} {:.2} ", cx, cy, mx, my));
    }
    let (xl, yl) = points[points.len() - 1];
    path.push_str(&format!("L {:.2} {:.2} ", xl, yl));
}

fn padded_chart_range(mut min: f32, mut max: f32) -> (f32, f32) {
    if !min.is_finite() || !max.is_finite() {
        return (0.0, 1.0);
    }
    if min >= 0.0 {
        min = 0.0;
    }
    if max <= 0.0 {
        max = 0.0;
    }
    if (max - min).abs() < 1e-6 {
        let center = min;
        let pad = (center.abs() * 0.05).max(1.0);
        min = center - pad;
        max = center + pad;
    }
    let r = (max - min).abs().max(1e-6);
    let pad = (r * 0.06).max(1.0);
    (min - pad, max + pad)
}

fn zero_anchor_ratio(min: f32, max: f32) -> f32 {
    let neg = (-min).max(0.0);
    let pos = max.max(0.0);
    let total = neg + pos;
    if total <= 1e-6 {
        0.5
    } else {
        (neg / total).clamp(0.0, 1.0)
    }
}

fn anchored_series_range(min: f32, max: f32, zero_ratio: f32) -> (f32, f32) {
    let neg_needed = (-min).max(0.0);
    let pos_needed = max.max(0.0);
    let ratio = zero_ratio.clamp(0.05, 0.95);
    let span_from_neg = if ratio > 1e-6 {
        neg_needed / ratio
    } else {
        0.0
    };
    let span_from_pos = if (1.0 - ratio) > 1e-6 {
        pos_needed / (1.0 - ratio)
    } else {
        0.0
    };
    let mut span = span_from_neg.max(span_from_pos).max(1.0);
    if !span.is_finite() {
        span = 1.0;
    }
    let padding_scale = 1.06_f32;
    let padded_span = span * padding_scale;
    let range_min = -padded_span * ratio;
    let range_max = padded_span * (1.0 - ratio);
    (range_min, range_max)
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
) -> Option<(
    Vec<ChartRenderChunk>,
    f32,
    f32,
    f32,
    Vec<String>,
    bool,
    Vec<Option<(f32, f32)>>,
)> {
    let rows = ui_telemetry_rows_snapshot();
    let newest_ts = rows.iter().map(|row| row.timestamp_ms).max()?;
    let history_start_ts = newest_ts.saturating_sub(HISTORY_MS);

    let left = COMBINED_CHART_GRID_LEFT;
    let right = (view_w as f32 - CHART_GRID_RIGHT_PAD as f32).max(left + 1.0);
    let top = CHART_GRID_TOP as f32;
    let bottom = (view_h as f32 - CHART_GRID_BOTTOM_PAD as f32).max(top + 1.0);
    let pw = right - left;
    let ph = bottom - top;

    let mut all_points: Vec<Vec<(i64, f32)>> = Vec::with_capacity(specs.len());
    let mut series_ranges: Vec<Option<(f32, f32)>> = Vec::with_capacity(specs.len());
    let mut labels = Vec::with_capacity(specs.len());
    let mut raw_min = f32::INFINITY;
    let mut raw_max = f32::NEG_INFINITY;

    for spec in specs {
        let mut points: Vec<(i64, f32)> = rows
            .iter()
            .filter(|row| row.data_type == spec.data_type && row.timestamp_ms >= history_start_ts)
            .filter_map(|row| {
                row.values
                    .get(spec.index)
                    .copied()
                    .flatten()
                    .filter(|value| value.is_finite())
                    .map(|value| (row.timestamp_ms, value))
            })
            .collect();
        points.sort_by_key(|(ts, _)| *ts);
        points.dedup_by_key(|(ts, _)| *ts);

        let mut series_min = f32::INFINITY;
        let mut series_max = f32::NEG_INFINITY;
        if !points.is_empty() {
            for &(_, value) in &points {
                series_min = series_min.min(value);
                series_max = series_max.max(value);
                raw_min = raw_min.min(value);
                raw_max = raw_max.max(value);
            }
        }
        series_ranges.push(
            (series_min.is_finite() && series_max.is_finite()).then_some((series_min, series_max)),
        );
        labels.push(default_series_label(data_layout, spec));
        all_points.push(points);
    }

    if !raw_min.is_finite() || !raw_max.is_finite() {
        return None;
    }

    let oldest_ts = all_points
        .iter()
        .filter_map(|points| points.first().map(|(ts, _)| *ts))
        .min()
        .unwrap_or(newest_ts);
    let start_ts = oldest_ts;
    let span_ms = (newest_ts - start_ts).max(1) as f32;

    let (y_min, y_max) = padded_chart_range(raw_min, raw_max);
    let common_zero_ratio = zero_anchor_ratio(y_min, y_max);
    let normalize_per_series = specs
        .iter()
        .map(|spec| spec.data_type.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        > 1;
    let map_x = |ts_ms: i64| pw * ((ts_ms.saturating_sub(start_ts) as f32) / span_ms);

    let mut paths = vec![String::new(); specs.len()];
    let mut gap_paths = vec![String::new(); specs.len()];
    let smooth_curves = span_ms <= 5.0 * 60_000.0;

    for (idx, points) in all_points.iter().enumerate() {
        if points.is_empty() {
            continue;
        }
        let mut curve_points: Vec<(f32, f32)> = Vec::new();
        let mut min_gap_ms: Option<i64> = None;
        for window in points.windows(2) {
            let gap_ms = window[1].0.saturating_sub(window[0].0);
            if gap_ms > 0 {
                min_gap_ms = Some(min_gap_ms.map(|prev| prev.min(gap_ms)).unwrap_or(gap_ms));
            }
        }
        let gap_threshold_ms = min_gap_ms
            .map(|gap_ms| (gap_ms * 6).max(500))
            .unwrap_or(500);

        for (point_idx, (ts_ms, value)) in points.iter().enumerate() {
            let x = map_x(*ts_ms);
            let (series_y_min, series_y_max) = if normalize_per_series {
                series_ranges[idx]
                    .map(|(min, max)| anchored_series_range(min, max, common_zero_ratio))
                    .unwrap_or((y_min, y_max))
            } else {
                (y_min, y_max)
            };
            let y = bottom - (*value - series_y_min) / (series_y_max - series_y_min) * ph;

            if point_idx == 0 {
                push_curve_point(&mut curve_points, x, y);
                continue;
            }

            let (prev_ts_ms, prev_value) = points[point_idx - 1];
            let prev_x = map_x(prev_ts_ms);
            let prev_y = bottom - (prev_value - series_y_min) / (series_y_max - series_y_min) * ph;
            let gap_ms = ts_ms.saturating_sub(prev_ts_ms);
            if gap_ms > gap_threshold_ms {
                flush_curve_segment(&mut paths[idx], &curve_points, smooth_curves);
                curve_points.clear();
                gap_paths[idx].push_str(&format!(
                    "M {:.2} {:.2} L {:.2} {:.2} ",
                    prev_x, prev_y, x, y
                ));
            }
            push_curve_point(&mut curve_points, x, y);
        }
        flush_curve_segment(&mut paths[idx], &curve_points, smooth_curves);
    }

    if paths.iter().all(|path| path.is_empty()) && gap_paths.iter().all(|path| path.is_empty()) {
        return None;
    }

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    paths.hash(&mut hasher);
    gap_paths.hash(&mut hasher);
    for spec in specs {
        spec.data_type.hash(&mut hasher);
        spec.index.hash(&mut hasher);
        spec.label.hash(&mut hasher);
    }
    newest_ts.hash(&mut hasher);

    let chunks = vec![ChartRenderChunk {
        id: 0,
        x: left as f64,
        width: pw as f64,
        right: right as f64,
        paths,
        gap_paths,
        signature: hasher.finish(),
        live: true,
    }];

    Some((
        chunks,
        y_min,
        y_max,
        span_ms / 60_000.0,
        labels,
        normalize_per_series,
        series_ranges
            .into_iter()
            .map(|range| range.map(|(min, max)| anchored_series_range(min, max, common_zero_ratio)))
            .collect(),
    ))
}

fn combined_state_chart_cached(
    specs: &[ChartSeriesSpec],
    view_w: f64,
    view_h: f64,
    title: Option<&str>,
    data_layout: &DataTabLayout,
    theme: &ThemeConfig,
) -> Element {
    let reseed_note = reseed_status_note();
    let Some((chunks, y_min, y_max, span_min, labels, normalize_per_series, series_scales)) =
        combined_chart_payload(specs, data_layout, view_w, view_h)
    else {
        return rsx! {
            div { style: "width:100%; background:{theme.app_background}; border-radius:14px; border:1px solid {theme.border}; padding:12px; display:flex; flex-direction:column; gap:8px;",
                if let Some(t) = title {
                    div { style: "color:{theme.text_primary}; font-weight:700; font-size:14px;", "{translate_text(t)}" }
                }
                if let Some((kind, note)) = reseed_note.as_ref() {
                    {
                        let (background, border, text) = match *kind {
                            "error" => (&theme.error_background, &theme.error_border, &theme.error_text),
                            "success" => (
                                &theme.notification_background,
                                &theme.notification_border,
                                &theme.notification_text,
                            ),
                            _ => (&theme.info_background, &theme.info_accent, &theme.info_text),
                        };
                        rsx! {
                            div { style: "padding:6px 8px; border-radius:8px; border:1px solid {border}; background:{background}; color:{text}; font-size:11px; line-height:1.35;",
                                "{translate_text(note)}"
                            }
                        }
                    }
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
    let x_label_bottom = CHART_X_LABEL_BOTTOM + 4.0;
    let scale_chip_style = |i: usize| {
        format!(
            "padding:0 5px; line-height:1.15; border-radius:999px; border:1px solid {border}; background:{bg}; color:{fg}; \
             box-shadow: inset 0 0 0 1px rgba(255,255,255,0.04); text-shadow: 0 1px 1px rgba(2,6,23,0.85);",
            border = series_color(i),
            bg = theme.panel_background_alt,
            fg = series_color(i),
        )
    };
    let x_pct = |x: f64, total: f64| format!("{:.4}%", (x / total) * 100.0);
    let y_pct = |y: f64, total: f64| format!("{:.4}%", (y / total) * 100.0);
    rsx! {
        div { style: "width:100%; background:{theme.app_background}; border-radius:14px; border:1px solid {theme.border}; padding:12px; display:flex; flex-direction:column; gap:8px;",
            if let Some(t) = title {
                div { style: "color:{theme.text_primary}; font-weight:700; font-size:14px;", "{translate_text(t)}" }
            }
            if let Some((kind, note)) = reseed_note.as_ref() {
                {
                    let (background, border, text) = match *kind {
                        "error" => (&theme.error_background, &theme.error_border, &theme.error_text),
                        "success" => (
                            &theme.notification_background,
                            &theme.notification_border,
                            &theme.notification_text,
                        ),
                        _ => (&theme.info_background, &theme.info_accent, &theme.info_text),
                    };
                    rsx! {
                        div { style: "padding:6px 8px; border-radius:8px; border:1px solid {border}; background:{background}; color:{text}; font-size:11px; line-height:1.35;",
                            "{translate_text(note)}"
                        }
                    }
                }
            }
            div { style: "display:flex; gap:6px; align-items:stretch;",
                if normalize_per_series {
                    div { style: "flex:0 0 132px; width:132px; min-width:132px; display:flex; flex-direction:column; justify-content:space-between; align-items:flex-end; font-size:clamp(8px, 1.8vw, 10px); padding-top:4px; padding-bottom:28px; overflow:visible;",
                        div { style: "display:flex; justify-content:flex-end; flex-wrap:nowrap; gap:6px; white-space:nowrap; width:100%; text-align:right;",
                            for (i, _) in labels.iter().enumerate() {
                                if let Some((_, series_max)) = series_scales.get(i).and_then(|scale| *scale) {
                                    div { style: "{scale_chip_style(i)}", {format!("{:.2}", series_max)} }
                                }
                            }
                        }
                        div { style: "display:flex; justify-content:flex-end; flex-wrap:nowrap; gap:6px; white-space:nowrap; width:100%; text-align:right;",
                            for (i, _) in labels.iter().enumerate() {
                                if let Some((series_min, series_max)) = series_scales.get(i).and_then(|scale| *scale) {
                                    div { style: "{scale_chip_style(i)}", {format!("{:.2}", (series_min + series_max) * 0.5)} }
                                }
                            }
                        }
                        div { style: "display:flex; justify-content:flex-end; flex-wrap:nowrap; gap:6px; white-space:nowrap; width:100%; text-align:right;",
                            for (i, _) in labels.iter().enumerate() {
                                if let Some((series_min, _)) = series_scales.get(i).and_then(|scale| *scale) {
                                    div { style: "{scale_chip_style(i)}", {format!("{:.2}", series_min)} }
                                }
                            }
                        }
                    }
                }
                div { style: "position:relative; flex:1 1 auto; min-width:0; aspect-ratio:{view_w}/{view_h};",
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
                    div { style: "position:absolute; inset:0; pointer-events:none; font-size:clamp(8px, 1.8vw, 10px); color:{theme.text_muted};",
                        if !normalize_per_series {
                            span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(top + 6.0, view_h)}; max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", {format!("{:.2}", y_max)} }
                            span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(top + inner_h / 2.0 + 4.0, view_h)}; transform:translateY(-50%); max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", {format!("{:.2}", y_mid)} }
                            span { style: "position:absolute; left:{CHART_Y_LABEL_LEFT}px; top:{y_pct(bottom + 1.0, view_h)}; transform:translateY(-100%); max-width:{CHART_Y_LABEL_MAX_WIDTH}px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;", {format!("{:.2}", y_min)} }
                        }
                        span { style: "position:absolute; left:{x_pct(left + CHART_X_LABEL_LEFT_INSET, view_w)}; bottom:{x_label_bottom}px;", {format!("-{:.1} min", span_min)} }
                        span { style: "position:absolute; left:{x_pct(view_w * 0.5, view_w)}; bottom:{x_label_bottom}px; transform:translateX(-50%);", {format!("-{:.1} min", span_min * 0.5)} }
                        span { style: "position:absolute; left:{x_pct(right - 52.0, view_w)}; bottom:{x_label_bottom}px;", "{translate_text(\"now\")}" }
                    }
                }
            }
            div { style: "display:flex; flex-wrap:wrap; gap:6px; padding:4px 6px; background:{theme.app_background}; border:1px solid {theme.border_soft}; border-radius:10px;",
                if normalize_per_series {
                    div { style: "font-size:11px; color:{theme.text_muted}; margin-right:6px;", "{translate_text(\"Scaled per series\")}" }
                }
                for (i, label) in labels.iter().enumerate() {
                    if !label.is_empty() {
                        div { style: "display:flex; align-items:center; gap:5px; font-size:11px; color:{theme.text_secondary};",
                            SeriesSwatch { index: i }
                            "{translate_text(label)}"
                        }
                    }
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
    valves: Option<&[SummaryItem]>,
    colors: Option<&ValveColorSet>,
    labels: Option<&BooleanLabels>,
    valve_labels: Option<&[BooleanLabels]>,
    theme: &ThemeConfig,
    use_layout_theme_overrides: bool,
) -> Element {
    let latest = latest_telemetry_row("VALVE_STATE", None);

    let Some(row) = latest.as_ref() else {
        return rsx! { div { style: "color:#94a3b8; font-size:12px;", "No valve state yet." } };
    };

    let default_items = [
        SummaryItem {
            label: translate_text("Pilot"),
            index: 0,
            formatter: None,
        },
        SummaryItem {
            label: translate_text("NormallyOpen"),
            index: 1,
            formatter: None,
        },
        SummaryItem {
            label: translate_text("Dump"),
            index: 2,
            formatter: None,
        },
        SummaryItem {
            label: translate_text("Igniter"),
            index: 3,
            formatter: None,
        },
        SummaryItem {
            label: translate_text("Nitrogen"),
            index: 4,
            formatter: None,
        },
        SummaryItem {
            label: translate_text("Nitrous"),
            index: 5,
            formatter: None,
        },
        SummaryItem {
            label: translate_text("Fill Lines"),
            index: 6,
            formatter: None,
        },
    ];

    let items: Vec<(String, Option<f32>)> = match valves {
        Some(list) if !list.is_empty() => list
            .iter()
            .map(|item| (item.label.clone(), value_at(row, item.index)))
            .collect(),
        _ => default_items
            .iter()
            .map(|item| (item.label.clone(), value_at(row, item.index)))
            .collect(),
    };

    let (open, closed, unknown) = valve_colors(colors, theme, use_layout_theme_overrides);

    rsx! {
        div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(150px, 1fr)); gap:10px; margin-bottom:12px;",
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
        div { style: "padding:10px; border-radius:12px; background:{bg}; border:1px solid {border};",
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
        div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(180px, 1fr)); gap:10px;",
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
                            style: action_style(&action.border, &action.bg, &action.fg, blink_now_ms, enabled, blink, actuated),
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
    } else if actuated.unwrap_or(false) {
        "none"
    } else if recommended {
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
    dt: &str,
    items: &[SummaryItem],
    style: Option<&SummaryCardStyle>,
    theme: &ThemeConfig,
    use_layout_theme_overrides: bool,
) -> Element {
    let want_minmax = dt != "VALVE_STATE" && dt != "GPS_DATA";

    let (chan_min, chan_max) = if want_minmax {
        charts_cache_get_channel_minmax(dt, 1200.0, 300.0)
    } else {
        (Vec::new(), Vec::new())
    };

    let latest = items
        .iter()
        .map(|item| {
            (
                item.label.clone(),
                item.index,
                latest_value(dt, item.index),
                item.formatter.as_ref(),
            )
        })
        .collect::<Vec<_>>();

    rsx! {
        div { style: "display:grid; gap:10px; margin-bottom:12px; grid-template-columns:repeat(auto-fit, minmax(140px, 1fr)); width:100%;",
            for (label, idx, value, formatter) in latest {
                SummaryCard {
                    label: translate_text(&label),
                    value: format_summary_value(value, formatter),
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
    min: Option<String>,
    max: Option<String>,
    style: Option<SummaryCardStyle>,
    theme: ThemeConfig,
    use_layout_theme_overrides: bool,
) -> Element {
    let mm = match (min.as_deref(), max.as_deref()) {
        (Some(mi), Some(ma)) => Some(format!(
            "{} {mi} • {} {ma}",
            translate_text("min"),
            translate_text("max")
        )),
        _ => None,
    };
    let background = if use_layout_theme_overrides {
        style
            .as_ref()
            .and_then(|style| style.background.as_deref())
            .unwrap_or(theme.panel_background_alt.as_str())
    } else {
        theme.panel_background_alt.as_str()
    };
    let border = if use_layout_theme_overrides {
        style
            .as_ref()
            .and_then(|style| style.border.as_deref())
            .unwrap_or(theme.border.as_str())
    } else {
        theme.border.as_str()
    };
    let label_color = if use_layout_theme_overrides {
        style
            .as_ref()
            .and_then(|style| style.label_color.as_deref())
            .unwrap_or(theme.info_accent.as_str())
    } else {
        theme.info_accent.as_str()
    };
    let value_color = if use_layout_theme_overrides {
        style
            .as_ref()
            .and_then(|style| style.value_color.as_deref())
            .unwrap_or(theme.text_primary.as_str())
    } else {
        theme.text_primary.as_str()
    };

    rsx! {
        div { style: "padding:10px; border-radius:12px; background:{background}; border:1px solid {border}; width:100%; min-width:0; box-sizing:border-box;",
            div { style: "font-size:12px; color:{label_color};", "{translate_text(&label)}" }
            div { style: "font-size:18px; color:{value_color}; line-height:1.1;", "{value}" }
            if let Some(t) = mm {
                div { style: "font-size:11px; color:{theme.text_muted}; margin-top:4px;", "{t}" }
            }
        }
    }
}

fn latest_value(dt: &str, idx: usize) -> Option<f32> {
    latest_telemetry_value(dt, None, idx)
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
