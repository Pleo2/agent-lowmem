// swift-tools-version: 6.0

import PackageDescription

let package = Package(
  name: "PressureSignalProbe",
  platforms: [.macOS(.v13)],
  products: [
    .library(name: "PressureProbeCore", targets: ["PressureProbeCore"]),
    .library(name: "PressureProbeMacOS", targets: ["PressureProbeMacOS"]),
    .executable(
      name: "pressure-probe-core-tests",
      targets: ["PressureProbeCoreTests"]
    ),
    .executable(
      name: "pressure-probe-macos-tests",
      targets: ["PressureProbeMacOSTests"]
    ),
    .executable(
      name: "agent-lowmem-pressure-probe",
      targets: ["agent-lowmem-pressure-probe"]
    ),
  ],
  targets: [
    .target(name: "PressureProbeCore"),
    .target(
      name: "PressureProbeMacOS",
      dependencies: ["PressureProbeCore"]
    ),
    .executableTarget(
      name: "PressureProbeCoreTests",
      dependencies: ["PressureProbeCore"],
      path: "Tests/PressureProbeCoreTests"
    ),
    .executableTarget(
      name: "PressureProbeMacOSTests",
      dependencies: ["PressureProbeCore", "PressureProbeMacOS"],
      path: "Tests/PressureProbeMacOSTests"
    ),
    .executableTarget(
      name: "agent-lowmem-pressure-probe",
      dependencies: ["PressureProbeCore", "PressureProbeMacOS"]
    ),
  ]
)
