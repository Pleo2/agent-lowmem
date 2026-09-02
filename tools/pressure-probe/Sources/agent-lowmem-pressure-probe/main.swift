import Darwin
import Foundation
import PressureProbeCore
import PressureProbeMacOS

let arguments = Array(CommandLine.arguments.dropFirst())

do {
  let configuration = try ProbeConfiguration.parse(arguments: arguments)
  let status = try ProbeRunner().run(configuration: configuration)
  exit(status)
} catch ProbeConfigurationError.helpRequested {
  print(ProbeConfiguration.usage)
  exit(0)
} catch let error as ProbeConfigurationError {
  fputs("agent-lowmem-pressure-probe: \(error)\n", stderr)
  exit(64)
} catch let error as JSONLineWriterError {
  fputs("agent-lowmem-pressure-probe: \(error)\n", stderr)
  exit(73)
} catch let error as SystemMetricsError {
  fputs("agent-lowmem-pressure-probe: \(error)\n", stderr)
  exit(69)
} catch let error as ProbeRunnerError {
  fputs("agent-lowmem-pressure-probe: \(error)\n", stderr)
  exit(70)
} catch {
  fputs("agent-lowmem-pressure-probe: unexpected failure: \(error)\n", stderr)
  exit(70)
}
