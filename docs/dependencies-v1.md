# Agent Lowmem v1 Dependency Record

| Requirement | Purpose | Production boundary | License |
| --- | --- | --- | --- |
| `serde 1.0` | Derive stable structured records | Serialization only | MIT OR Apache-2.0 |
| `serde_json 1.0` | Parse manifests and emit JSON | No script evaluation | MIT OR Apache-2.0 |
| `semver 1.0` | Validate declared package-manager versions | Data-only parsing | MIT OR Apache-2.0 |
| `sysctl 0.7.1` | Safe macOS sysctl reads | Read-only host inspector | MIT |

`Cargo.lock` is the authority for exact resolved versions. Every direct dependency addition requires purpose, source/API review, license review, one-worker tests, and a separate commit. Production dependencies may not add a network client, async runtime, daemon, shell evaluator, or lifecycle installer.

## Phase 1 Development Baseline — 2026-09-02

| Field | Evidence |
| --- | --- |
| Host key | `darwin/arm64`; macOS `26.6.2`; `Mac14,15`; `Apple M2`; `8589934592` physical-memory bytes; `16384` page-size bytes |
| Rust | `rustc 1.85.0 (4d91de4e4 2025-02-17)` |
| Commit under test | `3307f75` |
| Release binary | `402400` bytes |
| Maximum resident set size | `1540096` bytes |
| Warm-cache doctor timing | 20 recorded runs; median `2.460 ms`; p95 `5.541 ms` |
| Gate commands | `cargo fmt --all -- --check`<br>`cargo clippy --workspace --all-targets -j 1 -- -D warnings`<br>`cargo test --workspace -j 1 -- --test-threads=1`<br>`cargo build --release -p agent-lowmem -j 1`<br>`cargo test --release -p agent-lowmem --test doctor_budget -j 1 -- --ignored --test-threads=1 --nocapture`<br>`stat -f '%z bytes' target/release/agent-lowmem`<br>`/usr/bin/time -l target/release/agent-lowmem doctor >/dev/null`<br>`git diff --check` |

These are development measurements on the reference Mac, not a release or portability claim.
