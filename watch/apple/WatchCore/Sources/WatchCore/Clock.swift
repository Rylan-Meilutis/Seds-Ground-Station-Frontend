import Foundation

/// `T- mm:ss` counting down (clamped at 0) or `T+ mm:ss` counting up from
/// the value at send time; nil for an idle clock.
public func clockText(_ clock: LaunchClock, elapsedMs: Int64) -> String? {
    let elapsed = max(0, elapsedMs)
    switch clock.kind {
    case "tminus": return "T- " + minutesSeconds(clock.ms - elapsed)
    case "tplus": return "T+ " + minutesSeconds(clock.ms + elapsed)
    default: return nil
    }
}

func minutesSeconds(_ ms: Int64) -> String {
    let s = max(0, ms) / 1000
    return "\(pad2(s / 60)):\(pad2(s % 60))"
}

/// Phone link health derived from the last snapshot receive time.
public enum LinkState: Equatable {
    /// No snapshot was ever received (first run).
    case waiting
    case live
    /// No snapshot for longer than the timeout; associated value is the age
    /// of the last data in ms.
    case lost(lastDataMs: Int64)

    public init(receivedAtMs: Int64?, nowMs: Int64, timeoutS: Int) {
        guard let at = receivedAtMs else {
            self = .waiting
            return
        }
        let age = max(0, nowMs - at)
        self = age > Int64(timeoutS) * 1000 ? .lost(lastDataMs: age) : .live
    }

    public var lostText: String? {
        if case let .lost(ms) = self { return "Phone link lost · last data \(formatAge(ms))" }
        return nil
    }
}

/// Auto-cycle timing with pause/resume. Times are wall-clock ms.
public struct CycleScheduler: Equatable {
    public private(set) var isRunning = false
    private var accumulatedMs: Int64 = 0
    private var startedAtMs: Int64 = 0

    public init() {}

    public mutating func start(at nowMs: Int64) {
        guard !isRunning else { return }
        startedAtMs = nowMs
        isRunning = true
    }

    public mutating func pause(at nowMs: Int64) {
        guard isRunning else { return }
        accumulatedMs += max(0, nowMs - startedAtMs)
        isRunning = false
    }

    public mutating func toggle(at nowMs: Int64) {
        if isRunning { pause(at: nowMs) } else { start(at: nowMs) }
    }

    public func elapsedMs(at nowMs: Int64) -> Int64 {
        accumulatedMs + (isRunning ? max(0, nowMs - startedAtMs) : 0)
    }

    /// Index of the item to show after the elapsed running time.
    public func index(at nowMs: Int64, intervalS: Int, count: Int) -> Int {
        guard count > 0 else { return 0 }
        let step = Int64(max(intervalS, 1)) * 1000
        return Int((elapsedMs(at: nowMs) / step) % Int64(count))
    }
}
