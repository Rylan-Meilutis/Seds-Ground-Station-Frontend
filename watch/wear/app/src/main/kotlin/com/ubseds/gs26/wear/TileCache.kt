package com.ubseds.gs26.wear

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.BitmapFactory
import android.util.Log
import android.util.LruCache
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.core.content.edit
import com.ubseds.gs26.wear.core.CacheEntry
import com.ubseds.gs26.wear.core.MapManifest
import com.ubseds.gs26.wear.core.MapTiles
import com.ubseds.gs26.wear.core.Protocol
import com.ubseds.gs26.wear.core.TileId
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.io.File

/**
 * Map tiles on the watch (docs/watch-companion.md "Map background"): JPEG
 * files in the app's files directory, capped at the phone's size cap with
 * core `MapTiles.evictionOrder` (tiles outside the current set first, then
 * least recently used). Tiles come from the phone (Data Layer assets) or,
 * in direct mode, from the station's tile endpoint.
 */
class TileCache private constructor(context: Context) {
    private val dir = File(context.applicationContext.filesDir, "map_tiles").apply { mkdirs() }
    private val prefs = context.applicationContext.getSharedPreferences("gs26_map", Context.MODE_PRIVATE)
    private val lock = Any()
    private val entries = HashMap<String, CacheEntry>()
    private val bitmaps = object : LruCache<String, ImageBitmap>(MEMORY_TILES) {}

    private val _keys = MutableStateFlow<Set<String>>(emptySet())
    /** Keys of the cached tiles; changes whenever one is added or removed. */
    val keys: StateFlow<Set<String>> = _keys.asStateFlow()

    private val _manifest = MutableStateFlow(
        prefs.getString(KEY_MANIFEST, null)?.let(MapManifest::parse),
    )
    /** The phone's latest tile set (attribution, cap, set id). */
    val manifest: StateFlow<MapManifest?> = _manifest.asStateFlow()

    init {
        synchronized(lock) {
            dir.listFiles()?.forEach { f ->
                val key = keyOf(f.name)
                if (key == null || f.length() == 0L) f.delete()
                else entries[key] = CacheEntry(key, f.length(), f.lastModified())
            }
            _keys.value = entries.keys.toSet()
        }
    }

    private fun fileOf(key: String) = File(dir, key.replace('/', '_') + ".jpg")

    private fun keyOf(name: String): String? =
        name.removeSuffix(".jpg").replace('_', '/').takeIf { name.endsWith(".jpg") && TileId.parse(it) != null }

    fun has(key: String): Boolean = synchronized(lock) { key in entries }

    /** Cap of the cache: the phone's set cap, or the user's setting. */
    private fun cap(settingsCap: Long?): Long = _manifest.value?.capBytes ?: settingsCap ?: DEFAULT_CAP

    fun applyManifest(json: String) {
        val m = MapManifest.parse(json) ?: return
        prefs.edit { putString(KEY_MANIFEST, json) }
        _manifest.value = m
        evict(null)
    }

    /** Stores one tile (from the phone or the station) and trims the cache. */
    fun put(key: String, bytes: ByteArray, settingsCap: Long? = null) {
        if (TileId.parse(key) == null || bytes.isEmpty() || bytes.size > MAX_TILE_BYTES) return
        // Only JPEG/PNG images are kept.
        val jpeg = bytes.size > 3 && bytes[0] == 0xFF.toByte() && bytes[1] == 0xD8.toByte()
        val png = bytes.size > 8 && bytes[0] == 0x89.toByte() && bytes[1] == 'P'.code.toByte()
        if (!jpeg && !png) return
        val file = fileOf(key)
        val tmp = File(dir, file.name + ".part")
        try {
            tmp.writeBytes(bytes)
            if (!tmp.renameTo(file)) { tmp.delete(); return }
        } catch (e: Exception) {
            Log.w(TAG, "could not store tile $key", e)
            tmp.delete()
            return
        }
        synchronized(lock) {
            entries[key] = CacheEntry(key, bytes.size.toLong(), System.currentTimeMillis())
            bitmaps.remove(key)
        }
        evict(settingsCap)
    }

    private fun evict(settingsCap: Long?) {
        val plan = _manifest.value?.tiles?.toSet().orEmpty()
        synchronized(lock) {
            for (key in MapTiles.evictionOrder(entries.values.toList(), plan, cap(settingsCap))) {
                fileOf(key).delete()
                entries.remove(key)
                bitmaps.remove(key)
            }
            _keys.value = entries.keys.toSet()
        }
    }

    /** Decoded tile, or null when not cached or unreadable. Marks it used. */
    fun bitmap(key: String): ImageBitmap? {
        bitmaps.get(key)?.let { touch(key); return it }
        if (!has(key)) return null
        val decoded = runCatching { BitmapFactory.decodeFile(fileOf(key).path)?.asImageBitmap() }.getOrNull()
        if (decoded == null) {
            synchronized(lock) { entries.remove(key); _keys.value = entries.keys.toSet() }
            fileOf(key).delete()
            return null
        }
        bitmaps.put(key, decoded)
        touch(key)
        return decoded
    }

    private fun touch(key: String) {
        val now = System.currentTimeMillis()
        synchronized(lock) {
            val e = entries[key] ?: return
            // Persist the use time at most once a minute per tile.
            if (now - e.usedMs > 60_000) fileOf(key).setLastModified(now)
            entries[key] = e.copy(usedMs = now)
        }
    }

    /** The `tiles` report for the phone. */
    fun report(): String = Protocol.tilesMessage(_manifest.value?.set, keys.value)

    val attribution: String get() = _manifest.value?.attribution ?: MapTiles.ATTRIBUTION

    companion object {
        private const val TAG = "GsTileCache"
        private const val KEY_MANIFEST = "manifest_json"
        private const val MEMORY_TILES = 24
        private const val MAX_TILE_BYTES = 512 * 1024
        private const val DEFAULT_CAP = 3L * 1024 * 1024

        @SuppressLint("StaticFieldLeak")
        @Volatile private var instance: TileCache? = null
        fun get(context: Context): TileCache =
            instance ?: synchronized(this) { instance ?: TileCache(context).also { instance = it } }
    }
}
