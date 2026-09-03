use agent_lowmem::lock::{LockProbe, LockStatus};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
const SAMPLES: usize = 20;

#[test]
#[ignore = "release-only managed-files warm-cache measurement"]
fn managed_files_warm_cache_measurements_leave_no_child_and_release_the_lease() {
    let fixture = Fixture::new();

    measure(&fixture, "init-dry-run", &["init", "--dry-run", "--json"]);
    fixture.run_success(&["init", "--json"]);
    measure(
        &fixture,
        "restore-dry-run",
        &["restore", "--dry-run", "--json"],
    );
    measure(&fixture, "init-unchanged", &["init", "--json"]);
    fixture.run_success(&["restore", "--json"]);
    measure(&fixture, "restore-unchanged", &["restore", "--json"]);

    assert!(!fixture.child_marker.exists());
    assert_eq!(LockProbe::probe(&fixture.runtime), LockStatus::Available);
}

fn measure(fixture: &Fixture, operation: &str, arguments: &[&str]) {
    fixture.run_success(arguments);
    let mut elapsed = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        fixture.run_success(arguments);
        elapsed.push(started.elapsed());
    }
    elapsed.sort();
    let median = milliseconds(elapsed[9]);
    let p95 = milliseconds(elapsed[18]);
    eprintln!(
        "managed-files-budget operation={operation} samples={SAMPLES} median_ms={median:.3} p95_ms={p95:.3}"
    );
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    temporary: PathBuf,
    sentinels: PathBuf,
    runtime: PathBuf,
    child_marker: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "agent-lowmem-managed-budget-{nanos}-{}-{serial}",
            std::process::id()
        ));
        let root = base.join("repository");
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repositories/npm-single"),
            &root,
        );
        fs::create_dir(root.join(".git")).unwrap();
        let temporary = base.join("tmp");
        let sentinels = base.join("sentinels");
        fs::create_dir(&temporary).unwrap();
        fs::create_dir(&sentinels).unwrap();
        let child_marker = base.join("child-started");
        let sentinel = "#!/bin/sh\nprintf child >> \"$AGENT_LOWMEM_SENTINEL_MARKER\"\nexit 97\n";
        for executable in ["git", "node", "npm", "pnpm"] {
            let path = sentinels.join(executable);
            fs::write(&path, sentinel).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let runtime = fs::canonicalize(&temporary)
            .unwrap()
            .join("agent-lowmem-v1");
        Self {
            base,
            root: fs::canonicalize(root).unwrap(),
            temporary,
            sentinels,
            runtime,
            child_marker,
        }
    }

    fn run_success(&self, arguments: &[&str]) {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
            .args(arguments)
            .current_dir(&self.root)
            .env("TMPDIR", &self.temporary)
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.sentinels.display()),
            )
            .env("AGENT_LOWMEM_SENTINEL_MARKER", &self.child_marker)
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "command {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(serde_json::from_slice::<serde_json::Value>(&output.stdout).is_ok());
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
