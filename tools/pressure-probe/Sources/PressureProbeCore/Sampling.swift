public struct SamplingTick: Equatable, Sendable {
  public let expectedNanoseconds: UInt64
  public let delayNanoseconds: UInt64
  public let coalescedIntervals: UInt64
}

public struct SamplingSchedule: Sendable {
  private let intervalNanoseconds: UInt64
  private var nextExpectedNanoseconds: UInt64

  public init(startNanoseconds: UInt64, intervalNanoseconds: UInt64) {
    precondition(intervalNanoseconds > 0)
    self.intervalNanoseconds = intervalNanoseconds
    nextExpectedNanoseconds = startNanoseconds + intervalNanoseconds
  }

  public mutating func consume(nowNanoseconds: UInt64) -> SamplingTick {
    let expected = nextExpectedNanoseconds
    let delay = nowNanoseconds > expected ? nowNanoseconds - expected : 0
    let coalescedIntervals = delay / intervalNanoseconds
    nextExpectedNanoseconds =
      expected
      + ((coalescedIntervals + 1) * intervalNanoseconds)

    return SamplingTick(
      expectedNanoseconds: expected,
      delayNanoseconds: delay,
      coalescedIntervals: coalescedIntervals
    )
  }
}

public struct SamplePayload: Codable, Equatable, Sendable {
  public let pressureRaw: Int32?
  public let pressure: PressureLevel
  public let schedulerDelayNanoseconds: UInt64
  public let coalescedIntervals: UInt64
  public let swapUsedBytes: UInt64?
  public let processTreeFootprintBytes: UInt64?
  public let processCount: Int?
  public let measurementErrors: [String]

  public init(
    pressureRaw: Int32?,
    pressure: PressureLevel,
    schedulerDelayNanoseconds: UInt64,
    coalescedIntervals: UInt64,
    swapUsedBytes: UInt64?,
    processTreeFootprintBytes: UInt64?,
    processCount: Int?,
    measurementErrors: [String]
  ) {
    self.pressureRaw = pressureRaw
    self.pressure = pressure
    self.schedulerDelayNanoseconds = schedulerDelayNanoseconds
    self.coalescedIntervals = coalescedIntervals
    self.swapUsedBytes = swapUsedBytes
    self.processTreeFootprintBytes = processTreeFootprintBytes
    self.processCount = processCount
    self.measurementErrors = measurementErrors
  }
}

public struct RunSummary: Codable, Equatable, Sendable {
  public private(set) var sampleCount = 0
  public private(set) var warningSampleCount = 0
  public private(set) var criticalSampleCount = 0
  public private(set) var unknownSampleCount = 0
  public private(set) var measurementErrorCount = 0
  public private(set) var maxSchedulerDelayNanoseconds: UInt64 = 0
  public private(set) var maxSwapUsedBytes: UInt64?
  public private(set) var maxProcessTreeFootprintBytes: UInt64?

  public init() {}

  public mutating func observe(sample: SamplePayload) {
    sampleCount += 1
    maxSchedulerDelayNanoseconds = max(
      maxSchedulerDelayNanoseconds,
      sample.schedulerDelayNanoseconds
    )
    maxSwapUsedBytes = maximum(maxSwapUsedBytes, sample.swapUsedBytes)
    maxProcessTreeFootprintBytes = maximum(
      maxProcessTreeFootprintBytes,
      sample.processTreeFootprintBytes
    )
    measurementErrorCount += sample.measurementErrors.count

    switch sample.pressure {
    case .normal:
      break
    case .warning:
      warningSampleCount += 1
    case .critical:
      criticalSampleCount += 1
    case .unknown:
      unknownSampleCount += 1
    }
  }
}

private func maximum(_ lhs: UInt64?, _ rhs: UInt64?) -> UInt64? {
  switch (lhs, rhs) {
  case (.some(let left), .some(let right)):
    max(left, right)
  case (.some(let left), .none):
    left
  case (.none, .some(let right)):
    right
  case (.none, .none):
    nil
  }
}
