import SwiftUI
import WatchCore

/// Generic `items` page: label / value rows from the snapshot.
struct ItemsPage: View {
    @EnvironmentObject private var link: PhoneLink
    let page: Page

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let elapsed = link.elapsedMs(nowMs: currentTimeMs(context.date))
            ScrollView {
                VStack(alignment: .leading, spacing: 6) {
                    Text(page.title).font(.headline)
                    ForEach(page.items, id: \.self) { id in
                        ItemRow(label: link.snapshot?.label(forItem: id) ?? id,
                                item: link.snapshot?.items[id], elapsedMs: elapsed)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }
}

struct ItemRow: View {
    let label: String
    let item: ItemValue?
    let elapsedMs: Int64

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(label).font(.caption2).foregroundStyle(.secondary)
            HStack(alignment: .firstTextBaseline, spacing: 3) {
                Text(item?.displayText(elapsedMs: elapsedMs) ?? "--")
                    .font(.title3.monospacedDigit())
                    .foregroundStyle((item?.level ?? .unknown).color)
                    .lineLimit(1)
                    .minimumScaleFactor(0.5)
                if let unit = item?.unit, !unit.isEmpty {
                    Text(unit).font(.caption2).foregroundStyle(.secondary)
                }
                if item?.stale == true {
                    Text("stale").font(.caption2).foregroundStyle(.orange)
                }
            }
        }
    }
}

/// The cycle items as a vertical pager: one big value per page, advancing
/// every `interval_s`. The Digital Crown (primary) or a vertical swipe
/// moves through the items and restarts the wait, so the item being read
/// stays for a full interval; tap pauses/resumes. Logic: WatchCore.AutoCycle.
struct CyclePage: View {
    /// This page is the one on screen (crown focus, timer).
    let visible: Bool

    @EnvironmentObject private var link: PhoneLink
    @State private var cycle: AutoCycle?
    @State private var selection = 0
    /// Digital Crown position in items (detents of 1).
    @State private var crown = 0.0
    @FocusState private var crownFocused: Bool

    var body: some View {
        let settings = link.settings?.cycle ?? CycleSetting()
        let ids = settings.items
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let now = currentTimeMs(context.date)
            ZStack(alignment: .bottom) {
                if ids.isEmpty {
                    Text("No cycle items. Add some in Settings.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.center)
                } else {
                    TabView(selection: $selection) {
                        ForEach(Array(ids.enumerated()), id: \.offset) { offset, id in
                            CycleItemView(id: id, nowMs: now).tag(offset)
                        }
                    }
                    .tabViewStyle(.verticalPage)
                    Text(cycle?.footer(nowMs: now, intervalS: settings.intervalS, count: ids.count) ?? "")
                        .font(.caption2.monospacedDigit())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .minimumScaleFactor(0.7)
                        .allowsHitTesting(false)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture { cycle?.togglePause(nowMs: currentTimeMs()) }
            // The crown drives the pager explicitly so it works even inside
            // the horizontal page view; focus follows page visibility.
            .focusable(!ids.isEmpty)
            .focused($crownFocused)
            .digitalCrownRotation($crown, from: 0, through: Double(max(ids.count - 1, 1)), by: 1,
                                  sensitivity: .low, isContinuous: false, isHapticFeedbackEnabled: true)
            .onChange(of: context.date) { advance(count: ids.count, intervalS: settings.intervalS) }
        }
        .onAppear {
            if cycle == nil { cycle = AutoCycle(nowMs: currentTimeMs(), autostart: settings.autostart) }
            crownFocused = visible
        }
        .onChange(of: visible) { crownFocused = visible }
        .onChange(of: crown) {
            let count = link.settings?.cycle.items.count ?? 0
            let index = min(max(Int(crown.rounded()), 0), max(count - 1, 0))
            if index != selection { withAnimation { selection = index } }
        }
        .onChange(of: selection) {
            // Crown or swipe (not our own advance): restart the wait.
            cycle?.observe(selection: selection, nowMs: currentTimeMs())
            if Int(crown.rounded()) != selection { crown = Double(selection) }
        }
    }

    private func advance(count: Int, intervalS: Int) {
        guard visible, var c = cycle else { return }
        if c.tick(nowMs: currentTimeMs(), intervalS: intervalS, count: count) || c.index != selection {
            cycle = c
            withAnimation { selection = c.index }
        } else {
            cycle = c
        }
    }
}

struct CycleItemView: View {
    let id: String
    let nowMs: Int64

    @EnvironmentObject private var link: PhoneLink

    var body: some View {
        let item = link.snapshot?.items[id]
        VStack(spacing: 2) {
            Text(link.snapshot?.label(forItem: id) ?? id)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            Text(item?.displayText(elapsedMs: link.elapsedMs(nowMs: nowMs)) ?? "--")
                .font(.system(.title, design: .rounded).monospacedDigit().weight(.semibold))
                .foregroundStyle((item?.level ?? .unknown).color)
                .lineLimit(1)
                .minimumScaleFactor(0.4)
            if let unit = item?.unit, !unit.isEmpty {
                Text(unit).font(.footnote).foregroundStyle(.secondary)
            }
            if item?.stale == true {
                Text("stale").font(.caption2).foregroundStyle(.orange)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
