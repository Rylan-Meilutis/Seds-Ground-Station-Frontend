//! Display-only screening. A finite isolated impulse is not proof of bad data.
//! Keep ambiguous peaks visible and count them; discard only non-finite values.
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct AccelDisplayFilter {
    history: VecDeque<Vec<Option<f32>>>,
    last_time: Option<i64>,
    pub suspicious: usize,
    pub invalid: usize,
}

fn median(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

impl AccelDisplayFilter {
    /// Causal screening: return every sample immediately, using only past data.
    pub fn push(
        &mut self,
        time: i64,
        mut values: Vec<Option<f32>>,
    ) -> Option<(i64, Vec<Option<f32>>)> {
        for value in &mut values {
            if value.is_some_and(|v| !v.is_finite()) {
                *value = None;
                self.invalid += 1;
            }
        }
        // Do not compare separate acquisition sessions across a link outage.
        if self
            .last_time
            .is_some_and(|previous| time < previous || time.saturating_sub(previous) > 2000)
        {
            self.history.clear();
        }
        self.last_time = Some(time);
        if self.history.len() == 3 {
            let mut excursions = Vec::new();
            for (channel, value) in values.iter().enumerate() {
                let Some(value) = value else {
                    continue;
                };
                let mut neighbors: Vec<f32> = self
                    .history
                    .iter()
                    .filter_map(|v| v.get(channel).copied().flatten())
                    .collect();
                if neighbors.len() != 3 {
                    continue;
                }
                let baseline = median(&mut neighbors);
                let mut deviations: Vec<_> =
                    neighbors.iter().map(|v| (v - baseline).abs()).collect();
                // 0.1 m/s² avoids declaring quantization/noise on a flat axis a peak.
                let threshold = (6.0 * 1.4826 * median(&mut deviations)).max(0.1);
                if (value - baseline).abs() > threshold {
                    let supported = neighbors.iter().any(|v| {
                        (v - baseline).signum() == (value - baseline).signum()
                            && (v - baseline).abs() >= (value - baseline).abs() * 0.25
                    });
                    excursions.push(supported);
                }
            }
            // Multiple axes provide corroboration, not a reason to erase data.
            if excursions.len() == 1 && !excursions[0] {
                self.suspicious += 1;
            }
        }
        self.history.push_back(values.clone());
        if self.history.len() > 3 {
            self.history.pop_front();
        }
        Some((time, values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(values: &[f32]) -> (AccelDisplayFilter, Vec<f32>) {
        let mut filter = AccelDisplayFilter::default();
        let output = values
            .iter()
            .enumerate()
            .filter_map(|(i, value)| {
                filter
                    .push(i as i64 * 200, vec![Some(*value)])
                    .and_then(|(_, v)| v[0])
            })
            .collect();
        (filter, output)
    }
    #[test]
    fn ambiguous_impulse_is_flagged_not_erased() {
        let (filter, output) = run(&[0., 0., 0., 20., 0., 0., 0., 0.]);
        assert_eq!(filter.suspicious, 1);
        assert!(output.contains(&20.));
    }
    #[test]
    fn sustained_peak_is_preserved() {
        let (filter, output) = run(&[0., 0., 0., 10., 20., 10., 0., 0., 0., 0.]);
        // The onset can be suspicious without future context, but is never hidden.
        assert!(filter.suspicious > 0);
        assert!(output.contains(&20.));
    }
    #[test]
    fn invalid_values_are_removed_without_zero_substitution() {
        let (filter, output) = run(&[9.8, 9.8, 9.8, f32::NAN, 9.8, 9.8, 9.8, 9.8]);
        assert_eq!(filter.invalid, 1);
        assert!(output.iter().all(|v| *v == 9.8));
    }

    #[test]
    fn multi_axis_impulse_is_corroborated() {
        let mut filter = AccelDisplayFilter::default();
        for i in 0..8 {
            let value = if i == 3 { 20.0 } else { 0.0 };
            filter.push(i * 200, vec![Some(value), Some(value)]);
        }
        assert_eq!(filter.suspicious, 0);
    }

    #[test]
    fn queue_is_bounded_and_missing_neighbors_do_not_authorize_rejection() {
        let mut filter = AccelDisplayFilter::default();
        for i in 0..10000 {
            filter.push(i * 200, vec![None, Some(9.8)]);
            assert!(filter.history.len() <= 3);
            assert_eq!(filter.last_time, Some(i * 200));
        }
        assert_eq!(filter.suspicious, 0);
        assert_eq!(filter.invalid, 0);
    }

    #[test]
    fn first_and_last_sample_are_returned_without_lookahead() {
        let mut filter = AccelDisplayFilter::default();
        for i in 0..20 {
            assert_eq!(
                filter.push(i * 200, vec![Some(i as f32)]),
                Some((i * 200, vec![Some(i as f32)]))
            );
        }
    }
}
