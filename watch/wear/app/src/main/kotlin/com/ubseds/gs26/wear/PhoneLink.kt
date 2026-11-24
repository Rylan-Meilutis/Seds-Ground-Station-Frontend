package com.ubseds.gs26.wear

import android.content.Context
import android.location.Location
import android.annotation.SuppressLint
import android.util.Log
import androidx.core.content.edit
import androidx.core.net.toUri
import com.google.android.gms.wearable.CapabilityClient
import com.google.android.gms.wearable.Asset
import com.google.android.gms.wearable.CapabilityInfo
import com.google.android.gms.wearable.DataClient
import com.google.android.gms.wearable.DataEvent
import com.google.android.gms.wearable.DataEventBuffer
import com.google.android.gms.wearable.DataMapItem
import com.google.android.gms.wearable.PutDataMapRequest
import com.google.android.gms.wearable.Wearable
import com.ubseds.gs26.wear.core.Direct
import com.ubseds.gs26.wear.core.Protocol
import com.ubseds.gs26.wear.core.Search
import com.ubseds.gs26.wear.core.SearchRecorder
import com.ubseds.gs26.wear.core.Snapshot
import com.ubseds.gs26.wear.core.WatchSettings
import com.ubseds.gs26.wear.core.WatchState
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
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

    private val _search = MutableStateFlow(Search.decode(prefs.getString(KEY_SEARCH, null)))

    /**
     * Recovery search (core `Search`): this watch user's own opt-in and the
     * fixes waiting for the phone (docs/watch-companion.md "Recovery search").
     */
    val search: StateFlow<SearchRecorder> = _search.asStateFlow()
    private var lastShareSentMs = 0L

    /** Every 2 s while the app runs: follow the phone, record a fix, send the buffer. */
    fun searchTick(fix: Location?) {
        val info = _state.value.snapshot?.search
        var next = _search.value.apply(info)
        val now = System.currentTimeMillis()
        if (fix != null && now - fix.time < 30_000L) {
            val accuracy = if (fix.hasAccuracy()) fix.accuracy.toDouble() else null
            next = next.record(fix.latitude, fix.longitude, accuracy, fix.time, info)
        }
        next.nextMessage()?.let { send(it) }
        // The opt-in goes only to a reachable phone: repeat it until the
        // phone reports the station's answer.
        if (next.wantsToShare && info?.active == true && !info.sharing && now - lastShareSentMs > 10_000L) {
            lastShareSentMs = now
            send(Search.shareMessage(true))
        }
        saveSearch(next)
    }

    /** The watch user turned sharing on or off. Off stops at once and discards the buffer. */
    fun shareSearch(on: Boolean) {
        saveSearch(if (on) _search.value.copy(wantsToShare = true) else _search.value.stop())
        lastShareSentMs = System.currentTimeMillis()
        send(Search.shareMessage(on))
    }

    private fun saveSearch(next: SearchRecorder) {
        if (next == _search.value) return
        _search.value = next
        prefs.edit { putString(KEY_SEARCH, Search.encode(next)) }
    }

    /** Page id currently on screen, reported in `activity` messages. */
    @Volatile var currentPage: String? = null

    /** Direct link to the station, provisioned by the phone. */
    val direct: DirectLink = DirectLink.get(app)

    /** Map background tiles (from the phone, or the station in direct mode). */
    val tiles: TileCache = TileCache.get(app)
    private var reportJob: Job? = null

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

    private val tileMisses = HashMap<String, Long>()

    /**
     * Direct mode only (the phone's data is not fresh and the link is
     * provisioned): fetches up to [MAX_DIRECT_TILES] missing map tiles for the
     * current view from the station. A tile the station did not serve is not
     * asked for again for a minute.
     */
    suspend fun fetchMissingTiles(keys: List<String>) {
        val s = _state.value
        if (!s.settings.map.enabled) return
        val provision = direct.state.value.provision ?: return
        val now = System.currentTimeMillis()
        val phoneAge = s.snapshot?.let { (now - s.receivedAtMs).coerceAtLeast(0) }
        if (!Direct.shouldPoll(s.snapshot?.mode, phoneAge, Direct.phoneFreshMs(s.settings.power), foreground = true,
                configured = provision.configured)) return
        val wanted = keys.filter { !tiles.has(it) && (tileMisses[it]?.let { t -> now - t > 60_000 } ?: true) }
            .take(MAX_DIRECT_TILES)
        for (key in wanted) {
            val bytes = direct.fetchTile(key)
            if (bytes == null) { tileMisses[key] = System.currentTimeMillis(); continue }
            withIo { tiles.put(key, bytes, s.settings.map.capBytes) }
        }
    }

    fun start() {
        dataClient.addListener(this)
        capabilityClient.addListener(this, Protocol.PHONE_CAPABILITY)
        scope.launch { readCurrentSnapshot() }
        scope.launch { readDirectProvision() }
        scope.launch { readMapItems() }
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
        }.onSuccess {
            val reachable = it.isNotEmpty()
            if (reachable && !_phoneReachable.value) scheduleReport(2_000)
            _phoneReachable.value = reachable
        }
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
        val changed = events.filter { it.type == DataEvent.TYPE_CHANGED }.map { it.dataItem }
        val manifest = changed.filter { it.uri.path == Protocol.MAP_PATH }
            .mapNotNull { DataMapItem.fromDataItem(it).dataMap.getString(Protocol.KEY_JSON) }
            .lastOrNull()
        val tileAssets = changed.filter { it.uri.path.orEmpty().startsWith(Protocol.TILE_PATH_PREFIX) }
            .mapNotNull { item ->
                val map = DataMapItem.fromDataItem(item).dataMap
                val key = item.uri.path.orEmpty().removePrefix(Protocol.TILE_PATH_PREFIX)
                map.getAsset("tile")?.let { key to it }
            }
        if (manifest != null || tileAssets.isNotEmpty()) {
            scope.launch {
                manifest?.let(tiles::applyManifest)
                importTiles(tileAssets)
            }
        }
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

    /** Map manifest and tiles the phone sent while this app was closed. */
    private suspend fun readMapItems() {
        runCatching {
            val buffer = dataClient.getDataItems(("wear://*" + Protocol.MAP_PATH).toUri()).await()
            val json = try {
                buffer.firstOrNull()?.let { DataMapItem.fromDataItem(it).dataMap.getString(Protocol.KEY_JSON) }
            } finally {
                buffer.release()
            }
            json?.let(tiles::applyManifest)
            val items = dataClient.getDataItems(("wear://*" + Protocol.TILE_PATH_PREFIX).toUri(), DataClient.FILTER_PREFIX).await()
            val assets = try {
                items.mapNotNull { item ->
                    val key = item.uri.path.orEmpty().removePrefix(Protocol.TILE_PATH_PREFIX)
                    DataMapItem.fromDataItem(item).dataMap.getAsset("tile")?.let { key to it }
                }
            } finally {
                items.release()
            }
            importTiles(assets)
        }.onFailure { Log.w(TAG, "reading map tiles failed", it) }
        scheduleReport(0)
    }

    /** Copies tile assets into the cache, then reports what the watch holds. */
    private suspend fun importTiles(assets: List<Pair<String, Asset>>) {
        for ((key, asset) in assets) {
            if (tiles.has(key)) continue
            runCatching {
                val bytes = dataClient.getFdForAsset(asset).await().inputStream.use { it.readBytes() }
                withIo { tiles.put(key, bytes, _state.value.settings.map.capBytes) }
            }.onFailure { Log.w(TAG, "tile $key could not be read", it) }
        }
        if (assets.isNotEmpty()) scheduleReport(REPORT_DELAY_MS)
    }

    private suspend fun <T> withIo(block: () -> T): T = kotlinx.coroutines.withContext(Dispatchers.IO) { block() }

    /** Tells the phone which tiles the watch holds (debounced). */
    fun scheduleReport(delayMs: Long) {
        reportJob?.cancel()
        reportJob = scope.launch {
            delay(delayMs)
            send(tiles.report())
        }
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
        private const val KEY_SEARCH = "search_recorder_json"
        private const val REPORT_DELAY_MS = 5_000L
        private const val MAX_DIRECT_TILES = 6

        // Holds only the application context.
        @SuppressLint("StaticFieldLeak")
        @Volatile private var instance: PhoneLink? = null
        fun get(context: Context): PhoneLink =
            instance ?: synchronized(this) { instance ?: PhoneLink(context).also { instance = it } }
    }
}
