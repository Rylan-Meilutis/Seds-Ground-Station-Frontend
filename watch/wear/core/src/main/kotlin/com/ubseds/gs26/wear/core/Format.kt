package com.ubseds.gs26.wear.core

import java.util.Locale

/** Text formatting shared with the phone (`format_distance`, `format_age`) plus the T-clock. */
object Format {
    /** Metric: m below 1 km, then km (2 dp below 10 km, 1 dp above); imperial: ft then mi. */
    fun distance(meters: Double, metric: Boolean): String {
        if (!meters.isFinite() || meters < 0.0) return "--"
        return if (metric) {
            when {
                meters < 999.5 -> fmt("%.0f m", meters)
                meters < 9_995.0 -> fmt("%.2f km", meters / 1000.0)
                else -> fmt("%.1f km", meters / 1000.0)
            }
        } else {
            val feet = meters / 0.3048
            val miles = meters / 1609.344
            when {
                feet < 999.5 -> fmt("%.0f ft", feet)
                miles < 9.995 -> fmt("%.2f mi", miles)
                else -> fmt("%.1f mi", miles)
            }
        }
    }

    /** Compact age: `4s`, `2m 05s`, `1h 03m`. */
    fun age(ms: Long): String {
        val s = ms.coerceAtLeast(0) / 1000
        return when {
            s < 60 -> "${s}s"
            s < 3600 -> fmt("%dm %02ds", s / 60, s % 60)
            else -> fmt("%dh %02dm", s / 3600, (s % 3600) / 60)
        }
    }

    /**
     * Live T-clock text. `tminus` counts down from [Clock.ms] (clamped at 0),
     * `tplus` counts up, both from the receive instant. Idle (or unknown kind)
     * returns [idleText].
     */
    fun clock(clock: Clock?, idleText: String, elapsedMs: Long): String {
        val e = elapsedMs.coerceAtLeast(0)
        return when (clock?.kind) {
            "tminus" -> "T- " + mmss((clock.ms - e).coerceAtLeast(0))
            "tplus" -> "T+ " + mmss(clock.ms.coerceAtLeast(0) + e)
            else -> idleText
        }
    }

    private fun mmss(ms: Long): String {
        val s = ms / 1000
        return fmt("%02d:%02d", s / 60, s % 60)
    }

    // Locale.ROOT keeps '.' as the decimal separator on every watch locale.
    private fun fmt(pattern: String, vararg args: Any): String = String.format(Locale.ROOT, pattern, *args)
}
