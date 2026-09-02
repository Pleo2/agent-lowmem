import Darwin

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
