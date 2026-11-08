// swift-tools-version:5.9
import PackageDescription

// Platform-neutral watch logic (Foundation only) so it can be tested with
// `swift test` on macOS or Linux and linked into the watchOS app.
let package = Package(
    name: "WatchCore",
    platforms: [.watchOS(.v10), .macOS(.v13), .iOS(.v17)],
    products: [.library(name: "WatchCore", targets: ["WatchCore"])],
    targets: [
        .target(name: "WatchCore"),
        .testTarget(name: "WatchCoreTests", dependencies: ["WatchCore"]),
    ]
)
