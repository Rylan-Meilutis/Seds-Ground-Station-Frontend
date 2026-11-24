import Foundation

// Recovery search on the watch (docs/watch-companion.md "Recovery search").
// The watch records its own track only while the station's search runs AND
// the watch's user opted in on the watch AND the phone reports that the
// station accepted that opt-in. Fixes wait in a bounded, persisted buffer
// and go to the phone in `search_track` messages; the phone acknowledges the
// highest contiguous sequence number it holds, and the watch then drops them.
// Same rules as the Wear core (`Search.kt`) and the phone relay
// (`watch_companion/search_relay.rs`).

/// The snapshot's `search` object (absent when the phone has no search data).
public struct SearchInfo: Decodable, Equatable {
    public var active: Bool
    public var sessionId: String?
    /// The station accepted this watch's opt-in for `sessionId`.
    public var sharing: Bool
    /// Highest contiguous watch sequence number the phone holds.
    public var ackedSeq: Int64
    public var percent: Double?
    public var swathM: Double?

    enum CodingKeys: String, CodingKey {
        case active, sharing, percent
        case sessionId = "session_id", ackedSeq = "acked_seq", swathM = "swath_m"
    }

    public init(active: Bool, sessionId: String?, sharing: Bool, ackedSeq: Int64, percent: Double? = nil, swathM: Double? = nil) {
        self.active = active
        self.sessionId = sessionId
        self.sharing = sharing
        self.ackedSeq = ackedSeq
        self.percent = percent
        self.swathM = swathM
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        active = c.soft(.active, false)
        sessionId = c.softOptional(.sessionId)
        sharing = c.soft(.sharing, false)
        ackedSeq = c.soft(.ackedSeq, Int64(0))
        percent = c.softOptional(.percent)
        swathM = c.softOptional(.swathM)
    }
}

public struct SearchFix: Codable, Equatable {
    public var seq: Int64
    public var tMs: Int64
    public var lat: Double
    public var lon: Double
    public var accM: Double?

    enum CodingKeys: String, CodingKey {
        case seq, lat, lon
        case tMs = "t_ms", accM = "acc_m"
    }

    public init(seq: Int64, tMs: Int64, lat: Double, lon: Double, accM: Double?) {
        self.seq = seq
        self.tMs = tMs
        self.lat = lat
        self.lon = lon
        self.accM = accM
    }
}

public enum SearchLimits {
    public static let minMoveM = 5.0
    public static let maxIntervalMs: Int64 = 30_000
    public static let maxAccuracyM = 100.0
    public static let maxPending = 5_000
    public static let maxMessagePoints = 100
}

/// The watch's recorder and buffer (pure; persisted as JSON by the app).
public struct SearchRecorder: Codable, Equatable {
    /// The watch user's own choice ("Share watch track"). Never set by the phone.
    public var wantsToShare: Bool = false
    /// The session the buffer belongs to.
    public var sessionId: String? = nil
    public var nextSeq: Int64 = 1
    public var pending: [SearchFix] = []
    public var dropped: Int64 = 0
    var last: SearchFix? = nil

    public init() {}

    /// Whether the watch records now.
    public func recording(_ info: SearchInfo?) -> Bool {
        guard wantsToShare, let info, info.active, info.sharing, let session = info.sessionId else { return false }
        return sessionId == nil || sessionId == session
    }

    /// Follows the phone's snapshot: drops what the phone holds, and drops
    /// everything when the search moved to another session or ended for
    /// good (no session).
    public mutating func apply(_ info: SearchInfo?) {
        guard let info else { return }
        guard let session = info.sessionId else {
            pending.removeAll()
            sessionId = nil
            wantsToShare = false
            return
        }
        if sessionId != nil, sessionId != session {
            pending.removeAll()
            wantsToShare = false
        }
        sessionId = session
        nextSeq = max(nextSeq, info.ackedSeq + 1)
        pending.removeAll { $0.seq <= info.ackedSeq }
    }

    /// Offers one fix; returns true when it was kept.
    @discardableResult
    public mutating func record(lat: Double, lon: Double, accuracyM: Double?, tMs: Int64, info: SearchInfo?) -> Bool {
        guard recording(info), lat.isFinite, lon.isFinite, abs(lat) <= 90, abs(lon) <= 180,
              !(abs(lat) < 1e-6 && abs(lon) < 1e-6) else { return false }
        if let a = accuracyM, !(a >= 0 && a <= SearchLimits.maxAccuracyM) { return false }
        if let l = last {
            let moved = distanceM(LatLon(lat: l.lat, lon: l.lon), LatLon(lat: lat, lon: lon)) ?? 0
            guard tMs > l.tMs, moved >= SearchLimits.minMoveM || tMs - l.tMs >= SearchLimits.maxIntervalMs else { return false }
        }
        let fix = SearchFix(seq: nextSeq, tMs: tMs, lat: lat, lon: lon, accM: accuracyM)
        nextSeq += 1
        last = fix
        pending.append(fix)
        if pending.count > SearchLimits.maxPending {
            dropped += Int64(pending.count - SearchLimits.maxPending)
            pending.removeFirst(pending.count - SearchLimits.maxPending)
        }
        return true
    }

    /// The next message for the phone, oldest first.
    public func nextMessage() -> WatchMessage? {
        guard let session = sessionId, !pending.isEmpty else { return nil }
        return .searchTrack(sessionId: session, points: Array(pending.prefix(SearchLimits.maxMessagePoints)))
    }

    /// The user stopped sharing on the watch: nothing more is recorded and
    /// the buffer is discarded.
    public mutating func stop() {
        wantsToShare = false
        pending.removeAll()
        last = nil
    }
}
