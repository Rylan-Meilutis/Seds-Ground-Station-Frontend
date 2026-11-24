//! Recorded camera video and crew voice from a replay bundle
//! (`media/index.json`, `docs/replay-bundle.md` "Recorded media").
//!
//! A bundle made with `?video=1` / `?voice=1` carries whole camera segments
//! and voice bursts that overlap the recording, each with its stream and its
//! start and end on the recording's own clock (Unix ms, the same axis as the
//! replay's rows). This module reads the index, keeps each clip's bytes
//! (moved out of the archive, not copied), and answers which clip plays at a
//! playback position and how a `<video>`/`<audio>` element follows the local
//! station's clock ([`sync`]). Nothing here touches the network.
use super::station::Playback;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

pub(crate) const INDEX: &str = "media/index.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Video,
    Voice,
}

#[derive(Clone, Debug)]
pub(crate) struct Clip {
    pub kind: Kind,
    pub stream: String,
    pub file: String,
    /// Entry path in the bundle; also the clip's key.
    pub path: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub media_type: String,
    pub body: Arc<Vec<u8>>,
}

impl Clip {
    /// What a person reads for the clip's source.
    pub(crate) fn label(&self) -> String {
        match self.kind {
            Kind::Video => self.stream.clone(),
            Kind::Voice => {
                let speaker = self.file.trim_end_matches(".wav").split_once('-').map_or("", |(_, s)| s);
                match self.stream.as_str() {
                    "radio" => "Radio".to_string(),
                    stream if speaker.is_empty() => stream.to_string(),
                    stream => format!("{} · speaker {speaker}", if stream == "crew" { "Crew" } else { stream }),
                }
            }
        }
    }
}

/// One clip as `media/index.json` lists it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Listed {
    pub kind: Kind,
    pub stream: String,
    pub file: String,
    pub path: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub bytes: u64,
    pub media_type: String,
}

/// The clips an index lists; entries that are not complete clips are left out.
pub(crate) fn parse_index(index: &Value) -> Vec<Listed> {
    let mut out: Vec<Listed> = index["clips"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let kind = match c["kind"].as_str()? {
                "video" => Kind::Video,
                "voice" => Kind::Voice,
                _ => return None,
            };
            let (start_ms, end_ms) = (c["start_ms"].as_i64()?, c["end_ms"].as_i64()?);
            (end_ms >= start_ms).then_some(())?;
            Some(Listed {
                kind,
                stream: c["stream"].as_str()?.to_string(),
                file: c["file"].as_str()?.to_string(),
                path: c["path"].as_str()?.to_string(),
                start_ms,
                end_ms,
                bytes: c["bytes"].as_u64().unwrap_or(0),
                media_type: c["media_type"].as_str().unwrap_or(if kind == Kind::Video { "video/mp4" } else { "audio/wav" }).to_string(),
            })
        })
        .collect();
    out.sort_by(|a, b| (a.start_ms, a.kind, &a.stream, &a.path).cmp(&(b.start_ms, b.kind, &b.stream, &b.path)));
    out
}

/// A bundle's recorded media.
#[derive(Debug, Default)]
pub(crate) struct Media {
    pub clips: Vec<Clip>,
    /// The index's `video` / `voice` summaries (included, skipped, budget).
    pub video: Value,
    pub voice: Value,
}

/// Moves the clips `media/index.json` lists out of `files`; `None` when the
/// bundle has no index (made without `?video=1` / `?voice=1`).
pub(crate) fn take(files: &mut BTreeMap<String, Vec<u8>>) -> Option<Media> {
    let index: Value = serde_json::from_slice(files.get(INDEX)?).ok()?;
    let clips = parse_index(&index)
        .into_iter()
        .filter_map(|l| {
            let body = files.remove(&l.path)?;
            Some(Clip { kind: l.kind, stream: l.stream, file: l.file, path: l.path, start_ms: l.start_ms, end_ms: l.end_ms, media_type: l.media_type, body: Arc::new(body) })
        })
        .collect();
    Some(Media { clips, video: index["video"].clone(), voice: index["voice"].clone() })
}

impl Media {
    /// Streams of `kind`, in order.
    pub(crate) fn streams(&self, kind: Kind) -> Vec<String> {
        let mut out: Vec<String> = self.clips.iter().filter(|c| c.kind == kind).map(|c| c.stream.clone()).collect();
        out.sort();
        out.dedup();
        out
    }

    /// The clip of `stream` that covers `position`: the latest one that has
    /// started and not ended.
    pub(crate) fn at(&self, kind: Kind, stream: &str, position: i64) -> Option<&Clip> {
        self.clips.iter().filter(|c| c.kind == kind && c.stream == stream && c.start_ms <= position && position < c.end_ms).max_by_key(|c| c.start_ms)
    }

    /// Every clip of `kind` playing at `position` (voice bursts overlap).
    pub(crate) fn playing(&self, kind: Kind, position: i64) -> Vec<&Clip> {
        self.clips.iter().filter(|c| c.kind == kind && c.start_ms <= position && position < c.end_ms).collect()
    }

    /// The next clip of `kind` (and `stream`, if given) starting after `position`.
    pub(crate) fn next(&self, kind: Kind, stream: Option<&str>, position: i64) -> Option<&Clip> {
        self.clips.iter().filter(|c| c.kind == kind && stream.is_none_or(|s| c.stream == s) && c.start_ms > position).min_by_key(|c| c.start_ms)
    }

    pub(crate) fn bytes(&self) -> usize {
        self.clips.iter().map(|c| c.body.len()).sum()
    }
}

/// Fastest rate a media element is asked to play at; browsers refuse more
/// than 16x. Faster replays scrub instead (paused, seeked each tick).
pub(crate) const MAX_RATE: f64 = 16.0;

/// How one media element follows the playback clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sync {
    /// Where the element should be, in seconds into the clip.
    pub offset_s: f64,
    pub rate: f64,
    pub play: bool,
    /// Drift (s) tolerated before the element is seeked.
    pub tolerance_s: f64,
    pub muted: bool,
}

/// The element state for `clip` at `playback`.
pub(crate) fn sync(clip: &Clip, playback: &Playback) -> Sync {
    let offset_s = ((playback.position - clip.start_ms).max(0) as f64 / 1000.0).min((clip.end_ms - clip.start_ms).max(0) as f64 / 1000.0);
    let speed = if playback.speed.is_finite() && playback.speed > 0.0 { playback.speed } else { 1.0 };
    let scrub = speed > MAX_RATE;
    Sync {
        offset_s,
        rate: if scrub { 1.0 } else { speed },
        play: !playback.paused && !scrub,
        // The clock is sampled every 250 ms: allow a tick of drift at the
        // current speed (and seek on every tick while scrubbing or paused).
        tolerance_s: if scrub || playback.paused { 0.05 } else { (0.25 * speed).max(0.4) },
        // Sped-up sound is noise; browsers mute above 4x anyway.
        muted: speed > 1.0 && clip.kind == Kind::Video || speed > 4.0,
    }
}

/// Whether an element at `current_s` must be moved to `sync.offset_s` (the
/// rule the sync script applies in the page).
#[cfg(test)]
pub(crate) fn needs_seek(current_s: f64, sync: &Sync) -> bool {
    (current_s - sync.offset_s).abs() > sync.tolerance_s
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLE: &[u8] = include_bytes!("../../../tests/fixtures/backend-contract/replay-bundle/media.gsreplay");
    const INDEX_FIXTURE: &str = include_str!("../../../tests/fixtures/backend-contract/replay-bundle/media.index.json");
    const T0: i64 = 1_767_225_600_000;

    fn playback(position: i64, speed: f64, paused: bool) -> Playback {
        Playback { position, start: T0, end: T0 + 236_000, speed, paused, state: String::new() }
    }

    #[test]
    fn the_station_index_parses_in_time_order() {
        let listed = parse_index(&serde_json::from_str(INDEX_FIXTURE).unwrap());
        assert!(listed.iter().any(|c| c.kind == Kind::Video && c.stream == "pad" && c.media_type == "video/mp4"));
        let voice: Vec<_> = listed.iter().filter(|c| c.kind == Kind::Voice).collect();
        assert_eq!(voice.len(), 1);
        assert_eq!((voice[0].start_ms, voice[0].end_ms), (T0 + 30_000, T0 + 30_200));
        assert_eq!(voice[0].path, "media/voice/crew/1767225630000-7.wav");
        assert!(listed.windows(2).all(|w| w[0].start_ms <= w[1].start_ms));
        // Unknown kinds and clips without times are skipped, not guessed.
        let odd = serde_json::json!({"clips": [{"kind": "thermal", "stream": "a", "file": "f", "path": "p", "start_ms": 0, "end_ms": 1},
            {"kind": "video", "stream": "a", "file": "f", "path": "p", "start_ms": 5}, {"kind": "video", "stream": "a", "file": "f", "path": "p", "start_ms": 5, "end_ms": 1}]});
        assert!(parse_index(&odd).is_empty());
        assert!(parse_index(&Value::Null).is_empty());
    }

    #[test]
    fn a_bundle_with_media_opens_with_its_clips() {
        let session = super::super::bundle::open("media.gsreplay", BUNDLE).expect("the media bundle opens");
        let media = session.media.as_ref().expect("the bundle carries media");
        assert_eq!(media.streams(Kind::Video), ["pad"]);
        assert_eq!(media.streams(Kind::Voice), ["crew"]);
        let listed = parse_index(&serde_json::from_str(INDEX_FIXTURE).unwrap());
        assert_eq!(media.clips.len(), listed.len());
        for (clip, listed) in media.clips.iter().zip(&listed) {
            assert_eq!(clip.body.len() as u64, listed.bytes, "{}", clip.path);
        }
        assert_eq!(media.video["included"], 5);
        assert!(!session.routes.keys().any(|r| r.contains("media/")), "clips are not station routes");
        // The plain bundle has none.
        let plain = super::super::bundle::open("flight.gsreplay", include_bytes!("../../../tests/fixtures/backend-contract/replay-bundle/flight.gsreplay")).unwrap();
        assert!(plain.media.is_none());
    }

    #[test]
    fn the_clip_at_a_position_and_the_next_one() {
        let session = super::super::bundle::open("media.gsreplay", BUNDLE).unwrap();
        let media = session.media.as_ref().unwrap();
        let at = media.at(Kind::Video, "pad", T0 + 90_000).unwrap();
        assert_eq!(at.start_ms, T0 + 60_000);
        assert_eq!(media.at(Kind::Video, "pad", T0 + 60_000).unwrap().start_ms, T0 + 60_000, "a clip starts at its start");
        assert!(media.at(Kind::Video, "pad", T0 + 900_000).is_none());
        assert_eq!(media.playing(Kind::Voice, T0 + 30_100).len(), 1);
        assert!(media.playing(Kind::Voice, T0 + 30_200).is_empty(), "a burst ends at its end");
        assert_eq!(media.next(Kind::Voice, None, T0).unwrap().start_ms, T0 + 30_000);
        assert_eq!(media.playing(Kind::Voice, T0 + 30_100)[0].label(), "Crew · speaker 7");
    }

    #[test]
    fn elements_follow_the_playback_clock() {
        let clip = Clip { kind: Kind::Video, stream: "pad".into(), file: "f.mp4".into(), path: "p".into(), start_ms: T0, end_ms: T0 + 60_000, media_type: "video/mp4".into(), body: Arc::default() };
        let s = sync(&clip, &playback(T0 + 12_500, 1.0, false));
        assert_eq!((s.offset_s, s.rate, s.play, s.muted), (12.5, 1.0, true, false));
        assert!(!needs_seek(12.8, &s), "a tick of drift is tolerated");
        assert!(needs_seek(13.5, &s));
        // Paused: held exactly where the clock is.
        let s = sync(&clip, &playback(T0 + 12_500, 1.0, true));
        assert!(!s.play && needs_seek(12.6, &s));
        // 10x plays at 10x, muted; 50x is beyond what an element plays and scrubs.
        let s = sync(&clip, &playback(T0 + 1_000, 10.0, false));
        assert_eq!((s.rate, s.play, s.muted), (10.0, true, true));
        assert!(!needs_seek(1.0 + 2.4, &s));
        let s = sync(&clip, &playback(T0 + 1_000, 50.0, false));
        assert_eq!((s.rate, s.play), (1.0, false));
        // Clamped into the clip.
        assert_eq!(sync(&clip, &playback(T0 - 5_000, 1.0, false)).offset_s, 0.0);
        assert_eq!(sync(&clip, &playback(T0 + 70_000, 1.0, false)).offset_s, 60.0);
        // Voice keeps its sound up to 4x.
        let voice = Clip { kind: Kind::Voice, ..clip };
        assert!(!sync(&voice, &playback(T0, 2.0, false)).muted);
        assert!(sync(&voice, &playback(T0, 5.0, false)).muted);
    }
}
