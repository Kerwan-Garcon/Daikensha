// swift-tools-version:6.0
// The Android link's portable core (NotchBuddy/Sources/App/AndroidLink/Core), tested on its own.
// scripts/test-android-link.sh copies the core and Support/ into each target before building, so the
// tests compile the app's very files (one module, like the other swiftc tests of this repository).
// swift-crypto stands in for CryptoKit on Linux only (Docker); the app itself never depends on it.
import PackageDescription

let crypto: [Target.Dependency] = [
    .product(name: "Crypto", package: "swift-crypto", condition: .when(platforms: [.linux])),
]
let settings: [SwiftSetting] = [.define("ANDROID_LINK")]

let package = Package(
    name: "AndroidLinkTests",
    platforms: [.macOS(.v15)],
    dependencies: [
        .package(url: "https://github.com/apple/swift-crypto.git", exact: "3.15.1"),
    ],
    targets: [
        .executableTarget(name: "AndroidLinkTests", dependencies: crypto, swiftSettings: settings),
        .executableTarget(name: "AndroidLinkInterop", dependencies: crypto, swiftSettings: settings),
    ]
)
