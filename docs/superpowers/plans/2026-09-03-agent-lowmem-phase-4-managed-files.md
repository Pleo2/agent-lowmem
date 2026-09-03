# Agent Lowmem Phase 4 Managed Files Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add deterministic, reversible repository onboarding through `init` and `restore`, including zero-write previews, generated `.agent-lowmem.json`, one hashed `AGENTS.md` policy block, and a private crash-recovery journal.

**Architecture:** Reuse the Phase 2 repository classifier to build an immutable managed-files Plan A, acquire the existing per-user lease only for mutating commands, rebuild Plan B from the same Git root, and require exact equality before writing. Apply each repository file atomically through held directory descriptors while a private `prepared` journal is durable; transition it to `applied` only after exact verification. Restore removes only bytes proven to be Agent Lowmem-owned and preserves all unrelated or external bytes.

**Tech Stack:** Rust 1.85, edition 2024, `std`, existing `rustix`, `serde`, `serde_json`, and `sha2`; JSON Schema draft 2020-12; no shell, Git child, Node child, async runtime, network client, or new production dependency assumed.

**Spec:** `docs/superpowers/specs/2026-09-03-agent-lowmem-phase-4-managed-files-design.md`

## Global Constraints

- Work sequentially on `main`; use `-j 1` and `--test-threads=1` for full gates on the reference 8 GiB Mac.
- Preserve unrelated local changes. Stage only the files named by the current task and use one Conventional Commit per task.
- Follow RED → GREEN → REFACTOR. Do not begin a later task while the focused tests for the current task are red.
- Keep `#![forbid(unsafe_code)]`; add no first-party unsafe block, shell evaluator, async runtime, network client, daemon, process-table scan, or process launch.
- `doctor`, Plan A, Plan B, both dry runs, recovery classification, and restore inspection start zero child processes.
- Only non-dry-run `init` and `restore` may create the runtime directory, acquire the lease, create Git-private state, or mutate repository files.
- Treat `.agent-lowmem.json` and `AGENTS.md` as untrusted bytes. Follow no symlink and mutate only through validated, held directory descriptors.
- Bound reads before allocation: config and journal 262,144 bytes each, `AGENTS.md` 1,048,576 bytes, generated block 65,536 bytes, 128 workspaces, and 256 public candidates.
- Preserve manual configuration and all `AGENTS.md` exterior bytes byte for byte. Never persist manual configuration bytes or exterior Markdown bytes in the journal.
- Keep the existing closed `Reason` vocabulary and Phase 3 result schema unchanged. Managed-files output has its own schema and stable status line.
- Preserve the 12 MiB stripped-binary and 24 MiB parent-RSS gates. Record warm-cache dry-run and unchanged-command measurements without turning them into portability claims.
- Add no direct dependency during this plan. If existing APIs cannot satisfy an invariant, stop that task and perform the spec-required API/source/license/MSRV/transitive/size/security review before proposing a manifest change.

## File Responsibility Map

| File | Responsibility |
| --- | --- |
| `src/cli.rs` | Strict `init` and `restore` grammar only |
| `src/repository.rs` | Canonical Git root/metadata discovery and reusable no-child policy evidence |
| `src/configuration.rs` | Typed deterministic configuration serialization |
| `src/agents_policy.rs` | Marker parser, body renderer, block placement, and span edit plan |
| `src/atomic_file.rs` | Bounded no-follow reads and component-relative atomic file operations |
| `src/restoration.rs` | Private journal schema, state validation, recovery, and restoration state |
| `src/managed_files.rs` | Init/restore planning, public report, Plan A/B comparison, and orchestration |
| `src/main.rs` | Command dispatch and stdout/stderr contracts |
| `src/doctor.rs` | Phase 4 capability reporting |
| `schemas/managed-files-result-v1.schema.json` | Public managed-files output contract |
| `schemas/restoration-manifest-v1.schema.json` | Private journal contract used by code and tests |
| `tests/managed_files_*.rs` | End-to-end boundaries, failure injection, locking, privacy, and budgets |

## Dependency Order

```text
CLI + Git discovery
        |
        v
config renderer + AGENTS parser + report schema
        |
        v
journal model + atomic file primitives
        |
        v
planner -> init transaction -> recovery -> restore
        |
        v
CLI output -> doctor -> full Phase 4 gates
```

---

### Task 1: Strict managed-files CLI grammar

**Files:**
- Modify: `src/cli.rs`
- Modify: `src/main.rs`
- Test: `src/cli.rs`
- Test: `tests/doctor_cli.rs`

**Interfaces:**

```rust
pub enum CliCommand {
    Doctor { json: bool },
    Run(RunRequest),
    Init(InitRequest),
    Restore(RestoreRequest),
}

pub struct InitRequest {
    pub dry_run: bool,
    pub json: bool,
}

pub struct RestoreRequest {
    pub dry_run: bool,
    pub force_managed_block: bool,
    pub json: bool,
}
```

- [x] **Step 1: Add table-driven parser tests for every accepted ordering**

```rust
assert_eq!(
    parse(["init", "--json", "--dry-run"]).unwrap(),
    CliCommand::Init(InitRequest { dry_run: true, json: true }),
);
assert_eq!(
    parse(["restore", "--force-managed-block", "--json", "--dry-run"]).unwrap(),
    CliCommand::Restore(RestoreRequest {
        dry_run: true,
        force_managed_block: true,
        json: true,
    }),
);
```

- [x] **Step 2: Add rejection tables** for duplicate flags, abbreviations, unknown flags, positional arguments, `init --force-managed-block`, NULs, and non-UTF-8 tokens. Every case must return `Reason::InvalidCli`.

- [x] **Step 3: Confirm RED**

Run: `cargo test cli::tests -j 1 -- --test-threads=1`

Expected: compile failure because the two requests and command variants do not exist.

- [x] **Step 4: Implement one shared strict flag-set parser** that accepts flags in any allowed order, tracks duplicates explicitly, and never treats a value as positional input. Add exhaustive `main` arms that return preflight code 64 / `operation-unsupported`; Task 12 replaces this temporary unavailable boundary when orchestration exists.

- [x] **Step 5: Verify and commit**

Run: `cargo fmt --all -- --check`

Run: `cargo test cli::tests -j 1 -- --test-threads=1`

Run: `cargo test --test doctor_cli managed_file_commands_remain_unavailable_before_orchestration -j 1 -- --test-threads=1`

```bash
git add src/cli.rs src/main.rs tests/doctor_cli.rs docs/superpowers/plans/2026-09-03-agent-lowmem-phase-4-managed-files.md
git commit -m "feat: parse managed file commands"
```

### Task 2: Canonical Git metadata and bounded destination discovery

**Files:**
- Modify: `src/repository.rs`
- Create: `src/atomic_file.rs`
- Modify: `src/lib.rs`
- Test: `src/repository.rs`
- Test: `src/atomic_file.rs`

**Interfaces:**

```rust
#[derive(Clone, PartialEq, Eq)]
pub struct GitRepository {
    root: PathBuf,
    metadata: PathBuf,
}

impl GitRepository {
    pub(crate) fn root(&self) -> &Path;
    pub(crate) fn metadata(&self) -> &Path;
}

pub fn find_git_repository(start: &Path)
    -> Result<Option<GitRepository>, RepositoryError>;

pub(crate) enum OptionalFile {
    Absent,
    Regular {
        bytes: Vec<u8>,
        sha256: [u8; 32],
        mode: u32,
        owner_uid: u32,
    },
}

pub(crate) fn read_optional_bounded(
    directory: &File,
    identity: &str,
    max_bytes: usize,
) -> Result<OptionalFile, Reason>;
```

`GitRepository` gets a custom redacted `Debug` implementation that exposes only whether root and metadata were resolved. It must never format either path.

A pointer-file checkout is bound bidirectionally: root `.git` resolves to the metadata directory, and that directory's bounded `gitdir` file must resolve exactly back to the root `.git` identity. This admits ordinary absolute and relative Git worktree pointers while rejecting arbitrary directories.

- [x] **Step 1: Write Git discovery tests** for an ordinary `.git` directory, a valid relative worktree pointer, a valid absolute pointer admitted by the existing bounded/containment contract, an escaping or otherwise non-admitted pointer, an invalid/multiline pointer, a symlink marker, and a pointer whose target is not a directory.

- [x] **Step 2: Write bounded-reader tests** proving absent state, exact digest, early oversized rejection, symlink/special-file rejection, and no-follow behavior for parent and final components. Return mode and owner UID as discovery evidence without exposing file bytes through `Debug`; Task 7 enforces the exact private-directory `0700` and journal `0600` policy when it introduces held mutable directories.

- [x] **Step 3: Confirm RED**

Run: `cargo test repository::tests::resolves_git -j 1 -- --test-threads=1`

Run: `cargo test atomic_file::tests -j 1 -- --test-threads=1`

- [x] **Step 4: Refactor root discovery without behavior drift**

Replace the private `GitRoot` with `GitRepository`, make `find_git_root` callers use `find_git_repository`, and keep metadata paths private from `Debug`, public reports, and errors. Do not execute `git`.

- [x] **Step 5: Implement bounded descriptor-relative reads**

Use `openat` with `NOFOLLOW | CLOEXEC`, validate regular files from the opened descriptor, inspect length before allocation, read at most `max_bytes + 1`, and hash exactly the returned bytes.

- [x] **Step 6: Verify all repository regressions and commit**

Run: `cargo test repository -j 1 -- --test-threads=1`

Run: `cargo test evidence -j 1 -- --test-threads=1`

Run: `cargo test --test repository_policy -j 1 -- --test-threads=1`

```bash
git add src/repository.rs src/atomic_file.rs src/lib.rs
git commit -m "refactor: expose resolved git metadata"
```

### Task 3: Deterministic configuration generation

**Files:**
- Modify: `src/configuration.rs`
- Create: `tests/managed_files_configuration.rs`
- Create: `tests/fixtures/managed-files/npm-root/package.json`
- Create: `tests/fixtures/managed-files/npm-root/package-lock.json`
- Create: `tests/fixtures/managed-files/pnpm-workspaces/package.json`
- Create: `tests/fixtures/managed-files/pnpm-workspaces/pnpm-lock.yaml`
- Create: `tests/fixtures/managed-files/pnpm-workspaces/pnpm-workspace.yaml`
- Create: focused workspace manifests and installed-tool manifests below those fixtures

**Interfaces:**

```rust
impl AgentLowmemConfig {
    pub fn deterministic_bytes(&self) -> Result<Vec<u8>, Reason>;
    pub fn has_operations(&self) -> bool;
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedConfig<'a> {
    #[serde(rename = "$schema")]
    schema: &'static str,
    version: u8,
    package_manager: PackageManagerKind,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    operations: &'a BTreeMap<String, OperationConfig>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    workspaces: &'a BTreeMap<String, WorkspaceConfig>,
}

pub(crate) const CANONICAL_OPERATIONS: [CanonicalOperation; 4];

pub(crate) struct CanonicalOperation {
    pub key: &'static str,
    pub script: &'static str,
    pub timeout_seconds: u16,
}
```

- [x] **Step 1: Add golden serialization tests** for npm root operations and sorted pnpm workspaces. Assert `$schema`, version 1, exact manager kind, two-space indentation, LF, one final newline, and stable key ordering.

- [x] **Step 2: Add canonical table tests** asserting exact names and timeouts: `test`, `typecheck`, and `lint` at 900 seconds; `build` at 1,800 seconds.

- [x] **Step 3: Confirm RED**

Run: `cargo test configuration::tests::serializes -j 1 -- --test-threads=1`

- [x] **Step 4: Derive `Serialize` for typed config values and implement deterministic bytes** through the borrowed `GeneratedConfig` wrapper with `serde_json::to_vec_pretty`, then append exactly one LF. Keep `RawConfig` as the only deserialization model and omit empty operation/workspace maps deterministically.

- [x] **Step 5: Verify parser/serializer round-trip and schema compatibility**

```rust
let bytes = config.deterministic_bytes().unwrap();
assert_eq!(parse_config(&bytes).unwrap(), config);
assert_eq!(bytes.last(), Some(&b'\n'));
```

Run: `cargo test configuration -j 1 -- --test-threads=1`

- [x] **Step 6: Commit**

```bash
git add -f src/configuration.rs tests/managed_files_configuration.rs tests/fixtures/managed-files docs/superpowers/plans/2026-09-03-agent-lowmem-phase-4-managed-files.md
git commit -m "feat: render deterministic repository configuration"
```

### Task 4: Hashed `AGENTS.md` policy block

**Files:**
- Create: `src/agents_policy.rs`
- Modify: `src/lib.rs`
- Create: `tests/managed_agents_policy.rs`
- Create: `tests/fixtures/managed-files/agents/*.md`

**Interfaces:**

```rust
pub(crate) enum AgentsDocumentState {
    Absent,
    NoBlock { bytes: Vec<u8> },
    OneBlock(ManagedBlock),
}

pub(crate) struct ManagedBlock {
    pub span: Range<usize>,
    pub body: Range<usize>,
    pub format: u8,
    pub declared_sha256: [u8; 32],
}

pub(crate) struct AgentsEdit {
    pub target_bytes: Vec<u8>,
    pub managed_span: Range<usize>,
    pub inserted_separator: Vec<u8>,
}

pub(crate) fn inspect_agents(bytes: Option<Vec<u8>>)
    -> Result<AgentsDocumentState, Reason>;
pub(crate) fn render_policy_body(config: &AgentLowmemConfig)
    -> Result<Vec<u8>, Reason>;
pub(crate) fn plan_agents_edit(
    current: AgentsDocumentState,
    body: &[u8],
) -> Result<AgentsEdit, Reason>;
```

The format-1 body starts with the exact v1 policy paragraph and appends the operation examples in this exact form:

```markdown
## Agent Lowmem resource policy

Run supported heavy validation through Agent Lowmem. Run only one heavy
operation at a time, never use watch mode, and prefer focused validation
before broad suites. Do not retry OOM or timeout failures automatically.
Agent Lowmem v1 does not impose a memory cap or guarantee responsiveness;
use CI when a broad build cannot be constrained locally.

Supported commands:
- `agent-lowmem run <operation>`
- `agent-lowmem run <operation> --workspace <workspace>`
```

Render one concrete line per configured operation. Sort root lines by operation key, then workspace lines by workspace key and operation key. Omit either class when empty; do not render the angle-bracket examples literally.

- [ ] **Step 1: Add independent hash goldens** using known body bytes and a separately calculated lowercase SHA-256 marker.

- [ ] **Step 2: Add scanner tables** for no marker, one valid block, duplicate, nested, incomplete start/end, unsupported format, uppercase/invalid digest, hash mismatch, non-UTF-8, and over-limit input.

- [ ] **Step 3: Add placement tests** for absent file, empty file, file ending with and without LF, replacement with arbitrary prefix/suffix bytes, exact desired block, and 65,536-byte generated-block boundary.

- [ ] **Step 4: Confirm RED**

Run: `cargo test agents_policy::tests -j 1 -- --test-threads=1`

- [ ] **Step 5: Implement bounded byte scanning and deterministic rendering**

The digest covers only exact body bytes between marker newlines. The parser must find marker-looking text anywhere; it must not use a Markdown parser. Render only configured operation identities and exact `agent-lowmem run` examples—never raw scripts or rejected candidates.

- [ ] **Step 6: Verify exterior-byte preservation and commit**

Run: `cargo test agents_policy -j 1 -- --test-threads=1`

Run: `cargo test --test managed_agents_policy -j 1 -- --test-threads=1`

```bash
git add src/agents_policy.rs src/lib.rs tests/managed_agents_policy.rs tests/fixtures/managed-files/agents
git commit -m "feat: manage a hashed agents policy block"
```

### Task 5: Closed public report and JSON Schema

**Files:**
- Create: `schemas/managed-files-result-v1.schema.json`
- Create: `src/managed_files.rs`
- Modify: `src/lib.rs`
- Create: `tests/managed_files_report.rs`

**Interfaces:**

```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedFilesReport {
    pub schema_version: u8,
    pub command: ManagedCommand,
    pub dry_run: bool,
    pub outcome: ManagedOutcome,
    pub result: ManagedResult,
    pub files: Vec<ManagedFileReport>,
    pub operations: Vec<ManagedOperationReport>,
    pub manual_candidates: Vec<ManualCandidateReport>,
    pub issues: Vec<ManagedIssueReport>,
    pub manifest_state: ManifestState,
}

pub enum ManagedCommand { Init, Restore }
pub enum ManagedOutcome {
    Planned, Applied, Restored, Unchanged,
    RecoveryRequired, Conflict, Failed,
}
pub enum ManagedIdentity { Configuration, AgentsPolicy, RestorationManifest }
pub enum ManagedAction { Create, Replace, Remove, Unchanged, Preserve, Conflict }
pub enum ManifestState { Absent, Prepared, Applied }

pub struct ManagedResult {
    pub code: i32,
    pub reason: Reason,
}

pub struct ManagedFileReport {
    pub identity: ManagedIdentity,
    pub action: ManagedAction,
    pub before_sha256: Option<String>,
    pub target_sha256: Option<String>,
}

pub struct ManagedOperationReport {
    pub operation_key: String,
    pub workspace_key: Option<String>,
}

pub struct ManualCandidateReport {
    pub operation_prefix: String,
    pub script_name: String,
    pub workspace_key: Option<String>,
}

pub struct ManagedIssueReport {
    pub reason: Reason,
    pub operation_key: Option<String>,
    pub workspace_path: Option<String>,
    pub package_name: Option<String>,
}
```

- [ ] **Step 1: Write the schema first** with `additionalProperties: false`, all required top-level fields, closed enums, lowercase 64-character SHA-256 patterns, and optional fields matching the spec exactly.

- [ ] **Step 2: Add schema validation tests** against one sample for every outcome and negative samples for unknown identities, actions, reasons, extra fields, invalid hashes, target content, absolute paths, and timestamps.

- [ ] **Step 3: Confirm RED**

Run: `cargo test --test managed_files_report -j 1 -- --test-threads=1`

- [ ] **Step 4: Implement typed report construction and validation**

Sort files by identity, operations by `(workspace_key, operation_key)`, candidates by their full tuple, and issues by `(reason, workspace_path, package_name, operation_key)`. Permit only existing `Reason` values that Phase 4 can emit. Ordinary success uses `{ code: 0, reason: completed }`; the one exception is dry-run classification of a recoverable prepared journal, which uses `{ code: 0, reason: managed-file-conflict }` with outcome `recovery-required`. Neither case constructs `ExitResult`.

Validate the exact code/reason families: 2 for `invalid-cli`/`invalid-config`; 64 for supported preflight-policy reasons; 73 for `lock-held`/`nested-invocation`; 75 for `evidence-changed`; 78 for `managed-file-conflict`; 70 for `internal-error`; and the two code-0 forms above. Never admit child, timeout, or signal outcomes in this schema.

- [ ] **Step 5: Add redaction regression assertions**

```rust
for forbidden in [root_path, git_path, secret, raw_script, "transactionId", "timestamp"] {
    assert!(!serialized.contains(forbidden));
}
assert!(!serialized.contains('\u{1b}'));
```

- [ ] **Step 6: Verify and commit**

Run: `cargo test --test managed_files_report -j 1 -- --test-threads=1`

```bash
git add schemas/managed-files-result-v1.schema.json src/managed_files.rs src/lib.rs tests/managed_files_report.rs
git commit -m "feat: define managed file reports"
```

### Task 6: Private restoration journal model

**Files:**
- Create: `schemas/restoration-manifest-v1.schema.json`
- Create: `src/restoration.rs`
- Modify: `src/lib.rs`
- Create: `tests/restoration_manifest.rs`

**Interfaces:**

```rust
pub(crate) enum JournalState { Prepared, Applied }
pub(crate) enum Ownership { Managed, External }
pub(crate) enum PriorManagedState { Absent, Bytes(OwnedBytes) }

pub(crate) struct RestorationManifest {
    pub schema_version: u8,
    pub format_version: u8,
    pub state: JournalState,
    pub repository_sha256: String,
    pub transaction_sha256: String,
    pub configuration: ConfigurationRestoration,
    pub agents_policy: AgentsRestoration,
    pub previous_applied: Option<Box<RestorationManifest>>,
}

pub(crate) fn parse_manifest(bytes: &[u8])
    -> Result<RestorationManifest, Reason>;
pub(crate) fn serialize_manifest(manifest: &RestorationManifest)
    -> Result<Vec<u8>, Reason>;
pub(crate) fn transaction_digest(manifest: &RestorationManifest)
    -> Result<[u8; 32], Reason>;
```

- [ ] **Step 1: Define exact schema fields** for prepared/applied state, ownership, immediate-before managed bytes or absence, target managed bytes/digests, stable baseline, span placement, inserted separator, prefix/suffix digests, and destination actions. Prohibit absolute repository paths and external/manual bytes by construction.

The journal is stored only at resolved metadata identity `agent-lowmem/restoration-v1.json`. `repositorySha256` hashes the canonical repository identity; `transactionSha256` hashes canonical transaction content and is never a random or public transaction ID.

- [ ] **Step 2: Add positive and negative schema tests** for both states, invalid modes/owners, bad digests, impossible spans, oversized bytes, recursive previous state beyond one entry, unknown fields, and non-canonical serialization.

- [ ] **Step 3: Confirm RED**

Run: `cargo test --test restoration_manifest -j 1 -- --test-threads=1`

- [ ] **Step 4: Implement strict typed parsing and deterministic serialization**

Use two-space JSON, LF, one final newline, closed enums, and a transaction digest calculated from a canonical digest-input representation with the `transactionSha256` field excluded.

- [ ] **Step 5: Add privacy tests** proving the journal cannot serialize an absolute root, username, environment value, manual configuration bytes, or `AGENTS.md` prefix/suffix bytes.

- [ ] **Step 6: Verify and commit**

Run: `cargo test restoration -j 1 -- --test-threads=1`

Run: `cargo test --test restoration_manifest -j 1 -- --test-threads=1`

```bash
git add schemas/restoration-manifest-v1.schema.json src/restoration.rs src/lib.rs tests/restoration_manifest.rs
git commit -m "feat: model private restoration journals"
```

### Task 7: Durable component-relative atomic mutations

**Files:**
- Modify: `src/atomic_file.rs`
- Refactor: `src/result_file.rs`
- Create: `tests/atomic_managed_files.rs`

**Interfaces:**

```rust
pub(crate) struct HeldDirectory { /* private File descriptor */ }
pub(crate) struct FilePrecondition {
    pub state: ExpectedState,
}

impl HeldDirectory {
    pub fn open(path: &Path, expected_mode: Option<u32>) -> Result<Self, Reason>;
    pub fn open_or_create_private(
        parent: &HeldDirectory,
        name: &str,
        mode: u32,
    ) -> Result<Self, Reason>;
    pub fn read_optional(&self, name: &str, limit: usize)
        -> Result<OptionalFile, Reason>;
    pub fn replace_atomic(
        &self,
        name: &str,
        expected: &FilePrecondition,
        bytes: &[u8],
        mode: u32,
    ) -> Result<(), Reason>;
    pub fn remove_exact(
        &self,
        name: &str,
        expected: &FilePrecondition,
    ) -> Result<(), Reason>;
    pub fn sync(&self) -> Result<(), Reason>;
}
```

- [ ] **Step 1: Add primitive tests** for exclusive unpredictable temporary creation, `0600` mode under umasks `000` and `077`, preservation of no executable bit when replacing an existing regular file, exact-precondition rejection, parent/final symlink swaps, special files, temp cleanup, rename durability, exact remove, and `0700` private directories.

- [ ] **Step 2: Confirm RED**

Run: `cargo test atomic_file::tests -j 1 -- --test-threads=1`

Run: `cargo test --test atomic_managed_files -j 1 -- --test-threads=1`

- [ ] **Step 3: Implement the reusable primitive** with existing `rustix` APIs only: `openat`, `statat`, `renameat`, `unlinkat`, `fchmod`, descriptor metadata, file/directory `sync_all`, and exclusive no-follow temporary opens.

- [ ] **Step 4: Refactor Phase 3 result writes onto the primitive** while preserving its public API, schema, output precedence, and tests. Do not change result-file permissions or semantics.

- [ ] **Step 5: Run focused regressions**

Run: `cargo test atomic_file -j 1 -- --test-threads=1`

Run: `cargo test result_file -j 1 -- --test-threads=1`

Run: `cargo test --test result_file -j 1 -- --test-threads=1`

- [ ] **Step 6: Commit**

```bash
git add src/atomic_file.rs src/result_file.rs tests/atomic_managed_files.rs
git commit -m "feat: write managed files atomically"
```

### Task 8: No-child init planner and ownership decisions

**Files:**
- Modify: `src/managed_files.rs`
- Modify: `src/repository.rs`
- Modify: `src/configuration.rs`
- Modify: `src/agents_policy.rs`
- Modify: `src/restoration.rs`
- Create: `tests/managed_files_planning.rs`

**Interfaces:**

```rust
pub(crate) struct ManagedFilesPlan {
    command: ManagedCommand,
    root: GitRepository,
    repository_hash: [u8; 32],
    evidence: EvidenceSnapshot,
    configuration: PlannedFile,
    agents_policy: PlannedFile,
    journal: PlannedJournal,
    effective_config: AgentLowmemConfig,
    report: ManagedFilesReport,
}

pub fn plan_init(
    source: &impl HostSource,
    start: &Path,
    request: &InitRequest,
) -> Result<ManagedFilesPlan, ManagedFilesFailure>;

pub(crate) fn managed_plans_match(
    before: &ManagedFilesPlan,
    after: &ManagedFilesPlan,
) -> bool;
```

- [ ] **Step 1: Add root-operation planner tests** covering each canonical script as runnable/rejected, required installed-tool evidence, rejected omission, prefixed manual candidates, no runnable operation, unsupported manager, lockfile mismatch, and init host gate.

- [ ] **Step 2: Add workspace planner tests** for scoped-name final component, already-invalid key, collision, 128/129 workspace boundary, 256/257 candidate boundary, stable sorting, safe root survival, and unrelated-workspace survival.

- [ ] **Step 3: Add configuration ownership table tests** for absent, exact unjournaled adoption, valid different external preservation, external rerun, managed exact replacement, managed edited conflict, invalid config, unrunnable configured operation, symlink, special, non-UTF-8, and oversized input.

- [ ] **Step 4: Add AGENTS ownership table tests** for absent/create, no block/append, one valid/replace, exact/unchanged, pre-journal adoption, malformed/conflict, edited managed/conflict, and exterior-byte preservation.

- [ ] **Step 5: Confirm RED**

Run: `cargo test --test managed_files_planning -j 1 -- --test-threads=1`

- [ ] **Step 6: Extract one reusable repository-policy collection path**

Refactor `inspect_repository` and managed-file planning to consume the existing package-manager, workspace, script-graph, wrapper, adapter, and tool-version logic. Do not copy the classifier. Keep `plan_run` semantics unchanged. For an external configuration, validate every configured root/workspace operation against current evidence and build the AGENTS block from those effective configured operation keys—not from generated defaults.

- [ ] **Step 7: Build the immutable plan and exact comparison**

Compare command kind, request authorization, evidence identities/digests, ownership, actions, target bytes, rollback descriptors, and journal target. Exclude display paths, mtimes, terminal state, and map iteration order.

- [ ] **Step 8: Prove planning privacy and zero-child behavior**

Use sentinel executables for `git`, `node`, `npm`, and `pnpm`; assert no marker. Assert `Debug` and serialized reports omit raw scripts, target content, manual bytes, absolute paths, environment values, and Git metadata paths.

- [ ] **Step 9: Verify and commit**

Run: `cargo test --test managed_files_planning -j 1 -- --test-threads=1`

Run: `cargo test --test repository_policy -j 1 -- --test-threads=1`

```bash
git add src/managed_files.rs src/repository.rs src/configuration.rs src/agents_policy.rs src/restoration.rs tests/managed_files_planning.rs
git commit -m "feat: plan managed repository files"
```

### Task 9: Journaled init transaction and rollback

**Files:**
- Modify: `src/managed_files.rs`
- Modify: `src/restoration.rs`
- Modify: `src/atomic_file.rs`
- Modify: `src/lock.rs`
- Create: `tests/managed_files_transaction.rs`
- Create: `tests/managed_files_lock.rs`

**Interfaces:**

```rust
pub(crate) trait TransactionFaults {
    fn fail_at(&self, point: FaultPoint) -> bool;
}

pub(crate) enum FaultPoint {
    PreparedDurable,
    ConfigurationWritten,
    AgentsWritten,
    TargetsVerified,
    AppliedJournalDurable,
}

pub fn execute_init(
    source: &impl HostSource,
    start: &Path,
    runtime: &Path,
    request: &InitRequest,
) -> ManagedFilesOutcome;
```

- [ ] **Step 1: Add Plan A/B drift tests** mutating every source and destination identity between passes. Expect code 75 / `evidence-changed`, no prepared journal, and no repository mutation.

- [ ] **Step 2: Add global lease tests** proving `run`, `init`, and `restore` serialize through the same lock; dry-run creates/acquires nothing; nested invocation returns code 73; journal records no process identity.

- [ ] **Step 3: Add happy-path transaction tests** for new repo, external config, adopted config/block, managed update, unchanged rerun, exact journal modes, and apply order observable through a test-only fault seam.

- [ ] **Step 4: Add failure injection at every durable boundary**

For each `FaultPoint`, assert either complete immediate rollback plus restored prior applied journal, or a preserved valid prepared journal and code 70. Assert no completion outcome on partial state.

- [ ] **Step 5: Confirm RED**

Run: `cargo test --test managed_files_transaction -j 1 -- --test-threads=1`

Run: `cargo test --test managed_files_lock -j 1 -- --test-threads=1`

- [ ] **Step 6: Implement exact apply order**

1. Validate descriptors and Plan B.
2. Create private directory mode `0700`.
3. Atomically write/sync prepared journal mode `0600`.
4. Apply managed config if needed.
5. Apply AGENTS span if needed.
6. Verify exact installed targets through held descriptors.
7. Atomically write/sync applied journal.
8. Release lease by dropping it.

Acquire the lease with the Plan A repository hash and operation key `init`, then compute Plan B from Plan A's canonical Git root rather than from a newly interpreted current working directory.

- [ ] **Step 7: Implement handled rollback** using only immediate-before owned states; restore the prior applied journal or remove the first-init journal and its owned private directory when empty. Preserve prepared state when rollback proof fails.

- [ ] **Step 8: Verify and commit**

Run: `cargo test --test managed_files_transaction -j 1 -- --test-threads=1`

Run: `cargo test --test managed_files_lock -j 1 -- --test-threads=1`

Run: `cargo test --test run_lock -j 1 -- --test-threads=1`

```bash
git add src/managed_files.rs src/restoration.rs src/atomic_file.rs src/lock.rs tests/managed_files_transaction.rs tests/managed_files_lock.rs
git commit -m "feat: apply managed file transactions"
```

### Task 10: Prepared-journal crash recovery

**Files:**
- Modify: `src/restoration.rs`
- Modify: `src/managed_files.rs`
- Create: `tests/managed_files_recovery.rs`

**Interfaces:**

```rust
pub(crate) enum RecoveryClassification {
    NotRequired,
    Recoverable(RecoveryPlan),
    Conflict,
}

pub(crate) fn classify_prepared(
    repository: &HeldDirectory,
    manifest: &RestorationManifest,
) -> Result<RecoveryClassification, Reason>;

pub(crate) fn recover_prepared(
    repository: &HeldDirectory,
    metadata: &HeldDirectory,
    plan: &RecoveryPlan,
) -> Result<(), Reason>;
```

- [ ] **Step 1: Generate prepared fixtures for every before/target combination** across config and AGENTS. Assert recovery is allowed only when each destination equals one of those two recorded states.

- [ ] **Step 2: Test conflicting third states** for edited managed config, edited block, changed separator, missing expected file, duplicate marker, and surrounding-digest mismatch. Expect code 78 and byte-for-byte no change.

- [ ] **Step 3: Test command behavior**: both dry runs report `recovery-required` with code 0, reason `managed-file-conflict`, and zero writes; init rolls back, replans, then may apply; restore rolls back then restores the recovered previous applied state.

- [ ] **Step 4: Confirm RED**

Run: `cargo test --test managed_files_recovery -j 1 -- --test-threads=1`

- [ ] **Step 5: Implement classification and recovery** without full-file historical AGENTS bytes. Edit only a recognized intended managed span or a config matching a journal digest.

- [ ] **Step 6: Re-run failure injection followed by next-command recovery** for every Task 9 fault boundary.

- [ ] **Step 7: Verify and commit**

Run: `cargo test --test managed_files_recovery -j 1 -- --test-threads=1`

Run: `cargo test --test managed_files_transaction -j 1 -- --test-threads=1`

```bash
git add src/restoration.rs src/managed_files.rs tests/managed_files_recovery.rs
git commit -m "feat: recover interrupted managed changes"
```

### Task 11: Bounded restore, fresh-clone fallback, and force boundary

**Files:**
- Modify: `src/restoration.rs`
- Modify: `src/managed_files.rs`
- Modify: `src/agents_policy.rs`
- Create: `tests/managed_files_restore.rs`

**Interfaces:**

```rust
pub fn plan_restore(
    start: &Path,
    request: &RestoreRequest,
) -> Result<ManagedFilesPlan, ManagedFilesFailure>;

pub fn execute_restore(
    start: &Path,
    runtime: &Path,
    request: &RestoreRequest,
) -> ManagedFilesOutcome;
```

- [ ] **Step 1: Add applied-journal restore tests** for managed config deletion, external config preservation, current-target verification, block/separator removal, AGENTS deletion only when created and empty, unrelated prefix/suffix edits, journal deletion after proof, and private-directory removal only when empty/owned/mode `0700`.

- [ ] **Step 2: Add stable-baseline tests** proving repeated init updates retain config baseline `absent`, while external ownership remains external for the journal lifetime.

- [ ] **Step 3: Add force-boundary tests**: an edited but structurally complete single block is removable only with `--force-managed-block`; duplicate, nested, incomplete, unsupported format, config edits, and recovery conflicts remain unforceable.

- [ ] **Step 4: Add no-journal fresh-clone tests** for valid self-hashed block removal, exact deterministic config removal, non-reproducible config preservation with issue, empty AGENTS cleanup, no init host gate, and complete idempotency.

- [ ] **Step 5: Confirm RED**

Run: `cargo test --test managed_files_restore -j 1 -- --test-threads=1`

- [ ] **Step 6: Implement restore as its own journaled transaction**

Use Plan A/B and the same lease. Write a prepared restore journal before repository edits. Verify terminal repository state before deleting the journal. A handled failure rolls back to the immediate pre-restore managed state.

- [ ] **Step 7: Verify and commit**

Run: `cargo test --test managed_files_restore -j 1 -- --test-threads=1`

Run: `cargo test --test managed_files_recovery -j 1 -- --test-threads=1`

```bash
git add src/restoration.rs src/managed_files.rs src/agents_policy.rs tests/managed_files_restore.rs
git commit -m "feat: restore managed repository files"
```

### Task 12: Activate CLI orchestration and output contracts

**Files:**
- Modify: `src/main.rs`
- Modify: `src/managed_files.rs`
- Modify: `src/terminal.rs`
- Create: `tests/managed_files_cli.rs`
- Create: `tests/managed_files_dry_run.rs`

**Interfaces:**

```rust
pub struct ManagedFilesOutcome {
    pub report: ManagedFilesReport,
    pub human_diff: Option<String>,
}

pub fn render_managed_human(outcome: &ManagedFilesOutcome) -> String;

pub fn stable_managed_files_line(report: &ManagedFilesReport) -> String;
```

- [ ] **Step 1: Add end-to-end command tests** for all accepted CLI forms, exit-code mapping, one stable final stderr line, JSON-only stdout, no ANSI in JSON/stable line, and branding only in human apply output.

Include the supported-but-unvalidated Mac notice for `init`; prove `restore` emits no host-gate failure on the same injected host report.

- [ ] **Step 2: Add exact human diff goldens** showing only `.agent-lowmem.json` and the managed AGENTS span. Assert external configuration bytes, exterior Markdown beyond minimal locating context, private manifest identity/path, raw scripts, and absolute paths never appear.

- [ ] **Step 3: Add dry-run sentinel tests** proving neither command creates a runtime directory, lock, Git-private directory, temp file, managed file, or child-process marker.

- [ ] **Step 4: Add output-failure precedence tests**: pre-write report failure returns 70; post-write stdout failure preserves the repository outcome and emits one redacted warning; no output failure retries writes.

- [ ] **Step 5: Confirm RED**

Run: `cargo test --test managed_files_cli -j 1 -- --test-threads=1`

Run: `cargo test --test managed_files_dry_run -j 1 -- --test-threads=1`

- [ ] **Step 6: Dispatch `Init` and `Restore` in `main`**

Compute current directory, avoid `runtime_directory()` for dry-run, catch panics at the command boundary, render the immutable report once, and emit exactly:

```text
agent-lowmem: managed-files command=<init|restore> outcome=<outcome> code=<code> reason=<reason>
```

- [ ] **Step 7: Verify all CLI regressions and commit**

Run: `cargo test --test managed_files_cli -j 1 -- --test-threads=1`

Run: `cargo test --test managed_files_dry_run -j 1 -- --test-threads=1`

Run: `cargo test --test run_cli -j 1 -- --test-threads=1`

Run: `cargo test --test doctor_cli -j 1 -- --test-threads=1`

```bash
git add src/main.rs src/managed_files.rs src/terminal.rs tests/managed_files_cli.rs tests/managed_files_dry_run.rs
git commit -m "feat: activate managed file commands"
```

### Task 13: Phase 4 doctor capabilities

**Files:**
- Modify: `src/doctor.rs`
- Modify: `tests/doctor_cli.rs`
- Modify: `tests/doctor_budget.rs`

**Interfaces:**

```rust
pub struct DoctorReport {
    // existing fields remain
    pub init_available: bool,
    pub restore_available: bool,
}
```

- [ ] **Step 1: Add doctor decision-table tests**

`initAvailable` requires runtime-supported host plus supported repository inspection. `restoreAvailable` requires a Git root plus a managed destination or journal identity, including conflicting state, and never requires the init host gate.

- [ ] **Step 2: Update output contract tests** for phase `managed-files`, retained `managedRunsAvailable`, retained four-state lock, both new booleans, and next action `design the release and distribution phase`.

- [ ] **Step 3: Add zero-child and zero-write doctor regression** with sentinel executables and absent runtime/private directories.

- [ ] **Step 4: Confirm RED**

Run: `cargo test doctor::tests -j 1 -- --test-threads=1`

Run: `cargo test --test doctor_cli -j 1 -- --test-threads=1`

- [ ] **Step 5: Implement data-only capability assembly** using the managed destination classifier in inspection mode; do not construct a mutating plan or call `runtime_directory()` merely to determine init/restore availability.

- [ ] **Step 6: Verify and commit**

Run: `cargo test doctor -j 1 -- --test-threads=1`

Run: `cargo test --test doctor_cli -j 1 -- --test-threads=1`

```bash
git add src/doctor.rs tests/doctor_cli.rs tests/doctor_budget.rs
git commit -m "feat: report managed file capabilities"
```

### Task 14: Phase 4 convergence, security, resources, and evidence

**Files:**
- Modify: `tests/doctor_cli.rs`
- Create: `tests/managed_files_budget.rs`
- Modify: `docs/dependencies-v1.md`
- Modify: `docs/superpowers/plans/2026-09-03-agent-lowmem-phase-4-managed-files.md`

**Interfaces:**
- Produces: complete sequential evidence for every Phase 4 acceptance criterion and checks off this plan only after observed success.
- Preserves: all Phase 1–3 tests, schemas, runner output, signal cleanup, lock behavior, dependency set, binary/RSS budgets, and source boundaries.

- [ ] **Step 1: Strengthen source and dependency guards**

Fail if production sources add `std::process::Command` outside `src/process.rs`, Git/npm/pnpm config invocations, shell evaluators, unsafe blocks, network/async/runtime crates, process enumeration, raw absolute-path serialization, or environment-value persistence.

- [ ] **Step 2: Run formatting, lint, and complete tests sequentially**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --all-targets -j 1 -- -D warnings`

Run: `cargo test -j 1 -- --test-threads=1`

Run: `git diff --check`

- [ ] **Step 3: Validate both JSON Schemas against positive and negative fixtures**

Run: `cargo test --test managed_files_report -j 1 -- --test-threads=1`

Run: `cargo test --test restoration_manifest -j 1 -- --test-threads=1`

- [ ] **Step 4: Run locked dependency and advisory audits**

Run: `cargo metadata --locked --format-version 1`

Run: `cargo audit --deny warnings --file Cargo.lock`

Confirm `git diff -- Cargo.toml Cargo.lock` contains no Phase 4 dependency delta. If unrelated local manifest edits exist, record and exclude them rather than staging them.

- [ ] **Step 5: Build release and run retained resource gates**

Run: `cargo build --release -j 1`

Run: `cargo test --release --test doctor_budget -j 1 -- --ignored --test-threads=1 --nocapture`

Run: `cargo test --release --test run_budget -j 1 -- --ignored --test-threads=1 --nocapture`

Run: `stat -f '%z bytes' target/release/agent-lowmem`

Expected: stripped binary at most 12 MiB, parent RSS at most 24 MiB, and existing doctor/run timing and wakeup gates remain green.

- [ ] **Step 6: Measure Phase 4 warm-cache behavior**

Run: `cargo test --release --test managed_files_budget -j 1 -- --ignored --test-threads=1 --nocapture`

Record median and p95 for `init --dry-run`, `restore --dry-run`, unchanged `init`, and unchanged `restore` on the reference fixture and host. Assert the test creates no child and leaves the lease available; record values as reference evidence, not cross-host guarantees.

- [ ] **Step 7: Audit privacy, permissions, recovery, and idempotency**

Run focused tests for redaction, `0600`/`0700` under both umasks, Plan A/B drift, every fault boundary, next-command recovery, exterior Markdown preservation, fresh-clone fallback, force limits, repeated init, and repeated restore.

- [ ] **Step 8: Record exact evidence**

Append commit under test, Rust version, macOS/hardware reference profile, test totals, schema results, dependency graph status, advisory result, binary bytes, peak RSS, medians/p95s, and every gate command to `docs/dependencies-v1.md`.

- [ ] **Step 9: Independently compare implementation against all 15 acceptance criteria** in the Phase 4 spec. Search the plan and code for unresolved placeholder text and verify every named type/function is consistent with its callers.

- [ ] **Step 10: Mark completed checkboxes only from observed evidence and commit**

```bash
git add tests/doctor_cli.rs tests/managed_files_budget.rs docs/dependencies-v1.md docs/superpowers/plans/2026-09-03-agent-lowmem-phase-4-managed-files.md
git commit -m "docs: record phase 4 managed file evidence"
```

- [ ] **Step 11: Final repository proof and publication**

Run: `git status --short --branch`

Run: `git log --oneline --decorate -15`

Run: `git push origin main`

Expected: only explicitly preserved pre-existing user changes may remain unstaged; every Phase 4 commit is on `origin/main`; no development server, child fixture, temporary journal, runtime lock, or orphaned test process remains.

## Acceptance Coverage Matrix

| Spec acceptance criterion | Primary implementation tasks | Proof |
| --- | --- | --- |
| 1. Exact CLI grammar | 1, 12 | parser tables and executable CLI tests |
| 2. Deterministic zero-write dry-run | 8, 12, 14 | plan goldens plus filesystem/child sentinels |
| 3. Runnable canonical operations only | 3, 8 | root/workspace policy tables |
| 4. External config preservation | 8, 11 | ownership and restore tables |
| 5. One managed block, exterior unchanged | 4, 8 | byte-span goldens |
| 6. Ambiguity fails closed; force is narrow | 4, 11 | malformed/edited/force tables |
| 7. Lease plus Plan A/B revalidation | 9, 11 | drift and contention tests |
| 8. Atomic, journaled mutations | 6, 7, 9 | durability and fault-injection tests |
| 9. Rollback or recoverable prepared state | 9, 10 | every durable fault boundary |
| 10. Restore owns only proven bytes | 10, 11 | prefix/suffix and ownership tests |
| 11. Journal/output privacy | 5, 6, 14 | schema and sentinel-secret tests |
| 12. Byte-for-byte idempotency | 9, 11, 14 | repeated init/restore snapshots |
| 13. No inspection child process | 8, 12, 13, 14 | PATH sentinel executables |
| 14. Phase 1–3 regressions remain green | 7, 12, 13, 14 | full sequential suite and resource gates |
| 15. File and behavior scope is bounded | all; audited in 14 | source/dependency diff and final spec comparison |

## Implementation Checkpoints

1. **After Task 4:** generated bytes and marker semantics are independently reviewable; no write path exists.
2. **After Task 8:** Plan A is complete and dry-run semantics can be reviewed; no repository mutation is reachable.
3. **After Task 9:** init is transaction-safe internally but remains undispatched until output contracts are complete.
4. **After Task 11:** restore, crash recovery, and force scope converge before any public CLI activation.
5. **After Task 13:** Phase 4 behavior is reachable and doctor reports the new boundary.
6. **After Task 14:** resource, security, privacy, schema, and regression evidence authorizes the Phase 4 completion claim.

## Stop Conditions

Stop the current task and resolve the design boundary before proceeding if any of these occurs:

- a new production dependency appears necessary;
- safe descriptor-relative APIs cannot uphold no-follow, exact-precondition, mode, or durability requirements;
- recovery would need complete historical `AGENTS.md` bytes;
- restore cannot distinguish external from managed configuration;
- implementation needs a new `Reason`, a Phase 3 schema change, or a broader force flag;
- a dry-run or inspection test starts a child or writes any identity;
- an unrelated local change overlaps a file that the current task must edit;
- a full gate exceeds the 8 GiB Mac resource policy even with one Cargo job and one test thread.
