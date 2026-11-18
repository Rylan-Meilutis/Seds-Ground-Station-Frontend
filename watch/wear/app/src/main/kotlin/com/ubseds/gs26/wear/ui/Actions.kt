package com.ubseds.gs26.wear.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
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
import androidx.wear.compose.foundation.lazy.ScalingLazyColumn
import androidx.wear.compose.foundation.lazy.rememberScalingLazyListState
import androidx.wear.compose.material.Button
import androidx.wear.compose.material.ButtonDefaults
import androidx.wear.compose.material.Chip
import androidx.wear.compose.material.ChipDefaults
import androidx.wear.compose.material.CircularProgressIndicator
import androidx.wear.compose.material.CompactChip
import androidx.wear.compose.material.Text
import com.ubseds.gs26.wear.DirectLink
import com.ubseds.gs26.wear.PhoneLink
import com.ubseds.gs26.wear.core.ActionOutcome
import com.ubseds.gs26.wear.core.ActionResult
import com.ubseds.gs26.wear.core.ConfirmStyle
import com.ubseds.gs26.wear.core.Mode
import com.ubseds.gs26.wear.core.PageAction
import com.ubseds.gs26.wear.core.WatchState
import com.ubseds.gs26.wear.core.actionButton
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** What the action buttons need: the shown state, the direct link, and where a press goes. */
class PageActions(
    val view: WatchState,
    val now: Long,
    val direct: DirectLink.State,
    val onPress: (PageAction) -> Unit,
) {
    /** The shown data is recent enough to act on. */
    val fresh: Boolean
        get() {
            val snap = view.snapshot ?: return false
            return snap.mode == Mode.LIVE && view.elapsedMs(now) <= snap.linkTimeoutS * 1000
        }
}

/**
 * The page's opt-in action buttons (at most two), with the station's enable
 * state and reason. A press only opens the confirmation. Logic: core
 * `actionButton`.
 */
@Composable
fun ActionsRow(actions: List<PageAction>, ctx: PageActions) {
    Row(
        Modifier.fillMaxWidth().padding(start = 24.dp, end = 24.dp, bottom = 14.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        for (action in actions) {
            val state = actionButton(action.abort, ctx.view.snapshot?.actions?.get(action.cmd), ctx.fresh, ctx.direct.configured)
            CompactChip(
                onClick = { ctx.onPress(action) },
                enabled = state.enabled,
                label = {
                    Text(
                        if (state.note != null && !state.enabled) "${action.title} · ${state.note}" else action.title,
                        fontSize = 10.sp,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                colors = if (action.abort) {
                    ChipDefaults.chipColors(backgroundColor = AlertRed, contentColor = Color.White)
                } else {
                    ChipDefaults.secondaryChipColors()
                },
                modifier = Modifier.weight(1f),
            )
        }
    }
}

/**
 * Confirmation and result for one press: Abort needs a tap on "Confirm
 * ABORT"; everything else a press-and-hold until the ring fills. The result
 * shows accepted, refused with the station's reason, or outcome unknown.
 */
@Composable
fun ActionConfirmScreen(action: PageAction, view: WatchState, link: PhoneLink, onDone: () -> Unit) {
    val scope = rememberCoroutineScope()
    var sending by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<ActionResult?>(null) }
    val style = if (action.abort) ConfirmStyle.TAP else ConfirmStyle.HOLD
    val reason = view.snapshot?.actions?.get(action.cmd)?.reason
    val send: () -> Unit = {
        if (!sending && result == null) {
            sending = true
            scope.launch {
                result = link.direct.send(action.cmd)
                sending = false
            }
        }
    }
    val listState = rememberScalingLazyListState()
    ScalingLazyColumn(state = listState, horizontalAlignment = Alignment.CenterHorizontally) {
        item { Text(action.title, fontSize = 16.sp, fontWeight = FontWeight.Bold, textAlign = TextAlign.Center) }
        val done = result
        when {
            done != null -> {
                item { ResultText(done) }
                item { Chip(onClick = onDone, label = { Text("Done") }, colors = ChipDefaults.primaryChipColors()) }
            }
            sending -> item {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    CircularProgressIndicator(Modifier.size(28.dp))
                    Text("Sending to the station…", fontSize = 11.sp, color = Dim)
                }
            }
            else -> {
                reason?.let { item { Text(it, fontSize = 11.sp, color = Dim, textAlign = TextAlign.Center) } }
                link.direct.state.value.provision?.let { p ->
                    item { Text("Station: ${p.address}", fontSize = 10.sp, color = Dim) }
                }
                if (style == ConfirmStyle.TAP) {
                    item {
                        Button(
                            onClick = send,
                            colors = ButtonDefaults.buttonColors(backgroundColor = AlertRed),
                            modifier = Modifier.size(72.dp),
                        ) { Text("Confirm\nABORT", fontSize = 11.sp, textAlign = TextAlign.Center, fontWeight = FontWeight.Bold) }
                    }
                } else {
                    item { HoldToConfirm(onConfirmed = send) }
                }
                item { Chip(onClick = onDone, label = { Text("Cancel") }, colors = ChipDefaults.secondaryChipColors()) }
            }
        }
    }
}

/** Press and hold until the ring fills (1.5 s); releasing early cancels. */
@Composable
private fun HoldToConfirm(onConfirmed: () -> Unit) {
    var holding by remember { mutableStateOf(false) }
    var progress by remember { mutableFloatStateOf(0f) }
    LaunchedEffect(holding) {
        if (!holding) {
            progress = 0f
            return@LaunchedEffect
        }
        val start = System.currentTimeMillis()
        while (holding) {
            progress = ((System.currentTimeMillis() - start) / HOLD_MS.toFloat()).coerceAtMost(1f)
            if (progress >= 1f) {
                holding = false
                onConfirmed()
                break
            }
            delay(30)
        }
    }
    Box(
        Modifier.size(80.dp).background(Color(0xFF263238), CircleShape).pointerInput(Unit) {
            detectTapGestures(onPress = {
                holding = true
                tryAwaitRelease()
                holding = false
            })
        },
        contentAlignment = Alignment.Center,
    ) {
        CircularProgressIndicator(progress = progress, modifier = Modifier.size(78.dp), indicatorColor = Amber, strokeWidth = 5.dp)
        Text(if (holding) "Keep\nholding" else "Hold to\nsend", fontSize = 11.sp, textAlign = TextAlign.Center)
    }
}

@Composable
private fun ResultText(result: ActionResult) {
    val color = when (result.outcome) {
        ActionOutcome.ACCEPTED -> Color(0xFF66BB6A)
        ActionOutcome.REFUSED -> AlertRed
        ActionOutcome.UNKNOWN -> Amber
    }
    Text(result.text, color = color, fontSize = 12.sp, textAlign = TextAlign.Center, modifier = Modifier.padding(horizontal = 12.dp))
}

private const val HOLD_MS = 1_500L
