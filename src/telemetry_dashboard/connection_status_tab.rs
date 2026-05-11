use dioxus::prelude::*;
use dioxus_signals::Signal;
use std::collections::HashMap;

use super::layout::{ConnectionSectionKind, ConnectionTabLayout, ThemeConfig};
use super::types::BoardStatusEntry;
use super::{
    current_wallclock_ms, js_eval, reseed_note_banner, reseed_status_note, translate_text,
};

const LATENCY_WINDOW_MS: i64 = 20 * 60_000;
const LATENCY_MAX_POINTS: usize = 1200;
const LATENCY_SMOOTHING_ALPHA: f64 = 0.25;
const SCROLL_TRIGGER_THRESHOLD_MS: i64 = 1_500;
const LATENCY_CHART_HEIGHT_PX: u32 = 220;
const LATENCY_FULLSCREEN_CHART_HEIGHT_PX: u32 = 240;
const LATENCY_PLOT_LEFT_PX: f64 = 74.0;
const LATENCY_PLOT_RIGHT_PX: f64 = 20.0;
const LATENCY_PLOT_TOP_PX: f64 = 20.0;
const LATENCY_PLOT_BOTTOM_PX: f64 = 34.0;
const SCROLL_SUPPRESSION_GRACE_MS: i64 = 2_500;

#[derive(Clone, Copy)]
struct LatencyPoint {
    timestamp_ms: i64,
    value_ms: f64,
    scroll_suppressed: bool,
}

#[component]
pub fn ConnectionStatusTab(
    boards: Signal<Vec<BoardStatusEntry>>,
    ws_connected: bool,
    expected_boards: Vec<String>,
    layout: ConnectionTabLayout,
    title: String,
    theme: ThemeConfig,
) -> Element {
    let mut show_board = use_signal(|| true);
    let mut board_fullscreen = use_signal(|| false);
    let mut show_latency = use_signal(|| true);
    let mut latency_fullscreen = use_signal(|| false);
    let history = use_signal(HashMap::<String, Vec<LatencyPoint>>::new);
    let previous_last_seen = use_signal(HashMap::<String, u64>::new);
    let smoothed_intervals = use_signal(HashMap::<String, f64>::new);
    let board_age_now_ms = use_signal(current_wallclock_ms);
    let merged_boards = merged_connection_boards(&boards.read(), &expected_boards);

    {
        let mut board_age_now_ms = board_age_now_ms;
        use_future(move || async move {
            loop {
                #[cfg(target_arch = "wasm32")]
                gloo_timers::future::TimeoutFuture::new(1_000).await;
                #[cfg(not(target_arch = "wasm32"))]
                tokio::time::sleep(std::time::Duration::from_millis(1_000)).await;
                board_age_now_ms.set(current_wallclock_ms());
            }
        });
    }

    {
        use_effect(move || {
            install_connection_scroll_pause_marker();
        });
    }

    {
        let expected_boards = expected_boards.clone();
        let mut history = history;
        let mut previous_last_seen = previous_last_seen;
        let mut smoothed_intervals = smoothed_intervals;
        use_effect(move || {
            let merged = merged_connection_boards(&boards.read(), &expected_boards);
            let sample_now_ms = current_wallclock_ms();
            spawn(async move {
                let scroll_suppressed = recent_scroll_pause_likely().await;
                let mut previous_map = previous_last_seen.write();
                let mut smoothing_map = smoothed_intervals.write();
                let mut history_map = history.write();

                for entry in &merged {
                    let sender_id = entry.sender_id.clone();
                    let Some(last_seen_ms) = entry.last_seen_ms else {
                        continue;
                    };

                    let sample_ts = i64::try_from(last_seen_ms).unwrap_or(sample_now_ms);
                    let maybe_gap = previous_map
                        .get(&sender_id)
                        .copied()
                        .and_then(|prev| last_seen_ms.checked_sub(prev))
                        .filter(|gap| *gap > 0);

                    if let Some(gap_ms) = maybe_gap {
                        let next_value =
                            if let Some(prev_smoothed) = smoothing_map.get(&sender_id).copied() {
                                prev_smoothed
                                    + (gap_ms as f64 - prev_smoothed) * LATENCY_SMOOTHING_ALPHA
                            } else {
                                gap_ms as f64
                            };
                        smoothing_map.insert(sender_id.clone(), next_value);

                        let list = history_map.entry(sender_id.clone()).or_default();
                        list.push(LatencyPoint {
                            timestamp_ms: sample_ts,
                            value_ms: next_value,
                            scroll_suppressed,
                        });
                        if let Some(newest) = list.last().map(|point| point.timestamp_ms) {
                            let cutoff = newest.saturating_sub(LATENCY_WINDOW_MS);
                            let split = list.partition_point(|point| point.timestamp_ms < cutoff);
                            if split > 0 {
                                list.drain(0..split);
                            }
                        }
                        if list.len() > LATENCY_MAX_POINTS {
                            let drain = list.len() - LATENCY_MAX_POINTS;
                            list.drain(0..drain);
                        }
                    }

                    previous_map.insert(sender_id, last_seen_ms);
                }
            });
        });
    }

    let toggle_latency = move |_| {
        let next = !*show_latency.read();
        show_latency.set(next);
    };
    let toggle_board = move |_| {
        let next = !*show_board.read();
        show_board.set(next);
    };
    let toggle_board_fullscreen = move |_| {
        let next = !*board_fullscreen.read();
        board_fullscreen.set(next);
    };
    let toggle_latency_fullscreen = move |_| {
        let next = !*latency_fullscreen.read();
        latency_fullscreen.set(next);
    };

    rsx! {
        div { style: "padding:16px; height:100%; overflow-y:auto; overflow-x:hidden; -webkit-overflow-scrolling:auto;",
            h2 { style: "margin:0 0 12px 0; color:{theme.text_primary};", "{title}" }
            for (idx, section) in layout.sections.iter().enumerate() {
                match section.kind {
                    ConnectionSectionKind::BoardStatus => rsx! {
                        div { style: {
                                let top_margin = if idx == 0 { "" } else { "margin-top:16px;" };
                                format!(
                                    "padding:14px; border:1px solid {}; border-radius:14px; background:{};{}",
                                    theme.border,
                                    theme.panel_background,
                                    top_margin
                                )
                            },
                            div { style: "display:flex; align-items:center; justify-content:space-between; gap:12px; margin-bottom:8px;",
                                div { style: "font-size:14px; color:{theme.text_muted};", "{translate_text(&section.title.clone().unwrap_or_else(|| \"Board Status\".to_string()))}" }
                                div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                                    button {
                                        style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                                        onclick: toggle_board,
                                        {if *show_board.read() { translate_text("Collapse") } else { translate_text("Expand") }}
                                    }
                                    button {
                                        style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                                        onclick: toggle_board_fullscreen,
                                        "{translate_text(\"Fullscreen\")}"
                                    }
                                }
                            }
                            if *show_board.read() {
                                {render_board_table(&merged_boards, *board_age_now_ms.read(), ws_connected, &theme)}
                            }
                        }
                    },
                    ConnectionSectionKind::Latency => rsx! {
                        div { style: {
                                let top_margin = if idx == 0 { "" } else { "margin-top:16px;" };
                                format!(
                                    "padding:14px; border:1px solid {}; border-radius:14px; background:{};{}",
                                    theme.border,
                                    theme.panel_background,
                                    top_margin
                                )
                            },
                            div { style: "display:flex; align-items:center; justify-content:space-between; gap:12px; margin-bottom:8px;",
                                div { style: "font-size:14px; color:{theme.text_muted};", "{translate_text(&section.title.clone().unwrap_or_else(|| \"Packet Interval (ms)\".to_string()))}" }
                                div { style: "display:flex; gap:8px; flex-wrap:wrap;",
                                    button {
                                        style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                                        onclick: toggle_latency,
                                        {if *show_latency.read() { translate_text("Collapse") } else { translate_text("Expand") }}
                                    }
                                    button {
                                        style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                                        onclick: toggle_latency_fullscreen,
                                        "{translate_text(\"Fullscreen\")}"
                                    }
                                }
                            }

                            if *show_latency.read() {
                                div { style: latency_list_style(),
                                    for entry in merged_boards.iter() {
                                        div { style: latency_card_style(&theme),
                                            div { style: "font-size:12px; color:{theme.text_muted}; margin-bottom:6px;",
                                                "{entry.display_name()} ({entry.sender_id})"
                                            }
                                            {render_latency_chart(
                                                history.read().get(&entry.sender_id),
                                                LATENCY_CHART_HEIGHT_PX as f64,
                                                &theme,
                                            )}
                                        }
                                    }
                                }
                            }
                        }
                    },
                }
            }
        }

        if *board_fullscreen.read() {
            div { style: "position:fixed; inset:0; z-index:9998; padding:16px; background:{theme.app_background}; display:flex; flex-direction:column; gap:12px; overflow:auto;",
                div { style: "display:flex; align-items:center; justify-content:space-between; gap:12px;",
                    h2 { style: "margin:0; color:{theme.text_secondary};", "{translate_text(\"Board Status\")}" }
                    button {
                        style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                        onclick: toggle_board_fullscreen,
                        "{translate_text(\"Exit Fullscreen\")}"
                    }
                }
                {render_board_table(&merged_boards, *board_age_now_ms.read(), ws_connected, &theme)}
            }
        }

        if *latency_fullscreen.read() {
            div { style: "position:fixed; inset:0; z-index:9998; padding:16px; background:{theme.app_background}; display:flex; flex-direction:column; gap:12px; overflow:auto;",
                div { style: "display:flex; align-items:center; justify-content:space-between; gap:12px;",
                    h2 { style: "margin:0; color:{theme.text_secondary};", "{translate_text(\"Packet Interval (ms)\")}" }
                    button {
                        style: "padding:6px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:0.85rem; cursor:pointer;",
                        onclick: toggle_latency_fullscreen,
                        "{translate_text(\"Exit Fullscreen\")}"
                    }
                }
                div { style: latency_list_style(),
                    for entry in merged_boards.iter() {
                        div { style: latency_card_style(&theme),
                            div { style: "font-size:12px; color:{theme.text_muted}; margin-bottom:6px;",
                                "{entry.display_name()} ({entry.sender_id})"
                            }
                            {render_latency_chart(
                                history.read().get(&entry.sender_id),
                                LATENCY_FULLSCREEN_CHART_HEIGHT_PX as f64,
                                &theme,
                            )}
                        }
                    }
                }
            }
        }
    }
}

fn merged_connection_boards(
    live_boards: &[BoardStatusEntry],
    expected_boards: &[String],
) -> Vec<BoardStatusEntry> {
    let mut by_sender = live_boards
        .iter()
        .cloned()
        .map(|entry| (entry.sender_id.clone(), entry))
        .collect::<HashMap<_, _>>();

    for sender_id in expected_boards {
        let sender_id = sender_id.trim();
        if sender_id.is_empty() || by_sender.contains_key(sender_id) {
            continue;
        }
        if let Some(entry) = BoardStatusEntry::from_sender_id(sender_id) {
            by_sender.insert(sender_id.to_string(), entry);
        }
    }

    let mut rows = by_sender.into_values().collect::<Vec<_>>();
    rows.sort_by(|a, b| a.display_name().cmp(b.display_name()));
    rows
}

fn render_latency_chart(
    points: Option<&Vec<LatencyPoint>>,
    height: f64,
    theme: &ThemeConfig,
) -> Element {
    let reseed_note = reseed_status_note();
    let Some(points) = points else {
        return latency_empty_state("No data yet", reseed_note.as_ref(), theme);
    };

    if points.len() < 2 {
        return latency_empty_state("Collecting...", reseed_note.as_ref(), theme);
    }

    let width = 1200.0_f64;
    let left = LATENCY_PLOT_LEFT_PX;
    let right = width - LATENCY_PLOT_RIGHT_PX;
    let pad_top = LATENCY_PLOT_TOP_PX;
    let pad_bottom = LATENCY_PLOT_BOTTOM_PX;
    let inner_w = right - left;
    let inner_h = height - pad_top - pad_bottom;
    let grid_x_step = inner_w / 6.0_f64;
    let grid_y_step = inner_h / 6.0_f64;
    let (solid, dotted, y_min, y_max, span_min) =
        build_latency_polylines(points.as_slice(), width, height, Some(LATENCY_WINDOW_MS));
    if solid.is_empty() && dotted.is_empty() {
        return latency_empty_state("Collecting...", reseed_note.as_ref(), theme);
    }

    let y_mid = (y_min + y_max) * 0.5;
    let x_pct = |x: f64, total: f64| format!("{:.4}%", (x / total) * 100.0);
    let y_pct = |y: f64, total: f64| format!("{:.4}%", (y / total) * 100.0);
    rsx! {
        div { style: "display:flex; flex-direction:column;",
            if let Some((kind, note)) = reseed_note.as_ref() {
                {reseed_note_banner(kind, note, theme, true)}
            }
            div { style: "position:relative; width:100%; aspect-ratio:{width}/{height};",
                svg {
                    style: "position:absolute; inset:0; width:100%; height:100%; display:block; background:{theme.app_background}; border-radius:10px; border:1px solid {theme.border_soft};",
                    view_box: "0 0 {width} {height}",

                    defs {
                        clipPath { id: "latency-plot-clip",
                            rect {
                                x: "{left}",
                                y: "{pad_top}",
                                width: "{inner_w}",
                                height: "{inner_h}",
                            }
                        }
                    }

                    // gridlines
                    for i in 1..=5 {
                        line {
                            x1:"{left}", y1:"{pad_top + grid_y_step * (i as f64)}",
                            x2:"{right}", y2:"{pad_top + grid_y_step * (i as f64)}",
                            stroke: "{theme.border_soft}",
                            "stroke-width": "1"
                        }
                    }
                    for i in 1..=5 {
                        line {
                            x1:"{left + grid_x_step * (i as f64)}", y1:"{pad_top}",
                            x2:"{left + grid_x_step * (i as f64)}", y2:"{height - pad_bottom}",
                            stroke: "{theme.border_soft}",
                            "stroke-width": "1"
                        }
                    }

                    // axes
                    line { x1:"{left}", y1:"{height - pad_bottom}", x2:"{right}", y2:"{height - pad_bottom}", stroke:"{theme.border}", "stroke-width":"1" }
                    line { x1:"{left}", y1:"{pad_top}",  x2:"{left}",   y2:"{height - pad_bottom}", stroke:"{theme.border}", "stroke-width":"1" }

                    g { "clip-path": "url(#latency-plot-clip)",
                        for pts in solid.iter() {
                            if !pts.is_empty() {
                                polyline {
                                    points: "{pts}",
                                    fill: "none",
                                    stroke: "#22d3ee",
                                    "stroke-width": "2",
                                    "stroke-linejoin": "round",
                                    "stroke-linecap": "round",
                                }
                            }
                        }
                        for pts in dotted.iter() {
                            if !pts.is_empty() {
                                polyline {
                                    points: "{pts}",
                                    fill: "none",
                                    stroke: "#fbbf24",
                                    "stroke-width": "2",
                                    stroke_dasharray: "4 4",
                                    "stroke-linejoin": "round",
                                    "stroke-linecap": "round",
                                }
                            }
                        }
                    }
                }
                div { style: "position:absolute; inset:0; pointer-events:none; font-size:clamp(8px, 1.8vw, 10px); color:{theme.text_muted};",
                    span { style: "position:absolute; left:8px; top:{y_pct(pad_top + 6.0, height)}; width:{x_pct(left - 16.0, width)}; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; text-align:right;", {format!("{:.2}", y_max)} }
                    span { style: "position:absolute; left:8px; top:{y_pct(pad_top + inner_h / 2.0 + 4.0, height)}; transform:translateY(-50%); width:{x_pct(left - 16.0, width)}; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; text-align:right;", {format!("{:.2}", y_mid)} }
                    span { style: "position:absolute; left:8px; top:{y_pct(height - pad_bottom + 2.0, height)}; transform:translateY(-100%); width:{x_pct(left - 16.0, width)}; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; text-align:right;", {format!("{:.2}", y_min)} }
                    span { style: "position:absolute; left:{x_pct(left + 16.0, width)}; bottom:8px;", {format!("-{:.1} min", span_min)} }
                    span { style: "position:absolute; left:{x_pct(width * 0.5, width)}; bottom:8px; transform:translateX(-50%);", {format!("-{:.1} min", span_min * 0.5)} }
                    span { style: "position:absolute; left:{x_pct(right - 52.0, width)}; bottom:8px;", "now" }
                }
            }
            div { style: "margin-top:8px; display:flex; gap:12px; align-items:center; font-size:12px; color:{theme.text_secondary};",
                div { style: "display:flex; align-items:center; gap:6px;",
                    svg { width:"26", height:"8", view_box:"0 0 26 8",
                        line { x1:"1", y1:"4", x2:"25", y2:"4", stroke:"#22d3ee", stroke_width:"2", stroke_linecap:"round" }
                    }
                    "Smoothed interval"
                }
                div { style: "display:flex; align-items:center; gap:6px;",
                    svg { width:"26", height:"8", view_box:"0 0 26 8",
                        line { x1:"1", y1:"4", x2:"25", y2:"4", stroke:"#fbbf24", stroke_width:"2", stroke_dasharray:"4 4", stroke_linecap:"round" }
                    }
                    "Gap bridge"
                }
            }
        }
    }
}

fn latency_empty_state(
    message: &str,
    reseed_note: Option<&(&'static str, String)>,
    theme: &ThemeConfig,
) -> Element {
    rsx! {
        div { style: "display:flex; flex-direction:column; gap:8px;",
            if let Some((kind, note)) = reseed_note {
                {reseed_note_banner(kind, note, theme, false)}
            }
            div { style: "color:{theme.text_muted}; font-size:12px;", "{translate_text(message)}" }
        }
    }
}

fn latency_list_style() -> &'static str {
    "display:flex; flex-direction:column; gap:10px; width:100%;"
}

fn latency_card_style(theme: &ThemeConfig) -> String {
    format!(
        "padding:10px; border:1px solid {}; border-radius:10px; background:{}; width:100%; min-width:0;",
        theme.border_soft, theme.panel_background_alt
    )
}

fn build_latency_polylines(
    points: &[LatencyPoint],
    width: f64,
    height: f64,
    window_ms: Option<i64>,
) -> (Vec<String>, Vec<String>, f64, f64, f64) {
    if points.len() < 2 {
        return (Vec::new(), Vec::new(), 0.0, 0.0, 0.0);
    }

    let mut pts: Vec<LatencyPoint> = points.to_vec();
    pts.sort_by_key(|point| point.timestamp_ms);

    if let Some(win) = window_ms
        && let Some(newest) = pts.last().map(|point| point.timestamp_ms)
    {
        let start = newest.saturating_sub(win);
        let first_in = pts.partition_point(|point| point.timestamp_ms < start);
        if first_in > 0 {
            pts.drain(0..first_in);
        }
    }

    if pts.len() < 2 {
        return (Vec::new(), Vec::new(), 0.0, 0.0, 0.0);
    }

    let (t_min, t_max) = pts.iter().fold((i64::MAX, i64::MIN), |(mn, mx), point| {
        (mn.min(point.timestamp_ms), mx.max(point.timestamp_ms))
    });
    let (y_min, y_max) = pts
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(mn, mx), point| {
            (mn.min(point.value_ms), mx.max(point.value_ms))
        });

    let t_span = (t_max - t_min).max(1) as f64;
    let mut y_span = y_max - y_min;
    if !y_span.is_finite() || y_span.abs() < 1e-9 {
        y_span = 1.0;
    }

    let pad_l = LATENCY_PLOT_LEFT_PX;
    let pad_r = LATENCY_PLOT_RIGHT_PX;
    let pad_t = LATENCY_PLOT_TOP_PX;
    let pad_b = LATENCY_PLOT_BOTTOM_PX;
    let inner_w = width - pad_l - pad_r;
    let inner_h = height - pad_t - pad_b;

    let to_xy = |t: i64, y: f64| -> (f64, f64) {
        let x = (pad_l + ((t - t_min) as f64 / t_span) * inner_w).clamp(pad_l, width - pad_r);
        let y_norm = (y - y_min) / y_span;
        let y_px = (pad_t + (1.0 - y_norm) * inner_h).clamp(pad_t, height - pad_b);
        (x, y_px)
    };

    let mut deltas: Vec<i64> = pts
        .windows(2)
        .map(|w| (w[1].timestamp_ms - w[0].timestamp_ms).max(0))
        .collect();
    deltas.sort_unstable();
    let median_dt = if deltas.is_empty() {
        0
    } else {
        deltas[deltas.len() / 2]
    };
    let gap_threshold_ms = median_dt.saturating_mul(5).max(SCROLL_TRIGGER_THRESHOLD_MS);

    let mut solid: Vec<String> = Vec::new();
    let mut dotted: Vec<String> = Vec::new();
    let mut cur_solid = String::new();

    for (idx, point) in pts.iter().enumerate() {
        let (x, yy) = to_xy(point.timestamp_ms, point.value_ms);
        if idx > 0 {
            let prev = pts[idx - 1];
            let dt = (point.timestamp_ms - prev.timestamp_ms).max(0);
            if dt > gap_threshold_ms {
                if !cur_solid.is_empty() {
                    solid.push(std::mem::take(&mut cur_solid));
                }
                if point.scroll_suppressed
                    && dt <= gap_threshold_ms.saturating_add(SCROLL_SUPPRESSION_GRACE_MS)
                {
                    let (x0, y0) = to_xy(prev.timestamp_ms, prev.value_ms);
                    dotted.push(format!("{x0:.2},{y0:.2} {x:.2},{yy:.2}"));
                }
            }
        }

        if !cur_solid.is_empty() {
            cur_solid.push(' ');
        }
        cur_solid.push_str(&format!("{x:.2},{yy:.2}"));
    }

    if !cur_solid.is_empty() {
        solid.push(cur_solid);
    }

    let span_min = t_span / 60_000.0;
    (solid, dotted, y_min, y_max, span_min)
}

fn install_connection_scroll_pause_marker() {
    js_eval(
        r#"
        (function() {
          if (window.__gs26_connection_scroll_pause_marker_installed) return;
          window.__gs26_connection_scroll_pause_marker_installed = true;
          const isIos = (() => {
            try {
              const ua = navigator.userAgent || "";
              const platform = navigator.platform || "";
              return /iPad|iPhone|iPod/i.test(ua)
                || /iPad|iPhone|iPod/i.test(platform)
                || (platform === "MacIntel" && navigator.maxTouchPoints > 1);
            } catch (e) {
              return false;
            }
          })();
          window.__gs26_connection_scroll_pause_supported = isIos ? "1" : "0";
          window.__gs26_connection_scroll_pause_until = 0;
          const mark = () => {
            if (!isIos) return;
            window.__gs26_connection_scroll_pause_until = Date.now() + 2500;
          };
          window.addEventListener("scroll", mark, { passive: true, capture: true });
          window.addEventListener("touchstart", mark, { passive: true, capture: true });
          window.addEventListener("touchmove", mark, { passive: true, capture: true });
          try {
            if (window.visualViewport) {
              window.visualViewport.addEventListener("scroll", mark, { passive: true });
            }
          } catch (e) {}
        })();
        "#,
    );
}

async fn recent_scroll_pause_likely() -> bool {
    let eval = document::eval(
        r#"
        (function() {
          try {
            if (String(window.__gs26_connection_scroll_pause_supported || "0") !== "1") {
              return "0";
            }
            const until = Number(window.__gs26_connection_scroll_pause_until || 0);
            return until > Date.now() ? "1" : "0";
          } catch (e) {
            return "0";
          }
        })()
        "#,
    );
    eval.join::<String>().await.ok().as_deref() == Some("1")
}

fn render_board_table(
    boards: &[BoardStatusEntry],
    now_ms: i64,
    ws_connected: bool,
    theme: &ThemeConfig,
) -> Element {
    if boards.is_empty() {
        return rsx! {
            div { style: "color:{theme.text_muted};", "No board status yet." }
        };
    }

    let header_cell_style = format!(
        "font-weight:600; color:{}; padding:8px; border-bottom:1px solid {}; background:{}; min-width:0; white-space:normal; overflow-wrap:anywhere; word-break:break-word; line-height:1.2;",
        theme.text_primary, theme.border_soft, theme.app_background
    );
    let text_cell_style = format!(
        "padding:8px; border-bottom:1px solid {}; background:{}; color:{}; min-width:0; white-space:normal; overflow-wrap:anywhere; word-break:break-word; line-height:1.25;",
        theme.border_soft, theme.app_background, theme.text_primary
    );
    let numeric_cell_style = format!(
        "padding:8px; border-bottom:1px solid {}; background:{}; color:{}; min-width:0; white-space:nowrap; font-variant-numeric:tabular-nums; font-family: ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace;",
        theme.border_soft, theme.app_background, theme.text_primary
    );
    let border_right = format!("border-right:1px solid {};", theme.border_soft);

    rsx! {
        div { style: "border:1px solid {theme.border_soft}; border-radius:10px; overflow-x:auto; overflow-y:hidden;",
            div { style: "display:grid; grid-template-columns:minmax(120px, 1.15fr) minmax(120px, 1.15fr) minmax(64px, 0.7fr) minmax(140px, 1fr) minmax(92px, 0.8fr); min-width:560px; font-size:13px; color:{theme.text_secondary}; background:{theme.app_background};",
                div { style: "{header_cell_style}{border_right}", "Board" }
                div { style: "{header_cell_style}{border_right}", "Sender ID" }
                div { style: "{header_cell_style}{border_right}", "Seen" }
                div { style: "{header_cell_style}{border_right}", "Last Seen (ms)" }
                div { style: "{header_cell_style}", "Age (ms)" }

                for entry in boards.iter() {
                    div { style: "{text_cell_style}{border_right}", "{entry.display_name()}" }
                    div { style: "{text_cell_style}{border_right}", "{entry.sender_id}" }
                    div { style: "{numeric_cell_style}{border_right}", if entry.seen { "yes" } else { "no" } }
                    div { style: "{numeric_cell_style}{border_right}",
                        "{format_last_seen(entry.last_seen_ms)}"
                    }
                    div { style: "{numeric_cell_style}",
                        if let Some(age) = current_board_age_ms(entry, now_ms, ws_connected) { "{age}" } else if !ws_connected { "disconnected" } else { "—" }
                    }
                }
            }
        }
    }
}

fn current_board_age_ms(entry: &BoardStatusEntry, now_ms: i64, ws_connected: bool) -> Option<u64> {
    if !ws_connected {
        return None;
    }
    if let Some(age_ms) = entry.age_ms {
        return Some(age_ms);
    }

    if let Some(last_seen_ms) = entry.last_seen_ms
        && last_seen_ms >= 1_500_000_000_000
    {
        let now_ms = u64::try_from(now_ms.max(0)).unwrap_or(0);
        return Some(now_ms.saturating_sub(last_seen_ms));
    }

    None
}

fn format_last_seen(last_seen_ms: Option<u64>) -> String {
    let Some(ts) = last_seen_ms else {
        return "—".to_string();
    };

    // Heuristic: if it's Unix-epoch ms (>= ~2017-07-14), render human time.
    if ts >= 1_500_000_000_000 {
        return super::format_timestamp_ms_local_datetime(ts as i64);
    }

    format!("{ts} ms")
}
