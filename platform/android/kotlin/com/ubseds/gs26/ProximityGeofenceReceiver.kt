package com.ubseds.gs26

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import com.google.android.gms.location.Geofence
import com.google.android.gms.location.GeofencingEvent

/**
 * Geofencing API transitions around the proximity reference. Only wakes the
 * Rust reporter, which decides from its own fix; the transition is not sent
 * anywhere. With the app killed nothing is listening and this does nothing.
 */
class ProximityGeofenceReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val event = GeofencingEvent.fromIntent(intent) ?: return
        if (event.hasError()) return
        val inside = event.geofenceTransition == Geofence.GEOFENCE_TRANSITION_ENTER ||
            event.geofenceTransition == Geofence.GEOFENCE_TRANSITION_DWELL
        try {
            ProximityBridge.nativeOnGeofence(inside)
        } catch (_: UnsatisfiedLinkError) {
            // The Rust library is not loaded: the app is not running.
        }
    }
}
