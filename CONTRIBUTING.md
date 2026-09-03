# Contributing to Agent Lowmem

Thank you for improving Agent Lowmem. The project welcomes focused fixes, documentation, tests, and small features that preserve its native, no-sudo, resource-efficient, and reversible design.

## Before opening work

- Search existing issues and keep each proposal narrowly scoped.
- Use an issue before a large behavioral or architectural change.
- Never include tokens, credentials, environment values, usernames, or absolute local paths in issues, logs, fixtures, or commits.
- Read [SECURITY.md](SECURITY.md) and privately report a vulnerability instead of opening a public issue.

Maintainers aim to acknowledge a well-formed issue or pull request within seven days and provide an initial review or next decision within fourteen days. These are response goals, not service-level guarantees.

## Development

Agent Lowmem requires Rust 1.85 or newer. Keep validation sequential so development remains usable on low-memory Macs:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 -- --test-threads=1
cargo build --release -j 1
```

Add a failing test before changing behavior, then implement the smallest complete fix. Do not add daemons, `sudo`, shell evaluation, unbounded discovery, or background/watch execution.

## Commits and pull requests

- Use Conventional Commits, such as `fix: preserve child exit status`.
- Sign off every commit under the [Developer Certificate of Origin 1.1](https://developercertificate.org/) with `git commit -s`.
- Keep the pull request focused and describe tests, documentation changes, resource impact, and privacy impact.
- Update `CHANGELOG.md` when a user-visible behavior changes.

By signing off a commit, you certify that you have the right to submit the contribution under the project's FSL-1.1-MIT license. Contributors retain copyright in their contributions; the project will recognize external contributors in release notes without excluding dependency contributions.

## Conduct and support

Participation is governed by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). For ordinary project support, email [support@agentlowmem.dev](mailto:support@agentlowmem.dev).
