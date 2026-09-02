import Darwin
import Foundation

import PressureProbeCore

public enum SystemMetricsError: Error, CustomStringConvertible {
    case sysctlUnavailable(name: String, code: Int32)
    case unexpectedSize(name: String, expected: Int, actual: Int)
    case invalidString(name: String)
    case processEnumerationFailed
    case processFootprintUnavailable
    case processFootprintOverflow

    public var description: String {
        switch self {
        case let .sysctlUnavailable(name, code):
            "sysctl \(name) is unavailable (errno \(code))"
        case let .unexpectedSize(name, expected, actual):
            "sysctl \(name) returned \(actual) bytes; expected \(expected)"
        case let .invalidString(name):
            "sysctl \(name) returned invalid UTF-8"
        case .processEnumerationFailed:
            "process enumeration failed"
        case .processFootprintUnavailable:
            "no process-tree footprint was readable"
        case .processFootprintOverflow:
            "process-tree footprint overflowed UInt64"
        }
    }
}

public struct HostMetadata: Codable, Equatable, Sendable {
    public let osProductVersion: String
    public let cpuBrand: String
    public let memoryBytes: UInt64
    public let pageSizeBytes: UInt64
}

public struct PressureReading: Codable, Equatable, Sendable {
    public let rawValue: Int32
    public let level: PressureLevel
}

public struct SwapUsage: Codable, Equatable, Sendable {
    public let totalBytes: UInt64
    public let usedBytes: UInt64
    public let freeBytes: UInt64
}

public struct ProcessTreeFootprint: Codable, Equatable, Sendable {
    public let physicalFootprintBytes: UInt64
    public let processCount: Int
}

public struct SystemMetrics: Sendable {
    public init() {}

    public func hostMetadata() throws -> HostMetadata {
        HostMetadata(
            osProductVersion: try readString(name: "kern.osproductversion"),
            cpuBrand: try readString(name: "machdep.cpu.brand_string"),
            memoryBytes: try readInteger(name: "hw.memsize", as: UInt64.self),
            pageSizeBytes: try readInteger(name: "hw.pagesize", as: UInt64.self)
        )
    }

    public func currentPressure() throws -> PressureReading {
        let raw = try readInteger(
            name: "kern.memorystatus_vm_pressure_level",
            as: Int32.self
        )
        return PressureReading(rawValue: raw, level: PressureLevel(sysctlRaw: raw))
    }

    public func swapUsage() throws -> SwapUsage {
        var usage = xsw_usage()
        var size = MemoryLayout<xsw_usage>.size
        let result = "vm.swapusage".withCString { name in
            sysctlbyname(name, &usage, &size, nil, 0)
        }

        guard result == 0 else {
            throw SystemMetricsError.sysctlUnavailable(
                name: "vm.swapusage",
                code: errno
            )
        }
        guard size == MemoryLayout<xsw_usage>.size else {
            throw SystemMetricsError.unexpectedSize(
                name: "vm.swapusage",
                expected: MemoryLayout<xsw_usage>.size,
                actual: size
            )
        }

        return SwapUsage(
            totalBytes: usage.xsu_total,
            usedBytes: usage.xsu_used,
            freeBytes: usage.xsu_avail
        )
    }

    public func processTreeFootprint(rootPID: pid_t) throws -> ProcessTreeFootprint {
        let processIDs = try listAllProcessIDs()
        let parentByProcess = readParentMap(processIDs: processIDs)
        let selected = descendants(
            rootPID: rootPID,
            processIDs: processIDs,
            parentByProcess: parentByProcess
        )

        var total: UInt64 = 0
        var measuredCount = 0

        for processID in selected {
            guard let footprint = physicalFootprint(processID: processID) else {
                continue
            }
            let (sum, overflow) = total.addingReportingOverflow(footprint)
            guard !overflow else {
                throw SystemMetricsError.processFootprintOverflow
            }
            total = sum
            measuredCount += 1
        }

        guard measuredCount > 0 else {
            throw SystemMetricsError.processFootprintUnavailable
        }

        return ProcessTreeFootprint(
            physicalFootprintBytes: total,
            processCount: measuredCount
        )
    }
}

private func readInteger<T: FixedWidthInteger>(
    name: String,
    as _: T.Type
) throws -> T {
    let data = try readSysctlData(name: name)
    guard data.count == MemoryLayout<T>.size else {
        throw SystemMetricsError.unexpectedSize(
            name: name,
            expected: MemoryLayout<T>.size,
            actual: data.count
        )
    }
    return data.withUnsafeBytes { buffer in
        buffer.loadUnaligned(as: T.self)
    }
}

private func readString(name: String) throws -> String {
    var data = try readSysctlData(name: name)
    if data.last == 0 {
        data.removeLast()
    }
    guard let value = String(data: data, encoding: .utf8) else {
        throw SystemMetricsError.invalidString(name: name)
    }
    return value
}

private func readSysctlData(name: String) throws -> Data {
    var size = 0
    let sizeResult = name.withCString { pointer in
        sysctlbyname(pointer, nil, &size, nil, 0)
    }
    guard sizeResult == 0 else {
        throw SystemMetricsError.sysctlUnavailable(name: name, code: errno)
    }

    var data = Data(count: size)
    let readResult = data.withUnsafeMutableBytes { buffer in
        name.withCString { pointer in
            sysctlbyname(pointer, buffer.baseAddress, &size, nil, 0)
        }
    }
    guard readResult == 0 else {
        throw SystemMetricsError.sysctlUnavailable(name: name, code: errno)
    }
    guard size == data.count else {
        throw SystemMetricsError.unexpectedSize(
            name: name,
            expected: data.count,
            actual: size
        )
    }
    return data
}

private func listAllProcessIDs() throws -> [pid_t] {
    let estimatedCount = proc_listallpids(nil, 0)
    guard estimatedCount > 0 else {
        throw SystemMetricsError.processEnumerationFailed
    }

    var capacity = Int(estimatedCount) + 128
    for _ in 0..<3 {
        var processIDs = [pid_t](repeating: 0, count: capacity)
        let count = processIDs.withUnsafeMutableBytes { buffer in
            proc_listallpids(buffer.baseAddress, Int32(buffer.count))
        }
        guard count >= 0 else {
            throw SystemMetricsError.processEnumerationFailed
        }
        if Int(count) < capacity {
            return Array(processIDs.prefix(Int(count))).filter { $0 > 0 }
        }
        capacity *= 2
    }

    throw SystemMetricsError.processEnumerationFailed
}

private func readParentMap(processIDs: [pid_t]) -> [pid_t: pid_t] {
    var parents: [pid_t: pid_t] = [:]
    parents.reserveCapacity(processIDs.count)

    for processID in processIDs {
        var info = proc_bsdinfo()
        let bytesRead = withUnsafeMutablePointer(to: &info) { pointer in
            proc_pidinfo(
                processID,
                PROC_PIDTBSDINFO,
                0,
                UnsafeMutableRawPointer(pointer),
                Int32(MemoryLayout<proc_bsdinfo>.size)
            )
        }
        guard bytesRead == MemoryLayout<proc_bsdinfo>.size else {
            continue
        }
        parents[processID] = pid_t(info.pbi_ppid)
    }

    return parents
}

private func descendants(
    rootPID: pid_t,
    processIDs: [pid_t],
    parentByProcess: [pid_t: pid_t]
) -> [pid_t] {
    var childrenByParent: [pid_t: [pid_t]] = [:]
    for processID in processIDs {
        guard let parent = parentByProcess[processID] else {
            continue
        }
        childrenByParent[parent, default: []].append(processID)
    }

    var selected: [pid_t] = []
    var pending = [rootPID]
    var visited: Set<pid_t> = []

    while let processID = pending.popLast() {
        guard visited.insert(processID).inserted else {
            continue
        }
        selected.append(processID)
        pending.append(contentsOf: childrenByParent[processID] ?? [])
    }

    return selected
}

private func physicalFootprint(processID: pid_t) -> UInt64? {
    var usage = rusage_info_v4()
    let result = withUnsafeMutablePointer(to: &usage) { pointer in
        pointer.withMemoryRebound(
            to: Optional<UnsafeMutableRawPointer>.self,
            capacity: 1
        ) { rebound in
            proc_pid_rusage(processID, RUSAGE_INFO_V4, rebound)
        }
    }
    return result == 0 ? usage.ri_phys_footprint : nil
}
