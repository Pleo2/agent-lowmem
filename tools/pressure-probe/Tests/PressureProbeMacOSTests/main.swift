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

func recordsARealChildWithoutLoggingArguments() throws {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("pressure-probe-runner-\(UUID().uuidString)")
    try FileManager.default.createDirectory(
        at: directory,
        withIntermediateDirectories: false
    )
    defer { try? FileManager.default.removeItem(at: directory) }

    let trace = directory.appendingPathComponent("true.jsonl")
    let configuration = try ProbeConfiguration.parse(
        arguments: [
            "--output", trace.path,
            "--label", "smoke-01",
            "--", "/usr/bin/true", "secret-argument",
        ]
    )

    let status = try ProbeRunner().run(configuration: configuration)
    try expectEqual(status, 0)

    let contents = try String(contentsOf: trace, encoding: .utf8)
    try expectTrue(contents.contains("\"event\":\"session_start\""), "missing session_start")
    try expectTrue(contents.contains("\"event\":\"child_start\""), "missing child_start")
    try expectTrue(contents.contains("\"event\":\"child_exit\""), "missing child_exit")
    try expectTrue(contents.contains("\"event\":\"session_end\""), "missing session_end")
    try expectTrue(!contents.contains("secret-argument"), "raw command argument leaked")

    for line in contents.split(separator: "\n") {
        _ = try JSONSerialization.jsonObject(with: Data(line.utf8))
    }
}

func recordsPeriodicSwapAndProcessTreeContext() throws {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("pressure-probe-periodic-\(UUID().uuidString)")
    try FileManager.default.createDirectory(
        at: directory,
        withIntermediateDirectories: false
    )
    defer { try? FileManager.default.removeItem(at: directory) }

    let trace = directory.appendingPathComponent("sleep.jsonl")
    let configuration = try ProbeConfiguration.parse(
        arguments: [
            "--output", trace.path,
            "--label", "periodic-01",
            "--", "/bin/sleep", "1.3",
        ]
    )

    try expectEqual(try ProbeRunner().run(configuration: configuration), 0)
    let records = try String(contentsOf: trace, encoding: .utf8)
        .split(separator: "\n")
        .map { line in
            try JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any]
        }
        .compactMap { $0 }
    let samples = records.filter { $0["event"] as? String == "sample" }
    try expectTrue(samples.count >= 5, "expected initial and periodic samples")

    let contextualSample = samples.first { record in
        guard let data = record["data"] as? [String: Any] else {
            return false
        }
        return data["swapUsedBytes"] is NSNumber
            && data["processTreeFootprintBytes"] is NSNumber
            && data["processCount"] is NSNumber
    }
    try expectTrue(contextualSample != nil, "missing one-second context sample")
}

func preservesNonzeroChildExitStatus() throws {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("pressure-probe-status-\(UUID().uuidString)")
    try FileManager.default.createDirectory(
        at: directory,
        withIntermediateDirectories: false
    )
    defer { try? FileManager.default.removeItem(at: directory) }

    let trace = directory.appendingPathComponent("false.jsonl")
    let configuration = try ProbeConfiguration.parse(
        arguments: [
            "--output", trace.path,
            "--label", "status-01",
            "--", "/usr/bin/false",
        ]
    )

    try expectEqual(try ProbeRunner().run(configuration: configuration), 1)
}

let tests: [(String, () throws -> Void)] = [
    ("reads reference host measurements", readsReferenceHostMeasurements),
    (
        "observes own process and live child in tree footprint",
        observesOwnProcessAndLiveChildInTreeFootprint
    ),
    (
        "records a real child without logging arguments",
        recordsARealChildWithoutLoggingArguments
    ),
    (
        "records periodic swap and process-tree context",
        recordsPeriodicSwapAndProcessTreeContext
    ),
    ("preserves nonzero child exit status", preservesNonzeroChildExitStatus),
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
