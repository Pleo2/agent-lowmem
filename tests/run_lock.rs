use agent_lowmem::{
    lock::{ChildGroupIdentity, LeaseRecord, LockProbe, LockStatus, ProcessIdentity, UserLease},
    result::Reason,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Duration,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "agent-lowmem-lock-{nanos}-{}-{serial}",
            std::process::id()
        ));
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn record() -> LeaseRecord {
    LeaseRecord::new(
        ProcessIdentity::current().unwrap(),
        [0x2a; 32],
        "test",
        1_788_400_000,
    )
    .unwrap()
}

#[test]
fn exclusive_descriptor_lease_blocks_until_drop() {
    let fixture = Fixture::new();
    let first = UserLease::acquire(fixture.path(), record()).unwrap();

    assert_eq!(
        UserLease::acquire(fixture.path(), record()).unwrap_err(),
        Reason::LockHeld
    );
    assert_eq!(LockProbe::probe(fixture.path()), LockStatus::Held);

    drop(first);
    assert!(UserLease::acquire(fixture.path(), record()).is_ok());
}

#[cfg(unix)]
#[test]
fn creates_private_runtime_directory_and_lock_file() {
    let fixture = Fixture::new();
    let _lease = UserLease::acquire(fixture.path(), record()).unwrap();

    assert_eq!(
        fs::metadata(fixture.path()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(fixture.path().join("operation.lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn malformed_unlocked_record_fails_closed() {
    let fixture = Fixture::new();
    let lease = UserLease::acquire(fixture.path(), record()).unwrap();
    drop(lease);
    fs::write(fixture.path().join("operation.lock"), b"not-json\n").unwrap();

    assert_eq!(LockProbe::probe(fixture.path()), LockStatus::InvalidRecord);
    assert_eq!(
        UserLease::acquire(fixture.path(), record()).unwrap_err(),
        Reason::LockHeld
    );
}

#[test]
fn live_exact_child_group_becomes_orphan_recovery() {
    let fixture = Fixture::new();
    let group = rustix::process::getpgrp().as_raw_nonzero().get();
    let leader = ProcessIdentity::for_pid(group).unwrap();
    let mut lease = UserLease::acquire(fixture.path(), record()).unwrap();
    lease
        .set_child_group(ChildGroupIdentity::new(group, leader.start_identity()))
        .unwrap();
    drop(lease);

    assert_eq!(LockProbe::probe(fixture.path()), LockStatus::OrphanRecovery);
    assert_eq!(
        UserLease::acquire(fixture.path(), record()).unwrap_err(),
        Reason::LockHeld
    );
}

#[test]
fn stale_child_identity_is_replaced_and_record_debug_is_redacted() {
    let fixture = Fixture::new();
    let group = rustix::process::getpgrp().as_raw_nonzero().get();
    let leader = ProcessIdentity::for_pid(group).unwrap();
    let mut lease = UserLease::acquire(fixture.path(), record()).unwrap();
    lease
        .set_child_group(ChildGroupIdentity::new(
            group,
            leader.start_identity().saturating_add(1),
        ))
        .unwrap();
    drop(lease);

    assert_eq!(LockProbe::probe(fixture.path()), LockStatus::Available);
    assert!(UserLease::acquire(fixture.path(), record()).is_ok());
    let debug = format!("{:?}", record());
    assert!(!debug.contains(&std::process::id().to_string()));
    assert!(!debug.contains("2a2a2a"));
    assert!(!debug.contains("1788400000"));
}

#[test]
fn clearing_a_child_group_allows_the_next_lease() {
    let fixture = Fixture::new();
    let group = rustix::process::getpgrp().as_raw_nonzero().get();
    let leader = ProcessIdentity::for_pid(group).unwrap();
    let mut lease = UserLease::acquire(fixture.path(), record()).unwrap();
    lease
        .set_child_group(ChildGroupIdentity::new(group, leader.start_identity()))
        .unwrap();
    lease.clear_child_group().unwrap();
    drop(lease);

    assert_eq!(LockProbe::probe(fixture.path()), LockStatus::Available);
    assert!(UserLease::acquire(fixture.path(), record()).is_ok());
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_lock_and_non_private_runtime_directory() {
    use std::os::unix::fs::symlink;

    let unsafe_mode = Fixture::new();
    fs::create_dir(unsafe_mode.path()).unwrap();
    fs::set_permissions(unsafe_mode.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        UserLease::acquire(unsafe_mode.path(), record()).unwrap_err(),
        Reason::LockHeld
    );

    let linked = Fixture::new();
    fs::create_dir(linked.path()).unwrap();
    fs::set_permissions(linked.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let outside = Fixture::new();
    fs::write(outside.path(), b"outside").unwrap();
    symlink(outside.path(), linked.path().join("operation.lock")).unwrap();
    assert_eq!(
        UserLease::acquire(linked.path(), record()).unwrap_err(),
        Reason::LockHeld
    );

    let dangling_runtime = Fixture::new();
    symlink(
        dangling_runtime.path().with_extension("missing"),
        dangling_runtime.path(),
    )
    .unwrap();
    assert_eq!(
        LockProbe::probe(dangling_runtime.path()),
        LockStatus::InvalidRecord
    );
}

#[test]
fn nested_marker_is_rejected_before_creating_runtime_state() {
    let fixture = Fixture::new();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "nested_marker_child", "--nocapture"])
        .env("AGENT_LOWMEM_ACTIVE", "1")
        .env("AGENT_LOWMEM_TEST_RUNTIME", fixture.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();

    assert!(output.success());
    assert!(!fixture.path().exists());
}

#[test]
fn nested_marker_child() {
    let Some(runtime) = std::env::var_os("AGENT_LOWMEM_TEST_RUNTIME") else {
        return;
    };
    assert_eq!(
        UserLease::acquire(Path::new(&runtime), record()).unwrap_err(),
        Reason::NestedInvocation
    );
}

#[test]
fn a_second_process_observes_the_live_advisory_lock() {
    let fixture = Fixture::new();
    let ready = fixture.path().with_extension("ready");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "lease_holder_child", "--nocapture"])
        .env("AGENT_LOWMEM_TEST_RUNTIME", fixture.path())
        .env("AGENT_LOWMEM_TEST_READY", &ready)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..200 {
        if ready.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists());

    assert_eq!(LockProbe::probe(fixture.path()), LockStatus::Held);
    assert_eq!(
        UserLease::acquire(fixture.path(), record()).unwrap_err(),
        Reason::LockHeld
    );

    child.kill().unwrap();
    child.wait().unwrap();
    let _ = fs::remove_file(ready);
}

#[test]
fn lease_holder_child() {
    let (Some(runtime), Some(ready)) = (
        std::env::var_os("AGENT_LOWMEM_TEST_RUNTIME"),
        std::env::var_os("AGENT_LOWMEM_TEST_READY"),
    ) else {
        return;
    };
    let _lease = UserLease::acquire(Path::new(&runtime), record()).unwrap();
    fs::write(ready, b"ready").unwrap();
    thread::sleep(Duration::from_secs(30));
}
