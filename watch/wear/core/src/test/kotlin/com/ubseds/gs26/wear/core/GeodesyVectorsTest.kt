package com.ubseds.gs26.wear.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.double
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File
import kotlin.math.abs

/** Mirrors the Rust `shared_test_vectors_match` test, same tolerances. */
class GeodesyVectorsTest {
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

    private fun close(a: Double, b: Double, tol: Double) = abs(a - b) <= tol

    @Test
    fun geodesyVectors() {
        val cases = vectors["geodesy"]!!.jsonArray
        assertTrue(cases.size >= 10)
        for (c in cases.map { it.jsonObject }) {
            val name = c["name"]!!.jsonPrimitive.content
            val f = c["from"]!!.jsonArray
            val t = c["to"]!!.jsonArray
            val a = LatLon(f[0].jsonPrimitive.double, f[1].jsonPrimitive.double)
            val b = LatLon(t[0].jsonPrimitive.double, t[1].jsonPrimitive.double)
            val d = Geodesy.distanceM(a, b)!!
            val want = c["distance_m"]!!.jsonPrimitive.double
            val tol = c["distance_tol_m"]!!.jsonPrimitive.double
            assertTrue("$name: $d vs $want", close(d, want, tol))
            when (val brg = c["bearing_deg"]) {
                is JsonNull, null -> assertNull(name, Geodesy.initialBearingDeg(a, b))
                is JsonPrimitive -> {
                    if (brg.isString && brg.content == "any") continue
                    val got = Geodesy.initialBearingDeg(a, b)
                    assertNotNull(name, got)
                    assertTrue("$name: $got vs ${brg.double}", abs(Geodesy.signedDeltaDeg(got!!, brg.double)) <= 0.01)
                }
                else -> error("bad bearing in $name")
            }
        }
    }

    @Test
    fun distanceFormatVectors() {
        for (c in vectors["distance_format"]!!.jsonArray.map { it.jsonObject }) {
            val m = c["m"]!!.jsonPrimitive.double
            assertEquals("metric $m", c["metric"]!!.jsonPrimitive.content, Format.distance(m, true))
            assertEquals("imperial $m", c["imperial"]!!.jsonPrimitive.content, Format.distance(m, false))
        }
    }

    @Test
    fun ageFormatVectors() {
        for (c in vectors["age_format"]!!.jsonArray.map { it.jsonObject }) {
            assertEquals(c["text"]!!.jsonPrimitive.content, Format.age(c["ms"]!!.jsonPrimitive.long))
        }
    }

    @Test
    fun arrowVectors() {
        for (c in vectors["arrow"]!!.jsonArray.map { it.jsonObject }) {
            fun n(k: String) = c[k]!!.jsonPrimitive.double
            assertTrue(close(Geodesy.arrowRotationDeg(n("bearing_true"), n("heading_true")), n("arrow"), 1e-9))
            assertTrue(close(Geodesy.trueToMagneticDeg(n("bearing_true"), n("declination")), n("bearing_magnetic"), 1e-9))
        }
    }

    @Test
    fun nearPoleAndPoleOrigin() {
        val a = LatLon(89.9, 0.0)
        val b = LatLon(89.9, 180.0)
        assertTrue(close(Geodesy.distanceM(a, b)!!, Math.toRadians(0.2) * Geodesy.EARTH_RADIUS_M, 0.01))
        assertTrue(close(Geodesy.initialBearingDeg(a, b)!!, 0.0, 1e-6))
        assertTrue(close(Geodesy.initialBearingDeg(b, a)!!, 0.0, 1e-6))
        assertTrue(close(Geodesy.initialBearingDeg(a, LatLon(89.9, 90.0))!!, 45.0, 0.01))
        assertNull(Geodesy.initialBearingDeg(LatLon(90.0, 0.0), a))
        assertNull(Geodesy.initialBearingDeg(LatLon(-90.0, 0.0), LatLon(-89.0, 0.0)))
    }

    @Test
    fun shortDistancesAndNoBearingCases() {
        val a = LatLon(32.9904, -106.9750)
        val east = LatLon(32.9904, -106.9750 + 10.0 / (111_195.08 * kotlin.math.cos(Math.toRadians(32.9904))))
        assertTrue(close(Geodesy.distanceM(a, east)!!, 10.0, 0.01))
        assertTrue(close(Geodesy.initialBearingDeg(a, east)!!, 90.0, 0.01))
        assertEquals(0.0, Geodesy.distanceM(a, a)!!, 0.0)
        assertNull(Geodesy.initialBearingDeg(a, a))
        assertNull(Geodesy.initialBearingDeg(LatLon(40.0, -75.0), LatLon(40.000005, -75.0)))
    }

    @Test
    fun invalidInputsAndAngleHelpers() {
        assertNull(Geodesy.distanceM(LatLon(Double.NaN, 0.0), LatLon(0.0, 0.0)))
        assertNull(Geodesy.distanceM(LatLon(91.0, 0.0), LatLon(0.0, 0.0)))
        assertNull(Geodesy.initialBearingDeg(LatLon(0.0, 181.0), LatLon(0.0, 0.0)))
        assertEquals(0.0, Geodesy.normalizeDeg(-1e-15), 0.0)
        assertEquals(20.0, Geodesy.signedDeltaDeg(350.0, 10.0), 1e-9)
        assertEquals(-20.0, Geodesy.signedDeltaDeg(10.0, 350.0), 1e-9)
        assertEquals(180.0, Geodesy.signedDeltaDeg(0.0, 180.0), 1e-9)
        assertEquals(0.0, Geodesy.magneticToTrueDeg(350.0, 10.0), 1e-9)
        assertEquals(97.5, Geodesy.trueToMagneticDeg(90.0, -7.5), 1e-9)
        assertEquals("N", Geodesy.cardinal(359.0))
        assertEquals("S", Geodesy.cardinal(200.0))
        assertEquals("NW", Geodesy.cardinal(300.0))
        assertEquals("--", Format.distance(Double.NaN, true))
        assertEquals("328 ft", Format.distance(100.0, false))
    }
}
