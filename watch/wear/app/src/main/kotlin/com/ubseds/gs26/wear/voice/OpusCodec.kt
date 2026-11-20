package com.ubseds.gs26.wear.voice

import android.media.MediaCodec
import android.media.MediaFormat
import android.util.Log
import com.ubseds.gs26.wear.core.Voice
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * Opus through the platform codecs (Android 10+ ships `c2.android.opus.encoder`
 * and the decoder): 16 kHz mono, one packet per 20 ms frame, synchronous
 * MediaCodec use from the audio threads. The session falls back to PCM16
 * when either cannot be created.
 */
class OpusEncoder private constructor(private val codec: MediaCodec) {
    private var pts = 0L
    private val info = MediaCodec.BufferInfo()

    /** One 320-sample frame in, the packets the codec has ready out (normally one). */
    fun encode(pcm: ByteArray): List<ByteArray> {
        val out = ArrayList<ByteArray>(1)
        val inIndex = codec.dequeueInputBuffer(TIMEOUT_US)
        if (inIndex >= 0) {
            val buf = codec.getInputBuffer(inIndex)!!
            buf.clear(); buf.put(pcm)
            codec.queueInputBuffer(inIndex, 0, pcm.size, pts, 0)
            pts += Voice.FRAME_MS * 1000
        }
        while (true) {
            val outIndex = codec.dequeueOutputBuffer(info, 0)
            if (outIndex < 0) break
            if (info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG == 0 && info.size in 1..Voice.MAX_OPUS_BYTES) {
                val buf = codec.getOutputBuffer(outIndex)!!
                buf.position(info.offset); buf.limit(info.offset + info.size)
                out += ByteArray(info.size).also { buf.get(it) }
            }
            codec.releaseOutputBuffer(outIndex, false)
        }
        return out
    }

    fun release() = runCatching { codec.stop(); codec.release() }

    companion object {
        private const val TIMEOUT_US = 10_000L

        fun create(): OpusEncoder? = runCatching {
            val format = MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_OPUS, Voice.SAMPLE_RATE, 1).apply {
                setInteger(MediaFormat.KEY_BIT_RATE, 24_000)
                setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, Voice.FRAME_BYTES)
            }
            val codec = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_AUDIO_OPUS)
            try {
                codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
                codec.start()
            } catch (e: Exception) {
                codec.release(); throw e
            }
            OpusEncoder(codec)
        }.onFailure { Log.w("GsOpus", "no Opus encoder; using PCM16", it) }.getOrNull()
    }
}

/** One speaker's decoder; output resampled to 16 kHz mono PCM16 samples. */
class OpusDecoder private constructor(private val codec: MediaCodec) {
    private var pts = 0L
    private var outRate = 48_000
    private val info = MediaCodec.BufferInfo()

    fun decode(packet: ByteArray): ShortArray {
        val inIndex = codec.dequeueInputBuffer(TIMEOUT_US)
        if (inIndex >= 0) {
            val buf = codec.getInputBuffer(inIndex)!!
            buf.clear(); buf.put(packet)
            codec.queueInputBuffer(inIndex, 0, packet.size, pts, 0)
            pts += Voice.FRAME_MS * 1000
        }
        var samples = ShortArray(0)
        while (true) {
            val outIndex = codec.dequeueOutputBuffer(info, 0)
            if (outIndex == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                outRate = codec.outputFormat.getInteger(MediaFormat.KEY_SAMPLE_RATE)
                continue
            }
            if (outIndex < 0) break
            if (info.size > 0) {
                val buf = codec.getOutputBuffer(outIndex)!!.order(ByteOrder.LITTLE_ENDIAN)
                buf.position(info.offset); buf.limit(info.offset + info.size)
                val raw = ShortArray(info.size / 2).also { buf.asShortBuffer().get(it) }
                samples += resampleTo16k(raw, outRate)
            }
            codec.releaseOutputBuffer(outIndex, false)
        }
        return samples
    }

    fun release() = runCatching { codec.stop(); codec.release() }

    companion object {
        private const val TIMEOUT_US = 10_000L

        fun create(): OpusDecoder? = runCatching {
            val format = MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_OPUS, 48_000, 1).apply {
                setByteBuffer("csd-0", opusHead())
                setByteBuffer("csd-1", nanos(0)) // pre-skip
                setByteBuffer("csd-2", nanos(80_000_000)) // seek pre-roll
            }
            val codec = MediaCodec.createDecoderByType(MediaFormat.MIMETYPE_AUDIO_OPUS)
            try {
                codec.configure(format, null, null, 0)
                codec.start()
            } catch (e: Exception) {
                codec.release(); throw e
            }
            OpusDecoder(codec)
        }.onFailure { Log.w("GsOpus", "no Opus decoder", it) }.getOrNull()

        /** RFC 7845 identification header: mono, no pre-skip, 16 kHz input. */
        private fun opusHead(): ByteBuffer = ByteBuffer.allocate(19).order(ByteOrder.LITTLE_ENDIAN).apply {
            put("OpusHead".toByteArray(Charsets.US_ASCII)); put(1); put(1)
            putShort(0); putInt(Voice.SAMPLE_RATE); putShort(0); put(0)
            flip()
        }

        private fun nanos(value: Long): ByteBuffer =
            ByteBuffer.allocate(8).order(ByteOrder.nativeOrder()).putLong(value).apply { flip() }

        /** 48 kHz → 16 kHz by averaging each group of three (a simple low-pass); other rates nearest-sample. */
        fun resampleTo16k(raw: ShortArray, rate: Int): ShortArray = when (rate) {
            Voice.SAMPLE_RATE -> raw
            48_000 -> ShortArray(raw.size / 3) { i ->
                ((raw[3 * i] + raw[3 * i + 1] + raw[3 * i + 2]) / 3).toShort()
            }
            else -> {
                val n = (raw.size.toLong() * Voice.SAMPLE_RATE / rate).toInt()
                ShortArray(n) { i -> raw[(i.toLong() * rate / Voice.SAMPLE_RATE).toInt().coerceAtMost(raw.size - 1)] }
            }
        }
    }
}
