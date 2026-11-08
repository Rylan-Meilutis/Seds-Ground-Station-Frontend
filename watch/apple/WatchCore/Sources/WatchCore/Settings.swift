import Foundation

// Mirror of src/telemetry_dashboard/watch_companion/settings.rs. The phone
// sanitizes every document against its registry and echoes it back, so the
// watch only keeps its own edits within the documented limits.

public let minIntervalS = 2
public let maxIntervalS = 60
public let minStaleS = 5
public let maxStaleS = 600
public let maxCycleItems = 12

public enum North: String, Codable, CaseIterable {
    case trueNorth = "true"
    case magnetic
}

public enum Units: String, Codable, CaseIterable {
    case metric, imperial
}

public enum Power: String, Codable, CaseIterable {
    case saver, balanced, recovery
}

public enum Editor: String, Codable {
    case phone, watch, account
}

public struct PageSetting: Codable, Equatable, Identifiable {
    public var id: String
    public var enabled: Bool

    public init(id: String, enabled: Bool) {
        self.id = id
        self.enabled = enabled
    }

    enum CodingKeys: String, CodingKey { case id, enabled }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        enabled = c.soft(.enabled, true)
    }
}

public struct CycleSetting: Codable, Equatable {
    public var items = ["flight_state", "t_clock", "altitude", "velocity", "tank_pressure"]
    public var intervalS = 5
    /// Start cycling as soon as the cycle page is shown.
    public var autostart = true

    public init() {}

    enum CodingKeys: String, CodingKey { case items, intervalS = "interval_s", autostart }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = CycleSetting()
        items = c.soft(.items, d.items)
        intervalS = c.soft(.intervalS, d.intervalS)
        autostart = c.soft(.autostart, d.autostart)
    }
}

public struct CompassSetting: Codable, Equatable {
    public var north = North.trueNorth
    /// A target fix older than this is shown as stale.
    public var staleAfterS = 30
    public var targets = ["rocket", "ground_station", "rrls"]

    public init() {}

    enum CodingKeys: String, CodingKey { case north, staleAfterS = "stale_after_s", targets }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = CompassSetting()
        north = c.soft(.north, d.north)
        staleAfterS = c.soft(.staleAfterS, d.staleAfterS)
        targets = c.soft(.targets, d.targets)
    }
}

public struct WatchSettings: Codable, Equatable {
    public var version = 1
    public var updatedMs: Int64 = 0
    public var updatedBy = Editor.phone
    /// Display order.
    public var pages = ["flight", "propulsion", "trajectory", "health", "cycle", "compass"]
        .map { PageSetting(id: $0, enabled: true) }
    public var cycle = CycleSetting()
    public var compass = CompassSetting()
    public var units = Units.metric
    public var power = Power.balanced

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, updatedMs = "updated_ms", updatedBy = "updated_by", pages, cycle, compass, units, power
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = WatchSettings()
        version = c.soft(.version, d.version)
        updatedMs = c.soft(.updatedMs, d.updatedMs)
        updatedBy = c.soft(.updatedBy, d.updatedBy)
        pages = c.contains(.pages) ? c.lossyArray(.pages) : d.pages
        cycle = c.soft(.cycle, d.cycle)
        compass = c.soft(.compass, d.compass)
        units = c.soft(.units, d.units)
        power = c.soft(.power, d.power)
    }

    public var metric: Bool { units == .metric }

    /// Stamps an edit; time never goes backwards relative to `previousMs`.
    public func stamped(previousMs: Int64, nowMs: Int64, by editor: Editor = .watch) -> WatchSettings {
        var s = self
        s.updatedMs = max(nowMs, previousMs == Int64.max ? previousMs : previousMs + 1)
        s.updatedBy = editor
        return s
    }

    // MARK: Editing helpers (same invariants as the phone).

    public mutating func movePage(_ id: String, up: Bool) {
        moveIn(&pages, where: { $0.id == id }, up: up)
    }

    public mutating func togglePage(_ id: String) {
        if let i = pages.firstIndex(where: { $0.id == id }) { pages[i].enabled.toggle() }
    }

    /// Removes or adds a cycle item; never empties the list, never exceeds
    /// 12 items, and only adds ids in `known` when a catalog is available.
    public mutating func toggleCycleItem(_ id: String, known: Set<String>? = nil) {
        if let i = cycle.items.firstIndex(of: id) {
            if cycle.items.count > 1 { cycle.items.remove(at: i) }
        } else if cycle.items.count < maxCycleItems, known?.contains(id) ?? true {
            cycle.items.append(id)
        }
    }

    public mutating func moveCycleItem(_ id: String, up: Bool) {
        moveIn(&cycle.items, where: { $0 == id }, up: up)
    }

    public mutating func toggleTarget(_ id: String, known: Set<String>? = nil) {
        if let i = compass.targets.firstIndex(of: id) {
            if compass.targets.count > 1 { compass.targets.remove(at: i) }
        } else if known?.contains(id) ?? true {
            compass.targets.append(id)
        }
    }

    public mutating func setIntervalS(_ s: Int) {
        cycle.intervalS = min(max(s, minIntervalS), maxIntervalS)
    }

    public mutating func setStaleAfterS(_ s: Int) {
        compass.staleAfterS = min(max(s, minStaleS), maxStaleS)
    }
}

private func moveIn<T>(_ list: inout [T], where pred: (T) -> Bool, up: Bool) {
    guard let i = list.firstIndex(where: pred) else { return }
    if up, i > 0 {
        list.swapAt(i, i - 1)
    } else if !up, i + 1 < list.count {
        list.swapAt(i, i + 1)
    }
}

/// Pages to show for `settings`, resolved against the phone's catalog (used
/// while a local edit has not been echoed yet; otherwise the snapshot's
/// `pages` are authoritative).
public func resolvedPages(_ settings: WatchSettings, catalog: [Page]) -> [Page] {
    settings.pages.filter(\.enabled).compactMap { s in catalog.first { $0.id == s.id } }
}

/// True once the phone's echo includes our edit (or a newer one).
public func echoConfirms(pending: WatchSettings, echoed: WatchSettings?) -> Bool {
    guard let echoed else { return false }
    return echoed.updatedMs >= pending.updatedMs
}
