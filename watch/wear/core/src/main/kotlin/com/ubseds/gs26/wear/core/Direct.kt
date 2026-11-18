package com.ubseds.gs26.wear.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.net.URLEncoder

// Direct link to the station (docs/watch-companion.md "Direct link"). The
// phone provisions the station address and a scoped watch token; while the
// phone's data is not fresh the watch polls GET /api/watch/status itself and
// sends page actions to POST /api/command with the token. Checked against
// watch/test-vectors.json (link_route, action_outcome, action_button,
// provision, direct_status.merge), the same vectors as WatchCore (Swift).

/** Where the shown data comes from. */
enum class LinkRoute { PHONE, DIRECT, NONE }

object Direct {
    const val PATH = "/gs26/direct"

    /** The phone's data is fresh enough to prefer it: recent, and the phone has a station (or its demo). */
    fun phoneIsFresh(mode: String?, ageMs: Long?, freshMs: Long): Boolean =
        mode != null && ageMs != null && mode != Mode.NO_STATION && ageMs <= freshMs

    /** Prefer the phone while fresh; otherwise fresh direct data; otherwise whichever is newer. */
    fun chooseRoute(phoneMode: String?, phoneAgeMs: Long?, directAgeMs: Long?, freshMs: Long): LinkRoute {
        if (phoneIsFresh(phoneMode, phoneAgeMs, freshMs)) return LinkRoute.PHONE
        if (directAgeMs != null && directAgeMs <= freshMs) return LinkRoute.DIRECT
        return when {
            phoneAgeMs != null && directAgeMs != null -> if (directAgeMs < phoneAgeMs) LinkRoute.DIRECT else LinkRoute.PHONE
            phoneAgeMs != null -> LinkRoute.PHONE
            directAgeMs != null -> LinkRoute.DIRECT
            else -> LinkRoute.NONE
        }
    }

    /** Poll only in the foreground, when provisioned, while the phone's data is not fresh. */
    fun shouldPoll(phoneMode: String?, phoneAgeMs: Long?, freshMs: Long, foreground: Boolean, configured: Boolean): Boolean =
        foreground && configured && !phoneIsFresh(phoneMode, phoneAgeMs, freshMs)

    /** Three missed snapshots at the foreground rate, at least 6 s. */
    fun phoneFreshMs(power: String): Long = maxOf(
        6_000L,
        3 * when (power) {
            WatchSettings.POWER_SAVER -> 5_000L
            WatchSettings.POWER_RECOVERY -> 1_000L
            else -> 2_000L
        },
    )

    fun pollMs(power: String): Long = when (power) {
        WatchSettings.POWER_SAVER -> 5_000L
        WatchSettings.POWER_RECOVERY -> 2_000L
        else -> 3_000L
    }

    fun statusUrl(base: String, metric: Boolean, actions: List<String>, staleAfterS: Int): String {
        val q = buildList {
            add("units=" + if (metric) WatchSettings.UNITS_METRIC else WatchSettings.UNITS_IMPERIAL)
            add("stale_after_s=$staleAfterS")
            if (actions.isNotEmpty()) add("actions=" + actions.joinToString(",") { URLEncoder.encode(it, "UTF-8") })
        }
        return "$base/api/watch/status?" + q.joinToString("&")
    }

    fun commandUrl(base: String) = "$base/api/command"

    /** `POST /api/command` takes the command name as a JSON string. */
    fun commandBody(cmd: String): String = kotlinx.serialization.json.JsonPrimitive(cmd).toString()

    /** Every action on the shown pages, in page order. */
    fun pageActionCommands(pages: List<Page>): List<String> = pages.flatMap { p -> p.actions.map { it.cmd } }.distinct()
}

/** The phone's direct-link message (`type: direct`). */
@Serializable
data class DirectProvision(
    val enabled: Boolean = false,
    val addresses: List<String> = emptyList(),
    /** Kept in Keystore-encrypted storage only; null in any other stored copy. */
    val token: String? = null,
    @SerialName("expires_at_ms") val expiresAtMs: Long? = null,
    val username: String = "",
    val station: String = "",
    @SerialName("skip_tls") val skipTls: Boolean = false,
    val commands: List<String> = emptyList(),
) {
    /** First address without a trailing slash. */
    val base: String get() = addresses.firstOrNull().orEmpty().trimEnd('/')

    /** host[:port] of [base]. */
    val address: String get() = base.substringAfter("://").substringBefore('/')

    /** The watch can reach the station by itself. */
    val configured: Boolean get() = enabled && !token.isNullOrEmpty() && base.isNotEmpty()

    companion object {
        /** Parses the phone's message; null for another version or type. */
        fun parse(text: String): DirectProvision? = runCatching {
            val obj = Protocol.json.parseToJsonElement(text).jsonObject
            if (obj["v"]?.jsonPrimitive?.intOrNull != Protocol.VERSION) return null
            if (obj["type"]?.jsonPrimitive?.content != "direct") return null
            Protocol.json.decodeFromJsonElement(serializer(), obj)
        }.getOrNull()
    }
}

enum class ActionOutcome { ACCEPTED, REFUSED, UNKNOWN }

data class ActionResult(val outcome: ActionOutcome, val text: String)

/** What a press ended in, from the HTTP reply of POST /api/command (or the network error). */
fun actionOutcome(status: Int?, body: String, error: String?): ActionResult {
    val detail = body.trim().take(120)
    if (error != null) return ActionResult(ActionOutcome.UNKNOWN, "Outcome unknown: $error. Check the station.")
    if (status == null) return ActionResult(ActionOutcome.UNKNOWN, "Outcome unknown. Check the station.")
    return when {
        status in 200..299 -> ActionResult(ActionOutcome.ACCEPTED, if (detail.isEmpty()) "Accepted" else "Accepted: $detail")
        status == 401 -> ActionResult(
            ActionOutcome.REFUSED,
            "Refused: the watch token was not accepted. Open the GS app on the phone.",
        )
        status == 408 || status == 429 || status >= 500 ->
            ActionResult(ActionOutcome.UNKNOWN, "Outcome unknown (HTTP $status). Check the station.")
        status in 400..499 -> ActionResult(ActionOutcome.REFUSED, if (detail.isEmpty()) "Refused (HTTP $status)" else "Refused: $detail")
        else -> ActionResult(ActionOutcome.UNKNOWN, "Outcome unknown (HTTP $status). Check the station.")
    }
}

enum class ConfirmStyle {
    /** Abort: tap, then confirm. */
    TAP,

    /** Everything else: confirm screen, then press and hold. */
    HOLD,
}

data class ActionButtonState(val enabled: Boolean, val note: String?, val confirm: ConfirmStyle)

/**
 * How a page action button looks. Abort is never disabled by the policy or
 * stale data (the station admits a manual Abort under every policy);
 * everything else follows the station's enable state and needs fresh data.
 */
fun actionButton(abort: Boolean, state: ActionState?, fresh: Boolean, token: Boolean): ActionButtonState {
    val confirm = if (abort) ConfirmStyle.TAP else ConfirmStyle.HOLD
    if (!token) return ActionButtonState(false, "Direct link not set up on the phone", confirm)
    if (abort) return ActionButtonState(true, state?.reason, confirm)
    if (!fresh) return ActionButtonState(false, "No fresh station data", confirm)
    if (state == null || !state.enabled) return ActionButtonState(false, state?.reason ?: "Disabled by the station", confirm)
    return ActionButtonState(true, null, confirm)
}
