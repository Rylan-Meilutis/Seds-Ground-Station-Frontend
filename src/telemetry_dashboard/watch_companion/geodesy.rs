//! Spherical geodesy for the watch compass. The Swift and Kotlin watch apps
//! port these functions and check them against `watch/test-vectors.json`.
//!
//! Distances use the haversine form on the IUGG mean Earth radius. Over the
//! few kilometres of a recovery walk the spherical error versus WGS-84 is
//! below 0.5 %, far smaller than consumer GPS error. Bearings are initial
//! great-circle bearings relative to TRUE north.

/// IUGG mean Earth radius in metres.
pub const EARTH_RADIUS_M: f64 = 6_371_008.8;
/// Below this separation a bearing is noise and is not reported.
pub const MIN_BEARING_DISTANCE_M: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

impl LatLon {
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }
    pub fn valid(self) -> bool {
        self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
    }
}

/// Normalises any finite angle into `[0, 360)`.
pub fn normalize_deg(deg: f64) -> f64 {
    let v = deg.rem_euclid(360.0);
    // rem_euclid can return 360.0 for tiny negative inputs due to rounding.
    if v >= 360.0 { 0.0 } else { v }
}

/// Signed smallest difference `to - from` in `(-180, 180]`.
pub fn signed_delta_deg(from: f64, to: f64) -> f64 {
    let d = normalize_deg(to - from);
    if d > 180.0 { d - 360.0 } else { d }
}

/// Great-circle distance in metres (haversine, atan2 form for stability at
/// both tiny and antipodal separations).
pub fn distance_m(a: LatLon, b: LatLon) -> Option<f64> {
    if !a.valid() || !b.valid() {
        return None;
    }
    let (p1, p2) = (a.lat.to_radians(), b.lat.to_radians());
    let dp = p2 - p1;
    let dl = (b.lon - a.lon).to_radians();
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    let h = h.clamp(0.0, 1.0);
    Some(2.0 * EARTH_RADIUS_M * h.sqrt().atan2((1.0 - h).sqrt()))
}

/// Initial great-circle bearing from `a` to `b`, degrees clockwise from true
/// north in `[0, 360)`. `None` when inputs are invalid, the points are closer
/// than [`MIN_BEARING_DISTANCE_M`], or `a` is at a pole (every direction is
/// south/north there, so a compass bearing is undefined).
pub fn initial_bearing_deg(a: LatLon, b: LatLon) -> Option<f64> {
    let d = distance_m(a, b)?;
    if d < MIN_BEARING_DISTANCE_M || a.lat.abs() >= 90.0 - 1e-9 {
        return None;
    }
    let (p1, p2) = (a.lat.to_radians(), b.lat.to_radians());
    let dl = (b.lon - a.lon).to_radians();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    if x == 0.0 && y == 0.0 {
        return None;
    }
    Some(normalize_deg(y.atan2(x).to_degrees()))
}

/// Converts a true bearing to magnetic using declination (east positive):
/// magnetic = true - declination.
pub fn true_to_magnetic_deg(true_deg: f64, declination_deg: f64) -> f64 {
    normalize_deg(true_deg - declination_deg)
}

/// Converts a magnetic heading to true: true = magnetic + declination.
pub fn magnetic_to_true_deg(magnetic_deg: f64, declination_deg: f64) -> f64 {
    normalize_deg(magnetic_deg + declination_deg)
}

/// Angle to draw the arrow, clockwise from the top of the watch face, given
/// the target's true bearing and the watch's true heading. Both must use the
/// same north reference; the display setting only changes printed numbers.
pub fn arrow_rotation_deg(bearing_true_deg: f64, heading_true_deg: f64) -> f64 {
    normalize_deg(bearing_true_deg - heading_true_deg)
}

/// Eight-point compass label for a bearing.
pub fn cardinal(deg: f64) -> &'static str {
    const NAMES: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    NAMES[((normalize_deg(deg) + 22.5) / 45.0) as usize % 8]
}

/// Distance text: metric shows m below 1 km, then km (2 dp below 10 km,
/// 1 dp above); imperial shows ft below 1000 ft, then mi likewise.
pub fn format_distance(meters: f64, metric: bool) -> String {
    if !meters.is_finite() || meters < 0.0 {
        return "--".into();
    }
    if metric {
        if meters < 999.5 {
            format!("{:.0} m", meters)
        } else if meters < 9_995.0 {
            format!("{:.2} km", meters / 1000.0)
        } else {
            format!("{:.1} km", meters / 1000.0)
        }
    } else {
        let feet = meters / 0.3048;
        let miles = meters / 1609.344;
        if feet < 999.5 {
            format!("{:.0} ft", feet)
        } else if miles < 9.995 {
            format!("{:.2} mi", miles)
        } else {
            format!("{:.1} mi", miles)
        }
    }
}

/// Compact age text used for fix and link ages: `4s`, `2m 05s`, `1h 03m`.
pub fn format_age(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn short_distances_are_accurate_to_centimetres() {
        // 0.0001 deg of latitude ~ 11.1195 m on the mean sphere.
        let a = LatLon::new(32.9904, -106.9750);
        let b = LatLon::new(32.9905, -106.9750);
        let d = distance_m(a, b).unwrap();
        assert!(close(d, 11.1195, 0.001), "{d}");
        assert!(close(initial_bearing_deg(a, b).unwrap(), 0.0, 1e-6));
        // 10 m due east at mid latitude.
        let east = LatLon::new(
            32.9904,
            -106.9750 + 10.0 / (111_195.08 * 32.9904f64.to_radians().cos()),
        );
        assert!(close(distance_m(a, east).unwrap(), 10.0, 0.01));
        assert!(close(initial_bearing_deg(a, east).unwrap(), 90.0, 0.01));
    }

    #[test]
    fn sub_metre_and_coincident_points_have_no_bearing() {
        let a = LatLon::new(40.0, -75.0);
        assert_eq!(distance_m(a, a), Some(0.0));
        assert_eq!(initial_bearing_deg(a, a), None);
        let b = LatLon::new(40.000005, -75.0); // ~0.56 m
        assert!(initial_bearing_deg(a, b).is_none());
    }

    #[test]
    fn known_long_baseline() {
        // Spaceport America to Las Cruces (approx), checked against an
        // independent haversine implementation.
        let a = LatLon::new(32.9904, -106.9750);
        let b = LatLon::new(32.3199, -106.7637);
        let d = distance_m(a, b).unwrap();
        assert!(close(d, 77_135.894, 0.01), "{d}");
        let brg = initial_bearing_deg(a, b).unwrap();
        assert!(close(brg, 165.08326, 1e-4), "{brg}");
    }

    #[test]
    fn near_pole_crossing_and_pole_origin() {
        // Across the pole: both at 89.9 N on opposite meridians.
        let a = LatLon::new(89.9, 0.0);
        let b = LatLon::new(89.9, 180.0);
        let d = distance_m(a, b).unwrap();
        // 0.2 degrees of arc.
        assert!(close(d, 0.2f64.to_radians() * EARTH_RADIUS_M, 0.01), "{d}");
        assert!(close(initial_bearing_deg(a, b).unwrap(), 0.0, 1e-6));
        // Going the other way also heads due north first.
        assert!(close(initial_bearing_deg(b, a).unwrap(), 0.0, 1e-6));
        // Near the pole a 90 degree longitude offset is north-east-ish.
        let c = LatLon::new(89.9, 90.0);
        assert!(close(initial_bearing_deg(a, c).unwrap(), 45.0, 0.01));
        // At the pole itself there is no compass bearing.
        let pole = LatLon::new(90.0, 0.0);
        assert!(initial_bearing_deg(pole, a).is_none());
        assert!(close(
            initial_bearing_deg(LatLon::new(-89.95, 10.0), LatLon::new(-89.95, -170.0)).unwrap(),
            180.0,
            1e-6
        ));
    }

    #[test]
    fn antimeridian_takes_the_short_path() {
        let a = LatLon::new(0.0, 179.9995);
        let b = LatLon::new(0.0, -179.9995);
        let d = distance_m(a, b).unwrap();
        assert!(close(d, 111.195, 0.01), "{d}");
        assert!(close(initial_bearing_deg(a, b).unwrap(), 90.0, 1e-6));
        assert!(close(initial_bearing_deg(b, a).unwrap(), 270.0, 1e-6));
    }

    #[test]
    fn antipodal_distance_is_half_circumference() {
        let d = distance_m(LatLon::new(10.0, 20.0), LatLon::new(-10.0, -160.0)).unwrap();
        // Antipodes are numerically the worst case for haversine; still sub-metre.
        assert!(close(d, std::f64::consts::PI * EARTH_RADIUS_M, 1.0), "{d}");
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        assert!(distance_m(LatLon::new(f64::NAN, 0.0), LatLon::new(0.0, 0.0)).is_none());
        assert!(distance_m(LatLon::new(91.0, 0.0), LatLon::new(0.0, 0.0)).is_none());
        assert!(initial_bearing_deg(LatLon::new(0.0, 181.0), LatLon::new(0.0, 0.0)).is_none());
    }

    #[test]
    fn magnetic_true_and_arrow_math() {
        // 10 deg east declination: true 0 is magnetic 350.
        assert!(close(true_to_magnetic_deg(0.0, 10.0), 350.0, 1e-9));
        assert!(close(magnetic_to_true_deg(350.0, 10.0), 0.0, 1e-9));
        // West declination.
        assert!(close(true_to_magnetic_deg(90.0, -7.5), 97.5, 1e-9));
        // Facing east (90), target due north (0): arrow points left (270).
        assert!(close(arrow_rotation_deg(0.0, 90.0), 270.0, 1e-9));
        assert!(close(arrow_rotation_deg(350.0, 10.0), 340.0, 1e-9));
        assert!(close(signed_delta_deg(350.0, 10.0), 20.0, 1e-9));
        assert!(close(signed_delta_deg(10.0, 350.0), -20.0, 1e-9));
        assert_eq!(normalize_deg(-1e-15), 0.0);
        assert_eq!(cardinal(359.0), "N");
        assert_eq!(cardinal(200.0), "S");
        assert_eq!(cardinal(300.0), "NW");
    }

    #[test]
    fn distance_and_age_formatting() {
        assert_eq!(format_distance(0.4, true), "0 m");
        assert_eq!(format_distance(999.4, true), "999 m");
        assert_eq!(format_distance(999.6, true), "1.00 km");
        assert_eq!(format_distance(1234.0, true), "1.23 km");
        assert_eq!(format_distance(12_345.0, true), "12.3 km");
        assert_eq!(format_distance(100.0, false), "328 ft");
        assert_eq!(format_distance(1609.344, false), "1.00 mi");
        assert_eq!(format_distance(f64::NAN, true), "--");
        assert_eq!(format_age(4_900), "4s");
        assert_eq!(format_age(125_000), "2m 05s");
        assert_eq!(format_age(3_780_000), "1h 03m");
        assert_eq!(format_age(-5), "0s");
    }

    /// The watch apps read the same vectors; keep this file in sync.
    #[test]
    fn shared_test_vectors_match() {
        let raw = include_str!("../../../watch/test-vectors.json");
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        for case in v["geodesy"].as_array().unwrap() {
            let a = LatLon::new(
                case["from"][0].as_f64().unwrap(),
                case["from"][1].as_f64().unwrap(),
            );
            let b = LatLon::new(
                case["to"][0].as_f64().unwrap(),
                case["to"][1].as_f64().unwrap(),
            );
            let d = distance_m(a, b).unwrap();
            let want = case["distance_m"].as_f64().unwrap();
            let tol = case["distance_tol_m"].as_f64().unwrap();
            assert!(close(d, want, tol), "{}: {d} vs {want}", case["name"]);
            if case["bearing_deg"] == "any" {
                continue;
            }
            match case["bearing_deg"].as_f64() {
                Some(want) => {
                    let got =
                        initial_bearing_deg(a, b).unwrap_or_else(|| panic!("{}", case["name"]));
                    assert!(
                        signed_delta_deg(got, want).abs() <= 0.01,
                        "{}: {got} vs {want}",
                        case["name"]
                    );
                }
                None => assert!(initial_bearing_deg(a, b).is_none(), "{}", case["name"]),
            }
        }
        for case in v["distance_format"].as_array().unwrap() {
            let m = case["m"].as_f64().unwrap();
            assert_eq!(format_distance(m, true), case["metric"].as_str().unwrap());
            assert_eq!(
                format_distance(m, false),
                case["imperial"].as_str().unwrap()
            );
        }
        for case in v["age_format"].as_array().unwrap() {
            assert_eq!(
                format_age(case["ms"].as_i64().unwrap()),
                case["text"].as_str().unwrap()
            );
        }
        for case in v["arrow"].as_array().unwrap() {
            let got = arrow_rotation_deg(
                case["bearing_true"].as_f64().unwrap(),
                case["heading_true"].as_f64().unwrap(),
            );
            assert!(close(got, case["arrow"].as_f64().unwrap(), 1e-9));
            let mag = true_to_magnetic_deg(
                case["bearing_true"].as_f64().unwrap(),
                case["declination"].as_f64().unwrap(),
            );
            assert!(close(mag, case["bearing_magnetic"].as_f64().unwrap(), 1e-9));
        }
    }
}
