import Foundation

// Status page, alerts page and the vertical auto-cycle. Checked against
// watch/test-vectors.json (connection_status, alerts, auto_cycle), the same
// vectors as the Wear OS core.

public enum StatusTone: String, Equatable {
    case ok, waiting, warn, alert
}

public struct StatusLine: Equatable {
    public var text: String
    public var tone: StatusTone

    public init(_ text: String, _ tone: StatusTone) {
        self.text = text
        self.tone = tone
    }
}

/// What the watch knows about the phone and the phone's station.
public struct ConnectionStatus: Equatable {
    /// Phone app link: reachable, data age.
    public var phone: StatusLine
    /// The phone's station connection; nil before the first snapshot.
    public var station: StatusLine?
    /// Show "Open the GS app on your phone": only when the phone is not
    /// reachable and there is no fresh data from it.
    public var openPhoneApp: Bool

    public static let openPhoneAppText = "Open the GS app on your phone"

    public init(snapshot: Snapshot?, ageMs: Int64, phoneReachable: Bool) {
        guard let snap = snapshot else {
            phone = phoneReachable
                ? StatusLine("Phone app open · waiting for data", .waiting)
                : StatusLine(Self.openPhoneAppText, .alert)
            station = nil
            openPhoneApp = !phoneReachable
            return
        }
        let age = max(0, ageMs)
        let lost = age > Int64(max(1, snap.linkTimeoutS)) * 1000
        switch (lost, phoneReachable) {
        case (false, _):
            phone = StatusLine("Phone connected · updated \(formatAge(age)) ago", .ok)
        case (true, true):
            phone = StatusLine("Phone reachable · no update for \(formatAge(age))", .warn)
        case (true, false):
            phone = StatusLine("Phone not reachable · last update \(formatAge(age)) ago", .alert)
        }
        station = Self.stationLine(snap)
        openPhoneApp = lost && !phoneReachable
    }

    static func stationLine(_ snap: Snapshot) -> StatusLine {
        if snap.mode == .demo { return StatusLine("Demo data (offline preview)", .warn) }
        let name = snap.station?.name ?? ""
        let address = snap.station?.address ?? ""
        let label = !name.isEmpty ? name : !address.isEmpty ? address : "the station"
        switch snap.station?.link ?? "offline" {
        case "connected":
            return StatusLine("Connected to \(label)", .ok)
        case "connecting":
            return StatusLine("Connecting to \(address.isEmpty ? label : address)…", .waiting)
        case "reconnecting":
            return StatusLine("Reconnecting to \(label)…", .warn)
        case "sign_in":
            return StatusLine(address.isEmpty ? "Sign in on the phone" : "Sign in on the phone (\(address))", .warn)
        default:
            if !address.isEmpty { return StatusLine("Can't reach \(address)", .alert) }
            return StatusLine("Phone not connected to a station", .warn)
        }
    }
}

public struct AlertRow: Equatable, Identifiable {
    public var id: String
    public var label: String
    public var text: String
    public var level: Level
}

/// Items at `alert` level first, then `warn`, each group by label.
public func alertRows(_ snapshot: Snapshot?) -> [AlertRow] {
    guard let snap = snapshot else { return [] }
    let rank: (Level) -> Int = { $0 == .alert ? 0 : 1 }
    return snap.items
        .filter { $0.value.level == .alert || $0.value.level == .warn }
        .map { AlertRow(id: $0.key, label: snap.label(forItem: $0.key), text: $0.value.text, level: $0.value.level) }
        .sorted { (rank($0.level), $0.label, $0.id) < (rank($1.level), $1.label, $1.id) }
}

/// The vertical cycle pager: advances every `intervalS`; a manual move
/// (swipe or Digital Crown) restarts the wait so the item being read stays
/// for a full interval; tap pauses/resumes.
public struct AutoCycle: Equatable {
    public private(set) var index = 0
    public private(set) var isPaused: Bool
    /// Last automatic advance, manual move or resume.
    private var lastMoveMs: Int64

    public init(nowMs: Int64, autostart: Bool) {
        lastMoveMs = nowMs
        isPaused = !autostart
    }

    /// Advances when the interval has passed. Returns true when it moved.
    /// After a long gap (screen off) it moves one step, not several.
    @discardableResult
    public mutating func tick(nowMs: Int64, intervalS: Int, count: Int) -> Bool {
        guard count > 0 else { index = 0; return false }
        if index >= count { index = 0 }
        guard !isPaused, nowMs - lastMoveMs >= Int64(max(1, intervalS)) * 1000 else { return false }
        index = (index + 1) % count
        lastMoveMs = nowMs
        return true
    }

    /// The pager's selection changed. A selection equal to the current index
    /// is our own programmatic move echoing back; anything else is the user
    /// scrolling, which restarts the wait. Returns true for a user move.
    @discardableResult
    public mutating func observe(selection: Int, nowMs: Int64) -> Bool {
        guard selection != index else { return false }
        index = selection
        lastMoveMs = nowMs
        return true
    }

    public mutating func togglePause(nowMs: Int64) {
        isPaused.toggle()
        if !isPaused { lastMoveMs = nowMs }
    }

    /// Whole seconds (rounded up) until the next automatic advance.
    public func secondsToNext(nowMs: Int64, intervalS: Int) -> Int? {
        guard !isPaused else { return nil }
        let remaining = max(0, lastMoveMs + Int64(max(1, intervalS)) * 1000 - nowMs)
        return Int((remaining + 999) / 1000)
    }

    /// "2/5 · next in 3s" or "2/5 · paused"; empty without items.
    public func footer(nowMs: Int64, intervalS: Int, count: Int) -> String {
        guard count > 0 else { return "" }
        let position = "\(min(index, count - 1) + 1)/\(count)"
        guard let s = secondsToNext(nowMs: nowMs, intervalS: intervalS) else { return position + " · paused" }
        return position + " · next in \(s)s"
    }
}
