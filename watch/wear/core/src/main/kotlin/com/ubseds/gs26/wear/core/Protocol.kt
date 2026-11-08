package com.ubseds.gs26.wear.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/** Phone ⇄ watch wire format v1 (docs/watch-companion.md). */
object Protocol {
    const val VERSION = 1
    const val MAX_MESSAGE_BYTES = 32 * 1024

    const val SNAPSHOT_PATH = "/gs26/snapshot"
    const val WATCH_MESSAGE_PATH = "/gs26/watch"
    const val WATCH_SETTINGS_PATH = "/gs26/watch_settings"
    const val KEY_JSON = "json"
    const val KEY_SEQ = "seq"
    const val PHONE_CAPABILITY = "gs26_phone"

    val json = Json {
        ignoreUnknownKeys = true
        coerceInputValues = true
        explicitNulls = false
        encodeDefaults = true
        isLenient = false
    }

    /** Parses a snapshot; null for malformed, oversized or wrong-version input. */
    fun parseSnapshot(text: String): Snapshot? {
        if (text.length > MAX_MESSAGE_BYTES) return null
        val s = runCatching { json.decodeFromString(Snapshot.serializer(), text) }.getOrNull() ?: return null
        return if (s.v == VERSION) s else null
    }

    // The watch only ever sends these three messages.
    fun helloMessage(): String = buildJsonObject {
        put("v", VERSION); put("type", "hello")
    }.toString()

    fun activityMessage(foreground: Boolean, page: String?): String = buildJsonObject {
        put("v", VERSION); put("type", "activity"); put("foreground", foreground)
        if (page != null) put("page", page)
    }.toString()

    fun settingsMessage(settings: WatchSettings): String = JsonObject(
        mapOf(
            "v" to JsonPrimitive(VERSION),
            "type" to JsonPrimitive("settings"),
            "settings" to json.encodeToJsonElement(WatchSettings.serializer(), settings),
        )
    ).toString()
}

object Mode {
    const val LIVE = "live"
    const val DEMO = "demo"
    const val NO_STATION = "no_station"
}

object PageKind {
    const val ITEMS = "items"
    const val CYCLE = "cycle"
    const val COMPASS = "compass"
    val KNOWN = setOf(ITEMS, CYCLE, COMPASS)
}

@Serializable
data class Snapshot(
    val v: Int = 0,
    val seq: Long = 0,
    val mode: String = Mode.LIVE,
    val station: Station? = null,
    val account: Account? = null,
    val items: Map<String, ItemValue> = emptyMap(),
    val pages: List<Page> = emptyList(),
    val targets: List<Target> = emptyList(),
    val settings: WatchSettings = WatchSettings(),
    val catalog: Catalog = Catalog(),
    @SerialName("link_timeout_s") val linkTimeoutS: Long = 15,
)

@Serializable
data class Station(
    val name: String = "",
    val role: String = "unknown",
    val link: String = "offline",
    @SerialName("telemetry_age_ms") val telemetryAgeMs: Long? = null,
)

@Serializable
data class Account(
    @SerialName("signed_in") val signedIn: Boolean = false,
    val sync: String = "local_only",
)

@Serializable
data class Clock(val kind: String = "idle", val ms: Long = 0)

object Level {
    const val NORMAL = "normal"
    const val WARN = "warn"
    const val ALERT = "alert"
    const val UNKNOWN = "unknown"
}

@Serializable
data class ItemValue(
    val label: String = "",
    val text: String = "--",
    val value: Double? = null,
    val unit: String = "",
    @SerialName("age_ms") val ageMs: Long? = null,
    val stale: Boolean = false,
    val level: String = Level.UNKNOWN,
    val clock: Clock? = null,
)

@Serializable
data class DemoOffset(
    @SerialName("north_m") val northM: Double = 0.0,
    @SerialName("east_m") val eastM: Double = 0.0,
)

@Serializable
data class Target(
    val id: String,
    val label: String = "",
    val lat: Double = Double.NaN,
    val lon: Double = Double.NaN,
    @SerialName("alt_m") val altM: Double? = null,
    @SerialName("age_ms") val ageMs: Long = 0,
    val stale: Boolean = false,
    val source: String = "",
    val fallback: Boolean = false,
    @SerialName("demo_offset") val demoOffset: DemoOffset? = null,
)

@Serializable
data class Page(
    val id: String,
    val kind: String = "",
    val title: String = "",
    val items: List<String> = emptyList(),
)

@Serializable
data class CatalogItem(val id: String, val label: String = "", val group: String = "")

@Serializable
data class CatalogTarget(val id: String, val label: String = "")

@Serializable
data class Catalog(
    val items: List<CatalogItem> = emptyList(),
    val pages: List<Page> = emptyList(),
    val targets: List<CatalogTarget> = emptyList(),
)
