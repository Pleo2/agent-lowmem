use agent_lowmem::{
    configuration::{AgentLowmemConfig, OperationConfig, WorkspaceConfig, parse_config},
    repository::PackageManagerKind,
};
use std::collections::BTreeMap;

fn operation(script: &str, timeout_seconds: u16) -> OperationConfig {
    OperationConfig {
        script: script.to_owned(),
        timeout_seconds,
    }
}

#[test]
fn serializes_npm_root_operations_as_exact_canonical_json() {
    let config = AgentLowmemConfig {
        version: 1,
        package_manager: PackageManagerKind::Npm,
        operations: BTreeMap::from([
            ("test".to_owned(), operation("test", 900)),
            ("build".to_owned(), operation("build", 1_800)),
        ]),
        workspaces: BTreeMap::new(),
    };

    let bytes = config.deterministic_bytes().unwrap();

    assert_eq!(
        bytes,
        br#"{
  "$schema": "https://agentlowmem.dev/schema/v1.json",
  "version": 1,
  "packageManager": "npm",
  "operations": {
    "build": {
      "script": "build",
      "timeoutSeconds": 1800
    },
    "test": {
      "script": "test",
      "timeoutSeconds": 900
    }
  }
}
"#
    );
    assert_eq!(parse_config(&bytes).unwrap(), config);
    assert!(config.has_operations());
}

#[test]
fn serializes_pnpm_workspaces_in_stable_key_order_and_omits_empty_root_operations() {
    let config = AgentLowmemConfig {
        version: 1,
        package_manager: PackageManagerKind::Pnpm,
        operations: BTreeMap::new(),
        workspaces: BTreeMap::from([
            (
                "web".to_owned(),
                WorkspaceConfig {
                    path: "apps/web".to_owned(),
                    package_name: "@agent-lowmem/web".to_owned(),
                    operations: BTreeMap::from([
                        ("typecheck".to_owned(), operation("typecheck", 900)),
                        ("lint".to_owned(), operation("lint", 900)),
                    ]),
                },
            ),
            (
                "api".to_owned(),
                WorkspaceConfig {
                    path: "apps/api".to_owned(),
                    package_name: "@agent-lowmem/api".to_owned(),
                    operations: BTreeMap::from([("test".to_owned(), operation("test", 900))]),
                },
            ),
        ]),
    };

    let bytes = config.deterministic_bytes().unwrap();

    assert_eq!(
        bytes,
        br#"{
  "$schema": "https://agentlowmem.dev/schema/v1.json",
  "version": 1,
  "packageManager": "pnpm",
  "workspaces": {
    "api": {
      "path": "apps/api",
      "packageName": "@agent-lowmem/api",
      "operations": {
        "test": {
          "script": "test",
          "timeoutSeconds": 900
        }
      }
    },
    "web": {
      "path": "apps/web",
      "packageName": "@agent-lowmem/web",
      "operations": {
        "lint": {
          "script": "lint",
          "timeoutSeconds": 900
        },
        "typecheck": {
          "script": "typecheck",
          "timeoutSeconds": 900
        }
      }
    }
  }
}
"#
    );
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert!(!bytes.windows(2).any(|window| window == b"\r\n"));
    assert_eq!(parse_config(&bytes).unwrap(), config);
    assert!(config.has_operations());
}

#[test]
fn reports_no_operations_only_when_root_and_every_workspace_are_empty() {
    let config = AgentLowmemConfig {
        version: 1,
        package_manager: PackageManagerKind::Npm,
        operations: BTreeMap::new(),
        workspaces: BTreeMap::from([(
            "web".to_owned(),
            WorkspaceConfig {
                path: "apps/web".to_owned(),
                package_name: "@agent-lowmem/web".to_owned(),
                operations: BTreeMap::new(),
            },
        )]),
    };

    assert!(!config.has_operations());
    assert_eq!(
        config.deterministic_bytes().unwrap(),
        br#"{
  "$schema": "https://agentlowmem.dev/schema/v1.json",
  "version": 1,
  "packageManager": "npm",
  "workspaces": {
    "web": {
      "path": "apps/web",
      "packageName": "@agent-lowmem/web"
    }
  }
}
"#
    );
}
