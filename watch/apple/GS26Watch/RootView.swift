import SwiftUI
import WatchCore

struct RootView: View {
    static let settingsTag = "__settings"

    @EnvironmentObject private var link: PhoneLink
    @EnvironmentObject private var location: LocationService
    @Environment(\.scenePhase) private var scenePhase
    @State private var selection = ""

    var body: some View {
        Group {
            if link.snapshot == nil {
                FirstRunView()
            } else {
                TabView(selection: $selection) {
                    ForEach(link.pages) { page in
                        PageContainer(page: page).tag(page.id)
                    }
                    SettingsView().tag(Self.settingsTag)
                }
                .tabViewStyle(.page)
            }
        }
        .onChange(of: link.pages.map(\.id), initial: true) {
            let ids = link.pages.map(\.id)
            if selection != Self.settingsTag && !ids.contains(selection) {
                selection = ids.first ?? Self.settingsTag
            }
        }
        .onChange(of: scenePhase, initial: true) {
            link.setForeground(scenePhase == .active)
            updateLocation()
        }
        .onChange(of: selection) {
            link.setPage(selection == Self.settingsTag ? "settings" : selection)
            updateLocation()
        }
        .onChange(of: link.settings?.power) { updateLocation() }
    }

    /// Location and heading run only while the compass page is on screen.
    private func updateLocation() {
        let page = link.pages.first { $0.id == selection }
        if scenePhase == .active, page?.pageKind == .compass {
            location.start(recovery: link.settings?.power == .recovery)
        } else {
            location.stop()
        }
    }
}

/// One swipeable page with the status banners on top.
struct PageContainer: View {
    let page: Page

    var body: some View {
        VStack(spacing: 2) {
            StatusBanners()
            switch page.pageKind {
            case .items: ItemsPage(page: page)
            case .cycle: CyclePage()
            case .compass: CompassPage()
            case .unknown: UnknownPage(page: page)
            }
        }
    }
}

struct StatusBanners: View {
    @EnvironmentObject private var link: PhoneLink

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let lost = link.linkState(nowMs: currentTimeMs(context.date)).lostText
            VStack(spacing: 2) {
                if link.snapshot?.mode == .demo {
                    Banner(text: "DEMO", color: .orange)
                }
                if link.snapshot?.mode == .noStation {
                    Banner(text: "Phone not connected to a station", color: .gray)
                }
                if let lost {
                    Banner(text: lost, color: .red)
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
            .frame(maxWidth: .infinity)
            .padding(.vertical, 2)
            .background(color.opacity(0.85), in: RoundedRectangle(cornerRadius: 6))
    }
}

struct FirstRunView: View {
    var body: some View {
        VStack(spacing: 8) {
            Image(systemName: "iphone")
                .font(.largeTitle)
            Text("Open the GS app on your iPhone")
                .multilineTextAlignment(.center)
        }
        .padding()
    }
}

struct UnknownPage: View {
    let page: Page

    var body: some View {
        VStack(spacing: 6) {
            Text(page.title).font(.headline)
            Text("Update the watch app")
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
