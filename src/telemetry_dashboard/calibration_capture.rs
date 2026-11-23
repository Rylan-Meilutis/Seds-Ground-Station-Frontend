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

/// The DAQ's historical KG1000/AMP2 "raw" scale was
/// `code * 2.2104 / 2^24 * 2 / 16` (= code * 2.2104 / 2^27). DAQ load cells now
/// report ADC-pin volts, `code * 2.4 / 2^23` (internal 2.4 V reference, unity
/// gain), so volts = legacy raw * (2.4 / 2^23) / (2.2104 / 2^27)
/// = 38.4 / 2.2104 = 16000 / 921 exactly.
pub(super) const LEGACY_RAW_TO_VOLTS: f64 = 16000.0 / 921.0;
/// MCP3564R internal reference. At unity gain +/-2^23 codes is +/-VREF, so the
/// nominal full scale of KG1000 and AMP2 KG50 is 2.4 V at the ADC pin; codes
/// beyond it are over-range. The 2026-11-16 test fire showed the KG1000 input
/// pinned at code 0xB40000 (3.375 V; legacy raw 0.19427338), past this limit.
const ADC_FULL_SCALE_V: f64 = 2.4;
/// P6 differential input is measured with the analog PGA at x16, so its
/// full scale at the bridge is +/-2.4 V / 16 = +/-0.15 V.
const P6_PGA_GAIN: f64 = 16.0;
/// KG1000 amplifier output floor. This is an analog limit, so it cannot be
/// derived from the ADC: the test fire showed the amplifier stuck at legacy
/// raw 0.0012-0.0021 (zero load is about raw 0.0120 = 0.209 V), reading as
/// -95 to -105 kg. The legacy limit was raw 0.0025; in volts 0.0025 * k
/// (about 0.04343 V).
const KG1000_LOW_RAIL_V: f64 = 0.0025 * LEGACY_RAW_TO_VOLTS;
/// Robust spread limits (median absolute deviation, volts). A quiet KG1000
/// zero has MAD near legacy raw 2e-5 (about 0.2 kg); the limit stays at legacy
/// raw 2.5e-4 (about 2.4 kg), i.e. 2.5e-4 * k (about 4.343 mV).
const KG1000_MAX_MAD_V: f64 = 2.5e-4 * LEGACY_RAW_TO_VOLTS;
/// P6 noise was about 4e-6 V (0.09 kg); 2e-5 V is about 0.45 kg.
const KG50_P6_MAX_MAD_V: f64 = 2.0e-5;
/// No measured AMP2 KG50 scale yet: allow 0.1% of the ADC full scale (2.4 mV).
const KG50_AMP2_MAX_MAD_V: f64 = ADC_FULL_SCALE_V * 0.001;
/// More than this fraction of outlier samples means the input was not steady.
const MAX_OUTLIER_FRACTION: f32 = 0.10;
/// Smallest outlier band. It was 1e-6 in each input's own units; amplified
/// inputs keep the same physical band (1e-6 legacy raw = 1e-6 * k volts).
const OUTLIER_FLOOR: f32 = 0.000001;
const OUTLIER_SCALE: f32 = 3.5;
const TRIM_RATIO: f32 = 0.12;
/// Largest change between the means of the first and last thirds of one
/// capture (about 5 s). A reading still settling after a mass is placed, or
/// creeping through a binding mount, averages to a value that depends on when
/// the operator pressed Capture; field calibrations showed 5 kg steps reading
/// 0.43x to 1.35x of the fitted slope. KG1000: 1 mV is about 0.55 kg.
const KG1000_MAX_SETTLE_V: f64 = 1.0e-3;
/// P6: 1e-5 V is about 0.23 kg on the field KG50 calibration (~22.8 kg/mV).
const KG50_P6_MAX_SETTLE_V: f64 = 1.0e-5;
/// No measured AMP2 KG50 scale: same 0.1% of full scale as its MAD limit.
const KG50_AMP2_MAX_SETTLE_V: f64 = ADC_FULL_SCALE_V * 0.001;
/// A calibration step whose sensitivity differs from the whole fit by more
/// than this factor points at the load path (binding, bypass, seating) or at
/// an unsettled capture, not at the electronics: at-rest noise is ~0.1 kg.
const STEP_RATIO_LOW: f32 = 0.8;
const STEP_RATIO_HIGH: f32 = 1.25;

/// How a sensor reading is shown and entered. Calibration files and the wire
/// always carry volts for load cells; only the display may use millivolts.
/// Rule: amplified load-cell inputs (KG1000, AMP2 KG50; 0..2.4 V) use V with
/// 6 decimals (1 uV); the P6 differential KG50 input (+/-150 mV) uses mV with
/// 4 decimals (0.1 uV). Spreads (MAD, noise) are always shown in mV. Other
/// sensors keep their unitless raw value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReadingUnit {
    Volts,
    Millivolts,
    Raw,
}

pub(super) fn reading_unit(data_type: &str, kg50_input: &str) -> ReadingUnit {
    match data_type {
        "KG1000" => ReadingUnit::Volts,
        "KG50" if kg50_input == "p6_differential" => ReadingUnit::Millivolts,
        "KG50" => ReadingUnit::Volts,
        _ => ReadingUnit::Raw,
    }
}

impl ReadingUnit {
    pub(super) fn is_voltage(self) -> bool {
        self != ReadingUnit::Raw
    }
    /// Unit label for headings and inputs ("raw" for unitless sensors).
    pub(super) fn label(self) -> &'static str {
        match self {
            ReadingUnit::Volts => "V",
            ReadingUnit::Millivolts => "mV",
            ReadingUnit::Raw => "raw",
        }
    }
    fn per_volt(self) -> f64 {
        if self == ReadingUnit::Millivolts { 1000.0 } else { 1.0 }
    }
    fn decimals(self, raw_precision: usize) -> usize {
        match self {
            ReadingUnit::Volts => 6,
            ReadingUnit::Millivolts => 4,
            ReadingUnit::Raw => raw_precision,
        }
    }
    /// Text for an input field, in display units and without a suffix.
    pub(super) fn entry_text(self, value: f32, raw_precision: usize) -> String {
        let shown = value as f64 * self.per_volt();
        format!("{shown:.prec$}", prec = self.decimals(raw_precision))
    }
    /// Parse an input field in display units back to stored units (volts for
    /// load cells). Rejects non-finite input.
    pub(super) fn parse_entry(self, text: &str) -> Option<f32> {
        let shown = text.trim().parse::<f64>().ok()?;
        let value = (shown / self.per_volt()) as f32;
        value.is_finite().then_some(value)
    }
    /// A reading with its unit, e.g. "0.209346 V" or "4.2533 mV".
    pub(super) fn format(self, value: f32, raw_precision: usize) -> String {
        match self {
            ReadingUnit::Raw => format!("{value:.raw_precision$}"),
            _ => format!("{} {}", self.entry_text(value, raw_precision), self.label()),
        }
    }
    /// A spread or noise level: mV for voltages, raw otherwise.
    pub(super) fn format_spread(self, value: f32) -> String {
        if self.is_voltage() {
            format!("{:.5} mV", value as f64 * 1000.0)
        } else {
            format!("{value:.3e}")
        }
    }
}

/// Values at or beyond these limits come from a clipped input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RailLimits {
    pub(super) low: f32,
    pub(super) high: f32,
}

impl RailLimits {
    pub(super) fn railed(&self, value: f32) -> bool {
        !value.is_finite() || value <= self.low || value >= self.high
    }
}

/// Per-sensor capture limits for DAQ load cells, in volts. `kg50_input` is the
/// calibration file's selected KG50 input ("amp2" or "p6_differential").
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CaptureGuard {
    pub(super) rails: RailLimits,
    pub(super) max_mad: f32,
    /// Largest first-third to last-third change of the capture mean.
    pub(super) max_settle: f32,
    pub(super) outlier_floor: f32,
    pub(super) unit: ReadingUnit,
}

pub(super) fn load_cell_capture_guard(data_type: &str, kg50_input: &str) -> Option<CaptureGuard> {
    let unit = reading_unit(data_type, kg50_input);
    let amplified_floor = (OUTLIER_FLOOR as f64 * LEGACY_RAW_TO_VOLTS) as f32;
    let full_scale = ADC_FULL_SCALE_V as f32;
    match data_type {
        "KG1000" => Some(CaptureGuard {
            rails: RailLimits { low: KG1000_LOW_RAIL_V as f32, high: full_scale },
            max_mad: KG1000_MAX_MAD_V as f32,
            max_settle: KG1000_MAX_SETTLE_V as f32,
            outlier_floor: amplified_floor,
            unit,
        }),
        "KG50" if kg50_input == "p6_differential" => {
            let full_scale = (ADC_FULL_SCALE_V / P6_PGA_GAIN) as f32;
            Some(CaptureGuard {
                rails: RailLimits { low: -full_scale, high: full_scale },
                max_mad: KG50_P6_MAX_MAD_V as f32,
                max_settle: KG50_P6_MAX_SETTLE_V as f32,
                outlier_floor: OUTLIER_FLOOR,
                unit,
            })
        }
        "KG50" => Some(CaptureGuard {
            rails: RailLimits { low: -full_scale, high: full_scale },
            max_mad: KG50_AMP2_MAX_MAD_V as f32,
            max_settle: KG50_AMP2_MAX_SETTLE_V as f32,
            outlier_floor: amplified_floor,
            unit,
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
    pub(super) fn stats_text(&self, unit: ReadingUnit) -> String {
        stats_text(unit, self.min, self.max, self.mad)
    }
}

fn stats_text(unit: ReadingUnit, min: f32, max: f32, mad: f32) -> String {
    format!(
        "min {}, max {}, MAD {}",
        unit.format(min, 7),
        unit.format(max, 7),
        unit.format_spread(mad)
    )
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
        return Err("No live samples were available during capture.".to_string());
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
            let unit = guard.unit;
            return Err(format!(
                "Capture rejected: {railed} of {} samples were at the sensor's ADC/amplifier rail (min {}, max {}; usable range {} to {}). The input is clipped, so the reading does not reflect the load. Check the load cell wiring and supply, then capture again. Calibration was not changed.",
                raw.len(),
                unit.format(raw_min, 7),
                unit.format(raw_max, 7),
                unit.format(guard.rails.low, 7),
                unit.format(guard.rails.high, 7),
            ));
        }
    }
    let median = median_of_sorted(&sorted);
    let mut deviations: Vec<f32> = sorted.iter().map(|value| (value - median).abs()).collect();
    deviations.sort_by(f32::total_cmp);
    let mad = median_of_sorted(&deviations);
    let floor = guard.map_or(OUTLIER_FLOOR, |guard| guard.outlier_floor);
    let outlier_limit = (mad * OUTLIER_SCALE).max(floor);
    let mut filtered: Vec<f32> = sorted
        .iter()
        .copied()
        .filter(|value| (value - median).abs() <= outlier_limit)
        .collect();
    let rejected = total - filtered.len();
    if let Some(guard) = guard {
        let stats = stats_text(guard.unit, min, max, mad);
        if rejected as f32 > total as f32 * MAX_OUTLIER_FRACTION {
            return Err(format!(
                "Capture rejected: {rejected} of {total} samples ({:.0}%) were outliers; at most {:.0}% may be rejected ({stats}). The reading was not steady. Let the load settle and capture again. Calibration was not changed.",
                rejected as f32 * 100.0 / total as f32, MAX_OUTLIER_FRACTION * 100.0,
            ));
        }
        if !(mad <= guard.max_mad) {
            return Err(format!(
                "Capture rejected: the reading was too noisy (MAD {} exceeds the {} limit; {stats}). Let the load settle and capture again. Calibration was not changed.",
                guard.unit.format_spread(mad),
                guard.unit.format_spread(guard.max_mad),
            ));
        }
    }
    if let Some(guard) = guard {
        // Isolated outliers are already bounded above; judge the trend on the
        // samples that enter the average, in capture order.
        let kept: Vec<f32> = samples.iter().copied().filter(|value| (value - median).abs() <= outlier_limit).collect();
        let settle = settle_change(&kept);
        if !(settle.abs() <= guard.max_settle) {
            return Err(format!(
                "Capture rejected: the reading was still moving (the last third of the capture differs from the first third by {}, limit {}). Wait for the load to settle; if it keeps creeping, check the mount for binding or a line carrying load. Calibration was not changed.",
                guard.unit.format_spread(settle),
                guard.unit.format_spread(guard.max_settle),
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

/// Mean of the last third of a capture minus the mean of its first third, in
/// capture order. Zero for captures too short to split.
pub(super) fn settle_change(samples: &[f32]) -> f32 {
    let third = samples.len() / 3;
    if third == 0 {
        return 0.0;
    }
    let mean = |values: &[f32]| values.iter().map(|v| *v as f64).sum::<f64>() / values.len() as f64;
    (mean(&samples[samples.len() - third..]) - mean(&samples[..third])) as f32
}

/// Warnings for calibration points whose step-to-step sensitivity disagrees
/// with the whole least-squares fit. `points` are (reading, kg). Needs three
/// distinct loads. A consistent load path gives every step the same reading
/// per kg; a step far from the fit means binding, a load bypass, a seating
/// change or a capture taken before the reading settled.
pub(super) fn step_consistency_warnings(points: &[(f32, f32)]) -> Vec<String> {
    let mut pts: Vec<(f64, f64)> = points
        .iter()
        .filter(|(r, k)| r.is_finite() && k.is_finite())
        .map(|(r, k)| (*r as f64, *k as f64))
        .collect();
    pts.sort_by(|a, b| a.1.total_cmp(&b.1));
    pts.dedup_by(|a, b| (a.1 - b.1).abs() < 1e-6);
    if pts.len() < 3 {
        return Vec::new();
    }
    let n = pts.len() as f64;
    let (mk, mr) = (pts.iter().map(|p| p.1).sum::<f64>() / n, pts.iter().map(|p| p.0).sum::<f64>() / n);
    let sxx: f64 = pts.iter().map(|p| (p.1 - mk).powi(2)).sum();
    let sxy: f64 = pts.iter().map(|p| (p.1 - mk) * (p.0 - mr)).sum();
    let slope = sxy / sxx; // reading per kg
    if !slope.is_finite() || slope == 0.0 {
        return vec!["The points do not change with load; check the wiring and the load path before using this calibration.".to_string()];
    }
    pts.windows(2)
        .filter_map(|w| {
            let ratio = ((w[1].0 - w[0].0) / (w[1].1 - w[0].1) / slope) as f32;
            (!(STEP_RATIO_LOW..=STEP_RATIO_HIGH).contains(&ratio)).then(|| {
                format!(
                    "Step {:.3} -> {:.3} kg reads {ratio:.2}x the fitted sensitivity (expected 1.00). Re-capture after the reading settles; if it repeats, check the mount for binding or a line carrying load.",
                    w[0].1, w[1].1
                )
            })
        })
        .collect()
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

    const K: f32 = LEGACY_RAW_TO_VOLTS as f32;
    /// DAQ code 0xB40000 at the ADC pin (test fire 2026-11-16): 3.375 V.
    const CLIP_V: f32 = (0xB4_0000 as f64 * 2.4 / 8_388_608.0) as f32;
    /// KG1000 zero load, legacy raw 0.01205.
    const KG1000_ZERO_V: f32 = 0.01205 * K;

    fn noisy(center: f32, step: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| center + step * ((i % 7) as f32 - 3.0)).collect()
    }

    #[test]
    fn legacy_factor_is_the_exact_ratio_of_the_daq_formulas() {
        let volts_per_code = 2.4 / 8_388_608.0;
        let legacy_raw_per_code = 2.2104 / 16_777_216.0 * 2.0 / 16.0;
        assert!((LEGACY_RAW_TO_VOLTS - volts_per_code / legacy_raw_per_code).abs() < 1e-12);
        assert!((LEGACY_RAW_TO_VOLTS - 17.372_421_281_216_07).abs() < 1e-12);
        assert_eq!(CLIP_V, 3.375);
    }

    #[test]
    fn rails_are_in_volts_at_full_scale_and_kg1000_amplifier_floor() {
        let kg1000 = load_cell_capture_guard("KG1000", "amp2").unwrap();
        assert_eq!(kg1000.rails.high, 2.4);
        assert!((kg1000.rails.low - 0.043431).abs() < 1e-6, "{}", kg1000.rails.low);
        assert!(kg1000.rails.railed(CLIP_V) && kg1000.rails.railed(2.4));
        // Test-fire amplifier floor: legacy raw 0.0012-0.0021.
        assert!(kg1000.rails.railed(0.0012 * K) && kg1000.rails.railed(0.0021 * K));
        assert!(kg1000.rails.railed(0.0434));
        assert!(!kg1000.rails.railed(KG1000_ZERO_V) && !kg1000.rails.railed(0.06 * K) && !kg1000.rails.railed(2.39));
        let p6 = load_cell_capture_guard("KG50", "p6_differential").unwrap();
        assert!((p6.rails.high - 0.15).abs() < 1e-7 && (p6.rails.low + 0.15).abs() < 1e-7);
        assert!(!p6.rails.railed(0.0042510) && !p6.rails.railed(-0.0128));
        assert!(p6.rails.railed(0.2109375) && p6.rails.railed(-0.2109375));
        let amp2 = load_cell_capture_guard("KG50", "amp2").unwrap();
        assert_eq!((amp2.rails.low, amp2.rails.high), (-2.4, 2.4));
        assert!(amp2.rails.railed(CLIP_V) && !amp2.rails.railed(1.0));
        assert!(load_cell_capture_guard("FUEL_TANK_PRESSURE", "amp2").is_none());
    }

    #[test]
    fn mad_limits_are_in_volts() {
        let kg1000 = load_cell_capture_guard("KG1000", "amp2").unwrap();
        assert!((kg1000.max_mad - 4.3431e-3).abs() < 1e-7, "{}", kg1000.max_mad);
        assert_eq!(load_cell_capture_guard("KG50", "p6_differential").unwrap().max_mad, 2.0e-5);
        assert!((load_cell_capture_guard("KG50", "amp2").unwrap().max_mad - 2.4e-3).abs() < 1e-9);
        // A KG1000 zero with legacy-raw MAD 2e-5 (about 0.2 kg) still passes in volts.
        let quiet = noisy(KG1000_ZERO_V, 2e-5 * K, 200);
        assert!(summarize_capture(&quiet, &quiet, Some(&kg1000)).is_ok());
        let wide = noisy(KG1000_ZERO_V, 3e-3, 200);
        let err = summarize_capture(&wide, &wide, Some(&kg1000)).unwrap_err();
        assert!(err.contains("too noisy") && err.contains("4.34311 mV limit"), "{err}");
    }

    #[test]
    fn a_single_clipped_sample_aborts_the_capture() {
        let guard = load_cell_capture_guard("KG1000", "amp2").unwrap();
        let mut raw = noisy(KG1000_ZERO_V, 1e-5, 200);
        raw[17] = CLIP_V;
        let err = summarize_capture(&raw, &raw, Some(&guard)).unwrap_err();
        assert!(err.contains("1 of 200") && err.contains("rail") && err.contains("not changed"), "{err}");
        assert!(err.contains("max 3.375000 V") && err.contains("0.043431 V to 2.400000 V"), "{err}");
        // Mostly railed data used to produce MAD 0 and return the rail value.
        let railed = vec![CLIP_V; 200];
        assert!(summarize_capture(&railed, &railed, Some(&guard)).is_err());
        let low = vec![0.001935 * K; 200];
        assert!(summarize_capture(&low, &low, Some(&guard)).is_err());
    }

    #[test]
    fn clipping_is_checked_on_raw_before_thermal_correction() {
        let guard = load_cell_capture_guard("KG1000", "amp2").unwrap();
        let raw = vec![CLIP_V; 50];
        let corrected = vec![0.8; 50];
        assert!(summarize_capture(&raw, &corrected, Some(&guard)).is_err());
    }

    #[test]
    fn more_than_ten_percent_outliers_aborts() {
        let guard = load_cell_capture_guard("KG1000", "amp2").unwrap();
        let mut raw = noisy(KG1000_ZERO_V, 1e-6 * K, 100);
        for value in raw.iter_mut().take(11) {
            *value = 0.0125 * K;
        }
        let err = summarize_capture(&raw, &raw, Some(&guard)).unwrap_err();
        assert!(err.contains("11 of 100") && err.contains("outliers") && err.contains("MAD"), "{err}");
        for value in raw.iter_mut().take(11).skip(10) {
            *value = KG1000_ZERO_V;
        }
        let ok = summarize_capture(&raw, &raw, Some(&guard)).unwrap();
        assert_eq!(ok.rejected, 10);
        assert!((ok.mean - KG1000_ZERO_V).abs() < 5e-6 * K);
    }

    #[test]
    fn excessive_spread_aborts_and_stats_are_reported_in_volts() {
        let guard = load_cell_capture_guard("KG50", "p6_differential").unwrap();
        let wide = noisy(0.00425, 4e-5, 140);
        let err = summarize_capture(&wide, &wide, Some(&guard)).unwrap_err();
        assert!(err.contains("too noisy") && err.contains("min") && err.contains("max"), "{err}");
        let quiet = noisy(0.00425, 2e-6, 140);
        let ok = summarize_capture(&quiet, &quiet, Some(&guard)).unwrap();
        assert!((ok.mean - 0.00425).abs() < 5e-6);
        assert_eq!((ok.total, ok.rejected), (140, 0));
        assert!(ok.min < ok.max && ok.mad > 0.0);
        let stats = ok.stats_text(ReadingUnit::Millivolts);
        assert!(stats.contains(" mV") && stats.contains("MAD 0.00"), "{stats}");
    }

    #[test]
    fn reading_units_format_and_parse_without_ambiguity() {
        assert_eq!(reading_unit("KG1000", "p6_differential"), ReadingUnit::Volts);
        assert_eq!(reading_unit("KG50", "amp2"), ReadingUnit::Volts);
        assert_eq!(reading_unit("KG50", "p6_differential"), ReadingUnit::Millivolts);
        assert_eq!(reading_unit("FUEL_TANK_PRESSURE", "amp2"), ReadingUnit::Raw);
        let v = ReadingUnit::Volts;
        assert_eq!(v.format(0.2093456, 7), "0.209346 V");
        assert_eq!(v.entry_text(0.2093456, 7), "0.209346");
        assert_eq!(v.parse_entry(" 0.209346 "), Some(0.209346));
        let mv = ReadingUnit::Millivolts;
        assert_eq!(mv.format(0.0042533, 7), "4.2533 mV");
        // A displayed mV value round-trips back to volts.
        let entry = mv.entry_text(0.0042533, 7);
        assert!((mv.parse_entry(&entry).unwrap() - 0.0042533).abs() < 1e-9);
        assert_eq!(mv.parse_entry("150"), Some(0.15));
        assert_eq!(mv.parse_entry("abc"), None);
        assert_eq!(v.parse_entry("inf"), None);
        assert_eq!(ReadingUnit::Raw.format(1.5, 3), "1.500");
        assert_eq!(ReadingUnit::Raw.parse_entry("1.5"), Some(1.5));
        assert_eq!(v.format_spread(4.3431e-3), "4.34310 mV");
        assert_eq!(v.label(), "V");
        assert_eq!(mv.label(), "mV");
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

    #[test]
    fn a_creeping_capture_is_rejected_and_a_steady_one_passes() {
        let kg1000 = load_cell_capture_guard("KG1000", "amp2").unwrap();
        let steady = noisy(KG1000_ZERO_V, 2e-5 * K, 200);
        assert!(summarize_capture(&steady, &steady, Some(&kg1000)).is_ok());
        // 2 mV (about 1.1 kg) of creep over the capture, well inside the MAD limit.
        let creep: Vec<f32> = (0..200).map(|i| KG1000_ZERO_V + 2.0e-3 * i as f32 / 199.0).collect();
        let err = summarize_capture(&creep, &creep, Some(&kg1000)).unwrap_err();
        assert!(err.contains("still moving") && err.contains("not changed"), "{err}");
        let p6 = load_cell_capture_guard("KG50", "p6_differential").unwrap();
        let settling: Vec<f32> = (0..200).map(|i| 0.00419 + 2.0e-5 * i as f32 / 199.0).collect();
        assert!(summarize_capture(&settling, &settling, Some(&p6)).unwrap_err().contains("still moving"));
        let p6_steady = noisy(0.00419, 1e-6, 200);
        assert!(summarize_capture(&p6_steady, &p6_steady, Some(&p6)).is_ok());
        // Unguarded sensors keep their existing behaviour.
        assert!(summarize_capture(&creep, &creep, None).is_ok());
    }

    #[test]
    fn settle_change_compares_first_and_last_thirds_in_capture_order() {
        assert_eq!(settle_change(&[1.0, 1.0]), 0.0);
        assert!((settle_change(&[0.0, 0.0, 5.0, 5.0, 9.0, 9.0]) - 9.0).abs() < 1e-6);
        assert!((settle_change(&[9.0, 9.0, 5.0, 5.0, 0.0, 0.0]) + 9.0).abs() < 1e-6);
    }

    #[test]
    fn field_kg50_steps_are_flagged_and_a_linear_kg1000_set_is_not() {
        // Live GS calibration, 2026-11-22: KG1000 volts per 20.4 kg step.
        let kg1000 = [(0.24977598, 20.4), (0.28683203, 40.8), (0.32496646, 61.2), (0.36300135, 81.6), (0.398651, 102.0)];
        assert!(step_consistency_warnings(&kg1000).is_empty());
        // KG50 on P6, 5 kg steps: 1.35x, 0.43x, 1.30x of the fit.
        let kg50 = [(0.004191, 5.0), (0.004523, 10.0), (0.004628, 15.0), (0.004947, 20.0)];
        let warnings = step_consistency_warnings(&kg50);
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(warnings[1].contains("10.000 -> 15.000 kg reads 0.4"), "{}", warnings[1]);
        // The March 2026 points went down, up and down with load.
        let dead = [(-0.000944579, 2.0), (0.0015112788, 5.0), (-0.0020486573, 20.0)];
        assert!(!step_consistency_warnings(&dead).is_empty());
        assert!(step_consistency_warnings(&kg50[..2]).is_empty());
    }
}
