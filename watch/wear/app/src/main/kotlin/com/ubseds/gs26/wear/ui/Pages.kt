package com.ubseds.gs26.wear.ui

import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.pager.VerticalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.rotary.onRotaryScrollEvent
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
import com.ubseds.gs26.wear.core.AutoCycle
import com.ubseds.gs26.wear.core.ItemDisplay
import com.ubseds.gs26.wear.core.Level
import com.ubseds.gs26.wear.core.Page
import com.ubseds.gs26.wear.core.WatchState
import kotlinx.coroutines.launch

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

/**
 * The cycle items as a vertical pager, one big value per page, advancing
 * every `interval_s`. Rotary input (crown/bezel, the primary control) or a
 * vertical swipe moves through the items and restarts the wait, so the item
 * being read stays for a full interval; tap pauses/resumes. Logic:
 * core AutoCycle (shared vectors with the Apple Watch).
 */
@Composable
fun CyclePage(state: WatchState, now: Long, visible: Boolean, onLongPress: () -> Unit) {
    val cycle = state.settings.cycle
    val items = cycle.items
    if (items.isEmpty()) return Message("No cycle items. Add some in Settings.")
    val auto = remember { AutoCycle(System.currentTimeMillis(), cycle.autostart) }
    val pager = rememberPagerState { items.size }
    val scope = rememberCoroutineScope()
    val focus = remember { FocusRequester() }
    var rotary by remember { mutableFloatStateOf(0f) }

    // Timer: one tick per second while on screen.
    LaunchedEffect(now, visible) {
        if (visible && auto.tick(System.currentTimeMillis(), cycle.intervalS, items.size)) {
            pager.animateScrollToPage(auto.index)
        }
    }
    // Swipe or rotary settled on a page: a user move restarts the wait.
    LaunchedEffect(pager) {
        snapshotFlow { pager.settledPage }.collect { auto.observe(it, System.currentTimeMillis()) }
    }
    // Rotary input goes to this pager whenever the page is on screen.
    LaunchedEffect(visible) { if (visible) runCatching { focus.requestFocus() } }

    Box(
        Modifier.fillMaxSize()
            .onRotaryScrollEvent { event ->
                rotary += event.verticalScrollPixels
                val step = when {
                    rotary > ROTARY_STEP_PX -> 1
                    rotary < -ROTARY_STEP_PX -> -1
                    else -> 0
                }
                if (step != 0) {
                    rotary = 0f
                    val target = (pager.currentPage + step).coerceIn(0, items.size - 1)
                    scope.launch { pager.animateScrollToPage(target) }
                }
                true
            }
            .focusRequester(focus)
            .focusable()
            .pointerInput(Unit) {
                detectTapGestures(onTap = { auto.togglePause(System.currentTimeMillis()) }, onLongPress = { onLongPress() })
            },
    ) {
        VerticalPager(state = pager, modifier = Modifier.fillMaxSize()) { index ->
            val item = state.item(items[index], now)
            Column(
                Modifier.fillMaxSize().padding(horizontal = 20.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(item.label, fontSize = 12.sp, color = Dim, maxLines = 1)
                Text(
                    item.text,
                    fontSize = 26.sp,
                    fontWeight = FontWeight.Bold,
                    color = levelColor(item.level),
                    textAlign = TextAlign.Center,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.alpha(if (item.stale) 0.5f else 1f),
                )
                if (item.stale && item.ageText != null) Text("stale · ${item.ageText}", fontSize = 10.sp, color = Dim)
            }
        }
        // Small page indicator + countdown, secondary style.
        Text(
            auto.footer(now, cycle.intervalS, items.size),
            fontSize = 10.sp,
            color = Dim,
            maxLines = 1,
            modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 18.dp),
        )
    }
}

/** Rotary pixels per item step (one crown/bezel detent is roughly this much). */
private const val ROTARY_STEP_PX = 40f
