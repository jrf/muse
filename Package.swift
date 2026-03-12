// swift-tools-version: 6.2

import PackageDescription

let package = Package(
    name: "muse",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "muse",
            path: "Sources"
        ),
    ]
)
