package com.ubseds.gs26.wear.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.double
import kotlinx.serialization.json.int
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

/** Mirrors the Rust `map_tiles` tests: same vectors, same tolerances. */
class MapTilesVectorsTest {
    private val vectors: JsonObject by lazy {
        Json.parseToJsonElement(vectorFile().readText()).jsonObject["map_tiles"]!!.jsonObject
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

    private fun cases(name: String) = vectors[name]!!.jsonArray.map { it.jsonObject }
    private fun JsonElement.d() = jsonPrimitive.double
    private fun JsonElement.strings() = jsonArray.map { it.jsonPrimitive.content }
    private fun close(a: Double, b: Double, tol: Double) = abs(a - b) <= tol

    @Test
    fun tileXY() {
        for (c in cases("tile_xy")) {
            val name = c["name"]!!.jsonPrimitive.content
            val z = c["z"]!!.jsonPrimitive.int
            val (fx, fy) = MapTiles.tileXY(c["lat"]!!.d(), c["lon"]!!.d(), z)
            assertTrue("$name fx $fx", close(fx, c["fx"]!!.d(), 1e-9))
            assertTrue("$name fy $fy", close(fy, c["fy"]!!.d(), 1e-9))
            val tile = MapTiles.tileAt(c["lat"]!!.d(), c["lon"]!!.d(), z)
            assertEquals(name, c["key"]!!.jsonPrimitive.content, tile.key)
            assertEquals(tile, TileId.parse(tile.key))
            val b = tile.bounds()
            listOf("north", "west", "south", "east").forEachIndexed { i, k ->
                assertTrue("$name $k ${b[i]}", close(b[i], c[k]!!.d(), 1e-9))
            }
            assertTrue(name, close(MapTiles.tileSpanM(c["lat"]!!.d(), z), c["span_m"]!!.d(), 1e-6))
        }
    }

    @Test
    fun coverage() {
        for (c in cases("coverage")) {
            val centres = c["centres"]!!.jsonArray.map { p -> LatLon(p.jsonArray[0].d(), p.jsonArray[1].d()) }
            val levels = c["levels"]!!.jsonArray.map { l -> l.jsonArray[0].jsonPrimitive.int to l.jsonArray[1].d() }
            val got = MapTiles.coverage(centres, levels, c["max_zoom"]!!.jsonPrimitive.int, c["max_tiles"]!!.jsonPrimitive.int)
            assertEquals(c["name"]!!.jsonPrimitive.content, c["tiles"]!!.strings(), got.map { it.key })
        }
    }

    @Test
    fun budgetDiffAndEviction() {
        for (c in cases("budget")) {
            val sizes = c["sizes"]!!.jsonArray.map { it.jsonPrimitive.long }
            val chosen = c["chosen"]!!.jsonArray.map { it.jsonPrimitive.int }
            assertEquals(c["name"]!!.jsonPrimitive.content, chosen, MapTiles.fitBudget(sizes, c["cap"]!!.jsonPrimitive.long))
        }
        for (c in cases("manifest_diff")) {
            assertEquals(c["send"]!!.strings(), MapTiles.manifestDiff(c["plan"]!!.strings(), c["have"]!!.strings().toSet()))
        }
        for (c in cases("evict")) {
            val entries = c["entries"]!!.jsonArray.map { e ->
                val o = e.jsonObject
                CacheEntry(o["key"]!!.jsonPrimitive.content, o["bytes"]!!.jsonPrimitive.long, o["used_ms"]!!.jsonPrimitive.long)
            }
            assertEquals(
                c["name"]!!.jsonPrimitive.content,
                c["evict"]!!.strings(),
                MapTiles.evictionOrder(entries, c["plan"]!!.strings().toSet(), c["cap"]!!.jsonPrimitive.long),
            )
        }
    }

    @Test
    fun viewMath() {
        for (c in cases("auto_fit")) {
            val got = MapTiles.autoFitZoom(c["distance_m"]!!.d(), c["lat"]!!.d(), c["radius_px"]!!.d(), c["min_z"]!!.d(), c["max_z"]!!.d())
            assertTrue("${c["name"]} $got", close(got, c["zoom"]!!.d(), 1e-9))
        }
        for (c in cases("ancestor")) {
            val available = c["available"]!!.strings().toSet()
            val got = MapTiles.ancestor(TileId.parse(c["key"]!!.jsonPrimitive.content)!!, { it in available })
            when (val want = c["found"]) {
                is JsonNull, null -> assertNull(got)
                else -> {
                    val w = want.jsonObject
                    assertNotNull(got)
                    assertEquals(w["key"]!!.jsonPrimitive.content, got!!.key)
                    assertTrue(close(got.x, w["x"]!!.d(), 1e-12))
                    assertTrue(close(got.y, w["y"]!!.d(), 1e-12))
                    assertTrue(close(got.size, w["size"]!!.d(), 1e-12))
                }
            }
        }
        for (c in cases("visible")) {
            val got = MapTiles.visibleTiles(c["lat"]!!.d(), c["lon"]!!.d(), c["zoom"]!!.d(), c["width"]!!.d(), c["height"]!!.d())
            assertEquals(c["z"]!!.jsonPrimitive.int, got.z)
            assertTrue(close(got.tilePx, c["tile_px"]!!.d(), 1e-6))
            val want = c["tiles"]!!.jsonArray.map { it.jsonObject }
            assertEquals(want.size, got.tiles.size)
            got.tiles.zip(want).forEach { (g, w) ->
                assertEquals(w["key"]!!.jsonPrimitive.content, g.tile.key)
                assertTrue(close(g.left, w["left"]!!.d(), 1e-6))
                assertTrue(close(g.top, w["top"]!!.d(), 1e-6))
            }
        }
        for (c in cases("project")) {
            val center = c["center"] as JsonArray
            val (dx, dy) = MapTiles.project(c["lat"]!!.d(), c["lon"]!!.d(), center[0].d(), center[1].d(), c["zoom"]!!.d())
            assertTrue("${c["name"]}", close(dx, c["dx"]!!.d(), 1e-6))
            assertTrue("${c["name"]}", close(dy, c["dy"]!!.d(), 1e-6))
        }
    }

    @Test
    fun keysManifestAndReport() {
        assertEquals(TileId(15, 9204, 12054), TileId.parse("15/9204/12054"))
        for (bad in listOf("", "1/2", "1/2/3/4", "1/2/1", "23/0/0", "a/b/c", "-1/0/0", "3/8/0")) assertNull(bad, TileId.parse(bad))
        val m = MapManifest.parse("""{"v":1,"type":"map","set":"ab","tiles":["11/1/2"],"attribution":"x","cap_bytes":5,"max_zoom":11,"centres":[]}""")
        assertEquals(listOf("11/1/2"), m!!.tiles)
        assertEquals(5L, m.capBytes)
        assertNull(MapManifest.parse("""{"v":1,"type":"direct"}"""))
        val report = Json.parseToJsonElement(Protocol.tilesMessage("ab", listOf("13/5/6", "11/1/2"))).jsonObject
        assertEquals("tiles", report["type"]!!.jsonPrimitive.content)
        assertEquals(listOf("11/1/2", "13/5/6"), report["have"]!!.strings())
    }

    @Test
    fun mapSettingDefaultsAndCap() {
        val s = Protocol.json.decodeFromString(WatchSettings.serializer(), """{"version":2}""")
        assertTrue(s.map.enabled)
        assertEquals(3L * 1024 * 1024, s.map.capBytes)
        val off = Protocol.json.decodeFromString(WatchSettings.serializer(), """{"map":{"enabled":false,"cap_mb":9}}""")
        assertTrue(!off.map.enabled && off.map.capBytes == 5L * 1024 * 1024)
        assertTrue(s.withMapEnabled(false).map.enabled.not())
        // replan is phone-side only; the watch never needs it, but the vectors stay readable here.
        assertTrue(cases("replan").any { it["replan"]!!.jsonPrimitive.boolean })
    }
}
