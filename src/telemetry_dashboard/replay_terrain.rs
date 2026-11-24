//! Terrain for the 3D flight replay outside the live 3D map: the station's
//! heightmap around the recorded track (`GET /api/terrain/heightmap`, the
//! same request a replay bundle snapshots) and, for the standalone HTML
//! replay, the imagery tiles the scene drapes over it (`GET /tiles/...`).
//!
//! The standalone file opens from `file://` with no station, so whatever it
//! shows must be inside it. Its terrain is capped at
//! [`STANDALONE_TERRAIN_MAX_BYTES`]: the heightmap first (without it there is
//! no terrain), then imagery tiles nearest the pad first, for the two
//! textures the scene paints (`station_ui/rendering/three/terrain.js`
//! `imageryPlan`), until the cap. Anything that does not fit is left out and
//! the scene draws what it has (DEM without imagery, or flat ground).
use base64::Engine;
use serde_json::Value;

/// Heightmap plus base64 imagery embedded in one standalone replay file.
pub(crate) const STANDALONE_TERRAIN_MAX_BYTES: usize = 4 * 1024 * 1024;
/// One imagery tile (station tiles are 256 px JPEGs of tens of KiB).
pub(crate) const TILE_MAX_BYTES: usize = 512 * 1024;
/// Missing tiles in a row after which the station is taken to have none here.
const TILE_MISSES: usize = 6;
/// DEM samples per side, as the station's bundle requests it.
const GRID_SIZE: u32 = 129;
const EARTH_RADIUS_M: f64 = 6_371_008.8;
// terrain.js TERRAIN_DEFAULTS.
const MAX_TILES: usize = 36;
const MAX_IMAGERY_ZOOM: u32 = 17;

/// `[lat, lon]` of the replay's valid GPS fixes, in sample order.
fn fixes(document: &Value) -> Vec<(f64, f64)> {
    document["samples"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| matches!(s["data_type"].as_str(), Some("GPS_DATA" | "GPS" | "ROCKET_GPS")))
        .filter_map(|s| Some((s["values"][0].as_f64()?, s["values"][1].as_f64()?)))
        .filter(|(lat, lon)| lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0 && (lat.abs() > 1e-4 || lon.abs() > 1e-4))
        .collect()
}

fn distance_m((lat0, lon0): (f64, f64), (lat, lon): (f64, f64)) -> f64 {
    let north = (lat - lat0).to_radians() * EARTH_RADIUS_M;
    let east = (lon - lon0).to_radians() * EARTH_RADIUS_M * lat0.to_radians().cos();
    north.hypot(east)
}

/// The heightmap request for a replay: centred on the track's first fix and
/// covering the track plus 1 km (2–30 km), as a replay bundle requests it.
pub(crate) fn heightmap_path(document: &Value) -> Option<String> {
    let fixes = fixes(document);
    let first = *fixes.first()?;
    let reach = fixes.iter().map(|f| distance_m(first, *f)).fold(0.0, f64::max);
    let extent = (2.0 * (reach + 1000.0)).clamp(2000.0, 30000.0).round();
    Some(format!("/api/terrain/heightmap?lat={:.6}&lon={:.6}&extent_m={extent}&size={GRID_SIZE}", first.0, first.1))
}

fn world_pixel(lat: f64, lon: f64, z: u32) -> (f64, f64) {
    let n = 256.0 * 2f64.powi(z as i32);
    let rad = lat.clamp(-85.05, 85.05).to_radians();
    ((lon + 180.0) / 360.0 * n, (1.0 - (rad.tan() + 1.0 / rad.cos()).ln() / std::f64::consts::PI) / 2.0 * n)
}

fn imagery_zoom(lat: f64, extent_m: f64) -> u32 {
    let across = ((MAX_TILES as f64).sqrt().floor()).max(1.0);
    for z in (1..=MAX_IMAGERY_ZOOM.min(22)).rev() {
        let mpp = 2.0 * std::f64::consts::PI * EARTH_RADIUS_M * lat.to_radians().cos() / (256.0 * 2f64.powi(z as i32));
        if extent_m / (256.0 * mpp) + 1.0 <= across {
            return z;
        }
    }
    0
}

fn local_to_gps(lat0: f64, lon0: f64, east: f64, north: f64) -> (f64, f64) {
    let rad = std::f64::consts::PI / 180.0;
    (lat0 + north / EARTH_RADIUS_M / rad, lon0 + east / (EARTH_RADIUS_M * (lat0 * rad).cos().max(1e-6)) / rad)
}

/// The tiles of one texture, nearest the centre first (terrain.js `imageryPlan`).
fn plan_tiles(lat: f64, lon: f64, extent_m: f64) -> Vec<(u32, i64, i64)> {
    let z = imagery_zoom(lat, extent_m);
    let (n_lat, w_lon) = local_to_gps(lat, lon, -extent_m / 2.0, extent_m / 2.0);
    let (s_lat, e_lon) = local_to_gps(lat, lon, extent_m / 2.0, -extent_m / 2.0);
    let (x0, y0) = world_pixel(n_lat, w_lon, z);
    let (x1, y1) = world_pixel(s_lat, e_lon, z);
    let mut tiles = Vec::new();
    for tx in (x0 / 256.0).floor() as i64..=(x1 / 256.0).floor() as i64 {
        for ty in (y0 / 256.0).floor() as i64..=(y1 / 256.0).floor() as i64 {
            tiles.push((z, tx, ty));
        }
    }
    let (cx, cy) = world_pixel(lat, lon, z);
    let distance = |&(_, x, y): &(u32, i64, i64)| (x as f64 * 256.0 + 128.0 - cx).hypot(y as f64 * 256.0 + 128.0 - cy);
    tiles.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
    tiles.truncate(MAX_TILES);
    tiles
}

/// The imagery tiles the flight scene requests over `heightmap`: the near
/// texture (the inner quarter of the grid) first, then the far one.
pub(crate) fn imagery_tiles(heightmap: &Value) -> Vec<(u32, i64, i64)> {
    let (Some(lat), Some(lon), Some(extent), Some(size)) = (heightmap["lat_deg"].as_f64(), heightmap["lon_deg"].as_f64(), heightmap["extent_m"].as_f64(), heightmap["size"].as_u64()) else {
        return Vec::new();
    };
    if size < 2 || extent <= 0.0 {
        return Vec::new();
    }
    let cells = (size - 1) as f64;
    let near_cells = (cells / 4.0).round().max(2.0);
    let mut out = plan_tiles(lat, lon, extent * near_cells / cells);
    for tile in plan_tiles(lat, lon, extent) {
        if !out.contains(&tile) {
            out.push(tile);
        }
    }
    out
}

/// What [`embed`] put in the file, for the export's status line.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Embedded {
    pub heightmap: bool,
    pub tiles: usize,
    pub tiles_left_out: usize,
}

impl Embedded {
    pub(crate) fn note(&self) -> String {
        match (self.heightmap, self.tiles, self.tiles_left_out) {
            (false, _, _) => "No terrain: the station has none for this area.".into(),
            (true, 0, _) => "Terrain included, without imagery.".into(),
            (true, n, 0) => format!("Terrain and {n} imagery tiles included."),
            (true, n, left) => format!("Terrain and {n} imagery tiles included; {left} left out at the {} MiB cap.", STANDALONE_TERRAIN_MAX_BYTES >> 20),
        }
    }
}

/// Puts `heightmap` and the imagery `tile(z, x, y)` returns into the replay's
/// `replay_extras` (`terrain`, `terrain_tiles` as `{"z/x/y": base64 JPEG}`)
/// within `cap` bytes: nothing when the heightmap is unavailable or alone
/// exceeds the cap.
pub(crate) async fn embed<F, Fut>(document: &mut Value, heightmap: Value, cap: usize, mut tile: F) -> Embedded
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Option<Vec<u8>>>,
{
    let mut out = Embedded::default();
    if heightmap["available"] != true {
        return out;
    }
    let mut used = serde_json::to_string(&heightmap).map_or(usize::MAX, |t| t.len());
    if used > cap {
        return out;
    }
    let wanted = imagery_tiles(&heightmap);
    let mut tiles = serde_json::Map::new();
    let mut misses = 0;
    for (i, (z, x, y)) in wanted.iter().enumerate() {
        let Some(bytes) = tile(format!("/tiles/{z}/{x}/{y}.jpg")).await else {
            misses += 1;
            if misses >= TILE_MISSES && tiles.is_empty() {
                break;
            }
            continue;
        };
        misses = 0;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        // Key, quotes, colon and comma besides the data.
        let cost = encoded.len() + 24;
        if used + cost > cap {
            out.tiles_left_out = wanted.len() - i;
            break;
        }
        used += cost;
        tiles.insert(format!("{z}/{x}/{y}"), Value::String(encoded));
    }
    if !document["replay_extras"].is_object() {
        document["replay_extras"] = Value::Object(Default::default());
    }
    out.heightmap = true;
    out.tiles = tiles.len();
    document["replay_extras"]["terrain"] = heightmap;
    if !tiles.is_empty() {
        document["replay_extras"]["terrain_tiles"] = Value::Object(tiles);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heightmap() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/backend-contract/terrain/heightmap.json")).unwrap()
    }

    fn replay() -> Value {
        let row = |lat: f64, lon: f64| serde_json::json!({"data_type": "GPS_DATA", "sender_id": "FC", "values": [lat, lon, 900.0]});
        serde_json::json!({"format": "seds-replay", "version": 1, "samples": [
            {"data_type": "GPS_DATA", "sender_id": "FC", "values": [0.0, 0.0, 0.0]},
            row(31.758, -102.046), row(31.770, -102.040), {"data_type": "BAROMETER_DATA", "values": [1.0, 2.0]}, row(31.760, -102.030)
        ]})
    }

    #[test]
    fn the_heightmap_is_requested_around_the_track_like_a_bundle() {
        // First fix 31.758, -102.046; the farthest fix is ~1.5 km away: (1.5 + 1) km * 2.
        let path = heightmap_path(&replay()).unwrap();
        assert!(path.starts_with("/api/terrain/heightmap?lat=31.758000&lon=-102.046000&extent_m="), "{path}");
        let extent: f64 = path.split("extent_m=").nth(1).unwrap().split('&').next().unwrap().parse().unwrap();
        assert!((4900.0..5200.0).contains(&extent), "{extent}");
        assert!(path.ends_with("&size=129"));
        assert_eq!(heightmap_path(&serde_json::json!({"samples": []})), None, "no fix, no terrain");
    }

    /// Tiles `imageryPlan` in terrain.js asks for over the fixture heightmap
    /// (node: `imageryPlan(31.758, -102.046, extent, {})` for the near and far textures).
    #[test]
    fn the_tiles_are_the_ones_the_scene_paints() {
        let near: Vec<(u32, i64, i64)> = [[28382, 53331], [28381, 53331], [28382, 53330], [28381, 53330], [28382, 53332], [28383, 53331], [28381, 53332], [28383, 53330], [28383, 53332]]
            .iter().map(|[x, y]| (17, *x, *y)).collect();
        let far: Vec<(u32, i64, i64)> = [
            [14191, 26665], [14190, 26665], [14191, 26666], [14190, 26666], [14191, 26664], [14190, 26664], [14192, 26665], [14189, 26665], [14192, 26666],
            [14192, 26664], [14189, 26666], [14191, 26667], [14189, 26664], [14190, 26667], [14191, 26663], [14190, 26663], [14192, 26667], [14193, 26665],
            [14189, 26667], [14192, 26663], [14193, 26666], [14189, 26663], [14193, 26664], [14193, 26667], [14193, 26663],
        ].iter().map(|[x, y]| (16, *x, *y)).collect();
        assert_eq!(imagery_tiles(&heightmap()), [near, far].concat());
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(future)
    }

    #[test]
    fn terrain_is_embedded_heightmap_first_and_imagery_within_the_cap() {
        let jpeg = vec![0xffu8; 3000];
        let mut document = replay();
        let all = block_on(embed(&mut document, heightmap(), STANDALONE_TERRAIN_MAX_BYTES, |_| std::future::ready(Some(jpeg.clone()))));
        assert!(all.heightmap && all.tiles == imagery_tiles(&heightmap()).len() && all.tiles_left_out == 0, "{all:?}");
        assert_eq!(document["replay_extras"]["terrain"], heightmap());
        let tiles = document["replay_extras"]["terrain_tiles"].as_object().unwrap();
        let (z, x, y) = imagery_tiles(&heightmap())[0];
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(tiles[&format!("{z}/{x}/{y}")].as_str().unwrap()).unwrap(), jpeg);
        assert_eq!(all.note(), format!("Terrain and {} imagery tiles included.", all.tiles));

        // A small cap: the heightmap and the nearest tiles only.
        let mut capped = replay();
        let cap = serde_json::to_string(&heightmap()).unwrap().len() + 3 * 4100;
        let some = block_on(embed(&mut capped, heightmap(), cap, |_| std::future::ready(Some(jpeg.clone()))));
        assert_eq!((some.tiles, some.tiles_left_out > 0), (3, true));
        assert!(some.note().contains("left out at the 4 MiB cap"));
        assert!(capped.to_string().len() < replay().to_string().len() + cap + 200, "within the cap");

        // No imagery at the station: terrain alone, after a few misses.
        let mut asked = 0;
        let mut bare = replay();
        let none = block_on(embed(&mut bare, heightmap(), STANDALONE_TERRAIN_MAX_BYTES, |_| {
            asked += 1;
            std::future::ready(None)
        }));
        assert_eq!((none.heightmap, none.tiles, asked), (true, 0, TILE_MISSES));
        assert!(bare["replay_extras"]["terrain_tiles"].is_null());

        // Heightmap unavailable or over the cap: nothing at all.
        let mut plain = replay();
        let unavailable = serde_json::json!({"available": false});
        assert_eq!(block_on(embed(&mut plain, unavailable, STANDALONE_TERRAIN_MAX_BYTES, |_| std::future::ready(Some(jpeg.clone())))), Embedded::default());
        assert_eq!(block_on(embed(&mut plain, heightmap(), 100, |_| std::future::ready(Some(jpeg.clone())))), Embedded::default());
        assert!(plain["replay_extras"].is_null());
        assert_eq!(Embedded::default().note(), "No terrain: the station has none for this area.");
    }
}
