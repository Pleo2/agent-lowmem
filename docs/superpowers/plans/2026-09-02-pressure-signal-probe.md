# Pressure Signal Probe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a non-enforcing macOS diagnostic probe and a reproducible 20-run protocol that determine whether native memory-pressure events arrive early enough to support Agent Lowmem v1.

**Architecture:** A dependency-free Swift package wraps one real build or test command, inherits its terminal streams, and records JSON Lines from a single serial Dispatch queue. It observes both the public Dispatch memory-pressure event source and the private current-level sysctl, samples scheduler delay and swap every 250 ms, and measures the launched process tree once per second. It never terminates the workload because of pressure and it never changes the Rev 3 policy.

**Tech Stack:** Swift 6.3, Swift Package Manager, Foundation, Dispatch, Darwin `sysctlbyname`, and libproc.

**Spec:** `docs/superpowers/specs/2026-09-02-agent-lowmem-v1-design.md`

## Global Constraints

- The reference host is exactly Apple M2, 8 GiB unified memory, 16 KiB pages, and macOS 26.x.
- The probe is diagnostic-only: it records pressure but never signals a child because of pressure.
- Sampling interval is fixed at 250 ms; swap and process-tree footprint are sampled every fourth tick.
- The probe launches an argument array through `/usr/bin/env`; it never invokes a shell or evaluates command text.
- Raw command arguments and environment-variable values are never written to the evidence file.
- The output path is created with exclusive-create semantics and mode `0600`; an existing trace is never overwritten.
- Builds and tests use one job and tests run without parallel execution.
- Raw JSONL evidence stays untracked; only the protocol and an eventual aggregate decision may be committed.
- Rev 4 remains blocked until the campaign produces enough qualifying runs.

**Execution environment note:** The active Apple Command Line Tools installation contains neither `Testing` nor `XCTest` in SwiftPM's import paths. The implementation therefore uses two dependency-free executable test harnesses that exercise public APIs and return nonzero on failure: `pressure-probe-core-tests` and `pressure-probe-macos-tests`. This preserves the RED/GREEN cycles without pinning a private SDK framework path.

---

## File Structure

- `tools/pressure-probe/Package.swift`: dependency-free Swift package definition.
- `tools/pressure-probe/Sources/PressureProbeCore/Pressure.swift`: pressure normalization and Dispatch-mask decoding.
- `tools/pressure-probe/Sources/PressureProbeCore/Sampling.swift`: monotonic sampling schedule, event envelopes, payloads, and run summary.
- `tools/pressure-probe/Sources/PressureProbeCore/CommandLine.swift`: strict argument-array parser and redacted command metadata.
- `tools/pressure-probe/Sources/PressureProbeCore/JSONLineWriter.swift`: exclusive, owner-only JSONL output.
- `tools/pressure-probe/Sources/PressureProbeMacOS/SystemMetrics.swift`: native host, pressure, swap, and process-tree measurements.
- `tools/pressure-probe/Sources/PressureProbeMacOS/ProbeRunner.swift`: public Dispatch event source, fixed timer, child lifecycle, and summary.
- `tools/pressure-probe/Sources/agent-lowmem-pressure-probe/main.swift`: thin executable entry point and exit-code preservation.
- `tools/pressure-probe/Tests/PressureProbeCoreTests/*.swift`: deterministic core behavior tests.
- `tools/pressure-probe/Tests/PressureProbeMacOSTests/*.swift`: live macOS boundary and smoke tests.
- `docs/experiments/2026-09-02-pressure-signal-protocol.md`: campaign procedure and decision table.
- `.gitignore`: ignores Swift build products and raw pressure traces.

### Task 1: Deterministic pressure and sampling core

**Files:**
- Create: `tools/pressure-probe/Package.swift`
- Create: `tools/pressure-probe/Sources/PressureProbeCore/Pressure.swift`
- Create: `tools/pressure-probe/Sources/PressureProbeCore/Sampling.swift`
- Test: `tools/pressure-probe/Tests/PressureProbeCoreTests/main.swift`

**Interfaces:**
- Produces: `PressureLevel.init(sysctlRaw:)`, `PressureLevel.from(dispatchMask:)`, `SamplingSchedule.consume(nowNanoseconds:)`, `RunSummary.observe(sample:)`.
- Consumes: no project code.

- [ ] **Step 1: Create the package and write failing pressure tests**

```swift
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
```

- [ ] **Step 2: Run the focused tests and verify RED**

Run: `cd tools/pressure-probe && swift run -j 1 pressure-probe-core-tests`

Expected: compilation fails because `PressureLevel` does not exist.

- [ ] **Step 3: Implement the minimum pressure mapping**

```swift
public enum PressureLevel: String, Codable, Equatable, Sendable {
    case normal, warning, critical, unknown

    public init(sysctlRaw: Int32) {
        self = switch sysctlRaw {
        case 1: .normal
        case 2: .warning
        case 4: .critical
        default: .unknown
        }
    }

    public static func from(dispatchMask: UInt) -> [Self] {
        var levels: [Self] = []
        if dispatchMask & 0x01 != 0 { levels.append(.normal) }
        if dispatchMask & 0x02 != 0 { levels.append(.warning) }
        if dispatchMask & 0x04 != 0 { levels.append(.critical) }
        return levels.isEmpty ? [.unknown] : levels
    }
}
```

- [ ] **Step 4: Run pressure tests and verify GREEN**

Run: `cd tools/pressure-probe && swift run -j 1 pressure-probe-core-tests`

Expected: all pressure tests pass.

- [ ] **Step 5: Write a failing schedule test**

```swift
func reportsTimerDelayAndCoalescedIntervals() throws {
    var schedule = SamplingSchedule(startNanoseconds: 1_000_000_000, intervalNanoseconds: 250_000_000)
    let tick = schedule.consume(nowNanoseconds: 1_600_000_000)
    try expectEqual(tick.expectedNanoseconds, 1_250_000_000)
    try expectEqual(tick.delayNanoseconds, 350_000_000)
    try expectEqual(tick.coalescedIntervals, 1)
}
```

- [ ] **Step 6: Run the focused schedule test and verify RED**

Run: `cd tools/pressure-probe && swift run -j 1 pressure-probe-core-tests`

Expected: compilation fails because `SamplingSchedule` does not exist.

- [ ] **Step 7: Implement schedule records and summary accumulation**

Implement `SamplingSchedule`, generic `RecordEnvelope<Payload: Encodable>`, the session/sample/pressure/child payloads, and `RunSummary`. Use integer nanoseconds internally and convert only when encoding human-facing milliseconds.

- [ ] **Step 8: Run all core tests and commit**

Run: `cd tools/pressure-probe && swift run -j 1 pressure-probe-core-tests`

Expected: all core tests pass.

```bash
git add tools/pressure-probe
git commit -m "test: define pressure probe sampling core"
```

### Task 2: Safe CLI and JSONL evidence boundary

**Files:**
- Create: `tools/pressure-probe/Sources/PressureProbeCore/CommandLine.swift`
- Create: `tools/pressure-probe/Sources/PressureProbeCore/JSONLineWriter.swift`
- Test: `tools/pressure-probe/Tests/PressureProbeCoreTests/main.swift`

**Interfaces:**
- Consumes: record payloads from Task 1.
- Produces: `ProbeConfiguration.parse(arguments:)` and `JSONLineWriter.create(at:)` / `append(event:payload:)`.

- [ ] **Step 1: Write failing CLI tests**

```swift
func parsesOutputLabelAndArgumentArray() throws {
    let config = try ProbeConfiguration.parse(arguments: [
        "--output", "/tmp/run.jsonl", "--label", "next-cold-01", "--", "pnpm", "build"
    ])
    try expectEqual(config.label, "next-cold-01")
    try expectEqual(config.command, ["pnpm", "build"])
    try expectEqual(config.commandMetadata.executable, "pnpm")
}

func rejectsLabelsThatCouldContainPathsOrFreeText() throws {
    try expectThrows(ProbeConfigurationError.self) {
        _ = try ProbeConfiguration.parse(arguments: ["--output", "/tmp/x", "--label", "next build /Users/me", "--", "pnpm", "build"])
    }
}
```

- [ ] **Step 2: Run CLI tests and verify RED**

Run: `cd tools/pressure-probe && swift run -j 1 pressure-probe-core-tests`

Expected: compilation fails because `ProbeConfiguration` does not exist.

- [ ] **Step 3: Implement the strict CLI parser**

Accept exactly `--output PATH --label SAFE_LABEL -- COMMAND [ARG ...]`, plus `--help`. Restrict labels to 1–64 ASCII characters from `[A-Za-z0-9._-]`. Preserve the command argument array for execution but expose only the executable basename and argument count as recordable metadata.

- [ ] **Step 4: Run CLI tests and verify GREEN**

Run: `cd tools/pressure-probe && swift run -j 1 pressure-probe-core-tests`

Expected: all CLI tests pass.

- [ ] **Step 5: Write failing writer tests**

```swift
func createsOwnerOnlyFileAndRefusesOverwrite() throws {
    let path = temporaryDirectory.appending(path: "trace.jsonl")
    let writer = try JSONLineWriter.create(at: path)
    try writer.append(event: "sample", monotonicNanoseconds: 42, wallTime: "2026-09-02T00:00:00Z", payload: ["pressure": "normal"])
    try writer.close()
    var fileInfo = stat()
    try expectEqual(lstat(path.path, &fileInfo), 0)
    try expectEqual(fileInfo.st_mode & 0o777, 0o600)
    try expectThrows(JSONLineWriterError.self) {
        _ = try JSONLineWriter.create(at: path)
    }
}
```

- [ ] **Step 6: Run writer tests and verify RED**

Run: `cd tools/pressure-probe && swift run -j 1 pressure-probe-core-tests`

Expected: compilation fails because `JSONLineWriter` does not exist.

- [ ] **Step 7: Implement exclusive JSONL output**

Use `open(path, O_WRONLY | O_CREAT | O_EXCL, 0o600)`, one encoded envelope plus newline per append, stable sorted JSON keys, and explicit synchronization on clean close. Never replace or truncate an existing file.

- [ ] **Step 8: Run all core tests and commit**

Run: `cd tools/pressure-probe && swift run -j 1 pressure-probe-core-tests`

Expected: all core tests pass.

```bash
git add tools/pressure-probe
git commit -m "feat: add safe pressure evidence output"
```

### Task 3: Native macOS measurements

**Files:**
- Create: `tools/pressure-probe/Sources/PressureProbeMacOS/SystemMetrics.swift`
- Test: `tools/pressure-probe/Tests/PressureProbeMacOSTests/main.swift`

**Interfaces:**
- Consumes: `PressureLevel` from Task 1.
- Produces: `SystemMetrics.hostMetadata()`, `currentPressure()`, `swapUsedBytes()`, and `processTreeFootprint(rootPID:)`.

- [ ] **Step 1: Write live boundary tests**

```swift
func readsReferenceHostMeasurements() throws {
    let metrics = SystemMetrics()
    let host = try metrics.hostMetadata()
    try expectEqual(host.cpuBrand, "Apple M2")
    try expectEqual(host.memoryBytes, 8_589_934_592)
    try expectEqual(host.pageSizeBytes, 16_384)
    let pressure = try metrics.currentPressure()
    try expectTrue([1, 2, 4].contains(pressure.rawValue), "unexpected pressure")
    let swap = try metrics.swapUsage()
    try expectTrue(swap.usedBytes <= swap.totalBytes, "swap used exceeds total")
}

func ownProcessAppearsInTreeFootprint() throws {
    let snapshot = try SystemMetrics().processTreeFootprint(rootPID: getpid())
    try expectTrue(snapshot.processCount >= 1, "root process is missing")
    try expectTrue(snapshot.physicalFootprintBytes > 0, "footprint is empty")
}
```

- [ ] **Step 2: Run macOS tests and verify RED**

Run: `cd tools/pressure-probe && swift run -j 1 pressure-probe-macos-tests`

Expected: compilation fails because `SystemMetrics` does not exist.

- [ ] **Step 3: Implement native reads**

Read `kern.osproductversion`, `machdep.cpu.brand_string`, `hw.memsize`, `hw.pagesize`, `kern.memorystatus_vm_pressure_level`, and `vm.swapusage` through `sysctlbyname`. Enumerate PIDs with `proc_listallpids`, build the live parent map from `PROC_PIDTBSDINFO`, select the root and reachable descendants, then sum `ri_phys_footprint` from `proc_pid_rusage(RUSAGE_INFO_V4)`. Return explicit errors for unavailable mandatory reads and document that reparented or already-exited descendants can be missed.

- [ ] **Step 4: Run macOS tests and verify GREEN**

Run: `cd tools/pressure-probe && swift run -j 1 pressure-probe-macos-tests`

Expected: all live boundary tests pass on the reference Mac.

- [ ] **Step 5: Commit**

```bash
git add tools/pressure-probe
git commit -m "feat: observe native macOS pressure metrics"
```

### Task 4: Observational recorder and child lifecycle

**Files:**
- Create: `tools/pressure-probe/Sources/PressureProbeMacOS/ProbeRunner.swift`
- Create: `tools/pressure-probe/Sources/agent-lowmem-pressure-probe/main.swift`
- Test: `tools/pressure-probe/Tests/PressureProbeMacOSTests/main.swift`

**Interfaces:**
- Consumes: configuration and writer from Task 2; metrics from Task 3.
- Produces: `ProbeRunner.run(configuration:) -> Int32` preserving a normal child exit code or `128 + signal`.

- [ ] **Step 1: Write a failing smoke test**

```swift
func recordsARealChildWithoutLoggingArguments() throws {
    let trace = temporaryDirectory.appending(path: "true.jsonl")
    let config = try ProbeConfiguration.parse(arguments: [
        "--output", trace.path, "--label", "smoke-01", "--", "/usr/bin/true", "secret-argument"
    ])
    try expectEqual(try ProbeRunner().run(configuration: config), 0)
    let contents = try String(contentsOf: trace, encoding: .utf8)
    try expectTrue(contents.contains("session_start"), "missing session start")
    try expectTrue(contents.contains("child_exit"), "missing child exit")
    try expectTrue(!contents.contains("secret-argument"), "argument leaked")
}
```

- [ ] **Step 2: Run the runner test and verify RED**

Run: `cd tools/pressure-probe && swift run -j 1 pressure-probe-macos-tests`

Expected: compilation fails because `ProbeRunner` does not exist.

- [ ] **Step 3: Implement the minimum recorder**

Create the evidence file before launching the child. Write `session_start` and an initial sample, launch `/usr/bin/env` with the command argument array and inherited stdin/stdout/stderr, then activate:

```swift
DispatchSource.makeMemoryPressureSource(
    eventMask: [.normal, .warning, .critical],
    queue: recordingQueue
)
```

Use a fixed 250 ms timer on the same serial recording queue. Every sample records raw and normalized sysctl pressure plus timer delay; every fourth sample also records swap and best-effort tree footprint. Dispatch callbacks record their raw mask and decoded levels immediately. No pressure callback may signal or terminate the child.

- [ ] **Step 4: Preserve child outcome and finalize evidence**

After `waitUntilExit`, synchronously stop both Dispatch sources, write `child_exit` and `session_end`, synchronize and close the trace, print a concise summary, and return `terminationStatus` for normal exit or `128 + terminationStatus` for an uncaught signal.

- [ ] **Step 5: Run all tests and a release smoke trace**

Run: `cd tools/pressure-probe && swift run -j 1 pressure-probe-core-tests && swift run -j 1 pressure-probe-macos-tests`

Expected: all tests pass.

Run: `cd tools/pressure-probe && swift run -j 1 -c release agent-lowmem-pressure-probe --output /tmp/agent-lowmem-smoke.jsonl --label smoke-release -- /usr/bin/sleep 2`

Expected: exit 0, a summary on stderr, and ordered `session_start`, samples, `child_exit`, and `session_end` records without command arguments.

- [ ] **Step 6: Commit**

```bash
git add tools/pressure-probe
git commit -m "feat: record pressure during real workloads"
```

### Task 5: Reproducible campaign and decision gate

**Files:**
- Create: `docs/experiments/2026-09-02-pressure-signal-protocol.md`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: executable and JSONL schema from Task 4.
- Produces: exact collection procedure, run-quality rules, evidence table, and Rev 4 branching decision.

- [ ] **Step 1: Document build and invocation commands**

The protocol must build once with `swift build -j 1 -c release`, invoke `.build/release/agent-lowmem-pressure-probe`, store traces below `artifacts/pressure-probe/raw/`, and use safe labels such as `next-build-cold-01`.

- [ ] **Step 2: Define the 20 qualifying runs**

Collect at least 20 completed real workloads, at least five repetitions per available workload class and at least three classes among focused test, full test, typecheck, Next build, and Nest build. Alternate warm and cold repository-cache conditions, keep the normal editor/browser/agent workload open, and record AC versus battery in the experiment ledger. Do not allocate memory synthetically or continue a run after the machine becomes unsafe.

- [ ] **Step 3: Define valid and invalid evidence**

A run qualifies only when it has `session_start`, `child_start`, at least four samples, `child_exit`, and `session_end`; no measurement error occurred; and the workload/conditions ledger is complete. Interrupted traces remain useful for diagnosis but do not count toward the 20 completed runs.

- [ ] **Step 4: Define the Rev 4 decision table**

Use three outcomes:

1. Dispatch warning consistently precedes objective scheduler degradation: make Dispatch the in-run trigger and choose a sustained-warning window from the observed distribution.
2. Warning overlaps or follows degradation but critical still separates unsafe runs: lead with deterministic controls, keep critical termination best-effort, and make warning informational until more evidence exists.
3. Neither warning nor critical separates unsafe runs in time: remove preventive pressure termination from the v1 promise; any future swap derivative begins as observation-only.

No branch may infer a threshold from successful runs alone or claim that absence of a pressure event proves safety.

- [ ] **Step 5: Ignore raw evidence and verify documentation**

Add `.build/`, `.swiftpm/`, and `artifacts/pressure-probe/raw/*.jsonl` to `.gitignore`. Run:

```bash
git diff --check
rg -n 'T[B]D|T[O]DO|implement[ ]later|fill[ ]in' docs/superpowers/plans/2026-09-02-pressure-signal-probe.md docs/experiments/2026-09-02-pressure-signal-protocol.md
```

Expected: `git diff --check` succeeds and the placeholder scan returns no matches.

- [ ] **Step 6: Commit**

```bash
git add .gitignore docs/experiments docs/superpowers/plans
git commit -m "docs: define pressure signal experiment"
```

## Final Verification

- [ ] Run `swift format lint --recursive --strict Package.swift Sources Tests` if the installed Swift toolchain provides `swift format`; otherwise record that the optional formatter is unavailable.
- [ ] Run `swift run -j 1 pressure-probe-core-tests` and `swift run -j 1 pressure-probe-macos-tests` from `tools/pressure-probe`.
- [ ] Run `swift build -j 1 -c release` from `tools/pressure-probe`.
- [ ] Run a two-second release smoke trace and inspect every JSONL line with `python3 -m json.tool` or an equivalent read-only parser.
- [ ] Confirm the smoke file has mode `0600`, contains no command arguments, and is removed after inspection.
- [ ] Run `git diff --check` and inspect `git status --short`.
- [ ] Run `bash "$HOME/.codex/scripts/audit_mcp_processes.sh" --clean-orphans` and confirm no probe, Swift build, or duplicate MCP process remains.
