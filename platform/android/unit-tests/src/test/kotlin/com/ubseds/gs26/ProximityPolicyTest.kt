package com.ubseds.gs26

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ProximityPolicyTest {
    @Test
    fun permissionStatesMatchTheCodesRustReads() {
        // Rust: 0 not asked, 1 denied, 2 while in use, 3 all the time, 4 no location.
        assertEquals(0, ProximityPolicy.NOT_ASKED)
        assertEquals(4, ProximityPolicy.NO_LOCATION)
        fun state(fine: Boolean, coarse: Boolean, background: Boolean, sdk: Int = 34, asked: Boolean = false, hw: Boolean = true) =
            ProximityPolicy.permissionState(hw, fine, coarse, background, sdk, asked)
        assertEquals(ProximityPolicy.NO_LOCATION, state(fine = true, coarse = true, background = true, hw = false))
        assertEquals(ProximityPolicy.NOT_ASKED, state(fine = false, coarse = false, background = false))
        assertEquals(ProximityPolicy.DENIED, state(fine = false, coarse = false, background = false, asked = true))
        assertEquals(ProximityPolicy.WHILE_IN_USE, state(fine = true, coarse = true, background = false))
        assertEquals(ProximityPolicy.WHILE_IN_USE, state(fine = false, coarse = true, background = false))
        assertEquals(ProximityPolicy.ALL_THE_TIME, state(fine = true, coarse = true, background = true))
        // Before Android 10 foreground location covered the background.
        assertEquals(ProximityPolicy.ALL_THE_TIME, state(fine = true, coarse = false, background = false, sdk = 28))
        // A background grant without a foreground one is not a grant.
        assertEquals(ProximityPolicy.NOT_ASKED, state(fine = false, coarse = false, background = true))
    }

    @Test
    fun backgroundIsAskedOnlyAfterForegroundAndOnlyWhenWanted() {
        fun next(fg: Boolean, bg: Boolean, want: Boolean, sdk: Int = 34, permanent: Boolean = false) =
            ProximityPolicy.nextRequest(fg, bg, sdk, want, permanent)
        assertEquals(ProximityPolicy.Request.FOREGROUND, next(fg = false, bg = false, want = true))
        assertEquals(ProximityPolicy.Request.FOREGROUND, next(fg = false, bg = false, want = false))
        assertEquals(ProximityPolicy.Request.SETTINGS, next(fg = false, bg = false, want = true, permanent = true))
        assertEquals(ProximityPolicy.Request.BACKGROUND_RATIONALE, next(fg = true, bg = false, want = true))
        assertEquals(ProximityPolicy.Request.NONE, next(fg = true, bg = false, want = false))
        assertEquals(ProximityPolicy.Request.NONE, next(fg = true, bg = true, want = true))
        assertEquals(ProximityPolicy.Request.NONE, next(fg = true, bg = false, want = true, sdk = 28))
    }

    @Test
    fun fenceRadiusAndCentre() {
        assertEquals(100f, ProximityPolicy.fenceRadius(25f))
        assertEquals(180f, ProximityPolicy.fenceRadius(180f))
        assertEquals(10_000f, ProximityPolicy.fenceRadius(50_000f))
        assertEquals(100f, ProximityPolicy.fenceRadius(Float.NaN))
        assertTrue(ProximityPolicy.validCenter(42.0, -78.0))
        assertFalse(ProximityPolicy.validCenter(0.0, 0.0))
        assertFalse(ProximityPolicy.validCenter(91.0, 0.0))
        assertFalse(ProximityPolicy.validCenter(Double.NaN, 0.0))
        assertTrue(ProximityPolicy.geofencingAllowed(ProximityPolicy.ALL_THE_TIME))
        assertFalse(ProximityPolicy.geofencingAllowed(ProximityPolicy.WHILE_IN_USE))
    }

    @Test
    fun theUserFacingTextSaysNoLocationIsSent() {
        assertTrue(ProximityPolicy.NOTIFICATION_TEXT.contains("location is not sent"))
        for (sdk in listOf(29, 30, 34)) {
            val text = ProximityPolicy.backgroundRationale(sdk)
            assertTrue(text, text.contains("Allow all the time"))
            assertTrue(text, text.contains("stays on this phone"))
            assertTrue(text, text.contains("opt out"))
        }
        assertTrue(ProximityPolicy.backgroundRationale(30).contains("next screen"))
    }
}
