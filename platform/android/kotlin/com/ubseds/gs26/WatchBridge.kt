package com.ubseds.gs26

import android.content.Context
import android.net.Uri
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
import java.util.ArrayDeque

/**
 * Phone side of the Wear OS companion. Rust publishes snapshot JSON and polls
 * watch messages through the static methods below; this object only moves
 * strings between the Wear Data Layer and a small queue.
 */
object WatchBridge : MessageClient.OnMessageReceivedListener,
    DataClient.OnDataChangedListener,
    CapabilityClient.OnCapabilityChangedListener {
    private const val SNAPSHOT_PATH = "/gs26/snapshot"
    private const val WATCH_MESSAGE_PATH = "/gs26/watch"
    private const val WATCH_SETTINGS_PATH = "/gs26/watch_settings"
    private const val WATCH_CAPABILITY = "gs26_watch"
    private const val MAX_QUEUED = 32
    private const val MAX_MESSAGE_BYTES = 32 * 1024

    private val incoming = ArrayDeque<String>()
    private var appContext: Context? = null
    private var available = false

    @Volatile
    private var watchNodes = 0

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

    @JvmStatic
    fun takeIncoming(): String? = synchronized(incoming) { incoming.pollFirst() }

    /** bit0 supported, bit1 paired/connected node, bit2 app installed, bit3 reachable. */
    @JvmStatic
    fun state(): Int {
        if (!available) return 0
        return if (watchNodes > 0) 1 or 2 or 4 or 8 else 1
    }

    private fun enqueue(json: String) {
        if (json.toByteArray(Charsets.UTF_8).size > MAX_MESSAGE_BYTES) return
        synchronized(incoming) {
            if (incoming.size >= MAX_QUEUED) incoming.pollFirst()
            incoming.addLast(json)
        }
    }

    override fun onMessageReceived(event: MessageEvent) {
        if (event.path == WATCH_MESSAGE_PATH) enqueue(String(event.data, Charsets.UTF_8))
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
    }
}
