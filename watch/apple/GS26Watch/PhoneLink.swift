import Combine
import Foundation
import WatchConnectivity
import WatchCore

/// WatchConnectivity link to the GS26 phone app plus the direct link to the
/// station. Receives snapshots and the direct-link provisioning from the
/// phone and sends it only `hello`, `activity` and `settings` messages. The
/// shown data comes from the phone while its data is fresh, otherwise from
/// the station itself (WatchCore `chooseRoute`).
final class PhoneLink: NSObject, ObservableObject, WCSessionDelegate {
    /// The phone's last snapshot (pages, settings, catalog, values).
    @Published private(set) var phoneSnapshot: Snapshot? = nil
    @Published private(set) var phoneReceivedAtMs: Int64? = nil
    /// Local settings edit not yet echoed back by the phone.
    @Published private(set) var pendingSettings: WatchSettings? = nil
    /// WCSession.isReachable: the iPhone is in range and its GS app can be
    /// messaged (iOS wakes it in the background if needed).
    @Published private(set) var phoneReachable = false

    private let directKey = "gs26_direct"
    private let mapKey = "gs26_map"
    private let tileKey = "gs26_tile"

    private enum Keys {
        static let snapshot = "gs26.snapshot.json"
        static let receivedAt = "gs26.snapshot.receivedAtMs"
    }

    private let defaults = UserDefaults.standard
    private var foreground = false
    private var currentPage: String?
    private var activityTimer: Timer?
    /// Direct link to the station (provisioned by the phone).
    let direct = DirectLink()
    private var directChanges: AnyCancellable?
    /// Map background tiles (from the phone, or the station in direct mode).
    let tiles = TileStore()
    private var reportTimer: Timer?
    private var tileMisses: [String: Int64] = [:]

    override init() {
        super.init()
        if let json = defaults.string(forKey: Keys.snapshot), let snap = Snapshot.parse(json) {
            phoneSnapshot = snap
            phoneReceivedAtMs = Int64(defaults.double(forKey: Keys.receivedAt))
        }
        directChanges = direct.objectWillChange.sink { [weak self] _ in self?.objectWillChange.send() }
        if WCSession.isSupported() {
            WCSession.default.delegate = self
            WCSession.default.activate()
        }
    }

    // MARK: Route

    private var power: Power { settings?.power ?? .balanced }

    private func phoneAge(_ nowMs: Int64) -> Int64? {
        phoneReceivedAtMs.map { max(0, nowMs - $0) }
    }

    private func directAge(_ nowMs: Int64) -> Int64? {
        direct.receivedAtMs.map { max(0, nowMs - $0) }
    }

    /// Phone while its data is fresh, else the station itself.
    func route(nowMs: Int64 = currentTimeMs()) -> LinkRoute {
        chooseRoute(phoneMode: phoneSnapshot?.mode, phoneAgeMs: phoneAge(nowMs),
                    directAgeMs: direct.snapshot == nil ? nil : directAge(nowMs), freshMs: phoneFreshMs(power))
    }

    // MARK: Derived state

    /// The data to show: the phone's snapshot, or the station's own status
    /// with the phone's pages and settings.
    var snapshot: Snapshot? {
        if route() == .direct, let status = direct.snapshot {
            return status.mergedDirect(base: phoneSnapshot, address: direct.provision?.address ?? "")
        }
        return phoneSnapshot
    }

    var receivedAtMs: Int64? {
        route() == .direct ? direct.receivedAtMs : phoneReceivedAtMs
    }

    /// Effective settings: our pending edit, else the phone's copy.
    var settings: WatchSettings? { pendingSettings ?? phoneSnapshot?.settings }

    var isSyncing: Bool { pendingSettings != nil }

    /// Pages in display order. A pending local edit is applied at once by
    /// resolving it against the phone's catalog.
    var pages: [Page] {
        guard let snap = phoneSnapshot else { return [] }
        if let pending = pendingSettings, let catalog = snap.catalog?.pages, !catalog.isEmpty {
            return resolvedPages(pending, catalog: catalog, published: snap.pages)
        }
        return snap.pages
    }

    func connectionStatus(nowMs: Int64) -> ConnectionStatus {
        let current = route(nowMs: nowMs)
        let info = DirectInfo(configured: direct.configured, polling: direct.polling, error: direct.lastError)
        return ConnectionStatus(snapshot: snapshot, ageMs: elapsedMs(nowMs: nowMs), phoneReachable: phoneReachable,
                                route: current, direct: info)
    }

    /// The shown data is recent enough to act on.
    func dataIsFresh(nowMs: Int64) -> Bool {
        guard let snap = snapshot, snap.mode == .live else { return false }
        return elapsedMs(nowMs: nowMs) <= Int64(snap.linkTimeoutS) * 1000
    }

    func elapsedMs(nowMs: Int64) -> Int64 {
        max(0, nowMs - (receivedAtMs ?? nowMs))
    }

    /// Station enable state for a page action: the station's own status
    /// when reading it directly, else what the phone relayed.
    func actionState(_ cmd: String) -> ActionState? {
        snapshot?.actions[cmd]
    }

    /// Polls the station only while in front and the phone's data is not
    /// fresh. Call again when the power setting (poll rate) changes.
    func updateDirectPolling() {
        direct.setActive(foreground, intervalMs: directPollMs(power)) { [weak self] in
            guard let self, let provision = self.direct.provision, provision.configured else { return nil }
            let now = currentTimeMs()
            guard shouldPollDirect(phoneMode: self.phoneSnapshot?.mode, phoneAgeMs: self.phoneAge(now),
                                   freshMs: phoneFreshMs(self.power), foreground: self.foreground,
                                   configured: true)
            else { return nil }
            let s = self.settings ?? WatchSettings()
            return statusURL(base: provision.base, units: s.units, actions: pageActionCommands(self.pages),
                             staleAfterS: s.compass.staleAfterS)
        }
    }

    func compassDisplays(reading: CompassReading, nowMs: Int64) -> [TargetDisplay] {
        // Targets come from whichever source is shown.
        guard let snap = snapshot, let at = receivedAtMs else { return [] }
        return Compass.displays(targets: snap.targets, settings: settings ?? WatchSettings(),
                                reading: reading, receivedAtMs: at, nowMs: nowMs)
    }

    // MARK: Map tiles

    /// Tells the phone which tiles the watch holds (debounced), so it sends
    /// only the missing ones.
    func scheduleTileReport(after seconds: TimeInterval) {
        reportTimer?.invalidate()
        reportTimer = Timer.scheduledTimer(withTimeInterval: seconds, repeats: false) { [weak self] _ in
            guard let self else { return }
            self.send(.tiles(set: self.tiles.manifest?.set, have: self.tiles.keys.sorted()))
        }
    }

    /// Direct mode only (the phone's data is not fresh and the link is
    /// provisioned): fetches up to six missing map tiles for the current
    /// view from the station. A tile the station did not serve is not asked
    /// for again for a minute.
    func fetchMissingTiles(_ keys: [String]) {
        guard settings?.map.enabled ?? true, let provision = direct.provision, provision.configured else { return }
        let now = currentTimeMs()
        guard shouldPollDirect(phoneMode: phoneSnapshot?.mode, phoneAgeMs: phoneAge(now), freshMs: phoneFreshMs(power),
                               foreground: foreground, configured: true)
        else { return }
        let wanted = keys.filter { key in
            !tiles.has(key) && (tileMisses[key].map { now - $0 > 60_000 } ?? true)
        }.prefix(6)
        for key in wanted {
            tileMisses[key] = now // retried after a minute unless it arrives
            direct.fetchTile(key) { [weak self] data in
                guard let self, let data else { return }
                self.tiles.put(data, key: key, settingsCap: self.settings?.map.capBytes)
                self.tileMisses[key] = nil
            }
        }
    }

    // MARK: Watch -> phone

    func setForeground(_ isForeground: Bool) {
        foreground = isForeground
        updateDirectPolling()
        if isForeground, WCSession.isSupported(), WCSession.default.activationState == .activated {
            phoneReachable = WCSession.default.isReachable
        }
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
        let queued: String?
        switch message {
        case .settings: queued = "settings"
        case .tiles: queued = "tiles"
        default: queued = nil
        }
        if let type = queued {
            if session.isReachable {
                session.sendMessage(payload, replyHandler: nil, errorHandler: { [weak self] _ in
                    DispatchQueue.main.async { self?.queue(payload, type: type) }
                })
            } else {
                queue(payload, type: type)
            }
        } else if session.isReachable {
            // hello/activity are only useful live; never queued.
            session.sendMessage(payload, replyHandler: nil, errorHandler: nil)
        }
    }

    /// Queues a settings document or tile report for delivery after a
    /// disconnect, replacing any older queued message of the same type.
    private func queue(_ payload: [String: Any], type: String) {
        let session = WCSession.default
        let marker = "\"type\":\"\(type)\""
        for transfer in session.outstandingUserInfoTransfers {
            if let queued = transfer.userInfo[watchToPhoneKey] as? String, queued.contains(marker) {
                transfer.cancel()
            }
        }
        session.transferUserInfo(payload)
    }

    // MARK: Phone -> watch

    private func receive(_ json: String, replayedContext: Bool = false) {
        guard let snap = Snapshot.parse(json) else { return }
        let now = currentTimeMs()
        if let current = phoneSnapshot {
            // The stored application context on launch may be the snapshot
            // we already have; do not make old data look fresh.
            if replayedContext && snap.seq == current.seq { return }
            // Context and message can arrive out of order.
            if snap.seq < current.seq, let at = phoneReceivedAtMs, now - at < 3_000 { return }
        }
        phoneSnapshot = snap
        phoneReceivedAtMs = now
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
        let reachable = session.isReachable
        DispatchQueue.main.async {
            self.phoneReachable = reachable
            if let context { self.receive(context, replayedContext: true) }
            self.send(.hello)
            if self.foreground { self.sendActivity() }
            self.scheduleTileReport(after: 0)
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

    /// Direct-link provisioning (station address and watch token), queued
    /// by the phone with transferUserInfo.
    func session(_ session: WCSession, didReceiveUserInfo userInfo: [String: Any] = [:]) {
        if let json = userInfo[mapKey] as? String {
            DispatchQueue.main.async {
                self.tiles.apply(manifestJSON: json)
                self.scheduleTileReport(after: 5)
            }
        }
        guard let json = userInfo[directKey] as? String else { return }
        DispatchQueue.main.async { self.direct.apply(provisionJSON: json) }
    }

    /// One map tile from the phone (`transferFile`). The file is deleted when
    /// this returns, so it is moved into the cache here.
    func session(_ session: WCSession, didReceive file: WCSessionFile) {
        guard let key = file.metadata?[tileKey] as? String else { return }
        let adopted = tiles.adopt(fileAt: file.fileURL, key: key)
        guard adopted else { return }
        DispatchQueue.main.async {
            self.tiles.commit(settingsCap: self.settings?.map.capBytes)
            self.scheduleTileReport(after: 5)
        }
    }

    func sessionReachabilityDidChange(_ session: WCSession) {
        let reachable = session.isReachable
        DispatchQueue.main.async { self.phoneReachable = reachable }
        guard reachable else { return }
        DispatchQueue.main.async {
            self.send(.hello)
            if self.foreground { self.sendActivity() }
            self.scheduleTileReport(after: 2)
        }
    }
}
