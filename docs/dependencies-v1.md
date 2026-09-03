# Agent Lowmem v1 Dependency Record

| Requirement | Purpose | Production boundary | License |
| --- | --- | --- | --- |
| `rustix 1.1.4` (`std`, `fs`, `process`) | Safe component-relative no-follow reads, advisory lock, effective UID, and process-group probes | Filesystem and exact process/group identities only; no process enumeration | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| `libproc 0.14.11` (macOS only) | Read one PID's kernel process-start absolute time | Only `pidrusage::<RUsageInfoV4>(pid)`; process listing and command/path APIs are excluded | MIT |
| `serde 1.0` | Derive stable structured records | Serialization only | MIT OR Apache-2.0 |
| `serde_json 1.0` | Parse manifests and emit JSON | No script evaluation | MIT OR Apache-2.0 |
| `semver 1.0` | Validate declared package-manager versions | Data-only parsing | MIT OR Apache-2.0 |
| `sha2 0.11.0` (no default features) | SHA-256 over exact repository evidence bytes | Pure-Rust hashing only; no network or secret storage | MIT OR Apache-2.0 |
| `signal-hook 0.4.4` (`iterator` only) | Blocking delivery of `SIGINT`, `SIGTERM`, and `SIGHUP` to one listener thread | Safe iterator and close handle only; no async adapter, extended signal info, or arbitrary callback | MIT OR Apache-2.0 |
| `sysctl 0.7.1` | Safe macOS sysctl reads | Read-only host inspector | MIT |

`Cargo.lock` is the authority for exact resolved versions. Every direct dependency addition requires purpose, source/API review, license review, one-worker tests, and a separate commit. Production dependencies may not add a network client, async runtime, daemon, shell evaluator, or lifecycle installer.

`sha2 0.11.0` declares Rust 1.85 and adds only the RustCrypto digest stack with default allocation and OID features disabled. `rustix 1.1.4` declares Rust 1.63; this checkpoint enables `std`, `fs`, and `process` so evidence paths can be opened below a directory descriptor with `O_NOFOLLOW`, the effective UID can be checked, and a single process group can be probed without enumeration.

`libproc 0.14.11` declares Rust 1.72 and is scoped to macOS. Its production dependency delta is `errno` and the already-present `libc`; its build uses `bindgen 0.72.1` with the runtime feature and therefore adds build-only Clang/loading dependencies. Agent Lowmem calls only the safe one-PID `pidrusage::<RUsageInfoV4>` API and reads `ri_proc_start_abstime`; broad process, path, file-descriptor, and command inspection APIs are outside the boundary.

`signal-hook 0.4.4` declares Rust 1.66. Default features are disabled and only the synchronous `iterator` feature is enabled; its production delta is `signal-hook-registry` plus the already-present `libc`. The iterator uses a self-pipe, may coalesce repeated non-realtime signals, and its close handle wakes a blocking listener so the thread can be joined. Agent Lowmem installs only `SIGINT`, `SIGTERM`, and `SIGHUP`; after cleanup it uses the crate's safe `low_level::emulate_default_handler` helper to restore the shell-observable signal outcome. It does not use async adapters, extended signal metadata, or first-party unsafe registration. The dependency-only and Task 5 library-boundary stripped releases both remained `650624` bytes because the runner was not yet reachable from the CLI binary; the final linked delta is measured at the Phase 3 gate.

Task 7 adds no dependency. The planned `time 0.3.44` formatter was rejected because it is covered by RUSTSEC-2026-0009; the patched `time 0.3.47` requires Rust 1.88 and therefore exceeds the project's Rust 1.85 MSRV. Result timestamps use `std::time::SystemTime` plus a bounded, dependency-free UTC civil-date conversion with fixed epoch, leap-day, and maximum-year tests.

## Phase 1 Development Baseline — 2026-09-02

| Field | Evidence |
| --- | --- |
| Host key | `darwin/arm64`; macOS `26.6.2`; `Mac14,15`; `Apple M2`; `8589934592` physical-memory bytes; `16384` page-size bytes |
| Rust | `rustc 1.85.0 (4d91de4e4 2025-02-17)` |
| Commit under test | `05bf7ec` |
| Release binary | `402400` bytes |
| Maximum resident set size | `1540096` bytes |
| Warm-cache doctor timing | 20 recorded runs; median `2.343 ms`; p95 `3.440 ms` |
| Gate commands | `cargo fmt --all -- --check`<br>`cargo clippy --all-targets -j 1 -- -D warnings`<br>`cargo test -j 1 -- --test-threads=1`<br>`cargo build --release -j 1`<br>`cargo test --release --test doctor_budget -j 1 -- --ignored --test-threads=1 --nocapture`<br>`stat -f '%z bytes' target/release/agent-lowmem`<br>`/usr/bin/time -l target/release/agent-lowmem doctor >/dev/null`<br>`git diff --check` |

These are development measurements on the reference Mac, not a release or portability claim.

## Phase 2 Repository Policy Gate — 2026-09-03

| Field | Evidence |
| --- | --- |
| Host key | `darwin/arm64`; macOS `26.6.2`; `Mac14,15`; `Apple M2`; `8589934592` physical-memory bytes; `16384` page-size bytes |
| Rust | `rustc 1.85.0 (4d91de4e4 2025-02-17)` |
| Implementation HEAD under test | `8ef4af69bf6829dccc623a46d4b6fb385ff670b3` |
| Package managers | npm `12.0.2`; pnpm `11.25.0` |
| Adapter snapshot | Vitest `4.1.11`; Jest `30.5.1`; Node `24.14.1`; TypeScript `7.0.2`; ESLint `10.9.1`; Next.js `16.3.4`; `@nestjs/cli` `12.0.0`; `cross-env` `10.1.0`; `dotenv-cli` `11.0.0`; `rimraf` `6.1.3` |
| Tests | `94` active passed: `82` unit, `6` doctor CLI integration, `6` repository-policy integration; `2` release-only budget tests passed separately |
| Release binary | `650624` bytes; limit `12582912` bytes |
| Maximum resident set size | `1605632` bytes; limit `25165824` bytes |
| Outside-repository warm-cache timing | 20 recorded runs; median `2.077 ms`; p95 `4.763 ms`; median limit `100 ms` |
| npm single-package warm-cache timing | 20 recorded runs; median `2.095 ms`; p95 `2.354 ms`; limits `300/500 ms` |
| Rust gate commands | `cargo fmt --all -- --check`<br>`cargo clippy --all-targets -j 1 -- -D warnings`<br>`cargo test -j 1 -- --test-threads=1`<br>`cargo build --release -j 1`<br>`cargo test --release --test doctor_budget -j 1 -- --ignored --test-threads=1 --nocapture` |
| Resource commands | `stat -f '%z bytes' target/release/agent-lowmem`<br>`/usr/bin/time -l target/release/agent-lowmem doctor >/dev/null` |
| Boundary audit | `rg -n 'std::process::Command\|Command::new\|node --version\|npm config\|pnpm config\|kern\.memorystatus_vm_pressure_level\|NODE_OPTIONS' src` returned no matches; `rg -n 'tokio\|async-std\|reqwest\|ureq\|hyper' Cargo.toml Cargo.lock` returned no matches |
| Deferred command evidence | `run test` returned `64` with `operation-unsupported`; `init` returned `2` with `invalid-cli` |

These are development measurements on the reference Mac, not a release, distribution, or portability claim. Phase 2 starts no repository child and does not enable managed runs.
