import SwiftUI
import WatchCore

/// Last page: edits every WatchSettings field. Edits apply locally at once
/// and show "Syncing" until the phone echoes them back.
struct SettingsView: View {
    @EnvironmentObject private var link: PhoneLink

    var body: some View {
        NavigationStack {
            List {
                if let s = link.settings {
                    Section {
                        Label(link.isSyncing ? "Syncing…" : syncText, systemImage: link.isSyncing ? "arrow.triangle.2.circlepath" : "checkmark.circle")
                            .font(.footnote)
                    }
                    Section("Pages") {
                        NavigationLink("Order and visibility") { PagesEditor() }
                        NavigationLink("Rows and actions") { PageContentList() }
                    }
                    Section("Cycle") {
                        NavigationLink("Items (\(s.cycle.items.count))") { CycleItemsEditor() }
                        Stepper(value: binding(\.cycle.intervalS, set: { $0.setIntervalS($1) }), in: minIntervalS...maxIntervalS) {
                            StepperLabel(title: "Cycle every", value: "\(s.cycle.intervalS) s")
                        }
                        Toggle("Autostart", isOn: binding(\.cycle.autostart, set: { $0.cycle.autostart = $1 }))
                    }
                    Section("Compass") {
                        Picker("North", selection: binding(\.compass.north, set: { $0.compass.north = $1 })) {
                            Text("True").tag(North.trueNorth)
                            Text("Magnetic").tag(North.magnetic)
                        }
                        Stepper(value: binding(\.compass.staleAfterS, set: { $0.setStaleAfterS($1) }), in: minStaleS...maxStaleS, step: 5) {
                            StepperLabel(title: "Stale after", value: "\(s.compass.staleAfterS) s")
                        }
                        NavigationLink("Targets (\(s.compass.targets.count))") { TargetsEditor() }
                        Toggle("Map background", isOn: binding(\.map.enabled, set: { $0.map.enabled = $1 }))
                    }
                    Section("Direct link") {
                        Text(directText).font(.footnote)
                        Toggle("Use direct link", isOn: binding(\.directLink, set: { $0.directLink = $1 }))
                    }
                    Section("Display") {
                        Picker("Units", selection: binding(\.units, set: { $0.units = $1 })) {
                            Text("Metric").tag(Units.metric)
                            Text("Imperial").tag(Units.imperial)
                        }
                        Picker("Power", selection: binding(\.power, set: { $0.power = $1 })) {
                            Text("Saver").tag(Power.saver)
                            Text("Balanced").tag(Power.balanced)
                            Text("Recovery").tag(Power.recovery)
                        }
                    }
                } else {
                    Text("Settings arrive with the first update from the phone")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Settings")
        }
    }

    private var directText: String {
        guard let p = link.direct.provision, p.configured else {
            return "Not set up. Sign in on the phone app to let the watch reach the station by itself."
        }
        let actions = p.commands.isEmpty ? "read-only" : p.commands.joined(separator: ", ")
        return "\(p.address) · \(actions)"
    }

    private var syncText: String {
        switch link.snapshot?.account?.sync ?? "" {
        case "synced": return "Synced to account"
        case "pending": return "Saving to account"
        case "error": return "Account sync error"
        default: return "Saved on phone"
        }
    }

    private func binding<T>(_ path: KeyPath<WatchSettings, T>, set: @escaping (inout WatchSettings, T) -> Void) -> Binding<T> {
        settingsBinding(link, path, set: set)
    }
}

/// Stepper labels default to a large font on watchOS; keep them small so
/// the value fits between the -/+ buttons on a 40/41 mm watch.
struct StepperLabel: View {
    let title: String
    let value: String

    var body: some View {
        VStack(spacing: 0) {
            Text(title).font(.caption2).foregroundStyle(.secondary)
            Text(value).font(.footnote.monospacedDigit())
        }
        .lineLimit(1)
        .minimumScaleFactor(0.7)
    }
}

func settingsBinding<T>(
    _ link: PhoneLink, _ path: KeyPath<WatchSettings, T>, set: @escaping (inout WatchSettings, T) -> Void
) -> Binding<T> {
    Binding(
        get: { (link.settings ?? WatchSettings())[keyPath: path] },
        set: { value in link.edit { set(&$0, value) } }
    )
}

struct PagesEditor: View {
    @EnvironmentObject private var link: PhoneLink

    var body: some View {
        List {
            Section {
                ForEach(link.settings?.pages ?? []) { page in
                    Toggle(title(page.id), isOn: Binding(
                        get: { page.enabled },
                        set: { _ in link.edit { $0.togglePage(page.id) } }
                    ))
                    .swipeActions(edge: .leading) {
                        Button { link.edit { $0.movePage(page.id, up: true) } } label: { Image(systemName: "arrow.up") }
                    }
                    .swipeActions(edge: .trailing) {
                        Button { link.edit { $0.movePage(page.id, up: false) } } label: { Image(systemName: "arrow.down") }
                    }
                }
            } footer: {
                Text("Swipe a row to move it up or down.")
            }
        }
        .navigationTitle("Pages")
    }

    private func title(_ id: String) -> String {
        link.snapshot?.catalog?.pages.first { $0.id == id }?.title ?? id
    }
}

struct CycleItemsEditor: View {
    @EnvironmentObject private var link: PhoneLink

    var body: some View {
        let items = link.settings?.cycle.items ?? []
        let catalog = link.snapshot?.catalog?.items ?? []
        let known = catalog.isEmpty ? nil : Set(catalog.map(\.id))
        List {
            Section {
                ForEach(items, id: \.self) { id in
                    Button { link.edit { $0.toggleCycleItem(id, known: known) } } label: {
                        Label(label(id), systemImage: "minus.circle")
                    }
                    .disabled(items.count <= 1)
                    .swipeActions(edge: .leading) {
                        Button { link.edit { $0.moveCycleItem(id, up: true) } } label: { Image(systemName: "arrow.up") }
                    }
                    .swipeActions(edge: .trailing) {
                        Button { link.edit { $0.moveCycleItem(id, up: false) } } label: { Image(systemName: "arrow.down") }
                    }
                }
            } header: {
                Text("In cycle")
            } footer: {
                Text("Tap to remove, swipe to move.")
            }
            Section("Add") {
                ForEach(catalog.filter { !items.contains($0.id) }) { item in
                    Button { link.edit { $0.toggleCycleItem(item.id, known: known) } } label: {
                        Label(item.label, systemImage: "plus.circle")
                    }
                    .disabled(items.count >= maxCycleItems)
                }
            }
        }
        .navigationTitle("Cycle items")
    }

    private func label(_ id: String) -> String {
        link.snapshot?.label(forItem: id) ?? id
    }
}

struct TargetsEditor: View {
    @EnvironmentObject private var link: PhoneLink

    var body: some View {
        let selected = link.settings?.compass.targets ?? []
        let catalog = link.snapshot?.catalog?.targets ?? []
        let known = Set(catalog.map(\.id))
        List {
            ForEach(catalog) { target in
                Toggle(target.label, isOn: Binding(
                    get: { selected.contains(target.id) },
                    set: { _ in link.edit { $0.toggleTarget(target.id, known: known) } }
                ))
            }
        }
        .navigationTitle("Targets")
    }
}

/// Pages with rows (items pages) or actions; each opens its editor.
struct PageContentList: View {
    @EnvironmentObject private var link: PhoneLink

    var body: some View {
        let catalog = link.phoneSnapshot?.catalog?.pages ?? []
        List {
            ForEach(link.settings?.pages ?? []) { page in
                if let def = catalog.first(where: { $0.id == page.id }), def.pageKind == .items || !page.actions.isEmpty {
                    NavigationLink(def.title) { PageContentEditor(pageId: page.id) }
                }
            }
        }
        .navigationTitle("Page contents")
    }
}

/// Rows of one items page (tap to remove, swipe to move, add from the
/// catalog) and its action buttons. Actions are chosen on the phone, which
/// knows what the station lets this account run; here they can be removed.
struct PageContentEditor: View {
    let pageId: String

    @EnvironmentObject private var link: PhoneLink

    var body: some View {
        let def = link.phoneSnapshot?.catalog?.pages.first { $0.id == pageId }
        let defaults = def?.items ?? defaultPageItems[pageId] ?? []
        let setting = link.settings?.pages.first { $0.id == pageId }
        let rows = (setting?.items.isEmpty ?? true) ? defaults : setting?.items ?? []
        let catalog = link.phoneSnapshot?.catalog?.items ?? []
        let known = catalog.isEmpty ? nil : Set(catalog.map(\.id))
        List {
            if def?.pageKind == .items {
                Section {
                    ForEach(rows, id: \.self) { id in
                        Button { link.edit { $0.togglePageItem(pageId, id, defaults: defaults, known: known) } } label: {
                            Label(link.phoneSnapshot?.label(forItem: id) ?? id, systemImage: "minus.circle")
                        }
                        .disabled(rows.count <= 1)
                        .swipeActions(edge: .leading) {
                            Button { link.edit { $0.movePageItem(pageId, id, up: true, defaults: defaults) } } label: {
                                Image(systemName: "arrow.up")
                            }
                        }
                        .swipeActions(edge: .trailing) {
                            Button { link.edit { $0.movePageItem(pageId, id, up: false, defaults: defaults) } } label: {
                                Image(systemName: "arrow.down")
                            }
                        }
                    }
                } header: {
                    Text("Rows")
                } footer: {
                    Text("Tap to remove, swipe to move.")
                }
                Section("Add") {
                    ForEach(catalog.filter { !rows.contains($0.id) }) { item in
                        Button { link.edit { $0.togglePageItem(pageId, item.id, defaults: defaults, known: known) } } label: {
                            Label(item.label, systemImage: "plus.circle")
                        }
                        .disabled(rows.count >= maxPageItems)
                    }
                }
            }
            Section {
                ForEach(setting?.actions ?? [], id: \.self) { cmd in
                    Button { link.edit { $0.removePageAction(pageId, cmd) } } label: {
                        Label(cmd, systemImage: "minus.circle")
                    }
                }
            } header: {
                Text("Actions")
            } footer: {
                Text("Add actions in the phone app: Settings › Watch.")
            }
        }
        .navigationTitle(def?.title ?? pageId)
    }
}
