package com.ubseds.gs26.wear.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
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
    val s = state.settings
    val catalog = state.snapshot?.catalog
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
