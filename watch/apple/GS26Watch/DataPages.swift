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

/// One big value at a time, rotating every `interval_s`. Tap to pause.
struct CyclePage: View {
    @EnvironmentObject private var link: PhoneLink
    @State private var scheduler = CycleScheduler()

    var body: some View {
        let cycle = link.settings?.cycle ?? CycleSetting()
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let now = currentTimeMs(context.date)
            let ids = cycle.items
            let index = scheduler.index(at: now, intervalS: cycle.intervalS, count: ids.count)
            let id = ids.isEmpty ? nil : ids[min(index, ids.count - 1)]
            let item = id.flatMap { link.snapshot?.items[$0] }
            VStack(spacing: 4) {
                Text(id.map { link.snapshot?.label(forItem: $0) ?? $0 } ?? "Cycle")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Text(item?.displayText(elapsedMs: link.elapsedMs(nowMs: now)) ?? "--")
                    .font(.system(size: 44, weight: .semibold, design: .rounded).monospacedDigit())
                    .foregroundStyle((item?.level ?? .unknown).color)
                    .lineLimit(1)
                    .minimumScaleFactor(0.3)
                if let unit = item?.unit, !unit.isEmpty {
                    Text(unit).font(.caption).foregroundStyle(.secondary)
                }
                Text(ids.isEmpty ? "" : "\(index + 1)/\(ids.count)\(scheduler.isRunning ? "" : " · paused")")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .contentShape(Rectangle())
            .onTapGesture { scheduler.toggle(at: currentTimeMs()) }
        }
        .onAppear {
            if cycle.autostart { scheduler.start(at: currentTimeMs()) }
        }
        .onDisappear { scheduler.pause(at: currentTimeMs()) }
    }
}
