import Foundation

/// `T- ` counting down (clamped at 0) or `T+ ` counting up from the value at
/// send time; nil for an idle clock. Same text as the phone (`tclock.rs`).
public func clockText(_ clock: LaunchClock, elapsedMs: Int64) -> String? {
    let elapsed = max(0, elapsedMs)
    switch clock.kind {
    case "tminus": return tclockText(beforeT0: true, magnitudeMs: UInt64(max(0, saturatingAdd(clock.ms, -elapsed))))
    case "tplus": return tclockText(beforeT0: false, magnitudeMs: UInt64(max(0, saturatingAdd(clock.ms, elapsed))))
    default: return nil
    }
}

/// Signed ms from T-0: negative is `T- `, zero and up `T+ `.
public func tclockText(msFromT0: Int64, fractionDigits: Int = 0) -> String {
    tclockText(beforeT0: msFromT0 < 0, magnitudeMs: msFromT0.magnitude, fractionDigits: fractionDigits)
}

public func tclockText(beforeT0: Bool, magnitudeMs: UInt64, fractionDigits: Int = 0) -> String {
    (beforeT0 ? "T- " : "T+ ") + tclockBody(magnitudeMs, fractionDigits: fractionDigits)
}

/// `MM:SS` under an hour, `H:MM:SS` from one hour, `D:HH:MM:SS` from one day,
/// plus a truncated fraction of up to 3 digits. The lead field never wraps.
func tclockBody(_ magnitudeMs: UInt64, fractionDigits: Int = 0) -> String {
    let digits = min(3, max(0, fractionDigits))
    let total = magnitudeMs / 1000
    let days = total / 86_400, hours = total / 3_600 % 24
    let minutes = total / 60 % 60, seconds = total % 60
    var text: String
    if days > 0 {
        text = "\(days):\(pad2u(hours)):\(pad2u(minutes)):\(pad2u(seconds))"
    } else if hours > 0 {
        text = "\(hours):\(pad2u(minutes)):\(pad2u(seconds))"
    } else {
        text = "\(pad2u(minutes)):\(pad2u(seconds))"
    }
    if digits > 0 {
        var unit: UInt64 = 1
        for _ in digits..<3 { unit *= 10 }
        let fraction = String(magnitudeMs % 1000 / unit)
        text += "." + String(repeating: "0", count: digits - fraction.count) + fraction
    }
    return text
}

private func pad2u(_ v: UInt64) -> String { v < 10 ? "0\(v)" : "\(v)" }

private func saturatingAdd(_ a: Int64, _ b: Int64) -> Int64 {
    let (sum, overflow) = a.addingReportingOverflow(b)
    return overflow ? (b > 0 ? .max : .min) : sum
}
