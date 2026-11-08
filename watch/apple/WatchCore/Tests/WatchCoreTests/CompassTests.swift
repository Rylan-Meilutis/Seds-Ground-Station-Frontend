import Foundation
import XCTest
@testable import WatchCore

final class CompassTests: XCTestCase {
    let here = LatLon(lat: 32.9904, lon: -106.975)

    func target(_ id: String = "rocket", lat: Double, lon: Double, ageMs: Int64 = 0, stale: Bool = false,
                fallback: Bool = false, offset: (Double, Double)? = nil) throws -> Target {
        var json = #"{"id":"\#(id)","label":"Rocket","lat":\#(lat),"lon":\#(lon),"age_ms":\#(ageMs),"stale":\#(stale),"source":"GPS","fallback":\#(fallback)"#
        if let o = offset { json += #","demo_offset":{"north_m":\#(o.0),"east_m":\#(o.1)}"# }
        json += "}"
        return try JSONDecoder().decode(Target.self, from: Data(json.utf8))
    }

    func testDistanceBearingAndArrow() throws {
        let t = try target(lat: 32.981, lon: -106.986)
        var settings = WatchSettings()
        let reading = CompassReading(position: here, trueHeading: 180, declination: 10)
        var d = Compass.display(t, settings: settings, reading: reading, receivedAtMs: 0, nowMs: 0)
        XCTAssertEqual(d.distanceText, "1.46 km")
        XCTAssertEqual(d.bearingText, "224° SW")
        XCTAssertEqual(d.north, .trueNorth)
        XCTAssertEqual(try XCTUnwrap(d.arrowRotation), 44.47047, accuracy: 1e-4)
        XCTAssertNil(d.nearText)
        XCTAssertNil(d.savedPositionLabel)

        settings.compass.north = .magnetic
        d = Compass.display(t, settings: settings, reading: reading, receivedAtMs: 0, nowMs: 0)
        XCTAssertEqual(d.bearingText, "214° SW")
        XCTAssertEqual(d.north, .magnetic)
        XCTAssertEqual(try XCTUnwrap(d.arrowRotation), 44.47047, accuracy: 1e-4, "arrow ignores the north setting")

        // Magnetic requested but declination unknown: stays true and says so.
        d = Compass.display(t, settings: settings, reading: CompassReading(position: here, trueHeading: 180),
                            receivedAtMs: 0, nowMs: 0)
        XCTAssertEqual(d.bearingText, "224° SW")
        XCTAssertEqual(d.north, .trueNorth)

        settings.units = .imperial
        d = Compass.display(t, settings: settings, reading: reading, receivedAtMs: 0, nowMs: 0)
        XCTAssertEqual(d.distanceText, "0.91 mi")
    }

    func testNoHeadingOrNoPosition() throws {
        let t = try target(lat: 32.981, lon: -106.986, fallback: true)
        let s = WatchSettings()
        var d = Compass.display(t, settings: s, reading: CompassReading(position: here), receivedAtMs: 0, nowMs: 0)
        XCTAssertEqual(d.distanceText, "1.46 km")
        XCTAssertNotNil(d.bearingText)
        XCTAssertNil(d.arrowRotation)
        XCTAssertEqual(d.savedPositionLabel, "saved position")
        d = Compass.display(t, settings: s, reading: CompassReading(trueHeading: 0), receivedAtMs: 0, nowMs: 0)
        XCTAssertEqual(d.distanceText, "--")
        XCTAssertNil(d.bearingText)
        XCTAssertNil(d.arrowRotation)
    }

    func testWithinThirtyMetres() throws {
        let t = try target(lat: 32.9905, lon: -106.975)
        let d = Compass.display(t, settings: WatchSettings(), reading: CompassReading(position: here, trueHeading: 0),
                                receivedAtMs: 0, nowMs: 0)
        XCTAssertEqual(d.nearText, "Within 11 m — look around")
        XCTAssertNil(d.arrowRotation)
        XCTAssertEqual(d.bearingText, "0° N")
    }

    func testStaleness() throws {
        let s = WatchSettings() // stale_after_s 30
        let t = try target(lat: 32.981, lon: -106.986, ageMs: 3_000)
        let r = CompassReading(position: here)
        var d = Compass.display(t, settings: s, reading: r, receivedAtMs: 1_000, nowMs: 28_000)
        XCTAssertEqual(d.ageMs, 30_000)
        XCTAssertFalse(d.stale)
        XCTAssertEqual(d.ageText, "30s")
        d = Compass.display(t, settings: s, reading: r, receivedAtMs: 1_000, nowMs: 28_001)
        XCTAssertTrue(d.stale)
        // Clock skew backwards never reduces the age.
        d = Compass.display(t, settings: s, reading: r, receivedAtMs: 10_000, nowMs: 0)
        XCTAssertEqual(d.ageMs, 3_000)
        // Phone-flagged stale wins.
        let flagged = try target(lat: 32.981, lon: -106.986, stale: true)
        XCTAssertTrue(Compass.display(flagged, settings: s, reading: r, receivedAtMs: 0, nowMs: 0).stale)
    }

    func testDemoOffset() throws {
        let t = try target(lat: 0, lon: 0, offset: (100, 0))
        let d = Compass.display(t, settings: WatchSettings(), reading: CompassReading(position: here, trueHeading: 90),
                                receivedAtMs: 0, nowMs: 0)
        XCTAssertEqual(try XCTUnwrap(d.distanceM), 100, accuracy: 0.01)
        XCTAssertEqual(d.bearingText, "0° N")
        XCTAssertEqual(try XCTUnwrap(d.arrowRotation), 270, accuracy: 1e-6)
        let east = Compass.displaced(here, northM: 0, eastM: 50)
        XCTAssertEqual(try XCTUnwrap(distanceM(here, east)), 50, accuracy: 0.01)
        XCTAssertEqual(try XCTUnwrap(initialBearingDeg(here, east)), 90, accuracy: 0.01)
        let wrapped = Compass.displaced(LatLon(lat: 0, lon: 179.9999), northM: 0, eastM: 100)
        XCTAssertTrue(wrapped.isValid)
        XCTAssertLessThan(wrapped.lon, 0)
        // No watch fix: a demo target cannot be placed.
        let none = Compass.display(t, settings: WatchSettings(), reading: CompassReading(), receivedAtMs: 0, nowMs: 0)
        XCTAssertEqual(none.distanceText, "--")
    }

    func testTargetOrderFollowsSettings() throws {
        let rocket = try target("rocket", lat: 32.981, lon: -106.986)
        let gs = try target("ground_station", lat: 32.98, lon: -106.98)
        var s = WatchSettings()
        s.compass.targets = ["ground_station", "rocket"]
        let ids = Compass.displays(targets: [rocket, gs], settings: s, reading: CompassReading(), receivedAtMs: 0, nowMs: 0)
            .map(\.id)
        XCTAssertEqual(ids, ["ground_station", "rocket"])
        s.compass.targets = ["rrls"]
        XCTAssertTrue(Compass.displays(targets: [rocket, gs], settings: s, reading: CompassReading(), receivedAtMs: 0, nowMs: 0).isEmpty)
    }

    func testLinkLoss() {
        XCTAssertEqual(LinkState(receivedAtMs: nil, nowMs: 0, timeoutS: 15), .waiting)
        XCTAssertEqual(LinkState(receivedAtMs: 0, nowMs: 15_000, timeoutS: 15), .live)
        let lost = LinkState(receivedAtMs: 0, nowMs: 125_000, timeoutS: 15)
        XCTAssertEqual(lost, .lost(lastDataMs: 125_000))
        XCTAssertEqual(lost.lostText, "Phone link lost · last data 2m 05s")
        XCTAssertNil(LinkState.live.lostText)
    }

    func testClockText() {
        XCTAssertEqual(clockText(LaunchClock(kind: "tminus", ms: 5_000), elapsedMs: 1_500), "T- 00:03")
        XCTAssertEqual(clockText(LaunchClock(kind: "tminus", ms: 5_000), elapsedMs: 9_000), "T- 00:00")
        XCTAssertEqual(clockText(LaunchClock(kind: "tplus", ms: 83_000), elapsedMs: 1_500), "T+ 01:24")
        XCTAssertEqual(clockText(LaunchClock(kind: "tplus", ms: 6_000_000), elapsedMs: 0), "T+ 100:00")
        XCTAssertEqual(clockText(LaunchClock(kind: "tplus", ms: 1_000), elapsedMs: -5_000), "T+ 00:01")
        XCTAssertNil(clockText(LaunchClock(kind: "idle", ms: 0), elapsedMs: 0))
    }

    func testCycleScheduler() {
        var c = CycleScheduler()
        XCTAssertEqual(c.index(at: 100_000, intervalS: 5, count: 3), 0, "not started")
        c.start(at: 0)
        XCTAssertEqual(c.index(at: 4_999, intervalS: 5, count: 3), 0)
        XCTAssertEqual(c.index(at: 5_000, intervalS: 5, count: 3), 1)
        XCTAssertEqual(c.index(at: 15_000, intervalS: 5, count: 3), 0)
        c.pause(at: 7_000)
        XCTAssertFalse(c.isRunning)
        XCTAssertEqual(c.index(at: 60_000, intervalS: 5, count: 3), 1, "paused holds")
        c.toggle(at: 60_000)
        XCTAssertTrue(c.isRunning)
        XCTAssertEqual(c.index(at: 63_000, intervalS: 5, count: 3), 2)
        XCTAssertEqual(c.index(at: 63_000, intervalS: 5, count: 0), 0)
    }
}
