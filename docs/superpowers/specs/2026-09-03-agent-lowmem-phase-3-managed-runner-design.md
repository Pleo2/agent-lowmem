# Agent Lowmem Phase 3 Managed Runner Design

**Status:** Candidate for implementation-plan review

**Date:** 2026-09-03

**Parent specification:** `docs/superpowers/specs/2026-09-02-agent-lowmem-v1-design.md`, Revision 6

**Depends on:** `docs/superpowers/specs/2026-09-03-agent-lowmem-phase-2-repository-policy-design.md`

**Scope:** Enable configured `run` operations through a single per-user lock, post-lock evidence revalidation, one owned process group, bounded supervision, stable exit results, and optional atomic JSON output. Managed repository writes remain Phase 4.

## 1. Outcome

Phase 3 turns a Phase 2 `OperationPolicy` into one supervised npm or pnpm invocation. It enables only these CLI forms:

```text
agent-lowmem run <operation>
agent-lowmem run <operation> --workspace <key>
agent-lowmem run <operation> -- <forwarded-argument>...
agent-lowmem run <operation> --workspace <key> -- <forwarded-argument>...
agent-lowmem run <operation> [--workspace <key>] --json-file <relative-path> [-- <forwarded-argument>...]
```

`--workspace` and `--json-file` may appear once each, in either order, after the operation and before the first exact `--`. Everything after `--` is a forwarded UTF-8 argument-array element. Unknown flags, duplicates, missing values, NUL bytes, non-UTF-8 tokens, empty operation keys, or tokens after the forwarding boundary that violate the selected adapter contract fail before lock acquisition.

Phase 3 does not implement `init`, `restore`, managed `AGENTS.md` blocks, restoration manifests, release packaging, memory-pressure enforcement, process-table enumeration, automatic retries, heap mutation, or arbitrary shell commands.

## 2. Approved architecture

The runner is a synchronous Rust supervisor. `std::process::Command` performs the only repository child spawn and creates a new process group with the safe Unix `process_group(0)` API. Small reviewed safe wrappers provide advisory locking, process-group signaling, process identity, signal delivery, and SHA-256 hashing. No async runtime, daemon, resident monitor, shell wrapper, or Swift helper enters production.

The rejected alternatives are:

- a `kqueue` or general-purpose async event loop, because the initial runner has one child, three handled signals, and two deadlines and does not justify the added state and dependency surface;
- `/usr/bin/env`, `sh`, `timeout`, or another wrapper process, because it would weaken exact argument-array construction, exit-origin attribution, and process-group ownership;
- process-name or repository-path scanning, because names and paths do not prove ownership and would violate the privacy boundary.

## 3. Fixed invariants

The following rules are not configurable:

- only a configured operation with a runnable Phase 2 policy may start;
- `doctor`, initial planning, and post-lock planning start zero repository children;
- exactly one Agent Lowmem-managed heavy operation may hold the per-user lease;
- the launch uses the immutable package-manager executable and arguments returned by `OperationPolicy`;
- the child inherits stdin, stdout, stderr, and the caller environment unchanged except for `AGENT_LOWMEM_ACTIVE=1`;
- Agent Lowmem creates and signals only its own process group;
- no environment value, raw repository path, raw script, dotenv path, wrapper assignment, or forwarded argument is printed or persisted by Agent Lowmem;
- the post-lock plan must match the pre-lock plan byte-for-byte at the evidence layer and exactly at the typed-policy layer;
- the supervisor uses a monotonic clock for warning, grace, and final deadlines;
- steady-state child/deadline work wakes at most once per second; a delivered external signal may wake it immediately;
- timeout and interruption cleanup never targets an unrelated PID, name, path, port, or process group;
- there is no retry after evidence drift, spawn failure, child failure, signal, timeout, or cleanup failure;
- `NODE_OPTIONS` is inherited but never inspected, logged, added, removed, or rewritten;
- first-party production code retains `#![forbid(unsafe_code)]`.

## 4. Component boundaries

### 4.1 CLI request

`src/cli.rs` adds a `RunRequest` owned by `CliCommand::Run`:

```rust
pub struct RunRequest {
    pub operation_key: String,
    pub workspace_key: Option<String>,
    pub json_file: Option<String>,
    pub forwarded_arguments: Vec<String>,
}
```

The parser validates only CLI structure and the existing stable-key and relative-path lexical rules. Repository membership, configured operation selection, workspace cardinality, adapter argument policy, and JSON destination containment belong to later typed stages. `doctor` remains unchanged syntactically. `init` and `restore` remain unavailable.

### 4.2 Evidence-backed run planning

`src/repository.rs` gains a run-specific API rather than exposing private Phase 2 parsing internals:

```rust
pub fn plan_run(start: &Path, selection: &RunSelection) -> Result<RunPlan, Reason>;
```

`RunSelection` contains the operation key, optional workspace key, and forwarded arguments. `RunPlan` contains:

- the canonical Git root as a private runtime path;
- the selected root or exact workspace identity;
- the complete immutable `OperationPolicy`;
- a sorted `EvidenceSnapshot` produced from the exact bytes parsed to build the policy;
- a SHA-256 hash of the canonical root's raw platform path bytes for lock metadata;
- the redacted operation, adapter, control, disclosure, timeout, graph, and argument-count data required for terminal and JSON results.

The inspector must not parse a file and then reopen it merely to hash it. A single evidence reader returns the bytes used by the parser together with their digest. Every relative path is opened component by component beneath the already canonical Git root without following symlinks. Each final object must be a regular file. Any missing, replaced, escaped, special, unreadable, or malformed object fails closed through the existing reason vocabulary.

The sorted evidence set includes every file listed by the Phase 2 policy: root and selected-workspace manifests, `.agent-lowmem.json`, the selected lockfile, existing matrix-declared repository configuration, workspace declaration evidence, exact Node version evidence, and each resolved tool or wrapper package manifest. Duplicate relative identities collapse only when their digest is identical because they refer to the same admitted file.

### 4.3 Evidence snapshot and post-lock comparison

`src/evidence.rs` owns:

```rust
pub struct EvidenceDigest {
    pub relative_path: String,
    pub sha256: [u8; 32],
}

pub struct EvidenceSnapshot {
    pub files: Vec<EvidenceDigest>,
}
```

After the lease is acquired, `plan_run` is executed again from the same canonical Git root and the same `RunSelection`. The child remains unstarted. The launch may proceed only when all of the following are exactly equal:

- sorted evidence identities and digests;
- root or workspace identity;
- workspace cardinality;
- `OperationPolicy`, including launch executable, argument array, timeout, leaves, controls, disclosures, and evidence identities.

Any difference returns `ExitResult { origin: preflight, code: 75, reason: evidence-changed }`, releases the lease, writes the optional structured result, and starts no child. The runner does not classify a drift as safe, retry planning, or attribute the change to another process.

### 4.4 Per-user operation lease

`src/lock.rs` owns a `UserLease` whose open file descriptor holds a non-blocking exclusive advisory lock for the complete operation. The lock lives in an Agent Lowmem directory beneath the canonical macOS per-user temporary directory. The directory must be real, owned by the effective user, and mode `0700`; the lock must be a regular non-symlink file opened with no-follow semantics and mode `0600`.

Before acquisition, the runner checks for the exact inherited marker `AGENT_LOWMEM_ACTIVE=1`. A match returns code 73 with `nested-invocation` without opening the global lock. The child receives that marker while every other environment entry remains inherited.

A contended advisory lock returns code 73 with `lock-held`. Once exclusively held, the record is replaced while the file remains locked, flushed, and synchronized. The record contains only:

- schema version;
- owner PID and process-start identity;
- SHA-256 repository identity;
- stable operation category;
- acquisition timestamp;
- optional owned child process-group ID and leader start identity after spawn.

It contains no username, absolute repository path, command line, script, forwarded argument, environment value, or opaque token printed to the user.

The advisory lock, not a bare PID, is the authority for a live owner. When the file is unlocked, a persisted child-group entry blocks a new run only if the recorded leader PID still has the recorded process-start identity and the recorded process group is live. This condition is reported as orphan recovery and is never killed automatically. A missing leader identity or non-live group is stale diagnostic data that the next holder may replace. A malformed unlocked record fails closed as `lock-held` and tells the user how to inspect the file before manual removal.

`doctor` may probe the lock without waiting. It reports `available`, `held`, `orphan-recovery`, or `invalid-record`, but never prints the lock path, PIDs, process-group ID, repository hash, or timestamps.

### 4.5 Managed process-group launch

`src/process.rs` consumes only the already revalidated `RunPlan`. It builds:

```text
Command::new(policy.launch.executable)
  .args(policy.launch.arguments)
  .current_dir(canonical_git_root)
  .stdin(inherit)
  .stdout(inherit)
  .stderr(inherit)
  .env("AGENT_LOWMEM_ACTIVE", "1")
  .process_group(0)
```

No shell string is created by Agent Lowmem. npm or pnpm remains responsible for executing the trusted original repository script with the shell policy already proven and pinned by Phase 2.

The direct child PID is the new process-group ID. Immediately after spawn, the runner reads the group leader's process-start identity and updates the still-held lock record. If identity capture or record synchronization fails, the runner cleans the owned group before returning internal error 70.

Signal handling for `SIGINT`, `SIGTERM`, and `SIGHUP` is installed after successful post-lock revalidation and before spawn. If spawn fails, handlers are removed, the lease is released, `childStarted` remains false, and the result is internal error 70.

### 4.6 Supervisor state machine

`src/supervisor.rs` owns a synchronous state machine over injected abstractions for child status, monotonic time, signals, group operations, output, and sleeping. Production signal delivery uses one blocking signal-listener thread and a standard channel. The listener has no access to the child, lock, filesystem, output, or cleanup policy; it only sends the received signal number. It is explicitly closed and joined before the run returns or re-raises a signal.

The running state repeatedly:

1. checks the direct child with a non-blocking wait;
2. drains an immediately available external signal;
3. emits the 80-percent warning once when its monotonic deadline has passed;
4. begins timeout cleanup when the final monotonic deadline has passed;
5. otherwise waits on the signal channel until the earlier of the next one-second child check, the warning deadline, or the final deadline.

At a boundary, an already observed child terminal status wins over a later signal or deadline. An external signal already delivered to the listener wins before another timed wait. At the final deadline the runner performs one last child-status check; a still-running child becomes `deadline-exceeded`.

The one-second rule is a ceiling, not a required busy tick. The warning is emitted once, no later than one second after 80 percent of the configured timeout. The final timeout begins no later than one second after the configured deadline.

### 4.7 Cleanup and terminal outcomes

All terminal paths converge on one owned-group cleanup routine:

- normal direct-child completion first tests whether the owned group still exists; a surviving group receives the same bounded cleanup used for interruption;
- a natural nonzero exit preserves the exact normal exit code and uses `child-exit`;
- a natural child signal uses `128 + signal` and `child-signal`;
- an external `SIGINT`, `SIGTERM`, or `SIGHUP` is forwarded to the owned group, then becomes `external-signal`;
- a timeout sends `SIGTERM` to the owned group and becomes code 124 with `deadline-exceeded`;
- after `SIGTERM` or a forwarded external signal, the runner waits at most ten monotonic seconds for the group to disappear;
- if the group remains, `SIGKILL` is sent only to that same group, the direct child is reaped, and the same PGID is observed for at most ten additional monotonic seconds until absence is proven;
- a second handled external termination signal during graceful cleanup skips the remaining grace period and sends `SIGKILL` to the same group.

Cleanup never scans the process table and never signals the supervisor's own group. Group checks carry an explicit lifecycle phase: the captured leader identity is mandatory while the leader is expected, a reaped leader permits the original PGID to remain live through same-group descendants, and only after `SIGKILL` may macOS's transient `EPERM` group probe be observed as pending when `getpgid` simultaneously proves the leader absent. `ESRCH` means the owned group is already absent. Reaping uses non-blocking status checks inside the same bounded post-kill observation window; cleanup never enters an unbounded child wait. A persistent permission error, an identity mismatch, inability to reap the direct child, or inability to prove safe ownership produces a redacted cleanup failure. If no primary child/signal/timeout outcome exists, that failure returns internal error 70. If a primary outcome already exists, the stable final line preserves it and emits a separate redacted cleanup warning; the lock remains represented as orphan recovery if safe release cannot be established.

The lease is unlocked only after the group is absent or ownership can no longer be proven safely. The persisted child-group record is cleared and synchronized before ordinary unlock when absence is proven.

For an external signal, the runner writes the optional result, emits its final result line, clears handlers, releases proven-owned resources, restores default signal behavior, and re-raises the same signal. If re-raising fails, it returns internal error 70.

### 4.8 Panic boundary

`src/main.rs` places the managed-run orchestration behind `catch_unwind`. A guard owns the signal listener, optional group identity, and lease. Its explicit cleanup method is used on every expected path; its `Drop` performs only best-effort signaling, listener shutdown, and advisory-lock release during unwinding. After best-effort cleanup, a caught panic emits internal error 70. No panic payload is printed because it may contain repository data.

### 4.9 Terminal and structured results

Every `run` emits exactly one final stable line on Agent Lowmem's stderr namespace:

```text
agent-lowmem: result origin=<origin> code=<code> reason=<reason>
```

Child streams remain inherited and are never captured, prefixed, parsed, or redacted. Agent Lowmem warnings and disclosures use the `agent-lowmem:` prefix.

#### 4.9.1 CLI visual identity

The terminal UI derives from the established `agentlowmem.dev` branding: compact monospace typography, the lowercase `agent_lowmem` wordmark, restrained spacing, and a dark-neutral presentation whose only decorative accent is the brand gradient. A terminal cannot reproduce CSS geometry, so the approved `120deg` gradient is represented as a left-to-right TrueColor interpolation across visible wordmark or section-label characters using these exact stops:

```text
0%   #c9b6ff
38%  #8b83ff
70%  #4f6cff
100% #50d8ff
```

Color is progressive enhancement, never information. It may decorate the interactive wordmark, prompt marker, section labels, and progress accents. Success, warning, and failure retain distinct semantic styling and text labels; the gradient never replaces those meanings. The CLI does not attempt to set or bundle a font because terminal typography belongs to the user's emulator, but its layout assumes the same monospace character model used by the site.

ANSI output is allowed only when the destination stream is an interactive terminal, `NO_COLOR` is absent, `TERM` is not `dumb`, and 24-bit color support is positively identified. Unsupported or ambiguous terminals receive identical plain text. Every styled span resets immediately. Child output is untouched.

The stable `agent-lowmem: result ...` line, `--json`, `--json-file`, redirected output, snapshots, and machine-readable diagnostics never contain ANSI escapes. Disabling color must not change words, ordering, whitespace, exit status, or the number of emitted lines.

`src/result_file.rs` writes `schemas/result-v1.schema.json` records to the explicit `--json-file` destination. The destination must be a lexical repository-relative path beneath the canonical Git root. Its existing parents must be real directories beneath that root. A symlink or special-file destination is rejected. A missing file or existing regular file may be replaced atomically by a same-directory temporary regular file created with mode `0600`; the temporary file and parent directory are synchronized before success is reported.

The result record includes only:

- schema version and UTC RFC 3339 timestamp;
- origin, code, reason, stable message, safe next action, and `childStarted`;
- operation key and optional workspace key;
- package-manager kind and exact declared version;
- repository-relative evidence identities and SHA-256 digests;
- graph depth and leaf count;
- stable applied-control and disclosure identifiers;
- configured timeout, whether the warning was emitted, and elapsed whole milliseconds;
- lock acquisition, evidence recheck, spawn, cleanup action, and cleanup completion states;
- forwarded argument count, never argument values.

The result omits executable paths, raw launch arguments, raw scripts, package output, environment data, username, absolute paths, PIDs, process-group IDs, dotenv paths, assignment names or values, memory pressure, and process resource samples.

The JSON destination is validated before lock acquisition. A failure to write a requested preflight result returns internal error 70. After a child has started, a late JSON write failure emits a redacted warning but does not replace the already determined child, signal, or timeout result. This preserves the exact managed-command outcome.

## 5. End-to-end data flow

```text
RunRequest
  -> host capability gate and optional unvalidated-performance notice
  -> configured operation and workspace selection
  -> evidence-backed RunPlan A
  -> nested-invocation check
  -> non-blocking per-user lease acquisition
  -> evidence-backed RunPlan B under the lease
  -> exact evidence and typed-policy comparison
  -> signal listener installation
  -> one npm/pnpm spawn in a new process group
  -> lock record update with owned group identity
  -> child, signal, warning, and timeout supervision
  -> owned-group cleanup and direct-child reap
  -> optional atomic JSON result and stable stderr result line
  -> lock-record clearing and lease release
  -> optional external-signal re-raise
```

No stage before the package-manager spawn starts a repository child. Planning B does not reuse parsed objects or file bytes from planning A.

## 6. Error mapping

Phase 3 adds no reason or origin. It uses the closed 31-reason vocabulary already shared by `src/result.rs` and `schemas/result-v1.schema.json`.

| Event | Origin | Code | Reason | Child started |
| --- | --- | ---: | --- | --- |
| Invalid run syntax | `preflight` | 2 | `invalid-cli` | false |
| Invalid JSON destination syntax | `preflight` | 2 | `invalid-cli` | false |
| Unsupported host, repository, operation, workspace, script, adapter, version, or arguments | `preflight` | 64 | Existing specific reason | false |
| Live lock or malformed unlocked record | `preflight` | 73 | `lock-held` | false |
| Inherited active marker | `preflight` | 73 | `nested-invocation` | false |
| Post-lock evidence or policy difference | `preflight` | 75 | `evidence-changed` | false |
| Successful child | `child` | 0 | `completed` | true |
| Normal nonzero child | `child` | Exact child code | `child-exit` | true |
| Naturally signaled child | `child` | `128 + signal` | `child-signal` | true |
| Supervisor deadline | `supervisor-timeout` | 124 | `deadline-exceeded` | true |
| External handled signal | `external-signal` | `128 + signal` | `external-signal` | true |
| Spawn, lock I/O, signal setup, required identity, clock, or preflight result-write failure | `internal` | 70 | `internal-error` | false until spawn, true after spawn |

Normal child exit and natural child signal are distinguished with the Unix exit-status API, not by interpreting an integer greater than 128 after the fact.

## 7. Dependency boundary

No dependency is approved merely by this design. The implementation plan begins with separate source/API, license, MSRV, feature, transitive-dependency, and release-size reviews. Candidate responsibilities are:

- `sha2`: pure-Rust SHA-256 for evidence and repository identities;
- `rustix`: no-follow filesystem operations, effective-user identity, advisory locking, and process-group signals through safe interfaces;
- `signal-hook`: safe `SIGINT`, `SIGTERM`, and `SIGHUP` delivery without an async runtime;
- `libproc`: macOS PID start identity without first-party unsafe code or process-table enumeration;
- a pinned Rust-1.85-compatible UTC formatter only if an existing approved dependency cannot emit the schema's RFC 3339 timestamp without expanding scope.

Only required crate features are enabled. Exact versions are committed in `Cargo.lock` and recorded in `docs/dependencies-v1.md`. A candidate that requires Rust later than 1.85, a network client, async runtime, build-time bindgen on the user's machine, broad process enumeration, an incompatible license, or unacceptable binary/RSS growth is rejected or replaced before runner code depends on it.

## 8. Testing strategy

### 8.1 Pure unit tests

Injected fake clock, child, signal source, group controller, lease store, evidence reader, result writer, and output sink cover:

- every accepted and rejected CLI ordering;
- exact operation and workspace selection with forwarded arguments;
- deterministic evidence ordering and SHA-256 comparison;
- policy drift even when file identities are unchanged;
- evidence drift even when parsed semantics would be equivalent;
- signal-before-tick, child-before-signal, child-at-deadline, and timeout ordering;
- one 80-percent warning;
- one-second steady-state ceiling without busy polling;
- second-signal escalation;
- normal exit, natural signal, timeout, external signal, spawn failure, panic, and cleanup failure mappings;
- redacted human and JSON output;
- exact brand-gradient interpolation, immediate ANSI resets, and plain-text parity under `NO_COLOR`, non-TTY, `TERM=dumb`, or missing TrueColor support;
- proof that the stable result line and every structured output remain byte-free of ANSI escape sequences;
- exact result-schema validation.

### 8.2 Filesystem and lock integration tests

Temporary repositories and per-test lock directories cover:

- mode `0700` runtime directory and mode `0600` lock/result files;
- rejection of symlinked path components, special files, escaped paths, wrong ownership, and malformed records;
- live contention between two Agent Lowmem processes in different repositories;
- stale record replacement;
- exact parent and leader start-identity matching;
- orphan-recovery blocking without automatic signaling;
- nested invocation through `AGENT_LOWMEM_ACTIVE=1`;
- atomic result replacement and interrupted temporary writes.

### 8.3 Runner integration tests

Test-only executable fixtures stand in for npm and pnpm and record only fixture-approved markers. They prove:

- all initial and post-lock inspection sentinels remain unstarted;
- a deterministic barrier can mutate one evidence file after lease acquisition and before planning B, producing code 75 and no child;
- the expected executable and argument-array boundaries reach the child without a shell created by Agent Lowmem;
- stdin, stdout, and stderr are inherited;
- only the owned group receives forwarded, timeout, and escalation signals;
- direct-child and descendant processes are reaped or absent after completion;
- natural exit codes and natural signals are preserved;
- Ctrl-C produces the external-signal result before the supervisor re-raises `SIGINT`;
- requested JSON never enters child stdout or stderr.

Integration-only synchronization hooks are compiled behind `cfg(test)` and are unavailable in production.

### 8.4 Resource and source gates

The Phase 3 exit gate runs sequentially with one Cargo job and one test thread. It requires:

- release parent peak RSS at or below 24 MiB during supervision;
- stripped release binary at or below 12 MiB;
- no more than 1,800 steady-state child/deadline checks while supervising `/bin/sleep 1800`;
- no daemon, listener thread, lock owner, direct child, or proven member of the original group after normal completion, timeout, handled interruption, spawn failure, or caught panic;
- zero repository child starts during `doctor`, planning A, and planning B;
- source audits rejecting Tokio, async-std, network clients, shell-string construction, pressure reads, process-table enumeration, and first-party unsafe code;
- complete `cargo fmt`, `cargo clippy -D warnings`, unit/integration tests, release build, result-schema validation, binary-size measurement, RSS measurement, and dependency/license/advisory audits.

The long-running wakeup measurement is a release-only ignored test or dedicated measurement tool so the ordinary suite remains practical on the 8 GiB reference Mac.

## 9. File ownership

Expected production additions:

- `src/evidence.rs`: exact-byte evidence reads, digests, and comparison;
- `src/lock.rs`: per-user runtime directory, advisory lease, redacted record, and doctor probe;
- `src/process.rs`: one package-manager spawn and owned-group operations;
- `src/supervisor.rs`: signal/deadline state machine and cleanup;
- `src/run.rs`: preflight, recheck, orchestration, and outcome assembly;
- `src/result_file.rs`: redacted v1 record and atomic JSON destination;
- `tests/run_cli.rs`: end-to-end CLI and child-boundary coverage;
- `tests/run_lock.rs`: cross-process lock and orphan-record coverage;
- `tests/run_supervision.rs`: signals, deadlines, groups, cleanup, and exit preservation;
- `tests/fixtures/runner/`: test-only npm/pnpm and descendant fixtures.

Expected modifications:

- `src/lib.rs`: export Phase 3 modules;
- `src/cli.rs`: parse `RunRequest`;
- `src/repository.rs`: produce `RunPlan` from evidence-backed reads;
- `src/policy.rs`: expose all typed comparison and redacted-result fields without exposing scripts or arguments;
- `src/doctor.rs`: report managed-run availability and redacted lock state;
- `src/main.rs`: dispatch `run`, install the unwind boundary, and preserve final exit behavior;
- `src/result.rs`: add structured record types without changing reason/origin vocabulary;
- `schemas/result-v1.schema.json`: close and validate Phase 3 `details` fields while preserving schema version 1;
- `Cargo.toml`, `Cargo.lock`, and `docs/dependencies-v1.md`: record only dependencies that pass the explicit review.

Files may be split further only when a listed responsibility becomes too large to review. Runtime orchestration must not absorb repository parsing, lock mechanics, signal implementation, or JSON filesystem operations.

## 10. Phase 3 acceptance criteria

Phase 3 is complete only when:

1. `run` accepts only the grammar in §1 and refuses arbitrary commands.
2. Unsupported host or repository input starts no child and maps to the existing closed result vocabulary.
3. Planning A and planning B use exact bytes from child-free evidence reads.
4. Any relevant byte, policy, workspace-cardinality, or classification difference after acquisition returns 75, releases the lease, and starts no child.
5. Two managed operations across different repositories cannot run concurrently for the same user.
6. `AGENT_LOWMEM_ACTIVE=1` returns 73 with `nested-invocation`.
7. The only repository child is the selected npm/pnpm executable with the immutable Phase 2 argument array, inherited stdio, inherited environment, and a new process group.
8. Agent Lowmem prints or persists no raw script, forwarded argument, environment value, dotenv path, wrapper assignment, username, or absolute repository path.
9. The 80-percent warning appears once within one second and the final deadline begins within one second of the configured timeout.
10. Timeout sends `SIGTERM`, waits at most ten seconds, and escalates only the owned group to `SIGKILL`.
11. External `SIGINT`, `SIGTERM`, and `SIGHUP` are forwarded immediately, cleanup completes, the structured result is attempted, and the original signal is re-raised.
12. Natural normal exits and signals preserve their exact observable code and distinct origin/reason.
13. Normal completion, timeout, handled interruption, spawn failure, and caught panic leave no Agent Lowmem listener thread, live lease, direct child, or proven owned-group member.
14. `doctor` reports only the redacted four-state lock status and remains zero-child.
15. Optional JSON is atomic, mode `0600`, schema-valid, separate from child streams, and contains only the fields in §4.9.
16. The production package remains Rust-only, network-free, async-runtime-free, daemon-free, pressure-free, and first-party-unsafe-free.
17. Sequential tests, lint, release build, schema checks, dependency audits, 24 MiB RSS gate, 12 MiB binary gate, and the 1,800-check supervision gate all pass on the reference Mac.

## 11. Deferred work

Phase 4 owns `init`, `init --dry-run`, `restore`, `restore --dry-run`, deterministic `.agent-lowmem.json`, managed `AGENTS.md` blocks, and private restoration manifests.

Phase 5 owns README and website completion, CI matrices, npm launcher, Homebrew formula, signing, notarization, checksums, provenance, release automation, tag, and the first public release.

Pressure-triggered behavior, process footprint telemetry in the production runner, heap policy, sequential orchestrator expansion, additional ecosystems, escaped-process recovery, and automatic orphan termination require later evidence and separate design revisions.

## 12. Authoritative API references

- [Rust `CommandExt::process_group`](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html) for creating the child process group without a pre-exec closure.
- [Apple `killpg(2)`](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/killpg.2.html) for Darwin process-group signal semantics.
- [Rustix `flock`](https://docs.rs/rustix/latest/rustix/fs/fn.flock.html) and [`kill_process_group`](https://docs.rs/rustix/latest/rustix/process/fn.kill_process_group.html) for candidate safe wrappers.
- [Signal Hook iterator](https://docs.rs/signal-hook/latest/signal_hook/iterator/) for synchronous signal delivery without an async runtime.
- [Libproc process information](https://docs.rs/libproc/latest/libproc/proc_pid/fn.pidinfo.html) and [resource start identity](https://docs.rs/libproc/latest/libproc/pid_rusage/struct.RUsageInfoV4.html) for candidate macOS process identity.
- [RustCrypto SHA-2](https://docs.rs/sha2/latest/sha2/) for candidate SHA-256 evidence hashing.
