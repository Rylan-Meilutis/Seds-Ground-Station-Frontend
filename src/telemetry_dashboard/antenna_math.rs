//! Antenna geometry in a local east/north/up frame; angles are true bearings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltitudeReference {
    MeanSeaLevel,
    Ellipsoid,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: Option<f64>,
    pub altitude_reference: AltitudeReference,
}
impl Position {
    pub fn valid(self) -> bool {
        self.latitude.is_finite()
            && self.longitude.is_finite()
            && (-90.0..=90.0).contains(&self.latitude)
            && (-180.0..=180.0).contains(&self.longitude)
            && self.altitude_m.is_none_or(f64::is_finite)
    }
    fn ecef(self, height: f64) -> [f64; 3] {
        const A: f64 = 6_378_137.0;
        const F: f64 = 1.0 / 298.257_223_563;
        let e2 = F * (2.0 - F);
        let (sin_lat, cos_lat) = self.latitude.to_radians().sin_cos();
        let (sin_lon, cos_lon) = self.longitude.to_radians().sin_cos();
        let n = A / (1.0 - e2 * sin_lat * sin_lat).sqrt();
        [
            (n + height) * cos_lat * cos_lon,
            (n + height) * cos_lat * sin_lon,
            (n * (1.0 - e2) + height) * sin_lat,
        ]
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    pub bearing_deg: Option<f64>,
    pub elevation_deg: Option<f64>,
    pub distance_m: f64,
}

pub fn direction(origin: Position, target: Position) -> Option<Aim> {
    if !origin.valid() || !target.valid() {
        return None;
    }
    let common_altitude = origin.altitude_reference != AltitudeReference::Unknown
        && origin.altitude_reference == target.altitude_reference
        && origin.altitude_m.is_some()
        && target.altitude_m.is_some();
    let a = origin.ecef(if common_altitude {
        origin.altitude_m?
    } else {
        0.0
    });
    let b = target.ecef(if common_altitude {
        target.altitude_m?
    } else {
        0.0
    });
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let (slat, clat) = origin.latitude.to_radians().sin_cos();
    let (slon, clon) = origin.longitude.to_radians().sin_cos();
    let east = -slon * d[0] + clon * d[1];
    let north = -slat * clon * d[0] - slat * slon * d[1] + clat * d[2];
    let up = clat * clon * d[0] + clat * slon * d[1] + slat * d[2];
    let horizontal = east.hypot(north);
    let distance = horizontal.hypot(up);
    Some(Aim {
        bearing_deg: (horizontal > 0.01).then(|| east.atan2(north).to_degrees().rem_euclid(360.0)),
        elevation_deg: (common_altitude && distance > 0.01)
            .then(|| up.atan2(horizontal).to_degrees()),
        distance_m: distance,
    })
}

pub fn signed_error(target_deg: f64, measured_deg: f64) -> Option<f64> {
    (target_deg.is_finite() && measured_deg.is_finite())
        .then(|| (target_deg - measured_deg + 180.0).rem_euclid(360.0) - 180.0)
}

/// W3C intrinsic Z-X-Y orientation, looking along the back-camera axis (-Z).
/// Caller must supply an absolute/north-referenced alpha, not relative gyro yaw.
pub fn back_camera_direction(alpha: f64, beta: f64, gamma: f64) -> Option<Aim> {
    if ![alpha, beta, gamma].into_iter().all(f64::is_finite) {
        return None;
    }
    let (sa, ca) = alpha.to_radians().sin_cos();
    let (sb, cb) = beta.to_radians().sin_cos();
    let (sg, cg) = gamma.to_radians().sin_cos();
    let east = -ca * sg - sa * sb * cg;
    let north = -sa * sg + ca * sb * cg;
    let up = -cb * cg;
    let horizontal = east.hypot(north);
    Some(Aim {
        bearing_deg: (horizontal > 1e-6).then(|| east.atan2(north).to_degrees().rem_euclid(360.0)),
        elevation_deg: Some(up.atan2(horizontal).to_degrees()),
        distance_m: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(lat: f64, lon: f64, alt: f64) -> Position {
        Position {
            latitude: lat,
            longitude: lon,
            altitude_m: Some(alt),
            altitude_reference: AltitudeReference::MeanSeaLevel,
        }
    }
    #[test]
    fn cardinal_directions_and_date_line_use_short_path() {
        for (target, expected) in [
            (p(0.01, 0.0, 0.0), 0.0),
            (p(0.0, 0.01, 0.0), 90.0),
            (p(-0.01, 0.0, 0.0), 180.0),
            (p(0.0, -0.01, 0.0), 270.0),
        ] {
            let aim = direction(p(0.0, 0.0, 0.0), target).unwrap();
            assert!(
                signed_error(expected, aim.bearing_deg.unwrap())
                    .unwrap()
                    .abs()
                    < 0.01
            );
        }
        let aim = direction(p(0.0, 179.99, 0.0), p(0.0, -179.99, 0.0)).unwrap();
        assert!((aim.bearing_deg.unwrap() - 90.0).abs() < 0.01);
        assert!((2000.0..2500.0).contains(&aim.distance_m));
    }
    #[test]
    fn north_wraparound_and_invalid_sensor_values() {
        assert_eq!(signed_error(1.0, 359.0), Some(2.0));
        assert_eq!(signed_error(359.0, 1.0), Some(-2.0));
        assert_eq!(signed_error(360.0, 0.0), Some(0.0));
        assert_eq!(signed_error(f64::NAN, 0.0), None);
        assert!(direction(p(91.0, 0.0, 0.0), p(0.0, 0.0, 0.0)).is_none());
    }
    #[test]
    fn altitude_datums_must_match_for_vertical_guidance() {
        let mut target = p(0.01, 0.0, 1000.0);
        target.altitude_reference = AltitudeReference::Ellipsoid;
        let aim = direction(p(0.0, 0.0, 0.0), target).unwrap();
        assert!(aim.bearing_deg.is_some());
        assert_eq!(aim.elevation_deg, None);
        target.altitude_reference = AltitudeReference::Unknown;
        assert_eq!(
            direction(p(0.0, 0.0, 0.0), target).unwrap().elevation_deg,
            None
        );
    }
    #[test]
    fn coincident_and_overhead_targets_do_not_invent_a_bearing() {
        let origin = p(43.0, -78.0, 200.0);
        let same = direction(origin, origin).unwrap();
        assert_eq!(same.bearing_deg, None);
        assert_eq!(same.elevation_deg, None);
        let overhead = direction(origin, p(43.0, -78.0, 1200.0)).unwrap();
        assert_eq!(overhead.bearing_deg, None);
        assert!((overhead.elevation_deg.unwrap() - 90.0).abs() < 1e-6);
    }
    #[test]
    fn distant_same_height_target_is_below_geometric_horizon() {
        let aim = direction(p(0.0, 0.0, 100.0), p(0.0, 1.0, 100.0)).unwrap();
        assert!((aim.elevation_deg.unwrap() + 0.5).abs() < 0.01);
    }
    #[test]
    fn rear_camera_axis_tracks_yaw_and_tilt() {
        for (alpha, bearing) in [(0.0, 0.0), (90.0, 270.0), (180.0, 180.0), (270.0, 90.0)] {
            let aim = back_camera_direction(alpha, 90.0, 0.0).unwrap();
            assert!(
                signed_error(bearing, aim.bearing_deg.unwrap())
                    .unwrap()
                    .abs()
                    < 1e-6
            );
            assert!(aim.elevation_deg.unwrap().abs() < 1e-6);
        }
        let down = back_camera_direction(0.0, 0.0, 0.0).unwrap();
        assert_eq!(down.bearing_deg, None);
        assert!((down.elevation_deg.unwrap() + 90.0).abs() < 1e-6);
        assert!(back_camera_direction(f64::INFINITY, 0.0, 0.0).is_none());
    }
}

/// A readable 1/2/5 map radius, keeping the destination inside the viewport.
pub fn map_range_m(distance_m: f64) -> f64 {
    let needed = (distance_m.max(0.0) * 1.2).max(25.0);
    let decade = 10.0_f64.powf(needed.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .find(|step| step * decade >= needed)
        .unwrap_or(10.0)
        * decade
}
/// Relative GPS map: the selected origin stays centered and forward stays up.
pub fn heading_up_point(distance_m: f64, error_deg: f64, range_m: f64) -> (f64, f64) {
    let radius = (distance_m / range_m).clamp(0.0, 1.0) * 100.0;
    let angle = error_deg.to_radians();
    (140.0 + angle.sin() * radius, 140.0 - angle.cos() * radius)
}
#[cfg(test)]
mod heading_map_tests {
    use super::*;
    #[test]
    fn forward_and_right_follow_heading() {
        assert_eq!(heading_up_point(100.0, 0.0, 100.0), (140.0, 40.0));
        let (x, y) = heading_up_point(100.0, 90.0, 100.0);
        assert!((x - 240.0).abs() < 1e-8 && (y - 140.0).abs() < 1e-8);
        let (x, y) = heading_up_point(100.0, -90.0, 100.0);
        assert!((x - 40.0).abs() < 1e-8 && (y - 140.0).abs() < 1e-8);
    }
    #[test]
    fn map_radius_keeps_near_and_far_targets_visible() {
        for distance in [0.0, 1.0, 25.0, 100.0, 1113.0, 100000.0] {
            let range = map_range_m(distance);
            assert!(range >= 25.0 && range >= distance * 1.2);
        }
    }
}
