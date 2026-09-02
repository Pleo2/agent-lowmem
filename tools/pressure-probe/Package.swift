// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "PressureSignalProbe",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "PressureProbeCore", targets: ["PressureProbeCore"]),
        .executable(
            name: "pressure-probe-core-tests",
            targets: ["PressureProbeCoreTests"]
        ),
    ],
    targets: [
        .target(name: "PressureProbeCore"),
        .executableTarget(
            name: "PressureProbeCoreTests",
            dependencies: ["PressureProbeCore"],
            path: "Tests/PressureProbeCoreTests"
        ),
    ]
)
