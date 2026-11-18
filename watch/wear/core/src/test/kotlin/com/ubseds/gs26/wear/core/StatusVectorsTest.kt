package com.ubseds.gs26.wear.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/** connection_status, auto_cycle and alerts vectors (StatusTests.swift checks the same ones). */
class StatusVectorsTest {
    private val vectors: JsonObject by lazy { Json.parseToJsonElement(vectorFile().readText()).jsonObject }

    private fun vectorFile(): File {
        System.getProperty("gs26.testVectors")?.let { File(it) }?.takeIf { it.isFile }?.let { return it }
        var dir: File? = File("").absoluteFile
        while (dir != null) {
            listOf(File(dir, "test-vectors.json"), File(dir, "watch/test-vectors.json"))
                .firstOrNull { it.isFile }?.let { return it }
            dir = dir.parentFile
        }
        error("watch/test-vectors.json not found from ${File("").absolutePath}")
    }

    private fun line(o: JsonObject?): StatusLine? = o?.let {
        StatusLine(it["text"]!!.jsonPrimitive.content, StatusTone.valueOf(it["tone"]!!.jsonPrimitive.content.uppercase()))
    }

    @Test
    fun connectionStatusVectors() {
        val cases = vectors["connection_status"]!!.jsonArray.map { it.jsonObject }
        assertTrue(cases.size >= 10)
        for (c in cases) {
            val name = c["name"]!!.jsonPrimitive.content
            val snap = c["snapshot"]!!.takeIf { it != JsonNull }
                ?.let { Protocol.json.decodeFromJsonElement(Snapshot.serializer(), it) }
            val route = c["route"]?.jsonPrimitive?.content?.uppercase()?.let { LinkRoute.valueOf(it) } ?: LinkRoute.PHONE
            val direct = DirectInfo(
                configured = c["direct_configured"]?.jsonPrimitive?.boolean ?: false,
                polling = c["direct_polling"]?.jsonPrimitive?.boolean ?: false,
                error = c["direct_error"]?.takeIf { it != JsonNull }?.jsonPrimitive?.content,
            )
            val status = ConnectionStatus.of(snap, c["age_ms"]!!.jsonPrimitive.long, c["reachable"]!!.jsonPrimitive.boolean, route, direct)
            assertEquals(name, line(c["direct"]?.takeIf { it != JsonNull }?.jsonObject), status.direct)
            assertEquals(name, line(c["phone"]!!.jsonObject), status.phone)
            assertEquals(name, line(c["station"]!!.takeIf { it != JsonNull }?.jsonObject), status.station)
            assertEquals(name, c["open_phone_app"]!!.jsonPrimitive.boolean, status.openPhoneApp)
        }
    }

    @Test
    fun autoCycleVectors() {
        for (c in vectors["auto_cycle"]!!.jsonArray.map { it.jsonObject }) {
            val name = c["name"]!!.jsonPrimitive.content
            val interval = c["interval_s"]!!.jsonPrimitive.int
            val count = c["count"]!!.jsonPrimitive.int
            val cycle = AutoCycle(0, c["autostart"]!!.jsonPrimitive.boolean)
            for (step in c["steps"]!!.jsonArray.map { it.jsonObject }) {
                val at = step["at"]!!.jsonPrimitive.long
                when (val op = step["op"]!!.jsonPrimitive.content) {
                    "tick" -> cycle.tick(at, interval, count)
                    "scroll" -> cycle.observe(step["to"]!!.jsonPrimitive.int, at)
                    "tap" -> cycle.togglePause(at)
                    else -> error("unknown op $op")
                }
                assertEquals("$name @$at", step["index"]!!.jsonPrimitive.int, cycle.index)
                assertEquals("$name @$at", step["footer"]!!.jsonPrimitive.content, cycle.footer(at, interval, count))
            }
        }
    }

    @Test
    fun alertVectors() {
        for (c in vectors["alerts"]!!.jsonArray.map { it.jsonObject }) {
            val snap = Protocol.json.decodeFromString(Snapshot.serializer(), """{"items":${c["items"]}}""")
            val expect = c["expect"]!!.jsonArray.map { row -> row.jsonArray.map { it.jsonPrimitive.content } }
            assertEquals(c["name"]!!.jsonPrimitive.content, expect, alertRows(snap).map { listOf(it.label, it.text, it.level) })
        }
        assertTrue(alertRows(null).isEmpty())
    }
}
