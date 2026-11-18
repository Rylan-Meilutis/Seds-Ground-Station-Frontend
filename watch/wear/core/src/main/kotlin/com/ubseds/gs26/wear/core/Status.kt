package com.ubseds.gs26.wear.core

// Status page, alerts page and the vertical auto-cycle. Checked against
// watch/test-vectors.json (connection_status, alerts, auto_cycle), the same
// vectors as the Apple Watch core (WatchCore/Status.swift).

enum class StatusTone { OK, WAITING, WARN, ALERT }

data class StatusLine(val text: String, val tone: StatusTone)

/** The direct link's state for the status lines. */
data class DirectInfo(val configured: Boolean, val polling: Boolean, val error: String?) {
    companion object {
        val OFF = DirectInfo(configured = false, polling = false, error = null)
    }
}

/** What the watch knows about its data path ("via phone" or "direct to GS") and the station. */
data class ConnectionStatus(
    /** The data path in use and the data age. */
    val phone: StatusLine,
    /** The station connection; null before the first snapshot. */
    val station: StatusLine?,
    /** Show "Open the GS app on your phone": only when the phone is not
     *  reachable and there is no fresh data. */
    val openPhoneApp: Boolean,
    /** Direct-link trouble or progress while the shown data is not fresh. */
    val direct: StatusLine? = null,
) {
    companion object {
        const val OPEN_PHONE_APP = "Open the GS app on your phone"

        /** [snapshot] and [ageMs] belong to [route] (for DIRECT, the merged direct status). */
        fun of(
            snapshot: Snapshot?,
            ageMs: Long,
            phoneReachable: Boolean,
            route: LinkRoute = LinkRoute.PHONE,
            direct: DirectInfo = DirectInfo.OFF,
        ): ConnectionStatus {
            if (snapshot == null) {
                return ConnectionStatus(
                    phone = if (phoneReachable) StatusLine("Phone app open · waiting for data", StatusTone.WAITING)
                    else StatusLine(OPEN_PHONE_APP, StatusTone.ALERT),
                    station = null,
                    openPhoneApp = !phoneReachable,
                    direct = directLine(direct),
                )
            }
            val age = ageMs.coerceAtLeast(0)
            val lost = age > snapshot.linkTimeoutS.coerceAtLeast(1) * 1000
            val phone = when {
                route == LinkRoute.DIRECT && lost ->
                    StatusLine("Direct to GS · last update ${Format.age(age)} ago", StatusTone.ALERT)
                route == LinkRoute.DIRECT -> StatusLine("Direct to GS · updated ${Format.age(age)} ago", StatusTone.OK)
                !lost -> StatusLine("Via phone · updated ${Format.age(age)} ago", StatusTone.OK)
                phoneReachable -> StatusLine("Phone reachable · no update for ${Format.age(age)}", StatusTone.WARN)
                else -> StatusLine("Phone not reachable · last update ${Format.age(age)} ago", StatusTone.ALERT)
            }
            return ConnectionStatus(phone, stationLine(snapshot), lost && !phoneReachable, if (lost) directLine(direct) else null)
        }

        private fun directLine(d: DirectInfo): StatusLine? = when {
            !d.configured -> null
            d.error != null -> StatusLine("Can't reach GS directly: ${d.error}", StatusTone.ALERT)
            d.polling -> StatusLine("Contacting GS directly…", StatusTone.WAITING)
            else -> null
        }

        private fun stationLine(snap: Snapshot): StatusLine {
            if (snap.mode == Mode.DEMO) return StatusLine("Demo data (offline preview)", StatusTone.WARN)
            val name = snap.station?.name.orEmpty()
            val address = snap.station?.address.orEmpty()
            val label = name.ifEmpty { address.ifEmpty { "the station" } }
            return when (snap.station?.link ?: "offline") {
                "connected" -> StatusLine("Connected to $label", StatusTone.OK)
                "connecting" -> StatusLine("Connecting to ${address.ifEmpty { label }}…", StatusTone.WAITING)
                "reconnecting" -> StatusLine("Reconnecting to $label…", StatusTone.WARN)
                "sign_in" -> StatusLine(
                    if (address.isEmpty()) "Sign in on the phone" else "Sign in on the phone ($address)",
                    StatusTone.WARN,
                )
                else -> if (address.isNotEmpty()) StatusLine("Can't reach $address", StatusTone.ALERT)
                else StatusLine("Phone not connected to a station", StatusTone.WARN)
            }
        }
    }
}

data class AlertRow(val id: String, val label: String, val text: String, val level: String)

/** Items at `alert` level first, then `warn`, each group by label. */
fun alertRows(snapshot: Snapshot?): List<AlertRow> {
    val snap = snapshot ?: return emptyList()
    return snap.items
        .filter { (_, v) -> v.level == Level.ALERT || v.level == Level.WARN }
        .map { (id, v) ->
            val label = v.label.ifBlank { snap.catalog.items.firstOrNull { it.id == id }?.label ?: id }
            AlertRow(id, label, v.text, v.level)
        }
        .sortedWith(compareBy({ if (it.level == Level.ALERT) 0 else 1 }, { it.label }, { it.id }))
}

/**
 * The vertical cycle pager: advances every `intervalS`; a manual move
 * (swipe or rotary input) restarts the wait so the item being read stays
 * for a full interval; tap pauses/resumes.
 */
class AutoCycle(nowMs: Long, autostart: Boolean) {
    var index = 0
        private set
    var isPaused = !autostart
        private set

    /** Last automatic advance, manual move or resume. */
    private var lastMoveMs = nowMs

    /** Advances when the interval has passed; one step after a long gap. */
    fun tick(nowMs: Long, intervalS: Int, count: Int): Boolean {
        if (count <= 0) {
            index = 0
            return false
        }
        if (index >= count) index = 0
        if (isPaused || nowMs - lastMoveMs < intervalS.coerceAtLeast(1) * 1000L) return false
        index = (index + 1) % count
        lastMoveMs = nowMs
        return true
    }

    /** The pager settled on [selection]. Our own advance echoes back with the
     *  same index; anything else is the user, which restarts the wait. */
    fun observe(selection: Int, nowMs: Long): Boolean {
        if (selection == index) return false
        index = selection
        lastMoveMs = nowMs
        return true
    }

    fun togglePause(nowMs: Long) {
        isPaused = !isPaused
        if (!isPaused) lastMoveMs = nowMs
    }

    fun secondsToNext(nowMs: Long, intervalS: Int): Int? {
        if (isPaused) return null
        val remaining = (lastMoveMs + intervalS.coerceAtLeast(1) * 1000L - nowMs).coerceAtLeast(0)
        return ((remaining + 999) / 1000).toInt()
    }

    /** "2/5 · next in 3s" or "2/5 · paused"; empty without items. */
    fun footer(nowMs: Long, intervalS: Int, count: Int): String {
        if (count <= 0) return ""
        val position = "${minOf(index, count - 1) + 1}/$count"
        val s = secondsToNext(nowMs, intervalS) ?: return "$position · paused"
        return "$position · next in ${s}s"
    }
}
