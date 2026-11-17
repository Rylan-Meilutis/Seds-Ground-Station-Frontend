use std::collections::BTreeSet;

const CAPTURE_STALL_MS: u64 = 2_000;

/// A poll of the latest value is not a new sensor observation. Keep receipt
/// freshness separate from identity so retransmission cannot add sample weight.
pub(super) struct CaptureObservations {
    seen: BTreeSet<(String, i64)>,
    last_progress_ms: u64,
}

impl CaptureObservations {
    pub(super) fn new(now_mono_ms: u64) -> Self {
        Self {
            seen: BTreeSet::new(),
            last_progress_ms: now_mono_ms,
        }
    }

    pub(super) fn stalled(&self, now_mono_ms: u64) -> bool {
        now_mono_ms.saturating_sub(self.last_progress_ms) >= CAPTURE_STALL_MS
    }

    pub(super) fn accept(
        &mut self,
        sender: &str,
        observation_ms: i64,
        received_ms: i64,
        now_wall_ms: i64,
        now_mono_ms: u64,
        raw: f32,
    ) -> Result<bool, &'static str> {
        if !raw.is_finite() {
            return Err("The sensor returned an invalid raw reading. Calibration was not changed.");
        }
        let age_ms = now_wall_ms.checked_sub(received_ms);
        if !age_ms.is_some_and(|age| (0..=CAPTURE_STALL_MS as i64).contains(&age)) {
            return Err("Fresh sensor readings required. Calibration was not changed.");
        }
        if !self.seen.insert((sender.to_owned(), observation_ms)) {
            return Ok(false);
        }
        self.last_progress_ms = now_mono_ms;
        Ok(true)
    }
}

/// MCP3564R nominal full scale at unity gain: +/-2^23 codes is +/-VREF. Codes
/// beyond it are over-range. The 2026-11-16 test fire showed the KG1000 input
/// pinned at code 0xB40000 (raw 0.19427338), which is past this limit.
const ADC_FULL_SCALE_CODES: f64 = 8_388_608.0;
/// DAQ `mcp3564r_code_to_raw_value()`: the historical KG1000/AMP2 raw scale.
const AMPLIFIED_RAW_PER_CODE: f64 = 2.2104 / 16_777_216.0 * 2.0 / 16.0;
/// DAQ P6 differential input: code * 2.4 V / 2^23 / PGA 16, in volts.
const P6_RAW_PER_CODE: f64 = 2.4 / 8_388_608.0 / 16.0;
/// KG1000 amplifier output floor. This is an analog limit, so it cannot be
/// derived from the ADC: the test fire showed the amplifier stuck at raw
/// 0.0012-0.0021 (zero load is about 0.0120), reading as -95 to -105 kg.
const KG1000_LOW_RAIL_RAW: f32 = 0.0025;
/// Robust spread limits (median absolute deviation, raw units). A quiet KG1000
/// zero has MAD near 2e-5 (about 0.2 kg); 2.5e-4 is about 2.4 kg.
const KG1000_MAX_MAD_RAW: f32 = 2.5e-4;
/// P6 noise was about 4e-6 V (0.09 kg); 2e-5 V is about 0.45 kg.
const KG50_P6_MAX_MAD_RAW: f32 = 2.0e-5;
/// No measured AMP2 KG50 scale yet: allow 0.1% of the ADC full scale.
const KG50_AMP2_MAX_MAD_RAW: f32 = (ADC_FULL_SCALE_CODES * AMPLIFIED_RAW_PER_CODE * 0.001) as f32;
/// More than this fraction of outlier samples means the input was not steady.
const MAX_OUTLIER_FRACTION: f32 = 0.10;
const OUTLIER_FLOOR: f32 = 0.000001;
const OUTLIER_SCALE: f32 = 3.5;
const TRIM_RATIO: f32 = 0.12;

/// Raw values at or beyond these limits come from a clipped input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RailLimits {
    pub(super) low: f32,
    pub(super) high: f32,
}

impl RailLimits {
    pub(super) fn railed(&self, raw: f32) -> bool {
        !raw.is_finite() || raw <= self.low || raw >= self.high
    }
}

/// Per-sensor capture limits for DAQ load cells. `kg50_input` is the
/// calibration file's selected KG50 input ("amp2" or "p6_differential").
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CaptureGuard {
    pub(super) rails: RailLimits,
    pub(super) max_mad: f32,
}

pub(super) fn load_cell_capture_guard(data_type: &str, kg50_input: &str) -> Option<CaptureGuard> {
    let amplified_full_scale = (ADC_FULL_SCALE_CODES * AMPLIFIED_RAW_PER_CODE) as f32;
    match data_type {
        "KG1000" => Some(CaptureGuard {
            rails: RailLimits { low: KG1000_LOW_RAIL_RAW, high: amplified_full_scale },
            max_mad: KG1000_MAX_MAD_RAW,
        }),
        "KG50" if kg50_input == "p6_differential" => {
            let full_scale = (ADC_FULL_SCALE_CODES * P6_RAW_PER_CODE) as f32;
            Some(CaptureGuard {
                rails: RailLimits { low: -full_scale, high: full_scale },
                max_mad: KG50_P6_MAX_MAD_RAW,
            })
        }
        "KG50" => Some(CaptureGuard {
            rails: RailLimits { low: -amplified_full_scale, high: amplified_full_scale },
            max_mad: KG50_AMP2_MAX_MAD_RAW,
        }),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CaptureSummary {
    pub(super) mean: f32,
    pub(super) used: usize,
    pub(super) total: usize,
    pub(super) rejected: usize,
    pub(super) min: f32,
    pub(super) max: f32,
    pub(super) mad: f32,
}

impl CaptureSummary {
    pub(super) fn stats_text(&self) -> String {
        format!("min {:.7}, max {:.7}, MAD {:.3e}", self.min, self.max, self.mad)
    }
}

fn median_of_sorted(values: &[f32]) -> f32 {
    let len = values.len();
    if len % 2 == 0 {
        (values[len / 2 - 1] + values[len / 2]) * 0.5
    } else {
        values[len / 2]
    }
}

/// Robust average of a capture. `raw` are the samples as received (checked
/// against the rails); `samples` are the same samples after any thermal
/// correction and are what gets averaged. With a guard, a clipped sample, too
/// many outliers or too large a spread aborts the capture.
pub(super) fn summarize_capture(
    raw: &[f32],
    samples: &[f32],
    guard: Option<&CaptureGuard>,
) -> Result<CaptureSummary, String> {
    if samples.is_empty() {
        return Err("No live raw samples were available during capture.".to_string());
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f32::total_cmp);
    let total = sorted.len();
    let (min, max) = (sorted[0], sorted[total - 1]);
    if let Some(guard) = guard {
        let railed = raw.iter().filter(|value| guard.rails.railed(**value)).count();
        if railed > 0 {
            let raw_min = raw.iter().copied().fold(f32::INFINITY, f32::min);
            let raw_max = raw.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            return Err(format!(
                "Capture rejected: {railed} of {} samples were at the sensor's ADC/amplifier rail (raw min {raw_min:.7}, max {raw_max:.7}; usable range {:.7} to {:.7}). The input is clipped, so the reading does not reflect the load. Check the load cell wiring and supply, then capture again. Calibration was not changed.",
                raw.len(), guard.rails.low, guard.rails.high,
            ));
        }
    }
    let median = median_of_sorted(&sorted);
    let mut deviations: Vec<f32> = sorted.iter().map(|value| (value - median).abs()).collect();
    deviations.sort_by(f32::total_cmp);
    let mad = median_of_sorted(&deviations);
    let outlier_limit = (mad * OUTLIER_SCALE).max(OUTLIER_FLOOR);
    let mut filtered: Vec<f32> = sorted
        .iter()
        .copied()
        .filter(|value| (value - median).abs() <= outlier_limit)
        .collect();
    let rejected = total - filtered.len();
    if let Some(guard) = guard {
        let stats = format!("min {min:.7}, max {max:.7}, MAD {mad:.3e}");
        if rejected as f32 > total as f32 * MAX_OUTLIER_FRACTION {
            return Err(format!(
                "Capture rejected: {rejected} of {total} samples ({:.0}%) were outliers; at most {:.0}% may be rejected ({stats}). The reading was not steady. Let the load settle and capture again. Calibration was not changed.",
                rejected as f32 * 100.0 / total as f32, MAX_OUTLIER_FRACTION * 100.0,
            ));
        }
        if !(mad <= guard.max_mad) {
            return Err(format!(
                "Capture rejected: the reading was too noisy (MAD {mad:.3e} exceeds the {:.3e} limit; {stats}). Let the load settle and capture again. Calibration was not changed.",
                guard.max_mad,
            ));
        }
    }
    if filtered.is_empty() {
        filtered = sorted.clone();
    }
    let trim = ((filtered.len() as f32) * TRIM_RATIO).floor() as usize;
    let usable = if filtered.len() > trim * 2 + 2 {
        &filtered[trim..(filtered.len() - trim)]
    } else {
        filtered.as_slice()
    };
    let mean = usable.iter().copied().sum::<f32>() / usable.len() as f32;
    Ok(CaptureSummary { mean, used: usable.len(), total, rejected, min, max, mad })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_poll_and_retransmission_never_count_twice_or_extend_capture() {
        let mut capture = CaptureObservations::new(0);
        assert!(capture.accept("DAQ", 10, 100, 100, 0, 5.0).unwrap());
        for elapsed in (20..=2_000).step_by(20) {
            // Even a retry with a new receipt time is the same observation.
            assert!(
                !capture
                    .accept(
                        "DAQ",
                        10,
                        100 + elapsed as i64,
                        100 + elapsed as i64,
                        elapsed,
                        5.0
                    )
                    .unwrap()
            );
        }
        assert!(capture.stalled(2_000));
    }

    #[test]
    fn stable_values_from_distinct_observations_count_normally() {
        let mut capture = CaptureObservations::new(0);
        for elapsed in (0..4_000).step_by(20) {
            assert!(
                capture
                    .accept(
                        "DAQ",
                        elapsed as i64,
                        elapsed as i64,
                        elapsed as i64,
                        elapsed,
                        5.0
                    )
                    .unwrap()
            );
            assert!(!capture.stalled(elapsed));
        }
        assert_eq!(capture.seen.len(), 200);
    }

    #[test]
    fn delayed_old_observation_cannot_count_again_after_new_data() {
        let mut capture = CaptureObservations::new(0);
        assert!(capture.accept("DAQ", 10, 100, 100, 0, 5.0).unwrap());
        assert!(capture.accept("DAQ", 11, 120, 120, 20, 6.0).unwrap());
        assert!(!capture.accept("DAQ", 10, 140, 140, 40, 5.0).unwrap());
        assert_eq!(capture.seen.len(), 2);
    }

    #[test]
    fn stale_future_and_invalid_samples_never_enter_average() {
        let mut capture = CaptureObservations::new(0);
        assert!(capture.accept("DAQ", 1, 100, 2_101, 0, 5.0).is_err());
        assert!(capture.accept("DAQ", 2, 101, 100, 0, 5.0).is_err());
        assert!(capture.accept("DAQ", 3, 100, 100, 0, f32::NAN).is_err());
        assert!(
            capture
                .accept("DAQ", 4, 100, 100, 0, f32::INFINITY)
                .is_err()
        );
        assert!(capture.seen.is_empty());
    }

    #[test]
    fn stall_deadline_uses_monotonic_time_and_covers_no_data() {
        let capture = CaptureObservations::new(500);
        assert!(!capture.stalled(2_499));
        assert!(capture.stalled(2_500));
    }

    const CLIP_RAW: f32 = 0.19427338; // DAQ code 0xB40000, test fire 2026-11-16

    fn noisy(center: f32, step: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| center + step * ((i % 7) as f32 - 3.0)).collect()
    }

    #[test]
    fn rails_follow_the_adc_full_scale_and_kg1000_amplifier_floor() {
        let kg1000 = load_cell_capture_guard("KG1000", "amp2").unwrap();
        assert!((kg1000.rails.high - 0.1381500).abs() < 1e-6);
        assert!(kg1000.rails.railed(CLIP_RAW));
        assert!(kg1000.rails.railed(0.0012) && kg1000.rails.railed(0.0021));
        assert!(!kg1000.rails.railed(0.01205) && !kg1000.rails.railed(0.06));
        let p6 = load_cell_capture_guard("KG50", "p6_differential").unwrap();
        assert!((p6.rails.high - 0.15).abs() < 1e-6 && (p6.rails.low + 0.15).abs() < 1e-6);
        assert!(!p6.rails.railed(0.0042510) && !p6.rails.railed(-0.0128));
        assert!(p6.rails.railed(0.2109375) && p6.rails.railed(-0.2109375));
        assert!(load_cell_capture_guard("KG50", "amp2").unwrap().rails.railed(CLIP_RAW));
        assert!(load_cell_capture_guard("FUEL_TANK_PRESSURE", "amp2").is_none());
    }

    #[test]
    fn a_single_clipped_sample_aborts_the_capture() {
        let guard = load_cell_capture_guard("KG1000", "amp2").unwrap();
        let mut raw = noisy(0.01205, 1e-6, 200);
        raw[17] = CLIP_RAW;
        let err = summarize_capture(&raw, &raw, Some(&guard)).unwrap_err();
        assert!(err.contains("1 of 200") && err.contains("rail") && err.contains("not changed"), "{err}");
        // Mostly railed data used to produce MAD 0 and return the rail value.
        let railed = vec![CLIP_RAW; 200];
        assert!(summarize_capture(&railed, &railed, Some(&guard)).is_err());
        let low = vec![0.001935; 200];
        assert!(summarize_capture(&low, &low, Some(&guard)).is_err());
    }

    #[test]
    fn clipping_is_checked_on_raw_before_thermal_correction() {
        let guard = load_cell_capture_guard("KG1000", "amp2").unwrap();
        let raw = vec![CLIP_RAW; 50];
        let corrected = vec![0.05; 50];
        assert!(summarize_capture(&raw, &corrected, Some(&guard)).is_err());
    }

    #[test]
    fn more_than_ten_percent_outliers_aborts() {
        let guard = load_cell_capture_guard("KG1000", "amp2").unwrap();
        let mut raw = noisy(0.01205, 1e-6, 100);
        for value in raw.iter_mut().take(11) {
            *value = 0.0125;
        }
        let err = summarize_capture(&raw, &raw, Some(&guard)).unwrap_err();
        assert!(err.contains("11 of 100") && err.contains("outliers") && err.contains("MAD"), "{err}");
        for value in raw.iter_mut().take(11).skip(10) {
            *value = 0.01205;
        }
        let ok = summarize_capture(&raw, &raw, Some(&guard)).unwrap();
        assert_eq!(ok.rejected, 10);
        assert!((ok.mean - 0.01205).abs() < 5e-6);
    }

    #[test]
    fn excessive_spread_aborts_and_stats_are_reported() {
        let guard = load_cell_capture_guard("KG50", "p6_differential").unwrap();
        let wide = noisy(0.00425, 4e-5, 140);
        let err = summarize_capture(&wide, &wide, Some(&guard)).unwrap_err();
        assert!(err.contains("too noisy") && err.contains("min") && err.contains("max"), "{err}");
        let quiet = noisy(0.00425, 2e-6, 140);
        let ok = summarize_capture(&quiet, &quiet, Some(&guard)).unwrap();
        assert!((ok.mean - 0.00425).abs() < 5e-6);
        assert_eq!((ok.total, ok.rejected), (140, 0));
        assert!(ok.min < ok.max && ok.mad > 0.0);
        assert!(ok.stats_text().contains("MAD"));
    }

    #[test]
    fn unguarded_sensors_keep_the_previous_robust_average() {
        let mut values = vec![10.0; 50];
        values.extend([1000.0; 20]);
        let ok = summarize_capture(&values, &values, None).unwrap();
        assert_eq!(ok.mean, 10.0);
        assert_eq!(ok.rejected, 20);
        assert!(summarize_capture(&[], &[], None).is_err());
    }
}
