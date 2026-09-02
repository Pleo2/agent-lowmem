public enum PressureLevel: String, Codable, Equatable, Sendable {
    case normal
    case warning
    case critical
    case unknown

    public init(sysctlRaw: Int32) {
        self = switch sysctlRaw {
        case 1: .normal
        case 2: .warning
        case 4: .critical
        default: .unknown
        }
    }

    public static func from(dispatchMask: UInt) -> [PressureLevel] {
        var levels: [PressureLevel] = []

        if dispatchMask & 0x01 != 0 {
            levels.append(.normal)
        }
        if dispatchMask & 0x02 != 0 {
            levels.append(.warning)
        }
        if dispatchMask & 0x04 != 0 {
            levels.append(.critical)
        }

        return levels.isEmpty ? [.unknown] : levels
    }
}
