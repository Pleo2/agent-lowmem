# Agent Lowmem Phase 3 Managed Runner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable `agent-lowmem run` to execute one configured npm or pnpm operation through verified evidence, a per-user lease, an owned process group, bounded supervision, exact exit preservation, and optional atomic JSON output.

**Architecture:** Reuse Phase 2 policy construction to create an evidence-backed `RunPlan`, acquire a non-blocking per-user lease, rebuild and compare the complete plan under that lease, then spawn exactly one package manager in a new process group. A synchronous supervisor owns signals, warning and timeout deadlines, group cleanup, result emission, and lease release without an async runtime or process-table scan.

**Tech Stack:** Rust 1.85, edition 2024, standard library process APIs, existing serde/serde_json/semver/sysctl dependencies, and narrowly reviewed safe crates for SHA-256, Unix locking/signals, macOS process identity, signal delivery, and RFC 3339 formatting.

**Spec:** `docs/superpowers/specs/2026-09-03-agent-lowmem-phase-3-managed-runner-design.md`

## Global Constraints

- Work sequentially on `main`; use one Cargo job and one test thread for full gates.
- Use TDD for every behavior change and one Conventional Commit per independently reviewable task.
- Keep `#![forbid(unsafe_code)]`; no first-party unsafe blocks or `pre_exec` closures.
- Start no repository child during `doctor`, planning A, lease acquisition, or planning B.
- Spawn only the immutable npm/pnpm launch array from the revalidated `OperationPolicy`.
- Inherit stdio and environment; change only `AGENT_LOWMEM_ACTIVE=1` in the child.
- Add no Tokio, async-std, network client, daemon, pressure reader, process-table scan, shell-string construction, heap mutation, or automatic retry.
- Print or persist no raw script, forwarded argument, environment value, dotenv path, assignment, username, PID, process-group ID, or absolute repository path.
- Preserve the existing closed `Origin` and `Reason` vocabulary and the 24 MiB RSS / 12 MiB release-binary gates.
- Review every direct dependency's API, source, license, MSRV, features, transitive graph, and size before production use; update `Cargo.lock` and `docs/dependencies-v1.md` in the same task.

---

### Task 1: Strict `run` CLI request

**Files:**
- Modify: `src/cli.rs`
- Test: `src/cli.rs`

**Interfaces:**
- Consumes: existing `configuration::valid_key` and `configuration::valid_relative_path` made crate-visible without changing their grammar.
- Produces: `RunRequest { operation_key, workspace_key, json_file, forwarded_arguments }` and `CliCommand::Run(RunRequest)`; tests destructure the enum directly and require no convenience method.

- [x] **Step 1: Write failing table-driven parser tests**

```rust
assert_eq!(parse(["run", "test"]).unwrap(), CliCommand::Run(RunRequest {
    operation_key: "test".into(), workspace_key: None, json_file: None,
    forwarded_arguments: vec![],
}));
let CliCommand::Run(request) = parse(["run", "test", "--workspace", "web", "--", "src/a.test.ts"]).unwrap() else { panic!() };
assert_eq!(request.workspace_key.as_deref(), Some("web"));
let CliCommand::Run(request) = parse(["run", "test", "--json-file", ".agent-lowmem-result.json"]).unwrap() else { panic!() };
assert_eq!(request.json_file.as_deref(), Some(".agent-lowmem-result.json"));
assert_eq!(parse(["run", "test", "--workspace", "web", "--workspace", "api"]).unwrap_err().reason(), Reason::InvalidCli);
assert_eq!(parse(["run", "test", "--unknown"]).unwrap_err().reason(), Reason::InvalidCli);
```

- [x] **Step 2: Run the focused test and confirm RED**

Run: `cargo test cli::tests::parses_strict_run_requests -j 1 -- --test-threads=1`

Expected: compile failure because `RunRequest` and `CliCommand::Run` do not exist.

- [x] **Step 3: Implement the minimal state parser**

Parse exactly one operation, optional single `--workspace`, optional single `--json-file`, and one exact forwarding boundary. Reject non-UTF-8 and NUL-bearing values. Keep `run` structural errors at `invalid-cli`; policy validation remains outside this module.

- [x] **Step 4: Run focused and complete tests**

Run: `cargo test cli::tests -j 1 -- --test-threads=1`

Run: `cargo test -j 1 -- --test-threads=1`

Expected: all tests pass; the main binary still reports `operation-unsupported` until Task 8 dispatches `Run`.

- [x] **Step 5: Commit**

```bash
git add src/cli.rs src/configuration.rs
git commit -m "feat: parse strict managed run requests"
```

### Task 2: Exact-byte evidence snapshots

**Files:**
- Create: `src/evidence.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `docs/dependencies-v1.md`
- Test: `src/evidence.rs`

**Interfaces:**
- Consumes: canonical repository root and validated repository-relative paths.
- Produces: `EvidenceReader`, `EvidenceFile { relative_path, bytes, sha256 }`, `EvidenceFile::digest() -> EvidenceDigest`, `EvidenceDigest`, `EvidenceDigest::hex() -> String`, and sorted `EvidenceSnapshot`.

- [x] **Step 1: Record and review the evidence dependencies before use**

Pin an MSRV-compatible `sha2` release with default features disabled where possible and `rustix` with only `std` and `fs` for component-relative no-follow opens. Record their licenses, roles, enabled features, and dependency-tree delta in `docs/dependencies-v1.md`.

- [x] **Step 2: Write failing digest, ordering, and path-boundary tests**

```rust
let first = reader.read("package.json").unwrap();
assert_eq!(first.digest().hex(), "expected-fixture-sha256");
assert_eq!(EvidenceSnapshot::new(vec![z, a]).files[0].relative_path, "a.json");
assert_eq!(reader.read("../outside").unwrap_err(), Reason::RepositoryUnsupported);
assert_eq!(reader.read("linked/package.json").unwrap_err(), Reason::RepositoryUnsupported);
```

- [x] **Step 3: Run focused tests and confirm RED**

Run: `cargo test evidence::tests -j 1 -- --test-threads=1`

Expected: compile failure because the evidence module does not exist.

- [x] **Step 4: Implement one-read bytes plus digest**

Open each component below the canonical root with safe no-follow directory/file operations, require a regular final file, read once, hash those exact bytes, and return bytes plus digest. Sort and deduplicate by relative identity; reject conflicting duplicate digests.

- [x] **Step 5: Verify and commit**

Run: `cargo test evidence::tests -j 1 -- --test-threads=1`

Run: `cargo test -j 1 -- --test-threads=1`

```bash
git add Cargo.toml Cargo.lock docs/dependencies-v1.md src/evidence.rs src/lib.rs
git commit -m "feat: capture exact repository evidence"
```

### Task 3: Evidence-backed `RunPlan`

**Files:**
- Modify: `src/repository.rs`
- Modify: `src/policy.rs`
- Modify: `src/package_manager.rs`
- Test: `src/repository.rs`
- Test: `tests/repository_policy.rs`

**Interfaces:**
- Consumes: `RunSelection { operation_key, workspace_key, forwarded_arguments }`, `EvidenceReader`, configuration/workspace/adapter modules, and `OperationPolicy`.
- Produces: `plan_run(start: &Path, selection: &RunSelection) -> Result<RunPlan, Reason>` where `RunPlan` contains private root, typed policy, evidence snapshot, repository hash, and `RunPlan::redacted() -> RedactedRunPlan<'_>`.

- [x] **Step 1: Write failing run-plan tests**

```rust
let plan = plan_run(fixture.root(), &RunSelection::root("test", vec![])).unwrap();
assert_eq!(plan.policy.operation_key, "test");
assert!(plan.evidence.files.iter().any(|f| f.relative_path == ".agent-lowmem.json"));
assert!(!format!("{:?}", plan.redacted()).contains(fixture.root().to_str().unwrap()));
```

Include configured root/workspace selection, forwarded denial, lifecycle evidence, wrapper redaction, duplicate workspace cardinality, and unconfigured operation rejection.

- [x] **Step 2: Confirm RED**

Run: `cargo test repository::tests::plans_configured_run -j 1 -- --test-threads=1`

- [x] **Step 3: Refactor repository reads through `EvidenceReader`**

Keep `inspect_repository` behavior stable while making `plan_run` use exact bytes and collect every admitted evidence digest. Make `OperationPolicy` fully `Debug + Clone + PartialEq + Eq` through its redacted implementation and expose no raw script body.

- [x] **Step 4: Add exact replan comparison**

```rust
pub fn plans_match(before: &RunPlan, after: &RunPlan) -> bool {
    before.evidence == after.evidence
        && before.policy == after.policy
        && before.repository_hash == after.repository_hash
}
```

- [x] **Step 5: Verify and commit**

Run: `cargo test repository -j 1 -- --test-threads=1`

Run: `cargo test -j 1 -- --test-threads=1`

```bash
git add src/repository.rs src/policy.rs src/package_manager.rs tests/repository_policy.rs
git commit -m "feat: build evidence-backed run plans"
```

### Task 4: Per-user lease and process identity

**Files:**
- Create: `src/lock.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `docs/dependencies-v1.md`
- Test: `src/lock.rs`
- Create: `tests/run_lock.rs`

**Interfaces:**
- Consumes: repository hash, operation key, effective user, process identity source, and per-user runtime directory.
- Produces: `UserLease::acquire`, `UserLease::set_child_group`, `UserLease::clear_child_group`, `LockProbe`, and `LockStatus::{Available,Held,OrphanRecovery,InvalidRecord}`.

- [x] **Step 1: Review and pin safe low-level dependencies**

Extend the already pinned `rustix` feature set only for UID, advisory `flock`, and process-group probes. Review `libproc` for one-PID start identity only. Reject broad enumeration APIs and record licenses, MSRVs, build dependencies, features, and dependency-tree deltas.

- [x] **Step 2: Write failing lease tests**

```rust
let record = fixture_lock_record(); // local helper returns a complete schema-v1 record
let first = UserLease::acquire(&runtime, record.clone()).unwrap();
assert_eq!(UserLease::acquire(&runtime, record.clone()).unwrap_err(), Reason::LockHeld);
drop(first);
assert!(UserLease::acquire(&runtime, record).is_ok());
```

Add mode `0700`/`0600`, nested marker, malformed record, stale record, exact start-identity, orphan-recovery, symlink, wrong-owner, and two-process contention cases.

- [x] **Step 3: Confirm RED and implement the lease**

Run: `cargo test --test run_lock -j 1 -- --test-threads=1`

Use the advisory file descriptor as live ownership. Synchronize the redacted record while locked. Never automatically signal an orphan record.

- [x] **Step 4: Verify and commit**

Run: `cargo test lock -j 1 -- --test-threads=1`

Run: `cargo test --test run_lock -j 1 -- --test-threads=1`

Run: `cargo test -j 1 -- --test-threads=1`

```bash
git add Cargo.toml Cargo.lock docs/dependencies-v1.md src/lock.rs src/lib.rs tests/run_lock.rs
git commit -m "feat: enforce the per-user operation lease"
```

### Task 5: Owned process-group launch and signal source

**Files:**
- Create: `src/process.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `docs/dependencies-v1.md`
- Test: `src/process.rs`

**Interfaces:**
- Consumes: revalidated `RunPlan` and `UserLease`.
- Produces: `ManagedChild`, `OwnedProcessGroup`, `GroupController`, `SignalSource`, and `NativeSignalSource`.

- [ ] **Step 1: Review and pin `signal-hook`**

Enable only synchronous iterator functionality. Record its license, MSRV, libc/registry transitive dependencies, handler semantics, release-size delta, and lack of async runtime.

- [ ] **Step 2: Write failing spawn-boundary tests**

Assert inherited stdio, current Git root, exact executable/arguments, `AGENT_LOWMEM_ACTIVE=1`, new PGID equal to child PID, immediate leader identity, and no shell intermediary.

- [ ] **Step 3: Confirm RED and implement minimal launch**

Run: `cargo test process::tests -j 1 -- --test-threads=1`

Use `CommandExt::process_group(0)`, `Stdio::inherit()`, and the immutable launch array. Install the signal listener before spawn, update the lease after identity capture, and clean the group on post-spawn setup failure.

- [ ] **Step 4: Verify and commit**

Run: `cargo test process::tests -j 1 -- --test-threads=1`

Run: `cargo test -j 1 -- --test-threads=1`

```bash
git add Cargo.toml Cargo.lock docs/dependencies-v1.md src/process.rs src/lib.rs
git commit -m "feat: launch an owned package manager group"
```

### Task 6: Deterministic supervisor and cleanup

**Files:**
- Create: `src/supervisor.rs`
- Modify: `src/lib.rs`
- Test: `src/supervisor.rs`
- Create: `tests/run_supervision.rs`
- Create: `tests/fixtures/runner/managed-child.sh`

**Interfaces:**
- Consumes: `ManagedChild`, `OwnedProcessGroup`, `SignalSource`, injected `Clock`, configured timeout, and output sink.
- Produces: `SupervisionReport { result, warning_emitted, elapsed_millis, cleanup_action, cleanup_complete }`.

- [ ] **Step 1: Write pure failing state-machine tests**

Cover child success/failure/signal, signal-before-tick, child-before-signal, child-at-deadline, exactly one 80-percent warning, timeout, ten-second escalation, second-signal escalation, and no more than one ordinary wake per second.

- [ ] **Step 2: Confirm RED**

Run: `cargo test supervisor::tests -j 1 -- --test-threads=1`

- [ ] **Step 3: Implement the synchronous state machine**

Use `Instant` in production and injected time in unit tests. Check child before boundary decisions, wait on the signal channel until the earliest deadline, distinguish `ExitStatusExt::signal()` from normal codes, and converge every terminal state on one group-cleanup routine.

- [ ] **Step 4: Add real group integration tests**

Run fixture children that exit, self-signal, ignore TERM, and leave a same-group descendant. Assert only the fixture group receives signals and no fixture PID remains after cleanup.

- [ ] **Step 5: Verify and commit**

Run: `cargo test supervisor -j 1 -- --test-threads=1`

Run: `cargo test --test run_supervision -j 1 -- --test-threads=1`

Run: `cargo test -j 1 -- --test-threads=1`

```bash
git add src/supervisor.rs src/lib.rs tests/run_supervision.rs tests/fixtures/runner/managed-child.sh
git commit -m "feat: supervise managed process groups"
```

### Task 7: Atomic redacted result files

**Files:**
- Create: `src/result_file.rs`
- Modify: `src/result.rs`
- Modify: `src/lib.rs`
- Modify: `schemas/result-v1.schema.json`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `docs/dependencies-v1.md`
- Test: `src/result_file.rs`

**Interfaces:**
- Consumes: `RunPlan::redacted`, `SupervisionReport`, lock/recheck/spawn states, and optional repository-relative path.
- Produces: schema-valid `RunResultRecord` and `write_result_atomic(root, relative, record)`.

- [ ] **Step 1: Review an MSRV-compatible UTC formatter**

Prefer pinned `time 0.3.44` with only `std` and `formatting` if dependency and advisory review passes; otherwise choose a smaller reviewed formatter. Record the exact decision and dependency delta.

- [ ] **Step 2: Write failing schema/redaction/atomicity tests**

Assert RFC 3339 UTC, mode `0600`, same-directory atomic replacement, parent sync, symlink/special/escape rejection, no temporary residue after failure, schema validation, and absence of sentinel path/argument/environment values.

- [ ] **Step 3: Confirm RED and implement**

Run: `cargo test result_file::tests -j 1 -- --test-threads=1`

Close `details` in the v1 schema around the exact fields approved by the spec. Validate the destination before lease acquisition and preserve a primary child/signal/timeout result on late write failure.

- [ ] **Step 4: Verify and commit**

Run: `cargo test result_file -j 1 -- --test-threads=1`

Run: `cargo test result::tests -j 1 -- --test-threads=1`

Run: `cargo test -j 1 -- --test-threads=1`

```bash
git add Cargo.toml Cargo.lock docs/dependencies-v1.md schemas/result-v1.schema.json src/result.rs src/result_file.rs src/lib.rs
git commit -m "feat: write atomic managed run results"
```

### Task 8: Managed-run orchestration and CLI activation

**Files:**
- Create: `src/run.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Modify: `src/doctor.rs`
- Test: `src/run.rs`
- Create: `tests/run_cli.rs`
- Create: `tests/fixtures/runner/npm`
- Create: `tests/fixtures/runner/pnpm`

**Interfaces:**
- Consumes: `RunRequest`, host gate, `plan_run`, `UserLease`, `plans_match`, process launch, supervisor, and result writer.
- Produces: enabled `agent-lowmem run`, exactly one stable final result line, and `doctor` managed-run/lock reporting.

- [ ] **Step 1: Write failing orchestration tests**

Cover unsupported host, configured root/workspace operation, disclosures, nested invocation, lock contention, spawn failure, success, child failure, and optional JSON. Assert exactly one final result line.

- [ ] **Step 2: Add a deterministic post-lock mutation barrier test**

Compile a `cfg(test)`-only synchronization hook between lease acquisition and planning B. Mutate `package.json`, the lockfile, `.agent-lowmem.json`, workspace evidence, and tool manifest in separate cases; each must return 75 and leave the npm/pnpm sentinel unstarted.

- [ ] **Step 3: Confirm RED and implement orchestration**

Run: `cargo test --test run_cli -j 1 -- --test-threads=1`

Implement planning A, lease, planning B, exact comparison, signal setup, spawn, supervision, result write, record clear, unlock, and external-signal re-raise behind a `catch_unwind` cleanup guard.

- [ ] **Step 4: Update `doctor` without weakening zero-child inspection**

Set phase to `managed-runner`, report managed runs as available only when host/repository/configuration support them, include only the four-state redacted lock status, and update the next safe action to Phase 4 design.

- [ ] **Step 5: Verify and commit**

Run: `cargo test --test run_cli -j 1 -- --test-threads=1`

Run: `cargo test --test doctor_cli -j 1 -- --test-threads=1`

Run: `cargo test --test repository_policy -j 1 -- --test-threads=1`

Run: `cargo test -j 1 -- --test-threads=1`

```bash
git add src/run.rs src/lib.rs src/main.rs src/doctor.rs tests/run_cli.rs tests/fixtures/runner/npm tests/fixtures/runner/pnpm
git commit -m "feat: enable managed repository runs"
```

### Task 9: Phase 3 hardening and evidence checkpoint

**Files:**
- Modify: `tests/doctor_budget.rs`
- Create: `tests/run_budget.rs`
- Modify: `docs/dependencies-v1.md`

**Interfaces:**
- Consumes: complete Phase 3 CLI.
- Produces: reproducible Phase 3 verification evidence and updated resource/dependency record.

- [ ] **Step 1: Add release-only budget tests**

Measure release supervision RSS, binary size, a 30-minute-equivalent fake-clock wakeup count of at most 1,800, and zero surviving runner resources. Keep wall-clock long tests ignored outside the release gate.

- [ ] **Step 2: Run formatting, lint, and sequential tests**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --all-targets -j 1 -- -D warnings`

Run: `cargo test -j 1 -- --test-threads=1`

- [ ] **Step 3: Run release and resource gates**

Run: `cargo build --release -j 1`

Run: `cargo test --release --test doctor_budget -j 1 -- --ignored --test-threads=1 --nocapture`

Run: `cargo test --release --test run_budget -j 1 -- --ignored --test-threads=1 --nocapture`

Run: `stat -f '%z bytes' target/release/agent-lowmem`

Run: `/usr/bin/time -l target/release/agent-lowmem doctor >/dev/null`

- [ ] **Step 4: Run security and dependency audits**

Run source searches proving no async runtime, network client, private pressure API, process-table enumeration, shell-string launch, `NODE_OPTIONS` mutation, or first-party unsafe block. Run the available license, yanked-crate, and advisory checks and record exact commands and results.

- [ ] **Step 5: Record measurements and commit**

Append the host key, Rust version, commit under test, test totals, dependency versions/licenses, binary size, RSS, timing, wakeup count, and gate commands to `docs/dependencies-v1.md`.

```bash
git add tests/doctor_budget.rs tests/run_budget.rs docs/dependencies-v1.md
git commit -m "docs: record phase 3 runner evidence"
```

- [ ] **Step 6: Push the atomic Phase 3 history**

Run: `git status --short --branch`

Run: `git log --oneline --decorate -12`

Run: `git push origin main`

Expected: local `main` and `origin/main` resolve to the same Phase 3 evidence commit with a clean working tree.
