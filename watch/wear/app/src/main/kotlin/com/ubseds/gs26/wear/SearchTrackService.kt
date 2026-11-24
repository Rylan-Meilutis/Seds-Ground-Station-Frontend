package com.ubseds.gs26.wear

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
import android.os.IBinder
import android.os.Looper
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import com.google.android.gms.location.FusedLocationProviderClient
import com.google.android.gms.location.LocationCallback
import com.google.android.gms.location.LocationRequest
import com.google.android.gms.location.LocationResult
import com.google.android.gms.location.LocationServices
import com.google.android.gms.location.Priority
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * Recovery search on Wear OS with the app in the background
 * (docs/watch-companion.md "Recovery search"): a foreground service of type
 * location with an ongoing "Recovery search" notification and "Stop
 * sharing". Started from the open app only while this watch records (its
 * user opted in on the watch, the station's search runs and accepted the
 * opt-in: `SearchRecorder.recording`); it stops itself as soon as that is no
 * longer true. Every fix goes through [PhoneLink.searchTick], the same
 * recorder, buffer and relay as with the app open. It keeps the Data Layer
 * listener so the watch learns when the search ends.
 *
 * A plain ongoing notification, not the Ongoing Activity API: that needs
 * androidx.wear:wear-ongoing, which the build does not use yet.
 */
class SearchTrackService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var fused: FusedLocationProviderClient? = null
    private var linked = false
    private var ticking: Job? = null

    private val callback = object : LocationCallback() {
        override fun onLocationResult(result: LocationResult) {
            val link = PhoneLink.get(this@SearchTrackService)
            result.locations.forEach { link.searchTick(it) }
            if (!link.searchRecording()) stopSelf()
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val link = PhoneLink.get(this)
        if (intent?.action == ACTION_STOP) {
            link.shareSearch(false)
            stopSelf()
            return START_NOT_STICKY
        }
        if (!link.searchRecording() || !hasLocationPermission(this)) {
            stopSelf()
            return START_NOT_STICKY
        }
        try {
            ServiceCompat.startForeground(this, NOTIFICATION_ID, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION)
        } catch (e: RuntimeException) {
            // Not allowed from the background, or no location permission.
            Log.w(TAG, "cannot record the search track in the foreground", e)
            stopSelf()
            return START_NOT_STICKY
        }
        if (!linked) {
            linked = true
            link.start()
        }
        startUpdates()
        // Relay and follow the phone even while standing still (no new fixes).
        if (ticking == null) ticking = scope.launch {
            while (true) {
                delay(TICK_MS)
                link.searchTick(null)
                if (!link.searchRecording()) {
                    stopSelf()
                    break
                }
            }
        }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        scope.cancel()
        fused?.removeLocationUpdates(callback)
        fused = null
        if (linked) {
            linked = false
            PhoneLink.get(this).stop()
        }
        super.onDestroy()
    }

    @SuppressLint("MissingPermission")
    private fun startUpdates() {
        if (fused != null || !hasLocationPermission(this)) return
        val client = LocationServices.getFusedLocationProviderClient(this)
        fused = client
        val request = LocationRequest.Builder(Priority.PRIORITY_HIGH_ACCURACY, UPDATE_MS)
            .setMinUpdateDistanceMeters(MIN_DISTANCE_M)
            .build()
        try {
            client.requestLocationUpdates(request, callback, Looper.getMainLooper())
        } catch (e: RuntimeException) {
            Log.w(TAG, "fused location updates refused", e)
        }
    }

    private fun notification(): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Recovery search", NotificationManager.IMPORTANCE_LOW),
        )
        val open = PendingIntent.getActivity(
            this, 0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val stop = PendingIntent.getService(
            this, 1, Intent(this, SearchTrackService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_menu_mylocation)
            .setContentTitle("Recovery search")
            .setContentText("Sharing this watch's track · Stop any time")
            .setOngoing(true)
            .setCategory(NotificationCompat.CATEGORY_LOCATION_SHARING)
            .setContentIntent(open)
            .addAction(0, "Stop sharing", stop)
            .build()
    }

    companion object {
        private const val TAG = "GsSearchTrack"
        private const val CHANNEL = "gs26_search_track"
        private const val NOTIFICATION_ID = 27
        private const val ACTION_STOP = "com.ubseds.gs26.wear.SEARCH_TRACK_STOP"
        private const val UPDATE_MS = 2_000L
        private const val MIN_DISTANCE_M = 2f
        private const val TICK_MS = 2_000L

        /** From the open app only (Android requires it for a location service). */
        fun start(context: Context) {
            if (ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) !=
                PackageManager.PERMISSION_GRANTED
            ) return
            try {
                ContextCompat.startForegroundService(context, Intent(context, SearchTrackService::class.java))
            } catch (e: RuntimeException) {
                Log.w(TAG, "search track service refused", e)
            }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, SearchTrackService::class.java))
        }
    }
}
