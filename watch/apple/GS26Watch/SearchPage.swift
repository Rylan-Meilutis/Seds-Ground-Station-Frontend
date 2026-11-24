import Combine
import CoreLocation
import SwiftUI
import WatchCore

/// Recovery search on the watch (docs/watch-companion.md "Recovery search").
/// The watch records its track only after its user opts in here, only while
/// the station's search runs and the station accepted the opt-in (relayed by
/// the phone). Fixes wait on the watch until the phone acknowledges them.
final class SearchModel: ObservableObject {
    @Published private(set) var recorder: SearchRecorder
    private let defaults = UserDefaults.standard
    private static let key = "gs26.search.recorder"

    init() {
        if let data = UserDefaults.standard.data(forKey: Self.key),
           let saved = try? JSONDecoder().decode(SearchRecorder.self, from: data) {
            recorder = saved
        } else {
            recorder = SearchRecorder()
        }
    }

    func recording(_ link: PhoneLink) -> Bool { recorder.recording(link.phoneSnapshot?.search) }

    /// Every 2 s: follow the phone, record a fix, send the buffer.
    func tick(link: PhoneLink, location: CLLocation?) {
        var next = recorder
        let info = link.phoneSnapshot?.search
        next.apply(info)
        if let fix = location, -fix.timestamp.timeIntervalSinceNow < 30, fix.horizontalAccuracy >= 0 {
            next.record(lat: fix.coordinate.latitude, lon: fix.coordinate.longitude,
                        accuracyM: fix.horizontalAccuracy, tMs: currentTimeMs(fix.timestamp), info: info)
        }
        if let message = next.nextMessage() {
            link.sendSearch(message)
        }
        save(next)
    }

    func share(_ on: Bool, link: PhoneLink) {
        var next = recorder
        if on {
            next.wantsToShare = true
        } else {
            next.stop()
        }
        save(next)
        link.sendSearch(.searchShare(on))
    }

    private func save(_ next: SearchRecorder) {
        guard next != recorder else { return }
        recorder = next
        if let data = try? JSONEncoder().encode(next) { defaults.set(data, forKey: Self.key) }
    }
}

/// Shown while a search runs (or this watch still holds fixes).
struct SearchPage: View {
    @EnvironmentObject private var link: PhoneLink
    @EnvironmentObject private var search: SearchModel
    @State private var confirming = false

    var body: some View {
        let info = link.phoneSnapshot?.search
        ScrollView {
            VStack(alignment: .leading, spacing: 6) {
                Text("Recovery search").font(.headline)
                if let p = info?.percent {
                    Text(String(format: "Covered %.0f%%", p)).font(.title3).bold()
                }
                if search.recording(link) {
                    Banner(text: "● SHARING TRACK", color: .red)
                    Text("This watch sends its position to the station through your phone while the search is on.")
                        .font(.footnote)
                    Button("Stop sharing", role: .destructive) { search.share(false, link: link) }
                } else if search.recorder.wantsToShare {
                    Text("Waiting for the station to accept…").font(.footnote)
                    Button("Cancel", role: .destructive) { search.share(false, link: link) }
                } else if info?.active == true {
                    Button("Share watch track…") { confirming = true }
                } else {
                    Text("No search is running.").font(.footnote)
                }
                if !search.recorder.pending.isEmpty {
                    Text("\(search.recorder.pending.count) fixes waiting for the phone").font(.footnote).foregroundStyle(.secondary)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .confirmationDialog("Share this watch's track?", isPresented: $confirming) {
            Button("Share track") { search.share(true, link: link) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("While this recovery search is on, the watch sends its position to the station through your phone. Everyone signed in sees it on the search map. Stop any time.")
        }
    }
}
