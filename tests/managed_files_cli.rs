use std::{
    fs::{self},
    os::{fd::OwnedFd, unix::net::UnixStream},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn json_init_and_restore_emit_one_closed_report_and_one_stable_line() {
    let fixture = Fixture::new();

    let initialized = fixture.run(&["init", "--json"]);

    assert_eq!(initialized.status.code(), Some(0));
    let init: serde_json::Value = serde_json::from_slice(&initialized.stdout).unwrap();
    assert_eq!(init["schemaVersion"], 1);
    assert_eq!(init["command"], "init");
    assert_eq!(init["dryRun"], false);
    assert_eq!(init["outcome"], "applied");
    assert_eq!(init["result"]["code"], 0);
    assert_eq!(init["result"]["reason"], "completed");
    assert!(!String::from_utf8_lossy(&initialized.stdout).contains('\u{1b}'));
    assert_eq!(
        String::from_utf8(initialized.stderr).unwrap(),
        "agent-lowmem: managed-files command=init outcome=applied code=0 reason=completed\n"
    );

    let restored = fixture.run(&["restore", "--json"]);

    assert_eq!(restored.status.code(), Some(0));
    let restore: serde_json::Value = serde_json::from_slice(&restored.stdout).unwrap();
    assert_eq!(restore["command"], "restore");
    assert_eq!(restore["outcome"], "restored");
    assert_eq!(
        String::from_utf8(restored.stderr).unwrap(),
        "agent-lowmem: managed-files command=restore outcome=restored code=0 reason=completed\n"
    );
    assert!(!fixture.root.join(".agent-lowmem.json").exists());
    assert!(!fixture.root.join("AGENTS.md").exists());
}

#[test]
fn human_apply_uses_branding_while_json_and_dry_run_do_not() {
    let human = Fixture::new();
    let applied = human.run(&["init"]);
    let stdout = String::from_utf8(applied.stdout).unwrap();
    assert_eq!(applied.status.code(), Some(0));
    assert!(stdout.starts_with("agent_lowmem\nManaged files: init (applied)\n"));
    assert!(!stdout.contains('\u{1b}'));

    let dry_run = Fixture::new();
    let planned = dry_run.run(&["init", "--dry-run"]);
    let stdout = String::from_utf8(planned.stdout).unwrap();
    assert_eq!(planned.status.code(), Some(0));
    assert!(stdout.starts_with("Managed files: init (planned)\n"));
    assert!(!stdout.contains("agent_lowmem"));

    let json = Fixture::new();
    let output = json.run(&["init", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("agent_lowmem")
    );
}

#[test]
fn dry_run_human_output_is_a_bounded_unified_diff() {
    let fixture = Fixture::new();
    fs::write(
        fixture.root.join("AGENTS.md"),
        "PRIVATE PREFIX WITHOUT OUTPUT\n",
    )
    .unwrap();

    let output = fixture.run(&["init", "--dry-run"]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout,
        concat!(
            "Managed files: init (planned)\n\n",
            "--- /dev/null\n",
            "+++ b/.agent-lowmem.json\n",
            "@@ -0,0 +1,11 @@\n",
            "+{\n",
            "+  \"$schema\": \"https://agentlowmem.dev/schema/v1.json\",\n",
            "+  \"version\": 1,\n",
            "+  \"packageManager\": \"npm\",\n",
            "+  \"operations\": {\n",
            "+    \"test\": {\n",
            "+      \"script\": \"test\",\n",
            "+      \"timeoutSeconds\": 900\n",
            "+    }\n",
            "+  }\n",
            "+}\n",
            "\n",
            "--- a/AGENTS.md\n",
            "+++ b/AGENTS.md\n",
            "@@ -0,0 +2,13 @@\n",
            "+\n",
            "+<!-- agent-lowmem:start format=\"1\" content-sha256=\"36219c2e87999079ce2ab403e8b11234dac2ae8d07584c377f3d7d88fb2f9bfa\" -->\n",
            "+## Agent Lowmem resource policy\n",
            "+\n",
            "+Run supported heavy validation through Agent Lowmem. Run only one heavy\n",
            "+operation at a time, never use watch mode, and prefer focused validation\n",
            "+before broad suites. Do not retry OOM or timeout failures automatically.\n",
            "+Agent Lowmem v1 does not impose a memory cap or guarantee responsiveness;\n",
            "+use CI when a broad build cannot be constrained locally.\n",
            "+\n",
            "+Supported commands:\n",
            "+- `agent-lowmem run test`\n",
            "+<!-- agent-lowmem:end -->\n",
            "\n"
        )
    );
    assert!(!stdout.contains("PRIVATE PREFIX WITHOUT OUTPUT"));
    assert!(!stdout.contains("restoration-v1.json"));
    assert!(!stdout.contains(fixture.root.to_str().unwrap()));
    assert!(!stdout.contains("vitest run"));
}

#[test]
fn dry_run_diff_never_discloses_external_configuration_bytes() {
    let fixture = Fixture::new();
    let initialized = fixture.run(&["init", "--json"]);
    assert_eq!(initialized.status.code(), Some(0));

    let configuration_path = fixture.root.join(".agent-lowmem.json");
    let mut configuration: serde_json::Value =
        serde_json::from_slice(&fs::read(&configuration_path).unwrap()).unwrap();
    configuration["operations"]["test"]["timeoutSeconds"] = 777.into();
    fs::write(
        &configuration_path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&configuration).unwrap()
        ),
    )
    .unwrap();
    fs::remove_dir_all(fixture.root.join(".git/agent-lowmem")).unwrap();
    fs::remove_file(fixture.root.join("AGENTS.md")).unwrap();

    let output = fixture.run(&["init", "--dry-run"]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.contains("\"timeoutSeconds\": 777"));
    assert!(!stdout.contains("--- a/.agent-lowmem.json"));
    assert!(stdout.contains("+++ b/AGENTS.md"));
}

#[test]
fn force_restore_combination_reaches_the_orchestrator() {
    let fixture = Fixture::new();
    let output = fixture.run(&["restore", "--force-managed-block", "--dry-run", "--json"]);

    assert_eq!(output.status.code(), Some(0));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["command"], "restore");
    assert_eq!(report["dryRun"], true);
    assert_eq!(report["outcome"], "unchanged");
}

#[test]
fn managed_file_failures_use_the_closed_exit_mapping() {
    let unsupported = Fixture::empty();
    let missing_repository = unsupported.run(&["restore", "--json"]);
    assert_eq!(missing_repository.status.code(), Some(64));
    assert!(missing_repository.stdout.starts_with(b"{"));
    assert!(
        String::from_utf8(missing_repository.stderr)
            .unwrap()
            .ends_with("code=64 reason=repository-unsupported\n")
    );

    let conflict = Fixture::new();
    fs::write(conflict.root.join("AGENTS.md"), "<!-- agent-lowmem:start\n").unwrap();
    let conflicted = conflict.run(&["restore", "--json"]);
    assert_eq!(conflicted.status.code(), Some(78));
    assert!(
        String::from_utf8(conflicted.stderr)
            .unwrap()
            .ends_with("code=78 reason=managed-file-conflict\n")
    );
}

#[test]
fn output_failure_before_writes_returns_70_but_after_writes_preserves_success() {
    let before = Fixture::new();
    let failed_preview = before.run_with_stdout(&["init", "--dry-run", "--json"], broken_stdout());
    assert_eq!(failed_preview.status.code(), Some(70));
    assert!(!before.root.join(".agent-lowmem.json").exists());
    assert!(!before.root.join("AGENTS.md").exists());
    let stderr = String::from_utf8(failed_preview.stderr).unwrap();
    assert!(stderr.contains("agent-lowmem: warning managed-files output could not be written\n"));
    assert!(stderr.ends_with(
        "agent-lowmem: managed-files command=init outcome=failed code=70 reason=internal-error\n"
    ));

    let after = Fixture::new();
    let failed_output = after.run_with_stdout(&["init", "--json"], broken_stdout());
    assert_eq!(failed_output.status.code(), Some(0));
    assert!(after.root.join(".agent-lowmem.json").is_file());
    assert!(after.root.join("AGENTS.md").is_file());
    let stderr = String::from_utf8(failed_output.stderr).unwrap();
    assert!(stderr.contains("agent-lowmem: warning managed-files output could not be written\n"));
    assert!(stderr.ends_with(
        "agent-lowmem: managed-files command=init outcome=applied code=0 reason=completed\n"
    ));
}

fn broken_stdout() -> Stdio {
    let (writer, reader) = UnixStream::pair().unwrap();
    drop(reader);
    Stdio::from(OwnedFd::from(writer))
}

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    temporary: PathBuf,
}

impl Fixture {
    fn empty() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "agent-lowmem-managed-cli-{nanos}-{}-{serial}",
            std::process::id()
        ));
        let root = base.join("repository");
        let temporary = base.join("tmp");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir(&temporary).unwrap();
        Self {
            root: fs::canonicalize(root).unwrap(),
            base,
            temporary,
        }
    }

    fn new() -> Self {
        let fixture = Self::empty();
        fs::remove_dir(&fixture.root).unwrap();
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repositories/npm-single"),
            &fixture.root,
        );
        fs::create_dir(fixture.root.join(".git")).unwrap();
        fixture
    }

    fn run(&self, arguments: &[&str]) -> Output {
        self.command(arguments).output().unwrap()
    }

    fn run_with_stdout(&self, arguments: &[&str], stdout: Stdio) -> Output {
        let mut command = self.command(arguments);
        command.stdout(stdout).stderr(Stdio::piped());
        command.spawn().unwrap().wait_with_output().unwrap()
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agent-lowmem"));
        command
            .args(arguments)
            .current_dir(&self.root)
            .env("TMPDIR", &self.temporary)
            .env("NO_COLOR", "1")
            .env("TERM", "xterm-256color")
            .env_remove("COLORTERM");
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
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
