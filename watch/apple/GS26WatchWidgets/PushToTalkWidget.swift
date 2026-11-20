import SwiftUI
import WidgetKit

/// Complication / Smart Stack widget that opens the watch app straight on
/// push-to-talk (`gs26://voice`, handled by RootView.onOpenURL, which also
/// joins voice). Static: it shows no live state, so it never needs reloads.
struct PushToTalkEntry: TimelineEntry {
    let date: Date
}

struct PushToTalkProvider: TimelineProvider {
    func placeholder(in context: Context) -> PushToTalkEntry { PushToTalkEntry(date: .now) }

    func getSnapshot(in context: Context, completion: @escaping (PushToTalkEntry) -> Void) {
        completion(PushToTalkEntry(date: .now))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<PushToTalkEntry>) -> Void) {
        completion(Timeline(entries: [PushToTalkEntry(date: .now)], policy: .never))
    }
}

struct PushToTalkView: View {
    @Environment(\.widgetFamily) private var family
    let entry: PushToTalkEntry

    var body: some View {
        switch family {
        case .accessoryRectangular:
            HStack {
                Image(systemName: "mic.circle.fill").font(.title2)
                VStack(alignment: .leading) {
                    Text("GS crew voice").font(.headline)
                    Text("Push to talk").font(.caption)
                }
            }
        case .accessoryInline:
            Label("Push to talk", systemImage: "mic.fill")
        case .accessoryCorner:
            Image(systemName: "mic.fill")
                .widgetLabel("PTT")
        default:
            ZStack {
                AccessoryWidgetBackground()
                Image(systemName: "mic.fill").font(.title3)
            }
        }
    }
}

@main
struct PushToTalkWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "com.UBSEDS.GS26.watch.ptt", provider: PushToTalkProvider()) { entry in
            PushToTalkView(entry: entry)
                .containerBackground(.clear, for: .widget)
                .widgetURL(URL(string: "gs26://voice"))
        }
        .configurationDisplayName("Push to talk")
        .description("Open GS crew voice and talk.")
        .supportedFamilies([.accessoryCircular, .accessoryRectangular, .accessoryInline, .accessoryCorner])
    }
}
