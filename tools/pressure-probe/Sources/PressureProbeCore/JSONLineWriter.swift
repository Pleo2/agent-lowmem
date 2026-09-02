import Darwin
import Foundation

public enum JSONLineWriterError: Error, CustomStringConvertible {
    case outputExists
    case invalidPath
    case closed
    case encoding(String)
    case systemCall(operation: String, code: Int32)

    public var description: String {
        switch self {
        case .outputExists:
            "output already exists; refusing to overwrite it"
        case .invalidPath:
            "output path cannot be represented by the file system"
        case .closed:
            "evidence writer is already closed"
        case let .encoding(message):
            "JSON encoding failed: \(message)"
        case let .systemCall(operation, code):
            "\(operation) failed with errno \(code)"
        }
    }
}

public final class JSONLineWriter: @unchecked Sendable {
    private let descriptor: Int32
    private let encoder: JSONEncoder
    private let lock = NSLock()
    private var isClosed = false

    private init(descriptor: Int32) {
        self.descriptor = descriptor
        encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
    }

    deinit {
        lock.lock()
        if !isClosed {
            _ = Darwin.close(descriptor)
            isClosed = true
        }
        lock.unlock()
    }

    public static func create(at url: URL) throws -> JSONLineWriter {
        guard url.isFileURL else {
            throw JSONLineWriterError.invalidPath
        }

        let descriptor = url.withUnsafeFileSystemRepresentation { path -> Int32 in
            guard let path else {
                return -1
            }
            return Darwin.open(
                path,
                O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC,
                mode_t(S_IRUSR | S_IWUSR)
            )
        }

        guard descriptor >= 0 else {
            let code = errno
            if code == EEXIST {
                throw JSONLineWriterError.outputExists
            }
            if code == 0 {
                throw JSONLineWriterError.invalidPath
            }
            throw JSONLineWriterError.systemCall(operation: "open", code: code)
        }

        guard fchmod(descriptor, mode_t(S_IRUSR | S_IWUSR)) == 0 else {
            let code = errno
            _ = Darwin.close(descriptor)
            throw JSONLineWriterError.systemCall(operation: "fchmod", code: code)
        }

        return JSONLineWriter(descriptor: descriptor)
    }

    public func append<Payload: Encodable>(
        event: String,
        monotonicNanoseconds: UInt64,
        wallTime: String,
        payload: Payload
    ) throws {
        lock.lock()
        defer { lock.unlock() }

        guard !isClosed else {
            throw JSONLineWriterError.closed
        }

        let envelope = RecordEnvelope(
            schemaVersion: 1,
            event: event,
            monotonicNanoseconds: monotonicNanoseconds,
            wallTime: wallTime,
            data: payload
        )

        let encoded: Data
        do {
            encoded = try encoder.encode(envelope)
        } catch {
            throw JSONLineWriterError.encoding(String(describing: error))
        }

        var line = encoded
        line.append(0x0A)
        try writeAll(line)
    }

    public func close() throws {
        lock.lock()
        defer { lock.unlock() }

        guard !isClosed else {
            return
        }

        let synchronizationResult = fsync(descriptor)
        let synchronizationError = errno
        let closeResult = Darwin.close(descriptor)
        let closeError = errno
        isClosed = true

        if synchronizationResult != 0 {
            throw JSONLineWriterError.systemCall(
                operation: "fsync",
                code: synchronizationError
            )
        }
        if closeResult != 0 {
            throw JSONLineWriterError.systemCall(operation: "close", code: closeError)
        }
    }

    private func writeAll(_ data: Data) throws {
        try data.withUnsafeBytes { rawBuffer in
            guard let baseAddress = rawBuffer.baseAddress else {
                return
            }

            var written = 0
            while written < rawBuffer.count {
                let result = Darwin.write(
                    descriptor,
                    baseAddress.advanced(by: written),
                    rawBuffer.count - written
                )
                if result > 0 {
                    written += result
                } else if result == -1, errno == EINTR {
                    continue
                } else {
                    throw JSONLineWriterError.systemCall(
                        operation: "write",
                        code: errno
                    )
                }
            }
        }
    }
}

private struct RecordEnvelope<Payload: Encodable>: Encodable {
    let schemaVersion: Int
    let event: String
    let monotonicNanoseconds: UInt64
    let wallTime: String
    let data: Payload
}
