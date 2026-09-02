# Agent Lowmem v1 Design

**Status:** Proposed for implementation

**Date:** 2026-09-02

**Revision:** 3 — kernel-first scope approved

**Product:** Agent Lowmem

**Domain:** `agentlowmem.dev` (acquired 2026-09-02)

**Repository and npm package:** `agent-lowmem`

**Tagline:** More agents. Less RAM.

## 1. Summary

Agent Lowmem is an open-source, policy-first native command-line tool for running coding-agent workloads conservatively on memory-constrained Macs. Its core is written in Rust, while version 1 validates one protective profile: the Apple M2 MacBook Air with 8 GB of unified memory on macOS 26.x, running JavaScript/TypeScript repositories that use Node.js, npm or pnpm, Next.js, NestJS, Vitest, or Jest.

The tool uses the native macOS kernel pressure state as its only v1 protection boundary, generates repository guidance through a managed `AGENTS.md` block, and launches one heavy operation at a time with supported serial options and a conservative Node old-space guardrail. It observes the managed process group but does not pretend that macOS provides a hard per-tree memory cap. Agent Lowmem never requires `sudo`, makes no permanent macOS tuning changes, and only controls processes it starts.

The v1 promise is deliberately narrow:

> Reduce memory-pressure risk on an 8 GB Mac by serializing supported heavy work and stopping it when macOS reports unsafe pressure.

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

1. Detect whether the host matches the validated Apple M2 8 GB and macOS 26.x profile, while allowing read-only observation on other macOS arm64 profiles.
2. Classify memory pressure as green, amber, or red by mapping the current kernel state directly, without learned thresholds.
3. Generate an idempotent, clearly delimited Agent Lowmem policy block in `AGENTS.md`.
4. Detect npm or pnpm and supported JavaScript/TypeScript tools from repository evidence.
5. Run one heavy Agent Lowmem-managed operation at a time across the local user session.
6. Apply a fixed command-scoped Node heap guardrail when useful and supported single-worker or serial options.
7. Reject watch mode, serialize Agent Lowmem invocations, and disclose internal fan-out that a supported tool cannot control through a verified public interface.
8. Monitor memory while a managed child command runs.
9. Stop only the managed child process group on the first observed critical sample or after two consecutive warning samples.
10. Explain every decision in human-readable output and provide structured results without mixing them into child stdout or stderr.
11. Leave no daemon, service, global environment variable, or permanent system change behind.
12. Restore repository files changed by `init` through an explicit preview-first command.

## 4. Non-goals

Version 1 will not:

- optimize macOS kernel, swap, compressed memory, launch agents, or system settings;
- kill browsers, editors, MCP servers, containers, or unrelated processes;
- modify global shell profiles, npm configuration, Node installations, or package-manager stores;
- promise that every full build can complete locally under the fixed protective policy;
- replace CI for broad validation that exceeds the laptop's safe capacity;
- support Intel Macs, Linux, Windows, Docker orchestration, Python, Rust, Java, or mobile toolchains;
- claim validated protection for M1, M3, M4, other memory sizes, Intel Macs, or macOS major versions other than 26 in v1;
- support `init` or `run` for repositories without Git;
- run as a resident daemon or menu-bar application;
- coordinate multiple autonomous agents beyond publishing and enforcing repository policy;
- optimize token usage, model context, or an agent's semantic memory;
- prevent a human or agent from bypassing the policy by invoking package-manager commands directly;
- attribute system-wide pressure to a particular unrelated application;
- enforce a hard memory cap for a process tree, perfectly observe short-lived process peaks, or deduplicate shared physical pages across processes;
- predict pressure from compressor or swap derivatives in v1;
- guarantee that the Mac never swaps, freezes, or reaches OOM before a best-effort kernel signal can be observed.

## 5. Users and primary scenario

The primary user is a developer using a coding agent on an Apple M2 MacBook Air with 8 GB of unified memory.

The primary scenario is:

1. The developer opens a Git-backed JavaScript/TypeScript repository.
2. `agent-lowmem doctor` reports whether heavy work is currently safe.
3. `agent-lowmem init` creates the local policy and configuration.
4. The coding agent reads `AGENTS.md` and invokes validation through Agent Lowmem.
5. `agent-lowmem run test` or `agent-lowmem run build` serializes the operation, applies supported guardrails, and monitors pressure.
6. The policy prioritizes interactivity. The command either completes or is stopped with an actionable explanation when observed pressure becomes unsafe.

## 6. Product principles

### Safety before throughput

The laptop remaining usable is more important than maximizing build speed. A safe, slower run with verified serialization where available is preferable to a faster run that causes sustained swap growth or a system freeze.

### Capability before restriction

Agent Lowmem should still attempt useful work. It reduces concurrency and scopes validation before refusing execution. An amber or red preflight blocks a heavy command on the validated profile. Unavailable mandatory measurements return a measurement error instead of pretending the host is healthy or unhealthy.

### Explicit and reversible behavior

Every file change is previewable and delimited. Every runtime limit is command-scoped. No implicit global tuning is allowed.

### Evidence over guessed flags

Adapters must identify installed tools and versions from lockfiles, manifests, and resolved executables. An adapter may apply only options supported by that detected tool version.

### Ownership boundaries

Agent Lowmem may signal only the process group it created. It may report unrelated high-memory processes but never terminate or reconfigure them.

### Deliberate v1 throughput limit

Version 1 requests one worker wherever the detected tool exposes a verified option. It never claims to control internal workers when no stable, version-tested interface exists. This deliberately sacrifices throughput and marketing breadth for a simpler, more honest first release.

## 7. User interface

The executable is `agent-lowmem` and exposes four v1 commands.

### `agent-lowmem doctor`

Inspects the host and repository without writing files.

```text
agent-lowmem doctor
agent-lowmem doctor --json
```

It reports:

- host architecture, validated-run support, and observation-only status;
- physical memory;
- current kernel memory-pressure state and its direct green, amber, or red mapping;
- cumulative swap currently in use, labeled as historical evidence rather than proof of an active crisis;
- detected package manager and supported tools;
- fixed pressure policy, optional Node heap guardrail, its coverage, and known internal fan-out limitations;
- whether another managed heavy operation owns the global lock;
- warnings and recommended next action.

`doctor` succeeds when it can inspect the host even if the profile is observation-only or the current pressure state is amber or red. Its structured output exposes `runSupported: false` and the reasons. It does not require a Git repository; outside one, repository fields are reported as unavailable. A mandatory pressure measurement failure returns code 69.

### `agent-lowmem init`

Creates or updates repository policy.

```text
agent-lowmem init --dry-run
agent-lowmem init
```

The command:

1. requires the validated v1 run profile, a Git repository, and a root `package.json`;
2. runs the same inspection as `doctor`;
3. previews changes when `--dry-run` is supplied;
4. writes `.agent-lowmem.json`;
5. inserts or replaces one managed block in the Git root's `AGENTS.md`;
6. writes a private restoration manifest under the repository's resolved Git metadata directory, containing hashes and the prior managed content;
7. generates no timestamps or absolute paths and remains byte-for-byte idempotent for the same CLI version, configuration, and repository evidence.

The Git root and metadata directory are resolved by walking parent directories and interpreting a `.git` directory or worktree pointer file. `doctor` and `init` do not spawn `git` merely to discover the repository.

### `agent-lowmem run`

Runs a supported repository script under the active policy.

```text
agent-lowmem run test
agent-lowmem run test -- path/to/file.test.ts
agent-lowmem run test --workspace web -- path/to/file.test.ts
agent-lowmem run typecheck
agent-lowmem run build
agent-lowmem run lint
agent-lowmem run build --json-file .agent-lowmem-result.json
```

Only scripts present in the selected `package.json` may be executed. `--workspace <key>` is required for a monorepo target and must match a stable key in `.agent-lowmem.json`; pnpm targets are executed with the entry's exact `--filter`. Arbitrary shell execution is outside v1. Additional arguments are passed as argument-array elements without shell interpolation.

`run` requires the validated v1 profile and a green preflight. Amber and red preflights return code 75 without launching the script. The child inherits stdin, stdout, and stderr. `run` therefore does not support `--json` on stdout; it accepts `--json-file <path>` and atomically writes Agent Lowmem's structured result there. Test, typecheck, and lint operations default to a 15-minute timeout, while builds default to 30 minutes. Every operation prints a warning at 80% of its wall-clock timeout.

### `agent-lowmem restore`

Removes Agent Lowmem's managed repository changes.

```text
agent-lowmem restore --dry-run
agent-lowmem restore
```

It removes the managed `AGENTS.md` block and restores or removes `.agent-lowmem.json` according to the restoration manifest. `restore` does not require a validated host because recovery must remain possible after a hardware or operating-system change. After a fresh clone where that private manifest does not exist, it may remove a valid versioned managed block using its marker and semantic content hash, but it never reconstructs unknown prior content. It refuses restoration when managed content has been edited semantically and never rewrites unrelated `AGENTS.md` content.

Without a restoration manifest, `.agent-lowmem.json` is removed only when it byte-for-byte matches the deterministic configuration that the current CLI would generate from current repository evidence. Otherwise it is preserved and reported for manual review.

## 8. Architecture

Agent Lowmem is a native Rust CLI organized into focused modules. The executable does not require Node.js to start, does not embed a garbage collector, and does not use an asynchronous runtime.

The Rust workspace uses edition 2024 with Rust 1.85 as its minimum supported Rust version. Release and CI builds use the current stable Rust toolchain and a committed `Cargo.lock`.

First-party crates must declare `#![forbid(unsafe_code)]`. V1 starts with reviewed safe interfaces from `sysctl` 0.7.1 for named kernel values, `libproc` 0.14.11 for process-group enumeration and `RUsageInfoV4`, and `rustix` 1.1.4 for Unix process-group and signal operations; these exact versions are committed in `Cargo.lock`. Every direct runtime dependency requires a documented purpose and source review. If a future feature requires first-party Mach or Dispatch FFI, it must be specified separately behind one narrow platform crate rather than weakening this rule silently.

The core runs on one thread. It monitors the managed child with a synchronous `try_wait` and fixed sampling loop rather than adding Tokio, async-std, a background service, or a resident Dispatch queue.

Release builds use link-time optimization, one code-generation unit, symbol stripping, and `panic = "unwind"`. A top-level unwind boundary converts unexpected panics into cleanup of the owned child process group and lock before returning an internal error. Optimization targets runtime performance while preserving a small binary; lifecycle safety and measurement take precedence over speculative size reductions.

### 8.1 Host inspector

Responsibilities:

- detect `darwin`, `arm64`, chip family, physical memory, page size, and macOS major version;
- mark only Apple M2, 8 GiB, 16 KiB pages, and macOS major version 26 as the validated v1 run profile;
- read physical memory and the kernel memory-pressure state;
- read current swap usage as contextual telemetry, without deriving health from cumulative use;
- normalize the public macOS pressure constants into `normal`, `warning`, and `critical` states;
- probe required values at runtime so a compatible macOS 26 minor or patch update is accepted only when the expected pressure interface still works;
- avoid logging serial numbers, hardware UUIDs, usernames, environment secrets, or command environments.

The initial native keys are `kern.osproductversion`, `machdep.cpu.brand_string`, `hw.memsize`, `hw.pagesize`, `kern.memorystatus_vm_pressure_level`, and `vm.swapusage`. The capability probe validates existence, readable type, expected width or layout, and pressure-value domain. Profile and pressure keys are mandatory before `runSupported` can be true; `vm.swapusage` is optional context and may be reported as unavailable without changing the pressure class. V1 does not spawn `sw_vers`, `sysctl`, `vm_stat`, or `memory_pressure`; those command names describe diagnostic equivalents, not runtime dependencies.

For macOS 26, the exported pressure values must match the public dispatch flags `NORMAL = 0x01`, `WARN = 0x02`, and `CRITICAL = 0x04`. An unknown value or unavailable mandatory signal produces measurement error 69. It does not fabricate a green, amber, or red classification.

Platform calls are exposed to first-party code through a reviewed safe dependency boundary. First-party crates remain free of `unsafe` blocks.

V1 has no sample cache, derivative calculation, learned threshold, or calibration artifact. Each command reads the current state directly. This removes stale-cache behavior and makes `doctor` a single bounded native inspection.

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

The classifier maps the current kernel state directly and returns a class and reason. It never uses raw `Pages free`, the percentage printed by `memory_pressure -Q`, cumulative swap usage, compressor rates, or fitted thresholds as a health signal.

The kernel memory-pressure state is canonical:

- **Green:** the kernel reports `normal`.
- **Amber:** the kernel reports `warning`.
- **Red:** the kernel reports `critical`.

A green preflight is required for `run`. Amber and red preflights return code 75 without starting the child. During a run, critical pressure triggers protection on the first sample. Warning pressure triggers protection after two consecutive 250 ms samples; a normal sample resets the warning streak. Cumulative swap use is reported as historical context but never changes the class.

Kernel pressure is system-wide. If an unrelated application causes warning or critical pressure, the managed operation may be terminated even after substantial work. This is an explicit v1 trade-off: preserving laptop responsiveness takes precedence over attributing fault or preserving a long local build.

### 8.4 Fixed policy and observed footprint

The policy engine converts host, repository, and adapter evidence into a launch plan. Its fixed v1 controls are:

- one Agent Lowmem-managed heavy operation per local user;
- green-only launch;
- termination on the first observed critical sample and after two consecutive warning samples;
- watch mode denied;
- one worker where a verified adapter option exists;
- a 1,024 MiB V8 old-space guardrail per Node process where the tool preserves `NODE_OPTIONS`;
- a bounded wall-clock timeout with an 80% warning.

The 1,024 MiB value is deliberately conservative for the validated 8 GiB profile. It is not a total-memory cap: native allocations, additional Node processes, worker threads, memory-mapped files, and tools that strip `NODE_OPTIONS` remain outside it. If a supported fixture cannot complete under the guardrail, v1 reports the limitation and recommends focused validation or CI; it does not raise the value automatically.

Existing `NODE_OPTIONS` is never discarded. Agent Lowmem tokenizes it without evaluation solely to detect `--max-old-space-size` and `--max_old_space_size`, in either separated or `=` form. An unparsable value or a heap value other than 1,024 MiB returns configuration error 2 without printing the environment value. An equal value is preserved. Otherwise Agent Lowmem appends its guardrail to the original string. All other options remain byte-for-byte before the appended flag.

The runner enumerates the proven process group once per second and sums each visible member's `ri_phys_footprint`. Human and structured output call this `sampledAggregateFootprint`; it is observation only and never causes termination in v1. The value can miss short-lived workers and can overcount shared physical pages, so output always includes the one-second interval and this limitation. The highest sampled value is recorded for diagnostics.

V1 does not set `RLIMIT_AS`, `RLIMIT_RSS`, or another inherited memory rlimit. macOS does not provide a cgroup-equivalent process-tree cap, address-space limits conflict with V8's virtual-memory reservations, and an rlimit would not repair the accounting gaps above.

A launch plan contains:

- package manager executable and argument array;
- selected package script;
- selected workspace and lifecycle phases;
- command-scoped environment additions;
- adapter-provided serial options;
- optional Node old-space guardrail and its known coverage;
- verified serial options and disclosed internal fan-out limitations;
- global-lock requirement;
- fixed pressure action and sampling intervals;
- timeout and its warning point;
- explanations suitable for terminal and structured output.

The engine never mutates the caller's shell environment.

### 8.5 Tool adapters

Each adapter has one purpose: inspect a supported tool version and return argument-array additions, heap-guardrail coverage, and known worker behavior for an operation.

V1 adapters cover:

- npm script forwarding;
- pnpm script forwarding;
- Vitest single-worker, non-watch execution;
- Jest serial execution;
- Next.js build detection and pressure supervision without a blanket internal-worker guarantee;
- NestJS build execution under the fixed pressure policy;
- generic Node-backed `typecheck` and `lint` scripts under the fixed pressure policy.

Adapters must not inject a flag solely because it worked in another version. Version-specific mappings are represented as a tested compatibility matrix that distinguishes options such as Vitest file parallelism, pool behavior, and worker count. An unknown Vitest or Jest version returns unsupported code 64 when serialization cannot be proven.

Next.js is intentionally different. V1 may supervise a detected `next build`, but it does not edit or execute `next.config.*`, inject undocumented flags, or claim that all internal workers are serialized. The launch plan and documentation label Next.js internal fan-out as uncontrolled unless a future public, version-tested interface proves otherwise. They also disclose that known Next.js static workers may remove the inherited old-space flag, making guardrail coverage partial. Kernel pressure termination remains active regardless of framework behavior.

Package-manager lifecycle scripts associated with an operation run sequentially in the same managed process group and timeout. `init` records their presence, and `run` re-inspects them so a manifest change cannot silently bypass the generated policy. Adapter flags apply only to the target script, so a lifecycle phase containing watch mode or background execution is rejected. Known internal parallel orchestration is disclosed and permitted only for explicitly supervised tools such as the limited Next.js case above.

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

Before spawning, Agent Lowmem sets an inherited `AGENT_LOWMEM_ACTIVE=1` marker and an opaque run identifier. A nested invocation detects the marker before lock acquisition and returns code 73 with reason `nested-invocation`, without printing the identifier. This distinguishes package-script recursion from ordinary lock contention.

The parent installs signal handlers before spawning. An external `SIGINT`, `SIGTERM`, or `SIGHUP` is forwarded to the managed process group. After group cleanup and lock release, Agent Lowmem atomically writes a requested structured result, restores the default handler, and re-raises the same signal on itself so the shell observes the conventional signal status, including 130 for Ctrl-C. A top-level unwind boundary and RAII guards clean up the owned group and lock after a Rust panic before returning code 70. `SIGKILL` and host power loss cannot be caught; this limitation is explicit and is why the lock records both parent and child identities.

The monitor checks native pressure every 250 ms and samples aggregate process-group footprint every fourth tick. It terminates according to this order:

1. the first 250 ms sample reporting kernel critical pressure triggers `SIGTERM`;
2. kernel warning pressure for two consecutive 250 ms samples triggers `SIGTERM`;
3. reaching 80% of the operation timeout prints one warning but does not change the deadline;
4. operation timeout triggers `SIGTERM` and timeout status;
5. after ten seconds, `SIGKILL` is sent only to surviving members of the proven owned process group;
6. the lock is released after the group exits or ownership can no longer be proven.

The pressure action is fixed and cannot be weakened by repository configuration. If the supervisor sent the terminating signal for pressure, the parent returns 75 after cleanup regardless of the child's signal status. If it sent the signal for timeout, it returns 124. Attribution is intentionally irrelevant: the policy protects host usability whether pressure originated in the managed command or an unrelated application.

Agent Lowmem does not delete partial build artifacts. Its output states that the interrupted tool may have left generated files and recommends the tool's normal clean command or CI rather than guessing what can be removed.

### 8.8 Policy-file manager

The managed `AGENTS.md` section is bounded by a versioned marker containing a hash of the deterministic generated content:

```markdown
<!-- agent-lowmem:start version="1" content-sha256="<content-hash>" -->
## Agent Lowmem resource policy

Resource-heavy commands must run through Agent Lowmem. Run one heavy
operation at a time, never use watch mode, and prefer focused validation
before broad suites. Do not bypass an amber or red preflight, alter the
fixed memory guardrail, or retry an OOM automatically.
<!-- agent-lowmem:end -->
```

The marker's `version` identifies the managed-block format. The digest covers a canonical event stream produced by parsing the body between the markers as the limited Markdown subset generated by Agent Lowmem. In that stream, CRLF and LF are equivalent, soft line breaks and runs of prose whitespace become one ASCII space, and code spans, fenced code, links, and structural events retain their semantic content. Markdown parse failure is a conflict. This permits formatter reflow while detecting changed words, commands, links, or structure.

The actual marker contains the lowercase SHA-256 digest, not the literal placeholder shown in the format example. The generated block includes repository-specific supported commands and workspace selectors but no timestamps, usernames, or absolute paths. Generation remains byte-for-byte deterministic; semantic comparison is used only to validate replacement and restoration. A CLI release that intentionally changes generated policy produces one reviewable hash/body diff. Content outside the markers is immutable from Agent Lowmem's perspective.

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
      "lifecycleScripts": []
    }
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

The schema accepts operation timeouts from 60 through 3600 seconds and rejects unknown fields, arbitrary commands, absolute workspace paths, and lifecycle data that no longer matches repository evidence. Concurrency, watch denial, pressure action, sampling intervals, and the Node guardrail are fixed implementation policy rather than configurable fields. V1 has no safety-weakening override: an agent cannot make a run more permissive by editing a committed text reason.

## 10. Data flow

```text
CLI request
  -> host inspection
  -> validated-profile or observation-only decision
  -> repository inspection
  -> direct kernel-pressure classification
  -> adapter selection
  -> fixed-policy launch decision
  -> global lock acquisition
  -> final green-state recheck
  -> managed child launch
  -> 250 ms pressure monitoring and 1 s footprint observation
  -> child completion or owned-process termination
  -> lock release
  -> human result and optional atomic JSON result file
```

`doctor`, `init --dry-run`, and `restore --dry-run` stop before lock acquisition because they do not run heavy work. `doctor` may finish in observation-only mode. `init` requires the validated profile before writing. `restore` bypasses host validation so managed files remain recoverable.

## 11. Errors and exit codes

Errors must state what happened, what Agent Lowmem changed, and the safest next action.

| Code | Meaning |
| ---: | --- |
| 0 | Command completed successfully |
| 2 | Invalid CLI usage or configuration |
| 64 | Unsupported run profile, repository, package manager, workspace, tool version, or script |
| 69 | Required macOS measurement unavailable |
| 70 | Internal failure after best-effort owned-resource cleanup |
| 73 | Live heavy-operation lock or nested invocation already exists |
| 75 | Command blocked or terminated by the fixed pressure policy |
| 78 | Managed-file conflict prevents safe init or restore |
| 124 | Managed operation exceeded its configured timeout |
| child | Managed command exit code, preserved exactly even when it equals a reserved Agent Lowmem value |

Agent Lowmem internal codes apply when no child result exists, except protection termination and timeout. If a child naturally exits with the same number as an internal code, the shell value is inherently ambiguous; terminal text and the structured result's `exitOrigin` field distinguish `child`, `supervisor-pressure`, `supervisor-timeout`, `external-signal`, and `internal`.

A child naturally terminated by a signal uses the platform's conventional `128 + signal` representation. That rule does not apply to a signal Agent Lowmem sent for pressure or timeout: those return 75 and 124 respectively. For an external `SIGINT`, `SIGTERM`, or `SIGHUP`, Agent Lowmem forwards, cleans up, and re-raises the signal on itself; the shell therefore observes the normal signal result, including 130 for Ctrl-C.

No automatic retry occurs after OOM, pressure protection, timeout, or a failed build. The tool recommends focused validation or CI rather than silently increasing resources.

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
- host profile, runtime-capability results, and `runSupported` status;
- normalized measurements;
- pressure class and reasons;
- current swap usage and, for run results, sampled aggregate-footprint telemetry explicitly labeled non-enforcing;
- detected tools;
- fixed policy, guardrail coverage, and internal fan-out disclosures;
- operation result, exit origin, and exit reason.

Agent Lowmem-generated human and structured fields never include usernames, absolute home-directory paths, environment values, or unrelated process command lines. This redaction guarantee does not apply to inherited child output, which may contain project paths, stack traces, or tool-defined diagnostics.

## 14. Testing strategy

The implementation plan must preserve a low-resource Rust test suite. Repository validation runs sequentially with `cargo test -- --test-threads=1`; formatting, linting, tests, security checks, and release builds run as separate operations rather than concurrently.

### Unit tests

- native macOS snapshot normalization and public pressure-constant mapping;
- direct green, amber, red, warning-streak, immediate-critical, and unavailable-measurement boundaries;
- macOS-major, hardware, memory, page-size, and runtime-capability profile checks;
- observation-only versus validated-run decisions;
- fixed Node guardrail construction, existing-option parsing, conflict rejection, and value redaction;
- sampled process-group footprint aggregation and limitation metadata;
- package-manager and tool detection;
- adapter version mappings;
- launch-plan construction;
- semantic Markdown canonicalization plus managed-block insertion, replacement, conflicts, and restoration;
- stale and live lock decisions;
- signal, pressure, timeout, recursion, and panic cleanup state machines;
- redaction and JSON schema behavior;
- enforcement of the first-party `unsafe` prohibition.

### Integration tests

- fixture repositories for npm, pnpm, single packages, and monorepos;
- unambiguous and ambiguous workspace selection plus exact pnpm filtering;
- lifecycle phase detection and rejection of unsafe background/watch patterns;
- version-matrix fixtures proving exact serial strategies for Vitest and Jest;
- unsupported tool versions refusing execution instead of guessing flags;
- Next.js fixtures proving explicit uncontrolled-fan-out and partial-heap-coverage disclosures;
- NestJS and generic Node fixture builds under the fixed policy;
- lock contention between two processes;
- nested package-script invocation returning a distinct recursion reason;
- inherited TTY stdio without captured-pipe deadlock;
- `SIGINT`, `SIGTERM`, and `SIGHUP` forwarding to the managed group;
- parent re-signaling after external signals and shell-visible Ctrl-C status 130;
- managed child cleanup after simulated warning, critical pressure, timeout, and Rust panic;
- termination without signaling an unrelated sentinel process;
- exact child exit-code preservation plus child-signal, supervisor-pressure, supervisor-timeout, and external-signal disambiguation;
- atomic `--json-file` output separated from child streams;
- green preflight and final pre-launch pressure recheck using injected snapshots;
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

The `doctor` wall-time and memory measurements include native host inspection and repository discovery. `doctor` has no cache, does not wait for a second sample, and does not spawn `git` or monitoring utilities. Results are recorded with release artifacts. A budget regression blocks release unless this design is revised explicitly.

## 15. Acceptance criteria

Version 1 is ready when all of the following are true:

1. On the reference Apple M2 8 GB Mac, `doctor` reads native pressure and produces a deterministic, explained classification.
2. Green, amber, and red map only to the kernel's normal, warning, and critical states; no cache, derivative, or calibration artifact affects the result.
3. Missing or unknown mandatory measurements return code 69 and never masquerade as a healthy or unhealthy state.
4. `doctor` works outside Git and on unvalidated macOS arm64 profiles in observation-only mode with `runSupported: false`.
5. The validated run profile matches macOS major 26 while accepting minor and patch updates only after runtime-capability probes pass.
6. Amber or red preflight and a nonvalidated run profile do not start a child process.
7. The first monitor sample reporting kernel critical pressure triggers protection; two consecutive warning samples trigger protection within 500 ms plus scheduling tolerance.
8. Sampled aggregate footprint is reported with its interval and accounting limitations and never presented or enforced as a hard cap.
9. `init --dry-run` shows the exact proposed files and `init` produces the same deterministic content.
10. Repeated same-version `init` calls produce no duplicate block or unrelated diff; formatter-only Markdown reflow does not prevent update or restore.
11. Monorepo execution requires an unambiguous workspace and uses the exact configured npm workspace or pnpm filter.
12. A supported Vitest or Jest script runs with one worker and no watch mode; a version without proven serialization returns code 64.
13. A detected Next.js build is pressure-supervised while output explicitly states that internal fan-out is uncontrolled and heap-guardrail coverage may be partial.
14. The fixed 1,024 MiB Node guardrail preserves non-conflicting `NODE_OPTIONS`; conflicting or unparsable heap options return code 2 without exposing their values.
15. Two simultaneous managed heavy commands cannot start, and nested invocation returns code 73 with reason `nested-invocation`.
16. Pressure protection returns 75, timeout returns 124, a natural child signal uses `128 + signal`, and Ctrl-C cleans up before the parent re-signals itself so the shell observes 130.
17. Pressure, timeout, forwarded signals, and a simulated Rust panic clean up only the proven owned process group and lock.
18. A normal child failure preserves inherited output and its exact exit code; structured output records its origin.
19. `run --json-file` never mixes Agent Lowmem JSON into child stdout or stderr.
20. `restore --dry-run` previews reversal; restore remains available on an unsupported host, preserves unrelated `AGENTS.md` content, and safely handles a valid marker after a fresh clone.
21. Agent Lowmem performs no network request and leaves no background process after each command.
22. All first-party crates reject `unsafe` code at compile time, unit/integration/end-to-end tests run sequentially, and dependency-policy checks pass.
23. The native release binary and npm invocation satisfy their separate documented memory, startup, size, and cleanup budgets on the reference Mac.
24. The signed binary is identical across Homebrew, GitHub Release, and the macOS npm package; installing the portable npm root package remains non-failing on Linux, Windows, and Intel macOS.

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
- validated-profile and observation-only behavior;
- explanation that sampled aggregate footprint is telemetry rather than a hard memory cap;
- Next.js internal-fan-out and partial-heap-guardrail limitations;
- troubleshooting for OOM, pressure protection, `NODE_OPTIONS` conflict, recursion, lock contention, and unsupported flags;
- explicit explanation that direct package-manager commands bypass enforcement;
- npm platform-support and launcher-overhead documentation;
- security and privacy statement;
- contribution guide requiring low-memory-safe tests;
- website copy for `agentlowmem.dev` using the approved name and tagline.

## 18. Deferred roadmap

The following require separate specifications after v1 evidence exists:

- Linux and Intel Mac support;
- validated M1, M3, M4, additional memory-size profiles, and later macOS major versions;
- Python, Rust, Java, Flutter, and container adapters;
- controlled compressor/swap derivatives with a bootstrap collector, run-grouped holdout validation, and an explicit no-ship failure branch;
- a dedicated first-party platform crate if future Mach or Dispatch FFI requires narrowly reviewed `unsafe` code;
- higher-frequency process accounting, quantified sampling error, and any evidence-backed enforcement stronger than observation;
- multiple named policy profiles and a human-authorized mechanism for safety-weakening overrides;
- CI recommendation generation;
- interactive dashboard or menu-bar status;
- historical measurements and adaptive per-project guardrails;
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
- [`sysctl` crate](https://docs.rs/sysctl/latest/sysctl/) for the reviewed safe named-kernel-value boundary.
- [`libproc::processes::ProcFilter`](https://docs.rs/libproc/latest/libproc/processes/enum.ProcFilter.html) and [`RUsageInfoV4`](https://docs.rs/libproc/latest/libproc/pid_rusage/struct.RUsageInfoV4.html) for process-group enumeration and sampled physical footprint.
- [`rustix::process`](https://docs.rs/rustix/latest/rustix/process/) for safe process-group creation, validation, waiting, and signaling.
- [Next.js build worker source](https://github.com/vercel/next.js/blob/canary/packages/next/src/build/index.ts) and [worker implementation](https://github.com/vercel/next.js/blob/canary/packages/next/src/lib/worker.ts) for version-sensitive fan-out and inherited heap-limit behavior.
- [Node.js command-line options](https://nodejs.org/api/cli.html#node_optionsoptions) for `NODE_OPTIONS` handling.
- [Rust Reference: destructors](https://doc.rust-lang.org/reference/destructors.html) for the cleanup consequences of aborting without unwind.
- [Rust `Command`](https://doc.rust-lang.org/std/process/struct.Command.html) for inherited stdio behavior.
- [npm `package.json`](https://docs.npmjs.com/files/package.json/) for `os`, `cpu`, and optional dependency semantics.
- [Apple notarization workflow](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) for direct-download release handling.
