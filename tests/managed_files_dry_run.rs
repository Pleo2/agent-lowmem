use agent_lowmem::{
    cli::InitRequest,
    host::{HostReadError, HostSource},
    managed_files::execute_init,
};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn both_cli_dry_runs_create_no_runtime_private_temporary_or_managed_file() {
    let init = Fixture::new();
    let init_output = init.run_with_sentinels(&["init", "--dry-run", "--json"]);
    assert_eq!(init_output.status.code(), Some(0));
    init.assert_no_side_effects();

    let restore = Fixture::new();
    let initialized = execute_init(
        &SupportedHost::reference(),
        &restore.root,
        &restore.runtime,
        &InitRequest {
            dry_run: false,
            json: true,
        },
    );
    assert_eq!(initialized.report.result.code, 0);
    fs::remove_dir_all(&restore.runtime).unwrap();
    let before = snapshot(&restore.root);

    let restore_output = restore.run_with_sentinels(&["restore", "--dry-run", "--json"]);

    assert_eq!(restore_output.status.code(), Some(0));
    assert_eq!(snapshot(&restore.root), before);
    assert!(!restore.runtime.exists());
    assert!(!restore.marker.exists());
    assert_no_temporary_files(&restore.root);
}

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    runtime: PathBuf,
    temporary: PathBuf,
    sentinels: PathBuf,
    marker: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "agent-lowmem-managed-dry-run-{nanos}-{}-{serial}",
            std::process::id()
        ));
        let root = base.join("repository");
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repositories/npm-single"),
            &root,
        );
        fs::create_dir(root.join(".git")).unwrap();
        let temporary = base.join("tmp");
        let runtime = temporary.join("agent-lowmem-v1");
        let sentinels = base.join("sentinels");
        let marker = base.join("child-started");
        fs::create_dir(&temporary).unwrap();
        fs::create_dir(&sentinels).unwrap();
        let script = "#!/bin/sh\nprintf child >> \"$AGENT_LOWMEM_SENTINEL_MARKER\"\nexit 97\n";
        for name in ["git", "node", "npm", "pnpm"] {
            let path = sentinels.join(name);
            fs::write(&path, script).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self {
            base,
            root: fs::canonicalize(root).unwrap(),
            runtime,
            temporary,
            sentinels,
            marker,
        }
    }

    fn run_with_sentinels(&self, arguments: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
            .args(arguments)
            .current_dir(&self.root)
            .env("TMPDIR", &self.temporary)
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.sentinels.display()),
            )
            .env("AGENT_LOWMEM_SENTINEL_MARKER", &self.marker)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }

    fn assert_no_side_effects(&self) {
        assert!(!self.runtime.exists());
        assert!(!self.marker.exists());
        assert!(!self.root.join(".agent-lowmem.json").exists());
        assert!(!self.root.join("AGENTS.md").exists());
        assert!(!self.root.join(".git/agent-lowmem").exists());
        assert_no_temporary_files(&self.root);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn assert_no_temporary_files(root: &Path) {
    let mut files = Vec::new();
    collect_paths(root, root, &mut files);
    assert!(
        files
            .iter()
            .all(|path| !path.contains(".agent-lowmem.tmp.")),
        "temporary files: {files:?}"
    );
}

fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn collect_files(root: &Path, current: &Path, files: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(current).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            collect_files(root, &entry.path(), files);
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

fn collect_paths(root: &Path, current: &Path, paths: &mut Vec<String>) {
    for entry in fs::read_dir(current).unwrap() {
        let entry = entry.unwrap();
        paths.push(
            entry
                .path()
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        if entry.file_type().unwrap().is_dir() {
            collect_paths(root, &entry.path(), paths);
        }
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
