#![allow(clippy::redundant_locals)]

use super::{
    http_get_json, http_post_json, latest_telemetry_timestamp, latest_telemetry_value,
    layout::ThemeConfig, translate_text, TELEMETRY_RENDER_EPOCH,
};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
struct CapturePointReq {
    sensor_id: String,
    raw: f32,
}

#[derive(Serialize)]
struct CaptureSpanReq {
    sensor_id: String,
    raw: f32,
    known_kg: f32,
}

#[derive(Serialize)]
struct RefitReq {
    channel: String,
    mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct CalibrationTabLayout {
    #[serde(default = "default_capture_target_samples")]
    capture_target_samples: usize,
    #[serde(default)]
    fit_modes: Vec<String>,
    #[serde(default)]
    sensors: Vec<CalibrationSensorSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct CalibrationSensorSpec {
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum CaptureMode {
    SequenceZero,
    SequencePoint,
}

fn sleep_ms(ms: u32) -> impl Future<Output = ()> {
    #[cfg(target_arch = "wasm32")]
    {
        gloo_timers::future::TimeoutFuture::new(ms)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::time::sleep(std::time::Duration::from_millis(ms as u64))
    }
}

fn latest_raw(data_type: &str) -> Option<f32> {
    latest_telemetry_value(data_type, None, 0)
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

fn update_point_by_key(
    cfg: &mut CalibrationFile,
    channel: &str,
    index: usize,
    expected: f32,
    raw: f32,
) -> bool {
    let channel = cfg.channels.entry(channel.to_string()).or_default();
    if let Some(p) = channel.points.get_mut(index) {
        p.expected = expected.max(0.0);
        p.raw = raw;
        true
    } else {
        false
    }
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
pub fn CalibrationTab(theme: ThemeConfig) -> Element {
    let _ = *TELEMETRY_RENDER_EPOCH.read();
    let layout_cfg = use_signal(|| None::<CalibrationTabLayout>);
    let sensors = layout_cfg
        .read()
        .as_ref()
        .map(sensors_from_layout)
        .unwrap_or_default();
    let capture_target = layout_cfg
        .read()
        .as_ref()
        .map(|v| v.capture_target_samples)
        .unwrap_or_else(default_capture_target_samples)
        .max(10);

    let cfg = use_signal(|| None::<CalibrationFile>);
    let selected_sensor_id = use_signal(String::new);
    let fit_mode = use_signal(String::new);
    let known_kg = use_signal(|| "1.0".to_string());
    let manual_kg = use_signal(|| "1.0".to_string());
    let manual_raw = use_signal(String::new);
    let selected_point_idx = use_signal(|| None::<usize>);
    let status = use_signal(|| "Loading calibration...".to_string());

    let capture_active = use_signal(|| false);
    let capture_mode = use_signal(|| CaptureMode::SequencePoint);
    let capture_weight = use_signal(|| 0.0f32);
    let capture_vals = use_signal(Vec::<f32>::new);
    let capture_loop_started = use_signal(|| false);

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
        let mut selected_sensor_id = selected_sensor_id;
        use_effect(move || {
            let cur = selected_sensor_id.read().clone();
            if sensors.iter().any(|s| s.id == cur) {
                return;
            }
            if let Some(first) = sensors.first() {
                selected_sensor_id.set(first.id.clone());
            }
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
        let mut capture_loop_started = capture_loop_started;
        let selected_sensor_id = selected_sensor_id;
        let sensors = sensors.clone();
        let capture_target = capture_target;
        let mut capture_active = capture_active;
        let capture_mode = capture_mode;
        let capture_weight = capture_weight;
        let mut capture_vals = capture_vals;
        let fit_mode = fit_mode;
        let mut cfg = cfg;
        let mut status = status;
        use_effect(move || {
            if *capture_loop_started.read() {
                return;
            }
            capture_loop_started.set(true);

            let sensors = sensors.clone();
            spawn(async move {
                loop {
                    sleep_ms(20).await;
                    if !*capture_active.read() {
                        continue;
                    }
                    let selected_id = selected_sensor_id.read().clone();
                    let Some(sensor) = sensors.iter().find(|s| s.id == selected_id) else {
                        continue;
                    };
                    let Some(raw) = latest_raw(sensor.data_type.as_str()) else {
                        continue;
                    };

                    let mut vals = capture_vals.read().clone();
                    vals.push(raw);
                    let count = vals.len();
                    capture_vals.set(vals.clone());

                    if count < capture_target {
                        status.set(format!(
                            "Capturing {}: {count}/{capture_target}",
                            sensor.label
                        ));
                        continue;
                    }

                    let avg = vals.iter().sum::<f32>() / vals.len() as f32;
                    let mode = *capture_mode.read();
                    let weight = *capture_weight.read();
                    capture_active.set(false);
                    capture_vals.set(Vec::new());

                    let sensor_id = sensor.channel.clone();
                    let sensor_label = sensor.label.clone();
                    match mode {
                        CaptureMode::SequenceZero => {
                            let body = CapturePointReq {
                                sensor_id,
                                raw: avg,
                            };
                            match http_post_json::<CapturePointReq, CalibrationFile>(
                                "/api/calibration/capture_zero",
                                &body,
                            )
                            .await
                            {
                                Ok(new_cfg) => {
                                    cfg.set(Some(new_cfg));
                                    status.set(format!(
                                        "Captured zero on {} (avg raw {avg:.6})",
                                        sensor_label.clone()
                                    ));
                                }
                                Err(e) => status.set(format!("Zero capture failed: {e}")),
                            }
                        }
                        CaptureMode::SequencePoint => {
                            let body = CaptureSpanReq {
                                sensor_id: sensor_id.clone(),
                                raw: avg,
                                known_kg: weight,
                            };
                            match http_post_json::<CaptureSpanReq, CalibrationFile>(
                                "/api/calibration/capture_span",
                                &body,
                            )
                            .await
                            {
                                Ok(_) => {
                                    let refit = RefitReq {
                                        channel: sensor_id,
                                        mode: fit_mode.read().clone(),
                                    };
                                    match http_post_json::<RefitReq, CalibrationFile>(
                                        "/api/calibration/refit",
                                        &refit,
                                    )
                                    .await
                                    {
                                        Ok(new_cfg) => {
                                            cfg.set(Some(new_cfg));
                                            status.set(format!(
                                                "Captured point {} kg on {} (avg raw {avg:.6})",
                                                weight,
                                                sensor_label.clone()
                                            ));
                                        }
                                        Err(e) => status.set(format!("Refit failed: {e}")),
                                    }
                                }
                                Err(e) => status.set(format!("Point capture failed: {e}")),
                            }
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
    let channel_key = selected_sensor
        .as_ref()
        .map(|s| s.channel.clone())
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
        let mut fit_mode = fit_mode;
        use_effect(move || {
            let current = fit_mode.read().clone();
            if fit_modes.iter().any(|m| m == &current) {
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
    let raw_live = selected_sensor
        .as_ref()
        .and_then(|s| latest_raw(s.data_type.as_str()));
    let last_ts_ms = selected_sensor
        .as_ref()
        .and_then(|s| latest_telemetry_timestamp(&s.data_type, None));
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
    let ts_live_s = last_ts_ms
        .map(|v| format!("{v:>13}"))
        .unwrap_or_else(|| "-".to_string());
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

    rsx! {
        div { style: "padding:16px; display:flex; flex-direction:column; gap:10px; min-height:100%; overflow:visible; color:{theme.text_primary}; background:{theme.app_background};",
            h2 { style: "margin:0; color:{theme.info_accent};", "Calibration Sequence" }

            div { style: "display:flex; gap:8px; flex-wrap:wrap; align-items:center;",
                span { style: "color:{theme.text_secondary};", "Sensors" }
                for sensor in sensors.iter().cloned() {
                    button {
                        style: "{sensor_button_style(sensor.id == selected_id)}",
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

            div { style: "display:grid; gap:8px; grid-template-columns:repeat(auto-fit,minmax(190px,1fr));",
                {metric_card(&theme, "Last Timestamp (ms)", ts_live_s.clone())}
                {metric_card(&theme, "Live Raw", raw_live_s.clone())}
                {metric_card(&theme, "Calibrated Value", calibrated_live_s.clone())}
                {metric_card(&theme, "Active Fit", fit_type_s.clone())}
            }

            div { style: "display:flex; gap:8px; flex-wrap:wrap; align-items:center;",
                span { style: "color:{theme.text_secondary};", "Regression" }
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
                button {
                    style: "{neutral_button_style}",
                    disabled: cfg.read().is_none(),
                    onclick: {
                        let mut cfg = cfg;
                        let selected_sensor_id = selected_sensor_id;
                        let sensors = sensors.clone();
                        let fit_mode = fit_mode;
                        let mut status = status;
                        move |_| {
                            let selected_id = selected_sensor_id.read().clone();
                            let Some(sensor) = sensors.iter().find(|s| s.id == selected_id) else {
                                status.set("Invalid selected sensor".to_string());
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

            div { style: "display:flex; gap:8px; flex-wrap:wrap; align-items:center;",
                input {
                    style: "{input_style}",
                    r#type: "number",
                    step: "0.01",
                    value: "{manual_kg.read()}",
                    oninput: {
                        let mut manual_kg = manual_kg;
                        move |e| manual_kg.set(e.value())
                    }
                }
                input {
                    style: "{input_style}",
                    r#type: "number",
                    step: "0.000001",
                    placeholder: "raw value",
                    value: "{manual_raw.read()}",
                    oninput: {
                        let mut manual_raw = manual_raw;
                        move |e| manual_raw.set(e.value())
                    }
                }
                button {
                    style: "{success_button_style}",
                    disabled: cfg.read().is_none(),
                    onclick: {
                        let mut cfg = cfg;
                        let selected_sensor_id = selected_sensor_id;
                        let sensors = sensors.clone();
                        let manual_kg = manual_kg;
                        let manual_raw = manual_raw;
                        let fit_mode = fit_mode;
                        let mut status = status;
                        move |_| {
                            let Ok(kg) = manual_kg.read().parse::<f32>() else {
                                status.set("Invalid manual kg".to_string());
                                return;
                            };
                            let Ok(raw) = manual_raw.read().parse::<f32>() else {
                                status.set("Invalid manual raw".to_string());
                                return;
                            };
                            let selected_id = selected_sensor_id.read().clone();
                            let Some(sensor) = sensors.iter().find(|s| s.id == selected_id) else {
                                status.set("Invalid selected sensor".to_string());
                                return;
                            };
                            let selected_channel = sensor.channel.clone();
                            let mut next = cfg.read().clone().unwrap_or_default();
                            upsert_point_by_key(&mut next, &selected_channel, kg, raw);
                            spawn(async move {
                                match http_post_json::<CalibrationFile, CalibrationFile>("/api/calibration", &next).await {
                                    Ok(_) => {
                                        let body = RefitReq {
                                            channel: selected_channel,
                                            mode: fit_mode.read().clone(),
                                        };
                                        match http_post_json::<RefitReq, CalibrationFile>("/api/calibration/refit", &body).await {
                                            Ok(new_cfg) => {
                                                cfg.set(Some(new_cfg));
                                                status.set("Manual point added".to_string());
                                            }
                                            Err(e) => status.set(format!("Refit failed: {e}")),
                                        }
                                    }
                                    Err(e) => status.set(format!("Save failed: {e}")),
                                }
                            });
                        }
                    },
                    "Add/Update Point"
                }
                button {
                    style: "{neutral_button_style}",
                    disabled: cfg.read().is_none() || selected_point_idx.read().is_none(),
                    onclick: {
                        let mut cfg = cfg;
                        let selected_sensor_id = selected_sensor_id;
                        let sensors = sensors.clone();
                        let manual_kg = manual_kg;
                        let manual_raw = manual_raw;
                        let fit_mode = fit_mode;
                        let mut status = status;
                        move |_| {
                            let Some(idx) = *selected_point_idx.read() else {
                                status.set("Select a point first".to_string());
                                return;
                            };
                            let Ok(kg) = manual_kg.read().parse::<f32>() else {
                                status.set("Invalid manual kg".to_string());
                                return;
                            };
                            let Ok(raw) = manual_raw.read().parse::<f32>() else {
                                status.set("Invalid manual raw".to_string());
                                return;
                            };
                            let selected_id = selected_sensor_id.read().clone();
                            let Some(sensor) = sensors.iter().find(|s| s.id == selected_id) else {
                                status.set("Invalid selected sensor".to_string());
                                return;
                            };
                            let selected_channel = sensor.channel.clone();
                            let mut next = cfg.read().clone().unwrap_or_default();
                            if !update_point_by_key(&mut next, &selected_channel, idx, kg, raw) {
                                status.set("Invalid selected point".to_string());
                                return;
                            }
                            spawn(async move {
                                match http_post_json::<CalibrationFile, CalibrationFile>("/api/calibration", &next).await {
                                    Ok(_) => {
                                        let body = RefitReq {
                                            channel: selected_channel,
                                            mode: fit_mode.read().clone(),
                                        };
                                        match http_post_json::<RefitReq, CalibrationFile>("/api/calibration/refit", &body).await {
                                            Ok(new_cfg) => {
                                                cfg.set(Some(new_cfg));
                                                status.set("Point edited".to_string());
                                            }
                                            Err(e) => status.set(format!("Refit failed: {e}")),
                                        }
                                    }
                                    Err(e) => status.set(format!("Save failed: {e}")),
                                }
                            });
                        }
                    },
                    "Save Selected Edit"
                }
            }

            div { style: "display:flex; gap:8px; flex-wrap:wrap; align-items:center;",
                input {
                    style: "{input_style}",
                    r#type: "number",
                    step: "0.01",
                    value: "{known_kg.read()}",
                    oninput: {
                        let mut known_kg = known_kg;
                        move |e| known_kg.set(e.value())
                    }
                }
                button {
                    style: "{warning_button_style}",
                    disabled: *capture_active.read(),
                    onclick: {
                        let mut capture_active = capture_active;
                        let mut capture_mode = capture_mode;
                        let mut capture_weight = capture_weight;
                        let mut capture_vals = capture_vals;
                        move |_| {
                            capture_mode.set(CaptureMode::SequenceZero);
                            capture_weight.set(0.0);
                            capture_vals.set(Vec::new());
                            capture_active.set(true);
                        }
                    },
                    "Start New Sequence (0kg)"
                }
                button {
                    style: "{neutral_button_style}",
                    disabled: *capture_active.read() || !sequence_started,
                    onclick: {
                        let mut capture_active = capture_active;
                        let mut capture_mode = capture_mode;
                        let mut capture_weight = capture_weight;
                        let mut capture_vals = capture_vals;
                        let known_kg = known_kg;
                        let mut status = status;
                        move |_| {
                            let Ok(kg) = known_kg.read().parse::<f32>() else {
                                status.set("Invalid sequence kg".to_string());
                                return;
                            };
                            if kg <= 0.0 {
                                status.set("Sequence point kg must be > 0".to_string());
                                return;
                            }
                            capture_mode.set(CaptureMode::SequencePoint);
                            capture_weight.set(kg);
                            capture_vals.set(Vec::new());
                            capture_active.set(true);
                        }
                    },
                    "Continue Sequence"
                }
                if *capture_active.read() {
                    span { style: "color:{theme.warning_text};", "Capturing {capture_vals.read().len()}/{capture_target}" }
                }
            }

            div { style: "display:flex; gap:8px; flex-wrap:wrap; align-items:flex-start;",
                button {
                    style: "{error_button_style}",
                    disabled: cfg.read().is_none() || selected_point_idx.read().is_none(),
                    onclick: {
                        let mut cfg = cfg;
                        let selected_sensor_id = selected_sensor_id;
                        let sensors = sensors.clone();
                        let mut selected_point_idx = selected_point_idx;
                        let mut status = status;
                        move |_| {
                            let Some(idx) = *selected_point_idx.read() else {
                                status.set("Select a point first".to_string());
                                return;
                            };
                            let selected_id = selected_sensor_id.read().clone();
                            let Some(sensor) = sensors.iter().find(|s| s.id == selected_id) else {
                                status.set("Invalid selected sensor".to_string());
                                return;
                            };
                            let mut next = cfg.read().clone().unwrap_or_default();
                            if !remove_point_by_key(&mut next, &sensor.channel, idx) {
                                status.set("Invalid selected point".to_string());
                                return;
                            }
                            spawn(async move {
                                match http_post_json::<CalibrationFile, CalibrationFile>("/api/calibration", &next).await {
                                    Ok(new_cfg) => {
                                        cfg.set(Some(new_cfg));
                                        selected_point_idx.set(None);
                                        status.set("Point removed".to_string());
                                    }
                                    Err(e) => status.set(format!("Save failed: {e}")),
                                }
                            });
                        }
                    },
                    "Remove Selected"
                }
                button {
                    style: "{error_button_style}",
                    disabled: cfg.read().is_none(),
                    onclick: {
                        let mut cfg = cfg;
                        let selected_sensor_id = selected_sensor_id;
                        let sensors = sensors.clone();
                        let mut selected_point_idx = selected_point_idx;
                        let mut status = status;
                        move |_| {
                            let selected_id = selected_sensor_id.read().clone();
                            let Some(sensor) = sensors.iter().find(|s| s.id == selected_id) else {
                                status.set("Invalid selected sensor".to_string());
                                return;
                            };
                            let mut next = cfg.read().clone().unwrap_or_default();
                            reset_channel_by_key(&mut next, &sensor.channel);
                            spawn(async move {
                                match http_post_json::<CalibrationFile, CalibrationFile>("/api/calibration", &next).await {
                                    Ok(new_cfg) => {
                                        cfg.set(Some(new_cfg));
                                        selected_point_idx.set(None);
                                        status.set("Channel reset".to_string());
                                    }
                                    Err(e) => status.set(format!("Save failed: {e}")),
                                }
                            });
                        }
                    },
                    "Reset Channel"
                }
            }

            div { style: "display:grid; grid-template-columns:1fr; gap:6px; border:1px solid {theme.border}; border-radius:10px; padding:10px; background:{theme.panel_background};",
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
                        "{expected:.4} -> raw {raw:.6}"
                    }
                }
                if points.is_empty() {
                    div { style: "color:{theme.text_muted};", "(no points for this channel)" }
                }
            }

            div { style: "border:1px solid {theme.border}; border-radius:10px; padding:8px; background:{theme.panel_background};",
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
                svg { view_box: "0 0 {plot_w} {plot_h}", style: "width:100%; height:260px; display:block;",
                    rect { x:"0", y:"0", width:"{plot_w}", height:"{plot_h}", fill:"{theme.panel_background_alt}" }
                    line { x1:"{pad_l}", y1:"{pad_t}", x2:"{pad_l}", y2:"{plot_h - pad_b}", stroke:"{theme.border_strong}", "stroke-width":"1" }
                    line { x1:"{pad_l}", y1:"{plot_h - pad_b}", x2:"{plot_w - pad_r}", y2:"{plot_h - pad_b}", stroke:"{theme.border_strong}", "stroke-width":"1" }
                    if !fit_path.is_empty() {
                        path { d: "{fit_path}", fill:"none", stroke:"{fit_color}", "stroke-width":"2.5" }
                    }
                    for (cx, cy) in scatter_xy.iter() {
                        circle { cx:"{cx}", cy:"{cy}", r:"3.5", fill:"{theme.warning_text}" }
                    }
                    text { x:"6", y:"14", fill:"{theme.text_muted}", "font-size":"11", {format!("y max {:.3}", y_max)} }
                    text { x:"6", y:"{plot_h - pad_b + 4.0}", fill:"{theme.text_muted}", "font-size":"11", {format!("y min {:.3}", y_min)} }
                    text { x:"{pad_l}", y:"{plot_h - 6.0}", fill:"{theme.text_muted}", "font-size":"11", {format!("x min {:.3}", x_min)} }
                    text { x:"{plot_w - 130.0}", y:"{plot_h - 6.0}", fill:"{theme.text_muted}", "font-size":"11", {format!("x max {:.3}", x_max)} }
                }
            }

            div { style: "font-size:13px; color:{theme.text_muted};", "{status.read()}" }
        }
    }
}

fn metric_card(theme: &ThemeConfig, label: &str, value: String) -> Element {
    rsx! {
        div { style: "padding:8px 10px; border:1px solid {theme.border}; border-radius:10px; background:{theme.panel_background};",
            div { style: "font-size:11px; color:{theme.text_muted}; margin-bottom:2px;", "{label}" }
            div {
                style: "font-size:14px; color:{theme.text_primary}; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; display:inline-block; min-width:14ch; text-align:right; font-family: ui-monospace,SFMono-Regular,Menlo,Monaco,Consolas,monospace; font-variant-numeric:tabular-nums;",
                "{value}"
            }
        }
    }
}
