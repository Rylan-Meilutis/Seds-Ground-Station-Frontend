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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.compose.currentStateAsState
import androidx.wear.compose.foundation.lazy.ScalingLazyColumn
import androidx.wear.compose.foundation.lazy.rememberScalingLazyListState
import androidx.wear.compose.material.Chip
import androidx.wear.compose.material.ChipDefaults
import androidx.wear.compose.material.MaterialTheme
import androidx.wear.compose.material.Scaffold
import androidx.wear.compose.material.Text
import androidx.wear.compose.material.TimeText
import com.ubseds.gs26.wear.MainActivity
import com.ubseds.gs26.wear.PhoneLink
import com.ubseds.gs26.wear.core.ConnectionStatus
import com.ubseds.gs26.wear.core.Direct
import com.ubseds.gs26.wear.core.DirectInfo
import com.ubseds.gs26.wear.core.LinkRoute
import com.ubseds.gs26.wear.core.PageAction
import com.ubseds.gs26.wear.core.ItemDisplay
import com.ubseds.gs26.wear.core.Mode
import com.ubseds.gs26.wear.core.Page
import com.ubseds.gs26.wear.core.PageKind
import com.ubseds.gs26.wear.core.StatusLine
import com.ubseds.gs26.wear.core.StatusTone
import com.ubseds.gs26.wear.core.WatchState
import com.ubseds.gs26.wear.core.alertRows
import kotlinx.coroutines.delay

val Amber = Color(0xFFFFB300)
val AlertRed = Color(0xFFFF5252)
val Dim = Color(0xFF9E9E9E)

@Composable
fun WatchApp(link: PhoneLink) {
    val state by link.state.collectAsStateWithLifecycle()
    val phoneReachable by link.phoneReachable.collectAsStateWithLifecycle()
    // One shared tick drives T-clocks, ages and link-loss detection.
    val now by produceState(System.currentTimeMillis()) {
        while (true) {
            value = System.currentTimeMillis()
            delay(1_000L - value % 1_000L)
        }
    }
    val direct by link.direct.state.collectAsStateWithLifecycle()
    var showSettings by rememberSaveable { mutableStateOf(false) }
    var confirming by remember { mutableStateOf<PageAction?>(null) }
    val watchFix = remember { mutableStateOf<Location?>(null) }
    // Phone while its data is fresh, else the station itself (core Direct).
    val route = Direct.chooseRoute(
        phoneMode = state.snapshot?.mode,
        phoneAgeMs = state.snapshot?.let { state.elapsedMs(now) },
        directAgeMs = direct.snapshot?.let { (now - direct.receivedAtMs).coerceAtLeast(0) },
        freshMs = Direct.phoneFreshMs(state.settings.power),
    )
    val view = state.viewing(route, direct.snapshot, direct.receivedAtMs, direct.provision?.address.orEmpty())
    val status = ConnectionStatus.of(
        view.snapshot, view.elapsedMs(now), phoneReachable, route,
        DirectInfo(direct.configured, direct.polling, direct.error),
    )
    val actions = PageActions(view, now, direct, onPress = { confirming = it })
    // The tile / notification asks for the voice page (and to join).
    val openPage by MainActivity.openPage.collectAsStateWithLifecycle()
    var joinRequest by remember { mutableIntStateOf(0) }
    var voiceOverlay by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(openPage) {
        if (openPage == "voice") {
            joinRequest++
            confirming = null
            showSettings = false
            // Without a voice page in the layout, show it on its own.
            voiceOverlay = view.pages().none { it.kind == PageKind.VOICE }
            if (voiceOverlay) MainActivity.openPage.value = null
        }
    }

    MaterialTheme {
        Scaffold(timeText = { TimeText() }) {
            val pending = confirming
            when {
                pending != null -> {
                    BackHandler { confirming = null }
                    ActionConfirmScreen(pending, view, link, onDone = { confirming = null })
                }
                voiceOverlay -> {
                    BackHandler { voiceOverlay = false }
                    VoicePage(link, visible = true, joinRequest = joinRequest)
                }
                showSettings && state.snapshot != null -> {
                    BackHandler { showSettings = false }
                    SettingsScreen(state, link, onDone = { showSettings = false })
                }
                else -> MainPager(view, status, route, now, link, watchFix, actions, joinRequest, onOpenSettings = { showSettings = true })
            }
        }
    }
}

/**
 * Horizontal pages: Status (the landing page), Alerts, the pages chosen in
 * the watch settings (the cycle page pages vertically), Settings last.
 */
@Composable
private fun MainPager(
    state: WatchState,
    status: ConnectionStatus,
    route: LinkRoute,
    now: Long,
    link: PhoneLink,
    watchFix: MutableState<Location?>,
    actions: PageActions,
    joinRequest: Int,
    onOpenSettings: () -> Unit,
) {
    val pages = state.pages()
    val count = FIXED_PAGES + pages.size + 1 // final page opens settings
    val pager = rememberPagerState { count }
    val resumed by LocalLifecycleOwner.current.lifecycle.currentStateAsState()
    val current: Page? = pages.getOrNull(pager.currentPage - FIXED_PAGES)
    val currentName = when (pager.currentPage) {
        0 -> "status"
        1 -> "alerts"
        count - 1 -> "settings"
        else -> current?.id
    }

    val openPage by MainActivity.openPage.collectAsStateWithLifecycle()
    LaunchedEffect(openPage, pages.map { it.id }) {
        val target = openPage ?: return@LaunchedEffect
        val index = pages.indexOfFirst { it.id == target || (target == "voice" && it.kind == PageKind.VOICE) }
        if (index >= 0) pager.scrollToPage(FIXED_PAGES + index)
        MainActivity.openPage.value = null
    }

    LaunchedEffect(currentName) {
        link.currentPage = currentName
        link.sendActivity(foreground = true)
    }

    Box(Modifier.fillMaxSize()) {
        HorizontalPager(state = pager, modifier = Modifier.fillMaxSize()) { index ->
            val page = pages.getOrNull(index - FIXED_PAGES)
            val visible = index == pager.currentPage && resumed.isAtLeast(Lifecycle.State.RESUMED)
            Column(
                Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures(onLongPress = { onOpenSettings() }) },
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                if (index >= FIXED_PAGES) Banners(state, status)
                Box(Modifier.fillMaxWidth().weight(1f)) {
                    when {
                        index == 0 -> StatusPage(state, status, route, now)
                        index == 1 -> AlertsPage(state, status)
                        page == null -> SettingsEntry(onOpenSettings, enabled = state.snapshot != null)
                        page.kind == PageKind.ITEMS -> ItemsPage(page, state, now)
                        page.kind == PageKind.CYCLE -> CyclePage(state, now, visible, onLongPress = onOpenSettings)
                        page.kind == PageKind.VOICE -> VoicePage(link, visible, joinRequest)
                        page.kind == PageKind.COMPASS -> CompassPage(
                            state = state,
                            now = now,
                            visible = visible,
                            watchFix = watchFix,
                            link = link,
                            onLongPress = onOpenSettings,
                        )
                        else -> Message("Update the watch app")
                    }
                }
                // Opt-in page actions stay pinned below the content.
                if (page != null && page.actions.isNotEmpty()) ActionsRow(page.actions, actions)
            }
        }
        PageDots(count, pager.currentPage, Modifier.align(Alignment.BottomCenter).padding(bottom = 6.dp))
    }
}

/** Status and Alerts come before the configured pages. */
private const val FIXED_PAGES = 2

fun toneColor(tone: StatusTone): Color = when (tone) {
    StatusTone.OK -> Color(0xFF66BB6A)
    StatusTone.WAITING -> Dim
    StatusTone.WARN -> Amber
    StatusTone.ALERT -> AlertRed
}

/** DEMO / station / phone-link banners on the configured pages. */
@Composable
private fun Banners(state: WatchState, status: ConnectionStatus) {
    val lines = buildList {
        val station = status.station
        if (state.snapshot?.mode == Mode.DEMO) add("DEMO" to Amber)
        else if (station != null && station.tone != StatusTone.OK) add(station.text to toneColor(station.tone))
        if (status.phone.tone == StatusTone.WARN || status.phone.tone == StatusTone.ALERT) {
            add(status.phone.text to toneColor(status.phone.tone))
        }
        status.direct?.takeIf { it.tone == StatusTone.ALERT }?.let { add(it.text to toneColor(it.tone)) }
    }
    if (lines.isEmpty()) return
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
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(bottom = 2.dp).background(color, RoundedCornerShape(6.dp))
                    .padding(horizontal = 6.dp, vertical = 1.dp),
            )
        }
    }
}

/** Landing page: flight state, T-clock, the data path ("Via phone" / "Direct to GS") and the station. */
@Composable
private fun StatusPage(state: WatchState, status: ConnectionStatus, route: LinkRoute, now: Long) {
    val listState = rememberScalingLazyListState()
    ScalingLazyColumn(state = listState, modifier = Modifier.fillMaxSize()) {
        if (state.snapshot?.mode == Mode.DEMO) item { Text("DEMO", fontSize = 11.sp, color = Amber) }
        for (id in listOf("flight_state", "t_clock")) {
            if (state.snapshot?.items?.containsKey(id) == true) item { StatusItem(state.item(id, now)) }
        }
        item { StatusText(status.phone) }
        status.station?.let { item { StatusText(it) } }
        status.direct?.let { item { StatusText(it) } }
        if (route == LinkRoute.DIRECT) item { Text("Phone app not needed right now", fontSize = 10.sp, color = Dim) }
        if (status.openPhoneApp && status.phone.text != ConnectionStatus.OPEN_PHONE_APP) {
            item { Text(ConnectionStatus.OPEN_PHONE_APP, fontSize = 11.sp, color = Dim, textAlign = TextAlign.Center) }
        }
    }
}

@Composable
private fun StatusItem(item: ItemDisplay) {
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
        Text(item.label, fontSize = 11.sp, color = Dim, maxLines = 1)
        Text(
            item.text,
            fontSize = 16.sp,
            fontWeight = FontWeight.SemiBold,
            color = levelColor(item.level),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun StatusText(line: StatusLine) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Spacer(Modifier.size(6.dp).background(toneColor(line.tone), CircleShape))
        Spacer(Modifier.width(6.dp))
        Text(line.text, fontSize = 12.sp, maxLines = 3, overflow = TextOverflow.Ellipsis)
    }
}

/** Items the phone flags at warn or alert level, plus link problems. */
@Composable
private fun AlertsPage(state: WatchState, status: ConnectionStatus) {
    val rows = alertRows(state.snapshot)
    val listState = rememberScalingLazyListState()
    ScalingLazyColumn(state = listState, modifier = Modifier.fillMaxSize()) {
        item { Text("Alerts", style = MaterialTheme.typography.caption1, color = Dim) }
        if (status.phone.tone == StatusTone.WARN || status.phone.tone == StatusTone.ALERT) item { StatusText(status.phone) }
        status.station?.takeIf { it.tone == StatusTone.WARN || it.tone == StatusTone.ALERT }?.let { item { StatusText(it) } }
        status.direct?.takeIf { it.tone == StatusTone.ALERT }?.let { item { StatusText(it) } }
        items(rows.size) { i ->
            val row = rows[i]
            Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(row.label, fontSize = 11.sp, color = Dim, maxLines = 1)
                Text(row.text, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, color = levelColor(row.level), maxLines = 1)
            }
        }
        if (rows.isEmpty()) {
            item {
                Text(if (state.snapshot == null) "No data from the phone yet" else "No alerts", fontSize = 12.sp, color = Dim)
            }
        }
    }
}

@Composable
private fun SettingsEntry(onOpen: () -> Unit, enabled: Boolean) {
    Column(
        Modifier.fillMaxSize().padding(horizontal = 24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Chip(
            onClick = onOpen,
            enabled = enabled,
            label = { Text("Settings") },
            colors = ChipDefaults.secondaryChipColors(),
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
fun Message(text: String) {
    Box(Modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
        Text(text, textAlign = TextAlign.Center, style = MaterialTheme.typography.body2)
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
