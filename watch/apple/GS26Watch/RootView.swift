import SwiftUI
import WatchCore

/// Top level: horizontal pages. Status first (the landing page), Alerts,
/// the pages chosen in the watch settings (the cycle page scrolls
/// vertically), Settings last. Settings is never selected automatically.
struct RootView: View {
    static let statusTag = "__status"
    static let alertsTag = "__alerts"
    static let settingsTag = "__settings"

    @EnvironmentObject private var link: PhoneLink
    @EnvironmentObject private var location: LocationService
    @EnvironmentObject private var voice: VoiceSession
    @EnvironmentObject private var search: SearchModel
    @Environment(\.scenePhase) private var scenePhase
    static let searchTag = "__search"
    @State private var selection = RootView.statusTag
    /// Push-to-talk opened from the complication while no voice page is enabled.
    @State private var voiceSheet = false

    var body: some View {
        TabView(selection: $selection) {
            StatusPage().tag(Self.statusTag)
            AlertsPage().tag(Self.alertsTag)
            ForEach(link.pages) { page in
                PageContainer(page: page, visible: selection == page.id).tag(page.id)
            }
            if link.phoneSnapshot?.search?.active == true || !search.recorder.pending.isEmpty {
                SearchPage().tag(Self.searchTag)
            }
            SettingsView().tag(Self.settingsTag)
        }
        .tabViewStyle(.page)
        // Complication / Smart Stack widget: gs26://voice opens push-to-talk and joins.
        .onOpenURL { url in
            guard url.scheme == "gs26", url.host == "voice" else { return }
            if let page = link.pages.first(where: { $0.pageKind == .voice }) {
                selection = page.id
            } else {
                voiceSheet = true
            }
            voice.join()
        }
        .sheet(isPresented: $voiceSheet) { VoicePage() }
        .onChange(of: link.pages.map(\.id)) {
            // A page that disappeared (disabled in settings) falls back to
            // Status, never to Settings.
            let fixed = [Self.statusTag, Self.alertsTag, Self.searchTag, Self.settingsTag]
            if !fixed.contains(selection) && !link.pages.contains(where: { $0.id == selection }) {
                selection = Self.statusTag
            }
        }
        .onChange(of: scenePhase, initial: true) {
            link.setForeground(scenePhase == .active)
            updateLocation()
        }
        // Recovery search: every fix, also with the wrist down while sharing.
        // Background updates end as soon as this watch no longer records
        // (the user stopped, or the phone reports the search ended).
        .onAppear {
            location.onFix = { [weak search, weak link, weak location] fix in
                guard let search, let link, let location else { return }
                guard search.recording(link) else {
                    if location.background { location.stop() }
                    return
                }
                search.record(fix, link: link)
            }
        }
        .onChange(of: selection, initial: true) {
            link.setPage(pageName(selection))
            updateLocation()
        }
        .onChange(of: link.settings?.power) {
            updateLocation()
            link.updateDirectPolling()
        }
        // Recovery search: record and relay while opted in (SearchPage).
        .onReceive(Timer.publish(every: 2, on: .main, in: .common).autoconnect()) { _ in
            search.tick(link: link, location: location.location)
            updateLocation()
        }
    }

    private func pageName(_ tag: String) -> String {
        switch tag {
        case Self.statusTag: return "status"
        case Self.alertsTag: return "alerts"
        case Self.settingsTag: return "settings"
        case Self.searchTag: return "search"
        default: return tag
        }
    }

    /// Location and heading run only while the compass page is on screen,
    /// or while this watch shares its recovery search track. Sharing keeps
    /// location running with the wrist down (background location updates,
    /// started here while the app is in front); nothing else does.
    private func updateLocation() {
        let page = link.pages.first { $0.id == selection }
        let recovery = link.settings?.power == .recovery
        if search.recording(link) {
            if scenePhase == .active || location.background {
                location.start(recovery: recovery, background: true)
            }
        } else if scenePhase == .active, page?.pageKind == .compass {
            location.start(recovery: recovery)
        } else {
            location.stop()
        }
    }
}

/// Landing page: flight state, T-clock, the data path ("Via phone" or
/// "Direct to GS"), the station connection and direct-link trouble.
struct StatusPage: View {
    @EnvironmentObject private var link: PhoneLink

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let now = currentTimeMs(context.date)
            let status = link.connectionStatus(nowMs: now)
            let elapsed = link.elapsedMs(nowMs: now)
            ScrollView {
                VStack(alignment: .leading, spacing: 6) {
                    if link.snapshot?.mode == .demo {
                        Banner(text: "DEMO", color: .orange)
                    }
                    if let snap = link.snapshot {
                        ForEach(["flight_state", "t_clock"], id: \.self) { id in
                            if let item = snap.items[id] {
                                ItemRow(label: snap.label(forItem: id), item: item, elapsedMs: elapsed)
                            }
                        }
                    }
                    StatusLineView(line: status.phone, systemImage: link.route(nowMs: now) == .direct ? "wifi" : "iphone")
                    if let station = status.station {
                        StatusLineView(line: station, systemImage: "antenna.radiowaves.left.and.right")
                    }
                    if let direct = status.direct {
                        StatusLineView(line: direct, systemImage: "wifi.exclamationmark")
                    }
                    if status.openPhoneApp && status.phone.text != ConnectionStatus.openPhoneAppText {
                        Text(ConnectionStatus.openPhoneAppText)
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

struct StatusLineView: View {
    let line: StatusLine
    let systemImage: String

    var body: some View {
        Label {
            Text(line.text)
                .font(.footnote)
                .lineLimit(3)
                .minimumScaleFactor(0.8)
                .fixedSize(horizontal: false, vertical: true)
        } icon: {
            Image(systemName: systemImage).foregroundStyle(line.tone.color)
        }
    }
}

/// Items the phone flags at warn or alert level, plus link problems.
struct AlertsPage: View {
    @EnvironmentObject private var link: PhoneLink

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let status = link.connectionStatus(nowMs: currentTimeMs(context.date))
            let rows = alertRows(link.snapshot)
            ScrollView {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Alerts").font(.headline)
                    if status.phone.tone == .alert || status.phone.tone == .warn {
                        StatusLineView(line: status.phone, systemImage: "iphone")
                    }
                    if let station = status.station, station.tone == .alert || station.tone == .warn {
                        StatusLineView(line: station, systemImage: "antenna.radiowaves.left.and.right")
                    }
                    if let direct = status.direct, direct.tone == .alert {
                        StatusLineView(line: direct, systemImage: "wifi.exclamationmark")
                    }
                    ForEach(rows) { row in
                        VStack(alignment: .leading, spacing: 0) {
                            Text(row.label).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                            Text(row.text)
                                .font(.body.monospacedDigit().weight(.semibold))
                                .foregroundStyle(row.level.color)
                                .lineLimit(1)
                                .minimumScaleFactor(0.6)
                        }
                    }
                    if rows.isEmpty {
                        Text(link.snapshot == nil ? "No data from the phone yet" : "No alerts")
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

/// One configured page with the status banners on top.
struct PageContainer: View {
    let page: Page
    let visible: Bool

    var body: some View {
        VStack(spacing: 2) {
            StatusBanners()
            switch page.pageKind {
            case .items: ItemsPage(page: page)
            case .cycle: CyclePage(visible: visible)
            case .compass: CompassPage(visible: visible)
            case .voice: VoicePage()
            case .unknown: UnknownPage(page: page)
            }
            // Opt-in page actions stay pinned below the content.
            if !page.actions.isEmpty {
                ActionsRow(actions: page.actions)
            }
        }
    }
}

struct StatusBanners: View {
    @EnvironmentObject private var link: PhoneLink

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let status = link.connectionStatus(nowMs: currentTimeMs(context.date))
            VStack(spacing: 2) {
                if link.snapshot?.mode == .demo {
                    Banner(text: "DEMO", color: .orange)
                } else if let station = status.station, station.tone != .ok {
                    Banner(text: station.text, color: station.tone.color)
                }
                if status.phone.tone == .alert || status.phone.tone == .warn {
                    Banner(text: status.phone.text, color: status.phone.tone.color)
                }
                if let direct = status.direct, direct.tone == .alert {
                    Banner(text: direct.text, color: direct.tone.color)
                }
            }
        }
    }
}

struct Banner: View {
    let text: String
    let color: Color

    var body: some View {
        Text(text)
            .font(.caption2.weight(.semibold))
            .multilineTextAlignment(.center)
            .lineLimit(2)
            .minimumScaleFactor(0.8)
            .frame(maxWidth: .infinity)
            .padding(.vertical, 2)
            .background(color.opacity(0.85), in: RoundedRectangle(cornerRadius: 6))
    }
}

struct UnknownPage: View {
    let page: Page

    var body: some View {
        VStack(spacing: 6) {
            Text(page.title).font(.headline)
            Text("Update the watch app")
                .font(.footnote)
                .multilineTextAlignment(.center)
                .foregroundStyle(.secondary)
        }
        .frame(maxHeight: .infinity)
    }
}

extension Level {
    var color: Color {
        switch self {
        case .normal: return .primary
        case .warn: return .yellow
        case .alert: return .red
        case .unknown: return .secondary
        }
    }
}

extension StatusTone {
    var color: Color {
        switch self {
        case .ok: return .green
        case .waiting: return .gray
        case .warn: return .orange
        case .alert: return .red
        }
    }
}
