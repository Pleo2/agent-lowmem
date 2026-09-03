# Agent Lowmem Phase 4 Managed Files Design

**Date:** 2026-09-03
**Status:** Approved in conversation; written-spec review pending
**Scope:** Add deterministic, reversible repository onboarding through `init`, `init --dry-run`, `restore`, and `restore --dry-run`. Phase 4 manages only `.agent-lowmem.json`, one delimited `AGENTS.md` block, and one private restoration journal.

## 1. Objective

Phase 4 makes the Phase 3 runner adoptable by humans and coding agents without requiring them to hand-write policy. It converts the existing data-only repository inspection into a deterministic managed-file plan, previews exact repository changes without writing, applies those changes under the global Agent Lowmem lease, and restores only bytes that Agent Lowmem can prove it owns.

The design is deliberately conservative:

- inspection starts no child process;
- manual configuration is preserved byte for byte;
- malformed or ambiguous managed state fails closed;
- repository writes are atomic per file and recoverable as one journaled transaction;
- no command guesses, semantic merges, or broad force flags enter v1.

## 2. Scope boundary

Phase 4 includes:

- strict CLI parsing for `init` and `restore`;
- canonical root and workspace policy discovery for npm and pnpm repositories;
- deterministic `.agent-lowmem.json` generation;
- one versioned and hashed Agent Lowmem block in root `AGENTS.md`;
- exact human dry-run output and a versioned redacted JSON report;
- a private crash-recovery and restoration journal in resolved Git metadata;
- global-lease serialization and post-lock evidence revalidation;
- symlink-safe, component-relative, permission-controlled writes;
- idempotent re-application and narrowly bounded restore behavior.

Phase 4 does not include:

- support for package managers or languages outside the existing npm/pnpm JavaScript and TypeScript contract;
- agent instruction formats other than `AGENTS.md`;
- semantic JSON or Markdown merging;
- arbitrary script selection or command execution;
- additional force flags;
- memory-pressure enforcement, process enumeration, retries, daemons, or telemetry;
- CI, installation, packaging, signing, notarization, Homebrew, npm distribution, or website work.

## 3. User-facing commands

The accepted grammar is exactly:

```text
agent-lowmem init [--dry-run] [--json]
agent-lowmem restore [--dry-run] [--force-managed-block] [--json]
```

`--dry-run`, `--json`, and `--force-managed-block` may appear in either order where allowed, each at most once. `--force-managed-block` is valid only for `restore`. Unknown, abbreviated, duplicated, non-UTF-8, or positional arguments return code 2 with `invalid-cli`.

There are no interactive prompts. Running `init` without `--dry-run` authorizes only the exact plan recomputed after lock acquisition. Running `restore --force-managed-block` explicitly authorizes removal of one well-formed Agent Lowmem block whose body hash no longer matches its marker; it authorizes nothing else.

`init` requires the existing supported macOS arm64 runtime gate. A supported Mac outside the reference performance profile may initialize policy and receives the existing unvalidated-performance notice. `restore` bypasses host capability and performance-profile gates so managed state can be removed wherever the installed binary is executable.

## 4. Architecture

Phase 4 adds four bounded responsibilities:

1. **Managed-file discovery** reads Git metadata, repository manifests, existing managed destinations, and any private journal as data.
2. **Managed-file planning** produces an immutable `ManagedFilesPlan` with exact evidence digests, destination preconditions, actions, target bytes, and a redacted public report.
3. **Managed-file application** revalidates the plan under the global lease and applies atomic per-file changes through a private prepared journal.
4. **Restoration** validates current owned bytes against the applied journal or the narrow fresh-clone fallback, then removes or restores only the owned spans.

The data flow is:

```text
data-only inspection
        |
        v
ManagedFilesPlan A ----> dry-run report (zero writes)
        |
        v
global lease
        |
        v
ManagedFilesPlan B -- exact comparison --> prepared private journal
                                                |
                                                v
                                  config write -> AGENTS write
                                                |
                                                v
                                      applied private journal
```

The transaction is crash-recoverable, not falsely described as one multi-file atomic rename. Each individual replacement is atomic. The private journal makes partially applied combinations detectable and reversible.

## 5. Repository and Git metadata discovery

The existing parent walk remains the authority for the Git root. A `.git` directory is the metadata directory for an ordinary checkout. A valid `.git` pointer file is resolved using the existing bounded worktree grammar and containment rules. Phase 4 never executes `git`.

The private directory is `agent-lowmem` below the resolved per-worktree Git metadata directory. The journal identity is `restoration-v1.json`. Public reports call it only `restoration-manifest`; they never print its absolute or Git-relative path.

Discovery rejects:

- missing Git root for either command;
- missing root `package.json` for `init`;
- symlinked, non-regular, non-UTF-8, oversized, duplicate-marker, or structurally invalid managed destinations;
- a symlink or non-directory at the private directory identity;
- a symlink, special file, wrong owner, wrong mode, or invalid schema at the journal identity.

Reads are bounded before allocation: `.agent-lowmem.json` and the private journal are each limited to 262,144 bytes; `AGENTS.md` is limited to 1,048,576 bytes. The generated managed block is limited to 65,536 bytes, generation admits at most 128 workspaces, and the public candidate list admits at most 256 entries. Exceeding a destination limit returns `managed-file-conflict`; exceeding a discovery/cardinality limit returns `workspace-unsupported`. Nothing is silently truncated.

`init --dry-run` and `restore --dry-run` do not create the private directory, runtime directory, lock file, temporary file, or any repository file.

## 6. Canonical policy discovery

Generation uses the Phase 2 parser, bounded script graph, adapter matrix, package-manager evidence, workspace cardinality, and no-child inspection boundary. It does not introduce a second classifier.

The canonical operation names are exactly:

| Operation key | Exact script name | Default timeout |
| --- | --- | --- |
| `test` | `test` | 900 seconds |
| `typecheck` | `typecheck` | 900 seconds |
| `lint` | `lint` | 900 seconds |
| `build` | `build` | 1,800 seconds |

An operation is generated only when the exact script exists and the current policy engine classifies its complete reachable graph as runnable. Rejected canonical scripts are reported with their closed reason and are omitted from generated bytes.

Compatible exact script names beginning with `test:`, `typecheck:`, `lint:`, or `build:` are reported as manual candidates. They are never silently aliased to a canonical operation key.

The effective configuration must expose at least one currently runnable operation across the root and admitted workspaces. Otherwise init returns code 64 with `operation-unsupported` and writes nothing.

### 6.1 Workspaces

The planner considers only workspaces admitted by the existing npm or pnpm workspace parser. A workspace is generated only when it contains at least one runnable canonical operation.

Its proposed key is the final package-name component with an optional npm scope removed. For example, `@cobrix/web` proposes `web`. The proposed value must already satisfy `[a-z][a-z0-9-]{0,31}`; Phase 4 does not normalize punctuation, truncate, number, or hash a key.

Every generated key must be unique across admitted workspaces. A missing valid key or collision omits the affected workspace from generated bytes and reports `workspace-cardinality` plus the exact package name and relative workspace identity already allowed by the repository report. It does not block safe root operations or unrelated workspaces. A human may add an exact stable mapping manually.

Generated root operations sort by operation key. Generated workspaces sort by workspace key, and their operations sort by operation key.

## 7. Configuration ownership and generation

Generated `.agent-lowmem.json` uses:

- schema URL `https://agentlowmem.dev/schema/v1.json`;
- version `1`;
- the detected exact package-manager kind;
- two-space JSON indentation;
- LF line endings and one final newline;
- no timestamp, username, absolute path, host information, or generated comment.

The configuration decision table is:

| Current state | `init` action |
| --- | --- |
| Absent | Create deterministic configuration and mark it managed |
| Exactly equal to deterministic output, no journal | Adopt as managed and leave bytes unchanged |
| Valid, runnable, and different from deterministic output | Treat as external; preserve its exact bytes and build the AGENTS block from its configured operations |
| Previously managed and equal to the applied journal target | Replace only when deterministic output changed; otherwise unchanged |
| Previously managed but edited | Conflict 78; never overwrite |
| Invalid or containing a rejected configured operation | Fail before all writes with the existing configuration or policy reason |
| Symlink, special file, or non-UTF-8 | Conflict 78 |

An external configuration remains external for the life of that journal. Re-running `init` may accept new manual bytes only after they parse and every configured operation is currently runnable. `restore` never deletes or rewrites external configuration.

A managed configuration retains the original baseline `absent` state across idempotent updates. `restore` deletes it only when its current bytes equal the journal's latest target digest and bytes. There is no forced configuration removal.

## 8. Managed `AGENTS.md` block

The body is generated from a fixed format version and the operations actually present in the effective configuration. It contains the resource rules established by the v1 design and exact examples of supported root and workspace commands. It never contains raw package scripts, forwarded arguments, absolute paths, timestamps, environment values, or rejected candidates.

The marker form is:

```markdown
<!-- agent-lowmem:start format="1" content-sha256="<lowercase-sha256>" -->
## Agent Lowmem resource policy

<deterministic body>
<!-- agent-lowmem:end -->
```

The digest covers the exact UTF-8 body bytes between the marker newlines. Generated bytes use LF. The complete start marker, body, end marker, and any separator inserted solely for placement form the managed span recorded by the journal.

The decision table is:

| Current state | `init` action |
| --- | --- |
| `AGENTS.md` absent | Create it with one managed block |
| File present with no marker | Append one block with a deterministic separator; preserve all prior bytes |
| Exactly one valid Agent Lowmem block | Replace only that managed span; preserve prefix and suffix byte for byte |
| Exact desired block | Unchanged |
| Duplicate, nested, incomplete, unsupported-format, or hash-invalid marker | Conflict 78; write nothing |
| Symlink, special file, non-UTF-8, or over the managed-file size limit | Conflict 78 |

When updating a block that predates the local journal, it is adopted as Agent Lowmem-owned. Its restoration baseline is absent because the versioned markers are the ownership proof. This matches the fresh-clone restoration contract.

Phase 4 uses bounded byte scanning, not a CommonMark parser. Marker-looking text elsewhere is therefore intentionally treated as managed-state ambiguity and fails closed.

## 9. Managed plan and evidence

`ManagedFilesPlan` contains exactly these categories of private and public planning data:

- command kind and dry-run state;
- Git-root and resolved-metadata identities held privately;
- the supported host result for `init`;
- package manager and effective configuration ownership;
- sorted canonical operations, workspace mappings, disclosures, and manual candidates;
- exact source evidence snapshots used by the Phase 2 policy engine;
- exact precondition snapshots for `.agent-lowmem.json`, `AGENTS.md`, and the private journal;
- target bytes and SHA-256 digests for every planned write;
- a closed action for each identity: `create`, `replace`, `remove`, `unchanged`, `preserve`, or `conflict`;
- the transaction's immediate rollback descriptors and stable restoration baseline.

Plans compare exact command, evidence identities, digests, ownership, actions, and target bytes. Path display strings, map iteration order, modification times, and terminal capability do not participate.

Dry-run returns Plan A directly. Applying commands acquire the existing per-user global lease with operation key `init` or `restore`, compute Plan B from the Git root, and require exact Plan A/Plan B equality. Drift returns code 75 with `evidence-changed` and starts no write transaction.

Neither planning pass starts a repository child.

## 10. Private journal and transaction state

The private journal schema is version 1 and has exactly two durable states:

- `prepared`: the intended transaction may be partially applied;
- `applied`: the recorded latest managed targets are fully installed.

It records only:

- schema and format versions;
- a deterministic transaction digest;
- managed/external ownership flags;
- prior applied journal state when updating;
- immediate pre-transaction managed bytes or absence needed for rollback;
- intended managed bytes and digests;
- `AGENTS.md` managed-span placement metadata and surrounding digests;
- destination action states.

It does not store an absolute repository path, username, environment value, manual configuration bytes, or `AGENTS.md` content outside the managed span. Repository and transaction identity use SHA-256 digests.

The apply order is fixed:

1. validate all destination descriptors and Plan B;
2. create the private directory if absent with mode `0700`;
3. atomically write a mode-`0600` prepared journal and sync it;
4. atomically create or replace `.agent-lowmem.json` when owned by the transaction;
5. atomically create or replace `AGENTS.md` when needed;
6. verify the exact installed target bytes through held directory descriptors;
7. atomically replace the journal with its applied state and sync it;
8. release the global lease.

Existing regular repository files retain no executable bit. New managed repository files use mode `0600` as required by the v1 security contract. Atomic replacement happens within the destination directory. Temporary identities are unpredictable, opened with exclusive creation and no-follow semantics, and removed on handled failure.

If a handled error occurs after the prepared journal, the applier attempts rollback to the immediate pre-transaction managed states and restores the prior applied journal or removes the first-init journal. A failed rollback preserves the prepared journal and returns code 70. It never claims a completed transaction.

## 11. Crash recovery

A discovered prepared journal blocks ordinary planning until its state is classified.

For each managed destination, current bytes must equal either the recorded immediate-before state or the intended target state. If all destinations satisfy that condition:

- `--dry-run` reports `recovery-required` and performs zero writes;
- `init` rolls every destination back to its immediate-before state, restores the previous applied journal if present, replans from fresh evidence, and only then may begin a new transaction;
- `restore` rolls back the incomplete transaction and then computes restoration from the recovered prior applied state.

If any destination equals neither recorded state, recovery returns code 78 with `managed-file-conflict` and changes nothing. Phase 4 has no force option for transaction recovery.

Automatic recovery edits only a currently recognized intended managed span or a managed configuration whose digest matches the journal. It never restores an entire historical `AGENTS.md` file and therefore never erases unrelated edits made after a crash.

## 12. Restore behavior

With an applied journal, restore:

1. verifies the current managed configuration or external-preserve state;
2. locates exactly one current Agent Lowmem block;
3. verifies its latest target body hash and managed-span metadata;
4. plans removal of the managed configuration only when its stable baseline is absent;
5. plans removal of the managed block and its recorded inserted separator;
6. removes `AGENTS.md` only when Agent Lowmem created the file and no non-managed bytes remain;
7. deletes the applied journal only after repository restoration is proven complete;
8. removes the private directory only when empty and still owned with mode `0700`.

Unrelated prefix and suffix edits in `AGENTS.md` are preserved. An edited current managed block conflicts unless `--force-managed-block` is present. The force flag removes exactly one structurally complete start-to-end block and its provably recorded separator. It does not accept duplicate, nested, incomplete, or unsupported-format markers and never forces configuration deletion.

Without a private journal, the fresh-clone fallback may:

- remove exactly one structurally valid Agent Lowmem block whose body matches its own marker hash;
- remove `.agent-lowmem.json` only when it exactly equals deterministic output from current repository evidence;
- remove an empty `AGENTS.md` created by removing the block;
- preserve every other byte and file.

If current evidence cannot reproduce the configuration exactly, restore preserves it and reports that manual review is required. `--force-managed-block` does not change that rule.

Restore uses the same prepared/applied journal protocol for its own writes. Its applied terminal state is absence of the restoration journal after the repository state is verified.

## 13. Human and JSON output

Human dry-run output uses only the relative public identities `.agent-lowmem.json` and `AGENTS.md`. For generated or previously managed content it shows the exact replacement bytes as a unified diff. It never prints external configuration bytes, `AGENTS.md` bytes outside the minimal diff context necessary to locate the managed span, the private manifest path, or absolute paths.

`--json` writes one JSON document to stdout and no human formatting. Diagnostics and the stable final status line use stderr. JSON and the stable line never contain ANSI escapes.

The managed-files JSON schema is separate from the Phase 3 run-result schema. Its required top-level fields are:

```text
schemaVersion: 1
command: init | restore
dryRun: boolean
outcome: planned | applied | restored | unchanged | recovery-required | conflict | failed
result: { code, reason }
files: [{ identity, action, beforeSha256?, targetSha256? }]
operations: [{ operationKey, workspaceKey? }]
manualCandidates: [{ operationPrefix, scriptName, workspaceKey? }]
issues: [{ reason, operationKey?, workspacePath?, packageName? }]
manifestState: absent | prepared | applied
```

`identity` is limited to `configuration`, `agents-policy`, and `restoration-manifest`. Hashes are lowercase SHA-256. The report omits target content, manual configuration bytes, raw package scripts, absolute paths, environment values, timestamps, usernames, transaction IDs, and Git metadata paths.

Human and JSON output use the same immutable public report; rendering cannot change the plan.

The final stderr line is exactly:

```text
agent-lowmem: managed-files command=<init|restore> outcome=<outcome> code=<code> reason=<reason>
```

It is emitted once, remains unstyled, and is the only stable line-oriented managed-files contract. A successful JSON report uses `{ "code": 0, "reason": "completed" }` inside its separate managed-files schema; it does not construct a Phase 3 `ExitResult` with a false child origin.

After Phase 4, `doctor` reports phase `managed-files`, retains `managedRunsAvailable` and the four-state global lock, adds `initAvailable` and `restoreAvailable`, and starts no child. `initAvailable` requires the init host gate plus a supported repository inspection. `restoreAvailable` requires a Git root and the presence of a managed destination or journal identity, including a conflicting identity that restore can only report, but not the init host gate. Its next action becomes design of the release/distribution phase.

## 14. Exit behavior

Phase 4 reuses the closed v1 reason vocabulary and does not alter the Phase 3 run-result schema.

| Code | Managed-file meaning |
| --- | --- |
| `0` | Plan produced, transaction applied, restoration completed, or state already unchanged |
| `2` | Invalid CLI or invalid external configuration |
| `64` | Unsupported host for init, repository, package manager, workspace, operation, script, wrapper, tool, or shell policy |
| `73` | Global lease held or nested Agent Lowmem invocation |
| `75` | Evidence changed between planning and locked revalidation |
| `78` | Managed-file conflict or recovery state that cannot be safely resolved |
| `70` | Internal filesystem, serialization, durability, or rollback failure |

Success uses `completed`. Recoverable prepared state reported by dry-run uses `managed-file-conflict` with outcome `recovery-required`. No new reason token is added in Phase 4.

Failure to emit the requested JSON report preserves the primary repository outcome when repository writes have already completed and emits a redacted warning. Before repository writes, report-construction failure returns 70. Output failure never triggers an automatic retry of repository writes.

## 15. Security invariants

Phase 4 must preserve all existing invariants and additionally guarantees:

- `doctor`, both dry-run commands, both planning passes, recovery classification, and restore inspection start zero child processes;
- only `init` and `restore` without dry-run may write;
- all destination operations are relative to already validated directory descriptors;
- symlinks and special files fail closed at every source, destination, temporary, and journal identity;
- current bytes are revalidated immediately before each mutation;
- manual configuration bytes never enter the journal, JSON output, human output, debug output, or errors;
- `AGENTS.md` bytes outside the managed span never enter the journal;
- no environment-variable values, raw command lines, absolute paths, or usernames are persisted;
- no network access, shell evaluator, async runtime, private pressure API, process enumeration, or first-party unsafe block is added;
- rollback and restore never follow a path discovered after validation;
- deletion requires exact ownership and byte evidence; absence of proof means preservation.

The repository remains trusted data and trusted code for later `run`, but Phase 4 never executes it.

## 16. Testing strategy

Implementation follows test-driven development and includes:

1. strict parser tables for every accepted ordering and every rejected duplicate, positional, non-UTF-8, or cross-command flag;
2. golden deterministic root and workspace configurations for npm and pnpm;
3. golden managed blocks with independently calculated body hashes;
4. canonical-operation inclusion, rejected-operation omission, prefixed candidate reporting, invalid workspace key, and collision cases;
5. external valid configuration preservation and rejection of invalid or unrunnable manual mappings;
6. byte-for-byte idempotency across repeated dry-run and apply;
7. dry-run sentinels proving no runtime directory, lock, private Git directory, temporary file, managed file, or child process is created;
8. exact Plan A/Plan B mutation barriers for every source and managed destination;
9. configuration and AGENTS decision-table coverage for absent, exact, external, edited, malformed, duplicated, symlinked, special, non-UTF-8, and oversized states;
10. failure injection after prepared journal, configuration write, AGENTS write, verification, and applied-journal replacement;
11. automatic rollback and next-command recovery for every injected boundary;
12. recovery conflicts when a destination equals neither immediate-before nor target bytes;
13. restore preserving concurrent prefix and suffix edits outside the managed block;
14. exact fallback and forced-block behavior without a private journal;
15. `0600` file and `0700` directory modes under permissive and restrictive umasks;
16. JSON schema, closed enums, redaction, no ANSI, and output-failure precedence;
17. one-worker integration tests proving the global lease blocks concurrent init, restore, and run;
18. complete regression of Phase 1 through Phase 3 tests and source boundaries.

Release gates retain the 12 MiB stripped-binary and 24 MiB parent-RSS limits. Warm-cache dry-run and unchanged init/restore measurements are recorded on the reference 8 GiB Mac; they do not become portability claims.

## 17. File ownership

Expected production additions are:

- `src/managed_files.rs`: public planning types, action model, redacted report, and orchestration;
- `src/agents_policy.rs`: bounded marker parser, deterministic renderer, and managed-span edits;
- `src/restoration.rs`: private journal schema, state machine, rollback, recovery, and restore planning;
- `src/atomic_file.rs`: shared component-relative atomic create, replace, remove, sync, ownership, mode, and no-follow primitives;
- `schemas/managed-files-result-v1.schema.json`: public init/restore report contract;
- `schemas/restoration-manifest-v1.schema.json`: private journal schema used by tests and implementation.

Expected modifications are limited to CLI dispatch, configuration serialization, repository inspection reuse, doctor capability reporting, result guidance, exports, focused fixtures, tests, and Phase 4 dependency/evidence documentation.

No production dependency is assumed by this design. An implementation plan must first determine whether existing `std`, `rustix`, `serde`, `serde_json`, and `sha2` APIs satisfy every atomic-write and durability requirement. Any dependency proposal requires a separate source, license, MSRV, transitive-graph, size, and security review before production code uses it.

## 18. Acceptance criteria

Phase 4 is complete only when:

1. every accepted CLI form and rejection is covered by tests;
2. dry-run produces exact deterministic plans and performs zero writes or child starts;
3. generated configuration contains only runnable canonical operations and stable non-colliding workspace keys;
4. external valid configuration is preserved byte for byte and invalid configuration blocks all writes;
5. one valid managed block is inserted or replaced without changing exterior bytes;
6. ambiguous or edited managed state fails closed except for the exact forced-block restore boundary;
7. init and restore revalidate all evidence under the global lease;
8. each file mutation is atomic and the multi-file transaction is recoverable through the private journal;
9. every injected failure either proves complete rollback or leaves a prepared journal that the next command classifies safely;
10. restore changes only proven Agent Lowmem-owned bytes and preserves unrelated edits;
11. journal and public output satisfy the privacy and redaction invariants;
12. repeated init and restore are byte-for-byte idempotent;
13. no inspection path starts Git, Node, npm, pnpm, a package binary, or a repository script;
14. existing run behavior, signal cleanup, result schema, resource limits, and source audits remain green;
15. the implementation plan remains within the file and behavior boundaries listed here.

## 19. Deferred work

The following require later designs:

- configuration merge assistance or an explicit config-adoption workflow;
- compatibility blocks for other coding-agent formats;
- CI generation and remote execution;
- installers, release channels, signing, notarization, provenance, and npm launchers;
- memory-pressure behavior and any automatic scheduling beyond the existing global lease;
- policy format version 2 or result reason/schema changes.
