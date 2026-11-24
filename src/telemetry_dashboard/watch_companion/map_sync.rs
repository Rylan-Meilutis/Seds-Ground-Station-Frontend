//! Phone side of the watch map background (docs/watch-companion.md "Map
//! background"). While the dashboard is connected to a station and an
//! eligible watch (paired, app installed) has "Map background" on, the phone
//! builds a small tile set around the rocket's last fix, the ground station
//! and the RRLS from the station's own tiles (`GET /tiles/{z}/{x}/{y}.jpg`
//! through the phone's tile cache, the same source as the phone map, so it
//! works without internet), keeps it under the size cap and sends the watch
//! only the tiles it does not hold yet.
//!
//! Nothing here talks to a public tile server: the station serves tiles
//! from its offline bundle.
use super::map_tiles::{self, Centre, TileId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Persisted manifest and what the watch holds (`persist` key).
pub const STORAGE_KEY: &str = "gs26_watch_map_v1";
/// Most keys accepted from one watch report.
pub const MAX_REPORT_KEYS: usize = 1024;
/// A sent tile is not sent again for this long unless the watch reports it
/// missing after the transfer had time to finish.
pub const RESEND_AFTER_MS: i64 = 10 * 60 * 1000;
/// After a failed build (station unreachable) wait this long.
pub const RETRY_AFTER_MS: i64 = 60 * 1000;
/// Give up a build after this many consecutive failed tile reads (the
/// station went away); it is retried later.
pub const MAX_CONSECUTIVE_FAILURES: usize = 5;
/// A moving centre (the rocket in flight) rebuilds the set at most this
/// often.
pub const MIN_REBUILD_MS: i64 = 60 * 1000;

/// The tile set as the watch receives it (`type: map`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub v: u32,
    #[serde(rename = "type")]
    pub kind: String,
    /// Content id of `tiles` (changes whenever the set does).
    pub set: String,
    /// `z/x/y`, low zoom first.
    pub tiles: Vec<String>,
    pub attribution: String,
    /// Cache cap the watch should keep (the user's size cap).
    pub cap_bytes: u64,
    /// Deepest zoom in the set.
    pub max_zoom: u32,
    pub centres: Vec<Centre>,
    pub built_ms: i64,
    /// Station the tiles came from (not sent to the watch).
    #[serde(default, skip_serializing)]
    pub base: String,
    /// Sizes of the tiles, by key (not sent to the watch).
    #[serde(default, skip_serializing)]
    pub sizes: HashMap<String, u64>,
}

/// Content id: FNV-1a over the keys, as 16 hex digits.
pub fn set_id(tiles: &[String]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for key in tiles {
        for b in key.bytes().chain(std::iter::once(b';')) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// Centres of the tile set from the snapshot's targets: rocket first (the
/// recovery target), then the ground station, then the RRLS. Demo targets
/// (placed relative to the watch) and predicted positions
/// (`rocket_predicted`, a moving model estimate near the last rocket fix,
/// which is already a centre) are never centres.
pub fn centres_from_targets(targets: &[super::protocol::Target]) -> Vec<Centre> {
    ["rocket", "ground_station", "rrls"]
        .iter()
        .filter_map(|id| targets.iter().find(|t| t.id == *id))
        .filter(|t| t.demo_offset.is_none() && !t.predicted)
        .filter(|t| super::geodesy::LatLon::new(t.lat, t.lon).valid())
        .map(|t| Centre {
            id: t.id.to_string(),
            lat: t.lat,
            lon: t.lon,
        })
        .collect()
}

/// Builds a tile set: plan the coverage, read each tile (`read` returns its
/// bytes or None when the station has none), keep what fits the cap.
/// Returns None when the station could not be read at all.
pub fn build_set(
    centres: &[Centre],
    max_zoom: u32,
    cap_bytes: u64,
    now_ms: i64,
    base: &str,
    mut read: impl FnMut(TileId) -> Result<Option<Vec<u8>>, ()>,
    mut keep: impl FnMut(TileId, &[u8]),
) -> Option<Manifest> {
    let points: Vec<_> = centres
        .iter()
        .map(|c| super::geodesy::LatLon::new(c.lat, c.lon))
        .collect();
    let plan = map_tiles::coverage(
        &points,
        map_tiles::WATCH_LEVELS,
        max_zoom,
        map_tiles::MAX_SET_TILES,
    );
    let mut tiles = Vec::new();
    let mut sizes = HashMap::new();
    let mut total = 0u64;
    let mut failures = 0usize;
    let mut any_read = false;
    for tile in plan {
        match read(tile) {
            Ok(bytes) => {
                failures = 0;
                any_read = true;
                let Some(bytes) = bytes.filter(|b| !b.is_empty()) else {
                    continue;
                };
                // Same rule as `map_tiles::fit_budget`, applied while reading
                // so nothing beyond the cap is fetched twice.
                let size = bytes.len() as u64;
                if total.saturating_add(size) > cap_bytes {
                    continue;
                }
                total += size;
                keep(tile, &bytes);
                sizes.insert(tile.key(), size);
                tiles.push(tile.key());
            }
            Err(()) => {
                failures += 1;
                if failures >= MAX_CONSECUTIVE_FAILURES {
                    return None;
                }
            }
        }
    }
    if tiles.is_empty() && !any_read {
        return None;
    }
    let max_zoom_in_set = tiles
        .iter()
        .filter_map(|k| TileId::parse(k))
        .map(|t| t.z)
        .max()
        .unwrap_or(0);
    Some(Manifest {
        v: super::protocol::PROTOCOL_VERSION,
        kind: "map".into(),
        set: set_id(&tiles),
        tiles,
        attribution: map_tiles::ATTRIBUTION.into(),
        cap_bytes,
        max_zoom: max_zoom_in_set,
        centres: centres.to_vec(),
        built_ms: now_ms,
        base: base.into(),
        sizes,
    })
}

/// What the phone knows about the watch's tiles.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncState {
    pub manifest: Option<Manifest>,
    /// Tiles the watch reported (plus ones sent since).
    pub have: HashSet<String>,
    /// Tiles sent and not yet confirmed, with the send time.
    #[serde(default)]
    pub in_flight: HashMap<String, i64>,
    #[serde(default)]
    pub max_zoom: Option<(String, u32)>,
}

impl SyncState {
    /// Should a new set be built now?
    pub fn wants_build(&self, centres: &[Centre], cap_bytes: u64, base: &str, now_ms: i64) -> bool {
        let Some(m) = &self.manifest else {
            return !centres.is_empty();
        };
        if centres.is_empty() {
            return false;
        }
        if m.cap_bytes != cap_bytes || m.base != base {
            return true;
        }
        now_ms.saturating_sub(m.built_ms) >= MIN_REBUILD_MS
            && map_tiles::needs_replan(
                &m.centres,
                centres,
                now_ms.saturating_sub(m.built_ms),
                map_tiles::REPLAN_MOVE_M,
                map_tiles::REPLAN_MAX_AGE_MS,
            )
    }

    /// Adopts a new set. A set rebuilt because the old one aged out resends
    /// everything (the station's tiles may have improved).
    pub fn adopt(&mut self, manifest: Manifest, now_ms: i64) {
        let aged = self
            .manifest
            .as_ref()
            .is_some_and(|m| now_ms.saturating_sub(m.built_ms) > map_tiles::REPLAN_MAX_AGE_MS);
        if aged {
            self.have.clear();
            self.in_flight.clear();
        }
        let keep: HashSet<&String> = manifest.tiles.iter().collect();
        self.in_flight.retain(|k, _| keep.contains(k));
        self.manifest = Some(manifest);
    }

    /// The watch's report replaces what it holds.
    pub fn on_report(&mut self, have: Vec<String>) {
        let have: HashSet<String> = have
            .into_iter()
            .take(MAX_REPORT_KEYS)
            .filter(|k| TileId::parse(k).is_some())
            .collect();
        self.in_flight.retain(|k, _| !have.contains(k));
        self.have = have;
    }

    /// Tiles to send now: in the set, not on the watch, not sent recently.
    pub fn to_send(&self, now_ms: i64) -> Vec<String> {
        let Some(m) = &self.manifest else {
            return Vec::new();
        };
        map_tiles::manifest_diff(&m.tiles, &self.have)
            .into_iter()
            .filter(|k| {
                self.in_flight
                    .get(*k)
                    .is_none_or(|sent| now_ms.saturating_sub(*sent) >= RESEND_AFTER_MS)
            })
            .cloned()
            .collect()
    }

    pub fn mark_sent(&mut self, keys: &[String], now_ms: i64) {
        for key in keys {
            self.in_flight.insert(key.clone(), now_ms);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod runtime {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    static STATE: Mutex<Option<SyncState>> = Mutex::new(None);
    static LAST_CENTRES: Mutex<Vec<Centre>> = Mutex::new(Vec::new());
    static BUILDING: AtomicBool = AtomicBool::new(false);
    static LAST_FAILURE_MS: Mutex<Option<i64>> = Mutex::new(None);
    /// The manifest JSON last handed to the bridge.
    static SENT_MANIFEST: Mutex<Option<String>> = Mutex::new(None);

    fn with_state<R>(f: impl FnOnce(&mut SyncState) -> R) -> Option<R> {
        let mut guard = STATE.lock().ok()?;
        let state = guard.get_or_insert_with(|| {
            crate::telemetry_dashboard::persist::get_string(STORAGE_KEY)
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_default()
        });
        Some(f(state))
    }

    fn save(state: &SyncState) {
        if let Ok(raw) = serde_json::to_string(state) {
            crate::telemetry_dashboard::persist::set_string(STORAGE_KEY, &raw);
        }
    }

    fn tile_dir() -> std::path::PathBuf {
        std::env::temp_dir().join("gs26-watch-tiles")
    }

    fn tile_path(key: &str) -> std::path::PathBuf {
        tile_dir().join(format!("{}.jpg", key.replace('/', "_")))
    }

    /// Records the targets of the latest live snapshot.
    pub fn note_targets(targets: &[crate::telemetry_dashboard::watch_companion::protocol::Target]) {
        let centres = centres_from_targets(targets);
        if let Ok(mut slot) = LAST_CENTRES.lock() {
            *slot = centres;
        }
    }

    /// A `tiles` report from the watch. A watch that does not hold the
    /// current manifest (reinstalled, cleared) gets it again.
    pub fn on_report(set: Option<String>, have: Vec<String>) {
        let keys: Vec<String> = have.iter().take(MAX_REPORT_KEYS).cloned().collect();
        let current = with_state(|s| {
            s.on_report(have);
            save(s);
            s.manifest.as_ref().map(|m| m.set.clone())
        })
        .flatten();
        if current.is_some()
            && current != set
            && let Ok(mut sent) = SENT_MANIFEST.lock()
        {
            *sent = None;
        }
        if let Ok(json) = serde_json::to_string(&keys) {
            super::super::bridge::tiles_delivered(&json);
        }
    }

    fn line(text: &str) {
        let line = format!("[watch] map: {text}");
        eprintln!("{line}");
        crate::debug_log::append(&line);
    }

    /// One round, from the dashboard publisher. Never blocks: tile reads
    /// run on a blocking thread and the result is used on a later round.
    pub async fn tick(
        eligible: bool,
        link: super::super::bridge::LinkState,
        settings: &super::super::settings::WatchSettings,
    ) {
        if !super::super::COMPANION_PLATFORM
            || !eligible
            || !settings.map.enabled
            || !link.installed
        {
            return;
        }
        let now = crate::telemetry_dashboard::current_wallclock_ms();
        let base = crate::telemetry_dashboard::persisted_base_http_for_native_io();
        let cap = settings.map.cap_bytes();
        let centres = LAST_CENTRES.lock().map(|g| g.clone()).unwrap_or_default();

        // Deepest zoom the station has real tiles for (deeper ones are
        // upscaled from it there and on the watch alike).
        let known_zoom = with_state(|s| s.max_zoom.clone()).flatten();
        let max_zoom = match known_zoom {
            Some((b, z)) if b == base => z,
            _ => {
                #[derive(Deserialize)]
                struct MapConfig {
                    max_native_zoom: u32,
                }
                match crate::telemetry_dashboard::http_get_json::<MapConfig>("/api/map_config")
                    .await
                {
                    Ok(cfg) => {
                        let z = cfg.max_native_zoom.clamp(1, map_tiles::MAX_KEY_ZOOM);
                        with_state(|s| s.max_zoom = Some((base.clone(), z)));
                        z
                    }
                    // Older station or a transient error: plan to z15 (the
                    // station upscales deeper tiles itself) and ask again.
                    Err(_) => map_tiles::WATCH_LEVELS
                        .iter()
                        .map(|(z, _)| *z)
                        .max()
                        .unwrap_or(15),
                }
            }
        };

        let recently_failed = LAST_FAILURE_MS
            .lock()
            .ok()
            .and_then(|g| *g)
            .is_some_and(|t| now - t < RETRY_AFTER_MS);
        let wants = with_state(|s| s.wants_build(&centres, cap, &base, now)).unwrap_or(false);
        if wants && !recently_failed && !BUILDING.swap(true, Ordering::AcqRel) {
            let centres = centres.clone();
            let base_for_build = base.clone();
            std::thread::Builder::new()
                .name("gs26-watch-map".into())
                .spawn(move || {
                    let _ = std::fs::create_dir_all(tile_dir());
                    let result = build_set(
                        &centres,
                        max_zoom,
                        cap,
                        now,
                        &base_for_build,
                        |t| crate::fetch_station_tile(&base_for_build, t.z, t.x, t.y),
                        |t, bytes| {
                            let _ = std::fs::write(tile_path(&t.key()), bytes);
                        },
                    );
                    match result {
                        Some(manifest) => {
                            line(&format!(
                                "built set {} with {} tiles ({} bytes, cap {cap})",
                                manifest.set,
                                manifest.tiles.len(),
                                manifest.sizes.values().sum::<u64>()
                            ));
                            with_state(|s| {
                                s.adopt(manifest, now);
                                save(s);
                            });
                            if let Ok(mut f) = LAST_FAILURE_MS.lock() {
                                *f = None;
                            }
                        }
                        None => {
                            line("station tiles unavailable; will retry");
                            if let Ok(mut f) = LAST_FAILURE_MS.lock() {
                                *f = Some(now);
                            }
                        }
                    }
                    BUILDING.store(false, Ordering::Release);
                })
                .map_err(|_| BUILDING.store(false, Ordering::Release))
                .ok();
            return;
        }
        if BUILDING.load(Ordering::Acquire) {
            return;
        }

        // Hand the current set and the missing tiles to the bridge.
        let Some((manifest, send)) = with_state(|s| {
            let send = s.to_send(now);
            (s.manifest.clone(), send)
        }) else {
            return;
        };
        let Some(manifest) = manifest else { return };
        if let Ok(json) = serde_json::to_string(&manifest) {
            let mut sent = SENT_MANIFEST.lock().ok();
            if let Some(slot) = sent.as_deref_mut()
                && slot.as_deref() != Some(json.as_str())
            {
                super::super::bridge::send_map(&json);
                *slot = Some(json);
                prune_files(&manifest);
            }
        }
        let mut delivered = Vec::new();
        for key in send {
            let path = tile_path(&key);
            if !path.is_file() {
                // Temporary files can be purged while the app is closed.
                let Some(t) = TileId::parse(&key) else {
                    continue;
                };
                match crate::fetch_station_tile(&base, t.z, t.x, t.y) {
                    Ok(Some(bytes)) if !bytes.is_empty() => {
                        let _ = std::fs::write(&path, &bytes);
                    }
                    _ => continue,
                }
            }
            if let Some(p) = path.to_str() {
                super::super::bridge::send_tile(&key, p, &manifest.set);
                delivered.push(key);
            }
        }
        if !delivered.is_empty() {
            line(&format!(
                "sent {} tiles of set {}",
                delivered.len(),
                manifest.set
            ));
            with_state(|s| {
                s.mark_sent(&delivered, now);
                save(s);
            });
        }
    }

    /// Removes temporary tile files outside the current set.
    fn prune_files(manifest: &Manifest) {
        let keep: HashSet<String> = manifest
            .tiles
            .iter()
            .map(|k| format!("{}.jpg", k.replace('/', "_")))
            .collect();
        if let Ok(entries) = std::fs::read_dir(tile_dir()) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !keep.contains(&name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    /// Settings line for the map background.
    pub fn status_text() -> String {
        if BUILDING.load(Ordering::Acquire) {
            return "Preparing map tiles from the station…".into();
        }
        with_state(|s| match &s.manifest {
            None => {
                "No map tiles yet (needs a station connection and target positions)".to_string()
            }
            Some(m) => {
                let bytes: u64 = m.sizes.values().sum();
                let on_watch = m.tiles.iter().filter(|k| s.have.contains(*k)).count();
                format!(
                    "{} tiles ({:.1} MB) around {}; {} confirmed on the watch",
                    m.tiles.len(),
                    bytes as f64 / (1024.0 * 1024.0),
                    m.centres
                        .iter()
                        .map(|c| c.id.replace('_', " "))
                        .collect::<Vec<_>>()
                        .join(", "),
                    on_watch
                )
            }
        })
        .unwrap_or_default()
    }
}

#[cfg(target_arch = "wasm32")]
mod runtime {
    pub fn note_targets(_targets: &[super::super::protocol::Target]) {}
    pub fn on_report(_set: Option<String>, _have: Vec<String>) {}
    pub async fn tick(
        _eligible: bool,
        _link: super::super::bridge::LinkState,
        _settings: &super::super::settings::WatchSettings,
    ) {
    }
    pub fn status_text() -> String {
        String::new()
    }
}

pub use runtime::{note_targets, on_report, status_text, tick};

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> Vec<Centre> {
        vec![
            Centre {
                id: "rocket".into(),
                lat: 32.9903,
                lon: -106.9750,
            },
            Centre {
                id: "ground_station".into(),
                lat: 32.9411,
                lon: -106.9192,
            },
        ]
    }

    #[test]
    fn a_set_stays_under_the_cap_and_skips_missing_tiles() {
        let mut kept = Vec::new();
        let m = build_set(
            &site(),
            17,
            40_000,
            1_000,
            "https://gs.local:3000",
            |t| {
                Ok(if t.z == 12 {
                    None
                } else {
                    Some(vec![0u8; 3_000 + (t.x as usize % 5) * 100])
                })
            },
            |t, _| kept.push(t),
        )
        .unwrap();
        let total: u64 = m.sizes.values().sum();
        assert!(total <= 40_000, "{total}");
        assert!(m.tiles.iter().all(|k| !k.starts_with("12/")));
        assert_eq!(kept.len(), m.tiles.len());
        assert!(m.tiles[0].starts_with("11/"), "low zoom first");
        assert_eq!(m.set, set_id(&m.tiles));
        assert_eq!(m.kind, "map");
        // Phone-only fields stay off the wire.
        let wire = serde_json::to_value(&m).unwrap();
        assert!(wire.get("base").is_none() && wire.get("sizes").is_none());
        assert_eq!(wire["type"], "map");
        assert_eq!(wire["attribution"], map_tiles::ATTRIBUTION);
    }

    #[test]
    fn an_unreachable_station_builds_nothing() {
        let m = build_set(&site(), 17, 1 << 20, 0, "b", |_| Err(()), |_, _| {});
        assert!(m.is_none());
    }

    #[test]
    fn only_missing_tiles_are_sent_and_reports_replace_the_watch_state() {
        let mut s = SyncState::default();
        assert!(s.wants_build(&site(), 1 << 20, "b", 0));
        assert!(!s.wants_build(&[], 1 << 20, "b", 0));
        let m = build_set(
            &site(),
            13,
            1 << 20,
            0,
            "b",
            |_| Ok(Some(vec![1; 100])),
            |_, _| {},
        )
        .unwrap();
        let all = m.tiles.clone();
        s.adopt(m, 0);
        assert!(!s.wants_build(&site(), 1 << 20, "b", 1_000));
        assert!(s.wants_build(&site(), 2 << 20, "b", 1_000), "cap changed");
        assert!(
            s.wants_build(&site(), 1 << 20, "other", 1_000),
            "station changed"
        );
        assert_eq!(s.to_send(0), all);
        s.mark_sent(&all, 0);
        assert!(s.to_send(1_000).is_empty(), "in flight");
        // The watch got all but the last tile; it is resent only after the
        // transfer had time to finish.
        s.on_report(all[..all.len() - 1].to_vec());
        assert!(s.to_send(1_000).is_empty());
        assert_eq!(
            s.to_send(RESEND_AFTER_MS),
            vec![all.last().unwrap().clone()]
        );
        // Bad keys in a report are ignored.
        s.on_report(vec!["nope".into(), all[0].clone()]);
        assert_eq!(s.have.len(), 1);
    }

    #[test]
    fn moving_the_rocket_far_rebuilds_and_ageing_resends_everything() {
        let mut s = SyncState::default();
        let m = build_set(
            &site(),
            13,
            1 << 20,
            0,
            "b",
            |_| Ok(Some(vec![1; 100])),
            |_, _| {},
        )
        .unwrap();
        s.adopt(m.clone(), 0);
        s.on_report(m.tiles.clone());
        let mut landed = site();
        landed[0].lat += 0.05;
        assert!(
            !s.wants_build(&landed, 1 << 20, "b", 10),
            "at most once a minute"
        );
        assert!(s.wants_build(&landed, 1 << 20, "b", MIN_REBUILD_MS));
        let later = map_tiles::REPLAN_MAX_AGE_MS + 1;
        assert!(s.wants_build(&site(), 1 << 20, "b", later));
        s.adopt(m.clone(), later);
        assert!(s.have.is_empty());
        assert_eq!(s.to_send(later), m.tiles);
    }

    #[test]
    fn centres_come_from_live_targets_in_priority_order() {
        use super::super::protocol::{DemoOffset, Target};
        let t = |id: &'static str, lat: f64, demo: bool| Target {
            id,
            label: id.into(),
            lat,
            lon: -106.9,
            alt_m: None,
            age_ms: 0,
            stale: false,
            source: String::new(),
            fallback: false,
            predicted: false,
            demo_offset: demo.then_some(DemoOffset {
                north_m: 1.0,
                east_m: 1.0,
            }),
        };
        let centres = centres_from_targets(&[
            t("rrls", 32.9, false),
            t("ground_station", 32.8, false),
            t("rocket", 99.0, false),
        ]);
        assert_eq!(
            centres.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["ground_station", "rrls"]
        );
        assert!(centres_from_targets(&[t("rocket", 32.9, true)]).is_empty());
        let predicted = Target {
            predicted: true,
            ..t("rocket_predicted", 32.95, false)
        };
        assert_eq!(
            centres_from_targets(&[predicted, t("rocket", 32.9, false)])
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            vec!["rocket"]
        );
    }
}
