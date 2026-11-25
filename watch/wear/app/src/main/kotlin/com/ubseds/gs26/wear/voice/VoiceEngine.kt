package com.ubseds.gs26.wear.voice

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.util.Log
import androidx.core.content.ContextCompat
import com.google.android.gms.wearable.CapabilityClient
import com.google.android.gms.wearable.MessageClient
import com.google.android.gms.wearable.MessageEvent
import com.google.android.gms.wearable.Wearable
import com.ubseds.gs26.wear.PhoneLink
import com.ubseds.gs26.wear.core.DirectProvision
import com.ubseds.gs26.wear.core.Haptic
import com.ubseds.gs26.wear.core.Protocol
import com.ubseds.gs26.wear.core.PttMachine
import com.ubseds.gs26.wear.core.PttOutput
import com.ubseds.gs26.wear.core.RelayIn
import com.ubseds.gs26.wear.core.RosterPeer
import com.ubseds.gs26.wear.core.ServerMessage
import com.ubseds.gs26.wear.core.SocketOpener
import com.ubseds.gs26.wear.core.Voice
import com.ubseds.gs26.wear.core.VoiceFrames
import com.ubseds.gs26.wear.core.VoiceMixer
import com.ubseds.gs26.wear.core.VoiceRelay
import com.ubseds.gs26.wear.core.VoiceSetting
import com.ubseds.gs26.wear.core.VoiceSocket
import com.ubseds.gs26.wear.core.VoiceTransport
import com.ubseds.gs26.wear.core.WsMessage
import com.ubseds.gs26.wear.core.reconnectDelayMs
import com.ubseds.gs26.wear.core.startedTalking
import com.ubseds.gs26.wear.core.talkers
import com.ubseds.gs26.wear.core.voiceRetryable
import com.ubseds.gs26.wear.core.voiceTransport
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.tasks.await
import kotlinx.coroutines.withTimeoutOrNull
import java.io.IOException
import java.net.InetSocketAddress
import java.net.Socket
import java.security.SecureRandom
import java.security.cert.X509Certificate
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLSocket
import javax.net.ssl.X509TrustManager
import kotlin.concurrent.thread

/** What the voice page shows. */
data class VoiceUi(
    val joined: Boolean = false,
    val connected: Boolean = false,
    /** `Connecting…`, `Connected`, `Reconnecting in 3 s`, … */
    val status: String = "Voice off",
    /** `direct` or `phone` while connected. */
    val path: String? = null,
    val channel: String = "Crew",
    val talkers: List<String> = emptyList(),
    val peers: Int = 0,
    val talking: Boolean = false,
    val held: Boolean = false,
    /** Talking, but the station's talker cap is full: queued, not on air. */
    val waiting: Boolean = false,
    val error: String? = null,
    val codec: String = Voice.CODEC_OPUS,
    val radioAvailable: Boolean = false,
)

/**
 * The watch's crew-voice session (docs/watch-companion.md "Voice"), kept
 * alive by [VoiceService]. Connects straight to the station with the watch
 * token (core `voiceTransport`), else relays through the phone; captures
 * 16 kHz mono while joined and sends only while push-to-talk is on; plays
 * every speaker through the core [VoiceMixer].
 */
class VoiceEngine private constructor(context: Context) {
    private val app = context.applicationContext
    private val link = PhoneLink.get(app)
    private val audio = app.getSystemService(AudioManager::class.java)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val _ui = MutableStateFlow(VoiceUi())
    val ui: StateFlow<VoiceUi> = _ui.asStateFlow()

    private val ptt = PttMachine(toggle = false)
    private val pttLock = Any()
    private val mixer = VoiceMixer()
    private val incoming = Channel<Incoming>(Channel.UNLIMITED)
    private var session: Job? = null
    @Volatile private var transport: Transport? = null
    @Volatile private var codec = Voice.CODEC_OPUS
    @Volatile private var running = false
    @Volatile private var selfId: Long? = null
    @Volatile private var roster: List<RosterPeer> = emptyList()
    @Volatile private var volumeOverride: Int? = null
    private var encoder: OpusEncoder? = null
    private val decoders = HashMap<Long, OpusDecoder>()
    private val leftovers = HashMap<Long, ShortArray>()
    private var directFailures = 0
    private var recordThread: Thread? = null
    private var playThread: Thread? = null

    private val settings: VoiceSetting get() = link.state.value.settings.voice
    val volume: Int get() = volumeOverride ?: settings.volume

    fun micGranted() = ContextCompat.checkSelfPermission(app, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED

    // --- Commands from the UI / service --------------------------------------

    /** Starts the session (called by [VoiceService] once in the foreground). */
    fun join() {
        if (running) return
        if (!micGranted()) {
            _ui.update { it.copy(error = "Allow the microphone in the watch's Settings → Apps → GS26") }
            return
        }
        running = true
        encoder = OpusEncoder.create()
        val probe = if (encoder != null) OpusDecoder.create() else null
        probe?.release()
        codec = if (encoder != null && probe != null) Voice.CODEC_OPUS else Voice.CODEC_PCM16
        if (codec == Voice.CODEC_PCM16) { encoder?.release(); encoder = null }
        synchronized(pttLock) { ptt.toggle = settings.toggle }
        _ui.value = VoiceUi(joined = true, status = "Connecting…", channel = settings.channelLabel, codec = codec)
        startAudio()
        session = scope.launch { runSession() }
    }

    fun leave() {
        if (!running) return
        running = false
        apply(synchronized(pttLock) { ptt.event("leave") })
        session?.cancel()
        transport?.close()
        transport = null
        stopAudio()
        encoder?.release(); encoder = null
        synchronized(decoders) { decoders.values.forEach { it.release() }; decoders.clear(); leftovers.clear() }
        mixer.clear()
        _ui.value = VoiceUi(channel = settings.channelLabel)
    }

    /** Push-to-talk touch: `down` or `up`. */
    fun ptt(event: String) {
        if (!running) { haptic(Haptic.DENIED); return }
        synchronized(pttLock) { ptt.toggle = settings.toggle }
        apply(synchronized(pttLock) { ptt.event(event, System.currentTimeMillis()) })
    }

    /** Crown / rotary volume (applied at once; the page saves it to the settings). */
    fun setVolume(pct: Int) { volumeOverride = pct.coerceIn(0, VoiceSetting.MAX_VOLUME) }
    fun clearVolumeOverride() { volumeOverride = null }

    private fun apply(out: PttOutput) {
        out.send?.let { sendText(VoiceFrames.control("transmitting", it)) }
        out.haptic?.let { if (settings.haptics) haptic(it) }
        _ui.update { it.copy(talking = out.talking, waiting = it.waiting && out.talking) }
    }

    // --- Session ------------------------------------------------------------

    private sealed class Incoming {
        class Text(val text: String) : Incoming()
        class Frame(val data: ByteArray) : Incoming()
        class Closed(val reason: String) : Incoming()
    }

    private interface Transport {
        val path: String
        fun sendText(text: String)
        fun sendAudio(frame: ByteArray)
        fun close()
    }

    private suspend fun runSession() {
        var attempt = 0
        val random = SecureRandom()
        while (running && scope.isActive) {
            val provision = link.direct.state.value.provision
            val granted = provision?.configured == true && provision.voice
            val reachable = link.phoneReachable.value
            val choice = voiceTransport(granted, directFailures, reachable)
            var serverError: String? = null
            if (choice == VoiceTransport.NONE || provision == null) {
                _ui.update { it.copy(connected = false, status = "Voice not enabled", error = "Turn on Voice in the phone's Watch settings (needs the voice permission and Direct link).") }
                delay(5_000)
                continue
            }
            while (incoming.tryReceive().isSuccess) { /* drop stale messages */ }
            _ui.update { it.copy(status = if (choice == VoiceTransport.PHONE) "Connecting via phone…" else "Connecting to ${provision.address}…", error = null) }
            val t = try {
                if (choice == VoiceTransport.DIRECT) openDirect(provision) else openRelay()
            } catch (e: IOException) {
                if (choice == VoiceTransport.DIRECT) directFailures++
                Log.w(TAG, "voice connect failed", e)
                _ui.update { it.copy(error = e.message ?: "cannot connect") }
                null
            }
            if (t != null) {
                transport = t
                serverError = runConnected(t)
                t.close()
                transport = null
            }
            apply(synchronized(pttLock) { ptt.event("disconnect") })
            selfId = null
            roster = emptyList()
            mixer.clear()
            if (!running) break
            if (!voiceRetryable(serverError)) {
                _ui.update { it.copy(connected = false, path = null, status = "Voice refused", error = serverError) }
                break
            }
            attempt = if (t != null && serverError == null) 1 else attempt + 1
            val wait = reconnectDelayMs(attempt, random.nextDouble())
            _ui.update { it.copy(connected = false, path = null, talkers = emptyList(), status = "Reconnecting in ${(wait + 999) / 1000} s") }
            delay(wait)
        }
    }

    /** Runs one connection until it drops; returns the station's refusal, if any. */
    private suspend fun runConnected(t: Transport): String? {
        var lastPing = System.currentTimeMillis()
        var lastHeard = System.currentTimeMillis()
        var serverError: String? = null
        var joined = false
        while (running) {
            val msg = withTimeoutOrNull(500) { incoming.receive() }
            val now = System.currentTimeMillis()
            when (msg) {
                is Incoming.Text -> { lastHeard = now; handleText(msg.text)?.let { serverError = it; return serverError } ; if (selfId != null && !joined) { joined = true; if (t.path == "direct") directFailures = 0 } }
                is Incoming.Frame -> { lastHeard = now; handleFrame(msg.data) }
                is Incoming.Closed -> return serverError
                null -> {}
            }
            if (synchronized(pttLock) { ptt.timedOut(now) }) apply(synchronized(pttLock) { ptt.event("timeout") })
            if (now - lastPing >= Voice.PING_MS) {
                lastPing = now
                runCatching { t.sendText(VoiceFrames.ping()) }.onFailure { return serverError }
            }
            if (!joined && now - lastHeard > 10_000) {
                if (t.path == "direct") directFailures++
                return null
            }
            if (now - lastHeard > 45_000) return null
        }
        return serverError
    }

    /** Handles a station message; returns an error text that ends the session. */
    private fun handleText(text: String): String? {
        when (val m = ServerMessage.parse(text)) {
            is ServerMessage.Joined -> {
                selfId = m.id
                codec = m.codec
                apply(synchronized(pttLock) { ptt.event("ready") })
                _ui.update { it.copy(connected = true, path = transport?.path, status = "Connected", error = null, channel = settings.channelLabel) }
            }
            is ServerMessage.Roster -> {
                val started = startedTalking(roster, m.peers, selfId)
                val gone = roster.map { it.id }.toSet() - m.peers.map { it.id }.toSet()
                roster = m.peers
                gone.forEach { forget(it) }
                if (started.isNotEmpty() && settings.talkerHaptics) haptic(null)
                _ui.update { it.copy(talkers = talkers(m.peers, selfId), peers = m.peers.size) }
            }
            is ServerMessage.Handoff -> {
                apply(synchronized(pttLock) { ptt.event(if (m.held) "held" else "unheld") })
                _ui.update { it.copy(held = m.held) }
            }
            is ServerMessage.Floor -> _ui.update { it.copy(waiting = m.state == "waiting") }
            is ServerMessage.Error -> return m.message
            is ServerMessage.RadioLink -> {
                _ui.update { it.copy(radioAvailable = m.available) }
                if (m.available && settings.radio) sendText(VoiceFrames.control("radio", true))
            }
            else -> {}
        }
        return null
    }

    private fun forget(speaker: Long) {
        mixer.remove(speaker)
        synchronized(decoders) { decoders.remove(speaker)?.release(); leftovers.remove(speaker) }
    }

    private fun handleFrame(frame: ByteArray) {
        val c = codec
        if (!VoiceFrames.downlinkValid(frame, c)) return
        val speaker = VoiceFrames.speaker(frame) ?: return
        val payload = VoiceFrames.payload(frame)
        if (c == Voice.CODEC_PCM16) { mixer.push(speaker, VoiceFrames.decodePcm(payload)); return }
        synchronized(decoders) {
            val dec = decoders.getOrPut(speaker) { OpusDecoder.create() ?: return }
            var samples = (leftovers.remove(speaker) ?: ShortArray(0)) + dec.decode(payload)
            while (samples.size >= Voice.FRAME_SAMPLES) {
                mixer.push(speaker, samples.copyOfRange(0, Voice.FRAME_SAMPLES))
                samples = samples.copyOfRange(Voice.FRAME_SAMPLES, samples.size)
            }
            if (samples.isNotEmpty()) leftovers[speaker] = samples
        }
    }

    private fun sendText(text: String) {
        val t = transport ?: return
        scope.launch { runCatching { t.sendText(text) } }
    }

    // --- Direct transport -----------------------------------------------------

    private fun openDirect(p: DirectProvision): Transport {
        val ws = VoiceSocket(Voice.socketUrl(p.base), opener(p))
        ws.connect()
        ws.sendText(VoiceFrames.authMessage(p.token!!, codec))
        thread(name = "gs-voice-rx", isDaemon = true) {
            try {
                while (true) {
                    when (val m = ws.receive()) {
                        is WsMessage.Text -> incoming.trySend(Incoming.Text(m.text))
                        is WsMessage.Binary -> incoming.trySend(Incoming.Frame(m.data))
                        is WsMessage.Close -> { incoming.trySend(Incoming.Closed(m.reason)); break }
                    }
                }
            } catch (e: IOException) {
                if (!ws.closed) incoming.trySend(Incoming.Closed(e.message ?: "connection lost"))
            }
        }
        return object : Transport {
            override val path = "direct"
            override fun sendText(text: String) = ws.sendText(text)
            override fun sendAudio(frame: ByteArray) = ws.sendBinary(frame)
            override fun close() = ws.close()
        }
    }

    /** The phone's "skip TLS verification" for this station only, as for the status polls. */
    @SuppressLint("CustomX509TrustManager", "TrustAllX509TrustManager")
    private fun opener(p: DirectProvision): SocketOpener {
        if (!p.skipTls) return VoiceSocket.DefaultOpener
        val stationHost = p.address.substringBefore(':')
        return SocketOpener { host, port, tls, timeout ->
            if (!tls || host != stationHost) return@SocketOpener VoiceSocket.DefaultOpener.open(host, port, tls, timeout)
            val trustAll = object : X509TrustManager {
                override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) {}
                override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {}
                override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
            }
            val ctx = SSLContext.getInstance("TLS").apply { init(null, arrayOf(trustAll), null) }
            val raw = Socket().apply { connect(InetSocketAddress(host, port), timeout) }
            (ctx.socketFactory.createSocket(raw, host, port, true) as SSLSocket).apply { startHandshake() }
        }
    }

    // --- Phone relay ------------------------------------------------------------

    private val relayListener = MessageClient.OnMessageReceivedListener { event: MessageEvent ->
        if (event.path != Protocol.VOICE_PATH) return@OnMessageReceivedListener
        when (val m = VoiceRelay.parse(String(event.data, Charsets.UTF_8))) {
            is RelayIn.Server -> {
                m.texts.forEach { incoming.trySend(Incoming.Text(it)) }
                m.frames.forEach { incoming.trySend(Incoming.Frame(it)) }
            }
            is RelayIn.Closed -> incoming.trySend(Incoming.Closed(m.reason))
            null -> {}
        }
    }

    private suspend fun openRelay(): Transport {
        val messages = Wearable.getMessageClient(app)
        val nodes = Wearable.getCapabilityClient(app)
            .getCapability(Protocol.PHONE_CAPABILITY, CapabilityClient.FILTER_REACHABLE).await().nodes
        val node = nodes.firstOrNull { it.isNearby } ?: nodes.firstOrNull() ?: throw IOException("phone not reachable")
        messages.addListener(relayListener).await()
        val send: (String) -> Unit = { json ->
            messages.sendMessage(node.id, Protocol.WATCH_VOICE_PATH, json.toByteArray(Charsets.UTF_8))
        }
        send(VoiceRelay.open(codec))
        val batch = ArrayList<ByteArray>()
        return object : Transport {
            override val path = "phone"
            override fun sendText(text: String) {
                // Audio still batched goes out before a control (e.g. the end of a talk).
                val pending = synchronized(batch) { ArrayList(batch).also { batch.clear() } }
                if (pending.isNotEmpty()) VoiceRelay.audio(pending).forEach(send)
                VoiceRelay.control(text)?.let(send)
            }
            override fun sendAudio(frame: ByteArray) {
                val out = synchronized(batch) {
                    batch += frame
                    if (batch.size < RELAY_BATCH) null else ArrayList(batch).also { batch.clear() }
                }
                out?.let { VoiceRelay.audio(it).forEach(send) }
            }
            override fun close() {
                runCatching { send(VoiceRelay.close()) }
                messages.removeListener(relayListener)
            }
        }
    }

    // --- Audio --------------------------------------------------------------------

    @SuppressLint("MissingPermission") // checked in join()
    private fun startAudio() {
        audio.mode = AudioManager.MODE_IN_COMMUNICATION
        applyRoute()
        val attrs = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
            .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build()
        val fmtOut = AudioFormat.Builder().setSampleRate(Voice.SAMPLE_RATE)
            .setChannelMask(AudioFormat.CHANNEL_OUT_MONO).setEncoding(AudioFormat.ENCODING_PCM_16BIT).build()
        val outMin = AudioTrack.getMinBufferSize(Voice.SAMPLE_RATE, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_16BIT)
        val track = AudioTrack.Builder().setAudioAttributes(attrs).setAudioFormat(fmtOut)
            .setBufferSizeInBytes(maxOf(outMin, Voice.FRAME_BYTES * 4)).setTransferMode(AudioTrack.MODE_STREAM).build()
        val inMin = AudioRecord.getMinBufferSize(Voice.SAMPLE_RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        val record = AudioRecord(MediaRecorder.AudioSource.VOICE_COMMUNICATION, Voice.SAMPLE_RATE,
            AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT, maxOf(inMin, Voice.FRAME_BYTES * 4))
        track.play()
        playThread = thread(name = "gs-voice-play", isDaemon = true) {
            try {
                while (running) {
                    // Blocking write paces the loop at real time (20 ms per frame).
                    val frame = mixer.pull(volume)
                    track.write(frame, 0, frame.size)
                }
            } finally {
                runCatching { track.stop(); track.release() }
            }
        }
        recordThread = thread(name = "gs-voice-mic", isDaemon = true) {
            val buf = ShortArray(Voice.FRAME_SAMPLES)
            try {
                record.startRecording()
                while (running) {
                    var got = 0
                    while (got < buf.size && running) {
                        val n = record.read(buf, got, buf.size - got)
                        if (n <= 0) break
                        got += n
                    }
                    if (got < buf.size) continue
                    val talking = synchronized(pttLock) { ptt.talking }
                    val t = transport
                    if (!talking || t == null) continue
                    val pcm = VoiceFrames.encodePcm(buf)
                    val frames = if (codec == Voice.CODEC_OPUS) encoder?.encode(pcm).orEmpty() else listOf(pcm)
                    for (f in frames) if (VoiceFrames.uplinkValid(codec, f.size)) runCatching { t.sendAudio(f) }
                }
            } catch (e: RuntimeException) {
                Log.w(TAG, "microphone failed", e)
            } finally {
                runCatching { record.stop(); record.release() }
            }
        }
    }

    private fun stopAudio() {
        recordThread?.join(500); playThread?.join(500)
        recordThread = null; playThread = null
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) audio.clearCommunicationDevice()
        audio.mode = AudioManager.MODE_NORMAL
    }

    /** Route setting: the watch speaker, or a connected Bluetooth headset. */
    fun applyRoute() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) {
            @Suppress("DEPRECATION")
            audio.isSpeakerphoneOn = settings.route == VoiceSetting.ROUTE_SPEAKER
            return
        }
        val devices = audio.availableCommunicationDevices
        val headset = devices.firstOrNull {
            it.type == AudioDeviceInfo.TYPE_BLE_HEADSET || it.type == AudioDeviceInfo.TYPE_BLUETOOTH_SCO
        }
        val speaker = devices.firstOrNull { it.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER }
        val chosen = when (settings.route) {
            VoiceSetting.ROUTE_SPEAKER -> speaker
            else -> headset ?: speaker
        }
        if (chosen != null) audio.setCommunicationDevice(chosen) else audio.clearCommunicationDevice()
    }

    // --- Haptics ------------------------------------------------------------------

    /** Start/stop/denied feedback; null is the "someone started talking" tick. */
    private fun haptic(kind: Haptic?) {
        val vibrator = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            app.getSystemService(VibratorManager::class.java)?.defaultVibrator
        } else {
            @Suppress("DEPRECATION") app.getSystemService(Vibrator::class.java)
        } ?: return
        val effect = when (kind) {
            Haptic.START -> VibrationEffect.EFFECT_CLICK
            Haptic.STOP -> VibrationEffect.EFFECT_TICK
            Haptic.DENIED -> VibrationEffect.EFFECT_DOUBLE_CLICK
            null -> VibrationEffect.EFFECT_HEAVY_CLICK
        }
        runCatching { vibrator.vibrate(VibrationEffect.createPredefined(effect)) }
    }

    companion object {
        private const val TAG = "GsVoice"
        /** Relay: frames per watch message (100 ms). */
        private const val RELAY_BATCH = 5

        @SuppressLint("StaticFieldLeak")
        @Volatile private var instance: VoiceEngine? = null
        fun get(context: Context): VoiceEngine =
            instance ?: synchronized(this) { instance ?: VoiceEngine(context).also { instance = it } }
    }
}
