use std::{collections::BTreeSet, fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[test]
fn issue_forms_and_private_contact_are_complete() {
    for path in [
        ".github/ISSUE_TEMPLATE/bug.yml",
        ".github/ISSUE_TEMPLATE/feature.yml",
    ] {
        let form = read(path);
        for key in ["name:", "description:", "title:", "labels:", "body:"] {
            let line = form.lines().find(|line| line.starts_with(key));
            assert!(
                line.is_some_and(|line| line.trim() != key),
                "{path}: empty {key}"
            );
        }
    }

    let bug = read(".github/ISSUE_TEMPLATE/bug.yml");
    for private_value in [
        "tokens",
        "environment values",
        "usernames",
        "absolute paths",
    ] {
        assert!(
            bug.contains(private_value),
            "bug form must mention {private_value}"
        );
    }

    let config = read(".github/ISSUE_TEMPLATE/config.yml");
    assert!(config.contains("blank_issues_enabled: false"));
    assert!(config.contains("security/advisories/new"));
    assert!(config.contains("SECURITY.md"));
}

#[test]
fn pull_request_ownership_and_release_categories_are_explicit() {
    let pull_request = read(".github/PULL_REQUEST_TEMPLATE.md").to_lowercase();
    for item in [
        "scope",
        "tests",
        "resource impact",
        "privacy",
        "dco",
        "documentation",
    ] {
        assert!(
            pull_request.contains(item),
            "PR checklist must cover {item}"
        );
    }

    assert_eq!(read(".github/CODEOWNERS"), "* @Pleo2\n");

    let release = read(".github/release.yml").to_lowercase();
    for category in [
        "breaking changes",
        "features",
        "fixes",
        "performance",
        "documentation",
        "dependencies",
    ] {
        assert!(
            release.contains(category),
            "release notes must cover {category}"
        );
    }
    assert!(!release.contains("exclude-contributors"));
}

#[test]
fn canonical_labels_are_exact_unique_and_documented() {
    let labels = read(".github/labels.yml");
    let entries = labels
        .split("\n- name: ")
        .skip(1)
        .map(|entry| {
            let mut lines = entry.lines();
            let name = lines.next().expect("label name").trim().to_owned();
            let color = lines
                .find_map(|line| line.trim().strip_prefix("color: "))
                .expect("label color")
                .trim_matches('"')
                .to_owned();
            let description = entry
                .lines()
                .find_map(|line| line.trim().strip_prefix("description: "))
                .expect("label description")
                .trim_matches('"')
                .to_owned();
            (name, color, description)
        })
        .collect::<Vec<_>>();

    let expected = BTreeSet::from([
        "blocked",
        "bug",
        "documentation",
        "enhancement",
        "good first issue",
        "help wanted",
        "macos",
        "performance",
        "release",
        "security",
    ]);
    let names = entries
        .iter()
        .map(|entry| entry.0.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(names, expected);
    assert_eq!(entries.len(), 10);

    let colors = entries
        .iter()
        .map(|entry| entry.1.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(colors.len(), 10);
    assert!(entries.iter().all(|(_, color, description)| {
        color.len() == 6
            && color.bytes().all(|byte| byte.is_ascii_hexdigit())
            && !description.is_empty()
    }));
}
