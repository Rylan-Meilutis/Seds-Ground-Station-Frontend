import Foundation
import Security
import WatchCore

/// The watch's own connection to the station, used while the phone app is
/// suspended or out of reach. The phone provisions the station address and
/// a scoped watch token (WatchConnectivity user info, delivered even after
/// the phone app is suspended); the token lives only in the keychain.
///
/// Polls `GET /api/watch/status` with URLSession while the app is in the
/// foreground and the phone's data is not fresh (plain HTTP requests, no
/// long-lived sockets), and sends page actions to the ordinary
/// `POST /api/command` with the token, so the station's authorization and
/// admission apply exactly as for the phone. All state changes happen on the
/// main queue.
final class DirectLink: ObservableObject {
    @Published private(set) var provision: DirectProvision?
    @Published private(set) var snapshot: Snapshot?
    @Published private(set) var receivedAtMs: Int64?
    @Published private(set) var lastError: String?
    @Published private(set) var polling = false

    private enum Keys {
        static let provision = "gs26.direct.provision"
    }

    private let defaults = UserDefaults.standard
    private let trust = TrustDelegate()
    private lazy var session: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 8
        config.timeoutIntervalForResource = 10
        config.waitsForConnectivity = false
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        return URLSession(configuration: config, delegate: trust, delegateQueue: nil)
    }()
    private var timer: Timer?
    private var inFlight = false

    init() {
        if let data = defaults.data(forKey: Keys.provision),
           var stored = try? JSONDecoder().decode(DirectProvision.self, from: data) {
            stored.token = Keychain.token()
            if stored.configured {
                provision = stored
                trust.insecureHost = stored.skipTLS ? URL(string: stored.base)?.host : nil
            }
        }
    }

    var configured: Bool { provision?.configured ?? false }

    // MARK: Provisioning (from PhoneLink, on the main queue)

    func apply(provisionJSON json: String) {
        guard let next = DirectProvision.parse(json) else { return }
        if next.configured, let token = next.token {
            Keychain.setToken(token)
        } else {
            Keychain.deleteToken()
        }
        var stored = next
        stored.token = nil
        if let data = try? JSONEncoder().encode(stored) { defaults.set(data, forKey: Keys.provision) }
        if !next.configured || provision?.base != next.base {
            snapshot = nil
            receivedAtMs = nil
        }
        provision = next.configured ? next : nil
        trust.insecureHost = next.skipTLS ? URL(string: next.base)?.host : nil
        lastError = nil
    }

    // MARK: Polling

    /// Runs the poll timer while `active` (the app is in the foreground).
    /// Each tick asks `request` for the status URL, or nil when the route
    /// logic (WatchCore `shouldPollDirect`) says not to poll now.
    func setActive(_ active: Bool, intervalMs: Int64, request: @escaping () -> URL?) {
        timer?.invalidate()
        timer = nil
        guard active else {
            if polling { polling = false }
            return
        }
        let tick: () -> Void = { [weak self] in
            guard let self else { return }
            let url = request()
            // Only publish real changes: every set re-renders the views.
            if self.polling != (url != nil) { self.polling = url != nil }
            if let url { self.poll(url) }
        }
        tick()
        timer = Timer.scheduledTimer(withTimeInterval: Double(max(1_000, intervalMs)) / 1000, repeats: true) { _ in tick() }
    }

    private func poll(_ url: URL) {
        guard !inFlight, let token = provision?.token, let address = provision?.address else { return }
        inFlight = true
        var request = URLRequest(url: url)
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        session.dataTask(with: request) { [weak self] data, response, error in
            let status = (response as? HTTPURLResponse)?.statusCode
            let text = data.flatMap { String(data: $0, encoding: .utf8) }
            DispatchQueue.main.async {
                guard let self else { return }
                self.inFlight = false
                if let error {
                    self.lastError = Self.describe(error)
                    return
                }
                guard status == 200 else {
                    self.lastError = status == 401 ? "watch token not accepted" : "HTTP \(status ?? 0)"
                    return
                }
                guard let text, var snap = Snapshot.parse(text) else {
                    self.lastError = "unreadable reply"
                    return
                }
                if var station = snap.station, station.address.isEmpty {
                    station.address = address
                    snap.station = station
                }
                self.snapshot = snap
                self.receivedAtMs = currentTimeMs()
                self.lastError = nil
            }
        }.resume()
    }

    // MARK: Actions

    /// Sends one page action; `done` runs on the main queue. The caller has
    /// already shown the confirmation; this never retries (a retry could
    /// send the command twice).
    func send(_ cmd: String, done: @escaping (ActionResult) -> Void) {
        guard let provision, provision.configured, let token = provision.token,
              let url = commandURL(base: provision.base)
        else {
            done(ActionResult(.refused, "Refused: the direct link is not set up on the phone."))
            return
        }
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = commandBody(cmd)
        request.timeoutInterval = 10
        session.dataTask(with: request) { data, response, error in
            let result: ActionResult
            if let error {
                result = actionOutcome(status: nil, body: "", error: Self.describe(error))
            } else {
                let status = (response as? HTTPURLResponse)?.statusCode
                result = actionOutcome(status: status, body: data.map { String(decoding: $0, as: UTF8.self) } ?? "", error: nil)
            }
            DispatchQueue.main.async { done(result) }
        }.resume()
    }

    private static func describe(_ error: Error) -> String {
        guard let e = error as? URLError else { return error.localizedDescription }
        switch e.code {
        case .timedOut: return "timed out"
        case .notConnectedToInternet, .networkConnectionLost: return "no network"
        case .cannotFindHost, .dnsLookupFailed: return "host not found"
        case .cannotConnectToHost: return "connection refused"
        case .secureConnectionFailed, .serverCertificateUntrusted, .serverCertificateHasBadDate,
             .serverCertificateNotYetValid, .serverCertificateHasUnknownRoot:
            return "TLS certificate not trusted"
        case .appTransportSecurityRequiresSecureConnection: return "blocked by App Transport Security"
        default: return e.localizedDescription
        }
    }
}

/// Accepts the station's certificate without verification only for the
/// host the phone itself connects to that way ("skip TLS verification").
final class TrustDelegate: NSObject, URLSessionDelegate {
    private let lock = NSLock()
    private var host: String?

    var insecureHost: String? {
        get { lock.lock(); defer { lock.unlock() }; return host }
        set { lock.lock(); host = newValue; lock.unlock() }
    }

    func urlSession(_ session: URLSession, didReceive challenge: URLAuthenticationChallenge,
                    completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        if challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
           let host = insecureHost, challenge.protectionSpace.host == host,
           let serverTrust = challenge.protectionSpace.serverTrust {
            completionHandler(.useCredential, URLCredential(trust: serverTrust))
        } else {
            completionHandler(.performDefaultHandling, nil)
        }
    }
}

/// The watch token in the watch keychain (this device only; readable after
/// first unlock so polling works with the screen dimmed).
enum Keychain {
    private static let service = "com.UBSEDS.GS26.watch.direct"
    private static let account = "watch-token"

    private static var query: [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account]
    }

    static func token() -> String? {
        var q = query
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let data = out as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    static func setToken(_ token: String) {
        let attributes: [String: Any] = [kSecValueData as String: Data(token.utf8),
                                         kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly]
        if SecItemUpdate(query as CFDictionary, attributes as CFDictionary) == errSecItemNotFound {
            var add = query
            add.merge(attributes) { $1 }
            SecItemAdd(add as CFDictionary, nil)
        }
    }

    static func deleteToken() {
        SecItemDelete(query as CFDictionary)
    }
}
