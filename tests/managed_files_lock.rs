use agent_lowmem::{
    cli::InitRequest,
    host::{HostReadError, HostSource},
    lock::{LeaseRecord, ProcessIdentity, UserLease},
    managed_files::execute_init,
    result::Reason,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn init_uses_the_same_global_lease_and_stops_before_repository_writes() {
    let fixture = Fixture::new();
    let lease = UserLease::acquire(
        &fixture.runtime,
        LeaseRecord::new(
            ProcessIdentity::current().unwrap(),
            [0x2a; 32],
            "test",
            1_788_400_000,
        )
        .unwrap(),
    )
    .unwrap();

    let outcome = execute_init(
        &SupportedHost::reference(),
        &fixture.root,
        &fixture.runtime,
        &InitRequest {
            dry_run: false,
            json: true,
        },
    );

    assert_eq!(outcome.report.result.code, 73);
    assert_eq!(outcome.report.result.reason, Reason::LockHeld);
    assert!(!fixture.root.join(".agent-lowmem.json").exists());
    assert!(!fixture.root.join("AGENTS.md").exists());
    assert!(!fixture.root.join(".git/agent-lowmem").exists());
    drop(lease);
}

#[test]
fn nested_init_is_rejected_before_runtime_or_repository_writes() {
    let fixture = Fixture::new();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "nested_init_child", "--nocapture"])
        .env("AGENT_LOWMEM_ACTIVE", "1")
        .env("AGENT_LOWMEM_NESTED_ROOT", &fixture.root)
        .env("AGENT_LOWMEM_NESTED_RUNTIME", &fixture.runtime)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!fixture.runtime.exists());
    assert!(!fixture.root.join(".agent-lowmem.json").exists());
    assert!(!fixture.root.join("AGENTS.md").exists());
    assert!(!fixture.root.join(".git/agent-lowmem").exists());
}

#[test]
fn nested_init_child() {
    let (Ok(root), Ok(runtime)) = (
        std::env::var("AGENT_LOWMEM_NESTED_ROOT"),
        std::env::var("AGENT_LOWMEM_NESTED_RUNTIME"),
    ) else {
        return;
    };
    let outcome = execute_init(
        &SupportedHost::reference(),
        Path::new(&root),
        Path::new(&runtime),
        &InitRequest {
            dry_run: false,
            json: true,
        },
    );
    assert_eq!(outcome.report.result.code, 73);
    assert_eq!(outcome.report.result.reason, Reason::NestedInvocation);
}

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
            "agent-lowmem-managed-lock-{nanos}-{}-{serial}",
            std::process::id()
        ));
        let root = base.join("repository");
        copy_tree(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repositories/npm-single"),
            &root,
        );
        fs::create_dir(root.join(".git")).unwrap();
        Self {
            root: fs::canonicalize(root).unwrap(),
            runtime: base.join("runtime"),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.root.parent().unwrap());
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
