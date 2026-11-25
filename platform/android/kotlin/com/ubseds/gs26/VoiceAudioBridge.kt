package com.ubseds.gs26

import android.content.Context
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log

/**
 * Crew voice audio output on Android (Rust: src/native_voice_audio.rs).
 *
 * The operator chooses Phone (earpiece or the connected headset, like a call)
 * or Speaker on the voice page. Rust picks the device and calls [select]
 * (API 31+, `setCommunicationDevice`) or [legacy] (below 31,
 * `setSpeakerphoneOn`). While voice is joined the mode is
 * `MODE_IN_COMMUNICATION`; [release] restores the normal mode. A device
 * arriving or leaving calls [nativeOnDevicesChanged] so Rust re-applies the
 * choice: a headset plugged in does not take the speaker's place, and
 * "Phone" moves to the new headset as a call would.
 */
object VoiceAudioBridge {
    private const val TAG = "GsVoiceAudio"
    private var callback: AudioDeviceCallback? = null

    /** A device came or went: Rust re-applies the operator's choice. */
    @JvmStatic
    external fun nativeOnDevicesChanged()

    private fun audio(context: Context): AudioManager =
        context.getSystemService(Context.AUDIO_SERVICE) as AudioManager

    private fun watchDevices(manager: AudioManager) {
        if (callback != null) return
        val cb = object : AudioDeviceCallback() {
            override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) = changed()
            override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) = changed()
            private fun changed() {
                try { nativeOnDevicesChanged() } catch (e: UnsatisfiedLinkError) { Log.w(TAG, "native side not loaded", e) }
            }
        }
        manager.registerAudioDeviceCallback(cb, Handler(Looper.getMainLooper()))
        callback = cb
    }

    /**
     * Communication devices available now, as a bit mask of
     * `AudioDeviceInfo.TYPE_*` (bit n = type n), or -1 below API 31.
     */
    @JvmStatic
    fun devices(context: Context): Int {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return -1
        var mask = 0
        for (device in audio(context).availableCommunicationDevices) {
            if (device.type in 0..30) mask = mask or (1 shl device.type)
        }
        return mask
    }

    /** API 31+: plays voice on the first available device of [type]. */
    @JvmStatic
    fun select(context: Context, type: Int): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return false
        val manager = audio(context)
        manager.mode = AudioManager.MODE_IN_COMMUNICATION
        watchDevices(manager)
        val device = manager.availableCommunicationDevices.firstOrNull { it.type == type } ?: return false
        val ok = manager.setCommunicationDevice(device)
        Log.i(TAG, "[voice-audio] communication device type $type: ${if (ok) "selected" else "refused"}")
        return ok
    }

    /** Below API 31: speakerphone on for Speaker, off for Phone. */
    @JvmStatic
    @Suppress("DEPRECATION")
    fun legacy(context: Context, speaker: Boolean): Boolean {
        val manager = audio(context)
        manager.mode = AudioManager.MODE_IN_COMMUNICATION
        watchDevices(manager)
        manager.isSpeakerphoneOn = speaker
        Log.i(TAG, "[voice-audio] speakerphone ${if (speaker) "on" else "off"}")
        return manager.isSpeakerphoneOn == speaker
    }

    /** The device type playing voice now (0 when unknown). */
    @JvmStatic
    @Suppress("DEPRECATION")
    fun current(context: Context): Int {
        val manager = audio(context)
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            manager.communicationDevice?.type ?: 0
        } else if (manager.isSpeakerphoneOn) {
            AudioDeviceInfo.TYPE_BUILTIN_SPEAKER
        } else {
            AudioDeviceInfo.TYPE_BUILTIN_EARPIECE
        }
    }

    /** Voice left: stop watching devices and give the audio mode back. */
    @JvmStatic
    @Suppress("DEPRECATION")
    fun release(context: Context) {
        val manager = audio(context)
        callback?.let { manager.unregisterAudioDeviceCallback(it) }
        callback = null
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            manager.clearCommunicationDevice()
        } else {
            manager.isSpeakerphoneOn = false
        }
        if (manager.mode == AudioManager.MODE_IN_COMMUNICATION) manager.mode = AudioManager.MODE_NORMAL
    }
}
