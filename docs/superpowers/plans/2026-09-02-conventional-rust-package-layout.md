# Conventional Rust Package Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the only production Rust package to Cargo's conventional root `src/` and `tests/` layout without changing behavior, dependencies, or release scope.

**Architecture:** The repository root becomes the `agent-lowmem` package root and owns the single package manifest, lockfile, release profile, production sources, and integration tests. The Swift pressure probe remains an independent research tool under `tools/pressure-probe`; a Cargo workspace is deferred until a second Rust package has a real build, ownership, or distribution boundary.

**Tech Stack:** Rust 1.85.0, Cargo edition 2024, `serde`, `serde_json`, `semver`, `sysctl` 0.7.1, Swift 6.3 pressure-probe harnesses, Git.

**Spec:** `docs/superpowers/specs/2026-09-02-rust-package-layout-design.md`

## Global Constraints

- Work directly on a clean, synchronized `main`, as explicitly requested before the first release; do not create a feature branch or worktree.
- Execute every formatter, compiler, linter, test, and measurement sequentially with one worker.
- Preserve Rust 1.85.0, edition 2024, the existing direct dependency set, and every resolved version in `Cargo.lock`.
- Preserve all Rust module contents and public behavior; this migration changes paths and manifest ownership only.
- Keep `tools/pressure-probe` outside the production package and release artifacts.
- Do not add a Cargo workspace, second Rust package, new dependency, CLI behavior, schema value, or release artifact.
- Preserve the completed Phase 1 plan as historical evidence; annotate its superseded paths instead of rewriting its recorded steps.
- Use Conventional Commits and push only after the complete Rust and Swift gate matrix passes.
- Execute inline without subagents unless the user explicitly changes the low-memory execution policy.

---

## File Structure

- `Cargo.toml`: single root package metadata, dependencies, and release profile.
- `src/lib.rs`: crate safety boundary and public module exports.
- `src/main.rs`: thin executable entry point.
- `src/cli.rs`: strict CLI parsing.
- `src/doctor.rs`: doctor report assembly and presentation.
- `src/host.rs`: native host inspection.
- `src/repository.rs`: repository and package-manager evidence inspection.
- `src/result.rs`: result codes, origins, reasons, and serialization contracts.
- `tests/doctor_cli.rs`: executable behavior, redaction, and manifest-relative source guard.
- `tests/doctor_budget.rs`: ignored release-mode resource gate.
- `docs/dependencies-v1.md`: current single-package gate commands.
- `docs/superpowers/specs/2026-09-02-agent-lowmem-v1-design.md`: current production package terminology.
- `docs/superpowers/specs/2026-09-02-rust-package-layout-design.md`: migration decision and corrected source-guard note.
- `docs/superpowers/plans/2026-09-02-agent-lowmem-phase-1-native-foundation.md`: historical path annotation only.

### Task 1: Move the Production Package to the Repository Root

**Files:**
- Modify: `Cargo.toml`
- Delete: `crates/agent-lowmem/Cargo.toml`
- Move: `crates/agent-lowmem/src/*.rs` to `src/*.rs`
- Move: `crates/agent-lowmem/tests/*.rs` to `tests/*.rs`
- Preserve: `Cargo.lock`

**Interfaces:**
- Consumes: the existing `agent-lowmem` library and binary package at `crates/agent-lowmem`.
- Produces: the same package, targets, module names, binary name, dependencies, and result contracts from the repository root.

- [ ] **Step 1: Verify the approved root-layout acceptance check is currently red**

Run:

```bash
test -f src/main.rs
```

Expected: exit 1 because production sources still live below `crates/agent-lowmem`.

Run:

```bash
cargo metadata --locked --offline --no-deps --format-version 1 \
  | jq -e '.packages[0].manifest_path == (.workspace_root + "/Cargo.toml")'
```

Expected: `false` and exit 1 because the package manifest is not yet the root manifest.

- [ ] **Step 2: Replace the virtual workspace manifest with the root package manifest**

Replace `Cargo.toml` with exactly:

```toml
[package]
name = "agent-lowmem"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "MIT"
publish = false

[dependencies]
semver = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
sysctl = "=0.7.1"

[profile.release]
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "unwind"
```

The file must contain neither `[workspace]` nor `[workspace.package]`.

- [ ] **Step 3: Move the tracked package sources and tests with Git history**

Run:

```bash
git mv crates/agent-lowmem/src src
git mv crates/agent-lowmem/tests tests
git rm crates/agent-lowmem/Cargo.toml
```

Expected: `src/` and `tests/` exist at the root, and `crates/agent-lowmem` no longer exists.

- [ ] **Step 4: Verify the root package identity and unchanged lockfile**

Run:

```bash
test -f src/lib.rs
test -f src/main.rs
test -f tests/doctor_cli.rs
test ! -e crates/agent-lowmem
cargo metadata --locked --offline --no-deps --format-version 1 \
  | jq -e '(.packages | length) == 1 and .packages[0].name == "agent-lowmem" and .packages[0].manifest_path == (.workspace_root + "/Cargo.toml") and (.workspace_members | length) == 1'
git diff --exit-code HEAD -- Cargo.lock
```

Expected: every command exits 0; Cargo reports one root package and `Cargo.lock` is byte-for-byte unchanged.

- [ ] **Step 5: Run focused Rust validation**

Run sequentially:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 -- --test-threads=1
```

Expected: formatting and Clippy pass; 22 library tests and 6 executable integration tests pass, while the release-only budget test remains ignored in the normal suite.

- [ ] **Step 6: Inspect the structural diff and commit it**

Run:

```bash
git diff --check
git status --short
git diff --stat
git diff -- Cargo.toml
```

Expected: only the manifest replacement, Git-detected source/test moves, and removal of the nested manifest are present; no module contents, dependency versions, schemas, or Swift files changed.

Commit:

```bash
git add -A -- Cargo.toml crates/agent-lowmem src tests
git commit -m "refactor: adopt conventional Rust package layout"
```

### Task 2: Align Current Documentation Without Rewriting History

**Files:**
- Modify: `docs/dependencies-v1.md`
- Modify: `docs/superpowers/specs/2026-09-02-agent-lowmem-v1-design.md`
- Modify: `docs/superpowers/specs/2026-09-02-rust-package-layout-design.md`
- Modify: `docs/superpowers/plans/2026-09-02-agent-lowmem-phase-1-native-foundation.md`
- Preserve: all original Phase 1 task bodies and recorded commands.

**Interfaces:**
- Consumes: the root package produced by Task 1 and the approved layout design.
- Produces: current architecture and gate documentation that names a single root package, plus an explicit historical-path annotation for the completed Phase 1 plan.

- [ ] **Step 1: Verify current documentation still contains superseded active terminology**

Run:

```bash
rg -n 'Rust workspace|workspace-wide|production crates|cargo (clippy|test) --workspace' \
  docs/dependencies-v1.md \
  docs/superpowers/specs/2026-09-02-agent-lowmem-v1-design.md
```

Expected: matches identify the current architecture and gate text that must be updated.

- [ ] **Step 2: Update the dependency gate commands for the single root package**

In `docs/dependencies-v1.md`, preserve every recorded metric and replace only the gate-command cell with:

```markdown
| Gate commands | `cargo fmt --all -- --check`<br>`cargo clippy --all-targets -j 1 -- -D warnings`<br>`cargo test -j 1 -- --test-threads=1`<br>`cargo build --release -j 1`<br>`cargo test --release --test doctor_budget -j 1 -- --ignored --test-threads=1 --nocapture`<br>`stat -f '%z bytes' target/release/agent-lowmem`<br>`/usr/bin/time -l target/release/agent-lowmem doctor >/dev/null`<br>`git diff --check` |
```

- [ ] **Step 3: Update the active v1 architecture terminology**

In `docs/superpowers/specs/2026-09-02-agent-lowmem-v1-design.md` make these exact semantic replacements:

```text
workspace-wide rule
→ package-wide rule

### 8.2 Rust workspace
→ ### 8.2 Rust package

The Rust workspace uses edition 2024 with Rust 1.85 as its minimum supported Rust version.
→ The root Rust package uses edition 2024 with Rust 1.85 as its minimum supported Rust version.

First-party production crates compile with `#![forbid(unsafe_code)]` in v1.
→ The first-party production package compiles with `#![forbid(unsafe_code)]` in v1.

compile all first-party production crates with `#![forbid(unsafe_code)]`;
→ compile the first-party production package with `#![forbid(unsafe_code)]`;

All first-party production crates reject `unsafe`
→ All first-party production Rust code rejects `unsafe`
```

Do not alter CLI semantics, safety rules, result vocabularies, acceptance criteria, or distribution claims.

- [ ] **Step 4: Correct the migration design's source-guard instruction**

In `docs/superpowers/specs/2026-09-02-rust-package-layout-design.md`, replace migration step 4 with:

```markdown
4. Preserve the manifest-relative source guard in `tests/doctor_cli.rs`; its
   `env!("CARGO_MANIFEST_DIR")/src` lookup follows the package move without a
   code change. Update current gate and architecture documentation to the root
   package terminology.
```

Renumber the following migration steps without changing their meaning.

- [ ] **Step 5: Annotate the completed Phase 1 plan as historical evidence**

Immediately after the title and agentic-worker note in `docs/superpowers/plans/2026-09-02-agent-lowmem-phase-1-native-foundation.md`, add:

```markdown
> **Historical layout note (2026-09-02):** This completed plan records the
> original virtual-workspace implementation paths. The production package was
> subsequently moved from `crates/agent-lowmem` to the conventional root
> `src/` and `tests/` layout by
> `docs/superpowers/specs/2026-09-02-rust-package-layout-design.md`. The task
> bodies below remain unchanged as implementation evidence.
```

- [ ] **Step 6: Verify active references and documentation integrity**

Run:

```bash
if rg -n 'crates/agent-lowmem|Rust workspace|workspace-wide|production crates|cargo (clippy|test) --workspace' \
  Cargo.toml src tests docs/dependencies-v1.md \
  docs/superpowers/specs/2026-09-02-agent-lowmem-v1-design.md; then
  exit 1
fi
rg -n 'Historical layout note.*2026-09-02' \
  docs/superpowers/plans/2026-09-02-agent-lowmem-phase-1-native-foundation.md
git diff --check
```

Expected: no superseded reference remains in active code or current architecture docs; the historical note exists; Markdown whitespace is valid.

- [ ] **Step 7: Commit the documentation alignment**

Run:

```bash
git add docs/dependencies-v1.md \
  docs/superpowers/specs/2026-09-02-agent-lowmem-v1-design.md \
  docs/superpowers/specs/2026-09-02-rust-package-layout-design.md \
  docs/superpowers/plans/2026-09-02-agent-lowmem-phase-1-native-foundation.md
git commit -m "docs: align Rust package architecture references"
```

### Task 3: Run the Full Gate Matrix and Publish Main

**Files:**
- Verify: `Cargo.toml`, `Cargo.lock`, `src/`, `tests/`, `tools/pressure-probe/`, and updated documentation.
- Modify: none; any generated build output must remain ignored.

**Interfaces:**
- Consumes: the structural and documentation commits from Tasks 1 and 2.
- Produces: a verified, clean local `main` whose exact commits are published to `origin/main`.

- [ ] **Step 1: Run the complete Rust gate matrix**

Run sequentially from the repository root:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 -- --test-threads=1
cargo build --release -j 1
cargo test --release --test doctor_budget -j 1 -- --ignored --test-threads=1 --nocapture
stat -f '%z bytes' target/release/agent-lowmem
/usr/bin/time -l target/release/agent-lowmem doctor >/dev/null
```

Expected: all gates exit 0; 28 active Rust tests and the release-only budget test pass; the release binary is produced at `target/release/agent-lowmem`.

- [ ] **Step 2: Run the complete Swift research-probe gate matrix**

Run sequentially from `tools/pressure-probe`:

```bash
swift run -j 1 pressure-probe-core-tests
swift run -j 1 pressure-probe-macos-tests
swift build -j 1 -c release
```

Expected: 7 core checks and 5 macOS checks pass; the release build completes without adding tracked artifacts.

- [ ] **Step 3: Verify the final tree and commit boundaries**

Run from the repository root:

```bash
test -f src/main.rs
test -f tests/doctor_cli.rs
test ! -e crates/agent-lowmem
cargo metadata --locked --offline --no-deps --format-version 1 \
  | jq -e '(.packages | length) == 1 and .packages[0].name == "agent-lowmem" and .packages[0].manifest_path == (.workspace_root + "/Cargo.toml") and (.workspace_members | length) == 1'
git diff --exit-code HEAD~2 -- Cargo.lock
git diff --check
test -z "$(git status --porcelain=v1)"
git log -2 --format='%s'
```

Expected: the root layout is complete, the lockfile did not change across the two migration commits, the tree is clean, and the two commit subjects are:

```text
docs: align Rust package architecture references
refactor: adopt conventional Rust package layout
```

- [ ] **Step 4: Push and verify `main` remotely**

Run:

```bash
git push origin main
git fetch origin --prune
test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)"
gh api 'repos/Pleo2/agent-lowmem/git/trees/main?recursive=1' \
  --jq '[.tree[].path | select(. == "Cargo.toml" or . == "src/main.rs" or . == "tests/doctor_cli.rs" or . == "crates/agent-lowmem/Cargo.toml")]'
```

Expected remote paths:

```json
["Cargo.toml","src/main.rs","tests/doctor_cli.rs"]
```

The removed nested manifest must not appear. Do not delete historical remote branches or rewrite shared history as part of this migration.
