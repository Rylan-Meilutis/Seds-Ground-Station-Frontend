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
