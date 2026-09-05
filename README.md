<p align="center">
  <a href="https://agentlowmem.dev"><img src="assets/agent-lowmem.svg" width="88" alt="Agent Lowmem"></a>
</p>

<h1 align="center">Agent Lowmem</h1>

<p align="center">More agents. Less RAM.</p>

Agent Lowmem is a native Rust policy runner for predictable agent-launched validation on memory-constrained Apple Silicon Macs. It serializes heavy work, denies watch and background modes, supervises owned process groups, and keeps adoption reversible.

> Agent Lowmem `v0.1.0` is available for ARM64 Apple Silicon Macs running macOS 14 or newer.

The first release is not signed or notarized because an Apple Developer ID is not available.

## Install with Homebrew

Install the fully qualified formula so Homebrew trusts only Agent Lowmem from the Pleo2 tap:

```sh
brew install Pleo2/agent-lowmem/agent-lowmem
```

Upgrade or uninstall it with:

```sh
brew update
brew upgrade Pleo2/agent-lowmem/agent-lowmem
brew uninstall Pleo2/agent-lowmem/agent-lowmem
```

The Homebrew package uses the same unsigned and not notarized binary published in the immutable GitHub Release. Homebrew verifies its pinned SHA-256 before installation. Never disable Gatekeeper globally.

## Current commands

New here? Follow the [10-minute end-to-end test](docs/quickstart.md) in a disposable project before adopting a real repository.

```sh
agent-lowmem doctor
agent-lowmem --version
agent-lowmem init --dry-run
agent-lowmem init
agent-lowmem run test
agent-lowmem restore --dry-run
agent-lowmem restore
```

Every managed run uses a strict repository policy, one global heavy-operation lock, a visible deadline, signal forwarding, and ownership-checked cleanup.

## GitHub integration

Inspect the current repository's GitHub Actions configuration through the official GitHub CLI:

```sh
gh auth status
agent-lowmem github inspect
agent-lowmem github inspect --json
```

The integration accepts only a canonical `github.com` origin, invokes `gh api` directly without a shell, requests at most 100 workflows, caps captured output at 256 KiB, and terminates the request after 10 seconds. Agent Lowmem never reads or prints the GitHub token. The machine-readable report follows [`schemas/github-inspect-v1.schema.json`](schemas/github-inspect-v1.schema.json).

## Scope

- macOS on Apple Silicon;
- reference profile: M2 MacBook Air with 8 GiB RAM;
- JavaScript and TypeScript repositories using npm or pnpm;
- Node, Next.js, NestJS, Vitest, Jest, TypeScript, and ESLint operations admitted by the embedded policy matrix;
- no daemon, `sudo`, shell evaluation, private memory-pressure API, or hard memory-cap promise.

## Development

Use sequential, resource-bounded validation:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 -- --test-threads=1
cargo build --release -j 1
```

The production Rust package is at the repository root. The Swift package under `tools/pressure-probe` is an experimental measurement tool, not the production implementation.

## Release verification

Release archives are published with `SHA256SUMS`. After downloading both files from the GitHub release, verify the archive before extracting it:

```sh
shasum -a 256 -c SHA256SUMS
tar -tzf agent-lowmem-v0.1.0-aarch64-apple-darwin.tar.gz
```

The archive is expected to contain only `agent-lowmem`, `LICENSE.md`, and `README.md`. Until signing and notarization are available, macOS may quarantine the downloaded binary. Inspect that state explicitly and remove quarantine only from the verified Agent Lowmem binary you downloaded:

```sh
xattr -l ./agent-lowmem
xattr -d com.apple.quarantine ./agent-lowmem
./agent-lowmem --version
./agent-lowmem doctor
```

Never disable Gatekeeper globally. See [SECURITY.md](SECURITY.md) before reporting a vulnerability.

## License and contributions

Agent Lowmem uses the [FSL-1.1-MIT license](LICENSE.md), with `Copyright 2026 Jose Leonardo Moreno`. It is source-available during the initial two-year period and each version converts to MIT on its own second anniversary. See [COMMERCIAL.md](COMMERCIAL.md) for plain-language examples and [CONTRIBUTING.md](CONTRIBUTING.md) to contribute.

## Support

Email [support@agentlowmem.dev](mailto:support@agentlowmem.dev).
