package com.ubseds.gs26.wear

import android.annotation.SuppressLint
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import androidx.core.content.edit
import com.ubseds.gs26.wear.core.ActionResult
import com.ubseds.gs26.wear.core.Direct
import com.ubseds.gs26.wear.core.DirectProvision
import com.ubseds.gs26.wear.core.Protocol
import com.ubseds.gs26.wear.core.Snapshot
import com.ubseds.gs26.wear.core.actionOutcome
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.withContext
import java.io.IOException
import java.net.HttpURLConnection
import java.net.SocketTimeoutException
import java.net.URL
import java.net.UnknownHostException
import java.security.KeyStore
import java.security.cert.X509Certificate
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import javax.net.ssl.HttpsURLConnection
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLException
import javax.net.ssl.X509TrustManager

/**
 * The watch's own connection to the station, used while the phone app is in
 * the background or out of reach. The phone provisions the station address
 * and a scoped watch token over the Data Layer; the token is stored only
 * encrypted with an Android Keystore key. Polls `GET /api/watch/status`
 * with plain HTTP requests while the app is in front and the phone's data
 * is not fresh, and sends page actions to the ordinary `POST /api/command`
 * with the token, so the station's authorization and admission apply
 * exactly as for the phone.
 */
class DirectLink private constructor(context: Context) {
    data class State(
        val provision: DirectProvision? = null,
        val snapshot: Snapshot? = null,
        val receivedAtMs: Long = 0,
        val error: String? = null,
        val polling: Boolean = false,
    ) {
        val configured: Boolean get() = provision?.configured == true
    }

    private val app = context.applicationContext
    private val prefs = app.getSharedPreferences("gs26_direct", Context.MODE_PRIVATE)
    private val tokens = TokenStore(app)
    private val _state = MutableStateFlow(load())
    val state: StateFlow<State> = _state.asStateFlow()

    private fun load(): State {
        val stored = prefs.getString(KEY_PROVISION, null)?.let {
            runCatching { Protocol.json.decodeFromString(DirectProvision.serializer(), it) }.getOrNull()
        } ?: return State()
        val withToken = stored.copy(token = tokens.read())
        return State(provision = withToken.takeIf { it.configured })
    }

    /** Provisioning from the phone (`type: direct`). */
    fun apply(json: String) {
        val next = DirectProvision.parse(json) ?: return
        if (next.configured) tokens.write(next.token!!) else tokens.clear()
        prefs.edit { putString(KEY_PROVISION, Protocol.json.encodeToString(DirectProvision.serializer(), next.copy(token = null))) }
        _state.update { old ->
            val sameStation = old.provision?.base == next.base && next.configured
            State(
                provision = next.takeIf { it.configured },
                snapshot = if (sameStation) old.snapshot else null,
                receivedAtMs = if (sameStation) old.receivedAtMs else 0,
            )
        }
    }

    fun setPolling(polling: Boolean) = _state.update { it.copy(polling = polling) }

    /** One status request; the caller decides when (core `Direct.shouldPoll`). */
    suspend fun poll(url: String) {
        val p = _state.value.provision ?: return
        val token = p.token ?: return
        setPolling(true)
        val result = withContext(Dispatchers.IO) { request("GET", url, token, null, p) }
        val snap = result.body?.takeIf { result.status == 200 }?.let(Protocol::parseSnapshot)
        _state.update { old ->
            when {
                result.error != null -> old.copy(error = result.error)
                result.status == 401 -> old.copy(error = "watch token not accepted")
                result.status != 200 -> old.copy(error = "HTTP ${result.status}")
                snap == null -> old.copy(error = "unreadable reply")
                else -> old.copy(
                    snapshot = snap.station?.let { st ->
                        if (st.address.isEmpty()) snap.copy(station = st.copy(address = p.address)) else snap
                    } ?: snap,
                    receivedAtMs = System.currentTimeMillis(),
                    error = null,
                )
            }
        }
    }

    /** Sends one page action after the on-watch confirmation; never retried. */
    suspend fun send(cmd: String): ActionResult {
        val p = _state.value.provision
        val token = p?.token
        if (p == null || !p.configured || token == null) {
            return ActionResult(com.ubseds.gs26.wear.core.ActionOutcome.REFUSED, "Refused: the direct link is not set up on the phone.")
        }
        val result = withContext(Dispatchers.IO) { request("POST", Direct.commandUrl(p.base), token, Direct.commandBody(cmd), p) }
        return actionOutcome(result.status, result.body.orEmpty(), result.error)
    }

    private data class Reply(val status: Int?, val body: String?, val error: String?)

    private fun request(method: String, url: String, token: String, body: String?, p: DirectProvision): Reply {
        var conn: HttpURLConnection? = null
        return try {
            conn = (URL(url).openConnection() as HttpURLConnection).apply {
                requestMethod = method
                connectTimeout = 6_000
                readTimeout = 8_000
                useCaches = false
                setRequestProperty("Authorization", "Bearer $token")
                setRequestProperty("Accept", "application/json")
                if (this is HttpsURLConnection && p.skipTls && url.toHttpHost() == p.address.substringBefore(':')) {
                    trustThisStation(this)
                }
                if (body != null) {
                    doOutput = true
                    setRequestProperty("Content-Type", "application/json")
                    outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
                }
            }
            val status = conn.responseCode
            val stream = if (status in 200..299) conn.inputStream else conn.errorStream
            val text = stream?.bufferedReader()?.use { it.readText().take(Protocol.MAX_MESSAGE_BYTES) }.orEmpty()
            Reply(status, text, null)
        } catch (e: SocketTimeoutException) {
            Reply(null, null, "timed out")
        } catch (e: UnknownHostException) {
            Reply(null, null, "host not found")
        } catch (e: SSLException) {
            Reply(null, null, "TLS certificate not trusted")
        } catch (e: IOException) {
            Reply(null, null, e.message ?: "network error")
        } catch (e: RuntimeException) {
            Log.w(TAG, "direct request failed", e)
            Reply(null, null, e.message ?: "request failed")
        } finally {
            conn?.disconnect()
        }
    }

    private fun String.toHttpHost(): String = runCatching { URL(this).host }.getOrDefault("")

    /**
     * The phone's "skip TLS verification" setting for this station: accept
     * its certificate without verification, for this one host only.
     */
    @SuppressLint("CustomX509TrustManager", "TrustAllX509TrustManager", "BadHostnameVerifier")
    private fun trustThisStation(conn: HttpsURLConnection) {
        val trustAll = object : X509TrustManager {
            override fun checkClientTrusted(chain: Array<X509Certificate>, authType: String) {}
            override fun checkServerTrusted(chain: Array<X509Certificate>, authType: String) {}
            override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
        }
        val ctx = SSLContext.getInstance("TLS").apply { init(null, arrayOf(trustAll), null) }
        val host = conn.url.host
        conn.sslSocketFactory = ctx.socketFactory
        conn.setHostnameVerifier { name, _ -> name == host }
    }

    companion object {
        private const val TAG = "GsDirectLink"
        private const val KEY_PROVISION = "provision_json"

        @SuppressLint("StaticFieldLeak")
        @Volatile private var instance: DirectLink? = null
        fun get(context: Context): DirectLink =
            instance ?: synchronized(this) { instance ?: DirectLink(context).also { instance = it } }
    }
}

/** The watch token, encrypted with a non-exportable Android Keystore key. */
private class TokenStore(context: Context) {
    private val prefs = context.getSharedPreferences("gs26_direct_token", Context.MODE_PRIVATE)

    private fun key(): SecretKey {
        val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    fun write(token: String) = runCatching {
        val cipher = Cipher.getInstance(TRANSFORMATION).apply { init(Cipher.ENCRYPT_MODE, key()) }
        val sealed = cipher.iv + cipher.doFinal(token.toByteArray(Charsets.UTF_8))
        prefs.edit { putString(KEY, Base64.encodeToString(sealed, Base64.NO_WRAP)) }
    }.onFailure { Log.w("GsTokenStore", "could not store the watch token", it) }

    fun read(): String? = runCatching {
        val sealed = Base64.decode(prefs.getString(KEY, null) ?: return null, Base64.NO_WRAP)
        val cipher = Cipher.getInstance(TRANSFORMATION).apply {
            init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, sealed.copyOfRange(0, IV_BYTES)))
        }
        String(cipher.doFinal(sealed.copyOfRange(IV_BYTES, sealed.size)), Charsets.UTF_8)
    }.getOrNull()

    fun clear() = prefs.edit { remove(KEY) }

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "gs26_watch_token"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_BYTES = 12
        const val KEY = "token"
    }
}
