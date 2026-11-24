package com.ubseds.gs26.wear.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlin.math.abs

/**
 * Recovery search on the watch (docs/watch-companion.md "Recovery search").
 * The watch records its own track only while the station's search runs AND
 * the watch's user opted in on the watch AND the phone reports that the
 * station accepted that opt-in. Fixes wait in a bounded, persisted buffer
 * and go to the phone in `search_track` messages; the phone acknowledges the
 * highest contiguous sequence number it holds and the watch then drops them.
 * Same rules as WatchCore `Search.swift` and the phone's `search_relay.rs`.
 */
@Serializable
data class SearchInfo(
    val active: Boolean = false,
    @SerialName("session_id") val sessionId: String? = null,
    /** The station accepted this watch's opt-in for [sessionId]. */
    val sharing: Boolean = false,
    @SerialName("acked_seq") val ackedSeq: Long = 0,
    val percent: Double? = null,
    @SerialName("swath_m") val swathM: Double? = null,
)

@Serializable
data class SearchFix(
    val seq: Long,
    @SerialName("t_ms") val tMs: Long,
    val lat: Double,
    val lon: Double,
    @SerialName("acc_m") val accM: Double? = null,
)

object SearchLimits {
    const val MIN_MOVE_M = 5.0
    const val MAX_INTERVAL_MS = 30_000L
    const val MAX_ACCURACY_M = 100.0
    const val MAX_PENDING = 5_000
    const val MAX_MESSAGE_POINTS = 100
}

/** The watch's recorder and buffer (pure; the app persists it as JSON). */
@Serializable
data class SearchRecorder(
    /** The watch user's own choice ("Share watch track"). Never set by the phone. */
    val wantsToShare: Boolean = false,
    /** The session the buffer belongs to. */
    val sessionId: String? = null,
    val nextSeq: Long = 1,
    val pending: List<SearchFix> = emptyList(),
    val dropped: Long = 0,
    val last: SearchFix? = null,
) {
    fun recording(info: SearchInfo?): Boolean {
        if (!wantsToShare || info == null || !info.active || !info.sharing) return false
        val session = info.sessionId ?: return false
        return sessionId == null || sessionId == session
    }

    /** Follows the phone's snapshot (see the Swift twin). */
    fun apply(info: SearchInfo?): SearchRecorder {
        if (info == null) return this
        val session = info.sessionId ?: return copy(pending = emptyList(), sessionId = null, wantsToShare = false)
        var next = this
        if (sessionId != null && sessionId != session) next = next.copy(pending = emptyList(), wantsToShare = false)
        return next.copy(
            sessionId = session,
            nextSeq = maxOf(next.nextSeq, info.ackedSeq + 1),
            pending = next.pending.filter { it.seq > info.ackedSeq },
        )
    }

    /** Offers one fix; returns the recorder with it kept, or this one unchanged. */
    fun record(lat: Double, lon: Double, accuracyM: Double?, tMs: Long, info: SearchInfo?): SearchRecorder {
        if (!recording(info) || !lat.isFinite() || !lon.isFinite() || abs(lat) > 90 || abs(lon) > 180) return this
        if (abs(lat) < 1e-6 && abs(lon) < 1e-6) return this
        if (accuracyM != null && !(accuracyM >= 0 && accuracyM <= SearchLimits.MAX_ACCURACY_M)) return this
        last?.let { l ->
            val moved = Geodesy.distanceM(LatLon(l.lat, l.lon), LatLon(lat, lon)) ?: 0.0
            if (tMs <= l.tMs || (moved < SearchLimits.MIN_MOVE_M && tMs - l.tMs < SearchLimits.MAX_INTERVAL_MS)) return this
        }
        val fix = SearchFix(nextSeq, tMs, lat, lon, accuracyM)
        val all = pending + fix
        val keep = all.takeLast(SearchLimits.MAX_PENDING)
        return copy(nextSeq = nextSeq + 1, last = fix, pending = keep, dropped = dropped + (all.size - keep.size))
    }

    /** The next message for the phone, oldest first. */
    fun nextMessage(): String? {
        val session = sessionId ?: return null
        if (pending.isEmpty()) return null
        return Search.trackMessage(session, pending.take(SearchLimits.MAX_MESSAGE_POINTS))
    }

    /** The user stopped sharing on the watch: nothing more is recorded, the buffer is discarded. */
    fun stop(): SearchRecorder = copy(wantsToShare = false, pending = emptyList(), last = null)
}

object Search {
    fun shareMessage(sharing: Boolean): String = buildJsonObject {
        put("v", Protocol.VERSION); put("type", "search_share"); put("sharing", sharing)
    }.toString()

    fun trackMessage(sessionId: String, points: List<SearchFix>): String = JsonObject(
        mapOf(
            "v" to JsonPrimitive(Protocol.VERSION),
            "type" to JsonPrimitive("search_track"),
            "session_id" to JsonPrimitive(sessionId),
            "points" to Protocol.json.encodeToJsonElement(kotlinx.serialization.builtins.ListSerializer(SearchFix.serializer()), points),
        )
    ).toString()

    fun encode(recorder: SearchRecorder): String = Protocol.json.encodeToString(SearchRecorder.serializer(), recorder)
    fun decode(text: String?): SearchRecorder =
        text?.let { runCatching { Protocol.json.decodeFromString(SearchRecorder.serializer(), it) }.getOrNull() } ?: SearchRecorder()
}
