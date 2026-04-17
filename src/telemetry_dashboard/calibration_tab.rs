#![allow(clippy::redundant_locals)]

use super::{
    http_get_json, http_post_json, latest_telemetry_value, layout::ThemeConfig,
    persist, translate_text, TELEMETRY_RENDER_EPOCH,
};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct ChannelLinear {
    m: Option<f32>,
    b: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct FitMeta {
    #[serde(rename = "type")]
    fit_type: Option<String>,
    a: Option<f32>,
    b: Option<f32>,
    c: Option<f32>,
    d: Option<f32>,
    e: Option<f32>,
    x0: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct CalibrationPoint {
    expected: f32,
    raw: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
struct GenericCalibrationChannel {
    #[serde(default)]
    linear: ChannelLinear,
    #[serde(default)]
    zero_raw: Option<f32>,
    #[serde(default)]
    points: Vec<CalibrationPoint>,
    #[serde(default)]
    fit: Option<FitMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct CalibrationFile {
    full_mass_kg: Option<f32>,
    #[serde(default)]
    weights_kg: Vec<f32>,
    #[serde(default, rename = "extra_channels")]
    channels: BTreeMap<String, GenericCalibrationChannel>,
}

impl Default for CalibrationFile {
    fn default() -> Self {
        Self {
            full_mass_kg: Some(10.0),
            weights_kg: Vec::new(),
            channels: BTreeMap::new(),
        }
    }
}

#[derive(Serialize)]
struct RefitReq {
    channel: String,
    mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct CalibrationTabLayout {
    #[serde(default = "default_capture_target_samples")]
    pub(crate) capture_target_samples: usize,
    #[serde(default)]
    pub(crate) fit_modes: Vec<String>,
    #[serde(default)]
    pub(crate) sensors: Vec<CalibrationSensorSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct CalibrationSensorSpec {
    id: String,
    label: String,
    data_type: String,
    channel: String,
    #[serde(default)]
    fit_color: String,
    #[serde(default)]
    raw_label: String,
    #[serde(default)]
    expected_label: String,
    #[serde(default)]
    fit_modes: Vec<String>,
}

fn default_capture_target_samples() -> usize {
    200
}

const CALIBRATION_SELECTED_SENSOR_STORAGE_KEY: &str = "gs_calibration_selected_sensor";

#[derive(Clone, Copy, PartialEq, Eq)]
enum CaptureMode {
    SequenceZero,
    SequencePoint,
}

fn latest_raw(data_type: &str) -> Option<f32> {
    latest_telemetry_value(data_type, None, 0)
}

async fn sleep_ms(millis: u64) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(millis as u32).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
}

async fn capture_average_raw_sample<F>(
    data_type: String,
    samples: usize,
    mut on_progress: F,
) -> Result<(f32, usize), String>
where
    F: FnMut(usize, usize),
{
    let target = samples.clamp(1, 5_000);
    let mut captured_samples = Vec::with_capacity(target);
    for idx in 0..target {
        if let Some(raw) = latest_raw(&data_type) {
            captured_samples.push(raw);
        }
        let completed = idx + 1;
        if completed == 1 || completed == target || completed % 5 == 0 {
            on_progress(completed, target);
        }
        if idx + 1 < target {
            sleep_ms(20).await;
        }
    }
    if captured_samples.is_empty() {
        Err("No live raw samples were available during capture.".to_string())
    } else {
        captured_samples.sort_by(f32::total_cmp);
        const OUTLIER_FLOOR: f32 = 0.000001;
        const OUTLIER_SCALE: f32 = 3.5;
        const TRIM_RATIO: f32 = 0.12;
        let len = captured_samples.len();
        let median = if len % 2 == 0 {
            (captured_samples[len / 2 - 1] + captured_samples[len / 2]) * 0.5
        } else {
            captured_samples[len / 2]
        };
        let mut deviations: Vec<f32> = captured_samples
            .iter()
            .map(|value| (value - median).abs())
            .collect();
        deviations.sort_by(f32::total_cmp);
        let mad = if len % 2 == 0 {
            (deviations[len / 2 - 1] + deviations[len / 2]) * 0.5
        } else {
            deviations[len / 2]
        };
        let outlier_limit = (mad * OUTLIER_SCALE).max(OUTLIER_FLOOR);
        let mut filtered: Vec<f32> = captured_samples
            .iter()
            .copied()
            .filter(|value| (value - median).abs() <= outlier_limit)
            .collect();
        if filtered.is_empty() {
            filtered = captured_samples.clone();
        }
        filtered.sort_by(f32::total_cmp);
        let trim = ((filtered.len() as f32) * TRIM_RATIO).floor() as usize;
        let usable = if filtered.len() > trim * 2 + 2 {
            &filtered[trim..(filtered.len() - trim)]
        } else {
            filtered.as_slice()
        };
        let total: f32 = usable.iter().copied().sum();
        Ok((total / usable.len() as f32, usable.len()))
    }
}

fn fmt_fixed(v: Option<f32>, width: usize, prec: usize) -> String {
    match v {
        Some(x) => format!("{x:+width$.prec$}", width = width, prec = prec),
        None => "-".to_string(),
    }
}

fn sensors_from_layout(layout: &CalibrationTabLayout) -> Vec<CalibrationSensorSpec> {
    layout.sensors.clone()
}

fn channel_points_by_key(cfg: &CalibrationFile, channel: &str) -> Vec<(f32, f32)> {
    cfg.channels
        .get(channel)
        .map(|c| c.points.iter().map(|p| (p.raw, p.expected)).collect())
        .unwrap_or_default()
}

fn remove_point_by_key(cfg: &mut CalibrationFile, channel: &str, index: usize) -> bool {
    cfg.channels.get_mut(channel).is_some_and(|c| {
        if index < c.points.len() {
            c.points.remove(index);
            true
        } else {
            false
        }
    })
}

fn upsert_point_by_key(cfg: &mut CalibrationFile, channel: &str, expected: f32, raw: f32) {
    let expected = expected.max(0.0);
    let channel = cfg.channels.entry(channel.to_string()).or_default();
    if let Some(p) = channel
        .points
        .iter_mut()
        .find(|p| (p.expected - expected).abs() < 1e-6)
    {
        p.raw = raw;
    } else {
        channel.points.push(CalibrationPoint { expected, raw });
    }
}

fn matching_point_idx(points: &[(f32, f32)], expected: f32) -> Option<usize> {
    points
        .iter()
        .position(|(_, point_expected)| (*point_expected - expected).abs() < 1e-4)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis() as u64
}

fn reset_channel_by_key(cfg: &mut CalibrationFile, channel: &str) {
    cfg.channels.remove(channel);
}

fn fit_for_channel_key<'a>(cfg: &'a CalibrationFile, channel: &str) -> Option<&'a FitMeta> {
    cfg.channels.get(channel).and_then(|c| c.fit.as_ref())
}

fn linear_for_channel_key<'a>(
    cfg: &'a CalibrationFile,
    channel: &str,
) -> (&'a ChannelLinear, Option<&'a FitMeta>) {
    static DEFAULT_LINEAR: ChannelLinear = ChannelLinear {
        m: Some(1.0),
        b: Some(0.0),
    };
    cfg.channels
        .get(channel)
        .map(|c| (&c.linear, c.fit.as_ref()))
        .unwrap_or((&DEFAULT_LINEAR, None))
}

fn eval_fit_key(cfg: &CalibrationFile, channel: &str, raw: f32) -> Option<f32> {
    let (linear, fit) = linear_for_channel_key(cfg, channel);
    eval_fit_parts(linear, fit, raw)
}

fn eval_fit_parts(linear: &ChannelLinear, fit: Option<&FitMeta>, raw: f32) -> Option<f32> {
    let fit_type = fit.and_then(|f| f.fit_type.as_deref());
    if let Some(meta) = fit {
        let x = raw - meta.x0.unwrap_or(0.0);
        if fit_type == Some("poly4") {
            return Some(
                meta.a? * x.powi(4)
                    + meta.b? * x.powi(3)
                    + meta.c.unwrap_or(0.0) * x * x
                    + meta.d.unwrap_or(0.0) * x
                    + meta.e.unwrap_or(0.0),
            );
        }
        if fit_type == Some("poly3") {
            return Some(
                meta.a? * x * x * x
                    + meta.b? * x * x
                    + meta.c.unwrap_or(0.0) * x
                    + meta.d.unwrap_or(0.0),
            );
        }
        if fit_type == Some("poly2") {
            return Some(meta.a? * x * x + meta.b? * x + meta.c.unwrap_or(0.0));
        }
    }
    Some(linear.m? * raw + linear.b.unwrap_or(0.0))
}

fn fit_details_text_key(cfg: &CalibrationFile, channel: &str) -> Option<String> {
    fit_details_text_parts(linear_for_channel_key(cfg, channel))
}

fn fit_details_text_parts((linear, fit): (&ChannelLinear, Option<&FitMeta>)) -> Option<String> {
    let fit_type = fit.and_then(|f| f.fit_type.as_deref()).unwrap_or("linear");

    match fit_type {
        "poly4" => {
            let meta = fit?;
            Some(format!(
                "a={} b={} c={} d={} e={} x0={}",
                fmt_fixed(meta.a, 10, 4),
                fmt_fixed(meta.b, 10, 4),
                fmt_fixed(meta.c, 10, 4),
                fmt_fixed(meta.d, 10, 4),
                fmt_fixed(meta.e, 10, 4),
                fmt_fixed(meta.x0, 10, 4)
            ))
        }
        "poly3" => {
            let meta = fit?;
            Some(format!(
                "a={} b={} c={} d={} x0={}",
                fmt_fixed(meta.a, 10, 4),
                fmt_fixed(meta.b, 10, 4),
                fmt_fixed(meta.c, 10, 4),
                fmt_fixed(meta.d, 10, 4),
                fmt_fixed(meta.x0, 10, 4)
            ))
        }
        "poly2" => {
            let meta = fit?;
            Some(format!(
                "a={} b={} c={} x0={}",
                fmt_fixed(meta.a, 10, 4),
                fmt_fixed(meta.b, 10, 4),
                fmt_fixed(meta.c, 10, 4),
                fmt_fixed(meta.x0, 10, 4)
            ))
        }
        _ => {
            if linear.m.is_none() && linear.b.is_none() && fit.and_then(|f| f.x0).is_none() {
                return None;
            }
            Some(format!(
                "m={} b={} x0={}",
                fmt_fixed(linear.m, 10, 4),
                fmt_fixed(linear.b, 10, 4),
                fmt_fixed(fit.and_then(|f| f.x0), 10, 4)
            ))
        }
    }
}

#[component]
pub fn CalibrationTab(theme: ThemeConfig, can_edit: bool, capture_sample_count: usize) -> Element {
    let _ = *TELEMETRY_RENDER_EPOCH.read();
    let layout_cfg = use_signal(|| None::<CalibrationTabLayout>);
    let sensors = layout_cfg
        .read()
        .as_ref()
        .map(sensors_from_layout)
        .unwrap_or_default();
    let layout_capture_target = layout_cfg
        .read()
        .as_ref()
        .map(|v| v.capture_target_samples)
        .unwrap_or_else(default_capture_target_samples)
        .max(10);
    let effective_capture_sample_count = if capture_sample_count > 0 {
        capture_sample_count
    } else {
        layout_capture_target
    };

    let cfg = use_signal(|| None::<CalibrationFile>);
    let selected_sensor_id = use_signal(|| {
        persist::get_string(CALIBRATION_SELECTED_SENSOR_STORAGE_KEY).unwrap_or_default()
    });
    let fit_mode = use_signal(String::new);
    let known_kg = use_signal(|| "1.0".to_string());
    let manual_kg = use_signal(|| "1.0".to_string());
    let manual_raw = use_signal(String::new);
    let selected_point_idx = use_signal(|| None::<usize>);
    let inspected_point_idx = use_signal(|| None::<usize>);
    let status = use_signal(|| "Loading calibration...".to_string());
    let dirty = use_signal(|| false);
    let manual_capture_progress = use_signal(String::new);
    let manual_capture_progress_epoch = use_signal(|| 0u64);
    let last_sync_poll_ms = use_signal(now_ms);
    let sequence_dialog_open = use_signal(|| false);
    let sequence_dialog_mode = use_signal(|| CaptureMode::SequencePoint);
    let sequence_dialog_weight = use_signal(|| "1.0".to_string());
    let sequence_dialog_captured_raw = use_signal(String::new);
    let sequence_dialog_status = use_signal(String::new);
    let sequence_dialog_replace_existing = use_signal(|| false);
    let sequence_dialog_confirm_reset = use_signal(|| false);

    {
        let mut layout_cfg = layout_cfg;
        let mut status = status;
        use_effect(move || {
            spawn(async move {
                match http_get_json::<CalibrationTabLayout>("/api/calibration_config").await {
                    Ok(v) => layout_cfg.set(Some(v)),
                    Err(e) => status.set(format!(
                        "Failed to load calibration config, using defaults: {e}"
                    )),
                }
            });
        });
    }

    {
        let sensors = sensors.clone();
        let layout_cfg = layout_cfg;
        let mut selected_sensor_id = selected_sensor_id;
        use_effect(move || {
            if layout_cfg.read().is_none() {
                return;
            }
            let cur = selected_sensor_id.read().clone();
            if sensors.iter().any(|s| s.id == cur) {
                return;
            }
            if let Some(first) = sensors.first() {
                selected_sensor_id.set(first.id.clone());
            } else {
                selected_sensor_id.set(String::new());
                persist::_remove(CALIBRATION_SELECTED_SENSOR_STORAGE_KEY);
            }
        });
    }
    {
        let selected_sensor_id = selected_sensor_id;
        use_effect(move || {
            let sensor_id = selected_sensor_id.read().clone();
            if sensor_id.trim().is_empty() {
                return;
            }
            persist::set_string(CALIBRATION_SELECTED_SENSOR_STORAGE_KEY, &sensor_id);
        });
    }

    {
        let mut cfg = cfg;
        let mut status = status;
        use_effect(move || {
            spawn(async move {
                match http_get_json::<CalibrationFile>("/api/calibration").await {
                    Ok(v) => {
                        cfg.set(Some(v));
                        status.set("Calibration loaded".to_string());
                    }
                    Err(e) => status.set(format!("Failed to load: {e}")),
                }
            });
        });
    }

    {
        let mut manual_capture_progress = manual_capture_progress;
        let manual_capture_progress_epoch = manual_capture_progress_epoch;
        use_effect(move || {
            let message = manual_capture_progress.read().clone();
            if message.trim().is_empty() {
                return;
            }
            let epoch = *manual_capture_progress_epoch.read();
            spawn(async move {
                sleep_ms(5_000).await;
                if *manual_capture_progress_epoch.read() == epoch {
                    manual_capture_progress.set(String::new());
                }
            });
        });
    }

    {
        let mut cfg = cfg;
        let mut status = status;
        let mut last_sync_poll_ms = last_sync_poll_ms;
        let sequence_dialog_open = sequence_dialog_open;
        let dirty = dirty;
        use_effect(move || {
            if *sequence_dialog_open.read() || *dirty.read() {
                return;
            }
            let now = now_ms();
            let last = *last_sync_poll_ms.read();
            if now.saturating_sub(last) < 1_500 {
                return;
            }
            last_sync_poll_ms.set(now);
            let current_cfg = cfg.read().clone();
            spawn(async move {
                match http_get_json::<CalibrationFile>("/api/calibration").await {
                    Ok(remote_cfg) => {
                        if current_cfg.as_ref() != Some(&remote_cfg) {
                            cfg.set(Some(remote_cfg));
                            status.set("Calibration synced from backend".to_string());
                        }
                    }
                    Err(err) => {
                        if current_cfg.is_none() {
                            status.set(format!("Failed to load: {err}"));
                        }
                    }
                }
            });
        });
    }

    let selected_id = selected_sensor_id.read().clone();
    let selected_sensor = sensors
        .iter()
        .find(|s| s.id == selected_id)
        .cloned()
        .or_else(|| sensors.first().cloned());
    if sensors.is_empty() {
        return rsx! {};
    }
    let channel_key = selected_sensor
        .as_ref()
        .map(|s| s.channel.clone())
        .unwrap_or_default();
    let effective_selected_sensor_id = selected_sensor
        .as_ref()
        .map(|s| s.id.clone())
        .unwrap_or_default();
    let fit_modes = selected_sensor
        .as_ref()
        .map(|s| {
            if s.fit_modes.is_empty() {
                layout_cfg
                    .read()
                    .as_ref()
                    .map(|layout| layout.fit_modes.clone())
                    .filter(|modes| !modes.is_empty())
                    .unwrap_or_default()
            } else {
                s.fit_modes.clone()
            }
        })
        .unwrap_or_default();
    {
        let fit_modes = fit_modes.clone();
        let cfg = cfg;
        let channel_key = channel_key.clone();
        let mut fit_mode = fit_mode;
        use_effect(move || {
            let current = fit_mode.read().clone();
            if fit_modes.iter().any(|m| m == &current) {
                return;
            }
            let seeded = cfg
                .read()
                .as_ref()
                .and_then(|cfg| fit_for_channel_key(cfg, &channel_key))
                .and_then(|fit| fit.fit_type.clone());
            if let Some(seeded_mode) =
                seeded.filter(|mode| fit_modes.iter().any(|candidate| candidate == mode))
            {
                fit_mode.set(seeded_mode);
                return;
            }
            if let Some(first) = fit_modes.first() {
                fit_mode.set(first.clone());
            }
        });
    }
    let points = cfg
        .read()
        .as_ref()
        .map(|c| channel_points_by_key(c, &channel_key))
        .unwrap_or_default();
    {
        let points = points.clone();
        let mut selected_point_idx = selected_point_idx;
        let manual_kg = manual_kg;
        use_effect(move || {
            let Some(idx) = *selected_point_idx.read() else {
                return;
            };
            let Some((_, expected)) = points.get(idx).copied() else {
                selected_point_idx.set(None);
                return;
            };
            let kg_matches = manual_kg
                .read()
                .parse::<f32>()
                .ok()
                .is_some_and(|value| (value - expected).abs() <= 0.0001);
            if !kg_matches {
                selected_point_idx.set(None);
            }
        });
    }
    {
        let points = points.clone();
        let manual_kg = manual_kg;
        let mut selected_point_idx = selected_point_idx;
        use_effect(move || {
            let Ok(kg) = manual_kg.read().parse::<f32>() else {
                return;
            };
            let next_idx = matching_point_idx(&points, kg);
            if *selected_point_idx.read() != next_idx {
                selected_point_idx.set(next_idx);
            }
        });
    }
    let raw_live = selected_sensor
        .as_ref()
        .and_then(|s| latest_raw(s.data_type.as_str()));
    let sequence_started = cfg.read().as_ref().is_some_and(|c| {
        c.channels
            .get(&channel_key)
            .and_then(|channel| channel.zero_raw)
            .is_some()
    });
    let calibrated_live = cfg
        .read()
        .as_ref()
        .and_then(|c| raw_live.and_then(|raw| eval_fit_key(c, &channel_key, raw)));
    let raw_live_s = fmt_fixed(raw_live, 12, 6);
    let calibrated_live_s = fmt_fixed(calibrated_live, 12, 4);
    let fit_type_s = cfg
        .read()
        .as_ref()
        .and_then(|c| fit_for_channel_key(c, &channel_key))
        .and_then(|f| f.fit_type.clone())
        .unwrap_or_else(|| "linear".to_string());
    let fit_meta_text = cfg
        .read()
        .as_ref()
        .and_then(|c| fit_details_text_key(c, &channel_key));
    let fit_equation_text = fit_meta_text
        .clone()
        .unwrap_or_else(|| format!("{}={}", translate_text("type"), translate_text(&fit_type_s)));
    let fit_color = selected_sensor
        .as_ref()
        .map(|sensor| sensor.fit_color.as_str())
        .filter(|color| !color.trim().is_empty())
        .unwrap_or("#22d3ee");

    let plot_w = 900.0_f32;
    let plot_h = 260.0_f32;
    let pad_l = 56.0_f32;
    let pad_r = 14.0_f32;
    let pad_t = 14.0_f32;
    let pad_b = 28.0_f32;
    let mut x_min = points
        .iter()
        .map(|(x, _)| *x)
        .min_by(f32::total_cmp)
        .unwrap_or(0.0);
    let mut x_max = points
        .iter()
        .map(|(x, _)| *x)
        .max_by(f32::total_cmp)
        .unwrap_or(1.0);
    let mut y_min = points
        .iter()
        .map(|(_, y)| *y)
        .min_by(f32::total_cmp)
        .unwrap_or(0.0);
    let mut y_max = points
        .iter()
        .map(|(_, y)| *y)
        .max_by(f32::total_cmp)
        .unwrap_or(1.0);
    if (x_max - x_min).abs() < 1e-6 {
        x_min -= 1.0;
        x_max += 1.0;
    }
    if (y_max - y_min).abs() < 1e-6 {
        y_min -= 1.0;
        y_max += 1.0;
    }
    let x_pad = (x_max - x_min) * 0.1;
    let y_pad = (y_max - y_min) * 0.15;
    x_min -= x_pad;
    x_max += x_pad;
    y_min -= y_pad;
    y_max += y_pad;
    let sx =
        |x: f32| pad_l + ((x - x_min) / (x_max - x_min)).clamp(0.0, 1.0) * (plot_w - pad_l - pad_r);
    let sy = |y: f32| {
        pad_t + (1.0 - ((y - y_min) / (y_max - y_min)).clamp(0.0, 1.0)) * (plot_h - pad_t - pad_b)
    };
    let scatter_xy: Vec<(f32, f32)> = points.iter().map(|(x, y)| (sx(*x), sy(*y))).collect();
    let fit_path = cfg
        .read()
        .as_ref()
        .map(|c| {
            let samples = 80;
            let mut d = String::new();
            for i in 0..samples {
                let t = i as f32 / (samples - 1) as f32;
                let x = x_min + t * (x_max - x_min);
                if let Some(y) = eval_fit_key(c, &channel_key, x) {
                    let cmd = if d.is_empty() { "M" } else { "L" };
                    d.push_str(&format!("{cmd}{:.2},{:.2} ", sx(x), sy(y)));
                }
            }
            d
        })
        .unwrap_or_default();
    let highlighted_plot_point_idx = (*inspected_point_idx.read()).or(*selected_point_idx.read());
    let inspected_plot_point_idx = *inspected_point_idx.read();
    let active_plot_point = inspected_plot_point_idx.and_then(|idx| points.get(idx).copied());

    let sensor_button_style = |active: bool| {
        if active {
            format!(
                "padding:6px 10px; border-radius:999px; border:1px solid {}; background:{}; color:{}; cursor:pointer;",
                theme.info_accent, theme.info_background, theme.info_text
            )
        } else {
            format!(
                "padding:6px 10px; border-radius:999px; border:1px solid {}; background:{}; color:{}; cursor:pointer;",
                theme.button_border, theme.button_background, theme.button_text
            )
        }
    };
    let neutral_button_style = format!(
        "padding:6px 12px; border-radius:999px; border:1px solid {}; background:{}; color:{}; cursor:pointer;",
        theme.button_border, theme.button_background, theme.button_text
    );
    let success_button_style = format!(
        "padding:6px 12px; border-radius:999px; border:1px solid {}; background:{}; color:{}; cursor:pointer;",
        theme.notification_border, theme.notification_background, theme.notification_text
    );
    let warning_button_style = format!(
        "padding:6px 12px; border-radius:999px; border:1px solid {}; background:{}; color:{}; cursor:pointer;",
        theme.warning_border, theme.warning_background, theme.warning_text
    );
    let error_button_style = format!(
        "padding:6px 12px; border-radius:999px; border:1px solid {}; background:{}; color:{}; cursor:pointer;",
        theme.error_border, theme.error_background, theme.error_text
    );
    let input_style = format!(
        "padding:8px 10px; border-radius:10px; border:1px solid {}; background:{}; color:{};",
        theme.border, theme.panel_background_alt, theme.text_primary
    );
    let select_style = input_style.clone();
    let shell_style = format!(
        "padding:12px; display:flex; flex-direction:column; gap:12px; min-height:100%; overflow:visible; color:{}; background:{};",
        theme.text_primary, theme.tab_shell_background
    );
    let section_style = format!(
        "display:flex; flex-direction:column; gap:10px; padding:14px; border:1px solid {}; border-radius:16px; background:{}; box-shadow:0 10px 24px rgba(0,0,0,0.18);",
        theme.tab_shell_border, theme.panel_background
    );
    let toolbar_style = format!(
        "display:flex; gap:8px; flex-wrap:wrap; align-items:center; padding:10px 12px; border:1px solid {}; border-radius:14px; background:{};",
        theme.border_soft, theme.panel_background_alt
    );
    let sequence_points_summary = if points.is_empty() {
        "No saved points yet.".to_string()
    } else {
        points
            .iter()
            .map(|(raw, expected)| format!("{expected:.3} kg -> {raw:.6}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let active_plot_point_cx =
        inspected_plot_point_idx.and_then(|idx| scatter_xy.get(idx).map(|(cx, _)| *cx));
    let active_plot_point_cy =
        inspected_plot_point_idx.and_then(|idx| scatter_xy.get(idx).map(|(_, cy)| *cy));
    const GRAPH_POINT_RADIUS_IDLE: f32 = 5.0;
    const GRAPH_POINT_RADIUS_ACTIVE: f32 = 7.0;
    const GRAPH_POINT_TOUCH_RADIUS: f32 = 16.0;
    let point_overlay_height = 86.0_f32;
    let point_overlay_gap_x = 42.0_f32;
    let point_overlay_gap_y = 26.0_f32;
    let point_overlay_width_css = "172px";
    let point_overlay_horizontal_style = active_plot_point_cx.map(|cx| {
        let x_pct = ((cx / plot_w) * 100.0).clamp(0.0, 100.0);
        if x_pct <= 58.0 {
            format!("left:calc({x_pct:.4}% + {point_overlay_gap_x:.1}px);")
        } else {
            let right_pct = (((plot_w - cx) / plot_w) * 100.0).clamp(0.0, 100.0);
            format!("right:calc({right_pct:.4}% + {point_overlay_gap_x:.1}px);")
        }
    });
    let point_overlay_top = active_plot_point_cy.map(|cy| {
        let bottom_space = plot_h - cy;
        let top = if bottom_space > point_overlay_height + point_overlay_gap_y + 10.0 {
            cy + point_overlay_gap_y
        } else {
            cy - point_overlay_height - point_overlay_gap_y
        }
        .clamp(10.0, plot_h - point_overlay_height - 10.0);
        format!("{top:.1}px")
    });

    rsx! {
        div { style: "{shell_style}",
            style { {r#"
                @media (max-width: 1100px) {
                    .gs26-calibration-plot-overlay-desktop { display: none !important; }
                    .gs26-calibration-plot-overlay-mobile { display: block !important; }
                }
                @media (min-width: 1101px) {
                    .gs26-calibration-plot-overlay-mobile { display: none !important; }
                }
            "#} }
            div { style: "{section_style}",
                div { style: "display:flex; align-items:flex-start; justify-content:space-between; gap:12px; flex-wrap:wrap;",
                    div {
                        h2 { style: "margin:0; color:{theme.text_primary}; font-size:20px;", "Calibration" }
                        div { style: "margin-top:4px; color:{theme.text_muted}; font-size:13px;", "Tune live sensor fits, capture calibration points, and refit without leaving the dashboard." }
                        div { style: "margin-top:6px; color:{theme.text_secondary}; font-size:12px;",
                            if can_edit {
                                "Local edits stay on this page until Save pushes them to the backend. Saved changes sync to other open frontends."
                            } else {
                                "Current calibration data, active regression, and captured points are shown here."
                            }
                        }
                    }
                    div { style: "display:flex; align-items:center; gap:8px; flex-wrap:wrap;",
                        if *dirty.read() {
                            div { style: "padding:8px 10px; border-radius:12px; border:1px solid {theme.warning_border}; background:{theme.warning_background}; color:{theme.warning_text}; font-size:12px; font-weight:700; text-transform:uppercase; letter-spacing:0.04em;", "Unsaved" }
                        } else {
                            div { style: "padding:8px 10px; border-radius:12px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-size:12px; font-weight:700; text-transform:uppercase; letter-spacing:0.04em;", "Saved" }
                        }
                        if can_edit {
                            button {
                                style: "{success_button_style}",
                                disabled: cfg.read().is_none() || !*dirty.read(),
                                onclick: {
                                    let mut cfg = cfg;
                                    let selected_sensor = selected_sensor.clone();
                                    let fit_mode = fit_mode;
                                    let mut status = status;
                                    let mut dirty = dirty;
                                    move |_| {
                                        let Some(next) = cfg.read().clone() else {
                                            status.set("No calibration data loaded".to_string());
                                            return;
                                        };
                                        let Some(sensor) = selected_sensor.clone() else {
                                            status.set("No sensor selected".to_string());
                                            return;
                                        };
                                        status.set("Saving calibration to backend...".to_string());
                                        spawn(async move {
                                            match http_post_json::<CalibrationFile, CalibrationFile>("/api/calibration", &next).await {
                                                Ok(_) => {
                                                    let body = RefitReq {
                                                        channel: sensor.channel.clone(),
                                                        mode: fit_mode.read().clone(),
                                                    };
                                                    match http_post_json::<RefitReq, CalibrationFile>("/api/calibration/refit", &body).await {
                                                        Ok(new_cfg) => {
                                                            cfg.set(Some(new_cfg));
                                                            dirty.set(false);
                                                            status.set("Calibration saved".to_string());
                                                        }
                                                        Err(e) => status.set(format!("Refit failed: {e}")),
                                                    }
                                                }
                                                Err(e) => status.set(format!("Save failed: {e}")),
                                            }
                                        });
                                    }
                                },
                                "Save"
                            }
                        }
                    }
                }

            div { style: "{toolbar_style}",
                span { style: "color:{theme.text_secondary};", "Sensors" }
                for sensor in sensors.iter().cloned() {
                    button {
                        style: "{sensor_button_style(sensor.id == effective_selected_sensor_id)}",
                        onclick: {
                            let mut selected_sensor_id = selected_sensor_id;
                            let mut selected_point_idx = selected_point_idx;
                            let sensor_id = sensor.id.clone();
                            move |_| {
                                selected_sensor_id.set(sensor_id.clone());
                                selected_point_idx.set(None);
                            }
                        },
                        "{sensor.label}"
                    }
                }
            }

            div { style: "display:grid; gap:10px; grid-template-columns:repeat(auto-fit,minmax(190px,1fr));",
                {metric_card(&theme, "Live Raw", raw_live_s.clone())}
                {metric_card(&theme, "Calibrated Value", calibrated_live_s.clone())}
                {metric_card(&theme, "Active Fit", fit_type_s.clone())}
            }
            }

            div { style: "{section_style}",
            div { style: "{toolbar_style}",
                span { style: "color:{theme.text_secondary};", "Regression" }
                if can_edit {
                    select {
                        style: "{select_style}",
                        value: "{fit_mode.read()}",
                        onchange: {
                            let mut fit_mode = fit_mode;
                            move |e| fit_mode.set(e.value())
                        },
                        for mode in fit_modes.iter() {
                            option { value: "{mode}", "{mode}" }
                        }
                    }
                } else {
                    div { style: "padding:8px 12px; border-radius:999px; border:1px solid {theme.info_accent}; background:{theme.info_background}; color:{theme.info_text}; font-weight:700;", "{fit_mode.read()}" }
                }
                if can_edit {
                    button {
                        style: "{neutral_button_style}",
                        disabled: cfg.read().is_none(),
                        onclick: {
                            let mut cfg = cfg;
                            let selected_sensor = selected_sensor.clone();
                            let fit_mode = fit_mode;
                            let mut status = status;
                            move |_| {
                                let Some(sensor) = selected_sensor.clone() else {
                                    status.set("No sensor selected".to_string());
                                    return;
                                };
                                let body = RefitReq {
                                    channel: sensor.channel.clone(),
                                    mode: fit_mode.read().clone(),
                                };
                                spawn(async move {
                                    match http_post_json::<RefitReq, CalibrationFile>("/api/calibration/refit", &body).await {
                                        Ok(new_cfg) => {
                                            cfg.set(Some(new_cfg));
                                            status.set("Refit complete".to_string());
                                        }
                                        Err(e) => status.set(format!("Refit failed: {e}")),
                                    }
                                });
                            }
                        },
                        "Refit"
                    }
                }
            }

            if can_edit {
            div { style: "{toolbar_style}",
                input {
                    style: "{input_style}",
                    r#type: "number",
                    step: "0.01",
                    placeholder: "Known mass (kg)",
                    value: "{manual_kg.read()}",
                    disabled: !can_edit,
                    oninput: {
                        let mut manual_kg = manual_kg;
                        move |e| manual_kg.set(e.value())
                    }
                }
                input {
                    style: "{input_style}",
                    r#type: "number",
                    step: "0.000001",
                    placeholder: "Measured raw sensor value",
                    value: "{manual_raw.read()}",
                    disabled: !can_edit,
                    oninput: {
                        let mut manual_raw = manual_raw;
                        move |e| manual_raw.set(e.value())
                    }
                }
                button {
                    style: "{success_button_style}",
                    disabled: cfg.read().is_none() || !can_edit,
                    onclick: {
                        let mut cfg = cfg;
                        let selected_sensor = selected_sensor.clone();
                        let manual_kg = manual_kg;
                        let manual_raw = manual_raw;
                        let mut status = status;
                        let mut dirty = dirty;
                        move |_| {
                            let Ok(kg) = manual_kg.read().parse::<f32>() else {
                                status.set("Invalid manual kg".to_string());
                                return;
                            };
                            let Ok(raw) = manual_raw.read().parse::<f32>() else {
                                status.set("Invalid manual raw".to_string());
                                return;
                            };
                            let Some(sensor) = selected_sensor.clone() else {
                                status.set("No sensor selected".to_string());
                                return;
                            };
                            let selected_channel = sensor.channel.clone();
                            let mut next = cfg.read().clone().unwrap_or_default();
                            upsert_point_by_key(&mut next, &selected_channel, kg, raw);
                            cfg.set(Some(next.clone()));
                            dirty.set(true);
                            status.set("Point updated locally. Save to push changes to the backend.".to_string());
                        }
                    },
                    "Add/Update Point"
                }
                button {
                    style: "{neutral_button_style}",
                    disabled: selected_sensor.is_none() || !can_edit,
                    onclick: {
                        let selected_sensor = selected_sensor.clone();
                        let mut manual_raw = manual_raw;
                        let mut status = status;
                        let mut manual_capture_progress = manual_capture_progress;
                        let mut manual_capture_progress_epoch = manual_capture_progress_epoch;
                        let is_edit_capture = selected_point_idx.read().is_some();
                        move |_| {
                            let Some(sensor) = selected_sensor.clone() else {
                                status.set("No sensor selected".to_string());
                                return;
                            };
                            let capture_kind = if is_edit_capture { "edit" } else { "point" };
                            let initial_message = format!(
                                "Capturing {} samples for {} {}...",
                                effective_capture_sample_count, sensor.label, capture_kind
                            );
                            manual_capture_progress_epoch += 1;
                            manual_capture_progress.set(initial_message.clone());
                            status.set(initial_message);
                            let sensor_label = sensor.label.clone();
                            spawn(async move {
                                match capture_average_raw_sample(
                                    sensor.data_type.clone(),
                                    effective_capture_sample_count,
                                    |completed, target| {
                                        let progress = format!(
                                            "Capturing samples for {}: {}/{}",
                                            sensor_label, completed, target
                                        );
                                        manual_capture_progress.set(progress.clone());
                                        status.set(progress);
                                    },
                                )
                                .await
                                {
                                    Ok((raw, captured)) => {
                                        manual_raw.set(format!("{raw:.6}"));
                                        let done = format!(
                                            "Captured averaged raw sample {raw:.6} from {} samples on {}.",
                                            captured, sensor.label
                                        );
                                        manual_capture_progress.set(done.clone());
                                        status.set(done);
                                    }
                                    Err(err) => {
                                        manual_capture_progress.set(err.clone());
                                        status.set(err);
                                    }
                                }
                            });
                        }
                    },
                    if selected_point_idx.read().is_some() { "Capture for Edit" } else { "Capture for Point" }
                }
                if !manual_capture_progress.read().is_empty() {
                    div { style: "padding:6px 10px; min-height:20px; display:flex; align-items:center; font-size:12px; color:{theme.info_text}; border:1px solid {theme.info_accent}; border-radius:999px; background:{theme.info_background};",
                        "{manual_capture_progress.read()}"
                    }
                }
            }
            }

            if can_edit {
            div { style: "{toolbar_style}",
                input {
                    style: "{input_style}",
                    r#type: "number",
                    step: "0.01",
                    placeholder: "Sequence point mass (kg)",
                    value: "{known_kg.read()}",
                    disabled: !can_edit,
                    oninput: {
                        let mut known_kg = known_kg;
                        move |e| known_kg.set(e.value())
                    }
                }
                button {
                    style: "{warning_button_style}",
                    disabled: selected_sensor.is_none() || !can_edit,
                    onclick: {
                        let mut sequence_dialog_open = sequence_dialog_open;
                        let mut sequence_dialog_mode = sequence_dialog_mode;
                        let mut sequence_dialog_weight = sequence_dialog_weight;
                        let mut sequence_dialog_captured_raw = sequence_dialog_captured_raw;
                        let mut sequence_dialog_status = sequence_dialog_status;
                        let mut sequence_dialog_replace_existing = sequence_dialog_replace_existing;
                        let mut sequence_dialog_confirm_reset = sequence_dialog_confirm_reset;
                        let mut known_kg = known_kg;
                        move |_| {
                            sequence_dialog_mode.set(CaptureMode::SequenceZero);
                            sequence_dialog_weight.set("0".to_string());
                            sequence_dialog_captured_raw.set(String::new());
                            sequence_dialog_status.set(
                                "Capture a zero-load sample, then save to start a fresh sequence."
                                    .to_string(),
                            );
                            sequence_dialog_replace_existing.set(true);
                            sequence_dialog_confirm_reset.set(false);
                            known_kg.set("1.0".to_string());
                            sequence_dialog_open.set(true);
                        }
                    },
                    "Start New Sequence..."
                }
                if sequence_started {
                    button {
                        style: "{neutral_button_style}",
                        disabled: selected_sensor.is_none() || !can_edit,
                        onclick: {
                            let mut sequence_dialog_open = sequence_dialog_open;
                            let mut sequence_dialog_mode = sequence_dialog_mode;
                            let mut sequence_dialog_weight = sequence_dialog_weight;
                            let mut sequence_dialog_captured_raw = sequence_dialog_captured_raw;
                            let mut sequence_dialog_status = sequence_dialog_status;
                            let mut sequence_dialog_replace_existing = sequence_dialog_replace_existing;
                            let mut sequence_dialog_confirm_reset = sequence_dialog_confirm_reset;
                            let known_kg = known_kg;
                            move |_| {
                                sequence_dialog_mode.set(CaptureMode::SequencePoint);
                                sequence_dialog_weight.set(known_kg.read().clone());
                                sequence_dialog_captured_raw.set(String::new());
                                sequence_dialog_status.set(
                                    "Capture the current live reading for the next sequence point, then save it."
                                        .to_string(),
                                );
                                sequence_dialog_replace_existing.set(false);
                                sequence_dialog_confirm_reset.set(true);
                                sequence_dialog_open.set(true);
                            }
                        },
                        "Continue Sequence..."
                    }
                }
            }
            }

            if can_edit {
            div { style: "{toolbar_style}",
                button {
                    style: "{error_button_style}",
                    disabled: cfg.read().is_none() || selected_point_idx.read().is_none() || !can_edit,
                    onclick: {
                        let mut cfg = cfg;
                        let selected_sensor = selected_sensor.clone();
                        let mut selected_point_idx = selected_point_idx;
                        let mut status = status;
                        let mut dirty = dirty;
                        move |_| {
                            let Some(idx) = *selected_point_idx.read() else {
                                status.set("Select a point first".to_string());
                                return;
                            };
                            let Some(sensor) = selected_sensor.clone() else {
                                status.set("No sensor selected".to_string());
                                return;
                            };
                            let mut next = cfg.read().clone().unwrap_or_default();
                            if !remove_point_by_key(&mut next, &sensor.channel, idx) {
                                status.set("Invalid selected point".to_string());
                                return;
                            }
                            cfg.set(Some(next.clone()));
                            selected_point_idx.set(None);
                            dirty.set(true);
                            status.set("Point removed locally. Save to push changes to the backend.".to_string());
                        }
                    },
                    "Remove Selected"
                }
                button {
                    style: "{error_button_style}",
                    disabled: cfg.read().is_none() || !can_edit,
                    onclick: {
                        let mut cfg = cfg;
                        let selected_sensor = selected_sensor.clone();
                        let mut selected_point_idx = selected_point_idx;
                        let mut status = status;
                        let mut dirty = dirty;
                        move |_| {
                            let Some(sensor) = selected_sensor.clone() else {
                                status.set("No sensor selected".to_string());
                                return;
                            };
                            let mut next = cfg.read().clone().unwrap_or_default();
                            reset_channel_by_key(&mut next, &sensor.channel);
                            cfg.set(Some(next.clone()));
                            selected_point_idx.set(None);
                            dirty.set(true);
                            status.set("Channel reset locally. Save to push changes to the backend.".to_string());
                        }
                    },
                    "Reset Channel"
                }
            }
            }
            }

            div { style: "{section_style}",
            div { style: "display:grid; grid-template-columns:repeat(auto-fit, minmax(min(100%, 320px), 1fr)); gap:12px; align-items:start;",
                div { style: "display:flex; flex-direction:column; gap:8px;",
                    div { style: "font-size:13px; font-weight:700; color:{theme.text_secondary}; text-transform:uppercase; letter-spacing:0.04em;", "Points" }
                    div { style: "display:grid; grid-template-columns:1fr; gap:6px; border:1px solid {theme.border}; border-radius:12px; padding:10px; background:{theme.panel_background_alt}; max-height:420px; overflow:auto;",
                for (idx, (raw, expected)) in points.clone().into_iter().enumerate() {
                    button {
                        style: if *selected_point_idx.read() == Some(idx) {
                            format!(
                                "text-align:left; padding:8px; border-radius:8px; border:1px solid {}; background:{}; color:{}; cursor:pointer;",
                                theme.info_accent, theme.info_background, theme.info_text
                            )
                        } else {
                            format!(
                                "text-align:left; padding:8px; border-radius:8px; border:1px solid {}; background:{}; color:{}; cursor:pointer;",
                                theme.border_soft, theme.panel_background_alt, theme.text_primary
                            )
                        },
                        onclick: {
                            let mut selected_point_idx = selected_point_idx;
                            let mut manual_kg = manual_kg;
                            let mut manual_raw = manual_raw;
                            move |_| {
                                selected_point_idx.set(Some(idx));
                                manual_kg.set(format!("{expected}"));
                                manual_raw.set(format!("{raw}"));
                            }
                        },
                        div { style: "font-weight:700;", "{expected:.4} kg" }
                        div { style: "font-size:12px; opacity:0.82;", "raw {raw:.6}" }
                    }
                }
                if points.is_empty() {
                    div { style: "color:{theme.text_muted};", "(no points for this channel)" }
                }
            }
                }
                div { style: "display:flex; flex-direction:column; gap:10px;",
            div { style: "border:1px solid {theme.border}; border-radius:12px; padding:10px; background:{theme.panel_background_alt}; overflow:visible;",
                div {
                    style: "display:flex; align-items:center; gap:10px; flex-wrap:wrap; padding:6px 8px 10px 8px;",
                    svg { width: "30", height: "10", view_box: "0 0 30 10", style: "display:block; flex:0 0 auto;",
                        line { x1:"2", y1:"5", x2:"28", y2:"5", stroke:"{fit_color}", "stroke-width":"2.5", "stroke-linecap":"round" }
                    }
                    div {
                        style: "color:{fit_color}; font-size:12px; font-weight:700; letter-spacing:0.03em; text-transform:uppercase;",
                        "Calibration Fit"
                    }
                    div {
                        style: "color:{fit_color}; font-family: ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace; font-variant-numeric:tabular-nums; white-space:pre-wrap; word-break:break-word; line-height:1.45; min-height:20px;",
                        "{fit_equation_text}"
                    }
                }
                div {
                    style: "position:relative; width:100%; height:260px; overflow:visible;",
                    onclick: {
                        let mut inspected_point_idx = inspected_point_idx;
                        move |_| inspected_point_idx.set(None)
                    },
                svg { view_box: "0 0 {plot_w} {plot_h}", style: "width:100%; height:260px; display:block; overflow:visible;",
                    rect { x:"0", y:"0", width:"{plot_w}", height:"{plot_h}", fill:"{theme.panel_background_alt}" }
                    line { x1:"{pad_l}", y1:"{pad_t}", x2:"{pad_l}", y2:"{plot_h - pad_b}", stroke:"{theme.border_strong}", "stroke-width":"1" }
                    line { x1:"{pad_l}", y1:"{plot_h - pad_b}", x2:"{plot_w - pad_r}", y2:"{plot_h - pad_b}", stroke:"{theme.border_strong}", "stroke-width":"1" }
                    if !fit_path.is_empty() {
                        path { d: "{fit_path}", fill:"none", stroke:"{fit_color}", "stroke-width":"2.5" }
                    }
                    for (idx, (cx, cy)) in scatter_xy.iter().enumerate() {
                        circle {
                            cx:"{cx}",
                            cy:"{cy}",
                            r: if highlighted_plot_point_idx == Some(idx) { "{GRAPH_POINT_RADIUS_ACTIVE}" } else { "{GRAPH_POINT_RADIUS_IDLE}" },
                            fill: if highlighted_plot_point_idx == Some(idx) { "{theme.info_accent}" } else { "{theme.warning_text}" },
                            stroke: if highlighted_plot_point_idx == Some(idx) { "{theme.info_text}" } else { "none" },
                            "stroke-width": if highlighted_plot_point_idx == Some(idx) { "1.5" } else { "0" },
                            style: "cursor:pointer;",
                            onmouseenter: {
                                let mut inspected_point_idx = inspected_point_idx;
                                move |_| inspected_point_idx.set(Some(idx))
                            },
                            onmouseleave: {
                                let mut inspected_point_idx = inspected_point_idx;
                                move |_| inspected_point_idx.set(None)
                            },
                            onclick: {
                                let mut inspected_point_idx = inspected_point_idx;
                                move |evt| {
                                    evt.stop_propagation();
                                    inspected_point_idx.set(Some(idx));
                                }
                            }
                        }
                        circle {
                            cx:"{cx}",
                            cy:"{cy}",
                            r:"{GRAPH_POINT_TOUCH_RADIUS}",
                            fill:"transparent",
                            stroke:"transparent",
                            style:"cursor:pointer;",
                            onmouseenter: {
                                let mut inspected_point_idx = inspected_point_idx;
                                move |_| inspected_point_idx.set(Some(idx))
                            },
                            onmouseleave: {
                                let mut inspected_point_idx = inspected_point_idx;
                                move |_| inspected_point_idx.set(None)
                            },
                            onclick: {
                                let mut inspected_point_idx = inspected_point_idx;
                                move |evt| {
                                    evt.stop_propagation();
                                    inspected_point_idx.set(Some(idx));
                                }
                            }
                        }
                    }
                    text { x:"6", y:"14", fill:"{theme.text_muted}", "font-size":"11", {format!("y max {:.3}", y_max)} }
                    text { x:"6", y:"{plot_h - pad_b + 4.0}", fill:"{theme.text_muted}", "font-size":"11", {format!("y min {:.3}", y_min)} }
                    text { x:"{pad_l}", y:"{plot_h - 6.0}", fill:"{theme.text_muted}", "font-size":"11", {format!("x min {:.3}", x_min)} }
                    text { x:"{plot_w - 130.0}", y:"{plot_h - 6.0}", fill:"{theme.text_muted}", "font-size":"11", {format!("x max {:.3}", x_max)} }
                }
                if let (Some((raw, expected)), Some(panel_horizontal_style), Some(panel_top)) = (active_plot_point, point_overlay_horizontal_style.clone(), point_overlay_top.clone()) {
                    div {
                        class: "gs26-calibration-plot-overlay-desktop",
                        style: "position:absolute; {panel_horizontal_style} top:{panel_top}; transform:translateZ(0); width:{point_overlay_width_css}; max-width:calc(100% - 24px); box-sizing:border-box; padding:10px 12px; border-radius:14px; border:1px solid {theme.info_accent}; background:{theme.panel_background}; color:{theme.text_primary}; box-shadow:0 14px 28px rgba(0,0,0,0.28); pointer-events:none; z-index:3; overflow-wrap:anywhere;",
                        div { style: "font-size:11px; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.05em; margin-bottom:6px;", "Selected Point" }
                        div { style: "display:grid; gap:4px; font-size:12px; line-height:1.35;",
                            div { style: "display:flex; justify-content:space-between; gap:10px; flex-wrap:wrap;",
                                span { "Value" }
                                span { "{expected:.4} kg" }
                            }
                            div { style: "display:flex; justify-content:space-between; gap:10px; flex-wrap:wrap;",
                                span { "Raw" }
                                span { "{raw:.6}" }
                            }
                        }
                    }
                }
                }
                div {
                    class: "gs26-calibration-plot-overlay-mobile",
                    style: "display:none; width:100%; box-sizing:border-box; margin-top:10px; min-height:108px;",
                    if let Some((raw, expected)) = active_plot_point {
                        div {
                            style: "width:100%; box-sizing:border-box; padding:10px 12px; border-radius:14px; border:1px solid {theme.info_accent}; background:{theme.panel_background}; color:{theme.text_primary}; box-shadow:0 10px 24px rgba(0,0,0,0.18); overflow-wrap:anywhere;",
                            div { style: "font-size:11px; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.05em; margin-bottom:6px;", "Selected Point" }
                            div { style: "display:grid; gap:4px; font-size:12px; line-height:1.35;",
                                div { style: "display:flex; justify-content:space-between; gap:10px; flex-wrap:wrap;",
                                    span { "Value" }
                                    span { "{expected:.4} kg" }
                                }
                                div { style: "display:flex; justify-content:space-between; gap:10px; flex-wrap:wrap;",
                                    span { "Raw" }
                                    span { "{raw:.6}" }
                                }
                            }
                        }
                    } else {
                        div { style: "font-size:11px; color:{theme.text_muted}; text-transform:uppercase; letter-spacing:0.05em; margin-bottom:6px;", "Selected Point" }
                        div { style: "padding:10px 12px; border-radius:14px; border:1px dashed {theme.border}; background:{theme.panel_background}; color:{theme.text_muted}; font-size:12px;", "Tap a point to inspect its value and raw reading." }
                    }
                }
            }
                }
            }
            if *sequence_dialog_open.read() {
                div {
                    style: "position:fixed; inset:0; z-index:4200; display:flex; align-items:center; justify-content:center; padding:20px; background:rgba(0,0,0,0.45);",
                    onclick: {
                        let mut sequence_dialog_open = sequence_dialog_open;
                        move |_| sequence_dialog_open.set(false)
                    },
                    div {
                        style: "width:min(560px, 100%); display:flex; flex-direction:column; gap:12px; padding:16px; border-radius:16px; border:1px solid {theme.tab_shell_border}; background:{theme.panel_background}; box-shadow:0 16px 40px rgba(0,0,0,0.35);",
                        onclick: move |evt| evt.stop_propagation(),
                        div {
                            style: "display:flex; align-items:flex-start; justify-content:space-between; gap:12px; flex-wrap:wrap;",
                            div {
                                div { style: "font-size:18px; font-weight:700; color:{theme.text_primary};",
                                    if *sequence_dialog_mode.read() == CaptureMode::SequenceZero { "Start New Sequence" } else { "Continue Sequence" }
                                }
                                div { style: "margin-top:4px; font-size:13px; color:{theme.text_muted};", "{sequence_dialog_status.read()}" }
                            }
                        }
                        if *sequence_dialog_replace_existing.read() {
                            div {
                                style: "padding:12px; border-radius:12px; border:1px solid {theme.warning_border}; background:{theme.warning_background}; color:{theme.warning_text};",
                                div { style: "font-weight:700; margin-bottom:4px;", "Replace Existing Sequence" }
                                div { style: "font-size:13px;", "Saving this new sequence zero will replace the current channel calibration points and fit." }
                                label { style: "margin-top:8px; display:flex; align-items:center; gap:8px; font-size:13px;",
                                    input {
                                        r#type: "checkbox",
                                        checked: *sequence_dialog_confirm_reset.read(),
                                        onclick: {
                                            let mut sequence_dialog_confirm_reset = sequence_dialog_confirm_reset;
                                            move |_| {
                                                let next = !*sequence_dialog_confirm_reset.read();
                                                sequence_dialog_confirm_reset.set(next);
                                            }
                                        }
                                    }
                                    span { "I understand this will replace the existing sequence data." }
                                }
                            }
                        }
                        div { style: "{toolbar_style}",
                            input {
                                style: "{input_style}",
                                r#type: "number",
                                step: "0.01",
                                placeholder: if *sequence_dialog_mode.read() == CaptureMode::SequenceZero { "Sequence zero mass (kg)" } else { "Sequence point mass (kg)" },
                                value: "{sequence_dialog_weight.read()}",
                                disabled: *sequence_dialog_mode.read() == CaptureMode::SequenceZero,
                                oninput: {
                                    let mut sequence_dialog_weight = sequence_dialog_weight;
                                    move |e| sequence_dialog_weight.set(e.value())
                                }
                            }
                            input {
                                style: "{input_style}",
                                r#type: "text",
                                readonly: true,
                                placeholder: "Captured raw sample appears here",
                                value: "{sequence_dialog_captured_raw.read()}",
                            }
                            button {
                                style: "{success_button_style}",
                                disabled: !can_edit,
                                onclick: {
                                    let selected_sensor = selected_sensor.clone();
                                    let mut sequence_dialog_captured_raw = sequence_dialog_captured_raw;
                                    let mut sequence_dialog_status = sequence_dialog_status;
                                    move |_| {
                                        let Some(sensor) = selected_sensor.clone() else {
                                            sequence_dialog_status.set("No sensor selected.".to_string());
                                            return;
                                        };
                                        sequence_dialog_status.set(format!(
                                            "Capturing and averaging {} live samples for {}...",
                                            effective_capture_sample_count,
                                            sensor.label
                                        ));
                                        let sensor_label = sensor.label.clone();
                                        spawn(async move {
                                            match capture_average_raw_sample(
                                                sensor.data_type.clone(),
                                                effective_capture_sample_count,
                                                |completed, target| {
                                                    sequence_dialog_status.set(format!(
                                                        "Capturing samples for {}: {}/{}",
                                                        sensor_label, completed, target
                                                    ));
                                                },
                                            )
                                            .await
                                            {
                                                Ok((raw, captured)) => {
                                                    sequence_dialog_captured_raw.set(format!("{raw:.6}"));
                                                    sequence_dialog_status.set(format!(
                                                        "Captured averaged raw sample {raw:.6} from {} samples on {}.",
                                                        captured, sensor.label
                                                    ));
                                                }
                                                Err(err) => sequence_dialog_status.set(err),
                                            }
                                        });
                                    }
                                },
                                "Capture Averaged Sample"
                            }
                        }
                        div {
                            style: "display:grid; gap:6px; padding:12px; border:1px solid {theme.border}; border-radius:12px; background:{theme.panel_background_alt};",
                            div { style: "font-size:12px; font-weight:700; color:{theme.text_secondary}; text-transform:uppercase; letter-spacing:0.04em;", "Current Points" }
                            pre { style: "margin:0; font-size:12px; line-height:1.5; color:{theme.text_primary}; white-space:pre-wrap; word-break:break-word; font-family:ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace;", "{sequence_points_summary}" }
                        }
                        div { style: "display:flex; justify-content:flex-end; gap:8px; flex-wrap:wrap;",
                            button {
                                style: "{neutral_button_style}",
                                onclick: {
                                    let mut sequence_dialog_open = sequence_dialog_open;
                                    move |_| sequence_dialog_open.set(false)
                                },
                                "Cancel"
                            }
                            button {
                                style: "{warning_button_style}",
                                disabled: !can_edit
                                    || sequence_dialog_captured_raw.read().trim().is_empty()
                                    || (*sequence_dialog_replace_existing.read() && !*sequence_dialog_confirm_reset.read()),
                                onclick: {
                                    let selected_sensor = selected_sensor.clone();
                                    let mut cfg = cfg;
                                    let mut status = status;
                                    let mut dirty = dirty;
                                    let mut sequence_dialog_open = sequence_dialog_open;
                                    let mut sequence_dialog_status = sequence_dialog_status;
                                    let sequence_dialog_mode = sequence_dialog_mode;
                                    let sequence_dialog_weight = sequence_dialog_weight;
                                    let sequence_dialog_captured_raw = sequence_dialog_captured_raw;
                                    let sequence_dialog_replace_existing = sequence_dialog_replace_existing;
                                    move |_| {
                                        let Some(sensor) = selected_sensor.clone() else {
                                            sequence_dialog_status.set("No sensor selected.".to_string());
                                            return;
                                        };
                                        let Ok(raw) = sequence_dialog_captured_raw.read().trim().parse::<f32>() else {
                                            sequence_dialog_status.set("Capture a live raw sample before saving.".to_string());
                                            return;
                                        };
                                        let mode = *sequence_dialog_mode.read();
                                        let weight = if mode == CaptureMode::SequenceZero {
                                            0.0
                                        } else {
                                            match sequence_dialog_weight.read().trim().parse::<f32>() {
                                                Ok(value) if value > 0.0 => value,
                                                _ => {
                                                    sequence_dialog_status.set("Enter a sequence point mass greater than zero.".to_string());
                                                    return;
                                                }
                                            }
                                        };
                                        let replace_existing = *sequence_dialog_replace_existing.read();
                                        let mut next = cfg.read().clone().unwrap_or_default();
                                        if replace_existing {
                                            reset_channel_by_key(&mut next, &sensor.channel);
                                        }
                                        if mode == CaptureMode::SequenceZero {
                                            let channel = next.channels.entry(sensor.channel.clone()).or_default();
                                            channel.zero_raw = Some(raw);
                                            channel.points.clear();
                                            channel.fit = None;
                                        }
                                        if mode == CaptureMode::SequencePoint {
                                            upsert_point_by_key(&mut next, &sensor.channel, weight, raw);
                                        }
                                        cfg.set(Some(next.clone()));
                                        dirty.set(true);
                                        status.set(if mode == CaptureMode::SequenceZero {
                                            format!("Started a new sequence on {} locally. Save to push changes to the backend.", sensor.label)
                                        } else {
                                            format!("Captured {} kg on {} locally. Save to push changes to the backend.", weight, sensor.label)
                                        });
                                        sequence_dialog_open.set(false);
                                    }
                                },
                                "Save"
                            }
                        }
                    }
                }
            }
            }
        }
    }
}

fn metric_card(theme: &ThemeConfig, label: &str, value: String) -> Element {
    rsx! {
        div { style: "padding:10px 12px; border:1px solid {theme.border_soft}; border-radius:12px; background:{theme.panel_background_alt};",
            div { style: "font-size:11px; color:{theme.text_muted}; margin-bottom:4px; text-transform:uppercase; letter-spacing:0.04em;", "{label}" }
            div {
                style: "font-size:14px; color:{theme.text_primary}; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; display:inline-block; min-width:14ch; text-align:right; font-family: ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace; font-variant-numeric:tabular-nums; font-weight:700;",
                "{value}"
            }
        }
    }
}
