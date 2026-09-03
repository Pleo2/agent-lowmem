use agent_lowmem::{
    lock::{LockProbe, LockStatus},
    process::{GroupController, GroupPhase, GroupStatus, ManagedSignal, SignalSource},
    result::Reason,
    run::runtime_directory,
    supervisor::{ChildController, ChildOutcome, Clock, SupervisionOutput, supervise},
};
use rustix::process::{Pid, test_kill_process};
use std::{
    cell::Cell,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const BINARY_SIZE_LIMIT: u64 = 12 * 1024 * 1024;
const SUPERVISOR_RSS_LIMIT: u64 = 24 * 1024 * 1024;
const THIRTY_MINUTES: Duration = Duration::from_secs(1_800);
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
#[ignore = "release-only managed-run resource gate"]
fn stripped_binary_and_supervisor_rss_stay_within_phase_three_budgets() {
    let binary = Path::new(env!("CARGO_BIN_EXE_agent-lowmem"));
    let binary_size = fs::metadata(binary).unwrap().len();
    assert!(
        binary_size <= BINARY_SIZE_LIMIT,
        "release binary {binary_size} bytes exceeded {BINARY_SIZE_LIMIT} bytes"
    );

    let fixture = RunFixture::new();
    let output = Command::new("/usr/bin/time")
        .arg("-l")
        .arg(binary)
        .args(["run", "test"])
        .current_dir(&fixture.root)
        .env("PATH", &fixture.runner)
        .env("NO_COLOR", "1")
        .env("AGENT_LOWMEM_BUDGET_PIDS", &fixture.pids)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "managed release run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    let maximum_rss = parse_maximum_resident_set_size(&stderr).unwrap();
    eprintln!("run-budget binary_bytes={binary_size} parent_peak_rss_bytes={maximum_rss}");
    assert!(
        maximum_rss <= SUPERVISOR_RSS_LIMIT,
        "supervisor RSS {maximum_rss} bytes exceeded {SUPERVISOR_RSS_LIMIT} bytes"
    );
    assert_recorded_pids_absent(&fixture.pids);
    assert_eq!(
        LockProbe::probe(&runtime_directory().unwrap()),
        LockStatus::Available,
        "managed release run left the global lease unavailable"
    );
}

#[test]
#[ignore = "release-only 30-minute-equivalent wakeup gate"]
fn thirty_minute_supervision_uses_at_most_eighteen_hundred_wakeups() {
    let clock = FakeClock::default();
    let mut child = TimedChild::new(&clock, THIRTY_MINUTES);
    let group = AbsentGroup;
    let mut signals = CountingSignals::new(&clock);
    let mut output = SilentOutput;

    let report = supervise(
        &mut child,
        &group,
        &mut signals,
        &clock,
        &mut output,
        Duration::from_secs(3_600),
    );

    eprintln!(
        "run-budget simulated_seconds=1800 wakeups={}",
        signals.wakeups
    );
    assert_eq!(report.result.reason, Reason::Completed);
    assert!(report.cleanup_complete);
    assert!(signals.wakeups <= 1_800);
}

fn parse_maximum_resident_set_size(stderr: &str) -> Option<u64> {
    stderr.lines().find_map(|line| {
        line.contains("maximum resident set size")
            .then(|| line.split_whitespace().next()?.parse().ok())?
    })
}

fn assert_recorded_pids_absent(path: &Path) {
    let raw = fs::read_to_string(path).unwrap();
    for raw_pid in raw.split_whitespace() {
        let pid = Pid::from_raw(raw_pid.parse().unwrap()).unwrap();
        for _ in 0..200 {
            match test_kill_process(pid) {
                Err(error) if error == rustix::io::Errno::SRCH => break,
                Err(error) => panic!("could not prove PID {pid:?} absence: {error}"),
                Ok(()) => thread::sleep(Duration::from_millis(10)),
            }
        }
        assert_eq!(
            test_kill_process(pid),
            Err(rustix::io::Errno::SRCH),
            "managed runner PID {pid:?} survived or absence could not be proven"
        );
    }
}

struct RunFixture {
    base: PathBuf,
    root: PathBuf,
    runner: PathBuf,
    pids: PathBuf,
}

impl RunFixture {
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "agent-lowmem-run-budget-{nanos}-{}-{serial}",
            std::process::id()
        ));
        let root = base.join("repository");
        let runner = base.join("runner");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("node_modules/vitest")).unwrap();
        fs::create_dir_all(&runner).unwrap();
        fs::write(
            root.join("package.json"),
            r#"{"name":"fixture","packageManager":"npm@12.0.2","scripts":{"test":"vitest run"}}"#,
        )
        .unwrap();
        fs::write(root.join("package-lock.json"), "{}\n").unwrap();
        fs::write(
            root.join(".agent-lowmem.json"),
            r#"{"version":1,"packageManager":"npm","operations":{"test":{"script":"test","timeoutSeconds":300}}}"#,
        )
        .unwrap();
        fs::write(
            root.join("node_modules/vitest/package.json"),
            r#"{"name":"vitest","version":"4.1.11"}"#,
        )
        .unwrap();
        let npm = runner.join("npm");
        fs::write(
            &npm,
            r#"#!/bin/sh
set -eu
echo "$$" > "$AGENT_LOWMEM_BUDGET_PIDS"
/bin/sleep 1 &
descendant=$!
echo "$descendant" >> "$AGENT_LOWMEM_BUDGET_PIDS"
wait "$descendant"
"#,
        )
        .unwrap();
        fs::set_permissions(&npm, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            pids: base.join("pids"),
            root: fs::canonicalize(root).unwrap(),
            runner,
            base,
        }
    }
}

impl Drop for RunFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.base).unwrap();
    }
}

#[derive(Clone, Default)]
struct FakeClock(Rc<Cell<Duration>>);

impl FakeClock {
    fn advance(&self, duration: Duration) {
        self.0.set(self.0.get().saturating_add(duration));
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        self.0.get()
    }
}

struct TimedChild {
    clock: FakeClock,
    completes_at: Duration,
    reaped: bool,
}

impl TimedChild {
    fn new(clock: &FakeClock, completes_at: Duration) -> Self {
        Self {
            clock: clock.clone(),
            completes_at,
            reaped: false,
        }
    }
}

impl ChildController for TimedChild {
    fn try_wait(&mut self) -> Result<Option<ChildOutcome>, Reason> {
        if self.reaped || self.clock.now() < self.completes_at {
            return Ok(None);
        }
        self.reaped = true;
        Ok(Some(ChildOutcome::Code(0)))
    }
}

struct AbsentGroup;

impl GroupController for AbsentGroup {
    fn status(&self, _phase: GroupPhase) -> Result<GroupStatus, Reason> {
        Ok(GroupStatus::Absent)
    }

    fn send(&self, _signal: ManagedSignal, _phase: GroupPhase) -> Result<(), Reason> {
        panic!("an absent group must never receive a signal")
    }
}

struct CountingSignals {
    clock: FakeClock,
    wakeups: usize,
}

impl CountingSignals {
    fn new(clock: &FakeClock) -> Self {
        Self {
            clock: clock.clone(),
            wakeups: 0,
        }
    }
}

impl SignalSource for CountingSignals {
    fn try_recv(&mut self) -> Option<i32> {
        None
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Result<Option<i32>, Reason> {
        self.wakeups += 1;
        self.clock.advance(timeout);
        Ok(None)
    }

    fn shutdown(&mut self) -> Result<(), Reason> {
        Ok(())
    }
}

struct SilentOutput;

impl SupervisionOutput for SilentOutput {
    fn timeout_warning(&mut self) {}
}
