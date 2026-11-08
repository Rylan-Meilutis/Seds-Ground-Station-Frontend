import Foundation
import WatchConnectivity
import WatchCore

/// WatchConnectivity link to the GS26 phone app. Receives snapshots and
/// sends only `hello`, `activity` and `settings` messages.
final class PhoneLink: NSObject, ObservableObject, WCSessionDelegate {
    @Published private(set) var snapshot: Snapshot? = nil
    @Published private(set) var receivedAtMs: Int64? = nil
    /// Local settings edit not yet echoed back by the phone.
    @Published private(set) var pendingSettings: WatchSettings? = nil

    private enum Keys {
        static let snapshot = "gs26.snapshot.json"
        static let receivedAt = "gs26.snapshot.receivedAtMs"
    }

    private let defaults = UserDefaults.standard
    private var foreground = false
    private var currentPage: String?
    private var activityTimer: Timer?

    override init() {
        super.init()
        if let json = defaults.string(forKey: Keys.snapshot), let snap = Snapshot.parse(json) {
            snapshot = snap
            receivedAtMs = Int64(defaults.double(forKey: Keys.receivedAt))
        }
        if WCSession.isSupported() {
            WCSession.default.delegate = self
            WCSession.default.activate()
        }
    }

    // MARK: Derived state

    /// Effective settings: our pending edit, else the phone's copy.
    var settings: WatchSettings? { pendingSettings ?? snapshot?.settings }

    var isSyncing: Bool { pendingSettings != nil }

    /// Pages in display order. A pending local edit is applied at once by
    /// resolving it against the phone's catalog.
    var pages: [Page] {
        guard let snap = snapshot else { return [] }
        if let pending = pendingSettings, let catalog = snap.catalog?.pages, !catalog.isEmpty {
            return resolvedPages(pending, catalog: catalog)
        }
        return snap.pages
    }

    func linkState(nowMs: Int64) -> LinkState {
        LinkState(receivedAtMs: receivedAtMs, nowMs: nowMs, timeoutS: snapshot?.linkTimeoutS ?? 15)
    }

    func elapsedMs(nowMs: Int64) -> Int64 {
        max(0, nowMs - (receivedAtMs ?? nowMs))
    }

    func compassDisplays(reading: CompassReading, nowMs: Int64) -> [TargetDisplay] {
        guard let snap = snapshot, let at = receivedAtMs else { return [] }
        return Compass.displays(targets: snap.targets, settings: settings ?? WatchSettings(),
                                reading: reading, receivedAtMs: at, nowMs: nowMs)
    }

    // MARK: Watch -> phone

    func setForeground(_ isForeground: Bool) {
        foreground = isForeground
        sendActivity()
        activityTimer?.invalidate()
        activityTimer = nil
        if isForeground {
            activityTimer = Timer.scheduledTimer(withTimeInterval: 15, repeats: true) { [weak self] _ in
                self?.sendActivity()
            }
        }
    }

    func setPage(_ id: String?) {
        guard id != currentPage else { return }
        currentPage = id
        if foreground { sendActivity() }
    }

    /// Applies a settings edit locally at once and sends it to the phone.
    func edit(_ change: (inout WatchSettings) -> Void) {
        guard let current = settings else { return }
        var edited = current
        change(&edited)
        guard edited != current else { return }
        let stamped = edited.stamped(previousMs: current.updatedMs, nowMs: currentTimeMs())
        pendingSettings = stamped
        send(.settings(stamped))
    }

    private func sendActivity() {
        send(.activity(foreground: foreground, page: currentPage))
    }

    private func send(_ message: WatchMessage) {
        guard WCSession.isSupported() else { return }
        let session = WCSession.default
        guard session.activationState == .activated, let json = message.json() else { return }
        let payload: [String: Any] = [watchToPhoneKey: json]
        if case .settings = message {
            if session.isReachable {
                session.sendMessage(payload, replyHandler: nil, errorHandler: { [weak self] _ in
                    DispatchQueue.main.async { self?.queueSettings(payload) }
                })
            } else {
                queueSettings(payload)
            }
        } else if session.isReachable {
            // hello/activity are only useful live; never queued.
            session.sendMessage(payload, replyHandler: nil, errorHandler: nil)
        }
    }

    /// Queues a settings document for delivery after a disconnect, replacing
    /// any older queued one.
    private func queueSettings(_ payload: [String: Any]) {
        let session = WCSession.default
        for transfer in session.outstandingUserInfoTransfers where transfer.userInfo[watchToPhoneKey] != nil {
            transfer.cancel()
        }
        session.transferUserInfo(payload)
    }

    // MARK: Phone -> watch

    private func receive(_ json: String, replayedContext: Bool = false) {
        guard let snap = Snapshot.parse(json) else { return }
        let now = currentTimeMs()
        if let current = snapshot {
            // The stored application context on launch may be the snapshot
            // we already have; do not make old data look fresh.
            if replayedContext && snap.seq == current.seq { return }
            // Context and message can arrive out of order.
            if snap.seq < current.seq, let at = receivedAtMs, now - at < 3_000 { return }
        }
        snapshot = snap
        receivedAtMs = now
        if let pending = pendingSettings, echoConfirms(pending: pending, echoed: snap.settings) {
            pendingSettings = nil
        }
        defaults.set(json, forKey: Keys.snapshot)
        defaults.set(Double(now), forKey: Keys.receivedAt)
    }

    // MARK: WCSessionDelegate (callbacks arrive on a background queue)

    func session(_ session: WCSession, activationDidCompleteWith activationState: WCSessionActivationState, error: Error?) {
        guard activationState == .activated else { return }
        let context = session.receivedApplicationContext[phoneToWatchKey] as? String
        DispatchQueue.main.async {
            if let context { self.receive(context, replayedContext: true) }
            self.send(.hello)
            if self.foreground { self.sendActivity() }
        }
    }

    func session(_ session: WCSession, didReceiveApplicationContext applicationContext: [String: Any]) {
        guard let json = applicationContext[phoneToWatchKey] as? String else { return }
        DispatchQueue.main.async { self.receive(json) }
    }

    func session(_ session: WCSession, didReceiveMessage message: [String: Any]) {
        guard let json = message[phoneToWatchKey] as? String else { return }
        DispatchQueue.main.async { self.receive(json) }
    }

    func sessionReachabilityDidChange(_ session: WCSession) {
        guard session.isReachable else { return }
        DispatchQueue.main.async {
            self.send(.hello)
            if self.foreground { self.sendActivity() }
        }
    }
}
