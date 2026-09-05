# Agent Lowmem Phase 5 Release and Distribution Design

**Date:** 2026-09-03; no-pipeline revision approved 2026-09-04
**Status:** Approved in conversation; no-pipeline implementation plan complete
**Scope:** Publish the first supported Apple Silicon release through a security-gated public repository, immutable GitHub Release, verifiable ARM64 archive, and a dedicated Homebrew tap. Establish the minimum legal, security, contribution, and community surface required for responsible public maintenance.

## 1. Objective

Phase 5 turns the completed native runner and reversible onboarding workflow into an installable public MVP. It provides one trustworthy local route from a reviewed `main` commit to a versioned binary, one-command Homebrew installation, checksum verification, recorded release evidence, and a small public contribution surface.

The phase remains deliberately narrow:

- one macOS architecture and one supported OS floor;
- one binary archive and one checksum manifest per release;
- no Apple signing identity, installer package, daemon, auto-updater, or crates.io publication;
- no cross-repository release token;
- no GitHub Actions workflow, hosted or self-hosted CI runner, paid pipeline, or artifact attestation;
- no expansion of the GitHub inspection command or managed-runner behavior;
- no claim that an unsigned binary is notarized or accepted by Gatekeeper without user confirmation.

## 2. Product, ownership, and license

The product name is **Agent Lowmem**, the executable and Rust package are `agent-lowmem`, the public site is `https://agentlowmem.dev`, and the project is presented as **Agent Lowmem by Pleo2**.

The copyright holder and licensor are **Jose Leonardo Moreno**. Repository notices use:

```text
Copyright 2026 Jose Leonardo Moreno
```

The production repository is licensed under `FSL-1.1-MIT`. Each released version converts automatically to the MIT License two years after that version first becomes publicly available, according to the unmodified Functional Source License 1.1 MIT Future License terms.

Until that conversion, the project is described as **Fair Source** or **source-available**, not OSI Open Source. Public copy never calls the currently restricted version open source.

The license permits personal, educational, research, evaluation, internal business, integration, modification, and redistribution uses that are not a Competing Use. A Competing Use is determined by the standard FSL text: making the software available to others in a commercial product or service that substitutes for Agent Lowmem, substitutes for a product or service offered using it, or offers the same or substantially similar functionality.

`LICENSE.md` contains the official unmodified FSL-1.1-MIT text plus the project identity and copyright notice in the form supported by that license. `Cargo.toml` declares `license = "FSL-1.1-MIT"`, `repository`, `homepage`, `description`, and `readme`, while retaining `publish = false`.

`COMMERCIAL.md` explains the competing-use boundary in plain language, gives non-binding examples, and directs alternative-license requests to `support@agentlowmem.dev`. It does not promise that payment, sponsorship, or a contribution automatically earns an exception. The license text controls if the summary and license differ.

The dedicated Homebrew tap contains packaging metadata rather than the Agent Lowmem implementation and is licensed under MIT. Its formula declares the upstream artifact license as `FSL-1.1-MIT`.

This project documentation is an implementation decision, not individualized legal advice. A material commercial licensing agreement or a change to the competing-use boundary requires qualified legal review outside this phase.

## 3. Supported release contract

The first supported release is `v0.1.0` with package version `0.1.0`.

| Field | Contract |
| --- | --- |
| Operating system | macOS 14 Sonoma or later |
| Architecture | Apple Silicon ARM64 only |
| Rust target | `aarch64-apple-darwin` |
| Executable | `agent-lowmem` |
| Archive | `agent-lowmem-v0.1.0-aarch64-apple-darwin.tar.gz` |
| Checksum manifest | `SHA256SUMS` |
| Release tag | annotated SemVer tag `v0.1.0` |
| Signing | unsigned |
| Notarization | not notarized |
| Package registry | no crates.io publication |

The CLI accepts exactly `agent-lowmem --version` and `agent-lowmem -V` as equivalent top-level commands and prints exactly:

```text
agent-lowmem 0.1.0
```

The version comes from `env!("CARGO_PKG_VERSION")`; it is not duplicated as a handwritten runtime constant. Version output is plain text, contains no wordmark or ANSI escape, writes no file, starts no child, performs no repository inspection, and emits no stable result line on stderr. Extra or combined arguments remain invalid CLI input.

The release archive contains exactly these three regular files at its root:

```text
agent-lowmem
LICENSE.md
README.md
```

The executable has mode `0755`; the two documents have mode `0644`. macOS metadata, resource forks, extended attributes, absolute paths, build directories, logs, schemas, restoration journals, and Git metadata are excluded.

`SHA256SUMS` contains one lowercase SHA-256 digest and the exact archive filename, separated by two spaces and terminated by one LF. It does not include its own digest.

## 4. Repository state and public boundary

The authoritative implementation repository remains `Pleo2/agent-lowmem` with default branch `main`. It is private while Phase 5 files and local release tools are prepared.

Changing visibility to public is a one-time, explicit publication transaction. It may occur only after all pre-publication gates in section 10 pass against the exact remote `main` commit intended for `v0.1.0`. The visibility command names `Pleo2/agent-lowmem` explicitly and acknowledges GitHub's visibility-change consequence flag. No wildcard repository operation is allowed.

Before the visibility change, the implementation records:

- local and remote `main` commit equality;
- a clean tracked and untracked worktree;
- all reachable refs included in the secret scan;
- no submodule, Git LFS pointer, private attachment, local environment file, credential, token, key, customer data, or absolute user path in tracked content or release assets;
- the repository's public description and homepage;
- an explicit list of the files and repository settings being published.

If a suspected secret is found, publication stops. Redaction from the current tree alone is insufficient: the reachable history must be rewritten safely or the secret must be revoked, the corrected history rescanned, and the user must approve the new publication boundary.

After visibility becomes public and before the first tag is pushed:

- GitHub private vulnerability reporting is enabled;
- release immutability is enabled;
- GitHub Actions is disabled for the repository;
- no file exists below `.github/workflows/`;
- no external CI, self-hosted runner, release bot, or paid pipeline is introduced;
- all release validation runs locally on the maintainer's Apple Silicon Mac.

## 5. Community and governance surface

The public repository contains these top-level documents:

- `README.md`: product purpose, constraints, supported platform, installation, checksum verification, unsigned/notarized status, commands, uninstall, development, support, license, and contribution links;
- `LICENSE.md`: authoritative FSL-1.1-MIT license;
- `COMMERCIAL.md`: plain-language competing-use and alternative-license guidance;
- `CHANGELOG.md`: Keep a Changelog structure with a `0.1.0` entry and comparison-ready headings;
- `CONTRIBUTING.md`: development setup, sequential validation, branch naming, Conventional Commits, DCO sign-off, PR size, testing, review, and recognition policy;
- `SECURITY.md`: supported-version table, GitHub private vulnerability reporting as the primary channel, `support@agentlowmem.dev` fallback, disclosure expectations, and no public vulnerability issue instruction;
- `CODE_OF_CONDUCT.md`: Contributor Covenant 2.1 with `support@agentlowmem.dev` as the enforcement address;
- `ROADMAP.md`: only the ARM64 MVP, future Apple signing/notarization, and optional GitHub Offload milestones.

Contributions use Developer Certificate of Origin sign-off through `git commit -s`. The project does not claim copyright assignment and does not introduce a CLA service in v0.1.0. Contributions are accepted under the repository's current license. Alternative commercial licensing of a future collective work is not promised by this phase.

Repository templates are:

- `.github/ISSUE_TEMPLATE/bug.yml` for reproducible bugs without raw logs, tokens, environment values, or private paths;
- `.github/ISSUE_TEMPLATE/feature.yml` for bounded proposals tied to low-memory agentic development;
- `.github/ISSUE_TEMPLATE/config.yml` with blank issues disabled, Discussions omitted, and a private-security contact link;
- `.github/PULL_REQUEST_TEMPLATE.md` with scope, test evidence, resource impact, security/privacy checks, DCO confirmation, and documentation impact;
- `.github/CODEOWNERS` assigning repository ownership to `@Pleo2`;
- `.github/release.yml` grouping generated release notes without broad automation;
- `.github/labels.yml` as the version-controlled canonical label inventory.

The initial labels are exactly:

```text
bug
enhancement
documentation
security
good first issue
help wanted
performance
macos
blocked
release
```

Labels are applied idempotently through authenticated GitHub CLI calls after the repository is public. Existing unrelated labels are preserved. Label colors and descriptions are defined in `.github/labels.yml`; no label-sync action or long-lived token is added.

Maintainer response times are goals, not service-level guarantees: acknowledge ordinary issues within seven calendar days and provide a first substantive PR review within fourteen calendar days. Security reports follow `SECURITY.md`. External contributors are named in the relevant changelog entry and generated release notes; recognition never exposes private report identities without consent.

## 6. Local validation architecture

`scripts/check-release.sh` is the single local validation entry point for the MVP. It runs on the maintainer's supported Apple Silicon Mac, records non-secret host and tool versions, and invokes every gate sequentially with no background process, daemon, cache service, or networked build executor.

The script fails before packaging when any command fails and runs, in order:

1. repository, version, and architecture preflight;
2. `cargo fmt --all -- --check`;
3. `cargo clippy --all-targets -j 1 -- -D warnings`;
4. `cargo test -j 1 -- --test-threads=1`;
5. the explicit ignored release-only gates under `--release`;
6. dependency-license and RustSec audits from checksum-verified local tools;
7. `cargo build --release --locked -j 1`;
8. binary architecture, size, `--version`, and `doctor` smoke checks;
9. deterministic packaging and checksum verification.

The script writes a bounded, redacted evidence file outside tracked paths. It never pushes, tags, changes repository settings, creates a GitHub release, or updates the Homebrew tap.

## 7. Local release procedure

The maintainer executes the local validation entry point against a clean `main` that exactly matches `origin/main`. After reviewing its evidence and artifact inventory, the maintainer creates an annotated `vX.Y.Z` tag whose version matches `Cargo.toml`, pushes only that tag, and re-verifies the remote tag target.

The draft release is then created explicitly with the authenticated GitHub CLI using `gh release create --draft --verify-tag --generate-notes`. Exactly the versioned ARM64 archive and `SHA256SUMS` are uploaded. No script automatically publishes the draft, changes visibility, modifies repository settings, or mutates the Homebrew tap.

A human verifies filenames, checksums, release notes, unsigned-language disclosure, tag identity, and Homebrew formula inputs before publishing the immutable release. A failure before draft creation leaves no remote release. A failed draft may be deleted and rebuilt before publication. Published immutable assets are never replaced; a defect requires a new patch version.

## 8. Release integrity and verification

The release page and README provide checksum verification:

```sh
shasum -a 256 -c SHA256SUMS
```

Checksums detect accidental or untrusted-mirror modification when the manifest is obtained from the immutable GitHub Release. The release also records the exact source tag and locally audited commit. The MVP does not claim cryptographic build provenance or independent reproducibility: it has no GitHub artifact attestation, Apple signature, or notarization, and the checksum does not assert that the code is vulnerability-free.

The unsigned disclosure is adjacent to every manual installation route. Documentation never recommends globally disabling Gatekeeper, running `sudo spctl --master-disable`, or piping a remote script into a shell. If macOS blocks execution, the user is directed to verify the checksum and release-tag identity first, then use the narrow system-provided confirmation flow for that binary.

## 9. Homebrew distribution

The tap repository is exactly `Pleo2/homebrew-agent-lowmem`, public, with default branch `main`. Homebrew resolves it as tap `Pleo2/agent-lowmem`.

The tap contains:

```text
Formula/agent-lowmem.rb
README.md
LICENSE
```

The formula:

- uses the immutable `v0.1.0` archive URL from `Pleo2/agent-lowmem`;
- embeds the exact SHA-256 from the published `SHA256SUMS`;
- declares `license "FSL-1.1-MIT"`;
- requires macOS and ARM64;
- installs only `agent-lowmem` into `bin` and the shipped documents into the prefix share area;
- contains a test that asserts exact `agent-lowmem 0.1.0` output;
- contains no source build, post-install script, service, privileged operation, telemetry, or network call after Homebrew fetches the archive.

The supported install, update, and uninstall commands are:

```sh
brew install Pleo2/agent-lowmem/agent-lowmem
brew upgrade agent-lowmem
brew uninstall agent-lowmem
```

For `v0.1.0`, the formula is updated manually after the immutable release is public. The exact archive digest is copied from freshly downloaded release evidence, independently recomputed, and compared with `SHA256SUMS`. The tap change passes `brew style`, `brew audit --strict`, installation, formula test, upgrade-path simulation where applicable, and uninstall on Apple Silicon before it is pushed.

No personal access token, deploy key, GitHub App, formula-update action, or cross-repository workflow dispatch is introduced. Tap automation is a later design.

## 10. Pre-publication and release gates

Every gate is fail-closed and runs sequentially on the reference Mac. No validation gate depends on a hosted pipeline.

### 10.1 Repository safety

- local `main`, `origin/main`, and the intended release commit are identical;
- worktree is clean;
- all reachable refs pass a redacted secret scan using a checksum-verified release of a recognized scanner;
- tracked-file and history filename review finds no environment, credential, key, archive, database, customer, or local-user artifact;
- `git fsck --full` reports no corrupt object;
- the publication inventory contains no unexpected file;
- remote visibility is still private until every preceding check passes.

### 10.2 Code and dependency quality

- formatting, Clippy, and all active tests pass;
- every ignored release-only budget test passes under `--release`;
- `cargo metadata --locked` resolves one root package;
- the external dependency SPDX allowlist passes;
- RustSec audit passes with warnings denied;
- `Cargo.toml` and `Cargo.lock` changes are explained and reviewed;
- first-party production code retains `#![forbid(unsafe_code)]` and existing process/network boundaries unless separately specified.

### 10.3 Artifact quality

- the reference Mac and binary are ARM64;
- release binary remains at most 12 MiB;
- parent RSS remains at most 24 MiB on retained managed-run fixtures;
- the archive has the exact name, members, modes, and no extended metadata;
- archive checksum generation and verification pass;
- `--version` and `doctor` smoke checks pass from the extracted archive;
- a clean Homebrew install, test, and uninstall pass from the public tap.

### 10.4 Public repository readiness

- license and copyright identity are consistent across Cargo, release archive, repository, and tap;
- README contains no unsupported release claim before publication;
- contribution, security, conduct, roadmap, issue, PR, release-note, and label contracts are present;
- security reporting is private-first;
- releases are immutable before `v0.1.0` is published;
- the release remains draft until the final human gate;
- no GitHub inspection functionality is added or broadened by Phase 5.

## 11. Versioning and maintenance

Agent Lowmem follows Semantic Versioning. During `0.x`, public CLI grammars and versioned JSON schemas remain compatibility contracts within a minor release line; an intentionally breaking contract change requires a new minor version and migration notes. Bug fixes and packaging corrections use patch versions.

`CHANGELOG.md` is updated in the release commit. Generated GitHub release notes supplement rather than replace it. A published tag is never moved. A published archive or checksum is never overwritten. The Homebrew formula always points to one immutable versioned URL and digest.

Only the newest released version receives security fixes during the MVP. `SECURITY.md` states this explicitly. There is no in-process update checker, background network request, telemetry event, or self-update command.

After the first release, repository work moves from direct `main` commits to Conventional Commit branches and pull requests. The branch policy blocks force pushes and branch deletion but requires no pipeline status or approving review while the repository has one maintainer, avoiding a self-deadlock. The maintainer runs the documented local checks before merging. Release tags remain maintainer-only.

## 12. Failure and rollback model

Before visibility changes, all Phase 5 repository-file changes remain ordinary reversible Git commits.

After visibility changes, public exposure cannot be treated as a reversible privacy operation. If a secret is discovered, the response is revocation first, coordinated history remediation second, and public disclosure according to `SECURITY.md`; changing the repository back to private is not considered sufficient remediation.

A failed draft release can be deleted because it has not entered the immutable public contract. A published immutable release is corrected only by a new patch version. A bad Homebrew formula is repaired with a new tap commit; it never changes the upstream assets or tag.

The tap repository is created only after the first GitHub Release is public, so a formula cannot advertise a missing asset. If tap validation fails, the GitHub Release remains a valid manual-download release and the README must not claim Homebrew availability until the tap gate passes.

## 13. Explicit non-goals

Phase 5 does not add:

- Intel macOS, universal binaries, Linux, or Windows artifacts;
- macOS 13 or earlier support;
- Apple Developer ID signing, hardened-runtime signing, notarization, `.pkg`, `.dmg`, or App Store distribution;
- crates.io, npm, MacPorts, Nix, Docker, or a remote install script;
- automatic Homebrew tap mutation or cross-repository credentials;
- GitHub Actions workflows, hosted CI, a self-hosted runner, external pipelines, or artifact attestations;
- cache services, SBOM generators, release orchestration frameworks, semantic-release bots, CLA bots, or dependency update bots;
- telemetry, analytics, update checks, daemons, privileged helpers, or `sudo`;
- GitHub Offload implementation or changes to `agent-lowmem github inspect`;
- guaranteed commercial-license exceptions or individualized legal conclusions.

## 14. Acceptance criteria

Phase 5 is complete only when all of the following are observed:

1. `Cargo.toml`, `LICENSE.md`, `COMMERCIAL.md`, and release metadata consistently identify Jose Leonardo Moreno, Pleo2, and `FSL-1.1-MIT`.
2. `agent-lowmem --version` and `-V` print the exact package version with zero writes, zero children, empty stderr, and no ANSI.
3. README, changelog, contributing, security, conduct, roadmap, issue, PR, release-note, ownership, and label contracts pass content tests or deterministic inspection.
4. The local release checker passes on the supported ARM64 reference Mac with sequential format, lint, test, build, audit, package, and smoke gates.
5. The complete reachable Git history and tracked publication set pass the redacted pre-publication audit, and the audit records only findings status rather than secret candidates.
6. `Pleo2/agent-lowmem` becomes public only after the exact audited `main` commit is synchronized and clean.
7. Private vulnerability reporting and release immutability are enabled before the first tag.
8. The annotated tag `v0.1.0` points to the reviewed release commit and matches Cargo version `0.1.0`.
9. The local release procedure produces the exact ARM64 archive and checksum manifest, passes retained resource and dependency gates, and creates a draft only through a separate explicit maintainer command.
10. The archive contains exactly the binary, license, and README with correct modes and no macOS metadata; its SHA-256 verifies locally.
11. The published checksum, archive digest, annotated tag, release tag, and recorded audited commit agree exactly.
12. Human review publishes an immutable `v0.1.0` release without replacing assets or moving the tag.
13. `Pleo2/homebrew-agent-lowmem` is public and contains a reviewed formula for the immutable release URL and digest.
14. Homebrew style, audit, clean install, formula test, `doctor` smoke test, and uninstall pass on Apple Silicon.
15. The public README claims Homebrew availability only after criterion 14 and discloses the unsigned and non-notarized status beside installation instructions.
16. Canonical GitHub labels exist with the specified descriptions and existing unrelated labels remain untouched.
17. All pre-Phase-5 tests, JSON schemas, runner behavior, reversible managed files, resource limits, privacy rules, and process boundaries remain green.
18. The working tree is clean, local and remote `main` match, release and tap URLs resolve publicly, and exact verification evidence is appended to `docs/dependencies-v1.md`.

## 15. Stop conditions

Stop the current task and resolve the relevant boundary before continuing if any of these occurs:

- a possible secret, credential, private attachment, customer identity, or unexplained binary appears in reachable history or the publication inventory;
- repository visibility changes before the pre-publication gates pass;
- the license identifier, official text, copyright holder, competing-use summary, Cargo metadata, or formula disagree;
- the release tag and Cargo version differ, the tag commit is not reachable from `main`, or a release/tag identity already exists;
- a GitHub Actions workflow, external pipeline, self-hosted runner, release bot, or cross-repository secret appears;
- the reference Mac or binary is not ARM64, an artifact contains extra files or metadata, or a resource/dependency gate regresses;
- the release becomes public before checksum, tag identity, notes, unsigned disclosure, and asset inventory are verified;
- release immutability or private vulnerability reporting cannot be enabled;
- the Homebrew formula points to a mutable URL, wrong digest, missing release, different license, or unsupported platform;
- publication would require disabling Gatekeeper globally, using `sudo`, piping remote code into a shell, or claiming Apple trust that does not exist;
- unrelated GitHub inspection, runtime, policy, schema, or managed-file behavior enters the Phase 5 diff.

## 16. Normative external references

- Functional Source License 1.1 MIT Future License: `https://fsl.software/`
- SPDX identifier and canonical text: `https://spdx.org/licenses/FSL-1.1-MIT.html`
- Open Source Definition: `https://opensource.org/osd`
- Apple Developer ID requirements: `https://developer.apple.com/support/developer-id/`
- GitHub immutable releases: `https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases`
- Homebrew tap guidance: `https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap`
- Homebrew Formula Cookbook: `https://docs.brew.sh/Formula-Cookbook`
- Developer Certificate of Origin 1.1: `https://developercertificate.org/`
- Contributor Covenant 2.1: `https://www.contributor-covenant.org/version/2/1/code_of_conduct/`
