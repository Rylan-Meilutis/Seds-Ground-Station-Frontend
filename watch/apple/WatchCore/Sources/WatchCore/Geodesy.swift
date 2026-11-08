import Foundation

// Port of src/telemetry_dashboard/watch_companion/geodesy.rs. Checked against
// watch/test-vectors.json. Bearings are relative to TRUE north.

/// IUGG mean Earth radius in metres.
public let earthRadiusM = 6_371_008.8
/// Below this separation a bearing is noise and is not reported.
public let minBearingDistanceM = 1.0

public struct LatLon: Equatable {
    public var lat: Double
    public var lon: Double

    public init(lat: Double, lon: Double) {
        self.lat = lat
        self.lon = lon
    }

    public var isValid: Bool {
        lat.isFinite && lon.isFinite && (-90.0...90.0).contains(lat) && (-180.0...180.0).contains(lon)
    }
}

// Same constant ordering as Rust's f64::to_radians / to_degrees.
@inline(__always) func radians(_ deg: Double) -> Double { deg * (Double.pi / 180.0) }
@inline(__always) func degrees(_ rad: Double) -> Double { rad * (180.0 / Double.pi) }

/// Normalises any finite angle into `[0, 360)` (Rust `rem_euclid` semantics).
public func normalizeDeg(_ deg: Double) -> Double {
    var v = deg.truncatingRemainder(dividingBy: 360.0)
    if v < 0 { v += 360.0 }
    return v >= 360.0 ? 0.0 : v
}

/// Signed smallest difference `to - from` in `(-180, 180]`.
public func signedDeltaDeg(from: Double, to: Double) -> Double {
    let d = normalizeDeg(to - from)
    return d > 180.0 ? d - 360.0 : d
}

/// Great-circle distance in metres (haversine, atan2 form).
public func distanceM(_ a: LatLon, _ b: LatLon) -> Double? {
    guard a.isValid, b.isValid else { return nil }
    let p1 = radians(a.lat), p2 = radians(b.lat)
    let dp = p2 - p1
    let dl = radians(b.lon - a.lon)
    let sdp = sin(dp / 2.0), sdl = sin(dl / 2.0)
    let h = min(max(sdp * sdp + cos(p1) * cos(p2) * sdl * sdl, 0.0), 1.0)
    return 2.0 * earthRadiusM * atan2(h.squareRoot(), (1.0 - h).squareRoot())
}

/// Initial great-circle bearing from `a` to `b` in `[0, 360)`. Nil when
/// inputs are invalid, closer than 1 m, or `a` is at a pole.
public func initialBearingDeg(_ a: LatLon, _ b: LatLon) -> Double? {
    guard let d = distanceM(a, b) else { return nil }
    if d < minBearingDistanceM || abs(a.lat) >= 90.0 - 1e-9 { return nil }
    let p1 = radians(a.lat), p2 = radians(b.lat)
    let dl = radians(b.lon - a.lon)
    let y = sin(dl) * cos(p2)
    let x = cos(p1) * sin(p2) - sin(p1) * cos(p2) * cos(dl)
    if x == 0 && y == 0 { return nil }
    return normalizeDeg(degrees(atan2(y, x)))
}

/// magnetic = true - declination (east positive).
public func trueToMagneticDeg(_ trueDeg: Double, _ declinationDeg: Double) -> Double {
    normalizeDeg(trueDeg - declinationDeg)
}

/// true = magnetic + declination.
public func magneticToTrueDeg(_ magneticDeg: Double, _ declinationDeg: Double) -> Double {
    normalizeDeg(magneticDeg + declinationDeg)
}

/// Arrow angle clockwise from the top of the face. Both inputs true-north.
public func arrowRotationDeg(_ bearingTrueDeg: Double, _ headingTrueDeg: Double) -> Double {
    normalizeDeg(bearingTrueDeg - headingTrueDeg)
}

/// Eight-point compass label.
public func cardinal(_ deg: Double) -> String {
    let names = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"]
    return names[Int((normalizeDeg(deg) + 22.5) / 45.0) % 8]
}

/// Metric: m below 1 km, then km (2 dp below 10 km, 1 dp above).
/// Imperial: ft below 1000 ft, then mi likewise.
public func formatDistance(_ meters: Double, metric: Bool) -> String {
    guard meters.isFinite, meters >= 0 else { return "--" }
    if metric {
        if meters < 999.5 { return String(format: "%.0f m", meters) }
        if meters < 9_995.0 { return String(format: "%.2f km", meters / 1000.0) }
        return String(format: "%.1f km", meters / 1000.0)
    }
    let feet = meters / 0.3048
    let miles = meters / 1609.344
    if feet < 999.5 { return String(format: "%.0f ft", feet) }
    if miles < 9.995 { return String(format: "%.2f mi", miles) }
    return String(format: "%.1f mi", miles)
}

func pad2(_ v: Int64) -> String { v < 10 ? "0\(v)" : "\(v)" }

/// Compact age text: `4s`, `2m 05s`, `1h 03m`.
public func formatAge(_ ms: Int64) -> String {
    let s = max(ms, 0) / 1000
    if s < 60 { return "\(s)s" }
    if s < 3600 { return "\(s / 60)m \(pad2(s % 60))s" }
    return "\(s / 3600)h \(pad2((s % 3600) / 60))m"
}
