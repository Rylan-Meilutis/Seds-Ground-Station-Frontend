import Foundation

// Direct link to the station (docs/watch-companion.md "Direct link"). The
// phone provisions the station address and a scoped watch token; while the
// phone's data is not fresh the watch polls GET /api/watch/status itself and
// sends page actions to POST /api/command with the token. Checked against
// watch/test-vectors.json (link_route, action_outcome, action_button,
// provision, direct_status.merge), the same vectors as the Wear OS core.

/// Where the shown data comes from.
public enum LinkRoute: String, Equatable {
    case phone, direct, none
}

/// The phone's data is fresh enough to prefer it: recent, and the phone is
/// connected to a station (or showing its demo).
public func phoneIsFresh(mode: Mode?, ageMs: Int64?, freshMs: Int64) -> Bool {
    guard let mode, let ageMs else { return false }
    return mode != .noStation && ageMs <= freshMs
}

/// Prefer the phone while its data is fresh; otherwise fresh direct data;
/// otherwise whichever is newer.
public func chooseRoute(phoneMode: Mode?, phoneAgeMs: Int64?, directAgeMs: Int64?, freshMs: Int64) -> LinkRoute {
    if phoneIsFresh(mode: phoneMode, ageMs: phoneAgeMs, freshMs: freshMs) { return .phone }
    if let direct = directAgeMs, direct <= freshMs { return .direct }
    switch (phoneAgeMs, directAgeMs) {
    case let (phone?, direct?): return direct < phone ? .direct : .phone
    case (.some, nil): return .phone
    case (nil, .some): return .direct
    case (nil, nil): return .none
    }
}

/// Poll the station only while the app is in front, the phone provisioned
/// the link and the phone's data is not fresh.
public func shouldPollDirect(phoneMode: Mode?, phoneAgeMs: Int64?, freshMs: Int64, foreground: Bool, configured: Bool) -> Bool {
    foreground && configured && !phoneIsFresh(mode: phoneMode, ageMs: phoneAgeMs, freshMs: freshMs)
}

/// Phone data older than this is not preferred (three missed snapshots at
/// the foreground rate, at least 6 s).
public func phoneFreshMs(_ power: Power) -> Int64 {
    let active: Int64
    switch power {
    case .saver: active = 5_000
    case .balanced: active = 2_000
    case .recovery: active = 1_000
    }
    return max(6_000, 3 * active)
}

/// How often the watch polls the station itself.
public func directPollMs(_ power: Power) -> Int64 {
    switch power {
    case .saver: return 5_000
    case .balanced: return 3_000
    case .recovery: return 2_000
    }
}

// MARK: Provisioning

/// The phone's direct-link message (`type: direct`).
public struct DirectProvision: Codable, Equatable {
    public var enabled: Bool
    public var addresses: [String]
    /// Kept in the keychain only; nil in any other stored copy.
    public var token: String?
    public var expiresAtMs: Int64?
    public var username: String
    public var station: String
    public var skipTLS: Bool
    public var commands: [String]
    /// The token may join crew voice (granted by the station only with the
    /// voice transmit permission).
    public var voice: Bool

    enum CodingKeys: String, CodingKey {
        case enabled, addresses, token, username, station, commands, voice
        case expiresAtMs = "expires_at_ms", skipTLS = "skip_tls"
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        enabled = c.soft(.enabled, false)
        addresses = c.soft(.addresses, [])
        token = c.softOptional(.token)
        expiresAtMs = c.softOptional(.expiresAtMs)
        username = c.soft(.username, "")
        station = c.soft(.station, "")
        skipTLS = c.soft(.skipTLS, false)
        commands = c.soft(.commands, [])
        voice = c.soft(.voice, false)
    }

    /// Parses the phone's message; nil for another version or type.
    public static func parse(_ json: String) -> DirectProvision? {
        struct Header: Decodable { let v: Int?; let type: String? }
        guard let data = json.data(using: .utf8),
              let header = try? JSONDecoder().decode(Header.self, from: data),
              header.v == protocolVersion, header.type == "direct"
        else { return nil }
        return try? JSONDecoder().decode(DirectProvision.self, from: data)
    }

    /// First address without a trailing slash.
    public var base: String {
        var b = addresses.first ?? ""
        while b.hasSuffix("/") { b.removeLast() }
        return b
    }

    /// host[:port] of `base`.
    public var address: String {
        var rest = Substring(base)
        if let r = rest.range(of: "://") { rest = rest[r.upperBound...] }
        return String(rest.split(separator: "/", maxSplits: 1, omittingEmptySubsequences: false).first ?? "")
    }

    /// The watch can reach the station by itself.
    public var configured: Bool {
        enabled && !(token ?? "").isEmpty && !base.isEmpty
    }
}

// MARK: Requests

public func statusURL(base: String, units: Units, actions: [String], staleAfterS: Int) -> URL? {
    var c = URLComponents(string: base + "/api/watch/status")
    c?.queryItems = [
        URLQueryItem(name: "units", value: units.rawValue),
        URLQueryItem(name: "stale_after_s", value: String(staleAfterS)),
    ] + (actions.isEmpty ? [] : [URLQueryItem(name: "actions", value: actions.joined(separator: ","))])
    return c?.url
}

public func commandURL(base: String) -> URL? {
    URL(string: base + "/api/command")
}

/// The station's map tile (read-only; allowed for the watch token).
public func tileURL(base: String, key: String) -> URL? {
    guard TileId.parse(key) != nil else { return nil }
    return URL(string: base + "/tiles/" + key + ".jpg")
}

/// `POST /api/command` takes the command name as a JSON string.
public func commandBody(_ cmd: String) -> Data {
    (try? JSONEncoder().encode(cmd)) ?? Data()
}

/// Every action on the shown pages, in page order.
public func pageActionCommands(_ pages: [Page]) -> [String] {
    var out: [String] = []
    for page in pages {
        for action in page.actions where !out.contains(action.cmd) { out.append(action.cmd) }
    }
    return out
}

// MARK: Actions

public enum ActionOutcome: String, Equatable {
    case accepted, refused, unknown
}

public struct ActionResult: Equatable {
    public var outcome: ActionOutcome
    public var text: String

    public init(_ outcome: ActionOutcome, _ text: String) {
        self.outcome = outcome
        self.text = text
    }
}

/// What a press of a page action ended in, from the HTTP reply of
/// POST /api/command (or the network error).
public func actionOutcome(status: Int?, body: String, error: String?) -> ActionResult {
    let detail = String(body.trimmingCharacters(in: .whitespacesAndNewlines).prefix(120))
    if let error { return ActionResult(.unknown, "Outcome unknown: \(error). Check the station.") }
    guard let status else { return ActionResult(.unknown, "Outcome unknown. Check the station.") }
    switch status {
    case 200..<300:
        return ActionResult(.accepted, detail.isEmpty ? "Accepted" : "Accepted: \(detail)")
    case 401:
        return ActionResult(.refused, "Refused: the watch token was not accepted. Open the GS app on the phone.")
    case 408, 429, 500...:
        return ActionResult(.unknown, "Outcome unknown (HTTP \(status)). Check the station.")
    case 400..<500:
        return ActionResult(.refused, detail.isEmpty ? "Refused (HTTP \(status))" : "Refused: \(detail)")
    default:
        return ActionResult(.unknown, "Outcome unknown (HTTP \(status)). Check the station.")
    }
}

public enum ConfirmStyle: String, Equatable {
    /// Abort: tap, then confirm.
    case tap
    /// Everything else: confirm screen, then press and hold.
    case hold
}

public struct ActionButtonState: Equatable {
    public var enabled: Bool
    public var note: String?
    public var confirm: ConfirmStyle

    public init(enabled: Bool, note: String?, confirm: ConfirmStyle) {
        self.enabled = enabled
        self.note = note
        self.confirm = confirm
    }
}

/// How a page action button looks. Abort is never disabled by the policy
/// or stale data (the station admits a manual Abort under every policy);
/// everything else follows the station's enable state and needs fresh data.
public func actionButton(abort: Bool, state: ActionState?, fresh: Bool, token: Bool) -> ActionButtonState {
    let confirm: ConfirmStyle = abort ? .tap : .hold
    guard token else { return ActionButtonState(enabled: false, note: "Direct link not set up on the phone", confirm: confirm) }
    if abort { return ActionButtonState(enabled: true, note: state?.reason, confirm: confirm) }
    guard fresh else { return ActionButtonState(enabled: false, note: "No fresh station data", confirm: confirm) }
    guard let state, state.enabled else {
        return ActionButtonState(enabled: false, note: state?.reason ?? "Disabled by the station", confirm: confirm)
    }
    return ActionButtonState(enabled: true, note: nil, confirm: confirm)
}
