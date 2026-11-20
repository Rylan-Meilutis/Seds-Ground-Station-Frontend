import Foundation

// Wire format v1 (docs/watch-companion.md). Decoding is tolerant: unknown
// keys are ignored, missing or mistyped optional fields fall back to
// defaults, and a malformed entry in a list is dropped instead of failing
// the whole snapshot.

public let protocolVersion = 1
/// WatchConnectivity dictionary keys.
public let phoneToWatchKey = "gs26"
public let watchToPhoneKey = "gs26_watch"

extension KeyedDecodingContainer {
    func soft<T: Decodable>(_ key: Key, _ fallback: T) -> T {
        (try? decodeIfPresent(T.self, forKey: key)) ?? fallback
    }

    func softOptional<T: Decodable>(_ key: Key) -> T? {
        (try? decodeIfPresent(T.self, forKey: key)) ?? nil
    }

    func lossyArray<T: Decodable>(_ key: Key) -> [T] {
        let raw = (try? decodeIfPresent([Lossy<T>].self, forKey: key)) ?? nil
        return raw?.compactMap(\.value) ?? []
    }

    func lossyMap<T: Decodable>(_ key: Key) -> [String: T] {
        let raw = (try? decodeIfPresent([String: Lossy<T>].self, forKey: key)) ?? nil
        return raw?.compactMapValues(\.value) ?? [:]
    }
}

struct Lossy<T: Decodable>: Decodable {
    let value: T?
    init(from decoder: Decoder) throws { value = try? T(from: decoder) }
}

public enum Mode: String, Codable {
    case live, demo, noStation = "no_station"
}

public enum Level: String, Codable {
    case normal, warn, alert, unknown
}

public struct Station: Decodable, Equatable {
    public var name: String
    public var role: String
    /// `connected`, `connecting`, `reconnecting`, `sign_in` or `offline`.
    public var link: String
    /// Host[:port] of the phone's station; empty when none is chosen.
    public var address: String
    public var telemetryAgeMs: Int64?

    enum CodingKeys: String, CodingKey { case name, role, link, address, telemetryAgeMs = "telemetry_age_ms" }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = c.soft(.name, "")
        role = c.soft(.role, "unknown")
        link = c.soft(.link, "offline")
        address = c.soft(.address, "")
        telemetryAgeMs = c.softOptional(.telemetryAgeMs)
    }
}

public struct Account: Decodable, Equatable {
    public var signedIn: Bool
    public var sync: String

    enum CodingKeys: String, CodingKey { case signedIn = "signed_in", sync }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        signedIn = c.soft(.signedIn, false)
        sync = c.soft(.sync, "local_only")
    }
}

/// Launch clock. `kind` is `idle`, `tminus` or `tplus`; `ms` is remaining
/// (tminus) or elapsed (tplus) time at send.
public struct LaunchClock: Decodable, Equatable {
    public var kind: String
    public var ms: Int64

    public init(kind: String, ms: Int64) {
        self.kind = kind
        self.ms = ms
    }

    enum CodingKeys: String, CodingKey { case kind, ms }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        kind = c.soft(.kind, "idle")
        ms = c.soft(.ms, 0)
    }
}

public struct ItemValue: Decodable, Equatable {
    public var label: String
    public var text: String
    public var value: Double?
    public var unit: String
    public var ageMs: Int64?
    public var stale: Bool
    public var level: Level
    public var clock: LaunchClock?

    enum CodingKeys: String, CodingKey { case label, text, value, unit, ageMs = "age_ms", stale, level, clock }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        label = c.soft(.label, "")
        text = c.soft(.text, "--")
        value = c.softOptional(.value)
        unit = c.soft(.unit, "")
        ageMs = c.softOptional(.ageMs)
        stale = c.soft(.stale, false)
        level = c.soft(.level, Level.unknown)
        clock = c.softOptional(.clock)
    }

    /// Text to show now: a ticking T-clock when the item carries one.
    public func displayText(elapsedMs: Int64) -> String {
        clock.flatMap { clockText($0, elapsedMs: elapsedMs) } ?? text
    }
}

public struct DemoOffset: Decodable, Equatable {
    public var northM: Double
    public var eastM: Double

    enum CodingKeys: String, CodingKey { case northM = "north_m", eastM = "east_m" }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        northM = c.soft(.northM, 0)
        eastM = c.soft(.eastM, 0)
    }
}

public struct Target: Decodable, Equatable {
    public var id: String
    public var label: String
    public var lat: Double
    public var lon: Double
    public var altM: Double?
    public var ageMs: Int64
    public var stale: Bool
    public var source: String
    /// Saved/manual position rather than a live GPS fix.
    public var fallback: Bool
    public var demoOffset: DemoOffset?

    enum CodingKeys: String, CodingKey {
        case id, label, lat, lon, altM = "alt_m", ageMs = "age_ms", stale, source, fallback, demoOffset = "demo_offset"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        lat = try c.decode(Double.self, forKey: .lat)
        lon = try c.decode(Double.self, forKey: .lon)
        label = c.soft(.label, id)
        altM = c.softOptional(.altM)
        ageMs = c.soft(.ageMs, 0)
        stale = c.soft(.stale, false)
        source = c.soft(.source, "")
        fallback = c.soft(.fallback, false)
        demoOffset = c.softOptional(.demoOffset)
    }
}

public enum PageKind: Equatable {
    case items, cycle, compass, voice
    case unknown(String)
}

/// An opt-in action button on a page (at most two per page).
public struct PageAction: Decodable, Equatable, Identifiable {
    public var cmd: String
    public var label: String
    /// Abort: one tap plus confirm, never disabled on the watch.
    public var abort: Bool
    public var id: String { cmd }

    public init(cmd: String, label: String, abort: Bool) {
        self.cmd = cmd
        self.label = label
        self.abort = abort
    }

    enum CodingKeys: String, CodingKey { case cmd, label, abort }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        cmd = try c.decode(String.self, forKey: .cmd)
        label = c.soft(.label, cmd)
        abort = c.soft(.abort, cmd == "Abort")
    }
}

/// The station's enable state for one page action.
public struct ActionState: Decodable, Equatable {
    public var enabled: Bool
    public var reason: String?

    public init(enabled: Bool, reason: String? = nil) {
        self.enabled = enabled
        self.reason = reason
    }

    enum CodingKeys: String, CodingKey { case enabled, reason }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        enabled = c.soft(.enabled, false)
        reason = c.softOptional(.reason)
    }
}

public struct Page: Decodable, Equatable, Identifiable {
    public var id: String
    public var kind: String
    public var title: String
    public var items: [String]
    public var actions: [PageAction]

    public var pageKind: PageKind {
        switch kind {
        case "items": return .items
        case "cycle": return .cycle
        case "compass": return .compass
        case "voice": return .voice
        default: return .unknown(kind)
        }
    }

    enum CodingKeys: String, CodingKey { case id, kind, title, items, actions }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        kind = c.soft(.kind, "")
        title = c.soft(.title, id)
        items = c.soft(.items, [])
        let decoded: [PageAction] = c.lossyArray(.actions)
        actions = Array(decoded.prefix(maxPageActions))
    }
}

public struct CatalogItem: Decodable, Equatable, Identifiable {
    public var id: String
    public var label: String
    public var group: String

    enum CodingKeys: String, CodingKey { case id, label, group }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        label = c.soft(.label, id)
        group = c.soft(.group, "")
    }
}

public struct CatalogTarget: Decodable, Equatable, Identifiable {
    public var id: String
    public var label: String

    enum CodingKeys: String, CodingKey { case id, label }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        label = c.soft(.label, id)
    }
}

public struct Catalog: Decodable, Equatable {
    public var items: [CatalogItem]
    public var pages: [Page]
    public var targets: [CatalogTarget]

    enum CodingKeys: String, CodingKey { case items, pages, targets }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        items = c.lossyArray(.items)
        pages = c.lossyArray(.pages)
        targets = c.lossyArray(.targets)
    }
}

public struct Snapshot: Decodable, Equatable {
    public var v: Int
    public var seq: Int64
    public var mode: Mode
    public var station: Station?
    public var account: Account?
    public var items: [String: ItemValue]
    public var pages: [Page]
    public var targets: [Target]
    public var settings: WatchSettings?
    public var catalog: Catalog?
    /// Enable state of the page actions.
    public var actions: [String: ActionState]
    /// `phone` (relayed by the phone app) or `direct` (read from the station).
    public var source: String
    /// Seconds without a snapshot after which the phone link counts as lost.
    public var linkTimeoutS: Int

    enum CodingKeys: String, CodingKey {
        case v, seq, mode, station, account, items, pages, targets, settings, catalog, actions, source
        case linkTimeoutS = "link_timeout_s"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        v = c.soft(.v, protocolVersion)
        seq = c.soft(.seq, 0)
        mode = c.soft(.mode, Mode.live)
        station = c.softOptional(.station)
        account = c.softOptional(.account)
        items = c.lossyMap(.items)
        pages = c.lossyArray(.pages)
        targets = c.lossyArray(.targets)
        settings = c.softOptional(.settings)
        catalog = c.softOptional(.catalog)
        actions = c.lossyMap(.actions)
        source = c.soft(.source, "phone")
        linkTimeoutS = max(1, c.soft(.linkTimeoutS, 15))
    }

    public static func parse(_ json: String) -> Snapshot? {
        try? JSONDecoder().decode(Snapshot.self, from: Data(json.utf8))
    }

    /// A status read from the station itself (`GET /api/watch/status`),
    /// shown with the pages, settings and catalog of the phone's last
    /// snapshot. `address` is the host the watch used.
    public func mergedDirect(base: Snapshot?, address: String) -> Snapshot {
        var s = self
        s.source = "direct"
        s.pages = base?.pages ?? []
        s.settings = base?.settings
        s.catalog = base?.catalog
        s.account = base?.account
        if var station = s.station, station.address.isEmpty {
            station.address = address
            s.station = station
        }
        return s
    }

    /// Catalog label for an item id, falling back to the item's own label.
    public func label(forItem id: String) -> String {
        if let l = items[id]?.label, !l.isEmpty { return l }
        return catalog?.items.first { $0.id == id }?.label ?? id
    }
}

/// The only messages the watch sends.
public enum WatchMessage: Encodable, Equatable {
    case hello
    case activity(foreground: Bool, page: String?)
    case settings(WatchSettings)
    /// The map tiles the watch holds (`z/x/y`), so the phone sends only
    /// missing ones; `set` is the manifest it last received.
    case tiles(set: String?, have: [String])

    enum CodingKeys: String, CodingKey { case v, type, foreground, page, settings, set, have }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(protocolVersion, forKey: .v)
        switch self {
        case .hello:
            try c.encode("hello", forKey: .type)
        case let .activity(foreground, page):
            try c.encode("activity", forKey: .type)
            try c.encode(foreground, forKey: .foreground)
            try c.encodeIfPresent(page, forKey: .page)
        case let .settings(settings):
            try c.encode("settings", forKey: .type)
            try c.encode(settings, forKey: .settings)
        case let .tiles(set, have):
            try c.encode("tiles", forKey: .type)
            try c.encodeIfPresent(set, forKey: .set)
            try c.encode(have, forKey: .have)
        }
    }

    public func json() -> String? {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        guard let data = try? encoder.encode(self) else { return nil }
        return String(decoding: data, as: UTF8.self)
    }
}

/// Wall-clock milliseconds; ages are always differences of two such values
/// taken on the watch.
public func currentTimeMs(_ date: Date = Date()) -> Int64 {
    Int64((date.timeIntervalSince1970 * 1000).rounded(.down))
}
