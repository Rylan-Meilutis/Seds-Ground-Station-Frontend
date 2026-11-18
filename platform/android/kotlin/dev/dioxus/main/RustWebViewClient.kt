package dev.dioxus.main

import android.annotation.TargetApi
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import android.os.Handler
import android.net.http.SslError
import android.os.Build
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.ViewGroup
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.SslErrorHandler
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.LifecycleOwner
import androidx.webkit.WebViewAssetLoader
import org.json.JSONObject
import java.io.ByteArrayInputStream
import java.io.File
import java.io.InputStream
import java.lang.ref.WeakReference
import java.net.HttpURLConnection
import java.net.URL
import java.security.SecureRandom
import java.security.MessageDigest
import java.security.cert.X509Certificate
import javax.net.ssl.HostnameVerifier
import javax.net.ssl.HttpsURLConnection
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager

class RustWebViewClient(private val context: Context): WebViewClient() {
    private val tag = "GS26WebView"
    private val interceptedState = mutableMapOf<String, Boolean>()
    var currentUrl: String = "about:blank"
    private var lastInterceptedUrl: Uri? = null
    private var pendingUrlRedirect: String? = null
    private val tileCacheDir = File(context.cacheDir, "gs26-map-tiles").apply { mkdirs() }

    init {
        lockPhonesToPortrait(context)
    }

    /** Phones (smallest width < 600 dp) stay portrait because the header leaves
     *  no room for content in landscape. Tablets keep every orientation. */
    private fun lockPhonesToPortrait(context: Context) {
        val activity = context as? android.app.Activity ?: return
        if (activity.resources.configuration.smallestScreenWidthDp < 600) {
            activity.requestedOrientation = android.content.pm.ActivityInfo.SCREEN_ORIENTATION_USER_PORTRAIT
        }
    }

    private val assetLoader = WebViewAssetLoader.Builder()
        .setDomain(assetLoaderDomain())
        .addPathHandler("/", WebViewAssetLoader.AssetsPathHandler(context))
        .build()

    override fun shouldInterceptRequest(
        view: WebView,
        request: WebResourceRequest
    ): WebResourceResponse? {
        Log.e(tag, "shouldInterceptRequest url=${request.url} method=${request.method} mainFrame=${request.isForMainFrame}")

        pendingUrlRedirect?.let {
            Handler(Looper.getMainLooper()).post {
                view.loadUrl(it)
            }
            pendingUrlRedirect = null
            return null
        }

        proxyTileRequest(request.url)?.let {
            Log.e(tag, "tile proxy handled url=${request.url}")
            interceptedState[request.url.toString()] = true
            return it
        }

        lastInterceptedUrl = request.url
        return if (withAssetLoader()) {
            assetLoader.shouldInterceptRequest(request.url)
        } else {
            val rustWebview = view as RustWebView
            val normalizedRequest = normalizeDioxusInternalRequest(request)
            val effectiveUrl = normalizedRequest.url.toString()
            val response = handleRequest(rustWebview.id, normalizedRequest, rustWebview.isDocumentStartScriptEnabled)
            interceptedState[effectiveUrl] = response != null
            if (effectiveUrl != request.url.toString()) {
                interceptedState[request.url.toString()] = response != null
            }
            Log.e(tag, "intercept result url=${request.url} effectiveUrl=$effectiveUrl handled=${response != null}")
            response
        }
    }

    override fun shouldOverrideUrlLoading(
        view: WebView,
        request: WebResourceRequest
    ): Boolean {
        return shouldOverride(request.url.toString())
    }

    override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) {
        Log.e(tag, "onPageStarted url=$url")
        currentUrl = url
        if (interceptedState[url] == false) {
            val webView = view as RustWebView
            for (script in webView.initScripts) {
                view.evaluateJavascript(script, null)
            }
        }
        onPageLoading(url)
    }

    override fun onPageFinished(view: WebView, url: String) {
        Log.e(tag, "onPageFinished url=$url")
        watchForResume(view)
        onPageLoaded(url)
    }

    // ---------------------------------------------------------------------
    // Blank-page recovery.
    //
    // Android may kill the WebView renderer, typically while the app is in the
    // background. Without onRenderProcessGone the app process is killed with it;
    // with it, a dead WebView cannot be used again. wry created this WebView and
    // its Rust side keeps using it, so a new WebView cannot simply be swapped in:
    // the app restarts instead, once it is in the foreground. The session,
    // station and dashboard tab are persisted and come back on launch.
    //
    // On every resume the page is also asked whether its root has content. A
    // page that does not answer or stays empty past a grace period is reloaded;
    // the Rust side remounts the app on the same route.

    private val recoveryHandler = Handler(Looper.getMainLooper())
    private var watchedView: WeakReference<WebView>? = null
    private var resumeObserverInstalled = false
    private var restartWhenResumed = false
    private var recoveryCheckRunning = false
    private var lastRecoveryReloadMs = 0L

    private fun watchForResume(view: WebView) {
        watchedView = WeakReference(view)
        installResumeObserver()
    }

    private fun installResumeObserver() {
        if (resumeObserverInstalled) {
            return
        }
        val owner = context as? LifecycleOwner ?: return
        resumeObserverInstalled = true
        owner.lifecycle.addObserver(LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                onHostResumed()
            }
        })
    }

    private fun hostIsResumed(): Boolean {
        val owner = context as? LifecycleOwner ?: return true
        return owner.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)
    }

    private fun onHostResumed() {
        if (restartWhenResumed) {
            restartApp()
            return
        }
        val view = watchedView?.get() ?: return
        // Nothing in the app pauses the WebView, but the platform or a library
        // may have; a paused WebView keeps its timers stopped and looks frozen.
        view.onResume()
        view.resumeTimers()
        checkPageAfterResume()
    }

    private fun checkPageAfterResume() {
        val view = watchedView?.get() ?: return
        if (recoveryCheckRunning) {
            return
        }
        recoveryCheckRunning = true
        probePage(view) { healthy ->
            if (healthy) {
                recoveryCheckRunning = false
                return@probePage
            }
            // The page may still be rendering or reconnecting its edit socket.
            recoveryHandler.postDelayed({
                val again = watchedView?.get()
                if (again == null) {
                    recoveryCheckRunning = false
                    return@postDelayed
                }
                probePage(again) { healthyNow ->
                    recoveryCheckRunning = false
                    if (!healthyNow) {
                        reloadBlankPage(again, "blank after resume")
                    }
                }
            }, RECOVERY_GRACE_MS)
        }
    }

    /** Calls back with true when the page answered and its root has content. */
    private fun probePage(view: WebView, done: (Boolean) -> Unit) {
        var answered = false
        val timeout = Runnable {
            if (!answered) {
                answered = true
                done(false)
            }
        }
        recoveryHandler.postDelayed(timeout, RECOVERY_PROBE_TIMEOUT_MS)
        try {
            view.evaluateJavascript(RECOVERY_PROBE) { result ->
                if (!answered) {
                    answered = true
                    recoveryHandler.removeCallbacks(timeout)
                    done(result == "true")
                }
            }
        } catch (exc: Exception) {
            Log.e(tag, "recovery probe failed: ${exc.message}")
        }
    }

    private fun reloadBlankPage(view: WebView, reason: String) {
        val now = SystemClock.elapsedRealtime()
        if (lastRecoveryReloadMs != 0L && now - lastRecoveryReloadMs < RECOVERY_MIN_INTERVAL_MS) {
            return
        }
        lastRecoveryReloadMs = now
        Log.e(tag, "[webview] content process terminated, reloading ($reason)")
        view.reload()
    }

    @TargetApi(Build.VERSION_CODES.O)
    override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
        val crashed = detail.didCrash()
        Log.e(tag, "[webview] content process terminated, reloading (renderer gone, crashed=$crashed)")
        try {
            (view.parent as? ViewGroup)?.removeView(view)
            view.destroy()
        } catch (exc: Exception) {
            Log.e(tag, "could not release the dead WebView: ${exc.message}")
        }
        if (hostIsResumed()) {
            restartApp()
        } else {
            restartWhenResumed = true
            installResumeObserver()
        }
        // Handled: the app process is not killed along with the renderer.
        return true
    }

    private fun restartApp() {
        restartWhenResumed = false
        val activity = context as? Activity
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)
        if (launch == null) {
            Log.e(tag, "[webview] no launch intent; the app must be reopened")
            return
        }
        launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
        Log.e(tag, "[webview] restarting the app to replace the dead WebView")
        context.startActivity(launch)
        activity?.finishAffinity()
        // The Rust runtime is tied to the dead WebView; start a fresh process.
        Runtime.getRuntime().exit(0)
    }

    override fun onReceivedError(
        view: WebView,
        request: WebResourceRequest,
        error: WebResourceError
    ) {
        Log.e(tag, "onReceivedError url=${request.url} code=${error.errorCode} desc=${error.description}")
        if (error.errorCode == ERROR_CONNECT && request.isForMainFrame && request.url != lastInterceptedUrl) {
            view.stopLoading()
            view.loadUrl(request.url.toString())
            pendingUrlRedirect = request.url.toString()
        } else {
            super.onReceivedError(view, request, error)
        }
    }

    override fun onReceivedHttpError(
        view: WebView,
        request: WebResourceRequest,
        errorResponse: WebResourceResponse
    ) {
        Log.e(
            tag,
            "onReceivedHttpError url=${request.url} status=${errorResponse.statusCode} reason=${errorResponse.reasonPhrase}"
        )
        super.onReceivedHttpError(view, request, errorResponse)
    }

    /**
     * Voice, camera and recording pages are station documents in iframes. When
     * the operator chose to skip TLS verification for the saved station (a
     * self-signed certificate), the WebView accepts that station's certificate
     * too, matching the native HTTP client. Every other origin is refused.
     */
    override fun onReceivedSslError(view: WebView, handler: SslErrorHandler, error: SslError) {
        val station = loadStoredBaseUrl()?.let { normalizeBaseUrl(it) }
        if (station != null && sameOrigin(Uri.parse(error.url), Uri.parse(station)) && loadSkipTlsVerify(station)) {
            Log.e(tag, "accepting station certificate for url=${error.url} (TLS verification skipped for this station)")
            handler.proceed()
            return
        }
        Log.e(tag, "refusing TLS error url=${error.url} primary=${error.primaryError}")
        handler.cancel()
    }

    private fun sameOrigin(a: Uri, b: Uri): Boolean {
        fun port(uri: Uri): Int = if (uri.port != -1) uri.port else when (uri.scheme?.lowercase()) {
            "https" -> 443
            "http" -> 80
            else -> -1
        }
        return a.scheme.equals("https", ignoreCase = true) &&
            b.scheme.equals("https", ignoreCase = true) &&
            a.host != null && a.host.equals(b.host, ignoreCase = true) &&
            port(a) == port(b)
    }

    private fun proxyTileRequest(uri: Uri): WebResourceResponse? {
        val host = uri.host ?: return null
        if (host != "gs26.local") {
            return null
        }
        val path = uri.path ?: return null
        if (!path.startsWith("/tiles/")) {
            return null
        }

        val baseUrl = loadStoredBaseUrl() ?: return errorResponse(503, "Service Unavailable", "Missing backend URL")
        val normalizedBase = normalizeBaseUrl(baseUrl)
        if (normalizedBase.isEmpty()) {
            return errorResponse(503, "Service Unavailable", "Invalid backend URL")
        }

        val upstreamUrl = normalizedBase.trimEnd('/') + path
        val skipTlsVerify = loadSkipTlsVerify(normalizedBase)
        val cacheFile = tileCacheFile(normalizedBase, path)
        Log.e(tag, "proxying tile uri=$uri upstream=$upstreamUrl skipTls=$skipTlsVerify")

        return try {
            val connection = openConnection(upstreamUrl, skipTlsVerify)
            connection.instanceFollowRedirects = true
            connection.connectTimeout = 5000
            connection.readTimeout = 10000
            connection.setRequestProperty("User-Agent", "GS26-Android-WebView")
            connection.connect()

            val status = connection.responseCode
            val reason = connection.responseMessage ?: "OK"
            val mimeType = connection.contentType?.substringBefore(';') ?: "image/jpeg"
            val encoding = connection.contentEncoding ?: "binary"
            val headers = mutableMapOf<String, String>()
            connection.headerFields.forEach { (key, values) ->
                if (key != null && values != null && values.isNotEmpty()) {
                    headers[key] = values.joinToString(",")
                }
            }
            headers["Access-Control-Allow-Origin"] = "*"
            headers["Access-Control-Allow-Methods"] = "GET, OPTIONS"
            headers["Access-Control-Allow-Headers"] = "*"
            headers["Cross-Origin-Resource-Policy"] = "cross-origin"
            val bytes = selectResponseStream(connection).use { it.readBytes() }

            if (status in 200..299 && bytes.isNotEmpty()) {
                writeTileCache(cacheFile, bytes)
            } else if (cacheFile.exists()) {
                return cachedTileResponse(cacheFile, mimeType, encoding)
            }

            WebResourceResponse(
                mimeType,
                encoding,
                status,
                reason,
                headers,
                ByteArrayInputStream(bytes)
            )
        } catch (exc: Exception) {
            Log.e(tag, "tile proxy failed uri=$uri error=${exc.message}", exc)
            if (cacheFile.exists()) {
                return cachedTileResponse(cacheFile, "image/jpeg", "binary")
            }
            errorResponse(502, "Bad Gateway", exc.message ?: "Tile proxy failed")
        }
    }

    private fun tileCacheFile(baseUrl: String, path: String): File {
        val digest = MessageDigest.getInstance("SHA-256")
            .digest("$baseUrl|$path".toByteArray())
            .joinToString("") { "%02x".format(it) }
        return File(tileCacheDir, "$digest.bin")
    }

    private fun writeTileCache(file: File, bytes: ByteArray) {
        try {
            file.parentFile?.mkdirs()
            file.writeBytes(bytes)
        } catch (exc: Exception) {
            Log.e(tag, "tile cache write failed file=${file.absolutePath} error=${exc.message}", exc)
        }
    }

    private fun cachedTileResponse(file: File, mimeType: String, encoding: String): WebResourceResponse {
        return WebResourceResponse(
            mimeType,
            encoding,
            200,
            "OK",
            mapOf(
                "Access-Control-Allow-Origin" to "*",
                "Access-Control-Allow-Methods" to "GET, OPTIONS",
                "Access-Control-Allow-Headers" to "*",
                "Cross-Origin-Resource-Policy" to "cross-origin",
            ),
            file.inputStream()
        )
    }

    private fun openConnection(url: String, skipTlsVerify: Boolean): HttpURLConnection {
        val connection = URL(url).openConnection() as HttpURLConnection
        if (skipTlsVerify && connection is HttpsURLConnection) {
            val trustAll = arrayOf<TrustManager>(object : X509TrustManager {
                override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) {}
                override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {}
                override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
            })
            val sslContext = SSLContext.getInstance("TLS")
            sslContext.init(null, trustAll, SecureRandom())
            connection.sslSocketFactory = sslContext.socketFactory
            connection.hostnameVerifier = HostnameVerifier { _, _ -> true }
        }
        return connection
    }

    private fun selectResponseStream(connection: HttpURLConnection): InputStream {
        return try {
            connection.inputStream
        } catch (_: Exception) {
            connection.errorStream ?: ByteArrayInputStream(ByteArray(0))
        }
    }

    private fun errorResponse(status: Int, reason: String, message: String): WebResourceResponse {
        return WebResourceResponse(
            "text/plain",
            "utf-8",
            status,
            reason,
            mapOf(
                "Access-Control-Allow-Origin" to "*",
                "Access-Control-Allow-Methods" to "GET, OPTIONS",
                "Access-Control-Allow-Headers" to "*",
                "Cross-Origin-Resource-Policy" to "cross-origin",
            ),
            ByteArrayInputStream(message.toByteArray())
        )
    }

    private fun loadStoredBaseUrl(): String? {
        val storageFile = File(context.filesDir, "gs26/storage.json")
        if (!storageFile.exists()) {
            return null
        }
        return try {
            JSONObject(storageFile.readText()).optString("gs_base_url").takeIf { it.isNotBlank() }
        } catch (exc: Exception) {
            Log.e(tag, "failed to read storage.json: ${exc.message}", exc)
            null
        }
    }

    private fun loadSkipTlsVerify(baseUrl: String): Boolean {
        val storageFile = File(context.filesDir, "gs26/storage.json")
        if (!storageFile.exists()) {
            return false
        }
        return try {
            val key = "gs_skip_tls_verify_${normalizeBaseUrl(baseUrl)}"
            JSONObject(storageFile.readText()).optString(key) == "true"
        } catch (_: Exception) {
            false
        }
    }

    private fun normalizeBaseUrl(value: String): String {
        var base = value.trim()
        val hashIndex = base.indexOf('#')
        if (hashIndex >= 0) {
            base = base.substring(0, hashIndex)
        }
        val schemeIndex = base.indexOf("://")
        if (schemeIndex >= 0) {
            val rest = base.substring(schemeIndex + 3)
            val slashIndex = rest.indexOf('/')
            if (slashIndex >= 0) {
                base = base.substring(0, schemeIndex + 3 + slashIndex)
            }
        }
        return base.trimEnd('/')
    }

    private fun normalizeDioxusInternalRequest(request: WebResourceRequest): WebResourceRequest {
        val url = request.url
        val host = url.host ?: return request
        val encodedPath = url.encodedPath ?: return request

        // Dioxus internal event endpoint sometimes appears as `//__events` on Android.
        // Normalize to `/__events` before handing off to native request routing.
        if (host != "dioxus.index.html" || !encodedPath.startsWith("//")) {
            return request
        }

        val normalized = url.buildUpon().encodedPath(encodedPath.replaceFirst("//", "/")).build()
        if (normalized == url) {
            return request
        }

        Log.e(tag, "normalized dioxus request url=$url -> $normalized")
        return object : WebResourceRequest {
            override fun getUrl(): Uri = normalized
            override fun isForMainFrame(): Boolean = request.isForMainFrame
            override fun isRedirect(): Boolean = request.isRedirect()
            override fun hasGesture(): Boolean = request.hasGesture()
            override fun getMethod(): String = request.method
            override fun getRequestHeaders(): MutableMap<String, String> = request.requestHeaders
        }
    }

    companion object {
        private const val RECOVERY_PROBE =
            "(function(){var m=document.getElementById('main');return !!(m&&m.childElementCount>0);})()"
        private const val RECOVERY_GRACE_MS = 2000L
        private const val RECOVERY_PROBE_TIMEOUT_MS = 1500L
        private const val RECOVERY_MIN_INTERVAL_MS = 15000L

        init {
            System.loadLibrary("main")
        }
    }

    private external fun assetLoaderDomain(): String
    private external fun withAssetLoader(): Boolean
    private external fun handleRequest(webviewId: String, request: WebResourceRequest, isDocumentStartScriptEnabled: Boolean): WebResourceResponse?
    private external fun shouldOverride(url: String): Boolean
    private external fun onPageLoading(url: String)
    private external fun onPageLoaded(url: String)
}
