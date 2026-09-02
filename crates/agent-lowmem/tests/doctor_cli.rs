use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

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
        fs::write(self.root.join(relative), contents).unwrap();
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
    let secret = "agent-lowmem-secret-must-not-leak";

    let output = Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
        .args(["doctor", "--json"])
        .current_dir(fixture.path())
        .env("AGENT_LOWMEM_TEST_SECRET", secret)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["phase"], "native-foundation");
    assert!(!stdout.contains(fixture.path().to_str().unwrap()));
    assert!(!stdout.contains(secret));
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
