#![forbid(unsafe_code)]

use agent_lowmem::{
    cli::{CliCommand, InitRequest, RestoreRequest, parse},
    doctor::{inspect_doctor, render_human},
    github::{inspect as inspect_github, render_human as render_github_human},
    host::{HostReport, NativeHostSource, inspect_host},
    managed_files::{
        ManagedCommand, ManagedFilesOutcome, ManagedFilesReport, ManagedOutcome, ManagedResult,
        ManifestState, execute_init, execute_restore, render_managed_human,
    },
    process::reraise_signal,
    result::{ExitResult, Origin, Reason},
    run::{execute_run, runtime_directory},
    terminal::{
        TerminalCapabilities, render_wordmark, stable_managed_files_line, stable_result_line,
    },
};
use std::{
    io::{self, IsTerminal, Write},
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    let requested_run = arguments.first().is_some_and(|value| value == "run");
    let command = match parse(arguments) {
        Ok(command) => command,
        Err(error) => {
            let reason = error.reason();
            let code = if reason == Reason::OperationUnsupported {
                64
            } else {
                2
            };
            let result = ExitResult::new(Origin::Preflight, code, reason);
            return if requested_run {
                emit_result(result)
            } else {
                emit_failure(result, "requested command is invalid")
            };
        }
    };

    match command {
        CliCommand::Version => run_version(),
        CliCommand::Doctor { json } => run_doctor(json),
        CliCommand::GithubInspect { json } => run_github_inspect(json),
        CliCommand::Run(request) => run_managed(request),
        CliCommand::Init(request) => run_init(request),
        CliCommand::Restore(request) => run_restore(request),
    }
}

fn run_version() -> i32 {
    let output = format!("agent-lowmem {}", env!("CARGO_PKG_VERSION"));
    if write_stdout(&output).is_ok() { 0 } else { 70 }
}

fn run_github_inspect(json: bool) -> i32 {
    let current_dir = match std::env::current_dir() {
        Ok(current_dir) => current_dir,
        Err(_) => return emit_result(internal_result()),
    };
    let report = match inspect_github(&current_dir) {
        Ok(report) => report,
        Err(result) => return emit_result(result),
    };
    let output = if json {
        match serde_json::to_string(&report) {
            Ok(output) => output,
            Err(_) => return emit_result(internal_result()),
        }
    } else {
        let stdout = io::stdout();
        let terminal = TerminalCapabilities::from_environment(stdout.is_terminal());
        format!(
            "{}\n{}",
            render_wordmark(&terminal),
            render_github_human(&report)
        )
    };
    if write_stdout(&output).is_err() {
        return emit_result(internal_result());
    }
    emit_result(report.result)
}

fn run_init(request: InitRequest) -> i32 {
    let host = inspect_host(&NativeHostSource);
    let notice = init_host_notice(&host);
    run_managed_files(
        ManagedCommand::Init,
        request.dry_run,
        request.json,
        notice,
        |current_dir, runtime| execute_init(&NativeHostSource, current_dir, runtime, &request),
    )
}

fn init_host_notice(host: &HostReport) -> Option<&'static str> {
    (host.runtime_supported && !host.performance_validated).then_some(
        "agent-lowmem: notice this Mac is supported, but its performance profile is unvalidated",
    )
}

fn run_restore(request: RestoreRequest) -> i32 {
    run_managed_files(
        ManagedCommand::Restore,
        request.dry_run,
        request.json,
        None,
        |current_dir, runtime| execute_restore(current_dir, runtime, &request),
    )
}

fn run_managed_files(
    command: ManagedCommand,
    dry_run: bool,
    json: bool,
    notice: Option<&str>,
    execute: impl FnOnce(&Path, &Path) -> ManagedFilesOutcome,
) -> i32 {
    let current_dir = match std::env::current_dir() {
        Ok(current_dir) => current_dir,
        Err(_) => return emit_managed_internal(command, dry_run, json),
    };
    let runtime = if dry_run {
        None
    } else {
        match runtime_directory() {
            Ok(runtime) => Some(runtime),
            Err(_) => return emit_managed_internal(command, dry_run, json),
        }
    };
    let empty_runtime = PathBuf::new();
    let runtime = runtime.as_deref().unwrap_or(&empty_runtime);

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = catch_unwind(AssertUnwindSafe(|| execute(&current_dir, runtime)));
    std::panic::set_hook(previous_hook);
    let outcome = outcome.unwrap_or_else(|_| internal_managed_outcome(command, dry_run));

    let output = if json {
        match serde_json::to_string(&outcome.report) {
            Ok(output) => output,
            Err(_) => return emit_managed_internal(command, dry_run, json),
        }
    } else {
        let mut output = String::new();
        if !dry_run {
            let stdout = io::stdout();
            let terminal = TerminalCapabilities::from_environment(stdout.is_terminal());
            output.push_str(&render_wordmark(&terminal));
            output.push('\n');
        }
        output.push_str(&render_managed_human(&outcome));
        output
    };

    let output_failed = write_stdout(&output).is_err();
    let committed = matches!(
        outcome.report.outcome,
        ManagedOutcome::Applied | ManagedOutcome::Restored
    );
    let final_outcome = if output_failed && !committed {
        internal_managed_outcome(command, dry_run)
    } else {
        outcome
    };

    let stderr = io::stderr();
    let mut handle = stderr.lock();
    if output_failed {
        let _ = writeln!(
            handle,
            "agent-lowmem: warning managed-files output could not be written"
        );
    }
    if let Some(notice) = notice {
        let _ = writeln!(handle, "{notice}");
    }
    let _ = writeln!(
        handle,
        "{}",
        stable_managed_files_line(&final_outcome.report)
    );
    final_outcome.report.result.code
}

fn emit_managed_internal(command: ManagedCommand, dry_run: bool, json: bool) -> i32 {
    let outcome = internal_managed_outcome(command, dry_run);
    let output = if json {
        serde_json::to_string(&outcome.report).ok()
    } else {
        Some(render_managed_human(&outcome))
    };
    let output_failed = output
        .as_deref()
        .is_some_and(|output| write_stdout(output).is_err());
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    if output_failed {
        let _ = writeln!(
            handle,
            "agent-lowmem: warning managed-files output could not be written"
        );
    }
    let _ = writeln!(handle, "{}", stable_managed_files_line(&outcome.report));
    outcome.report.result.code
}

fn internal_managed_outcome(command: ManagedCommand, dry_run: bool) -> ManagedFilesOutcome {
    let report = ManagedFilesReport::new(
        command,
        dry_run,
        ManagedOutcome::Failed,
        ManagedResult::new(70, Reason::InternalError).expect("the internal result is valid"),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        ManifestState::Absent,
    )
    .expect("the internal managed-files report is valid");
    ManagedFilesOutcome {
        report,
        human_diff: None,
    }
}

fn run_managed(request: agent_lowmem::cli::RunRequest) -> i32 {
    let current_dir = match std::env::current_dir() {
        Ok(current_dir) => current_dir,
        Err(_) => return emit_result(internal_result()),
    };
    let runtime = match runtime_directory() {
        Ok(runtime) => runtime,
        Err(_) => return emit_result(internal_result()),
    };
    let stderr = io::stderr();
    let terminal = TerminalCapabilities::from_environment(stderr.is_terminal());
    let mut handle = stderr.lock();
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        execute_run(
            &NativeHostSource,
            &current_dir,
            &runtime,
            &request,
            &terminal,
            &mut handle,
        )
    }));
    std::panic::set_hook(previous_hook);
    let outcome = outcome.unwrap_or_else(|_| agent_lowmem::run::RunOutcome {
        result: internal_result(),
        child_started: false,
        external_signal: None,
    });
    let _ = writeln!(handle, "{}", stable_result_line(outcome.result));
    drop(handle);
    if let Some(signal) = outcome.external_signal {
        if reraise_signal(signal).is_err() {
            let stderr = io::stderr();
            let mut handle = stderr.lock();
            let _ = writeln!(
                handle,
                "agent-lowmem: warning external signal could not be re-raised"
            );
            return 70;
        }
    }
    outcome.result.code
}

fn run_doctor(json: bool) -> i32 {
    let current_dir = match std::env::current_dir() {
        Ok(current_dir) => current_dir,
        Err(_) => return internal_failure("could not inspect the current directory"),
    };
    let report = inspect_doctor(&NativeHostSource, &current_dir);
    let output = if json {
        match serde_json::to_string(&report) {
            Ok(output) => output,
            Err(_) => return internal_failure("could not serialize the doctor report"),
        }
    } else {
        render_human(&report)
    };

    match write_stdout(&output) {
        Ok(()) => 0,
        Err(_) => internal_failure("could not write the doctor report"),
    }
}

fn write_stdout(output: &str) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    writeln!(handle, "{output}")?;
    handle.flush()
}

fn internal_failure(message: &str) -> i32 {
    emit_failure(
        ExitResult::new(Origin::Internal, 70, Reason::InternalError),
        message,
    )
}

const fn internal_result() -> ExitResult {
    ExitResult::new(Origin::Internal, 70, Reason::InternalError)
}

fn emit_result(result: ExitResult) -> i32 {
    debug_assert!(result.is_valid());
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    let _ = writeln!(handle, "{}", stable_result_line(result));
    result.code
}

fn emit_failure(result: ExitResult, message: &str) -> i32 {
    debug_assert!(result.is_valid());
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    let _ = writeln!(handle, "agent-lowmem: {message}");
    let _ = writeln!(handle, "{}", stable_result_line(result));
    result.code
}

#[cfg(test)]
mod tests {
    use super::init_host_notice;
    use agent_lowmem::host::{HostReport, ProfileField};

    #[test]
    fn init_notice_is_limited_to_supported_unvalidated_hosts() {
        let mut report = HostReport {
            operating_system: "darwin".to_owned(),
            architecture: "arm64".to_owned(),
            macos_version: Some("26.6.2".to_owned()),
            hardware_model: Some("Mac99,1".to_owned()),
            cpu_brand: Some("Apple M9".to_owned()),
            physical_memory_bytes: Some(8_589_934_592),
            page_size_bytes: Some(16_384),
            runtime_supported: true,
            performance_validated: false,
            mismatched_profile_fields: vec![ProfileField::HardwareModel],
            failure_reason: None,
        };

        assert_eq!(
            init_host_notice(&report),
            Some(
                "agent-lowmem: notice this Mac is supported, but its performance profile is unvalidated"
            )
        );
        report.performance_validated = true;
        assert_eq!(init_host_notice(&report), None);
        report.runtime_supported = false;
        report.performance_validated = false;
        assert_eq!(init_host_notice(&report), None);
    }
}
