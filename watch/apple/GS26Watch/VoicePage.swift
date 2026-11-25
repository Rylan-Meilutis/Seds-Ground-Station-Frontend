import SwiftUI
import WatchCore

/// Crew voice page: channel and connection, a big push-to-talk button, who
/// is talking, Join/Leave. The Digital Crown sets the playback volume.
struct VoicePage: View {
    @EnvironmentObject private var link: PhoneLink
    @EnvironmentObject private var voice: VoiceSession
    @State private var crown = Double(defaultVoiceVolume)
    @State private var pressing = false

    private var mode: PttMode { link.settings?.voice.ptt ?? .hold }

    var body: some View {
        VStack(spacing: 4) {
            HStack {
                Text(voice.channelName).font(.caption.weight(.semibold))
                Spacer()
                Text("\(voice.volumePct)%").font(.caption2.monospacedDigit()).foregroundStyle(.secondary)
            }
            Text(statusText)
                .font(.caption2)
                .foregroundStyle(statusColor)
                .lineLimit(2)
                .minimumScaleFactor(0.8)
                .frame(maxWidth: .infinity, alignment: .leading)
            if voice.joined {
                pttButton
                Text(talkingText)
                    .font(.caption2)
                    .lineLimit(1)
                    .minimumScaleFactor(0.7)
                Button("Leave", role: .destructive) { voice.leave() }
                    .font(.caption2)
                    .buttonStyle(.borderless)
            } else {
                Spacer(minLength: 0)
                Button {
                    voice.join()
                } label: {
                    Label("Join voice", systemImage: "mic.fill").frame(maxWidth: .infinity)
                }
                .tint(.green)
                .disabled(!(link.settings?.voice.enabled ?? true))
                if !(link.settings?.voice.enabled ?? true) {
                    Text("Voice is off in the phone's watch settings").font(.caption2).foregroundStyle(.secondary)
                }
                Spacer(minLength: 0)
            }
        }
        .focusable()
        .digitalCrownRotation($crown, from: 0, through: Double(maxVoiceVolume), by: 5,
                              sensitivity: .low, isContinuous: false, isHapticFeedbackEnabled: true)
        .onChange(of: crown) { voice.crownVolume(Int(crown.rounded())) }
        .onAppear { crown = Double(voice.volumePct) }
    }

    private var pttButton: some View {
        let label = voice.talking ? (voice.waiting ? "Waiting" : "Talking") : (mode == .hold ? "Hold to talk" : "Tap to talk")
        return Circle()
            .fill(voice.talking ? Color.red : (canTalk ? Color.green : Color.gray))
            .overlay(
                VStack(spacing: 2) {
                    Image(systemName: voice.talking ? "mic.fill" : "mic")
                        .font(.title2)
                    Text(label).font(.caption2.weight(.semibold))
                }
                .foregroundStyle(.white)
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityLabel(label)
            .accessibilityAddTraits(.isButton)
            // Press-and-hold: down on touch, up on release; toggle: each
            // touch-down flips (the state machine ignores `up`).
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { _ in
                        guard !pressing else { return }
                        pressing = true
                        voice.pressDown()
                    }
                    .onEnded { _ in
                        pressing = false
                        voice.pressUp()
                    }
            )
    }

    private var canTalk: Bool { voice.phase == .connected && !voice.held }

    private var statusText: String {
        let path = voice.transport == .phone ? "Via phone" : "Direct to GS"
        switch voice.phase {
        case .idle: return voice.granted ? "Not joined" : "Voice not set up for this watch"
        case .permission: return "Allow the microphone…"
        case .connecting: return "Connecting… (\(path.lowercased()))"
        case .connected:
            if voice.held { return "Another watch has voice" }
            return "\(path) · \(voice.peerCount) in channel" + (voice.radioAvailable ? " · radio" : "")
        case let .reconnecting(ms): return "Reconnecting in \(max(1, ms / 1000)) s"
        case let .failed(message): return message
        }
    }

    private var statusColor: Color {
        switch voice.phase {
        case .connected: return voice.held ? .orange : .green
        case .failed: return .red
        case .reconnecting: return .orange
        default: return .secondary
        }
    }

    private var talkingText: String {
        voice.talkers.isEmpty ? (voice.talking ? "You are talking" : "Nobody talking")
            : voice.talkers.joined(separator: ", ")
    }
}
