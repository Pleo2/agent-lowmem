import Darwin
import Foundation
import PressureProbeCore

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

func expectThrows<E: Error>(
  _ errorType: E.Type,
  _ operation: () throws -> Void,
  file: StaticString = #filePath,
  line: UInt = #line
) throws {
  do {
    try operation()
    throw TestFailure(
      description: "\(file):\(line): expected \(errorType), but no error was thrown"
    )
  } catch is E {
    return
  } catch {
    throw TestFailure(
      description: "\(file):\(line): expected \(errorType), got \(error)"
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

func mapsKnownSysctlValues() throws {
  try expectEqual(PressureLevel(sysctlRaw: 1), .normal)
  try expectEqual(PressureLevel(sysctlRaw: 2), .warning)
  try expectEqual(PressureLevel(sysctlRaw: 4), .critical)
  try expectEqual(PressureLevel(sysctlRaw: 3), .unknown)
}

func decodesCombinedDispatchMaskWithoutLosingEvents() throws {
  try expectEqual(
    PressureLevel.from(dispatchMask: 0x06),
    [.warning, .critical]
  )
}

func reportsTimerDelayAndCoalescedIntervals() throws {
  var schedule = SamplingSchedule(
    startNanoseconds: 1_000_000_000,
    intervalNanoseconds: 250_000_000
  )

  let tick = schedule.consume(nowNanoseconds: 1_600_000_000)

  try expectEqual(tick.expectedNanoseconds, 1_250_000_000)
  try expectEqual(tick.delayNanoseconds, 350_000_000)
  try expectEqual(tick.coalescedIntervals, 1)
}

func summarizesObservedSamplesWithoutInventingMissingMetrics() throws {
  var summary = RunSummary()
  summary.observe(
    sample: SamplePayload(
      pressureRaw: 1,
      pressure: .normal,
      schedulerDelayNanoseconds: 10,
      coalescedIntervals: 0,
      swapUsedBytes: 100,
      processTreeFootprintBytes: 200,
      processCount: 1,
      measurementErrors: []
    )
  )
  summary.observe(
    sample: SamplePayload(
      pressureRaw: 2,
      pressure: .warning,
      schedulerDelayNanoseconds: 30,
      coalescedIntervals: 0,
      swapUsedBytes: 150,
      processTreeFootprintBytes: nil,
      processCount: nil,
      measurementErrors: ["process-tree-unavailable"]
    )
  )

  try expectEqual(summary.sampleCount, 2)
  try expectEqual(summary.warningSampleCount, 1)
  try expectEqual(summary.maxSchedulerDelayNanoseconds, 30)
  try expectEqual(summary.maxSwapUsedBytes, 150)
  try expectEqual(summary.maxProcessTreeFootprintBytes, 200)
  try expectEqual(summary.measurementErrorCount, 1)
}

func parsesOutputLabelAndArgumentArray() throws {
  let config = try ProbeConfiguration.parse(
    arguments: [
      "--output", "/tmp/run.jsonl",
      "--label", "next-cold-01",
      "--", "pnpm", "build",
    ]
  )

  try expectEqual(config.outputURL.path, "/tmp/run.jsonl")
  try expectEqual(config.label, "next-cold-01")
  try expectEqual(config.command, ["pnpm", "build"])
  try expectEqual(config.commandMetadata.executable, "pnpm")
  try expectEqual(config.commandMetadata.argumentCount, 1)
}

func rejectsLabelsThatCouldContainPathsOrFreeText() throws {
  try expectThrows(ProbeConfigurationError.self) {
    _ = try ProbeConfiguration.parse(
      arguments: [
        "--output", "/tmp/run.jsonl",
        "--label", "next build /Users/me",
        "--", "pnpm", "build",
      ]
    )
  }
}

func createsOwnerOnlyJSONLAndRefusesOverwrite() throws {
  let directory = FileManager.default.temporaryDirectory
    .appendingPathComponent("pressure-probe-tests-\(UUID().uuidString)")
  try FileManager.default.createDirectory(
    at: directory,
    withIntermediateDirectories: false
  )
  defer { try? FileManager.default.removeItem(at: directory) }

  let trace = directory.appendingPathComponent("trace.jsonl")
  let writer = try JSONLineWriter.create(at: trace)
  try writer.append(
    event: "sample",
    monotonicNanoseconds: 42,
    wallTime: "2026-09-02T00:00:00Z",
    payload: ["pressure": "normal"]
  )
  try writer.close()

  var fileInfo = stat()
  try expectEqual(lstat(trace.path, &fileInfo), 0)
  try expectEqual(fileInfo.st_mode & 0o777, 0o600)

  let data = try Data(contentsOf: trace)
  let object = try JSONSerialization.jsonObject(with: data) as? [String: Any]
  try expectEqual(object?["schemaVersion"] as? Int, 1)
  try expectEqual(object?["event"] as? String, "sample")
  let payload = object?["data"] as? [String: String]
  try expectEqual(payload?["pressure"], "normal")

  try expectThrows(JSONLineWriterError.self) {
    _ = try JSONLineWriter.create(at: trace)
  }
  try expectEqual(try Data(contentsOf: trace), data)
}

let tests: [(String, () throws -> Void)] = [
  ("maps known sysctl values", mapsKnownSysctlValues),
  (
    "decodes combined Dispatch mask without losing events",
    decodesCombinedDispatchMaskWithoutLosingEvents
  ),
  (
    "reports timer delay and coalesced intervals",
    reportsTimerDelayAndCoalescedIntervals
  ),
  (
    "summarizes samples without inventing missing metrics",
    summarizesObservedSamplesWithoutInventingMissingMetrics
  ),
  ("parses output, label, and argument array", parsesOutputLabelAndArgumentArray),
  (
    "rejects labels that could contain paths or free text",
    rejectsLabelsThatCouldContainPathsOrFreeText
  ),
  (
    "creates owner-only JSONL and refuses overwrite",
    createsOwnerOnlyJSONLAndRefusesOverwrite
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
