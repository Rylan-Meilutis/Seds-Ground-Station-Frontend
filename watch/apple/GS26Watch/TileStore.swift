import Foundation
import UIKit
import WatchCore

/// Map tiles on the watch (docs/watch-companion.md "Map background"): JPEG
/// files in Application Support/map_tiles, capped at the phone's size cap
/// with WatchCore `MapTiles.evictionOrder` (tiles outside the current set
/// first, then least recently used). Tiles arrive from the phone with
/// WatchConnectivity file transfers or, in direct mode, from the station's
/// tile endpoint. All published state changes happen on the main queue.
final class TileStore: ObservableObject {
    /// Keys of the cached tiles.
    @Published private(set) var keys: Set<String> = []
    /// The phone's latest tile set (attribution, cap, set id).
    @Published private(set) var manifest: MapManifest?

    private enum Keys {
        static let manifest = "gs26.map.manifest"
    }

    private static let maxTileBytes = 512 * 1024
    private static let defaultCap: Int64 = 3 * 1024 * 1024

    private let defaults = UserDefaults.standard
    private let dir: URL
    private let lock = NSLock()
    private var entries: [String: TileCacheEntry] = [:]
    private let images = NSCache<NSString, UIImage>()

    init() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? FileManager.default.temporaryDirectory
        dir = support.appendingPathComponent("map_tiles", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        var excluded = dir
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? excluded.setResourceValues(values)
        images.countLimit = 24
        if let json = defaults.string(forKey: Keys.manifest) { manifest = MapManifest.parse(json) }
        let files = (try? FileManager.default.contentsOfDirectory(
            at: dir, includingPropertiesForKeys: [.fileSizeKey, .contentModificationDateKey])) ?? []
        for file in files {
            guard let key = Self.key(of: file.lastPathComponent) else {
                try? FileManager.default.removeItem(at: file)
                continue
            }
            let values = try? file.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey])
            let size = Int64(values?.fileSize ?? 0)
            let used = Int64((values?.contentModificationDate ?? Date()).timeIntervalSince1970 * 1000)
            if size == 0 {
                try? FileManager.default.removeItem(at: file)
            } else {
                entries[key] = TileCacheEntry(key: key, bytes: size, usedMs: used)
            }
        }
        keys = Set(entries.keys)
    }

    private func url(for key: String) -> URL {
        dir.appendingPathComponent(key.replacingOccurrences(of: "/", with: "_") + ".jpg")
    }

    private static func key(of name: String) -> String? {
        guard name.hasSuffix(".jpg") else { return nil }
        let key = String(name.dropLast(4)).replacingOccurrences(of: "_", with: "/")
        return TileId.parse(key) == nil ? nil : key
    }

    var attribution: String { manifest?.attribution ?? MapTiles.attribution }

    func has(_ key: String) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return entries[key] != nil
    }

    // MARK: Updates (call on the main queue)

    func apply(manifestJSON json: String) {
        guard let next = MapManifest.parse(json) else { return }
        defaults.set(json, forKey: Keys.manifest)
        manifest = next
        evict(settingsCap: nil)
    }

    /// Moves a received file into the cache. Called from the WatchConnectivity
    /// callback before it returns (the system deletes the file afterwards), so
    /// only the file move happens there; `commit` publishes on main.
    func adopt(fileAt source: URL, key: String) -> Bool {
        guard TileId.parse(key) != nil,
              let size = (try? source.resourceValues(forKeys: [.fileSizeKey]))?.fileSize,
              size > 0, size <= Self.maxTileBytes
        else { return false }
        let target = url(for: key)
        try? FileManager.default.removeItem(at: target)
        do {
            try FileManager.default.moveItem(at: source, to: target)
        } catch {
            return false
        }
        lock.lock()
        entries[key] = TileCacheEntry(key: key, bytes: Int64(size), usedMs: currentTimeMs())
        lock.unlock()
        images.removeObject(forKey: key as NSString)
        return true
    }

    /// Stores tile bytes (from the station in direct mode).
    func put(_ data: Data, key: String, settingsCap: Int64?) {
        guard TileId.parse(key) != nil, !data.isEmpty, data.count <= Self.maxTileBytes,
              UIImage(data: data) != nil
        else { return }
        let target = url(for: key)
        guard (try? data.write(to: target, options: .atomic)) != nil else { return }
        lock.lock()
        entries[key] = TileCacheEntry(key: key, bytes: Int64(data.count), usedMs: currentTimeMs())
        lock.unlock()
        images.removeObject(forKey: key as NSString)
        evict(settingsCap: settingsCap)
    }

    /// Publishes adopted files and trims the cache.
    func commit(settingsCap: Int64?) {
        evict(settingsCap: settingsCap)
    }

    private func evict(settingsCap: Int64?) {
        let cap = manifest?.capBytes ?? settingsCap ?? Self.defaultCap
        let plan = Set(manifest?.tiles ?? [])
        lock.lock()
        for key in MapTiles.evictionOrder(entries: Array(entries.values), plan: plan, cap: cap) {
            try? FileManager.default.removeItem(at: url(for: key))
            entries[key] = nil
            images.removeObject(forKey: key as NSString)
        }
        let next = Set(entries.keys)
        lock.unlock()
        if keys != next { keys = next }
    }

    /// Decoded tile, nil when not cached or unreadable. Marks it used.
    func image(_ key: String) -> UIImage? {
        if let cached = images.object(forKey: key as NSString) {
            touch(key)
            return cached
        }
        guard has(key), let image = UIImage(contentsOfFile: url(for: key).path) else { return nil }
        images.setObject(image, forKey: key as NSString)
        touch(key)
        return image
    }

    private func touch(_ key: String) {
        let now = currentTimeMs()
        lock.lock()
        defer { lock.unlock() }
        guard var entry = entries[key] else { return }
        // Persist the use time at most once a minute per tile.
        if now - entry.usedMs > 60_000 {
            try? FileManager.default.setAttributes([.modificationDate: Date()], ofItemAtPath: url(for: key).path)
        }
        entry.usedMs = now
        entries[key] = entry
    }

    /// The `tiles` report for the phone.
    func reportJSON() -> String? {
        MapTiles.reportJSON(set: manifest?.set, have: keys)
    }
}
