package com.ubseds.gs26

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.util.Log

/**
 * Rust's entry points for recording a recovery search track in the
 * background (src/telemetry_dashboard/recovery_search_native.rs). Rust calls
 * [start] only while the user shares their track in a running search, and
 * [stop] when they stop, the search ends, or location is no longer allowed.
 * Fixes from [SearchTrackService] go back to Rust, which keeps them in the
 * same outbox as foreground fixes; nothing here sends a location anywhere.
 */
object SearchTrackBridge {
    private const val TAG = "GsSearchTrack"
    private const val PREFS = "gs26_search_track"
    private const val ASKED_NOTIFICATIONS = "asked_notifications"
    private const val REQUEST_NOTIFICATIONS = 2613

    /** One fused fix (any thread). Rust drops it unless it asked for background recording. */
    @JvmStatic
    external fun nativeOnSearchFix(lat: Double, lon: Double, accuracyM: Double, timeMs: Long)

    /** "Stop sharing" in the notification: Rust opts out of the search. */
    @JvmStatic
    external fun nativeOnStopRequested()

    private fun granted(context: Context, permission: String): Boolean =
        context.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED

    /** Starts the service (no-op while it runs or without a location permission). */
    @JvmStatic
    fun start(context: Context) {
        if (SearchTrackService.instance != null) return
        if (!SearchTrackPolicy.canStart(
                granted(context, Manifest.permission.ACCESS_FINE_LOCATION),
                granted(context, Manifest.permission.ACCESS_COARSE_LOCATION),
            )
        ) return
        askForNotifications(context)
        val intent = Intent(context, SearchTrackService::class.java)
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        } catch (t: Throwable) {
            // Not allowed from the background on this Android version: the
            // app records while open and Rust asks again later.
            Log.w(TAG, "search track service refused", t)
        }
    }

    @JvmStatic
    fun stop(context: Context) {
        try {
            context.stopService(Intent(context, SearchTrackService::class.java))
        } catch (t: Throwable) {
            Log.w(TAG, "could not stop search track service", t)
        }
    }

    private fun askForNotifications(context: Context) {
        val activity = context as? Activity ?: return
        val prefs = activity.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val notificationsGranted = Build.VERSION.SDK_INT < SearchTrackPolicy.SDK_NOTIFICATION_PERMISSION ||
            granted(activity, Manifest.permission.POST_NOTIFICATIONS)
        if (!SearchTrackPolicy.askForNotifications(
                Build.VERSION.SDK_INT,
                notificationsGranted,
                prefs.getBoolean(ASKED_NOTIFICATIONS, false),
            )
        ) return
        prefs.edit().putBoolean(ASKED_NOTIFICATIONS, true).apply()
        activity.runOnUiThread {
            activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQUEST_NOTIFICATIONS)
        }
    }
}
