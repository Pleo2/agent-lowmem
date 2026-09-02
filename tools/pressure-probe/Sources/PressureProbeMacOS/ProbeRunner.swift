import Darwin
import Dispatch
import Foundation
import PressureProbeCore

public enum ProbeRunnerError: Error, CustomStringConvertible {
  case childLaunchFailed(executable: String, message: String)
  case recordingFailed(String)

  public var description: String {
    switch self {
    case .childLaunchFailed(let executable, let message):
      "could not launch \(executable): \(message)"
    case .recordingFailed(let message):
      "evidence recording failed: \(message)"
    }
  }
}

public struct ProbeRunner: Sendable {
  public static let sampleIntervalMilliseconds = 250
  public static let footprintIntervalMilliseconds = 1_000

  private let metrics: SystemMetrics

  public init(metrics: SystemMetrics = SystemMetrics()) {
    self.metrics = metrics
  }

  public func run(configuration: ProbeConfiguration) throws -> Int32 {
    let host = try metrics.hostMetadata()
    let writer = try JSONLineWriter.create(at: configuration.outputURL)
    let recordingQueue = DispatchQueue(
      label: "dev.agentlowmem.pressure-probe.recording",
      qos: .userInitiated
    )
    let startNanoseconds = monotonicNanoseconds()
    let state = RecordingState(
      writer: writer,
      metrics: metrics,
      startNanoseconds: startNanoseconds
    )

    recordingQueue.sync {
      state.recordSessionStart(
        configuration: configuration,
        host: host,
        nowNanoseconds: startNanoseconds
      )
      state.recordInitialSample(nowNanoseconds: monotonicNanoseconds())
    }

    let child = Process()
    child.executableURL = URL(fileURLWithPath: "/usr/bin/env")
    child.arguments = configuration.command
    child.standardInput = FileHandle.standardInput
    child.standardOutput = FileHandle.standardOutput
    child.standardError = FileHandle.standardError

    do {
      try child.run()
    } catch {
      let launchTime = monotonicNanoseconds()
      recordingQueue.sync {
        state.recordChildLaunchError(
          executable: configuration.commandMetadata.executable,
          nowNanoseconds: launchTime
        )
        state.recordSessionEnd(
          complete: false,
          nowNanoseconds: launchTime
        )
      }
      try writer.close()
      throw ProbeRunnerError.childLaunchFailed(
        executable: configuration.commandMetadata.executable,
        message: String(describing: error)
      )
    }

    let childPID = child.processIdentifier
    recordingQueue.sync {
      state.recordChildStart(
        processID: childPID,
        nowNanoseconds: monotonicNanoseconds()
      )
    }

    let timer = DispatchSource.makeTimerSource(queue: recordingQueue)
    timer.schedule(
      deadline: .now() + .milliseconds(Self.sampleIntervalMilliseconds),
      repeating: .milliseconds(Self.sampleIntervalMilliseconds),
      leeway: .milliseconds(5)
    )
    timer.setEventHandler {
      state.recordScheduledSample(
        rootPID: childPID,
        nowNanoseconds: monotonicNanoseconds()
      )
    }

    let pressureSource = DispatchSource.makeMemoryPressureSource(
      eventMask: [.normal, .warning, .critical],
      queue: recordingQueue
    )
    pressureSource.setEventHandler { [weak pressureSource] in
      guard let pressureSource else {
        return
      }
      state.recordDispatchPressure(
        rawMask: pressureSource.data.rawValue,
        nowNanoseconds: monotonicNanoseconds()
      )
    }

    pressureSource.resume()
    timer.resume()
    child.waitUntilExit()
    timer.cancel()
    pressureSource.cancel()

    let childExitNanoseconds = monotonicNanoseconds()
    recordingQueue.sync {
      state.recordChildExit(
        status: child.terminationStatus,
        reason: child.terminationReason,
        nowNanoseconds: childExitNanoseconds
      )
      state.recordSessionEnd(
        complete: true,
        nowNanoseconds: childExitNanoseconds
      )
    }

    try writer.close()
    let snapshot = recordingQueue.sync { state.snapshot() }
    if let failure = snapshot.recordingFailure {
      throw ProbeRunnerError.recordingFailed(failure)
    }

    printSummary(snapshot, label: configuration.label)
    switch child.terminationReason {
    case .exit:
      return child.terminationStatus
    case .uncaughtSignal:
      return 128 + child.terminationStatus
    @unknown default:
      return child.terminationStatus
    }
  }
}

private final class RecordingState: @unchecked Sendable {
  private let writer: JSONLineWriter
  private let metrics: SystemMetrics
  private let formatter: ISO8601DateFormatter
  private var schedule: SamplingSchedule
  private var summary = RunSummary()
  private var sampleIndex = 0
  private var childStartNanoseconds: UInt64?
  private var dispatchWarningEventCount = 0
  private var dispatchCriticalEventCount = 0
  private var recordingFailure: String?

  init(
    writer: JSONLineWriter,
    metrics: SystemMetrics,
    startNanoseconds: UInt64
  ) {
    self.writer = writer
    self.metrics = metrics
    schedule = SamplingSchedule(
      startNanoseconds: startNanoseconds,
      intervalNanoseconds: UInt64(
        ProbeRunner.sampleIntervalMilliseconds * 1_000_000
      )
    )
    formatter = ISO8601DateFormatter()
    formatter.formatOptions = [
      .withInternetDateTime,
      .withFractionalSeconds,
    ]
  }

  func recordSessionStart(
    configuration: ProbeConfiguration,
    host: HostMetadata,
    nowNanoseconds: UInt64
  ) {
    append(
      event: "session_start",
      nowNanoseconds: nowNanoseconds,
      payload: SessionStartPayload(
        probeVersion: "0.1.0",
        sessionID: UUID().uuidString.lowercased(),
        label: configuration.label,
        sampleIntervalMilliseconds: ProbeRunner.sampleIntervalMilliseconds,
        footprintIntervalMilliseconds: ProbeRunner.footprintIntervalMilliseconds,
        probeProcessID: getpid(),
        command: configuration.commandMetadata,
        host: host,
        pressureStateInterface: "kern.memorystatus_vm_pressure_level",
        pressureEventInterface: "DISPATCH_SOURCE_TYPE_MEMORYPRESSURE"
      )
    )
  }

  func recordInitialSample(nowNanoseconds: UInt64) {
    recordSample(
      schedulerDelayNanoseconds: 0,
      coalescedIntervals: 0,
      includeContext: true,
      rootPID: nil,
      nowNanoseconds: nowNanoseconds
    )
  }

  func recordChildStart(processID: pid_t, nowNanoseconds: UInt64) {
    childStartNanoseconds = nowNanoseconds
    append(
      event: "child_start",
      nowNanoseconds: nowNanoseconds,
      payload: ChildStartPayload(processID: processID)
    )
  }

  func recordChildLaunchError(executable: String, nowNanoseconds: UInt64) {
    append(
      event: "child_launch_error",
      nowNanoseconds: nowNanoseconds,
      payload: ChildLaunchErrorPayload(executable: executable)
    )
  }

  func recordScheduledSample(rootPID: pid_t, nowNanoseconds: UInt64) {
    let tick = schedule.consume(nowNanoseconds: nowNanoseconds)
    sampleIndex += 1
    recordSample(
      schedulerDelayNanoseconds: tick.delayNanoseconds,
      coalescedIntervals: tick.coalescedIntervals,
      includeContext: sampleIndex % 4 == 0,
      rootPID: rootPID,
      nowNanoseconds: nowNanoseconds
    )
  }

  func recordDispatchPressure(rawMask: UInt, nowNanoseconds: UInt64) {
    let levels = PressureLevel.from(dispatchMask: rawMask)
    if levels.contains(.warning) {
      dispatchWarningEventCount += 1
    }
    if levels.contains(.critical) {
      dispatchCriticalEventCount += 1
    }
    append(
      event: "dispatch_pressure",
      nowNanoseconds: nowNanoseconds,
      payload: DispatchPressurePayload(rawMask: rawMask, levels: levels)
    )
  }

  func recordChildExit(
    status: Int32,
    reason: Process.TerminationReason,
    nowNanoseconds: UInt64
  ) {
    let duration = childStartNanoseconds.map { nowNanoseconds - $0 }
    append(
      event: "child_exit",
      nowNanoseconds: nowNanoseconds,
      payload: ChildExitPayload(
        status: status,
        reason: reason == .exit ? "exit" : "uncaught_signal",
        durationNanoseconds: duration
      )
    )
  }

  func recordSessionEnd(complete: Bool, nowNanoseconds: UInt64) {
    append(
      event: "session_end",
      nowNanoseconds: nowNanoseconds,
      payload: SessionEndPayload(
        complete: complete,
        summary: summary,
        dispatchWarningEventCount: dispatchWarningEventCount,
        dispatchCriticalEventCount: dispatchCriticalEventCount,
        recordingFailure: recordingFailure
      )
    )
  }

  func snapshot() -> RecordingSnapshot {
    RecordingSnapshot(
      summary: summary,
      dispatchWarningEventCount: dispatchWarningEventCount,
      dispatchCriticalEventCount: dispatchCriticalEventCount,
      recordingFailure: recordingFailure
    )
  }

  private func recordSample(
    schedulerDelayNanoseconds: UInt64,
    coalescedIntervals: UInt64,
    includeContext: Bool,
    rootPID: pid_t?,
    nowNanoseconds: UInt64
  ) {
    var errors: [String] = []
    let pressure: PressureReading?
    do {
      pressure = try metrics.currentPressure()
    } catch {
      pressure = nil
      errors.append("pressure-unavailable")
    }

    let swapUsedBytes: UInt64?
    if includeContext {
      do {
        swapUsedBytes = try metrics.swapUsage().usedBytes
      } catch {
        swapUsedBytes = nil
        errors.append("swap-unavailable")
      }
    } else {
      swapUsedBytes = nil
    }

    let footprint: ProcessTreeFootprint?
    if includeContext, let rootPID {
      do {
        footprint = try metrics.processTreeFootprint(rootPID: rootPID)
      } catch {
        footprint = nil
        errors.append("process-tree-unavailable")
      }
    } else {
      footprint = nil
    }

    let payload = SamplePayload(
      pressureRaw: pressure?.rawValue,
      pressure: pressure?.level ?? .unknown,
      schedulerDelayNanoseconds: schedulerDelayNanoseconds,
      coalescedIntervals: coalescedIntervals,
      swapUsedBytes: swapUsedBytes,
      processTreeFootprintBytes: footprint?.physicalFootprintBytes,
      processCount: footprint?.processCount,
      measurementErrors: errors
    )
    summary.observe(sample: payload)
    append(event: "sample", nowNanoseconds: nowNanoseconds, payload: payload)
  }

  private func append<Payload: Encodable>(
    event: String,
    nowNanoseconds: UInt64,
    payload: Payload
  ) {
    guard recordingFailure == nil else {
      return
    }
    do {
      try writer.append(
        event: event,
        monotonicNanoseconds: nowNanoseconds,
        wallTime: formatter.string(from: Date()),
        payload: payload
      )
    } catch {
      recordingFailure = String(describing: error)
    }
  }
}

private struct SessionStartPayload: Encodable {
  let probeVersion: String
  let sessionID: String
  let label: String
  let sampleIntervalMilliseconds: Int
  let footprintIntervalMilliseconds: Int
  let probeProcessID: pid_t
  let command: CommandMetadata
  let host: HostMetadata
  let pressureStateInterface: String
  let pressureEventInterface: String
}

private struct ChildStartPayload: Encodable {
  let processID: pid_t
}

private struct ChildLaunchErrorPayload: Encodable {
  let executable: String
}

private struct DispatchPressurePayload: Encodable {
  let rawMask: UInt
  let levels: [PressureLevel]
}

private struct ChildExitPayload: Encodable {
  let status: Int32
  let reason: String
  let durationNanoseconds: UInt64?
}

private struct SessionEndPayload: Encodable {
  let complete: Bool
  let summary: RunSummary
  let dispatchWarningEventCount: Int
  let dispatchCriticalEventCount: Int
  let recordingFailure: String?
}

private struct RecordingSnapshot: Sendable {
  let summary: RunSummary
  let dispatchWarningEventCount: Int
  let dispatchCriticalEventCount: Int
  let recordingFailure: String?
}

private func monotonicNanoseconds() -> UInt64 {
  var time = timespec()
  guard clock_gettime(CLOCK_MONOTONIC_RAW, &time) == 0 else {
    return DispatchTime.now().uptimeNanoseconds
  }
  return UInt64(time.tv_sec) * 1_000_000_000 + UInt64(time.tv_nsec)
}

private func printSummary(_ snapshot: RecordingSnapshot, label: String) {
  let delayMilliseconds =
    Double(
      snapshot.summary.maxSchedulerDelayNanoseconds
    ) / 1_000_000
  let message = String(
    format:
      "Pressure probe %@: %d samples, max scheduler delay %.2f ms, %d warning events, %d critical events\n",
    label,
    snapshot.summary.sampleCount,
    delayMilliseconds,
    snapshot.dispatchWarningEventCount,
    snapshot.dispatchCriticalEventCount
  )
  fputs(message, stderr)
}
