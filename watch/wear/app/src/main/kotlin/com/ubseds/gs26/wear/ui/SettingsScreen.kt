package com.ubseds.gs26.wear.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.activity.compose.BackHandler
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.wear.compose.foundation.lazy.ScalingLazyColumn
import androidx.wear.compose.foundation.lazy.ScalingLazyListScope
import androidx.wear.compose.foundation.lazy.rememberScalingLazyListState
import androidx.wear.compose.material.Chip
import androidx.wear.compose.material.ChipDefaults
import androidx.wear.compose.material.CompactButton
import androidx.wear.compose.material.ListHeader
import androidx.wear.compose.material.Switch
import androidx.wear.compose.material.Text
import androidx.wear.compose.material.ToggleChip
import com.ubseds.gs26.wear.PhoneLink
import com.ubseds.gs26.wear.core.PageKind
import com.ubseds.gs26.wear.core.VoiceSetting
import com.ubseds.gs26.wear.core.WatchSettings
import com.ubseds.gs26.wear.core.WatchSettings.Companion.NORTH_MAGNETIC
import com.ubseds.gs26.wear.core.WatchSettings.Companion.NORTH_TRUE
import com.ubseds.gs26.wear.core.WatchSettings.Companion.POWER_BALANCED
import com.ubseds.gs26.wear.core.WatchSettings.Companion.POWER_RECOVERY
import com.ubseds.gs26.wear.core.WatchSettings.Companion.POWER_SAVER
import com.ubseds.gs26.wear.core.WatchSettings.Companion.UNITS_IMPERIAL
import com.ubseds.gs26.wear.core.WatchSettings.Companion.UNITS_METRIC
import com.ubseds.gs26.wear.core.WatchState

/**
 * Every change is applied locally at once and sent to the phone, which
 * sanitizes it and echoes the authoritative copy in the next snapshot.
 */
@Composable
fun SettingsScreen(state: WatchState, link: PhoneLink, onDone: () -> Unit) {
    var editingPage by rememberSaveable { mutableStateOf<String?>(null) }
    val page = editingPage
    if (page != null) {
        BackHandler { editingPage = null }
        PageContentScreen(state, link, page, onDone = { editingPage = null })
        return
    }
    val s = state.settings
    val catalog = state.snapshot?.catalog
    val direct by link.direct.state.collectAsStateWithLifecycle()
    val edit: ((WatchSettings) -> WatchSettings) -> Unit = link::editSettings
    val pageTitles = catalog?.pages?.associate { it.id to it.title }.orEmpty()
    val itemLabels = catalog?.items?.associate { it.id to it.label }.orEmpty()
    val knownItems = itemLabels.keys
    val targets = catalog?.targets.orEmpty()
    val listState = rememberScalingLazyListState()

    ScalingLazyColumn(state = listState, modifier = Modifier.fillMaxSize()) {
        item { ListHeader { Text("Watch settings") } }
        item {
            Text(
                if (state.syncing) "Syncing with phone…" else "Synced",
                fontSize = 11.sp,
                color = if (state.syncing) Amber else Dim,
            )
        }

        item { ListHeader { Text("Pages") } }
        for (p in s.pages) {
            orderedToggle(
                key = "page-${p.id}",
                label = pageTitles[p.id] ?: p.id,
                checked = p.enabled,
                onToggle = { edit { it.togglePage(p.id) } },
                onMove = { up -> edit { it.movePage(p.id, up) } },
            )
        }

        for (p in s.pages) {
            val def = catalog?.pages?.firstOrNull { it.id == p.id } ?: continue
            if (def.kind != PageKind.ITEMS && p.actions.isEmpty()) continue
            item(key = "content-${p.id}") {
                Chip(
                    onClick = { editingPage = p.id },
                    label = { Text("Rows & actions: ${def.title}", fontSize = 12.sp) },
                    colors = ChipDefaults.secondaryChipColors(),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }

        item { ListHeader { Text("Direct link") } }
        item {
            val p = direct.provision
            Text(
                if (p != null && p.configured) {
                    "${p.address} · " + (if (p.commands.isEmpty()) "read-only" else p.commands.joinToString(", "))
                } else {
                    "Not set up. Sign in on the phone app."
                },
                fontSize = 11.sp,
                color = Dim,
                textAlign = TextAlign.Center,
            )
        }
        item {
            ToggleChip(
                checked = s.directLink,
                onCheckedChange = { edit { it.copy(directLink = !it.directLink) } },
                label = { Text("Use direct link", fontSize = 12.sp) },
                toggleControl = { Switch(checked = s.directLink) },
                modifier = Modifier.fillMaxWidth(),
            )
        }

        item { ListHeader { Text("Voice") } }
        item {
            Text(
                when {
                    direct.provision?.voice == true -> "Crew voice allowed for this account"
                    direct.provision?.configured == true -> "No voice permission (or Voice off on the phone)"
                    else -> "Needs the direct link from the phone"
                },
                fontSize = 11.sp, color = Dim, textAlign = TextAlign.Center,
            )
        }
        item {
            ToggleChip(
                checked = s.voice.enabled,
                onCheckedChange = { edit { it.withVoice(it.voice.copy(enabled = !it.voice.enabled)) } },
                label = { Text("Voice", fontSize = 12.sp) },
                toggleControl = { Switch(checked = s.voice.enabled) },
                modifier = Modifier.fillMaxWidth(),
            )
        }
        item {
            Chip(
                onClick = { edit { it.withVoice(it.voice.copy(ptt = if (it.voice.toggle) VoiceSetting.PTT_HOLD else VoiceSetting.PTT_TOGGLE)) } },
                label = { Text(if (s.voice.toggle) "Push-to-talk: tap to toggle" else "Push-to-talk: hold", fontSize = 12.sp) },
                colors = ChipDefaults.secondaryChipColors(),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        item {
            Chip(
                onClick = { edit { it.withVoice(it.voice.copy(channel = if (it.voice.radio) VoiceSetting.CHANNEL_CREW else VoiceSetting.CHANNEL_CREW_RADIO)) } },
                label = { Text("Channel: ${s.voice.channelLabel}", fontSize = 12.sp) },
                colors = ChipDefaults.secondaryChipColors(),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        item {
            val routes = listOf(VoiceSetting.ROUTE_AUTO, VoiceSetting.ROUTE_SPEAKER, VoiceSetting.ROUTE_BLUETOOTH)
            val labels = mapOf(VoiceSetting.ROUTE_AUTO to "Auto", VoiceSetting.ROUTE_SPEAKER to "Watch speaker", VoiceSetting.ROUTE_BLUETOOTH to "Bluetooth headset")
            Chip(
                onClick = {
                    edit { it.withVoice(it.voice.copy(route = routes[(routes.indexOf(it.voice.route) + 1).mod(routes.size)])) }
                },
                label = { Text("Audio: ${labels[s.voice.route] ?: "Auto"}", fontSize = 12.sp) },
                colors = ChipDefaults.secondaryChipColors(),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        item {
            ToggleChip(
                checked = s.voice.haptics,
                onCheckedChange = { edit { it.withVoice(it.voice.copy(haptics = !it.voice.haptics)) } },
                label = { Text("Talk haptics", fontSize = 12.sp) },
                toggleControl = { Switch(checked = s.voice.haptics) },
                modifier = Modifier.fillMaxWidth(),
            )
        }
        item {
            ToggleChip(
                checked = s.voice.talkerHaptics,
                onCheckedChange = { edit { it.withVoice(it.voice.copy(talkerHaptics = !it.voice.talkerHaptics)) } },
                label = { Text("Tap when others talk", fontSize = 12.sp) },
                toggleControl = { Switch(checked = s.voice.talkerHaptics) },
                modifier = Modifier.fillMaxWidth(),
            )
        }

        item { ListHeader { Text("Cycle") } }
        item {
            Stepper(
                label = "Every ${s.cycle.intervalS} s",
                onMinus = { edit { it.withInterval(it.cycle.intervalS - 1) } },
                onPlus = { edit { it.withInterval(it.cycle.intervalS + 1) } },
            )
        }
        // Selected items first, in cycle order; then the rest of the catalog.
        for (id in s.cycle.items) {
            orderedToggle(
                key = "cycle-$id",
                label = itemLabels[id] ?: id,
                checked = true,
                onToggle = { edit { it.toggleCycleItem(id) { k -> k in knownItems } } },
                onMove = { up -> edit { it.moveCycleItem(id, up) } },
            )
        }
        for (ci in catalog?.items.orEmpty().filter { it.id !in s.cycle.items }) {
            orderedToggle(
                key = "cycle-${ci.id}",
                label = ci.label,
                checked = false,
                onToggle = { edit { it.toggleCycleItem(ci.id) { k -> k in knownItems } } },
                onMove = null,
            )
        }

        item { ListHeader { Text("Compass") } }
        item {
            ChoiceChip("North", if (s.magneticNorth) "Magnetic" else "True") {
                edit { it.withNorth(if (it.magneticNorth) NORTH_TRUE else NORTH_MAGNETIC) }
            }
        }
        item {
            ToggleChip(
                checked = s.map.enabled,
                onCheckedChange = { edit { it.withMapEnabled(!it.map.enabled) } },
                label = { Text("Map background", fontSize = 12.sp) },
                toggleControl = { Switch(checked = s.map.enabled) },
                modifier = Modifier.fillMaxWidth(),
            )
        }
        for (t in targets) {
            orderedToggle(
                key = "target-${t.id}",
                label = t.label,
                checked = t.id in s.compass.targets,
                onToggle = { edit { it.toggleTarget(t.id) { k -> targets.any { x -> x.id == k } } } },
                onMove = null,
            )
        }

        item { ListHeader { Text("General") } }
        item {
            ChoiceChip("Units", if (s.metric) "Metric" else "Imperial") {
                edit { it.withUnits(if (it.metric) UNITS_IMPERIAL else UNITS_METRIC) }
            }
        }
        item {
            val label = when (s.power) {
                POWER_SAVER -> "Power saver"
                POWER_RECOVERY -> "Recovery (fast)"
                else -> "Balanced"
            }
            ChoiceChip("Power", label) {
                edit {
                    it.withPower(
                        when (it.power) {
                            POWER_SAVER -> POWER_BALANCED
                            POWER_BALANCED -> POWER_RECOVERY
                            else -> POWER_SAVER
                        },
                    )
                }
            }
        }
        item {
            Chip(
                onClick = onDone,
                label = { Text("Done") },
                colors = ChipDefaults.primaryChipColors(),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

/** A toggle row; with [onMove], small up/down buttons for reordering. */
private fun ScalingLazyListScope.orderedToggle(
    key: String,
    label: String,
    checked: Boolean,
    onToggle: () -> Unit,
    onMove: ((up: Boolean) -> Unit)?,
) {
    item(key = key) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
            if (onMove != null) CompactButton(onClick = { onMove(true) }) { Text("▲", fontSize = 10.sp) }
            ToggleChip(
                checked = checked,
                onCheckedChange = { onToggle() },
                label = { Text(label, fontSize = 12.sp, maxLines = 2) },
                toggleControl = { Switch(checked = checked) },
                modifier = Modifier.weight(1f),
            )
            if (onMove != null) CompactButton(onClick = { onMove(false) }) { Text("▼", fontSize = 10.sp) }
        }
    }
}

@Composable
private fun ChoiceChip(title: String, value: String, onClick: () -> Unit) {
    Chip(
        onClick = onClick,
        label = { Text(title) },
        secondaryLabel = { Text(value) },
        colors = ChipDefaults.secondaryChipColors(),
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun Stepper(label: String, onMinus: () -> Unit, onPlus: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        CompactButton(onClick = onMinus) { Text("−") }
        Text(label, textAlign = TextAlign.Center, modifier = Modifier.weight(1f))
        CompactButton(onClick = onPlus) { Text("+") }
    }
}

/**
 * Rows of one items page (toggle, reorder, add from the catalog) and its
 * action buttons, which can be removed here; adding actions is done on the
 * phone, which knows what the station lets this account run.
 */
@Composable
private fun PageContentScreen(state: WatchState, link: PhoneLink, pageId: String, onDone: () -> Unit) {
    val edit: ((WatchSettings) -> WatchSettings) -> Unit = link::editSettings
    val catalog = state.snapshot?.catalog
    val def = catalog?.pages?.firstOrNull { it.id == pageId }
    val defaults = def?.items ?: WatchSettings.DEFAULT_PAGE_ITEMS[pageId].orEmpty()
    val setting = state.settings.pages.firstOrNull { it.id == pageId }
    val rows = setting?.items?.ifEmpty { defaults } ?: defaults
    val labels = catalog?.items?.associate { it.id to it.label }.orEmpty()
    val known = labels.keys
    val listState = rememberScalingLazyListState()
    ScalingLazyColumn(state = listState, modifier = Modifier.fillMaxSize()) {
        item { ListHeader { Text(def?.title ?: pageId) } }
        if (def?.kind == PageKind.ITEMS) {
            for (id in rows) {
                orderedToggle(
                    key = "row-$id",
                    label = labels[id] ?: id,
                    checked = true,
                    onToggle = { edit { it.togglePageItem(pageId, id, defaults) { k -> known.isEmpty() || k in known } } },
                    onMove = { up -> edit { it.movePageItem(pageId, id, up, defaults) } },
                )
            }
            for (ci in catalog?.items.orEmpty().filter { it.id !in rows }) {
                orderedToggle(
                    key = "row-${ci.id}",
                    label = ci.label,
                    checked = false,
                    onToggle = { edit { it.togglePageItem(pageId, ci.id, defaults) { k -> k in known } } },
                    onMove = null,
                )
            }
        }
        item { ListHeader { Text("Actions") } }
        for (cmd in setting?.actions.orEmpty()) {
            orderedToggle(
                key = "action-$cmd",
                label = cmd,
                checked = true,
                onToggle = { edit { it.removePageAction(pageId, cmd) } },
                onMove = null,
            )
        }
        item { Text("Add actions in the phone app: Settings › Watch.", fontSize = 10.sp, color = Dim, textAlign = TextAlign.Center) }
        item {
            Chip(
                onClick = onDone,
                label = { Text("Done") },
                colors = ChipDefaults.primaryChipColors(),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}
