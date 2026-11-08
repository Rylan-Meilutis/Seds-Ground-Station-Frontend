package com.ubseds.gs26.wear.core

/** Item row as shown on the watch, with ages and the T-clock advanced to `now`. */
data class ItemDisplay(
    val id: String,
    val label: String,
    val text: String,
    val level: String,
    val stale: Boolean,
    val ageText: String?,
)

sealed interface LinkState {
    /** Never received a snapshot: show "Open the GS app on your phone". */
    data object NeverConnected : LinkState
    data object Ok : LinkState
    data class Lost(val sinceMs: Long) : LinkState {
        val text: String get() = "Phone link lost · last data ${Format.age(sinceMs)}"
    }
}

/**
 * The watch's whole view of the world: the last snapshot, when it arrived,
 * and an optimistic local settings edit that the phone has not echoed yet.
 */
data class WatchState(
    val snapshot: Snapshot? = null,
    val receivedAtMs: Long = 0,
    val pendingSettings: WatchSettings? = null,
) {
    /** Local edit while syncing, otherwise the phone's authoritative copy. */
    val settings: WatchSettings get() = pendingSettings ?: snapshot?.settings ?: WatchSettings()
    val syncing: Boolean get() = pendingSettings != null

    /** A snapshot arrived; the pending edit is done once the phone echoes it (or something newer). */
    fun onSnapshot(s: Snapshot, nowMs: Long) = WatchState(
        snapshot = s,
        receivedAtMs = nowMs,
        pendingSettings = pendingSettings?.takeIf { s.settings.updatedMs < it.updatedMs },
    )

    /** Applies an edit locally and stamps it. Returns null when nothing changed. */
    fun edit(nowMs: Long, change: (WatchSettings) -> WatchSettings): WatchState? {
        val current = settings
        val next = change(current)
        if (next == current) return null
        return copy(pendingSettings = next.stamped(current.updatedMs, nowMs))
    }

    fun elapsedMs(nowMs: Long): Long = (nowMs - receivedAtMs).coerceAtLeast(0)

    fun link(nowMs: Long): LinkState {
        val s = snapshot ?: return LinkState.NeverConnected
        val since = elapsedMs(nowMs)
        return if (since > s.linkTimeoutS.coerceAtLeast(1) * 1000) LinkState.Lost(since) else LinkState.Ok
    }

    /**
     * Pages in display order. The phone resolves them, but while a local edit
     * is syncing they are rebuilt from the catalog so changes show at once.
     */
    fun pages(): List<Page> {
        val s = snapshot ?: return emptyList()
        val pending = pendingSettings ?: return s.pages
        if (s.catalog.pages.isEmpty()) return s.pages
        val byId = s.catalog.pages.associateBy { it.id }
        return pending.pages.filter { it.enabled }.mapNotNull { byId[it.id] }
    }

    fun item(id: String, nowMs: Long): ItemDisplay {
        val v = snapshot?.items?.get(id)
            ?: return ItemDisplay(id, catalogLabel(id), "--", Level.UNKNOWN, stale = false, ageText = null)
        val elapsed = elapsedMs(nowMs)
        val base = if (v.unit.isNotEmpty() && v.text != "--") "${v.text} ${v.unit}" else v.text
        return ItemDisplay(
            id = id,
            label = v.label.ifBlank { catalogLabel(id) },
            text = if (v.clock != null) Format.clock(v.clock, base, elapsed) else base,
            level = v.level,
            stale = v.stale,
            ageText = v.ageMs?.let { Format.age(it + elapsed) },
        )
    }

    private fun catalogLabel(id: String) = snapshot?.catalog?.items?.firstOrNull { it.id == id }?.label ?: id
}
