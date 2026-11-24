package com.ubseds.gs26

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SearchTrackPolicyTest {
    @Test
    fun startsOnlyWithALocationPermissionAndNeverNeedsBackgroundLocation() {
        assertFalse(SearchTrackPolicy.canStart(fineGranted = false, coarseGranted = false))
        assertTrue(SearchTrackPolicy.canStart(fineGranted = true, coarseGranted = false))
        assertTrue(SearchTrackPolicy.canStart(fineGranted = false, coarseGranted = true))
    }

    @Test
    fun asksForNotificationsOnceFromAndroid13() {
        assertFalse(SearchTrackPolicy.askForNotifications(sdkInt = 32, granted = false, askedBefore = false))
        assertTrue(SearchTrackPolicy.askForNotifications(sdkInt = 33, granted = false, askedBefore = false))
        assertFalse(SearchTrackPolicy.askForNotifications(sdkInt = 34, granted = true, askedBefore = false))
        assertFalse(SearchTrackPolicy.askForNotifications(sdkInt = 34, granted = false, askedBefore = true))
    }

    @Test
    fun dropsImpossibleFixes() {
        assertTrue(SearchTrackPolicy.usable(31.76, -102.03))
        assertFalse(SearchTrackPolicy.usable(0.0, 0.0))
        assertFalse(SearchTrackPolicy.usable(Double.NaN, 1.0))
        assertFalse(SearchTrackPolicy.usable(91.0, 1.0))
        assertFalse(SearchTrackPolicy.usable(1.0, 181.0))
    }

    @Test
    fun theNotificationSaysWhatIsSentAndHowToStop() {
        assertTrue(SearchTrackPolicy.NOTIFICATION_TITLE.startsWith("Recovery search"))
        assertTrue(SearchTrackPolicy.NOTIFICATION_TEXT.contains("ground station"))
        assertTrue(SearchTrackPolicy.NOTIFICATION_TEXT.contains("Stop sharing"))
        assertTrue(SearchTrackPolicy.STOP_LABEL == "Stop sharing")
        assertTrue(SearchTrackPolicy.UPDATE_MS >= SearchTrackPolicy.MIN_UPDATE_MS)
    }
}
