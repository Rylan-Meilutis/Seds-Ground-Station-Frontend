package com.ubseds.gs26.wear

import android.Manifest
import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.hardware.GeomagneticField
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.location.Location
import android.os.Looper
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import com.google.android.gms.location.LocationCallback
import com.google.android.gms.location.LocationRequest
import com.google.android.gms.location.LocationResult
import com.google.android.gms.location.LocationServices
import com.google.android.gms.location.Priority
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.sin

fun hasLocationPermission(context: Context) =
    ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED

/**
 * Watch GPS fix, only requested while [active]. Recovery power uses high
 * accuracy at 1 s (max 2 s); otherwise balanced power at 5 s. The last fix is
 * kept in [holder] so switching pages does not lose it.
 */
@SuppressLint("MissingPermission")
@Composable
fun WatchLocationEffect(active: Boolean, recovery: Boolean, permitted: Boolean, holder: MutableState<Location?>) {
    val context = LocalContext.current
    DisposableEffect(active, recovery, permitted) {
        if (!active || !permitted || !hasLocationPermission(context)) return@DisposableEffect onDispose { }
        val client = LocationServices.getFusedLocationProviderClient(context)
        val request = if (recovery) {
            LocationRequest.Builder(Priority.PRIORITY_HIGH_ACCURACY, 1_000L).setMaxUpdateDelayMillis(2_000L).build()
        } else {
            LocationRequest.Builder(Priority.PRIORITY_BALANCED_POWER_ACCURACY, 5_000L).build()
        }
        val callback = object : LocationCallback() {
            override fun onLocationResult(result: LocationResult) {
                result.lastLocation?.let { holder.value = it }
            }
        }
        client.lastLocation.addOnSuccessListener { loc -> if (loc != null && holder.value == null) holder.value = loc }
        client.requestLocationUpdates(request, callback, Looper.getMainLooper())
        onDispose { client.removeLocationUpdates(callback) }
    }
}

/** Smoothed magnetic azimuth of the top of the watch face, degrees, plus calibration state. */
data class Heading(val magneticDeg: Double, val needsCalibration: Boolean)

@Composable
fun rememberHeading(active: Boolean): MutableState<Heading?> {
    val context = LocalContext.current
    val state = remember { mutableStateOf<Heading?>(null) }
    DisposableEffect(active) {
        val sm = context.getSystemService(Context.SENSOR_SERVICE) as SensorManager
        val sensor = sm.getDefaultSensor(Sensor.TYPE_ROTATION_VECTOR)
        if (!active || sensor == null) return@DisposableEffect onDispose { }
        val listener = HeadingListener { state.value = it }
        sm.registerListener(listener, sensor, SensorManager.SENSOR_DELAY_UI)
        onDispose { sm.unregisterListener(listener) }
    }
    return state
}

private class HeadingListener(private val onHeading: (Heading) -> Unit) : SensorEventListener {
    private val rotation = FloatArray(9)
    private val orientation = FloatArray(3)
    private var sinS = 0.0
    private var cosS = 0.0
    private var primed = false

    override fun onSensorChanged(event: SensorEvent) {
        SensorManager.getRotationMatrixFromVector(rotation, event.values)
        SensorManager.getOrientation(rotation, orientation)
        val az = orientation[0].toDouble()
        // Low-pass on the unit vector so 359° → 1° does not swing through 180°.
        if (!primed) {
            sinS = sin(az); cosS = cos(az); primed = true
        } else {
            sinS += ALPHA * (sin(az) - sinS)
            cosS += ALPHA * (cos(az) - cosS)
        }
        val deg = Math.toDegrees(atan2(sinS, cosS)).let { if (it < 0) it + 360 else it }
        onHeading(Heading(deg, needsCalibration(event.accuracy)))
    }

    // Accuracy also arrives with every event; that is what drives the prompt.
    override fun onAccuracyChanged(sensor: Sensor, acc: Int) = Unit

    private fun needsCalibration(acc: Int) =
        acc == SensorManager.SENSOR_STATUS_UNRELIABLE || acc == SensorManager.SENSOR_STATUS_ACCURACY_LOW

    private companion object {
        const val ALPHA = 0.15
    }
}

/** Magnetic declination (east positive) at [loc], 0 when unknown. */
fun declinationAt(loc: Location?): Double {
    loc ?: return 0.0
    return GeomagneticField(
        loc.latitude.toFloat(), loc.longitude.toFloat(), loc.altitude.toFloat(), System.currentTimeMillis(),
    ).declination.toDouble()
}
