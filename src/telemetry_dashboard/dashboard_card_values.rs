//! Dashboard presentation uses one selected timeline for values and graphs.
use crate::telemetry_dashboard::dashboard_history::Sample;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CaptureStats {
    pub last: Sample,
    pub min: f32,
    pub max: f32,
}

pub(super) fn capture_stats(
    samples: &[Sample],
    window: Option<(i64, i64)>,
) -> Option<CaptureStats> {
    let (start, end) = window.filter(|(start, end)| start <= end)?;
    let mut stats: Option<CaptureStats> = None;
    for sample in samples
        .iter()
        .copied()
        .filter(|sample| sample.value.is_finite() && (start..=end).contains(&sample.time))
    {
        match &mut stats {
            Some(stats) => {
                stats.min = stats.min.min(sample.value);
                stats.max = stats.max.max(sample.value);
                if sample.time >= stats.last.time {
                    stats.last = sample;
                }
            }
            None => {
                stats = Some(CaptureStats {
                    last: sample,
                    min: sample.value,
                    max: sample.value,
                })
            }
        }
    }
    stats
}

pub(super) fn display_value(
    aggregate: bool,
    live: Option<f32>,
    capture: Option<CaptureStats>,
) -> Option<f32> {
    if aggregate {
        capture.map(|stats| stats.last.value)
    } else {
        live
    }
}

#[derive(Default)]
pub(super) struct TrendGeometry {
    pub path: String,
    /// A single observed point has no stroke; show it without joining gaps.
    pub isolated: Vec<(f64, f64)>,
}

pub(super) fn trend_geometry(samples: &[Sample], window: Option<(i64, i64)>) -> TrendGeometry {
    let start = window
        .map(|w| w.0)
        .or_else(|| samples.first().map(|s| s.time))
        .unwrap_or(0);
    let end = window
        .map(|w| w.1)
        .or_else(|| samples.last().map(|s| s.time))
        .unwrap_or(start);
    if end < start {
        return TrendGeometry::default();
    }
    let valid = |sample: &&Sample| sample.value.is_finite() && (start..=end).contains(&sample.time);
    let low = samples
        .iter()
        .filter(valid)
        .map(|s| f64::from(s.value))
        .reduce(f64::min);
    let high = samples
        .iter()
        .filter(valid)
        .map(|s| f64::from(s.value))
        .reduce(f64::max);
    let (Some(low), Some(high)) = (low, high) else {
        return TrendGeometry::default();
    };
    // Resolve the actual f32 measurements rather than hiding small changes
    // behind a fixed microvolt or relative-to-absolute-value padding floor.
    let padding = ((high - low) * 0.05)
        .max(high.abs().max(low.abs()) * f64::from(f32::EPSILON) * 4.0)
        .max(f64::MIN_POSITIVE);
    let mut points = Vec::new();
    let mut previous: Option<Sample> = None;
    for sample in samples.iter().copied() {
        if !sample.value.is_finite() || !(start..=end).contains(&sample.time) {
            previous = None;
            continue;
        }
        let connected = previous.is_some_and(|previous| {
            sample.segment == previous.segment
                && sample.time >= previous.time
                && sample.time.saturating_sub(previous.time) <= 1_000
        });
        let x = sample.time.saturating_sub(start) as f64 * 260.0
            / end.saturating_sub(start).max(1) as f64;
        let y = 60.0
            - ((f64::from(sample.value) - low + padding) / (high - low + 2.0 * padding)) * 60.0;
        points.push((x, y, connected));
        previous = Some(sample);
    }
    let mut result = TrendGeometry::default();
    for (index, &(x, y, connected)) in points.iter().enumerate() {
        result.path.push_str(&format!(
            "{}{x:.2},{y:.2} ",
            if connected { "L" } else { "M" }
        ));
        if !connected && !points.get(index + 1).is_some_and(|point| point.2) {
            result.isolated.push((x, y));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(time: i64, value: f32, segment: u64) -> Sample {
        Sample {
            time,
            value,
            segment,
        }
    }

    #[test]
    fn capture_value_and_extrema_never_fall_back_to_live_or_outside_the_launch_window() {
        let samples = [
            sample(99, -500.0, 1),
            sample(100, 2.0, 1),
            sample(200, 10.0, 1),
            sample(300, 3.0, 1),
            sample(301, 9999.0, 1),
        ];
        let capture = capture_stats(&samples, Some((100, 300))).unwrap();
        assert_eq!(
            (capture.min, capture.max, capture.last.value),
            (2.0, 10.0, 3.0)
        );
        assert_eq!(display_value(true, Some(9999.0), Some(capture)), Some(3.0));
        assert_eq!(display_value(true, Some(9999.0), None), None);
        assert_eq!(
            display_value(false, Some(9999.0), Some(capture)),
            Some(9999.0)
        );
        assert_eq!(capture_stats(&samples, None), None);
        assert_eq!(capture_stats(&samples, Some((400, 500))), None);
    }

    #[test]
    fn tiny_input_changes_and_extreme_finite_values_keep_finite_visible_geometry() {
        let tiny = trend_geometry(&[sample(0, 0.003489, 1), sample(1, 0.00348901, 1)], None);
        let numbers: Vec<f64> = tiny
            .path
            .split(['M', 'L', ',', ' '])
            .filter_map(|s| s.parse().ok())
            .collect();
        assert!(numbers.iter().all(|n| n.is_finite()));
        assert!((numbers[1] - numbers[3]).abs() > 30.0);
        let extreme = trend_geometry(&[sample(0, -f32::MAX, 1), sample(1, f32::MAX, 1)], None);
        assert!(!extreme.path.contains("NaN") && !extreme.path.contains("inf"));
        assert_eq!(extreme.path.matches('L').count(), 1);
    }

    #[test]
    fn capture_axis_and_segment_breaks_match_the_history_timeline() {
        let samples = [
            sample(1000, 1.0, 1),
            sample(1001, 2.0, 1),
            sample(21000, 3.0, 2),
            sample(21001, 4.0, 2),
        ];
        let geometry = trend_geometry(&samples, Some((1000, 31000)));
        assert_eq!(geometry.path.matches('M').count(), 2);
        assert_eq!(geometry.path.matches('L').count(), 2);
        assert!(geometry.path.contains("M0.00,"));
        assert!(geometry.path.contains("M173.33,"));
        assert!(geometry.isolated.is_empty());
        let short = [
            sample(1000, 1.0, 1),
            sample(1001, 2.0, 1),
            sample(1003, 3.0, 2),
            sample(1004, 4.0, 2),
        ];
        let geometry = trend_geometry(&short, None);
        assert_eq!(geometry.path.matches('M').count(), 2);
        assert_eq!(geometry.path.matches('L').count(), 2);
    }

    #[test]
    fn isolated_points_remain_visible_and_missing_samples_do_not_join_a_gap() {
        let geometry = trend_geometry(
            &[
                sample(0, 1.0, 1),
                sample(1, f32::NAN, 1),
                sample(2, 2.0, 1),
                sample(3, 3.0, 1),
                sample(20_000, 4.0, 1),
            ],
            Some((0, 30_000)),
        );
        assert_eq!(geometry.path.matches('M').count(), 3);
        assert_eq!(geometry.path.matches('L').count(), 1);
        assert_eq!(geometry.isolated.len(), 2);
        let none = trend_geometry(&[sample(0, 1.0, 1)], Some((1, 2)));
        assert!(none.path.is_empty() && none.isolated.is_empty());
    }
}
