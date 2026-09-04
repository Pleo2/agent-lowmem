use std::{fs, path::PathBuf};

const CHECKOUT_SHA: &str = "d23441a48e516b6c34aea4fa41551a30e30af803";

fn read_workflow(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".github/workflows")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

fn uses(workflow: &str) -> Vec<&str> {
    workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("uses: "))
        .collect()
}

fn has_background_operator(workflow: &str) -> bool {
    workflow.lines().any(|line| {
        let command = line.trim();
        command.ends_with(" &") || command.contains(" & ")
    })
}

#[test]
fn ci_workflow_is_arm64_read_only_and_bounded() {
    let workflow = read_workflow("ci.yml");

    for required in [
        "name: CI",
        "pull_request:",
        "push:",
        "branches: [main]",
        "permissions:",
        "contents: read",
        "group: ci-${{ github.workflow }}-${{ github.ref }}",
        "cancel-in-progress: true",
        "validate:",
        "runs-on: macos-14",
        "timeout-minutes: 20",
        "cargo fmt --all -- --check",
        "cargo clippy --all-targets -j 1 -- -D warnings",
        "cargo test -j 1 -- --test-threads=1",
        "cargo build --release --locked -j 1",
        "target/release/agent-lowmem --version",
        "target/release/agent-lowmem doctor",
    ] {
        assert!(
            workflow.contains(required),
            "missing CI boundary: {required}"
        );
    }

    assert_eq!(
        uses(&workflow),
        [format!("actions/checkout@{CHECKOUT_SHA}")]
    );
}

#[test]
fn ci_workflow_has_no_privileged_or_mutating_escape_hatch() {
    let workflow = read_workflow("ci.yml");
    let lowercase = workflow.to_ascii_lowercase();

    for rejected in [
        "-latest",
        "contents: write",
        "id-token: write",
        "attestations: write",
        "cache",
        "self-hosted",
        "sudo",
        "workflow_dispatch",
    ] {
        assert!(
            !lowercase.contains(rejected),
            "forbidden CI capability: {rejected}"
        );
    }
    assert!(!has_background_operator(&workflow));
}
