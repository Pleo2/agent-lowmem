use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn empty() -> Self {
        let id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("agent-lowmem-doctor-cli-test-{timestamp}-{id}"));
        fs::create_dir_all(&root).unwrap();
        Self {
            root: fs::canonicalize(root).unwrap(),
        }
    }

    fn git_repo() -> Self {
        let fixture = Self::empty();
        fs::create_dir(fixture.root.join(".git")).unwrap();
        fixture
    }

    fn path(&self) -> &Path {
        &self.root
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
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn agent_lowmem(current_dir: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
        .args(arguments)
        .current_dir(current_dir)
        .output()
        .unwrap()
}

#[test]
fn doctor_json_exits_zero_and_redacts_paths_and_environment_values() {
    let fixture = Fixture::empty();
    let secret = format!(
        "agent-lowmem-secret-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

    let output = Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
        .args(["doctor", "--json"])
        .current_dir(fixture.path())
        .env("AGENT_LOWMEM_TEST_SECRET", &secret)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["phase"], "managed-files");
    assert!(report["managedRunsAvailable"].is_boolean());
    assert_eq!(report["initAvailable"], false);
    assert_eq!(report["restoreAvailable"], false);
    assert_eq!(
        report["nextAction"],
        "design the release and distribution phase"
    );
    assert!(matches!(
        report["lockStatus"].as_str(),
        Some("available" | "held" | "orphan-recovery" | "invalid-record")
    ));
    assert!(!stdout.contains(fixture.path().to_str().unwrap()));
    assert!(!stdout.contains(&secret));
    assert!(output.stderr.is_empty());
}

#[test]
fn human_doctor_reports_capabilities_and_phase_limit() {
    let fixture = Fixture::empty();

    let output = agent_lowmem(fixture.path(), &["doctor"]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Agent Lowmem doctor"));
    assert!(stdout.contains("Runtime supported:"));
    assert!(stdout.contains("Performance validated:"));
    assert!(stdout.contains("Repository available:"));
    assert!(stdout.contains("Managed runs: unavailable"));
    assert!(stdout.contains("Init: unavailable"));
    assert!(stdout.contains("Restore: unavailable"));
    assert!(stdout.contains("Operation lock:"));
}

#[test]
fn doctor_reports_restore_for_a_conflicting_managed_identity() {
    let fixture = Fixture::git_repo();
    fixture.write("AGENTS.md", "<!-- agent-lowmem:start\n");

    let output = agent_lowmem(fixture.path(), &["doctor", "--json"]);

    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["restoreAvailable"], true);
}

#[test]
fn invalid_arguments_exit_two() {
    let fixture = Fixture::empty();

    let output = agent_lowmem(fixture.path(), &["--json", "doctor"]);

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn managed_file_commands_are_dispatched_by_the_cli() {
    let unsupported = Fixture::empty();
    let init = agent_lowmem(unsupported.path(), &["init", "--dry-run", "--json"]);
    assert_eq!(init.status.code(), Some(64));
    assert!(init.stdout.starts_with(b"{"));
    assert_eq!(
        String::from_utf8(init.stderr).unwrap().lines().last(),
        Some(
            "agent-lowmem: managed-files command=init outcome=failed code=64 reason=repository-unsupported"
        )
    );

    let repository = Fixture::git_repo();
    let restore = agent_lowmem(repository.path(), &["restore", "--dry-run", "--json"]);
    assert_eq!(restore.status.code(), Some(0));
    assert!(restore.stdout.starts_with(b"{"));
    assert_eq!(
        String::from_utf8(restore.stderr).unwrap().lines().last(),
        Some(
            "agent-lowmem: managed-files command=restore outcome=unchanged code=0 reason=completed"
        )
    );
}

#[test]
fn unconfigured_run_exits_two_without_starting_a_repository_child() {
    let fixture = Fixture::git_repo();
    fixture.write(
        "package.json",
        r#"{"packageManager":"npm@12.0.2","scripts":{"test":"touch child-started"}}"#,
    );
    fixture.write("package-lock.json", "{}\n");

    let output = agent_lowmem(fixture.path(), &["run", "test"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(!fixture.path().join("child-started").exists());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr.lines().last(),
        Some("agent-lowmem: result origin=preflight code=2 reason=invalid-config")
    );
}

#[cfg(unix)]
#[test]
fn zero_child_sentinels_detect_attempts_but_doctor_starts_none() {
    let fixture = Fixture::git_repo();
    fixture.write("package.json", r#"{"packageManager":"pnpm@11.25.0"}"#);
    fixture.write("pnpm-lock.yaml", "lockfileVersion: '9.0'\n");
    let sentinel_dir = fixture.path().join("sentinels");
    fs::create_dir(&sentinel_dir).unwrap();
    let marker = fixture.path().join("child-process-started");
    let sentinel_script =
        "#!/bin/sh\nprintf '%s\\n' \"$0\" >> \"$AGENT_LOWMEM_SENTINEL_MARKER\"\nexit 97\n";

    for name in ["git", "node", "npm", "pnpm"] {
        let sentinel = sentinel_dir.join(name);
        fs::write(&sentinel, sentinel_script).unwrap();
        let mut permissions = fs::metadata(&sentinel).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&sentinel, permissions).unwrap();
    }

    let isolated_path = format!("{}:/usr/bin:/bin", sentinel_dir.display());
    let self_check = Command::new(sentinel_dir.join("git"))
        .env("PATH", &isolated_path)
        .env("AGENT_LOWMEM_SENTINEL_MARKER", &marker)
        .output()
        .unwrap();
    assert_eq!(self_check.status.code(), Some(97));
    assert!(marker.exists());
    fs::remove_file(&marker).unwrap();
    let before = snapshot(&fixture.root);
    let isolated_temporary = fixture.path().join("isolated-tmp");
    fs::create_dir(&isolated_temporary).unwrap();

    let doctor = Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
        .arg("doctor")
        .current_dir(fixture.path())
        .env("PATH", isolated_path)
        .env("TMPDIR", &isolated_temporary)
        .env("AGENT_LOWMEM_SENTINEL_MARKER", &marker)
        .output()
        .unwrap();

    assert_eq!(doctor.status.code(), Some(0));
    assert!(!marker.exists());
    assert!(!isolated_temporary.join("agent-lowmem-v1").exists());
    assert!(!fixture.path().join(".git/agent-lowmem").exists());
    let mut expected = before;
    expected.push(("isolated-tmp".to_owned(), Vec::new()));
    expected.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(snapshot(&fixture.root), expected);
}

#[test]
fn source_guard_confines_launches_and_rejects_unsafe_blocks() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut source_files = Vec::new();
    collect_rust_files(&source_root, &mut source_files);

    for source_file in source_files {
        let source = fs::read_to_string(&source_file).unwrap();
        let is_managed_process_boundary =
            source_file.strip_prefix(&source_root).unwrap() == Path::new("process.rs");
        for forbidden in [
            "git config",
            "node --version",
            "npm config",
            "pnpm config",
            "Command::new(\"sh\")",
            "Command::new(\"bash\")",
            "Command::new(\"zsh\")",
            ".arg(\"-c\")",
            "std::net",
            "TcpStream",
            "UdpSocket",
            "memorystatus_vm_pressure",
            "proc_listallpids",
            "proc_listpids",
            "sysinfo::System",
            "std::env::vars",
            ".envs(",
            "unsafe {",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} contains forbidden production token {forbidden}",
                source_file.display()
            );
        }
        if !is_managed_process_boundary {
            for forbidden in ["std::process::Command", "Command::new"] {
                assert!(
                    !source.contains(forbidden),
                    "{} starts a child outside the managed process boundary via {forbidden}",
                    source_file.display()
                );
            }
        }
    }

    let manifest =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    for forbidden_dependency in [
        "tokio",
        "async-std",
        "smol",
        "reqwest",
        "ureq",
        "hyper",
        "isahc",
        "surf",
    ] {
        assert!(
            !manifest.contains(forbidden_dependency),
            "Cargo.toml contains forbidden runtime or network dependency {forbidden_dependency}"
        );
    }
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}

fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn collect(root: &Path, current: &Path, entries: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if entry.file_type().unwrap().is_dir() {
                entries.push((relative, Vec::new()));
                collect(root, &path, entries);
            } else {
                entries.push((relative, fs::read(path).unwrap()));
            }
        }
    }

    let mut entries = Vec::new();
    collect(root, root, &mut entries);
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}
