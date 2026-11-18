package com.ubseds.gs26

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log

/**
 * Keeps the phone app's process (and with it the Rust watch publisher) at
 * foreground priority while a Wear OS watch with the GS app is connected, so
 * the watch keeps getting snapshots when the phone app is in the background.
 * The watch also reads the station directly when this is not running.
 */
class WatchLinkService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            stopSelf()
            return START_NOT_STICKY
        }
        val notification = notification(this)
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
        } catch (t: Throwable) {
            // Not allowed from the background on this Android version: the
            // watch falls back to its direct link.
            Log.w(TAG, "watch link foreground service refused", t)
            stopSelf()
        }
        return START_NOT_STICKY
    }

    companion object {
        private const val TAG = "GsWatchLink"
        private const val CHANNEL_ID = "gs26_watch_link"
        private const val NOTIFICATION_ID = 2626
        private const val ACTION_STOP = "com.ubseds.gs26.WATCH_LINK_STOP"

        @Volatile
        private var running = false

        /** Starts the service; call while the app is in the foreground. */
        fun start(context: Context) {
            if (running) return
            try {
                val intent = Intent(context, WatchLinkService::class.java)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                    context.startForegroundService(intent)
                } else {
                    context.startService(intent)
                }
                running = true
            } catch (t: Throwable) {
                Log.w(TAG, "could not start watch link service", t)
            }
        }

        fun stop(context: Context) {
            if (!running) return
            running = false
            try {
                context.stopService(Intent(context, WatchLinkService::class.java))
            } catch (t: Throwable) {
                Log.w(TAG, "could not stop watch link service", t)
            }
        }

        private fun notification(context: Context): Notification {
            val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                val manager = context.getSystemService(NotificationManager::class.java)
                manager?.createNotificationChannel(
                    NotificationChannel(CHANNEL_ID, "Watch link", NotificationManager.IMPORTANCE_LOW).apply {
                        description = "Keeps the GS watch app updated while this app is in the background"
                        setShowBadge(false)
                    },
                )
                Notification.Builder(context, CHANNEL_ID)
            } else {
                @Suppress("DEPRECATION")
                Notification.Builder(context)
            }
            return builder
                .setContentTitle("GS watch connected")
                .setContentText("Sending station updates to your watch")
                .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
                .setOngoing(true)
                .build()
        }
    }
}
