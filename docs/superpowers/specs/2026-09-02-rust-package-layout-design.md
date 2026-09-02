# Agent Lowmem Single-Package Rust Layout Design

## Status

Approved direction. This document defines the migration boundary; it does not
change runtime behavior, public result contracts, or release scope.

## Context

Agent Lowmem currently uses a virtual Cargo workspace whose only member is
`crates/agent-lowmem`. That layout is valid Cargo, but it adds a second manifest
and an extra directory level without providing isolation between multiple Rust
packages. The repository has one production Rust package. The Swift pressure
probe under `tools/pressure-probe` is research tooling and is not a Cargo
workspace member or production dependency.

Before the first release, CLI parsing, host inspection, repository policy,
launch planning, managed files, locking, and process supervision remain modules
of the single `agent-lowmem` package unless a second independently versioned or
distributed Rust package becomes necessary.

## Decision

Use Cargo's conventional single-package layout at the repository root:

```text
agent-lowmem/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── src/
│   ├── lib.rs
│   ├── main.rs
│   ├── cli.rs
│   ├── doctor.rs
│   ├── host.rs
│   ├── repository.rs
│   └── result.rs
├── tests/
│   ├── doctor_budget.rs
│   └── doctor_cli.rs
├── schemas/
├── tools/
│   └── pressure-probe/
└── docs/
```

The root `Cargo.toml` becomes the package manifest. It owns package metadata,
direct dependencies, and release profiles. The virtual `[workspace]` and
`[workspace.package]` sections are removed while there is only one Rust
package. `Cargo.lock` and `rust-toolchain.toml` remain at the repository root.

## Alternatives Considered

### Keep the virtual workspace

This preserves existing paths and makes a future second crate cheap to add, but
retains indirection today for a package split that has not been justified.

### Root package plus an empty workspace boundary

Cargo permits a root package to also be a workspace root. This would retain
workspace commands, but an empty multi-package abstraction still adds policy
without serving a current component boundary.

### Single root package

This is the selected option. It follows Cargo's default package layout, makes
the production entry point immediately visible, removes one manifest, and
keeps future modules inside one tested safety boundary. A workspace can be
introduced later when a real second Rust package exists.

## Migration

1. Merge the package metadata and dependencies from
   `crates/agent-lowmem/Cargo.toml` into the root `Cargo.toml`.
2. Move `crates/agent-lowmem/src` to `src` and
   `crates/agent-lowmem/tests` to `tests` using Git-aware moves.
3. Remove the now-empty `crates/agent-lowmem` directory.
4. Preserve the manifest-relative source guard in `tests/doctor_cli.rs`; its
   `env!("CARGO_MANIFEST_DIR")/src` lookup follows the package move without a
   code change. Update current gate and architecture documentation to the root
   package terminology.
5. Preserve the completed Phase 1 plan as historical implementation evidence,
   but add a clear note that its original paths were superseded by this
   migration rather than rewriting its recorded steps.
6. Regenerate only lockfile metadata that Cargo proves must change. Dependency
   versions must not change as part of this structural migration.

## Boundaries

- No Rust module is split or merged.
- No public CLI, exit code, result schema, privacy rule, or safety invariant
  changes.
- The Swift pressure probe stays under `tools/pressure-probe` and remains
  excluded from production artifacts.
- No second Rust package, workspace member, distribution package, or new
  dependency is introduced.
- Future Rust packages go under `crates/<package-name>` only after they have an
  independent build, testing, ownership, or distribution boundary.

## Verification

The migration is complete only when all of the following hold:

- `cargo metadata --locked --offline` reports `agent-lowmem` at the repository
  root and no package below `crates/agent-lowmem`;
- `cargo fmt --all -- --check` passes;
- `cargo clippy --all-targets -j 1 -- -D warnings` passes;
- `cargo test -j 1 -- --test-threads=1` passes;
- the release build and ignored `doctor_budget` gate pass;
- both Swift pressure-probe test executables and its release build pass;
- `rg` finds no active code, gate, or current architecture reference that
  requires `crates/agent-lowmem`;
- `git diff --check` passes and no generated artifacts are tracked.

## Rollback

The migration is one structural commit. If verification fails before push, do
not publish it; keep the failure and proposed disposition visible for review.
Once published, roll it back with a normal revert commit rather than rewriting
shared history.
