//! Bounded ingress history for dashboard trends. Each envelope keeps actual
//! first/last/min/max samples and their source timestamps; gaps stay gaps.
use std::collections::BTreeMap;

const LIVE_MS: i64 = 60_000;
const LIVE_BUCKET_MS: i64 = 250;
const CAPTURE_BUCKET_MS: i64 = 1_000;
const MAX_SERIES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sample {
    pub time: i64,
    pub value: f32,
    /// Internal continuity identity; never an invented telemetry observation.
    pub segment: u64,
}

#[derive(Clone)]
struct Envelope {
    first: Sample,
    last: Sample,
    min: Sample,
    max: Sample,
}
impl Envelope {
    fn new(sample: Sample) -> Self {
        Self {
            first: sample,
            last: sample,
            min: sample,
            max: sample,
        }
    }
    fn add(&mut self, sample: Sample) {
        if sample.time < self.first.time {
            self.first = sample;
        }
        if sample.time >= self.last.time {
            self.last = sample;
        }
        if sample.value < self.min.value {
            self.min = sample;
        }
        if sample.value > self.max.value {
            self.max = sample;
        }
    }
    fn samples(&self) -> Vec<Sample> {
        let mut samples = vec![self.first, self.last, self.min, self.max];
        samples.sort_by_key(|s| s.time);
        samples.dedup_by(|a, b| a == b);
        samples
    }
}

#[derive(Default)]
struct Series {
    live: BTreeMap<i64, Envelope>,
    capture: BTreeMap<i64, Envelope>,
    canonical: BTreeMap<i64, Envelope>,
    live_segment:u64,
    source_segment:u64,
}

/// Capture identity is the durable authority/session and admitted Launch action
/// anchor. Receipt time and T0 never substitute for missing source evidence.
pub(crate) struct History<K> {
    series: BTreeMap<K, Series>,
    newest: Option<i64>,
    next_segment:u64,
    pub launch: Option<(i64, i64)>,
    identity: Option<String>,
    newest_source: Option<i64>,
}
impl<K> Default for History<K> {
    fn default() -> Self {
        Self {
            series: BTreeMap::new(),
            newest: None,
            next_segment:0,
            launch: None,
            identity: None,
            newest_source: None,
        }
    }
}
impl<K: Ord + Clone> History<K> {
    /// Explicit missing/invalid data breaks both timelines without a value.
    pub fn gap(&mut self,key:&K) {
        if let Some(series)=self.series.get_mut(key) {
            self.next_segment=self.next_segment.wrapping_add(1);
            series.live_segment=self.next_segment;
            self.next_segment=self.next_segment.wrapping_add(1);
            series.source_segment=self.next_segment;
        }
    }
    /// An explicit display epoch reset invalidates cached graph timelines.
    /// Ordinary reseed never calls this; fresh canonical evidence can rebuild.
    pub fn reset_display_epoch(&mut self) {
        self.series.clear();
        self.newest=None;
        self.newest_source=None;
        self.launch=None;
        self.identity=None;
    }

    pub fn observe_launch(&mut self, identity: String, anchor: Option<i64>, test_fire: bool) {
        if self.identity.as_ref() != Some(&identity) {
            self.launch = None;
            self.identity = Some(identity);
            for series in self.series.values_mut() {
                series.capture.clear();
            }
        }
        let Some(anchor) = anchor else {
            return;
        };
        if self.launch.is_some_and(|(start, _)| start == anchor) {
            return;
        }
        self.launch(anchor, test_fire);
    }
    fn launch(&mut self, anchor: i64, test_fire: bool) {
        if self.launch.is_some_and(|(start, _)| start == anchor) {
            return;
        }
        let end = anchor.saturating_add(if test_fire { 30_000 } else { 600_000 });
        self.launch = Some((anchor, end));
        for series in self.series.values_mut() {
            series.capture.clear();
            for sample in series.canonical.values().flat_map(Envelope::samples) {
                if (anchor..=end).contains(&sample.time) {
                    insert(&mut series.capture, CAPTURE_BUCKET_MS, sample);
                }
            }
        }
    }
    #[cfg(test)]
    fn record(&mut self, key: K, sample: Sample) {
        self.record_source(key, sample, Some(sample.time));
    }
    pub fn record_source(&mut self, key: K, sample: Sample, source: Option<i64>) {
        if !sample.value.is_finite() {
            self.gap(&key);
            return;
        }
        if let Some(time) = source {
            self.newest_source = Some(self.newest_source.map_or(time, |old| old.max(time)));
        }
        self.record_live(key.clone(), sample);
        let Some(time) = source else {
            // A sample without source-time evidence must not join the next
            // known source sample to an earlier one across that missing span.
            if let Some(series)=self.series.get_mut(&key) {
                self.next_segment=self.next_segment.wrapping_add(1);
                series.source_segment=self.next_segment;
            }
            return;
        };
        let Some(series) = self.series.get_mut(&key) else {
            return;
        };
        let source = Sample {
            time,
            value: sample.value,
            segment:series.source_segment,
        };
        let floor = self
            .newest_source
            .unwrap_or(time)
            .saturating_sub(600_000)
            .div_euclid(CAPTURE_BUCKET_MS);
        if time.div_euclid(CAPTURE_BUCKET_MS) >= floor {
            insert(&mut series.canonical, CAPTURE_BUCKET_MS, source);
        }
        while series
            .canonical
            .first_key_value()
            .is_some_and(|(bucket, _)| *bucket < floor)
            || series.canonical.len() > 601
        {
            series.canonical.pop_first();
        }
        if let Some((start, end)) = self.launch
            && (start..=end).contains(&time)
        {
            insert(&mut series.capture, CAPTURE_BUCKET_MS, source);
        }
    }
    fn record_live(&mut self, key: K, mut sample: Sample) {
        if !sample.value.is_finite() {
            self.gap(&key);
            return;
        }
        if !self.series.contains_key(&key) {
            if self.series.len() >= MAX_SERIES {return;}
            self.next_segment=self.next_segment.wrapping_add(1);
            let live_segment=self.next_segment;
            self.next_segment=self.next_segment.wrapping_add(1);
            self.series.insert(key.clone(),Series {live_segment,source_segment:self.next_segment,..Default::default()});
        }
        let newest = self
            .newest
            .map_or(sample.time, |time| time.max(sample.time));
        let prune = self
            .newest
            .is_none_or(|old| old.div_euclid(LIVE_BUCKET_MS) != newest.div_euclid(LIVE_BUCKET_MS));
        self.newest = Some(newest);
        let series = self.series.get_mut(&key).expect("series was inserted above");
        sample.segment=series.live_segment;
        if sample.time >= newest.saturating_sub(LIVE_MS) {
            insert(&mut series.live, LIVE_BUCKET_MS, sample);
        }
        // Global expiry also bounds inactive sources after a large time jump.
        let floor = newest.saturating_sub(LIVE_MS).div_euclid(LIVE_BUCKET_MS);
        if prune {
            for series in self.series.values_mut() {
                while series
                    .live
                    .first_key_value()
                    .is_some_and(|(bucket, _)| *bucket < floor)
                {
                    series.live.pop_first();
                }
            }
        }
    }
    #[cfg(test)]
    fn test_points(&self,key:&K,aggregate:bool)->Vec<Sample> {
        self.samples(key,aggregate).into_iter().map(|mut point|{point.segment=0;point}).collect()
    }

    pub fn matching_samples(&self, matches: impl Fn(&K) -> bool, aggregate: bool) -> Vec<Sample> {
        let mut samples: Vec<_> = self
            .series
            .keys()
            .filter(|key| matches(key))
            .flat_map(|key| self.samples(key, aggregate))
            .collect();
        samples.sort_by_key(|sample| sample.time);
        samples
    }
    pub fn samples(&self, key: &K, aggregate: bool) -> Vec<Sample> {
        self.series
            .get(key)
            .map(|series| {
                let buckets = if aggregate {
                    &series.capture
                } else {
                    &series.live
                };
                buckets.values().flat_map(Envelope::samples).collect()
            })
            .unwrap_or_default()
    }
}
fn insert(buckets: &mut BTreeMap<i64, Envelope>, width: i64, sample: Sample) {
    buckets
        .entry(sample.time.div_euclid(width))
        .and_modify(|bucket| bucket.add(sample))
        .or_insert_with(|| Envelope::new(sample));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(time: i64, value: f32) -> Sample {
        Sample { time, value,segment:0 }
    }
    #[test]
    fn retains_subframe_spikes_and_actual_times_in_order() {
        let mut history = History::default();
        for (t, v) in [(100, 1.), (101, 500.), (103, -7.), (249, 2.)] {
            history.record("DAQ", sample(t, v));
        }
        assert_eq!(
            history.test_points(&"DAQ", false),
            vec![
                sample(100, 1.),
                sample(101, 500.),
                sample(103, -7.),
                sample(249, 2.)
            ]
        );
        history.record("DAQ", sample(1_000_000, f32::NAN));
        assert_eq!(history.test_points(&"DAQ", false).len(), 4);
    }
    #[test]
    fn testfire_capture_is_retained_and_new_launch_resets_it() {
        let mut history = History::default();
        history.launch(10_000, true);
        for t in [9_999, 10_000, 40_000, 40_001, 700_000] {
            history.record("DAQ", sample(t, t as f32));
        }
        assert_eq!(
            history.test_points(&"DAQ", true),
            vec![sample(10_000, 10_000.), sample(40_000, 40_000.)]
        );
        history.launch(10_000, false); // Repeated snapshot cannot extend the window.
        assert_eq!(history.launch, Some((10_000, 40_000)));
        history.launch(700_000, false);
        assert_eq!(history.launch, Some((700_000, 1_300_000)));
        assert_eq!(
            history.test_points(&"DAQ", true),
            vec![sample(700_000, 700_000.)]
        );
    }
    #[test]
    fn flight_capture_stops_at_ten_minutes_and_survives_live_expiry() {
        let mut history = History::default();
        history.launch(0, false);
        for t in [0, 600_000, 600_001, 900_000] {
            history.record("FC", sample(t, t as f32));
        }
        assert_eq!(
            history.test_points(&"FC", true),
            vec![sample(0, 0.), sample(600_000, 600_000.)]
        );
        assert_eq!(
            history.test_points(&"FC", false),
            vec![sample(900_000, 900_000.)]
        );
    }
    #[test]
    fn bounded_even_at_high_rate_and_sparse_gaps_are_not_filled() {
        let mut history = History::default();
        history.launch(0, false);
        for t in 0..700_000 {
            history.record(0, sample(t, (t % 123) as f32));
        }
        assert!(history.test_points(&0, false).len() <= 4 * 241);
        assert!(history.test_points(&0, true).len() <= 4 * 601);
        for key in 1..1000 {
            history.record(key, sample(700_000, 1.));
        }
        assert_eq!(history.series.len(), MAX_SERIES);
        let mut sparse = History::default();
        sparse.record(0, sample(1, 1.));
        sparse.record(0, sample(30_000, 3.));
        assert_eq!(
            sparse.test_points(&0, false),
            vec![sample(1, 1.), sample(30_000, 3.)]
        );
    }
    #[test]
    fn delayed_launch_backfills_and_sender_and_channel_keys_stay_separate() {
        let mut history = History::default();
        history.record(("DAQ", 0), sample(10_000, 12.));
        history.record(("FC", 0), sample(10_000, 100.));
        history.record(("FC", 1), sample(10_000, -2.));
        history.launch(9_000, false);
        assert_eq!(
            history.test_points(&("DAQ", 0), true),
            vec![sample(10_000, 12.)]
        );
        assert_eq!(history.test_points(&("FC", 1), true), vec![sample(10_000, -2.)]);
    }
    #[test]
    fn canonical_capture_is_independent_of_receipt_pause_speed_and_missing_clock() {
        let mut history = History::default();
        history.observe_launch("session-a".into(), Some(100_000), true);
        // Model advances 100ms while receipt moves 20s; then pauses another20s.
        history.record_source("DAQ", sample(1_000_000, 1.), Some(100_000));
        history.record_source("DAQ", sample(1_020_000, 9.), Some(100_100));
        history.record_source("DAQ", sample(1_040_000, 2.), Some(100_100));
        history.record_source("DAQ", sample(1_041_000, 100.), None);
        history.record_source("DAQ", sample(1_060_000, 3.), Some(130_001));
        let captured = history.test_points(&"DAQ", true);
        assert!(
            captured
                .iter()
                .all(|s| s.time <= 130_000 && s.value != 100.)
        );
        assert!(captured.iter().any(|s| s.time == 100_100 && s.value == 9.));
        history.observe_launch("session-a".into(), None, true); // reconnect clock missing
        assert_eq!(history.test_points(&"DAQ", true), captured);
        history.observe_launch("session-b".into(), None, true); // reset session
        assert_eq!(history.launch, None);
        assert!(history.test_points(&"DAQ", true).is_empty());
        history.observe_launch("session-b".into(), Some(130_000), false);
        assert_eq!(history.launch, Some((130_000, 730_000)));
    }
    #[test]
    fn explicit_missing_and_invalid_samples_break_continuity_without_points() {
        let mut history=History::default();
        history.record("DAQ",sample(1000,1.));
        history.gap(&"DAQ"); // An explicit None channel has no plotted value.
        history.record("DAQ",sample(1002,10.));
        history.record("DAQ",sample(1003,f32::NAN));
        history.record("DAQ",sample(1004,-3.));
        let points=history.samples(&"DAQ",false);
        assert_eq!(points.iter().map(|p|(p.time,p.value)).collect::<Vec<_>>(),vec![(1000,1.),(1002,10.),(1004,-3.)]);
        assert_ne!(points[0].segment,points[1].segment);
        assert_ne!(points[1].segment,points[2].segment);
    }
    #[test]
    fn wildcard_sender_series_cannot_join_each_other() {
        let mut history=History::default();
        history.record("DAQ",sample(1000,1.));
        history.record("FC",sample(1001,2.));
        let points=history.matching_samples(|_|true,false);
        assert_eq!(points.len(),2);
        assert_ne!(points[0].segment,points[1].segment);
    }
    #[test]
    fn explicit_display_epoch_reset_allows_backstep_and_discards_old_graph_epoch() {
        let mut history=History::default();
        history.observe_launch("old-session".into(),Some(900_000),true);
        history.record("DAQ",sample(900_000,500.));
        history.reset_display_epoch();
        assert!(history.samples(&"DAQ",false).is_empty());
        assert_eq!(history.launch,None);assert!(history.samples(&"DAQ",true).is_empty());
        history.record("DAQ",sample(1000,5.));
        assert_eq!(history.samples(&"DAQ",false).iter().map(|p|(p.time,p.value)).collect::<Vec<_>>(),vec![(1000,5.)]);
    }

    #[test]
    fn missing_source_time_breaks_aggregate_without_fake_canonical_timestamp() {
        let mut history=History::default();
        history.observe_launch("session".into(),Some(1000),true);
        history.record_source("DAQ",sample(50_000,1.),Some(1000));
        history.record_source("DAQ",sample(50_001,99.),None);
        history.record_source("DAQ",sample(50_002,10.),Some(1002));
        let points=history.samples(&"DAQ",true);
        assert_eq!(points.iter().map(|p|(p.time,p.value)).collect::<Vec<_>>(),vec![(1000,1.),(1002,10.)]);
        assert_ne!(points[0].segment,points[1].segment);
    }

}
