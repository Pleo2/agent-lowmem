#![forbid(unsafe_code)]

use agent_lowmem::{
    cli::{CliCommand, parse},
    doctor::{inspect_doctor, render_human},
    host::NativeHostSource,
    process::reraise_signal,
    result::{ExitResult, Origin, Reason},
    run::{execute_run, runtime_directory},
    terminal::{TerminalCapabilities, stable_result_line},
};
use std::{
    io::{self, IsTerminal, Write},
    panic::{AssertUnwindSafe, catch_unwind},
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
        CliCommand::Doctor { json } => run_doctor(json),
        CliCommand::Run(request) => run_managed(request),
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
    writeln!(handle, "{output}")
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
