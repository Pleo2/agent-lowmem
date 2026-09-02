# Agent Lowmem v1 Design

**Status:** Candidate for final design review

**Date:** 2026-09-02

**Revision:** 4 — deterministic, evidence-first MVP

**Product:** Agent Lowmem

**Domain:** `agentlowmem.dev` (acquired 2026-09-02)

**Repository and npm package:** `agent-lowmem`

**Tagline:** More agents. Less RAM.

## 1. Summary

Agent Lowmem is an open-source native command-line policy runner for coding-agent workloads on memory-constrained Macs. The production CLI is written in Rust. Version 1 is validated on the owner's Apple M2 MacBook Air with 8 GiB of unified memory (`Mac14,15`, 16 KiB pages) and macOS 26.x, in JavaScript and TypeScript repositories that use Node.js, npm or pnpm, Next.js, NestJS, Vitest, or Jest.

Version 1 enforces only controls whose behavior is deterministic and independently testable:

- one Agent Lowmem-managed heavy operation per local user;
- no watch mode or recognized background execution;
- one test worker where the detected tool version exposes a verified public option;
- bounded wall-clock time;
- focused validation guidance before broad suites;
- owned process-group cleanup on timeout, interruption, or supervisor failure.

Version 1 does **not** set a Node heap size, infer a safe memory budget, block launch from a private pressure snapshot, or terminate a command in response to an unvalidated memory-pressure signal. Those mechanisms previously created a stronger promise than the available evidence supported.

The v1 promise is deliberately narrow:

> Reduce avoidable memory contention on the validated 8 GiB Mac by serializing supported heavy work and applying verified low-concurrency execution rules.

This is risk reduction, not a guarantee that every build will complete or that macOS will never swap, beachball, freeze, or terminate a process.

## 2. Problem

Coding agents frequently launch tests, type checks, builds, package managers, browser tooling, and helpers concurrently. On an 8 GiB Mac, the editor, browser, agent client, MCP processes, and operating system already consume much of the available memory before repository validation begins.

Existing controls are informal instructions scattered across repositories. They depend on every agent remembering to:

- run only one heavy operation at a time;
- disable watch mode;
- request one test worker;
- prefer a focused test before a full suite;
- use a timeout;
- stop only task-owned processes.

Agent Lowmem turns those practices into a reusable repository policy and a local execution boundary. It does not pretend that macOS offers a cgroup-equivalent hard memory limit for an arbitrary descendant tree.

## 3. Goals

Version 1 must:

1. Identify the validated reference host exactly and report other macOS arm64 hosts as observation-only.
2. Generate an idempotent, clearly delimited Agent Lowmem block in the Git root's `AGENTS.md`.
3. Detect npm or pnpm, configured workspaces, supported scripts, and supported tool versions from repository evidence without executing repository code during inspection.
4. Run one Agent Lowmem-managed heavy operation at a time across the local user session.
5. Reject watch mode, recognized background execution, nested Agent Lowmem invocation, and unsupported concurrency controls.
6. Apply one-worker or serial flags only through a version-tested public interface.
7. Re-inspect package scripts and lifecycle scripts for every run so repository drift cannot silently bypass the launch policy.
8. Preserve the caller's environment, including `NODE_OPTIONS`, without adding or rewriting a heap limit.
9. Launch the package manager with an argument array, inherit terminal streams, and supervise only the process group Agent Lowmem creates.
10. Warn at 80% of the configured timeout and terminate the owned process group at the deadline.
11. Explain each decision in human-readable output and optionally write a structured result without mixing JSON into child output.
12. Leave no daemon, service, global environment variable, permanent macOS setting, or background monitor behind.
13. Restore managed repository files through a preview-first command, including a narrow explicit escape hatch for an edited managed block.

## 4. Non-goals and explicit limitations

Version 1 will not:

- tune the macOS kernel, swap, compressed memory, launch agents, or system settings;
- kill or reconfigure browsers, editors, MCP servers, containers, or unrelated processes;
- modify global shell profiles, npm configuration, Node installations, package-manager stores, or inherited `NODE_OPTIONS`;
- assign green, amber, or red health from `kern.memorystatus_vm_pressure_level`;
- use a memory-pressure event as a preflight or termination boundary;
- enforce a per-process or process-tree memory cap;
- promise that every full build can complete locally;
- promise that the Mac will never swap, beachball, freeze, or reach OOM;
- attribute system-wide pressure to the managed command;
- retry an OOM, failed command, interrupted command, or timeout automatically;
- clean partial build artifacts using guessed paths;
- support Intel Macs, Linux, Windows, Docker orchestration, Python, Rust, Java, Flutter, or mobile toolchains in the native v1 runner;
- claim validation for M1, M3, M4, a different Mac model, another memory size, or another macOS major version;
- support `init` or `run` outside a Git-backed JavaScript or TypeScript repository;
- coordinate multiple autonomous agents beyond publishing and enforcing repository policy;
- prevent a user or agent from bypassing the policy by invoking package-manager commands directly;
- guarantee cleanup of a descendant that deliberately or indirectly escapes the owned process group with `setsid` or a new process group.

The last limitation is important: Agent Lowmem signals the process group it created. A process that escapes that group is no longer inside the ownership boundary and may survive cleanup. V1 reports this boundary and never scans for similarly named processes to compensate.

## 5. Users and primary scenario

The primary user is a developer using a coding agent on the validated Apple M2 MacBook Air with 8 GiB of unified memory.

The primary scenario is:

1. The developer opens a Git-backed JavaScript or TypeScript repository.
2. `agent-lowmem doctor` reports host support, repository support, available operations, and current lock state.
3. `agent-lowmem init --dry-run` previews policy and configuration changes.
4. `agent-lowmem init` writes the approved managed files.
5. The coding agent reads `AGENTS.md` and starts with a focused validation command.
6. `agent-lowmem run test -- path/to/file.test.ts` or another configured operation acquires the global lock and applies the adapter's verified low-concurrency strategy.
7. The command completes, fails normally, is interrupted, or reaches its timeout; Agent Lowmem reports the origin and releases owned resources.

For broad Next.js or NestJS builds whose internal fan-out cannot be controlled through a verified interface, output states that limitation before launch and recommends CI when local interactivity matters more than completing the full build.

## 6. Product principles

### Evidence before enforcement

An observed signal does not become a termination boundary until prospective measurements show that it is timely and sufficiently specific on the validated host. Unknown pressure timing and an unmeasured heap number are not safety controls.

### Deterministic controls first

Serialization, no-watch rules, verified test-worker flags, timeouts, and process ownership have observable behavior without learned thresholds. They form the v1 policy.

### Safety before throughput

The laptop remaining usable is more important than maximizing local build speed. Focused validation and CI are valid outcomes when a broad local operation cannot be constrained honestly.

### Explicit and reversible behavior

Every repository write is previewable and delimited. Runtime controls are command-scoped. No global tuning is allowed.

### Evidence over guessed flags

Adapters identify the selected tool and installed version from repository files. They apply only options supported by a tested version range. An unknown version returns unsupported rather than inheriting a plausible flag from another version.

### Ownership boundaries

Agent Lowmem may signal only the process group it created. It does not infer ownership from names, ports, paths, or resource consumption.

## 7. User interface

The executable is `agent-lowmem` and exposes four v1 commands.

### `agent-lowmem doctor`

Inspects the host and repository without writing files or executing repository code.

```text
agent-lowmem doctor
agent-lowmem doctor --json
```

It reports:

- operating system, architecture, exact hardware identity, physical memory, page size, and validated-profile status;
- whether `init` and `run` are supported on the host;
- Git root availability without printing the absolute root in structured output;
- detected package manager, workspaces, configured scripts, installed tool versions, and adapter support;
- fixed v1 controls and known internal fan-out limitations;
- whether another Agent Lowmem operation owns the per-user lock;
- the next recommended action.

`doctor` does not report a current memory-health color and does not read the private pressure-level sysctl. It succeeds with `runSupported: false` on an unvalidated macOS arm64 host or outside a repository, provided the inspection itself completes.

### `agent-lowmem init`

Creates or updates repository policy.

```text
agent-lowmem init --dry-run
agent-lowmem init
```

The command:

1. requires the validated v1 host, a Git repository, and a root `package.json`;
2. performs the same repository inspection as `doctor`;
3. previews exact changes when `--dry-run` is supplied;
4. writes `.agent-lowmem.json`;
5. inserts or replaces one managed block in the Git root's `AGENTS.md`;
6. writes a private restoration manifest inside the repository's resolved Git metadata directory;
7. emits no timestamp or absolute path into managed repository files;
8. remains byte-for-byte idempotent for the same CLI version, configuration, and repository evidence.

The Git root and metadata directory are resolved by walking parents and interpreting either a `.git` directory or a worktree pointer file. Inspection does not spawn `git`.

### `agent-lowmem run`

Runs a supported configured operation.

```text
agent-lowmem run test
agent-lowmem run test -- path/to/file.test.ts
agent-lowmem run test --workspace web -- path/to/file.test.ts
agent-lowmem run typecheck
agent-lowmem run lint
agent-lowmem run build
agent-lowmem run build --json-file .agent-lowmem-result.json
```

Only operations present in `.agent-lowmem.json` and scripts present in the selected `package.json` may run. `--workspace <key>` must match a stable configured key. Arbitrary shell commands are outside v1. Extra arguments are forwarded as argument-array elements without shell interpolation by Agent Lowmem.

The trust boundary is explicit: Agent Lowmem starts npm or pnpm without constructing a shell string, but those package managers execute package scripts using their documented shell semantics. Agent Lowmem is not a sandbox for repository code.

`run` requires the validated profile. It acquires the global lock, revalidates the relevant manifests and scripts, and releases the lock without starting a child if evidence changed during launch planning. Child stdin, stdout, and stderr are inherited. Agent Lowmem JSON is therefore available only through `--json-file <path>`.

Test, typecheck, and lint operations default to 15 minutes. Builds default to 30 minutes. Each operation prints one warning at 80% and is terminated at its deadline. Timeouts may be configured from 60 through 3,600 seconds.

### `agent-lowmem restore`

Removes Agent Lowmem-managed repository changes.

```text
agent-lowmem restore --dry-run
agent-lowmem restore
agent-lowmem restore --dry-run --force-managed-block
agent-lowmem restore --force-managed-block
```

`restore` never requires a validated host. With a restoration manifest, it verifies exact managed bytes and restores or removes `.agent-lowmem.json` and the managed `AGENTS.md` block without touching surrounding content.

After a fresh clone without a private manifest, it may remove a managed block whose exact body hash matches its marker. It removes `.agent-lowmem.json` only when the file exactly matches deterministic current output; otherwise it preserves the file for manual review.

If the block body was edited or reformatted, ordinary restore returns conflict code 78. `--force-managed-block` is a narrow escape hatch: it removes exactly one well-formed start-to-end Agent Lowmem block even when its body hash differs. It never removes text outside the markers, follows no symlink, reconstructs no unknown prior content, and never forces removal of a conflicting `.agent-lowmem.json`. Dry-run displays the exact affected byte range before the destructive form is used.

## 8. Architecture

### 8.1 Production language boundary

The shipped CLI and its production libraries are Rust. They do not link a Swift helper, require a Swift runtime, or invoke the experimental probe.

The repository contains `tools/pressure-probe`, a Swift research instrument with no third-party package dependencies. Swift was selected for that probe because the public macOS Dispatch memory-pressure API is directly exposed there and the reference Mac already had Swift 6.3 while Rust was not installed. The probe is observational, is excluded from release artifacts, and does not define the production architecture.

If evidence later justifies pressure-based behavior, it requires a new design revision and a narrow Rust macOS integration. The safe Rust core remains independent of Dispatch. Any required FFI or `unsafe` code must be isolated in one platform module, documented with safety invariants, audited, and tested separately; it cannot enter by silently weakening a workspace-wide rule.

### 8.2 Rust workspace

The Rust workspace uses edition 2024 with Rust 1.85 as its minimum supported Rust version. Stable release and CI toolchains use a committed `Cargo.lock`.

The production design separates:

- CLI parsing and human/structured presentation;
- host capability inspection;
- repository and package evidence inspection;
- adapter selection and launch-plan construction;
- managed-file generation and restoration;
- per-user locking;
- owned process-group lifecycle and timeout supervision.

First-party production crates compile with `#![forbid(unsafe_code)]` in v1. Platform operations must enter through reviewed safe standard-library or dependency interfaces. Direct runtime dependencies require a documented purpose, source review, license approval, and a version committed in `Cargo.lock`; this spec does not pre-approve a crate merely by naming it.

The supervisor uses no Tokio, async-std, daemon, resident service, or polling of the system process table. Its steady-state child loop performs only child-status, signal, warning-deadline, and timeout work.

Release builds use link-time optimization, one code-generation unit, symbol stripping, and `panic = "unwind"`. A top-level unwind boundary performs best-effort owned process-group and lock cleanup before returning internal error 70.

### 8.3 Host inspector

The validated v1 compatibility key is the conjunction of:

- `darwin` operating system;
- `arm64` architecture;
- hardware model exactly `Mac14,15`;
- CPU brand exactly `Apple M2` after trimming terminating whitespace;
- physical memory exactly `8,589,934,592` bytes;
- page size exactly `16,384` bytes;
- macOS product-version major exactly `26`.

The exact model and brand checks prevent an `Apple M2 Pro`, `Apple M2 Max`, or another M2 Mac from passing by prefix. Minor and patch releases within macOS 26 are accepted only when every required property remains readable with its expected type. An unavailable or differently shaped property makes the host unvalidated; it is never guessed.

These native properties are compatibility-sensitive profile identifiers, not memory-health signals. `doctor` may display their availability on another macOS arm64 host, but `init` and `run` return unsupported code 64 when the complete key does not match.

### 8.4 Repository inspector

The inspector:

- locates the Git root and root `AGENTS.md` without executing `git`;
- parses root and selected-workspace `package.json` files as data;
- identifies npm or pnpm from `packageManager` plus the matching lockfile;
- enumerates declared workspaces and requires an explicit stable key;
- resolves a supported tool's installed `package.json` without executing the package;
- compares its exact semantic version with the committed adapter matrix;
- reads the selected target plus its `pre<name>` and `post<name>` lifecycle scripts on every run;
- rejects ambiguous package-manager evidence, missing scripts, shell control operators it cannot classify safely, watch commands, background execution, and unsupported orchestrators;
- returns explicit unsupported or conflict states instead of guessing.

The v1 script grammar is deliberately small. A managed phase must resolve to one direct supported executable plus literal arguments accepted by that adapter. The committed adapter matrix defines allowed executable names, version ranges, arguments, and denial tokens. Shell pipelines, lists, redirections, substitutions, background markers, shell functions, and unrecognized wrappers are unsupported. Forwarded CLI arguments are checked by the same adapter, so `--watch` cannot be reintroduced after `--`.

`init` does not freeze lifecycle-script contents into configuration. A lifecycle phase is safe only when the same parser and adapter matrix classify its direct command as `controlled` or explicitly `disclosed`; otherwise it blocks the run. A safe addition is incorporated into the current launch plan and reported as repository drift. This avoids forcing a new `init` merely because a classifiable `pretest` was added.

Before locking, the launch plan records hashes of the relevant manifests. After acquiring the lock, Agent Lowmem re-reads those files. If any hash differs, it releases the lock, launches nothing, returns temporary-failure code 75, and recommends rerunning the command.

### 8.5 Fixed execution policy

Every launch plan contains:

- exactly one selected package manager and configured operation;
- selected workspace and current lifecycle phases;
- an argument array rather than a shell command built by Agent Lowmem;
- adapter-provided serial or non-watch options;
- a classification of internal fan-out as controlled, disclosed, or unsupported;
- the per-user lock requirement;
- the 80% warning point and final timeout;
- explanations for terminal and structured output.

The fixed v1 controls are:

- one managed heavy operation per local user;
- watch mode denied;
- recognized background execution denied;
- one worker where a verified adapter option exists;
- no automatic retry;
- no `NODE_OPTIONS` mutation;
- no pressure preflight, polling, or pressure-triggered termination;
- a bounded timeout and owned group cleanup.

The engine neither inspects environment values for policy nor mutates the caller's shell environment.

### 8.6 Tool adapters

Adapters cover:

- npm and pnpm script forwarding;
- Vitest non-watch, one-worker execution for tested versions;
- Jest `--runInBand` execution for tested versions;
- direct Next.js build recognition with explicit uncontrolled-internal-fan-out disclosure;
- direct NestJS build recognition with explicit bundler-fan-out disclosure when applicable;
- direct TypeScript typecheck and lint scripts whose command form is classifiable.

An adapter may inject an option only when the installed version falls inside its tested compatibility range and the script form makes forwarding unambiguous. An unknown version, a compound shell pipeline, Turbo/Nx-style orchestration, or a script whose concurrency cannot be classified returns unsupported code 64 unless it is one of the explicitly disclosed Next.js or NestJS cases.

Next.js and NestJS are not described as single-worker builds unless a public version-tested interface proves that claim. On the reference 8 GiB host, their launch message recommends CI for broad builds when internal fan-out is disclosed. Agent Lowmem still prevents a second managed top-level operation, but it does not represent that lock as control over framework workers.

### 8.7 Global operation lock

A per-user lock prevents two Agent Lowmem heavy operations from starting in different repositories.

The lock is created exclusively with user-only permissions in the macOS per-user temporary directory and records:

- owner PID and process-start identity;
- repository-path hash, never the raw path;
- command category;
- owned child process-group identity after launch;
- acquisition time.

A lock is stale only when both PID and process-start identity no longer match a live process. If a stale parent lock references a still-live child group, `doctor` reports an orphan-recovery condition and `run` remains blocked. Version 1 never kills that group automatically because parent death makes proof of continuing ownership weaker.

A nested command detects `AGENT_LOWMEM_ACTIVE=1` before lock acquisition and returns code 73 with reason `nested-invocation`. The opaque run identifier is never printed.

### 8.8 Managed runner

The runner starts npm or pnpm in a new process group. Child stdin, stdout, and stderr remain attached to the caller's terminal; Agent Lowmem does not buffer them.

The parent installs signal handling before spawn. External `SIGINT`, `SIGTERM`, and `SIGHUP` are forwarded to the owned process group. After cleanup and optional structured-result writing, the parent restores the default disposition and re-raises the same signal so the shell observes the conventional status, including 130 for Ctrl-C.

The supervisor checks child state and deadlines at no more than four wakeups per second. It does not sample memory, enumerate processes, or poll private pressure state. At 80% of the timeout it prints one warning. At the deadline it sends `SIGTERM` to the owned process group, waits up to ten seconds, and sends `SIGKILL` only to the same group if members remain.

The lock is released only after the group exits or ownership can no longer be proven. A child that escaped the group may remain and is reported as outside the enforceable boundary. Agent Lowmem does not delete partial artifacts.

### 8.9 Policy-file manager

The managed `AGENTS.md` block uses versioned markers and an exact byte hash:

```markdown
<!-- agent-lowmem:start format="1" content-sha256="<sha256>" -->
## Agent Lowmem resource policy

Run supported heavy validation through Agent Lowmem. Run only one heavy
operation at a time, never use watch mode, and prefer focused validation
before broad suites. Do not retry OOM or timeout failures automatically.
Agent Lowmem v1 does not impose a memory cap or guarantee responsiveness;
use CI when a broad build cannot be constrained locally.
<!-- agent-lowmem:end -->
```

The real marker contains the lowercase SHA-256 digest of the exact UTF-8 body bytes between the marker lines. Generated files use LF endings. There is no CommonMark parser and no semantic canonicalization dependency. A formatter change inside the block is therefore an intentional conflict handled by ordinary refusal or explicit `restore --force-managed-block`.

Generation is deterministic and contains repository-specific commands and workspace keys but no timestamp, username, or absolute path. Agent Lowmem never rewrites content outside the markers.

## 9. Configuration

`.agent-lowmem.json` is committed so humans and agents share one operation allowlist.

```json
{
  "$schema": "https://agentlowmem.dev/schema/v1.json",
  "version": 1,
  "packageManager": "pnpm",
  "operations": {
    "test": { "script": "test", "timeoutSeconds": 900 },
    "typecheck": { "script": "typecheck", "timeoutSeconds": 900 },
    "lint": { "script": "lint", "timeoutSeconds": 900 },
    "build": { "script": "build", "timeoutSeconds": 1800 }
  }
}
```

`init` includes only present scripts. A user may remove an operation to make it unavailable. Lifecycle phases are current repository evidence and are not duplicated in configuration.

A monorepo adds stable workspace keys:

```json
{
  "workspaces": {
    "web": {
      "path": "apps/web",
      "packageManagerSelector": "@acme/web",
      "operations": {
        "test": { "script": "test", "timeoutSeconds": 900 }
      }
    }
  }
}
```

The schema accepts timeouts from 60 through 3,600 seconds and rejects unknown fields, arbitrary commands, absolute workspace paths, duplicate selectors, and missing scripts. Concurrency, watch denial, lock behavior, environment preservation, retry behavior, and cleanup are fixed implementation policy rather than configurable fields.

## 10. Data flow

```text
CLI request
  -> host capability inspection
  -> repository and configuration inspection
  -> adapter selection and fixed launch plan
  -> relevant-file hash capture
  -> per-user lock acquisition
  -> relevant-file hash recheck
  -> managed process-group launch
  -> child, signal, warning-deadline, and timeout supervision
  -> child completion or owned-group cleanup
  -> lock release
  -> stable human result and optional atomic JSON result
```

If the post-lock hash check fails, Agent Lowmem releases the lock and returns 75 without launching a child. `doctor`, dry-run commands, and `restore` do not acquire the heavy-operation lock.

## 11. Errors and exit results

Errors state what happened, whether a child started, what Agent Lowmem changed, and the safest next action.

| Code | Meaning |
| ---: | --- |
| 0 | Command completed successfully |
| 2 | Invalid CLI usage or configuration schema |
| 64 | Unsupported host, repository, package manager, workspace, script form, tool, or tool version |
| 70 | Internal failure after best-effort owned-resource cleanup |
| 73 | Live operation lock or nested Agent Lowmem invocation |
| 75 | Repository evidence changed during launch planning; retry from fresh inspection |
| 78 | Managed-file or persistent configuration conflict |
| 124 | Managed operation exceeded its configured timeout |
| child | Natural managed-command exit code, preserved exactly |

Every `run` prints one stable final line to Agent Lowmem's stderr namespace:

```text
agent-lowmem: result origin=<origin> code=<code> reason=<reason>
```

`origin` is one of `preflight`, `child`, `supervisor-timeout`, `external-signal`, or `internal`. `preflight` means no repository child started and covers unsupported input, lock contention, configuration conflict, or changed launch evidence. This line and the optional structured result disambiguate a natural child code that equals a reserved wrapper code. Pressure is not an exit origin in v1.

A natural child signal is represented as `128 + signal`. When the user sends `SIGINT`, `SIGTERM`, or `SIGHUP` to Agent Lowmem, the parent forwards, cleans up, and re-raises the signal on itself.

## 12. Security and privacy

Version 1 is local-only and sends no telemetry.

It must:

- compile all first-party production crates with `#![forbid(unsafe_code)]`;
- exclude the Swift pressure probe and raw traces from production packages;
- avoid printing or persisting environment-variable values;
- avoid shell-string construction by Agent Lowmem;
- document that package-manager scripts remain trusted repository code;
- validate configuration against a bundled schema;
- refuse symlink traversal for managed writes and lock creation;
- use user-only permissions plus atomic writes for configuration, manifests, locks, `AGENTS.md`, and JSON results;
- store no raw command line or repository path in the lock;
- make network access unnecessary for `doctor`, `init`, `run`, and `restore` after installation;
- commit `Cargo.lock` and reject unknown licenses, yanked crates, and known advisories in CI;
- publish SHA-256 checksums and GitHub build provenance for release binaries;
- strip before code signing, verify the final signature, and notarize direct-download archives;
- keep npm packages free of lifecycle scripts and network downloaders.

## 13. Observability

`doctor`, `init`, and `restore` accept `--json` on stdout. `run` accepts `--json-file <path>` because child streams are inherited.

Structured output includes:

- schema version and timestamp;
- validated-host result and capability reasons;
- repository-evidence hashes rather than absolute paths;
- selected tool versions and adapter classifications;
- applied serial and no-watch controls;
- disclosed internal fan-out;
- lock, timeout, child, cleanup, exit-origin, and exit-reason data.

It does not include a green, amber, or red health field, a pressure level, a synthetic memory budget, a sampled aggregate footprint, environment values, usernames, raw home paths, or unrelated process information.

This redaction guarantee does not apply to inherited child output, which may contain project paths, stack traces, or tool-defined diagnostics.

## 14. Testing strategy

Repository validation runs sequentially. Formatting, linting, unit tests, integration tests, security checks, and release builds are separate commands; tests use one worker.

### Unit tests

- exact validated-host matching, including rejection of M2 Pro/Max prefix matches;
- observation-only and unsupported-host behavior;
- package-manager, workspace, tool-version, and script-form inspection;
- lifecycle re-inspection and post-lock evidence-change handling;
- adapter version matrices and exact forwarded arguments;
- watch, background, compound-script, and unsupported-orchestrator rejection;
- proof that inherited `NODE_OPTIONS` is unchanged;
- launch-plan explanations and fan-out classification;
- exact-byte managed-block insertion, replacement, conflict, and forced removal;
- live and stale lock decisions;
- signal, timeout, nested invocation, and panic cleanup state machines;
- stable result-line, redaction, and structured-output behavior;
- compile-time first-party `unsafe` prohibition.

### Integration tests

- npm, pnpm, single-package, and monorepo fixtures;
- exact npm workspace and pnpm filter selection;
- safe lifecycle drift accepted and unsafe lifecycle drift rejected;
- a manifest change between planning and lock recheck returning 75 with no child;
- supported and unsupported Vitest/Jest version fixtures;
- Next.js and NestJS disclosure behavior without false single-worker claims;
- nested invocation and two-process lock contention;
- inherited TTY stdio without captured-pipe deadlock;
- exact environment preservation;
- `SIGINT`, `SIGTERM`, and `SIGHUP` forwarding;
- timeout escalation from `SIGTERM` to owned-group `SIGKILL`;
- an escaped-process fixture proving that output states the ownership limitation rather than claiming cleanup;
- normal child exit, child signal, timeout, external signal, and internal failure disambiguation;
- atomic JSON output separate from child streams;
- interrupted file writes, marker upgrades, fresh-clone restore, conflicts, and forced-block restore;
- portable npm installation with platform-specific optional dependencies.

### End-to-end tests

The reference `Mac14,15` M2 8 GiB host validates `doctor`, `init --dry-run`, `init`, one focused test, one typecheck, one small build, Ctrl-C cleanup, timeout cleanup, structured output, and restore.

Tests never intentionally exhaust memory. The Swift pressure campaign is a separate observational experiment and is not part of the product test suite.

### Resource budgets

On the reference host, the release build must satisfy:

- parent-process peak resident memory at or below 24 MiB for `doctor` and `run` supervision;
- stripped `aarch64-apple-darwin` binary at or below 12 MiB;
- npm-launcher plus native-process aggregate peak resident memory at or below 80 MiB before the repository child starts;
- median `doctor` time at or below 100 ms outside a repository over 20 warm-cache runs;
- median `doctor` time at or below 300 ms and p95 at or below 500 ms in a committed single-package reference fixture over 20 warm-cache runs;
- no more than 2 seconds of parent CPU time while supervising `/bin/sleep 1800`;
- no daemon, probe, lock owner, or member of the original owned process group remaining after normal completion, timeout cleanup, or handled external interruption.

Cold-cache repository discovery is measured and published but is not compared with the warm-cache gate. Resource measurements run on AC power with the macOS version, fixture commit, toolchain, and measurement command recorded.

## 15. Acceptance criteria

Version 1 is ready only when:

1. `doctor` matches only the exact `Mac14,15`, `Apple M2`, 8 GiB, 16 KiB-page, macOS 26 profile and reports all others as unvalidated.
2. Production code neither reads `kern.memorystatus_vm_pressure_level` nor claims current memory health.
3. Production code never adds, removes, parses for enforcement, or rewrites a Node heap limit.
4. `init --dry-run` displays exact changes and repeated `init` is byte-for-byte idempotent.
5. A formatter-modified managed block conflicts normally and can be removed only through the narrow forced-block path without altering surrounding text.
6. Configuration contains only allowlisted operations and stable workspace selectors; lifecycle data comes from fresh repository evidence.
7. A relevant manifest change after lock acquisition returns 75, releases the lock, and starts no child.
8. Supported Vitest and Jest versions run without watch mode and with one worker; unknown versions return 64.
9. Next.js and NestJS never receive a false single-worker claim and display CI guidance when internal fan-out is uncontrolled.
10. The child receives inherited `NODE_OPTIONS` unchanged.
11. Two managed heavy operations cannot run concurrently and nested invocation returns 73.
12. The 80% warning is emitted once; timeout returns 124 after signaling only the owned process group.
13. External signals are forwarded and the parent re-signals itself after cleanup.
14. A normal child failure preserves its output and exact code; the stable result line and JSON distinguish origin.
15. An escaped descendant is never claimed as cleaned up or targeted through process-name scanning.
16. `restore` works on unsupported hosts, preserves unrelated content, and implements ordinary and forced-block behavior exactly.
17. All first-party production crates reject `unsafe`, production commands make no network request, and no production package contains the Swift probe.
18. Unit, integration, end-to-end, dependency-policy, and release checks pass sequentially.
19. Native and npm-launcher resource budgets pass on the recorded reference fixture and host.
20. Homebrew, GitHub Release, and the macOS npm platform package contain the same signed native binary; portable npm installation remains non-failing on unsupported platforms.

## 16. Distribution and compatibility

Version 1 is an MIT-licensed Rust binary for `aarch64-apple-darwin`. Homebrew, GitHub Release, and Cargo installation execute the production CLI without Node.js or Swift. The project-local npm route uses Node.js only for a minimal portable launcher.

The primary installation path is:

```text
brew install pleo2/tap/agent-lowmem
agent-lowmem init --dry-run
agent-lowmem init
```

GitHub Releases publish a notarized archive. The fixed release order is compile, test, strip, Developer ID sign, signature verification, notarization, checksum, and provenance. Nothing mutates the binary after signing.

The portable `agent-lowmem` npm root package has no `os` or `cpu` restriction and declares platform packages such as `@agent-lowmem/darwin-arm64` as optional dependencies. The platform package contains the same signed binary and declares its operating-system and CPU restriction.

The launcher resolves the installed platform package, starts its binary with inherited stdio, and returns the child status. It never downloads a binary, runs a lifecycle hook, or builds native code during installation. Unsupported platforms install successfully and receive a clear error only when execution is attempted.

## 17. Documentation deliverables

The first release includes:

- a concise README centered on avoiding unnecessary concurrency on an 8 GiB Mac;
- installation and five-minute quick start;
- the exact validated-host key and unvalidated-host behavior;
- generated `AGENTS.md` policy example;
- supported-tool/version matrix and exact adapter flags;
- focused-first validation examples;
- Next.js and NestJS internal-fan-out limitations and CI guidance;
- explicit statements that v1 has no heap cap, pressure kill, or responsiveness guarantee;
- timeout, recursion, lock, lifecycle-drift, forced-restore, and escaped-process troubleshooting;
- explanation that package scripts remain trusted code and direct package-manager commands bypass enforcement;
- npm platform-support and launcher-overhead documentation;
- security, privacy, and low-resource contribution guidance;
- website copy for `agentlowmem.dev` using the approved name and tagline.

## 18. Pressure research and deferred roadmap

The observational Swift probe and its protocol remain useful, but they no longer block the deterministic v1 design. Their purpose is to decide whether a later design may promote public Dispatch pressure events from research evidence into production behavior.

The pressure experiment must complete its documented baseline, workload, timing, and information-sufficiency gates. Raw traces remain local and ignored. Only an aggregate report may be committed after privacy review.

A future pressure feature requires all of the following:

1. prospective evidence on the exact validated host and macOS build;
2. a stated Outcome A, B, or C from the experiment protocol;
3. no private pressure sysctl in the production contract;
4. a new spec revision defining whether events are informational or enforcing;
5. a narrow audited Rust macOS integration and new resource budgets;
6. separate exit-origin and cleanup tests.

Other deferred work includes:

- an evidence-backed Node heap policy based on workload fixtures rather than a universal constant;
- Linux, Intel Mac, and other Apple Silicon or memory profiles;
- Python, Rust, Java, Flutter, and container adapters;
- quantified process-tree accounting and any future budget enforcement;
- CI recommendation generation;
- interactive dashboards or menu-bar status;
- historical measurements and adaptive per-project policies;
- compatibility blocks for agent formats other than `AGENTS.md`;
- an installable Codex skill layered on top of the stable CLI contract.

The CLI remains the enforcement layer. Future skills teach agents to use it; they do not duplicate process-control logic.

## 19. Revision 4 decision record

| Rev 3 issue | Rev 4 decision |
| --- | --- |
| Pressure timing was unproven | Pressure no longer gates or terminates v1 runs |
| The polled pressure sysctl is private | Production v1 does not read it; research labels it private |
| Public Dispatch events require a queue and are edge-oriented | Dispatch stays in the Swift experiment until evidence and a Rust integration spec exist |
| The 1,024 MiB heap value lacked evidence | The reference Node v24.14.1 reported a 2,240 MiB heap limit, so 1,024 MiB would halve that current default without proving safety; all automatic heap mutation is removed |
| `NODE_OPTIONS` conflicts and script-local overrides were ambiguous | The inherited environment is preserved unchanged |
| Two warning samples could kill a long build in 500 ms | Warning-streak termination is removed |
| Pressure preflight and mid-run termination shared code 75 | Pressure exits are removed; 75 now means only launch evidence changed |
| M2 prefix matching could accept M2 Pro or Max | Model and CPU brand use exact equality |
| Lifecycle changes forced re-init | Lifecycle scripts are fresh run-time evidence and safe drift is reconciled |
| Process-group escape was omitted | It is an explicit non-goal, test fixture, and output limitation |
| Long-run supervisor cost was unspecified | A 30-minute CPU budget and no-process-enumeration rule are release gates |
| Markdown semantic hashing added a parser and upgrade risk | Exact body bytes are hashed; forced block removal is explicit |
| `doctor` timing had no fixture conditions | Host-only and repository-fixture warm-cache budgets are separate |
| Post-lock evidence failure was undefined | It releases the lock, launches nothing, and returns 75 |
| A TTY override was proposed as an agent-proof boundary | V1 has no safety-weakening override because a PTY is not an authority boundary |
| Swift looked like a production-stack change | Swift is research-only; production and distribution remain Rust |

## 20. Brand decision

The canonical brand is **Agent Lowmem**. The product name emphasizes a precise developer problem and the messaging emphasizes capability rather than reduced intelligence.

Canonical identifiers:

- display name: `Agent Lowmem`;
- domain: `agentlowmem.dev`, acquired on 2026-09-02;
- repository: `agent-lowmem`;
- npm package: `agent-lowmem`;
- executable: `agent-lowmem`;
- tagline: `More agents. Less RAM.`

The spelling `lowmem` is mandatory. `lowmen`, `agent-lowmen`, and `agentlowmen` are incorrect.

## 21. Authoritative references

- [Apple Dispatch memory-pressure source](https://developer.apple.com/documentation/dispatch/dispatch_source_type_memorypressure) for the public event interface.
- [Apple Dispatch memory-pressure events](https://developer.apple.com/documentation/dispatch/dispatchsource/memorypressureevent) for normal, warning, and critical transition semantics.
- [Agent Lowmem pressure-signal experiment](../../experiments/2026-09-02-pressure-signal-protocol.md) for the observational evidence gate.
- [Rust `Command`](https://doc.rust-lang.org/std/process/struct.Command.html) for child execution and inherited stdio.
- [Rust Unix `CommandExt`](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html) for Unix child-process configuration.
- [Node.js command-line options](https://nodejs.org/api/cli.html#node_optionsoptions) for inherited `NODE_OPTIONS` behavior.
- [npm scripts](https://docs.npmjs.com/cli/using-npm/scripts) for package-manager shell and lifecycle semantics.
- [pnpm CLI](https://pnpm.io/cli/run) for script forwarding and workspace filters.
- [Vitest CLI](https://vitest.dev/guide/cli) for public run, watch, file-parallelism, and worker controls.
- [Jest CLI](https://jestjs.io/docs/cli) for public `--runInBand` and watch controls.
- [Apple notarization workflow](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) for direct-download release handling.
