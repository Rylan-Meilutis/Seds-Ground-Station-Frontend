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
                    }
                    Section("Cycle") {
                        NavigationLink("Items (\(s.cycle.items.count))") { CycleItemsEditor() }
                        Stepper(value: binding(\.cycle.intervalS, set: { $0.setIntervalS($1) }), in: minIntervalS...maxIntervalS) {
                            Text("Every \(s.cycle.intervalS) s")
                        }
                        Toggle("Autostart", isOn: binding(\.cycle.autostart, set: { $0.cycle.autostart = $1 }))
                    }
                    Section("Compass") {
                        Picker("North", selection: binding(\.compass.north, set: { $0.compass.north = $1 })) {
                            Text("True").tag(North.trueNorth)
                            Text("Magnetic").tag(North.magnetic)
                        }
                        Stepper(value: binding(\.compass.staleAfterS, set: { $0.setStaleAfterS($1) }), in: minStaleS...maxStaleS, step: 5) {
                            Text("Stale after \(s.compass.staleAfterS) s")
                        }
                        NavigationLink("Targets (\(s.compass.targets.count))") { TargetsEditor() }
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
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Settings")
        }
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
