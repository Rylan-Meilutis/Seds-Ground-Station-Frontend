import SwiftUI

@main
struct GS26WatchApp: App {
    @StateObject private var link: PhoneLink
    @StateObject private var location = LocationService()
    @StateObject private var voice: VoiceSession
    @StateObject private var search = SearchModel()

    init() {
        let link = PhoneLink()
        _link = StateObject(wrappedValue: link)
        _voice = StateObject(wrappedValue: VoiceSession(link: link))
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(link)
                .environmentObject(location)
                .environmentObject(voice)
                .environmentObject(search)
        }
    }
}
