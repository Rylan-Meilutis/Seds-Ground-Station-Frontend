package com.ubseds.gs26.wear

import android.content.Context
import android.annotation.SuppressLint
import android.util.Log
import androidx.core.content.edit
import androidx.core.net.toUri
import com.google.android.gms.wearable.CapabilityClient
import com.google.android.gms.wearable.CapabilityInfo
import com.google.android.gms.wearable.DataClient
import com.google.android.gms.wearable.DataEvent
import com.google.android.gms.wearable.DataEventBuffer
import com.google.android.gms.wearable.DataMapItem
import com.google.android.gms.wearable.PutDataMapRequest
import com.google.android.gms.wearable.Wearable
import com.ubseds.gs26.wear.core.Direct
import com.ubseds.gs26.wear.core.Protocol
import com.ubseds.gs26.wear.core.Snapshot
import com.ubseds.gs26.wear.core.WatchSettings
import com.ubseds.gs26.wear.core.WatchState
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.tasks.await

/**
 * The watch's connection to the phone: snapshots and the direct-link
 * provisioning in over the Data Layer, hello/activity/settings out. While
 * the phone's data is not fresh, [pollDirectIfNeeded] reads the station
 * itself through [direct] (core `Direct.shouldPoll`).
 */
class PhoneLink private constructor(context: Context) :
    DataClient.OnDataChangedListener, CapabilityClient.OnCapabilityChangedListener {
    private val app = context.applicationContext
    private val prefs = app.getSharedPreferences("gs26_watch", Context.MODE_PRIVATE)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val dataClient = Wearable.getDataClient(app)
    private val messageClient = Wearable.getMessageClient(app)
    private val capabilityClient = Wearable.getCapabilityClient(app)

    private val _state = MutableStateFlow(loadPersisted())
    val state: StateFlow<WatchState> = _state.asStateFlow()

    private val _phoneReachable = MutableStateFlow(false)

    /** A connected phone node advertises the GS phone app (`gs26_phone`). */
    val phoneReachable: StateFlow<Boolean> = _phoneReachable.asStateFlow()

    /** Page id currently on screen, reported in `activity` messages. */
    @Volatile var currentPage: String? = null

    /** Direct link to the station, provisioned by the phone. */
    val direct: DirectLink = DirectLink.get(app)

    /**
     * Polls the station once if the phone's data is not fresh (call from the
     * foreground loop). Returns the delay before the next call.
     */
    suspend fun pollDirectIfNeeded(): Long {
        val s = _state.value
        val power = s.settings.power
        val provision = direct.state.value.provision
        val now = System.currentTimeMillis()
        val phoneAge = s.snapshot?.let { (now - s.receivedAtMs).coerceAtLeast(0) }
        val wanted = Direct.shouldPoll(s.snapshot?.mode, phoneAge, Direct.phoneFreshMs(power), foreground = true,
            configured = provision?.configured == true)
        if (wanted && provision != null) {
            direct.poll(Direct.statusUrl(provision.base, s.settings.metric, Direct.pageActionCommands(s.pages()), s.settings.compass.staleAfterS))
        } else {
            direct.setPolling(false)
        }
        return Direct.pollMs(power)
    }

    fun start() {
        dataClient.addListener(this)
        capabilityClient.addListener(this, Protocol.PHONE_CAPABILITY)
        scope.launch { readCurrentSnapshot() }
        scope.launch { readDirectProvision() }
        scope.launch { refreshReachable() }
        send(Protocol.helloMessage())
    }

    fun stop() {
        dataClient.removeListener(this)
        capabilityClient.removeListener(this, Protocol.PHONE_CAPABILITY)
    }

    override fun onCapabilityChanged(info: CapabilityInfo) {
        scope.launch { refreshReachable() }
    }

    private suspend fun refreshReachable() {
        runCatching {
            capabilityClient.getCapability(Protocol.PHONE_CAPABILITY, CapabilityClient.FILTER_REACHABLE).await().nodes
        }.onSuccess { _phoneReachable.value = it.isNotEmpty() }
            .onFailure { Log.w(TAG, "phone reachability query failed", it) }
    }

    fun sendActivity(foreground: Boolean) = send(Protocol.activityMessage(foreground, currentPage))

    /** Optimistic local edit, then message + DataItem to the phone. */
    fun editSettings(change: (WatchSettings) -> WatchSettings) {
        val next = _state.value.edit(System.currentTimeMillis(), change) ?: return
        _state.value = next
        val settings = next.pendingSettings ?: return
        persist(next)
        val msg = Protocol.settingsMessage(settings)
        send(msg)
        scope.launch {
            runCatching {
                val req = PutDataMapRequest.create(Protocol.WATCH_SETTINGS_PATH).apply {
                    dataMap.putString(Protocol.KEY_JSON, msg)
                    dataMap.putLong("updated_ms", settings.updatedMs)
                }.asPutDataRequest().setUrgent()
                dataClient.putDataItem(req).await()
            }.onFailure { Log.w(TAG, "settings DataItem failed", it) }
        }
    }

    override fun onDataChanged(events: DataEventBuffer) {
        // Extract before the buffer is released when this callback returns.
        events
            .filter { it.type == DataEvent.TYPE_CHANGED && it.dataItem.uri.path == Direct.PATH }
            .mapNotNull { DataMapItem.fromDataItem(it.dataItem).dataMap.getString(Protocol.KEY_JSON) }
            .lastOrNull()
            ?.let { json -> scope.launch { direct.apply(json) } }
        val json = events
            .filter { it.type == DataEvent.TYPE_CHANGED && it.dataItem.uri.path == Protocol.SNAPSHOT_PATH }
            .mapNotNull { DataMapItem.fromDataItem(it.dataItem).dataMap.getString(Protocol.KEY_JSON) }
            .lastOrNull() ?: return
        scope.launch { accept(json, fromCurrentItem = false) }
    }

    private suspend fun readCurrentSnapshot() {
        runCatching {
            val buffer = dataClient.getDataItems(("wear://*" + Protocol.SNAPSHOT_PATH).toUri()).await()
            val json = try {
                buffer.firstOrNull()?.let { DataMapItem.fromDataItem(it).dataMap.getString(Protocol.KEY_JSON) }
            } finally {
                buffer.release()
            }
            if (json != null) accept(json, fromCurrentItem = true)
        }.onFailure { Log.w(TAG, "reading current snapshot failed", it) }
    }

    /** Provisioning the phone stored while this app was not running. */
    private suspend fun readDirectProvision() {
        runCatching {
            val buffer = dataClient.getDataItems(("wear://*" + Direct.PATH).toUri()).await()
            val json = try {
                buffer.firstOrNull()?.let { DataMapItem.fromDataItem(it).dataMap.getString(Protocol.KEY_JSON) }
            } finally {
                buffer.release()
            }
            if (json != null) direct.apply(json)
        }.onFailure { Log.w(TAG, "reading direct-link provisioning failed", it) }
    }

    private fun accept(json: String, fromCurrentItem: Boolean) {
        val snap: Snapshot = Protocol.parseSnapshot(json) ?: return
        val current = _state.value
        // The stored DataItem may be the same snapshot we persisted earlier;
        // re-accepting it would reset its receive time and hide link loss.
        if (fromCurrentItem && current.snapshot?.seq == snap.seq) return
        val next = current.onSnapshot(snap, System.currentTimeMillis())
        _state.value = next
        prefs.edit { putString(KEY_SNAPSHOT, json) }
        persist(next)
    }

    private fun persist(s: WatchState) = prefs.edit {
        putLong(KEY_RECEIVED, s.receivedAtMs)
        val pending = s.pendingSettings
        if (pending == null) remove(KEY_PENDING)
        else putString(KEY_PENDING, Protocol.json.encodeToString(WatchSettings.serializer(), pending))
    }

    private fun loadPersisted(): WatchState {
        val snap = prefs.getString(KEY_SNAPSHOT, null)?.let(Protocol::parseSnapshot)
        val pending = prefs.getString(KEY_PENDING, null)?.let {
            runCatching { Protocol.json.decodeFromString(WatchSettings.serializer(), it) }.getOrNull()
        }
        return WatchState(snap, prefs.getLong(KEY_RECEIVED, 0L), pending)
    }

    private fun send(json: String) {
        scope.launch {
            runCatching {
                val nodes = capabilityClient
                    .getCapability(Protocol.PHONE_CAPABILITY, CapabilityClient.FILTER_REACHABLE).await().nodes
                val bytes = json.toByteArray(Charsets.UTF_8)
                for (node in nodes) messageClient.sendMessage(node.id, Protocol.WATCH_MESSAGE_PATH, bytes).await()
            }.onFailure { Log.w(TAG, "send failed", it) }
        }
    }

    companion object {
        private const val TAG = "GsPhoneLink"
        private const val KEY_SNAPSHOT = "snapshot_json"
        private const val KEY_RECEIVED = "received_ms"
        private const val KEY_PENDING = "pending_settings_json"

        // Holds only the application context.
        @SuppressLint("StaticFieldLeak")
        @Volatile private var instance: PhoneLink? = null
        fun get(context: Context): PhoneLink =
            instance ?: synchronized(this) { instance ?: PhoneLink(context).also { instance = it } }
    }
}
