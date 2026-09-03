use agent_lowmem::{
    lock::{LeaseRecord, LockProbe, LockStatus, ProcessIdentity, UserLease},
    process::{
        GroupController, GroupPhase, ManagedSignal, NativeSignalSource, SignalSource, spawn_managed,
    },
    repository::{RunSelection, plan_run},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    runtime: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "agent-lowmem-process-{nanos}-{}-{serial}",
            std::process::id()
        ));
        let root = base.join("repository");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"name":"fixture","packageManager":"npm@12.0.2","scripts":{"test":"node --test capture.test.mjs"}}"#,
        )
        .unwrap();
        fs::write(root.join("package-lock.json"), "{}\n").unwrap();
        fs::write(
            root.join(".agent-lowmem.json"),
            r#"{"version":1,"packageManager":"npm","operations":{"test":{"script":"test","timeoutSeconds":300}}}"#,
        )
        .unwrap();
        fs::write(root.join(".node-version"), "24.14.1\n").unwrap();
        fs::write(
            root.join("capture.test.mjs"),
            r#"import { writeFileSync } from 'node:fs';
writeFileSync('child.json', JSON.stringify({
  cwd: process.cwd(),
  active: process.env.AGENT_LOWMEM_ACTIVE,
  pid: process.pid,
  ppid: process.ppid
}));
await new Promise(resolve => setTimeout(resolve, 1500));
"#,
        )
        .unwrap();
        Self {
            runtime: base.join("runtime"),
            root: fs::canonicalize(root).unwrap(),
        }
    }

    fn wait_for(&self, relative: &str) {
        for _ in 0..200 {
            if self.root.join(relative).exists() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out waiting for child evidence");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.root.parent().unwrap());
    }
}

fn lease(runtime: &Path) -> UserLease {
    UserLease::acquire(
        runtime,
        LeaseRecord::new(
            ProcessIdentity::current().unwrap(),
            [7; 32],
            "test",
            1_788_400_000,
        )
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn launches_the_exact_plan_in_a_new_owned_process_group() {
    let fixture = Fixture::new();
    let plan = plan_run(&fixture.root, &RunSelection::root("test", Vec::new())).unwrap();
    assert_eq!(plan.policy().launch.executable, "npm");
    assert_eq!(
        plan.policy().launch.arguments,
        [
            "--script-shell=/bin/sh",
            "run",
            "test",
            "--",
            "--test-concurrency=1",
        ]
    );
    let mut lease = lease(&fixture.runtime);

    let (mut child, mut signals) = spawn_managed(&plan, &mut lease).unwrap();
    fixture.wait_for("child.json");
    let evidence: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.root.join("child.json")).unwrap()).unwrap();

    assert_eq!(evidence["cwd"], fixture.root.to_str().unwrap());
    assert_eq!(evidence["active"], "1");
    assert_eq!(child.group().id(), child.id() as i32);
    assert!(child.group().is_live());
    assert_eq!(LockProbe::probe(&fixture.runtime), LockStatus::Held);
    let lock_record: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.runtime.join("operation.lock")).unwrap()).unwrap();
    assert_eq!(
        lock_record["childGroup"]["processGroupId"],
        child.id() as i32
    );
    assert!(child.wait().unwrap().success());
    lease.clear_child_group().unwrap();
    signals.shutdown().unwrap();
}

#[test]
fn signals_only_the_owned_group() {
    let fixture = Fixture::new();
    let plan = plan_run(&fixture.root, &RunSelection::root("test", Vec::new())).unwrap();
    let mut lease = lease(&fixture.runtime);
    let (mut child, mut signals) = spawn_managed(&plan, &mut lease).unwrap();
    fixture.wait_for("child.json");

    child
        .group()
        .send(ManagedSignal::Terminate, GroupPhase::LeaderExpected)
        .unwrap();
    let status = child.wait().unwrap();

    assert!(!status.success());
    lease.clear_child_group().unwrap();
    signals.shutdown().unwrap();
}

#[test]
fn signal_listener_can_be_closed_and_joined_without_a_signal() {
    let mut source = NativeSignalSource::install().unwrap();

    assert_eq!(source.try_recv(), None);
    source.shutdown().unwrap();
}
