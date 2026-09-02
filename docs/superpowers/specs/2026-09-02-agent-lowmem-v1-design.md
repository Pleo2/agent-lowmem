# Agent Lowmem v1 Design

**Status:** Proposed for implementation

**Date:** 2026-09-02

**Product:** Agent Lowmem

**Domain:** `agentlowmem.dev` (acquired 2026-09-02)

**Repository and npm package:** `agent-lowmem`

**Tagline:** More agents. Less RAM.

## 1. Summary

Agent Lowmem is an open-source, policy-first native command-line tool for running coding-agent workloads safely on memory-constrained Macs. Its core is written in Rust, while version 1 targets Apple Silicon Macs with 8 GB of unified memory and JavaScript/TypeScript repositories that use Node.js, npm or pnpm, Next.js, NestJS, Vitest, or Jest.

The tool measures current memory pressure, generates repository guidance for agents through a managed `AGENTS.md` block, and launches heavy commands with conservative process, heap, and worker limits. It never requires `sudo`, makes no permanent macOS tuning changes, and only controls processes it starts.

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

1. Detect whether the host is a supported Apple Silicon Mac and measure its current memory state.
2. Classify that state as green, amber, or red using documented, deterministic rules.
3. Generate an idempotent, clearly delimited Agent Lowmem policy block in `AGENTS.md`.
4. Detect npm or pnpm and supported JavaScript/TypeScript tools from repository evidence.
5. Run one heavy Agent Lowmem-managed operation at a time across the local user session.
6. Apply command-scoped Node heap limits and supported single-worker or serial options.
7. Reject watch mode and unmanaged parallelism for heavy commands.
8. Monitor memory while a managed child command runs.
9. Stop only the managed child process tree after sustained red pressure.
10. Explain every decision in human-readable output and offer equivalent JSON output for agents.
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
- run as a resident daemon or menu-bar application;
- coordinate multiple autonomous agents beyond publishing and enforcing repository policy;
- optimize token usage, model context, or an agent's semantic memory.

## 5. Users and primary scenario

The primary user is a developer using a coding agent on an Apple Silicon Mac with 8 GB of unified memory.

The primary scenario is:

1. The developer opens a JavaScript/TypeScript repository.
2. `agent-lowmem doctor` reports whether heavy work is currently safe.
3. `agent-lowmem init` creates the local policy and configuration.
4. The coding agent reads `AGENTS.md` and invokes validation through Agent Lowmem.
5. `agent-lowmem run test` or `agent-lowmem run build` serializes the operation, chooses a safe budget, and monitors pressure.
6. The Mac stays interactive. The command either completes or exits safely with an actionable explanation.

## 6. Product principles

### Safety before throughput

The laptop remaining usable is more important than maximizing build speed. A safe, slower single-worker run is preferable to a faster run that causes sustained swap growth or a system freeze.

### Capability before restriction

Agent Lowmem should still attempt useful work. It reduces concurrency and scopes validation before refusing execution. A red host state is the only default preflight condition that blocks a heavy command.

### Explicit and reversible behavior

Every file change is previewable and delimited. Every runtime limit is command-scoped. No implicit global tuning is allowed.

### Evidence over guessed flags

Adapters must identify installed tools and versions from lockfiles, manifests, and resolved executables. An adapter may apply only options supported by that detected tool version.

### Ownership boundaries

Agent Lowmem may signal only the process group it created. It may report unrelated high-memory processes but never terminate or reconfigure them.

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
- current memory-pressure class and contributing measurements;
- swap currently in use, labeled as historical/current pressure evidence rather than proof of an active crisis;
- detected package manager and supported tools;
- proposed Node heap and worker policy;
- whether another managed heavy operation owns the global lock;
- warnings and recommended next action.

### `agent-lowmem init`

Creates or updates repository policy.

```text
agent-lowmem init --dry-run
agent-lowmem init
```

The command:

1. requires a supported host and a repository containing `package.json`;
2. runs the same inspection as `doctor`;
3. previews changes when `--dry-run` is supplied;
4. writes `.agent-lowmem.json`;
5. inserts or replaces one managed block in the nearest applicable `AGENTS.md`;
6. writes a private restoration manifest at the path resolved by `git rev-parse --git-path agent-lowmem/restore-v1.json`, containing hashes and the prior managed content;
7. remains idempotent when executed repeatedly.

### `agent-lowmem run`

Runs a supported repository script under the active policy.

```text
agent-lowmem run test
agent-lowmem run test -- path/to/file.test.ts
agent-lowmem run typecheck
agent-lowmem run build
agent-lowmem run lint
```

Only scripts present in `package.json` may be selected. Arbitrary shell execution is outside v1. Additional arguments are passed as argument-array elements without shell interpolation.

### `agent-lowmem restore`

Removes Agent Lowmem's managed repository changes.

```text
agent-lowmem restore --dry-run
agent-lowmem restore
```

It removes the managed `AGENTS.md` block and restores or removes `.agent-lowmem.json` according to the restoration manifest. It refuses restoration when the relevant content changed outside its managed boundaries and explains how to resolve the conflict manually. It never rewrites unrelated `AGENTS.md` content.

## 8. Architecture

Agent Lowmem is a native Rust CLI organized into focused modules. The executable does not require Node.js to start, does not embed a garbage collector, and does not use an asynchronous runtime.

The Rust workspace uses edition 2024 with Rust 1.85 as its minimum supported Rust version. Release and CI builds use the current stable Rust toolchain and a committed `Cargo.lock`.

First-party crates must declare `#![forbid(unsafe_code)]`. A reviewed dependency may encapsulate platform-specific unsafe code behind a safe interface, but every direct runtime dependency requires a documented purpose. The core runs on one thread; it monitors the managed child with a synchronous `try_wait` and sampling loop rather than adding Tokio, async-std, or a background service.

Release builds use link-time optimization, one code-generation unit, symbol stripping, and `panic = "abort"`. Optimization targets runtime performance while preserving a small binary; correctness and measurement take precedence over speculative micro-optimization.

### 8.1 Host inspector

Responsibilities:

- verify `darwin` and `arm64`;
- read physical memory;
- collect `memory_pressure`, `vm_stat`, and `sysctl vm.swapusage` observations;
- normalize observations into a stable internal snapshot;
- avoid logging serial numbers, hardware UUIDs, usernames, environment secrets, or command environments.

It depends only on a process-execution abstraction and parsers with fixture-based tests.

### 8.2 Repository inspector

Responsibilities:

- locate the repository root and its root-level `AGENTS.md`;
- parse `package.json` without executing it;
- identify npm or pnpm from `packageManager` and lockfiles;
- detect supported frameworks and test runners from declared and resolved dependencies;
- return explicit unsupported or ambiguous states instead of guessing.

### 8.3 Pressure classifier

The classifier consumes a memory snapshot and returns a class, reasons, and a safe process budget.

Initial v1 policy for an 8 GB host:

- **Green:** reported free-memory percentage is at least 30%, and macOS does not report critical pressure.
- **Amber:** reported free-memory percentage is 15% through 29%, or swap use is at least 25% of physical memory.
- **Red:** reported free-memory percentage is below 15%, macOS reports critical pressure, or the required measurements cannot be obtained reliably.

The most severe matching condition wins. Swap usage alone cannot produce red because macOS may retain swap after a prior peak.

The default Node heap is command-scoped and derived from the class:

| State | Node old-space ceiling | Heavy command behavior |
| --- | ---: | --- |
| Green | 768 MiB | Start with one worker |
| Amber | 512 MiB | Start with one worker and an explicit warning |
| Red | none | Do not start |

The heap ceiling is not presented as a total-process memory cap. Native allocations and child processes remain reasons to enforce serialization and monitoring.

### 8.4 Policy engine

The policy engine converts host and repository evidence into a launch plan. A launch plan contains:

- package manager executable and argument array;
- selected package script;
- command-scoped environment additions;
- adapter-provided serial options;
- global-lock requirement;
- monitoring thresholds;
- explanations suitable for terminal and JSON output.

The engine never mutates the caller's shell environment.

### 8.5 Tool adapters

Each adapter has one purpose: inspect a supported tool version and return safe arguments for a known operation.

V1 adapters cover:

- npm script forwarding;
- pnpm script forwarding;
- Vitest single-worker, non-watch execution;
- Jest serial execution;
- Next.js build execution under the calculated Node budget;
- NestJS build execution under the calculated Node budget;
- generic Node-backed `typecheck` and `lint` scripts under the calculated budget.

Adapters must not inject a flag solely because it worked in another version. Version-specific mappings are represented as tested data. If no safe mapping exists, Agent Lowmem runs only the package script under serialization and the Node budget, while reporting that worker-level enforcement was unavailable.

### 8.6 Global operation lock

A per-user lock prevents two Agent Lowmem heavy operations from running simultaneously across repositories.

The lock records:

- owner PID;
- process start identity;
- repository path hash;
- command category;
- acquisition time.

The lock resides in the macOS per-user temporary directory, not the repository. A lock is stale only when both PID and process-start identity no longer match a live process. Agent Lowmem reports a live owner and exits; it does not terminate the owner.

### 8.7 Managed runner and monitor

The runner starts the package-manager command in a dedicated process group without invoking an intermediate shell. It samples memory every five seconds.

If pressure becomes red:

1. one red sample produces a warning;
2. three consecutive red samples produce sustained-red status;
3. the runner sends `SIGTERM` to its managed process group;
4. after ten seconds, it sends `SIGKILL` only to surviving members of that same group;
5. it releases the lock and exits with code 75.

Agent Lowmem does not delete partial build artifacts. Its output states that the interrupted tool may have left generated files and recommends the tool's normal clean command or CI rather than guessing what can be removed.

### 8.8 Policy-file manager

The managed `AGENTS.md` section is bounded by stable markers:

```markdown
<!-- agent-lowmem:start -->
## Agent Lowmem resource policy

Resource-heavy commands must run through Agent Lowmem. Run one heavy
operation at a time, never use watch mode, and prefer focused validation
before broad suites. Do not bypass a red preflight or raise a memory limit
after OOM without explicit user approval.
<!-- agent-lowmem:end -->
```

The generated block also includes repository-specific supported commands. Content outside the markers is immutable from Agent Lowmem's perspective.

## 9. Configuration

`.agent-lowmem.json` is committed with the repository so humans and agents share the same policy.

```json
{
  "$schema": "https://agentlowmem.dev/schema/v1.json",
  "version": 1,
  "packageManager": "pnpm",
  "scripts": {
    "test": "test",
    "typecheck": "typecheck",
    "lint": "lint",
    "build": "build"
  },
  "policy": {
    "maxHeavyOperations": 1,
    "watchMode": "deny",
    "focusedTestsFirst": true,
    "sustainedRedSamples": 3
  }
}
```

`init` includes only scripts that exist. The user may remove a script mapping to make it unavailable through `run`. Version 1 does not permit configuration to raise heap ceilings, enable parallel heavy operations, disable red-pressure termination, or execute arbitrary commands. Those safety boundaries require a future design review.

## 10. Data flow

```text
CLI request
  -> host inspection
  -> repository inspection
  -> pressure classification
  -> policy decision
  -> adapter selection
  -> global lock acquisition
  -> managed child launch
  -> five-second pressure monitoring
  -> child completion or owned-process termination
  -> lock release
  -> human and optional JSON result
```

`doctor`, `init --dry-run`, and `restore --dry-run` stop before lock acquisition because they do not run heavy work.

## 11. Errors and exit codes

Errors must state what happened, what Agent Lowmem changed, and the safest next action.

| Code | Meaning |
| ---: | --- |
| 0 | Command completed successfully |
| 2 | Invalid CLI usage or configuration |
| 64 | Unsupported host, repository, package manager, or script |
| 69 | Required macOS measurement unavailable |
| 73 | Live heavy-operation lock already exists |
| 75 | Command blocked before launch or terminated for sustained red pressure |
| 78 | Managed-file conflict prevents safe init or restore |
| child | Managed command failed normally; preserve its exit code when it does not conflict with reserved codes |

No automatic retry occurs after OOM, red pressure, or a failed build. The tool recommends focused validation or CI rather than silently increasing resources.

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
- keep the npm package free of lifecycle scripts and network downloaders.

## 13. Observability

Human output is concise and uses the green, amber, and red vocabulary with text labels so it does not depend on color perception.

`--json` is a global option accepted by all four commands. Its output is versioned and includes:

- schema version;
- timestamp;
- host support status;
- normalized measurements;
- pressure class and reasons;
- detected tools;
- selected policy;
- operation result and exit reason.

JSON output never includes usernames, absolute home-directory paths, environment values, or unrelated process command lines.

## 14. Testing strategy

The implementation plan must preserve a low-resource Rust test suite. Repository validation runs sequentially with `cargo test -- --test-threads=1`; formatting, linting, tests, security checks, and release builds run as separate operations rather than concurrently.

### Unit tests

- parsers for captured `memory_pressure`, `vm_stat`, and swap outputs;
- green, amber, red, and unavailable classifier boundaries;
- package-manager and tool detection;
- adapter version mappings;
- launch-plan construction;
- managed-block insertion, replacement, conflicts, and restoration;
- stale and live lock decisions;
- red-pressure sample state machine;
- redaction and JSON schema behavior.
- enforcement of the first-party `unsafe` prohibition.

### Integration tests

- fixture repositories for npm and pnpm;
- Vitest and Jest commands proving serial argument injection;
- Next.js and NestJS fixture builds under a synthetic budget;
- lock contention between two processes;
- managed child termination without signaling an unrelated sentinel process;
- interrupted atomic writes and restoration conflicts.
- verification that the npm package invokes the native Mach-O executable without a Node.js launcher.

### End-to-end tests

One macOS Apple Silicon workflow validates `doctor`, `init`, a focused test run, a small build, and `restore`. Heavy pressure is simulated through injected snapshots; the automated test suite must not intentionally exhaust host memory.

The project's own tests run sequentially by default so Agent Lowmem follows the policy it publishes.

### Resource budgets

On the reference M2 8 GB Mac, the release build must satisfy all of these budgets:

- `doctor` parent-process peak resident memory at or below 24 MiB;
- median `doctor` wall time at or below 250 ms over 20 consecutive runs;
- stripped `aarch64-apple-darwin` executable at or below 12 MiB;
- no background process or thread remaining after command completion.

Measurements exclude the short-lived macOS utilities invoked to collect host evidence and are recorded with the release artifacts. A budget regression blocks release unless this design is revised explicitly.

## 15. Acceptance criteria

Version 1 is ready when all of the following are true:

1. On an Apple Silicon 8 GB Mac, `doctor` produces a deterministic classification and explains it.
2. On unsupported hosts, read-only inspection fails clearly without writing files.
3. `init --dry-run` shows the exact proposed files and `init` produces the same content.
4. Repeated `init` calls produce no duplicate block or unrelated diff.
5. A detected Vitest or Jest test script runs with one worker and no watch mode.
6. Supported builds and type checks receive only command-scoped memory settings.
7. Two simultaneous managed heavy commands cannot start.
8. A red preflight does not start a child process.
9. Three simulated consecutive red samples terminate only the managed process group and return code 75.
10. A normal child failure preserves useful output and its non-reserved exit code.
11. `restore --dry-run` previews the reversal and `restore` preserves unrelated `AGENTS.md` content.
12. The CLI performs no network request and leaves no background process after each command.
13. Unit, integration, and end-to-end tests run sequentially and pass within the project's own declared low-memory policy.
14. All first-party crates reject `unsafe` code at compile time and dependency-policy checks pass.
15. The release binary satisfies the documented memory, startup, size, and cleanup budgets on the reference Mac.
16. Homebrew, GitHub Release, and npm installations execute the same versioned native binary.

## 16. Distribution and compatibility

Version 1 is an MIT-licensed Rust binary for `aarch64-apple-darwin`. The CLI itself has no Node.js runtime requirement. Node.js, npm, and pnpm are inspected only as properties of the repository being optimized.

The primary installation path is Homebrew:

```text
brew install pleo2/tap/agent-lowmem
agent-lowmem init
```

GitHub Releases publishes the same stripped binary, SHA-256 checksum, and build provenance.

For project-local installation, the npm package contains the prebuilt `aarch64-apple-darwin` executable, declares its supported operating system and CPU in package metadata, and exposes it directly through `bin`. It contains no JavaScript launcher, `postinstall` hook, or network downloader.

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
- troubleshooting for OOM, red pressure, lock contention, and unsupported flags;
- security and privacy statement;
- contribution guide requiring low-memory-safe tests;
- website copy for `agentlowmem.dev` using the approved name and tagline.

## 18. Deferred roadmap

The following require separate specifications after v1 evidence exists:

- Linux and Intel Mac support;
- Python, Rust, Java, Flutter, and container adapters;
- multiple policy profiles or user-overridden ceilings;
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
