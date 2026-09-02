use crate::result::Reason;
use std::ffi::OsString;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliCommand {
    Doctor { json: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CliError {
    reason: Reason,
}

impl CliError {
    pub const fn reason(self) -> Reason {
        self.reason
    }
}

pub fn parse<I, S>(arguments: I) -> Result<CliCommand, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let tokens = arguments
        .into_iter()
        .map(|argument| argument.into().into_string().map_err(|_| invalid_cli()))
        .collect::<Result<Vec<_>, _>>()?;

    match tokens.as_slice() {
        [command] if command == "doctor" => Ok(CliCommand::Doctor { json: false }),
        [command, flag] if command == "doctor" && flag == "--json" => {
            Ok(CliCommand::Doctor { json: true })
        }
        [command, ..] if command == "run" => Err(CliError {
            reason: Reason::OperationUnsupported,
        }),
        _ => Err(invalid_cli()),
    }
}

const fn invalid_cli() -> CliError {
    CliError {
        reason: Reason::InvalidCli,
    }
}

#[cfg(test)]
mod tests {
    use super::{CliCommand, parse};
    use crate::result::Reason;

    #[test]
    fn parses_only_the_phase_one_doctor_forms() {
        assert_eq!(
            parse(["doctor"]).unwrap(),
            CliCommand::Doctor { json: false }
        );
        assert_eq!(
            parse(["doctor", "--json"]).unwrap(),
            CliCommand::Doctor { json: true }
        );
        assert_eq!(
            parse(["--json", "doctor"]).unwrap_err().reason(),
            Reason::InvalidCli
        );
        assert_eq!(
            parse(["run", "test"]).unwrap_err().reason(),
            Reason::OperationUnsupported
        );
    }

    #[test]
    fn rejects_unknown_or_abbreviated_arguments() {
        assert_eq!(
            parse([] as [&str; 0]).unwrap_err().reason(),
            Reason::InvalidCli
        );
        assert_eq!(
            parse(["doctor", "--js"]).unwrap_err().reason(),
            Reason::InvalidCli
        );
        assert_eq!(parse(["unknown"]).unwrap_err().reason(), Reason::InvalidCli);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_command_tokens() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let token = OsString::from_vec(vec![0xff]);

        assert_eq!(parse([token]).unwrap_err().reason(), Reason::InvalidCli);
    }
}
