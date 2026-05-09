#![allow(clippy::redundant_locals)]

use super::{
    TELEMETRY_RENDER_EPOCH, http_get_json, http_post_json, latest_telemetry_value,
    layout::{ThemeConfig, ValueFormatter}, persist, translate_text,
};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
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
    #[serde(default)]
    formatter: Option<ValueFormatter>,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectedCalibrationPoint {
    Zero,
    Weighted(usize),
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

fn sensor_raw_precision(sensor: Option<&CalibrationSensorSpec>, default_precision: usize) -> usize {
    sensor
        .and_then(|sensor| sensor.formatter.as_ref())
        .and_then(|formatter| formatter.precision)
        .unwrap_or(default_precision)
}

fn format_sensor_raw_value(
    value: f32,
    sensor: Option<&CalibrationSensorSpec>,
    default_precision: usize,
) -> String {
    let precision = sensor_raw_precision(sensor, default_precision);
    format_raw_with_precision(value, precision)
}

fn format_raw_with_precision(value: f32, precision: usize) -> String {
    format!("{value:.precision$}")
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
    if expected <= 1e-6 {
        let channel = cfg.channels.entry(channel.to_string()).or_default();
        channel.zero_raw = Some(raw);
        return;
    }
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

fn zero_raw_for_channel_key(cfg: &CalibrationFile, channel: &str) -> Option<f32> {
    cfg.channels.get(channel).and_then(|c| c.zero_raw)
}

fn remove_zero_by_key(cfg: &mut CalibrationFile, channel: &str) -> bool {
    cfg.channels.get_mut(channel).is_some_and(|c| {
        let had_zero = c.zero_raw.is_some();
        c.zero_raw = None;
        had_zero
    })
}

fn now_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now().max(0.0) as u64
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_millis() as u64
    }
}

fn reset_channel_by_key(cfg: &mut CalibrationFile, channel: &str) {
    cfg.channels.remove(channel);
}

fn points_for_channel_key(cfg: &CalibrationFile, channel: &str) -> Vec<(f64, f64)> {
    let mut points: Vec<(f64, f64)> = cfg
        .channels
        .get(channel)
        .map(|channel| {
            channel
                .points
                .iter()
                .map(|p| (p.raw as f64, p.expected as f64))
                .collect()
        })
        .unwrap_or_default();
    if let Some(zero_raw) = zero_raw_for_channel_key(cfg, channel) {
        points.push((zero_raw as f64, 0.0));
    }
    points
}

fn fit_line(xs: &[f64], ys: &[f64]) -> Result<(f64, f64), String> {
    let n = xs.len() as f64;
    let sx: f64 = xs.iter().sum();
    let sy: f64 = ys.iter().sum();
    let sxx: f64 = xs.iter().map(|x| x * x).sum();
    let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| x * y).sum();
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-18 {
        return Err("degenerate points for linear fit".to_string());
    }
    Ok(((n * sxy - sx * sy) / denom, (sy * sxx - sx * sxy) / denom))
}

fn fit_line_through_zero(xs: &[f64], ys: &[f64]) -> Result<f64, String> {
    let denom: f64 = xs.iter().map(|x| x * x).sum();
    if denom.abs() < 1e-18 {
        return Err("degenerate points for linear-zero fit".to_string());
    }
    Ok(xs.iter().zip(ys).map(|(x, y)| x * y).sum::<f64>() / denom)
}

fn solve_linear_system(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Result<Vec<f64>, String> {
    let n = a.len();
    if n == 0 || b.len() != n || a.iter().any(|row| row.len() != n) {
        return Err("invalid linear system dimensions".to_string());
    }
    for i in 0..n {
        let mut pivot = i;
        let mut max_abs = a[i][i].abs();
        for (r, row) in a.iter().enumerate().skip(i + 1) {
            if row[i].abs() > max_abs {
                max_abs = row[i].abs();
                pivot = r;
            }
        }
        if max_abs < 1e-18 {
            return Err("degenerate system".to_string());
        }
        if pivot != i {
            a.swap(i, pivot);
            b.swap(i, pivot);
        }
        let pivot_val = a[i][i];
        for item in a[i].iter_mut().skip(i) {
            *item /= pivot_val;
        }
        b[i] /= pivot_val;
        for r in 0..n {
            if r == i {
                continue;
            }
            let factor = a[r][i];
            if factor.abs() < 1e-18 {
                continue;
            }
            let pivot_tail = a[i][i..].to_vec();
            for (dest, pivot_entry) in a[r].iter_mut().skip(i).zip(pivot_tail.iter()) {
                *dest -= factor * *pivot_entry;
            }
            b[r] -= factor * b[i];
        }
    }
    Ok(b)
}

fn fit_poly2(xs: &[f64], ys: &[f64]) -> Result<(f64, f64, f64), String> {
    let sx: f64 = xs.iter().sum();
    let sx2: f64 = xs.iter().map(|x| x * x).sum();
    let sx3: f64 = xs.iter().map(|x| x * x * x).sum();
    let sx4: f64 = xs.iter().map(|x| x * x * x * x).sum();
    let sy: f64 = ys.iter().sum();
    let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| x * y).sum();
    let sx2y: f64 = xs.iter().zip(ys).map(|(x, y)| x * x * y).sum();
    let a = vec![
        vec![sx4, sx3, sx2],
        vec![sx3, sx2, sx],
        vec![sx2, sx, xs.len() as f64],
    ];
    let b = vec![sx2y, sxy, sy];
    let sol = solve_linear_system(a, b)?;
    Ok((sol[0], sol[1], sol[2]))
}

fn fit_poly2_through_zero(xs: &[f64], ys: &[f64]) -> Result<(f64, f64), String> {
    let sx2: f64 = xs.iter().map(|x| x * x).sum();
    let sx3: f64 = xs.iter().map(|x| x * x * x).sum();
    let sx4: f64 = xs.iter().map(|x| x * x * x * x).sum();
    let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| x * y).sum();
    let sx2y: f64 = xs.iter().zip(ys).map(|(x, y)| x * x * y).sum();
    let det = sx4 * sx2 - sx3 * sx3;
    if det.abs() < 1e-18 {
        return Err("degenerate points for poly2-zero fit".to_string());
    }
    Ok((
        (sx2y * sx2 - sxy * sx3) / det,
        (sx4 * sxy - sx3 * sx2y) / det,
    ))
}

fn fit_poly3(xs: &[f64], ys: &[f64]) -> Result<(f64, f64, f64, f64), String> {
    let sx: f64 = xs.iter().sum();
    let sx2: f64 = xs.iter().map(|x| x * x).sum();
    let sx3: f64 = xs.iter().map(|x| x * x * x).sum();
    let sx4: f64 = xs.iter().map(|x| x * x * x * x).sum();
    let sx5: f64 = xs.iter().map(|x| x * x * x * x * x).sum();
    let sx6: f64 = xs.iter().map(|x| x * x * x * x * x * x).sum();
    let sy: f64 = ys.iter().sum();
    let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| x * y).sum();
    let sx2y: f64 = xs.iter().zip(ys).map(|(x, y)| x * x * y).sum();
    let sx3y: f64 = xs.iter().zip(ys).map(|(x, y)| x * x * x * y).sum();
    let a = vec![
        vec![sx6, sx5, sx4, sx3],
        vec![sx5, sx4, sx3, sx2],
        vec![sx4, sx3, sx2, sx],
        vec![sx3, sx2, sx, xs.len() as f64],
    ];
    let b = vec![sx3y, sx2y, sxy, sy];
    let sol = solve_linear_system(a, b)?;
    Ok((sol[0], sol[1], sol[2], sol[3]))
}

fn fit_poly3_through_zero(xs: &[f64], ys: &[f64]) -> Result<(f64, f64, f64), String> {
    let sx2: f64 = xs.iter().map(|x| x * x).sum();
    let sx3: f64 = xs.iter().map(|x| x * x * x).sum();
    let sx4: f64 = xs.iter().map(|x| x * x * x * x).sum();
    let sx5: f64 = xs.iter().map(|x| x * x * x * x * x).sum();
    let sx6: f64 = xs.iter().map(|x| x * x * x * x * x * x).sum();
    let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| x * y).sum();
    let sx2y: f64 = xs.iter().zip(ys).map(|(x, y)| x * x * y).sum();
    let sx3y: f64 = xs.iter().zip(ys).map(|(x, y)| x * x * x * y).sum();
    let a = vec![
        vec![sx6, sx5, sx4],
        vec![sx5, sx4, sx3],
        vec![sx4, sx3, sx2],
    ];
    let b = vec![sx3y, sx2y, sxy];
    let sol = solve_linear_system(a, b)?;
    Ok((sol[0], sol[1], sol[2]))
}

fn fit_poly_degree(xs: &[f64], ys: &[f64], degree: usize) -> Result<Vec<f64>, String> {
    if xs.len() <= degree {
        return Err(format!("need at least {} points for poly{degree} fit", degree + 1));
    }
    let n = degree + 1;
    let mut a = vec![vec![0.0; n]; n];
    let mut b = vec![0.0; n];
    for (row, row_values) in a.iter_mut().enumerate().take(n) {
        for (col, slot) in row_values.iter_mut().enumerate().take(n) {
            let power = (2 * degree).saturating_sub(row + col) as i32;
            *slot = xs.iter().map(|x| x.powi(power)).sum();
        }
        let power = degree.saturating_sub(row) as i32;
        b[row] = xs.iter().zip(ys).map(|(x, y)| x.powi(power) * y).sum();
    }
    solve_linear_system(a, b)
}

fn fit_poly4(xs: &[f64], ys: &[f64]) -> Result<(f64, f64, f64, f64, f64), String> {
    let sol = fit_poly_degree(xs, ys, 4)?;
    Ok((sol[0], sol[1], sol[2], sol[3], sol[4]))
}

fn fit_poly4_through_zero(xs: &[f64], ys: &[f64]) -> Result<(f64, f64, f64, f64), String> {
    if xs.len() < 4 {
        return Err("need at least 4 points for poly4-zero fit".to_string());
    }
    let a = vec![
        vec![xs.iter().map(|x| x.powi(8)).sum(), xs.iter().map(|x| x.powi(7)).sum(), xs.iter().map(|x| x.powi(6)).sum(), xs.iter().map(|x| x.powi(5)).sum()],
        vec![xs.iter().map(|x| x.powi(7)).sum(), xs.iter().map(|x| x.powi(6)).sum(), xs.iter().map(|x| x.powi(5)).sum(), xs.iter().map(|x| x.powi(4)).sum()],
        vec![xs.iter().map(|x| x.powi(6)).sum(), xs.iter().map(|x| x.powi(5)).sum(), xs.iter().map(|x| x.powi(4)).sum(), xs.iter().map(|x| x.powi(3)).sum()],
        vec![xs.iter().map(|x| x.powi(5)).sum(), xs.iter().map(|x| x.powi(4)).sum(), xs.iter().map(|x| x.powi(3)).sum(), xs.iter().map(|x| x.powi(2)).sum()],
    ];
    let b = vec![
        xs.iter().zip(ys).map(|(x, y)| x.powi(4) * y).sum(),
        xs.iter().zip(ys).map(|(x, y)| x.powi(3) * y).sum(),
        xs.iter().zip(ys).map(|(x, y)| x.powi(2) * y).sum(),
        xs.iter().zip(ys).map(|(x, y)| x * y).sum(),
    ];
    let sol = solve_linear_system(a, b)?;
    Ok((sol[0], sol[1], sol[2], sol[3]))
}

fn sse_line(xs: &[f64], ys: &[f64], m: f64, b: f64) -> f64 {
    xs.iter().zip(ys).map(|(x, y)| (y - (m * x + b)).powi(2)).sum()
}

fn sse_poly2(xs: &[f64], ys: &[f64], a: f64, b: f64, c: f64) -> f64 {
    xs.iter().zip(ys).map(|(x, y)| (y - (a * x * x + b * x + c)).powi(2)).sum()
}

fn sse_poly3(xs: &[f64], ys: &[f64], a: f64, b: f64, c: f64, d: f64) -> f64 {
    xs.iter().zip(ys).map(|(x, y)| (y - (a * x.powi(3) + b * x * x + c * x + d)).powi(2)).sum()
}

fn sse_poly4(xs: &[f64], ys: &[f64], a: f64, b: f64, c: f64, d: f64, e0: f64) -> f64 {
    xs.iter().zip(ys).map(|(x, y)| (y - (a * x.powi(4) + b * x.powi(3) + c * x * x + d * x + e0)).powi(2)).sum()
}

fn aic(sse: f64, n: usize, k: usize) -> f64 {
    if n == 0 {
        return f64::INFINITY;
    }
    let s = sse.max(1e-18);
    (n as f64) * (s / n as f64).ln() + 2.0 * (k as f64)
}

fn local_refit_channel(cfg: &mut CalibrationFile, channel: &str, mode: &str) -> Result<(), String> {
    let pts = points_for_channel_key(cfg, channel);
    if pts.len() < 2 {
        return Err("need at least 2 points".to_string());
    }
    let xs: Vec<f64> = pts.iter().map(|(x, _)| *x).collect();
    let ys: Vec<f64> = pts.iter().map(|(_, y)| *y).collect();
    let zero_hint = zero_raw_for_channel_key(cfg, channel)
        .map(|v| v as f64)
        .or_else(|| pts.iter().find(|(_, y)| y.abs() < 1e-9).map(|(x, _)| *x));
    let mut candidates: Vec<(&str, f64)> = Vec::new();
    let (lin_m, lin_b) = fit_line(&xs, &ys)?;
    candidates.push(("linear", aic(sse_line(&xs, &ys, lin_m, lin_b), xs.len(), 2)));
    let mut lin0_m = None;
    if let Some(x0) = zero_hint {
        let xs_shift: Vec<f64> = xs.iter().map(|x| x - x0).collect();
        let m = fit_line_through_zero(&xs_shift, &ys)?;
        lin0_m = Some((m, x0));
        candidates.push(("linear_zero", aic(sse_line(&xs_shift, &ys, m, 0.0), xs_shift.len(), 1)));
    }
    let mut poly2 = None;
    if xs.len() >= 3 {
        let v = fit_poly2(&xs, &ys)?;
        candidates.push(("poly2", aic(sse_poly2(&xs, &ys, v.0, v.1, v.2), xs.len(), 3)));
        poly2 = Some(v);
    }
    let mut poly2_zero = None;
    if let Some(x0) = zero_hint && xs.len() >= 2 {
        let xs_shift: Vec<f64> = xs.iter().map(|x| x - x0).collect();
        let v = fit_poly2_through_zero(&xs_shift, &ys)?;
        candidates.push(("poly2_zero", aic(sse_poly2(&xs_shift, &ys, v.0, v.1, 0.0), xs_shift.len(), 2)));
        poly2_zero = Some((v.0, v.1, x0));
    }
    let mut poly3 = None;
    if xs.len() >= 4 {
        let v = fit_poly3(&xs, &ys)?;
        candidates.push(("poly3", aic(sse_poly3(&xs, &ys, v.0, v.1, v.2, v.3), xs.len(), 4)));
        poly3 = Some(v);
    }
    let mut poly3_zero = None;
    if let Some(x0) = zero_hint && xs.len() >= 3 {
        let xs_shift: Vec<f64> = xs.iter().map(|x| x - x0).collect();
        let v = fit_poly3_through_zero(&xs_shift, &ys)?;
        candidates.push(("poly3_zero", aic(sse_poly3(&xs_shift, &ys, v.0, v.1, v.2, 0.0), xs_shift.len(), 3)));
        poly3_zero = Some((v.0, v.1, v.2, x0));
    }
    let mut poly4 = None;
    if xs.len() >= 5 {
        let v = fit_poly4(&xs, &ys)?;
        candidates.push(("poly4", aic(sse_poly4(&xs, &ys, v.0, v.1, v.2, v.3, v.4), xs.len(), 5)));
        poly4 = Some(v);
    }
    let mut poly4_zero = None;
    if let Some(x0) = zero_hint && xs.len() >= 4 {
        let xs_shift: Vec<f64> = xs.iter().map(|x| x - x0).collect();
        let v = fit_poly4_through_zero(&xs_shift, &ys)?;
        candidates.push(("poly4_zero", aic(sse_poly4(&xs_shift, &ys, v.0, v.1, v.2, v.3, 0.0), xs_shift.len(), 4)));
        poly4_zero = Some((v.0, v.1, v.2, v.3, x0));
    }
    let chosen = if mode == "best" {
        candidates
            .iter()
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(m, _)| *m)
            .ok_or_else(|| "no fit candidates".to_string())?
    } else {
        mode
    };
    let channel_slot = cfg.channels.entry(channel.to_string()).or_default();
    match chosen {
        "linear" => {
            channel_slot.linear.m = Some(lin_m as f32);
            channel_slot.linear.b = Some(lin_b as f32);
            channel_slot.fit = Some(FitMeta { fit_type: Some("linear".to_string()), x0: None, ..FitMeta::default() });
        }
        "linear_zero" => {
            let (m, x0) = lin0_m.ok_or_else(|| "linear_zero fit unavailable".to_string())?;
            channel_slot.linear.m = Some(m as f32);
            channel_slot.linear.b = Some((-m * x0) as f32);
            channel_slot.fit = Some(FitMeta { fit_type: Some("linear".to_string()), x0: Some(x0 as f32), ..FitMeta::default() });
        }
        "poly2" | "parabolic" | "quadratic" => {
            let (a, b, c) = poly2.ok_or_else(|| "poly2 fit unavailable".to_string())?;
            channel_slot.linear.m = Some(b as f32);
            channel_slot.linear.b = Some(c as f32);
            channel_slot.fit = Some(FitMeta { fit_type: Some("poly2".to_string()), a: Some(a as f32), b: Some(b as f32), c: Some(c as f32), ..FitMeta::default() });
        }
        "poly2_zero" | "parabolic_zero" | "quadratic_zero" => {
            let (a, b, x0) = poly2_zero.ok_or_else(|| "poly2_zero fit unavailable".to_string())?;
            let m_lin = a + b;
            channel_slot.linear.m = Some(m_lin as f32);
            channel_slot.linear.b = Some((-m_lin * x0) as f32);
            channel_slot.fit = Some(FitMeta { fit_type: Some("poly2".to_string()), a: Some(a as f32), b: Some(b as f32), c: Some(0.0), d: Some(0.0), x0: Some(x0 as f32), ..FitMeta::default() });
        }
        "poly3" | "cubic" => {
            let (a, b, c, d) = poly3.ok_or_else(|| "poly3 fit unavailable".to_string())?;
            channel_slot.linear.m = Some(c as f32);
            channel_slot.linear.b = Some(d as f32);
            channel_slot.fit = Some(FitMeta { fit_type: Some("poly3".to_string()), a: Some(a as f32), b: Some(b as f32), c: Some(c as f32), d: Some(d as f32), ..FitMeta::default() });
        }
        "poly3_zero" | "cubic_zero" => {
            let (a, b, c, x0) = poly3_zero.ok_or_else(|| "poly3_zero fit unavailable".to_string())?;
            channel_slot.linear.m = Some(c as f32);
            channel_slot.linear.b = Some((-c * x0) as f32);
            channel_slot.fit = Some(FitMeta { fit_type: Some("poly3".to_string()), a: Some(a as f32), b: Some(b as f32), c: Some(c as f32), d: Some(0.0), x0: Some(x0 as f32), ..FitMeta::default() });
        }
        "poly4" | "quartic" => {
            let (a, b, c, d, e) = poly4.ok_or_else(|| "poly4 fit unavailable".to_string())?;
            channel_slot.linear.m = Some(d as f32);
            channel_slot.linear.b = Some(e as f32);
            channel_slot.fit = Some(FitMeta { fit_type: Some("poly4".to_string()), a: Some(a as f32), b: Some(b as f32), c: Some(c as f32), d: Some(d as f32), e: Some(e as f32), x0: None });
        }
        "poly4_zero" | "quartic_zero" => {
            let (a, b, c, d, x0) = poly4_zero.ok_or_else(|| "poly4_zero fit unavailable".to_string())?;
            channel_slot.linear.m = Some(d as f32);
            channel_slot.linear.b = Some((-d * x0) as f32);
            channel_slot.fit = Some(FitMeta { fit_type: Some("poly4".to_string()), a: Some(a as f32), b: Some(b as f32), c: Some(c as f32), d: Some(d as f32), e: Some(0.0), x0: Some(x0 as f32) });
        }
        _ => return Err("invalid fit mode".to_string()),
    }
    Ok(())
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
    let value = eval_fit_parts(linear, fit, raw)?;
    let zero_raw = cfg
        .channels
        .get(channel)
        .and_then(|channel| channel.zero_raw);
    let zero_offset = zero_raw
        .and_then(|baseline_raw| eval_fit_parts(linear, fit, baseline_raw))
        .unwrap_or(0.0);
    Some(value - zero_offset)
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

#[cfg(test)]
mod calibration_eval_tests {
    use super::{CalibrationFile, ChannelLinear, GenericCalibrationChannel, eval_fit_key};
    use std::collections::BTreeMap;

    #[test]
    fn zero_raw_offsets_linear_calibrated_value() {
        let mut channels = BTreeMap::new();
        channels.insert(
            "LOADCELL".to_string(),
            GenericCalibrationChannel {
                linear: ChannelLinear {
                    m: Some(2.0),
                    b: Some(1.0),
                },
                zero_raw: Some(3.0),
                ..Default::default()
            },
        );
        let cfg = CalibrationFile {
            channels,
            ..Default::default()
        };

        assert_eq!(eval_fit_key(&cfg, "LOADCELL", 3.0), Some(0.0));
        assert_eq!(eval_fit_key(&cfg, "LOADCELL", 4.5), Some(3.0));
    }
}

#[component]
pub fn CalibrationTab(theme: ThemeConfig, can_edit: bool, capture_sample_count: usize) -> Element {
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
    let selected_point = use_signal(|| None::<SelectedCalibrationPoint>);
    let inspected_point_idx = use_signal(|| None::<SelectedCalibrationPoint>);
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
    let raw_precision = sensor_raw_precision(selected_sensor.as_ref(), 6);
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
        let mut selected_point = selected_point;
        let manual_kg = manual_kg;
        let zero_raw = cfg
            .read()
            .as_ref()
            .and_then(|c| zero_raw_for_channel_key(c, &channel_key));
        use_effect(move || {
            let Some(selection) = *selected_point.read() else {
                return;
            };
            let expected = match selection {
                SelectedCalibrationPoint::Zero => {
                    if zero_raw.is_none() {
                        selected_point.set(None);
                        return;
                    }
                    0.0
                }
                SelectedCalibrationPoint::Weighted(idx) => {
                    let Some((_, expected)) = points.get(idx).copied() else {
                        selected_point.set(None);
                        return;
                    };
                    expected
                }
            };
            let kg_matches = manual_kg
                .read()
                .parse::<f32>()
                .ok()
                .is_some_and(|value| (value - expected).abs() <= 0.0001);
            if !kg_matches {
                selected_point.set(None);
            }
        });
    }
    {
        let points = points.clone();
        let manual_kg = manual_kg;
        let mut selected_point = selected_point;
        let zero_raw = cfg
            .read()
            .as_ref()
            .and_then(|c| zero_raw_for_channel_key(c, &channel_key));
        use_effect(move || {
            let Ok(kg) = manual_kg.read().parse::<f32>() else {
                return;
            };
            let next_selection = if kg.abs() <= 0.0001 && zero_raw.is_some() {
                Some(SelectedCalibrationPoint::Zero)
            } else {
                matching_point_idx(&points, kg).map(SelectedCalibrationPoint::Weighted)
            };
            if *selected_point.read() != next_selection {
                selected_point.set(next_selection);
            }
        });
    }
    let sequence_started = cfg.read().as_ref().is_some_and(|c| {
        c.channels
            .get(&channel_key)
            .and_then(|channel| channel.zero_raw)
            .is_some()
    });
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
    let zero_raw = cfg
        .read()
        .as_ref()
        .and_then(|c| zero_raw_for_channel_key(c, &channel_key));
    let mut display_points = Vec::with_capacity(points.len() + usize::from(zero_raw.is_some()));
    if let Some(raw) = zero_raw {
        display_points.push((raw, 0.0, SelectedCalibrationPoint::Zero));
    }
    for (idx, (raw, expected)) in points.iter().copied().enumerate() {
        display_points.push((raw, expected, SelectedCalibrationPoint::Weighted(idx)));
    }

    let plot_w = 900.0_f32;
    let plot_h = 260.0_f32;
    let pad_l = 56.0_f32;
    let pad_r = 14.0_f32;
    let pad_t = 14.0_f32;
    let pad_b = 28.0_f32;
    let mut x_min = display_points
        .iter()
        .map(|(x, _, _)| *x)
        .min_by(f32::total_cmp)
        .unwrap_or(0.0);
    let mut x_max = display_points
        .iter()
        .map(|(x, _, _)| *x)
        .max_by(f32::total_cmp)
        .unwrap_or(1.0);
    let mut y_min = display_points
        .iter()
        .map(|(_, y, _)| *y)
        .min_by(f32::total_cmp)
        .unwrap_or(0.0);
    let mut y_max = display_points
        .iter()
        .map(|(_, y, _)| *y)
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
    let scatter_xy: Vec<(f32, f32)> = display_points
        .iter()
        .map(|(x, y, _)| (sx(*x), sy(*y)))
        .collect();
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
    let highlighted_plot_point_idx = (*inspected_point_idx.read()).or(*selected_point.read());
    let inspected_plot_point_idx = *inspected_point_idx.read();
    let active_plot_point =
        inspected_plot_point_idx.and_then(|selection| {
            display_points
                .iter()
                .find(|(_, _, point)| *point == selection)
                .map(|(raw, expected, _)| (*raw, *expected))
        });
    let active_plot_point_coords =
        inspected_plot_point_idx.and_then(|selection| {
            display_points
                .iter()
                .position(|(_, _, point)| *point == selection)
                .and_then(|idx| scatter_xy.get(idx).copied())
        });

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
    let sequence_points_summary = if display_points.is_empty() {
        "No saved points yet.".to_string()
    } else {
        display_points
            .iter()
            .map(|(raw, expected, _)| format!("{expected:.3} kg -> {raw:.raw_precision$}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let active_plot_point_cx = active_plot_point_coords.map(|(cx, _)| cx);
    let active_plot_point_cy = active_plot_point_coords.map(|(_, cy)| cy);
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
    let point_rows: Vec<(f32, f32, SelectedCalibrationPoint, String, String)> = display_points
        .iter()
        .map(|(raw, expected, selection)| {
            let label = if expected.abs() <= 0.0001 {
                "0.0000 kg (zero)".to_string()
            } else {
                format!("{expected:.4} kg")
            };
            let raw_label = format!("raw {}", format_raw_with_precision(*raw, raw_precision));
            (*raw, *expected, *selection, label, raw_label)
        })
        .collect();
    let plotted_points: Vec<(f32, f32, SelectedCalibrationPoint)> = scatter_xy
        .iter()
        .copied()
        .zip(display_points.iter().map(|(_, _, selection)| *selection))
        .map(|((cx, cy), selection)| (cx, cy, selection))
        .collect();

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
                            let mut selected_point = selected_point;
                            let sensor_id = sensor.id.clone();
                            move |_| {
                                selected_sensor_id.set(sensor_id.clone());
                                selected_point.set(None);
                            }
                        },
                        "{sensor.label}"
                    }
                }
            }

            div { style: "display:grid; gap:10px; grid-template-columns:repeat(auto-fit,minmax(190px,1fr));",
                CalibrationLiveMetrics {
                    theme: theme.clone(),
                    selected_sensor: selected_sensor.clone(),
                    calibration: cfg.read().clone(),
                    channel_key: channel_key.clone(),
                }
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
                            let mut dirty = dirty;
                            move |_| {
                                let Some(mut current_cfg) = cfg.read().clone() else {
                                    status.set("No calibration data loaded".to_string());
                                    return;
                                };
                                let Some(sensor) = selected_sensor.clone() else {
                                    status.set("No sensor selected".to_string());
                                    return;
                                };
                                match local_refit_channel(
                                    &mut current_cfg,
                                    &sensor.channel,
                                    fit_mode.read().as_str(),
                                ) {
                                    Ok(()) => {
                                        cfg.set(Some(current_cfg));
                                        dirty.set(true);
                                        status.set("Refit preview updated locally".to_string());
                                    }
                                    Err(e) => status.set(format!("Refit failed: {e}")),
                                }
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
                        let is_edit_capture = selected_point.read().is_some();
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
                                        manual_raw.set(format_sensor_raw_value(
                                            raw,
                                            Some(&sensor),
                                            6,
                                        ));
                                        let done = format!(
                                            "Captured averaged raw sample {} from {} samples on {}.",
                                            format_sensor_raw_value(raw, Some(&sensor), 6),
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
                    if selected_point.read().is_some() { "Capture for Edit" } else { "Capture for Point" }
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
                                "Capture a zero-load sample, save it, then keep adding sequence points."
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
                                    "Capture the current live reading for a sequence point, then save it. Reusing the same mass recaptures that point."
                                        .to_string(),
                                );
                                sequence_dialog_replace_existing.set(false);
                                sequence_dialog_confirm_reset.set(false);
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
                    disabled: cfg.read().is_none() || selected_point.read().is_none() || !can_edit,
                    onclick: {
                        let mut cfg = cfg;
                        let selected_sensor = selected_sensor.clone();
                        let mut selected_point = selected_point;
                        let mut status = status;
                        let mut dirty = dirty;
                        move |_| {
                            let Some(selection) = *selected_point.read() else {
                                status.set("Select a point first".to_string());
                                return;
                            };
                            let Some(sensor) = selected_sensor.clone() else {
                                status.set("No sensor selected".to_string());
                                return;
                            };
                            let mut next = cfg.read().clone().unwrap_or_default();
                            let removed = match selection {
                                SelectedCalibrationPoint::Zero => {
                                    remove_zero_by_key(&mut next, &sensor.channel)
                                }
                                SelectedCalibrationPoint::Weighted(idx) => {
                                    remove_point_by_key(&mut next, &sensor.channel, idx)
                                }
                            };
                            if !removed {
                                status.set("Invalid selected point".to_string());
                                return;
                            }
                            cfg.set(Some(next.clone()));
                            selected_point.set(None);
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
                        let mut selected_point = selected_point;
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
                            selected_point.set(None);
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
                for (raw, expected, selection, point_label, raw_label) in point_rows.clone().into_iter() {
                    button {
                        style: if *selected_point.read() == Some(selection) {
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
                            let mut selected_point = selected_point;
                            let mut manual_kg = manual_kg;
                            let mut manual_raw = manual_raw;
                            move |_| {
                                selected_point.set(Some(selection));
                                manual_kg.set(format!("{expected}"));
                                manual_raw.set(format_raw_with_precision(raw, raw_precision));
                            }
                        },
                        div { style: "font-weight:700;", "{point_label}" }
                        div { style: "font-size:12px; opacity:0.82;", "{raw_label}" }
                    }
                }
                if display_points.is_empty() {
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
                    for (cx, cy, selection) in plotted_points.clone().into_iter() {
                        circle {
                            cx:"{cx}",
                            cy:"{cy}",
                            r: if highlighted_plot_point_idx == Some(selection) { "{GRAPH_POINT_RADIUS_ACTIVE}" } else { "{GRAPH_POINT_RADIUS_IDLE}" },
                            fill: if highlighted_plot_point_idx == Some(selection) { "{theme.info_accent}" } else { "{theme.warning_text}" },
                            stroke: if highlighted_plot_point_idx == Some(selection) { "{theme.info_text}" } else { "none" },
                            "stroke-width": if highlighted_plot_point_idx == Some(selection) { "1.5" } else { "0" },
                            style: "cursor:pointer;",
                            onmouseenter: {
                                let mut inspected_point_idx = inspected_point_idx;
                                move |_| inspected_point_idx.set(Some(selection))
                            },
                            onmouseleave: {
                                let mut inspected_point_idx = inspected_point_idx;
                                move |_| inspected_point_idx.set(None)
                            },
                            onclick: {
                                let mut inspected_point_idx = inspected_point_idx;
                                move |evt| {
                                    evt.stop_propagation();
                                    inspected_point_idx.set(Some(selection));
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
                                move |_| inspected_point_idx.set(Some(selection))
                            },
                            onmouseleave: {
                                let mut inspected_point_idx = inspected_point_idx;
                                move |_| inspected_point_idx.set(None)
                            },
                            onclick: {
                                let mut inspected_point_idx = inspected_point_idx;
                                move |evt| {
                                    evt.stop_propagation();
                                    inspected_point_idx.set(Some(selection));
                                }
                            }
                        }
                    }
                    text { x:"6", y:"14", fill:"{theme.text_muted}", "font-size":"11", {format!("y max {:.3}", y_max)} }
                    text { x:"6", y:"{plot_h - pad_b + 4.0}", fill:"{theme.text_muted}", "font-size":"11", {format!("y min {:.3}", y_min)} }
                    text { x:"{pad_l}", y:"{plot_h - 6.0}", fill:"{theme.text_muted}", "font-size":"11", {format!("x min {:.raw_precision$}", x_min)} }
                    text { x:"{plot_w - 130.0}", y:"{plot_h - 6.0}", fill:"{theme.text_muted}", "font-size":"11", {format!("x max {:.raw_precision$}", x_max)} }
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
                                span { "{format_raw_with_precision(raw, raw_precision)}" }
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
                                    span { "{format_raw_with_precision(raw, raw_precision)}" }
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
                                    let known_kg = known_kg;
                                    let mut cfg = cfg;
                                    let mut dirty = dirty;
                                    let mut status = status;
                                    let mut sequence_dialog_mode = sequence_dialog_mode;
                                    let mut sequence_dialog_weight = sequence_dialog_weight;
                                    let mut sequence_dialog_captured_raw = sequence_dialog_captured_raw;
                                    let mut sequence_dialog_status = sequence_dialog_status;
                                    let mut sequence_dialog_replace_existing = sequence_dialog_replace_existing;
                                    let mut sequence_dialog_confirm_reset = sequence_dialog_confirm_reset;
                                    move |_| {
                                        let Some(sensor) = selected_sensor.clone() else {
                                            sequence_dialog_status.set("No sensor selected.".to_string());
                                            return;
                                        };
                                        sequence_dialog_status.set(format!(
                                            "Capturing and adding {} live samples for {}...",
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
                                                    } else {
                                                        upsert_point_by_key(&mut next, &sensor.channel, weight, raw);
                                                    }
                                                    cfg.set(Some(next));
                                                    dirty.set(true);
                                                    sequence_dialog_captured_raw.set(
                                                        format_sensor_raw_value(
                                                            raw,
                                                            Some(&sensor),
                                                            6,
                                                        ),
                                                    );
                                                    if mode == CaptureMode::SequenceZero {
                                                        status.set(format!(
                                                            "Captured zero for {} locally. Continue adding points, then Save to push changes to the backend.",
                                                            sensor.label
                                                        ));
                                                        sequence_dialog_mode.set(CaptureMode::SequencePoint);
                                                        sequence_dialog_weight.set(known_kg.read().clone());
                                                        sequence_dialog_replace_existing.set(false);
                                                        sequence_dialog_confirm_reset.set(false);
                                                        sequence_dialog_status.set(format!(
                                                            "Zero captured as {} from {} samples on {}. Enter the next mass and capture again.",
                                                            format_sensor_raw_value(raw, Some(&sensor), 6),
                                                            captured,
                                                            sensor.label
                                                        ));
                                                    } else {
                                                        status.set(format!(
                                                            "Captured {} kg on {} locally. Capture another point or Save to push changes to the backend.",
                                                            weight, sensor.label
                                                        ));
                                                        sequence_dialog_status.set(format!(
                                                            "Added/updated {weight} kg as {} from {} samples on {}. Enter the next mass and capture again.",
                                                            format_sensor_raw_value(raw, Some(&sensor), 6),
                                                            captured,
                                                            sensor.label
                                                        ));
                                                    }
                                                }
                                                Err(err) => sequence_dialog_status.set(err),
                                            }
                                        });
                                    }
                                },
                                if *sequence_dialog_mode.read() == CaptureMode::SequenceZero {
                                    "Capture Zero"
                                } else {
                                    "Capture Averaged Sample"
                                }
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
                                style: "{success_button_style}",
                                disabled: !can_edit,
                                onclick: {
                                    let mut sequence_dialog_open = sequence_dialog_open;
                                    move |_| {
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

#[component]
fn CalibrationLiveMetrics(
    theme: ThemeConfig,
    selected_sensor: Option<CalibrationSensorSpec>,
    calibration: Option<CalibrationFile>,
    channel_key: String,
) -> Element {
    let _ = *TELEMETRY_RENDER_EPOCH.read();
    let raw_live = selected_sensor
        .as_ref()
        .and_then(|sensor| latest_raw(sensor.data_type.as_str()));
    let calibrated_live = calibration
        .as_ref()
        .and_then(|cfg| raw_live.and_then(|raw| eval_fit_key(cfg, &channel_key, raw)));
    let raw_live_s = fmt_fixed(raw_live, 12, sensor_raw_precision(selected_sensor.as_ref(), 6));
    let calibrated_live_s = fmt_fixed(calibrated_live, 12, 4);

    rsx! {
        {metric_card(&theme, "Live Raw", raw_live_s)}
        {metric_card(&theme, "Calibrated Value", calibrated_live_s)}
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
