use agent_lowmem::{
    cli::InitRequest,
    host::{HostReadError, HostSource},
    managed_files::{ManagedOutcome, ManifestState, execute_init},
    result::Reason,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn dry_run_classifies_every_before_and_target_combination_without_writes() {
    for configuration_is_target in [false, true] {
        for agents_is_target in [false, true] {
            let fixture = Fixture::prepared_first_init();
            if !configuration_is_target {
                fs::remove_file(fixture.root.join(".agent-lowmem.json")).unwrap();
            }
            if !agents_is_target {
                fs::remove_file(fixture.root.join("AGENTS.md")).unwrap();
            }
            let repository_before = snapshot(&fixture.root);
            let runtime_before = snapshot(&fixture.runtime);

            let outcome = execute_init(
                &SupportedHost::reference(),
                &fixture.root,
                &fixture.runtime,
                &request(true),
            );

            assert_eq!(outcome.report.outcome, ManagedOutcome::RecoveryRequired);
            assert_eq!(outcome.report.result.code, 0);
            assert_eq!(outcome.report.result.reason, Reason::ManagedFileConflict);
            assert_eq!(outcome.report.manifest_state, ManifestState::Prepared);
            assert_eq!(snapshot(&fixture.root), repository_before);
            assert_eq!(snapshot(&fixture.runtime), runtime_before);
        }
    }
}

#[test]
fn dry_run_classifies_updated_before_and_target_combinations_without_writes() {
    for configuration_is_target in [false, true] {
        for agents_is_target in [false, true] {
            let fixture = Fixture::prepared_update();
            let journal = journal_value(&fixture.root);
            if !configuration_is_target {
                fs::write(
                    fixture.root.join(".agent-lowmem.json"),
                    journal["configuration"]["immediateBefore"]["owned"]["bytes"]
                        .as_str()
                        .unwrap(),
                )
                .unwrap();
            }
            if !agents_is_target {
                let path = fixture.root.join("AGENTS.md");
                let current = fs::read(&path).unwrap();
                let start = journal["agentsPolicy"]["managedSpan"]["start"]
                    .as_u64()
                    .unwrap() as usize;
                let end = journal["agentsPolicy"]["managedSpan"]["end"]
                    .as_u64()
                    .unwrap() as usize;
                let immediate = journal["agentsPolicy"]["immediateBefore"]["owned"]["bytes"]
                    .as_str()
                    .unwrap()
                    .as_bytes();
                let mut before = Vec::new();
                before.extend_from_slice(&current[..start]);
                before.extend_from_slice(immediate);
                before.extend_from_slice(&current[end..]);
                fs::write(path, before).unwrap();
            }
            let before = snapshot(&fixture.root);

            let outcome = execute_init(
                &SupportedHost::reference(),
                &fixture.root,
                &fixture.runtime,
                &request(true),
            );

            assert_eq!(outcome.report.outcome, ManagedOutcome::RecoveryRequired);
            assert_eq!(outcome.report.result.code, 0);
            assert_eq!(snapshot(&fixture.root), before);
        }
    }
}

#[test]
fn init_rolls_back_a_recoverable_transaction_then_replans_fresh_evidence() {
    let fixture = Fixture::prepared_first_init();
    fixture.write(
        "node_modules/eslint/package.json",
        r#"{"name":"eslint","version":"10.9.1"}"#,
    );
    fixture.write(
        "package.json",
        r#"{"name":"npm-single","private":true,"packageManager":"npm@12.0.2","scripts":{"test":"vitest run","lint":"eslint ."}}"#,
    );

    let outcome = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );

    assert_eq!(outcome.report.outcome, ManagedOutcome::Applied);
    assert_eq!(outcome.report.result.reason, Reason::Completed);
    assert!(
        fs::read_to_string(fixture.root.join(".agent-lowmem.json"))
            .unwrap()
            .contains("\"lint\"")
    );
    assert!(
        fs::read_to_string(fixture.root.join("AGENTS.md"))
            .unwrap()
            .contains("agent-lowmem run lint")
    );
    assert_eq!(journal_value(&fixture.root)["state"], "applied");
}

#[test]
fn init_recovers_an_update_and_keeps_exactly_one_previous_applied_journal() {
    let fixture = Fixture::prepared_update();

    let outcome = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );

    assert_eq!(outcome.report.outcome, ManagedOutcome::Applied);
    assert_eq!(outcome.report.result.reason, Reason::Completed);
    let journal = journal_value(&fixture.root);
    assert_eq!(journal["state"], "applied");
    assert_eq!(journal["previousApplied"]["state"], "applied");
    assert!(journal["previousApplied"]["previousApplied"].is_null());
}

#[test]
fn recovery_preserves_unmanaged_agents_bytes_around_the_owned_separator_and_block() {
    let fixture = Fixture::new();
    fixture.write("AGENTS.md", "human policy");
    let first = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );
    assert_eq!(first.report.outcome, ManagedOutcome::Applied);
    mark_journal_prepared(&fixture.root);
    fixture.add_lint_operation();

    let outcome = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );

    assert_eq!(outcome.report.outcome, ManagedOutcome::Applied);
    let agents = fs::read_to_string(fixture.root.join("AGENTS.md")).unwrap();
    assert!(agents.starts_with("human policy\n\n<!-- agent-lowmem:start"));
    assert!(agents.contains("agent-lowmem run lint"));
    assert_eq!(agents.matches("<!-- agent-lowmem:start").count(), 1);
}

#[test]
fn a_third_configuration_state_is_a_non_mutating_conflict() {
    let fixture = Fixture::prepared_first_init();
    fixture.write(
        ".agent-lowmem.json",
        "{\"version\":1,\"packageManager\":\"npm\",\"operations\":{}}\n",
    );
    let before = snapshot(&fixture.root);

    let outcome = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );

    assert_eq!(outcome.report.outcome, ManagedOutcome::Conflict);
    assert_eq!(outcome.report.result.code, 78);
    assert_eq!(outcome.report.result.reason, Reason::ManagedFileConflict);
    assert_eq!(snapshot(&fixture.root), before);
}

#[test]
fn invalid_or_repositioned_agents_content_is_a_non_mutating_conflict() {
    for mutation in ["edited-block", "duplicate-marker", "surrounding-digest"] {
        let fixture = Fixture::prepared_first_init();
        let agents_path = fixture.root.join("AGENTS.md");
        let mut agents = fs::read_to_string(&agents_path).unwrap();
        match mutation {
            "edited-block" => agents = agents.replace("Supported commands:", "Other commands:"),
            "duplicate-marker" => {
                agents.push_str("<!-- agent-lowmem:start duplicate -->\n");
            }
            "surrounding-digest" => agents.push_str("external edit\n"),
            _ => unreachable!(),
        }
        fs::write(&agents_path, agents).unwrap();
        let before = snapshot(&fixture.root);

        let outcome = execute_init(
            &SupportedHost::reference(),
            &fixture.root,
            &fixture.runtime,
            &request(true),
        );

        assert_eq!(
            outcome.report.outcome,
            ManagedOutcome::Conflict,
            "{mutation}"
        );
        assert_eq!(outcome.report.result.code, 78, "{mutation}");
        assert_eq!(snapshot(&fixture.root), before, "{mutation}");
    }
}

#[test]
fn a_missing_updated_target_is_a_non_mutating_conflict() {
    let fixture = Fixture::new();
    let first = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );
    assert_eq!(first.report.outcome, ManagedOutcome::Applied);
    fixture.add_lint_operation();
    let update = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );
    assert_eq!(update.report.outcome, ManagedOutcome::Applied);
    mark_journal_prepared(&fixture.root);
    fs::remove_file(fixture.root.join(".agent-lowmem.json")).unwrap();
    let before = snapshot(&fixture.root);

    let outcome = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );

    assert_eq!(outcome.report.outcome, ManagedOutcome::Conflict);
    assert_eq!(outcome.report.result.code, 78);
    assert_eq!(snapshot(&fixture.root), before);
}

#[test]
fn a_changed_owned_separator_is_a_non_mutating_conflict() {
    let fixture = Fixture::new();
    fixture.write("AGENTS.md", "human policy");
    let first = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );
    assert_eq!(first.report.outcome, ManagedOutcome::Applied);
    mark_journal_prepared(&fixture.root);
    let agents_path = fixture.root.join("AGENTS.md");
    let agents = fs::read_to_string(&agents_path).unwrap().replacen(
        "human policy\n\n<!--",
        "human policy\n<!--",
        1,
    );
    fs::write(&agents_path, agents).unwrap();
    let before = snapshot(&fixture.root);

    let outcome = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(true),
    );

    assert_eq!(outcome.report.outcome, ManagedOutcome::Conflict);
    assert_eq!(outcome.report.result.code, 78);
    assert_eq!(snapshot(&fixture.root), before);
}

fn request(dry_run: bool) -> InitRequest {
    InitRequest {
        dry_run,
        json: true,
    }
}

fn mark_journal_prepared(root: &Path) {
    let path = root.join(".git/agent-lowmem/restoration-v1.json");
    let applied = fs::read_to_string(&path).unwrap();
    let mut prepared = applied.replacen("\"state\": \"applied\"", "\"state\": \"prepared\"", 1);
    let value: serde_json::Value = serde_json::from_str(&prepared).unwrap();
    let old_digest = value["transactionSha256"].as_str().unwrap().to_owned();
    let mut digest_input = value;
    digest_input
        .as_object_mut()
        .unwrap()
        .remove("transactionSha256");
    let digest: [u8; 32] = Sha256::digest(serde_json::to_vec(&digest_input).unwrap()).into();
    let new_digest = digest
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").unwrap();
            output
        });
    prepared = prepared.replacen(&old_digest, &new_digest, 1);
    fs::write(path, prepared).unwrap();
}

fn journal_value(root: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(root.join(".git/agent-lowmem/restoration-v1.json")).unwrap())
        .unwrap()
}

fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    if !root.exists() {
        return Vec::new();
    }
    let mut files = Vec::new();
    snapshot_inner(root, root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn snapshot_inner(root: &Path, current: &Path, files: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(current).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            snapshot_inner(root, &entry.path(), files);
        } else {
            files.push((
                entry
                    .path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                fs::read(entry.path()).unwrap(),
            ));
        }
    }
}

struct Fixture {
    root: PathBuf,
    runtime: PathBuf,
}

impl Fixture {
    fn prepared_first_init() -> Self {
        let fixture = Self::new();
        let outcome = execute_init(
            &SupportedHost::reference(),
            &fixture.root,
            &fixture.runtime,
            &request(false),
        );
        assert_eq!(outcome.report.outcome, ManagedOutcome::Applied);
        mark_journal_prepared(&fixture.root);
        fixture
    }

    fn prepared_update() -> Self {
        let fixture = Self::new();
        let first = execute_init(
            &SupportedHost::reference(),
            &fixture.root,
            &fixture.runtime,
            &request(false),
        );
        assert_eq!(first.report.outcome, ManagedOutcome::Applied);
        fixture.add_lint_operation();
        let update = execute_init(
            &SupportedHost::reference(),
            &fixture.root,
            &fixture.runtime,
            &request(false),
        );
        assert_eq!(update.report.outcome, ManagedOutcome::Applied);
        mark_journal_prepared(&fixture.root);
        fixture
    }

    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "agent-lowmem-managed-recovery-{nanos}-{}-{serial}",
            std::process::id()
        ));
        let root = base.join("repository");
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repositories/npm-single"),
            &root,
        );
        fs::create_dir(root.join(".git")).unwrap();
        Self {
            root: fs::canonicalize(root).unwrap(),
            runtime: base.join("runtime"),
        }
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    fn add_lint_operation(&self) {
        self.write(
            "node_modules/eslint/package.json",
            r#"{"name":"eslint","version":"10.9.1"}"#,
        );
        self.write(
            "package.json",
            r#"{"name":"npm-single","private":true,"packageManager":"npm@12.0.2","scripts":{"test":"vitest run","lint":"eslint ."}}"#,
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.root.parent().unwrap());
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

struct SupportedHost {
    values: BTreeMap<&'static str, &'static str>,
}

impl SupportedHost {
    fn reference() -> Self {
        Self {
            values: BTreeMap::from([
                ("kern.osproductversion", "26.6.2"),
                ("hw.model", "Mac14,15"),
                ("machdep.cpu.brand_string", "Apple M2"),
                ("hw.memsize", "8589934592"),
                ("hw.pagesize", "16384"),
            ]),
        }
    }
}

impl HostSource for SupportedHost {
    fn operating_system(&self) -> &str {
        "macos"
    }

    fn architecture(&self) -> &str {
        "aarch64"
    }

    fn read(&self, key: &'static str) -> Result<String, HostReadError> {
        self.values
            .get(key)
            .map(|value| (*value).to_owned())
            .ok_or(HostReadError::Missing(key))
    }
}
