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

## Phase 3 Managed Runner Gate — 2026-09-03

| Field | Evidence |
| --- | --- |
| Host key | `darwin/arm64`; macOS `26.6.2`; `Mac14,15`; `Apple M2`; `8589934592` physical-memory bytes; `16384` page-size bytes |
| Rust | `rustc 1.85.0 (4d91de4e4 2025-02-17)`; Cargo `1.85.0 (d73d2caf9 2024-12-31)` |
| Implementation HEAD under test | `1523ed6119cc41a7a559207eec2c1fc567734b79` |
| Direct resolved dependencies | `rustix 1.1.4`; `semver 1.0.28`; `serde 1.0.229`; `serde_json 1.0.151`; `sha2 0.11.0`; `signal-hook 0.4.4`; `sysctl 0.7.1`; macOS-only `libproc 0.14.11` |
| Tests | `160` active passed: `120` unit and `40` integration; `4` ignored release-only resource tests passed separately |
| Release binary | `902320` bytes; limit `12582912` bytes |
| Managed-run parent peak RSS | `2195456` bytes while supervising the fixture runner; limit `25165824` bytes |
| Doctor peak RSS | `1736704` bytes from `/usr/bin/time -l` |
| Outside-repository warm-cache timing | 20 recorded runs; median `2.213 ms`; p95 `2.601 ms`; median limit `100 ms` |
| npm single-package warm-cache timing | 20 recorded runs; median `2.323 ms`; p95 `2.809 ms`; limits `300/500 ms` |
| Long-run wakeup gate | `1800` signal waits over a fake-clock `1800`-second managed run; limit `1800` |
| Resource cleanup | The release managed-run gate recorded the direct runner and descendant PIDs and proved both absent after completion |
| License gate | `cargo metadata --locked --format-version 1` reported `56` packages; all SPDX expressions matched the reviewed MIT, Apache-2.0, BSD-3-Clause, ISC, Unlicense, LLVM-exception, and Unicode-3.0 allowlist |
| Advisory and yanked gate | Official Apple-Silicon `cargo-audit 0.22.2` loaded `1239` RustSec advisories and scanned `56` locked crate dependencies with `--deny warnings`; no vulnerability, warning, or yanked-crate finding was emitted |
| Source boundary audit | No Tokio/async-std, network client, private memory-pressure API, process-table enumeration, first-party `unsafe` block, `sh -c` launch, or `NODE_OPTIONS` mutation. `src/process.rs` contains the sole production `Command::new`, using the validated executable plus argument array |
| Result schema gate | Draft 2020-12 schema identity and the closed-schema/redaction record test passed |
| Rust gate commands | `cargo fmt --all -- --check`<br>`cargo clippy --all-targets -j 1 -- -D warnings`<br>`cargo test -j 1 -- --test-threads=1`<br>`cargo build --release -j 1` |
| Release resource commands | `cargo test --release --test doctor_budget -j 1 -- --ignored --test-threads=1 --nocapture`<br>`cargo test --release --test run_budget -j 1 -- --ignored --test-threads=1 --nocapture`<br>`stat -f '%z bytes' target/release/agent-lowmem`<br>`/usr/bin/time -l target/release/agent-lowmem doctor >/dev/null` |
| Audit commands | `cargo metadata --locked --format-version 1` plus the reviewed SPDX allowlist<br>`cargo-audit audit --deny warnings --file Cargo.lock` using the official `cargo-audit-aarch64-apple-darwin-v0.22.2` release<br>negative `rg` source/dependency boundary searches<br>`cargo test result_file::tests::structured_record_matches_the_closed_schema_and_omits_sensitive_values -j 1 -- --test-threads=1`<br>`git diff --check` |

These are development measurements on the reference 8 GiB Mac, not a release, distribution, or portability claim. The fake-clock gate proves supervisor scheduling behavior without keeping the Mac busy for 30 wall-clock minutes; the release integration gate separately exercises a real process group and verifies cleanup.

### Phase 3 audit transcript

The license command and its result were:

```bash
cargo metadata --locked --format-version 1 | jq -er '["MIT","Unlicense OR MIT","BSD-3-Clause","MIT OR Apache-2.0","Apache-2.0 OR MIT","Apache-2.0/MIT","Apache-2.0","ISC","Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT","MIT/Apache-2.0","Unlicense/MIT","(MIT OR Apache-2.0) AND Unicode-3.0"] as $allowed | [.packages[] | . as $package | select(($package.license == null) or (($allowed | index($package.license)) == null)) | {name, version, license}] as $rejected | if ($rejected | length) == 0 then "license-audit packages=\(.packages | length) status=pass" else error("rejected licenses: \($rejected)") end'
# license-audit packages=56 status=pass
```

The advisory and yanked-crate commands and result were:

```bash
mkdir target/phase3-audit-tool
gh release download 'cargo-audit/v0.22.2' --repo rustsec/rustsec --pattern 'cargo-audit-aarch64-apple-darwin-v0.22.2.tgz' --dir target/phase3-audit-tool
tar -xzf target/phase3-audit-tool/cargo-audit-aarch64-apple-darwin-v0.22.2.tgz -C target/phase3-audit-tool
target/phase3-audit-tool/cargo-audit-aarch64-apple-darwin-v0.22.2/cargo-audit audit --deny warnings --file Cargo.lock
# Loaded 1239 security advisories
# Scanning Cargo.lock for vulnerabilities (56 crate dependencies)
# exit 0; no vulnerability, warning, or yanked-crate finding
```

The production-boundary command and result were:

```bash
set -e
if rg -n 'tokio|async-std|async_std|reqwest|ureq|hyper|curl|TcpStream|UdpSocket|std::net' Cargo.toml Cargo.lock src; then exit 1; fi
if rg -n 'kern\.memorystatus_vm_pressure_level|memorystatus|vm_pressure|proc_listallpids|proc_listpids|proc_listpidspath|listpids|sysinfo::System' src; then exit 1; fi
if rg -n 'unsafe[[:space:]]*\{' src; then exit 1; fi
if rg -n 'Command::new\(("|r#)(/bin/)?(sh|bash)|\.arg\("-c"\)|\.args\(\[[[:space:]]*"-c"' src; then exit 1; fi
if rg -n '\.env\([^\n]*NODE_OPTIONS|NODE_OPTIONS[^\n]*\.env\(' src; then exit 1; fi
test "$(rg -n 'Command::new' src | wc -l | tr -d ' ')" -eq 1
rg -n '#!\[forbid\(unsafe_code\)\]|Command::new' src/lib.rs src/process.rs
# src/lib.rs:1:#![forbid(unsafe_code)]
# src/process.rs:197:    let mut command = Command::new(&launch.executable);
# source-boundary-audit status=pass command_spawns=1
```
