package com.ubseds.gs26.wear.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.double
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.longOrNull
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/** The `voice` vectors (VoiceTests.swift checks the same ones). */
class VoiceVectorsTest {
    private val voice: JsonObject by lazy {
        Json.parseToJsonElement(vectorFile().readText()).jsonObject["voice"]!!.jsonObject
    }

    private fun vectorFile(): File {
        System.getProperty("gs26.testVectors")?.let { File(it) }?.takeIf { it.isFile }?.let { return it }
        var dir: File? = File("").absoluteFile
        while (dir != null) {
            listOf(File(dir, "test-vectors.json"), File(dir, "watch/test-vectors.json"))
                .firstOrNull { it.isFile }?.let { return it }
            dir = dir.parentFile
        }
        error("watch/test-vectors.json not found")
    }

    private fun JsonObject.str(key: String) = this[key]?.takeIf { it != JsonNull }?.jsonPrimitive?.content
    private fun hex(s: String) = ByteArray(s.length / 2) { s.substring(2 * it, 2 * it + 2).toInt(16).toByte() }
    private fun peers(a: JsonArray) = ServerMessage.parsePeers(a)

    @Test
    fun pttVectors() {
        assertEquals(Voice.MAX_TALK_MS, voice["max_talk_ms"]!!.jsonPrimitive.long)
        for (case in voice["ptt"]!!.jsonArray.map { it.jsonObject }) {
            val name = case.str("name")
            val m = PttMachine(toggle = case.str("mode") == "toggle")
            for ((i, step) in case["steps"]!!.jsonArray.map { it.jsonObject }.withIndex()) {
                val out = m.event(step.str("event")!!, 1_000L * i)
                val where = "$name step $i ${step.str("event")}"
                assertEquals(where, step["talking"]!!.jsonPrimitive.boolean, out.talking)
                assertEquals(where, step["send"]!!.takeIf { it != JsonNull }?.jsonPrimitive?.booleanOrNull, out.send)
                assertEquals(where, step.str("haptic"), out.haptic?.name?.lowercase())
            }
        }
    }

    @Test
    fun talkTimesOutAfterMaxTalk() {
        val m = PttMachine(toggle = true)
        m.event("ready"); m.event("down", 5_000)
        assertFalse(m.timedOut(5_000 + Voice.MAX_TALK_MS - 1))
        assertTrue(m.timedOut(5_000 + Voice.MAX_TALK_MS))
    }

    @Test
    fun backoffAndTransportVectors() {
        val b = voice["backoff"]!!.jsonObject
        for (d in b["delays"]!!.jsonArray.map { it.jsonObject }) {
            assertEquals("attempt ${d["attempt"]}", d["ms"]!!.jsonPrimitive.long,
                reconnectDelayMs(d["attempt"]!!.jsonPrimitive.int, d["jitter"]!!.jsonPrimitive.double))
        }
        for (r in b["retryable"]!!.jsonArray.map { it.jsonObject }) {
            assertEquals(r.str("error"), r["retry"]!!.jsonPrimitive.boolean, voiceRetryable(r.str("error")))
        }
        for (t in voice["transport"]!!.jsonArray.map { it.jsonObject }) {
            assertEquals(t.str("name"), t.str("transport"), voiceTransport(
                t["granted"]!!.jsonPrimitive.boolean, t["direct_failures"]!!.jsonPrimitive.int,
                t["phone_reachable"]!!.jsonPrimitive.boolean,
            ).name.lowercase())
        }
    }

    @Test
    fun frameVectors() {
        val f = voice["frames"]!!.jsonObject
        for (d in f["downlink"]!!.jsonArray.map { it.jsonObject }) {
            val bytes = hex(d.str("hex")!!)
            val name = d.str("name")
            assertEquals(name, d["speaker"]!!.takeIf { it != JsonNull }?.jsonPrimitive?.longOrNull, VoiceFrames.speaker(bytes))
            assertEquals(name, d["valid"]!!.jsonPrimitive.boolean, VoiceFrames.downlinkValid(bytes, d.str("codec")!!))
            if (VoiceFrames.speaker(bytes) != null) {
                assertEquals(name, d["payload_len"]!!.jsonPrimitive.int, VoiceFrames.payload(bytes).size)
            }
        }
        for (p in f["pcm16"]!!.jsonArray.map { it.jsonObject }) {
            val samples = p["samples"]!!.jsonArray.map { it.jsonPrimitive.int.toShort() }.toShortArray()
            val bytes = hex(p.str("hex")!!)
            assertArrayEquals(bytes, VoiceFrames.encodePcm(samples))
            assertArrayEquals(samples, VoiceFrames.decodePcm(bytes))
        }
        for (u in f["uplink_valid"]!!.jsonArray.map { it.jsonObject }) {
            assertEquals(u.toString(), u["valid"]!!.jsonPrimitive.boolean,
                VoiceFrames.uplinkValid(u.str("codec")!!, u["len"]!!.jsonPrimitive.int))
        }
        val auth = f["auth"]!!.jsonObject
        assertEquals(auth["json"], Json.parseToJsonElement(VoiceFrames.authMessage(auth.str("token")!!, auth.str("codec")!!)))
        for (s in f["server"]!!.jsonArray.map { it.jsonObject }) {
            val msg = ServerMessage.parse(s["json"]!!.toString())
            when (s.str("kind")) {
                "joined" -> {
                    msg as ServerMessage.Joined
                    assertEquals(s["id"]!!.jsonPrimitive.long, msg.id)
                    assertEquals(s.str("codec"), msg.codec)
                }
                "handoff" -> assertEquals(s["held"]!!.jsonPrimitive.boolean, (msg as ServerMessage.Handoff).held)
                "error" -> assertEquals(s.str("message"), (msg as ServerMessage.Error).message)
                "radio_link" -> assertEquals(s["radio_available"]!!.jsonPrimitive.boolean, (msg as ServerMessage.RadioLink).available)
                "unknown" -> assertEquals(ServerMessage.Unknown, msg)
                else -> error("unhandled kind ${s.str("kind")}")
            }
        }
        assertEquals(ServerMessage.Unknown, ServerMessage.parse("not json"))
    }

    @Test
    fun talkerVectors() {
        for (c in voice["talkers"]!!.jsonArray.map { it.jsonObject }) {
            val self = c["self"]!!.jsonPrimitive.long
            val prev = peers(c["prev"]!!.jsonArray)
            val next = peers(c["next"]!!.jsonArray)
            assertEquals(c.str("name"), c["talkers"]!!.jsonArray.map { it.jsonPrimitive.content }, talkers(next, self))
            assertEquals(c.str("name"), c["started"]!!.jsonArray.map { it.jsonPrimitive.long }, startedTalking(prev, next, self))
        }
    }

    @Test
    fun mixAndVolumeVectors() {
        val mix = voice["mix"]!!.jsonObject
        for (c in mix["cases"]!!.jsonArray.map { it.jsonObject }) {
            val m = VoiceMixer(mix["frame_samples"]!!.jsonPrimitive.int, mix["prebuffer"]!!.jsonPrimitive.int, mix["max_depth"]!!.jsonPrimitive.int)
            val pct = c["volume_pct"]!!.jsonPrimitive.int
            for ((i, step) in c["steps"]!!.jsonArray.map { it.jsonObject }.withIndex()) {
                step["push"]?.jsonArray?.let { p ->
                    m.push(p[0].jsonPrimitive.long, p[1].jsonArray.map { it.jsonPrimitive.int.toShort() }.toShortArray())
                }
                step["remove"]?.let { m.remove(it.jsonPrimitive.long) }
                step["pull"]?.jsonArray?.let { p ->
                    assertArrayEquals("${c.str("name")} step $i", p.map { it.jsonPrimitive.int.toShort() }.toShortArray(), m.pull(pct))
                }
            }
        }
        for (v in voice["volume"]!!.jsonArray.map { it.jsonObject }) {
            assertEquals(v["result"]!!.jsonPrimitive.int, volumeAfterCrown(v["pct"]!!.jsonPrimitive.int, v["steps"]!!.jsonPrimitive.int))
        }
    }

    @Test
    fun relayEnvelopesRoundTripAndFilterControls() {
        val open = Json.parseToJsonElement(VoiceRelay.open("opus")).jsonObject
        assertEquals("voice", open.str("type")); assertEquals("open", open.str("op")); assertEquals("opus", open.str("codec"))
        assertNull(VoiceRelay.control("""{"type":"auth","token":"x"}"""))
        assertNull(VoiceRelay.control("garbage"))
        val ctl = Json.parseToJsonElement(VoiceRelay.control(VoiceFrames.control("transmitting", true))!!).jsonObject
        assertEquals(true, ctl["msg"]!!.jsonObject["enabled"]!!.jsonPrimitive.boolean)
        val frames = (0 until 23).map { byteArrayOf(it.toByte(), 1, 2) }
        val audio = VoiceRelay.audio(frames)
        assertEquals(3, audio.size)
        assertEquals(10, Json.parseToJsonElement(audio[0]).jsonObject["frames"]!!.jsonArray.size)
        val b64 = java.util.Base64.getEncoder().encodeToString(byteArrayOf(5, 0, 0, 0, 9))
        val server = VoiceRelay.parse("""{"v":1,"type":"voice","op":"server","text":["{\"type\":\"ping\"}"],"frames":["$b64"]}""") as RelayIn.Server
        assertEquals(listOf("""{"type":"ping"}"""), server.texts)
        assertEquals(5L, VoiceFrames.speaker(server.frames[0]))
        assertEquals(RelayIn.Closed("bye"), VoiceRelay.parse("""{"v":1,"type":"voice","op":"closed","reason":"bye"}"""))
        assertNull(VoiceRelay.parse("""{"v":2,"type":"voice","op":"closed"}"""))
        assertNull(VoiceRelay.parse("""{"v":1,"type":"hello"}"""))
    }

    @Test
    fun settingsAndProvisionCarryVoice() {
        val s = Protocol.json.decodeFromString(WatchSettings.serializer(),
            """{"voice":{"channel":"crew_radio","ptt":"toggle","talker_haptics":true,"route":"speaker","volume":120}}""")
        assertTrue(s.voice.radio && s.voice.toggle && s.voice.talkerHaptics)
        assertEquals("speaker", s.voice.route)
        assertEquals(VoiceSetting(), WatchSettings().voice)
        assertEquals(150, s.withVoice(s.voice.copy(volume = 999)).voice.volume)
        assertTrue("voice" in WatchSettings().pages.map { it.id })
        val p = DirectProvision.parse("""{"v":1,"type":"direct","enabled":true,"addresses":["https://gs:3000"],"token":"gsw1.x","voice":true}""")!!
        assertTrue(p.voice)
        assertFalse(DirectProvision.parse("""{"v":1,"type":"direct","enabled":true}""")!!.voice)
        assertEquals("wss://gs:3000/api/voice/ws", Voice.socketUrl(p.base))
        assertEquals("ws://10.0.0.2:3000/api/voice/ws", Voice.socketUrl("http://10.0.0.2:3000/"))
        assertEquals("Recovery radio", Voice.speakerLabel(Voice.RADIO_SPEAKER, emptyList()))
    }
}
