# Agent Lowmem v1 Design

**Status:** Proposed for implementation

**Date:** 2026-09-02

**Revision:** 2 — technical review incorporated

**Product:** Agent Lowmem

**Domain:** `agentlowmem.dev` (acquired 2026-09-02)

**Repository and npm package:** `agent-lowmem`

**Tagline:** More agents. Less RAM.

## 1. Summary

Agent Lowmem is an open-source, policy-first native command-line tool for running coding-agent workloads safely on memory-constrained Macs. Its core is written in Rust, while version 1 targets the Apple M2 MacBook Air with 8 GB of unified memory and JavaScript/TypeScript repositories that use Node.js, npm or pnpm, Next.js, NestJS, Vitest, or Jest.

The tool measures native macOS memory-pressure signals, generates repository guidance for agents through a managed `AGENTS.md` block, and launches heavy commands with a conservative whole-process-tree budget and one-worker policy. Node old-space settings are secondary guardrails, not the safety boundary. Agent Lowmem never requires `sudo`, makes no permanent macOS tuning changes, and only controls processes it starts.

The v1 promise is deliberately narrow:

> Keep an 8 GB Mac responsive while an agent runs supported tests, type checks, and builds one heavy operation at a time.

## 2. Problem

Coding agents frequently launch tests, type checks, builds, package managers, browser tooling, and helper processes concurrently. On an 8 GB MacBook, the editor, browser, agent client, MCP processes, and operating system already consume much of the available memory before project validation begins.

Existing controls are usually informal instructions scattered across repositories. They depend on every agent remembering to:

- avoid parallel heavy work;
- disable watch mode;
- use a single test worker;
- set a conservative Node heap for each command;
- prefer focused tests before broad suites;
- stop only task-owned processes when pressure becomes unsafe.

These practices work but are inconsistent, reactive, and difficult to reuse. Agent Lowmem turns them into an observable and repeatable local policy.

## 3. Goals

Version 1 must:

1. Detect whether the host matches a supported calibrated Apple M2 8 GB profile and measure its current memory state.
2. Classify that state as green, amber, or red using documented, deterministic rules.
3. Generate an idempotent, clearly delimited Agent Lowmem policy block in `AGENTS.md`.
4. Detect npm or pnpm and supported JavaScript/TypeScript tools from repository evidence.
5. Run one heavy Agent Lowmem-managed operation at a time across the local user session.
6. Apply a command-scoped managed-process-tree budget, an adaptive Node heap guardrail when useful, and supported single-worker or serial options.
7. Reject watch mode and unmanaged parallelism for heavy commands.
8. Monitor memory while a managed child command runs.
9. Stop only the managed child process tree after critical pressure, calibrated swap thrashing, or a sustained managed-budget violation.
10. Explain every decision in human-readable output and provide structured results without mixing them into child stdout or stderr.
11. Leave no daemon, service, global environment variable, or permanent system change behind.
12. Restore repository files changed by `init` through an explicit preview-first command.

## 4. Non-goals

Version 1 will not:

- optimize macOS kernel, swap, compressed memory, launch agents, or system settings;
- kill browsers, editors, MCP servers, containers, or unrelated processes;
- modify global shell profiles, npm configuration, Node installations, or package-manager stores;
- promise that every full build can complete locally within the safe budget;
- replace CI for broad validation that exceeds the laptop's safe capacity;
- support Intel Macs, Linux, Windows, Docker orchestration, Python, Rust, Java, or mobile toolchains;
- claim calibrated protection for M1, M3, M4, or other Apple Silicon profiles in v1;
- support repositories without Git;
- run as a resident daemon or menu-bar application;
- coordinate multiple autonomous agents beyond publishing and enforcing repository policy;
- optimize token usage, model context, or an agent's semantic memory;
- prevent a human or agent from bypassing the policy by invoking package-manager commands directly;
- attribute system-wide pressure to a particular unrelated application.

## 5. Users and primary scenario

The primary user is a developer using a coding agent on an Apple M2 MacBook Air with 8 GB of unified memory.

The primary scenario is:

1. The developer opens a Git-backed JavaScript/TypeScript repository.
2. `agent-lowmem doctor` reports whether heavy work is currently safe.
3. `agent-lowmem init` creates the local policy and configuration.
4. The coding agent reads `AGENTS.md` and invokes validation through Agent Lowmem.
5. `agent-lowmem run test` or `agent-lowmem run build` serializes the operation, chooses a safe budget, and monitors pressure.
6. The Mac stays interactive. The command either completes or exits safely with an actionable explanation.

## 6. Product principles

### Safety before throughput

The laptop remaining usable is more important than maximizing build speed. A safe, slower single-worker run is preferable to a faster run that causes sustained swap growth or a system freeze.

### Capability before restriction

Agent Lowmem should still attempt useful work. It reduces concurrency and scopes validation before refusing execution. A red host state or insufficient calibrated budget blocks a heavy command; unavailable mandatory measurements return a measurement error instead of pretending the host is red.

### Explicit and reversible behavior

Every file change is previewable and delimited. Every runtime limit is command-scoped. No implicit global tuning is allowed.

### Evidence over guessed flags

Adapters must identify installed tools and versions from lockfiles, manifests, and resolved executables. An adapter may apply only options supported by that detected tool version.

### Ownership boundaries

Agent Lowmem may signal only the process group it created. It may report unrelated high-memory processes but never terminate or reconfigure them.

### Deliberate v1 throughput limit

Version 1 uses one worker in both green and amber states. Green permits a larger safe budget and amber applies stronger warnings and termination sensitivity, but neither uses multiple workers. This deliberately sacrifices healthy-host throughput for a simpler, more predictable first release.

## 7. User interface

The executable is `agent-lowmem` and exposes four v1 commands.

### `agent-lowmem doctor`

Inspects the host and repository without writing files.

```text
agent-lowmem doctor
agent-lowmem doctor --json
```

It reports:

- supported host and architecture;
- physical memory;
- current kernel memory-pressure state and contributing measurements;
- rolling swapout and compressor rates when a recent ephemeral sample exists, otherwise an explicit `not observed` state;
- cumulative swap currently in use, labeled as historical evidence rather than proof of an active crisis;
- detected package manager and supported tools;
- proposed managed-process-tree budget, optional Node heap guardrail, and worker policy;
- whether another managed heavy operation owns the global lock;
- warnings and recommended next action.

### `agent-lowmem init`

Creates or updates repository policy.

```text
agent-lowmem init --dry-run
agent-lowmem init
```

The command:

1. requires a supported host, a Git repository, and a root `package.json`;
2. runs the same inspection as `doctor`;
3. previews changes when `--dry-run` is supplied;
4. writes `.agent-lowmem.json`;
5. inserts or replaces one managed block in the Git root's `AGENTS.md`;
6. writes a private restoration manifest at the path resolved by `git rev-parse --git-path agent-lowmem/restore-v1.json`, containing hashes and the prior managed content;
7. generates no timestamps or absolute paths and remains byte-for-byte idempotent for the same CLI version, configuration, and repository evidence.

### `agent-lowmem run`

Runs a supported repository script under the active policy.

```text
agent-lowmem run test
agent-lowmem run test -- path/to/file.test.ts
agent-lowmem run test --workspace @acme/web -- path/to/file.test.ts
agent-lowmem run typecheck
agent-lowmem run build
agent-lowmem run lint
agent-lowmem run build --json-file .agent-lowmem-result.json
```

Only scripts present in the selected `package.json` may be executed. `--workspace <name-or-path>` is required when a monorepo target cannot be selected unambiguously; pnpm targets are executed with an exact `--filter`. Arbitrary shell execution is outside v1. Additional arguments are passed as argument-array elements without shell interpolation.

The child inherits stdin, stdout, and stderr. `run` therefore does not support `--json` on stdout; it accepts `--json-file <path>` and atomically writes Agent Lowmem's structured result there. Test, typecheck, and lint operations default to a 15-minute timeout, while builds default to 30 minutes.

### `agent-lowmem restore`

Removes Agent Lowmem's managed repository changes.

```text
agent-lowmem restore --dry-run
agent-lowmem restore
```

It removes the managed `AGENTS.md` block and restores or removes `.agent-lowmem.json` according to the restoration manifest. After a fresh clone where that private manifest does not exist, it may remove a valid versioned managed block using its marker and content hash, but it never reconstructs unknown prior content. It refuses restoration when managed content has been edited manually and never rewrites unrelated `AGENTS.md` content.

Without a restoration manifest, `.agent-lowmem.json` is removed only when it byte-for-byte matches the deterministic configuration that the current CLI would generate from current repository evidence. Otherwise it is preserved and reported for manual review.

## 8. Architecture

Agent Lowmem is a native Rust CLI organized into focused modules. The executable does not require Node.js to start, does not embed a garbage collector, and does not use an asynchronous runtime.

The Rust workspace uses edition 2024 with Rust 1.85 as its minimum supported Rust version. Release and CI builds use the current stable Rust toolchain and a committed `Cargo.lock`.

First-party crates must declare `#![forbid(unsafe_code)]`. A reviewed dependency may encapsulate platform-specific unsafe code behind a safe interface, but every direct runtime dependency requires a documented purpose. The core runs on one thread; it monitors the managed child with a synchronous `try_wait` and sampling loop rather than adding Tokio, async-std, or a background service.

Release builds use link-time optimization, one code-generation unit, symbol stripping, and `panic = "unwind"`. A top-level unwind boundary converts unexpected panics into cleanup of the owned child process group and lock before returning an internal error. Optimization targets runtime performance while preserving a small binary; lifecycle safety and measurement take precedence over speculative size reductions.

### 8.1 Host inspector

Responsibilities:

- verify `darwin`, `arm64`, Apple M2, and 8 GB physical memory against the calibration compatibility key;
- read physical memory and the kernel memory-pressure state;
- read VM, compressor, swapin, and swapout counters through native sysctl and Mach interfaces rather than spawning monitoring utilities;
- sample counters with a monotonic timestamp and derive rates only from deltas between snapshots;
- normalize the public macOS pressure constants into `normal`, `warning`, and `critical` states;
- avoid logging serial numbers, hardware UUIDs, usernames, environment secrets, or command environments.

For the macOS 26 calibration profile, the exported pressure values must match the public dispatch flags `NORMAL = 0x01`, `WARN = 0x02`, and `CRITICAL = 0x04`. An unknown value or unavailable mandatory signal produces measurement error 69. It does not fabricate a green, amber, or red classification.

Platform calls are exposed to first-party code through a reviewed safe dependency boundary. First-party crates remain free of `unsafe` blocks.

The inspector stores only native counters, pressure state, and monotonic/wall timestamps in a per-user ephemeral cache. `doctor` uses a recent compatible sample to report rates without waiting; when none exists it reports rates as `not observed` and updates the cache. `run` always takes a second snapshot after a one-second preflight interval before launching the child, so derivative-based policy never uses a fabricated zero.

### 8.2 Repository inspector

Responsibilities:

- locate the Git root and its root-level `AGENTS.md`;
- parse `package.json` without executing it;
- identify npm or pnpm from `packageManager` and lockfiles;
- enumerate declared npm or pnpm workspaces and require an explicit target when selection is ambiguous;
- detect supported frameworks and test runners from declared and resolved dependencies;
- detect `pre*` and `post*` lifecycle scripts associated with each configured operation;
- return explicit unsupported or ambiguous states instead of guessing.

### 8.3 Pressure classifier

The classifier consumes a current snapshot plus a short rolling window and returns a class and reasons. It never uses raw `Pages free`, the percentage printed by `memory_pressure -Q`, or cumulative swap usage as the canonical health signal.

The kernel memory-pressure state is canonical:

- **Green:** the kernel reports `normal` and no calibrated early-warning condition is active.
- **Amber:** the kernel reports `warning`, or a calibrated swapout/compressor condition is active while the kernel remains normal.
- **Red:** the kernel reports `critical`, or warning pressure coincides with calibrated swap thrashing for two consecutive one-second samples.

A transition to kernel critical is red immediately. A non-critical red condition requires two consecutive samples. Recovery requires five consecutive samples at a less severe state so the classifier does not flap. Cumulative swap use is reported as historical context but never changes the state on its own.

Numeric derivative thresholds are stored in a bundled, versioned calibration artifact rather than embedded as unexplained constants. Before the runner is implemented, the repository must contain `calibration/macos-26-m2-8gb-v1.json` with:

- at least 50 labeled snapshots from the reference Mac across normal, warning, successful heavy operations, and interrupted heavy operations;
- monotonic sampling intervals and raw counter deltas;
- the selected swapout and compressor thresholds;
- false-positive and false-negative counts for the labeled sample set;
- the hardware and macOS compatibility key, without user or machine identifiers.

Controlled calibration must not intentionally exhaust the host. Critical examples may come from safe OS simulation or previously captured failures and are kept separate from normal production measurements.

The selected thresholds must produce zero false negatives across labeled critical/interrupted windows and no more than a 5% false-positive rate across labeled successful windows. A dataset that misses either bound cannot ship; it requires more evidence or a narrower compatibility key.

### 8.4 Managed budget and policy engine

The policy engine converts host, repository, calibration, and adapter evidence into a launch plan. The primary limit is a budget for the entire managed process tree. The initial budget is computed as:

```text
managed budget = min(adapter maximum, max(0, calibrated usable headroom - host reserve))
```

The calibration artifact defines the host reserve and the method used to calculate usable headroom from native VM counters. Every adapter declares a minimum viable budget. When the calculated budget is below that minimum, Agent Lowmem returns code 75 without launching a command that is expected to OOM.

Where useful, the engine derives a command-scoped `--max-old-space-size` guardrail from the managed budget, expected worker count, and an adapter-specific native-memory reserve. It never treats V8 old-space as a total memory cap. A repository may specify a higher or lower old-space value only with a non-empty justification, and the value may not exceed the total managed budget.

The runner enumerates the proven process group and conservatively sums each member's macOS physical-footprint measurement. Calibration and adapter budgets use the same accounting method so shared-memory overcounting is consistent. A sustained budget violation can terminate the group even when the kernel has not yet reached critical pressure.

A launch plan contains:

- package manager executable and argument array;
- selected package script;
- selected workspace and lifecycle phases;
- command-scoped environment additions;
- adapter-provided serial options;
- minimum and maximum viable budgets;
- managed process-tree budget and optional Node old-space guardrail;
- global-lock requirement;
- monitoring thresholds;
- timeout and pressure action;
- explanations suitable for terminal and structured output.

The engine never mutates the caller's shell environment.

### 8.5 Tool adapters

Each adapter has one purpose: inspect a supported tool version and return safe arguments, budget bounds, and worker behavior for a known operation.

V1 adapters cover:

- npm script forwarding;
- pnpm script forwarding;
- Vitest single-worker, non-watch execution;
- Jest serial execution;
- Next.js build execution with explicit version-tested internal-worker control;
- NestJS build execution under the calculated managed budget;
- generic Node-backed `typecheck` and `lint` scripts under the calculated budget.

Adapters must not inject a flag solely because it worked in another version. Version-specific mappings are represented as a tested compatibility matrix that distinguishes options such as Vitest file parallelism, pool behavior, and worker count. For a tool that can fan out internally, including Next.js and Vitest, an unknown version or missing verified worker strategy returns unsupported code 64 rather than pretending global serialization controls internal workers.

Package-manager lifecycle scripts associated with an operation run sequentially in the same managed process group, timeout, and total budget. `init` records their presence. Adapter flags apply only to the target script, so a lifecycle phase containing watch mode, background execution, or known parallel orchestration is rejected unless its exact behavior is covered by the compatibility matrix.

### 8.6 Global operation lock

A per-user lock prevents two Agent Lowmem heavy operations from running simultaneously across repositories.

The lock records:

- owner PID;
- process start identity;
- repository path hash;
- command category;
- owned child process-group identity, once launched;
- acquisition time.

The lock resides in the macOS per-user temporary directory, not the repository. A lock is stale only when both PID and process-start identity no longer match a live process. If a stale parent lock still references a live child group, `doctor` reports an orphan-recovery condition and `run` remains blocked; Agent Lowmem never kills that group without proving ownership and receiving an explicit future recovery command. Version 1 prints the proven PID/group identity and a manual recovery instruction but performs no automatic orphan kill. A normal live owner is reported and never terminated.

### 8.7 Managed runner and monitor

The runner starts the package-manager command in a dedicated process group without invoking an intermediate shell. The child inherits stdin, stdout, and stderr so terminal colors, interactivity, and backpressure remain owned by the terminal rather than by in-memory pipes.

The parent installs signal handlers before spawning. `SIGINT`, `SIGTERM`, and `SIGHUP` are forwarded to the managed process group, and the parent waits for group cleanup before releasing the lock. A top-level unwind boundary and RAII guards perform the same cleanup after a Rust panic. `SIGKILL` and host power loss cannot be caught; this limitation is explicit and is why the lock records both parent and child identities.

The monitor samples native pressure, counter rates, and managed process-tree footprint every second. It terminates according to this order:

1. kernel critical pressure triggers immediate `SIGTERM`;
2. calibrated warning-plus-thrashing for two consecutive samples triggers `SIGTERM`;
3. a managed process-tree budget violation for two consecutive samples triggers `SIGTERM`;
4. operation timeout triggers `SIGTERM` and timeout status;
5. after ten seconds, `SIGKILL` is sent only to surviving members of the proven owned process group;
6. the lock is released after the group exits or ownership can no longer be proven.

The default pressure action is `terminate`. A repository may select `warn` only with a non-empty justification; this changes in-run pressure conditions 1 and 2 to warnings but never permits launch from a red preflight and does not disable whole-tree budget or timeout termination. `doctor` and every run then state that the keep-responsive guarantee is weakened. Attribution is intentionally irrelevant: the default protects host usability whether pressure originated in the managed command or an unrelated application.

Agent Lowmem does not delete partial build artifacts. Its output states that the interrupted tool may have left generated files and recommends the tool's normal clean command or CI rather than guessing what can be removed.

### 8.8 Policy-file manager

The managed `AGENTS.md` section is bounded by a versioned marker containing a hash of the deterministic generated content:

```markdown
<!-- agent-lowmem:start version="1" content-sha256="<content-hash>" -->
## Agent Lowmem resource policy

Resource-heavy commands must run through Agent Lowmem. Run one heavy
operation at a time, never use watch mode, and prefer focused validation
before broad suites. Do not bypass a red preflight or raise a memory limit
after OOM without explicit user approval.
<!-- agent-lowmem:end -->
```

The marker's `version` identifies the managed-block format, and the digest covers the generated body between the markers but not the marker lines themselves. The actual marker contains the lowercase SHA-256 digest, not the literal placeholder shown in the format example. The generated block includes repository-specific supported commands and workspace selectors but no timestamps, usernames, or absolute paths. Idempotence is byte-for-byte for the same policy format and evidence; a CLI release that intentionally changes generated policy produces one reviewable hash/body diff. Content outside the markers is immutable from Agent Lowmem's perspective.

## 9. Configuration

`.agent-lowmem.json` is committed with the repository so humans and agents share the same policy.

```json
{
  "$schema": "https://agentlowmem.dev/schema/v1.json",
  "version": 1,
  "packageManager": "pnpm",
  "operations": {
    "test": {
      "script": "test",
      "timeoutSeconds": 900,
      "lifecycleScripts": []
    },
    "typecheck": {
      "script": "typecheck",
      "timeoutSeconds": 900,
      "lifecycleScripts": []
    },
    "lint": {
      "script": "lint",
      "timeoutSeconds": 900,
      "lifecycleScripts": []
    },
    "build": {
      "script": "build",
      "timeoutSeconds": 1800,
      "lifecycleScripts": [],
      "nodeOldSpaceMiB": "auto"
    }
  },
  "policy": {
    "maxHeavyOperations": 1,
    "watchMode": "deny",
    "focusedTestsFirst": true,
    "workers": 1,
    "onPressure": "terminate",
    "monitorIntervalMs": 1000
  }
}
```

`init` includes only scripts that exist and records detected lifecycle phases. The user may remove an operation to make it unavailable through `run`.

A monorepo adds a `workspaces` map. Keys are stable values accepted by `--workspace`; each entry contains a repository-relative `path`, the exact `packageManagerSelector` passed to npm `--workspace` or pnpm `--filter`, and its own `operations` map. For example:

```json
{
  "workspaces": {
    "web": {
      "path": "apps/web",
      "packageManagerSelector": "@acme/web",
      "operations": {
        "test": {
          "script": "test",
          "timeoutSeconds": 900,
          "lifecycleScripts": []
        }
      }
    }
  }
}
```

A command without `--workspace` uses only root operations. Commands never infer a target when multiple workspaces match, and generated `AGENTS.md` examples use the stable key rather than an absolute path.

Version 1 permits two justified overrides:

- an operation may replace `"nodeOldSpaceMiB": "auto"` with a positive MiB value plus `"overrideReason"`;
- policy may set `"onPressure": "warn"` plus `"overrideReason"`.

The schema accepts operation timeouts from 60 through 3600 seconds. It rejects an old-space value below an adapter's declared viable floor or above the calculated whole-tree budget, an empty override reason, multiple heavy operations, more than one worker, enabled watch mode, monitor intervals other than 1000 ms, and arbitrary commands. Overrides are reported by `doctor`, embedded in structured run results, and never silently normalized.

## 10. Data flow

```text
CLI request
  -> host inspection
  -> repository inspection
  -> pressure classification
  -> calibration compatibility check
  -> policy decision
  -> adapter selection
  -> one-second derivative preflight
  -> global lock acquisition
  -> managed child launch
  -> one-second native pressure and process-tree monitoring
  -> child completion or owned-process termination
  -> lock release
  -> human result and optional atomic JSON result file
```

`doctor`, `init --dry-run`, and `restore --dry-run` stop before lock acquisition because they do not run heavy work.

## 11. Errors and exit codes

Errors must state what happened, what Agent Lowmem changed, and the safest next action.

| Code | Meaning |
| ---: | --- |
| 0 | Command completed successfully |
| 2 | Invalid CLI usage or configuration |
| 64 | Unsupported host, calibration, repository, package manager, workspace, tool version, or script |
| 69 | Required macOS measurement unavailable |
| 73 | Live heavy-operation lock already exists |
| 75 | Command blocked for insufficient safe budget or terminated for pressure/budget protection |
| 78 | Managed-file conflict prevents safe init or restore |
| 124 | Managed operation exceeded its configured timeout |
| child | Managed command exit code, preserved exactly even when it equals a reserved Agent Lowmem value |

Agent Lowmem internal codes apply when no child result exists, except protection termination and timeout. If a child naturally exits with the same number as an internal code, the shell value is inherently ambiguous; terminal text and the structured result's `exitOrigin` field distinguish `child`, `supervisor`, and `timeout`. A signal-terminated child follows the platform's conventional `128 + signal` representation.

No automatic retry occurs after OOM, red pressure, timeout, or a failed build. The tool recommends focused validation or CI rather than silently increasing resources.

## 12. Security and privacy

Version 1 is local-only and sends no telemetry.

It must:

- compile all first-party crates with `#![forbid(unsafe_code)]`;
- avoid printing or persisting environment-variable values;
- avoid shell-string construction for child commands;
- validate configuration against a bundled schema;
- refuse symlink traversal when writing managed files;
- use atomic writes for configuration, manifests, and `AGENTS.md` updates;
- store no raw process command lines in the lock;
- hash repository paths recorded outside the repository;
- make network access unnecessary for `doctor`, `init`, `run`, and `restore` after package installation;
- commit `Cargo.lock` and deny unknown licenses, duplicate critical dependencies, yanked crates, and known advisories in CI;
- publish SHA-256 checksums and GitHub build provenance for release binaries;
- strip before code signing, verify the final signature, and notarize the archive distributed outside Homebrew;
- keep npm packages free of lifecycle scripts and network downloaders.

## 13. Observability

Human output is concise and uses the green, amber, and red vocabulary with text labels so it does not depend on color perception.

`doctor`, `init`, and `restore` accept `--json` on stdout because they do not stream a managed child. `run` instead accepts `--json-file <path>` and atomically writes a versioned result after the child group has exited. Child stdin, stdout, and stderr remain inherited and unmodified.

Structured output includes:

- schema version;
- timestamp;
- host support status;
- normalized measurements;
- pressure class and reasons;
- detected tools;
- selected policy;
- operation result, exit origin, and exit reason.

Agent Lowmem-generated human and structured fields never include usernames, absolute home-directory paths, environment values, or unrelated process command lines. This redaction guarantee does not apply to inherited child output, which may contain project paths, stack traces, or tool-defined diagnostics.

## 14. Testing strategy

The implementation plan must preserve a low-resource Rust test suite. Repository validation runs sequentially with `cargo test -- --test-threads=1`; formatting, linting, tests, security checks, and release builds run as separate operations rather than concurrently.

### Unit tests

- native macOS snapshot normalization and public pressure-constant mapping;
- counter-delta calculations using monotonic timestamps;
- ephemeral-cache freshness, compatibility, and first-sample `not observed` behavior;
- green, amber, red, hysteresis, immediate-critical, and unavailable-measurement boundaries;
- calibration artifact schema and macOS/hardware compatibility checks;
- managed-budget and Node guardrail calculations;
- package-manager and tool detection;
- adapter version mappings;
- launch-plan construction;
- managed-block insertion, replacement, conflicts, and restoration;
- stale and live lock decisions;
- signal, pressure, budget, timeout, and panic cleanup state machines;
- redaction and JSON schema behavior;
- enforcement of the first-party `unsafe` prohibition.

### Integration tests

- fixture repositories for npm, pnpm, single packages, and monorepos;
- unambiguous and ambiguous workspace selection plus exact pnpm filtering;
- lifecycle phase detection and rejection of unsafe background/watch patterns;
- version-matrix fixtures proving the exact serial strategy for Vitest, Jest, Next.js, and NestJS;
- unsupported tool versions refusing execution instead of guessing flags;
- Next.js and NestJS fixture builds under a synthetic whole-tree budget;
- lock contention between two processes;
- inherited TTY stdio without captured-pipe deadlock;
- `SIGINT`, `SIGTERM`, and `SIGHUP` forwarding to the managed group;
- managed child cleanup after simulated pressure, budget violation, timeout, and Rust panic;
- termination without signaling an unrelated sentinel process;
- exact child exit-code preservation and `exitOrigin` disambiguation;
- atomic `--json-file` output separated from child streams;
- one-second derivative preflight using an injected monotonic clock;
- interrupted atomic writes, versioned marker upgrades, fresh-clone restore, and restoration conflicts;
- portable npm root-package installation with platform-specific optional dependencies.

### End-to-end tests

One macOS Apple M2 8 GB workflow validates `doctor`, `init`, a focused test run, a small build, Ctrl-C cleanup, structured result output, and `restore`. Heavy pressure is simulated through injected snapshots; the automated test suite must not intentionally exhaust host memory.

Packaging workflows install the root npm package on macOS arm64, macOS x64, Linux, and Windows. Installation must succeed everywhere; execution succeeds only on supported macOS arm64 and returns a clear unsupported-platform error elsewhere.

The project's own tests run sequentially by default so Agent Lowmem follows the policy it publishes.

### Resource budgets

On the reference M2 8 GB Mac, the release build must satisfy all of these budgets:

- `doctor` parent-process peak resident memory at or below 24 MiB;
- median `doctor` wall time at or below 250 ms over 20 consecutive runs;
- stripped `aarch64-apple-darwin` executable at or below 12 MiB;
- npm-launcher plus native-process aggregate peak resident memory at or below 80 MiB;
- no background process or owned child remaining after command completion.

The `doctor` wall-time and memory measurements include all native host inspection and ephemeral-cache access because v1 no longer spawns monitoring utilities. A default `doctor` call does not wait to create a second sample. Results are recorded with release artifacts. A budget regression blocks release unless this design is revised explicitly.

## 15. Acceptance criteria

Version 1 is ready when all of the following are true:

1. On the reference Apple M2 8 GB Mac, `doctor` reads native pressure and produces a deterministic, explained classification.
2. The versioned calibration artifact meets its sample, labeling, compatibility, and error-count requirements before `run` ships.
3. Missing or unknown mandatory measurements return code 69 and never masquerade as red.
4. Kernel critical pressure triggers immediate protection; calibrated warning-plus-thrashing and managed-budget violations trigger within two one-second samples.
5. On uncalibrated Apple Silicon profiles, unsupported hosts, or repositories without Git, inspection fails clearly without writing files.
6. `init --dry-run` shows the exact proposed files and `init` produces the same deterministic content.
7. Repeated same-version `init` calls produce no duplicate block or unrelated diff; a policy-changing upgrade produces one versioned managed-block diff.
8. Monorepo execution requires an unambiguous workspace and uses the exact configured npm workspace or pnpm filter.
9. A supported Vitest or Jest script runs with one worker and no watch mode; unsupported versions return code 64.
10. A supported Next.js build uses a version-tested internal-worker strategy; an unknown strategy returns code 64.
11. Node old-space is an adapter-derived guardrail inside the whole-tree budget, and justified overrides cannot exceed that budget.
12. Two simultaneous managed heavy commands cannot start.
13. Critical preflight, insufficient viable budget, and incompatible calibration do not start a child process.
14. Pressure, budget, timeout, forwarded signals, and a simulated Rust panic clean up only the proven owned process group and lock.
15. A normal child failure preserves inherited output and its exact exit code; structured output records its origin.
16. `run --json-file` never mixes Agent Lowmem JSON into child stdout or stderr.
17. `restore --dry-run` previews reversal; restore preserves unrelated `AGENTS.md` content and safely handles a valid managed marker after a fresh clone.
18. Agent Lowmem performs no network request and leaves no background process after each command.
19. Unit, integration, and end-to-end tests run sequentially and pass within the project's own low-memory policy.
20. All first-party crates reject `unsafe` code at compile time and dependency-policy checks pass.
21. The native release binary and npm invocation satisfy their separate documented memory, startup, size, and cleanup budgets on the reference Mac.
22. The final release sequence is build, strip, sign, verify, notarize, checksum, and provenance publication.
23. Homebrew, GitHub Release, and the macOS npm platform package contain the same signed native binary.
24. Installing the npm root package does not fail a Linux, Windows, or Intel teammate's dependency installation.

## 16. Distribution and compatibility

Version 1 is an MIT-licensed Rust binary for `aarch64-apple-darwin`. Homebrew, GitHub Release, and Cargo installations execute it without Node.js. The project-local npm distribution uses Node.js only for its small portable launcher; Node.js, npm, and pnpm are otherwise inspected as properties of the repository being optimized.

The primary installation path is Homebrew:

```text
brew install pleo2/tap/agent-lowmem
agent-lowmem init
```

GitHub Releases publishes a notarized archive. Release order is fixed: compile, test, strip, Developer ID sign, signature verification, notarization, SHA-256 checksum, and build provenance. Nothing mutates the binary after signing.

For project-local installation, the portable `agent-lowmem` root package has no `os` or `cpu` restriction and declares platform packages such as `@agent-lowmem/darwin-arm64` as optional dependencies. The platform package contains the same signed native binary as GitHub Releases and declares its own operating-system and CPU restriction.

The root package includes a minimal JavaScript launcher that resolves the installed optional platform package, starts its binary with inherited stdio, and returns the exact child status. It never downloads an executable, runs a lifecycle hook, or falls back to building native code during installation. Unsupported platforms install successfully and receive a clear error only if they try to execute the unsupported v1 CLI. The npm route therefore has temporary launcher overhead; packaging tests measure and publish native-only and combined peak memory separately.

```text
pnpm add -D agent-lowmem
pnpm exec agent-lowmem init
```

Equivalent npm installation is supported. Building from source with Cargo is documented for contributors but is not the default installation path.

## 17. Documentation deliverables

The first release includes:

- a concise README centered on the 8 GB Mac problem;
- installation and five-minute quick start;
- explanation of green, amber, and red states;
- generated `AGENTS.md` policy example;
- supported-tool matrix with tested versions;
- calibration methodology, compatibility key, thresholds, and labeled aggregate results;
- troubleshooting for OOM, red pressure, lock contention, and unsupported flags;
- explicit explanation that direct package-manager commands bypass enforcement;
- npm platform-support and launcher-overhead documentation;
- security and privacy statement;
- contribution guide requiring low-memory-safe tests;
- website copy for `agentlowmem.dev` using the approved name and tagline.

## 18. Deferred roadmap

The following require separate specifications after v1 evidence exists:

- Linux and Intel Mac support;
- calibrated M1, M3, M4, and additional memory-size profiles;
- Python, Rust, Java, Flutter, and container adapters;
- multiple named policy profiles beyond the two justified v1 overrides;
- CI recommendation generation;
- interactive dashboard or menu-bar status;
- historical measurements and adaptive per-project budgets;
- compatibility blocks for agent formats other than `AGENTS.md`;
- an installable Codex skill layered on top of the stable CLI contract.

The CLI remains the enforcement layer. Future skills teach agents how to use it but do not duplicate resource-control logic.

## 19. Brand decision

The canonical brand is **Agent Lowmem**. The product name emphasizes a precise developer problem and the v1 messaging emphasizes capability rather than reduced intelligence.

Canonical identifiers:

- display name: `Agent Lowmem`;
- domain: `agentlowmem.dev`, acquired on 2026-09-02;
- repository: `agent-lowmem`;
- npm package: `agent-lowmem`;
- executable: `agent-lowmem`;
- tagline: `More agents. Less RAM.`

The spelling `lowmem` is mandatory. The variants `lowmen`, `agent-lowmen`, and `agentlowmen` are incorrect and must not appear in product assets, package metadata, or domains.

## 20. Authoritative references

- [Apple XNU memorystatus notifications](https://github.com/apple-oss-distributions/xnu/blob/main/doc/vm/memorystatus_notify.md) for kernel pressure states, available-memory accounting, and hysteresis.
- [Apple dispatch memory-pressure source](https://developer.apple.com/documentation/dispatch/dispatch_source_type_memorypressure) for the public normal, warning, and critical event model.
- [Rust Reference: destructors](https://doc.rust-lang.org/reference/destructors.html) for the cleanup consequences of aborting without unwind.
- [Rust `Command`](https://doc.rust-lang.org/std/process/struct.Command.html) for inherited stdio behavior.
- [npm `package.json`](https://docs.npmjs.com/files/package.json/) for `os`, `cpu`, and optional dependency semantics.
- [Apple notarization workflow](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) for direct-download release handling.
