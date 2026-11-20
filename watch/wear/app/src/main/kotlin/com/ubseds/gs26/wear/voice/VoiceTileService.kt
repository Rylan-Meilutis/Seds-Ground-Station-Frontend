package com.ubseds.gs26.wear.voice

import androidx.concurrent.futures.CallbackToFutureAdapter
import androidx.wear.protolayout.ActionBuilders
import androidx.wear.protolayout.ColorBuilders.argb
import androidx.wear.protolayout.DimensionBuilders.dp
import androidx.wear.protolayout.DimensionBuilders.expand
import androidx.wear.protolayout.DimensionBuilders.sp
import androidx.wear.protolayout.LayoutElementBuilders
import androidx.wear.protolayout.ModifiersBuilders
import androidx.wear.protolayout.ResourceBuilders
import androidx.wear.protolayout.TimelineBuilders
import androidx.wear.tiles.RequestBuilders
import androidx.wear.tiles.TileBuilders
import androidx.wear.tiles.TileService
import com.google.common.util.concurrent.ListenableFuture
import com.ubseds.gs26.wear.MainActivity

/** Wear OS tile: one big "Push to talk" button that opens the voice page and joins. */
class VoiceTileService : TileService() {
    override fun onTileRequest(requestParams: RequestBuilders.TileRequest): ListenableFuture<TileBuilders.Tile> =
        CallbackToFutureAdapter.getFuture { completer ->
            completer.set(
                TileBuilders.Tile.Builder()
                    .setResourcesVersion(RESOURCES_VERSION)
                    .setFreshnessIntervalMillis(0)
                    .setTileTimeline(TimelineBuilders.Timeline.fromLayoutElement(layout()))
                    .build(),
            )
            "voice tile"
        }

    override fun onTileResourcesRequest(requestParams: RequestBuilders.ResourcesRequest): ListenableFuture<ResourceBuilders.Resources> =
        CallbackToFutureAdapter.getFuture { completer ->
            completer.set(ResourceBuilders.Resources.Builder().setVersion(RESOURCES_VERSION).build())
            "voice tile resources"
        }

    private fun layout(): LayoutElementBuilders.LayoutElement {
        val open = ActionBuilders.LaunchAction.Builder()
            .setAndroidActivity(
                ActionBuilders.AndroidActivity.Builder()
                    .setPackageName(packageName)
                    .setClassName(MainActivity::class.java.name)
                    .addKeyToExtraMapping(
                        MainActivity.EXTRA_OPEN_PAGE,
                        ActionBuilders.AndroidStringExtra.Builder().setValue("voice").build(),
                    )
                    .build(),
            )
            .build()
        val button = LayoutElementBuilders.Box.Builder()
            .setWidth(dp(120f)).setHeight(dp(120f))
            .setModifiers(
                ModifiersBuilders.Modifiers.Builder()
                    .setClickable(ModifiersBuilders.Clickable.Builder().setId("ptt").setOnClick(open).build())
                    .setBackground(
                        ModifiersBuilders.Background.Builder()
                            .setColor(argb(0xFFFFB300.toInt()))
                            .setCorner(ModifiersBuilders.Corner.Builder().setRadius(dp(60f)).build())
                            .build(),
                    )
                    .setSemantics(ModifiersBuilders.Semantics.Builder().setContentDescription("Push to talk").build())
                    .build(),
            )
            .addContent(text("Push to talk", 16f, 0xFF000000.toInt()))
            .build()
        return LayoutElementBuilders.Column.Builder()
            .setWidth(expand()).setHeight(expand())
            .setHorizontalAlignment(LayoutElementBuilders.HORIZONTAL_ALIGN_CENTER)
            .addContent(text("GS crew voice", 12f, 0xFFBDBDBD.toInt()))
            .addContent(LayoutElementBuilders.Spacer.Builder().setHeight(dp(6f)).build())
            .addContent(button)
            .build()
    }

    private fun text(value: String, size: Float, color: Int) = LayoutElementBuilders.Text.Builder()
        .setText(value)
        .setFontStyle(LayoutElementBuilders.FontStyle.Builder().setSize(sp(size)).setColor(argb(color)).build())
        .build()

    private companion object {
        const val RESOURCES_VERSION = "1"
    }
}
