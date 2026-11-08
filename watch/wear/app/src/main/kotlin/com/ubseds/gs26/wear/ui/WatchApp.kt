package com.ubseds.gs26.wear.ui

import android.location.Location
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.compose.currentStateAsState
import androidx.wear.compose.material.Chip
import androidx.wear.compose.material.ChipDefaults
import androidx.wear.compose.material.MaterialTheme
import androidx.wear.compose.material.Scaffold
import androidx.wear.compose.material.Text
import androidx.wear.compose.material.TimeText
import com.ubseds.gs26.wear.PhoneLink
import com.ubseds.gs26.wear.core.LinkState
import com.ubseds.gs26.wear.core.Mode
import com.ubseds.gs26.wear.core.Page
import com.ubseds.gs26.wear.core.PageKind
import com.ubseds.gs26.wear.core.WatchState
import kotlinx.coroutines.delay

val Amber = Color(0xFFFFB300)
val AlertRed = Color(0xFFFF5252)
val Dim = Color(0xFF9E9E9E)

@Composable
fun WatchApp(link: PhoneLink) {
    val state by link.state.collectAsStateWithLifecycle()
    // One shared tick drives T-clocks, ages and link-loss detection.
    val now by produceState(System.currentTimeMillis()) {
        while (true) {
            value = System.currentTimeMillis()
            delay(1_000L - value % 1_000L)
        }
    }
    var showSettings by rememberSaveable { mutableStateOf(false) }
    val watchFix = remember { mutableStateOf<Location?>(null) }

    MaterialTheme {
        Scaffold(timeText = { TimeText() }) {
            when {
                state.snapshot == null -> Message("Open the GS app on your phone")
                showSettings -> {
                    BackHandler { showSettings = false }
                    SettingsScreen(state, link, onDone = { showSettings = false })
                }
                else -> MainPager(state, now, link, watchFix, onOpenSettings = { showSettings = true })
            }
        }
    }
}

@Composable
private fun MainPager(
    state: WatchState,
    now: Long,
    link: PhoneLink,
    watchFix: MutableState<Location?>,
    onOpenSettings: () -> Unit,
) {
    val pages = state.pages()
    val count = pages.size + 1 // final page opens settings
    val pager = rememberPagerState { count }
    val resumed by LocalLifecycleOwner.current.lifecycle.currentStateAsState()
    val current: Page? = pages.getOrNull(pager.currentPage)

    LaunchedEffect(current?.id) {
        link.currentPage = current?.id ?: "settings"
        link.sendActivity(foreground = true)
    }

    Box(Modifier.fillMaxSize()) {
        HorizontalPager(state = pager, modifier = Modifier.fillMaxSize()) { index ->
            val page = pages.getOrNull(index)
            Column(
                Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures(onLongPress = { onOpenSettings() }) },
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Banners(state, now)
                Box(Modifier.fillMaxWidth().weight(1f)) {
                    when (page?.kind) {
                        null -> SettingsEntry(onOpenSettings)
                        PageKind.ITEMS -> ItemsPage(page, state, now)
                        PageKind.CYCLE -> CyclePage(state, now, onLongPress = onOpenSettings)
                        PageKind.COMPASS -> CompassPage(
                            state = state,
                            now = now,
                            visible = index == pager.currentPage && resumed.isAtLeast(Lifecycle.State.RESUMED),
                            watchFix = watchFix,
                            onLongPress = onOpenSettings,
                        )
                        else -> Message("Update the watch app")
                    }
                }
            }
        }
        PageDots(count, pager.currentPage, Modifier.align(Alignment.BottomCenter).padding(bottom = 6.dp))
    }
}

/** DEMO / no-station / link-lost banners shown on every page. */
@Composable
private fun Banners(state: WatchState, now: Long) {
    val snap = state.snapshot ?: return
    val lines = buildList {
        when (snap.mode) {
            Mode.DEMO -> add("DEMO" to Amber)
            Mode.NO_STATION -> add("Phone not connected to a station" to Amber)
        }
        (state.link(now) as? LinkState.Lost)?.let { add(it.text to AlertRed) }
    }
    Column(
        Modifier.fillMaxWidth().padding(top = 26.dp, start = 28.dp, end = 28.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        for ((text, color) in lines) {
            Text(
                text,
                color = Color.Black,
                fontSize = 10.sp,
                fontWeight = FontWeight.Bold,
                textAlign = TextAlign.Center,
                maxLines = 2,
                modifier = Modifier.padding(bottom = 2.dp).background(color, RoundedCornerShape(6.dp))
                    .padding(horizontal = 6.dp, vertical = 1.dp),
            )
        }
    }
}

@Composable
private fun SettingsEntry(onOpen: () -> Unit) {
    Column(
        Modifier.fillMaxSize().padding(horizontal = 24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Chip(
            onClick = onOpen,
            label = { Text("Settings") },
            colors = ChipDefaults.secondaryChipColors(),
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
fun Message(text: String) {
    Box(Modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
        Text(text, textAlign = TextAlign.Center, style = MaterialTheme.typography.title3)
    }
}

@Composable
fun PageDots(count: Int, selected: Int, modifier: Modifier = Modifier) {
    if (count <= 1) return
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        repeat(count) { i ->
            Spacer(
                Modifier.size(if (i == selected) 6.dp else 4.dp)
                    .background(if (i == selected) Color.White else Dim, CircleShape),
            )
        }
    }
}
