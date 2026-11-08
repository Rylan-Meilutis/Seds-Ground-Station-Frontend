package com.ubseds.gs26.wear.ui

import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.wear.compose.foundation.lazy.ScalingLazyColumn
import androidx.wear.compose.foundation.lazy.items
import androidx.wear.compose.foundation.lazy.rememberScalingLazyListState
import androidx.wear.compose.material.MaterialTheme
import androidx.wear.compose.material.Text
import com.ubseds.gs26.wear.core.ItemDisplay
import com.ubseds.gs26.wear.core.Level
import com.ubseds.gs26.wear.core.Page
import com.ubseds.gs26.wear.core.WatchState
import kotlinx.coroutines.delay

fun levelColor(level: String): Color = when (level) {
    Level.WARN -> Amber
    Level.ALERT -> AlertRed
    Level.UNKNOWN -> Dim
    else -> Color.White
}

@Composable
fun ItemsPage(page: Page, state: WatchState, now: Long) {
    val listState = rememberScalingLazyListState()
    ScalingLazyColumn(state = listState, modifier = Modifier.fillMaxSize()) {
        item { Text(page.title, style = MaterialTheme.typography.caption1, color = Dim) }
        items(page.items) { id -> ItemRow(state.item(id, now)) }
    }
}

@Composable
private fun ItemRow(item: ItemDisplay) {
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 8.dp).alpha(if (item.stale) 0.5f else 1f),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(item.label, fontSize = 11.sp, color = Dim, maxLines = 1)
        Text(
            item.text,
            fontSize = 18.sp,
            fontWeight = FontWeight.SemiBold,
            color = levelColor(item.level),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        if (item.stale && item.ageText != null) Text("stale · ${item.ageText}", fontSize = 10.sp, color = Dim)
    }
}

/** Rotates through the configured cycle items; tap pauses/resumes. */
@Composable
fun CyclePage(state: WatchState, now: Long, onLongPress: () -> Unit) {
    val cycle = state.settings.cycle
    val items = cycle.items
    var index by rememberSaveable { mutableIntStateOf(0) }
    var paused by rememberSaveable { mutableStateOf(!cycle.autostart) }
    if (items.isEmpty()) return Message("No cycle items")
    val shown = index % items.size

    LaunchedEffect(paused, cycle.intervalS, items.size) {
        while (!paused) {
            delay(cycle.intervalS * 1_000L)
            index = (index + 1) % items.size
        }
    }

    val item = state.item(items[shown], now)
    Column(
        Modifier.fillMaxSize().padding(horizontal = 20.dp)
            .pointerInput(Unit) { detectTapGestures(onTap = { paused = !paused }, onLongPress = { onLongPress() }) },
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(item.label, fontSize = 13.sp, color = Dim, maxLines = 1)
        Text(
            item.text,
            fontSize = 30.sp,
            fontWeight = FontWeight.Bold,
            color = levelColor(item.level),
            textAlign = TextAlign.Center,
            maxLines = 2,
            modifier = Modifier.alpha(if (item.stale) 0.5f else 1f),
        )
        if (item.stale && item.ageText != null) Text("stale · ${item.ageText}", fontSize = 11.sp, color = Dim)
        Text(
            "${shown + 1}/${items.size}" + if (paused) " · paused" else "",
            fontSize = 11.sp,
            color = Dim,
            modifier = Modifier.padding(top = 6.dp),
        )
    }
}
