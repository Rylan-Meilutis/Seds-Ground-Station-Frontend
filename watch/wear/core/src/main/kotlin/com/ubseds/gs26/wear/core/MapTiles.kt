package com.ubseds.gs26.wear.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlin.math.PI
import kotlin.math.atan
import kotlin.math.cos
import kotlin.math.floor
import kotlin.math.ln
import kotlin.math.log2
import kotlin.math.pow
import kotlin.math.roundToLong
import kotlin.math.sinh
import kotlin.math.sqrt
import kotlin.math.tan

/**
 * Map background tile math, a port of the phone's `map_tiles.rs`
 * (docs/watch-companion.md "Map background"). Checked against the
 * `map_tiles` vectors in watch/test-vectors.json, like the Swift core.
 */
data class TileId(val z: Int, val x: Int, val y: Int) {
    val key: String get() = "$z/$x/$y"

    /** (north, west, south, east) in degrees. */
    fun bounds(): DoubleArray {
        val (n, w) = MapTiles.corner(z, x, y)
        val (s, e) = MapTiles.corner(z, x + 1, y + 1)
        return doubleArrayOf(n, w, s, e)
    }

    companion object {
        fun of(z: Int, x: Int, y: Int): TileId? {
            if (z !in 0..MapTiles.MAX_KEY_ZOOM) return null
            val n = 1L shl z
            return if (x in 0 until n && y in 0 until n) TileId(z, x, y) else null
        }

        fun parse(key: String): TileId? {
            val parts = key.split('/')
            if (parts.size != 3 || parts.any { p -> p.isEmpty() || !p.all { it in '0'..'9' } || p.length > 9 }) return null
            return of(parts[0].toInt(), parts[1].toInt(), parts[2].toInt())
        }
    }
}

/** The phone's tile-set manifest (`type: map`). */
@Serializable
data class MapManifest(
    val v: Int = 0,
    val type: String = "",
    val set: String = "",
    val tiles: List<String> = emptyList(),
    val attribution: String = MapTiles.ATTRIBUTION,
    @SerialName("cap_bytes") val capBytes: Long = 3L * 1024 * 1024,
    @SerialName("max_zoom") val maxZoom: Int = 15,
) {
    companion object {
        fun parse(json: String): MapManifest? = runCatching {
            Protocol.json.decodeFromString(serializer(), json)
        }.getOrNull()?.takeIf { it.v == Protocol.VERSION && it.type == "map" }
    }
}

/** One cached tile on the watch. */
data class CacheEntry(val key: String, val bytes: Long, val usedMs: Long)

/** Where a missing tile can be cut from: an ancestor and the sub-square of it (fractions). */
data class AncestorCrop(val key: String, val x: Double, val y: Double, val size: Double)

/** One tile to draw: offset of its top-left corner from the view centre in px, before rotation. */
data class Placement(val tile: TileId, val left: Double, val top: Double)

data class VisibleTiles(val z: Int, val tilePx: Double, val tiles: List<Placement>)

object MapTiles {
    const val TILE_PX = 256.0
    const val MAX_LAT = 85.05112878
    const val MAX_KEY_ZOOM = 22
    val MERCATOR_CIRCUMFERENCE_M = 2.0 * PI * 6_378_137.0
    const val ATTRIBUTION = "Imagery © Esri, Maxar, Earthstar Geographics"

    /** Zoom range of the compass map (the crown zooms within it). */
    const val MIN_VIEW_ZOOM = 10.0
    const val MAX_VIEW_ZOOM = 17.0
    /** Auto-fit keeps the target inside this range. */
    const val MIN_FIT_ZOOM = 11.0
    const val MAX_FIT_ZOOM = 16.0

    fun corner(z: Int, x: Int, y: Int): Pair<Double, Double> {
        val n = (1L shl z).toDouble()
        val lon = x / n * 360.0 - 180.0
        val lat = Math.toDegrees(atan(sinh(PI * (1.0 - 2.0 * y / n))))
        return lat to lon
    }

    /** Fractional tile coordinates of a point at zoom [z]. */
    fun tileXY(lat: Double, lon: Double, z: Int): Pair<Double, Double> {
        val n = (1L shl z).toDouble()
        val r = Math.toRadians(lat.coerceIn(-MAX_LAT, MAX_LAT))
        val fx = (lon + 180.0) / 360.0 * n
        val fy = (1.0 - ln(tan(r) + 1.0 / cos(r)) / PI) / 2.0 * n
        return fx to fy
    }

    private fun clampIndex(v: Double, n: Long): Int = floor(v).coerceAtLeast(0.0).toLong().coerceAtMost(n - 1).toInt()

    fun tileAt(lat: Double, lon: Double, z: Int): TileId {
        val (fx, fy) = tileXY(lat, lon, z)
        val n = 1L shl z
        return TileId(z, clampIndex(fx, n), clampIndex(fy, n))
    }

    fun tileSpanM(lat: Double, z: Int): Double = MERCATOR_CIRCUMFERENCE_M * cos(Math.toRadians(lat)) / (1L shl z).toDouble()

    /** The phone's tile-set plan (same order as `map_tiles::coverage`). */
    fun coverage(centres: List<LatLon>, levels: List<Pair<Int, Double>>, maxZoom: Int, maxTiles: Int): List<TileId> {
        val merged = sortedMapOf<Int, Double>()
        for ((z0, radius) in levels) {
            val z = minOf(z0, maxZoom, MAX_KEY_ZOOM)
            merged[z] = maxOf(merged[z] ?: 0.0, radius)
        }
        val out = ArrayList<TileId>()
        val seen = HashSet<TileId>()
        for ((z, radius) in merged) {
            val n = 1L shl z
            for (c in centres.filter { it.valid() }) {
                val (fx, fy) = tileXY(c.lat, c.lon, z)
                val r = radius / tileSpanM(c.lat, z)
                val cells = ArrayList<Triple<Long, Int, Int>>()
                for (y in clampIndex(fy - r, n)..clampIndex(fy + r, n)) {
                    for (x in clampIndex(fx - r, n)..clampIndex(fx + r, n)) {
                        val dx = x + 0.5 - fx
                        val dy = y + 0.5 - fy
                        cells.add(Triple(((dx * dx + dy * dy) * 1e6).roundToLong(), y, x))
                    }
                }
                cells.sortWith(compareBy({ it.first }, { it.second }, { it.third }))
                for ((_, y, x) in cells) {
                    val id = TileId(z, x, y)
                    if (seen.add(id)) out.add(id)
                }
            }
        }
        return out.take(maxTiles)
    }

    /** Greedy byte budget in plan order; a size of 0 is never kept. */
    fun fitBudget(sizes: List<Long>, cap: Long): List<Int> {
        var total = 0L
        val chosen = ArrayList<Int>()
        sizes.forEachIndexed { i, size ->
            if (size > 0 && total + size <= cap) {
                total += size
                chosen.add(i)
            }
        }
        return chosen
    }

    /** Plan tiles not held yet, in plan order. */
    fun manifestDiff(plan: List<String>, have: Set<String>): List<String> = plan.filter { it !in have }

    /** Keys to delete so the cache fits [cap]: outside the plan first, then least recently used, then by key. */
    fun evictionOrder(entries: List<CacheEntry>, plan: Set<String>, cap: Long): List<String> {
        var total = entries.sumOf { it.bytes }
        val order = entries.sortedWith(compareBy({ it.key in plan }, { it.usedMs }, { it.key }))
        val out = ArrayList<String>()
        for (e in order) {
            if (total <= cap) break
            total -= e.bytes
            out.add(e.key)
        }
        return out
    }

    /** Zoom that puts a target [distanceM] away at [radiusPx] from the centre, clamped. */
    fun autoFitZoom(distanceM: Double, lat: Double, radiusPx: Double, minZ: Double, maxZ: Double): Double {
        if (!(distanceM.isFinite() && distanceM > 0.0)) return maxZ
        val z = log2(MERCATOR_CIRCUMFERENCE_M * cos(Math.toRadians(lat)) * radiusPx / (TILE_PX * distanceM))
        return z.coerceIn(minZ, maxZ)
    }

    fun ancestor(tile: TileId, available: (String) -> Boolean, maxUp: Int = 6): AncestorCrop? {
        for (up in 1..minOf(maxUp, tile.z)) {
            val parent = TileId(tile.z - up, tile.x shr up, tile.y shr up)
            if (available(parent.key)) {
                val size = 1.0 / (1L shl up).toDouble()
                return AncestorCrop(parent.key, (tile.x - (parent.x shl up)) * size, (tile.y - (parent.y shl up)) * size, size)
            }
        }
        return null
    }

    /** Tiles covering a [width]×[height] view (its circumscribed circle) at fractional [zoom]. */
    fun visibleTiles(lat: Double, lon: Double, zoom: Double, width: Double, height: Double): VisibleTiles {
        val z = floor(zoom + 0.5).coerceAtLeast(0.0).toInt().coerceAtMost(MAX_KEY_ZOOM)
        val px = TILE_PX * 2.0.pow(zoom - z)
        val (fx, fy) = tileXY(lat, lon, z)
        val r = sqrt(width * width + height * height) / 2.0 / px
        val n = 1L shl z
        val out = ArrayList<Placement>()
        for (y in clampIndex(fy - r, n)..clampIndex(fy + r, n)) {
            for (x in clampIndex(fx - r, n)..clampIndex(fx + r, n)) {
                out.add(Placement(TileId(z, x, y), (x - fx) * px, (y - fy) * px))
            }
        }
        return VisibleTiles(z, px, out)
    }

    /** Pixel offset (east, south) of a point from the view centre at [zoom]. */
    fun project(lat: Double, lon: Double, centerLat: Double, centerLon: Double, zoom: Double): Pair<Double, Double> {
        val (u, v) = tileXY(lat, lon, 0)
        val (cu, cv) = tileXY(centerLat, centerLon, 0)
        val scale = TILE_PX * 2.0.pow(zoom)
        return (u - cu) * scale to (v - cv) * scale
    }
}
