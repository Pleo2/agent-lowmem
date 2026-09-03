use crate::{
    configuration::valid_relative_path, result::Reason, script::tokenizer::CommandSegment,
};
use semver::Version;

const CROSS_ENV_VERSION: &str = "10.1.0";
const DOTENV_CLI_VERSION: &str = "11.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapperKind {
    CrossEnv,
    Dotenv,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapperIdentity {
    pub package_name: String,
    pub version: Version,
}

impl WrapperIdentity {
    pub fn new(package_name: impl Into<String>, version: Version) -> Self {
        Self {
            package_name: package_name.into(),
            version,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WrapperEvidence {
    pub kind: WrapperKind,
    pub consumed_count: u8,
}

impl WrapperEvidence {
    pub const fn new(kind: WrapperKind, consumed_count: u8) -> Self {
        Self {
            kind,
            consumed_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnwrappedSegment {
    arguments: Vec<String>,
    evidence: Option<WrapperEvidence>,
}

impl UnwrappedSegment {
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    pub const fn evidence(&self) -> Option<WrapperEvidence> {
        self.evidence
    }
}

pub fn unwrap_segment(
    segment: &CommandSegment,
    identity: Option<&WrapperIdentity>,
) -> Result<UnwrappedSegment, Reason> {
    let arguments = segment.arguments();
    match arguments.first().map(String::as_str) {
        Some("cross-env") => unwrap_cross_env(arguments, identity),
        Some("dotenv") => unwrap_dotenv(arguments, identity),
        Some("cross-env-shell") => Err(Reason::WrapperUnsupported),
        Some(_) => Ok(UnwrappedSegment {
            arguments: arguments.to_vec(),
            evidence: None,
        }),
        None => Err(Reason::WrapperUnsupported),
    }
}

fn unwrap_cross_env(
    arguments: &[String],
    identity: Option<&WrapperIdentity>,
) -> Result<UnwrappedSegment, Reason> {
    require_identity(identity, "cross-env", CROSS_ENV_VERSION)?;
    let mut command_index = 1;
    let mut consumed_count = 0_usize;
    while let Some(argument) = arguments.get(command_index) {
        if !argument.contains('=') {
            break;
        }
        if !valid_assignment(argument) {
            return Err(Reason::WrapperUnsupported);
        }
        consumed_count += 1;
        command_index += 1;
    }
    if consumed_count == 0 || command_index >= arguments.len() {
        return Err(Reason::WrapperUnsupported);
    }
    finish_unwrap(
        arguments,
        command_index,
        WrapperKind::CrossEnv,
        consumed_count,
    )
}

fn unwrap_dotenv(
    arguments: &[String],
    identity: Option<&WrapperIdentity>,
) -> Result<UnwrappedSegment, Reason> {
    require_identity(identity, "dotenv-cli", DOTENV_CLI_VERSION)?;
    let mut index = 1;
    let mut consumed_count = 0_usize;
    while arguments.get(index).map(String::as_str) == Some("-e") {
        let path = arguments.get(index + 1).ok_or(Reason::WrapperUnsupported)?;
        if !valid_relative_path(path) {
            return Err(Reason::WrapperUnsupported);
        }
        consumed_count += 1;
        index += 2;
    }
    if arguments.get(index).map(String::as_str) != Some("--") {
        return Err(Reason::WrapperUnsupported);
    }
    finish_unwrap(arguments, index + 1, WrapperKind::Dotenv, consumed_count)
}

fn finish_unwrap(
    arguments: &[String],
    command_index: usize,
    kind: WrapperKind,
    consumed_count: usize,
) -> Result<UnwrappedSegment, Reason> {
    let command = arguments
        .get(command_index)
        .ok_or(Reason::WrapperUnsupported)?;
    if matches!(command.as_str(), "cross-env" | "cross-env-shell" | "dotenv") {
        return Err(Reason::WrapperUnsupported);
    }
    let consumed_count = u8::try_from(consumed_count).map_err(|_| Reason::WrapperUnsupported)?;
    Ok(UnwrappedSegment {
        arguments: arguments[command_index..].to_vec(),
        evidence: Some(WrapperEvidence::new(kind, consumed_count)),
    })
}

fn require_identity(
    identity: Option<&WrapperIdentity>,
    package_name: &str,
    version: &str,
) -> Result<(), Reason> {
    let identity = identity.ok_or(Reason::WrapperUnsupported)?;
    if identity.package_name != package_name
        || identity.version != Version::parse(version).map_err(|_| Reason::WrapperUnsupported)?
    {
        return Err(Reason::WrapperUnsupported);
    }
    Ok(())
}

fn valid_assignment(argument: &str) -> bool {
    let Some((key, _value)) = argument.split_once('=') else {
        return false;
    };
    let bytes = key.as_bytes();
    !bytes.is_empty()
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::{WrapperEvidence, WrapperIdentity, WrapperKind, unwrap_segment};
    use crate::{result::Reason, script::tokenizer::tokenize_script};
    use semver::Version;

    fn segment(script: &str) -> crate::script::tokenizer::CommandSegment {
        tokenize_script(script.as_bytes()).unwrap().segments()[0].clone()
    }

    fn wrapper(package_name: &str, version: &str) -> WrapperIdentity {
        WrapperIdentity::new(package_name, Version::parse(version).unwrap())
    }

    #[test]
    fn unwraps_cross_env_without_retaining_assignments() {
        let result = unwrap_segment(
            &segment("cross-env SECRET=value NODE_ENV=test vitest run"),
            Some(&wrapper("cross-env", "10.1.0")),
        )
        .unwrap();

        assert_eq!(result.arguments(), ["vitest", "run"]);
        assert_eq!(
            result.evidence(),
            Some(WrapperEvidence::new(WrapperKind::CrossEnv, 2))
        );
        let debug = format!("{result:?}");
        assert!(!debug.contains("SECRET"));
        assert!(!debug.contains("NODE_ENV"));
        assert!(!debug.contains("value"));
    }

    #[test]
    fn unwraps_dotenv_files_without_retaining_paths() {
        let result = unwrap_segment(
            &segment("dotenv -e .env.test -e config/local.env -- jest --runInBand"),
            Some(&wrapper("dotenv-cli", "11.0.0")),
        )
        .unwrap();

        assert_eq!(result.arguments(), ["jest", "--runInBand"]);
        assert_eq!(
            result.evidence(),
            Some(WrapperEvidence::new(WrapperKind::Dotenv, 2))
        );
        let debug = format!("{result:?}");
        assert!(!debug.contains(".env.test"));
        assert!(!debug.contains("config/local.env"));
    }

    #[test]
    fn leaves_an_unwrapped_command_unchanged() {
        let original = segment("vitest run");
        let result = unwrap_segment(&original, None).unwrap();

        assert_eq!(result.arguments(), original.arguments());
        assert_eq!(result.evidence(), None);
    }

    #[test]
    fn rejects_invalid_cross_env_forms_and_versions() {
        for (script, identity) in [
            ("cross-env vitest run", Some(wrapper("cross-env", "10.1.0"))),
            (
                "cross-env 1KEY=value vitest",
                Some(wrapper("cross-env", "10.1.0")),
            ),
            ("cross-env KEY vitest", Some(wrapper("cross-env", "10.1.0"))),
            ("cross-env KEY=value", Some(wrapper("cross-env", "10.1.0"))),
            (
                "cross-env KEY=value 1BAD=secret vitest",
                Some(wrapper("cross-env", "10.1.0")),
            ),
            ("cross-env KEY=value vitest", None),
            (
                "cross-env KEY=value vitest",
                Some(wrapper("cross-env", "9.0.0")),
            ),
            (
                "cross-env KEY=value vitest",
                Some(wrapper("other", "10.1.0")),
            ),
            ("cross-env-shell KEY=value vitest", None),
        ] {
            assert_eq!(
                unwrap_segment(&segment(script), identity.as_ref()).unwrap_err(),
                Reason::WrapperUnsupported,
                "wrapper should be rejected: {script}"
            );
        }
    }

    #[test]
    fn rejects_invalid_dotenv_forms_and_versions() {
        for (script, identity) in [
            ("dotenv jest", Some(wrapper("dotenv-cli", "11.0.0"))),
            ("dotenv --", Some(wrapper("dotenv-cli", "11.0.0"))),
            ("dotenv -e -- jest", Some(wrapper("dotenv-cli", "11.0.0"))),
            (
                "dotenv -e '' -- jest",
                Some(wrapper("dotenv-cli", "11.0.0")),
            ),
            (
                "dotenv -e /tmp/file -- jest",
                Some(wrapper("dotenv-cli", "11.0.0")),
            ),
            (
                "dotenv -e ../secret -- jest",
                Some(wrapper("dotenv-cli", "11.0.0")),
            ),
            (
                "dotenv -e config/../secret -- jest",
                Some(wrapper("dotenv-cli", "11.0.0")),
            ),
            (
                "dotenv --override -- jest",
                Some(wrapper("dotenv-cli", "11.0.0")),
            ),
            ("dotenv -- jest", None),
            ("dotenv -- jest", Some(wrapper("dotenv-cli", "10.0.0"))),
            ("dotenv -- jest", Some(wrapper("dotenv", "11.0.0"))),
        ] {
            assert_eq!(
                unwrap_segment(&segment(script), identity.as_ref()).unwrap_err(),
                Reason::WrapperUnsupported,
                "wrapper should be rejected: {script}"
            );
        }
    }

    #[test]
    fn rejects_nested_transparent_wrappers() {
        for script in [
            "cross-env KEY=value dotenv -- jest",
            "cross-env KEY=value cross-env OTHER=value jest",
            "dotenv -- cross-env KEY=value jest",
            "dotenv -- dotenv -- jest",
        ] {
            let identity = if script.starts_with("cross-env") {
                wrapper("cross-env", "10.1.0")
            } else {
                wrapper("dotenv-cli", "11.0.0")
            };
            assert_eq!(
                unwrap_segment(&segment(script), Some(&identity)).unwrap_err(),
                Reason::WrapperUnsupported
            );
        }
    }
}
