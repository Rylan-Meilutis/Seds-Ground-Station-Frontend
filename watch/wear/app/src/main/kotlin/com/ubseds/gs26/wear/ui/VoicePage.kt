package com.ubseds.gs26.wear.ui

import android.Manifest
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.rotary.onRotaryScrollEvent
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.wear.compose.material.CompactChip
import androidx.wear.compose.material.ChipDefaults
import androidx.wear.compose.material.Text
import com.ubseds.gs26.wear.PhoneLink
import com.ubseds.gs26.wear.core.volumeAfterCrown
import com.ubseds.gs26.wear.voice.VoiceEngine
import com.ubseds.gs26.wear.voice.VoiceService
import kotlinx.coroutines.delay

/**
 * Crew voice: channel and connection, a big push-to-talk button (hold or
 * tap-to-toggle per the settings), who is talking, Join / Leave. The crown
 * (rotary input) sets the playback volume while this page is shown.
 * [joinRequest] changes when the tile asks to join.
 */
@Composable
fun VoicePage(link: PhoneLink, visible: Boolean, joinRequest: Int) {
    val context = LocalContext.current
    val engine = remember { VoiceEngine.get(context) }
    val ui by engine.ui.collectAsStateWithLifecycle()
    val state by link.state.collectAsStateWithLifecycle()
    val voice = state.settings.voice
    var micDenied by remember { mutableStateOf(false) }
    var volume by remember { mutableIntStateOf(engine.volume) }
    var volumeEdited by remember { mutableIntStateOf(0) }
    val focus = remember { FocusRequester() }
    var rotary by remember { mutableFloatStateOf(0f) }

    val permissions = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { result ->
        if (result[Manifest.permission.RECORD_AUDIO] == true) {
            micDenied = false
            VoiceService.join(context)
        } else {
            micDenied = true
        }
    }
    val join = {
        if (engine.micGranted()) {
            VoiceService.join(context)
        } else {
            val wanted = buildList {
                add(Manifest.permission.RECORD_AUDIO)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) add(Manifest.permission.POST_NOTIFICATIONS)
            }
            permissions.launch(wanted.toTypedArray())
        }
    }

    LaunchedEffect(joinRequest) { if (joinRequest > 0 && !ui.joined) join() }
    LaunchedEffect(visible) { if (visible) runCatching { focus.requestFocus() } }
    LaunchedEffect(voice.volume) { if (volumeEdited == 0) volume = voice.volume }
    // Save the crown's volume to the settings once it rests for a moment.
    LaunchedEffect(volumeEdited) {
        if (volumeEdited == 0) return@LaunchedEffect
        delay(800)
        link.editSettings { it.withVoice(it.voice.copy(volume = volume)) }
        engine.clearVolumeOverride()
        volumeEdited = 0
    }

    Column(
        Modifier.fillMaxSize().padding(horizontal = 14.dp, vertical = 22.dp)
            .onRotaryScrollEvent { e ->
                rotary += e.verticalScrollPixels
                val steps = (rotary / ROTARY_STEP_PX).toInt()
                if (steps != 0) {
                    rotary -= steps * ROTARY_STEP_PX
                    volume = volumeAfterCrown(volume, steps)
                    engine.setVolume(volume)
                    volumeEdited++
                }
                true
            }
            .focusRequester(focus)
            .focusable(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(
            "${voice.channelLabel} · " + when {
                !ui.joined -> "off"
                ui.connected -> if (ui.path == "phone") "via phone" else "direct to GS"
                else -> ui.status
            },
            fontSize = 11.sp, color = if (ui.connected) Color(0xFF66BB6A) else Dim,
            maxLines = 1, overflow = TextOverflow.Ellipsis,
        )
        val talkers = ui.talkers
        Text(
            when {
                ui.held -> "Your other device has voice"
                ui.talking -> "You are talking"
                talkers.isNotEmpty() -> "${talkers.joinToString(", ")} talking"
                ui.joined && ui.connected -> "${ui.peers} in channel · vol $volume%"
                else -> "Volume $volume%"
            },
            fontSize = 12.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
            color = if (talkers.isNotEmpty()) Amber else Color.White,
        )
        PttButton(
            enabled = ui.joined && ui.connected && !ui.held,
            talking = ui.talking,
            toggle = voice.toggle,
            onDown = { if (ui.joined) engine.ptt("down") else join() },
            onUp = { if (ui.joined) engine.ptt("up") },
        )
        val note = when {
            micDenied -> "Allow the microphone in the watch's Settings → Apps → GS26"
            ui.error != null && !ui.connected -> ui.error
            else -> null
        }
        if (note != null) {
            Text(note, fontSize = 10.sp, color = AlertRed, textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        CompactChip(
            onClick = { if (ui.joined) VoiceService.leave(context) else join() },
            label = { Text(if (ui.joined) "Leave" else "Join voice", fontSize = 12.sp) },
            colors = if (ui.joined) ChipDefaults.secondaryChipColors() else ChipDefaults.primaryChipColors(),
        )
    }
}

@Composable
private fun PttButton(enabled: Boolean, talking: Boolean, toggle: Boolean, onDown: () -> Unit, onUp: () -> Unit) {
    val color = when {
        talking -> AlertRed
        enabled -> Amber
        else -> Color(0xFF424242)
    }
    val label = when {
        talking -> if (toggle) "Tap to stop" else "Talking"
        !enabled -> "Join first"
        toggle -> "Tap to talk"
        else -> "Hold to talk"
    }
    Box(
        Modifier.size(92.dp).background(color, CircleShape)
            .semantics { contentDescription = "Push to talk" }
            .pointerInput(Unit) {
                awaitEachGesture {
                    awaitFirstDown(requireUnconsumed = false).consume()
                    onDown()
                    waitForUpOrCancellation()
                    onUp()
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = Color.Black, fontWeight = FontWeight.Bold, fontSize = 13.sp, textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth().padding(6.dp))
    }
}

/** Rotary travel for one 5 % volume step. */
private const val ROTARY_STEP_PX = 48f
