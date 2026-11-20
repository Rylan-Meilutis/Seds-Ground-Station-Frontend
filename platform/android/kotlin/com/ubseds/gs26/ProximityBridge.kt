package com.ubseds.gs26

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.util.Log

/**
 * Rust's entry points for the proximity lock (src/telemetry_dashboard/
 * proximity_native.rs). Rust decides in range / out of range and reports;
 * this object asks for permission, runs [ProximityService] (a foreground
 * service of type location that keeps the app, and so the Rust reporter,
 * alive in the background) and forwards Geofencing API wake-ups.
 */
object ProximityBridge {
    private const val TAG = "GsProximity"
    private const val PREFS = "gs26_proximity"
    private const val ASKED = "asked_location"
    private const val REQUEST_FOREGROUND = 2611
    private const val REQUEST_BACKGROUND = 2612

    /** A geofence transition: Rust evaluates at once. */
    @JvmStatic
    external fun nativeOnGeofence(inside: Boolean)

    private fun granted(context: Context, permission: String): Boolean =
        context.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED

    private fun backgroundGranted(context: Context): Boolean =
        Build.VERSION.SDK_INT < ProximityPolicy.SDK_BACKGROUND_PERMISSION ||
            granted(context, Manifest.permission.ACCESS_BACKGROUND_LOCATION)

    private fun asked(context: Context): Boolean =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(ASKED, false)

    private fun markAsked(context: Context) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(ASKED, true).apply()
    }

    @JvmStatic
    fun permissionState(context: Context): Int {
        val hardware = context.packageManager.hasSystemFeature(PackageManager.FEATURE_LOCATION)
        return ProximityPolicy.permissionState(
            hasLocationHardware = hardware,
            fine = granted(context, Manifest.permission.ACCESS_FINE_LOCATION),
            coarse = granted(context, Manifest.permission.ACCESS_COARSE_LOCATION),
            background = backgroundGranted(context),
            sdkInt = Build.VERSION.SDK_INT,
            askedBefore = asked(context),
        )
    }

    @JvmStatic
    fun requestPermission(context: Context, background: Boolean) {
        val activity = context as? Activity ?: return
        activity.runOnUiThread {
            val foreground = granted(activity, Manifest.permission.ACCESS_FINE_LOCATION) ||
                granted(activity, Manifest.permission.ACCESS_COARSE_LOCATION)
            val permanentlyDenied = !foreground && asked(activity) &&
                !activity.shouldShowRequestPermissionRationale(Manifest.permission.ACCESS_FINE_LOCATION)
            when (
                ProximityPolicy.nextRequest(
                    foregroundGranted = foreground,
                    backgroundGranted = backgroundGranted(activity),
                    sdkInt = Build.VERSION.SDK_INT,
                    wantBackground = background,
                    foregroundPermanentlyDenied = permanentlyDenied,
                )
            ) {
                ProximityPolicy.Request.FOREGROUND -> {
                    markAsked(activity)
                    activity.requestPermissions(
                        arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION),
                        REQUEST_FOREGROUND,
                    )
                }
                ProximityPolicy.Request.BACKGROUND_RATIONALE -> {
                    // Google Play requires an in-app explanation before the
                    // background request; Android 11+ then opens Settings.
                    AlertDialog.Builder(activity)
                        .setTitle(ProximityPolicy.BACKGROUND_RATIONALE_TITLE)
                        .setMessage(ProximityPolicy.backgroundRationale(Build.VERSION.SDK_INT))
                        .setPositiveButton("Continue") { _, _ ->
                            activity.requestPermissions(
                                arrayOf(Manifest.permission.ACCESS_BACKGROUND_LOCATION),
                                REQUEST_BACKGROUND,
                            )
                        }
                        .setNegativeButton("Not now", null)
                        .show()
                }
                ProximityPolicy.Request.SETTINGS -> openSettings(activity)
                ProximityPolicy.Request.NONE -> {}
            }
        }
    }

    /** Starts the service, or moves the fence of the one already running. */
    @JvmStatic
    fun start(context: Context, lat: Double, lon: Double, radiusM: Float) {
        if (!ProximityPolicy.validCenter(lat, lon)) return
        val radius = ProximityPolicy.fenceRadius(radiusM)
        val running = ProximityService.instance
        if (running != null) {
            running.update(lat, lon, radius)
            return
        }
        val intent = Intent(context, ProximityService::class.java)
            .putExtra(ProximityService.EXTRA_LAT, lat)
            .putExtra(ProximityService.EXTRA_LON, lon)
            .putExtra(ProximityService.EXTRA_RADIUS, radius)
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        } catch (t: Throwable) {
            // Not allowed from the background on this Android version: the app
            // reports while open and the station sees it as unknown otherwise.
            Log.w(TAG, "proximity service refused", t)
        }
    }

    @JvmStatic
    fun stop(context: Context) {
        try {
            context.stopService(Intent(context, ProximityService::class.java))
        } catch (t: Throwable) {
            Log.w(TAG, "could not stop proximity service", t)
        }
    }

    @JvmStatic
    fun openSettings(context: Context) {
        try {
            val intent = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null))
            if (context !is Activity) intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            context.startActivity(intent)
        } catch (t: Throwable) {
            Log.w(TAG, "could not open app settings", t)
        }
    }
}
