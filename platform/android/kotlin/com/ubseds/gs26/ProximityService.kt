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
import com.google.android.gms.location.Geofence
import com.google.android.gms.location.GeofencingClient
import com.google.android.gms.location.GeofencingRequest
import com.google.android.gms.location.LocationCallback
import com.google.android.gms.location.LocationRequest
import com.google.android.gms.location.LocationResult
import com.google.android.gms.location.LocationServices
import com.google.android.gms.location.Priority

/**
 * The proximity lock's foreground service (type location). While the user
 * takes part it keeps this process, and with it the Rust reporter, running
 * when the app is in the background; feeds fused location fixes into the
 * same Rust fix the map uses (LocationShim.nativeOnLocationUpdate); and,
 * with "Allow all the time", registers a Geofencing API fence around the
 * reference point whose transitions wake the reporter. No location leaves
 * the phone from here: the Rust reporter sends only the in-range flag.
 *
 * If Android kills the app, this stops too and the heartbeats stop; the
 * station then sees this device as unknown (its stale policy applies).
 */
class ProximityService : Service() {
    private var fused: com.google.android.gms.location.FusedLocationProviderClient? = null
    private var geofencing: GeofencingClient? = null
    private var fenceIntent: PendingIntent? = null

    private val callback = object : LocationCallback() {
        override fun onLocationResult(result: LocationResult) {
            result.lastLocation?.let(::forward)
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val notification = notification(this)
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION)
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
        } catch (t: Throwable) {
            Log.w(TAG, "proximity foreground service refused", t)
            stopSelf()
            return START_NOT_STICKY
        }
        instance = this
        startUpdates()
        val lat = intent?.getDoubleExtra(EXTRA_LAT, Double.NaN) ?: Double.NaN
        val lon = intent?.getDoubleExtra(EXTRA_LON, Double.NaN) ?: Double.NaN
        val radius = intent?.getFloatExtra(EXTRA_RADIUS, Float.NaN) ?: Float.NaN
        update(lat, lon, radius)
        // Restarting without the app (and its Rust reporter) would report nothing.
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        instance = null
        fused?.removeLocationUpdates(callback)
        fenceIntent?.let { geofencing?.removeGeofences(it) }
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
        val request = LocationRequest.Builder(Priority.PRIORITY_HIGH_ACCURACY, UPDATE_MS)
            .setMinUpdateIntervalMillis(MIN_UPDATE_MS)
            .build()
        try {
            client.requestLocationUpdates(request, callback, Looper.getMainLooper())
        } catch (t: Throwable) {
            Log.w(TAG, "fused location updates refused", t)
        }
    }

    /** Moves the OS fence to the reference point (no-op without a valid one). */
    @SuppressLint("MissingPermission")
    fun update(lat: Double, lon: Double, radiusM: Float) {
        if (!ProximityPolicy.validCenter(lat, lon)) return
        if (!ProximityPolicy.geofencingAllowed(ProximityBridge.permissionState(this))) return
        val client = geofencing ?: LocationServices.getGeofencingClient(this).also { geofencing = it }
        val pending = fenceIntent ?: PendingIntent.getBroadcast(
            this,
            0,
            Intent(this, ProximityGeofenceReceiver::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or
                (if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else 0),
        ).also { fenceIntent = it }
        val fence = Geofence.Builder()
            .setRequestId(FENCE_ID)
            .setCircularRegion(lat, lon, ProximityPolicy.fenceRadius(radiusM))
            .setExpirationDuration(Geofence.NEVER_EXPIRE)
            .setTransitionTypes(Geofence.GEOFENCE_TRANSITION_ENTER or Geofence.GEOFENCE_TRANSITION_EXIT)
            .build()
        val request = GeofencingRequest.Builder()
            .setInitialTrigger(GeofencingRequest.INITIAL_TRIGGER_ENTER)
            .addGeofence(fence)
            .build()
        try {
            client.removeGeofences(pending).addOnCompleteListener {
                client.addGeofences(request, pending).addOnFailureListener { e ->
                    Log.w(TAG, "geofence not added", e)
                }
            }
        } catch (t: Throwable) {
            Log.w(TAG, "geofencing refused", t)
        }
    }

    private fun forward(location: Location) {
        try {
            LocationShim.nativeOnLocationUpdate(
                location.latitude,
                location.longitude,
                if (location.hasAltitude()) location.altitude else Double.NaN,
                if (location.hasAccuracy()) location.accuracy.toDouble() else Double.NaN,
                location.time,
            )
        } catch (_: UnsatisfiedLinkError) {
            // The Rust library is not loaded (process restarted without the app).
            stopSelf()
        }
    }

    companion object {
        private const val TAG = "GsProximity"
        private const val CHANNEL_ID = "gs26_proximity"
        private const val NOTIFICATION_ID = 2627
        private const val FENCE_ID = "gs26-proximity"
        private const val UPDATE_MS = 10_000L
        private const val MIN_UPDATE_MS = 5_000L
        const val EXTRA_LAT = "lat"
        const val EXTRA_LON = "lon"
        const val EXTRA_RADIUS = "radius_m"

        @Volatile
        var instance: ProximityService? = null
            private set

        private fun notification(context: Context): Notification {
            val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val manager = context.getSystemService(NotificationManager::class.java)
                manager?.createNotificationChannel(
                    NotificationChannel(CHANNEL_ID, ProximityPolicy.NOTIFICATION_TITLE, NotificationManager.IMPORTANCE_LOW).apply {
                        description = "Shown while this phone takes part in the ground station's proximity lock"
                        setShowBadge(false)
                    },
                )
                Notification.Builder(context, CHANNEL_ID)
            } else {
                @Suppress("DEPRECATION")
                Notification.Builder(context)
            }
            return builder
                .setContentTitle(ProximityPolicy.NOTIFICATION_TITLE)
                .setContentText(ProximityPolicy.NOTIFICATION_TEXT)
                .setSmallIcon(android.R.drawable.ic_menu_mylocation)
                .setOngoing(true)
                .build()
        }
    }
}
