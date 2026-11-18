package com.ubseds.gs26.wear.core

import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.int
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.longOrNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/** link_route, action_outcome, action_button, page_layout, provision and direct_status vectors (DirectTests.swift checks the same ones). */
class DirectVectorsTest {
    private val vectors: JsonObject by lazy { Json.parseToJsonElement(vectorFile().readText()).jsonObject }

    private fun vectorFile(): File {
        System.getProperty("gs26.testVectors")?.let { File(it) }?.takeIf { it.isFile }?.let { return it }
        var dir: File? = File("").absoluteFile
        while (dir != null) {
            listOf(File(dir, "test-vectors.json"), File(dir, "watch/test-vectors.json"))
                .firstOrNull { it.isFile }?.let { return it }
            dir = dir.parentFile
        }
        error("watch/test-vectors.json not found from ${File("").absolutePath}")
    }

    private fun cases(key: String) = vectors[key]!!.jsonArray.map { it.jsonObject }
    private fun JsonObject.str(key: String) = this[key]?.takeIf { it != JsonNull }?.jsonPrimitive?.content

    @Test
    fun linkRouteVectors() {
        val all = cases("link_route")
        assertTrue(all.size >= 10)
        for (c in all) {
            val name = c.str("name")!!
            val phone = c["phone"]?.takeIf { it != JsonNull }?.jsonObject
            val mode = phone?.str("mode")
            val phoneAge = phone?.get("age_ms")?.jsonPrimitive?.long
            val directAge = c["direct_age_ms"]?.takeIf { it != JsonNull }?.jsonPrimitive?.longOrNull
            val fresh = c["fresh_ms"]!!.jsonPrimitive.long
            val route = Direct.chooseRoute(mode, phoneAge, directAge, fresh)
            assertEquals(name, c.str("route")!!.uppercase(), route.name)
            val poll = Direct.shouldPoll(
                mode, phoneAge, fresh, c["foreground"]!!.jsonPrimitive.boolean, c["configured"]!!.jsonPrimitive.boolean,
            )
            assertEquals(name, c["poll"]!!.jsonPrimitive.boolean, poll)
        }
        assertEquals(6_000L, Direct.phoneFreshMs(WatchSettings.POWER_BALANCED))
        assertEquals(15_000L, Direct.phoneFreshMs(WatchSettings.POWER_SAVER))
        assertEquals(2_000L, Direct.pollMs(WatchSettings.POWER_RECOVERY))
    }

    @Test
    fun actionOutcomeVectors() {
        for (c in cases("action_outcome")) {
            val r = actionOutcome(c["status"]?.takeIf { it != JsonNull }?.jsonPrimitive?.intOrNull, c.str("body")!!, c.str("error"))
            assertEquals(c.str("name"), c.str("kind")!!.uppercase(), r.outcome.name)
            assertEquals(c.str("name"), c.str("text"), r.text)
        }
    }

    @Test
    fun actionButtonVectors() {
        for (c in cases("action_button")) {
            val name = c.str("name")
            val b = actionButton(
                abort = c["abort"]!!.jsonPrimitive.boolean,
                state = ActionState(c["enabled"]!!.jsonPrimitive.boolean, c.str("reason")),
                fresh = c["fresh"]!!.jsonPrimitive.boolean,
                token = c["token"]!!.jsonPrimitive.boolean,
            )
            val expect = c["expect"]!!.jsonObject
            assertEquals(name, expect["enabled"]!!.jsonPrimitive.boolean, b.enabled)
            assertEquals(name, expect.str("note"), b.note)
            assertEquals(name, expect.str("confirm")!!.uppercase(), b.confirm.name)
        }
    }

    @Test
    fun pageLayoutVectors() {
        val pageList = ListSerializer(Page.serializer())
        val all = cases("page_layout")
        assertTrue(all.size >= 4)
        for (c in all) {
            val name = c.str("name")
            val settings = Protocol.json.decodeFromJsonElement(WatchSettings.serializer(), c["settings"]!!)
            val catalog = Protocol.json.decodeFromJsonElement(pageList, c["catalog_pages"]!!)
            val published = Protocol.json.decodeFromJsonElement(pageList, c["published"]!!)
            val pages = resolvedPages(settings, catalog, published)
            val expect = c["expect"]!!.jsonArray.map { it.jsonObject }
            assertEquals(name, expect.map { it.str("id") }, pages.map { it.id })
            for ((page, want) in pages.zip(expect)) {
                assertEquals("$name ${page.id}", want.str("kind"), page.kind)
                assertEquals("$name ${page.id}", want["items"]!!.jsonArray.map { it.jsonPrimitive.content }, page.items)
                assertEquals("$name ${page.id}", want["actions"]!!.jsonArray.map { it.jsonPrimitive.content }, page.actions.map { it.cmd })
                assertTrue(page.actions.size <= WatchSettings.MAX_PAGE_ACTIONS)
            }
        }
    }

    @Test
    fun provisionVectors() {
        for (c in cases("provision")) {
            val name = c.str("name")
            val p = DirectProvision.parse(c["json"]!!.toString())
            val want = c["expect"]?.takeIf { it != JsonNull }?.jsonObject
            if (want == null) {
                assertNull(name, p)
                continue
            }
            requireNotNull(p) { "$name did not parse" }
            assertEquals(name, want["configured"]!!.jsonPrimitive.boolean, p.configured)
            assertEquals(name, want.str("base"), p.base)
            assertEquals(name, want.str("address"), p.address)
            assertEquals(name, want.str("token"), p.token)
            assertEquals(name, want["commands"]!!.jsonArray.map { it.jsonPrimitive.content }, p.commands)
        }
    }

    @Test
    fun directStatusMerge() {
        val m = vectors["direct_status"]!!.jsonObject["merge"]!!.jsonObject
        val base = Protocol.parseSnapshot(m["base"]!!.toString())!!
        val direct = Protocol.parseSnapshot(m["direct"]!!.toString())!!
        val merged = direct.mergedDirect(base, m.str("address")!!)
        val e = m["expect"]!!.jsonObject
        assertEquals(e.str("mode"), merged.mode)
        assertEquals(e.str("source"), merged.source)
        assertEquals(e.str("station_address"), merged.station?.address)
        assertEquals(e.str("station_link"), merged.station?.link)
        assertEquals(e["page_ids"]!!.jsonArray.map { it.jsonPrimitive.content }, merged.pages.map { it.id })
        assertEquals(e.str("altitude"), merged.items["altitude"]?.text)
        assertEquals(e["targets"]!!.jsonPrimitive.int, merged.targets.size)
        assertEquals(e["dump_enabled"]!!.jsonPrimitive.boolean, merged.actions["Dump"]?.enabled)
        assertEquals(e.str("dump_reason"), merged.actions["Dump"]?.reason)
        assertEquals(e.str("units"), merged.settings.units)
        assertEquals(listOf("Abort", "Dump"), Direct.pageActionCommands(merged.pages))
        // The merged state renders the phone's pages with the station's values.
        val state = WatchState(base, 1_000).viewing(LinkRoute.DIRECT, direct, 5_000, "gs.local:3000")
        assertEquals(5_000L, state.receivedAtMs)
        assertEquals("1234 m", state.item("altitude", 5_000).text)
        assertEquals(listOf("flight"), state.pages().map { it.id })
        assertEquals(base, WatchState(base, 1_000).viewing(LinkRoute.PHONE, direct, 5_000, "x").snapshot)
    }

    @Test
    fun requests() {
        assertEquals(
            "https://gs.local:3000/api/watch/status?units=imperial&stale_after_s=30&actions=Abort,Dump",
            Direct.statusUrl("https://gs.local:3000", metric = false, actions = listOf("Abort", "Dump"), staleAfterS = 30),
        )
        assertEquals("\"Abort\"", Direct.commandBody("Abort"))
    }

    @Test
    fun pageItemEditing() {
        val defaults = WatchSettings.DEFAULT_PAGE_ITEMS["flight"]!!
        var s = WatchSettings()
        s = s.copy(pages = s.pages.map { if (it.id == "flight") it.copy(items = emptyList()) else it })
        s = s.togglePageItem("flight", "tank_pressure", defaults)
        assertEquals(defaults + "tank_pressure", s.pages[0].items)
        s = s.movePageItem("flight", "tank_pressure", up = true, defaults = defaults)
        assertEquals(listOf("flight_state", "t_clock", "altitude", "tank_pressure", "velocity"), s.pages[0].items)
        for (id in s.pages[0].items) s = s.togglePageItem("flight", id, defaults)
        assertEquals(listOf("velocity"), s.pages[0].items)
        for (i in 0 until 20) s = s.togglePageItem("flight", "item$i", defaults)
        assertEquals(WatchSettings.MAX_PAGE_ITEMS, s.pages[0].items.size)
        s = s.copy(pages = s.pages.map { if (it.id == "flight") it.copy(actions = listOf("Abort", "Dump")) else it })
        assertEquals(listOf("Dump"), s.removePageAction("flight", "Abort").pages[0].actions)
        assertEquals(2, WatchSettings().version)
        assertTrue(WatchSettings().directLink)
    }
}
