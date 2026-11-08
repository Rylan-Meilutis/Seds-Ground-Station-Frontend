import SwiftUI

@main
struct GS26WatchApp: App {
    @StateObject private var link = PhoneLink()
    @StateObject private var location = LocationService()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(link)
                .environmentObject(location)
        }
    }
}
