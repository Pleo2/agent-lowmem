# Agent Lowmem v1 Dependency Record

| Requirement | Purpose | Production boundary | License |
| --- | --- | --- | --- |
| `serde 1.0` | Derive stable structured records | Serialization only | MIT OR Apache-2.0 |
| `serde_json 1.0` | Parse manifests and emit JSON | No script evaluation | MIT OR Apache-2.0 |
| `semver 1.0` | Validate declared package-manager versions | Data-only parsing | MIT OR Apache-2.0 |
| `sysctl 0.7.1` | Safe macOS sysctl reads | Read-only host inspector | MIT |

`Cargo.lock` is the authority for exact resolved versions. Every direct dependency addition requires purpose, source/API review, license review, one-worker tests, and a separate commit. Production dependencies may not add a network client, async runtime, daemon, shell evaluator, or lifecycle installer.
