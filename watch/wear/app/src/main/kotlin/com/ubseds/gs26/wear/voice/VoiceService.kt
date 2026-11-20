package com.ubseds.gs26.wear.voice

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import com.ubseds.gs26.wear.MainActivity
import com.ubseds.gs26.wear.R

/**
 * Keeps crew voice running with the wrist down or another app in front: a
 * foreground service of type `microphone` while [VoiceEngine] is joined.
 * Started only from the voice page (Android requires the app in front to
 * start a microphone service); Leave stops it.
 */
class VoiceService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val engine = VoiceEngine.get(this)
        if (intent?.action == ACTION_LEAVE) {
            engine.leave()
            ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
            stopSelf()
            return START_NOT_STICKY
        }
        try {
            ServiceCompat.startForeground(this, NOTIFICATION_ID, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
        } catch (e: RuntimeException) {
            // Not allowed from the background, or no microphone permission.
            Log.w("GsVoiceService", "cannot start voice in the foreground", e)
            stopSelf()
            return START_NOT_STICKY
        }
        engine.join()
        if (!engine.ui.value.joined) {
            ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
            stopSelf()
        }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        VoiceEngine.get(this).leave()
        super.onDestroy()
    }

    private fun notification(): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Crew voice", NotificationManager.IMPORTANCE_LOW),
        )
        val open = PendingIntent.getActivity(
            this, 0,
            Intent(this, MainActivity::class.java).putExtra(MainActivity.EXTRA_OPEN_PAGE, "voice")
                .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val leave = PendingIntent.getService(
            this, 1, Intent(this, VoiceService::class.java).setAction(ACTION_LEAVE),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_voice)
            .setContentTitle("GS crew voice")
            .setContentText("Joined · open for push-to-talk")
            .setOngoing(true)
            .setCategory(NotificationCompat.CATEGORY_CALL)
            .setContentIntent(open)
            .addAction(0, "Leave", leave)
            .build()
    }

    companion object {
        private const val CHANNEL = "gs26_voice"
        private const val NOTIFICATION_ID = 26
        private const val ACTION_LEAVE = "com.ubseds.gs26.wear.voice.LEAVE"

        fun join(context: Context) =
            ContextCompat.startForegroundService(context, Intent(context, VoiceService::class.java))

        fun leave(context: Context) {
            context.startService(Intent(context, VoiceService::class.java).setAction(ACTION_LEAVE))
        }
    }
}
