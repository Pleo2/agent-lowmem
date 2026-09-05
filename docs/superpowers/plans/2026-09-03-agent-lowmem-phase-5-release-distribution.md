# Agent Lowmem Phase 5 No-Pipeline Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish Agent Lowmem v0.1.0 as an unsigned Apple Silicon binary with a locally verified archive, immutable GitHub Release, public community contracts, and a manually maintained Homebrew tap without GitHub Actions or another paid pipeline.

**Architecture:** Build, test, audit, and package sequentially on the maintainer's ARM64 Mac. Keep deterministic packaging, release validation, and publication auditing in three focused POSIX scripts; keep all visibility, tagging, draft creation, publication, and tap mutations as explicit maintainer commands after fail-closed local gates. The implementation repository and Homebrew tap remain separate and use no cross-repository credential.

**Tech Stack:** Rust 1.85.0, Cargo, POSIX shell on macOS, GitHub CLI, Gitleaks 8.18.4 with a mandatory detection canary, cargo-audit 0.22.2, Homebrew formulae, FSL-1.1-MIT.

**Spec:** `docs/superpowers/specs/2026-09-03-agent-lowmem-phase-5-release-distribution-design.md`

## Global Constraints

- Support only macOS 14 or later on `aarch64-apple-darwin`.
- The first release and Cargo package version are exactly `v0.1.0` and `0.1.0`.
- Preserve `publish = false`; do not publish to crates.io.
- Use `FSL-1.1-MIT`, copyright `2026 Jose Leonardo Moreno`, and the identity Agent Lowmem by Pleo2.
- Keep the release unsigned and not notarized; never suggest disabling Gatekeeper globally.
- Run Cargo work sequentially with `-j 1` and tests with `--test-threads=1`.
- Keep the release binary at or below 12 MiB and retained parent RSS at or below 24 MiB.
- Do not add GitHub Actions, another hosted pipeline, a self-hosted runner, artifact attestations, runtime dependencies, daemons, telemetry, update checks, installers, caches, or cross-repository secrets.
- Do not change `agent-lowmem github inspect`, managed-runner behavior, policy semantics, or versioned JSON schemas.
- Every repository commit uses Conventional Commits and is pushed atomically to `main` until `v0.1.0` is published.
- Stop before visibility, tag, release publication, or tap creation if its immediately preceding gate is not proven.

## File Responsibility Map

- `scripts/package-release.sh`: stage exactly three files, create the ARM64 archive, create `SHA256SUMS`, and verify it.
- `scripts/check-release.sh`: run the complete local code, dependency, resource, binary, package, and smoke gate; write bounded redacted evidence.
- `scripts/audit-publication.sh`: inspect the complete reachable Git/ref publication boundary with a caller-supplied Gitleaks binary; write counts and pass/fail status only.
- `tests/release_package.rs`: exercise packaging against real temporary files and inspect the resulting archive.
- `tests/release_check.rs`: exercise strict checker argument/preflight behavior without executing a full release build.
- `tests/publication_audit.rs`: exercise publication-audit refusal and evidence behavior in isolated Git fixtures.
- `docs/dependencies-v1.md`: append-only local and remote release evidence.

---

### Task 1: License metadata and zero-side-effect version command — completed

- [x] `27dc903 feat: expose release version and license` added `LICENSE.md`, `COMMERCIAL.md`, Cargo metadata, strict `--version`/`-V`, and passing tests.

### Task 2: Public documentation and maintenance contracts — completed

- [x] `5d39cba docs: add public project governance` added README, changelog, contributing, security, conduct, roadmap, and passing document contracts.

### Task 3: GitHub community templates and canonical labels — completed

- [x] `532220b chore: add GitHub community contracts` added issue forms, PR template, CODEOWNERS, release-note configuration, label inventory, and passing contracts.

### Task 4: Remove the paid-pipeline dependency — completed

- [x] `ae1733b docs: redesign release without pipelines` established the local-only release design.
- [x] `c20e641 ci: remove paid GitHub Actions pipeline` removed `.github/workflows/ci.yml` and `tests/workflow_contract.rs`.
- [x] Verified the removal push created no workflow run and `.github/workflows/` contains no file.

### Task 5: Deterministic ARM64 release packaging

**Files:**
- Create: `scripts/package-release.sh`
- Create: `tests/release_package.rs`
- Modify: `.gitignore`

**Interfaces:**
- Consumes exactly three positional arguments: version, executable binary path, and output directory.
- Produces `agent-lowmem-v{version}-aarch64-apple-darwin.tar.gz` and `SHA256SUMS` below the output directory.

- [x] **Step 1: Write the failing package integration tests**

Create a real temporary executable and invoke `sh scripts/package-release.sh`. Assert invalid versions, missing binaries, non-regular binaries, non-executable binaries, extra arguments, and an output directory equal to the repository root fail without changing repository files. For version `0.1.0`, assert exact output names, exact archive members `agent-lowmem`, `LICENSE.md`, `README.md`, modes `0755/0644/0644`, no `._` members, and exactly:

```text
{64 lowercase hex characters}  agent-lowmem-v0.1.0-aarch64-apple-darwin.tar.gz
```

Then run `shasum -a 256 -c SHA256SUMS` inside the output directory and require success.

- [x] **Step 2: Confirm RED**

Run `cargo test --test release_package -j 1 -- --test-threads=1`. Expected: the script is missing.

- [x] **Step 3: Implement the packaging script**

Use `#!/bin/sh`, `set -eu`, exactly three arguments, an ASCII numeric `MAJOR.MINOR.PATCH` grammar with no leading zeros except `0`, `mktemp -d`, and a cleanup trap. Resolve the repository from the script location, reject the repository root as output, create the output directory, remove only the exact archive and `SHA256SUMS`, and stage with:

```sh
install -m 0755 "$binary" "$stage/agent-lowmem"
install -m 0644 "$repository/LICENSE.md" "$stage/LICENSE.md"
install -m 0644 "$repository/README.md" "$stage/README.md"
COPYFILE_DISABLE=1 tar -C "$stage" -czf "$output/$archive" agent-lowmem LICENSE.md README.md
(cd "$output" && shasum -a 256 "$archive" > SHA256SUMS)
(cd "$output" && shasum -a 256 -c SHA256SUMS)
```

Add `/dist/` to `.gitignore`.

- [x] **Step 4: Verify, commit, and push**

Run the focused test, `shellcheck scripts/package-release.sh` when ShellCheck is installed, the full sequential test suite, `cargo build --release --locked -j 1`, a real package rehearsal into `dist`, archive/mode inspection, checksum verification, and `git diff --check`. Commit `build: package the Apple Silicon release` and push `main`.

### Task 6: Complete local release checker

**Files:**
- Create: `scripts/check-release.sh`
- Create: `tests/release_check.rs`
- Modify: `.gitignore`

**Interfaces:**
- Consumes exactly four arguments: version, cargo-audit executable, output directory, and evidence-file path.
- Produces a verified package through `scripts/package-release.sh` and a mode-`0600` redacted evidence file outside the repository.

- [x] **Step 1: Write strict preflight tests**

Assert missing/extra arguments, invalid version, missing/non-executable audit tool, evidence inside the repository, output equal to repository root, non-ARM64 host, dirty worktree, and local/remote `main` divergence all fail before Cargo or the audit executable starts. Use isolated Git fixtures and sentinel executables; assert errors contain no absolute fixture path or environment value.

- [x] **Step 2: Confirm RED**

Run `cargo test --test release_check -j 1 -- --test-threads=1`. Expected: the checker is missing.

- [x] **Step 3: Implement fail-closed preflight and sequential gates**

Use POSIX shell with `set -eu`, resolve the repository from the script location, require `uname -m = arm64`, clean `git status --porcelain=v1 --untracked-files=all`, `HEAD = refs/remotes/origin/main`, package version equality from `cargo metadata --locked --no-deps --format-version 1`, and an external evidence path. Run exactly:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 -- --test-threads=1
cargo test --release --test doctor_budget -- --ignored --test-threads=1
cargo test --release --test managed_files_budget -- --ignored --test-threads=1
cargo test --release --test run_budget -- --ignored --test-threads=1
cargo metadata --locked --format-version 1
"$cargo_audit" audit --deny warnings --file Cargo.lock
cargo build --release --locked -j 1
```

Apply the reviewed SPDX allowlist from `docs/dependencies-v1.md` to external packages only. Require `file target/release/agent-lowmem` to contain `arm64`, size at most `12582912`, exact version output, successful `doctor`, successful packaging, exact archive inventory, and checksum verification.

- [x] **Step 4: Write evidence atomically**

Create a same-directory temporary file under `umask 077`, record only UTC time, HEAD, tool versions, package count, audit status, active/ignored gate status, binary/archive byte counts, archive identities, and overall status, then rename to the caller's external evidence path. Never record environment contents, usernames, absolute paths, tokens, Git remotes, source bytes, or tool stderr.

- [x] **Step 5: Verify, commit, and push**

Download the official `cargo-audit-aarch64-apple-darwin-v0.22.2.tgz` into `mktemp -d`, require SHA-256 `ec7ca4263769593df4d909be85b94a6b79efa2897be5d2bb8ebd516e823175af`, extract it, and run the checker after its commit against clean synchronized `main`. Run ShellCheck when available, focused/full tests, and `git diff --check`. Commit `build: add local release verification` and push before the real clean run.

### Task 7: Publication audit and release-candidate evidence

**Files:**
- Create: `scripts/audit-publication.sh`
- Create: `tests/publication_audit.rs`
- Modify: `docs/dependencies-v1.md`
- Modify: `README.md`
- Modify: the Phase 5 spec status

**Interfaces:**
- Consumes exactly two arguments: Gitleaks executable and external evidence-file path.
- Produces a mode-`0600`, redacted audit record for the complete reachable ref set.

- [x] **Step 1: Write failing publication-audit tests**

Use isolated Git repositories to assert refusal of missing/extra arguments, missing/non-executable scanner, evidence inside the repository, dirty worktree, local/remote divergence, shallow repositories, submodules, Git LFS pointers, suspicious tracked filenames, corrupt Git objects, a scanner that cannot detect the built-in canary, and a scanner finding. Assert the real scanner command uses the version-compatible `detect` Git-history mode with redaction and all refs; assert evidence contains counts/status only and no candidate values or absolute paths.

- [x] **Step 2: Confirm RED and implement**

Run `cargo test --test publication_audit -j 1 -- --test-threads=1`, then implement POSIX `set -eu` checks using `git status --porcelain=v1 --untracked-files=all`, `git fsck --full`, `git rev-list --all`, `git ls-files`, `.gitmodules`, LFS pointer signatures, and a closed suspicious-filename grammar. Before trusting a clean result, require Gitleaks to detect a runtime-assembled synthetic GitHub token and return its finding exit code without exposing the candidate. Invoke the repository scan with `detect --source . --redact --no-banner --exit-code 1 --log-opts=--all`. Write external evidence atomically with mode `0600` and status/counts only.

- [x] **Step 3: Run the real all-ref scan**

Download Gitleaks `v8.18.4` ARM64 and its checksum list to `mktemp -d`, require archive SHA-256 `a480d8593acd8215b22402cf0f3f88b01dcd3610c63b5391db640f7767e62104`, extract it, and run the audit against all refs. This replaces the originally selected `v8.30.1`, whose official ARM64 binary failed the required detection canary. Stop on any finding and report only category/location, never the candidate. Move the temporary tool directory to Trash after use.

- [x] **Step 4: Run and record complete release gates**

Run the Task 6 checker with the checksum-verified cargo-audit binary, re-run the publication audit, and append exact non-secret evidence to `docs/dependencies-v1.md`: host/tool versions, audited commit, test totals, scan status, external dependency/license totals, advisories, binary/archive bytes, resource results, artifact inventory, and exact commands. Update README to release-candidate language without Homebrew availability and set the spec status to `Implemented locally; publication gates pending`.

- [x] **Step 5: Commit, push, and audit the final candidate**

Commit `docs: record phase 5 release candidate evidence`, push `main`, then rerun both audits against that exact clean synchronized commit. If the recorded commit must change, add one evidence-only follow-up commit and audit it again. Proceed only when the final HEAD itself is the audited commit.

### Task 8: Public repository settings and canonical labels

**Files:**
- Modify: `docs/dependencies-v1.md` after remote verification.

**Interfaces:**
- Consumes the exact clean release-candidate commit from Task 7.
- Produces public `Pleo2/agent-lowmem`, disabled Actions, private vulnerability reporting, immutable releases, and canonical labels; no tag.

- [x] **Step 1: Revalidate the irreversible boundary**

Fetch `origin/main`; require HEAD equality, a completely clean worktree, fresh successful local release and all-ref publication audits, successful `git fsck --full`, zero workflow files, and the exact publication inventory recorded in Task 7. Verify authenticated metadata still reports `PRIVATE`.

- [x] **Step 2: Make the exact repository public and verify anonymously**

Run:

```sh
gh repo edit Pleo2/agent-lowmem --visibility public --accept-visibility-change-consequences
```

Then query `https://api.github.com/repos/Pleo2/agent-lowmem` without an authorization header and require `visibility=public` and `private=false`.

- [x] **Step 3: Disable Actions and enable security/release settings**

Disable repository Actions through the GitHub API and verify `enabled=false`. Enable private vulnerability reporting and release immutability through their supported GitHub settings, then re-read both independently. A missing or unavailable setting blocks tagging.

- [x] **Step 4: Apply labels without deleting unrelated labels**

For each exact entry in `.github/labels.yml`, run `gh label create --force --repo Pleo2/agent-lowmem` with its name, six-digit color, and description. Verify all ten canonical labels and preserve every unrelated label.

- [x] **Step 5: Record external state**

Append visibility, anonymous verification, Actions-disabled state, vulnerability-reporting state, immutability state, label inventory, and verification time to `docs/dependencies-v1.md`. Commit `docs: record public repository readiness`, push, rerun local release/publication audits, and do not tag.

### Task 9: Create, verify, and publish v0.1.0 locally

**Files:**
- Modify: `docs/dependencies-v1.md` after observed remote results.

**Interfaces:**
- Consumes public synchronized `main`, successful local audits, Cargo 0.1.0, and enabled immutable releases.
- Produces annotated `v0.1.0` and one manually verified immutable GitHub Release.

- [x] **Step 1: Prove tag preconditions**

Require no local or remote `v0.1.0` tag, no release with that name, clean synchronized `main`, exact Cargo version, public visibility, disabled Actions, private vulnerability reporting, release immutability, and fresh successful Task 6/7 evidence.

- [x] **Step 2: Create and push the annotated tag**

Run:

```sh
git tag -a v0.1.0 -m "release: v0.1.0"
git show --no-patch --format=fuller v0.1.0
git push origin v0.1.0
```

Verify the remote tag resolves to the exact audited commit and do not move or reuse it.

- [x] **Step 3: Create the draft explicitly**

Run `gh release create v0.1.0 --draft --verify-tag --generate-notes --title "Agent Lowmem v0.1.0" dist/agent-lowmem-v0.1.0-aarch64-apple-darwin.tar.gz dist/SHA256SUMS`. Do not pass `--latest` or publish automatically.

- [x] **Step 4: Verify downloaded draft assets independently**

Download both assets to a new temporary directory, verify the checksum, exact members and modes, execute extracted `--version` and `doctor`, compare the tag target and local audited commit, and confirm the release is still a draft. Move temporary downloads to Trash.

- [x] **Step 5: Publish and record the immutable release**

Review generated notes and add the unsigned/not-notarized disclosure if absent. Publish with `gh release edit v0.1.0 --draft=false --latest`, then verify public URLs, immutable setting, exact tag target, asset names, sizes, and checksum from a fresh anonymous download. Append evidence and commit `docs: record v0.1.0 publication`.

### Task 10: Homebrew tap, public docs, and no-CI branch policy

**Files:**
- Create in a fresh repository: `Pleo2/homebrew-agent-lowmem/Formula/agent-lowmem.rb`
- Create in that repository: `README.md`
- Create in that repository: `LICENSE`
- Modify here: `README.md`, `CHANGELOG.md`, `tests/community_contract.rs`, `docs/dependencies-v1.md`, Phase 5 spec status, and this plan's checkboxes.

**Interfaces:**
- Consumes the immutable public v0.1.0 archive URL and independently verified SHA-256.
- Produces public tap `Pleo2/homebrew-agent-lowmem`, verified install/uninstall, final public docs, and protection against force-push/deletion without CI requirements.

- [x] **Step 1: Create the tap without inherited repository state**

Use a fresh `mktemp -d`; create only `Formula/agent-lowmem.rb`, `README.md`, and MIT `LICENSE`; initialize Git; and create public `Pleo2/homebrew-agent-lowmem`. The first commit is `chore: initialize Agent Lowmem tap`. The formula declares upstream `FSL-1.1-MIT` and contains the exact public versioned archive URL and freshly recomputed digest.

- [x] **Step 2: Validate the Homebrew lifecycle**

Run `brew style`, `brew audit --strict`, direct install, `brew test`, `agent-lowmem --version`, `agent-lowmem doctor`, and uninstall. Require no daemon, service, residual binary, or tap-owned runtime process. Commit `feat: distribute agent-lowmem 0.1.0` and push the tap only after all checks pass.

- [x] **Step 3: Publish Homebrew instructions and finalize documents**

Only after Step 2, add `brew install Pleo2/agent-lowmem/agent-lowmem`, upgrade, and uninstall commands to README with the unsigned/not-notarized warning adjacent. Change the community contract from rejecting to requiring the exact install command. Finalize changelog/recognition, mark the spec `Released`, and mark plan checkboxes only from observed evidence.

- [x] **Step 4: Establish the no-CI branch policy**

Create a GitHub ruleset for `main` that blocks branch deletion and non-fast-forward pushes. Do not require a status check, Actions workflow, external pipeline, or second approving reviewer. Verify the active ruleset through the API before relying on it.

- [x] **Step 5: Final convergence**

Run all active and ignored release-only tests sequentially, both local audit scripts, anonymous implementation/release/tap URL checks, a fresh install from the public tap, exact version and doctor smoke checks, uninstall, tag/asset/checksum verification, and `git diff --check`. Append final evidence, commit `docs: complete the v0.1.0 release`, push, and prove clean synchronized `main`. Do not begin signing, Intel support, auto-update, tap automation, GitHub Actions, or GitHub Offload.
