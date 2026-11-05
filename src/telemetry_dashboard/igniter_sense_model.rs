//! Presentation only; measured analog signal never certifies ignition/continuity.
pub(super) const CAPABILITY: &str = "Board reports no supported current conversion.";
pub(super) fn capability(status: &[Option<f32>]) -> &'static str {
    if status.get(3).copied().flatten() == Some(1.0) {
        CAPABILITY
    } else {
        "Calibrated current is unavailable or not reported by this board."
    }
}
pub(super) struct View {
    pub label: &'static str,
    pub counts: String,
    pub volts: String,
}
fn finite(values: &[Option<f32>], at: usize) -> Option<f32> {
    values.get(at).copied().flatten().filter(|n| n.is_finite())
}
fn owner(values: &[Option<f32>], start: usize) -> Option<[u16; 6]> {
    let slice = values.get(start..start + 6)?;
    let mut words = [0; 6];
    for (at, value) in slice.iter().enumerate() {
        words[at] = value
            .filter(|v| v.is_finite() && v.fract() == 0.0 && (0.0..=65535.0).contains(v))?
            as u16;
    }
    Some(words)
}
pub(super) fn view(
    status: &[Option<f32>],
    signal: &[Option<f32>],
    live_elapsed_ms: Option<u64>,
    client_utc_ms: Option<u64>,
) -> View {
    let code = finite(status, 0)
        .filter(|n| n.fract() == 0.0 && (0.0..=8.0).contains(n))
        .map(|n| n as u8);
    let source_age = finite(status, 1).filter(|n| *n >= 0.0).map(|n| n as u64);
    let backend_utc = status.get(10..14).and_then(|limbs| {
        let mut stamp = 0_u64;
        for (at, limb) in limbs.iter().enumerate() {
            let value =
                limb.filter(|v| v.is_finite() && v.fract() == 0.0 && (0.0..=65535.0).contains(v))?;
            stamp |= (value as u64) << (at * 16);
        }
        Some(stamp)
    });
    let transit = backend_utc.zip(client_utc_ms).and_then(|(backend, now)| {
        const FIRST: u64 = 1_577_836_800_000;
        const LAST: u64 = 4_102_444_800_000;
        ((FIRST..LAST).contains(&backend)
            && (FIRST..LAST).contains(&now)
            && backend <= now.saturating_add(100))
        .then_some(now.saturating_sub(backend))
    });
    let age = source_age
        .zip(live_elapsed_ms)
        .zip(transit)
        .map(|((source, elapsed), transit)| source.saturating_add(elapsed.max(transit)));
    let label = if status.is_empty() && signal.is_empty() {
        "No igniter ADC report received"
    } else if live_elapsed_ms.is_none() {
        "Recorded or unavailable · no fresh live source"
    } else if matches!(code, Some(1 | 7)) && transit.is_none() {
        "Source clock unavailable · last signal retained"
    } else if live_elapsed_ms.is_some_and(|age| age > 1000)
        || (matches!(code, Some(1 | 7)) && age.is_none_or(|age| age > 1000))
    {
        "Stale · last signal retained"
    } else if matches!(code, Some(1 | 7))
        && owner(status, 4)
            .zip(owner(signal, 3))
            .is_none_or(|(status, signal)| status != signal)
    {
        "Waiting for matching ADC sample · last signal retained"
    } else {
        match code {
            Some(0)
                if owner(status, 4).is_some_and(|words| {
                    words[..4].iter().all(|word| *word == 0)
                        && words[4..].iter().any(|word| *word != 0)
                }) =>
            {
                "ADC reports received; board identity must be commissioned before live readings can be confirmed"
            }
            Some(0) => "Waiting for an owned ADC sample",
            Some(1) => "Live measured signal",
            Some(2) => "Stale · last signal retained",
            Some(3) => "ADC unavailable · acquisition fault",
            Some(4) => "Raw only · reference unavailable",
            Some(5) => "Source clock unavailable",
            Some(6) => "Retired sample · waiting for current owner",
            Some(7) => "ADC at rail · check signal range",
            _ => "Unsupported measurement status",
        }
    };
    let counts = finite(signal, 0)
        .filter(|n| n.fract() == 0.0 && (0.0..=4095.0).contains(n))
        .map(|n| format!("{n:.0} counts"))
        .unwrap_or_else(|| "—".into());
    let volts = finite(signal, 1)
        .filter(|n| (0.0..=3.6).contains(n))
        .map(|n| format!("{n:.4} V"))
        .unwrap_or_else(|| "—".into());
    View {
        label,
        counts,
        volts,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn tagged(values: &[Option<f32>]) -> Vec<Option<f32>> {
        values
            .iter()
            .copied()
            .chain([
                Some(4.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(1.0),
                Some(0.0),
            ])
            .collect()
    }
    const NOW: u64 = 1_770_000_000_000;
    fn status(code: f32, age: f32) -> Vec<Option<f32>> {
        let mut values = tagged(&[Some(code), Some(age), Some(0.0), Some(1.0)]);
        values.extend(
            [0_u32, 16, 32, 48]
                .into_iter()
                .map(|shift| Some(((NOW >> shift) & 0xffff) as f32)),
        );
        values
    }
    fn signal() -> Vec<Option<f32>> {
        tagged(&[Some(2048.0), Some(1.6504), None])
    }
    #[test]
    fn missing_report_is_explicit() {
        assert_eq!(
            view(&[], &[], None, None).label,
            "No igniter ADC report received"
        );
    }
    #[test]
    fn received_adc_without_boot_identity_requires_commissioning_not_hardware_guessing() {
        let mut report = status(0.0, 39.0);
        report[4..8].fill(Some(0.0));
        report[8] = Some(8055.0);
        let display = view(&report, &[], Some(20), Some(NOW));
        assert_eq!(
            display.label,
            "ADC reports received; board identity must be commissioned before live readings can be confirmed"
        );
        assert_eq!(display.counts, "—");
        assert_eq!(display.volts, "—");
        assert!(
            view(&report, &[], None, Some(NOW))
                .label
                .starts_with("Recorded")
        );
        assert!(
            view(&report, &[], Some(1001), Some(NOW))
                .label
                .starts_with("Stale")
        );
        report[8] = Some(0.0);
        assert_eq!(
            view(&report, &[], Some(20), Some(NOW)).label,
            "Waiting for an owned ADC sample"
        );
        report[8] = Some(8055.0);
        report[4] = Some(f32::NAN);
        assert_eq!(
            view(&report, &[], Some(20), Some(NOW)).label,
            "Waiting for an owned ADC sample"
        );
    }
    #[test]
    fn fresh_raw_voltage_is_separate_from_unavailable_current() {
        let a = view(&status(1.0, 0.0), &signal(), Some(50), Some(NOW));
        assert_eq!(a.label, "Live measured signal");
        assert_eq!(a.counts, "2048 counts");
        assert_eq!(a.volts, "1.6504 V");
        assert!(CAPABILITY.contains("no supported current conversion"));
        assert_eq!(capability(&status(1.0, 0.0)), CAPABILITY);
        assert!(capability(&[]).contains("not reported"));
    }
    #[test]
    fn history_stale_and_missing_are_never_zero_or_fresh() {
        assert!(
            view(&status(1.0, 0.0), &[], None, Some(NOW))
                .label
                .contains("Recorded")
        );
        let a = view(&status(1.0, 100.0), &[], Some(901), Some(NOW));
        assert!(a.label.starts_with("Stale"));
        assert_eq!(a.counts, "—");
        assert_eq!(a.volts, "—");
    }
    #[test]
    fn nan_infinity_and_invalid_schema_status_stay_unknown() {
        let a = view(
            &[Some(f32::NAN)],
            &[Some(f32::INFINITY), Some(f32::NEG_INFINITY)],
            Some(0),
            Some(NOW),
        );
        assert_eq!(a.label, "Unsupported measurement status");
        assert_eq!(a.counts, "—");
        assert_eq!(a.volts, "—");
        assert!(
            view(&[Some(9.0)], &[], Some(0), Some(NOW))
                .label
                .starts_with("Unsupported")
        );
    }
    #[test]
    fn exact_zero_is_a_rail_measurement_not_an_ampere_claim() {
        let a = view(
            &status(7.0, 0.0),
            &tagged(&[Some(0.0), Some(0.0), Some(999.0)]),
            Some(0),
            Some(NOW),
        );
        assert!(a.label.contains("rail"));
        assert_eq!(a.volts, "0.0000 V");
        assert!(!a.counts.contains("A"));
    }
    #[test]
    fn every_failure_status_is_explicit() {
        for code in 0..=8 {
            assert!(
                !view(&status(code as f32, 0.0), &signal(), Some(0), Some(NOW))
                    .label
                    .is_empty()
            );
        }
        assert!(
            view(&status(3.0, 0.0), &[], Some(0), Some(NOW))
                .label
                .contains("fault")
        );
    }
    #[test]
    fn unmatched_status_boot_or_sequence_cannot_certify_other_sample() {
        let mut different = signal();
        different[3] = Some(5.0);
        assert!(
            view(&status(1.0, 0.0), &different, Some(0), Some(NOW))
                .label
                .starts_with("Waiting")
        );
        different = signal();
        different[7] = Some(2.0);
        assert!(
            view(&status(1.0, 0.0), &different, Some(0), Some(NOW))
                .label
                .starts_with("Waiting")
        );
        assert!(
            view(
                &status(1.0, 0.0),
                &[Some(12.0), Some(1.0), None],
                Some(0),
                Some(NOW)
            )
            .label
            .starts_with("Waiting")
        );
    }
    #[test]
    fn delayed_or_clock_skewed_network_delivery_is_not_fresh_on_arrival() {
        assert!(
            view(&status(1.0, 0.0), &signal(), Some(0), Some(NOW + 5000))
                .label
                .starts_with("Stale")
        );
        assert!(
            view(&status(1.0, 0.0), &signal(), Some(0), Some(NOW - 1000))
                .label
                .starts_with("Source clock")
        );
        assert!(
            view(&status(1.0, 0.0), &signal(), Some(0), None)
                .label
                .starts_with("Source clock")
        );
    }
}
