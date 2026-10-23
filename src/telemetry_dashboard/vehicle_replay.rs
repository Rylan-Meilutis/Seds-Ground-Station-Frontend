//! Saved samples only. No access to the live telemetry cache or flight signals.
use super::vehicle_tab::VehicleTelemetryBinding;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, rc::Rc};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Recording {
    pub id: String,
    pub bytes: u64,
    pub active: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub upper_id: i64,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Sample {
    pub id: i64,
    pub timestamp_ms: i64,
    pub source_timestamp_ms: Option<i64>,
    pub sender_id: String,
    pub data_type: String,
    pub values: Vec<Option<f32>>,
    #[serde(default)]
    pub phase: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub upper_id: i64,
    pub next_id: i64,
    pub has_more: bool,
    pub rows: Vec<Sample>,
    pub seeds: Vec<Sample>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Index {
    series: BTreeMap<String, BTreeMap<String, Vec<Sample>>>,
}
impl Index {
    pub fn new(samples: Vec<Sample>) -> Self {
        let mut this = Self::default();
        for sample in samples {
            this.series
                .entry(sample.data_type.clone())
                .or_default()
                .entry(sample.sender_id.clone())
                .or_default()
                .push(sample);
        }
        for sources in this.series.values_mut() {
            for rows in sources.values_mut() {
                rows.sort_by_key(|row| (row.timestamp_ms, row.id));
                rows.dedup_by_key(|row| row.id);
            }
        }
        this
    }
    pub fn latest(&self, kind: &str, sender: Option<&str>, at: i64) -> Option<&Sample> {
        let sources = self.series.get(kind)?;
        fn previous(rows: &[Sample], at: i64) -> Option<&Sample> {
            let n = rows.partition_point(|row| row.timestamp_ms <= at);
            n.checked_sub(1).map(|i| &rows[i])
        }
        if let Some(sender) = sender {
            previous(sources.get(sender)?, at)
        } else {
            sources
                .values()
                .filter_map(|rows| previous(rows, at))
                .max_by_key(|row| (row.timestamp_ms, row.id))
        }
    }
}
#[derive(Clone, Default, PartialEq)]
pub struct Frame {
    pub data: Option<Rc<Index>>,
    pub at_ms: i64,
}
impl Frame {
    pub fn value(&self, binding: &VehicleTelemetryBinding, hold: bool) -> Option<f32> {
        let row = self.data.as_ref()?.latest(
            &binding.data_type,
            binding.sender_id.as_deref(),
            self.at_ms,
        )?;
        if !hold && self.at_ms.saturating_sub(row.timestamp_ms) > 5000 {
            return None;
        }
        row.values
            .get(binding.index)
            .copied()
            .flatten()
            .map(|v| v * binding.scale + binding.offset)
            .filter(|v| v.is_finite())
    }
    pub fn phase(&self) -> Option<String> {
        let sources = self.data.as_ref()?.series.get("FLIGHT_STATE")?;
        sources
            .values()
            .filter_map(|rows| {
                let n = rows.partition_point(|r| r.timestamp_ms <= self.at_ms);
                n.checked_sub(1).map(|i| &rows[i])
            })
            .filter(|row| row.phase.is_some())
            .max_by_key(|row| (row.timestamp_ms, row.id))?
            .phase
            .clone()
    }
    pub fn gps(&self) -> Option<(f64, f64, f64)> {
        let row = self
            .data
            .as_ref()?
            .latest("GPS_DATA", Some("FC"), self.at_ms)?;
        if self.at_ms.saturating_sub(row.timestamp_ms) > 5000 {
            return None;
        }
        let lat = row.values.first().copied().flatten()? as f64;
        let lon = row.values.get(1).copied().flatten()? as f64;
        let alt = row.values.get(2).copied().flatten()? as f64;
        (lat.is_finite()
            && lon.is_finite()
            && alt.is_finite()
            && lat.abs() <= 90.
            && lon.abs() <= 180.)
            .then_some((lat, lon, alt))
    }
}
#[derive(Clone, PartialEq)]
pub struct Buffer {
    pub origin: String,
    pub recording: String,
    pub upper_id: i64,
    pub start_ms: i64,
    pub end_ms: i64,
    pub data: Rc<Index>,
}
impl Buffer {
    pub fn contains(&self, origin: &str, id: &str, upper: i64, at: i64) -> bool {
        self.origin == origin
            && self.recording == id
            && self.upper_id == upper
            && (self.start_ms..=self.end_ms).contains(&at)
    }
}
pub fn advance(position: f64, elapsed: f64, speed: f64, duration: f64) -> f64 {
    if !position.is_finite() || !elapsed.is_finite() || !speed.is_finite() || !duration.is_finite()
    {
        return 0.;
    }
    (position + elapsed.clamp(0., 1000.) * speed.clamp(0.25, 4.)).clamp(0., duration.max(0.))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample(id: i64, time: i64, value: f32) -> Sample {
        Sample {
            id,
            timestamp_ms: time,
            source_timestamp_ms: None,
            sender_id: "FC".into(),
            data_type: "ROLL".into(),
            values: vec![Some(value)],
            phase: None,
        }
    }
    fn binding() -> VehicleTelemetryBinding {
        VehicleTelemetryBinding {
            data_type: "ROLL".into(),
            sender_id: Some("FC".into()),
            index: 0,
            scale: 2.,
            offset: 1.,
        }
    }
    #[test]
    fn seeking_uses_only_previous_saved_samples_never_future_or_live_values() {
        let frame = Frame {
            data: Some(Rc::new(Index::new(vec![
                sample(1, 1000, 10.),
                sample(2, 2000, 90.),
            ]))),
            at_ms: 1500,
        };
        assert_eq!(frame.value(&binding(), false), Some(21.));
        let before = Frame {
            at_ms: 999,
            ..frame.clone()
        };
        assert_eq!(before.value(&binding(), false), None);
        let missing = Frame::default();
        assert_eq!(missing.value(&binding(), false), None);
    }
    #[test]
    fn stale_attitude_is_missing_but_explicit_saved_latches_can_hold() {
        let frame = Frame {
            data: Some(Rc::new(Index::new(vec![sample(1, 1000, 10.)]))),
            at_ms: 7000,
        };
        assert_eq!(frame.value(&binding(), false), None);
        assert_eq!(frame.value(&binding(), true), Some(21.));
    }
    #[test]
    fn clock_reversal_and_equal_timestamp_ties_have_deterministic_saved_order() {
        let index = Index::new(vec![
            sample(1, 2000, 1.),
            sample(2, 1000, 2.),
            sample(3, 2000, 3.),
        ]);
        assert_eq!(index.latest("ROLL", None, 1500).unwrap().id, 2);
        assert_eq!(index.latest("ROLL", None, 2000).unwrap().id, 3);
    }
    #[test]
    fn recording_switch_and_snapshot_identity_cannot_reuse_old_buffers() {
        let b = Buffer {
            origin: "https://gs".into(),
            recording: "one".into(),
            upper_id: 5,
            start_ms: 1000,
            end_ms: 2000,
            data: Rc::default(),
        };
        assert!(b.contains("https://gs", "one", 5, 1500));
        assert!(!b.contains("https://rrls", "one", 5, 1500));
        assert!(!b.contains("https://gs", "two", 5, 1500));
        assert!(!b.contains("https://gs", "one", 6, 1500));
        assert!(!b.contains("https://gs", "one", 5, 999));
    }
    #[test]
    fn playback_preserves_fractional_slow_motion_and_stops_at_bounds() {
        let mut time = 0.;
        for _ in 0..20 {
            time = advance(time, 50., 0.25, 5000.);
        }
        assert_eq!(time, 250.);
        assert_eq!(advance(4900., 50., 4., 5000.), 5000.);
        assert_eq!(advance(100., -50., 1., 5000.), 100.);
        assert_eq!(advance(0., 1e12, 1., 5000.), 1000.);
    }
}
