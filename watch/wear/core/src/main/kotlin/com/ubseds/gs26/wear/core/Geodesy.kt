package com.ubseds.gs26.wear.core

import kotlin.math.abs
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * Port of the phone's `watch_companion/geodesy.rs`. Checked against
 * `watch/test-vectors.json`; keep both in sync.
 */
object Geodesy {
    /** IUGG mean Earth radius in metres. */
    const val EARTH_RADIUS_M = 6_371_008.8
    /** Below this separation a bearing is noise and is not reported. */
    const val MIN_BEARING_DISTANCE_M = 1.0

    private val CARDINALS = arrayOf("N", "NE", "E", "SE", "S", "SW", "W", "NW")

    /** Normalises any finite angle into [0, 360). */
    fun normalizeDeg(deg: Double): Double {
        val v = deg.mod(360.0)
        return if (v >= 360.0) 0.0 else v
    }

    /** Signed smallest difference `to - from` in (-180, 180]. */
    fun signedDeltaDeg(from: Double, to: Double): Double {
        val d = normalizeDeg(to - from)
        return if (d > 180.0) d - 360.0 else d
    }

    /** Great-circle distance in metres (haversine, atan2 form). */
    fun distanceM(a: LatLon, b: LatLon): Double? {
        if (!a.valid() || !b.valid()) return null
        val p1 = Math.toRadians(a.lat)
        val p2 = Math.toRadians(b.lat)
        val dp = p2 - p1
        val dl = Math.toRadians(b.lon - a.lon)
        val h = (sin(dp / 2).pow(2) + cos(p1) * cos(p2) * sin(dl / 2).pow(2)).coerceIn(0.0, 1.0)
        return 2.0 * EARTH_RADIUS_M * atan2(sqrt(h), sqrt(1.0 - h))
    }

    /**
     * Initial great-circle bearing from [a] to [b], degrees clockwise from true
     * north in [0, 360). Null when inputs are invalid, the points are closer
     * than [MIN_BEARING_DISTANCE_M], or [a] is at a pole.
     */
    fun initialBearingDeg(a: LatLon, b: LatLon): Double? {
        val d = distanceM(a, b) ?: return null
        if (d < MIN_BEARING_DISTANCE_M || abs(a.lat) >= 90.0 - 1e-9) return null
        val p1 = Math.toRadians(a.lat)
        val p2 = Math.toRadians(b.lat)
        val dl = Math.toRadians(b.lon - a.lon)
        val y = sin(dl) * cos(p2)
        val x = cos(p1) * sin(p2) - sin(p1) * cos(p2) * cos(dl)
        if (x == 0.0 && y == 0.0) return null
        return normalizeDeg(Math.toDegrees(atan2(y, x)))
    }

    /** magnetic = true - declination (east positive). */
    fun trueToMagneticDeg(trueDeg: Double, declinationDeg: Double) = normalizeDeg(trueDeg - declinationDeg)

    /** true = magnetic + declination. */
    fun magneticToTrueDeg(magneticDeg: Double, declinationDeg: Double) = normalizeDeg(magneticDeg + declinationDeg)

    /** Arrow angle clockwise from the top of the face; both inputs true north. */
    fun arrowRotationDeg(bearingTrueDeg: Double, headingTrueDeg: Double) = normalizeDeg(bearingTrueDeg - headingTrueDeg)

    /** Eight-point compass label. */
    fun cardinal(deg: Double): String = CARDINALS[((normalizeDeg(deg) + 22.5) / 45.0).toInt() % 8]

    /** Point [northM]/[eastM] metres from [origin] (flat-earth offset, fine for demo distances). */
    fun offset(origin: LatLon, northM: Double, eastM: Double): LatLon {
        val lat = origin.lat + Math.toDegrees(northM / EARTH_RADIUS_M)
        val cosLat = cos(Math.toRadians(origin.lat)).coerceAtLeast(1e-6)
        var lon = origin.lon + Math.toDegrees(eastM / (EARTH_RADIUS_M * cosLat))
        if (lon > 180.0) lon -= 360.0
        if (lon < -180.0) lon += 360.0
        return LatLon(lat.coerceIn(-90.0, 90.0), lon)
    }
}

data class LatLon(val lat: Double, val lon: Double) {
    fun valid(): Boolean =
        lat.isFinite() && lon.isFinite() && lat in -90.0..90.0 && lon in -180.0..180.0
}
