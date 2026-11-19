//! Map background for the watch compass (docs/watch-companion.md "Map
//! background"): Web Mercator tile math, the tile set the phone builds for
//! the watch, the byte budget, the manifest diff and the watch-side eviction
//! order. Pure functions; the Swift (`WatchCore/MapTiles.swift`) and Kotlin
//! (`core/.../MapTiles.kt`) ports are checked against the same
//! `map_tiles` vectors in `watch/test-vectors.json`.
use super::geodesy::{LatLon, distance_m};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// Tile edge in pixels (the GS serves 256 px JPEG tiles).
pub const TILE_PX: f64 = 256.0;
/// Web Mercator latitude limit.
pub const MAX_LAT: f64 = 85.051_128_78;
/// Equatorial circumference of the Web Mercator sphere in metres.
pub const MERCATOR_CIRCUMFERENCE_M: f64 = 2.0 * std::f64::consts::PI * 6_378_137.0;
/// Deepest zoom a key may name.
pub const MAX_KEY_ZOOM: u32 = 22;

/// Zoom levels and radii of the watch tile set: a watch face shows roughly
/// 0.5–10 km across these, and the watch scales the nearest level between
/// them.
pub const WATCH_LEVELS: &[(u32, f64)] = &[
    (11, 10_000.0),
    (12, 5_000.0),
    (13, 2_500.0),
    (14, 1_200.0),
    (15, 600.0),
];
/// Upper bound on tiles in one set, whatever the byte cap.
pub const MAX_SET_TILES: usize = 200;
/// Rebuild the set when a centre moved further than this.
pub const REPLAN_MOVE_M: f64 = 500.0;
/// Rebuild (and resend) a set older than this.
pub const REPLAN_MAX_AGE_MS: i64 = 7 * 24 * 3600 * 1000;
/// Tile imagery credit shown on the watch. The GS serves tiles from its
/// offline bundle, downloaded from Esri World Imagery (`map_downloader`).
pub const ATTRIBUTION: &str = "Imagery © Esri, Maxar, Earthstar Geographics";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    pub z: u32,
    pub x: u32,
    pub y: u32,
}

impl TileId {
    pub fn new(z: u32, x: u32, y: u32) -> Option<Self> {
        let n = 1u64 << z.min(MAX_KEY_ZOOM);
        (z <= MAX_KEY_ZOOM && (x as u64) < n && (y as u64) < n).then_some(Self { z, x, y })
    }

    /// `z/x/y`, the wire form.
    pub fn key(self) -> String {
        format!("{}/{}/{}", self.z, self.x, self.y)
    }

    pub fn parse(key: &str) -> Option<Self> {
        let mut parts = key.split('/');
        let z = parts.next()?.parse().ok()?;
        let x = parts.next()?.parse().ok()?;
        let y = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Self::new(z, x, y)
    }

    /// North-west corner (lat, lon) of tile `(z, x, y)`; `x`/`y` may be one
    /// past the edge for the south-east corner.
    pub fn corner(z: u32, x: u32, y: u32) -> (f64, f64) {
        let n = (1u64 << z) as f64;
        let lon = x as f64 / n * 360.0 - 180.0;
        let lat = (std::f64::consts::PI * (1.0 - 2.0 * y as f64 / n))
            .sinh()
            .atan()
            .to_degrees();
        (lat, lon)
    }

    /// (north, west, south, east) in degrees.
    pub fn bounds(self) -> (f64, f64, f64, f64) {
        let (north, west) = Self::corner(self.z, self.x, self.y);
        let (south, east) = Self::corner(self.z, self.x + 1, self.y + 1);
        (north, west, south, east)
    }
}

/// Fractional tile coordinates of a point at zoom `z`.
pub fn tile_xy(lat: f64, lon: f64, z: u32) -> (f64, f64) {
    let n = (1u64 << z) as f64;
    let lat = lat.clamp(-MAX_LAT, MAX_LAT);
    let fx = (lon + 180.0) / 360.0 * n;
    let r = lat.to_radians();
    let fy = (1.0 - (r.tan() + 1.0 / r.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    (fx, fy)
}

fn clamp_index(v: f64, n: u32) -> u32 {
    (v.floor().max(0.0) as u64).min(n as u64 - 1) as u32
}

/// The tile containing a point.
pub fn tile_at(lat: f64, lon: f64, z: u32) -> TileId {
    let (fx, fy) = tile_xy(lat, lon, z);
    let n = 1u32 << z;
    TileId {
        z,
        x: clamp_index(fx, n),
        y: clamp_index(fy, n),
    }
}

/// Ground width of one tile at `lat` and zoom `z`, metres.
pub fn tile_span_m(lat: f64, z: u32) -> f64 {
    MERCATOR_CIRCUMFERENCE_M * lat.to_radians().cos() / (1u64 << z) as f64
}

/// The watch tile set: per level (low zoom first; `z` clamped to
/// `max_zoom`, duplicates keep the larger radius), per centre in priority
/// order, the tiles within the radius nearest first; duplicates skipped;
/// at most `max_tiles`.
pub fn coverage(
    centres: &[LatLon],
    levels: &[(u32, f64)],
    max_zoom: u32,
    max_tiles: usize,
) -> Vec<TileId> {
    let mut merged: BTreeMap<u32, f64> = BTreeMap::new();
    for &(z, radius) in levels {
        let z = z.min(max_zoom).min(MAX_KEY_ZOOM);
        let entry = merged.entry(z).or_insert(0.0);
        *entry = entry.max(radius);
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (&z, &radius) in &merged {
        let n = 1u32 << z;
        for c in centres.iter().filter(|c| c.valid()) {
            let (fx, fy) = tile_xy(c.lat, c.lon, z);
            let r = radius / tile_span_m(c.lat, z);
            let (x0, x1) = (clamp_index(fx - r, n), clamp_index(fx + r, n));
            let (y0, y1) = (clamp_index(fy - r, n), clamp_index(fy + r, n));
            let mut cells = Vec::new();
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let dx = x as f64 + 0.5 - fx;
                    let dy = y as f64 + 0.5 - fy;
                    let d = ((dx * dx + dy * dy) * 1e6).round() as i64;
                    cells.push((d, y, x));
                }
            }
            cells.sort_unstable();
            for (_, y, x) in cells {
                let id = TileId { z, x, y };
                if seen.insert(id) {
                    out.push(id);
                }
            }
        }
    }
    out.truncate(max_tiles);
    out
}

/// Greedy byte budget in plan order: indices of the tiles to keep. A size
/// of 0 (tile missing at the source) is never kept.
pub fn fit_budget(sizes: &[u64], cap: u64) -> Vec<usize> {
    let mut total = 0u64;
    let mut chosen = Vec::new();
    for (i, &size) in sizes.iter().enumerate() {
        if size > 0 && total.saturating_add(size) <= cap {
            total += size;
            chosen.push(i);
        }
    }
    chosen
}

/// Plan tiles the watch does not have yet, in plan order.
pub fn manifest_diff<'a>(plan: &'a [String], have: &HashSet<String>) -> Vec<&'a String> {
    plan.iter().filter(|k| !have.contains(*k)).collect()
}

/// One tile in the watch cache.
#[derive(Clone, Debug, PartialEq)]
pub struct CacheEntry {
    pub key: String,
    pub bytes: u64,
    pub used_ms: i64,
}

/// Keys to delete so the cache fits `cap`: tiles outside the current plan
/// first, then least recently used, then by key.
pub fn eviction_order(entries: &[CacheEntry], plan: &HashSet<String>, cap: u64) -> Vec<String> {
    let mut total: u64 = entries.iter().map(|e| e.bytes).sum();
    let mut order: Vec<&CacheEntry> = entries.iter().collect();
    order.sort_by(|a, b| {
        (plan.contains(&a.key), a.used_ms, &a.key).cmp(&(plan.contains(&b.key), b.used_ms, &b.key))
    });
    let mut out = Vec::new();
    for e in order {
        if total <= cap {
            break;
        }
        total = total.saturating_sub(e.bytes);
        out.push(e.key.clone());
    }
    out
}

/// Zoom that puts a target `distance_m` away at `radius_px` from the view
/// centre, clamped to `[min_z, max_z]`.
pub fn auto_fit_zoom(distance_m: f64, lat: f64, radius_px: f64, min_z: f64, max_z: f64) -> f64 {
    if !(distance_m.is_finite() && distance_m > 0.0) {
        return max_z;
    }
    let z = (MERCATOR_CIRCUMFERENCE_M * lat.to_radians().cos() * radius_px
        / (TILE_PX * distance_m))
        .log2();
    z.clamp(min_z, max_z)
}

/// Where a missing tile can be cut from: the nearest available ancestor and
/// the sub-square of it (fractions of the ancestor's edge).
#[derive(Clone, Debug, PartialEq)]
pub struct AncestorCrop {
    pub key: String,
    pub x: f64,
    pub y: f64,
    pub size: f64,
}

pub fn ancestor(tile: TileId, available: &HashSet<String>, max_up: u32) -> Option<AncestorCrop> {
    for up in 1..=max_up.min(tile.z) {
        let parent = TileId {
            z: tile.z - up,
            x: tile.x >> up,
            y: tile.y >> up,
        };
        let key = parent.key();
        if available.contains(&key) {
            let size = 1.0 / (1u64 << up) as f64;
            return Some(AncestorCrop {
                key,
                x: (tile.x - (parent.x << up)) as f64 * size,
                y: (tile.y - (parent.y << up)) as f64 * size,
                size,
            });
        }
    }
    None
}

/// A named centre of the tile set (`rocket`, `ground_station`, `rrls`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Centre {
    pub id: String,
    pub lat: f64,
    pub lon: f64,
}

/// Rebuild the tile set? Not without centres; always without a previous
/// set; when it is older than `max_age_ms`, the centre ids changed, or a
/// centre moved more than `move_m`.
pub fn needs_replan(
    previous: &[Centre],
    current: &[Centre],
    age_ms: i64,
    move_m: f64,
    max_age_ms: i64,
) -> bool {
    if current.is_empty() {
        return false;
    }
    if previous.is_empty() || age_ms > max_age_ms {
        return true;
    }
    let prev: BTreeMap<&str, &Centre> = previous.iter().map(|c| (c.id.as_str(), c)).collect();
    let now: BTreeMap<&str, &Centre> = current.iter().map(|c| (c.id.as_str(), c)).collect();
    if prev.keys().ne(now.keys()) {
        return true;
    }
    now.iter().any(|(id, c)| {
        let p = prev[id];
        distance_m(LatLon::new(p.lat, p.lon), LatLon::new(c.lat, c.lon)).is_none_or(|d| d > move_m)
    })
}

/// One tile to draw: its key and the offset of its top-left corner from
/// the view centre in pixels, before rotation.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub tile: TileId,
    pub left: f64,
    pub top: f64,
}

/// Tiles covering a `width`×`height` view (its circumscribed circle, so a
/// rotated map has no gaps) centred on a point at fractional `zoom`.
/// Returns the integer zoom drawn, the tile edge in pixels and the tiles.
pub fn visible_tiles(
    lat: f64,
    lon: f64,
    zoom: f64,
    width: f64,
    height: f64,
) -> (u32, f64, Vec<Placement>) {
    let z = ((zoom + 0.5).floor().max(0.0) as u32).min(MAX_KEY_ZOOM);
    let px = TILE_PX * 2f64.powf(zoom - z as f64);
    let (fx, fy) = tile_xy(lat, lon, z);
    let r = (width * width + height * height).sqrt() / 2.0 / px;
    let n = 1u32 << z;
    let mut out = Vec::new();
    for y in clamp_index(fy - r, n)..=clamp_index(fy + r, n) {
        for x in clamp_index(fx - r, n)..=clamp_index(fx + r, n) {
            out.push(Placement {
                tile: TileId { z, x, y },
                left: (x as f64 - fx) * px,
                top: (y as f64 - fy) * px,
            });
        }
    }
    (z, px, out)
}

/// Pixel offset (east, south) of a point from the view centre at `zoom`.
pub fn project(lat: f64, lon: f64, center_lat: f64, center_lon: f64, zoom: f64) -> (f64, f64) {
    let (u, v) = tile_xy(lat, lon, 0);
    let (cu, cv) = tile_xy(center_lat, center_lon, 0);
    let scale = TILE_PX * 2f64.powf(zoom);
    ((u - cu) * scale, (v - cv) * scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn vectors() -> Value {
        let raw = include_str!("../../../watch/test-vectors.json");
        let v: Value = serde_json::from_str(raw).unwrap();
        v["map_tiles"].clone()
    }

    fn f(v: &Value) -> f64 {
        v.as_f64().unwrap()
    }

    fn strings(v: &Value) -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect()
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn tile_xy_vectors() {
        for c in vectors()["tile_xy"].as_array().unwrap() {
            let name = c["name"].as_str().unwrap();
            let z = c["z"].as_u64().unwrap() as u32;
            let (fx, fy) = tile_xy(f(&c["lat"]), f(&c["lon"]), z);
            assert!(close(fx, f(&c["fx"]), 1e-9), "{name}: fx {fx}");
            assert!(close(fy, f(&c["fy"]), 1e-9), "{name}: fy {fy}");
            let tile = tile_at(f(&c["lat"]), f(&c["lon"]), z);
            assert_eq!(tile.key(), c["key"].as_str().unwrap(), "{name}");
            assert_eq!(TileId::parse(&tile.key()), Some(tile));
            let (n, w, s, e) = tile.bounds();
            for (got, want) in [(n, "north"), (w, "west"), (s, "south"), (e, "east")] {
                assert!(close(got, f(&c[want]), 1e-9), "{name}: {want} {got}");
            }
            assert!(
                close(tile_span_m(f(&c["lat"]), z), f(&c["span_m"]), 1e-6),
                "{name}"
            );
        }
    }

    #[test]
    fn coverage_vectors() {
        for c in vectors()["coverage"].as_array().unwrap() {
            let centres: Vec<LatLon> = c["centres"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| LatLon::new(f(&p[0]), f(&p[1])))
                .collect();
            let levels: Vec<(u32, f64)> = c["levels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| (l[0].as_u64().unwrap() as u32, f(&l[1])))
                .collect();
            let got: Vec<String> = coverage(
                &centres,
                &levels,
                c["max_zoom"].as_u64().unwrap() as u32,
                c["max_tiles"].as_u64().unwrap() as usize,
            )
            .into_iter()
            .map(TileId::key)
            .collect();
            assert_eq!(got, strings(&c["tiles"]), "{}", c["name"]);
        }
    }

    #[test]
    fn budget_diff_and_eviction_vectors() {
        let v = vectors();
        for c in v["budget"].as_array().unwrap() {
            let sizes: Vec<u64> = c["sizes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_u64().unwrap())
                .collect();
            let chosen: Vec<usize> = c["chosen"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_u64().unwrap() as usize)
                .collect();
            assert_eq!(
                fit_budget(&sizes, c["cap"].as_u64().unwrap()),
                chosen,
                "{}",
                c["name"]
            );
        }
        for c in v["manifest_diff"].as_array().unwrap() {
            let plan = strings(&c["plan"]);
            let have: HashSet<String> = strings(&c["have"]).into_iter().collect();
            let got: Vec<String> = manifest_diff(&plan, &have).into_iter().cloned().collect();
            assert_eq!(got, strings(&c["send"]), "{}", c["name"]);
        }
        for c in v["evict"].as_array().unwrap() {
            let entries: Vec<CacheEntry> = c["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| CacheEntry {
                    key: e["key"].as_str().unwrap().into(),
                    bytes: e["bytes"].as_u64().unwrap(),
                    used_ms: e["used_ms"].as_i64().unwrap(),
                })
                .collect();
            let plan: HashSet<String> = strings(&c["plan"]).into_iter().collect();
            assert_eq!(
                eviction_order(&entries, &plan, c["cap"].as_u64().unwrap()),
                strings(&c["evict"]),
                "{}",
                c["name"]
            );
        }
    }

    #[test]
    fn view_vectors() {
        let v = vectors();
        for c in v["auto_fit"].as_array().unwrap() {
            let got = auto_fit_zoom(
                f(&c["distance_m"]),
                f(&c["lat"]),
                f(&c["radius_px"]),
                f(&c["min_z"]),
                f(&c["max_z"]),
            );
            assert!(close(got, f(&c["zoom"]), 1e-9), "{}: {got}", c["name"]);
        }
        for c in v["ancestor"].as_array().unwrap() {
            let tile = TileId::parse(c["key"].as_str().unwrap()).unwrap();
            let available: HashSet<String> = strings(&c["available"]).into_iter().collect();
            let got = ancestor(tile, &available, 6);
            match &c["found"] {
                Value::Null => assert!(got.is_none(), "{}", c["name"]),
                want => {
                    let got = got.unwrap_or_else(|| panic!("{}", c["name"]));
                    assert_eq!(got.key, want["key"].as_str().unwrap());
                    assert!(close(got.x, f(&want["x"]), 1e-12));
                    assert!(close(got.y, f(&want["y"]), 1e-12));
                    assert!(close(got.size, f(&want["size"]), 1e-12));
                }
            }
        }
        for c in v["visible"].as_array().unwrap() {
            let (z, px, tiles) = visible_tiles(
                f(&c["lat"]),
                f(&c["lon"]),
                f(&c["zoom"]),
                f(&c["width"]),
                f(&c["height"]),
            );
            assert_eq!(z as u64, c["z"].as_u64().unwrap());
            assert!(close(px, f(&c["tile_px"]), 1e-6));
            let want = c["tiles"].as_array().unwrap();
            assert_eq!(tiles.len(), want.len());
            for (got, want) in tiles.iter().zip(want) {
                assert_eq!(got.tile.key(), want["key"].as_str().unwrap());
                assert!(close(got.left, f(&want["left"]), 1e-6));
                assert!(close(got.top, f(&want["top"]), 1e-6));
            }
        }
        for c in v["project"].as_array().unwrap() {
            let (dx, dy) = project(
                f(&c["lat"]),
                f(&c["lon"]),
                f(&c["center"][0]),
                f(&c["center"][1]),
                f(&c["zoom"]),
            );
            assert!(close(dx, f(&c["dx"]), 1e-6), "{}", c["name"]);
            assert!(close(dy, f(&c["dy"]), 1e-6), "{}", c["name"]);
        }
    }

    #[test]
    fn replan_vectors() {
        let centres = |v: &Value| -> Vec<Centre> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|c| Centre {
                    id: c[0].as_str().unwrap().into(),
                    lat: f(&c[1]),
                    lon: f(&c[2]),
                })
                .collect()
        };
        for c in vectors()["replan"].as_array().unwrap() {
            assert_eq!(
                needs_replan(
                    &centres(&c["previous"]),
                    &centres(&c["current"]),
                    c["age_ms"].as_i64().unwrap(),
                    f(&c["move_m"]),
                    c["max_age_ms"].as_i64().unwrap(),
                ),
                c["replan"].as_bool().unwrap(),
                "{}",
                c["name"]
            );
        }
    }

    #[test]
    fn keys_are_validated() {
        assert_eq!(TileId::parse("15/9204/12054"), TileId::new(15, 9204, 12054));
        for bad in [
            "", "1/2", "1/2/3/4", "1/2/1", "23/0/0", "a/b/c", "-1/0/0", "3/8/0",
        ] {
            assert_eq!(TileId::parse(bad), None, "{bad}");
        }
    }

    /// The default watch set around three nearby centres stays well inside
    /// the tile limit, and a far-away centre is still covered at low zoom.
    #[test]
    fn default_levels_cover_a_launch_site() {
        let site = [
            LatLon::new(32.9903, -106.9750),
            LatLon::new(32.9411, -106.9192),
            LatLon::new(32.9500, -106.9300),
        ];
        let tiles = coverage(&site, WATCH_LEVELS, 17, MAX_SET_TILES);
        assert!(
            tiles.len() > 20 && tiles.len() < MAX_SET_TILES,
            "{}",
            tiles.len()
        );
        assert_eq!(tiles.first().unwrap().z, 11, "low zoom first");
        let max_native_13 = coverage(&site, WATCH_LEVELS, 13, MAX_SET_TILES);
        assert!(max_native_13.iter().all(|t| t.z <= 13));
    }
}
