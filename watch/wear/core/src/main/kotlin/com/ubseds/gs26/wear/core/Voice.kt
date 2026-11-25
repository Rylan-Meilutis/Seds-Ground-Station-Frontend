package com.ubseds.gs26.wear.core

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import java.util.Base64
import kotlin.math.floor
import kotlin.math.min

/**
 * Crew voice on the watch (docs/watch-companion.md "Voice"): the station's
 * `/api/voice/ws` protocol, push-to-talk, reconnect, transport choice,
 * playout mixing and the phone-relay envelope. Checked against the `voice`
 * vectors in watch/test-vectors.json (VoiceCore.swift checks the same ones).
 */
object Voice {
    const val SAMPLE_RATE = 16_000
    const val FRAME_SAMPLES = 320
    const val FRAME_BYTES = FRAME_SAMPLES * 2
    const val FRAME_MS = 20L
    const val MAX_OPUS_BYTES = 400
    /** Any talk stops after this long, so a stuck button cannot hold the channel. */
    const val MAX_TALK_MS = 60_000L
    const val PING_MS = 10_000L
    const val WS_PATH = "/api/voice/ws"
    /** Speaker id of audio from the RRLS recovery radio. */
    const val RADIO_SPEAKER = 0xffff_fffeL
    const val CODEC_OPUS = "opus"
    const val CODEC_PCM16 = "pcm16"

    /** `http(s)://host[:port]` → `ws(s)://host[:port]/api/voice/ws`. */
    fun socketUrl(base: String): String {
        val b = base.trimEnd('/')
        return when {
            b.startsWith("https://") -> "wss://" + b.removePrefix("https://")
            b.startsWith("http://") -> "ws://" + b.removePrefix("http://")
            b.startsWith("ws://") || b.startsWith("wss://") -> b
            else -> "ws://$b"
        } + WS_PATH
    }

    fun speakerLabel(id: Long, peers: List<RosterPeer>): String =
        if (id == RADIO_SPEAKER) "Recovery radio" else peers.firstOrNull { it.id == id }?.name ?: "Crew"
}

// --- Push-to-talk -----------------------------------------------------------

enum class Haptic { START, STOP, DENIED }

/** Result of one PTT event: whether we talk now, the `transmitting` control to send (null: none), feedback. */
data class PttOutput(val talking: Boolean, val send: Boolean?, val haptic: Haptic?)

/**
 * Press-and-hold or tap-to-toggle (`voice.ptt` vectors). The app feeds
 * touch `down`/`up`, connection `ready`/`disconnect`, handoff `held`/
 * `unheld`, `timeout` (talk longer than [Voice.MAX_TALK_MS]) and `leave`.
 * Reconnecting never resumes talking.
 */
class PttMachine(var toggle: Boolean) {
    var ready = false; private set
    var held = false; private set
    var talking = false; private set
    var talkStartedMs = 0L; private set

    fun event(name: String, nowMs: Long = 0): PttOutput = when (name) {
        "ready" -> { ready = true; out(null, null) }
        "down" -> down(nowMs)
        "up" -> if (!toggle && talking) stop(true) else out(null, null)
        "timeout" -> if (talking) stop(true) else out(null, null)
        "disconnect" -> { ready = false; if (talking) stop(false) else out(null, null) }
        "leave" -> { val o = if (talking) stop(ready) else out(null, null); ready = false; o }
        "held" -> { held = true; if (talking) stop(true) else out(null, null) }
        "unheld" -> { held = false; out(null, null) }
        else -> out(null, null)
    }

    /** True when the current talk has run past [Voice.MAX_TALK_MS]. */
    fun timedOut(nowMs: Long) = talking && nowMs - talkStartedMs >= Voice.MAX_TALK_MS

    private fun down(nowMs: Long): PttOutput {
        if (!ready || held) return out(null, Haptic.DENIED)
        if (talking) return if (toggle) stop(true) else out(null, null)
        talking = true
        talkStartedMs = nowMs
        return out(true, Haptic.START)
    }

    private fun stop(send: Boolean): PttOutput {
        talking = false
        return out(if (send) false else null, Haptic.STOP)
    }

    private fun out(send: Boolean?, haptic: Haptic?) = PttOutput(talking, send, haptic)
}

// --- Reconnect and transport ------------------------------------------------

/** `floor(min(1 s × 2^(attempt−1), 30 s) × (0.75 + 0.5 × jitter))`, attempt from 1. */
fun reconnectDelayMs(attempt: Int, jitter: Double): Long {
    val exp = (attempt - 1).coerceIn(0, 30)
    val raw = min(1000.0 * (1L shl exp).toDouble(), 30_000.0)
    return floor(raw * (0.75 + 0.5 * jitter.coerceIn(0.0, 1.0))).toLong()
}

/** A dropped connection (null) and a full channel are retried; refusals are not. */
fun voiceRetryable(serverError: String?): Boolean =
    serverError == null || serverError.contains("full", ignoreCase = true)

enum class VoiceTransport { DIRECT, PHONE, NONE }

/** Direct to the station first; the phone relays after two direct failures in a row. */
fun voiceTransport(granted: Boolean, directFailures: Int, phoneReachable: Boolean): VoiceTransport = when {
    !granted -> VoiceTransport.NONE
    directFailures < 2 -> VoiceTransport.DIRECT
    phoneReachable -> VoiceTransport.PHONE
    else -> VoiceTransport.DIRECT
}

// --- Framing ----------------------------------------------------------------

object VoiceFrames {
    /** Speaker id of a station frame (LE u32), or null without a payload. */
    fun speaker(frame: ByteArray): Long? {
        if (frame.size < 5) return null
        return (frame[0].toLong() and 0xff) or ((frame[1].toLong() and 0xff) shl 8) or
            ((frame[2].toLong() and 0xff) shl 16) or ((frame[3].toLong() and 0xff) shl 24)
    }

    fun payload(frame: ByteArray): ByteArray = frame.copyOfRange(4.coerceAtMost(frame.size), frame.size)

    fun payloadValid(codec: String, len: Int): Boolean = when (codec) {
        Voice.CODEC_PCM16 -> len == Voice.FRAME_BYTES
        Voice.CODEC_OPUS -> len in 1..Voice.MAX_OPUS_BYTES
        else -> false
    }

    fun downlinkValid(frame: ByteArray, codec: String): Boolean =
        speaker(frame) != null && payloadValid(codec, frame.size - 4)

    fun uplinkValid(codec: String, len: Int): Boolean = payloadValid(codec, len)

    fun encodePcm(samples: ShortArray): ByteArray {
        val out = ByteArray(samples.size * 2)
        for (i in samples.indices) {
            val v = samples[i].toInt()
            out[2 * i] = (v and 0xff).toByte()
            out[2 * i + 1] = ((v shr 8) and 0xff).toByte()
        }
        return out
    }

    fun decodePcm(bytes: ByteArray, offset: Int = 0, length: Int = bytes.size - offset): ShortArray {
        val n = length / 2
        return ShortArray(n) { i ->
            val lo = bytes[offset + 2 * i].toInt() and 0xff
            val hi = bytes[offset + 2 * i + 1].toInt()
            ((hi shl 8) or lo).toShort()
        }
    }

    fun authMessage(token: String, codec: String): String = buildJsonObject {
        put("type", "auth"); put("token", token); put("codec", codec)
    }.toString()

    /** `transmitting`, `active` or `radio`. */
    fun control(type: String, enabled: Boolean): String = buildJsonObject {
        put("type", type); put("enabled", enabled)
    }.toString()

    fun ping(): String = buildJsonObject { put("type", "ping") }.toString()
}

// --- Station messages -------------------------------------------------------

data class RosterPeer(val id: Long, val name: String, val transmitting: Boolean, val device: String)

sealed class ServerMessage {
    data class Joined(val id: Long, val codec: String) : ServerMessage()
    data class Roster(val peers: List<RosterPeer>) : ServerMessage()
    data class Handoff(val held: Boolean, val by: String) : ServerMessage()
    /** Talk floor: `on_air`, `waiting` (the station has its cap of talkers) or `idle`. */
    data class Floor(val state: String, val reason: String?) : ServerMessage()
    data class Error(val message: String) : ServerMessage()
    data class RadioLink(val configured: Boolean, val available: Boolean) : ServerMessage()
    data class RadioState(val enabled: Boolean) : ServerMessage()
    data class BroadcastState(val enabled: Boolean) : ServerMessage()
    data object Unknown : ServerMessage()

    companion object {
        fun parse(text: String): ServerMessage = runCatching { parse(Protocol.json.parseToJsonElement(text).jsonObject) }
            .getOrDefault(Unknown)

        fun parse(o: JsonObject): ServerMessage {
            fun str(k: String, from: JsonObject = o) = from[k]?.jsonPrimitive?.contentOrNull
            fun bool(k: String, from: JsonObject = o) = from[k]?.jsonPrimitive?.booleanOrNull ?: false
            return when (str("type")) {
                "joined" -> Joined(o["id"]?.jsonPrimitive?.longOrNull ?: 0, str("codec") ?: Voice.CODEC_PCM16)
                "roster" -> Roster(parsePeers(o["peers"]))
                "handoff" -> Handoff(bool("held"), str("by").orEmpty())
                "floor" -> Floor(str("state") ?: "idle", str("reason"))
                "error" -> Error(str("message").orEmpty())
                "radio_link" -> {
                    val r = o["radio"] as? JsonObject ?: JsonObject(emptyMap())
                    RadioLink(bool("configured", r), bool("available", r))
                }
                "radio_state" -> RadioState(bool("enabled"))
                "broadcast_state" -> BroadcastState(bool("enabled"))
                else -> Unknown
            }
        }

        fun parsePeers(element: kotlinx.serialization.json.JsonElement?): List<RosterPeer> =
            (element as? JsonArray).orEmpty().mapNotNull { e ->
                val p = e as? JsonObject ?: return@mapNotNull null
                RosterPeer(
                    id = p["id"]?.jsonPrimitive?.longOrNull ?: return@mapNotNull null,
                    name = p["name"]?.jsonPrimitive?.contentOrNull.orEmpty(),
                    transmitting = p["transmitting"]?.jsonPrimitive?.booleanOrNull ?: false,
                    device = p["device"]?.jsonPrimitive?.contentOrNull ?: "web",
                )
            }
    }
}

/** Names of the others talking now (`voice.talkers`). */
fun talkers(peers: List<RosterPeer>, self: Long?): List<String> =
    peers.filter { it.transmitting && it.id != self }.map { it.name }

/** Ids of others that started talking between two rosters (the talker haptic). */
fun startedTalking(prev: List<RosterPeer>, next: List<RosterPeer>, self: Long?): List<Long> {
    val before = prev.filter { it.transmitting }.map { it.id }.toSet()
    return next.filter { it.transmitting && it.id != self && it.id !in before }.map { it.id }
}

/** Crown / rotary: 5 % per step, 0–150 % (`voice.volume`). */
fun volumeAfterCrown(pct: Int, steps: Int): Int = (pct + 5 * steps).coerceIn(0, VoiceSetting.MAX_VOLUME)

// --- Playout ----------------------------------------------------------------

/**
 * Per-speaker playout buffer (`voice.mix`): a speaker starts playing once it
 * has [prebuffer] frames, keeps at most [maxDepth] (oldest dropped) and goes
 * back to buffering after an underrun. [pull] mixes one frame: integer sum
 * × volume / 100 truncated toward zero, clipped to 16 bits.
 */
class VoiceMixer(val frameSamples: Int = Voice.FRAME_SAMPLES, val prebuffer: Int = 2, val maxDepth: Int = 10) {
    private class Stream { val queue = ArrayDeque<ShortArray>(); var playing = false }
    private val streams = LinkedHashMap<Long, Stream>()

    @Synchronized fun push(speaker: Long, samples: ShortArray) {
        val s = streams.getOrPut(speaker) { Stream() }
        while (s.queue.size >= maxDepth) s.queue.removeFirst()
        s.queue.addLast(if (samples.size == frameSamples) samples else samples.copyOf(frameSamples))
    }

    @Synchronized fun remove(speaker: Long) { streams.remove(speaker) }

    @Synchronized fun clear() { streams.clear() }

    @Synchronized fun active(): Boolean = streams.values.any { it.queue.isNotEmpty() || it.playing }

    @Synchronized fun pull(volumePct: Int): ShortArray {
        val sum = LongArray(frameSamples)
        for (s in streams.values) {
            if (!s.playing && s.queue.size >= prebuffer) s.playing = true
            if (!s.playing) continue
            val frame = s.queue.removeFirstOrNull()
            if (frame == null) { s.playing = false; continue }
            for (i in 0 until frameSamples) sum[i] += frame[i].toLong()
        }
        return ShortArray(frameSamples) { i ->
            (sum[i] * volumePct / 100).coerceIn(Short.MIN_VALUE.toLong(), Short.MAX_VALUE.toLong()).toShort()
        }
    }
}

// --- Phone relay envelope ---------------------------------------------------

/** What the phone relay sent the watch. */
sealed class RelayIn {
    data class Server(val texts: List<String>, val frames: List<ByteArray>) : RelayIn()
    data class Closed(val reason: String) : RelayIn()
}

/** The `{"v":1,"type":"voice",…}` messages of the phone relay (docs "Phone relay"). */
object VoiceRelay {
    const val MAX_FRAMES = 10
    private val b64e = Base64.getEncoder()
    private val b64d = Base64.getDecoder()
    private val controls = setOf("transmitting", "active", "radio", "ping")

    private fun envelope(op: String, body: MutableMap<String, kotlinx.serialization.json.JsonElement>.() -> Unit) =
        JsonObject(linkedMapOf<String, kotlinx.serialization.json.JsonElement>(
            "v" to JsonPrimitive(Protocol.VERSION), "type" to JsonPrimitive("voice"), "op" to JsonPrimitive(op),
        ).apply(body)).toString()

    fun open(codec: String) = envelope("open") { put("codec", JsonPrimitive(codec)) }
    fun close() = envelope("close") {}

    /** A station control message; null for anything the phone would not forward. */
    fun control(msg: String): String? {
        val o = runCatching { Protocol.json.parseToJsonElement(msg).jsonObject }.getOrNull() ?: return null
        if (o["type"]?.jsonPrimitive?.contentOrNull !in controls) return null
        return envelope("control") { put("msg", o) }
    }

    /** Up to [MAX_FRAMES] uplink frames per message. */
    fun audio(frames: List<ByteArray>): List<String> = frames.chunked(MAX_FRAMES).map { chunk ->
        envelope("audio") { put("frames", JsonArray(chunk.map { JsonPrimitive(b64e.encodeToString(it)) })) }
    }

    /** Parses a phone → watch relay message; null when it is not one. */
    fun parse(text: String): RelayIn? = runCatching {
        if (text.length > Protocol.MAX_MESSAGE_BYTES) return null
        val o = Protocol.json.parseToJsonElement(text).jsonObject
        if (o["v"]?.jsonPrimitive?.longOrNull != Protocol.VERSION.toLong()) return null
        if (o["type"]?.jsonPrimitive?.contentOrNull != "voice") return null
        when (o["op"]?.jsonPrimitive?.contentOrNull) {
            "server" -> RelayIn.Server(
                texts = o["text"]?.jsonArray.orEmpty().mapNotNull { it.jsonPrimitive.contentOrNull },
                frames = o["frames"]?.jsonArray.orEmpty().mapNotNull { e ->
                    e.jsonPrimitive.contentOrNull?.let { runCatching { b64d.decode(it) }.getOrNull() }
                },
            )
            "closed" -> RelayIn.Closed(o["reason"]?.jsonPrimitive?.contentOrNull.orEmpty())
            else -> null
        }
    }.getOrNull()
}
