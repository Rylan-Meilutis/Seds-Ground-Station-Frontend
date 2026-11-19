import Foundation

// Map background tile math, a port of the phone's map_tiles.rs
// (docs/watch-companion.md "Map background"). Checked against the
// `map_tiles` vectors in watch/test-vectors.json, like the Kotlin core.

public struct TileId: Hashable, Comparable {
    public var z: Int
    public var x: Int
    public var y: Int

    public init(z: Int, x: Int, y: Int) {
        self.z = z
        self.x = x
        self.y = y
    }

    /// Validated tile, nil when out of range.
    public static func make(z: Int, x: Int, y: Int) -> TileId? {
        guard (0...MapTiles.maxKeyZoom).contains(z) else { return nil }
        let n = 1 << z
        guard (0..<n).contains(x), (0..<n).contains(y) else { return nil }
        return TileId(z: z, x: x, y: y)
    }

    /// `z/x/y`, the wire form.
    public var key: String { "\(z)/\(x)/\(y)" }

    public static func parse(_ key: String) -> TileId? {
        let parts = key.split(separator: "/", omittingEmptySubsequences: false)
        guard parts.count == 3 else { return nil }
        var values: [Int] = []
        for part in parts {
            guard !part.isEmpty, part.count <= 9, part.allSatisfy({ $0 >= "0" && $0 <= "9" }), let v = Int(part) else {
                return nil
            }
            values.append(v)
        }
        return make(z: values[0], x: values[1], y: values[2])
    }

    /// (north, west, south, east) in degrees.
    public var bounds: (north: Double, west: Double, south: Double, east: Double) {
        let nw = MapTiles.corner(z: z, x: x, y: y)
        let se = MapTiles.corner(z: z, x: x + 1, y: y + 1)
        return (nw.lat, nw.lon, se.lat, se.lon)
    }

    public static func < (a: TileId, b: TileId) -> Bool {
        (a.z, a.x, a.y) < (b.z, b.x, b.y)
    }
}

/// The phone's tile-set manifest (`type: map`).
public struct MapManifest: Codable, Equatable {
    public var set: String
    public var tiles: [String]
    public var attribution: String
    public var capBytes: Int64
    public var maxZoom: Int

    enum CodingKeys: String, CodingKey {
        case set, tiles, attribution, capBytes = "cap_bytes", maxZoom = "max_zoom"
    }

    public init(set: String, tiles: [String], attribution: String = MapTiles.attribution,
                capBytes: Int64 = 3 * 1024 * 1024, maxZoom: Int = 15) {
        self.set = set
        self.tiles = tiles
        self.attribution = attribution
        self.capBytes = capBytes
        self.maxZoom = maxZoom
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        set = c.soft(.set, "")
        tiles = c.soft(.tiles, [])
        attribution = c.soft(.attribution, MapTiles.attribution)
        capBytes = c.soft(.capBytes, 3 * 1024 * 1024)
        maxZoom = c.soft(.maxZoom, 15)
    }

    /// Parses the phone's message; nil for another version or type.
    public static func parse(_ json: String) -> MapManifest? {
        struct Header: Decodable { let v: Int?; let type: String? }
        guard let data = json.data(using: .utf8),
              let header = try? JSONDecoder().decode(Header.self, from: data),
              header.v == protocolVersion, header.type == "map"
        else { return nil }
        return try? JSONDecoder().decode(MapManifest.self, from: data)
    }
}

/// One cached tile on the watch.
public struct TileCacheEntry: Equatable {
    public var key: String
    public var bytes: Int64
    public var usedMs: Int64

    public init(key: String, bytes: Int64, usedMs: Int64) {
        self.key = key
        self.bytes = bytes
        self.usedMs = usedMs
    }
}

/// Where a missing tile can be cut from: an ancestor and the sub-square of it (fractions).
public struct AncestorCrop: Equatable {
    public var key: String
    public var x: Double
    public var y: Double
    public var size: Double
}

/// One tile to draw: offset of its top-left corner from the view centre in px, before rotation.
public struct TilePlacement: Equatable {
    public var tile: TileId
    public var left: Double
    public var top: Double
}

public struct VisibleTiles: Equatable {
    public var z: Int
    public var tilePx: Double
    public var tiles: [TilePlacement]
}

public enum MapTiles {
    public static let tilePx = 256.0
    public static let maxLat = 85.05112878
    public static let maxKeyZoom = 22
    public static let mercatorCircumferenceM = 2.0 * Double.pi * 6_378_137.0
    public static let attribution = "Imagery © Esri, Maxar, Earthstar Geographics"
    /// Zoom range of the compass map (the crown zooms within it).
    public static let minViewZoom = 10.0
    public static let maxViewZoom = 17.0
    /// Auto-fit keeps the target inside this range.
    public static let minFitZoom = 11.0
    public static let maxFitZoom = 16.0

    public static func corner(z: Int, x: Int, y: Int) -> (lat: Double, lon: Double) {
        let n = Double(1 << z)
        let lon = Double(x) / n * 360.0 - 180.0
        let lat = degrees(atan(sinh(Double.pi * (1.0 - 2.0 * Double(y) / n))))
        return (lat, lon)
    }

    /// Fractional tile coordinates of a point at zoom `z`.
    public static func tileXY(lat: Double, lon: Double, z: Int) -> (x: Double, y: Double) {
        let n = Double(1 << z)
        let r = radians(min(max(lat, -maxLat), maxLat))
        let fx = (lon + 180.0) / 360.0 * n
        let fy = (1.0 - log(tan(r) + 1.0 / cos(r)) / Double.pi) / 2.0 * n
        return (fx, fy)
    }

    static func clampIndex(_ v: Double, _ n: Int) -> Int {
        let f = max(v.rounded(.down), 0)
        return f >= Double(n - 1) ? n - 1 : Int(f)
    }

    public static func tileAt(lat: Double, lon: Double, z: Int) -> TileId {
        let f = tileXY(lat: lat, lon: lon, z: z)
        let n = 1 << z
        return TileId(z: z, x: clampIndex(f.x, n), y: clampIndex(f.y, n))
    }

    public static func tileSpanM(lat: Double, z: Int) -> Double {
        mercatorCircumferenceM * cos(radians(lat)) / Double(1 << z)
    }

    /// The phone's tile-set plan (same order as `map_tiles::coverage`).
    public static func coverage(centres: [LatLon], levels: [(z: Int, radiusM: Double)], maxZoom: Int, maxTiles: Int) -> [TileId] {
        var merged: [Int: Double] = [:]
        for level in levels {
            let z = min(level.z, maxZoom, maxKeyZoom)
            merged[z] = max(merged[z] ?? 0, level.radiusM)
        }
        var out: [TileId] = []
        var seen = Set<TileId>()
        for z in merged.keys.sorted() {
            let radius = merged[z] ?? 0
            let n = 1 << z
            for c in centres where c.isValid {
                let f = tileXY(lat: c.lat, lon: c.lon, z: z)
                let r = radius / tileSpanM(lat: c.lat, z: z)
                var cells: [(d: Int64, y: Int, x: Int)] = []
                for y in clampIndex(f.y - r, n)...clampIndex(f.y + r, n) {
                    for x in clampIndex(f.x - r, n)...clampIndex(f.x + r, n) {
                        let dx = Double(x) + 0.5 - f.x
                        let dy = Double(y) + 0.5 - f.y
                        cells.append((Int64(((dx * dx + dy * dy) * 1e6).rounded()), y, x))
                    }
                }
                cells.sort { ($0.d, $0.y, $0.x) < ($1.d, $1.y, $1.x) }
                for cell in cells {
                    let id = TileId(z: z, x: cell.x, y: cell.y)
                    if seen.insert(id).inserted { out.append(id) }
                }
            }
        }
        return Array(out.prefix(maxTiles))
    }

    /// Greedy byte budget in plan order; a size of 0 is never kept.
    public static func fitBudget(sizes: [Int64], cap: Int64) -> [Int] {
        var total: Int64 = 0
        var chosen: [Int] = []
        for (i, size) in sizes.enumerated() where size > 0 && total + size <= cap {
            total += size
            chosen.append(i)
        }
        return chosen
    }

    /// Plan tiles not held yet, in plan order.
    public static func manifestDiff(plan: [String], have: Set<String>) -> [String] {
        plan.filter { !have.contains($0) }
    }

    /// Keys to delete so the cache fits `cap`: outside the plan first, then
    /// least recently used, then by key.
    public static func evictionOrder(entries: [TileCacheEntry], plan: Set<String>, cap: Int64) -> [String] {
        var total = entries.reduce(Int64(0)) { $0 + $1.bytes }
        let order = entries.sorted { a, b in
            let pa = plan.contains(a.key) ? 1 : 0
            let pb = plan.contains(b.key) ? 1 : 0
            if pa != pb { return pa < pb }
            if a.usedMs != b.usedMs { return a.usedMs < b.usedMs }
            return a.key < b.key
        }
        var out: [String] = []
        for e in order {
            if total <= cap { break }
            total -= e.bytes
            out.append(e.key)
        }
        return out
    }

    /// Zoom that puts a target `distanceM` away at `radiusPx` from the centre, clamped.
    public static func autoFitZoom(distanceM: Double, lat: Double, radiusPx: Double, minZ: Double, maxZ: Double) -> Double {
        guard distanceM.isFinite, distanceM > 0 else { return maxZ }
        let z = log2(mercatorCircumferenceM * cos(radians(lat)) * radiusPx / (tilePx * distanceM))
        return min(max(z, minZ), maxZ)
    }

    public static func ancestor(_ tile: TileId, available: (String) -> Bool, maxUp: Int = 6) -> AncestorCrop? {
        let limit = min(maxUp, tile.z)
        guard limit >= 1 else { return nil }
        for up in 1...limit {
            let parent = TileId(z: tile.z - up, x: tile.x >> up, y: tile.y >> up)
            if available(parent.key) {
                let size = 1.0 / Double(1 << up)
                return AncestorCrop(key: parent.key, x: Double(tile.x - (parent.x << up)) * size,
                                    y: Double(tile.y - (parent.y << up)) * size, size: size)
            }
        }
        return nil
    }

    /// Tiles covering a `width`×`height` view (its circumscribed circle) at fractional `zoom`.
    public static func visibleTiles(lat: Double, lon: Double, zoom: Double, width: Double, height: Double) -> VisibleTiles {
        let z = min(max(Int((zoom + 0.5).rounded(.down)), 0), maxKeyZoom)
        let px = tilePx * pow(2.0, zoom - Double(z))
        let f = tileXY(lat: lat, lon: lon, z: z)
        let r = (width * width + height * height).squareRoot() / 2.0 / px
        let n = 1 << z
        var out: [TilePlacement] = []
        for y in clampIndex(f.y - r, n)...clampIndex(f.y + r, n) {
            for x in clampIndex(f.x - r, n)...clampIndex(f.x + r, n) {
                out.append(TilePlacement(tile: TileId(z: z, x: x, y: y), left: (Double(x) - f.x) * px, top: (Double(y) - f.y) * px))
            }
        }
        return VisibleTiles(z: z, tilePx: px, tiles: out)
    }

    /// Pixel offset (east, south) of a point from the view centre at `zoom`.
    public static func project(lat: Double, lon: Double, centerLat: Double, centerLon: Double, zoom: Double) -> (dx: Double, dy: Double) {
        let p = tileXY(lat: lat, lon: lon, z: 0)
        let c = tileXY(lat: centerLat, lon: centerLon, z: 0)
        let scale = tilePx * pow(2.0, zoom)
        return ((p.x - c.x) * scale, (p.y - c.y) * scale)
    }

    /// The `tiles` report (watch → phone): the tiles the watch holds.
    public static func reportJSON(set: String?, have: Set<String>) -> String? {
        WatchMessage.tiles(set: set, have: have.sorted()).json()
    }
}
