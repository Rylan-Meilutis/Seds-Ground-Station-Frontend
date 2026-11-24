package com.ubseds.gs26

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import android.util.Log
import java.io.File

/**
 * Replay files opened from other apps (docs/replay-bundle.md, "Opening files
 * from other apps"). The generated MainActivity passes ACTION_VIEW and
 * ACTION_SEND intents here; the content URI is copied into the app cache on a
 * worker thread (read permission lasts only as long as the activity), then
 * Rust is signalled and pulls the copy with [takePending]
 * (src/opened_file.rs).
 */
object OpenedFileBridge {
    private const val TAG = "GsOpenedFile"
    /** Same ceiling as Rust's opened_file::MAX_BYTES. */
    private const val MAX_BYTES = 192L shl 20

    private val lock = Any()
    private var pending: String? = null

    /** Copy finished (or failed): Rust pulls with [takePending]. */
    @JvmStatic
    external fun nativeOnOpenedFile()

    /** "name\npath\nerror" for the newest opened file, once; null when none. */
    @JvmStatic
    fun takePending(): String? = synchronized(lock) {
        val value = pending
        pending = null
        value
    }

    private fun streamUri(intent: Intent): Uri? =
        if (Build.VERSION.SDK_INT >= 33) {
            intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(Intent.EXTRA_STREAM) as? Uri
        }

    private fun uriOf(intent: Intent?): Uri? = when (intent?.action) {
        Intent.ACTION_VIEW -> intent.data
        Intent.ACTION_SEND -> streamUri(intent) ?: intent.data
        else -> null
    }

    private fun displayName(activity: Activity, uri: Uri): String {
        if (uri.scheme == "content") {
            try {
                activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
                    if (cursor.moveToFirst()) {
                        val name = cursor.getString(0)
                        if (!name.isNullOrBlank()) return name
                    }
                }
            } catch (error: Exception) {
                Log.w(TAG, "display name unavailable: $error")
            }
        }
        return uri.lastPathSegment?.substringAfterLast('/')?.takeIf { it.isNotBlank() } ?: "opened file"
    }

    private fun publish(name: String, path: String, error: String) {
        val clean = name.replace('\n', ' ')
        synchronized(lock) { pending = "$clean\n$path\n${error.replace('\n', ' ')}" }
        try {
            nativeOnOpenedFile()
        } catch (error: UnsatisfiedLinkError) {
            // Rust also pulls once at start.
            Log.w(TAG, "native library not ready: $error")
        }
    }

    /** Called from MainActivity.onCreate (first launch) and onNewIntent. */
    @JvmStatic
    fun accept(activity: Activity, intent: Intent?) {
        val uri = uriOf(intent) ?: return
        val name = displayName(activity, uri)
        val resolver = activity.contentResolver
        val cache = File(activity.cacheDir, "opened-files")
        Thread {
            try {
                cache.mkdirs()
                cache.listFiles()?.forEach { it.delete() }
                val target = File(cache, "opened-${System.currentTimeMillis()}")
                val input = resolver.openInputStream(uri) ?: throw IllegalStateException("no data")
                var total = 0L
                input.use { source ->
                    target.outputStream().use { sink ->
                        val buffer = ByteArray(64 * 1024)
                        while (true) {
                            val read = source.read(buffer)
                            if (read < 0) break
                            total += read
                            if (total > MAX_BYTES) break
                            sink.write(buffer, 0, read)
                        }
                    }
                }
                if (total > MAX_BYTES) {
                    target.delete()
                    publish(name, "", "$name is larger than 192 MiB, the largest replay this app opens.")
                } else {
                    publish(name, target.absolutePath, "")
                }
            } catch (error: Exception) {
                Log.w(TAG, "could not copy $uri: $error")
                publish(name, "", "Could not read $name: ${error.message ?: error.javaClass.simpleName}")
            }
        }.start()
    }
}
