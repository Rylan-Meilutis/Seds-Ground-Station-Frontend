import Foundation

/// What the watch knows about itself. Headings are degrees from TRUE north.
public struct CompassReading: Equatable {
    public var position: LatLon?
    public var trueHeading: Double?
    /// East-positive magnetic declination, when the watch reported both a
    /// true and a magnetic heading.
    public var declination: Double?

    public init(position: LatLon? = nil, trueHeading: Double? = nil, declination: Double? = nil) {
        self.position = position
        self.trueHeading = trueHeading
        self.declination = declination
    }
}

/// Everything the compass page shows for one target.
public struct TargetDisplay: Equatable, Identifiable {
    public var id: String
    public var label: String
    public var source: String
    public var distanceM: Double?
    public var distanceText: String
    /// Bearing in the selected north reference, e.g. `224° SW`.
    public var bearingText: String?
    /// North reference actually used for `bearingText` (magnetic falls back
    /// to true until the watch knows the declination).
    public var north: North
    /// Arrow angle clockwise from 12 o'clock; nil without heading, without a
    /// bearing, or when closer than `Compass.nearRadiusM`.
    public var arrowRotation: Double?
    /// "Within N m — look around" when the arrow is not trustworthy.
    public var nearText: String?
    public var stale: Bool
    public var ageMs: Int64
    public var ageText: String
    public var savedPosition: Bool
    public var savedPositionLabel: String? { savedPosition ? "saved position" : nil }
}

public enum Compass {
    /// Inside this radius GPS and compass error dominate the arrow.
    public static let nearRadiusM = 30.0

    /// Target position; demo targets sit at the watch position plus offset.
    public static func position(of target: Target, watch: LatLon?) -> LatLon? {
        guard let offset = target.demoOffset else { return LatLon(lat: target.lat, lon: target.lon) }
        guard let watch else { return nil }
        return displaced(watch, northM: offset.northM, eastM: offset.eastM)
    }

    /// Point `northM`/`eastM` metres from `p` on the mean sphere.
    public static func displaced(_ p: LatLon, northM: Double, eastM: Double) -> LatLon {
        let lat = min(max(p.lat + degrees(northM / earthRadiusM), -90), 90)
        let cosLat = cos(radians(p.lat))
        let dLon = abs(cosLat) < 1e-12 ? 0 : degrees(eastM / (earthRadiusM * cosLat))
        return LatLon(lat: lat, lon: normalizeDeg(p.lon + dLon + 180) - 180)
    }

    public static func display(
        _ target: Target, settings: WatchSettings, reading: CompassReading, receivedAtMs: Int64, nowMs: Int64
    ) -> TargetDisplay {
        let metric = settings.metric
        let ageMs = target.ageMs + max(0, nowMs - receivedAtMs)
        let stale = target.stale || ageMs > Int64(settings.compass.staleAfterS) * 1000
        var distance: Double?
        var bearing: Double?
        if let watch = reading.position, let pos = position(of: target, watch: watch) {
            distance = distanceM(watch, pos)
            bearing = initialBearingDeg(watch, pos)
        }
        let near = distance.map { $0 < nearRadiusM } ?? false

        var north = North.trueNorth
        var shown = bearing
        if settings.compass.north == .magnetic, let dec = reading.declination, let b = bearing {
            north = .magnetic
            shown = trueToMagneticDeg(b, dec)
        }
        var rotation: Double?
        if !near, let b = bearing, let h = reading.trueHeading {
            rotation = arrowRotationDeg(b, h)
        }
        return TargetDisplay(
            id: target.id,
            label: target.label,
            source: target.source,
            distanceM: distance,
            distanceText: distance.map { formatDistance($0, metric: metric) } ?? "--",
            bearingText: shown.map { "\(Int($0.rounded()) % 360)° \(cardinal($0))" },
            north: north,
            arrowRotation: rotation,
            nearText: near ? "Within \(formatDistance(distance ?? 0, metric: metric)) — look around" : nil,
            stale: stale,
            ageMs: ageMs,
            ageText: formatAge(ageMs),
            savedPosition: target.fallback
        )
    }

    /// Targets in the order chosen in settings (only those the phone sent).
    public static func displays(
        targets: [Target], settings: WatchSettings, reading: CompassReading, receivedAtMs: Int64, nowMs: Int64
    ) -> [TargetDisplay] {
        let wanted = settings.compass.targets
        let ordered = wanted.isEmpty ? targets : wanted.compactMap { id in targets.first { $0.id == id } }
        return ordered.map {
            display($0, settings: settings, reading: reading, receivedAtMs: receivedAtMs, nowMs: nowMs)
        }
    }
}
