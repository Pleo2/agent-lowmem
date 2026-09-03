use crate::{repository::PackageManagerKind, result::Reason};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

const SCHEMA_URL: &str = "https://agentlowmem.dev/schema/v1.json";
const CONFIG_VERSION: u8 = 1;
const MIN_TIMEOUT_SECONDS: u16 = 60;
const MAX_TIMEOUT_SECONDS: u16 = 3_600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLowmemConfig {
    pub version: u8,
    pub package_manager: PackageManagerKind,
    pub operations: BTreeMap<String, OperationConfig>,
    pub workspaces: BTreeMap<String, WorkspaceConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationConfig {
    pub script: String,
    pub timeout_seconds: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceConfig {
    pub path: String,
    pub package_name: String,
    pub operations: BTreeMap<String, OperationConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigError(Reason);

impl ConfigError {
    pub const fn reason(self) -> Reason {
        self.0
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawConfig {
    #[serde(rename = "$schema")]
    schema: Option<String>,
    version: u8,
    package_manager: RawPackageManagerKind,
    #[serde(default)]
    operations: BTreeMap<String, RawOperationConfig>,
    #[serde(default)]
    workspaces: BTreeMap<String, RawWorkspaceConfig>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RawPackageManagerKind {
    Npm,
    Pnpm,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawOperationConfig {
    script: String,
    timeout_seconds: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawWorkspaceConfig {
    path: String,
    package_name: String,
    #[serde(default)]
    operations: BTreeMap<String, RawOperationConfig>,
}

pub fn parse_config(bytes: &[u8]) -> Result<AgentLowmemConfig, ConfigError> {
    let raw: RawConfig = serde_json::from_slice(bytes).map_err(|_| invalid_config())?;
    if raw.version != CONFIG_VERSION || raw.schema.as_deref().is_some_and(|s| s != SCHEMA_URL) {
        return Err(invalid_config());
    }

    let operations = validate_operations(raw.operations)?;
    let mut workspaces = BTreeMap::new();
    let mut package_names = BTreeSet::new();
    for (key, workspace) in raw.workspaces {
        if !valid_key(&key)
            || !valid_relative_path(&workspace.path)
            || !valid_package_name(&workspace.package_name)
            || !package_names.insert(workspace.package_name.clone())
        {
            return Err(invalid_config());
        }
        workspaces.insert(
            key,
            WorkspaceConfig {
                path: workspace.path,
                package_name: workspace.package_name,
                operations: validate_operations(workspace.operations)?,
            },
        );
    }

    Ok(AgentLowmemConfig {
        version: raw.version,
        package_manager: match raw.package_manager {
            RawPackageManagerKind::Npm => PackageManagerKind::Npm,
            RawPackageManagerKind::Pnpm => PackageManagerKind::Pnpm,
        },
        operations,
        workspaces,
    })
}

pub fn select_operation<'a>(
    config: &'a AgentLowmemConfig,
    workspace_key: Option<&str>,
    operation_key: &str,
) -> Result<&'a OperationConfig, ConfigError> {
    let operations = match workspace_key {
        Some(key) => {
            &config
                .workspaces
                .get(key)
                .ok_or(ConfigError(Reason::WorkspaceCardinality))?
                .operations
        }
        None => &config.operations,
    };
    operations
        .get(operation_key)
        .ok_or(ConfigError(Reason::OperationUnsupported))
}

fn validate_operations(
    operations: BTreeMap<String, RawOperationConfig>,
) -> Result<BTreeMap<String, OperationConfig>, ConfigError> {
    operations
        .into_iter()
        .map(|(key, operation)| {
            if !valid_key(&key)
                || operation.script.is_empty()
                || !(MIN_TIMEOUT_SECONDS..=MAX_TIMEOUT_SECONDS).contains(&operation.timeout_seconds)
            {
                return Err(invalid_config());
            }
            Ok((
                key,
                OperationConfig {
                    script: operation.script,
                    timeout_seconds: operation.timeout_seconds,
                },
            ))
        })
        .collect()
}

fn valid_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

pub(crate) fn valid_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains(['\\', '\0'])
        && value
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

pub(crate) fn valid_package_name(value: &str) -> bool {
    if value.len() > 214
        || value.contains("...")
        || value
            .bytes()
            .any(|byte| matches!(byte, b'^' | b'*' | b'!' | b'[' | b']' | b'{' | b'}'))
    {
        return false;
    }

    match value.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, name)| valid_package_part(scope) && valid_package_part(name)),
        None => valid_package_part(value),
    }
}

fn valid_package_part(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'.' | b'_' | b'~')
        })
}

const fn invalid_config() -> ConfigError {
    ConfigError(Reason::InvalidConfig)
}

#[cfg(test)]
mod tests {
    use super::{parse_config, select_operation};
    use crate::{repository::PackageManagerKind, result::Reason};
    use std::{fs, path::Path};

    #[test]
    fn parses_the_minimal_root_configuration() {
        let config = parse_config(
            br#"{
              "$schema":"https://agentlowmem.dev/schema/v1.json",
              "version":1,
              "packageManager":"pnpm",
              "operations":{"test":{"script":"test","timeoutSeconds":900}}
            }"#,
        )
        .unwrap();

        assert_eq!(config.version, 1);
        assert_eq!(config.package_manager, PackageManagerKind::Pnpm);
        assert_eq!(config.operations["test"].script, "test");
        assert_eq!(config.operations["test"].timeout_seconds, 900);
        assert!(config.workspaces.is_empty());
    }

    #[test]
    fn parses_an_exact_workspace_configuration() {
        let config = parse_config(
            br#"{
              "version":1,
              "packageManager":"npm",
              "workspaces":{
                "web":{
                  "path":"apps/web",
                  "packageName":"@acme/web",
                  "operations":{"typecheck":{"script":"check:types","timeoutSeconds":3600}}
                }
              }
            }"#,
        )
        .unwrap();

        let workspace = &config.workspaces["web"];
        assert_eq!(workspace.path, "apps/web");
        assert_eq!(workspace.package_name, "@acme/web");
        assert_eq!(
            select_operation(&config, Some("web"), "typecheck")
                .unwrap()
                .script,
            "check:types"
        );
    }

    #[test]
    fn rejects_unknown_fields_and_invalid_constants() {
        for bytes in [
            br#"{"version":1,"packageManager":"npm","unknown":true}"#.as_slice(),
            br#"{"version":2,"packageManager":"npm"}"#.as_slice(),
            br#"{"$schema":"https://example.test/schema.json","version":1,"packageManager":"npm"}"#
                .as_slice(),
            br#"{"version":1,"packageManager":"yarn"}"#.as_slice(),
            br#"{"packageManager":"npm"}"#.as_slice(),
            br#"{"version":1}"#.as_slice(),
            br#"not-json"#.as_slice(),
        ] {
            assert_eq!(
                parse_config(bytes).unwrap_err().reason(),
                Reason::InvalidConfig,
                "input should be rejected: {}",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn rejects_invalid_operation_keys_scripts_and_timeouts() {
        for bytes in [
            br#"{"version":1,"packageManager":"npm","operations":{"Test":{"script":"test","timeoutSeconds":900}}}"#.as_slice(),
            br#"{"version":1,"packageManager":"npm","operations":{"test":{"script":"","timeoutSeconds":900}}}"#.as_slice(),
            br#"{"version":1,"packageManager":"npm","operations":{"test":{"script":"test","timeoutSeconds":59}}}"#.as_slice(),
            br#"{"version":1,"packageManager":"npm","operations":{"test":{"script":"test","timeoutSeconds":3601}}}"#.as_slice(),
            br#"{"version":1,"packageManager":"npm","operations":{"test":{"script":"test","timeoutSeconds":900,"command":"echo unsafe"}}}"#.as_slice(),
            br#"{"version":1,"packageManager":"npm","operations":{"abcdefghijklmnopqrstuvwxyz1234567":{"script":"test","timeoutSeconds":900}}}"#.as_slice(),
        ] {
            assert_eq!(parse_config(bytes).unwrap_err().reason(), Reason::InvalidConfig);
        }
    }

    #[test]
    fn rejects_unsafe_workspace_paths_and_package_names() {
        for (path, package_name) in [
            ("/apps/web", "@acme/web"),
            ("apps/../web", "@acme/web"),
            ("apps/./web", "@acme/web"),
            ("apps//web", "@acme/web"),
            ("apps/web/", "@acme/web"),
            ("apps\\web", "@acme/web"),
            ("apps/web", "@acme/*"),
            ("apps/web", "@acme/web..."),
            ("apps/web", "@acme/^web"),
            ("apps/web", "@acme"),
            ("apps/web", "ACME-WEB"),
        ] {
            let input = format!(
                r#"{{"version":1,"packageManager":"pnpm","workspaces":{{"web":{{"path":"{path}","packageName":"{package_name}","operations":{{}}}}}}}}"#
            );
            assert_eq!(
                parse_config(input.as_bytes()).unwrap_err().reason(),
                Reason::InvalidConfig,
                "path={path} package={package_name}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_configured_package_names() {
        let input = br#"{
          "version":1,
          "packageManager":"pnpm",
          "workspaces":{
            "web":{"path":"apps/web","packageName":"@acme/web","operations":{}},
            "admin":{"path":"apps/admin","packageName":"@acme/web","operations":{}}
          }
        }"#;

        assert_eq!(
            parse_config(input).unwrap_err().reason(),
            Reason::InvalidConfig
        );
    }

    #[test]
    fn selects_only_existing_root_or_workspace_operations() {
        let config = parse_config(
            br#"{
              "version":1,
              "packageManager":"npm",
              "operations":{"test":{"script":"test","timeoutSeconds":900}},
              "workspaces":{"web":{"path":"apps/web","packageName":"@acme/web","operations":{}}}
            }"#,
        )
        .unwrap();

        assert_eq!(
            select_operation(&config, None, "missing")
                .unwrap_err()
                .reason(),
            Reason::OperationUnsupported
        );
        assert_eq!(
            select_operation(&config, Some("missing"), "test")
                .unwrap_err()
                .reason(),
            Reason::WorkspaceCardinality
        );
        assert_eq!(
            select_operation(&config, Some("web"), "test")
                .unwrap_err()
                .reason(),
            Reason::OperationUnsupported
        );
    }

    #[test]
    fn schema_constants_match_the_rust_contract() {
        let schema_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("schemas/agent-lowmem-v1.schema.json");
        let schema: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(schema_path).unwrap()).unwrap();

        assert_eq!(
            schema["properties"]["$schema"]["const"],
            "https://agentlowmem.dev/schema/v1.json"
        );
        assert_eq!(schema["properties"]["version"]["const"], 1);
        assert_eq!(
            schema["properties"]["packageManager"]["enum"],
            serde_json::json!(["npm", "pnpm"])
        );
        assert_eq!(
            schema["$defs"]["operation"]["properties"]["timeoutSeconds"]["minimum"],
            60
        );
        assert_eq!(
            schema["$defs"]["operation"]["properties"]["timeoutSeconds"]["maximum"],
            3600
        );
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["$defs"]["operation"]["additionalProperties"], false);
        assert_eq!(schema["$defs"]["workspace"]["additionalProperties"], false);
    }
}
