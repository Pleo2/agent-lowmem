import Foundation

public enum ProbeConfigurationError: Error, Equatable, CustomStringConvertible {
  case helpRequested
  case invalidArguments
  case invalidLabel

  public var description: String {
    switch self {
    case .helpRequested:
      ProbeConfiguration.usage
    case .invalidArguments:
      "invalid arguments\n\n\(ProbeConfiguration.usage)"
    case .invalidLabel:
      "label must contain 1-64 ASCII letters, digits, dots, underscores, or hyphens"
    }
  }
}

public struct CommandMetadata: Codable, Equatable, Sendable {
  public let executable: String
  public let argumentCount: Int
}

public struct ProbeConfiguration: Equatable, Sendable {
  public static let usage = """
    Usage:
      agent-lowmem-pressure-probe --output PATH --label SAFE_LABEL -- COMMAND [ARG ...]

    The probe observes a real command and never terminates it because of pressure.
    """

  public let outputURL: URL
  public let label: String
  public let command: [String]
  public let commandMetadata: CommandMetadata

  public static func parse(arguments: [String]) throws -> ProbeConfiguration {
    if arguments == ["--help"] || arguments == ["-h"] {
      throw ProbeConfigurationError.helpRequested
    }

    guard let delimiterIndex = arguments.firstIndex(of: "--") else {
      throw ProbeConfigurationError.invalidArguments
    }

    let optionArguments = arguments[..<delimiterIndex]
    let command = Array(arguments[arguments.index(after: delimiterIndex)...])
    guard !command.isEmpty, !command[0].isEmpty else {
      throw ProbeConfigurationError.invalidArguments
    }

    var outputPath: String?
    var label: String?
    var index = optionArguments.startIndex

    while index < optionArguments.endIndex {
      let option = optionArguments[index]
      let valueIndex = optionArguments.index(after: index)
      guard valueIndex < optionArguments.endIndex else {
        throw ProbeConfigurationError.invalidArguments
      }

      let value = optionArguments[valueIndex]
      switch option {
      case "--output" where outputPath == nil:
        outputPath = value
      case "--label" where label == nil:
        label = value
      default:
        throw ProbeConfigurationError.invalidArguments
      }
      index = optionArguments.index(after: valueIndex)
    }

    guard let outputPath, !outputPath.isEmpty, let label else {
      throw ProbeConfigurationError.invalidArguments
    }
    guard isSafeLabel(label) else {
      throw ProbeConfigurationError.invalidLabel
    }

    return ProbeConfiguration(
      outputURL: URL(fileURLWithPath: outputPath),
      label: label,
      command: command,
      commandMetadata: CommandMetadata(
        executable: URL(fileURLWithPath: command[0]).lastPathComponent,
        argumentCount: command.count - 1
      )
    )
  }
}

private func isSafeLabel(_ label: String) -> Bool {
  guard (1...64).contains(label.utf8.count) else {
    return false
  }

  return label.utf8.allSatisfy { byte in
    switch byte {
    case 45, 46, 48...57, 65...90, 95, 97...122:
      true
    default:
      false
    }
  }
}
