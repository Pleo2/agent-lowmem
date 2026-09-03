# Agent Lowmem Phase 2 Repository Policy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the data-only repository policy engine that validates Agent Lowmem configuration and workspaces, classifies bounded package scripts, and produces immutable launch-policy data without enabling process execution.

**Architecture:** Extend the Phase 1 inspector through focused Rust modules with typed boundaries: configuration and workspace identity first, script grammar and bounded expansion second, then an embedded exact-version adapter matrix and policy assembler. `doctor` consumes only redacted summaries; `run`, locks, hashing/recheck, signals, managed files, and distribution remain later phases.

**Tech Stack:** Rust 1.85.0, edition 2024, `serde`, `serde_json`, `semver`, safe `std::fs`; JSON artifacts and committed repository fixtures; no async runtime, shell parser, glob dependency, YAML dependency, or new production process API.

**Spec:** `docs/superpowers/specs/2026-09-03-agent-lowmem-phase-2-repository-policy-design.md`

## Global Constraints

- Work directly on `main` until the first release, as approved by the repository owner; use Conventional Commits and push each independently verified task atomically.
- Keep the working tree clean between tasks and preserve unrelated user changes if any appear.
- Rust 1.85.0 is pinned; every first-party production target uses edition 2024 and `#![forbid(unsafe_code)]`.
- Every Cargo command that may compile uses `-j 1`; tests use `--test-threads=1`; never run formatter, Clippy, tests, builds, or measurements concurrently.
- Production inspection starts no child, makes no network request, evaluates no repository code, and reads no user/global package-manager configuration.
- Do not read or mutate `NODE_OPTIONS`; do not add a heap cap, pressure enforcement, daemon, async runtime, global cache, or process-table polling.
- Never expose an absolute repository path, username, raw script, environment value, assignment, dotenv path, or arbitrary manifest fragment in human or JSON output.
- Reject unsupported syntax and versions explicitly; never approximate, execute to discover, or silently weaken policy.
- Do not add a direct dependency during this plan. If implementation evidence makes one unavoidable, stop that task and obtain design approval before changing `Cargo.toml`.
- `run`, `init`, and `restore` remain unavailable throughout Phase 2. Do not create a release, tag, npm package, Homebrew formula, or website deployment.
- The initial matrix snapshot is exact and dated 2026-09-03: npm `12.0.2`, pnpm `11.25.0`, Node `24.14.1`, Vitest `4.1.11`, Jest `30.5.1`, TypeScript `7.0.2`, ESLint `10.9.1`, Next.js `16.3.4`, `@nestjs/cli` `12.0.0`, `cross-env` `10.1.0`, `dotenv-cli` `11.0.0`, and `rimraf` `6.1.3`. Adding or changing a version is a separate fixture-backed review.
- Reuse the closed result vocabulary exactly: structural configuration failures use `invalid-config`; unsupported workspace syntax and identity use `workspace-unsupported` or `workspace-cardinality`; repository shell conflicts use `script-shell-unsupported`; tokenizer, references, leaf budget, and wrappers use `script-syntax-unsupported`, `script-reference-unsupported`, `script-graph-too-large`, and `wrapper-unsupported`; missing tools and exact-version misses use `tool-unsupported` and `tool-version-unsupported`; policy denials use `watch-denied`, `ui-denied`, `background-denied`, `parallel-denied`, `argument-denied`, or `nonfinal-injection-required`. Do not add a reason in Phase 2.

## File Structure

- `src/configuration.rs`: strict v1 configuration types, structural decoding, lexical validation, and semantic operation lookup.
- `src/workspace.rs`: supported npm/pnpm workspace declaration parsing, deterministic single-segment wildcard expansion, and exact path/name cardinality.
- `src/package_manager.rs`: repository-owned script-shell evidence, exact Node-version evidence, and non-executable npm/pnpm argument arrays.
- `src/script/mod.rs`: shared redacted script-policy types.
- `src/script/tokenizer.rs`: finite-state tokenizer for the Revision 6 grammar.
- `src/script/graph.rs`: potential lifecycle collection, same-package reference expansion, cycle/depth/leaf limits.
- `src/script/wrapper.rs`: exact transparent-wrapper recognition with redacted evidence.
- `src/adapter.rs`: embedded matrix parsing, exact lookup, classification, controls, suffixes, and denial tokens.
- `src/policy.rs`: immutable `OperationPolicy` assembly and final eligibility decisions.
- `src/repository.rs`: existing Git/package-manager discovery plus delegation to Phase 2 modules.
- `src/doctor.rs`: redacted compatible-operation summaries.
- `src/lib.rs`: module exports.
- `schemas/agent-lowmem-v1.schema.json`: committed configuration schema.
- `schemas/adapter-matrix-v1.schema.json`: committed matrix schema.
- `adapters/matrix-v1.json`: exact-version adapter policy.
- `tests/fixtures/repositories/`: deterministic npm/pnpm repository evidence.
- `tests/repository_policy.rs`: cross-module, zero-child, redaction, and executable-boundary tests.
- `tests/doctor_cli.rs`: extended human/JSON behavior.
- `tests/doctor_budget.rs`: root-repository warm-cache timing measurement.
- `docs/dependencies-v1.md`: Phase 2 evidence and unchanged dependency boundary.

---

### Task 1: Strict Configuration Contract

**Files:**
- Create: `src/configuration.rs`
- Create: `schemas/agent-lowmem-v1.schema.json`
- Modify: `src/lib.rs`
- Test: unit tests inside `src/configuration.rs`

**Interfaces:**
- Consumes: `PackageManagerKind` from `src/repository.rs` and `Reason` from `src/result.rs`.
- Produces: `AgentLowmemConfig`, `OperationConfig`, `WorkspaceConfig`, `ConfigError`, `parse_config(bytes: &[u8])`, and `select_operation(config, workspace_key, operation_key)`.

- [ ] **Step 1: Write failing structural-decoding tests**

Add tests that require the following public shapes and exact failures:

```rust
#[test]
fn parses_the_minimal_root_configuration() {
    let config = parse_config(br#"{
      "$schema":"https://agentlowmem.dev/schema/v1.json",
      "version":1,
      "packageManager":"pnpm",
      "operations":{"test":{"script":"test","timeoutSeconds":900}}
    }"#).unwrap();
    assert_eq!(config.version, 1);
    assert_eq!(config.package_manager, PackageManagerKind::Pnpm);
    assert_eq!(config.operations["test"].script, "test");
}

#[test]
fn rejects_unknown_fields_and_invalid_bounds() {
    for bytes in [
        br#"{"version":1,"packageManager":"npm","unknown":true}"#.as_slice(),
        br#"{"version":2,"packageManager":"npm"}"#.as_slice(),
        br#"{"version":1,"packageManager":"npm","operations":{"Test":{"script":"test","timeoutSeconds":59}}}"#.as_slice(),
        br#"{"version":1,"packageManager":"npm","operations":{"test":{"script":"test","timeoutSeconds":3601}}}"#.as_slice(),
    ] {
        assert_eq!(parse_config(bytes).unwrap_err().reason(), Reason::InvalidConfig);
    }
}
```

Also cover unknown nested fields, invalid `$schema`, missing required fields, empty scripts, workspace path components `.`/`..`, absolute paths, selector operators, invalid package names, duplicate configured package names, and operation/workspace keys longer than 32 bytes.

- [ ] **Step 2: Run the focused tests and verify RED**

```bash
cargo test -j 1 configuration::tests -- --test-threads=1
```

Expected: compilation fails because `configuration` and `parse_config` do not exist.

- [ ] **Step 3: Implement the strict data model and lexical validators**

Use `#[serde(deny_unknown_fields)]` on private raw types and expose:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLowmemConfig {
    pub version: u8,
    pub package_manager: PackageManagerKind,
    pub operations: BTreeMap<String, OperationConfig>,
    pub workspaces: BTreeMap<String, WorkspaceConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationConfig {
    pub script: String,
    pub timeout_seconds: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceConfig {
    pub path: String,
    pub package_name: String,
    pub operations: BTreeMap<String, OperationConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigError(Reason);

impl ConfigError {
    pub const fn reason(self) -> Reason;
}

pub fn parse_config(bytes: &[u8]) -> Result<AgentLowmemConfig, ConfigError>;
pub fn select_operation<'a>(
    config: &'a AgentLowmemConfig,
    workspace_key: Option<&str>,
    operation_key: &str,
) -> Result<&'a OperationConfig, ConfigError>;
```

Validate keys byte-by-byte without regex. Normalize workspace paths by splitting on `/`; reject leading `/`, empty components, `.`, `..`, backslash, NUL, and trailing slash. Accept one unscoped npm name or `@scope/name`, then reject every selector operator in the design.

- [ ] **Step 4: Add and cross-check the JSON schema**

Create a draft 2020-12 schema with `additionalProperties: false` at every object level, exact URL/version constants, key pattern `^[a-z][a-z0-9-]{0,31}$`, timeout bounds 60/3600, and required operation fields. Add a unit test loading it with `include_str!("../schemas/agent-lowmem-v1.schema.json")` and asserting Rust/schema constant parity.

- [ ] **Step 5: Run focused and complete gates**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 configuration::tests -- --test-threads=1
cargo test -j 1 -- --test-threads=1
git diff --check
```

- [ ] **Step 6: Commit and push the configuration contract**

```bash
git add src/configuration.rs src/lib.rs schemas/agent-lowmem-v1.schema.json
git commit -m "feat: add strict repository configuration"
git push origin main
```

### Task 2: Supported Workspace Declaration Parsing

**Files:**
- Create: `src/workspace.rs`
- Modify: `src/lib.rs`
- Test: unit tests inside `src/workspace.rs`

**Interfaces:**
- Consumes: canonical Git root and root `package.json` bytes from repository inspection.
- Produces: `WorkspacePattern`, `WorkspaceCandidate`, `WorkspaceError`, `parse_npm_workspaces`, `parse_pnpm_workspace`, and `expand_workspace_patterns`.

- [ ] **Step 1: Write failing parser and expansion tests**

Cover npm arrays and `{ "packages": [...] }`, pnpm's strict scalar sequence plus optional scalar `scriptShell`, `shellEmulator`, and `enablePrePostScripts`, literal paths, `apps/*`, deterministic sorting, and rejected `**`, partial wildcards, exclusions, braces, YAML flow collections/anchors/aliases/tags/multiline scalars, tabs, other top-level keys, inconsistent indentation, symlinks, canonical escapes, and unnamed packages.

```rust
assert_eq!(
    expand_workspace_patterns(root, &patterns).unwrap(),
    vec![
        WorkspaceCandidate::new("apps/api", "@acme/api"),
        WorkspaceCandidate::new("apps/web", "@acme/web"),
    ]
);
```

- [ ] **Step 2: Run the workspace tests and verify RED**

```bash
cargo test -j 1 workspace::tests -- --test-threads=1
```

- [ ] **Step 3: Implement the narrow declaration parsers**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
enum PatternSegment {
    Literal(String),
    Wildcard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePattern {
    segments: Vec<PatternSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkspaceCandidate {
    pub relative_path: String,
    pub package_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PnpmWorkspaceDocument {
    pub patterns: Vec<WorkspacePattern>,
    pub script_shell: Option<String>,
    pub shell_emulator: Option<bool>,
    pub enable_pre_post_scripts: Option<bool>,
}

impl WorkspaceCandidate {
    pub fn new(relative_path: impl Into<String>, package_name: impl Into<String>) -> Self;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceError(Reason);

impl WorkspaceError {
    pub const fn reason(self) -> Reason;
}

pub fn parse_npm_workspaces(root_manifest: &[u8])
    -> Result<Vec<WorkspacePattern>, WorkspaceError>;
pub fn parse_pnpm_workspace(bytes: &[u8])
    -> Result<PnpmWorkspaceDocument, WorkspaceError>;
pub fn expand_workspace_patterns(
    root: &Path,
    patterns: &[WorkspacePattern],
) -> Result<Vec<WorkspaceCandidate>, WorkspaceError>;
```

Implement literal segments and `*` whole segments only. Sort `read_dir` names before descent, reject symlinks before canonicalization, and require canonical candidates to remain inside the canonical root. The pnpm line-state parser accepts blank/comment lines, exactly two-space-indented `- scalar` entries below `packages:`, and the three declared scalar keys; do not add YAML or glob dependencies.

- [ ] **Step 4: Implement exact configured-workspace resolution**

```rust
pub fn resolve_configured_workspace<'a>(
    configured: &WorkspaceConfig,
    candidates: &'a [WorkspaceCandidate],
) -> Result<&'a WorkspaceCandidate, WorkspaceError>;
```

Require exactly one candidate whose normalized path and package name both match. Name-only, path-only, zero, or multiple matches return `workspace-cardinality`.

- [ ] **Step 5: Run gates, commit, and push**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 workspace::tests -- --test-threads=1
cargo test -j 1 -- --test-threads=1
git diff --check
git add src/workspace.rs src/lib.rs
git commit -m "feat: validate exact workspace identity"
git push origin main
```

### Task 3: Package-Manager and Node Version Evidence

**Files:**
- Create: `src/package_manager.rs`
- Modify: `src/lib.rs`
- Modify: `src/repository.rs`
- Test: unit tests inside `src/package_manager.rs`
- Test: `tests/doctor_cli.rs`

**Interfaces:**
- Consumes: `PackageManagerReport`, root `.npmrc`, `pnpm-workspace.yaml`, `.node-version`, `.nvmrc`, and an optional exact workspace package name.
- Produces: `RepositoryShellPolicy`, `NodeVersionEvidence`, `LaunchArray`, `inspect_npmrc`, `inspect_pnpm_settings`, `inspect_node_version`, and `build_launch_array`.

- [ ] **Step 1: Write failing shell, Node-version, and argument-array tests**

Require exact arrays:

```rust
assert_eq!(
    build_launch_array(PackageManagerKind::Npm, "test", None, &[]).unwrap(),
    LaunchArray::new("npm", ["--script-shell=/bin/sh", "run", "test"]),
);
assert_eq!(
    build_launch_array(PackageManagerKind::Pnpm, "test", Some("@acme/web"), &[]).unwrap(),
    LaunchArray::new("pnpm", [
        "--config.script-shell=/bin/sh",
        "--config.shell-emulator=false",
        "--filter", "@acme/web", "--fail-if-no-match", "run", "test"
    ]),
);
```

Also test forwarded arguments after one `--`, npm workspace ordering, `/bin/sh` acceptance, other shells/interpolation rejection, pnpm `shellEmulator: true` rejection, exact `24.14.1`/`v24.14.1` normalization, conflicting Node files, and ranges/aliases/extra tokens as unsupported.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
cargo test -j 1 package_manager::tests -- --test-threads=1
```

- [ ] **Step 3: Implement repository shell-policy parsing**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepositoryShellPolicy {
    pub script_shell_supported: bool,
    pub shell_emulator_disabled: bool,
}

pub fn inspect_npmrc(bytes: Option<&[u8]>)
    -> Result<RepositoryShellPolicy, Reason>;
pub fn inspect_pnpm_settings(document: &PnpmWorkspaceDocument)
    -> Result<RepositoryShellPolicy, Reason>;
```

Parse only repository files. Ignore unrelated well-formed `.npmrc` keys. Reject malformed relevant entries, interpolation, explicit non-`/bin/sh` script shells, and enabled shell emulation with `script-shell-unsupported`. Treat declared lifecycle scripts as potentially active regardless of lifecycle-enable settings.

- [ ] **Step 4: Implement exact Node evidence and immutable argument data**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeVersionEvidence(pub Version);

pub fn inspect_node_version(
    node_version: Option<&[u8]>,
    nvmrc: Option<&[u8]>,
) -> Result<Option<NodeVersionEvidence>, Reason>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchArray {
    pub executable: String,
    pub arguments: Vec<String>,
}

impl LaunchArray {
    pub fn new<I, S>(executable: impl Into<String>, arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>;
}
```

Accept exactly one semantic-version token with optional leading `v`; both files must agree after normalization. Build argument vectors without `Command`, shell strings, or environment inspection.

- [ ] **Step 5: Strengthen the production source guard**

Extend `tests/doctor_cli.rs` so Phase 2 production source rejects `std::process::Command`, `Command::new`, `node --version`, `npm config`, and `pnpm config`; retain the existing first-party `unsafe` guard. Test-only subprocess use remains permitted.

- [ ] **Step 6: Run gates, commit, and push**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 package_manager::tests -- --test-threads=1
cargo test -j 1 -- --test-threads=1
git diff --check
git add src/package_manager.rs src/repository.rs src/lib.rs tests/doctor_cli.rs
git commit -m "feat: inspect package manager policy data"
git push origin main
```

### Task 4: Finite-State Script Tokenizer

**Files:**
- Create: `src/script/mod.rs`
- Create: `src/script/tokenizer.rs`
- Modify: `src/lib.rs`
- Test: unit tests inside `src/script/tokenizer.rs`

**Interfaces:**
- Consumes: raw package-script bytes held only during inspection.
- Produces: `TokenizedScript`, `CommandSegment`, and `tokenize_script`.

- [ ] **Step 1: Write the complete table-driven grammar tests**

Accepted entries include safe unquoted words, single-quoted literals, double-quoted `\"` and `\\`, quoted glob characters, `--`, and multiple `&&` segments. Rejected entries include every Revision 6 §8.4.2 form: newline/CR, pipe/or/lone ampersand/semicolon, dollar/backtick/leading tilde, redirection, unquoted globs, grouping/braces, comment, invalid backslash, adjacent fragments, empty segment, leading/trailing `&&`, non-UTF-8, and unterminated quotes.

```rust
#[test]
fn tokenizes_safe_segments_without_reconstructing_shell_text() {
    let parsed = tokenize_script(br#"rimraf dist && vitest run 'src/*.test.ts'"#).unwrap();
    assert_eq!(parsed.segments().len(), 2);
    assert_eq!(parsed.segments()[1].arguments(), ["vitest", "run", "src/*.test.ts"]);
}

#[test]
fn rejects_every_unsupported_operator() {
    for script in [b"a | b".as_slice(), b"a || b", b"a &", b"a; b", b"a > out"] {
        assert_eq!(tokenize_script(script).unwrap_err(), Reason::ScriptSyntaxUnsupported);
    }
}
```

- [ ] **Step 2: Run tokenizer tests and verify RED**

```bash
cargo test -j 1 script::tokenizer::tests -- --test-threads=1
```

- [ ] **Step 3: Implement the finite-state tokenizer**

Define states `BetweenWords`, `Unquoted`, `SingleQuoted`, `DoubleQuoted`, and `AfterQuoted`. Iterate bytes once and recognize `&&` only outside quotes.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenizedScript {
    segments: Vec<CommandSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSegment {
    arguments: Vec<String>,
}

pub fn tokenize_script(bytes: &[u8]) -> Result<TokenizedScript, Reason>;
```

Do not retain input bytes and do not implement `Display`, shell escaping, or reconstruction.

- [ ] **Step 4: Add privacy assertions**

Pass a secret sentinel inside an invalid script and assert the error's `Debug` form contains only the enum/reason, never the input. Confirm returned structures contain decoded accepted arguments but no original full script.

- [ ] **Step 5: Run gates, commit, and push**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 script::tokenizer::tests -- --test-threads=1
cargo test -j 1 -- --test-threads=1
git diff --check
git add src/script src/lib.rs
git commit -m "feat: tokenize bounded package scripts"
git push origin main
```

### Task 5: Lifecycle and Bounded Script Graph

**Files:**
- Create: `src/script/graph.rs`
- Modify: `src/script/mod.rs`
- Test: unit tests inside `src/script/graph.rs`

**Interfaces:**
- Consumes: selected script key and selected package's `BTreeMap<String, String>` scripts.
- Produces: `ScriptGraph`, `LeafOccurrence`, `ScriptPhase`, and `expand_script_graph`.

- [ ] **Step 1: Write failing lifecycle and graph-bound tests**

Assert lifecycle order `pre<target>`, target, `post<target>`; potential-lifecycle labels; Node references without added lifecycle; npm/pnpm references with their declared lifecycles; depth three accepted and depth four rejected; cycle rejection; repeated-reference occurrence charging; exactly 32 leaves accepted and leaf 33 rejected; and reference flags/arguments/`--`/globs/workspace selectors/missing keys rejected.

- [ ] **Step 2: Run graph tests and verify RED**

```bash
cargo test -j 1 script::graph::tests -- --test-threads=1
```

- [ ] **Step 3: Implement graph types and bounded recursion**

```rust
pub const MAX_REFERENCE_DEPTH: u8 = 3;
pub const MAX_LEAF_OCCURRENCES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptPhase { Pre, Target, Post, Referenced }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeafOccurrence {
    pub script_key: String,
    pub phase: ScriptPhase,
    pub potential_lifecycle: bool,
    pub depth: u8,
    pub segment: CommandSegment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptGraph {
    pub leaves: Vec<LeafOccurrence>,
}

pub fn expand_script_graph(
    target: &str,
    scripts: &BTreeMap<String, String>,
) -> Result<ScriptGraph, Reason>;
```

Use an active `Vec<&str>` for cycles and one leaf counter shared by lifecycle/reference expansion. Check the counter before every push. Expand only exact `node --run`, `npm run`, and `pnpm run` three-token references.

- [ ] **Step 4: Run gates, commit, and push**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 script::graph::tests -- --test-threads=1
cargo test -j 1 -- --test-threads=1
git diff --check
git add src/script/graph.rs src/script/mod.rs
git commit -m "feat: expand bounded script graphs"
git push origin main
```

### Task 6: Transparent Wrapper Classification

**Files:**
- Create: `src/script/wrapper.rs`
- Modify: `src/script/mod.rs`
- Test: unit tests inside `src/script/wrapper.rs`

**Interfaces:**
- Consumes: one `CommandSegment` plus exact installed wrapper identity/version from adapter lookup.
- Produces: `UnwrappedSegment`, `WrapperEvidence`, `WrapperKind`, and `unwrap_segment`.

- [ ] **Step 1: Write failing cross-env and dotenv tests**

```rust
let result = unwrap_segment(
    segment(["cross-env", "SECRET=value", "NODE_ENV=test", "vitest", "run"]),
    wrapper("cross-env", "10.1.0"),
).unwrap();
assert_eq!(result.arguments(), ["vitest", "run"]);
assert_eq!(result.evidence(), WrapperEvidence::new(WrapperKind::CrossEnv, 2));
assert!(!format!("{result:?}").contains("SECRET"));
```

Accept repeated `dotenv -e <relative-file>` pairs before mandatory `--`. Reject missing commands, invalid assignment keys, absolute/empty/escaping paths, unsupported options, nested wrappers, `cross-env-shell`, and unknown wrapper versions.

- [ ] **Step 2: Run wrapper tests and verify RED**

```bash
cargo test -j 1 script::wrapper::tests -- --test-threads=1
```

- [ ] **Step 3: Implement redacted unwrapping**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapperKind { CrossEnv, Dotenv }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapperIdentity {
    pub package_name: String,
    pub version: Version,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WrapperEvidence {
    pub kind: WrapperKind,
    pub consumed_count: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnwrappedSegment {
    arguments: Vec<String>,
    evidence: Option<WrapperEvidence>,
}

pub fn unwrap_segment(
    segment: &CommandSegment,
    identity: Option<&WrapperIdentity>,
) -> Result<UnwrappedSegment, Reason>;
```

Build a new argument vector after consuming sensitive wrapper arguments; never store or format consumed values. Permit at most one unwrap operation.

- [ ] **Step 4: Run gates, commit, and push**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 script::wrapper::tests -- --test-threads=1
cargo test -j 1 -- --test-threads=1
git diff --check
git add src/script/wrapper.rs src/script/mod.rs
git commit -m "feat: classify transparent script wrappers"
git push origin main
```

### Task 7: Exact-Version Adapter Matrix

**Files:**
- Create: `src/adapter.rs`
- Create: `schemas/adapter-matrix-v1.schema.json`
- Create: `adapters/matrix-v1.json`
- Create: `tests/fixtures/repositories/adapter-packages/`
- Modify: `src/lib.rs`
- Modify: `src/repository.rs`
- Test: unit tests inside `src/adapter.rs`

**Interfaces:**
- Consumes: executable/subcommand tokens, exact package or Node version evidence, selected package directory, Git root, and forwarded arguments.
- Produces: `InstalledPackage`, `resolve_installed_package`, `AdapterMatrix`, `AdapterMatch`, `Classification`, `ControlDecision`, `Disclosure`, `load_embedded_matrix`, and `match_adapter`.

- [ ] **Step 1: Write failing structure and exact-version tests**

Assert the embedded artifact parses, contains exactly the snapshot versions from Global Constraints, contains no version-range operators, and uses unique package/executable/version tuples. For each entry, assert the exact version matches and one-patch lower/higher versions return `tool-version-unsupported`.

Create fixture `package.json` files containing only `name` and `version`. Require one fixture per package matrix entry and one matrix entry per fixture.

Also require package resolution to prefer `<selected>/node_modules/<identity>/package.json`, fall back once to `<root>/node_modules/<identity>/package.json`, reject scoped-name traversal, symlinks/canonical escapes, malformed identity/version, and a manifest whose `name` differs from the requested matrix identity.

- [ ] **Step 2: Run adapter tests and verify RED**

```bash
cargo test -j 1 adapter::tests -- --test-threads=1
```

- [ ] **Step 3: Define the schemas and initial artifact**

Encode these exact initial decisions:

```text
vitest 4.1.11       controlled: require `run`; suffix `--no-file-parallelism --maxWorkers=1`
jest 30.5.1         controlled: suffix `--runInBand`
node 24.14.1        controlled: require `--test`; suffix `--test-concurrency=1`
typescript 7.0.2    controlled: `tsc` compile/build; deny `--watch` and `-w`; no suffix
eslint 10.9.1       controlled: suffix `--concurrency=off`; deny watch/UI forms
next 16.3.4         disclosed: `next build`; `internal-fanout-uncontrolled`
@nestjs/cli 12.0.0  disclosed: `nest build`; `internal-fanout-uncontrolled`
cross-env 10.1.0    transparent wrapper
dotenv-cli 11.0.0   transparent wrapper
rimraf 6.1.3        auxiliary: static relative paths only
```

Package-manager entries contain npm `12.0.2` and pnpm `11.25.0` with Task 3's templates. Deny watch, UI, background, parallel/race, and conflicting concurrency as exact tokens or exact `--key=value` forms, never substrings.

- [ ] **Step 4: Implement embedded parsing and exact lookup**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Classification { Controlled, Disclosed, Auxiliary }

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AdapterMatrix {
    evidence: BTreeMap<String, String>,
    package_managers: Vec<PackageManagerRule>,
    adapters: Vec<AdapterRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct PackageManagerRule {
    name: String,
    version: String,
    root_arguments: Vec<String>,
    workspace_arguments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AdapterRule {
    package_name: String,
    executable: String,
    version: String,
    classification: Classification,
    required_prefix: Vec<String>,
    suffix: Vec<String>,
    denial_tokens: Vec<String>,
    disclosure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlDecision {
    AlreadyControlled,
    RequiresSuffix(Vec<String>),
    NoControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disclosure {
    pub identifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterMatch<'a> {
    pub rule: &'a AdapterRule,
    pub classification: Classification,
    pub control: ControlDecision,
    pub disclosure: Option<Disclosure>,
}

pub fn load_embedded_matrix() -> Result<AdapterMatrix, Reason>;
pub fn match_adapter<'a>(
    matrix: &'a AdapterMatrix,
    package_name: &str,
    version: &Version,
    arguments: &[String],
) -> Result<AdapterMatch<'a>, Reason>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub package_name: String,
    pub version: Version,
    pub evidence_file: String,
}

pub fn resolve_installed_package(
    git_root: &Path,
    selected_package: &Path,
    package_name: &str,
) -> Result<InstalledPackage, Reason>;
```

Load with `include_str!("../adapters/matrix-v1.json")`. Validate uniqueness, exact semver, non-empty stable identifiers, and classification-specific fields in Rust. Resolve only matrix-approved package names, split a scoped identity into exactly two safe path components, reject symlinks before canonicalization, and store only the repository-relative manifest identity.

- [ ] **Step 5: Implement controls and forwarded-argument decisions**

Return the specific denial reason before proposing a suffix. Recognize already-present exact controls idempotently. Return suffixes as argument data only; never mutate or reconstruct a script.

- [ ] **Step 6: Record official matrix evidence**

Add a top-level `_evidence` object containing only official documentation URLs from the plan's Reference Inputs. Runtime ignores it; tests require HTTPS and allowlisted official hosts. Do not copy documentation prose into the artifact.

- [ ] **Step 7: Run gates, commit, and push**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 adapter::tests -- --test-threads=1
cargo test -j 1 -- --test-threads=1
git diff --check
git add src/adapter.rs src/repository.rs src/lib.rs schemas/adapter-matrix-v1.schema.json adapters/matrix-v1.json tests/fixtures/repositories/adapter-packages
git commit -m "feat: add exact adapter policy matrix"
git push origin main
```

### Task 8: Immutable Operation Policy Assembly

**Files:**
- Create: `src/policy.rs`
- Modify: `src/lib.rs`
- Modify: `src/repository.rs`
- Test: unit tests inside `src/policy.rs`

**Interfaces:**
- Consumes: validated configuration/selection, exact package identity, `ScriptGraph`, wrapper evidence, matrix matches, and `LaunchArray`.
- Produces: `OperationPolicy`, `PolicyLeaf`, `PolicyTarget`, `PolicyInput`, and `build_operation_policy` for Phase 3.

- [ ] **Step 1: Write failing eligibility and injection tests**

Cover controlled/disclosed/auxiliary leaves, zero controlled/disclosed target leaves, lifecycle/nested missing controls, final top-level suffix eligibility, already-controlled idempotence, disclosure collection, forwarded-argument placement, denial precedence, and exact workspace arrays.

```rust
let json = serde_json::to_string(&policy.redacted_summary()).unwrap();
for forbidden in [repo_root, raw_script, "SECRET=value", ".env.private"] {
    assert!(!json.contains(forbidden));
}
```

- [ ] **Step 2: Run policy tests and verify RED**

```bash
cargo test -j 1 policy::tests -- --test-threads=1
```

- [ ] **Step 3: Implement immutable policy types**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationPolicy {
    pub target: PolicyTarget,
    pub operation_key: String,
    pub script_key: String,
    pub timeout_seconds: u16,
    pub leaves: Vec<PolicyLeaf>,
    pub launch: LaunchArray,
    pub disclosures: Vec<String>,
    pub evidence_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyTarget {
    Root,
    Workspace { key: String, package_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyLeaf {
    pub script_key: String,
    pub phase: ScriptPhase,
    pub classification: Classification,
    pub potential_lifecycle: bool,
    pub wrapper: Option<WrapperEvidence>,
    pub control: ControlDecision,
}

pub struct PolicyInput<'a> {
    pub target: PolicyTarget,
    pub operation_key: &'a str,
    pub operation: &'a OperationConfig,
    pub graph: &'a ScriptGraph,
    pub matrix: &'a AdapterMatrix,
    pub package_manager: PackageManagerKind,
    pub package_manager_version: &'a Version,
    pub installed_versions: &'a BTreeMap<String, Version>,
    pub forwarded_arguments: &'a [String],
    pub evidence_files: &'a [String],
}

pub fn build_operation_policy(input: PolicyInput<'_>)
    -> Result<OperationPolicy, Reason>;
```

Keep canonical paths only in short-lived `PolicyInput`; store normalized repository-relative evidence identities. Do not derive `Serialize` for the full policy; expose a separate redacted summary.

- [ ] **Step 4: Implement exact final-leaf rules**

Locate the final top-level target leaf after wrapper unwrapping. A missing suffix is eligible only there and only when the leaf is not a reference. Missing control elsewhere returns `nonfinal-injection-required`. Forwarded arguments require the same eligible recipient and adapter contract. Require at least one controlled/disclosed target leaf.

- [ ] **Step 5: Run gates, commit, and push**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 policy::tests -- --test-threads=1
cargo test -j 1 -- --test-threads=1
git diff --check
git add src/policy.rs src/repository.rs src/lib.rs
git commit -m "feat: assemble immutable operation policies"
git push origin main
```

### Task 9: Repository Fixtures and Doctor Integration

**Files:**
- Create: `tests/repository_policy.rs`
- Create: `tests/fixtures/repositories/npm-single/`
- Create: `tests/fixtures/repositories/npm-workspace/`
- Create: `tests/fixtures/repositories/pnpm-single/`
- Create: `tests/fixtures/repositories/pnpm-workspace/`
- Create: `tests/fixtures/repositories/hostile/`
- Modify: `src/repository.rs`
- Modify: `src/doctor.rs`
- Modify: `tests/doctor_cli.rs`

**Interfaces:**
- Consumes: all Phase 2 typed modules.
- Produces: enriched `RepositoryReport` and `DoctorReport` with redacted `OperationSummary` values and executable-boundary evidence.

- [ ] **Step 1: Create fixture manifests and failing integration tests**

Each fixture contains `.git/`, root `package.json`, exactly one lockfile, optional workspace/configuration evidence, selected package manifests, and minimal `node_modules/<package>/package.json` evidence. No fixture installs dependencies or contains executable package code.

Test npm/pnpm root and workspace success plus duplicate name, zero match, path/name mismatch, symlink escape, unsupported workspace syntax, unknown version, shell conflict, hostile script syntax, wrapper redaction, graph limit, and every denial category.

- [ ] **Step 2: Add zero-child sentinels and verify RED**

Copy each source fixture to a temporary directory, create executable sentinels named `git`, `node`, `npm`, `pnpm`, `vitest`, `jest`, `tsc`, `eslint`, `next`, `nest`, `cross-env`, `dotenv`, and `rimraf`, prepend their directory to `PATH`, run only the Agent Lowmem test binary, and assert no marker is created.

```bash
cargo test -j 1 --test repository_policy -- --test-threads=1
```

Expected: tests fail because doctor does not yet expose Phase 2 operation summaries.

- [ ] **Step 3: Extend repository and doctor reports**

```rust
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OperationSummary {
    pub workspace_key: Option<String>,
    pub operation_key: String,
    pub status: OperationStatus,
    pub configured: bool,
    pub reason: Option<Reason>,
    pub disclosures: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OperationStatus { Runnable, Rejected }
```

When config exists, report configured operations. When absent, analyze only existing canonical root keys `test`, `typecheck`, `lint`, and `build`, label them candidates, and never synthesize configuration. Sort workspaces and operations lexicographically.

- [ ] **Step 4: Extend human and JSON privacy assertions**

Assert JSON includes exact manager/operation summaries but excludes fixture roots, home paths, raw scripts, secrets, assignments, dotenv paths, and environment values. Human output names only stable workspace/operation keys, classifications, disclosures, rejection reasons, and next action. Preserve the Phase 2 `run` unavailable message.

- [ ] **Step 5: Run focused and complete gates**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 --test repository_policy -- --test-threads=1
cargo test -j 1 --test doctor_cli -- --test-threads=1
cargo test -j 1 -- --test-threads=1
git diff --check
```

- [ ] **Step 6: Commit and push doctor integration**

```bash
git add src/repository.rs src/doctor.rs tests/repository_policy.rs tests/doctor_cli.rs tests/fixtures/repositories
git commit -m "feat: report repository operation policies"
git push origin main
```

### Task 10: Phase 2 Exit Gate and Evidence

**Completion record (2026-09-03):** Tasks 1–9 were implemented, verified, committed atomically, and pushed to `main`. Task 10's Rust, release-resource, production-boundary, privacy, deferred-command, and scope gates passed against implementation HEAD `8ef4af69bf6829dccc623a46d4b6fb385ff670b3`. Exact development measurements are recorded in `docs/dependencies-v1.md`. No runner, managed-file generator, distribution package, tag, or release was created; the saved next action remains a separately approved Phase 3 managed-runner design.

**Files:**
- Modify: `tests/doctor_budget.rs`
- Modify: `docs/dependencies-v1.md`
- Modify: `docs/superpowers/plans/2026-09-03-agent-lowmem-phase-2-repository-policy.md`

**Interfaces:**
- Consumes: complete Phase 2 implementation.
- Produces: sequential verification evidence and the Phase 2 completion decision; no release artifact.

- [ ] **Step 1: Add the committed repository doctor budget**

Extend `doctor_budget.rs` with an ignored release-only test that copies `tests/fixtures/repositories/npm-single` to a temporary directory, runs the release binary 20 times, sorts elapsed milliseconds, and asserts median index 9 at most 300 ms and p95 index 18 at most 500 ms. Preserve the outside-repository 100 ms median gate.

- [ ] **Step 2: Run the complete Rust gate sequentially**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 -- --test-threads=1
cargo build --release -j 1
cargo test --release --test doctor_budget -j 1 -- --ignored --test-threads=1 --nocapture
stat -f '%z bytes' target/release/agent-lowmem
/usr/bin/time -l target/release/agent-lowmem doctor >/dev/null
git diff --check
```

Expected: every command passes; outside-repository median is at most 100 ms; fixture median/p95 are at most 300/500 ms; binary is at most 12 MiB; parent maximum RSS is at most 24 MiB.

- [ ] **Step 3: Re-run the production-boundary audit**

```bash
rg -n 'std::process::Command|Command::new|node --version|npm config|pnpm config|kern\.memorystatus_vm_pressure_level|NODE_OPTIONS' src
rg -n 'tokio|async-std|reqwest|ureq|hyper' Cargo.toml Cargo.lock
```

Expected: inspect every match. Production modules contain no process launch, private pressure read, environment-policy read, async runtime, or network client. Crate-level `forbid(unsafe_code)` declarations and test/source-guard literals are intentional.

- [ ] **Step 4: Record exact Phase 2 measurements**

Append a dated table to `docs/dependencies-v1.md` containing HEAD under test, matrix versions, test totals, release binary bytes, maximum RSS bytes, outside-repository median/p95, fixture median/p95, and exact commands. Label them development measurements, not a release claim.

- [ ] **Step 5: Verify scope and artifacts**

Run each command separately and inspect its result:

```bash
test ! -f README.md
test ! -d npm
test ! -d Formula
test -z "$(git tag --points-at HEAD)"
target/release/agent-lowmem run test >/dev/null 2>&1; test "$?" = 64
target/release/agent-lowmem init >/dev/null 2>&1; test "$?" = 2
git status --short
```

Confirm all ten Phase 2 exit conditions have evidence and no runner, managed-file, distribution, tag, or release work entered the implementation.

- [ ] **Step 6: Commit the evidence and completion record**

```bash
git add tests/doctor_budget.rs docs/dependencies-v1.md docs/superpowers/plans/2026-09-03-agent-lowmem-phase-2-repository-policy.md
git commit -m "docs: record phase 2 repository policy evidence"
git push origin main
```

- [ ] **Step 7: Verify remote completion**

```bash
test -z "$(git status --porcelain=v1)"
test "$(git branch --show-current)" = main
test "$(git rev-parse HEAD)" = "$(git ls-remote origin refs/heads/main | awk '{print $1}')"
gh pr list --repo Pleo2/agent-lowmem --state open --json number,title
gh release list --repo Pleo2/agent-lowmem --limit 10
```

Expected: clean `main`, local HEAD equals remote `main`, no Phase 2 PR, and no release. The saved next action is a separately approved Phase 3 managed-runner design.

## Reference Inputs for Matrix Review

- npm scripts and workspaces: `https://docs.npmjs.com/cli/using-npm/scripts/` and `https://docs.npmjs.com/cli/using-npm/workspaces/`
- pnpm run and settings: `https://pnpm.io/cli/run` and `https://pnpm.io/settings`
- Vitest CLI: `https://v4.vitest.dev/guide/cli`
- Jest CLI: `https://jestjs.io/docs/cli`
- Node test runner: `https://nodejs.org/api/test.html`
- TypeScript CLI: `https://www.typescriptlang.org/docs/handbook/compiler-options.html`
- ESLint CLI: `https://eslint.org/docs/latest/use/command-line-interface`
- Next.js CLI: `https://nextjs.org/docs/app/api-reference/cli/next`
- Nest CLI: `https://docs.nestjs.com/cli/usages`

The registry snapshot records exact package versions only. Documentation and executable conformance must still be reviewed before an adapter becomes executable in Phase 3.
