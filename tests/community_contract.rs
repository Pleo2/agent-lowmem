use std::{fs, path::PathBuf};

const MAX_DOCUMENT_BYTES: u64 = 128 * 1024;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_document(path: &str) -> String {
    let path = repository_root().join(path);
    let metadata = fs::metadata(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
    assert!(
        metadata.len() <= MAX_DOCUMENT_BYTES,
        "{path:?} is unexpectedly large"
    );
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
}

#[test]
fn public_documents_share_the_release_and_licensing_contract() {
    let documents = [
        "README.md",
        "CHANGELOG.md",
        "CONTRIBUTING.md",
        "SECURITY.md",
        "CODE_OF_CONDUCT.md",
        "ROADMAP.md",
        "COMMERCIAL.md",
    ];
    let all_documents = documents
        .iter()
        .map(|path| read_document(path))
        .collect::<Vec<_>>()
        .join("\n");

    for required in [
        "Agent Lowmem",
        "FSL-1.1-MIT",
        "Jose Leonardo Moreno",
        "support@agentlowmem.dev",
        "macOS 14",
        "ARM64",
        "not signed or notarized",
    ] {
        assert!(
            all_documents.contains(required),
            "missing public contract: {required}"
        );
    }

    assert!(!all_documents.contains("spctl --master-disable"));
    assert!(!all_documents.contains("curl | sh"));
    assert!(!all_documents.contains("curl -fsSL"));
    assert!(all_documents.contains("brew install Pleo2/agent-lowmem/agent-lowmem"));
    assert!(!all_documents.contains("Agent Lowmem is open source"));
}

#[test]
fn contribution_security_and_roadmap_policies_are_explicit() {
    let contributing = read_document("CONTRIBUTING.md");
    assert!(contributing.contains("git commit -s"));
    assert!(contributing.contains("seven days"));
    assert!(contributing.contains("fourteen days"));

    let security = read_document("SECURITY.md");
    assert!(security.contains("privately report a vulnerability"));
    assert!(security.contains("newest released version"));

    let roadmap = read_document("ROADMAP.md");
    for milestone in ["ARM64 MVP", "Trusted macOS distribution", "GitHub Offload"] {
        assert!(roadmap.contains(milestone));
    }
}

#[test]
fn changelog_exposes_the_expected_release_headings() {
    let changelog = read_document("CHANGELOG.md");
    assert!(changelog.contains("## [Unreleased]"));
    assert!(changelog.contains("## [0.1.0] - 2026-09-05"));
}
