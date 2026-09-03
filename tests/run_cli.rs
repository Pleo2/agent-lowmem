use agent_lowmem::{
    lock::{LeaseRecord, ProcessIdentity, UserLease},
    run::runtime_directory,
};
use std::{
    fs,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    marker: PathBuf,
}

impl Fixture {
    fn npm() -> Self {
        let fixture = Self::empty();
        fs::create_dir_all(fixture.root.join("node_modules/vitest")).unwrap();
        fs::write(
            fixture.root.join("package.json"),
            r#"{"name":"fixture","packageManager":"npm@12.0.2","scripts":{"test":"vitest run"}}"#,
        )
        .unwrap();
        fs::write(fixture.root.join("package-lock.json"), "{}\n").unwrap();
        fs::write(
            fixture.root.join(".agent-lowmem.json"),
            r#"{"version":1,"packageManager":"npm","operations":{"test":{"script":"test","timeoutSeconds":300}}}"#,
        )
        .unwrap();
        fs::write(
            fixture.root.join("node_modules/vitest/package.json"),
            r#"{"name":"vitest","version":"4.1.11"}"#,
        )
        .unwrap();
        fixture
    }

    fn pnpm_workspace() -> Self {
        let fixture = Self::empty();
        fs::create_dir_all(fixture.root.join("apps/web/node_modules/vitest")).unwrap();
        fs::write(
            fixture.root.join("package.json"),
            r#"{"name":"fixture","packageManager":"pnpm@11.25.0"}"#,
        )
        .unwrap();
        fs::write(
            fixture.root.join("pnpm-lock.yaml"),
            "lockfileVersion: '9.0'\n",
        )
        .unwrap();
        fs::write(
            fixture.root.join("pnpm-workspace.yaml"),
            "packages:\n  - apps/*\n",
        )
        .unwrap();
        fs::write(
            fixture.root.join("apps/web/package.json"),
            r#"{"name":"@fixture/web","scripts":{"test":"vitest run"}}"#,
        )
        .unwrap();
        fs::write(
            fixture
                .root
                .join("apps/web/node_modules/vitest/package.json"),
            r#"{"name":"vitest","version":"4.1.11"}"#,
        )
        .unwrap();
        fs::write(
            fixture.root.join(".agent-lowmem.json"),
            r#"{"version":1,"packageManager":"pnpm","workspaces":{"web":{"path":"apps/web","packageName":"@fixture/web","operations":{"test":{"script":"test","timeoutSeconds":300}}}}}"#,
        )
        .unwrap();
        fixture
    }

    fn disclosed() -> Self {
        let fixture = Self::empty();
        fs::create_dir_all(fixture.root.join("node_modules/next")).unwrap();
        fs::write(
            fixture.root.join("package.json"),
            r#"{"name":"fixture","packageManager":"npm@12.0.2","scripts":{"build":"next build"}}"#,
        )
        .unwrap();
        fs::write(fixture.root.join("package-lock.json"), "{}\n").unwrap();
        fs::write(
            fixture.root.join(".agent-lowmem.json"),
            r#"{"version":1,"packageManager":"npm","operations":{"build":{"script":"build","timeoutSeconds":300}}}"#,
        )
        .unwrap();
        fs::write(
            fixture.root.join("node_modules/next/package.json"),
            r#"{"name":"next","version":"16.3.4"}"#,
        )
        .unwrap();
        fixture
    }

    fn empty() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!(
            "agent-lowmem-run-cli-{}-{serial}",
            std::process::id()
        ));
        let root = base.join("repository");
        fs::create_dir_all(root.join(".git")).unwrap();
        Self {
            marker: base.join("child-started"),
            root: fs::canonicalize(root).unwrap(),
            base,
        }
    }

    fn run(&self, arguments: &[&str]) -> Output {
        let runner = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runner");
        Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
            .args(arguments)
            .current_dir(&self.root)
            .env("PATH", runner)
            .env("AGENT_LOWMEM_SENTINEL_MARKER", &self.marker)
            .env("NO_COLOR", "1")
            .env("TERM", "xterm-256color")
            .env_remove("COLORTERM")
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn runs_npm_success_and_failure_with_one_stable_final_line() {
    let fixture = Fixture::npm();
    let secret_argument = "SECRET-FORWARDED-ARGUMENT";
    let success = fixture.run(&[
        "run",
        "test",
        "--json-file",
        "result.json",
        "--",
        secret_argument,
    ]);
    assert_eq!(success.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&success.stdout),
        "fixture-child-stdout\n"
    );
    let stderr = String::from_utf8(success.stderr).unwrap();
    assert!(stderr.starts_with("agent_lowmem\n"));
    assert!(stderr.contains("fixture-child-stderr\n"));
    assert_eq!(stderr.matches("agent-lowmem: result ").count(), 1);
    assert!(stderr.ends_with("agent-lowmem: result origin=child code=0 reason=completed\n"));
    assert!(!stderr.contains('\u{1b}'));
    assert!(fixture.marker.exists());

    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.root.join("result.json")).unwrap()).unwrap();
    assert_eq!(result["origin"], "child");
    assert_eq!(result["code"], 0);
    assert_eq!(result["childStarted"], true);
    assert_eq!(result["details"]["spawnState"], "started");
    assert_eq!(result["details"]["forwardedArgumentCount"], 1);
    let serialized_result = serde_json::to_string(&result).unwrap();
    assert!(!serialized_result.contains('\u{1b}'));
    assert!(!serialized_result.contains(secret_argument));

    fs::remove_file(&fixture.marker).unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
        .args(["run", "test"])
        .current_dir(&fixture.root)
        .env(
            "PATH",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runner"),
        )
        .env("AGENT_LOWMEM_SENTINEL_MARKER", &fixture.marker)
        .env("AGENT_LOWMEM_FIXTURE_EXIT", "17")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(17));
    let stderr = String::from_utf8(failed.stderr).unwrap();
    assert_eq!(stderr.matches("agent-lowmem: result ").count(), 1);
    assert!(stderr.ends_with("agent-lowmem: result origin=child code=17 reason=child-exit\n"));
}

#[test]
fn runs_the_exact_configured_pnpm_workspace() {
    let fixture = Fixture::pnpm_workspace();
    let output = fixture.run(&["run", "test", "--workspace", "web"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(fixture.marker.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .ends_with("agent-lowmem: result origin=child code=0 reason=completed\n")
    );
}

#[test]
fn emits_disclosure_and_blocks_nested_lock_and_spawn_failures_before_child_success() {
    let disclosed = Fixture::disclosed();
    let output = disclosed.run(&["run", "build"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(stderr.contains("agent-lowmem: disclosure internal-fanout-uncontrolled\n"));

    let nested = Fixture::npm();
    let output = Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
        .args(["run", "test"])
        .current_dir(&nested.root)
        .env("AGENT_LOWMEM_ACTIVE", "1")
        .env("AGENT_LOWMEM_SENTINEL_MARKER", &nested.marker)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(73));
    assert!(!nested.marker.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .ends_with("agent-lowmem: result origin=preflight code=73 reason=nested-invocation\n")
    );

    let spawn = Fixture::npm();
    let empty_path = spawn.base.join("empty-path");
    fs::create_dir(&empty_path).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
        .args(["run", "test", "--json-file", "spawn-failure.json"])
        .current_dir(&spawn.root)
        .env("PATH", empty_path)
        .env("AGENT_LOWMEM_SENTINEL_MARKER", &spawn.marker)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(70));
    assert!(!spawn.marker.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .ends_with("agent-lowmem: result origin=internal code=70 reason=internal-error\n")
    );
    let result: serde_json::Value =
        serde_json::from_slice(&fs::read(spawn.root.join("spawn-failure.json")).unwrap()).unwrap();
    assert_eq!(result["childStarted"], false);
    assert_eq!(result["details"]["spawnState"], "failed");
    assert_eq!(result["details"]["cleanupAction"], "none");
    assert_eq!(result["details"]["cleanupComplete"], true);
}

#[test]
fn a_live_global_lease_blocks_the_cli_before_spawn() {
    let fixture = Fixture::npm();
    let runtime = runtime_directory().unwrap();
    let _lease = UserLease::acquire(
        &runtime,
        LeaseRecord::new(
            ProcessIdentity::current().unwrap(),
            [9; 32],
            "test",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        )
        .unwrap(),
    )
    .unwrap();

    let doctor = fixture.run(&["doctor", "--json"]);
    let report: serde_json::Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(report["lockStatus"], "held");

    let output = fixture.run(&["run", "test"]);

    assert_eq!(output.status.code(), Some(73));
    assert!(!fixture.marker.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .ends_with("agent-lowmem: result origin=preflight code=73 reason=lock-held\n")
    );
}

#[test]
fn invalid_run_syntax_emits_only_one_unstyled_final_result_line() {
    let fixture = Fixture::empty();
    let output = fixture.run(&["run"]);

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "agent-lowmem: result origin=preflight code=2 reason=invalid-cli\n"
    );
}

#[test]
fn a_late_json_conflict_warns_without_replacing_the_child_result() {
    let fixture = Fixture::npm();
    let result_path = fixture.root.join("result.json");
    let output = Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
        .args(["run", "test", "--json-file", "result.json"])
        .current_dir(&fixture.root)
        .env(
            "PATH",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runner"),
        )
        .env("AGENT_LOWMEM_SENTINEL_MARKER", &fixture.marker)
        .env("AGENT_LOWMEM_RESULT_CONFLICT", &result_path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("agent-lowmem: warning structured result could not be written\n"));
    assert!(stderr.ends_with("agent-lowmem: result origin=child code=0 reason=completed\n"));
    assert_eq!(stderr.matches("agent-lowmem: result ").count(), 1);
}

#[test]
fn external_sigint_is_forwarded_reported_and_reraised() {
    let fixture = Fixture::npm();
    let runner = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runner");
    let child = Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
        .args(["run", "test"])
        .current_dir(&fixture.root)
        .env("PATH", runner)
        .env("AGENT_LOWMEM_SENTINEL_MARKER", &fixture.marker)
        .env("AGENT_LOWMEM_FIXTURE_WAIT", "1")
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    for _ in 0..200 {
        if fixture.marker.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(fixture.marker.exists());
    let pid = rustix::process::Pid::from_raw(child.id() as i32).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::INT).unwrap();
    let output = child.wait_with_output().unwrap();

    assert_eq!(output.status.signal(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.ends_with(
        "agent-lowmem: result origin=external-signal code=130 reason=external-signal\n"
    ));
    assert_eq!(stderr.matches("agent-lowmem: result ").count(), 1);
}
