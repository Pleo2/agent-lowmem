# Agent Lowmem Phase 1 Native Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILLS: Use `superpowers:executing-plans` and `superpowers:test-driven-development` to implement this plan task-by-task. Execute inline and sequentially on the reference Mac; do not dispatch subagents unless the user explicitly authorizes the additional memory risk. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish the production Rust workspace and deliver a private development checkpoint of `agent-lowmem doctor` that inspects the reference host and basic Git/package-manager evidence without starting child processes.

**Architecture:** One small production crate contains focused modules for result contracts, CLI parsing, host inspection, repository discovery, and doctor presentation. Native host facts enter through an injectable `HostSource`; the macOS implementation uses the safe `sysctl` crate, while deterministic tests use in-memory evidence. This phase does not implement `run`, locking, script classification, managed files, or distribution, and must not be released independently.

**Tech Stack:** Rust 1.85.0, edition 2024, Cargo resolver 3, `serde`, `serde_json`, `semver`, and `sysctl` 0.7.1. No Tokio, async runtime, daemon, Swift production code, or first-party `unsafe`.

**Spec:** `docs/superpowers/specs/2026-09-02-agent-lowmem-v1-design.md`

## Global Constraints

- Start from commit `edcc3d7` on a clean isolated branch named `feat/agent-lowmem-phase-1`; use `superpowers:using-git-worktrees` when execution begins.
- Rust 1.85.0 is the minimum and pinned implementation toolchain; every first-party production target uses edition 2024 and `#![forbid(unsafe_code)]`.
- Run only one install, compile, formatter, linter, test, or measurement command at a time. Every Cargo command that can compile uses `-j 1`; tests also use `--test-threads=1`.
- Production `doctor` starts no child process, executes no Git/Node/npm/pnpm/package binary, makes no network request, writes no repository file, and reads no user/global package-manager configuration.
- During Phase 1 the first-party production crate has no `std::process::Command`; subprocesses are permitted only in integration tests that verify the executable boundary. Phase 3 will introduce one reviewed runner module and narrow this source guard to inspection modules.
- Do not read `kern.memorystatus_vm_pressure_level`, apply a heap cap, mutate `NODE_OPTIONS`, or include the Swift pressure probe in a Rust target.
- Structured output contains no absolute repository path, username, home path, environment value, or raw package script.
- Direct dependencies require a purpose and license note in `docs/dependencies-v1.md`; `Cargo.lock` records exact resolved versions.
- Phase 1 is an internal checkpoint. It does not create a release, tag, npm package, Homebrew formula, or public support claim.

---

## File Structure

- `Cargo.toml`: Rust workspace membership, shared metadata, and release profile.
- `rust-toolchain.toml`: exact Rust 1.85.0 toolchain plus `rustfmt` and `clippy`.
- `crates/agent-lowmem/Cargo.toml`: production crate metadata and reviewed direct dependencies.
- `crates/agent-lowmem/src/lib.rs`: first-party safety boundary and module exports.
- `crates/agent-lowmem/src/main.rs`: thin process entry point and exit-code handoff.
- `crates/agent-lowmem/src/result.rs`: closed reason/origin vocabulary and valid code combinations.
- `crates/agent-lowmem/src/cli.rs`: strict manual parser for `doctor` and `--json`.
- `crates/agent-lowmem/src/host.rs`: injectable host evidence, native sysctl reader, support classification, and exact reference-profile match.
- `crates/agent-lowmem/src/repository.rs`: data-only Git-root, root-manifest, lockfile, and package-manager detection.
- `crates/agent-lowmem/src/doctor.rs`: human and JSON doctor report assembly without absolute paths.
- `crates/agent-lowmem/tests/doctor_cli.rs`: executable-level behavior, redaction, and zero-child-process sentinels.
- `crates/agent-lowmem/tests/doctor_budget.rs`: ignored release-mode 20-run warm-cache timing gate.
- `schemas/result-v1.schema.json`: closed v1 `origin` and `reason` contract from Rev 6.
- `docs/dependencies-v1.md`: direct-dependency purpose, API boundary, license, and lockfile policy.

## Out of Scope and Ordered Follow-up Plans

1. `2026-09-02-agent-lowmem-phase-2-repository-policy.md`: workspace cardinality, repository config parsing, tokenizer, wrappers, bounded script graph, adapter matrix, and `.agent-lowmem.json` schema.
2. `2026-09-02-agent-lowmem-phase-3-managed-runner.md`: launch planning, evidence hashes, per-user lock, package-manager argument arrays, process groups, signals, deadlines, cleanup, and JSON result files.
3. `2026-09-02-agent-lowmem-phase-4-managed-files.md`: deterministic `init`, `AGENTS.md` markers, restoration manifest, dry-run, conflicts, and forced-block restore.
4. `2026-09-02-agent-lowmem-phase-5-distribution.md`: full acceptance matrix, dependency policy, resource gates, npm launcher, Homebrew, signing, notarization, release provenance, README, and website handoff.

Only Phase 1 is active. Create each later plan after the preceding phase passes its exit gate so interfaces reflect verified code rather than speculation.

### Task 1: Install the minimal Rust toolchain and create the safe workspace

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `crates/agent-lowmem/Cargo.toml`
- Create: `crates/agent-lowmem/src/lib.rs`
- Create: `crates/agent-lowmem/src/main.rs`
- Create: `docs/dependencies-v1.md`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: clean repository at `edcc3d7` and Homebrew 6.x.
- Produces: buildable `agent-lowmem` library/binary package using Rust 1.85.0 with first-party unsafe code forbidden.

- [x] **Step 1: Install only rustup and the pinned minimal toolchain**

Run sequentially:

```bash
brew install rustup
export PATH="$(brew --prefix rustup)/bin:$PATH"
rustup toolchain install 1.85.0 --profile minimal --component rustfmt --component clippy
rustc +1.85.0 --version
cargo +1.85.0 --version
```

Expected: `rustc 1.85.0` and `cargo 1.85.0`. Do not install another Rust formula or nightly toolchain.

- [x] **Step 2: Create the pinned workspace files**

`Cargo.toml`:

```toml
[workspace]
members = ["crates/agent-lowmem"]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "MIT"

[profile.release]
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "unwind"
```

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.85.0"
profile = "minimal"
components = ["rustfmt", "clippy"]
```

`crates/agent-lowmem/Cargo.toml`:

```toml
[package]
name = "agent-lowmem"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish = false

[dependencies]
semver = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
sysctl = "=0.7.1"
```

`crates/agent-lowmem/src/lib.rs`:

```rust
#![forbid(unsafe_code)]
```

`crates/agent-lowmem/src/main.rs` initially contains only the same safety boundary and a successful empty entry point so workspace validation can run before Task 5 wires the CLI:

```rust
#![forbid(unsafe_code)]

fn main() {}
```

Append `/target/` to `.gitignore`; do not ignore `Cargo.lock`.

- [x] **Step 3: Record the direct dependency boundary**

Create `docs/dependencies-v1.md` with this table and policy:

```markdown
# Agent Lowmem v1 Dependency Record

| Requirement | Purpose | Production boundary | License |
| --- | --- | --- | --- |
| `serde 1.0` | Derive stable structured records | Serialization only | MIT OR Apache-2.0 |
| `serde_json 1.0` | Parse manifests and emit JSON | No script evaluation | MIT OR Apache-2.0 |
| `semver 1.0` | Validate declared package-manager versions | Data-only parsing | MIT OR Apache-2.0 |
| `sysctl 0.7.1` | Safe macOS sysctl reads | Read-only host inspector | MIT |

`Cargo.lock` is the authority for exact resolved versions. Every direct dependency addition requires purpose, source/API review, license review, one-worker tests, and a separate commit. Production dependencies may not add a network client, async runtime, daemon, shell evaluator, or lifecycle installer.
```

- [x] **Step 4: Generate the lockfile and verify the baseline**

Run:

```bash
cargo check --workspace --all-targets -j 1
cargo fmt --all -- --check
```

Expected: both commands exit 0 and create one committed `Cargo.lock`.

- [x] **Step 5: Commit the workspace**

```bash
git add .gitignore Cargo.toml Cargo.lock rust-toolchain.toml crates/agent-lowmem docs/dependencies-v1.md
git commit -m "build: initialize Agent Lowmem Rust workspace"
```

### Task 2: Implement the closed result contract before command behavior

**Files:**
- Create: `crates/agent-lowmem/src/result.rs`
- Create: `schemas/result-v1.schema.json`
- Modify: `crates/agent-lowmem/src/lib.rs`
- Test: `crates/agent-lowmem/src/result.rs`

**Interfaces:**
- Consumes: Rev 6 §11.
- Produces: `Origin`, `Reason`, `ExitResult::is_valid()`, `Reason::ALL`, and the exact JSON enum consumed by every later phase.

- [x] **Step 1: Write failing unit tests for serialization and origin/code compatibility**

```rust
#[test]
fn serializes_stable_kebab_case_tokens() {
    assert_eq!(serde_json::to_string(&Reason::ScriptGraphTooLarge).unwrap(), "\"script-graph-too-large\"");
    assert_eq!(serde_json::to_string(&Origin::SupervisorTimeout).unwrap(), "\"supervisor-timeout\"");
}

#[test]
fn rejects_invalid_origin_code_reason_combinations() {
    assert!(ExitResult::new(Origin::Preflight, 64, Reason::HostUnsupported).is_valid());
    assert!(ExitResult::new(Origin::Child, 0, Reason::Completed).is_valid());
    assert!(!ExitResult::new(Origin::Child, 0, Reason::InternalError).is_valid());
    assert!(!ExitResult::new(Origin::Preflight, 75, Reason::ChildExit).is_valid());
}
```

- [x] **Step 2: Run the focused test and verify RED**

Run: `cargo test -p agent-lowmem result::tests -j 1 -- --test-threads=1`

Expected: compilation fails because the result types do not exist.

- [x] **Step 3: Implement the exact enums and compatibility matcher**

Define both enums as `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]` with `#[serde(rename_all = "kebab-case")]`. Define `ExitResult { origin: Origin, code: i32, reason: Reason }`, a `const fn new(origin, code, reason)`, and `Reason::ALL: [Reason; 31]`. `Reason` contains exactly these 31 variants, in this order:

```rust
Completed,
InvalidCli,
InvalidConfig,
HostUnsupported,
RepositoryUnsupported,
PackageManagerUnsupported,
WorkspaceUnsupported,
WorkspaceCardinality,
OperationUnsupported,
ScriptSyntaxUnsupported,
ScriptShellUnsupported,
ScriptReferenceUnsupported,
ScriptGraphTooLarge,
WrapperUnsupported,
ToolUnsupported,
ToolVersionUnsupported,
WatchDenied,
UiDenied,
BackgroundDenied,
ParallelDenied,
ArgumentDenied,
NonfinalInjectionRequired,
LockHeld,
NestedInvocation,
EvidenceChanged,
ManagedFileConflict,
ChildExit,
ChildSignal,
DeadlineExceeded,
ExternalSignal,
InternalError,
```

`Origin` contains `Preflight`, `Child`, `SupervisorTimeout`, `ExternalSignal`, and `Internal`. Add `pub mod result;` to `lib.rs`. Implement `ExitResult::is_valid()` by matching exhaustively on every `Reason` variant and checking that variant's permitted origin/code combination; do not use a wildcard reason arm.

- [x] **Step 4: Create and test the schema enum**

Create `schemas/result-v1.schema.json` as Draft 2020-12 with required `schemaVersion`, `timestamp`, `origin`, `code`, `reason`, `message`, `nextAction`, and `childStarted` properties. Set `additionalProperties: false`; allow future phase-specific stable records only beneath an optional `details` object. Define the five exact origin strings and the 31 exact reason strings above, then express the Rev 6 origin/code/reason combinations through `oneOf`. Add a unit test that parses the schema with `serde_json`, extracts `/properties/reason/enum`, and asserts equality with `Reason::ALL` serialized in order.

- [x] **Step 5: Run tests and commit**

```bash
cargo test -p agent-lowmem result::tests -j 1 -- --test-threads=1
git add crates/agent-lowmem/src/lib.rs crates/agent-lowmem/src/result.rs schemas/result-v1.schema.json
git commit -m "feat: define Agent Lowmem result contract"
```

Expected: all focused tests pass and the schema contains no reason absent from Rust or vice versa.

### Task 3: Implement deterministic host inspection through an injectable source

**Files:**
- Create: `crates/agent-lowmem/src/host.rs`
- Modify: `crates/agent-lowmem/src/lib.rs`
- Test: `crates/agent-lowmem/src/host.rs`

**Interfaces:**
- Consumes: safe `sysctl` API and Rev 6 §8.3 reference constants.
- Produces: `HostSource::read(key)`, `NativeHostSource`, `inspect_host(source) -> HostReport`, and `ProfileField` mismatch values.

- [x] **Step 1: Write failing deterministic profile tests**

```rust
#[test]
fn matches_only_the_exact_reference_profile() {
    let source = FakeHostSource::reference();
    let report = inspect_host(&source);
    assert!(report.runtime_supported);
    assert!(report.performance_validated);
    assert!(report.mismatched_profile_fields.is_empty());
}

#[test]
fn supports_a_capable_non_reference_mac_without_transferring_validation() {
    let mut source = FakeHostSource::reference();
    source.values.insert("hw.model", "Mac15,12");
    let report = inspect_host(&source);
    assert!(report.runtime_supported);
    assert!(!report.performance_validated);
    assert_eq!(report.mismatched_profile_fields, vec![ProfileField::HardwareModel]);
}

#[test]
fn rejects_a_missing_mandatory_native_read() {
    let mut source = FakeHostSource::reference();
    source.values.remove("hw.pagesize");
    let report = inspect_host(&source);
    assert!(!report.runtime_supported);
    assert_eq!(report.failure_reason, Some(Reason::HostUnsupported));
}
```

- [x] **Step 2: Run the focused test and verify RED**

Run: `cargo test -p agent-lowmem host::tests -j 1 -- --test-threads=1`

Expected: compilation fails because `HostSource` and `HostReport` do not exist.

- [x] **Step 3: Implement host evidence and exact classification**

Define:

```rust
pub trait HostSource {
    fn operating_system(&self) -> &str;
    fn architecture(&self) -> &str;
    fn read(&self, key: &'static str) -> Result<String, HostReadError>;
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HostReport {
    pub operating_system: String,
    pub architecture: String,
    pub macos_version: Option<String>,
    pub hardware_model: Option<String>,
    pub cpu_brand: Option<String>,
    pub physical_memory_bytes: Option<u64>,
    pub page_size_bytes: Option<u64>,
    pub runtime_supported: bool,
    pub performance_validated: bool,
    pub mismatched_profile_fields: Vec<ProfileField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<Reason>,
}
```

Read only `kern.osproductversion`, `hw.model`, `machdep.cpu.brand_string`, `hw.memsize`, and `hw.pagesize`. Normalize Rust's `macos` target name to the public value `darwin` and `aarch64` to `arm64`; reject every other OS/architecture pair. Trim terminal whitespace, parse the macOS major numerically, and require Darwin, arm64, macOS 13+, and all mandatory reads for runtime support. Performance validation requires exact `Mac14,15`, exact `Apple M2`, `8_589_934_592`, `16_384`, and macOS major 26.

Add `pub mod host;` to `lib.rs`.

- [x] **Step 4: Implement the native safe source and live smoke test**

`NativeHostSource::read` uses only:

```rust
use sysctl::{Ctl, Sysctl};

let control = Ctl::new(key).map_err(|source| HostReadError::Sysctl(source.to_string()))?;
control
    .value_string()
    .map(|value| value.trim().to_owned())
    .map_err(|source| HostReadError::Sysctl(source.to_string()))
```

`HostReadError` is a small first-party enum with `Sysctl(String)`, `Missing(&'static str)`, and `InvalidNumber(&'static str)` variants. It stores no environment value or path.

Add a macOS-only test that asserts the current host report contains `Mac14,15`, `Apple M2`, `8_589_934_592`, and `16_384`. It may assert `performance_validated` only when the observed macOS major is 26.

- [x] **Step 5: Verify forbidden behavior and commit**

```bash
cargo test -p agent-lowmem host::tests -j 1 -- --test-threads=1
rg -n 'memorystatus_vm_pressure|std::process::Command|unsafe' crates/agent-lowmem/src/host.rs
git add crates/agent-lowmem/src/lib.rs crates/agent-lowmem/src/host.rs
git commit -m "feat: inspect Agent Lowmem host capabilities"
```

Expected: tests pass; `rg` returns no match.

### Task 4: Add data-only Git and root package-manager evidence

**Files:**
- Create: `crates/agent-lowmem/src/repository.rs`
- Modify: `crates/agent-lowmem/src/lib.rs`
- Test: `crates/agent-lowmem/src/repository.rs`

**Interfaces:**
- Consumes: a starting directory and filesystem data only.
- Produces: `find_git_root(start) -> Result<Option<GitRoot>, RepositoryError>` and `inspect_repository(start) -> RepositoryReport` without serializing the root path.

- [x] **Step 1: Write failing fixture tests**

Create fixtures inside a unique test temporary directory using Rust filesystem APIs. Cover:

```rust
#[test]
fn detects_pnpm_from_manifest_and_matching_lockfile_without_exposing_root() {
    let fixture = Fixture::git_repo();
    fixture.write("package.json", r#"{"packageManager":"pnpm@10.33.0"}"#);
    fixture.write("pnpm-lock.yaml", "lockfileVersion: '9.0'\n");
    let report = inspect_repository(fixture.path());
    assert!(report.git_root_available);
    let manager = report.package_manager.as_ref().unwrap();
    assert_eq!(manager.kind, PackageManagerKind::Pnpm);
    assert_eq!(manager.version.to_string(), "10.33.0");
    assert!(!serde_json::to_string(&report).unwrap().contains(fixture.path().to_str().unwrap()));
}

#[test]
fn rejects_a_declared_manager_with_the_wrong_lockfile() {
    let fixture = Fixture::git_repo();
    fixture.write("package.json", r#"{"packageManager":"npm@11.11.0"}"#);
    fixture.write("pnpm-lock.yaml", "lockfileVersion: '9.0'\n");
    let report = inspect_repository(fixture.path());
    assert_eq!(report.failure_reason, Some(Reason::PackageManagerUnsupported));
}
```

Also cover no Git root, parent walking, a `.git` directory, a valid worktree `.git` pointer file, malformed JSON, missing root package, missing version, and ambiguous npm/pnpm lockfiles.

- [x] **Step 2: Run the focused tests and verify RED**

Run: `cargo test -p agent-lowmem repository::tests -j 1 -- --test-threads=1`

Expected: compilation fails because repository inspection types do not exist.

- [x] **Step 3: Implement the minimum data-only inspector**

The public serialized report is:

```rust
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryReport {
    pub git_root_available: bool,
    pub root_package_available: bool,
    pub package_manager: Option<PackageManagerReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<Reason>,
}
```

Keep the canonical root path in a private `GitRoot(PathBuf)` that has no `Serialize` implementation. Accept only `npm@<valid semver>` with exactly one `package-lock.json`, or `pnpm@<valid semver>` with exactly one `pnpm-lock.yaml`. Do not execute Git, read scripts, enumerate workspaces, or inspect user/global configuration in this phase.

Add `pub mod repository;` to `lib.rs`.

- [x] **Step 4: Run tests, inspect the production imports, and commit**

```bash
cargo test -p agent-lowmem repository::tests -j 1 -- --test-threads=1
rg -n 'std::process|Command::new|\.command\(' crates/agent-lowmem/src/repository.rs
git add crates/agent-lowmem/src/lib.rs crates/agent-lowmem/src/repository.rs
git commit -m "feat: inspect repository evidence as data"
```

Expected: tests pass; `rg` returns no match.

### Task 5: Wire the strict doctor CLI and redacted presentation

**Files:**
- Create: `crates/agent-lowmem/src/cli.rs`
- Create: `crates/agent-lowmem/src/doctor.rs`
- Modify: `crates/agent-lowmem/src/lib.rs`
- Modify: `crates/agent-lowmem/src/main.rs`
- Test: `crates/agent-lowmem/src/cli.rs`
- Test: `crates/agent-lowmem/src/doctor.rs`
- Test: `crates/agent-lowmem/tests/doctor_cli.rs`

**Interfaces:**
- Consumes: `inspect_host`, `inspect_repository`, and result vocabulary.
- Produces: `CliCommand::Doctor { json }`, `DoctorReport`, human output on stdout, JSON output on stdout, diagnostics on stderr, and deterministic wrapper exit codes.

- [x] **Step 1: Write failing parser tests**

```rust
#[test]
fn parses_only_the_phase_one_doctor_forms() {
    assert_eq!(parse(["doctor"]).unwrap(), CliCommand::Doctor { json: false });
    assert_eq!(parse(["doctor", "--json"]).unwrap(), CliCommand::Doctor { json: true });
    assert_eq!(parse(["--json", "doctor"]).unwrap_err().reason(), Reason::InvalidCli);
    assert_eq!(parse(["run", "test"]).unwrap_err().reason(), Reason::OperationUnsupported);
}
```

The parser operates on `OsString`, rejects non-UTF-8 command tokens, accepts no abbreviated flags, and does not import a shell or CLI framework.

- [x] **Step 2: Run parser tests and verify RED**

Run: `cargo test -p agent-lowmem cli::tests -j 1 -- --test-threads=1`

Expected: compilation fails because `CliCommand` does not exist.

- [x] **Step 3: Implement doctor report assembly**

Define:

```rust
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub schema_version: u8,
    pub phase: &'static str,
    pub host: HostReport,
    pub repository: RepositoryReport,
    pub next_action: &'static str,
}
```

For this private checkpoint, set `schema_version` to `1`, `phase` to `native-foundation`, and `next_action` to `implement repository policy before enabling managed runs`. Do not serialize a timestamp or claim that `run` is implemented. Human output states the same limitation explicitly.

- [x] **Step 4: Wire the binary entry point**

Add `pub mod cli;` and `pub mod doctor;` to `lib.rs`. `main.rs` creates `NativeHostSource`, obtains `current_dir`, invokes doctor, and prints exactly one representation to stdout. A completed inspection returns 0 even outside a repository or on an unsupported host because `doctor` reports capability rather than launching work. Invalid CLI returns 2, the intentionally unavailable `run` path returns 64, and an inspection failure that prevents any report returns 70. Panic handling and runner cleanup remain Phase 3 because this phase owns no child or lock.

- [x] **Step 5: Write executable-level tests**

Use `env!("CARGO_BIN_EXE_agent-lowmem")` and an isolated working directory to assert:

- `doctor --json` exits 0 and parses as JSON;
- JSON contains no fixture absolute path or environment value;
- human `doctor` contains the product name, runtime-support state, performance-validation state, repository availability, and phase limitation;
- invalid arguments exit 2;
- `run test` exits 64, starts no repository child, and writes `agent-lowmem: result origin=preflight code=64 reason=operation-unsupported` to stderr.

- [x] **Step 6: Run focused tests and commit**

```bash
cargo test -p agent-lowmem cli::tests -j 1 -- --test-threads=1
cargo test -p agent-lowmem doctor::tests -j 1 -- --test-threads=1
cargo test -p agent-lowmem --test doctor_cli -j 1 -- --test-threads=1
git add crates/agent-lowmem/src crates/agent-lowmem/tests/doctor_cli.rs
git commit -m "feat: add native Agent Lowmem doctor checkpoint"
```

### Task 6: Prove the zero-child boundary and record the Phase 1 baseline

**Files:**
- Modify: `crates/agent-lowmem/tests/doctor_cli.rs`
- Create: `crates/agent-lowmem/tests/doctor_budget.rs`
- Modify: `docs/dependencies-v1.md`

**Interfaces:**
- Consumes: release binary from Task 5.
- Produces: executable sentinel proof, 20-run timing gate, binary-size/RSS evidence, and the Phase 1 completion decision.

- [x] **Step 1: Add a failing child-process sentinel test**

The test creates executable sentinels named `git`, `node`, `npm`, and `pnpm` in an isolated directory. Each sentinel writes its name to a marker before exiting 97. Run the absolute Agent Lowmem binary with `PATH` containing only the sentinel directory plus `/usr/bin:/bin`, then assert doctor exits normally and the marker does not exist.

Also scan `crates/agent-lowmem/src` in the test and fail if any production file contains `std::process::Command`, `Command::new`, `memorystatus_vm_pressure`, or `unsafe {`.

- [x] **Step 2: Run the sentinel test before completing the guard**

First invoke one sentinel directly from the test with its isolated `PATH` and assert that it writes the marker; delete the marker, then invoke Agent Lowmem `doctor` with the same `PATH` and assert the marker remains absent. Run:

```bash
cargo test -p agent-lowmem --test doctor_cli zero_child -j 1 -- --test-threads=1
```

Expected: the fixture self-check proves the sentinel can detect a subprocess, while the doctor invocation passes with no marker. Production code never contains an intentional subprocess implementation.

- [x] **Step 3: Add the ignored warm-cache release timing test**

`doctor_budget.rs` launches the release binary 20 times from a committed-equivalent empty temporary directory, records `Instant` elapsed milliseconds, sorts them, and calculates median index 9 and p95 index 18. Assert median at most 100 ms. Repository-fixture timing remains Phase 2 because this phase does not yet classify scripts and tools.

Run:

```bash
cargo test --release -p agent-lowmem --test doctor_budget -j 1 -- --ignored --test-threads=1
```

- [x] **Step 4: Run the complete sequential Phase 1 gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -j 1 -- -D warnings
cargo test --workspace -j 1 -- --test-threads=1
cargo build --release -p agent-lowmem -j 1
cargo test --release -p agent-lowmem --test doctor_budget -j 1 -- --ignored --test-threads=1
stat -f '%z bytes' target/release/agent-lowmem
/usr/bin/time -l target/release/agent-lowmem doctor >/dev/null
git diff --check
```

Expected:

- formatter, Clippy, and all tests exit 0;
- timing test reports median at or below 100 ms outside a repository;
- release binary is at or below 12 MiB;
- `/usr/bin/time -l` reports peak resident memory at or below 24 MiB;
- no raw trace, Swift build product, `target/`, absolute path, or environment value is tracked.

- [x] **Step 5: Record exact measurements and commit**

Append one dated Phase 1 table to `docs/dependencies-v1.md` containing the host key, Rust version, commit under test, release binary bytes, peak resident bytes, median doctor milliseconds, p95 doctor milliseconds, and commands above. These are development measurements, not a release claim.

```bash
git add crates/agent-lowmem/tests docs/dependencies-v1.md Cargo.lock
git commit -m "test: verify Agent Lowmem doctor resource boundary"
```

## Phase 1 Exit Gate

Phase 1 is complete only when all six tasks are committed, the working tree is clean, the complete sequential gate passes on the reference Mac, `doctor` starts no child, the result schema matches all 31 Rust reasons, current-host evidence matches the exact reference profile, and no release artifact has been published.

The saved next action is to create `docs/superpowers/plans/2026-09-02-agent-lowmem-phase-2-repository-policy.md` from the verified Phase 1 interfaces. Do not begin tokenizer, workspace, adapter, lock, runner, `init`, or distribution work in parallel with Phase 1.
