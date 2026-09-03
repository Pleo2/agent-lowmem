# Agent Lowmem Phase 2 Repository Policy Design

**Status:** Approved for implementation planning

**Date:** 2026-09-03

**Parent specification:** `docs/superpowers/specs/2026-09-02-agent-lowmem-v1-design.md`, Revision 6

**Scope:** Repository evidence, configuration, script classification, and launch-policy construction. Process execution remains Phase 3.

## 1. Outcome

Phase 2 turns the Phase 1 repository probe into a deterministic, data-only policy engine for Git-backed JavaScript and TypeScript repositories. Given a repository root and an optional committed `.agent-lowmem.json`, it identifies one root package or one configured workspace, validates an operation, classifies every potentially reachable script segment, and returns either a launch policy or one closed v1 rejection reason.

The phase does not enable `agent-lowmem run`, acquire a global operation lock, execute npm or pnpm, write managed files, or publish a release. The CLI continues to expose only `doctor`; its human and JSON reports gain repository-policy evidence and compatible operation guidance.

## 2. Approved decomposition

Implementation is sequential and divided into three independently reviewable blocks:

1. configuration and exact workspace identity;
2. tokenizer, wrappers, and bounded same-package script graph;
3. embedded adapter matrix and launch-policy construction.

Each block has its own RED-GREEN test cycle, focused fixtures, complete single-worker regression gate, and Conventional Commit. Later blocks consume typed outputs from earlier blocks rather than reparsing files or duplicating policy.

## 3. Invariants

Phase 2 preserves these non-negotiable constraints from Revision 6:

- inspection starts no child process and never evaluates repository code;
- inspection makes no network request and reads no user or global package-manager configuration;
- repository scripts are parsed for policy but never printed, persisted in results, rewritten, or passed through a shell by Agent Lowmem;
- absolute repository paths, usernames, environment values, assignments, and dotenv paths never enter structured output;
- `NODE_OPTIONS` is neither read for policy nor changed;
- no pressure signal, heap limit, daemon, async runtime, or process-table polling enters production code;
- first-party Rust remains `#![forbid(unsafe_code)]`;
- every compiling Cargo command uses `-j 1`, and tests use `--test-threads=1`;
- every new direct dependency requires a separate documented purpose, API/source review, license review, and lockfile commit.

## 4. Component boundaries

### 4.1 Repository evidence

`src/repository.rs` remains responsible for finding the Git root, reading the root `package.json`, and matching npm or pnpm with exactly one corresponding lockfile. It delegates richer interpretation to focused modules and returns repository-relative identities only.

The inspector resolves tool and wrapper package manifests only through lexical package resolution rooted in the selected package directory and Git root. It follows no symlink outside the canonical Git root. Missing, ambiguous, escaped, malformed, or unsupported evidence produces a typed rejection and never falls back to executing Node resolution.

### 4.2 Configuration

`src/configuration.rs` owns the strict `.agent-lowmem.json` data model and semantic validation. JSON decoding rejects unknown fields. The accepted top-level fields are exactly `$schema`, `version`, `packageManager`, `operations`, and `workspaces`.

The fixed rules are:

- `version` equals `1`;
- `$schema`, when present, equals `https://agentlowmem.dev/schema/v1.json`;
- `packageManager` equals the manager proven by `package.json` and its lockfile;
- operation keys match `[a-z][a-z0-9-]{0,31}`;
- an operation contains only `script` and `timeoutSeconds`;
- the script is one exact non-empty key from the selected package's `scripts` object;
- timeouts are integers from 60 through 3,600 seconds;
- workspace keys obey the operation-key grammar and are unique by JSON-object construction;
- workspace entries contain only `path`, `packageName`, and `operations`;
- workspace paths are normalized repository-relative directory paths with no empty, `.`, or `..` component and no symlink escape;
- package names follow the supported npm name grammar and contain none of `...`, `^`, `*`, `!`, `[`, `]`, `{`, or `}`;
- configured workspace package names are unique.

Schema errors return `invalid-config` with code 2. Repository-semantic incompatibilities use their specific code-64 reason, including `workspace-unsupported`, `workspace-cardinality`, and `operation-unsupported`.

`schemas/agent-lowmem-v1.schema.json` documents the structural contract. Rust semantic validation remains authoritative for filesystem identity, scripts, and cardinality.

### 4.3 Workspace declarations

`src/workspace.rs` maps supported declarations to exact canonical package identities. The first implementation deliberately supports a narrow, common subset:

- npm `workspaces` as an array of strings or an object containing only a `packages` array;
- pnpm `pnpm-workspace.yaml` with one top-level `packages` sequence of scalar strings;
- declaration patterns made of safe repository-relative path segments, where `*` is allowed only as an entire segment;
- literal paths and single-segment wildcard expansion through sorted directory reads;
- exclusion patterns, `**`, partial-segment wildcards, brace expansion, YAML aliases, anchors, tags, flow collections, multiline scalars, additional pnpm workspace keys, and all unknown syntax are unsupported.

The pnpm parser is a purpose-built parser for this declared subset, not a general YAML parser. It accepts UTF-8, blank lines, comments, and consistently indented dash entries below `packages:`. Single- and double-quoted entries use the same non-interpolating literal restrictions defined by this design. A comment marker inside a quoted scalar remains literal; an unquoted comment begins at `#`.

Candidate directories are canonicalized, must remain inside the Git root, may not be symlinks, and must contain a valid `package.json` with an exact `name`. Expansion order is lexicographically sorted by repository-relative path so reports and tests are deterministic.

A configured workspace succeeds only when its normalized path and exact package name identify one and the same expanded candidate. Zero matches, duplicate package names, duplicate canonical paths, or path/name disagreement returns `workspace-cardinality`. Unsupported declaration syntax returns `workspace-unsupported`.

### 4.4 Package-manager semantics

`src/package_manager.rs` owns matrix-declared npm/pnpm repository configuration and package-manager argument-array construction. It reads only files inside the Git root. For the initial adapters this means the root `.npmrc` and `pnpm-workspace.yaml`; it never queries npm/pnpm and never reads machine, user, global, or environment configuration.

The `.npmrc` reader recognizes UTF-8 line-oriented `key=value` entries, blank lines, and full-line comments. It compares normalized relevant keys case-sensitively with the matrix declaration. A relevant value containing interpolation or substitution is unsupported. An explicit script shell other than `/bin/sh`, or an enabled pnpm shell emulator, returns `script-shell-unsupported`. Unrelated well-formed keys are ignored and never copied into output.

Argument arrays follow the four templates in Revision 6 §8.4.6. They always pin `/bin/sh`; pnpm also disables its shell emulator, uses the exact validated workspace package name, and adds `--fail-if-no-match`. Phase 2 builds and tests these arrays as immutable data but does not execute them.

### 4.5 Script tokenizer

`src/script/tokenizer.rs` implements a finite-state policy tokenizer, not a shell parser. Its output preserves decoded tokens and segment order for classification while retaining only repository-relative script-key identities for diagnostics. It never reconstructs an executable string.

The accepted and rejected grammar is exactly Revision 6 §8.4.2. `&&` is the only separator. Empty segments, unsupported operators, interpolation, substitution, redirection, grouping, comments, newlines, unquoted globs, invalid escapes, adjacent quoted/unquoted fragments, and malformed quotes return `script-syntax-unsupported`.

Tokenizer tests are table-driven and include every accepted token class and every rejected construct listed by the parent specification. Fuzzing is not required for the Phase 2 exit gate, but the tokenizer API accepts byte slices internally so later fuzz coverage does not require a public-interface change.

### 4.6 Wrappers and script graph

`src/script/graph.rs` resolves the selected script, its declared pre/post lifecycle phases, and exact same-package references. It owns all recursion state and emits an ordered graph of leaf occurrences.

The fixed bounds are:

- selected target at depth zero;
- maximum reference depth three;
- cycle-free active expansion stack;
- maximum 32 leaf occurrences across the target, potential lifecycle phases, and expanded references;
- occurrences, rather than unique script names, consume the segment budget.

Only exact `node --run <script>`, `npm run <script>`, and `pnpm run <script>` reference forms are expanded. Flags, extra arguments, `--`, workspace selection, globs, missing keys, and cycles return `script-reference-unsupported`. Attempting to admit leaf 33 returns `script-graph-too-large`.

`src/script/wrapper.rs` recognizes only matrix-approved versions and exact forms of `cross-env` and `dotenv-cli`. It returns redacted wrapper evidence: wrapper kind and consumed assignment/file count. It never exposes assignment keys or values or dotenv paths. `cross-env-shell`, malformed assignments, absolute or escaping dotenv paths, unsupported options, nested wrappers, and missing wrapped commands return `wrapper-unsupported` or the more specific denial supplied by the matrix.

### 4.7 Adapter matrix

`adapters/matrix-v1.json` is the sole policy source for supported package-manager, tool, wrapper, and auxiliary-command forms. `schemas/adapter-matrix-v1.schema.json` validates its structure, and `src/adapter.rs` embeds the artifact with `include_str!` and parses it once per inspection without global mutable caches.

The initial matrix contains only exact versions represented by committed fixtures. Each admitted version records:

- package identity and executable names;
- command/subcommand form;
- controlled, disclosed, auxiliary, denied, or unsupported classification;
- existing-control recognition;
- optional final-segment suffix;
- denial and conflicting-argument tokens;
- forwarded-argument rules;
- package-manager launch-array template and command-scoped script-shell controls;
- repository configuration files and keys that can change script semantics;
- stable disclosure identifiers.

Version ranges are forbidden in the initial artifact. Adding a new exact version requires its fixture suite in the same commit. Unknown versions return `tool-version-unsupported` or `package-manager-unsupported`; no nearest-version fallback exists.

Phase 2 covers exact fixture-tested forms of npm, pnpm, Vitest, Jest, Node's test runner, TypeScript `tsc`, ESLint, Next.js, NestJS, `cross-env`, `dotenv-cli`, and `rimraf`. Next.js and NestJS are disclosed as `internal-fanout-uncontrolled`, never described as single-worker. The accepted `rimraf` form contains one or more static repository-relative paths, no options, glob, or `..` component.

### 4.8 Policy construction

`src/policy.rs` combines configuration, workspace identity, script graph, installed package evidence, and adapter classifications into one immutable `OperationPolicy` for Phase 3.

An operation policy contains:

- selected package manager and exact version;
- root or exact workspace identity without an absolute path;
- configured operation label, exact script key, and timeout;
- ordered potential lifecycle and target leaf occurrences;
- redacted wrapper/reference evidence;
- each leaf classification and stable explanation identifier;
- already-present controls and the optional final-leaf suffix;
- the complete package-manager argument array with the child executable represented separately;
- repository-relative evidence-file identities for later hashing;
- disclosures and one final runnable/rejected decision.

No environment values or original raw script bodies are retained. The package-manager argument array is data and is not executable in Phase 2.

A policy is runnable only when every reachable leaf is controlled, disclosed, or auxiliary and the selected target contains at least one controlled or disclosed leaf. Missing controls may be injected only into the eligible final top-level adapter leaf. A lifecycle, nested, non-final, reference, or otherwise ineligible required injection returns `nonfinal-injection-required`.

Forwarded user arguments are modeled and validated by the policy API for Phase 3 but are not exposed through the Phase 2 CLI. Denial tokens map to `watch-denied`, `ui-denied`, `background-denied`, `parallel-denied`, or `argument-denied`.

### 4.9 Doctor integration

`src/doctor.rs` reports the richer evidence without making `init` or `run` available. Human output lists supported root/workspace operation labels, classifications, disclosures, and the next safe action. JSON output uses stable enums and repository-relative keys; it contains no raw script, absolute path, environment data, assignment, or dotenv path.

When `.agent-lowmem.json` is absent, `doctor` may classify canonical root scripts named `test`, `typecheck`, `lint`, and `build` as compatible candidates. It does not silently create mappings. Candidate analysis follows the same grammar and matrix rules as configured operations.

The CLI parser continues to return `operation-unsupported` for `run` throughout Phase 2. `init` and `restore` remain unavailable until Phase 4.

## 5. Data flow

```text
Git root and root package evidence
  -> package-manager and workspace declaration parsing
  -> strict optional configuration parsing
  -> exact root/workspace selection
  -> target plus potential lifecycle script collection
  -> tokenizer and bounded reference expansion
  -> wrapper unwrapping and installed-package evidence
  -> embedded adapter classification
  -> immutable operation policy or closed rejection reason
  -> redacted doctor presentation
```

Every arrow is an in-process, read-only transformation. No Phase 2 component owns a command runner, lock, signal handler, timeout loop, or managed-file writer.

## 6. Errors and privacy

All failures reuse the closed 31-reason vocabulary in `src/result.rs` and `schemas/result-v1.schema.json`. Phase 2 adds no reason and changes no origin/code mapping.

Internal errors may preserve a repository-relative evidence class and stable diagnostic identifier. They must not contain raw scripts, absolute paths, environment values, assignment names or values, dotenv paths, arbitrary manifest fragments, or tool output. Tests assert redaction against sentinel secrets and home-directory paths.

Malformed or unsupported repository evidence fails closed. An unavailable package, ambiguous workspace, unknown exact version, unsupported syntax, or unclassifiable segment never becomes a permissive warning.

## 7. Testing strategy

### Unit tests

- strict configuration schema and semantic rules;
- npm and pnpm workspace subset parsing, deterministic expansion, canonical containment, and exact cardinality;
- every tokenizer acceptance and rejection rule;
- wrapper redaction and malformed-form rejection;
- lifecycle inclusion, reference depth, cycles, and 32-occurrence accounting;
- matrix schema, exact-version lookup, classifications, suffix eligibility, and denial mapping;
- policy completeness and absence of sensitive fields;
- enriched doctor human/JSON presentation.

### Integration fixtures

Committed temporary-repository templates cover npm/pnpm, root packages, monorepos, duplicate identities, mismatched paths/names, escaping symlinks, supported and unsupported workspace syntax, script-shell conflicts, exact tool versions, wrappers, compound scripts, lifecycle drift inputs, denial forms, and all adapter categories.

Sentinel executables named `git`, `node`, `npm`, `pnpm`, and supported package binaries fail the test if inspection starts them. Source guards prohibit `std::process::Command` and runner dependencies from first-party Phase 2 production modules.

### Resource gates

The complete suite runs sequentially. On the reference Mac, release `doctor` must preserve the 24 MiB parent RSS and 12 MiB binary limits. A committed single-package fixture is measured over 20 warm-cache runs with median at most 300 ms and p95 at most 500 ms. The outside-repository Phase 1 median remains at most 100 ms.

## 8. Files and ownership

Expected production additions:

- `src/configuration.rs`: strict committed configuration;
- `src/workspace.rs`: supported declaration parsing and exact identity;
- `src/package_manager.rs`: repository configuration and launch-array data;
- `src/script/mod.rs`: script-policy types and module boundary;
- `src/script/tokenizer.rs`: finite-state grammar;
- `src/script/graph.rs`: lifecycle and reference expansion;
- `src/script/wrapper.rs`: redacted transparent wrappers;
- `src/adapter.rs`: embedded matrix validation and lookup;
- `src/policy.rs`: immutable operation-policy construction;
- `schemas/agent-lowmem-v1.schema.json`: configuration structure;
- `schemas/adapter-matrix-v1.schema.json`: matrix structure;
- `adapters/matrix-v1.json`: exact fixture-backed policies;
- `tests/fixtures/repositories/`: data-only repository fixtures;
- `tests/repository_policy.rs`: executable-boundary, redaction, and integration coverage.

Expected modifications:

- `src/lib.rs`: export Phase 2 modules;
- `src/repository.rs`: delegate and aggregate richer evidence;
- `src/doctor.rs`: present compatible operations and rejections;
- `tests/doctor_cli.rs`: preserve zero-child and redaction contracts;
- `docs/dependencies-v1.md`: record any approved direct dependency and Phase 2 measurements.

Files may be split further only when a responsibility becomes too large to review; public interfaces and ownership boundaries remain those defined here.

## 9. Phase 2 exit gate

Phase 2 is complete only when:

1. all three approved blocks are committed on `main` with Conventional Commits;
2. the working tree is clean and `main` matches `origin/main`;
3. formatting, Clippy with warnings denied, unit tests, integration tests, release build, doctor timing, binary size, RSS, and diff checks pass sequentially;
4. inspection starts no child across doctor, configuration, workspace, tokenizer, graph, wrapper, matrix, and policy paths;
5. every Revision 6 repository-policy rejection maps to the existing closed result vocabulary;
6. npm/pnpm root and workspace fixtures prove exact cardinality and deterministic behavior;
7. the tokenizer and graph fixtures prove every grammar rule, depth three, cycle rejection, 32 accepted occurrences, and rejection of occurrence 33;
8. every admitted matrix version has fixture evidence and unknown versions fail closed;
9. doctor output remains free of absolute paths, raw scripts, environment values, assignments, and dotenv paths;
10. `run`, `init`, and `restore` remain unavailable and no release, tag, npm package, or Homebrew formula is published.

The saved next action after this gate is to create the Phase 3 managed-runner design and implementation plan from the verified `OperationPolicy` interface.

## 10. Deferred work

Phase 3 owns evidence hashing/recheck, the cross-repository user lock, package-manager process launch, process groups, signals, deadlines, cleanup, child exit preservation, and JSON result files.

Phase 4 owns deterministic `.agent-lowmem.json` and `AGENTS.md` generation, dry-run, restoration manifests, conflicts, and forced managed-block removal.

Phase 5 owns complete release documentation, CI and dependency policy, npm launcher, Homebrew, signing, notarization, provenance, website handoff, and the first public release.

Sequential orchestrators, broader workspace glob syntax, version ranges, pressure enforcement, heap policies, other operating systems, and non-JavaScript toolchains remain outside v1 or explicitly deferred by Revision 6.
