import Foundation
import XCTest
@testable import WatchCore

/// Same checks as the `map_tiles` tests in map_tiles.rs and the Kotlin
/// MapTilesVectorsTest: the `map_tiles` vectors, same tolerances.
final class MapTilesTests: XCTestCase {
    static func vectors() throws -> [String: Any] {
        let packageDir = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent() // WatchCoreTests
            .deletingLastPathComponent() // Tests
            .deletingLastPathComponent() // WatchCore
        let url = packageDir.appendingPathComponent("../../test-vectors.json").standardizedFileURL
        let root = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any]
        return try XCTUnwrap(root?["map_tiles"] as? [String: Any])
    }

    private func cases(_ name: String) throws -> [[String: Any]] {
        try XCTUnwrap(Self.vectors()[name] as? [[String: Any]], name)
    }

    private func d(_ v: Any?) -> Double { (v as? NSNumber)?.doubleValue ?? .nan }
    private func i(_ v: Any?) -> Int { (v as? NSNumber)?.intValue ?? -1 }
    private func i64(_ v: Any?) -> Int64 { (v as? NSNumber)?.int64Value ?? -1 }
    private func strings(_ v: Any?) -> [String] { (v as? [String]) ?? [] }

    func testTileXY() throws {
        for c in try cases("tile_xy") {
            let name = c["name"] as? String ?? ""
            let z = i(c["z"])
            let f = MapTiles.tileXY(lat: d(c["lat"]), lon: d(c["lon"]), z: z)
            XCTAssertEqual(f.x, d(c["fx"]), accuracy: 1e-9, name)
            XCTAssertEqual(f.y, d(c["fy"]), accuracy: 1e-9, name)
            let tile = MapTiles.tileAt(lat: d(c["lat"]), lon: d(c["lon"]), z: z)
            XCTAssertEqual(tile.key, c["key"] as? String, name)
            XCTAssertEqual(TileId.parse(tile.key), tile)
            let b = tile.bounds
            XCTAssertEqual(b.north, d(c["north"]), accuracy: 1e-9, name)
            XCTAssertEqual(b.west, d(c["west"]), accuracy: 1e-9, name)
            XCTAssertEqual(b.south, d(c["south"]), accuracy: 1e-9, name)
            XCTAssertEqual(b.east, d(c["east"]), accuracy: 1e-9, name)
            XCTAssertEqual(MapTiles.tileSpanM(lat: d(c["lat"]), z: z), d(c["span_m"]), accuracy: 1e-6, name)
        }
    }

    func testCoverage() throws {
        for c in try cases("coverage") {
            let centres = (c["centres"] as? [[Double]] ?? []).map { LatLon(lat: $0[0], lon: $0[1]) }
            let levels = (c["levels"] as? [[Double]] ?? []).map { (z: Int($0[0]), radiusM: $0[1]) }
            let got = MapTiles.coverage(centres: centres, levels: levels, maxZoom: i(c["max_zoom"]), maxTiles: i(c["max_tiles"]))
            XCTAssertEqual(got.map(\.key), strings(c["tiles"]), c["name"] as? String ?? "")
        }
    }

    func testBudgetDiffAndEviction() throws {
        for c in try cases("budget") {
            let sizes = (c["sizes"] as? [NSNumber] ?? []).map(\.int64Value)
            let chosen = (c["chosen"] as? [NSNumber] ?? []).map(\.intValue)
            XCTAssertEqual(MapTiles.fitBudget(sizes: sizes, cap: i64(c["cap"])), chosen, c["name"] as? String ?? "")
        }
        for c in try cases("manifest_diff") {
            XCTAssertEqual(MapTiles.manifestDiff(plan: strings(c["plan"]), have: Set(strings(c["have"]))), strings(c["send"]))
        }
        for c in try cases("evict") {
            let entries = (c["entries"] as? [[String: Any]] ?? []).map {
                TileCacheEntry(key: $0["key"] as? String ?? "", bytes: i64($0["bytes"]), usedMs: i64($0["used_ms"]))
            }
            XCTAssertEqual(MapTiles.evictionOrder(entries: entries, plan: Set(strings(c["plan"])), cap: i64(c["cap"])),
                           strings(c["evict"]), c["name"] as? String ?? "")
        }
    }

    func testViewMath() throws {
        for c in try cases("auto_fit") {
            let got = MapTiles.autoFitZoom(distanceM: d(c["distance_m"]), lat: d(c["lat"]), radiusPx: d(c["radius_px"]),
                                           minZ: d(c["min_z"]), maxZ: d(c["max_z"]))
            XCTAssertEqual(got, d(c["zoom"]), accuracy: 1e-9, c["name"] as? String ?? "")
        }
        for c in try cases("ancestor") {
            let available = Set(strings(c["available"]))
            let tile = try XCTUnwrap(TileId.parse(c["key"] as? String ?? ""))
            let got = MapTiles.ancestor(tile, available: { available.contains($0) })
            if let want = c["found"] as? [String: Any] {
                let crop = try XCTUnwrap(got)
                XCTAssertEqual(crop.key, want["key"] as? String)
                XCTAssertEqual(crop.x, d(want["x"]), accuracy: 1e-12)
                XCTAssertEqual(crop.y, d(want["y"]), accuracy: 1e-12)
                XCTAssertEqual(crop.size, d(want["size"]), accuracy: 1e-12)
            } else {
                XCTAssertNil(got)
            }
        }
        for c in try cases("visible") {
            let got = MapTiles.visibleTiles(lat: d(c["lat"]), lon: d(c["lon"]), zoom: d(c["zoom"]),
                                            width: d(c["width"]), height: d(c["height"]))
            XCTAssertEqual(got.z, i(c["z"]))
            XCTAssertEqual(got.tilePx, d(c["tile_px"]), accuracy: 1e-6)
            let want = c["tiles"] as? [[String: Any]] ?? []
            XCTAssertEqual(got.tiles.count, want.count)
            for (g, w) in zip(got.tiles, want) {
                XCTAssertEqual(g.tile.key, w["key"] as? String)
                XCTAssertEqual(g.left, d(w["left"]), accuracy: 1e-6)
                XCTAssertEqual(g.top, d(w["top"]), accuracy: 1e-6)
            }
        }
        for c in try cases("project") {
            let center = c["center"] as? [Double] ?? [0, 0]
            let p = MapTiles.project(lat: d(c["lat"]), lon: d(c["lon"]), centerLat: center[0], centerLon: center[1], zoom: d(c["zoom"]))
            XCTAssertEqual(p.dx, d(c["dx"]), accuracy: 1e-6, c["name"] as? String ?? "")
            XCTAssertEqual(p.dy, d(c["dy"]), accuracy: 1e-6, c["name"] as? String ?? "")
        }
    }

    func testKeysManifestReportAndSettings() throws {
        XCTAssertEqual(TileId.parse("15/9204/12054"), TileId(z: 15, x: 9204, y: 12054))
        for bad in ["", "1/2", "1/2/3/4", "1/2/1", "23/0/0", "a/b/c", "-1/0/0", "3/8/0"] {
            XCTAssertNil(TileId.parse(bad), bad)
        }
        let m = try XCTUnwrap(MapManifest.parse(#"{"v":1,"type":"map","set":"ab","tiles":["11/1/2"],"attribution":"x","cap_bytes":5,"max_zoom":11,"centres":[]}"#))
        XCTAssertEqual(m.tiles, ["11/1/2"])
        XCTAssertEqual(m.capBytes, 5)
        XCTAssertNil(MapManifest.parse(#"{"v":1,"type":"direct"}"#))
        let report = try XCTUnwrap(MapTiles.reportJSON(set: "ab", have: ["13/5/6", "11/1/2"]))
        let parsed = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(report.utf8)) as? [String: Any])
        XCTAssertEqual(parsed["type"] as? String, "tiles")
        XCTAssertEqual(parsed["have"] as? [String], ["11/1/2", "13/5/6"])
        let settings = try JSONDecoder().decode(WatchSettings.self, from: Data(#"{"version":2}"#.utf8))
        XCTAssertTrue(settings.map.enabled)
        XCTAssertEqual(settings.map.capBytes, 3 * 1024 * 1024)
        let off = try JSONDecoder().decode(WatchSettings.self, from: Data(#"{"map":{"enabled":false,"cap_mb":9}}"#.utf8))
        XCTAssertFalse(off.map.enabled)
        XCTAssertEqual(off.map.capBytes, 5 * 1024 * 1024)
    }
}
