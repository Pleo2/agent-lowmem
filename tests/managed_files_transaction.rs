use agent_lowmem::{
    cli::InitRequest,
    host::{HostReadError, HostSource},
    managed_files::{ManagedAction, ManagedIdentity, ManagedOutcome, execute_init},
    result::Reason,
};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn dry_run_returns_the_plan_without_lock_or_repository_writes() {
    let fixture = Fixture::new();
    let before = snapshot(&fixture.root);

    let outcome = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(true),
    );

    assert_eq!(outcome.report.outcome, ManagedOutcome::Planned);
    assert_eq!(outcome.report.result.reason, Reason::Completed);
    assert_eq!(snapshot(&fixture.root), before);
    assert!(!fixture.runtime.exists());
    assert!(!fixture.root.join(".git/agent-lowmem").exists());
}

#[test]
fn applies_a_new_repository_transaction_in_the_exact_private_modes() {
    let fixture = Fixture::new();

    let outcome = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );

    assert_eq!(outcome.report.outcome, ManagedOutcome::Applied);
    assert_eq!(outcome.report.result.reason, Reason::Completed);
    assert!(fixture.root.join(".agent-lowmem.json").is_file());
    assert!(fixture.root.join("AGENTS.md").is_file());
    let journal = fixture.root.join(".git/agent-lowmem/restoration-v1.json");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&journal).unwrap()).unwrap()["state"],
        "applied"
    );
    assert_eq!(mode(&fixture.root.join(".agent-lowmem.json")), 0o600);
    assert_eq!(mode(&fixture.root.join("AGENTS.md")), 0o600);
    assert_eq!(mode(&journal), 0o600);
    assert_eq!(mode(&fixture.root.join(".git/agent-lowmem")), 0o700);
    let journal_text = fs::read_to_string(journal).unwrap();
    assert!(!journal_text.contains("\"pid\""));
    assert!(!journal_text.contains(fixture.root.to_str().unwrap()));
}

#[test]
fn preserves_external_configuration_and_is_unchanged_on_an_exact_rerun() {
    let fixture = Fixture::new();
    let external = b"{\"version\":1,\"packageManager\":\"npm\",\"operations\":{\"checks\":{\"script\":\"test\",\"timeoutSeconds\":300}}}\n";
    fs::write(fixture.root.join(".agent-lowmem.json"), external).unwrap();

    let first = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );
    assert_eq!(first.report.outcome, ManagedOutcome::Applied);
    assert_eq!(
        fs::read(fixture.root.join(".agent-lowmem.json")).unwrap(),
        external
    );
    assert_eq!(
        action(&first, ManagedIdentity::Configuration),
        ManagedAction::Preserve
    );
    assert!(
        fs::read_to_string(fixture.root.join("AGENTS.md"))
            .unwrap()
            .contains("agent-lowmem run checks")
    );
    let before = snapshot(&fixture.root);

    let second = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );

    assert_eq!(second.report.outcome, ManagedOutcome::Unchanged);
    assert_eq!(snapshot(&fixture.root), before);
}

#[test]
fn adopts_exact_generated_files_without_rewriting_their_bytes() {
    let fixture = Fixture::new();
    let first = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );
    assert_eq!(first.report.outcome, ManagedOutcome::Applied);
    let configuration = fs::read(fixture.root.join(".agent-lowmem.json")).unwrap();
    let agents = fs::read(fixture.root.join("AGENTS.md")).unwrap();
    fs::remove_dir_all(fixture.root.join(".git/agent-lowmem")).unwrap();

    let adopted = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );

    assert_eq!(adopted.report.outcome, ManagedOutcome::Applied);
    assert_eq!(
        action(&adopted, ManagedIdentity::Configuration),
        ManagedAction::Unchanged
    );
    assert_eq!(
        action(&adopted, ManagedIdentity::AgentsPolicy),
        ManagedAction::Unchanged
    );
    assert_eq!(
        fs::read(fixture.root.join(".agent-lowmem.json")).unwrap(),
        configuration
    );
    assert_eq!(fs::read(fixture.root.join("AGENTS.md")).unwrap(), agents);
    let journal: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.root.join(".git/agent-lowmem/restoration-v1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["state"], "applied");
    assert_eq!(journal["configuration"]["stableBaseline"]["state"], "bytes");
    assert_eq!(journal["agentsPolicy"]["stableBaseline"]["state"], "absent");
}

#[test]
fn updates_managed_targets_and_keeps_one_prior_applied_journal() {
    let fixture = Fixture::new();
    let first = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );
    assert_eq!(first.report.outcome, ManagedOutcome::Applied);
    fixture.write(
        "node_modules/eslint/package.json",
        r#"{"name":"eslint","version":"10.9.1"}"#,
    );
    fixture.write(
        "package.json",
        r#"{"name":"npm-single","private":true,"packageManager":"npm@12.0.2","scripts":{"test":"vitest run","lint":"eslint ."}}"#,
    );

    let updated = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );

    assert_eq!(updated.report.outcome, ManagedOutcome::Applied);
    assert_eq!(
        action(&updated, ManagedIdentity::Configuration),
        ManagedAction::Replace
    );
    let config = fs::read_to_string(fixture.root.join(".agent-lowmem.json")).unwrap();
    let agents = fs::read_to_string(fixture.root.join("AGENTS.md")).unwrap();
    assert!(config.contains("\"lint\""));
    assert!(agents.contains("agent-lowmem run lint"));
    let journal: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.root.join(".git/agent-lowmem/restoration-v1.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["state"], "applied");
    assert_eq!(journal["previousApplied"]["state"], "applied");
    assert!(journal["previousApplied"]["previousApplied"].is_null());
}

#[test]
fn unchanged_fast_path_revalidates_private_journal_modes() {
    let fixture = Fixture::new();
    let first = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );
    assert_eq!(first.report.outcome, ManagedOutcome::Applied);
    fs::set_permissions(
        fixture.root.join(".git/agent-lowmem"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let second = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );

    assert_eq!(second.report.outcome, ManagedOutcome::Conflict);
    assert_eq!(second.report.result.code, 78);
    assert_eq!(second.report.result.reason, Reason::ManagedFileConflict);
}

#[test]
fn rejects_an_existing_journal_with_a_non_private_mode() {
    let fixture = Fixture::new();
    let first = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );
    assert_eq!(first.report.outcome, ManagedOutcome::Applied);
    let journal = fixture.root.join(".git/agent-lowmem/restoration-v1.json");
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o644)).unwrap();
    let before = snapshot(&fixture.root);

    let second = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &request(false),
    );

    assert_eq!(second.report.outcome, ManagedOutcome::Conflict);
    assert_eq!(second.report.result.code, 78);
    assert_eq!(second.report.result.reason, Reason::ManagedFileConflict);
    assert_eq!(snapshot(&fixture.root), before);
    assert_eq!(mode(&journal), 0o644);
}

fn action(
    outcome: &agent_lowmem::managed_files::ManagedFilesOutcome,
    identity: ManagedIdentity,
) -> ManagedAction {
    outcome
        .report
        .files
        .iter()
        .find(|file| file.identity == identity)
        .unwrap()
        .action
}

fn request(dry_run: bool) -> InitRequest {
    InitRequest {
        dry_run,
        json: true,
    }
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
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
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "agent-lowmem-managed-transaction-{nanos}-{}-{serial}",
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
