package com.ubseds.gs26.wear.core

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.BufferedInputStream
import java.io.ByteArrayInputStream
import java.io.IOException
import java.net.ServerSocket
import kotlin.concurrent.thread

class VoiceSocketTest {
    @Test
    fun acceptKeyMatchesRfcExample() {
        assertEquals("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=", VoiceSocket.acceptKey("dGhlIHNhbXBsZSBub25jZQ=="))
    }

    @Test
    fun framesRoundTripWithAllLengthEncodings() {
        val mask = byteArrayOf(0x37, 0xfa.toByte(), 0x21, 0x3d)
        // RFC 6455 5.7: masked "Hello".
        assertArrayEquals(
            byteArrayOf(0x81.toByte(), 0x85.toByte(), 0x37, 0xfa.toByte(), 0x21, 0x3d, 0x7f, 0x9f.toByte(), 0x4d, 0x51, 0x58),
            VoiceSocket.encodeFrame(VoiceSocket.OP_TEXT, "Hello".toByteArray(), mask),
        )
        for (len in listOf(0, 125, 126, 65535, 65536, 70000)) {
            val payload = ByteArray(len) { (it * 7).toByte() }
            for (m in listOf(mask, null)) {
                val bytes = VoiceSocket.encodeFrame(VoiceSocket.OP_BINARY, payload, m)
                val header = when { len < 126 -> 2; len <= 65535 -> 4; else -> 10 } + if (m != null) 4 else 0
                assertEquals(header + len, bytes.size)
                val f = VoiceSocket.readFrame(ByteArrayInputStream(bytes), 1 shl 20)
                assertTrue(f.fin); assertEquals(VoiceSocket.OP_BINARY, f.opcode)
                assertArrayEquals(payload, f.payload)
            }
        }
        try {
            VoiceSocket.readFrame(ByteArrayInputStream(VoiceSocket.encodeFrame(2, ByteArray(200), null)), 100)
            fail("oversize frame accepted")
        } catch (_: IOException) {}
    }

    /** Loopback server: handshake, echo, ping, a fragmented message, then close. */
    @Test
    fun talksToALoopbackServer() {
        val server = ServerSocket(0)
        val pongs = mutableListOf<ByteArray>()
        val t = thread {
            server.accept().use { c ->
                val input = BufferedInputStream(c.getInputStream())
                val out = c.getOutputStream()
                var key = ""
                while (true) {
                    val line = VoiceSocket.readLine(input)
                    if (line.isEmpty()) break
                    if (line.lowercase().startsWith("sec-websocket-key:")) key = line.substringAfter(':').trim()
                }
                out.write(("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n" +
                    "Sec-WebSocket-Accept: ${VoiceSocket.acceptKey(key)}\r\n\r\n").toByteArray())
                // Echo one text and one binary message (client frames must be masked).
                repeat(2) {
                    val f = VoiceSocket.readFrame(input, 1 shl 16)
                    check(f.opcode == VoiceSocket.OP_TEXT || f.opcode == VoiceSocket.OP_BINARY)
                    out.write(VoiceSocket.encodeFrame(f.opcode, f.payload, null))
                }
                out.write(VoiceSocket.encodeFrame(VoiceSocket.OP_PING, byteArrayOf(1, 2), null))
                pongs += VoiceSocket.readFrame(input, 64).also { check(it.opcode == VoiceSocket.OP_PONG) }.payload
                out.write(VoiceSocket.encodeFrame(VoiceSocket.OP_TEXT, "frag".toByteArray(), null, fin = false))
                out.write(VoiceSocket.encodeFrame(VoiceSocket.OP_CONTINUATION, "mented".toByteArray(), null))
                out.write(VoiceSocket.encodeFrame(VoiceSocket.OP_CLOSE, byteArrayOf(0x03, 0xe8.toByte()) + "bye".toByteArray(), null))
                VoiceSocket.readFrame(input, 64).also { check(it.opcode == VoiceSocket.OP_CLOSE) }
            }
        }
        val ws = VoiceSocket("ws://127.0.0.1:${server.localPort}/api/voice/ws", readTimeoutMs = 5_000)
        ws.connect()
        ws.sendText("""{"type":"ping"}""")
        assertEquals(WsMessage.Text("""{"type":"ping"}"""), ws.receive())
        ws.sendBinary(byteArrayOf(9, 8, 7))
        assertArrayEquals(byteArrayOf(9, 8, 7), (ws.receive() as WsMessage.Binary).data)
        assertEquals(WsMessage.Text("fragmented"), ws.receive())
        assertEquals(WsMessage.Close(1000, "bye"), ws.receive())
        t.join(5_000)
        server.close()
        assertArrayEquals(byteArrayOf(1, 2), pongs.single())
        assertTrue(ws.closed)
    }

    @Test
    fun refusesABadHandshake() {
        val server = ServerSocket(0)
        val t = thread {
            server.accept().use { c ->
                val input = BufferedInputStream(c.getInputStream())
                while (VoiceSocket.readLine(input).isNotEmpty()) {}
                c.getOutputStream().write("HTTP/1.1 101 OK\r\nSec-WebSocket-Accept: wrong\r\n\r\n".toByteArray())
            }
        }
        try {
            VoiceSocket("ws://127.0.0.1:${server.localPort}/api/voice/ws").connect()
            fail("bad accept key accepted")
        } catch (_: IOException) {}
        t.join(5_000); server.close()
    }
}
