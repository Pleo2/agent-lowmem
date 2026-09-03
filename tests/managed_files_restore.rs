use agent_lowmem::{
    cli::{InitRequest, RestoreRequest},
    host::{HostReadError, HostSource},
    lock::{LeaseRecord, ProcessIdentity, UserLease},
    managed_files::{
        ManagedAction, ManagedIdentity, ManagedOutcome, execute_init, execute_restore,
    },
    result::Reason,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn applied_restore_removes_owned_files_journal_and_empty_private_directory() {
    let fixture = Fixture::initialized();

    let outcome = execute_restore(&fixture.root, &fixture.runtime, &restore(false, false));

    assert_eq!(
        outcome.report.outcome,
        ManagedOutcome::Restored,
        "{outcome:?}"
    );
    assert_eq!(outcome.report.result.reason, Reason::Completed);
    assert!(!fixture.root.join(".agent-lowmem.json").exists());
    assert!(!fixture.root.join("AGENTS.md").exists());
    assert!(!fixture.root.join(".git/agent-lowmem").exists());
}

#[test]
fn restore_preserves_external_configuration_and_unmanaged_agents_edits() {
    let fixture = Fixture::new();
    let external = b"{\"version\":1,\"packageManager\":\"npm\",\"operations\":{\"checks\":{\"script\":\"test\",\"timeoutSeconds\":300}}}\n";
    fs::write(fixture.root.join(".agent-lowmem.json"), external).unwrap();
    fs::write(fixture.root.join("AGENTS.md"), "human prefix").unwrap();
    fixture.init();
    let agents_path = fixture.root.join("AGENTS.md");
    let agents = fs::read_to_string(&agents_path).unwrap();
    fs::write(&agents_path, format!("edited {agents}human suffix\n")).unwrap();

    let outcome = execute_restore(&fixture.root, &fixture.runtime, &restore(false, false));

    assert_eq!(
        outcome.report.outcome,
        ManagedOutcome::Restored,
        "{outcome:?}"
    );
    assert_eq!(
        action(&outcome, ManagedIdentity::Configuration),
        ManagedAction::Preserve
    );
    assert_eq!(
        fs::read(fixture.root.join(".agent-lowmem.json")).unwrap(),
        external
    );
    assert_eq!(
        fs::read_to_string(agents_path).unwrap(),
        "edited human prefixhuman suffix\n"
    );
    assert!(!fixture.root.join(".git/agent-lowmem").exists());
}

#[test]
fn repeated_init_updates_retain_the_absent_configuration_baseline() {
    let fixture = Fixture::initialized();
    fixture.add_lint_operation();
    fixture.init();

    let outcome = execute_restore(&fixture.root, &fixture.runtime, &restore(false, false));

    assert_eq!(
        outcome.report.outcome,
        ManagedOutcome::Restored,
        "{outcome:?}"
    );
    assert!(!fixture.root.join(".agent-lowmem.json").exists());
    assert!(!fixture.root.join("AGENTS.md").exists());
}

#[test]
fn configuration_edits_conflict_even_when_block_force_is_requested() {
    let fixture = Fixture::initialized();
    fs::write(fixture.root.join(".agent-lowmem.json"), "external edit\n").unwrap();
    let before = snapshot(&fixture.root);

    let outcome = execute_restore(&fixture.root, &fixture.runtime, &restore(false, true));

    assert_eq!(outcome.report.outcome, ManagedOutcome::Conflict);
    assert_eq!(outcome.report.result.code, 78);
    assert_eq!(snapshot(&fixture.root), before);
}

#[test]
fn force_removes_one_structurally_complete_edited_block_but_not_ambiguous_markers() {
    let fixture = Fixture::initialized();
    rewrite_block_with_valid_hash(&fixture.root.join("AGENTS.md"), "edited body\n");
    let before = snapshot(&fixture.root);
    let denied = execute_restore(&fixture.root, &fixture.runtime, &restore(false, false));
    assert_eq!(denied.report.outcome, ManagedOutcome::Conflict);
    assert_eq!(snapshot(&fixture.root), before);

    let forced = execute_restore(&fixture.root, &fixture.runtime, &restore(false, true));
    assert_eq!(forced.report.outcome, ManagedOutcome::Restored);
    assert!(!fixture.root.join("AGENTS.md").exists());
    assert!(!fixture.root.join(".agent-lowmem.json").exists());

    let ambiguous = Fixture::initialized();
    let path = ambiguous.root.join("AGENTS.md");
    let mut bytes = fs::read_to_string(&path).unwrap();
    bytes.push_str("<!-- agent-lowmem:start duplicate -->\n");
    fs::write(&path, bytes).unwrap();
    let before = snapshot(&ambiguous.root);
    let conflict = execute_restore(&ambiguous.root, &ambiguous.runtime, &restore(false, true));
    assert_eq!(conflict.report.outcome, ManagedOutcome::Conflict);
    assert_eq!(snapshot(&ambiguous.root), before);
}

#[test]
fn force_accepts_one_complete_block_with_a_stale_body_hash() {
    let fixture = Fixture::initialized();
    let path = fixture.root.join("AGENTS.md");
    let document = fs::read_to_string(&path).unwrap();
    let edited = document.replacen(
        "Run supported heavy validation through Agent Lowmem.",
        "Run one deliberately edited validation command.",
        1,
    );
    fs::write(&path, edited).unwrap();

    let denied = execute_restore(&fixture.root, &fixture.runtime, &restore(false, false));
    assert_eq!(denied.report.outcome, ManagedOutcome::Conflict);
    let forced = execute_restore(&fixture.root, &fixture.runtime, &restore(false, true));
    assert_eq!(forced.report.outcome, ManagedOutcome::Restored);
    assert!(!path.exists());
}

#[test]
fn fresh_clone_fallback_removes_only_reproducible_managed_files_and_is_idempotent() {
    let fixture = Fixture::initialized();
    fs::remove_dir_all(fixture.root.join(".git/agent-lowmem")).unwrap();

    let first = execute_restore(&fixture.root, &fixture.runtime, &restore(false, false));

    assert_eq!(first.report.outcome, ManagedOutcome::Restored);
    assert!(!fixture.root.join(".agent-lowmem.json").exists());
    assert!(!fixture.root.join("AGENTS.md").exists());
    let after_first = snapshot(&fixture.root);

    let second = execute_restore(&fixture.root, &fixture.runtime, &restore(false, false));
    assert_eq!(second.report.outcome, ManagedOutcome::Unchanged);
    assert_eq!(snapshot(&fixture.root), after_first);
}

#[test]
fn fresh_clone_preserves_non_reproducible_configuration_and_reports_review() {
    let fixture = Fixture::new();
    let external = b"{\"version\":1,\"packageManager\":\"npm\",\"operations\":{\"checks\":{\"script\":\"test\",\"timeoutSeconds\":300}}}\n";
    fs::write(fixture.root.join(".agent-lowmem.json"), external).unwrap();
    fixture.init();
    fs::remove_dir_all(fixture.root.join(".git/agent-lowmem")).unwrap();

    let outcome = execute_restore(&fixture.root, &fixture.runtime, &restore(false, false));

    assert_eq!(outcome.report.outcome, ManagedOutcome::Restored);
    assert_eq!(
        fs::read(fixture.root.join(".agent-lowmem.json")).unwrap(),
        external
    );
    assert!(!fixture.root.join("AGENTS.md").exists());
    assert_eq!(outcome.report.issues.len(), 1);
    assert_eq!(outcome.report.issues[0].reason, Reason::ManagedFileConflict);
}

#[test]
fn fresh_clone_fallback_restores_each_managed_destination_independently() {
    let configuration_only = Fixture::initialized();
    fs::remove_file(configuration_only.root.join("AGENTS.md")).unwrap();
    fs::remove_dir_all(configuration_only.root.join(".git/agent-lowmem")).unwrap();
    let configuration = execute_restore(
        &configuration_only.root,
        &configuration_only.runtime,
        &restore(false, false),
    );
    assert_eq!(configuration.report.outcome, ManagedOutcome::Restored);
    assert!(!configuration_only.root.join(".agent-lowmem.json").exists());

    let agents_only = Fixture::initialized();
    fs::remove_file(agents_only.root.join(".agent-lowmem.json")).unwrap();
    fs::remove_dir_all(agents_only.root.join(".git/agent-lowmem")).unwrap();
    let agents = execute_restore(
        &agents_only.root,
        &agents_only.runtime,
        &restore(false, false),
    );
    assert_eq!(agents.report.outcome, ManagedOutcome::Restored);
    assert!(!agents_only.root.join("AGENTS.md").exists());
}

#[test]
fn restore_dry_run_is_non_mutating() {
    let fixture = Fixture::initialized();
    let before = snapshot(&fixture.root);

    let outcome = execute_restore(&fixture.root, &fixture.runtime, &restore(true, false));

    assert_eq!(outcome.report.outcome, ManagedOutcome::Planned);
    assert_eq!(snapshot(&fixture.root), before);
}

#[test]
fn restore_uses_the_global_lease_before_repository_writes() {
    let fixture = Fixture::initialized();
    let before = snapshot(&fixture.root);
    let lease = UserLease::acquire(
        &fixture.runtime,
        LeaseRecord::new(
            ProcessIdentity::current().unwrap(),
            [0x2a; 32],
            "test",
            1_788_400_000,
        )
        .unwrap(),
    )
    .unwrap();

    let outcome = execute_restore(&fixture.root, &fixture.runtime, &restore(false, false));

    assert_eq!(outcome.report.result.code, 73);
    assert_eq!(outcome.report.result.reason, Reason::LockHeld);
    assert_eq!(snapshot(&fixture.root), before);
    drop(lease);
}

#[test]
fn force_never_accepts_incomplete_nested_or_unsupported_markers() {
    for invalid in [
        "<!-- agent-lowmem:start format=\"1\" content-sha256=\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" -->\nbody\n",
        "<!-- agent-lowmem:start format=\"2\" content-sha256=\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" -->\nbody\n<!-- agent-lowmem:end -->\n",
        "<!-- agent-lowmem:start format=\"1\" content-sha256=\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" -->\n<!-- agent-lowmem:start nested -->\n<!-- agent-lowmem:end -->\n",
    ] {
        let fixture = Fixture::initialized();
        fs::write(fixture.root.join("AGENTS.md"), invalid).unwrap();
        let before = snapshot(&fixture.root);

        let outcome = execute_restore(&fixture.root, &fixture.runtime, &restore(false, true));

        assert_eq!(outcome.report.outcome, ManagedOutcome::Conflict);
        assert_eq!(snapshot(&fixture.root), before);
    }
}

#[test]
fn restore_leaves_a_nonempty_owned_private_directory_after_removing_the_journal() {
    let fixture = Fixture::initialized();
    let keep = fixture.root.join(".git/agent-lowmem/keep");
    fs::write(&keep, "unrelated private data\n").unwrap();

    let outcome = execute_restore(&fixture.root, &fixture.runtime, &restore(false, false));

    assert_eq!(outcome.report.outcome, ManagedOutcome::Restored);
    assert!(fixture.root.join(".git/agent-lowmem").is_dir());
    assert_eq!(
        fs::read_to_string(keep).unwrap(),
        "unrelated private data\n"
    );
    assert!(
        !fixture
            .root
            .join(".git/agent-lowmem/restoration-v1.json")
            .exists()
    );
}

#[test]
fn restore_rejects_a_non_private_metadata_directory_before_writing() {
    let fixture = Fixture::initialized();
    let private = fixture.root.join(".git/agent-lowmem");
    fs::set_permissions(&private, fs::Permissions::from_mode(0o755)).unwrap();
    let before = snapshot(&fixture.root);

    let outcome = execute_restore(&fixture.root, &fixture.runtime, &restore(false, false));

    assert_eq!(outcome.report.outcome, ManagedOutcome::Conflict);
    assert_eq!(snapshot(&fixture.root), before);
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

fn restore(dry_run: bool, force_managed_block: bool) -> RestoreRequest {
    RestoreRequest {
        dry_run,
        force_managed_block,
        json: true,
    }
}

fn rewrite_block_with_valid_hash(path: &Path, body: &str) {
    let document = fs::read_to_string(path).unwrap();
    let start = document.find("<!-- agent-lowmem:start").unwrap();
    let line_end = document[start..].find('\n').unwrap() + start;
    let end = document.find("<!-- agent-lowmem:end -->").unwrap();
    let digest: [u8; 32] = Sha256::digest(body.as_bytes()).into();
    let digest = digest
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").unwrap();
            output
        });
    let marker = format!("<!-- agent-lowmem:start format=\"1\" content-sha256=\"{digest}\" -->\n");
    let mut edited = String::new();
    edited.push_str(&document[..start]);
    edited.push_str(&marker);
    edited.push_str(body);
    edited.push_str(&document[end..]);
    assert!(line_end > start);
    fs::write(path, edited).unwrap();
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
    fn initialized() -> Self {
        let fixture = Self::new();
        fixture.init();
        fixture
    }

    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "agent-lowmem-managed-restore-{nanos}-{}-{serial}",
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

    fn init(&self) {
        let outcome = execute_init(
            &SupportedHost::reference(),
            &self.root,
            &self.runtime,
            &InitRequest {
                dry_run: false,
                json: true,
            },
        );
        assert!(
            matches!(
                outcome.report.outcome,
                ManagedOutcome::Applied | ManagedOutcome::Unchanged
            ),
            "{outcome:?}"
        );
    }

    fn add_lint_operation(&self) {
        let tool = self.root.join("node_modules/eslint/package.json");
        fs::create_dir_all(tool.parent().unwrap()).unwrap();
        fs::write(tool, r#"{"name":"eslint","version":"10.9.1"}"#).unwrap();
        fs::write(
            self.root.join("package.json"),
            r#"{"name":"npm-single","private":true,"packageManager":"npm@12.0.2","scripts":{"test":"vitest run","lint":"eslint ."}}"#,
        )
        .unwrap();
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
