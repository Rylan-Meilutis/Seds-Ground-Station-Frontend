package com.ubseds.gs26.wear.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Mirror of the phone's `WatchSettings` (settings.rs). Enum-like fields are
 * strings so a newer phone's values survive a round trip. The phone sanitizes
 * every document it receives, so these helpers only keep the obvious limits.
 */
@Serializable
data class WatchSettings(
    val version: Int = 1,
    @SerialName("updated_ms") val updatedMs: Long = 0,
    @SerialName("updated_by") val updatedBy: String = "phone",
    val pages: List<PageSetting> = DEFAULT_PAGES.map { PageSetting(it, true) },
    val cycle: CycleSetting = CycleSetting(),
    val compass: CompassSetting = CompassSetting(),
    val units: String = UNITS_METRIC,
    val power: String = POWER_BALANCED,
) {
    val metric: Boolean get() = units != UNITS_IMPERIAL
    val magneticNorth: Boolean get() = compass.north == NORTH_MAGNETIC

    fun togglePage(id: String) = copy(pages = pages.map { if (it.id == id) it.copy(enabled = !it.enabled) else it })

    fun movePage(id: String, up: Boolean) = copy(pages = moveIn(pages, up) { it.id == id })

    /** Adds or removes a cycle item; never empties the list, never exceeds [MAX_CYCLE_ITEMS]. */
    fun toggleCycleItem(id: String, known: (String) -> Boolean = { true }): WatchSettings {
        val items = cycle.items
        val next = when {
            id in items -> if (items.size > 1) items - id else items
            items.size < MAX_CYCLE_ITEMS && known(id) -> items + id
            else -> items
        }
        return copy(cycle = cycle.copy(items = next))
    }

    fun moveCycleItem(id: String, up: Boolean) = copy(cycle = cycle.copy(items = moveIn(cycle.items, up) { it == id }))

    fun withInterval(seconds: Int) = copy(cycle = cycle.copy(intervalS = seconds.coerceIn(MIN_INTERVAL_S, MAX_INTERVAL_S)))

    fun withStaleAfter(seconds: Int) =
        copy(compass = compass.copy(staleAfterS = seconds.coerceIn(MIN_STALE_S, MAX_STALE_S)))

    fun toggleTarget(id: String, known: (String) -> Boolean = { true }): WatchSettings {
        val t = compass.targets
        val next = when {
            id in t -> if (t.size > 1) t - id else t
            known(id) -> t + id
            else -> t
        }
        return copy(compass = compass.copy(targets = next))
    }

    fun withNorth(north: String) = copy(compass = compass.copy(north = north))
    fun withUnits(units: String) = copy(units = units)
    fun withPower(power: String) = copy(power = power)

    /** Stamps a watch edit; time never goes backwards relative to [previousMs]. */
    fun stamped(previousMs: Long, nowMs: Long): WatchSettings {
        val floor = if (previousMs == Long.MAX_VALUE) previousMs else previousMs + 1
        return copy(updatedMs = maxOf(nowMs, floor), updatedBy = EDITOR_WATCH)
    }

    companion object {
        const val MIN_INTERVAL_S = 2
        const val MAX_INTERVAL_S = 60
        const val MIN_STALE_S = 5
        const val MAX_STALE_S = 600
        const val MAX_CYCLE_ITEMS = 12

        const val NORTH_TRUE = "true"
        const val NORTH_MAGNETIC = "magnetic"
        const val UNITS_METRIC = "metric"
        const val UNITS_IMPERIAL = "imperial"
        const val POWER_SAVER = "saver"
        const val POWER_BALANCED = "balanced"
        const val POWER_RECOVERY = "recovery"
        const val EDITOR_WATCH = "watch"

        val DEFAULT_PAGES = listOf("flight", "propulsion", "trajectory", "health", "cycle", "compass")
        val DEFAULT_TARGETS = listOf("rocket", "ground_station", "rrls")
        val DEFAULT_CYCLE = listOf("flight_state", "t_clock", "altitude", "velocity", "tank_pressure")
    }
}

@Serializable
data class PageSetting(val id: String, val enabled: Boolean = true)

@Serializable
data class CycleSetting(
    val items: List<String> = WatchSettings.DEFAULT_CYCLE,
    @SerialName("interval_s") val intervalS: Int = 5,
    val autostart: Boolean = true,
)

@Serializable
data class CompassSetting(
    val north: String = WatchSettings.NORTH_TRUE,
    @SerialName("stale_after_s") val staleAfterS: Int = 30,
    val targets: List<String> = WatchSettings.DEFAULT_TARGETS,
)

private fun <T> moveIn(list: List<T>, up: Boolean, pred: (T) -> Boolean): List<T> {
    val i = list.indexOfFirst(pred)
    val j = if (up) i - 1 else i + 1
    if (i < 0 || j !in list.indices) return list
    return list.toMutableList().apply { this[i] = list[j]; this[j] = list[i] }
}
