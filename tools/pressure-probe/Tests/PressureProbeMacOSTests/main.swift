import Darwin
import Foundation

import PressureProbeCore
import PressureProbeMacOS

struct TestFailure: Error, CustomStringConvertible {
    let description: String
}

func expectEqual<T: Equatable>(
    _ actual: T,
    _ expected: T,
    file: StaticString = #filePath,
    line: UInt = #line
) throws {
    guard actual == expected else {
        throw TestFailure(
            description: "\(file):\(line): expected \(expected), got \(actual)"
        )
    }
}

func expectTrue(
    _ condition: @autoclosure () -> Bool,
    _ message: String,
    file: StaticString = #filePath,
    line: UInt = #line
) throws {
    guard condition() else {
        throw TestFailure(description: "\(file):\(line): \(message)")
    }
}

func readsReferenceHostMeasurements() throws {
    let metrics = SystemMetrics()
    let host = try metrics.hostMetadata()

    try expectEqual(host.cpuBrand, "Apple M2")
    try expectEqual(host.memoryBytes, 8_589_934_592)
    try expectEqual(host.pageSizeBytes, 16_384)
    try expectTrue(host.osProductVersion.hasPrefix("26."), "expected macOS 26.x")

    let pressure = try metrics.currentPressure()
    try expectTrue([1, 2, 4].contains(pressure.rawValue), "unexpected pressure value")
    try expectTrue(pressure.level != .unknown, "known pressure must normalize")

    let swap = try metrics.swapUsage()
    try expectTrue(swap.usedBytes <= swap.totalBytes, "swap used exceeds total")
    try expectTrue(swap.freeBytes <= swap.totalBytes, "swap free exceeds total")
}

func observesOwnProcessAndLiveChildInTreeFootprint() throws {
    let child = Process()
    child.executableURL = URL(fileURLWithPath: "/bin/sleep")
    child.arguments = ["2"]
    try child.run()
    defer {
        if child.isRunning {
            child.terminate()
        }
        child.waitUntilExit()
    }

    let snapshot = try SystemMetrics().processTreeFootprint(rootPID: getpid())
    try expectTrue(snapshot.processCount >= 2, "root and live child must be observed")
    try expectTrue(snapshot.physicalFootprintBytes > 0, "footprint must be positive")
}

let tests: [(String, () throws -> Void)] = [
    ("reads reference host measurements", readsReferenceHostMeasurements),
    (
        "observes own process and live child in tree footprint",
        observesOwnProcessAndLiveChildInTreeFootprint
    ),
]

var failures = 0
for (name, test) in tests {
    do {
        try test()
        print("PASS: \(name)")
    } catch {
        failures += 1
        fputs("FAIL: \(name): \(error)\n", stderr)
    }
}

exit(failures == 0 ? 0 : 1)
