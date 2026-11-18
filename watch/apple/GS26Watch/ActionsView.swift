import SwiftUI
import WatchCore

/// The page's opt-in action buttons (at most two). A button shows the
/// station's enable state and reason; every press opens a confirmation
/// (Abort: tap Confirm; others: press and hold) and then shows whether the
/// station accepted, refused (with its reason) or the outcome is unknown.
/// Logic: WatchCore `actionButton` / `actionOutcome`.
struct ActionsRow: View {
    let actions: [PageAction]

    @EnvironmentObject private var link: PhoneLink
    @State private var confirming: PageAction?

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let now = currentTimeMs(context.date)
            let fresh = link.dataIsFresh(nowMs: now)
            HStack(spacing: 4) {
                ForEach(actions) { action in
                    let state = actionButton(abort: action.abort, state: link.actionState(action.cmd),
                                             fresh: fresh, token: link.direct.configured)
                    Button {
                        confirming = action
                    } label: {
                        VStack(spacing: 0) {
                            Text(action.label)
                                .font(.caption.weight(.semibold))
                                .lineLimit(1)
                                .minimumScaleFactor(0.6)
                            if let note = state.note {
                                Text(note)
                                    .font(.system(size: 9))
                                    .lineLimit(2)
                                    .minimumScaleFactor(0.7)
                                    .foregroundStyle(.secondary)
                            }
                        }
                        .frame(maxWidth: .infinity)
                    }
                    .tint(action.abort ? Color.red : Color.accentColor)
                    .disabled(!state.enabled)
                    .accessibilityHint(state.note ?? "")
                }
            }
        }
        .sheet(item: $confirming) { action in
            ActionConfirmView(action: action)
        }
    }
}

/// Confirmation and result for one press.
struct ActionConfirmView: View {
    let action: PageAction

    @EnvironmentObject private var link: PhoneLink
    @Environment(\.dismiss) private var dismiss
    @State private var sending = false
    @State private var result: ActionResult?
    @State private var holding = false

    private static let holdSeconds = 1.5

    var body: some View {
        ScrollView {
            VStack(spacing: 8) {
                Text(action.label).font(.headline).multilineTextAlignment(.center)
                if let result {
                    ResultView(result: result)
                    Button("Done") { dismiss() }
                } else if sending {
                    ProgressView()
                    Text("Sending to the station…").font(.footnote).foregroundStyle(.secondary)
                } else {
                    if let reason = link.actionState(action.cmd)?.reason {
                        Text(reason).font(.footnote).foregroundStyle(.secondary).multilineTextAlignment(.center)
                    }
                    Text(link.direct.provision.map { "Station: \($0.address)" } ?? "")
                        .font(.caption2).foregroundStyle(.secondary)
                    if action.abort {
                        Button(role: .destructive) { send() } label: {
                            Text("Confirm ABORT").font(.headline).frame(maxWidth: .infinity)
                        }
                        .tint(.red)
                    } else {
                        // Re-checked while the sheet is open: the station may disable it.
                        let state = actionButton(abort: false, state: link.actionState(action.cmd),
                                                 fresh: link.dataIsFresh(nowMs: currentTimeMs()),
                                                 token: link.direct.configured)
                        HoldToConfirm(seconds: Self.holdSeconds, holding: $holding) { send() }
                            .disabled(!state.enabled)
                            .opacity(state.enabled ? 1 : 0.4)
                    }
                    Button("Cancel", role: .cancel) { dismiss() }
                }
            }
        }
    }

    private func send() {
        guard !sending, result == nil else { return }
        sending = true
        link.direct.send(action.cmd) { outcome in
            sending = false
            result = outcome
        }
    }
}

/// Press and hold until the ring fills; releasing early cancels.
struct HoldToConfirm: View {
    let seconds: Double
    @Binding var holding: Bool
    let action: () -> Void

    var body: some View {
        ZStack {
            Circle().stroke(Color.secondary.opacity(0.4), lineWidth: 6)
            Circle()
                .trim(from: 0, to: holding ? 1 : 0)
                .stroke(Color.orange, style: StrokeStyle(lineWidth: 6, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .animation(holding ? .linear(duration: seconds) : .easeOut(duration: 0.2), value: holding)
            Text(holding ? "Keep holding" : "Hold to send")
                .font(.caption2)
                .multilineTextAlignment(.center)
        }
        .frame(width: 84, height: 84)
        .contentShape(Circle())
        .onLongPressGesture(minimumDuration: seconds, maximumDistance: 20) {
            holding = false
            action()
        } onPressingChanged: { pressing in
            holding = pressing
        }
        .accessibilityLabel("Press and hold to send")
    }
}

struct ResultView: View {
    let result: ActionResult

    var body: some View {
        Label {
            Text(result.text).font(.footnote).fixedSize(horizontal: false, vertical: true)
        } icon: {
            Image(systemName: icon).foregroundStyle(color)
        }
    }

    private var icon: String {
        switch result.outcome {
        case .accepted: return "checkmark.circle.fill"
        case .refused: return "xmark.octagon.fill"
        case .unknown: return "questionmark.circle.fill"
        }
    }

    private var color: Color {
        switch result.outcome {
        case .accepted: return .green
        case .refused: return .red
        case .unknown: return .orange
        }
    }
}
