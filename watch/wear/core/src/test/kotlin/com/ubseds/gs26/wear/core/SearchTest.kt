package com.ubseds.gs26.wear.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SearchTest {
    private val on = SearchInfo(active = true, sessionId = "rs-1", sharing = true, ackedSeq = 0)

    @Test
    fun nothingIsRecordedWithoutTheUsersOptInAndTheStationsAcceptance() {
        var r = SearchRecorder()
        assertEquals(r, r.record(31.0, -102.0, 5.0, 0, on))
        r = r.copy(wantsToShare = true)
        assertEquals(r, r.record(31.0, -102.0, 5.0, 0, null))
        assertEquals(r, r.record(31.0, -102.0, 5.0, 0, on.copy(sharing = false)))
        assertEquals(r, r.record(31.0, -102.0, 5.0, 0, on.copy(active = false)))
        r = r.apply(on).record(31.0, -102.0, 5.0, 0, on)
        assertEquals(1, r.pending.size)
        r = r.stop()
        assertTrue(r.pending.isEmpty())
        assertEquals(r, r.record(31.001, -102.0, 5.0, 60_000, on))
    }

    @Test
    fun thinningBufferingAndAcknowledgement() {
        var r = SearchRecorder(wantsToShare = true).apply(on)
        r = r.record(31.0, -102.0, 5.0, 0, on)
        r = r.record(31.00001, -102.0, 5.0, 1_000, on) // 1 m: dropped
        r = r.record(31.0001, -102.0, 5.0, 2_000, on) // 11 m
        r = r.record(31.0001, -102.0, null, 40_000, on) // 30 s
        r = r.record(31.01, -102.0, 250.0, 50_000, on) // poor accuracy
        assertEquals(listOf(1L, 2L, 3L), r.pending.map { it.seq })
        val message = r.nextMessage()
        assertNotNull(message)
        assertTrue(message!!.contains("\"type\":\"search_track\""))
        assertTrue(message.contains("\"session_id\":\"rs-1\""))
        r = r.apply(on.copy(ackedSeq = 2))
        assertEquals(listOf(3L), r.pending.map { it.seq })
        assertEquals(r, Search.decode(Search.encode(r)))
        r = r.apply(SearchInfo(active = true, sessionId = "rs-2"))
        assertTrue(r.pending.isEmpty())
        assertFalse(r.wantsToShare)
        assertNull(SearchRecorder().nextMessage())
    }

    @Test
    fun theSnapshotSearchObjectDecodes() {
        val snap = Protocol.parseSnapshot("""{"v":1,"seq":1,"mode":"live","search":{"active":true,"session_id":"rs-1","sharing":true,"acked_seq":7,"percent":12.5}}""")
        assertEquals(SearchInfo(true, "rs-1", true, 7, 12.5, null), snap!!.search)
        assertNull(Protocol.parseSnapshot("""{"v":1,"seq":1,"mode":"live"}""")!!.search)
        assertEquals("""{"v":1,"type":"search_share","sharing":true}""", Search.shareMessage(true))
    }
}
