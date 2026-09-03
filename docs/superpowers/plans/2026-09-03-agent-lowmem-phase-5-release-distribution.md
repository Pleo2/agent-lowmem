# Agent Lowmem Phase 5 Release and Distribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish Agent Lowmem v0.1.0 as a security-gated, unsigned Apple Silicon binary with an immutable GitHub Release, public community contracts, verifiable provenance, and a manually maintained Homebrew tap.

**Architecture:** Keep runtime changes limited to a zero-side-effect version command. Put legal/community contracts in repository documents, enforce them with data-only integration tests, build on one ARM64 macOS runner, package through one deterministic local script, and leave publication behind explicit audit and human gates. The implementation repository and Homebrew tap remain separate; no cross-repository credential or automatic tap mutation is introduced.

**Tech Stack:** Rust 1.85.0, Cargo, POSIX shell on macOS, GitHub Actions, GitHub CLI, GitHub artifact attestations, Homebrew formulae, FSL-1.1-MIT.

**Spec:** `docs/superpowers/specs/2026-09-03-agent-lowmem-phase-5-release-distribution-design.md`

## Global Constraints

- Support only macOS 14 or later on `aarch64-apple-darwin`.
- The first release and Cargo package version are exactly `v0.1.0` and `0.1.0`.
- Preserve `publish = false`; do not publish to crates.io.
- Use `FSL-1.1-MIT`, copyright `2026 Jose Leonardo Moreno`, and the identity Agent Lowmem by Pleo2.
- Keep the release unsigned and not notarized; never suggest disabling Gatekeeper globally.
- Run Cargo work sequentially with `-j 1` and tests with `--test-threads=1`.
- Keep the release binary at or below 12 MiB and retained parent RSS at or below 24 MiB.
- Do not add runtime dependencies, daemons, telemetry, update checks, installers, caches, or cross-repository secrets.
- Do not change `agent-lowmem github inspect`, managed-runner behavior, policy semantics, or versioned JSON schemas.
- GitHub-owned actions must use reviewed full commit SHAs: `actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803` and `actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6`.
- Every repository commit uses Conventional Commits and is pushed atomically to `main` until `v0.1.0` is published.
- Stop before repository visibility, tag, release publication, or tap creation if the corresponding gate is not proven.

---

### Task 1: License metadata and zero-side-effect version command

**Files:**
- Create: `LICENSE.md`
- Create: `COMMERCIAL.md`
- Create: `tests/version_cli.rs`
- Modify: `Cargo.toml`
- Modify: `src/cli.rs`
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: existing `cli::parse`, `CliCommand`, and `main::write_stdout`.
- Produces: `CliCommand::Version`, exact stdout `agent-lowmem {CARGO_PKG_VERSION}\n`, Cargo SPDX metadata, and the authoritative FSL notice.

- [ ] **Step 1: Add failing CLI tests**

Create `tests/version_cli.rs` with tests that run the compiled binary in an isolated empty directory, set sentinel `git`, `gh`, `node`, `npm`, and `pnpm` executables on `PATH`, and assert:

```rust
for arguments in [["--version"].as_slice(), ["-V"].as_slice()] {
    let output = command(arguments, &fixture);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"agent-lowmem 0.1.0\n");
    assert!(output.stderr.is_empty());
}
assert!(!fixture.child_marker.exists());
assert_eq!(snapshot(&fixture.root), fixture.initial_snapshot);
```

Also assert `--version --json`, `-V doctor`, `version`, and `-v` exit 2.

- [ ] **Step 2: Confirm RED**

Run: `cargo test --test version_cli -j 1 -- --test-threads=1`

Expected: accepted forms fail with the current invalid-CLI result.

- [ ] **Step 3: Implement the strict version command**

Add `Version` to `CliCommand`, recognize only the two single-token forms before other command matches, dispatch it before any current-directory or host inspection, and write:

```rust
fn run_version() -> i32 {
    let output = format!("agent-lowmem {}", env!("CARGO_PKG_VERSION"));
    if write_stdout(&output).is_ok() { 0 } else { 70 }
}
```

Do not emit `ExitResult` on success or failure; a broken stdout returns 70 with no retry.

- [ ] **Step 4: Add canonical licensing files and Cargo metadata**

Set the package fields exactly:

```toml
license = "FSL-1.1-MIT"
repository = "https://github.com/Pleo2/agent-lowmem"
homepage = "https://agentlowmem.dev"
description = "Native macOS resource guardrails for agentic development on low-memory Apple Silicon."
readme = "README.md"
publish = false
```

Create `LICENSE.md` from the canonical SPDX FSL-1.1-MIT text, replacing only the permitted notice variable with `Copyright 2026 Jose Leonardo Moreno`. Create `COMMERCIAL.md` with permitted internal-use examples, prohibited competing-use examples, `support@agentlowmem.dev`, and an explicit statement that `LICENSE.md` controls.

- [ ] **Step 5: Verify and commit**

Run:

```bash
cargo fmt --all -- --check
cargo test --test version_cli -j 1 -- --test-threads=1
cargo test cli::tests -j 1 -- --test-threads=1
git diff --check
```

Commit:

```bash
git add Cargo.toml LICENSE.md COMMERCIAL.md src/cli.rs src/main.rs tests/version_cli.rs
git commit -m "feat: expose release version and license"
```

Push with `git push origin main` after confirming the commit contains only the six listed paths.

### Task 2: Public documentation and maintenance contracts

**Files:**
- Create: `CHANGELOG.md`
- Create: `CONTRIBUTING.md`
- Create: `SECURITY.md`
- Create: `CODE_OF_CONDUCT.md`
- Create: `ROADMAP.md`
- Create: `tests/community_contract.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes: v0.1.0 platform, license, version, support-email, and command contracts.
- Produces: one internally consistent public onboarding and governance surface without claiming Homebrew availability before it passes.

- [ ] **Step 1: Add failing document-contract tests**

Create `tests/community_contract.rs` with bounded reads and explicit assertions for all seven documents. At minimum assert exact product identity, `FSL-1.1-MIT`, `Jose Leonardo Moreno`, `support@agentlowmem.dev`, macOS 14, ARM64, unsigned/not-notarized language, DCO sign-off, seven/fourteen-day response goals, supported newest-version security policy, and the three roadmap milestones. Reject `curl ... | sh`, `spctl --master-disable`, claims of current open-source status, and an active Homebrew install claim.

```rust
assert!(read("README.md").contains("Apple Silicon"));
assert!(read("README.md").contains("not signed or notarized"));
assert!(read("CONTRIBUTING.md").contains("git commit -s"));
assert!(read("SECURITY.md").contains("privately report a vulnerability"));
assert!(!all_documents.contains("spctl --master-disable"));
```

- [ ] **Step 2: Confirm RED**

Run: `cargo test --test community_contract -j 1 -- --test-threads=1`

Expected: missing files fail the contract.

- [ ] **Step 3: Write the public documents**

Update README with manual release verification commands but retain an explicit pre-release notice and omit `brew install` until Task 10. Use Keep a Changelog headings `Unreleased` and `0.1.0 - 2026-09-03`. Use Contributor Covenant 2.1 verbatim with the enforcement address. Limit ROADMAP to:

```markdown
1. ARM64 MVP — v0.1.0 release and Homebrew tap.
2. Trusted macOS distribution — Developer ID signing and notarization when available.
3. GitHub Offload — optional design for moving broad validation to hosted runners.
```

- [ ] **Step 4: Verify and commit**

Run the focused test, `cargo test -j 1 -- --test-threads=1`, and `git diff --check`.

Commit and push:

```bash
git add README.md CHANGELOG.md CONTRIBUTING.md SECURITY.md CODE_OF_CONDUCT.md ROADMAP.md tests/community_contract.rs
git commit -m "docs: add public project governance"
git push origin main
```

### Task 3: GitHub community templates and canonical labels

**Files:**
- Create: `.github/ISSUE_TEMPLATE/bug.yml`
- Create: `.github/ISSUE_TEMPLATE/feature.yml`
- Create: `.github/ISSUE_TEMPLATE/config.yml`
- Create: `.github/PULL_REQUEST_TEMPLATE.md`
- Create: `.github/CODEOWNERS`
- Create: `.github/release.yml`
- Create: `.github/labels.yml`
- Create: `tests/github_community.rs`

**Interfaces:**
- Consumes: support, privacy, DCO, scope, and recognition contracts from Task 2.
- Produces: parseable GitHub forms, one PR checklist, release-note categories, ownership, and ten version-controlled label definitions.

- [ ] **Step 1: Add failing template tests**

Parse every YAML file as text plus a minimal indentation/key contract without adding a YAML crate. Assert bug and feature forms have non-empty `name`, `description`, `title`, `labels`, and `body`; config disables blank issues and links to `SECURITY.md`; the PR template covers scope, tests, resource impact, privacy, DCO, and docs. Assert `.github/labels.yml` defines exactly the ten spec labels with unique six-digit hex colors and non-empty descriptions.

- [ ] **Step 2: Confirm RED**

Run: `cargo test --test github_community -j 1 -- --test-threads=1`

- [ ] **Step 3: Add templates and labels**

Use issue forms rather than free-form Markdown issues. Bug prompts explicitly tell reporters to redact tokens, environment values, usernames, and absolute paths. Set:

```yaml
blank_issues_enabled: false
contact_links:
  - name: Private security report
    url: https://github.com/Pleo2/agent-lowmem/security/advisories/new
    about: Report suspected vulnerabilities privately.
```

Set CODEOWNERS to `* @Pleo2`. Release-note categories include breaking changes, features, fixes, performance, documentation, and dependencies; exclude no contributor from recognition.

- [ ] **Step 4: Verify and commit**

Run focused and complete sequential tests, then:

```bash
git add .github tests/github_community.rs
git commit -m "chore: add GitHub community contracts"
git push origin main
```

### Task 4: ARM64 continuous integration

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `tests/workflow_contract.rs`

**Interfaces:**
- Consumes: pinned Rust toolchain and current sequential validation commands.
- Produces: read-only `CI / validate` status on `macos-14`, with no cache or publication permission.

- [ ] **Step 1: Add failing workflow-policy tests**

Assert CI includes `pull_request`, push to `main`, `runs-on: macos-14`, `timeout-minutes: 20`, `contents: read`, the exact pinned checkout SHA, `-j 1`, `--test-threads=1`, `--locked`, `--version`, and `doctor`. Reject `-latest`, `write`, `cache`, `self-hosted`, `sudo`, background operators, floating `uses:` refs, and any other action.

- [ ] **Step 2: Confirm RED**

Run: `cargo test --test workflow_contract ci_ -j 1 -- --test-threads=1`

- [ ] **Step 3: Add the exact CI workflow**

Set the workflow display name to `CI`. Use one job named `validate`, `concurrency.group: ci-${{ github.workflow }}-${{ github.ref }}`, `cancel-in-progress: true`, and these commands in order:

```bash
rustc --version && cargo --version && uname -m && sw_vers -productVersion
cargo fmt --all -- --check
cargo clippy --all-targets -j 1 -- -D warnings
cargo test -j 1 -- --test-threads=1
cargo build --release --locked -j 1
test "$(target/release/agent-lowmem --version)" = "agent-lowmem 0.1.0"
target/release/agent-lowmem doctor
```

- [ ] **Step 4: Verify and commit**

Run the focused test, full tests, and `git diff --check`. Commit `ci: validate Apple Silicon builds` and push `main`. Observe the first `CI / validate` run to completion before Task 7.

### Task 5: Deterministic release packaging boundary

**Files:**
- Create: `scripts/package-release.sh`
- Create: `tests/release_package.rs`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: positional arguments `$1` version, `$2` binary path, and `$3` output directory in `scripts/package-release.sh`.
- Produces: one exact archive and `SHA256SUMS`; output lives below ignored `/dist/`.

- [ ] **Step 1: Add failing package tests**

Create a temporary executable and invoke the script. Assert invalid versions and missing/non-executable binaries fail. For `0.1.0`, assert exact filenames, archive members `agent-lowmem`, `LICENSE.md`, `README.md`, modes `0755/0644/0644`, no `._` members, one checksum line matching `^[0-9a-f]{64}  agent-lowmem-v0.1.0-aarch64-apple-darwin.tar.gz\n$`, and successful `shasum -a 256 -c`.

- [ ] **Step 2: Confirm RED**

Run: `cargo test --test release_package -j 1 -- --test-threads=1`

- [ ] **Step 3: Implement the packaging script**

Use `#!/bin/sh`, `set -eu`, an exact SemVer shell case, `mktemp -d`, a cleanup trap, `install -m`, and:

```sh
archive="agent-lowmem-v${version}-aarch64-apple-darwin.tar.gz"
COPYFILE_DISABLE=1 tar -C "$stage" -czf "$output/$archive" agent-lowmem LICENSE.md README.md
(cd "$output" && shasum -a 256 "$archive" > SHA256SUMS)
(cd "$output" && shasum -a 256 -c SHA256SUMS)
```

Reject output paths equal to the repository root and remove/recreate only the two exact output identities. Add `/dist/` to `.gitignore`.

- [ ] **Step 4: Verify and commit**

Run the focused test, ShellCheck if installed, full sequential tests, and a real release build/package smoke test. Commit `build: package the Apple Silicon release` and push.

### Task 6: Security-pinned draft release workflow

**Files:**
- Create: `.github/workflows/release.yml`
- Modify: `tests/workflow_contract.rs`

**Interfaces:**
- Consumes: annotated `vX.Y.Z` tag, Cargo version, packaging script, and exact release gates.
- Produces: a draft GitHub Release with archive, checksum manifest, and GitHub attestations.

- [ ] **Step 1: Extend failing workflow tests**

Assert tag-only trigger, `macos-14`, timeout 30, non-cancelling release concurrency, exact three write permissions, pinned checkout/attest SHAs, strict tag/Cargo/main checks, all retained release gates, ARM64 checks, packaging script use, local checksum verification, `gh release create --draft --verify-tag`, exact asset upload, and no automatic publication. Reject third-party actions, cache, PAT names, `workflow_dispatch`, `--draft=false`, and tap mutation.

- [ ] **Step 2: Confirm RED**

Run: `cargo test --test workflow_contract release_ -j 1 -- --test-threads=1`

- [ ] **Step 3: Implement validation and build steps**

Use `fetch-depth: 0`; validate the tag with shell `case`; obtain Cargo version with `cargo metadata --locked --no-deps --format-version 1`; verify `git merge-base --is-ancestor "$GITHUB_SHA" origin/main`; reject an existing release with `gh release view "$GITHUB_REF_NAME"`.

Download `cargo-audit-aarch64-apple-darwin-v0.22.2.tgz` from the exact `cargo-audit/v0.22.2` RustSec release, require SHA-256 `ec7ca4263769593df4d909be85b94a6b79efa2897be5d2bb8ebd516e823175af`, extract below `$RUNNER_TEMP`, and run `cargo-audit audit --deny warnings --file Cargo.lock`.

Run all active tests plus the five ignored release gates explicitly. Build locked, assert `uname -m` and `file target/release/agent-lowmem` report ARM64, assert size, and invoke `scripts/package-release.sh`.

- [ ] **Step 4: Implement attestation and draft creation**

Attest `dist/agent-lowmem-v${version}-aarch64-apple-darwin.tar.gz` and `dist/SHA256SUMS` with the pinned attestation action. Create the draft only after attestations succeed:

```bash
gh release create "$GITHUB_REF_NAME" --draft --verify-tag --generate-notes \
  --title "Agent Lowmem $GITHUB_REF_NAME" \
  "dist/$archive" "dist/SHA256SUMS"
```

- [ ] **Step 5: Verify and commit**

Run workflow policy tests, all tests, format, Clippy, `git diff --check`, and a local package rehearsal. Commit `ci: prepare verified draft releases` and push. Do not create a tag.

### Task 7: Pre-publication audit and release-candidate evidence

**Files:**
- Create: `scripts/audit-publication.sh`
- Create: `tests/publication_audit.rs`
- Modify: `docs/dependencies-v1.md`
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-09-03-agent-lowmem-phase-5-release-distribution-design.md`

**Interfaces:**
- Consumes: clean synchronized `main`, a caller-supplied Gitleaks executable, and the complete reachable Git ref set.
- Produces: redacted pass/fail evidence and the exact release-candidate commit; never prints candidate secret values.

- [ ] **Step 1: Add failing audit-policy tests**

Assert the script refuses dirty worktrees, local/remote divergence, missing scanner, shallow repositories, submodules, LFS pointers, suspicious tracked filenames, and failed `git fsck`. Assert it invokes the scanner with redaction and all reachable history, records counts/status only, and never writes inside `.git` or tracked paths.

- [ ] **Step 2: Confirm RED and implement the script**

Run the focused test, then implement a POSIX script taking exactly `$1` as the Gitleaks executable path and `$2` as the evidence-file path. Resolve and compare HEAD/origin main, use `git status --porcelain=v1 --untracked-files=all`, `git fsck --full`, `git rev-list --all`, `git ls-files`, and Gitleaks redacted Git mode. Write the evidence atomically with mode `0600` outside the repository.

- [ ] **Step 3: Run the real secret and publication audit**

Download Gitleaks `v8.30.1` ARM64 and its official checksum list to `mktemp -d`, verify archive SHA-256 `b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5`, extract, and run the audit script against all refs. Move the temporary tool directory to Trash afterward. Stop immediately on any finding and report only its category/location, never its value.

- [ ] **Step 4: Run complete local release gates**

Run format, Clippy, all active tests, every ignored release test, schemas, locked metadata/license audit, RustSec audit, release build, version/doctor smoke, package test, archive inspection, checksum verification, size/RSS gates, and source/workflow policy guards sequentially.

- [ ] **Step 5: Record evidence and commit**

Append exact host/tool versions, audited commit, test totals, scan status, dependencies, advisories, binary/archive bytes, resource results, action SHAs, artifact inventory, and gate commands to `docs/dependencies-v1.md`. Change the spec status to `Implemented locally; publication gates pending`. Update README from early development to release candidate without claiming Homebrew or a public release.

Commit `docs: record phase 5 release candidate evidence`, push, rerun the audit against that new commit, and append a follow-up evidence commit only if the commit identity itself must be recorded. Confirm clean synchronized `main` and green CI before Task 8.

### Task 8: Public repository settings and canonical labels

**Files:**
- Modify: `docs/dependencies-v1.md` only after remote verification.

**Interfaces:**
- Consumes: the exact audited clean release-candidate commit from Task 7.
- Produces: public `Pleo2/agent-lowmem`, private vulnerability reporting, immutable releases, and canonical labels; no release tag.

- [ ] **Step 1: Revalidate the irreversible boundary**

Fetch `origin/main`; require local HEAD equality, a completely clean worktree, green `CI / validate` on that HEAD, the second redacted all-ref secret scan, successful `git fsck --full`, and the exact publication inventory recorded in Task 7. Re-read repository metadata and confirm it is still private. Any difference blocks the visibility change.

- [ ] **Step 2: Make the exact repository public**

After replacing the invalid Step 1 during plan review with the full Task 7 gate rerun, execute:

```bash
gh repo edit Pleo2/agent-lowmem --visibility public --accept-visibility-change-consequences
```

Immediately verify unauthenticated repository metadata from a request without GitHub credentials. Do not infer public visibility only from the authenticated CLI.

- [ ] **Step 3: Enable security and release settings**

Enable private vulnerability reporting through the GitHub repository security setting. Enable release immutability under Settings > General > Releases before any tag. Re-read both settings independently; a missing setting blocks Task 9.

- [ ] **Step 4: Apply labels idempotently**

For each exact entry in `.github/labels.yml`, use `gh label create --force --repo Pleo2/agent-lowmem --name ... --color ... --description ...`. Verify the ten canonical labels and preserve unrelated labels.

- [ ] **Step 5: Record the external state**

Append visibility, vulnerability-reporting, immutability, Actions permissions, label inventory, CI run URL, and verification time to `docs/dependencies-v1.md`. Commit `docs: record public repository readiness`, push, and rerun CI. Do not tag.

### Task 9: Create and verify the v0.1.0 draft release

**Files:**
- Modify: `docs/dependencies-v1.md` after observed remote results.

**Interfaces:**
- Consumes: public synchronized `main`, enabled immutable releases, green CI, Cargo 0.1.0, and release workflow.
- Produces: annotated `v0.1.0`, a successful release workflow, and one verified draft release.

- [ ] **Step 1: Prove tag preconditions**

Verify no local/remote `v0.1.0` tag, no release of that name, clean synchronized main, exact Cargo version, public visibility, private vulnerability reporting, release immutability, and green CI on HEAD.

- [ ] **Step 2: Create and push the annotated tag**

```bash
git tag -a v0.1.0 -m "release: v0.1.0"
git show --no-patch --format=fuller v0.1.0
git push origin v0.1.0
```

- [ ] **Step 3: Wait for the release workflow**

Resolve the exact run triggered by the tag, use `gh run watch --exit-status`, and inspect every job conclusion. On failure, do not move/reuse the tag and do not publish partial assets; diagnose whether the unpublished draft can be deleted and release a patch version if the immutable tag boundary requires it.

- [ ] **Step 4: Verify the draft independently**

Download both assets to a new temporary directory, compare the archive checksum, inspect exact members/modes, execute extracted `--version` and `doctor`, and run:

```bash
gh attestation verify agent-lowmem-v0.1.0-aarch64-apple-darwin.tar.gz --repo Pleo2/agent-lowmem
gh attestation verify SHA256SUMS --repo Pleo2/agent-lowmem
```

Confirm the release is still draft and move temporary downloads to Trash.

- [ ] **Step 5: Publish the immutable release**

Review generated notes and unsigned/not-notarized disclosure, then publish with `gh release edit v0.1.0 --draft=false --latest`. Verify public asset URLs, immutability, tag target, checksum, and attestation again. Append evidence and commit `docs: record v0.1.0 publication`.

### Task 10: Homebrew tap, post-release documentation, and branch policy

**Files:**
- Create in separate repository: `Pleo2/homebrew-agent-lowmem/Formula/agent-lowmem.rb`
- Create in separate repository: `Pleo2/homebrew-agent-lowmem/README.md`
- Create in separate repository: `Pleo2/homebrew-agent-lowmem/LICENSE`
- Modify: `README.md`
- Modify: `CHANGELOG.md`
- Modify: `tests/community_contract.rs`
- Modify: `docs/dependencies-v1.md`
- Modify: `docs/superpowers/specs/2026-09-03-agent-lowmem-phase-5-release-distribution-design.md`
- Modify: this plan's completed checkboxes

**Interfaces:**
- Consumes: immutable public release URL and independently verified archive SHA-256.
- Produces: public tap `Pleo2/homebrew-agent-lowmem`, one-command install, verified uninstall, public docs, and protected post-release `main` workflow.

- [ ] **Step 1: Create the public tap without inherited secrets**

Use a fresh `mktemp -d`, initialize only the three specified files, create `Pleo2/homebrew-agent-lowmem` as a public repository, and push one `chore: initialize Agent Lowmem tap` commit. The tap LICENSE is MIT for packaging metadata; the formula declares upstream `FSL-1.1-MIT`.

- [ ] **Step 2: Add the formula from verified runtime evidence**

Set the formula URL to `https://github.com/Pleo2/agent-lowmem/releases/download/v0.1.0/agent-lowmem-v0.1.0-aarch64-apple-darwin.tar.gz`. Set `sha256` to the digest independently recomputed from that public URL. Use:

```ruby
class AgentLowmem < Formula
  desc "Native macOS resource guardrails for low-memory agentic development"
  homepage "https://agentlowmem.dev"
  url "https://github.com/Pleo2/agent-lowmem/releases/download/v0.1.0/agent-lowmem-v0.1.0-aarch64-apple-darwin.tar.gz"
  version "0.1.0"
  license "FSL-1.1-MIT"

  depends_on :macos
  depends_on arch: :arm64

  def install
    bin.install "agent-lowmem"
    prefix.install "LICENSE.md", "README.md"
  end

  test do
    assert_equal "agent-lowmem #{version}\n", shell_output("#{bin}/agent-lowmem --version")
  end
end
```

The snippet deliberately omits `sha256` because its literal value cannot exist before Task 9. At execution, compute it with `shasum -a 256` from a fresh public download, require equality with the published `SHA256SUMS`, and use `apply_patch` to insert a `sha256` statement immediately after `version`. Its quoted literal must be the observed 64-character lowercase digest before the formula is staged or tested.

- [ ] **Step 3: Validate a clean Homebrew lifecycle**

Run `brew style`, `brew audit --strict`, direct install, `brew test`, `agent-lowmem --version`, `agent-lowmem doctor`, and uninstall. Confirm no daemon, service, or residual Agent Lowmem binary remains. Commit `feat: distribute agent-lowmem 0.1.0` in the tap and push.

- [ ] **Step 4: Publish Homebrew instructions and recognition**

Only after Step 3, add `brew install Pleo2/agent-lowmem/agent-lowmem`, upgrade, and uninstall instructions to the implementation README. Keep the unsigned/not-notarized warning adjacent. Update `tests/community_contract.rs` so it now requires, rather than rejects, the exact verified Homebrew command. Finalize changelog and recognition; mark the spec `Released` and the plan checkboxes from observed evidence only.

- [ ] **Step 5: Establish post-release branch policy**

Create a GitHub ruleset for `main` requiring the observed `CI / validate` status, blocking force pushes and deletion, and allowing merges without a mandatory second approval while `@Pleo2` is the sole maintainer. Verify the ruleset through the API before relying on it.

- [ ] **Step 6: Final convergence and commit**

Run all local and release-only tests sequentially, verify public repository/release/tap URLs without credentials, verify install from the public tap once more, confirm exact tags and immutable assets, run `git diff --check`, and append final evidence.

Commit `docs: complete the v0.1.0 release`, push, wait for CI, and prove clean synchronized `main`. Do not begin signing, Intel, auto-update, tap automation, or GitHub Offload work.
