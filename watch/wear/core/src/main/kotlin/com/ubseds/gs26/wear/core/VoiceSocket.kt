package com.ubseds.gs26.wear.core

import java.io.BufferedInputStream
import java.io.ByteArrayOutputStream
import java.io.EOFException
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.net.URI
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.Base64
import javax.net.ssl.SSLSocket
import javax.net.ssl.SSLSocketFactory

/** One message read from a [VoiceSocket]. */
sealed class WsMessage {
    data class Text(val text: String) : WsMessage()
    class Binary(val data: ByteArray) : WsMessage()
    data class Close(val code: Int, val reason: String) : WsMessage()
}

/** Opens the TCP (ws) or TLS (wss) connection; the app supplies the skip-TLS one for a trusted station. */
fun interface SocketOpener {
    fun open(host: String, port: Int, tls: Boolean, connectTimeoutMs: Int): Socket
}

/**
 * Minimal RFC 6455 client for the station's voice socket: no extensions,
 * masked client frames, ping answered automatically, fragmented messages
 * reassembled up to [maxMessageBytes]. Blocking; one reader thread calls
 * [receive], any thread may send.
 */
class VoiceSocket(
    private val url: String,
    private val opener: SocketOpener = DefaultOpener,
    private val connectTimeoutMs: Int = 6_000,
    private val readTimeoutMs: Int = 40_000,
    private val maxMessageBytes: Int = 64 * 1024,
) {
    private var socket: Socket? = null
    private lateinit var input: InputStream
    private lateinit var output: OutputStream
    private val writeLock = Any()
    private val random = SecureRandom()
    @Volatile var closed = false; private set

    /** Connects and completes the opening handshake; throws [IOException] on failure. */
    fun connect() {
        val uri = URI(url)
        val tls = when (uri.scheme) {
            "wss" -> true
            "ws" -> false
            else -> throw IOException("not a WebSocket URL: $url")
        }
        val host = uri.host ?: throw IOException("no host in $url")
        val port = if (uri.port > 0) uri.port else if (tls) 443 else 80
        val s = opener.open(host, port, tls, connectTimeoutMs)
        s.soTimeout = readTimeoutMs
        s.tcpNoDelay = true
        socket = s
        input = BufferedInputStream(s.getInputStream())
        output = s.getOutputStream()
        val key = Base64.getEncoder().encodeToString(ByteArray(16).also(random::nextBytes))
        val path = (uri.rawPath?.ifEmpty { "/" } ?: "/") + (uri.rawQuery?.let { "?$it" } ?: "")
        val hostHeader = if (uri.port > 0) "$host:${uri.port}" else host
        val request = "GET $path HTTP/1.1\r\nHost: $hostHeader\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n" +
            "Sec-WebSocket-Key: $key\r\nSec-WebSocket-Version: 13\r\nUser-Agent: GS26-Wear\r\n\r\n"
        output.write(request.toByteArray(Charsets.ISO_8859_1))
        output.flush()
        val status = readLine(input)
        if (!status.startsWith("HTTP/1.1 101")) {
            close()
            throw IOException("voice socket refused: ${status.ifEmpty { "no reply" }}")
        }
        val headers = HashMap<String, String>()
        while (true) {
            val line = readLine(input)
            if (line.isEmpty()) break
            val i = line.indexOf(':')
            if (i > 0) headers[line.substring(0, i).trim().lowercase()] = line.substring(i + 1).trim()
        }
        if (headers["sec-websocket-accept"] != acceptKey(key)) {
            close()
            throw IOException("voice socket handshake failed")
        }
    }

    fun sendText(text: String) = send(OP_TEXT, text.toByteArray(Charsets.UTF_8))
    fun sendBinary(data: ByteArray) = send(OP_BINARY, data)
    fun sendPing() = send(OP_PING, ByteArray(0))

    private fun send(opcode: Int, payload: ByteArray) {
        val frame = encodeFrame(opcode, payload, ByteArray(4).also(random::nextBytes))
        synchronized(writeLock) {
            if (closed) throw IOException("voice socket closed")
            output.write(frame)
            output.flush()
        }
    }

    /** Next text, binary or close message; answers pings; throws on I/O errors and timeouts. */
    fun receive(): WsMessage {
        val buffer = ByteArrayOutputStream()
        var messageOp = -1
        while (true) {
            val f = readFrame(input, maxMessageBytes)
            when (f.opcode) {
                OP_PING -> runCatching { send(OP_PONG, f.payload) }
                OP_PONG -> {}
                OP_CLOSE -> {
                    val code = if (f.payload.size >= 2) ((f.payload[0].toInt() and 0xff) shl 8) or (f.payload[1].toInt() and 0xff) else 1005
                    val reason = if (f.payload.size > 2) String(f.payload, 2, f.payload.size - 2, Charsets.UTF_8) else ""
                    runCatching { synchronized(writeLock) { if (!closed) { output.write(encodeFrame(OP_CLOSE, f.payload.copyOf(minOf(2, f.payload.size)), ByteArray(4).also(random::nextBytes))); output.flush() } } }
                    close()
                    return WsMessage.Close(code, reason)
                }
                OP_TEXT, OP_BINARY, OP_CONTINUATION -> {
                    if (f.opcode == OP_CONTINUATION) {
                        if (messageOp < 0) throw IOException("unexpected continuation frame")
                    } else {
                        if (messageOp >= 0) throw IOException("interleaved message")
                        messageOp = f.opcode
                    }
                    if (buffer.size() + f.payload.size > maxMessageBytes) throw IOException("voice message too large")
                    buffer.write(f.payload)
                    if (f.fin) {
                        val data = buffer.toByteArray()
                        return if (messageOp == OP_TEXT) WsMessage.Text(String(data, Charsets.UTF_8)) else WsMessage.Binary(data)
                    }
                }
                else -> throw IOException("unknown opcode ${f.opcode}")
            }
        }
    }

    /** Sends a close frame (best effort) and closes the connection. */
    fun close(code: Int = 1000) {
        val s = socket ?: return
        synchronized(writeLock) {
            if (!closed) {
                runCatching {
                    output.write(encodeFrame(OP_CLOSE, byteArrayOf((code shr 8).toByte(), code.toByte()), ByteArray(4).also(random::nextBytes)))
                    output.flush()
                }
            }
            closed = true
        }
        runCatching { s.close() }
    }

    class Frame(val fin: Boolean, val opcode: Int, val payload: ByteArray)

    companion object {
        const val OP_CONTINUATION = 0x0
        const val OP_TEXT = 0x1
        const val OP_BINARY = 0x2
        const val OP_CLOSE = 0x8
        const val OP_PING = 0x9
        const val OP_PONG = 0xA
        private const val GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

        val DefaultOpener = SocketOpener { host, port, tls, timeout ->
            if (tls) {
                val raw = Socket().apply { connect(InetSocketAddress(host, port), timeout) }
                val ssl = (SSLSocketFactory.getDefault() as SSLSocketFactory).createSocket(raw, host, port, true) as SSLSocket
                ssl.sslParameters = ssl.sslParameters.apply { endpointIdentificationAlgorithm = "HTTPS" }
                ssl.startHandshake()
                ssl
            } else {
                Socket().apply { connect(InetSocketAddress(host, port), timeout) }
            }
        }

        fun acceptKey(key: String): String = Base64.getEncoder().encodeToString(
            MessageDigest.getInstance("SHA-1").digest((key + GUID).toByteArray(Charsets.ISO_8859_1)),
        )

        /** One final frame; [mask] (4 bytes) for client frames, null for server frames. */
        fun encodeFrame(opcode: Int, payload: ByteArray, mask: ByteArray?, fin: Boolean = true): ByteArray {
            val out = ByteArrayOutputStream(payload.size + 14)
            out.write((if (fin) 0x80 else 0) or (opcode and 0x0f))
            val maskBit = if (mask != null) 0x80 else 0
            when {
                payload.size < 126 -> out.write(maskBit or payload.size)
                payload.size <= 0xffff -> {
                    out.write(maskBit or 126)
                    out.write(payload.size shr 8); out.write(payload.size and 0xff)
                }
                else -> {
                    out.write(maskBit or 127)
                    val len = payload.size.toLong()
                    for (shift in 56 downTo 0 step 8) out.write(((len shr shift) and 0xff).toInt())
                }
            }
            if (mask != null) {
                out.write(mask)
                for (i in payload.indices) out.write(payload[i].toInt() xor mask[i and 3].toInt())
            } else {
                out.write(payload)
            }
            return out.toByteArray()
        }

        /** Reads one frame (masked or not); refuses payloads over [max]. */
        fun readFrame(input: InputStream, max: Int): Frame {
            val b0 = readByte(input)
            val b1 = readByte(input)
            var len = (b1 and 0x7f).toLong()
            if (len == 126L) len = ((readByte(input) shl 8) or readByte(input)).toLong()
            else if (len == 127L) {
                len = 0
                repeat(8) { len = (len shl 8) or readByte(input).toLong() }
            }
            if (len < 0 || len > max) throw IOException("voice frame too large ($len B)")
            val mask = if (b1 and 0x80 != 0) ByteArray(4).also { readFully(input, it) } else null
            val payload = ByteArray(len.toInt())
            readFully(input, payload)
            if (mask != null) for (i in payload.indices) payload[i] = (payload[i].toInt() xor mask[i and 3].toInt()).toByte()
            return Frame(fin = b0 and 0x80 != 0, opcode = b0 and 0x0f, payload = payload)
        }

        private fun readByte(input: InputStream): Int {
            val b = input.read()
            if (b < 0) throw EOFException("voice socket closed")
            return b
        }

        private fun readFully(input: InputStream, buf: ByteArray) {
            var off = 0
            while (off < buf.size) {
                val n = input.read(buf, off, buf.size - off)
                if (n < 0) throw EOFException("voice socket closed")
                off += n
            }
        }

        /** An HTTP header line (ISO-8859-1, without CRLF); at most 8 KiB. */
        internal fun readLine(input: InputStream): String {
            val sb = StringBuilder()
            while (sb.length < 8192) {
                val b = input.read()
                if (b < 0) break
                if (b == '\n'.code) break
                if (b != '\r'.code) sb.append(b.toChar())
            }
            return sb.toString()
        }
    }
}
