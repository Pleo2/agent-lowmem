use crate::{
    configuration::{valid_key, valid_relative_path},
    result::Reason,
};
use std::ffi::OsString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Version,
    Doctor { json: bool },
    GithubInspect { json: bool },
    Run(RunRequest),
    Init(InitRequest),
    Restore(RestoreRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRequest {
    pub operation_key: String,
    pub workspace_key: Option<String>,
    pub json_file: Option<String>,
    pub forwarded_arguments: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitRequest {
    pub dry_run: bool,
    pub json: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoreRequest {
    pub dry_run: bool,
    pub force_managed_block: bool,
    pub json: bool,
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
        [flag] if flag == "--version" || flag == "-V" => Ok(CliCommand::Version),
        [command] if command == "doctor" => Ok(CliCommand::Doctor { json: false }),
        [command, flag] if command == "doctor" && flag == "--json" => {
            Ok(CliCommand::Doctor { json: true })
        }
        [group, command] if group == "github" && command == "inspect" => {
            Ok(CliCommand::GithubInspect { json: false })
        }
        [group, command, flag] if group == "github" && command == "inspect" && flag == "--json" => {
            Ok(CliCommand::GithubInspect { json: true })
        }
        [command, rest @ ..] if command == "run" => parse_run(rest),
        [command, rest @ ..] if command == "init" => parse_init(rest),
        [command, rest @ ..] if command == "restore" => parse_restore(rest),
        _ => Err(invalid_cli()),
    }
}

fn parse_init(tokens: &[String]) -> Result<CliCommand, CliError> {
    let flags = parse_managed_file_flags(tokens, false)?;
    Ok(CliCommand::Init(InitRequest {
        dry_run: flags.dry_run,
        json: flags.json,
    }))
}

fn parse_restore(tokens: &[String]) -> Result<CliCommand, CliError> {
    let flags = parse_managed_file_flags(tokens, true)?;
    Ok(CliCommand::Restore(RestoreRequest {
        dry_run: flags.dry_run,
        force_managed_block: flags.force_managed_block,
        json: flags.json,
    }))
}

#[derive(Debug, Default)]
struct ManagedFileFlags {
    dry_run: bool,
    force_managed_block: bool,
    json: bool,
}

fn parse_managed_file_flags(
    tokens: &[String],
    allow_force_managed_block: bool,
) -> Result<ManagedFileFlags, CliError> {
    let mut flags = ManagedFileFlags::default();
    for token in tokens {
        match token.as_str() {
            "--dry-run" if !flags.dry_run => flags.dry_run = true,
            "--force-managed-block" if allow_force_managed_block && !flags.force_managed_block => {
                flags.force_managed_block = true;
            }
            "--json" if !flags.json => flags.json = true,
            _ => return Err(invalid_cli()),
        }
    }
    Ok(flags)
}

fn parse_run(tokens: &[String]) -> Result<CliCommand, CliError> {
    let Some((operation_key, options)) = tokens.split_first() else {
        return Err(invalid_cli());
    };
    if !valid_key(operation_key) {
        return Err(invalid_cli());
    }

    let mut workspace_key = None;
    let mut json_file = None;
    let mut forwarded_arguments = Vec::new();
    let mut index = 0;
    while index < options.len() {
        match options[index].as_str() {
            "--" => {
                forwarded_arguments.extend_from_slice(&options[index + 1..]);
                break;
            }
            "--workspace" if workspace_key.is_none() => {
                let value = options.get(index + 1).ok_or_else(invalid_cli)?;
                if !valid_key(value) {
                    return Err(invalid_cli());
                }
                workspace_key = Some(value.clone());
                index += 2;
            }
            "--json-file" if json_file.is_none() => {
                let value = options.get(index + 1).ok_or_else(invalid_cli)?;
                if !valid_relative_path(value) {
                    return Err(invalid_cli());
                }
                json_file = Some(value.clone());
                index += 2;
            }
            _ => return Err(invalid_cli()),
        }
    }
    if forwarded_arguments
        .iter()
        .any(|argument| argument.contains('\0'))
    {
        return Err(invalid_cli());
    }

    Ok(CliCommand::Run(RunRequest {
        operation_key: operation_key.clone(),
        workspace_key,
        json_file,
        forwarded_arguments,
    }))
}

const fn invalid_cli() -> CliError {
    CliError {
        reason: Reason::InvalidCli,
    }
}

#[cfg(test)]
mod tests {
    use super::{CliCommand, InitRequest, RestoreRequest, RunRequest, parse};
    use crate::result::Reason;

    #[test]
    fn parses_only_the_supported_version_flags() {
        assert_eq!(parse(["--version"]).unwrap(), CliCommand::Version);
        assert_eq!(parse(["-V"]).unwrap(), CliCommand::Version);

        for arguments in [vec!["version"], vec!["-v"], vec!["--version", "--json"]] {
            assert_eq!(parse(arguments).unwrap_err().reason(), Reason::InvalidCli);
        }
    }

    #[test]
    fn parses_doctor_forms() {
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

    #[test]
    fn parses_strict_run_requests() {
        assert_eq!(
            parse(["run", "test"]).unwrap(),
            CliCommand::Run(RunRequest {
                operation_key: "test".to_owned(),
                workspace_key: None,
                json_file: None,
                forwarded_arguments: Vec::new(),
            })
        );
        assert_eq!(
            parse(["run", "test", "--workspace", "web", "--", "src/a.test.ts",]).unwrap(),
            CliCommand::Run(RunRequest {
                operation_key: "test".to_owned(),
                workspace_key: Some("web".to_owned()),
                json_file: None,
                forwarded_arguments: vec!["src/a.test.ts".to_owned()],
            })
        );
        assert_eq!(
            parse([
                "run",
                "build",
                "--json-file",
                ".agent-lowmem-result.json",
                "--workspace",
                "web",
            ])
            .unwrap(),
            CliCommand::Run(RunRequest {
                operation_key: "build".to_owned(),
                workspace_key: Some("web".to_owned()),
                json_file: Some(".agent-lowmem-result.json".to_owned()),
                forwarded_arguments: Vec::new(),
            })
        );
    }

    #[test]
    fn parses_every_strict_init_request_ordering() {
        for (arguments, expected) in [
            (
                vec!["init"],
                InitRequest {
                    dry_run: false,
                    json: false,
                },
            ),
            (
                vec!["init", "--dry-run"],
                InitRequest {
                    dry_run: true,
                    json: false,
                },
            ),
            (
                vec!["init", "--json"],
                InitRequest {
                    dry_run: false,
                    json: true,
                },
            ),
            (
                vec!["init", "--dry-run", "--json"],
                InitRequest {
                    dry_run: true,
                    json: true,
                },
            ),
            (
                vec!["init", "--json", "--dry-run"],
                InitRequest {
                    dry_run: true,
                    json: true,
                },
            ),
        ] {
            assert_eq!(parse(arguments).unwrap(), CliCommand::Init(expected));
        }
    }

    #[test]
    fn parses_every_strict_restore_request_ordering() {
        for (arguments, expected) in [
            (vec![], (false, false, false)),
            (vec!["--dry-run"], (true, false, false)),
            (vec!["--force-managed-block"], (false, true, false)),
            (vec!["--json"], (false, false, true)),
            (
                vec!["--dry-run", "--force-managed-block"],
                (true, true, false),
            ),
            (
                vec!["--force-managed-block", "--dry-run"],
                (true, true, false),
            ),
            (vec!["--dry-run", "--json"], (true, false, true)),
            (vec!["--json", "--dry-run"], (true, false, true)),
            (vec!["--force-managed-block", "--json"], (false, true, true)),
            (vec!["--json", "--force-managed-block"], (false, true, true)),
            (
                vec!["--dry-run", "--force-managed-block", "--json"],
                (true, true, true),
            ),
            (
                vec!["--dry-run", "--json", "--force-managed-block"],
                (true, true, true),
            ),
            (
                vec!["--force-managed-block", "--dry-run", "--json"],
                (true, true, true),
            ),
            (
                vec!["--force-managed-block", "--json", "--dry-run"],
                (true, true, true),
            ),
            (
                vec!["--json", "--dry-run", "--force-managed-block"],
                (true, true, true),
            ),
            (
                vec!["--json", "--force-managed-block", "--dry-run"],
                (true, true, true),
            ),
        ] {
            let mut command = vec!["restore"];
            command.extend(arguments);
            assert_eq!(
                parse(command).unwrap(),
                CliCommand::Restore(RestoreRequest {
                    dry_run: expected.0,
                    force_managed_block: expected.1,
                    json: expected.2,
                })
            );
        }
    }

    #[test]
    fn rejects_ambiguous_managed_file_requests() {
        for arguments in [
            vec!["init", "--dry-run", "--dry-run"],
            vec!["init", "--json", "--json"],
            vec!["init", "--force-managed-block"],
            vec!["init", "--dry"],
            vec!["init", "--js"],
            vec!["init", "--unknown"],
            vec!["init", "repository"],
            vec!["init", "--json", "repository"],
            vec!["restore", "--dry-run", "--dry-run"],
            vec!["restore", "--force-managed-block", "--force-managed-block"],
            vec!["restore", "--json", "--json"],
            vec!["restore", "--force"],
            vec!["restore", "--dry"],
            vec!["restore", "--js"],
            vec!["restore", "--unknown"],
            vec!["restore", "repository"],
            vec!["restore", "--json", "repository"],
            vec!["--json", "init"],
            vec!["--dry-run", "restore"],
            vec!["init", "--json\0"],
            vec!["restore", "--dry-run\0"],
        ] {
            assert_eq!(parse(arguments).unwrap_err().reason(), Reason::InvalidCli);
        }
    }

    #[test]
    fn rejects_ambiguous_run_requests() {
        for arguments in [
            vec!["run"],
            vec!["run", "Test"],
            vec!["run", "test", "--workspace"],
            vec!["run", "test", "--workspace", "web", "--workspace", "api"],
            vec![
                "run",
                "test",
                "--json-file",
                "result.json",
                "--json-file",
                "other.json",
            ],
            vec!["run", "test", "--unknown"],
            vec!["run", "test", "--", "ok", "bad\0argument"],
        ] {
            assert_eq!(parse(arguments).unwrap_err().reason(), Reason::InvalidCli);
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_command_tokens() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let command = OsString::from_vec(vec![0xff]);
        let flag = OsString::from_vec(vec![0xff]);

        assert_eq!(parse([command]).unwrap_err().reason(), Reason::InvalidCli);
        assert_eq!(
            parse([OsString::from("init"), flag.clone()])
                .unwrap_err()
                .reason(),
            Reason::InvalidCli
        );
        assert_eq!(
            parse([OsString::from("restore"), flag])
                .unwrap_err()
                .reason(),
            Reason::InvalidCli
        );
    }
}
