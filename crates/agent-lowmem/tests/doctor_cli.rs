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
    assert_eq!(report["phase"], "native-foundation");
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
    assert!(stdout.contains("Managed runs: unavailable in Phase 1"));
}

#[test]
fn invalid_arguments_exit_two() {
    let fixture = Fixture::empty();

    let output = agent_lowmem(fixture.path(), &["--json", "doctor"]);

    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn unavailable_run_exits_64_without_starting_a_repository_child() {
    let fixture = Fixture::git_repo();
    fixture.write(
        "package.json",
        r#"{"packageManager":"npm@11.11.0","scripts":{"test":"touch child-started"}}"#,
    );
    fixture.write("package-lock.json", "{}\n");

    let output = agent_lowmem(fixture.path(), &["run", "test"]);

    assert_eq!(output.status.code(), Some(64));
    assert!(!fixture.path().join("child-started").exists());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr.lines().last(),
        Some("agent-lowmem: result origin=preflight code=64 reason=operation-unsupported")
    );
}

#[cfg(unix)]
#[test]
fn zero_child_sentinels_detect_attempts_but_doctor_starts_none() {
    let fixture = Fixture::git_repo();
    fixture.write("package.json", r#"{"packageManager":"pnpm@10.33.0"}"#);
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

    let doctor = Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
        .arg("doctor")
        .current_dir(fixture.path())
        .env("PATH", isolated_path)
        .env("AGENT_LOWMEM_SENTINEL_MARKER", &marker)
        .output()
        .unwrap();

    assert_eq!(doctor.status.code(), Some(0));
    assert!(!marker.exists());
}

#[test]
fn zero_child_source_guard_rejects_launches_and_unsafe_blocks() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut source_files = Vec::new();
    collect_rust_files(&source_root, &mut source_files);

    for source_file in source_files {
        let source = fs::read_to_string(&source_file).unwrap();
        for forbidden in [
            "std::process::Command",
            "Command::new",
            "memorystatus_vm_pressure",
            "unsafe {",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} contains forbidden production token {forbidden}",
                source_file.display()
            );
        }
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
