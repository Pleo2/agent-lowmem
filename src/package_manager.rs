use crate::{
    configuration::valid_package_name, repository::PackageManagerKind, result::Reason,
    workspace::PnpmWorkspaceDocument,
};
use semver::Version;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepositoryShellPolicy {
    pub script_shell_supported: bool,
    pub shell_emulator_disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeVersionEvidence(pub Version);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchArray {
    pub executable: String,
    pub arguments: Vec<String>,
}

impl LaunchArray {
    pub fn new<I, S>(executable: impl Into<String>, arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            executable: executable.into(),
            arguments: arguments.into_iter().map(Into::into).collect(),
        }
    }
}

pub fn inspect_npmrc(bytes: Option<&[u8]>) -> Result<RepositoryShellPolicy, Reason> {
    let Some(bytes) = bytes else {
        return Ok(supported_shell_policy());
    };
    let contents = std::str::from_utf8(bytes).map_err(|_| Reason::ScriptShellUnsupported)?;
    if contents.contains(['\r', '\0']) {
        return Err(Reason::ScriptShellUnsupported);
    }

    let mut script_shell_seen = false;
    for line in contents.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.trim() != line {
            return Err(Reason::ScriptShellUnsupported);
        }
        let (key, value) = line.split_once('=').ok_or(Reason::ScriptShellUnsupported)?;
        if !valid_npmrc_key(key) {
            return Err(Reason::ScriptShellUnsupported);
        }
        if key == "script-shell" {
            if script_shell_seen || value != "/bin/sh" || contains_substitution(value) {
                return Err(Reason::ScriptShellUnsupported);
            }
            script_shell_seen = true;
        }
    }

    Ok(supported_shell_policy())
}

pub fn inspect_pnpm_settings(
    document: &PnpmWorkspaceDocument,
) -> Result<RepositoryShellPolicy, Reason> {
    if document
        .script_shell
        .as_deref()
        .is_some_and(|shell| shell != "/bin/sh" || contains_substitution(shell))
        || document.shell_emulator == Some(true)
    {
        return Err(Reason::ScriptShellUnsupported);
    }
    Ok(supported_shell_policy())
}

pub fn inspect_node_version(
    node_version: Option<&[u8]>,
    nvmrc: Option<&[u8]>,
) -> Result<Option<NodeVersionEvidence>, Reason> {
    let node_version = node_version.map(parse_exact_node_version).transpose()?;
    let nvmrc = nvmrc.map(parse_exact_node_version).transpose()?;
    match (node_version, nvmrc) {
        (None, None) => Ok(None),
        (Some(version), None) | (None, Some(version)) => Ok(Some(NodeVersionEvidence(version))),
        (Some(left), Some(right)) if left == right => Ok(Some(NodeVersionEvidence(left))),
        (Some(_), Some(_)) => Err(Reason::ToolVersionUnsupported),
    }
}

pub fn build_launch_array(
    package_manager: PackageManagerKind,
    script: &str,
    workspace_package_name: Option<&str>,
    forwarded_arguments: &[String],
) -> Result<LaunchArray, Reason> {
    if script.is_empty()
        || script.contains(['\0', '\n', '\r'])
        || workspace_package_name.is_some_and(|name| !valid_package_name(name))
        || forwarded_arguments
            .iter()
            .any(|argument| argument.contains('\0'))
    {
        return Err(Reason::InvalidConfig);
    }

    let mut arguments = match package_manager {
        PackageManagerKind::Npm => {
            let mut arguments = vec!["--script-shell=/bin/sh".to_owned()];
            if let Some(package_name) = workspace_package_name {
                arguments.push(format!("--workspace={package_name}"));
            }
            arguments.extend(["run".to_owned(), script.to_owned()]);
            arguments
        }
        PackageManagerKind::Pnpm => {
            let mut arguments = vec![
                "--config.script-shell=/bin/sh".to_owned(),
                "--config.shell-emulator=false".to_owned(),
            ];
            if let Some(package_name) = workspace_package_name {
                arguments.extend([
                    "--filter".to_owned(),
                    package_name.to_owned(),
                    "--fail-if-no-match".to_owned(),
                ]);
            }
            arguments.extend(["run".to_owned(), script.to_owned()]);
            arguments
        }
    };
    if !forwarded_arguments.is_empty() {
        arguments.push("--".to_owned());
        arguments.extend_from_slice(forwarded_arguments);
    }

    let executable = match package_manager {
        PackageManagerKind::Npm => "npm",
        PackageManagerKind::Pnpm => "pnpm",
    };
    Ok(LaunchArray::new(executable, arguments))
}

const fn supported_shell_policy() -> RepositoryShellPolicy {
    RepositoryShellPolicy {
        script_shell_supported: true,
        shell_emulator_disabled: true,
    }
}

fn valid_npmrc_key(key: &str) -> bool {
    !key.is_empty()
        && key.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b'@')
        })
}

fn contains_substitution(value: &str) -> bool {
    value.contains("${") || value.contains("$(") || value.contains('`')
}

fn parse_exact_node_version(bytes: &[u8]) -> Result<Version, Reason> {
    let value = std::str::from_utf8(bytes).map_err(|_| Reason::ToolVersionUnsupported)?;
    let value = value
        .strip_suffix('\n')
        .unwrap_or(value)
        .strip_prefix('v')
        .unwrap_or_else(|| value.strip_suffix('\n').unwrap_or(value));
    if value.is_empty()
        || value.bytes().any(|byte| byte.is_ascii_whitespace())
        || value.contains('\0')
    {
        return Err(Reason::ToolVersionUnsupported);
    }
    Version::parse(value).map_err(|_| Reason::ToolVersionUnsupported)
}

#[cfg(test)]
mod tests {
    use super::{
        LaunchArray, build_launch_array, inspect_node_version, inspect_npmrc, inspect_pnpm_settings,
    };
    use crate::{repository::PackageManagerKind, result::Reason, workspace::parse_pnpm_workspace};

    #[test]
    fn builds_exact_root_and_workspace_launch_arrays() {
        assert_eq!(
            build_launch_array(PackageManagerKind::Npm, "test", None, &[]).unwrap(),
            LaunchArray::new("npm", ["--script-shell=/bin/sh", "run", "test"]),
        );
        assert_eq!(
            build_launch_array(
                PackageManagerKind::Npm,
                "test",
                Some("@acme/web"),
                &["--runInBand".into()],
            )
            .unwrap(),
            LaunchArray::new(
                "npm",
                [
                    "--script-shell=/bin/sh",
                    "--workspace=@acme/web",
                    "run",
                    "test",
                    "--",
                    "--runInBand",
                ],
            ),
        );
        assert_eq!(
            build_launch_array(PackageManagerKind::Pnpm, "test", Some("@acme/web"), &[],).unwrap(),
            LaunchArray::new(
                "pnpm",
                [
                    "--config.script-shell=/bin/sh",
                    "--config.shell-emulator=false",
                    "--filter",
                    "@acme/web",
                    "--fail-if-no-match",
                    "run",
                    "test",
                ],
            ),
        );
        assert_eq!(
            build_launch_array(
                PackageManagerKind::Pnpm,
                "lint",
                None,
                &["--fix".into(), "src".into()],
            )
            .unwrap(),
            LaunchArray::new(
                "pnpm",
                [
                    "--config.script-shell=/bin/sh",
                    "--config.shell-emulator=false",
                    "run",
                    "lint",
                    "--",
                    "--fix",
                    "src",
                ],
            ),
        );
    }

    #[test]
    fn inspects_only_supported_repository_npmrc_shell_policy() {
        let absent = inspect_npmrc(None).unwrap();
        assert!(absent.script_shell_supported);
        assert!(absent.shell_emulator_disabled);

        let supported = inspect_npmrc(Some(
            b"# local repository config\nregistry=https://registry.npmjs.org/\nscript-shell=/bin/sh\n",
        ))
        .unwrap();
        assert!(supported.script_shell_supported);

        for contents in [
            b"script-shell=/bin/bash\n".as_slice(),
            b"script-shell=${SHELL}\n",
            b"script-shell=$(which sh)\n",
            b"script-shell\n",
            b"script-shell=/bin/sh\nscript-shell=/bin/sh\n",
            b" script-shell=/bin/sh\n",
            b"script-shell=/bin/sh\r\n",
        ] {
            assert_eq!(
                inspect_npmrc(Some(contents)),
                Err(Reason::ScriptShellUnsupported)
            );
        }
    }

    #[test]
    fn rejects_unsupported_pnpm_shell_settings() {
        let default_document = parse_pnpm_workspace(b"packages:\n  - apps/*\n").unwrap();
        assert!(inspect_pnpm_settings(&default_document).is_ok());

        let pinned = parse_pnpm_workspace(
            b"packages:\n  - apps/*\nscriptShell: /bin/sh\nshellEmulator: false\nenablePrePostScripts: true\n",
        )
        .unwrap();
        assert!(inspect_pnpm_settings(&pinned).is_ok());

        for yaml in [
            b"packages:\n  - apps/*\nscriptShell: /bin/bash\n".as_slice(),
            b"packages:\n  - apps/*\nscriptShell: '${SHELL}'\n",
            b"packages:\n  - apps/*\nshellEmulator: true\n",
        ] {
            let document = parse_pnpm_workspace(yaml).unwrap();
            assert_eq!(
                inspect_pnpm_settings(&document),
                Err(Reason::ScriptShellUnsupported)
            );
        }
    }

    #[test]
    fn normalizes_exact_node_version_evidence() {
        let plain = inspect_node_version(Some(b"24.14.1\n"), None)
            .unwrap()
            .unwrap();
        let prefixed = inspect_node_version(None, Some(b"v24.14.1\n"))
            .unwrap()
            .unwrap();
        let matching = inspect_node_version(Some(b"24.14.1"), Some(b"v24.14.1\n"))
            .unwrap()
            .unwrap();

        assert_eq!(plain, prefixed);
        assert_eq!(plain, matching);
        assert_eq!(plain.0.to_string(), "24.14.1");
        assert_eq!(inspect_node_version(None, None).unwrap(), None);
    }

    #[test]
    fn rejects_ambiguous_or_non_exact_node_version_evidence() {
        for bytes in [
            b"lts/*\n".as_slice(),
            b">=24\n",
            b"24\n",
            b"24.14.1 extra\n",
            b"24.14.1\n\n",
            b"v\n",
            b"\xff",
        ] {
            assert_eq!(
                inspect_node_version(Some(bytes), None),
                Err(Reason::ToolVersionUnsupported)
            );
        }
        assert_eq!(
            inspect_node_version(Some(b"24.14.1\n"), Some(b"22.22.1\n")),
            Err(Reason::ToolVersionUnsupported)
        );
    }
}
