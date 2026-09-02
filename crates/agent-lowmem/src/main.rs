#![forbid(unsafe_code)]

use agent_lowmem::{
    cli::{CliCommand, parse},
    doctor::{inspect_doctor, render_human},
    host::NativeHostSource,
    result::{ExitResult, Origin, Reason},
};
use std::io::{self, Write};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let command = match parse(std::env::args_os().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            let reason = error.reason();
            let code = if reason == Reason::OperationUnsupported {
                64
            } else {
                2
            };
            return emit_failure(
                ExitResult::new(Origin::Preflight, code, reason),
                "requested command is unavailable in the native-foundation checkpoint",
            );
        }
    };

    match command {
        CliCommand::Doctor { json } => run_doctor(json),
    }
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

fn emit_failure(result: ExitResult, message: &str) -> i32 {
    debug_assert!(result.is_valid());
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    let _ = writeln!(handle, "agent-lowmem: {message}");
    let _ = writeln!(
        handle,
        "agent-lowmem: result origin={} code={} reason={}",
        result.origin.as_str(),
        result.code,
        result.reason.as_str()
    );
    result.code
}
