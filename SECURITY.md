# Security policy

## Supported versions

Security fixes are provided for the newest released version of Agent Lowmem. Pre-release source snapshots and older versions may receive a fix at the maintainer's discretion but are not supported security release lines.

The v0.1.0 distribution target is ARM64 Apple Silicon on macOS 14 or newer. Release binaries are not signed or notarized while the project has no Apple Developer ID; users must verify the published SHA-256 checksum before running a downloaded artifact.

## Reporting a vulnerability

Please privately report a vulnerability through [GitHub private vulnerability reporting](https://github.com/Pleo2/agent-lowmem/security/advisories/new). If that channel is unavailable, email [support@agentlowmem.dev](mailto:support@agentlowmem.dev) with a minimal description and a safe way to reproduce the issue.

Do not open a public issue or include secrets, access tokens, environment values, usernames, absolute local paths, or unrelated machine data. Maintainers aim to acknowledge a report within seven days and provide an initial assessment within fourteen days.

Agent Lowmem runs local development commands and supervises owned process groups. A report is especially useful when it identifies an ownership-boundary bypass, unsafe signal delivery, shell injection, secret exposure, path traversal, rollback failure, or artifact-integrity issue.
