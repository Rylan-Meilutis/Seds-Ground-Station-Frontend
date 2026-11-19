package com.ubseds.gs26.wear.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.wear.compose.material.Text
import com.ubseds.gs26.wear.TileCache
import com.ubseds.gs26.wear.core.LatLon
import com.ubseds.gs26.wear.core.MapTiles
import kotlin.math.ceil
import kotlin.math.cos
import kotlin.math.roundToInt
import kotlin.math.sin

/** A target drawn on the map. */
data class MapMarker(val id: String, val pos: LatLon, val selected: Boolean, val stale: Boolean)

/** What the map draws: centre, zoom, rotation (map degrees clockwise from north-up). */
data class MapView(val center: LatLon, val zoom: Double, val rotationDeg: Double, val user: LatLon?)

private val MarkerColors = mapOf(
    "rocket" to Color(0xFFFF7043),
    "ground_station" to Color(0xFF66BB6A),
    "rrls" to Color(0xFF42A5F5),
)

/**
 * Station map tiles under the compass (docs/watch-companion.md "Map
 * background"): the cached tiles for [view] (an ancestor tile cut to size
 * where a level is missing), the targets, the bearing line from the watch to
 * the selected target and a north tick. Draws nothing when no tile covers
 * the view, so the plain compass shows through. Shows the imagery credit. [onVisible] gets the keys
 * of the view's tiles at the drawn zoom (to fetch missing ones directly).
 */
@Composable
fun MapLayer(
    cache: TileCache,
    cachedKeys: Set<String>,
    view: MapView,
    markers: List<MapMarker>,
    onVisible: suspend (List<String>) -> Unit,
) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val density = LocalDensity.current
        val w = with(density) { maxWidth.toPx() }.toDouble()
        val h = with(density) { maxHeight.toPx() }.toDouble()
        val visible = MapTiles.visibleTiles(view.center.lat, view.center.lon, view.zoom, w, h)
        val keys = visible.tiles.map { it.tile.key }
        LaunchedEffect(keys) { onVisible(keys) }
        // Each tile: its own image, or the nearest cached ancestor cut to size.
        val sources = visible.tiles.mapNotNull { p ->
            if (p.tile.key in cachedKeys) {
                cache.bitmap(p.tile.key)?.let { Triple(p, it, null) }
            } else {
                MapTiles.ancestor(p.tile, { it in cachedKeys })?.let { crop ->
                    cache.bitmap(crop.key)?.let { Triple(p, it, crop) }
                }
            }
        }
        if (sources.isEmpty()) return@BoxWithConstraints
        Canvas(Modifier.fillMaxSize()) {
            val c = center
            rotate(view.rotationDeg.toFloat(), c) {
                val size = ceil(visible.tilePx).toInt() + 1
                for ((p, image, crop) in sources) {
                    val dst = IntOffset((c.x + p.left).roundToInt(), (c.y + p.top).roundToInt())
                    if (crop == null) {
                        drawImage(image, dstOffset = dst, dstSize = IntSize(size, size))
                    } else {
                        drawCrop(image, crop.x, crop.y, crop.size, dst, size)
                    }
                }
            }
            // Dim the imagery so the compass text stays readable.
            drawRect(Color.Black.copy(alpha = 0.35f))
            val toScreen = { pos: LatLon ->
                val (dx, dy) = MapTiles.project(pos.lat, pos.lon, view.center.lat, view.center.lon, view.zoom)
                val r = Math.toRadians(view.rotationDeg)
                Offset(
                    (c.x + dx * cos(r) - dy * sin(r)).toFloat(),
                    (c.y + dx * sin(r) + dy * cos(r)).toFloat(),
                )
            }
            val user = view.user?.let(toScreen)
            markers.firstOrNull { it.selected }?.let { m ->
                if (user != null) drawLine(Color.White.copy(alpha = 0.8f), user, toScreen(m.pos), strokeWidth = 2f)
            }
            for (m in markers) {
                val at = toScreen(m.pos)
                val color = (MarkerColors[m.id] ?: Color.White).let { if (m.stale) it.copy(alpha = 0.5f) else it }
                val radius = if (m.selected) 7f else 5f
                drawCircle(color, radius, at)
                drawCircle(Color.Black, radius, at, style = Stroke(1.5f))
            }
            if (user != null) {
                drawCircle(Color.White, 5f, user)
                drawCircle(Color(0xFF4FC3F7), 3.5f, user)
            }
            northTick(view.rotationDeg)
        }
        // The tile licence asks for the imagery credit.
        Text(
            cache.attribution, fontSize = 7.sp, color = Color.White.copy(alpha = 0.7f), maxLines = 1,
            modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 16.dp),
        )
    }
}

private fun DrawScope.drawCrop(image: ImageBitmap, fx: Double, fy: Double, fsize: Double, dst: IntOffset, size: Int) {
    val src = IntOffset((fx * image.width).toInt(), (fy * image.height).toInt())
    val srcSize = IntSize(
        (fsize * image.width).toInt().coerceAtLeast(1),
        (fsize * image.height).toInt().coerceAtLeast(1),
    )
    drawImage(image, srcOffset = src, srcSize = srcSize, dstOffset = dst, dstSize = IntSize(size, size))
}

/** Small north marker on the rim. */
private fun DrawScope.northTick(rotationDeg: Double) {
    rotate(rotationDeg.toFloat(), center) {
        val r = size.minDimension / 2f
        drawLine(Color(0xFFFF5252), Offset(center.x, center.y - r + 2f), Offset(center.x, center.y - r + 12f), strokeWidth = 3f)
    }
}
