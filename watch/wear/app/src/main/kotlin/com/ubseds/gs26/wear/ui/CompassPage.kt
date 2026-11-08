package com.ubseds.gs26.wear.ui

import android.Manifest
import android.location.Location
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.wear.compose.material.Chip
import androidx.wear.compose.material.ChipDefaults
import androidx.wear.compose.material.Text
import com.ubseds.gs26.wear.WatchLocationEffect
import com.ubseds.gs26.wear.core.Compass
import com.ubseds.gs26.wear.core.Geodesy
import com.ubseds.gs26.wear.core.LatLon
import com.ubseds.gs26.wear.core.WatchSettings
import com.ubseds.gs26.wear.core.WatchState
import com.ubseds.gs26.wear.declinationAt
import com.ubseds.gs26.wear.hasLocationPermission
import com.ubseds.gs26.wear.rememberHeading
import kotlin.math.abs

/**
 * Recovery compass. GPS and the rotation-vector sensor run only while this
 * page is [visible]; the arrow is target bearing minus true heading.
 */
@Composable
fun CompassPage(
    state: WatchState,
    now: Long,
    visible: Boolean,
    watchFix: MutableState<Location?>,
    onLongPress: () -> Unit,
) {
    val context = LocalContext.current
    val snapshot = state.snapshot ?: return
    val settings = state.settings
    var permitted by rememberSaveable { mutableStateOf(hasLocationPermission(context)) }
    var asked by rememberSaveable { mutableStateOf(false) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { r ->
        permitted = r[Manifest.permission.ACCESS_FINE_LOCATION] == true || hasLocationPermission(context)
    }
    val requestPermission = {
        launcher.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION))
    }
    LaunchedEffect(visible) {
        if (visible && !permitted && !asked) { asked = true; requestPermission() }
    }

    WatchLocationEffect(visible, settings.power == WatchSettings.POWER_RECOVERY, permitted, watchFix)
    val heading by rememberHeading(visible)

    val targets = Compass.orderedTargets(snapshot, settings)
    if (targets.isEmpty()) return Message("No compass targets from the phone")
    var selectedId by rememberSaveable { mutableStateOf(targets.first().id) }
    val selected = targets.indexOfFirst { it.id == selectedId }.coerceAtLeast(0)
    fun step(by: Int) { selectedId = targets[(selected + by).mod(targets.size)].id }

    val fix = watchFix.value
    val declination = declinationAt(fix)
    val headingTrue = heading?.let { Geodesy.magneticToTrueDeg(it.magneticDeg, declination) }
    val view = Compass.view(
        targets[selected], fix?.let { LatLon(it.latitude, it.longitude) }, headingTrue, declination,
        settings, state.receivedAtMs, now,
    )

    Box(
        Modifier.fillMaxSize()
            .pointerInput(targets.size, selected) {
                detectTapGestures(onTap = { step(1) }, onLongPress = { onLongPress() })
            }
            .pointerInput(targets.size, selected) {
                var total = 0f
                detectVerticalDragGestures(
                    onDragStart = { total = 0f },
                    onDragEnd = { if (abs(total) > 40f) step(if (total < 0) 1 else -1) },
                    onVerticalDrag = { _, dy -> total += dy },
                )
            },
        contentAlignment = Alignment.Center,
    ) {
        view.arrowDeg?.let { Arrow(it, if (view.stale) Dim else Color(0xFF4FC3F7)) }

        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(view.label, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                if (view.stale) {
                    Text(
                        "STALE", fontSize = 9.sp, color = Color.Black, fontWeight = FontWeight.Bold,
                        modifier = Modifier.padding(start = 4.dp).background(Amber, RoundedCornerShape(4.dp))
                            .padding(horizontal = 3.dp),
                    )
                }
            }
            view.fallbackLabel?.let { Text(it, fontSize = 10.sp, color = Amber) }
            Text(view.distanceText, fontSize = 30.sp, fontWeight = FontWeight.Bold)
            when {
                !permitted -> Chip(
                    onClick = { requestPermission() },
                    label = { Text("Allow location", fontSize = 12.sp) },
                    colors = ChipDefaults.secondaryChipColors(),
                )
                fix == null -> Hint("Waiting for watch GPS")
                view.nearText != null -> Hint(view.nearText!!, Amber)
                else -> view.bearingText?.let { Text(it, fontSize = 12.sp) }
            }
            if (heading?.needsCalibration == true) Hint("Move watch in a figure-8", Amber)
            Text("fix ${view.ageText}", fontSize = 10.sp, color = Dim)
            if (targets.size > 1) Text("${selected + 1}/${targets.size} · tap for next", fontSize = 9.sp, color = Dim)
        }
    }
}

@Composable
private fun Hint(text: String, color: Color = Dim) {
    Text(text, fontSize = 11.sp, color = color, textAlign = TextAlign.Center, modifier = Modifier.padding(horizontal = 24.dp))
}

/** Arrow around the rim, pointing [deg] clockwise from 12 o'clock. */
@Composable
private fun Arrow(deg: Double, color: Color) {
    Canvas(Modifier.fillMaxSize()) {
        rotate(deg.toFloat()) {
            val c = center
            val r = size.minDimension / 2f
            val tip = Offset(c.x, c.y - r + 4f)
            val base = c.y - r + r * 0.28f
            val half = r * 0.14f
            val path = Path().apply {
                moveTo(tip.x, tip.y)
                lineTo(c.x + half, base)
                lineTo(c.x, base - half * 0.5f)
                lineTo(c.x - half, base)
                close()
            }
            drawPath(path, color)
        }
    }
}
