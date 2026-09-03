use crate::{
    cli::RunRequest,
    host::{HostSource, inspect_host},
    lock::{LeaseRecord, ProcessIdentity, UserLease},
    process::{
        GroupController, GroupPhase, ManagedChild, ManagedSignal, NativeSignalSource, SignalSource,
        spawn_managed,
    },
    repository::{RunPlan, RunSelection, plan_run, plans_match},
    result::{ExitResult, Origin, Reason},
    result_file::{
        EvidenceRecheckState, LockState, RunLifecycle, RunResultDetails, RunResultRecord,
        SpawnState, ValidatedResultDestination, validate_result_destination,
        write_validated_result_atomic,
    },
    supervisor::{InstantClock, SupervisionOutput, supervise},
    terminal::{TerminalCapabilities, render_wordmark},
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOutcome {
    pub result: ExitResult,
    pub child_started: bool,
    pub external_signal: Option<crate::process::ManagedSignal>,
}

impl RunOutcome {
    pub const fn new(origin: Origin, code: i32, reason: Reason, child_started: bool) -> Self {
        Self {
            result: ExitResult::new(origin, code, reason),
            child_started,
            external_signal: None,
        }
    }
}

pub fn execute_run(
    source: &impl HostSource,
    start: &Path,
    runtime: &Path,
    request: &RunRequest,
    terminal: &TerminalCapabilities,
    output: &mut impl Write,
) -> RunOutcome {
    execute_run_core(source, start, runtime, request, terminal, output, || {})
}

pub fn runtime_directory() -> Result<PathBuf, Reason> {
    fs::canonicalize(std::env::temp_dir())
        .map(|temporary| temporary.join("agent-lowmem-v1"))
        .map_err(|_| Reason::InternalError)
}

#[cfg(test)]
fn execute_run_with_hook(
    source: &impl HostSource,
    start: &Path,
    runtime: &Path,
    request: &RunRequest,
    terminal: &TerminalCapabilities,
    output: &mut impl Write,
    post_lock_hook: impl FnOnce(),
) -> RunOutcome {
    execute_run_core(
        source,
        start,
        runtime,
        request,
        terminal,
        output,
        post_lock_hook,
    )
}

fn execute_run_core(
    source: &impl HostSource,
    start: &Path,
    runtime: &Path,
    request: &RunRequest,
    terminal: &TerminalCapabilities,
    output: &mut impl Write,
    post_lock_hook: impl FnOnce(),
) -> RunOutcome {
    if writeln!(output, "{}", render_wordmark(terminal)).is_err() {
        return outcome_for(Reason::InternalError, false);
    }
    let host = inspect_host(source);
    if !host.runtime_supported {
        return outcome_for(Reason::HostUnsupported, false);
    }
    let selection = RunSelection {
        operation_key: request.operation_key.clone(),
        workspace_key: request.workspace_key.clone(),
        forwarded_arguments: request.forwarded_arguments.clone(),
    };
    let plan_before = match plan_run(start, &selection) {
        Ok(plan) => plan,
        Err(reason) => return outcome_for(reason, false),
    };
    if !host.performance_validated
        && writeln!(
            output,
            "agent-lowmem: notice performance is not validated for this supported Mac"
        )
        .is_err()
    {
        return outcome_for(Reason::InternalError, false);
    }
    for disclosure in &plan_before.policy().disclosures {
        if writeln!(output, "agent-lowmem: disclosure {disclosure}").is_err() {
            return outcome_for(Reason::InternalError, false);
        }
    }
    let destination = match request.json_file.as_deref() {
        Some(relative) => match validate_result_destination(plan_before.root(), relative) {
            Ok(destination) => Some(destination),
            Err(reason) => return outcome_for(reason, false),
        },
        None => None,
    };
    let acquired_at = match unix_seconds_now() {
        Ok(seconds) => seconds,
        Err(reason) => return finish_preflight(&plan_before, destination.as_ref(), reason, false),
    };
    let record = match ProcessIdentity::current().and_then(|owner| {
        LeaseRecord::new(
            owner,
            plan_before.repository_hash(),
            request.operation_key.clone(),
            acquired_at,
        )
    }) {
        Ok(record) => record,
        Err(reason) => return finish_preflight(&plan_before, destination.as_ref(), reason, false),
    };
    let mut lease = match UserLease::acquire(runtime, record) {
        Ok(lease) => lease,
        Err(reason) => return finish_preflight(&plan_before, destination.as_ref(), reason, false),
    };

    post_lock_hook();
    let plan_after = match plan_run(plan_before.root(), &selection) {
        Ok(plan) if plans_match(&plan_before, &plan) => plan,
        Ok(_) | Err(_) => {
            return finish_with_details(
                &plan_before,
                destination.as_ref(),
                Reason::EvidenceChanged,
                RunLifecycle {
                    lock: LockState::Acquired,
                    evidence_recheck: EvidenceRecheckState::Changed,
                    spawn: SpawnState::NotAttempted,
                },
                false,
            );
        }
    };

    let (child, signals) = match spawn_managed(&plan_after, &mut lease) {
        Ok(managed) => managed,
        Err(failure) => {
            if should_clear_spawn_failure_identity(failure.cleanup_complete) {
                let _ = lease.clear_child_group();
            }
            let spawn_report = crate::supervisor::SupervisionReport {
                result: outcome_for(failure.reason, failure.child_started).result,
                external_signal: None,
                warning_emitted: false,
                elapsed_millis: 0,
                cleanup_action: if failure.child_started {
                    crate::supervisor::CleanupAction::Kill
                } else {
                    crate::supervisor::CleanupAction::None
                },
                cleanup_complete: failure.cleanup_complete,
            };
            if !failure.cleanup_complete {
                let _ = writeln!(
                    output,
                    "agent-lowmem: warning managed process cleanup is incomplete"
                );
            }
            return finish_with_details_and_report(
                &plan_after,
                destination.as_ref(),
                failure.reason,
                RunLifecycle {
                    lock: LockState::Acquired,
                    evidence_recheck: EvidenceRecheckState::Matched,
                    spawn: SpawnState::Failed,
                },
                failure.child_started,
                Some(&spawn_report),
            );
        }
    };
    let mut guard = ManagedRunGuard::new(lease, child, signals);
    let group = *guard
        .child
        .as_ref()
        .expect("managed child was just installed")
        .group();
    let clock = InstantClock::start();
    let mut supervision_output = WriterSupervisionOutput { output };
    let report = supervise(
        guard.child.as_mut().expect("managed child is present"),
        &group,
        guard.signals.as_mut().expect("signal source is present"),
        &clock,
        &mut supervision_output,
        std::time::Duration::from_secs(u64::from(plan_after.policy().timeout_seconds)),
    );
    let signal_shutdown_failed = guard
        .signals
        .as_mut()
        .is_some_and(|signals| signals.shutdown().is_err());
    guard.signals.take();

    let lifecycle = RunLifecycle {
        lock: LockState::Acquired,
        evidence_recheck: EvidenceRecheckState::Matched,
        spawn: SpawnState::Started,
    };
    let details = RunResultDetails::from_plan(&plan_after, lifecycle, Some(&report));
    if let Some(destination) = destination.as_ref() {
        if RunResultRecord::now(report.result, true, Some(details))
            .and_then(|record| write_validated_result_atomic(destination, &record))
            .is_err()
        {
            let _ = writeln!(
                supervision_output.output,
                "agent-lowmem: warning structured result could not be written"
            );
        }
    }

    if report.cleanup_complete
        && guard
            .lease
            .as_mut()
            .is_some_and(|lease| lease.clear_child_group().is_err())
    {
        let _ = writeln!(
            supervision_output.output,
            "agent-lowmem: warning managed lease record could not be cleared"
        );
    }
    if !report.cleanup_complete {
        let _ = writeln!(
            supervision_output.output,
            "agent-lowmem: warning managed process cleanup is incomplete"
        );
    }
    if signal_shutdown_failed {
        let _ = writeln!(
            supervision_output.output,
            "agent-lowmem: warning signal listener shutdown was incomplete"
        );
    }

    guard.disarm();
    RunOutcome {
        result: report.result,
        child_started: true,
        external_signal: report.external_signal,
    }
}

const fn should_clear_spawn_failure_identity(cleanup_complete: bool) -> bool {
    cleanup_complete
}

struct WriterSupervisionOutput<'a, W: Write> {
    output: &'a mut W,
}

impl<W: Write> SupervisionOutput for WriterSupervisionOutput<'_, W> {
    fn timeout_warning(&mut self) {
        let _ = writeln!(
            self.output,
            "agent-lowmem: warning managed operation reached 80% of its timeout"
        );
    }
}

struct ManagedRunGuard {
    lease: Option<UserLease>,
    child: Option<ManagedChild>,
    signals: Option<NativeSignalSource>,
}

impl ManagedRunGuard {
    fn new(lease: UserLease, child: ManagedChild, signals: NativeSignalSource) -> Self {
        Self {
            lease: Some(lease),
            child: Some(child),
            signals: Some(signals),
        }
    }

    fn disarm(&mut self) {
        self.child.take();
        self.signals.take();
        self.lease.take();
    }
}

impl Drop for ManagedRunGuard {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let phase = match child.try_wait() {
                Ok(Some(_)) => GroupPhase::LeaderReaped,
                Ok(None) | Err(_) => GroupPhase::LeaderExpected,
            };
            if child.group().send(ManagedSignal::Kill, phase).is_ok() {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
                while std::time::Instant::now() < deadline {
                    match child.try_wait() {
                        Ok(Some(_)) | Err(_) => break,
                        Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
                    }
                }
            }
        }
        if let Some(signals) = self.signals.as_mut() {
            let _ = signals.shutdown();
        }
    }
}

fn finish_preflight(
    plan: &RunPlan,
    destination: Option<&ValidatedResultDestination>,
    reason: Reason,
    lock_acquired: bool,
) -> RunOutcome {
    finish_with_details(
        plan,
        destination,
        reason,
        RunLifecycle {
            lock: if lock_acquired {
                LockState::Acquired
            } else {
                LockState::NotAcquired
            },
            evidence_recheck: EvidenceRecheckState::NotRun,
            spawn: SpawnState::NotAttempted,
        },
        false,
    )
}

fn finish_with_details(
    plan: &RunPlan,
    destination: Option<&ValidatedResultDestination>,
    reason: Reason,
    lifecycle: RunLifecycle,
    child_started: bool,
) -> RunOutcome {
    finish_with_details_and_report(plan, destination, reason, lifecycle, child_started, None)
}

fn finish_with_details_and_report(
    plan: &RunPlan,
    destination: Option<&ValidatedResultDestination>,
    reason: Reason,
    lifecycle: RunLifecycle,
    child_started: bool,
    supervision: Option<&crate::supervisor::SupervisionReport>,
) -> RunOutcome {
    let outcome = outcome_for(reason, child_started);
    let details = RunResultDetails::from_plan(plan, lifecycle, supervision);
    if let Some(destination) = destination {
        let record = RunResultRecord::now(outcome.result, child_started, Some(details));
        if record
            .and_then(|record| write_validated_result_atomic(destination, &record))
            .is_err()
        {
            return outcome_for(Reason::InternalError, child_started);
        }
    }
    outcome
}

fn outcome_for(reason: Reason, child_started: bool) -> RunOutcome {
    match reason {
        Reason::InvalidCli | Reason::InvalidConfig => {
            RunOutcome::new(Origin::Preflight, 2, reason, false)
        }
        Reason::LockHeld | Reason::NestedInvocation => {
            RunOutcome::new(Origin::Preflight, 73, reason, false)
        }
        Reason::EvidenceChanged => RunOutcome::new(Origin::Preflight, 75, reason, false),
        Reason::ManagedFileConflict => RunOutcome::new(Origin::Preflight, 78, reason, false),
        Reason::InternalError => RunOutcome::new(Origin::Internal, 70, reason, child_started),
        Reason::Completed
        | Reason::ChildExit
        | Reason::ChildSignal
        | Reason::DeadlineExceeded
        | Reason::ExternalSignal => {
            RunOutcome::new(Origin::Internal, 70, Reason::InternalError, child_started)
        }
        _ => RunOutcome::new(Origin::Preflight, 64, reason, false),
    }
}

fn unix_seconds_now() -> Result<u64, Reason> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Reason::InternalError)?
        .as_secs();
    (seconds > 0)
        .then_some(seconds)
        .ok_or(Reason::InternalError)
}

#[cfg(test)]
mod tests {
    use super::{RunOutcome, execute_run_with_hook, should_clear_spawn_failure_identity};
    use crate::{
        cli::RunRequest,
        host::{HostReadError, HostSource},
        result::{Origin, Reason},
        terminal::TerminalCapabilities,
    };
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct FakeHost {
        supported: bool,
    }

    impl HostSource for FakeHost {
        fn operating_system(&self) -> &str {
            if self.supported { "macos" } else { "linux" }
        }

        fn architecture(&self) -> &str {
            "aarch64"
        }

        fn read(&self, key: &'static str) -> Result<String, HostReadError> {
            let value = match key {
                "kern.osproductversion" => "26.6.2",
                "hw.model" => "Mac14,15",
                "machdep.cpu.brand_string" => "Apple M2",
                "hw.memsize" => "8589934592",
                "hw.pagesize" => "16384",
                _ => return Err(HostReadError::Missing(key)),
            };
            Ok(value.to_owned())
        }
    }

    struct Fixture {
        base: PathBuf,
        root: PathBuf,
        runtime: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let base = std::env::temp_dir().join(format!(
                "agent-lowmem-run-unit-{}-{serial}",
                std::process::id()
            ));
            let root = base.join("repository");
            fs::create_dir_all(root.join(".git")).unwrap();
            fs::create_dir_all(root.join("node_modules/vitest")).unwrap();
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
            Self {
                runtime: base.join("runtime"),
                root: fs::canonicalize(root).unwrap(),
                base,
            }
        }

        fn workspace() -> Self {
            let fixture = Self::new();
            fs::create_dir_all(fixture.root.join("apps/web/node_modules/vitest")).unwrap();
            fs::write(
                fixture.root.join("package.json"),
                r#"{"name":"fixture","packageManager":"npm@12.0.2","workspaces":["apps/*"]}"#,
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
                r#"{"version":1,"packageManager":"npm","workspaces":{"web":{"path":"apps/web","packageName":"@fixture/web","operations":{"test":{"script":"test","timeoutSeconds":300}}}}}"#,
            )
            .unwrap();
            fixture
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    fn plain_terminal() -> TerminalCapabilities {
        TerminalCapabilities {
            is_terminal: false,
            no_color: false,
            term: None,
            colorterm: None,
        }
    }

    #[test]
    fn spawn_failure_preserves_recovery_identity_until_cleanup_is_proven() {
        assert!(should_clear_spawn_failure_identity(true));
        assert!(!should_clear_spawn_failure_identity(false));
    }

    #[test]
    fn unsupported_host_stops_before_repository_inspection() {
        let mut output = Vec::new();
        let outcome = execute_run_with_hook(
            &FakeHost { supported: false },
            std::path::Path::new("/path-that-must-not-be-read"),
            std::path::Path::new("/runtime-that-must-not-be-created"),
            &RunRequest {
                operation_key: "test".to_owned(),
                workspace_key: None,
                json_file: None,
                forwarded_arguments: Vec::new(),
            },
            &plain_terminal(),
            &mut output,
            || {},
        );

        assert_eq!(
            outcome,
            RunOutcome::new(Origin::Preflight, 64, Reason::HostUnsupported, false)
        );
        assert_eq!(String::from_utf8(output).unwrap(), "agent_lowmem\n");
    }

    #[test]
    fn every_post_lock_evidence_mutation_returns_75_without_starting_a_child() {
        for (relative, replacement) in [
            (
                "package.json",
                r#"{"name":"changed","packageManager":"npm@12.0.2","scripts":{"test":"vitest run"}}"#,
            ),
            ("package-lock.json", "{\"changed\":true}\n"),
            (
                ".agent-lowmem.json",
                r#"{"version":1,"packageManager":"npm","operations":{"test":{"script":"test","timeoutSeconds":301}}}"#,
            ),
            (
                "node_modules/vitest/package.json",
                r#"{"name":"vitest","version":"4.1.12"}"#,
            ),
        ] {
            let fixture = Fixture::new();
            let mut output = Vec::new();
            let path = fixture.root.join(relative);
            let outcome = execute_run_with_hook(
                &FakeHost { supported: true },
                &fixture.root,
                &fixture.runtime,
                &RunRequest {
                    operation_key: "test".to_owned(),
                    workspace_key: None,
                    json_file: Some("result.json".to_owned()),
                    forwarded_arguments: Vec::new(),
                },
                &plain_terminal(),
                &mut output,
                || fs::write(path, replacement).unwrap(),
            );

            assert_eq!(
                outcome,
                RunOutcome::new(Origin::Preflight, 75, Reason::EvidenceChanged, false),
                "mutation {relative}"
            );
            let result: serde_json::Value =
                serde_json::from_slice(&fs::read(fixture.root.join("result.json")).unwrap())
                    .unwrap();
            assert_eq!(result["childStarted"], false);
            assert_eq!(result["details"]["evidenceRecheckState"], "changed");
            assert_eq!(result["details"]["spawnState"], "not-attempted");
            assert!(!fixture.root.join("child-started").exists());
        }
    }

    #[test]
    fn post_lock_workspace_evidence_mutation_returns_75() {
        let fixture = Fixture::workspace();
        let mut output = Vec::new();
        let workspace_manifest = fixture.root.join("apps/web/package.json");
        let outcome = execute_run_with_hook(
            &FakeHost { supported: true },
            &fixture.root,
            &fixture.runtime,
            &RunRequest {
                operation_key: "test".to_owned(),
                workspace_key: Some("web".to_owned()),
                json_file: None,
                forwarded_arguments: Vec::new(),
            },
            &plain_terminal(),
            &mut output,
            || {
                fs::write(
                    workspace_manifest,
                    r#"{"name":"@fixture/web","scripts":{"test":"vitest run --changed"}}"#,
                )
                .unwrap()
            },
        );

        assert_eq!(
            outcome,
            RunOutcome::new(Origin::Preflight, 75, Reason::EvidenceChanged, false)
        );
        assert!(!fixture.root.join("child-started").exists());
    }

    #[test]
    fn panic_after_lock_releases_the_advisory_lease() {
        let fixture = Fixture::new();
        let mut output = Vec::new();
        let request = RunRequest {
            operation_key: "test".to_owned(),
            workspace_key: None,
            json_file: None,
            forwarded_arguments: Vec::new(),
        };
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            execute_run_with_hook(
                &FakeHost { supported: true },
                &fixture.root,
                &fixture.runtime,
                &request,
                &plain_terminal(),
                &mut output,
                || panic!("test-only panic after lease acquisition"),
            )
        }));
        assert!(panic.is_err());

        let mut retry_output = Vec::new();
        let retry = execute_run_with_hook(
            &FakeHost { supported: true },
            &fixture.root,
            &fixture.runtime,
            &request,
            &plain_terminal(),
            &mut retry_output,
            || fs::write(fixture.root.join("package-lock.json"), "{\"changed\":true}").unwrap(),
        );
        assert_eq!(retry.result.reason, Reason::EvidenceChanged);
    }
}
