package com.ubseds.gs26

/**
 * Pure decisions behind recording a recovery search track in the background
 * (no Android types; unit-tested in platform/android/unit-tests). Whether to
 * record at all is Rust's decision (src/telemetry_dashboard/recovery_search.rs,
 * `background_recording`): only while the station's search runs AND this
 * user opted in to share their track for it AND location is allowed. Rust
 * starts [SearchTrackService] then and stops it when any of that ends.
 */
object SearchTrackPolicy {
    /** Fused location interval while sharing; the Rust recorder keeps a fix after 5 m or 30 s. */
    const val UPDATE_MS = 5_000L
    const val MIN_UPDATE_MS = 2_000L
    const val MIN_DISTANCE_M = 5f

    /** Android 13 (Tiramisu) made notifications a runtime permission. */
    const val SDK_NOTIFICATION_PERMISSION = 33

    /**
     * The service may start only with a location permission. "While using"
     * is enough: Rust starts it from the open app (the user's opt-in), and a
     * location foreground service started then keeps that access in the
     * background. Background location ("Allow all the time") is never
     * required or requested for this.
     */
    fun canStart(fineGranted: Boolean, coarseGranted: Boolean): Boolean = fineGranted || coarseGranted

    /** Ask once for notifications so the ongoing "Recovery search" notice shows in the shade. */
    fun askForNotifications(sdkInt: Int, granted: Boolean, askedBefore: Boolean): Boolean =
        sdkInt >= SDK_NOTIFICATION_PERMISSION && !granted && !askedBefore

    /** A fix worth handing to Rust (Rust checks accuracy and thinning again). */
    fun usable(lat: Double, lon: Double): Boolean =
        !lat.isNaN() && !lon.isNaN() && lat in -90.0..90.0 && lon in -180.0..180.0 &&
            !(kotlin.math.abs(lat) < 1e-6 && kotlin.math.abs(lon) < 1e-6)

    const val CHANNEL_NAME = "Recovery search"
    const val NOTIFICATION_TITLE = "Recovery search: sharing your track"
    const val NOTIFICATION_TEXT =
        "Your position goes to the ground station while this search runs, also with the app closed. Stop sharing any time."
    const val STOP_LABEL = "Stop sharing"
}
