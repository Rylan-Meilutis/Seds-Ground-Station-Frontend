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
     * returns [idleText]. Same text as the phone (`tclock.rs`).
     */
    fun clock(clock: Clock?, idleText: String, elapsedMs: Long): String {
        val e = elapsedMs.coerceAtLeast(0)
        return when (clock?.kind) {
            "tminus" -> tclock(true, saturatingAdd(clock.ms, -e).coerceAtLeast(0).toULong())
            "tplus" -> tclock(false, saturatingAdd(clock.ms.coerceAtLeast(0), e).toULong())
            else -> idleText
        }
    }

    /** Signed ms from T-0: negative is `T- `, zero and up `T+ `. */
    fun tclock(msFromT0: Long, fractionDigits: Int = 0): String {
        val magnitude = if (msFromT0 < 0) (-(msFromT0 + 1)).toULong() + 1uL else msFromT0.toULong()
        return tclock(msFromT0 < 0, magnitude, fractionDigits)
    }

    fun tclock(beforeT0: Boolean, magnitudeMs: ULong, fractionDigits: Int = 0): String =
        (if (beforeT0) "T- " else "T+ ") + tclockBody(magnitudeMs, fractionDigits)

    /**
     * `MM:SS` under an hour, `H:MM:SS` from one hour, `D:HH:MM:SS` from one
     * day, plus a truncated fraction of up to 3 digits. The lead field never wraps.
     */
    fun tclockBody(magnitudeMs: ULong, fractionDigits: Int = 0): String {
        val digits = fractionDigits.coerceIn(0, 3)
        val total = magnitudeMs / 1000uL
        val days = total / 86_400uL
        val hours = total / 3_600uL % 24uL
        val minutes = total / 60uL % 60uL
        val seconds = total % 60uL
        val text = StringBuilder(
            when {
                days > 0uL -> "$days:${pad2(hours)}:${pad2(minutes)}:${pad2(seconds)}"
                hours > 0uL -> "$hours:${pad2(minutes)}:${pad2(seconds)}"
                else -> "${pad2(minutes)}:${pad2(seconds)}"
            }
        )
        if (digits > 0) {
            var unit = 1uL
            repeat(3 - digits) { unit *= 10uL }
            text.append('.').append((magnitudeMs % 1000uL / unit).toString().padStart(digits, '0'))
        }
        return text.toString()
    }

    private fun pad2(v: ULong): String = v.toString().padStart(2, '0')

    private fun saturatingAdd(a: Long, b: Long): Long {
        val sum = a + b
        val overflow = ((a xor sum) and (b xor sum)) < 0
        return if (overflow) (if (b > 0) Long.MAX_VALUE else Long.MIN_VALUE) else sum
    }

    // Locale.ROOT keeps '.' as the decimal separator on every watch locale.
    private fun fmt(pattern: String, vararg args: Any): String = String.format(Locale.ROOT, pattern, *args)
}
