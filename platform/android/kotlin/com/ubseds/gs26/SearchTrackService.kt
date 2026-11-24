package com.ubseds.gs26

import android.Manifest
import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.location.Location
import android.os.Build
import android.os.IBinder
import android.os.Looper
import android.util.Log
import com.google.android.gms.location.FusedLocationProviderClient
import com.google.android.gms.location.LocationCallback
import com.google.android.gms.location.LocationRequest
import com.google.android.gms.location.LocationResult
import com.google.android.gms.location.LocationServices
import com.google.android.gms.location.Priority

/**
 * Recovery search track in the background: a foreground service of type
 * location with an ongoing "Recovery search" notification. Rust starts it
 * only while the user shares their track in a running search
 * ([SearchTrackBridge]) and stops it when sharing or the search ends. It
 * keeps this process, and with it the Rust uploader, running with the app in
 * the background, and hands every fused fix to Rust, which records it into
 * the same outbox as foreground fixes. Nothing is sent from here.
 *
 * "Stop sharing" in the notification stops the service at once and asks
 * Rust to opt out. If Android kills the app, this stops too; fixes Rust
 * already took are in the persisted outbox.
 */
class SearchTrackService : Service() {
    private var fused: FusedLocationProviderClient? = null

    private val callback = object : LocationCallback() {
        override fun onLocationResult(result: LocationResult) {
            result.locations.forEach(::forward)
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            try {
                SearchTrackBridge.nativeOnStopRequested()
            } catch (_: UnsatisfiedLinkError) {
            }
            stopSelf()
            return START_NOT_STICKY
        }
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(NOTIFICATION_ID, notification(this), ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION)
            } else {
                startForeground(NOTIFICATION_ID, notification(this))
            }
        } catch (t: Throwable) {
            // From the background, or without location permission (Android 14+).
            Log.w(TAG, "search track foreground service refused", t)
            stopSelf()
            return START_NOT_STICKY
        }
        instance = this
        startUpdates()
        // Restarting without the app (and its Rust recorder) would record nothing.
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        instance = null
        fused?.removeLocationUpdates(callback)
        fused = null
        super.onDestroy()
    }

    private fun hasLocation(): Boolean =
        checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED ||
            checkSelfPermission(Manifest.permission.ACCESS_COARSE_LOCATION) == PackageManager.PERMISSION_GRANTED

    @SuppressLint("MissingPermission")
    private fun startUpdates() {
        if (fused != null || !hasLocation()) return
        val client = LocationServices.getFusedLocationProviderClient(this)
        fused = client
        val request = LocationRequest.Builder(Priority.PRIORITY_HIGH_ACCURACY, SearchTrackPolicy.UPDATE_MS)
            .setMinUpdateIntervalMillis(SearchTrackPolicy.MIN_UPDATE_MS)
            .setMinUpdateDistanceMeters(SearchTrackPolicy.MIN_DISTANCE_M)
            .build()
        try {
            client.requestLocationUpdates(request, callback, Looper.getMainLooper())
        } catch (t: Throwable) {
            Log.w(TAG, "fused location updates refused", t)
        }
    }

    private fun forward(location: Location) {
        if (!SearchTrackPolicy.usable(location.latitude, location.longitude)) return
        try {
            SearchTrackBridge.nativeOnSearchFix(
                location.latitude,
                location.longitude,
                if (location.hasAccuracy()) location.accuracy.toDouble() else Double.NaN,
                location.time,
            )
        } catch (_: UnsatisfiedLinkError) {
            // The Rust library is not loaded (process restarted without the app).
            stopSelf()
        }
    }

    companion object {
        private const val TAG = "GsSearchTrack"
        private const val CHANNEL_ID = "gs26_search_track"
        private const val NOTIFICATION_ID = 2628
        const val ACTION_STOP = "com.ubseds.gs26.SEARCH_TRACK_STOP"

        @Volatile
        var instance: SearchTrackService? = null
            private set

        private fun notification(context: Context): Notification {
            val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val manager = context.getSystemService(NotificationManager::class.java)
                manager?.createNotificationChannel(
                    NotificationChannel(CHANNEL_ID, SearchTrackPolicy.CHANNEL_NAME, NotificationManager.IMPORTANCE_LOW).apply {
                        description = "Shown while this phone shares its track in a recovery search"
                        setShowBadge(false)
                    },
                )
                Notification.Builder(context, CHANNEL_ID)
            } else {
                @Suppress("DEPRECATION")
                Notification.Builder(context)
            }
            val immutable = PendingIntent.FLAG_IMMUTABLE
            val stop = PendingIntent.getService(
                context,
                1,
                Intent(context, SearchTrackService::class.java).setAction(ACTION_STOP),
                PendingIntent.FLAG_UPDATE_CURRENT or immutable,
            )
            val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)
            val open = launch?.let {
                PendingIntent.getActivity(context, 0, it, PendingIntent.FLAG_UPDATE_CURRENT or immutable)
            }
            builder
                .setContentTitle(SearchTrackPolicy.NOTIFICATION_TITLE)
                .setContentText(SearchTrackPolicy.NOTIFICATION_TEXT)
                .setStyle(Notification.BigTextStyle().bigText(SearchTrackPolicy.NOTIFICATION_TEXT))
                .setSmallIcon(android.R.drawable.ic_menu_mylocation)
                .setOngoing(true)
                .addAction(
                    Notification.Action.Builder(null, SearchTrackPolicy.STOP_LABEL, stop).build(),
                )
            if (open != null) builder.setContentIntent(open)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                builder.setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
            }
            return builder.build()
        }
    }
}
