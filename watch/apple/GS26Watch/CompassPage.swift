import SwiftUI
import WatchCore

/// Recovery compass: arrow to the selected target, distance, bearing and
/// fix age. Tap the target name to switch target.
struct CompassPage: View {
    @EnvironmentObject private var link: PhoneLink
    @EnvironmentObject private var location: LocationService
    @State private var selectedId: String?

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let targets = link.compassDisplays(reading: location.reading, nowMs: currentTimeMs(context.date))
            if let target = targets.first(where: { $0.id == selectedId }) ?? targets.first {
                content(target, count: targets.count)
                    .contentShape(Rectangle())
                    .onTapGesture { selectNext(after: target.id, in: targets) }
            } else {
                Text("No target positions yet")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
                    .frame(maxHeight: .infinity)
            }
        }
    }

    private func selectNext(after id: String, in targets: [TargetDisplay]) {
        guard let i = targets.firstIndex(where: { $0.id == id }), targets.count > 1 else { return }
        selectedId = targets[(i + 1) % targets.count].id
    }

    @ViewBuilder
    private func content(_ t: TargetDisplay, count: Int) -> some View {
        VStack(spacing: 2) {
            HStack(spacing: 4) {
                Text(t.label).font(.headline).lineLimit(1)
                if count > 1 { Image(systemName: "chevron.right.circle").font(.caption2) }
                if t.stale {
                    Text("STALE")
                        .font(.caption2.weight(.bold))
                        .padding(.horizontal, 3)
                        .background(Color.orange, in: RoundedRectangle(cornerRadius: 3))
                }
            }
            if let saved = t.savedPositionLabel {
                Text(saved).font(.caption2).foregroundStyle(.secondary)
            }
            center(t).frame(maxHeight: .infinity)
            Text(t.distanceText).font(.title3.monospacedDigit().weight(.semibold))
            HStack(spacing: 6) {
                if let bearing = t.bearingText {
                    Text("\(bearing) \(t.north == .magnetic ? "M" : "T")")
                }
                Text("fix \(t.ageText)")
            }
            .font(.caption2.monospacedDigit())
            .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func center(_ t: TargetDisplay) -> some View {
        if location.denied {
            HintText("Allow location for GS26 in Settings › Privacy & Security")
        } else if location.location == nil {
            HintText("Waiting for watch GPS")
        } else if let near = t.nearText {
            HintText(near)
        } else if !location.headingAvailable {
            HintText("No compass on this watch")
        } else {
            VStack(spacing: 2) {
                if let rotation = t.arrowRotation {
                    ArrowView(rotation: rotation).opacity(location.needsCalibration ? 0.4 : 1)
                }
                if location.needsCalibration {
                    HintText("Move your wrist in a figure-8")
                }
            }
        }
    }
}

private struct HintText: View {
    let text: String
    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text)
            .font(.footnote)
            .multilineTextAlignment(.center)
            .foregroundStyle(.secondary)
    }
}

/// Arrow that turns the short way round instead of spinning through 360.
struct ArrowView: View {
    let rotation: Double
    @State private var shown: Double = 0

    var body: some View {
        Image(systemName: "location.north.fill")
            .resizable()
            .scaledToFit()
            .frame(width: 54, height: 54)
            .foregroundStyle(.tint)
            .rotationEffect(.degrees(shown))
            .animation(.easeOut(duration: 0.25), value: shown)
            .onChange(of: rotation, initial: true) {
                shown += signedDeltaDeg(from: normalizeDeg(shown), to: rotation)
            }
    }
}
