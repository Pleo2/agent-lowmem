# Agent Lowmem v1 Design

**Status:** Candidate for final implementation review

**Date:** 2026-09-02

**Revision:** 6 — bounded implementation contract

**Product:** Agent Lowmem

**Domain:** `agentlowmem.dev` (acquired 2026-09-02)

**Repository and npm package:** `agent-lowmem`

**Tagline:** More agents. Less RAM.

## 1. Summary

Agent Lowmem is an open-source native command-line policy runner that makes agent-launched validation predictable on Apple Silicon Macs. The production CLI is written in Rust. Version 1 targets JavaScript and TypeScript repositories that use Node.js, npm or pnpm, Next.js, NestJS, Vitest, Jest, or ESLint.

Its distinctive runtime value is not an instruction that an agent may ignore. Agent Lowmem owns a cross-repository per-user lock and the lifecycle of the process group it launches. That prevents two compliant agents in different repositories from starting heavy work together and gives timeout or interruption cleanup one narrow, auditable ownership boundary.

Version 1 enforces only controls whose behavior is deterministic and independently testable:

- one Agent Lowmem-managed heavy operation per local user;
- no watch mode or recognized background execution;
- one test worker where the detected tool version exposes a verified public option;
- bounded wall-clock time;
- focused validation guidance before broad suites;
- owned process-group cleanup on timeout, interruption, or supervisor failure.

Version 1 does **not** set a Node heap size, infer a safe memory budget, block launch from a private pressure snapshot, or terminate a command in response to an unvalidated memory-pressure signal. Those mechanisms previously created a stronger promise than the available evidence supported.

The v1 promise is deliberately narrow:

> One managed heavy operation across repositories, verified low-concurrency test execution, no watch mode, and cleanup of the process group Agent Lowmem owns.

The owner's `Mac14,15` M2 MacBook Air with 8 GiB of unified memory and macOS 26.x remains the reference benchmark for resource budgets. Other supported macOS arm64 hosts may run with an explicit unvalidated-performance notice. This is risk reduction, not a guarantee that every build will complete or that macOS will never swap, beachball, freeze, or terminate a process.

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

1. Detect a supported macOS arm64 runtime and separately identify whether the host matches the validated performance-reference profile.
2. Generate an idempotent, clearly delimited Agent Lowmem block in the Git root's `AGENTS.md`.
3. Detect npm or pnpm, configured workspaces, supported scripts, and supported tool versions from repository evidence without starting Node.js, a package manager, a repository executable, or repository code during inspection.
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
- claim performance validation for M1, M3, M4, a different Mac model, another memory size, or another macOS major version;
- support `init` or `run` outside a Git-backed JavaScript or TypeScript repository;
- coordinate multiple autonomous agents beyond publishing and enforcing repository policy;
- prevent a user or agent from bypassing the policy by invoking package-manager commands directly;
- guarantee cleanup of a descendant that deliberately or indirectly escapes the owned process group with `setsid` or a new process group.

The last limitation is important: Agent Lowmem signals the process group it created. A process that escapes that group is no longer inside the ownership boundary and may survive cleanup. V1 reports this boundary and never scans for similarly named processes to compensate.

## 5. Users and primary scenario

The primary user is a developer using a coding agent on an Apple Silicon Mac with limited memory. The initial performance evidence comes from the owner's Apple M2 MacBook Air with 8 GiB of unified memory.

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

An observed signal does not become a termination boundary until prospective measurements show that it is timely and sufficiently specific on the reference host. Unknown pressure timing and an unmeasured heap number are not safety controls.

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

- operating system, architecture, hardware identity, physical memory, page size, supported-runtime status, and performance-validation status;
- whether `init` and `run` are supported on the host;
- Git root availability without printing the absolute root in structured output;
- detected package manager, workspaces, configured scripts, installed tool versions, and adapter support;
- fixed v1 controls and known internal fan-out limitations;
- whether another Agent Lowmem operation owns the per-user lock;
- the next recommended action.

`doctor` does not report a current memory-health color and does not read the private pressure-level sysctl. A capability-compatible macOS arm64 host reports `runSupported: true`; a host outside the reference profile also reports `performanceValidated: false` and the mismatched profile fields. Outside a repository, host inspection still succeeds while repository operations are unavailable.

`doctor` starts no child process. Repository inspection for `doctor`, `init`, `init --dry-run`, and the pre-lock and post-lock phases of `run` is data-only: it does not start `node`, npm, pnpm, Git, a package binary, or a repository script. Only `run`, after acquiring the lock and successfully rechecking evidence, may start the selected package manager.

### `agent-lowmem init`

Creates or updates repository policy.

```text
agent-lowmem init --dry-run
agent-lowmem init
```

The command:

1. requires a supported macOS arm64 runtime, a Git repository, and a root `package.json`;
2. performs the same repository inspection as `doctor`;
3. previews exact changes when `--dry-run` is supplied;
4. writes `.agent-lowmem.json`;
5. inserts or replaces one managed block in the Git root's `AGENTS.md`;
6. writes a private restoration manifest inside the repository's resolved Git metadata directory;
7. emits no timestamp or absolute path into managed repository files;
8. remains byte-for-byte idempotent for the same CLI version, configuration, and repository evidence.

The Git root and metadata directory are resolved by walking parents and interpreting either a `.git` directory or a worktree pointer file. Inspection does not spawn `git`.

`init` seeds only canonical scripts that are present and runnable under the current matrix. If a canonical `test`, `typecheck`, `lint`, or `build` script is denied or unsupported, dry-run reports compatible same-package candidates with the corresponding name prefix, such as `test:unit`, but never aliases one silently. A human may map one of those exact script names to an operation in `.agent-lowmem.json`.

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

The trust boundary is explicit: Agent Lowmem starts npm or pnpm without constructing a shell string, but those package managers execute package scripts using their documented shell semantics. The package-manager argument array pins the script shell to `/bin/sh` and disables pnpm's shell emulator for the managed invocation. Agent Lowmem is not a sandbox for repository code.

`run` requires a supported macOS arm64 runtime. On a host outside the reference profile, it prints one stable unvalidated-performance notice before lock acquisition; the notice is informational and does not require an override. The command then acquires the global lock, revalidates the relevant manifests and scripts, and releases the lock without starting a child if evidence changed during launch planning. Child stdin, stdout, and stderr are inherited. Agent Lowmem JSON is therefore available only through `--json-file <path>`.

Test, typecheck, and lint operations default to 15 minutes. Builds default to 30 minutes. Each operation prints one warning at 80% and is terminated at its deadline. Timeouts may be configured from 60 through 3,600 seconds.

### `agent-lowmem restore`

Removes Agent Lowmem-managed repository changes.

```text
agent-lowmem restore --dry-run
agent-lowmem restore
agent-lowmem restore --dry-run --force-managed-block
agent-lowmem restore --force-managed-block
```

`restore` bypasses the `init` and `run` host-capability and reference-profile gates, although the installed native binary must still be executable on the platform. With a restoration manifest, it verifies exact managed bytes and restores or removes `.agent-lowmem.json` and the managed `AGENTS.md` block without touching surrounding content.

After a fresh clone without a private manifest, it may remove a managed block whose exact body hash matches its marker. It removes `.agent-lowmem.json` only when the file exactly matches deterministic current output; otherwise it preserves the file for manual review.

If the block body was edited or reformatted, ordinary restore returns conflict code 78. `--force-managed-block` is a narrow escape hatch: it removes exactly one well-formed start-to-end Agent Lowmem block even when its body hash differs. It never removes text outside the markers, follows no symlink, reconstructs no unknown prior content, and never forces removal of a conflicting `.agent-lowmem.json`. Dry-run displays the exact affected byte range before the destructive form is used.

## 8. Architecture

### 8.1 Production language boundary

The shipped CLI and its production libraries are Rust. They do not link a Swift helper, require a Swift runtime, or invoke the experimental probe.

The repository contains `tools/pressure-probe`, a Swift research instrument with no third-party package dependencies. Swift was selected for that probe because the public macOS Dispatch memory-pressure API is directly exposed there and the reference Mac already had Swift 6.3 while Rust was not installed. The probe is observational, is excluded from release artifacts, and does not define the production architecture.

If evidence later justifies pressure-based behavior, it requires a new design revision and a narrow Rust macOS integration. The safe Rust core remains independent of Dispatch. Any required FFI or `unsafe` code must be isolated in one platform module, documented with safety invariants, audited, and tested separately; it cannot enter by silently weakening a package-wide rule.

### 8.2 Rust package

The root Rust package uses edition 2024 with Rust 1.85 as its minimum supported Rust version. Stable release and CI toolchains use a committed `Cargo.lock`.

The production design separates:

- CLI parsing and human/structured presentation;
- host capability inspection;
- repository and package evidence inspection;
- adapter selection and launch-plan construction;
- managed-file generation and restoration;
- per-user locking;
- owned process-group lifecycle and timeout supervision.

The first-party production package compiles with `#![forbid(unsafe_code)]` in v1. Platform operations must enter through reviewed safe standard-library or dependency interfaces. Direct runtime dependencies require a documented purpose, source review, license approval, and a version committed in `Cargo.lock`; this spec does not pre-approve a crate merely by naming it.

The supervisor uses no Tokio, async-std, daemon, resident service, or polling of the system process table. Its steady-state child loop performs only child-status, signal, warning-deadline, and timeout work.

Release builds use link-time optimization, one code-generation unit, symbol stripping, and `panic = "unwind"`. A top-level unwind boundary performs best-effort owned process-group and lock cleanup before returning internal error 70.

### 8.3 Host inspector

The supported v1 runtime requires:

- `darwin` operating system;
- `arm64` architecture;
- macOS 13.0 or later;
- readable native process-group, signal, monotonic-clock, per-user temporary-directory, and atomic-file capabilities used by the runner.

An unavailable capability returns unsupported code 64. Support means the deterministic lock, parsing, launch, timeout, and cleanup contract is available; it is not a performance claim.

The performance-reference profile is tracked independently and requires:

- hardware model exactly `Mac14,15`;
- CPU brand exactly `Apple M2` after trimming terminating whitespace;
- physical memory exactly `8,589,934,592` bytes;
- page size exactly `16,384` bytes;
- macOS product-version major exactly `26`.

The exact model and brand checks prevent an `Apple M2 Pro`, `Apple M2 Max`, or another M2 Mac from inheriting the reference benchmark by prefix. A different profile may still set `runSupported: true`, but it sets `performanceValidated: false`; resource-budget results and responsiveness claims never transfer to it.

### 8.4 Repository inspector and script grammar

#### 8.4.1 Responsibilities

The inspector:

- locates the Git root and root `AGENTS.md` without executing `git`;
- parses root and selected-workspace `package.json` files as data;
- identifies npm or pnpm from `packageManager` plus the matching lockfile;
- enumerates declared workspaces and requires an explicit stable key whose configured path and package name identify exactly one workspace;
- resolves a supported tool or wrapper's installed `package.json` without executing that package;
- compares its exact semantic version with the committed adapter matrix;
- reads the selected target plus its `pre<name>` and `post<name>` lifecycle scripts on every run;
- reads only matrix-declared repository package-manager configuration files as data and rejects repository-owned custom script-shell semantics;
- treats each declared pre/post lifecycle script as potentially reachable without querying whether machine policy will execute it;
- tokenizes every relevant script, expands supported same-package script references within the fixed graph limits, and classifies every leaf segment;
- returns explicit unsupported or conflict states instead of guessing.

Inspection has a zero-child-process contract. `doctor`, `init`, `init --dry-run`, launch planning, and the post-lock evidence recheck never start `node`, npm, pnpm, Git, a package binary, or any repository command. They also do not read user-level or global package-manager configuration. This keeps inspection inside the latency budget, avoids collecting machine-specific paths, and makes repository evidence reproducible.

For each exact package-manager version, the adapter matrix names the repository-owned configuration files and keys that can alter script-shell semantics. The initial npm adapter parses the Git-root `.npmrc` for `script-shell`. The initial pnpm adapter parses its matrix-declared project configuration, including relevant `.npmrc` and `pnpm-workspace.yaml` keys, for `scriptShell` and `shellEmulator`. Unknown syntax, substitution, an explicit shell other than `/bin/sh`, or an enabled shell emulator returns code 64 with reason `script-shell-unsupported`. Agent Lowmem rejects an explicit repository requirement rather than silently changing that repository's intended semantics.

Machine, user, global, and environment configuration cannot change the grammar used by a managed launch: the matrix-defined runtime argument array passes npm `--script-shell=/bin/sh`, and passes pnpm `--config.script-shell=/bin/sh` plus `--config.shell-emulator=false`. These options are accepted only for exact fixture-tested package-manager versions and rely on documented command-line precedence. They are command-scoped and do not modify any configuration file.

Agent Lowmem neither queries nor forces npm/pnpm lifecycle-enable settings. For classification it always includes each declared `pre<name>` and `post<name>` as a potential phase. A package manager may omit a pre/post phase because of machine policy, which only removes work that was already classified; no setting can introduce an unclassified declared lifecycle phase. Terminal and structured previews label these entries `potentialLifecycle: true` until the child runs.

#### 8.4.2 Classification grammar

Agent Lowmem's tokenizer is a policy analyzer, not a shell. It never evaluates a script and never reconstructs a modified shell string. After validation, npm or pnpm receives the original repository script unchanged; any Agent Lowmem policy flags are separate package-manager arguments.

A script may contain one or more command segments separated by `&&` outside quotes. The package manager's shell preserves the original left-to-right short-circuit behavior. Filesystem changes made by an earlier segment remain visible to later segments, just as they are during an ordinary package-manager run; Agent Lowmem does not claim the segments are state-independent.

Each argument is either one safe unquoted word or one fully quoted word; adjacent quoted and unquoted fragments are unsupported. For classification only, quotes are decoded. The original script bytes remain untouched.

The tokenizer accepts:

- non-empty unquoted words containing only ASCII letters, digits, `_`, `@`, `%`, `+`, `=`, `:`, `,`, `.`, `/`, or `-`;
- single-quoted literal arguments containing no single quote or line break; a backslash has no special meaning inside them;
- double-quoted literal arguments containing no `$`, backtick, or line break, where only `\"` and `\\` are accepted escape sequences;
- `*`, `?`, and `[` inside a quoted argument, because the shell passes them literally to the tool;
- `--` as an ordinary argument boundary;
- `&&` as the only top-level command separator.

It rejects the whole script with code 64 when it finds:

- any carriage return or line feed;
- `|`, `||`, a lone `&`, or `;` outside quotes;
- `$` or backtick outside single quotes, including `$(...)`, `${...}`, and `$VAR`, or an unquoted token-leading `~`;
- `>`, `>>`, `<`, `2>&1`, or another redirection;
- unquoted glob metacharacters `*`, `?`, or `[`;
- `(`, `)`, `{`, or `}` grouping outside quotes;
- a shell comment outside quotes;
- any backslash outside single quotes except the two permitted double-quote escapes above;
- adjacent quoted/unquoted word fragments;
- an empty segment, a leading or trailing `&&`, or an unterminated quote.

These restrictions do not sandbox the repository. They make static classification and final-argument placement unambiguous while the package manager retains responsibility for executing the original trusted script.

#### 8.4.3 Transparent wrappers

The tokenizer unwraps exactly two transparent wrappers when their installed package, version, and argument form match the adapter matrix:

- `cross-env KEY=value... <command> [args]`: consume one or more leading assignment tokens and classify the remaining command. Each decoded token must contain `=`, and its key must match `[A-Za-z_][A-Za-z0-9_]*`; the value may be empty. `cross-env-shell` is always denied because it explicitly delegates a new shell program.
- `dotenv [-e <file>]... -- <command> [args]`: resolve the executable to the reviewed `dotenv-cli` package, consume zero or more `-e` pairs, require `--`, and classify the remaining command. Each file must be a non-empty lexical path relative to the Git root with no `..` component; inspection never opens the dotenv file.

Unwrapping occurs at most once per segment. A wrapper with an unknown version, missing real command, unsupported option, absolute dotenv path, or malformed assignment is unsupported. Agent Lowmem's own output reports only the wrapper kind and count of consumed assignments or files; it never prints or persists assignment names, values, or dotenv paths. As stated in §12, this cannot redact a package manager that echoes its trusted script to inherited child output.

Any other leading executable is classified normally. In particular, `env`, `cross-env-shell`, `concurrently`, and parallel orchestrators are not treated as transparent wrappers.

#### 8.4.4 Bounded same-package script references

V1 expands only exact same-package references through fixture-tested forms of `node --run <script>`, `npm run <script>`, and `pnpm run <script>`. The referenced name must be one exact literal key in the same `package.json`. A reference with flags, arguments, `--`, a glob, workspace selection, or forwarded outer arguments is unsupported with reason `script-reference-unsupported`.

Expansion has three independent bounds:

- the selected target is depth zero and a reference may descend through at most depth three;
- the active expansion stack must remain cycle-free;
- the complete reachable graph may contain at most 32 leaf-segment occurrences across the selected target, all potential pre/post phases, and every expanded reference.

The segment budget counts occurrences, not unique script names: referencing the same safe script twice consumes its segments twice. Agent Lowmem checks the limit before admitting the thirty-third segment and returns code 64 with reason `script-graph-too-large`. A reference cycle returns `script-reference-unsupported`. These fixed limits are implementation constants, not repository configuration.

Package-manager references expand their declared pre/post scripts as potential lifecycle phases; Node references do not imply npm/pnpm lifecycle execution. Every nested controlled segment and lifecycle segment must already contain its required controls because Agent Lowmem never injects flags inside the expanded graph.

`npm-run-all`, `run-s`, and other sequential script orchestrators are deferred and return `script-reference-unsupported` in v1. `npm-run-all -p`, `run-p`, `concurrently`, Turbo/Nx fan-out, and every recognized parallel or race form return `parallel-denied`. This keeps the initial recursive grammar limited to three exact reference forms.

#### 8.4.5 Segment classification and flag placement

Each leaf segment resolves to one of:

- **controlled:** a matrix entry and tested installed version prove the required no-watch and low-concurrency state. The segment is either already controlled or requires a documented suffix injection.
- **disclosed:** a recognized build or analysis tool may fan out internally without a verified public control. It may run, but terminal and structured output state the limitation.
- **auxiliary:** a reviewed, versioned command form used for bounded preparation or cleanup. Its accepted arguments are defined in the matrix, it receives no policy flags, and the label makes no general claim that arbitrary filesystem commands are harmless.
- **denied:** a watch, UI, parallel, background, race, or adapter-specific denial token is present.
- **unsupported:** the executable, version, wrapper, script reference, or argument form is absent from the matrix.

A selected operation is runnable only when every reachable leaf segment is controlled, disclosed, or auxiliary and the selected target script contains at least one controlled or disclosed segment. Multiple controlled or disclosed segments are allowed because `&&` keeps them sequential.

npm and pnpm append forwarded arguments to the end of the selected script. Therefore Agent Lowmem may inject missing adapter flags only into the final top-level segment, after unwrapping `cross-env` or `dotenv`, and only when that leaf is not a script reference. Every earlier controlled segment and every lifecycle or nested segment must already contain its required controls. If a missing control would require editing a non-final, lifecycle, or nested segment, the operation is unsupported with reason `nonfinal-injection-required`.

Forwarded user arguments after Agent Lowmem's `--` go to that same final leaf. The final adapter validates them and rejects watch, UI, parallel, or conflicting concurrency options before launch. If the final leaf has no adapter-declared forwarded-argument contract, extra arguments are unsupported rather than attached to the wrong command.

If no injection or forwarded argument is required, Agent Lowmem runs the original script unchanged. If injection is allowed, it adds only the matrix-defined suffix to the package-manager argument array; it never edits `package.json` or synthesizes a replacement script.

#### 8.4.6 Adapter matrix artifact

`adapters/matrix-v1.json` is the versioned source of truth for:

- package-manager identities, tested versions, launch-array templates, repository-configuration files and keys, and lifecycle/forwarding semantics;
- executable and wrapper names;
- package identities and tested exact versions or ranges;
- command and subcommand forms;
- controlled, disclosed, and auxiliary classifications;
- required existing controls and permitted final-segment suffixes;
- denial tokens and forwarded-argument rules;
- auxiliary argument schemas and disclosure identifiers.

The artifact is validated against a bundled schema in CI and embedded into the release binary. The initial matrix begins with exact versions exercised by committed fixtures. A range may widen only after every newly covered version passes the same adapter fixture suite. Runtime never approximates an unknown version to the nearest entry.

The first matrix covers exact tested npm and pnpm versions plus tested forms of Vitest, Jest, the Node test runner, TypeScript `tsc`, ESLint, Next.js, NestJS, `cross-env`, `dotenv-cli`, and a deliberately short auxiliary set. That auxiliary set includes `rimraf` only with one or more static repository-relative paths, no option token, no glob, and no `..` component, which is enough for forms such as `rimraf dist && next build`. System `rm`, `cp`, and `mv` are not accepted in the initial matrix. Adding a package manager, tool, wrapper, auxiliary command form, denial token, or version is a reviewed artifact change with fixture evidence.

The initial launch templates are argument arrays, never reconstructed shell strings. Omitting the optional `-- <arguments...>` tail when it is empty, their semantic forms are:

```text
npm root:      npm --script-shell=/bin/sh run <script> [-- <arguments...>]
npm workspace: npm --script-shell=/bin/sh --workspace=<packageName> run <script> [-- <arguments...>]
pnpm root:      pnpm --config.script-shell=/bin/sh --config.shell-emulator=false run <script> [-- <arguments...>]
pnpm workspace: pnpm --config.script-shell=/bin/sh --config.shell-emulator=false --filter <packageName> --fail-if-no-match run <script> [-- <arguments...>]
```

The exact option spelling, ordering, forwarding behavior, and no-match exit behavior are fixture-tested for every admitted package-manager version. No launch template changes pre/post enablement. The pnpm workspace template uses both preflight cardinality and `--fail-if-no-match`: either a stale zero-match selection or a repository change blocks success rather than allowing a no-op test command to appear green.

#### 8.4.7 Lifecycle scripts and drift

`init` does not freeze lifecycle contents into configuration. On every run, Agent Lowmem re-reads `pre<name>` and `post<name>` and applies the same grammar, wrapper, script-reference, graph-budget, and matrix rules whether or not the current package-manager policy will execute those phases.

npm and pnpm do not pass the selected script's additional arguments to its pre/post phases. Consequently a lifecycle controlled segment must already include its required control flags; Agent Lowmem never claims to inject into it. Lifecycle phases may contain only accepted leaf classifications and execute inside the same owned process group and operation timeout.

A safe addition since the last `init` is incorporated into the current launch plan and reported as repository drift. An unsafe addition blocks the run with code 64 and names the repository-relative script key plus a stable reason, never the script contents.

#### 8.4.8 Evidence recheck

Before acquiring the lock, the launch plan records SHA-256 hashes of every repository file used for the decision: root and selected-workspace `package.json`, `.agent-lowmem.json`, the selected lockfile, every matrix-declared repository package-manager configuration file that exists, workspace-declaration data, and each resolved tool or wrapper `package.json`. No user/global configuration fingerprint exists because inspection does not read those files and command-scoped launch flags neutralize their script-shell settings.

After acquiring the lock, Agent Lowmem re-reads, re-parses, and re-hashes the same repository files without starting a child. On any difference, workspace-cardinality change, or classification change it releases the lock, launches nothing, and returns code 75 with reason `evidence-changed`. Human output names only the repository-relative manifest, lockfile, configuration file, package identity, or workspace evidence class that changed. Agent Lowmem does not retry automatically or persist cross-run counters solely to guess which external process caused the change.

### 8.5 Fixed execution policy

Every launch plan contains:

- exactly one selected package manager and configured operation;
- either the root package or one workspace whose configured path and package name have cardinality one;
- potential lifecycle phases and ordered classified segments within the depth-three and 32-segment graph limits;
- a matrix-defined package-manager argument array, including command-scoped script-shell controls, rather than a shell command built by Agent Lowmem;
- transparent-wrapper and bounded same-package script-reference evidence;
- existing controls plus any final-segment adapter suffix;
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

### 8.6 Adapter behavior

Adapters implement the matrix contract; executable names, package identities, versions, flags, and denial tokens live only in `adapters/matrix-v1.json`.

A controlled adapter can:

- confirm that a segment already contains its tested no-watch and low-concurrency state;
- return one exact suffix when the segment is the eligible final argument recipient;
- validate forwarded user arguments against conflicts and denial tokens;
- explain the applied or already-present control without exposing the original script.

A disclosed adapter returns no control suffix. It emits a stable disclosure identifier such as `internal-fanout-uncontrolled`, which appears in the launch preview, terminal warning, and structured result.

Vitest control requires a tested non-watch invocation with file parallelism disabled; Jest requires a tested non-watch invocation with `--runInBand`; the Node test runner requires a supported `--test-concurrency=1` form; TypeScript `tsc` accepts only fixture-tested non-watch compilation forms; and ESLint uses its tested single-thread setting or documented single-thread default. Exact forms remain matrix data rather than prose duplicated across the implementation.

Next.js and NestJS are disclosed rather than described as single-worker builds unless a future public, version-tested interface proves otherwise. On the 8 GiB reference host, their launch message recommends CI for broad builds. The global lock prevents a second managed top-level operation, but it never represents that lock as control over framework workers.

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

The steady-state supervisor checks child state and deadlines at no more than one wakeup per second. Signal handlers remain immediately available and do not wait for that tick to mark an external interruption. The supervisor does not sample memory, enumerate processes, or poll private pressure state. It prints the 80% warning within one second of its deadline. At the final deadline, with at most one second of scheduling tolerance, it sends `SIGTERM` to the owned process group, waits up to ten seconds, and sends `SIGKILL` only to the same group if members remain.

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

`init` includes only present canonical scripts that the current matrix can classify. A user may remove an operation to make it unavailable or add a mapping to another exact script reported as compatible by `doctor` or `init --dry-run`. Operation keys must match `[a-z][a-z0-9-]{0,31}`; they are labels, not commands. Lifecycle phases are current repository evidence and are not duplicated in configuration.

A monorepo adds stable workspace keys:

```json
{
  "workspaces": {
    "web": {
      "path": "apps/web",
      "packageName": "@acme/web",
      "operations": {
        "test": { "script": "test", "timeoutSeconds": 900 }
      }
    }
  }
}
```

The key `web` is the stable Agent Lowmem CLI key; it is never passed to a package manager. `path` must be a canonical repository-relative directory with no `..` component or symlink escape. `packageName` must equal that directory's `package.json.name`, satisfy the supported npm package-name grammar, and contain no pnpm selector operator such as `...`, `^`, `*`, `!`, `[`, `]`, `{`, or `}`.

Using repository manifests as data, Agent Lowmem verifies that the path is included by the supported root workspace declaration and that the exact package name identifies exactly one declared workspace at that same path. Zero matches, duplicate names, a name/path disagreement, or a package name that could be interpreted as selector syntax returns code 64 with reason `workspace-cardinality` and starts no child. Unsupported workspace-declaration syntax returns `workspace-unsupported`. Package-manager selector arguments are generated only from this validated exact name; `.agent-lowmem.json` never accepts a free-form selector.

The configuration schema accepts timeouts from 60 through 3,600 seconds and rejects unknown fields, including the removed `packageManagerSelector`, arbitrary commands, invalid operation keys, absolute workspace paths, duplicate configured package names, and missing scripts. Semantic validation performs the cardinality and declaration checks that JSON Schema cannot express. Concurrency, watch denial, graph limits, lock behavior, environment preservation, retry behavior, and cleanup are fixed implementation policy rather than configurable fields.

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

No stage before `managed process-group launch` starts a child process. If the post-lock hash or semantic check fails, Agent Lowmem releases the lock and returns 75 without launching the package manager. `doctor`, dry-run commands, and `restore` do not acquire the heavy-operation lock.

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

`reason` is a closed, ASCII, kebab-case v1 vocabulary. Each reason is valid only with the following origin and code class:

| Origin and code | Permitted `reason` values |
| --- | --- |
| `child`, `0` | `completed` |
| `preflight`, `2` | `invalid-cli`, `invalid-config` |
| `preflight`, `64` | `host-unsupported`, `repository-unsupported`, `package-manager-unsupported`, `workspace-unsupported`, `workspace-cardinality`, `operation-unsupported`, `script-syntax-unsupported`, `script-shell-unsupported`, `script-reference-unsupported`, `script-graph-too-large`, `wrapper-unsupported`, `tool-unsupported`, `tool-version-unsupported`, `watch-denied`, `ui-denied`, `background-denied`, `parallel-denied`, `argument-denied`, `nonfinal-injection-required` |
| `preflight`, `73` | `lock-held`, `nested-invocation` |
| `preflight`, `75` | `evidence-changed` |
| `preflight`, `78` | `managed-file-conflict` |
| `child`, nonzero normal child code | `child-exit` |
| `child`, `128 + signal` from a natural child signal | `child-signal` |
| `supervisor-timeout`, `124` | `deadline-exceeded` |
| `external-signal`, `128 + signal` | `external-signal` |
| `internal`, `70` | `internal-error` |

The committed `schemas/result-v1.schema.json` uses this exact enum:

```json
{
  "reason": {
    "type": "string",
    "enum": [
      "completed",
      "invalid-cli",
      "invalid-config",
      "host-unsupported",
      "repository-unsupported",
      "package-manager-unsupported",
      "workspace-unsupported",
      "workspace-cardinality",
      "operation-unsupported",
      "script-syntax-unsupported",
      "script-shell-unsupported",
      "script-reference-unsupported",
      "script-graph-too-large",
      "wrapper-unsupported",
      "tool-unsupported",
      "tool-version-unsupported",
      "watch-denied",
      "ui-denied",
      "background-denied",
      "parallel-denied",
      "argument-denied",
      "nonfinal-injection-required",
      "lock-held",
      "nested-invocation",
      "evidence-changed",
      "managed-file-conflict",
      "child-exit",
      "child-signal",
      "deadline-exceeded",
      "external-signal",
      "internal-error"
    ]
  }
}
```

No runtime branch may emit a value outside this enum. Adding, removing, or renaming a reason requires a new result-schema version. Human-readable `message` and `nextAction` fields may improve without a schema-version change and must never be parsed as stable agent contracts.

## 12. Security and privacy

Agent Lowmem itself is local-only and sends no telemetry. The trusted repository command it launches remains outside that guarantee and may perform tool-defined network activity.

It must:

- compile the first-party production package with `#![forbid(unsafe_code)]`;
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
- supported-runtime and performance-validation results with capability reasons;
- repository-evidence hashes rather than absolute paths;
- selected workspace key and package name, tool versions, and adapter classifications;
- graph depth, total leaf occurrences, and potential-lifecycle labels;
- applied script-shell, serial, and no-watch controls;
- disclosed internal fan-out;
- lock, timeout, child, cleanup, exit-origin, and closed-enum exit-reason data.

It does not include a green, amber, or red health field, a pressure level, a synthetic memory budget, a sampled aggregate footprint, environment values, usernames, raw home paths, or unrelated process information.

This redaction guarantee does not apply to inherited child output, which may contain project paths, stack traces, or tool-defined diagnostics.

## 14. Testing strategy

Repository validation runs sequentially. Formatting, linting, unit tests, integration tests, security checks, and release builds are separate commands; tests use one worker.

### Unit tests

- supported macOS arm64 capability checks and exact reference-profile matching;
- unvalidated-performance notice behavior on a supported non-reference host;
- package-manager, exact workspace-cardinality, tool-version, and script-form inspection;
- data-only repository configuration parsing, custom script-shell rejection, and command-scoped launch controls;
- proof through executable sentinels that inspection and post-lock evidence recheck start no child process;
- conservative potential-lifecycle classification independent of lifecycle-enable configuration;
- tokenizer quoting, `&&` segmentation, literal quoted globs, and every rejected construct from §8.4.2;
- `cross-env` and `dotenv` unwrapping plus redaction;
- same-package script-reference expansion, depth-three and 32-segment limits, occurrence counting, and cycle detection;
- lifecycle re-inspection and post-lock evidence-change handling;
- adapter-matrix schema, exact version matching, classifications, suffix placement, and forwarded arguments;
- watch, UI, background, parallel, race, and unsupported-orchestrator rejection;
- non-final or nested required-injection rejection;
- proof that inherited `NODE_OPTIONS` is unchanged;
- launch-plan explanations and fan-out classification;
- exact-byte managed-block insertion, replacement, conflict, and forced removal;
- live and stale lock decisions;
- signal, timeout, nested invocation, and panic cleanup state machines;
- stable result-line, redaction, closed-reason enum, origin/code compatibility, and structured-output schema behavior;
- compile-time first-party `unsafe` prohibition.

### Integration tests

- npm, pnpm, single-package, and monorepo fixtures;
- exact supported and unsupported npm/pnpm version fixtures, including launch-array option spelling, ordering, forwarding, and script-shell precedence;
- hostile machine/user script-shell configuration overridden by command-scoped launch flags, while an explicit incompatible repository configuration is rejected before launch;
- `doctor`, `init --dry-run`, `init`, pre-lock planning, and post-lock recheck completing with sentinel `node`, npm, pnpm, Git, and package executables that fail if started;
- exact npm workspace and pnpm filter selection with one matching package name;
- zero-match, duplicate-name, name/path mismatch, selector-operator, and post-lock workspace-drift fixtures rejected before child launch;
- pnpm `--fail-if-no-match` proving that an independently stale zero-match selection cannot exit successfully;
- `rimraf dist && next build` accepted as auxiliary plus final disclosed segments;
- `cross-env NODE_ENV=test vitest run` accepted with policy flags reaching the final Vitest command;
- `dotenv -e .env.test -- vitest run` accepted without logging the dotenv path;
- quoted ESLint glob arguments accepted while equivalent unquoted shell globs are rejected;
- multiple disclosed `&&` segments accepted in original order;
- a controlled non-final segment accepted only when its controls are already present;
- zero controlled/disclosed target segments rejected for a selected operation;
- exact `node --run`, `npm run`, and `pnpm run` references accepted only for cycle-free same-package graphs with depth at most three and at most 32 total leaf occurrences;
- a 32-segment graph accepted, the attempted thirty-third segment rejected with `script-graph-too-large`, and repeated references charged on every occurrence;
- `npm-run-all`, `run-s`, placeholders, script globs, and recursion cycles rejected as unsupported; `run-p`, `npm-run-all -p`, `concurrently`, and Turbo/Nx fan-out rejected as parallel;
- pipe, logical-or, lone ampersand, semicolon, substitution, redirection, grouping, comment, newline, and malformed-quote fixtures rejected;
- forwarded denial tokens rejected and forwarded paths attached only to the final adapter recipient;
- a tested direct TypeScript compilation accepted while `tsc --watch` and `tsc -w` are denied;
- declared pre/post phases classified under both lifecycle-enabled and lifecycle-disabled machine configurations, with safe drift accepted and unsafe drift rejected;
- a manifest change between planning and lock recheck returning 75 with no child;
- supported and unsupported Vitest, Jest, Node test, TypeScript, and ESLint version fixtures;
- Next.js and NestJS disclosure behavior without false single-worker claims;
- nested invocation and two-process lock contention;
- inherited TTY stdio without captured-pipe deadlock;
- exact environment preservation;
- `SIGINT`, `SIGTERM`, and `SIGHUP` forwarding;
- timeout escalation from `SIGTERM` to owned-group `SIGKILL`;
- an escaped-process fixture proving that output states the ownership limitation rather than claiming cleanup;
- normal child exit, child signal, timeout, external signal, and internal failure disambiguation;
- every terminal path mapped to one enum value accepted by `schemas/result-v1.schema.json`, with unknown reasons and invalid origin/code combinations rejected;
- atomic JSON output separate from child streams;
- interrupted file writes, marker upgrades, fresh-clone restore, conflicts, and forced-block restore;
- portable npm installation with platform-specific optional dependencies.

### End-to-end tests

The reference `Mac14,15` M2 8 GiB host validates `doctor`, `init --dry-run`, `init`, one focused test, one typecheck, one small compound build, Ctrl-C cleanup, timeout cleanup, structured output, and restore. A separate macOS arm64 fixture injects a non-reference profile and proves that `runSupported` remains true while `performanceValidated` is false and the notice is emitted once.

Tests never intentionally exhaust memory. The Swift pressure campaign is a separate observational experiment and is not part of the product test suite.

### Resource budgets

On the reference host, the release build must satisfy:

- parent-process peak resident memory at or below 24 MiB for `doctor` and `run` supervision;
- stripped `aarch64-apple-darwin` binary at or below 12 MiB;
- npm-launcher plus native-process aggregate peak resident memory at or below 80 MiB before the repository child starts;
- zero child-process starts during `doctor`, `init`, `init --dry-run`, launch planning, and post-lock evidence recheck;
- median `doctor` time at or below 100 ms outside a repository over 20 warm-cache runs;
- median `doctor` time at or below 300 ms and p95 at or below 500 ms in a committed single-package reference fixture over 20 warm-cache runs;
- at most 1,800 steady-state child/deadline checks while supervising `/bin/sleep 1800`;
- no more than 2 seconds of parent CPU time while supervising `/bin/sleep 1800`;
- no daemon, probe, lock owner, or member of the original owned process group remaining after normal completion, timeout cleanup, or handled external interruption.

Cold-cache repository discovery is measured and published but is not compared with the warm-cache gate. Resource measurements run on AC power with the macOS version, fixture commit, toolchain, and measurement command recorded.

## 15. Acceptance criteria

Version 1 is ready only when:

1. `doctor` reports runtime support on capability-compatible macOS 13-or-later arm64 hosts and independently matches only the exact `Mac14,15`, `Apple M2`, 8 GiB, 16 KiB-page, macOS 26 reference profile as performance-validated; `run` emits the notice once on every other supported profile.
2. Production code neither reads `kern.memorystatus_vm_pressure_level` nor claims current memory health.
3. Production code never adds, removes, parses for enforcement, or rewrites a Node heap limit.
4. `init --dry-run` displays exact changes, compatible candidate scripts, and rejection reasons without silently aliasing a script; repeated `init` is byte-for-byte idempotent.
5. A formatter-modified managed block conflicts normally and can be removed only through the narrow forced-block path without altering surrounding text.
6. Configuration maps valid operation labels only to exact present scripts; each workspace key maps to a canonical path and exact package name with cardinality one, no free-form package-manager selector is accepted, and pnpm workspace launches also fail on no match.
7. The tokenizer accepts the literal and quoted grammar plus sequential `&&`, rejects every construct in §8.4.2, and never evaluates or reconstructs the script.
8. `doctor`, `init`, `init --dry-run`, pre-lock planning, and post-lock evidence recheck start no child process; only `run` after a successful locked recheck may start npm or pnpm.
9. An explicit incompatible repository script shell or pnpm shell emulator returns 64 before launch; each managed launch pins `/bin/sh`, disables the pnpm shell emulator, does not read user/global configuration, and classifies every declared pre/post phase regardless of lifecycle-enable settings.
10. Tested `cross-env` and `dotenv-cli` forms unwrap for classification without exposing assignments or paths; malformed forms and `cross-env-shell` return 64.
11. Exact `node --run`, `npm run`, and `pnpm run` references run only when their same-package graph is cycle-free, no deeper than three references, contains at most 32 leaf occurrences, and is already controlled; sequential orchestrators are unsupported and parallel or race orchestrators are denied.
12. Missing policy or user arguments reach only an eligible final top-level adapter leaf; a need to modify an earlier, lifecycle, or nested leaf returns 64 with `nonfinal-injection-required`.
13. `adapters/matrix-v1.json` is schema-valid, embedded, backed by exact package-manager and tool-version fixtures before any range is widened, and is the sole adapter-policy source.
14. A relevant evidence-file, workspace-cardinality, or classification change after lock acquisition returns 75, releases the lock, and starts no child.
15. Supported Vitest, Jest, Node test-runner, TypeScript, and ESLint versions run in their matrix-proven non-watch, low-concurrency form; unknown versions return 64.
16. Next.js and NestJS never receive a false single-worker claim and display CI guidance when internal fan-out is uncontrolled.
17. The child receives inherited `NODE_OPTIONS` unchanged.
18. Two managed heavy operations cannot run concurrently across repositories and nested invocation returns 73.
19. The steady-state supervisor wakes no more than once per second, emits the 80% warning once within one second, and returns 124 after signaling only the owned process group at the deadline.
20. External signals are recognized independently of the one-second deadline tick, forwarded to the owned group, and re-raised by the parent after cleanup.
21. A normal child failure preserves its output and exact code; every result uses a reason from the closed v1 enum, satisfies the origin/code mapping, and validates against `schemas/result-v1.schema.json`.
22. An escaped descendant is never claimed as cleaned up or targeted through process-name scanning.
23. `restore` works when the executable can run even if host inspection marks `init` and `run` unsupported, preserves unrelated content, and implements ordinary and forced-block behavior exactly.
24. All first-party production Rust code rejects `unsafe`, Agent Lowmem itself makes no network request, and no production package contains the Swift probe.
25. Unit, integration, end-to-end, dependency-policy, and release checks pass sequentially.
26. Native and npm-launcher resource budgets pass on the recorded reference fixture and host.
27. Homebrew, GitHub Release, and the macOS npm platform package contain the same signed native binary; portable npm installation remains non-failing on unsupported platforms.
28. The project license is explicitly selected, recorded in package metadata, and published with the release artifacts.

## 16. Distribution and compatibility

Version 1 is a Rust binary for `aarch64-apple-darwin` with a macOS 13.0 deployment target. The project license must be selected and recorded before any public release or package publication; no prerelease artifact may claim a project license before that decision. Homebrew, GitHub Release, and Cargo installation execute the production CLI without Node.js or Swift. The project-local npm route uses Node.js only for a minimal portable launcher. Runtime compatibility and reference-profile performance validation remain separate claims.

The primary installation path is:

```text
brew install pleo2/tap/agent-lowmem
agent-lowmem init --dry-run
agent-lowmem init
```

GitHub Releases publish a notarized archive. The fixed release order is compile, test, strip, Developer ID sign, signature verification, notarization, checksum, and provenance. Nothing mutates the binary after signing.

The public-release presentation gate also requires:

- an evidence-backed benchmark table from the exact reference host, with its methodology and limitations;
- release notes, checksums, provenance, and installation instructions that resolve to the same signed binary;
- a social preview for `agentlowmem.dev` that does not add a browser runtime request;
- landing-page terminal output updated from the real release CLI rather than an illustrative contract;
- live `docs` and `benchmarks` routes only when their content exists, with `blog` remaining optional;
- automated landing checks that reject production JavaScript or external runtime dependencies and enforce its compressed-size and accessibility budgets.

The portable `agent-lowmem` npm root package has no `os` or `cpu` restriction and declares platform packages such as `@agent-lowmem/darwin-arm64` as optional dependencies. The platform package contains the same signed binary and declares its operating-system and CPU restriction.

The launcher resolves the installed platform package, starts its binary with inherited stdio, and returns the child status. It never downloads a binary, runs a lifecycle hook, or builds native code during installation. Unsupported platforms install successfully and receive a clear error only when execution is attempted.

## 17. Documentation deliverables

The first release includes:

- a concise README centered on avoiding unnecessary concurrency on an 8 GiB Mac;
- installation and five-minute quick start;
- the supported-runtime contract, exact performance-reference key, and unvalidated-performance notice;
- the zero-child-process inspection contract and command-scoped npm/pnpm script-shell controls;
- the accepted script grammar, final-segment injection limit, transparent wrappers, and bounded same-package reference constraints;
- exact workspace path/name cardinality and pnpm no-match behavior;
- generated `AGENTS.md` policy example;
- supported-tool/version matrix and exact adapter flags;
- the closed result-reason vocabulary and versioned JSON schema;
- focused-first validation examples;
- Next.js and NestJS internal-fan-out limitations and CI guidance;
- explicit statements that v1 has no heap cap, pressure kill, or responsiveness guarantee;
- timeout, recursion depth, graph size, lock, lifecycle-drift, workspace-cardinality, forced-restore, and escaped-process troubleshooting;
- explanation that package scripts remain trusted code and direct package-manager commands bypass enforcement;
- npm platform-support and launcher-overhead documentation;
- security, privacy, and low-resource contribution guidance;
- website copy for `agentlowmem.dev` using the approved name and tagline.

## 18. Pressure research and deferred roadmap

The observational Swift probe and its repository-local protocol at `docs/experiments/2026-09-02-pressure-signal-protocol.md` remain useful, but they no longer block the deterministic v1 design. Their purpose is to decide whether a later design may promote public Dispatch pressure events from research evidence into production behavior. The protocol is internal evidence, not an externally resolvable authoritative reference.

The pressure experiment must complete its documented baseline, workload, timing, and information-sufficiency gates. Raw traces remain local and ignored. Only an aggregate report may be committed after privacy review.

A future pressure feature requires all of the following:

1. prospective evidence on the exact performance-reference host and macOS build;
2. a stated Outcome A, B, or C from the experiment protocol;
3. no private pressure sysctl in the production contract;
4. a new spec revision defining whether events are informational or enforcing;
5. a narrow audited Rust macOS integration and new resource budgets;
6. separate exit-origin and cleanup tests.

Other deferred work includes:

- `npm-run-all`, `run-s`, and other sequential orchestration syntax after real repository demand justifies the larger parser and graph surface;
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

## 19. Revision decision record

### Revision 4 — evidence-based enforcement

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

### Revision 5 — practical script compatibility

| Review issue or proposal | Revision 5 decision |
| --- | --- |
| Direct-command-only parsing rejected common repository scripts | Accept a deliberately small literal grammar with sequential `&&`, transparent wrappers, and bounded same-package script references |
| `&&` was described as having no shared state | Preserve the original shell execution and state exactly; classification is per segment, but filesystem effects and short-circuiting remain shared |
| Adapter flags were proposed for an arbitrary target segment | npm and pnpm append selected-script arguments at the end, so injection is allowed only into an eligible final top-level leaf; other missing controls return `nonfinal-injection-required` |
| Exactly one controlled or disclosed segment was proposed | Permit multiple sequential targets when every segment is safe; the selected script must contain at least one non-auxiliary target |
| `cross-env` and `dotenv` are common wrappers | Unwrap one tested literal wrapper for classification while running the original script and redacting assignments and paths |
| `npm-run-all -s` was described as transparent | Treat it as a bounded recursive script-reference graph; accept only exact names whose complete graph already satisfies policy |
| A broad `inert` utility list was proposed | Use the narrower name `auxiliary` and matrix-reviewed argument schemas; v1 starts with static repository-relative `rimraf` paths rather than trusting general `rm`, `cp`, or `mv` forms |
| Broad guessed adapter version ranges were proposed | Begin with exact fixture-tested versions and widen a range only with the same fixture evidence |
| The pitch still emphasized unproven pressure control | Lead with the enforceable cross-repository lock and owned process-group cleanup contract |
| Exact `Mac14,15` matching unnecessarily blocked deterministic controls | Support capability-compatible macOS arm64 and keep exact host matching only for performance claims and resource budgets |
| Four supervisor wakeups per second had no value after pressure polling was removed | Limit steady-state deadline and child-status work to one wakeup per second and specify deadline tolerance |
| A second consecutive evidence-change note implied hidden cross-run state | Keep v1 stateless: return one deterministic `evidence-changed` result with no automatic retry or attribution |

### Revision 6 — bounded implementation contract

Revision 6 supersedes any conflicting Revision 5 decision while preserving Revision 5 as review history.

| Final review issue or simplification | Revision 6 decision |
| --- | --- |
| Querying npm/pnpm configuration could consume most or all of the `doctor` latency budget | Inspection and post-lock recheck start zero children; parse only matrix-declared repository configuration as data, reject incompatible repository shell semantics, and pin safe shell behavior with fixture-tested command-scoped launch options |
| Lifecycle-enable configuration would otherwise require another precedence engine | Always classify declared pre/post scripts as potential phases and never force enablement; a package-manager policy may only omit preclassified work |
| Reference depth alone did not bound total graph expansion | Retain depth three and cycle detection, then add a fixed 32-leaf-occurrence cap across the selected script, potential lifecycle phases, and all references |
| `reason` values were scattered prose rather than an agent contract | Define one closed origin/code-compatible enum and require the same enum in `schemas/result-v1.schema.json`; vocabulary changes require a new schema version |
| A free-form pnpm filter could match zero or multiple packages while returning misleading success | Replace `packageManagerSelector` with exact `packageName`, require one path/name match before launch, reject selector operators, recheck cardinality under the lock, and add pnpm `--fail-if-no-match` as defense in depth |
| `npm-run-all` created the largest parser surface in the initial release | Defer `npm-run-all`, `run-s`, and similar orchestrators; v1 expands only exact fixture-tested `node --run`, `npm run`, and `pnpm run` references |
| The experiment used a repository-relative link that could not resolve from an external copy of the spec | Identify the protocol as repository-local evidence in §18 and keep §21 limited to externally resolvable primary references |

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
- [Rust `Command`](https://doc.rust-lang.org/std/process/struct.Command.html) for child execution and inherited stdio.
- [Rust Unix `CommandExt`](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html) for Unix child-process configuration.
- [Node.js command-line options](https://nodejs.org/api/cli.html#node_optionsoptions) for inherited `NODE_OPTIONS` behavior.
- [npm `run`](https://docs.npmjs.com/cli/v11/commands/npm-run/) for selected-script argument forwarding, pre/post exclusion, and script execution.
- [npm configuration](https://docs.npmjs.com/using-npm/config/) for command-line precedence, default `/bin/sh`, and `script-shell` configuration.
- [npm scripts](https://docs.npmjs.com/cli/using-npm/scripts) for package-manager lifecycle semantics.
- [pnpm `run`](https://pnpm.io/cli/run) for script forwarding and lifecycle behavior.
- [pnpm settings](https://pnpm.io/settings) for supported configuration sources and command-line precedence.
- [pnpm other settings](https://pnpm.io/settings/other) for `scriptShell`, `shellEmulator`, and lifecycle settings.
- [pnpm workspace filtering](https://pnpm.io/workspaces#failifnomatch) for `failIfNoMatch` behavior.
- [`cross-env` README](https://github.com/kentcdodds/cross-env/blob/main/README.md) for its single-command execution and the separate shell-enabled bin.
- [`dotenv-cli` README](https://github.com/entropitor/dotenv-cli) for repeated `-e` files and the `--` command boundary.
- [Vitest CLI](https://vitest.dev/guide/cli) and [`fileParallelism`](https://vitest.dev/config/fileparallelism) for public run, watch, file-parallelism, and worker controls.
- [Jest CLI](https://jestjs.io/docs/cli) for public `--runInBand` and watch controls.
- [Node.js `--test-concurrency`](https://nodejs.org/api/cli.html#--test-concurrencyconcurrency) for test-runner file concurrency.
- [TypeScript `tsc` CLI options](https://www.typescriptlang.org/docs/handbook/compiler-options.html) for project, build, no-emit, and watch forms.
- [ESLint `--concurrency`](https://eslint.org/docs/latest/use/command-line-interface#--concurrency) for its documented threading control and default.
- [Apple notarization workflow](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) for direct-download release handling.
