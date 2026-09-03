<p align="center">
  <a href="https://agentlowmem.dev"><img src="assets/agent-lowmem.svg" width="88" alt="Agent Lowmem"></a>
</p>

<h1 align="center">Agent Lowmem</h1>

<p align="center">More agents. Less RAM.</p>

Agent Lowmem is a native Rust policy runner for predictable agent-launched validation on memory-constrained Apple Silicon Macs. It serializes heavy work, denies watch and background modes, supervises owned process groups, and keeps adoption reversible.

> Agent Lowmem is in early development. There is no supported public release yet.

## Current commands

```sh
agent-lowmem doctor
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

## Support

Email [support@agentlowmem.dev](mailto:support@agentlowmem.dev).
