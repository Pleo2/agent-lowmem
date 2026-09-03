use agent_lowmem::{
    lock::{LeaseRecord, ProcessIdentity, UserLease},
    process::{SignalSource, spawn_managed},
    repository::{RunSelection, plan_run},
    supervisor::{InstantClock, SupervisionOutput, supervise},
};
use rustix::process::{Pid, test_kill_process};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const HELPER_MARKER: &str = "AGENT_LOWMEM_SUPERVISION_HELPER";
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    runtime: PathBuf,
    evidence: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = env::temp_dir().join(format!(
            "agent-lowmem-supervision-{nanos}-{}-{serial}",
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
        fs::write(root.join("capture.test.mjs"), "export {};\n").unwrap();
        Self {
            runtime: base.join("runtime"),
            evidence: base.join("pids"),
            root: fs::canonicalize(root).unwrap(),
            base,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

struct OwnedTestChild(Child);

impl Drop for OwnedTestChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Default)]
struct SilentOutput;

impl SupervisionOutput for SilentOutput {
    fn timeout_warning(&mut self) {}
}

#[test]
fn managed_supervision_helper() {
    if env::var_os(HELPER_MARKER).is_none() {
        return;
    }
    let root = PathBuf::from(env::var_os("AGENT_LOWMEM_FIXTURE_ROOT").unwrap());
    let runtime = PathBuf::from(env::var_os("AGENT_LOWMEM_FIXTURE_RUNTIME").unwrap());
    let timeout = Duration::from_millis(
        env::var("AGENT_LOWMEM_FIXTURE_TIMEOUT_MS")
            .unwrap()
            .parse()
            .unwrap(),
    );
    let plan = plan_run(&root, &RunSelection::root("test", Vec::new())).unwrap();
    let mut lease = UserLease::acquire(
        &runtime,
        LeaseRecord::new(
            ProcessIdentity::current().unwrap(),
            [8; 32],
            "test",
            1_788_400_000,
        )
        .unwrap(),
    )
    .unwrap();
    let (mut child, mut signals) = spawn_managed(&plan, &mut lease).unwrap();
    let group = *child.group();
    let clock = InstantClock::start();
    let mut output = SilentOutput;
    let report = supervise(
        &mut child,
        &group,
        &mut signals,
        &clock,
        &mut output,
        timeout,
    );
    if report.cleanup_complete {
        lease.clear_child_group().unwrap();
    }
    signals.shutdown().unwrap();
    println!(
        "REPORT:{}:{}:{:?}:{}",
        report.result.code,
        report.result.reason.as_str(),
        report.cleanup_action,
        report.cleanup_complete
    );
}

#[test]
fn preserves_exact_normal_exit_and_natural_signal() {
    let exited = run_case("exit", 5_000);
    assert!(exited.contains("REPORT:17:child-exit:None:true"));
    assert_recorded_pids_absent(&exited.fixture.evidence);

    let signaled = run_case("self-signal", 5_000);
    assert!(signaled.contains("REPORT:143:child-signal:None:true"));
    assert_recorded_pids_absent(&signaled.fixture.evidence);
}

#[test]
fn cleans_a_surviving_descendant_without_touching_an_unrelated_process() {
    let unrelated = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let mut unrelated = OwnedTestChild(unrelated);
    let completed = run_case("leave-descendant", 5_000);

    assert!(completed.contains("REPORT:0:completed:Terminate:true"));
    assert_recorded_pids_absent(&completed.fixture.evidence);
    assert!(unrelated.0.try_wait().unwrap().is_none());
}

#[test]
fn second_signal_escalates_an_ignored_timeout_and_reaps_the_group() {
    let timed_out = run_case("ignore-term", 100);

    assert!(
        timed_out.contains("REPORT:124:deadline-exceeded:Kill:true"),
        "unexpected helper output: {}",
        timed_out.stdout
    );
    assert_recorded_pids_absent(&timed_out.fixture.evidence);
}

struct CaseOutput {
    stdout: String,
    fixture: Fixture,
}

impl CaseOutput {
    fn contains(&self, expected: &str) -> bool {
        self.stdout.contains(expected)
    }
}

fn run_case(mode: &str, timeout_millis: u64) -> CaseOutput {
    let fixture = Fixture::new();
    let runner = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/runner");
    let path = format!(
        "{}:{}",
        runner.display(),
        env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env::current_exe().unwrap())
        .args(["--exact", "managed_supervision_helper", "--nocapture"])
        .env(HELPER_MARKER, "1")
        .env("AGENT_LOWMEM_FIXTURE_ROOT", &fixture.root)
        .env("AGENT_LOWMEM_FIXTURE_RUNTIME", &fixture.runtime)
        .env("AGENT_LOWMEM_FIXTURE_EVIDENCE", &fixture.evidence)
        .env("AGENT_LOWMEM_FIXTURE_MODE", mode)
        .env(
            "AGENT_LOWMEM_FIXTURE_TIMEOUT_MS",
            timeout_millis.to_string(),
        )
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "helper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    CaseOutput {
        stdout: String::from_utf8(output.stdout).unwrap(),
        fixture,
    }
}

fn assert_recorded_pids_absent(evidence: &Path) {
    let raw = fs::read_to_string(evidence).unwrap();
    for raw_pid in raw.split_whitespace() {
        let pid = Pid::from_raw(raw_pid.parse().unwrap()).unwrap();
        for _ in 0..200 {
            if test_kill_process(pid).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(test_kill_process(pid).is_err(), "fixture PID survived");
    }
}
