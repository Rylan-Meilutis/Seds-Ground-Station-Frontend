import Foundation
import XCTest
@testable import WatchCore

final class ModelTests: XCTestCase {
    // The example from docs/watch-companion.md, filled in, plus fields this
    // build does not know about.
    static let docSnapshot = """
    {"v":1,"seq":42,"mode":"live","future_top_level":{"x":1},
     "station":{"name":"GroundStation","role":"primary","link":"connected","telemetry_age_ms":350},
     "account":{"signed_in":true,"sync":"synced"},
     "items":{"flight_state":{"label":"Flight state","text":"Descent","unit":"","age_ms":0,"stale":false,"level":"normal"},
              "t_clock":{"label":"T-clock","text":"T+ 01:23","unit":"","age_ms":0,"stale":false,"level":"normal","clock":{"kind":"tplus","ms":83000}},
              "altitude":{"label":"Altitude","text":"1234","value":1234.0,"unit":"m","age_ms":200,"stale":false,"level":"normal","color":"red"},
              "velocity":{"label":"Velocity","text":"--","unit":"m/s","age_ms":null,"stale":true,"level":"unknown"}},
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
                "pages":[{"id":"flight","kind":"items","title":"Flight","items":["flight_state","t_clock","altitude","velocity"]},
                         {"id":"propulsion","kind":"items","title":"Propulsion","items":["tank_pressure"]},
                         {"id":"cycle","kind":"cycle","title":"Cycle","items":[]},
                         {"id":"compass","kind":"compass","title":"Recovery","items":[]}],
                "targets":[{"id":"rocket","label":"Rocket"}]},
     "link_timeout_s":15}
    """

    func testDecodesDocExample() throws {
        let s = try XCTUnwrap(Snapshot.parse(Self.docSnapshot))
        XCTAssertEqual(s.seq, 42)
        XCTAssertEqual(s.mode, .live)
        XCTAssertEqual(s.station?.telemetryAgeMs, 350)
        XCTAssertEqual(s.account?.signedIn, true)
        XCTAssertEqual(s.items.count, 4)
        XCTAssertEqual(s.items["altitude"]?.value, 1234)
        XCTAssertNil(s.items["velocity"]?.ageMs)
        XCTAssertEqual(s.items["velocity"]?.level, .unknown)
        XCTAssertEqual(s.items["t_clock"]?.clock, LaunchClock(kind: "tplus", ms: 83000))
        XCTAssertEqual(s.items["t_clock"]?.displayText(elapsedMs: 2000), "T+ 01:25")
        XCTAssertEqual(s.pages.map(\.pageKind), [.items, .cycle, .compass])
        XCTAssertEqual(s.targets.count, 2)
        XCTAssertNil(s.targets[1].altM)
        XCTAssertTrue(s.targets[1].fallback)
        XCTAssertEqual(s.settings?.updatedMs, 1_730_000_000_000)
        XCTAssertEqual(s.settings?.compass.north, .trueNorth)
        XCTAssertEqual(s.settings?.cycle.intervalS, 5)
        XCTAssertEqual(s.catalog?.pages.count, 4)
        XCTAssertEqual(s.linkTimeoutS, 15)
        XCTAssertEqual(s.label(forItem: "altitude"), "Altitude")
        XCTAssertEqual(s.label(forItem: "nope"), "nope")
    }

    func testUnknownPageKindAndTolerantDecoding() throws {
        let json = """
        {"v":1,"seq":1,"mode":"replay","items":{"a":{"text":"5"},"b":42},
         "pages":[{"id":"map","kind":"map","title":"Map","items":[]},{"kind":"items"},{"id":"x","kind":"items","title":"X","items":["a"]}],
         "targets":[{"id":"rocket","lat":1.0,"lon":2.0,"demo_offset":{"north_m":10,"east_m":-5}},{"id":"broken","lat":"n/a"}]}
        """
        let s = try XCTUnwrap(Snapshot.parse(json))
        XCTAssertEqual(s.mode, .live, "unknown mode falls back")
        XCTAssertEqual(s.items.keys.sorted(), ["a"])
        XCTAssertEqual(s.items["a"]?.level, .unknown)
        XCTAssertEqual(s.pages.map(\.id), ["map", "x"])
        XCTAssertEqual(s.pages[0].pageKind, .unknown("map"))
        XCTAssertEqual(s.targets.map(\.id), ["rocket"])
        XCTAssertEqual(s.targets[0].label, "rocket")
        XCTAssertEqual(s.targets[0].demoOffset?.eastM, -5)
        XCTAssertNil(s.settings)
        XCTAssertEqual(s.linkTimeoutS, 15)
        XCTAssertNil(Snapshot.parse("not json"))
    }

    func testOutgoingMessages() throws {
        XCTAssertEqual(WatchMessage.hello.json(), #"{"type":"hello","v":1}"#)
        XCTAssertEqual(WatchMessage.activity(foreground: true, page: "compass").json(),
                       #"{"foreground":true,"page":"compass","type":"activity","v":1}"#)
        XCTAssertEqual(WatchMessage.activity(foreground: false, page: nil).json(),
                       #"{"foreground":false,"type":"activity","v":1}"#)
        let s = WatchSettings().stamped(previousMs: 0, nowMs: 5)
        let json = try XCTUnwrap(WatchMessage.settings(s).json())
        let obj = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(json.utf8)) as? [String: Any])
        XCTAssertEqual(obj["type"] as? String, "settings")
        let inner = try XCTUnwrap(obj["settings"] as? [String: Any])
        XCTAssertEqual(inner["updated_by"] as? String, "watch")
    }

    func testSettingsEncodeWithDocKeys() throws {
        var s = WatchSettings()
        s.compass.north = .magnetic
        let enc = JSONEncoder()
        enc.outputFormatting = [.sortedKeys]
        let json = String(decoding: try enc.encode(s), as: UTF8.self)
        for key in ["\"version\":1", "\"updated_ms\":0", "\"updated_by\":\"phone\"", "\"interval_s\":5", "\"autostart\":true",
                    "\"north\":\"magnetic\"", "\"stale_after_s\":30", "\"units\":\"metric\"", "\"power\":\"balanced\"",
                    "\"enabled\":true"] {
            XCTAssertTrue(json.contains(key), "\(key) missing in \(json)")
        }
        let back = try JSONDecoder().decode(WatchSettings.self, from: Data(json.utf8))
        XCTAssertEqual(back, s)
        // Missing fields take defaults; unknown values fall back.
        let partial = try JSONDecoder().decode(WatchSettings.self, from: Data(#"{"units":"imperial","power":"turbo","x":1}"#.utf8))
        XCTAssertEqual(partial.units, .imperial)
        XCTAssertEqual(partial.power, .balanced)
        XCTAssertEqual(partial.pages, WatchSettings().pages)
    }

    func testSettingsEditing() {
        var s = WatchSettings()
        s.movePage("flight", up: true)
        XCTAssertEqual(s.pages[0].id, "flight")
        s.movePage("compass", up: true)
        XCTAssertEqual(s.pages[s.pages.count - 2].id, "compass")
        s.movePage("compass", up: false)
        XCTAssertEqual(s.pages.last?.id, "compass")
        s.togglePage("propulsion")
        XCTAssertEqual(s.pages.first { $0.id == "propulsion" }?.enabled, false)

        s.cycle.items = ["altitude"]
        s.toggleCycleItem("altitude")
        XCTAssertEqual(s.cycle.items, ["altitude"], "never empties")
        s.toggleCycleItem("velocity")
        s.moveCycleItem("velocity", up: true)
        XCTAssertEqual(s.cycle.items, ["velocity", "altitude"])
        s.toggleCycleItem("unknown", known: ["altitude", "velocity"])
        XCTAssertEqual(s.cycle.items.count, 2)
        for i in 0..<20 { s.toggleCycleItem("item\(i)") }
        XCTAssertEqual(s.cycle.items.count, maxCycleItems)

        s.compass.targets = ["rocket"]
        s.toggleTarget("rocket")
        XCTAssertEqual(s.compass.targets, ["rocket"])
        s.toggleTarget("rrls", known: ["rocket", "rrls"])
        XCTAssertEqual(s.compass.targets, ["rocket", "rrls"])

        s.setIntervalS(0)
        XCTAssertEqual(s.cycle.intervalS, 2)
        s.setIntervalS(500)
        XCTAssertEqual(s.cycle.intervalS, 60)
        s.setStaleAfterS(1)
        XCTAssertEqual(s.compass.staleAfterS, 5)
        s.setStaleAfterS(99_999)
        XCTAssertEqual(s.compass.staleAfterS, 600)
    }

    func testStampingAndEcho() {
        let a = WatchSettings().stamped(previousMs: 0, nowMs: 1_000, by: .phone)
        let b = a.stamped(previousMs: a.updatedMs, nowMs: 500) // watch clock behind
        XCTAssertEqual(b.updatedMs, 1_001)
        XCTAssertEqual(b.updatedBy, .watch)
        XCTAssertEqual(a.stamped(previousMs: 10, nowMs: 2_000).updatedMs, 2_000)
        XCTAssertFalse(echoConfirms(pending: b, echoed: a))
        XCTAssertTrue(echoConfirms(pending: b, echoed: b))
        XCTAssertFalse(echoConfirms(pending: b, echoed: nil))
    }

    func testResolvedPages() throws {
        let snap = try XCTUnwrap(Snapshot.parse(Self.docSnapshot))
        var s = try XCTUnwrap(snap.settings)
        s.togglePage("flight")
        s.movePage("compass", up: true)
        let ids = resolvedPages(s, catalog: snap.catalog?.pages ?? []).map(\.id)
        XCTAssertEqual(ids, ["propulsion", "compass", "cycle"])
    }
}
