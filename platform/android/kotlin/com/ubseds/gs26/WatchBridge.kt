package com.ubseds.gs26

import android.content.Context
import android.net.Uri
import android.util.Log
import com.google.android.gms.wearable.Asset
import com.google.android.gms.wearable.CapabilityClient
import com.google.android.gms.wearable.CapabilityInfo
import com.google.android.gms.wearable.DataClient
import com.google.android.gms.wearable.DataEvent
import com.google.android.gms.wearable.DataEventBuffer
import com.google.android.gms.wearable.DataMapItem
import com.google.android.gms.wearable.MessageClient
import com.google.android.gms.wearable.MessageEvent
import com.google.android.gms.wearable.PutDataMapRequest
import com.google.android.gms.wearable.Wearable
import java.io.File
import java.util.ArrayDeque
import org.json.JSONArray
import org.json.JSONObject

/**
 * Phone side of the Wear OS companion. Rust publishes snapshot JSON and polls
 * watch messages through the static methods below; this object only moves
 * strings between the Wear Data Layer and a small queue. While a watch with
 * the GS app is reachable it runs [WatchLinkService] so the publisher keeps
 * running in the background.
 */
object WatchBridge : MessageClient.OnMessageReceivedListener,
    DataClient.OnDataChangedListener,
    CapabilityClient.OnCapabilityChangedListener {
    private const val SNAPSHOT_PATH = "/gs26/snapshot"
    private const val WATCH_MESSAGE_PATH = "/gs26/watch"
    private const val WATCH_SETTINGS_PATH = "/gs26/watch_settings"
    /** Direct-link provisioning: station address + scoped watch token. */
    private const val DIRECT_PATH = "/gs26/direct"
    /** Map background: tile-set manifest, and one DataItem per tile. */
    private const val MAP_PATH = "/gs26/map"
    private const val TILE_PATH_PREFIX = "/gs26/tile/"
    private const val MAX_TILE_BYTES = 512 * 1024
    /** Crew voice relay (docs/watch-companion.md "Phone relay"). */
    private const val WATCH_VOICE_PATH = "/gs26/watch_voice"
    private const val VOICE_PATH = "/gs26/voice"
    private const val MAX_VOICE_QUEUED = 64
    private const val WATCH_CAPABILITY = "gs26_watch"
    private const val MAX_QUEUED = 32
    private const val MAX_MESSAGE_BYTES = 32 * 1024

    private val incoming = ArrayDeque<String>()
    private val voiceIncoming = ArrayDeque<String>()

    /** Ids of reachable nodes running the watch app (voice relay targets). */
    @Volatile
    private var watchNodeIds: List<String> = emptyList()
    private var appContext: Context? = null
    private var available = false

    /** Reachable nodes running the watch app. */
    @Volatile
    private var watchNodes = 0

    /** Known nodes (reachable or not) that have the watch app installed. */
    @Volatile
    private var installedNodes = 0

    /** The Data Layer answered the first capability query. */
    @Volatile
    private var queried = false

    @Volatile
    private var seq = 0L

    @JvmStatic
    fun start(context: Context) {
        synchronized(this) {
            if (appContext != null) return
            val ctx = context.applicationContext
            try {
                Wearable.getMessageClient(ctx).addListener(this)
                Wearable.getDataClient(ctx).addListener(this)
                Wearable.getCapabilityClient(ctx).addListener(this, WATCH_CAPABILITY)
                appContext = ctx
                available = true
            } catch (_: Throwable) {
                // Play services / Wear API missing: stay unsupported.
                available = false
                return
            }
        }
        val ctx = appContext ?: return
        Wearable.getCapabilityClient(ctx)
            .getCapability(WATCH_CAPABILITY, CapabilityClient.FILTER_REACHABLE)
            .addOnSuccessListener { onCapabilityChanged(it) }
        refreshInstalled(ctx)
        // Settings edited while the phone app was closed arrive as a data item.
        Wearable.getDataClient(ctx).getDataItems(Uri.parse("wear://*$WATCH_SETTINGS_PATH"))
            .addOnSuccessListener { items ->
                items.forEach { item -> DataMapItem.fromDataItem(item).dataMap.getString("json")?.let(::enqueue) }
                items.release()
            }
    }

    @JvmStatic
    fun publish(json: String, urgent: Boolean) {
        val ctx = appContext ?: return
        seq++
        val request = PutDataMapRequest.create(SNAPSHOT_PATH).apply {
            dataMap.putString("json", json)
            dataMap.putLong("seq", seq)
        }
        val put = request.asPutDataRequest()
        if (urgent) put.setUrgent()
        Wearable.getDataClient(ctx).putDataItem(put)
    }

    /**
     * Direct-link provisioning for the watch. A DataItem syncs to the watch
     * whenever it is connected, also after this app went to the background;
     * the watch moves the token into its Keystore-encrypted storage. A
     * message with `enabled: false` replaces it and makes the watch forget
     * the token.
     */
    @JvmStatic
    fun provision(json: String) {
        val ctx = appContext ?: return
        val request = PutDataMapRequest.create(DIRECT_PATH).apply {
            dataMap.putString("json", json)
            dataMap.putLong("at", System.currentTimeMillis())
        }
        Wearable.getDataClient(ctx).putDataItem(request.asPutDataRequest().setUrgent())
    }

    /**
     * Map background manifest (`type: map`). Synced as a DataItem like the
     * snapshot; tile DataItems that left the set are deleted.
     */
    @JvmStatic
    fun sendMap(json: String) {
        val ctx = appContext ?: return
        val keep = try {
            val tiles = JSONObject(json).optJSONArray("tiles") ?: JSONArray()
            (0 until tiles.length()).map { tiles.getString(it) }.toSet()
        } catch (_: Throwable) {
            return
        }
        val client = Wearable.getDataClient(ctx)
        val request = PutDataMapRequest.create(MAP_PATH).apply {
            dataMap.putString("json", json)
            dataMap.putLong("at", System.currentTimeMillis())
        }
        client.putDataItem(request.asPutDataRequest().setUrgent())
        client.getDataItems(Uri.parse("wear://*$TILE_PATH_PREFIX"), DataClient.FILTER_PREFIX)
            .addOnSuccessListener { items ->
                val stale = items.mapNotNull { item ->
                    item.uri.takeIf { tileKey(it) !in keep }
                }
                items.release()
                stale.forEach { client.deleteDataItems(it) }
            }
    }

    /**
     * One map tile: DataItem `/gs26/tile/{z}/{x}/{y}` carrying the JPEG as an
     * Asset. The watch stores it; the phone deletes the item once the watch
     * reports it (or when it leaves the set).
     */
    @JvmStatic
    fun sendTile(key: String, path: String, set: String) {
        val ctx = appContext ?: return
        if (!TILE_KEY.matches(key)) return
        val bytes = try {
            File(path).takeIf { it.isFile && it.length() in 1..MAX_TILE_BYTES }?.readBytes()
        } catch (t: Throwable) {
            Log.w(TAG, "could not read tile $key", t)
            null
        } ?: return
        val request = PutDataMapRequest.create(TILE_PATH_PREFIX + key).apply {
            dataMap.putAsset("tile", Asset.createFromBytes(bytes))
            dataMap.putString("set", set)
            dataMap.putString("key", key)
        }
        Wearable.getDataClient(ctx).putDataItem(request.asPutDataRequest())
    }

    /** The watch reported these tiles (JSON array of keys): drop their DataItems. */
    @JvmStatic
    fun tilesDelivered(json: String) {
        val ctx = appContext ?: return
        val keys = try {
            val array = JSONArray(json)
            (0 until array.length()).map { array.getString(it) }.filter { TILE_KEY.matches(it) }
        } catch (_: Throwable) {
            return
        }
        val client = Wearable.getDataClient(ctx)
        keys.forEach { client.deleteDataItems(Uri.parse("wear://*$TILE_PATH_PREFIX$it")) }
    }

    private val TILE_KEY = Regex("^\\d{1,2}/\\d{1,7}/\\d{1,7}$")
    private const val TAG = "GsWatchBridge"

    private fun tileKey(uri: Uri): String = uri.path.orEmpty().removePrefix(TILE_PATH_PREFIX)

    @JvmStatic
    fun takeIncoming(): String? = synchronized(incoming) { incoming.pollFirst() }

    /** Next crew voice relay message from the watch (own queue, drained every 20 ms). */
    @JvmStatic
    fun takeVoice(): String? = synchronized(voiceIncoming) { voiceIncoming.pollFirst() }

    /** Crew voice relay to the watch: a message to each reachable watch node, never queued. */
    @JvmStatic
    fun sendVoice(json: String) {
        val ctx = appContext ?: return
        val bytes = json.toByteArray(Charsets.UTF_8)
        if (bytes.size > MAX_MESSAGE_BYTES) return
        val client = Wearable.getMessageClient(ctx)
        for (node in watchNodeIds) client.sendMessage(node, VOICE_PATH, bytes)
    }

    /** bit0 supported, bit1 paired/connected node, bit2 app installed, bit3 reachable,
     *  bit4 the Data Layer has answered (same bits as the iOS bridge). */
    @JvmStatic
    fun state(): Int {
        if (!available) return 0
        var bits = 1
        if (queried) bits = bits or 16
        if (installedNodes > 0 || watchNodes > 0) bits = bits or 2 or 4
        if (watchNodes > 0) bits = bits or 8
        return bits
    }

    /** The phone app's Settings only offer watch settings once a watch with the
     *  companion app is known, so count it even while it is out of range. */
    private fun refreshInstalled(ctx: Context) {
        Wearable.getCapabilityClient(ctx)
            .getCapability(WATCH_CAPABILITY, CapabilityClient.FILTER_ALL)
            .addOnSuccessListener {
                installedNodes = it.nodes.size
                queried = true
                updateService(ctx)
            }
    }

    /** Foreground service while a watch with the GS app is connected. */
    private fun updateService(ctx: Context) {
        if (watchNodes > 0) WatchLinkService.start(ctx) else WatchLinkService.stop(ctx)
    }

    private fun enqueue(json: String) {
        if (json.toByteArray(Charsets.UTF_8).size > MAX_MESSAGE_BYTES) return
        synchronized(incoming) {
            if (incoming.size >= MAX_QUEUED) incoming.pollFirst()
            incoming.addLast(json)
        }
    }

    override fun onMessageReceived(event: MessageEvent) {
        when (event.path) {
            WATCH_MESSAGE_PATH -> enqueue(String(event.data, Charsets.UTF_8))
            WATCH_VOICE_PATH -> {
                if (event.data.size > MAX_MESSAGE_BYTES) return
                synchronized(voiceIncoming) {
                    if (voiceIncoming.size >= MAX_VOICE_QUEUED) voiceIncoming.pollFirst()
                    voiceIncoming.addLast(String(event.data, Charsets.UTF_8))
                }
            }
        }
    }

    override fun onDataChanged(events: DataEventBuffer) {
        events.forEach { event ->
            if (event.type == DataEvent.TYPE_CHANGED && event.dataItem.uri.path == WATCH_SETTINGS_PATH) {
                DataMapItem.fromDataItem(event.dataItem).dataMap.getString("json")?.let(::enqueue)
            }
        }
        events.release()
    }

    override fun onCapabilityChanged(info: CapabilityInfo) {
        watchNodes = info.nodes.size
        watchNodeIds = info.nodes.map { it.id }
        appContext?.let(::refreshInstalled)
    }
}
