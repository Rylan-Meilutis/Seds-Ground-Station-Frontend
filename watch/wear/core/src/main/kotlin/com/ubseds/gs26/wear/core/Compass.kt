package com.ubseds.gs26.wear.core

import kotlin.math.roundToInt

/** Everything the compass page shows for one target, computed from plain inputs. */
data class CompassTarget(
    val id: String,
    val label: String,
    val source: String,
    /** Null until both the watch and the target have a usable position. */
    val distanceM: Double?,
    val distanceText: String,
    /** e.g. `165° S true`; null without a bearing. */
    val bearingText: String?,
    /** Clockwise from 12 o'clock; null without bearing/heading or when near. */
    val arrowDeg: Double?,
    val ageMs: Long,
    val ageText: String,
    val stale: Boolean,
    /** "saved position" for manual/fallback fixes. */
    val fallbackLabel: String?,
    /** "Within N m — look around" when closer than [Compass.NEAR_M]. */
    val nearText: String?,
)

object Compass {
    const val NEAR_M = 30.0
    const val FALLBACK_LABEL = "saved position"

    /** Snapshot targets in the order configured in settings, skipping ones the phone did not send. */
    fun orderedTargets(snapshot: Snapshot, settings: WatchSettings): List<Target> {
        val byId = snapshot.targets.associateBy { it.id }
        return settings.compass.targets.mapNotNull { byId[it] }
    }

    /** Text for a true-north angle in the user's chosen reference. */
    fun angleText(trueDeg: Double, declinationDeg: Double, magnetic: Boolean): String {
        val shown = if (magnetic) Geodesy.trueToMagneticDeg(trueDeg, declinationDeg) else Geodesy.normalizeDeg(trueDeg)
        val whole = shown.roundToInt() % 360
        return "$whole° ${Geodesy.cardinal(shown)} ${if (magnetic) "mag" else "true"}"
    }

    /**
     * @param watch the watch's own fix, null while waiting for GPS
     * @param headingTrueDeg watch heading relative to true north, null without a heading
     * @param declinationDeg magnetic declination at the watch, east positive
     */
    fun view(
        target: Target,
        watch: LatLon?,
        headingTrueDeg: Double?,
        declinationDeg: Double,
        settings: WatchSettings,
        receivedAtMs: Long,
        nowMs: Long,
    ): CompassTarget {
        val pos: LatLon? = when {
            target.demoOffset != null -> watch?.let { Geodesy.offset(it, target.demoOffset.northM, target.demoOffset.eastM) }
            else -> LatLon(target.lat, target.lon).takeIf { it.valid() }
        }
        val distance = if (watch != null && pos != null) Geodesy.distanceM(watch, pos) else null
        val bearing = if (watch != null && pos != null) Geodesy.initialBearingDeg(watch, pos) else null
        val near = distance != null && distance < NEAR_M
        val age = currentAgeMs(target.ageMs, receivedAtMs, nowMs)
        return CompassTarget(
            id = target.id,
            label = target.label.ifBlank { target.id },
            source = target.source,
            distanceM = distance,
            distanceText = distance?.let { Format.distance(it, settings.metric) } ?: "--",
            bearingText = bearing?.let { angleText(it, declinationDeg, settings.magneticNorth) },
            arrowDeg = if (bearing != null && headingTrueDeg != null && !near) {
                Geodesy.arrowRotationDeg(bearing, headingTrueDeg)
            } else null,
            ageMs = age,
            ageText = Format.age(age),
            stale = target.stale || age > settings.compass.staleAfterS * 1000L,
            fallbackLabel = if (target.fallback) FALLBACK_LABEL else null,
            nearText = if (near) "Within ${Format.distance(distance!!, settings.metric)} — look around" else null,
        )
    }

    /** Age at send time plus the time since the snapshot arrived. */
    fun currentAgeMs(ageAtSendMs: Long, receivedAtMs: Long, nowMs: Long): Long =
        ageAtSendMs.coerceAtLeast(0) + (nowMs - receivedAtMs).coerceAtLeast(0)
}
