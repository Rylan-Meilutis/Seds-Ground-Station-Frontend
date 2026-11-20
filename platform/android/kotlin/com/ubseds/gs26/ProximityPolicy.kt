package com.ubseds.gs26

/**
 * Pure decisions behind the proximity lock's Android glue (no Android types,
 * unit-tested in platform/android/unit-tests). The geofence decision itself
 * (in range / out of range, hysteresis, accuracy) is made in Rust
 * (src/telemetry_dashboard/proximity_lock.rs); nothing here sends a
 * location anywhere.
 */
object ProximityPolicy {
    // The codes Rust reads from ProximityBridge.permissionState.
    const val NOT_ASKED = 0
    const val DENIED = 1
    const val WHILE_IN_USE = 2
    const val ALL_THE_TIME = 3
    const val NO_LOCATION = 4

    /** Android 10 (Q) split background location into its own permission. */
    const val SDK_BACKGROUND_PERMISSION = 29
    /** Android 11 (R) sends the background request to the Settings page. */
    const val SDK_BACKGROUND_IN_SETTINGS = 30

    /** The Geofencing API works poorly below ~100 m; and stays sane above. */
    const val MIN_FENCE_M = 100f
    const val MAX_FENCE_M = 10_000f

    enum class Request { FOREGROUND, BACKGROUND_RATIONALE, SETTINGS, NONE }

    fun permissionState(
        hasLocationHardware: Boolean,
        fine: Boolean,
        coarse: Boolean,
        background: Boolean,
        sdkInt: Int,
        askedBefore: Boolean,
    ): Int = when {
        !hasLocationHardware -> NO_LOCATION
        !(fine || coarse) -> if (askedBefore) DENIED else NOT_ASKED
        sdkInt < SDK_BACKGROUND_PERMISSION -> ALL_THE_TIME
        background -> ALL_THE_TIME
        else -> WHILE_IN_USE
    }

    /**
     * What to ask next. Foreground first; background only when asked for,
     * after an in-app explanation (Google Play's required rationale), and
     * through Settings once the system will no longer show its prompt.
     */
    fun nextRequest(
        foregroundGranted: Boolean,
        backgroundGranted: Boolean,
        sdkInt: Int,
        wantBackground: Boolean,
        foregroundPermanentlyDenied: Boolean,
    ): Request = when {
        !foregroundGranted && foregroundPermanentlyDenied -> Request.SETTINGS
        !foregroundGranted -> Request.FOREGROUND
        !wantBackground || backgroundGranted || sdkInt < SDK_BACKGROUND_PERMISSION -> Request.NONE
        else -> Request.BACKGROUND_RATIONALE
    }

    /** The OS fence radius for a requested one (NaN or out of range clamps). */
    fun fenceRadius(requestedM: Float): Float =
        if (requestedM.isNaN()) MIN_FENCE_M else requestedM.coerceIn(MIN_FENCE_M, MAX_FENCE_M)

    fun validCenter(lat: Double, lon: Double): Boolean =
        !lat.isNaN() && !lon.isNaN() && lat in -90.0..90.0 && lon in -180.0..180.0 &&
            !(kotlin.math.abs(lat) < 1e-6 && kotlin.math.abs(lon) < 1e-6)

    /** Geofencing needs background location from Android 10 on. */
    fun geofencingAllowed(state: Int): Boolean = state == ALL_THE_TIME

    const val NOTIFICATION_TITLE = "Proximity lock"
    const val NOTIFICATION_TEXT =
        "Telling the ground station only whether you are near the rocket. Your location is not sent."
    const val BACKGROUND_RATIONALE_TITLE = "Allow location all the time?"
    fun backgroundRationale(sdkInt: Int): String =
        "The ground station's proximity lock refuses arming and filling while someone is near the rocket. " +
            "To keep telling the station \"in range\" or \"out of range\" while this app is in the background, " +
            "allow location " + (if (sdkInt >= SDK_BACKGROUND_IN_SETTINGS) "\"Allow all the time\" on the next screen" else "\"Allow all the time\"") +
            ". Your location stays on this phone; only the in-range flag is sent. You can opt out in Settings > Proximity."
}
