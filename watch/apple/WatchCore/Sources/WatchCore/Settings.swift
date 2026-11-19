import Foundation

// Mirror of src/telemetry_dashboard/watch_companion/settings.rs. The phone
// sanitizes every document against its registry and echoes it back, so the
// watch only keeps its own edits within the documented limits.

public let minIntervalS = 2
public let maxIntervalS = 60
public let minStaleS = 5
public let maxStaleS = 600
public let maxCycleItems = 12
/// Rows on one `items` page.
public let maxPageItems = 8
/// Action buttons on one page.
public let maxPageActions = 2

/// Rows of each `items` page before page configuration existed (version 1),
/// the same as the phone registry.
public let defaultPageItems: [String: [String]] = [
    "flight": ["flight_state", "t_clock", "altitude", "velocity"],
    "propulsion": ["tank_pressure", "chamber_pressure", "loadcell_mass", "fill_percent"],
    "trajectory": ["altitude", "velocity", "gps_altitude", "gps_sats"],
    "health": ["station_link", "boards", "av_battery", "rocket_fix_age"],
]

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
    /// `items` pages: the rows to show, in order. Empty (a version 1
    /// document) means the page's default rows.
    public var items: [String]
    /// Opt-in action buttons (GS command names), at most two.
    public var actions: [String]

    public init(id: String, enabled: Bool, items: [String] = [], actions: [String] = []) {
        self.id = id
        self.enabled = enabled
        self.items = items
        self.actions = actions
    }

    enum CodingKeys: String, CodingKey { case id, enabled, items, actions }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        enabled = c.soft(.enabled, true)
        items = c.soft(.items, [])
        actions = c.soft(.actions, [])
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

/// Map background under the compass: station tiles sent by the phone and
/// cached on the watch, capped at `capMb` (1–5 MB).
public struct MapSetting: Codable, Equatable {
    public var enabled = true
    public var capMb = 3

    public init() {}

    enum CodingKeys: String, CodingKey { case enabled, capMb = "cap_mb" }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let d = MapSetting()
        enabled = c.soft(.enabled, d.enabled)
        capMb = c.soft(.capMb, d.capMb)
    }

    public var capBytes: Int64 { Int64(min(max(capMb, 1), 5)) * 1024 * 1024 }
}

public struct WatchSettings: Codable, Equatable {
    public var version = 2
    public var updatedMs: Int64 = 0
    public var updatedBy = Editor.phone
    /// Display order.
    public var pages = ["flight", "propulsion", "trajectory", "health", "cycle", "compass"]
        .map { PageSetting(id: $0, enabled: true, items: defaultPageItems[$0] ?? []) }
    public var cycle = CycleSetting()
    public var compass = CompassSetting()
    public var units = Units.metric
    public var power = Power.balanced
    /// The watch may read the station itself while the phone app is away.
    public var directLink = true
    /// Map background under the compass.
    public var map = MapSetting()

    public init() {}

    enum CodingKeys: String, CodingKey {
        case version, updatedMs = "updated_ms", updatedBy = "updated_by", pages, cycle, compass, units, power
        case directLink = "direct_link", map
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
        directLink = c.soft(.directLink, d.directLink)
        map = c.soft(.map, d.map)
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

    /// Adds or removes a row on an `items` page; never empties the page,
    /// never exceeds `maxPageItems`. `defaults` are the page's rows when the
    /// document has none yet (version 1).
    public mutating func togglePageItem(_ page: String, _ item: String, defaults: [String], known: Set<String>? = nil) {
        guard let i = pages.firstIndex(where: { $0.id == page }) else { return }
        var items = pages[i].items.isEmpty ? defaults : pages[i].items
        if let at = items.firstIndex(of: item) {
            if items.count > 1 { items.remove(at: at) }
        } else if items.count < maxPageItems, known?.contains(item) ?? true {
            items.append(item)
        }
        pages[i].items = items
    }

    public mutating func movePageItem(_ page: String, _ item: String, up: Bool, defaults: [String]) {
        guard let i = pages.firstIndex(where: { $0.id == page }) else { return }
        if pages[i].items.isEmpty { pages[i].items = defaults }
        moveIn(&pages[i].items, where: { $0 == item }, up: up)
    }

    /// Removes an action button from a page (adding actions is done on the
    /// phone, which knows what the station permits).
    public mutating func removePageAction(_ page: String, _ cmd: String) {
        guard let i = pages.firstIndex(where: { $0.id == page }) else { return }
        pages[i].actions.removeAll { $0 == cmd }
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
/// `pages` are authoritative). Rows come from the settings (the catalog's
/// rows when none are set); actions are truncated to two and then limited
/// to those the phone published on some page, i.e. the ones the station
/// permits. Checked against the `page_layout` vectors.
public func resolvedPages(_ settings: WatchSettings, catalog: [Page], published: [Page] = []) -> [Page] {
    var permitted: [String: PageAction] = [:]
    for page in published {
        for action in page.actions where permitted[action.cmd] == nil { permitted[action.cmd] = action }
    }
    return settings.pages.filter(\.enabled).compactMap { s in
        guard var page = catalog.first(where: { $0.id == s.id }) else { return nil }
        if page.pageKind == .items, !s.items.isEmpty {
            page.items = Array(s.items.prefix(maxPageItems))
        }
        var actions: [PageAction] = []
        for cmd in s.actions.prefix(maxPageActions) {
            if let action = permitted[cmd], !actions.contains(where: { $0.cmd == cmd }) { actions.append(action) }
        }
        page.actions = actions
        return page
    }
}

/// True once the phone's echo includes our edit (or a newer one).
public func echoConfirms(pending: WatchSettings, echoed: WatchSettings?) -> Bool {
    guard let echoed else { return false }
    return echoed.updatedMs >= pending.updatedMs
}
