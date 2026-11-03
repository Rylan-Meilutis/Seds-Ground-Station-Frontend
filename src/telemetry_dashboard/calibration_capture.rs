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
}
