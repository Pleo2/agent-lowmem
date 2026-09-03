use agent_lowmem::{
    result::{ExitResult, Origin, Reason},
    result_file::{RunResultRecord, write_result_atomic},
};
use rustix::fs::Mode;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn enforces_mode_0600_under_a_restrictive_umask() {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "restrictive_umask_child", "--nocapture"])
        .env("AGENT_LOWMEM_UMASK_CHILD", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn restrictive_umask_child() {
    if std::env::var_os("AGENT_LOWMEM_UMASK_CHILD").is_none() {
        return;
    }
    let fixture = Fixture::new();
    let previous = rustix::process::umask(Mode::RWXG | Mode::RWXO | Mode::WUSR);
    write_result_atomic(&fixture.0, "result.json", &record()).unwrap();
    rustix::process::umask(previous);
    assert_eq!(
        fs::metadata(fixture.0.join("result.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

fn record() -> RunResultRecord {
    RunResultRecord::at_unix_seconds(
        ExitResult::new(Origin::Child, 0, Reason::Completed),
        true,
        None,
        951_782_400,
    )
    .unwrap()
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "agent-lowmem-result-umask-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
