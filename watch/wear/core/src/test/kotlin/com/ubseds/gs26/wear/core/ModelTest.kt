package com.ubseds.gs26.wear.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ModelTest {
    /** The documented example (docs/watch-companion.md) with the elided parts filled in. */
    private val example = """
        {"v":1,"seq":42,"mode":"live",
         "station":{"name":"GroundStation","role":"primary","link":"connected","telemetry_age_ms":350},
         "account":{"signed_in":true,"sync":"synced"},
         "items":{"flight_state":{"label":"Flight state","text":"Descent","unit":"","age_ms":0,"stale":false,"level":"normal"},
                  "t_clock":{"label":"T-clock","text":"T+ 01:23","unit":"","age_ms":0,"stale":false,"level":"normal","clock":{"kind":"tplus","ms":83000}},
                  "altitude":{"label":"Altitude","text":"1234","value":1234.0,"unit":"m","age_ms":200,"stale":false,"level":"normal"}},
         "pages":[{"id":"flight","kind":"items","title":"Flight","items":["flight_state","t_clock","altitude","velocity"]},
                  {"id":"cycle","kind":"cycle","title":"Cycle","items":[]},
                  {"id":"compass","kind":"compass","title":"Recovery","items":[]}],
         "targets":[{"id":"rocket","label":"Rocket","lat":32.99,"lon":-106.97,"alt_m":1401.0,"age_ms":3000,"stale":false,"source":"Rocket GPS","fallback":false},
                    {"id":"ground_station","label":"Ground station","lat":32.98,"lon":-106.98,"alt_m":null,"age_ms":0,"stale":false,"source":"Saved GroundStation (manual)","fallback":true}],
         "settings":{"version":1,"updated_ms":1730000000000,"updated_by":"phone",
                  "pages":[{"id":"flight","enabled":true},{"id":"propulsion","enabled":true},{"id":"trajectory","enabled":true},
                           {"id":"health","enabled":true},{"id":"cycle","enabled":true},{"id":"compass","enabled":true}],
                  "cycle":{"items":["flight_state","t_clock","altitude","velocity","tank_pressure"],"interval_s":5,"autostart":true},
                  "compass":{"north":"true","stale_after_s":30,"targets":["rocket","ground_station","rrls"]},
                  "units":"metric","power":"balanced"},
         "catalog":{"items":[{"id":"altitude","label":"Altitude","group":"Trajectory"}],
                    "pages":[{"id":"flight","kind":"items","title":"Flight","items":["flight_state"]},
                             {"id":"propulsion","kind":"items","title":"Propulsion","items":["tank_pressure"]},
                             {"id":"cycle","kind":"cycle","title":"Cycle","items":[]},
                             {"id":"compass","kind":"compass","title":"Recovery","items":[]}],
                    "targets":[{"id":"rocket","label":"Rocket"}]},
         "link_timeout_s":15,
         "future_top_level":{"x":1}}
    """.trimIndent()

    private fun snap() = Protocol.parseSnapshot(example)!!

    @Test
    fun parsesDocumentedSnapshot() {
        val s = snap()
        assertEquals(42L, s.seq)
        assertEquals(Mode.LIVE, s.mode)
        assertEquals("GroundStation", s.station!!.name)
        assertEquals(350L, s.station!!.telemetryAgeMs)
        assertEquals(listOf("flight", "cycle", "compass"), s.pages.map { it.id })
        assertEquals(83000L, s.items["t_clock"]!!.clock!!.ms)
        assertEquals(1234.0, s.items["altitude"]!!.value!!, 0.0)
        assertNull(s.targets[1].altM)
        assertTrue(s.targets[1].fallback)
        assertEquals(1730000000000L, s.settings.updatedMs)
        assertEquals(5, s.settings.cycle.intervalS)
        assertEquals(15L, s.linkTimeoutS)
    }

    @Test
    fun rejectsBadSnapshots() {
        assertNull(Protocol.parseSnapshot("not json"))
        assertNull(Protocol.parseSnapshot("""{"v":2,"seq":1}"""))
        assertNull(Protocol.parseSnapshot("{\"v\":1,\"pad\":\"" + "x".repeat(Protocol.MAX_MESSAGE_BYTES) + "\"}"))
        // Minimal valid snapshot gets defaults.
        val s = Protocol.parseSnapshot("""{"v":1,"mode":"demo"}""")!!
        assertEquals(Mode.DEMO, s.mode)
        assertTrue(s.pages.isEmpty())
    }

    @Test
    fun unknownPageKindIsKeptForTheUpdateCard() {
        val s = Protocol.parseSnapshot("""{"v":1,"pages":[{"id":"map","kind":"map3d","title":"Map","extra":true}]}""")!!
        assertEquals("map3d", s.pages.single().kind)
        assertFalse(s.pages.single().kind in PageKind.KNOWN)
    }

    @Test
    fun outgoingMessagesAreOnlyTheThreeAllowedTypes() {
        val hello = Json.parseToJsonElement(Protocol.helloMessage()).jsonObject
        assertEquals("hello", hello["type"]!!.jsonPrimitive.content)
        assertEquals(1, hello["v"]!!.jsonPrimitive.content.toInt())
        val act = Json.parseToJsonElement(Protocol.activityMessage(true, "compass")).jsonObject
        assertEquals("activity", act["type"]!!.jsonPrimitive.content)
        assertEquals("compass", act["page"]!!.jsonPrimitive.content)
        val set = Json.parseToJsonElement(Protocol.settingsMessage(WatchSettings().stamped(0, 5))).jsonObject
        assertEquals("settings", set["type"]!!.jsonPrimitive.content)
        val inner = set["settings"]!!.jsonObject
        assertEquals("watch", inner["updated_by"]!!.jsonPrimitive.content)
        assertEquals(5, inner["cycle"]!!.jsonObject["interval_s"]!!.jsonPrimitive.content.toInt())
    }

    @Test
    fun settingsEditingKeepsLimits() {
        var s = WatchSettings()
        s = s.movePage("flight", up = true)
        assertEquals("flight", s.pages[0].id)
        s = s.movePage("compass", up = true)
        assertEquals("compass", s.pages[s.pages.size - 2].id)
        s = s.movePage("compass", up = false).movePage("compass", up = false)
        assertEquals("compass", s.pages.last().id)
        s = s.togglePage("health")
        assertFalse(s.pages.first { it.id == "health" }.enabled)

        s = s.copy(cycle = s.cycle.copy(items = listOf("altitude")))
        s = s.toggleCycleItem("altitude")
        assertEquals(listOf("altitude"), s.cycle.items)
        s = s.toggleCycleItem("velocity").moveCycleItem("velocity", up = true)
        assertEquals(listOf("velocity", "altitude"), s.cycle.items)
        s = s.toggleCycleItem("unknown") { it in setOf("altitude", "velocity") }
        assertEquals(2, s.cycle.items.size)
        s = s.copy(cycle = s.cycle.copy(items = (1..12).map { "i$it" }))
        assertEquals(12, s.toggleCycleItem("i13").cycle.items.size)

        assertEquals(2, s.withInterval(0).cycle.intervalS)
        assertEquals(60, s.withInterval(999).cycle.intervalS)
        assertEquals(7, s.withInterval(7).cycle.intervalS)
        assertEquals(5, s.withStaleAfter(1).compass.staleAfterS)
        assertEquals(600, s.withStaleAfter(99999).compass.staleAfterS)

        s = s.copy(compass = s.compass.copy(targets = listOf("rocket")))
        assertEquals(listOf("rocket"), s.toggleTarget("rocket").compass.targets)
        assertEquals(listOf("rocket", "rrls"), s.toggleTarget("rrls").compass.targets)
    }

    @Test
    fun stampingIsMonotonic() {
        val a = WatchSettings().stamped(0, 1_000)
        assertEquals(1_000L, a.updatedMs)
        assertEquals("watch", a.updatedBy)
        val b = a.stamped(a.updatedMs, 500) // watch clock behind
        assertEquals(1_001L, b.updatedMs)
    }

    @Test
    fun optimisticEditSyncsWhenEchoed() {
        val s = snap()
        val st = WatchState().onSnapshot(s, 10_000)
        val edited = st.edit(5) { it.togglePage("cycle") }!!
        assertTrue(edited.syncing)
        assertEquals(s.settings.updatedMs + 1, edited.settings.updatedMs)
        assertEquals(listOf("flight", "propulsion", "compass"), edited.pages().map { it.id })
        assertNull(edited.edit(6) { it }) // no-op edit sends nothing
        // An older snapshot keeps the local edit.
        assertTrue(edited.onSnapshot(s, 11_000).syncing)
        // The echo clears it.
        val echoed = s.copy(settings = edited.settings.copy(updatedBy = "phone"))
        val done = edited.onSnapshot(echoed, 12_000)
        assertFalse(done.syncing)
        assertEquals(s.pages, done.pages())
    }

    @Test
    fun linkLossAfterTimeout() {
        assertEquals(LinkState.NeverConnected, WatchState().link(0))
        val st = WatchState().onSnapshot(snap(), 100_000)
        assertEquals(LinkState.Ok, st.link(115_000))
        val lost = st.link(175_000)
        assertTrue(lost is LinkState.Lost)
        assertEquals("Phone link lost · last data 1m 15s", (lost as LinkState.Lost).text)
    }

    @Test
    fun itemsTickClockAndAges() {
        val st = WatchState().onSnapshot(snap(), 0)
        assertEquals("T+ 01:28", st.item("t_clock", 5_400).text)
        assertEquals("1234 m", st.item("altitude", 0).text)
        assertEquals("2s", st.item("altitude", 1_900).ageText)
        assertEquals("--", st.item("velocity", 0).text)
        assertEquals(Level.UNKNOWN, st.item("velocity", 0).level)
        assertEquals("T- 00:03", Format.clock(Clock("tminus", 5_000), "x", 1_200))
        assertEquals("T- 00:00", Format.clock(Clock("tminus", 5_000), "x", 9_000))
        assertEquals("T- 61:05", Format.clock(Clock("tminus", 3_665_000), "x", 0))
        assertEquals("Hold", Format.clock(Clock("idle", 0), "Hold", 9_000))
    }

    @Test
    fun compassTargetsDistanceBearingStaleness() {
        val s = snap()
        val settings = s.settings
        val targets = Compass.orderedTargets(s, settings)
        assertEquals(listOf("rocket", "ground_station"), targets.map { it.id }) // rrls not sent
        val watch = LatLon(32.9904, -106.975)
        val rocket = Compass.view(targets[0], watch, 90.0, 10.0, settings, receivedAtMs = 0, nowMs = 1_000)
        assertNotNull(rocket.distanceM)
        assertEquals(Format.distance(rocket.distanceM!!, true), rocket.distanceText)
        val brg = Geodesy.initialBearingDeg(watch, LatLon(32.99, -106.97))!!
        assertEquals(Geodesy.arrowRotationDeg(brg, 90.0), rocket.arrowDeg!!, 1e-9)
        assertTrue(rocket.bearingText!!.endsWith("true"))
        assertEquals(4_000L, rocket.ageMs)
        assertFalse(rocket.stale)
        assertNull(rocket.fallbackLabel)
        // Stale once current age passes stale_after_s (30 s): 3 s at send + 28 s since.
        assertTrue(Compass.view(targets[0], watch, 90.0, 0.0, settings, 0, 28_000).stale)
        // Phone-flagged stale wins regardless of age.
        assertTrue(Compass.view(targets[0].copy(stale = true), watch, 0.0, 0.0, settings, 0, 0).stale)

        val gs = Compass.view(targets[1], watch, null, 0.0, settings, 0, 0)
        assertEquals("saved position", gs.fallbackLabel)
        assertNull(gs.arrowDeg) // no heading
        assertNotNull(gs.bearingText)

        val mag = Compass.view(targets[0], watch, 0.0, 10.0, settings.withNorth(WatchSettings.NORTH_MAGNETIC), 0, 0)
        val expected = Geodesy.trueToMagneticDeg(brg, 10.0)
        assertTrue(mag.bearingText!!.startsWith("${Math.round(expected) % 360}°"))
        assertTrue(mag.bearingText!!.endsWith("mag"))
        // Arrow is independent of the north setting.
        assertEquals(Compass.view(targets[0], watch, 0.0, 10.0, settings, 0, 0).arrowDeg!!, mag.arrowDeg!!, 1e-12)
    }

    @Test
    fun compassWaitingForGpsAndNearTarget() {
        val settings = WatchSettings()
        val t = Target(id = "rocket", label = "Rocket", lat = 32.9905, lon = -106.975)
        val waiting = Compass.view(t, null, 0.0, 0.0, settings, 0, 0)
        assertEquals("--", waiting.distanceText)
        assertNull(waiting.bearingText)
        assertNull(waiting.arrowDeg)

        val near = Compass.view(t, LatLon(32.9904, -106.975), 0.0, 0.0, settings, 0, 0)
        assertEquals("Within 11 m — look around", near.nearText)
        assertNull(near.arrowDeg)
        val nearFt = Compass.view(t, LatLon(32.9904, -106.975), 0.0, 0.0, settings.withUnits("imperial"), 0, 0)
        assertEquals("Within 36 ft — look around", nearFt.nearText)

        val far = Compass.view(t.copy(lat = 32.9910), LatLon(32.9904, -106.975), 0.0, 0.0, settings, 0, 0)
        assertNull(far.nearText)
        assertNotNull(far.arrowDeg)
    }

    @Test
    fun demoOffsetIsRelativeToWatch() {
        val t = Target(id = "rocket", label = "Rocket", lat = 0.0, lon = 0.0, demoOffset = DemoOffset(northM = 300.0, eastM = 400.0))
        val watch = LatLon(32.9904, -106.975)
        val v = Compass.view(t, watch, 0.0, 0.0, WatchSettings(), 0, 0)
        assertEquals(500.0, v.distanceM!!, 0.5)
        assertEquals(Math.toDegrees(kotlin.math.atan2(400.0, 300.0)), Geodesy.normalizeDeg(v.arrowDeg!!), 0.05)
        // Without a watch fix there is nothing to offset from.
        assertNull(Compass.view(t, null, 0.0, 0.0, WatchSettings(), 0, 0).distanceM)
    }
}
